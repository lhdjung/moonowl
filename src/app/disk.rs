//! The Viewer and the disk: a document opened, written, reloaded when it
//! changes, and put down.

use super::*;

impl Viewer {
    /* ------------------------------------- what changed on the disk */

    /// A theme file was written — by hand, by an LLM, or by this app.
    ///
    /// The set is replaced and whatever is in use is put back on from the new
    /// files, so editing a theme beside the reader shows up in it. Nothing is
    /// written down: nobody chose a theme, and an editor saving every few
    /// seconds must not rewrite `settings.toml` every few seconds.
    ///
    /// A theme whose file has *gone* takes the reader somewhere else, and that
    /// one *is* remembered, because it is a choice made on their behalf.
    ///
    /// A theme being composed in the editor is the live theme and has no id,
    /// so every save would otherwise read as "the theme you are reading in has
    /// been deleted". The first branch below is the guard.
    pub fn themes_changed(&mut self, themes: Vec<crate::theme::Theme>) {
        // Without this a draft, which is in no file, would read as deleted,
        // and the reader would be moved to another theme *and that would be
        // written down*, from under the open editor.
        if self.editing.is_some() && self.pane.is_some() {
            self.store.set_themes(themes);
            self.preview_draft();
            return;
        }
        let before = self.store.theme().clone();
        let mut themes = themes;
        // **Missing is not gone.** A theme saved from an editor with a typo in
        // it does not parse, and is not in the list — but its file is there,
        // and treating it as deleted moved the reader to another theme and
        // wrote that down, so fixing the typo brought the theme back with
        // nothing pointing at it. The last good copy stays on until the file
        // reads again.
        let unreadable = !before.id.is_empty()
            && !themes.iter().any(|theme| theme.id == before.id)
            && self
                .store
                .themes_dir()
                .join(format!("{}.toml", before.id))
                .exists();
        if unreadable {
            self.notice = format!(
                "{}.toml could not be read, so the last version that could is still on.",
                before.id
            );
            themes.push(before.clone());
        }
        self.store.set_themes(themes);
        let still_there = self
            .store
            .themes()
            .iter()
            .any(|theme| theme.id == before.id);
        if still_there {
            // Unconditional: a theme that came back unchanged costs one
            // comparison in the widget and nothing else.
            self.chosen.set(self.store.palette());
            // Said only when there is something to say, or a theme file saved
            // elsewhere would wipe whatever the notice line was holding.
            if let Some(complaint) = self.store.complaint.clone() {
                self.notice = complaint;
            }
        } else if let Some(index) = self
            .renamed_elsewhere()
            .or_else(|| self.store.replacement_for(&before))
        {
            let worn = self.store.wear(index);
            self.chosen.set(self.store.palette());
            self.notice = format!("{} is gone. Now reading in {}.", before.name, worn.name);
        }
        self.generation += 1;
    }

    /// Where the worn theme went if another window renamed it: that window
    /// moved the settings to the new name a moment before this news arrived,
    /// and replacing the theme here wrote the replacement over its choice.
    /// One small read, and only when the worn theme has gone.
    fn renamed_elsewhere(&self) -> Option<usize> {
        let settings = crate::settings::load(self.store.dir());
        let id = settings.get("theme")?.as_str()?;
        self.store.themes().iter().position(|theme| theme.id == id)
    }

    /// The open document was rewritten underneath the reader.
    ///
    /// A paper recompiled by LaTeX is the case this exists for, and the reader
    /// stays where they were. Where that is comes off the layout rather than
    /// out of the library: the library has the last position *written down*,
    /// and this is the one moment the two can differ by a whole scroll.
    ///
    /// What goes is everything read out of the old file — outline, labels,
    /// links, index, crop — and everything pointing into it, which is the
    /// history. What stays is the reader's own: fit, zoom, spread, rotation,
    /// panel, theme.
    ///
    /// Answers the token of the scan it restarted, or `None`; a task is the
    /// caller's to spawn.
    pub fn document_changed(&mut self, path: &str) -> Option<u64> {
        if path != self.document.path() {
            return None;
        }
        // A write or reload in flight may have read the file before this
        // draft arrived, so it is owed another when it lands: dropped, a
        // compiler's second draft stayed off screen until its third.
        if self.writing.is_some() {
            self.reload_owed = true;
            return None;
        }
        // Somebody else's draft: putting ours back would take theirs away.
        self.forget_steps();
        // On a thread, like a write: a reopen parses the file, which is
        // long on a scanned volume. The document in hand is
        // kept until the new one is ready, so nothing goes blank meanwhile.
        self.offload(
            false,
            None,
            None,
            Paging::Unknown,
            |_| Ok(()),
            |viewer, _| {
                // Still asked, because asking is what *renames* the document.
                // What is not done with the answer is announce it: a reader
                // watching a paper recompile sees the page redraw and the title
                // change, and "Reloaded — the document changed on disk" only
                // worries somebody who did not know what a reload is.
                if viewer.store.renamed(&viewer.document.title()) {
                    // The window's own title is the shell's to set, and it is
                    // only ever told through `Showing`. Same path: the desk,
                    // the restore list and the watch all find nothing to do.
                    viewer.frame.ask(Ask::Showing {
                        path: viewer.document.path().to_string(),
                        title: viewer.store.title().to_string(),
                    });
                }
            },
        );
        None
    }

    /// Whether a write is still in flight, said if so. One at a time: the
    /// second would be editing a file the first is about to replace.
    /// Whether a write of the reader's own is still going into this document —
    /// which, unlike a reload, cannot be let go of: what it lands as is the
    /// only thing that keeps the highlight when the write is refused, and it
    /// keeps it in this document's entry. So the document stays until then.
    fn writing_own(&mut self) -> bool {
        if self.keeps_typed_words() {
            return true;
        }
        let writing = self.writing.is_some() && !self.reloading;
        if writing {
            self.notice = "Still writing the last change into the document.".into();
        }
        writing
    }

    /// Whether the window or tab may go. Not while a write of its own is
    /// going into the document: a write that is refused keeps the highlight
    /// in this window's journal, and a window that has gone keeps nothing.
    /// Pages arranged and not saved are asked about first, and the window
    /// goes if the reader closes without saving.
    pub fn may_close(&mut self) -> bool {
        !self.writing_own() && !self.asks_before_closing(Ask::Close)
    }

    pub(super) fn busy(&mut self) -> bool {
        self.busy_with(true)
    }

    /// [`Self::busy`] for a change that is kept only beside the document,
    /// in the journal: it never lets go of the document, so a thread that
    /// reads it is no reason to wait.
    pub(super) fn busy_beside(&mut self) -> bool {
        self.busy_with(false)
    }

    fn busy_with(&mut self, into_file: bool) -> bool {
        if self.markup_reading.is_some() {
            self.notice = "Still reading this document's highlights. Try again in a moment.".into();
            return true;
        }
        // A write lets go of the document, and a thread reading it for the
        // reader, such as recognising text or exporting it, would then stop
        // halfway with nothing to show for it. A thread that only writes a
        // file of its own, such as an attachment, does not hold it up.
        if into_file && self.making.is_some() && self.making_reads {
            self.notice = Self::STILL_BUSY.into();
            return true;
        }
        if self.writing.is_some() {
            self.notice = if self.reloading {
                "The document is being reloaded. Try again in a moment.".into()
            } else {
                "Still writing the last change into the document.".into()
            };
        }
        self.writing.is_some()
    }

    /// A change into the file — a highlight, a signature, marks found again
    /// — with the file as it was kept for undo. The step is only kept when the
    /// write is.
    pub(super) fn write_step(
        &mut self,
        journal: Vec<crate::library::Highlight>,
        work: impl FnOnce(&str) -> Result<(), String> + Send + 'static,
        done: impl FnOnce(&mut Viewer, Result<(), String>) + 'static,
    ) {
        let touched = self.marked_pages(std::iter::empty());
        self.write_step_on(Some(touched), journal, None, Paging::Same, work, done);
    }

    /// [`Viewer::write_step`] naming the pages the reopen reads marks off,
    /// or `None` for all of them: a passage put back lands wherever it is
    /// found.
    ///
    /// `marks` are the bookmarks as they were before a page edit, for undo to
    /// put back; `paging` is what the write does to the pages.
    pub(super) fn write_step_on(
        &mut self,
        touched: Option<Vec<usize>>,
        journal: Vec<crate::library::Highlight>,
        marks: Option<Vec<crate::library::Mark>>,
        paging: Paging,
        work: impl FnOnce(&str) -> Result<(), String> + Send + 'static,
        done: impl FnOnce(&mut Viewer, Result<(), String>) + 'static,
    ) {
        let before = crate::markup::Before::make();
        let keeping = before.keeper();
        // Undo takes a page edit back: each page goes back to where it was.
        let edited = match &paging {
            Paging::Edit(edit) => Some((edit.clone(), self.document.pages())),
            _ => None,
        };
        self.write_file(
            touched,
            paging,
            move |path| {
                keeping.take(path)?;
                work(path)
            },
            move |viewer, written| {
                if written.is_ok() {
                    let pages = viewer.document.pages();
                    viewer.did(Step {
                        journal,
                        file: Some(before),
                        marks,
                        moved: edited.map(|(edit, was)| edit.moved(was).inverse(pages)),
                    });
                } else {
                    // A removal empties the journal before it writes. When
                    // the write fails, the journal is put back and read
                    // against the file again, or the marks kept only in
                    // Moonowl are lost with no step to undo.
                    viewer.store.set_journal(journal);
                    viewer.sync_journal();
                }
                done(viewer, written);
            },
        );
    }

    /// The pages (one-based) that can hold a highlight after a write: those
    /// with a mark now, those one is being written on ([`Viewer::marking`]),
    /// and those `journals` name — for undo, the journal of either side of
    /// the step. The reopen after a write reads only these — see
    /// [`MarkupRead::of`].
    pub(super) fn marked_pages<'a>(
        &self,
        journals: impl Iterator<Item = &'a crate::library::Highlight>,
    ) -> Vec<usize> {
        let pages: std::collections::BTreeSet<usize> = (self.markup.iter())
            .map(|mark| mark.page)
            .chain(self.marking.iter().map(|(page, ..)| *page))
            .chain(journals.map(|mark| mark.page as usize))
            .collect();
        pages.into_iter().collect()
    }

    /// The write itself: the half that [`Viewer::write_step`] and undo go
    /// through. `touched` is the pages the reopen reads marks off, and
    /// `None` is all of them.
    ///
    /// It writes the document on a thread of its own and carries on when the
    /// write lands.
    ///
    /// `work` is the whole file read, rewritten by `FPDF_SaveAsCopy` and
    /// written back, and the reopen after it loads every page for its size —
    /// invisible on a paper and seconds on a scanned volume, which on the
    /// window's thread would freeze the window. Both are on the thread; `done`
    /// is the caller's second half, run by [`Viewer::landed`] once the
    /// mailbox says `Event::DocumentWritten`.
    ///
    /// **The file is let go of first, and reopened whatever happens**: pdfium
    /// holds it open, and on Windows nothing can rename over it while it does.
    /// See [`crate::render::PageSource::release`].
    ///
    /// The watch is told the burst on its way is ours from the thread, right
    /// after the write and before the reopen — inside its settle window, or it
    /// would reload the document a second time. See
    /// [`crate::watch::Watching::wrote`]. **Not said on the way through
    /// `document_changed`**: `wrote` retakes the baseline from the disk, and a
    /// compiler's next draft landing in that moment would become the baseline
    /// and never be reported.
    // ponytail: pdfium's one lock is held for the length of the save, so a
    // page mounted for the first time in that moment still waits for it.
    pub(super) fn write_file(
        &mut self,
        touched: Option<Vec<usize>>,
        paging: Paging,
        work: impl FnOnce(&str) -> Result<(), String> + Send + 'static,
        done: impl FnOnce(&mut Viewer, Result<(), String>) + 'static,
    ) {
        // **Only into the draft on screen.** A mark is a page and an index or
        // a set of quads in *this* file; applied to one Zotero or a compiler
        // wrote since, it takes out or covers something else. Refused, the
        // reopen below still runs and brings the new draft in.
        let unwritten = self.unwritten.take();
        let expected = self.left.take().or_else(|| self.document.stamp());
        // **A file that is no longer there is not let go of.** pdfium's
        // handle is then the only thing keeping it readable: released for a
        // write that could only fail, a document moved in the Finder while it
        // was open would go blank on every page.
        if expected.is_some() && !std::path::Path::new(self.document.path()).exists() {
            // Nothing will land to replace the stand-in, so it goes now.
            self.marking.clear();
            done(
                self,
                Err("The document is no longer where it was opened from, so nothing was written into it.".into()),
            );
            return;
        }
        let work = move |path: &str| {
            if expected.is_some() && crate::render::stamp_of(path) != expected {
                return Err("The document changed on disk. Try again now it has reloaded.".into());
            }
            crate::markup::into_draft(expected, || work(path))
        };
        self.document.release();
        self.offload(true, touched, unwritten, paging, work, done);
    }

    /// The thread under [`Viewer::write_step`], which [`Viewer::document_changed`]
    /// shares for the reopen alone: `ours` is whether the watch is to be told
    /// the burst on its way is this reader's.
    fn offload(
        &mut self,
        ours: bool,
        touched: Option<Vec<usize>>,
        unwritten: Option<Box<dyn FnOnce() + Send>>,
        paging: Paging,
        work: impl FnOnce(&str) -> Result<(), String> + Send + 'static,
        done: impl FnOnce(&mut Viewer, Result<(), String>) + 'static,
    ) {
        let path = self.document.path().to_string();
        let password = self.document.password().map(str::to_string);
        let before = self.document.clone();
        let landing = Arc::new(Mutex::new(None));
        self.writing = Some((Arc::clone(&landing), Box::new(done)));
        self.reloading = !ours;
        let (watching, window, post) = (
            self.watching.clone(),
            self.window.clone(),
            self.post.clone(),
        );
        let writing = crate::stats::Writing::begin();
        std::thread::spawn(move || {
            let _writing = writing;
            let written = work(&path);
            if let (Err(_), Some(keep)) = (&written, unwritten) {
                keep();
            }
            let left = (ours && written.is_ok())
                .then(|| crate::render::stamp_of(&path))
                .flatten();
            // Only a write that happened is a burst of ours: a refused one
            // left the disk to whoever wrote it last, and taking the baseline
            // from that would swallow their next draft.
            if let Some(watching) = watching.filter(|_| ours && written.is_ok()) {
                watching.wrote(&window, std::path::Path::new(&path));
            }
            let reopened = crate::render::open_with(&path, password.as_deref());
            // A write that failed moved nothing, and a compiler's draft is
            // another document, whose pages are its own.
            let paging = match paging {
                _ if !ours => Paging::Unknown,
                _ if written.is_err() => Paging::Same,
                paging => paging,
            };
            let mut paged = Paged::Unknown;
            let markup = reopened.as_ref().ok().map(|document| {
                // What the document before knew of its pages' spaces, this
                // one knows too, moved with the pages: relearning them is the
                // walk opening just paid for.
                let (pages, spaces) = (document.pages(), before.spaces());
                paged = match &paging {
                    Paging::Same => {
                        document.learn_spaces(&spaces);
                        Paged::Same
                    }
                    Paging::Edit(edit) => {
                        document.learn_spaces(&edit.spaces(&spaces, pages));
                        Paged::Moved(edit.moved(before.pages()))
                    }
                    Paging::Back(moved) => Paged::Back(moved.clone()),
                    Paging::Unknown => Paged::Unknown,
                };
                MarkupRead::of(&**document, touched.as_deref())
            });
            *landing.lock().unwrap_or_else(|e| e.into_inner()) =
                Some((written, left, reopened, markup, paged));
            post.send(crate::emit::News {
                event: Event::DocumentWritten,
                target: None,
            });
        });
    }

    /// A write of this reader's own has landed: take up the document it left
    /// and run the second half of whatever asked for it.
    ///
    /// Nothing happens for a write nobody is waiting for any more — the
    /// reader opened another document in the meantime, which forgot it.
    ///
    /// Answers the token of the scan it restarted, as `document_changed` does.
    pub fn landed(&mut self) -> Option<u64> {
        let (landing, _) = self.writing.as_ref()?;
        let (written, left, reopened, markup, paged) =
            landing.lock().unwrap_or_else(|e| e.into_inner()).take()?;
        let (_, done) = self.writing.take()?;
        self.left = reopened.is_err().then_some(left).flatten();
        let restarted = self.adopt(reopened, markup, paged);
        // The page keeps it in its texture until the new draft is drawn.
        self.marking.clear();
        done(self, written.map_err(plainly));
        if self.opening.is_some() {
            self.opened();
        }
        if std::mem::take(&mut self.reload_owed) {
            let path = self.document.path().to_string();
            self.document_changed(&path);
        }
        if std::mem::take(&mut self.comment_owed) {
            self.save_comment();
        }
        self.write_owed_fields();
        // The first starts a write, and the others wait for it in turn.
        for (page, spot) in std::mem::take(&mut self.clicks_owed) {
            self.fill_field(page, spot, crate::forms::Fill::Click);
        }
        restarted
    }

    /// The second half of a reopen, for a document [`Viewer::offload`]'s
    /// thread opened: a write of this reader's own, or a rebuild's.
    fn adopt(
        &mut self,
        reopened: Result<Arc<dyn PageSource>, crate::render::Refusal>,
        markup: Option<MarkupRead>,
        paged: Paged,
    ) -> Option<u64> {
        let mut at = self.layout.anchor(self.scroll_top);
        // The reader stays on the page they were reading, wherever it went;
        // on a page that went, they land at the top of the page after it.
        if let Some(moved) = paged.moved() {
            match moved.page(at.page) {
                Some(page) => at.page = page,
                None => {
                    at.page = moved.landing(at.page);
                    at.offset = 0.0;
                }
            }
        }
        let reopened = match reopened {
            Ok(document) => document,
            // A compiler that is still writing is what `whole()` in `watch.rs`
            // is there to rule out, so this is the genuinely broken file —
            // and the document already open is the better thing to be looking
            // at than an empty window. If it was let go of for a write that
            // then failed, it is taken up again, or every page is blank until
            // ⌘O.
            Err(refused) => {
                self.document.retake();
                // Whatever was asked of it while it was let go of was
                // answered with nothing, and cached.
                self.forget_annotations();
                self.forget_texts();
                self.notice = format!("The document could not be reopened. {refused}");
                return None;
            }
        };
        // An organiser open on this document arranges pages of the draft
        // that has just gone, so it goes too. A document reopened as it was,
        // after a write that failed, is the same pages, and the organiser
        // goes on arranging them.
        let unchanged =
            self.document.stamp().is_some() && self.document.stamp() == reopened.stamp();
        if let Some(organiser) = self.organiser.as_mut().filter(|o| o.own().is_some()) {
            if unchanged {
                organiser.reopened(reopened.clone());
            } else {
                self.organiser = None;
                self.notice =
                    "The document changed on disk, so the pages being arranged were put away."
                        .into();
            }
        }
        // The document put down is let go of now, not when its last holder
        // goes: a thread still reading its links would otherwise load page
        // after page of it, each one ahead of a page of this one being drawn.
        // A thread reading it for the reader, such as one saving its text,
        // keeps it until it is done.
        let read_for_reader = self.making.is_some() && self.making_reads;
        if !Arc::ptr_eq(&self.document, &reopened) && !read_for_reader {
            self.document.release();
        }
        self.document = reopened;
        self.chosen.show(self.document.clone());
        // A comparison is a list of places in the text that was there. Only
        // a write of the reader's own that moved nothing, such as a
        // highlight, leaves the text as it was.
        if self.comparing.is_some() && (self.reloading || !matches!(paged, Paged::Same)) {
            self.stop_comparing();
            self.notice = "The document changed, so the comparison was put away.".into();
        }
        self.headings = Rc::new(self.document.outline());
        self.picked_heading = None;
        self.labels = self.document.labels();
        self.forget_annotations();
        // And the text with them, along with whatever was selected: both are
        // indices into a document that no longer exists. The markup journal is
        // where a passage *does* survive a rebuild, and it survives as a quote
        // to be looked up again rather than as a range.
        //
        self.forget_texts();
        // **A comment being typed outlives a write of our own**, which
        // changed a mark and never a word: its passage is where it was, and
        // so is a mark on a page that lost none — a write adds at the end of
        // a page's list, and only a removal moves what comes after.
        let typing = !self.reloading && matches!(paged, Paged::Same) && self.commenting.is_some();
        let on_page = |marks: &[crate::markup::Mark], page: usize| {
            marks.iter().filter(|mark| mark.page == page).count()
        };
        let kept_mark = (self.mark_open.clone())
            .filter(|_| typing)
            .map(|open| (on_page(&self.markup, open.0), open));
        let kept_passage = typing.then(|| (self.markup_at, self.selection.take()));
        // Before the marks are read off the new file, which matches the
        // journal against them.
        if let Paged::Moved(moved) = &paged {
            self.follow_pages(moved);
        }
        match markup {
            Some(read) => self.take_markup(read),
            None => self.read_markup(),
        }
        // An annotation's index is its place in a list that was just
        // rewritten: a popover or a Sign window still holding one would take
        // the wrong annotation out of the file.
        self.mark_open = kept_mark
            .filter(|(before, open)| on_page(&self.markup, open.0) >= *before)
            .map(|(_, open)| open);
        self.markup_at = None;
        self.close_asking_of_document();
        // And a Remove waiting for its second press: the row it was armed on
        // is now whichever annotation took that index, and one click would
        // take that out of the file.
        self.arming = None;
        if self.signing.is_some() {
            self.read_signed();
        }
        self.selection = None;
        self.sweep_from = None;
        if let Some((at, selection)) = kept_passage.filter(|(at, _)| at.is_some()) {
            self.markup_at = at;
            self.selection = selection;
        }
        // If the comment's mark or passage did not survive, the field has
        // nowhere to stand, so the draft goes and the reader is told.
        if self.commenting.is_some() && self.mark_open.is_none() && self.markup_at.is_none() {
            self.commenting = None;
            self.comment_menu = false;
            self.notice = "The document changed while a comment was being written, so the comment was not saved.".into();
        }
        // A field being filled in is a field of the draft that was there. A
        // write of our own moved no field; any other change may have.
        // The reading line, and boxes clicked while a write was under way,
        // are places in the draft that was there. The line goes back under
        // the first line in the window.
        // ponytail: after a recompile the band could stay at its height on
        // its page rather than go back to the top of the window.
        if self.reloading || !matches!(paged, Paged::Same) {
            self.clicks_owed.clear();
            self.reading = None;
        }
        let typed = self.filling.is_some() || !self.fields_owed.is_empty();
        if typed && (self.reloading || !matches!(paged, Paged::Same)) {
            self.filling = None;
            self.fields_owed.clear();
            self.notice = "The document changed while a field was being filled in, so the words were not saved.".into();
        }
        // A rebuild's pages are not the ones the history was taken on. A write
        // of our own changed a mark, never a page, and following a reference,
        // marking it and stepping back is the whole of reading one.
        // A page edit moves the history with the pages, and a place on a
        // page that went goes with it.
        match paged.moved() {
            None if matches!(paged, Paged::Same) && !self.reloading => {}
            Some(moved) => {
                let follow = |places: &mut Vec<Anchor>| {
                    places.retain_mut(|place| {
                        moved
                            .page(place.page)
                            .map(|page| place.page = page)
                            .is_some()
                    })
                };
                follow(&mut self.past);
                follow(&mut self.future);
                self.landing = None;
            }
            _ => {
                self.past.clear();
                self.future.clear();
            }
        }
        let sizes = (0..self.document.pages())
            .map(|index| self.document.size_of(index))
            .collect();
        self.layout.replace_sizes(sizes);
        // The thumbnail column is laid out from those sizes, and a draft of a
        // different length leaves it a page short or a page long. `resize`
        // rebuilds it for a window that changed, and the window did not.
        self.relay_column();
        if self.trimming {
            // Measured again rather than kept: the margins are a fact about
            // the file, and this is a different file.
            self.measure_crop();
        }
        self.edition += 1;
        self.generation += 1;
        // Clamped by `go_to`, so a draft that lost its last chapter lands on
        // the end of what is left rather than nowhere.
        self.go_to(at);
        // A new draft under the reader is not the reader scrolling.
        self.relaid_at = self.scroll_top;
        // Forgotten whether or not the bar is up: a menu puts the bar away
        // and keeps the index for the ⌘G after it, and that ⌘G would have
        // searched the draft before this one.
        let was = self.search.current();
        self.search.forget();
        if self.find_open {
            self.find_again(was)
        } else {
            None
        }
    }

    /// A different document, in this window. ⌘O, and the menu item under it.
    ///
    /// **⌘O replaces the document in the window it was pressed in** and ⇧⌘O
    /// asks for a window: ⌘O is the reader saying
    /// *this one instead*.
    ///
    /// Everything [`Viewer::document_changed`] clears is cleared, **and the
    /// library entry with it**: a recompile is the same document and this is a
    /// different one, so the marks, the title and the remembered place all
    /// move. What stays is the reader's own — fit, zoom, spread, rotation,
    /// panel, theme — because a setting is not a property of a document.
    ///
    /// Answers whether it opened; a document that would not open leaves this
    /// window showing the one it had. The find bar is closed rather than
    /// searched again, which is where this parts company with
    /// `document_changed`: a query asked of a paper is not a query asked of
    /// the next book.
    pub fn open_here(&mut self, path: &str) {
        self.open_here_with(path, None);
    }

    /// The same, with the password for a document that wants one.
    ///
    /// **Locked is not an error here, it is a question**, and this is the one
    /// place that turns it into one: [`Locked`] goes up and the window keeps
    /// what it had. The answer comes back through [`Self::unlock`].
    ///
    /// Whether the sentence says "it needs a password" or "that one was not
    /// right" is decided here rather than by pdfium, which reports both as
    /// `FPDF_ERR_PASSWORD`: the difference is whether this call supplied
    /// one.
    ///
    /// The document is opened on a thread, because opening parses the file,
    /// which takes seconds on a large scanned volume. [`Viewer::opened`] is
    /// the second half.
    pub(super) fn open_here_with(&mut self, path: &str, password: Option<&str>) {
        if self.writing_own() {
            return;
        }
        // The picker, a drop and a handover all arrive here; the command line
        // and the socket were made absolute at the door. See `config::absolute`.
        let path = &crate::config::absolute(path);
        if path == self.document.path() {
            self.notice = "That document is already open here.".into();
            return;
        }
        // Open in another window: that one comes forward, through the same
        // door a second launch uses. Two windows on one document each write
        // the whole of its marks, and the last one wins.
        if self.shown_elsewhere(path) {
            self.notice = "That document is open in another window.".into();
            self.frame.ask(Ask::NewWindowOn(path.clone()));
            // An empty tab that was opened for it is a dead end beside the
            // tab that has it. A window of its own stays: a whole window
            // going is a surprise.
            if self.empty() && self.locked.is_none() && self.tabs.get() {
                self.frame.ask(Ask::Close);
            }
            return;
        }
        let landing = Arc::new(Mutex::new(None));
        self.opening = Some(Arc::clone(&landing));
        let (path, password) = (path.clone(), password.map(str::to_string));
        let post = self.post.clone();
        let working = crate::stats::Writing::begin();
        std::thread::spawn(move || {
            let _working = working;
            let opened = crate::render::open_with(&path, password.as_deref());
            *landing.lock().unwrap_or_else(|e| e.into_inner()) = Some(Opened {
                path,
                with_password: password.is_some(),
                opened,
            });
            post.send(crate::emit::News {
                event: Event::DocumentOpened,
                target: None,
            });
        });
    }

    /// A document [`Viewer::open_here_with`] opened on its thread: taken up,
    /// or the reason it was not said. One that an open asked for later has
    /// taken the place of is let go of.
    pub fn opened(&mut self) {
        let Some(landing) = self.opening.take() else {
            return;
        };
        // A write of the reader's own began while it was opening. The
        // document is taken up when the write lands, in `landed`.
        if self.writing.is_some() && !self.reloading {
            self.opening = Some(landing);
            return;
        }
        let Some(Opened {
            path,
            with_password,
            opened,
        }) = landing.lock().unwrap_or_else(|e| e.into_inner()).take()
        else {
            // The news of an earlier open, whose result this one replaced.
            self.opening = Some(landing);
            return;
        };
        let path = &path;
        let opened = match opened {
            Ok(document) => document,
            Err(crate::render::Refusal::Locked) => {
                self.locked = Some(Locked {
                    path: path.to_string(),
                    typed: String::new(),
                    wrong: with_password,
                });
                // **An empty window that is asking is showing, as far as the
                // desk is concerned.** A window down as empty is where the
                // next document handed over goes, and this one would send it
                // on — back to the desk, back to this window, for ever. See
                // `stop_unlocking`. A window with a document keeps it on the
                // desk and in the watch, since that is what it still shows.
                if self.empty() {
                    self.frame.ask(Ask::Showing {
                        path: path.to_string(),
                        title: crate::store::called(path, ""),
                    });
                }
                return;
            }
            Err(refused) => {
                // Through `stop_unlocking`, which tells the desk: otherwise a
                // prompt given up here would leave it believing this window
                // shows the locked document, and send anybody opening it here.
                self.stop_unlocking();
                self.notice = refused.to_string();
                return;
            }
        };
        self.locked = None;
        // Where the reader was in the document being put down, written before
        // the store stops pointing at it. `remember` hands it to the scribe,
        // which keeps one place per document — so this cannot be skipped on
        // the grounds that the scroll has not moved since the last one.
        let at = self.layout.anchor(self.scroll_top);
        self.store
            .remember(self.layout.kept(at), self.label(at.page));
        let declared = opened.title();
        if !Arc::ptr_eq(&self.document, &opened) {
            self.document.release();
        }
        self.document = opened;
        // Before the desk's own word to the watch, which does not know
        // which draft this window opened.
        if let Some(watching) = &self.watching {
            watching.opened(&self.window, path, self.document.stamp());
        }
        let place = self.store.opened(path, &declared);
        self.take_up(place);
        self.frame.ask(Ask::Showing {
            path: path.clone(),
            title: self.store.title().to_string(),
        });
        self.take_forward(false);
    }

    /// The document is put down and this window is showing none.
    ///
    /// **The gesture the app calls Close**, which is not closing the window:
    /// the window stays, the toolbar keeps what is not about a document, and
    /// the start screen is what is in front of the reader. It is also the one
    /// gesture that empties the restore list — a window that goes because the
    /// app is quitting was open at the end, and a document the reader put down
    /// is one they have finished with — and that is the caller's to say,
    /// because the list belongs to the process.
    ///
    /// Everything [`Viewer::open_here`] clears is cleared, and what takes the
    /// document's place is [`crate::render::Nothing`].
    pub fn close_document(&mut self) {
        if self.empty() || self.writing_own() {
            return;
        }
        // Where they got to, written while the store still points at the file
        // it is about. Exactly as `open_here` does it, and for the same
        // reason: this is the last moment either half is true.
        let at = self.layout.anchor(self.scroll_top);
        self.store
            .remember(self.layout.kept(at), self.label(at.page));
        // **Written now rather than eventually**, which is the one place in
        // this reader that waits for the scribe: the screen about to go up
        // says, on its first row, where the reader stopped in the document
        // they are putting down. Read before the file has caught up, that is
        // the page they were on when they last *opened* it — stale exactly
        // when it is most looked at.
        crate::store::flush();
        self.document.release();
        self.document = crate::render::nothing();
        self.store.closed();
        self.take_up(None);
        // Nothing to say. The screen that arrives says what it is.
        self.notice = String::new();
    }

    /// Whether this window has a document in it.
    ///
    /// Asked in the two places it decides something: what the toolbar carries,
    /// and whether the body is the document or the start screen. Everything
    /// else goes on being written for a document, because a document of no
    /// pages answers every question already — see [`crate::render::Nothing`].
    pub fn empty(&self) -> bool {
        self.document.pages() == 0
    }

    /// The last few documents read, for the start screen and the Open menu,
    /// with the one already open left out — reopening it here would be a
    /// no-op, and opening it in a *second* window is what the title menu is
    /// for. Six; the menu takes what it wants.
    pub fn recents(&self) -> Vec<crate::store::Recent> {
        let open = self.document.path();
        self.store
            .recents()
            .into_iter()
            .filter(|entry| entry.path != open)
            .take(6)
            .collect()
    }

    /// Take a document off the recently-read list, and off the shelf.
    pub fn forget(&mut self, path: &str) {
        // Forgetting is losing the marks and the place, and a window reading
        // the document would go on writing them into an entry that is gone.
        if self.shown_elsewhere(path) {
            self.notice = "That document is open in another window.".into();
            return;
        }
        self.store.forget(path);
        self.generation += 1;
    }

    /// Ask whether to empty the recently read list. The start screen and the
    /// Open menu both ask this way.
    pub fn ask_clear_recents(&mut self) {
        self.menu = None;
        // Positions are only kept while the app remembers them at all.
        let says = if self.store.flag("remember_position") {
            "The files are only taken off the list. Their bookmarks, highlights, and reading positions are kept."
        } else {
            "The files are only taken off the list. Their bookmarks and highlights are kept."
        };
        self.asking = Some(Asking {
            title: "Remove list of recent documents?".into(),
            says: says.into(),
            other: Other::Keep("Cancel"),
            go: "Clear list",
            of_document: false,
            then: Box::new(|viewer| {
                viewer.store.clear_recents();
                viewer.generation += 1;
            }),
        });
    }

    /// Whether another window of this process is showing the document.
    pub(super) fn shown_elsewhere(&self, path: &str) -> bool {
        self.desk
            .as_ref()
            .and_then(|desk| desk.shown_by(path))
            .is_some_and(|label| label != self.window)
    }

    /// Everything a window has to put down when the document under it changes,
    /// and everything it has to pick up again.
    ///
    /// Shared by [`Viewer::open_here`] and [`Viewer::close_document`] because
    /// the list is the part that is easy to get wrong: a cache left pointing
    /// into the document that was there a moment ago is a rectangle drawn over
    /// the wrong page.
    fn take_up(&mut self, place: Option<crate::layout::Anchor>) {
        // A reload still in flight was of the document put down, and what it
        // lands as is nothing this one wants. A write of our own never gets
        // here: see [`Viewer::writing_own`].
        self.writing = None;
        self.marking.clear();
        self.reload_owed = false;
        self.left = None;
        self.forget_steps();
        self.headings = Rc::new(self.document.outline());
        // An index into the outline just replaced, and a spot on a page of it.
        self.picked_heading = None;
        self.landing = None;
        self.labels = self.document.labels();
        // A different document has different markup, and its own answer to
        // whether it can be written — and `said_standing` goes with it,
        // because "said once" means once per document.
        self.forget_annotations();
        // Before the markup is read, which reads its quotes off this text.
        self.forget_texts();
        self.read_markup();
        self.said_standing = false;
        self.close_asking_of_document();
        self.markup_at = None;
        self.mark_open = None;
        self.commenting = None;
        self.comment_menu = false;
        self.arming = None;
        self.signing = None;
        // A signature armed for the last document would land on the first
        // click in this one, with nothing on screen saying it was armed.
        self.placing = None;
        self.note_open = None;
        self.selection = None;
        self.sweep_from = None;
        self.reading = None;
        self.filling = None;
        self.clicks_owed.clear();
        self.fields_owed.clear();
        self.comparing = None;
        // A page scrolling by itself, or held still, would go on in the new
        // document with nothing on screen saying why.
        self.stop_still();
        self.past.clear();
        self.future.clear();
        self.search.forget();
        self.close_find();
        // This document's own margins as they were last measured, or nothing
        // until the answer: a different document laid out under the last
        // one's margins is every page drawn once wrong and once right, and
        // one opened whole and trimmed a moment later visibly shrinks.
        self.layout.crop = if self.trimming {
            self.remembered_crop()
        } else {
            None
        };
        let sizes = (0..self.document.pages())
            .map(|index| self.document.size_of(index))
            .collect();
        self.layout.replace_sizes(sizes);
        if self.trimming {
            self.measure_crop();
        }
        // A document with no contents opens on its pages, which is what
        // `restore` does at startup: the difference between a panel and an
        // empty box.
        self.tab = if self.headings.is_empty() {
            Tab::Pages
        } else {
            Tab::Contents
        };
        self.edition += 1;
        self.opened += 1;
        self.generation += 1;
        self.chosen.show(self.document.clone());
        self.scroll_top = 0.0;
        // The place is kept as a fraction of the whole upright page, and
        // this window may be turned, or have the margins the document was
        // last trimmed by in already.
        self.go_to(
            self.layout
                .from_kept(place.unwrap_or(crate::layout::Anchor {
                    page: 1,
                    offset: 0.0,
                })),
        );
        self.relaid_at = self.scroll_top;
        self.relay_column();
        self.revealed = false;
        self.reveal_thumb();
        self.notice = String::new();
    }
}

impl Viewer {
    /// Work on the themes or palettes directory, done on the scribe so it
    /// keeps its order with every other write, and `then` given the answer
    /// on this thread. One at a time: asked while one is out, this does
    /// nothing and answers `false`, so a second press of Save cannot make a
    /// second file.
    pub(super) fn on_disk<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
        then: impl FnOnce(&mut Viewer, T) + 'static,
    ) -> bool {
        self.shelve(work, then, false)
    }

    /// Said when a file cannot be written because the last one is still
    /// being written.
    pub(super) const BUSY: &str = "Still saving the last file. Try again in a moment.";
    /// Said when something read on a thread waits for a file being saved.
    pub(super) const STILL_BUSY: &str =
        "Moonowl is still busy with the last file. Try again in a moment.";

    /// [`Self::on_disk`] for a file the reader asked for, such as pages
    /// saved or a document's text, on a thread of its own. Such a file
    /// keeps no order with the app's own writes, and reading a long book
    /// for it would hold up every write behind it on the scribe. Quitting
    /// still waits for it. One at a time, as [`Self::on_disk`] is, but
    /// apart from it.
    pub(super) fn on_thread<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
        then: impl FnOnce(&mut Viewer, T) + 'static,
    ) -> bool {
        // A write lets go of the document until it lands, and a thread
        // that reads it meanwhile would find it gone. A reload keeps it.
        if self.writing.is_some() && !self.reloading {
            return false;
        }
        let taken = self.shelve(work, then, true);
        if taken {
            self.making_reads = true;
        }
        taken
    }

    /// [`Self::on_thread`] for work that does not read the open document,
    /// such as writing the highlights out or saving an attachment through
    /// the lopdf process. A highlight or a form filled in meanwhile does
    /// not wait for it.
    pub(super) fn on_thread_without_document<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
        then: impl FnOnce(&mut Viewer, T) + 'static,
    ) -> bool {
        let taken = self.shelve(work, then, true);
        if taken {
            self.making_reads = false;
        }
        taken
    }

    /// [`Self::on_thread`] for work on the document being read, such as
    /// comparing it or recognising its text. An answer that is ready only
    /// after the document was reloaded or replaced is about pages that may
    /// not be there any more, so it is dropped, and the reader is told.
    pub(super) fn on_thread_reading<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
        then: impl FnOnce(&mut Viewer, T) + 'static,
    ) -> bool {
        let edition = self.edition;
        self.on_thread(work, move |viewer, answer| {
            if viewer.edition != edition {
                viewer.notice = "The document changed while it was being read, \
                                 so nothing was done. Try again."
                    .into();
                return;
            }
            then(viewer, answer)
        })
    }

    /// [`Self::on_disk`], on a thread of its own when `apart`.
    fn shelve<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
        then: impl FnOnce(&mut Viewer, T) + 'static,
        apart: bool,
    ) -> bool {
        let held = match apart {
            true => &self.making,
            false => &self.shelving,
        };
        if held.is_some() {
            return false;
        }
        let landing = Arc::new(Mutex::new(None));
        let (slot, post) = (Arc::clone(&landing), self.post.clone());
        let working = crate::stats::Writing::begin();
        // What a job that panicked answers with. It still answers, because
        // until it does the window refuses every change as busy.
        struct Failed;
        let job = move || {
            let _working = working;
            let answer: Box<dyn std::any::Any + Send> =
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
                    Ok(answer) => Box::new(answer),
                    Err(_) => Box::new(Failed),
                };
            *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(answer);
            post.send(crate::emit::News {
                event: Event::Shelved,
                target: None,
            });
        };
        match apart {
            true => drop(std::thread::spawn(job)),
            false => crate::store::later(job),
        }
        let shelving: Shelving = (
            landing,
            Box::new(move |viewer, answer| match answer.downcast::<T>() {
                Ok(answer) => then(viewer, *answer),
                Err(_) => viewer.notice = "Something went wrong, and that was not done.".into(),
            }),
        );
        match apart {
            true => self.making = Some(shelving),
            false => self.shelving = Some(shelving),
        }
        true
    }

    /// The answers [`Self::on_disk`] and [`Self::on_thread`] waited for,
    /// those that have landed.
    pub fn shelved(&mut self) {
        for apart in [false, true] {
            let held = match apart {
                true => &mut self.making,
                false => &mut self.shelving,
            };
            let landed = (held.as_ref())
                .and_then(|(landing, _)| landing.lock().unwrap_or_else(|e| e.into_inner()).take());
            if let (Some(answer), Some((_, then))) = (landed, held.take()) {
                then(self, answer);
                // A comment typed while the thread was reading waited for it.
                if apart && std::mem::take(&mut self.comment_owed) {
                    self.save_comment();
                }
                if apart {
                    self.write_owed_fields();
                }
            }
        }
    }
}
