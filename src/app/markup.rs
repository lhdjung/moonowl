//! The Viewer's marks: highlights and their comments, the highlight colours,
//! and undoing a write to the document.

use super::*;

impl Viewer {
    /* ------------------------------------------------------------- markup */

    /// Read the document's own markup, ask the disk where a new mark could
    /// go, and bring the journal into line with both. Called at open and
    /// after every reload.
    /// **Read on a thread**, as a write's reopen reads it: a probe file
    /// written beside the document and the words under every mark, which on
    /// a syncing volume stalled the window at every ⌘O. The marks of the
    /// document before are gone at once; a write waits for the answer (see
    /// [`Viewer::busy`]), which [`Viewer::markup_landed`] takes up.
    pub(super) fn read_markup(&mut self) {
        self.markup.clear();
        self.quotes.clear();
        self.columns.clear();
        let landing = Arc::new(Mutex::new(None));
        self.markup_reading = Some(Arc::clone(&landing));
        let (document, post) = (self.document.clone(), self.post.clone());
        let working = crate::stats::Writing::begin();
        std::thread::spawn(move || {
            let _working = working;
            let read = MarkupRead::of(&*document, None);
            *landing.lock().unwrap_or_else(|e| e.into_inner()) = Some(read);
            post.send(crate::emit::News {
                event: "markup-read".into(),
                target: None,
                payload: crate::emit::Payload::Nothing,
            });
        });
    }

    /// The read [`Viewer::read_markup`] started, if it is the latest and has
    /// landed: one for a document since put down landed in a slot nobody
    /// holds any more.
    pub fn markup_landed(&mut self) {
        let Some(read) = self
            .markup_reading
            .as_ref()
            .and_then(|landing| landing.lock().unwrap_or_else(|e| e.into_inner()).take())
        else {
            return;
        };
        self.take_markup(read);
    }

    pub(super) fn take_markup(&mut self, read: MarkupRead) {
        // A reload's read is newer than one still on its way.
        self.markup_reading = None;
        // The walk that read these loaded every page, so every heading can
        // now be placed on its own.
        self.place_headings();
        self.markup = read.marks;
        self.quotes = read.quotes;
        self.columns = read.columns;
        self.standing = read.standing;
        // Said once, on the reload that lost them: they are no longer drawn,
        // and the sidebar is where they wait.
        let lost = self.sync_journal();
        if lost > 0 {
            self.notice = match lost {
                1 => "This version of the document lost a highlight. The sidebar can put it back.".into(),
                n => format!("This version of the document lost {n} highlights. The sidebar can put them back."),
            };
        }
        // The first comment makes room for itself, and the last gives it back.
        // Before the window has a size, its first resize does this.
        if self.window_width > 0.0 {
            self.resize(self.window_width, self.layout.viewport.height);
        }
    }

    /// The journal, rebuilt from what the file says.
    ///
    /// **This is where a recompile is survived**, and the one job the journal
    /// has that a document cannot do for itself: a paper rebuilt by LaTeX is a
    /// new file and every annotation went with it, but the words are usually
    /// still there. So each mark is written down with the passage it covers,
    /// and [`Viewer::restore_markup`] looks the quotes up again.
    ///
    /// The rule is **everything is thrown away and rebuilt from the file**,
    /// and what survives is only what the file cannot carry: a mark beside a
    /// document that could not be written, and a mark a rebuild lost.
    ///
    /// A mark is the same mark when its colour and its words are the same —
    /// not its page and not its index, because a rebuild moves a passage
    /// (which is the case this exists for) and an index shifts whenever an
    /// earlier annotation is added or taken away.
    ///
    /// The quotes are read once, with the marks: the folded copy compares,
    /// the plain one is written. Answers how many marks this reading lost.
    fn sync_journal(&mut self) -> usize {
        let inside: Vec<(String, String, crate::markup::Mark)> = self
            .markup
            .iter()
            .zip(&self.quotes)
            .map(|(mark, quote)| (mark.color.to_lowercase(), quote.clone(), mark.clone()))
            .collect();
        let mut next = Vec::new();
        let mut lost_now = 0;
        for held in self.store.journal() {
            let known = inside.iter().any(|(colour, quote, _)| {
                *colour == held.color.to_lowercase() && folded(quote) == folded(&held.quote)
            });
            if known {
                // In the file, so the file's own entry below is the one to
                // keep — this copy is last time's reading of it.
                continue;
            }
            // Was in the file and is not, while the file still carries others:
            // taken off in another app, not lost to a rebuild — a rebuild takes
            // them all. ponytail: the last one taken off elsewhere still reads
            // as a rebuild; a file id across rebuilds would tell them apart.
            if held.annotation_id.is_some() && !inside.is_empty() {
                continue;
            }
            // Not in the file. Either it never was, or it went with a
            // rebuild, and **`annotation_id` is what says so**: `None` is the
            // app's own mark for a highlight the document is not carrying,
            // and it is what the panel reads to know which rows to list and
            // which passages it can offer to put back.
            let mut lost = held.clone();
            if lost.annotation_id.take().is_some() {
                lost.lost = true;
                lost_now += 1;
            }
            next.push(lost);
        }
        for (_, quote, mark) in &inside {
            let height = self.document.size_of(mark.page.saturating_sub(1)).height;
            next.push(crate::store::Store::markup_entry(
                mark.page,
                crate::markup::flat(&mark.quads, height),
                &mark.color,
                quote,
                &mark.note,
                Some(format!("{}:{}", mark.page, mark.index)),
            ));
        }
        self.store.set_journal(next);
        lost_now
    }

    /// The marks the journal is holding that are not in the document: the
    /// ones a rebuild lost, and the ones a document that cannot be written
    /// never took.
    pub fn markup_adrift(&self) -> Vec<&crate::library::Highlight> {
        self.store
            .journal()
            .iter()
            .filter(|held| held.annotation_id.is_none())
            .collect()
    }

    /// How many of those could be put back, which is what the offer says.
    pub fn restorable(&self) -> usize {
        if !self.standing.into_file {
            return 0;
        }
        self.markup_adrift()
            .iter()
            .filter(|held| !held.quote.trim().is_empty())
            .count()
    }

    /// Look the lost passages up again and write back the ones still there.
    ///
    /// **A guess, and never a thing that happens on its own** — a button,
    /// because this reader does not write to somebody's file without being
    /// asked.
    ///
    /// The lookup starts on the page the passage used to be on and works
    /// outwards. What it does not find is left in the journal and counted out
    /// loud: a passage that was rewritten is not a passage that moved.
    pub fn restore_markup(&mut self) {
        let wanted: Vec<crate::library::Highlight> = self
            .markup_adrift()
            .into_iter()
            .filter(|held| !held.quote.trim().is_empty())
            .cloned()
            .collect();
        if wanted.is_empty() || !self.standing.into_file || self.busy() {
            return;
        }
        // Asked first, as marking asks: see [`Viewer::mark_selection`].
        if self.standing.signed && !self.said_standing {
            self.ask_to_break(
                CHANGING_BREAKS_A_SIGNATURE,
                "Put them back anyway",
                |viewer| {
                    viewer.said_standing = true;
                    viewer.restore_markup();
                },
            );
            return;
        }
        // Looked up on the thread, off the document as it is on disk: a
        // passage that was rewritten is a read of every page's text, which
        // is the stall `Viewer::write_step` exists to keep out of the window. The
        // journal is left as it is — the reload the write causes reads the
        // file, and a passage back in it is a row `sync_journal` replaces
        // with the file's own; one that was not found stays adrift.
        let password = self.document.password().map(str::to_string);
        let author = self.author();
        let counted = Arc::new(Mutex::new((0usize, 0usize)));
        let counting = Arc::clone(&counted);
        self.write_step_on(
            None,
            self.store.journal().to_vec(),
            move |path| {
                let document = crate::render::open_with(path, password.as_deref())
                    .map_err(|e| e.to_string())?;
                let mut found = Vec::new();
                let mut lost = 0;
                for held in &wanted {
                    match find_quote(&*document, held.page as usize, &held.quote) {
                        Some((page, quads)) => {
                            found.push((page, quads, held.color.clone(), held.note.clone()))
                        }
                        None => lost += 1,
                    }
                }
                drop(document);
                // Written, not found: counted as each colour lands, so a
                // failure part of the way says what did make it in.
                *counting.lock().unwrap_or_else(|e| e.into_inner()) = (0, lost);
                if found.is_empty() {
                    return Err(format!(
                        "{} could not be found in this document.",
                        said_of(lost, "passage", "passages"),
                    ));
                }
                // One rewrite per colour and comment, not per passage: `add`
                // takes a run of pages, and each call is the whole file saved
                // again. Most marks carry no comment, so this is still about
                // one per colour.
                let mut by_colour = std::collections::BTreeMap::<_, Vec<_>>::new();
                for (page, quads, color, note) in found {
                    by_colour
                        .entry((color, note))
                        .or_default()
                        .push((page, quads));
                }
                for ((color, note), runs) in &by_colour {
                    crate::markup::add_noted(path, runs, color, &author, note)?;
                    counting.lock().unwrap_or_else(|e| e.into_inner()).0 += runs.len();
                }
                Ok(())
            },
            move |viewer, written| {
                let (wrote, lost) = *counted.lock().unwrap_or_else(|e| e.into_inner());
                viewer.notice = match written {
                    Err(refused) if wrote > 0 => format!(
                        "{} put back. {refused}",
                        said_of(wrote, "passage", "passages"),
                    ),
                    Err(refused) => refused,
                    Ok(()) if lost == 0 => {
                        format!("{} put back.", said_of(wrote, "passage", "passages"))
                    }
                    Ok(()) => format!(
                        "{} put back. {} could not be found in this document.",
                        said_of(wrote, "passage", "passages"),
                        said_of(lost, "passage", "passages"),
                    ),
                };
            },
        );
    }

    /// Every mark the panel lists: the document's own first, in reading
    /// order, then whatever the journal is holding.
    ///
    /// One list, because to a reader they are one thing. They are told apart
    /// by a word on the row rather than by a section of their own.
    pub fn markup_rows(&self) -> Vec<MarkRow> {
        // Built once per reading of the document and of the journal, because
        // the panel asks on every scroll frame.
        let (edition, journal) = (self.edition, self.store.journal_rev());
        if let Some((for_edition, for_journal, rows)) = self.mark_rows.borrow().as_ref() {
            if *for_edition == edition && *for_journal == journal {
                return rows.clone();
            }
        }
        let rows = self.build_markup_rows();
        *self.mark_rows.borrow_mut() = Some((edition, journal, rows.clone()));
        rows
    }

    fn build_markup_rows(&self) -> Vec<MarkRow> {
        let mut inside: Vec<_> = self.markup.iter().zip(&self.quotes).collect();
        inside.sort_by(|(a, _), (b, _)| {
            (a.page, a.begins())
                .partial_cmp(&(b.page, b.begins()))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut rows: Vec<MarkRow> = inside
            .into_iter()
            .map(|(mark, quote)| MarkRow {
                page: mark.page,
                color: mark.color.clone(),
                quote: quote.clone(),
                note: mark.note.clone(),
                key: MarkKey::InFile(mark.page, mark.index),
            })
            .collect();
        // Only the ones the file is not already showing: the journal mirrors
        // every mark in the document as well, so that a rebuild has the
        // quotes to look up — see [`Viewer::sync_journal`] — and listing both
        // copies would list every mark twice.
        rows.extend(self.markup_adrift().into_iter().map(|held| MarkRow {
            page: held.page as usize,
            color: held.color.clone(),
            quote: held.quote.clone(),
            note: held.note.clone(),
            key: MarkKey::Beside(held.id.clone()),
        }));
        rows
    }

    /// Put the colour popover up over the selection, or say why not.
    ///
    /// Answers whether it opened, so that the gesture that opens it by
    /// itself — letting go of a sweep — can stay silent while the key says
    /// something.
    pub fn open_markup(&mut self) -> bool {
        let Some(sweep) = self.selection.filter(|sweep| !sweep.is_empty()) else {
            return false;
        };
        // Where the sweep *ends*, which is where the reader's pointer is and
        // is what the app anchors to as well — the last rectangle of the
        // range, not the first.
        let (_, last) = sweep.span();
        let areas = self.selected_areas(last.page);
        let Some(area) = areas.last().copied() else {
            return false;
        };
        self.menu = None;
        self.mark_open = None;
        self.commenting = None;
        self.markup_at = Some((last.page, area));
        true
    }

    /* ------------------------------------------------------------ comments */

    /// "Comment" in either popover: the field comes up in it, holding the
    /// mark's comment if it has one. A comment is the mark's `/Contents`, so
    /// it goes where marks go into the file, and nowhere else.
    pub fn begin_comment(&mut self) {
        if !self.standing.into_file {
            self.notice = format!(
                "{}, so a comment cannot be written into it.",
                self.standing.refused
            );
            return;
        }
        let had = match &self.mark_open {
            Some((_, _, MarkKey::InFile(page, index), _)) => self.note_of(*page, *index),
            Some((_, _, MarkKey::Beside(_), _)) => {
                self.notice =
                    "This highlight is kept beside the document, where a comment cannot go.".into();
                return;
            }
            None => String::new(),
        };
        self.commenting = Some(had);
    }

    pub fn type_comment(&mut self, typed: &str) {
        if let Some(draft) = &mut self.commenting {
            *draft = typed.to_string();
        }
    }

    /// ⌘Enter, Done, Escape, or a press anywhere else: onto the mark
    /// clicked, or onto the selection as a new mark in one of the six at
    /// random, so two comments side by side are told apart by colour, card
    /// and passage alike. The mark's menu goes with the field, changed or not.
    ///
    /// **Words are never thrown away.** While another write is under way the
    /// field stays up with the words in it, and the notice says why; `false`
    /// then, so that a press elsewhere does not go on to put the mark away
    /// under it.
    pub fn save_comment(&mut self) -> bool {
        let Some(typed) = &self.commenting else {
            return true;
        };
        let note = typed.trim().to_string();
        let writes = match &self.mark_open {
            Some((_, _, MarkKey::InFile(page, index), _)) => self.note_of(*page, *index) != note,
            _ => self.markup_at.is_some() && !note.is_empty(),
        };
        if writes && self.busy() {
            return false;
        }
        self.commenting = None;
        if let Some((_, _, MarkKey::InFile(page, index), _)) = self.mark_open.take() {
            self.note_markup(page, index, note);
        } else if self.markup_at.is_some() && !note.is_empty() {
            let colours = self.markup_colors();
            // Random enough: std seeds every `RandomState` afresh.
            let dice = std::hash::BuildHasher::hash_one(
                &std::collections::hash_map::RandomState::new(),
                0,
            );
            let colour = colours.get(dice as usize % colours.len().max(1));
            self.mark_noted(colour.map_or("#ffd60a", String::as_str), &note);
        }
        true
    }

    /// "Remove comment": the mark stays, its words go.
    pub fn remove_comment(&mut self) {
        if let Some((_, _, MarkKey::InFile(page, index), _)) = self.mark_open.clone() {
            self.note_markup(page, index, String::new());
        }
    }

    /// The name new marks and comments are signed with, or nothing. See
    /// `author` in `settings.rs`.
    pub fn author(&self) -> String {
        self.store.text("author").trim().to_string()
    }

    /// The name typed into Settings, kept once the typing stops.
    pub fn set_author(&mut self, name: String) {
        self.store.set_soon(vec![("author".into(), json!(name))]);
    }

    /// Who wrote a mark's comment and when, as its card says it: from the
    /// note the page has for it. Empty while the page's notes are not in.
    pub(super) fn byline_of(&self, page: usize, index: usize) -> String {
        let Some(at) = page.checked_sub(1) else {
            return String::new();
        };
        self.notes_on(at)
            .iter()
            .find(|note| {
                matches!(self.mark_of_note(page, note.rect),
                    Some((MarkKey::InFile(_, of), _)) if of == index)
            })
            .map(byline)
            .unwrap_or_default()
    }

    /// The comment on the mark at `index` on `page`, or nothing.
    pub fn note_of(&self, page: usize, index: usize) -> String {
        self.markup
            .iter()
            .find(|mark| mark.page == page && mark.index == index)
            .map(|mark| mark.note.clone())
            .unwrap_or_default()
    }

    /// A comment written onto a mark already in the file, or taken off it
    /// when empty. Asked as [`Viewer::recolour_markup`] asks.
    fn note_markup(&mut self, page: usize, index: usize, note: String) {
        if self.note_of(page, index) == note || self.busy() {
            return;
        }
        if self.standing.signed && !self.said_standing {
            self.ask_to_break(
                CHANGING_BREAKS_A_SIGNATURE,
                "Comment anyway",
                move |viewer| {
                    viewer.said_standing = true;
                    viewer.note_markup(page, index, note);
                },
            );
            return;
        }
        self.mark_open = None;
        let author = self.author();
        self.write_step(
            self.store.journal().to_vec(),
            move |path| crate::markup::set_note(path, page, index, &note, &author),
            |viewer, written| {
                if let Err(refused) = written {
                    viewer.notice = refused;
                }
            },
        );
    }

    /// Take it down again. `false` when it was not up, which is what lets
    /// Escape go on to the next thing it means.
    pub fn close_markup(&mut self) -> bool {
        self.markup_at.take().is_some()
    }

    /* --------------------------------------------- the highlight colours */

    /// The window that edits the six colours, over whatever is open. The
    /// swatches stay up underneath so that a colour chosen in the window can
    /// be used on the passage the reader has just swept.
    pub fn open_markup_colours(&mut self) {
        self.colours_open = true;
        self.recolour_to = None;
        self.begin_palette();
    }

    /// Take it down. `false` when it was not up, which is what lets Escape
    /// go on to the next thing it means.
    pub fn close_markup_colours(&mut self) -> bool {
        if !self.colours_open {
            return false;
        }
        // A picker open inside it goes first: Escape means the thing on top.
        if self.picking.take().is_some() {
            return true;
        }
        // **A palette being edited is saved with the window closing**, as a
        // theme is with Settings: a stray click beside the window must not
        // lose the work. A save refused keeps the window up, with why. Over a
        // highlight the change was the highlight's: see `palette_kept`.
        if self.palette_kept() && !self.save_palette() {
            return true;
        }
        self.colours_open = false;
        self.palette_draft = None;
        self.palette_from = None;
        if let (Some(hex), Some((_, _, key, _))) = (self.recolour_to.take(), self.mark_open.clone())
        {
            self.recolour_markup(&key, &hex);
        }
        true
    }

    /// "Apply" in the colours window: the mark it was opened over takes this
    /// colour now, and the window goes, its question answered.
    pub fn use_markup_colour(&mut self, hex: &str) {
        self.colours_open = false;
        self.picking = None;
        self.recolour_to = None;
        if let Some((_, _, key, _)) = self.mark_open.clone() {
            self.recolour_markup(&key, hex);
        }
    }

    /// One of the six, changed in the draft, as it would go into the file —
    /// [`crate::palette::legible`] — so that the field, the palette's file and
    /// the document all say the same colour. `at` is one-based.
    pub fn set_markup_color(&mut self, at: usize, hex: String) {
        let Some(rgb) = crate::palette::read_colour(&hex) else {
            return;
        };
        let hex = crate::palette::hex(crate::palette::legible(rgb));
        if self.mark_open.is_some() {
            self.recolour_to = Some(hex.clone());
        }
        if self.palette_draft.is_none() {
            self.begin_palette();
        }
        if let Some(slot) = self
            .palette_draft
            .as_mut()
            .and_then(|draft| draft.colors.get_mut(at.wrapping_sub(1)))
        {
            *slot = hex;
        }
    }

    /// The draft's name, as typed.
    pub fn set_palette_name(&mut self, name: String) {
        if let Some(draft) = self.palette_draft.as_mut() {
            draft.name = name;
        }
    }

    /// Begin editing the palette chosen, putting down any draft there was.
    fn begin_palette(&mut self) {
        let chosen = self.store.highlight_palette().clone();
        self.palette_from = Some(chosen.clone());
        self.palette_draft = Some(chosen);
    }

    /// Whether the draft has been changed since the editing began.
    fn palette_changed(&self) -> bool {
        self.palette_draft != self.palette_from
    }

    /// Whether the draft is to be saved without Save being pressed: changed,
    /// and not opened over a highlight. There a colour changed is that
    /// highlight's, and the palette keeps it only when asked — or recolouring
    /// one mark would write a copy of a shipped palette and make every mark
    /// after it in that.
    fn palette_kept(&self) -> bool {
        self.palette_changed() && self.mark_open.is_none()
    }

    /// Whether the draft differs from its file: changed, or a new palette
    /// that has no file yet.
    pub fn palette_unsaved(&self) -> bool {
        self.palette_draft
            .as_ref()
            .is_some_and(|draft| draft.id.is_empty() || self.palette_changed())
    }

    /// Begin a palette of the reader's own, from the colours of the one in
    /// use. Untouched, it is put down again when the window closes, as a new
    /// theme is: opening the editor and leaving makes nothing.
    pub fn new_palette(&mut self) {
        use crate::palettes::HighlightPalette;
        if self.palette_kept() && !self.save_palette() {
            return;
        }
        self.recolour_to = None;
        let draft = HighlightPalette {
            id: String::new(),
            name: HighlightPalette::free_name(self.store.palettes(), "New palette"),
            colors: self.store.highlight_palette().colors.clone(),
            built_in: false,
        };
        self.palette_from = Some(draft.clone());
        self.palette_draft = Some(draft);
    }

    /// Make another palette the one new marks are made in. A draft with
    /// changes in it is saved first, for the window's own reason: a click
    /// must not lose the work.
    pub fn choose_palette(&mut self, id: &str) {
        if self.palette_kept() && !self.save_palette() {
            return;
        }
        self.recolour_to = None;
        self.store
            .set(vec![("highlight_palette".into(), json!(id))]);
        self.begin_palette();
    }

    /// Write the draft to its file and make it the palette in use. A shipped
    /// palette is not written over: changed, it is saved as a copy — named
    /// for it, unless the reader named it — which is what editing a built-in
    /// theme does. `false`, with the reason on the notice line, when the
    /// checks or the disk refuse it.
    pub fn save_palette(&mut self) -> bool {
        use crate::palettes::HighlightPalette;
        let (Some(mut draft), Some(from)) = (self.palette_draft.clone(), self.palette_from.clone())
        else {
            return true;
        };
        if draft.built_in {
            if draft.name.trim() == from.name.trim() {
                draft.name = HighlightPalette::free_name(
                    self.store.palettes(),
                    &format!("{} copy", from.name),
                );
            }
            draft.place(String::new(), false);
        }
        let dir = self.store.palettes_dir().to_path_buf();
        match HighlightPalette::save(&dir, &draft) {
            Ok(saved) => {
                self.reload_palettes();
                // The file is named for the palette, so saving one renamed or
                // copied moves it: the setting goes with it.
                self.store
                    .set(vec![("highlight_palette".into(), json!(saved.id))]);
                self.notice = format!("Saved {}.", saved.name);
                self.palette_from = Some(saved.clone());
                self.palette_draft = Some(saved);
                true
            }
            Err(said) => {
                self.notice = said;
                false
            }
        }
    }

    /// Put the changes down and go back to the palette as saved — and, over a
    /// highlight, the colour it was to take with them. Whenever the draft is
    /// put down, so is `recolour_to`: the highlight takes only a colour the
    /// window still shows.
    pub fn discard_palette(&mut self) {
        self.picking = None;
        self.recolour_to = None;
        match &self.palette_from {
            Some(from) if !from.id.is_empty() => self.palette_draft = Some(from.clone()),
            // A new palette has nothing to go back to but the one in use.
            _ => self.begin_palette(),
        }
    }

    /// Ask whether to delete the palette being edited, which is only ever
    /// one of the reader's own.
    pub fn ask_delete_palette(&mut self) {
        let Some(from) = self.palette_from.clone().filter(|one| !one.built_in) else {
            return;
        };
        self.asking = Some(Asking {
            title: format!("Delete {}?", from.name),
            says: "This cannot be undone.".into(),
            keep: "Keep it",
            go: "Delete palette",
            then: Box::new(move |viewer| viewer.delete_palette(&from)),
        });
    }

    /// Delete one, and go back to the shipped default.
    fn delete_palette(&mut self, gone: &crate::palettes::HighlightPalette) {
        use crate::palettes::HighlightPalette;
        let dir = self.store.palettes_dir().to_path_buf();
        match HighlightPalette::delete(&dir, &gone.id) {
            Ok(()) => {
                self.reload_palettes();
                self.store.set(vec![(
                    "highlight_palette".into(),
                    json!(crate::palettes::DEFAULT),
                )]);
                self.begin_palette();
                self.notice = format!("Deleted {}.", gone.name);
            }
            Err(said) => self.notice = said,
        }
    }

    /// The palettes directory, read again.
    fn reload_palettes(&mut self) {
        use crate::palettes::HighlightPalette;
        let dir = self.store.palettes_dir().to_path_buf();
        self.store.set_palettes(HighlightPalette::load_all(&dir));
    }

    /// The palettes as the folder now has them: a file saved here, in
    /// another window, or by hand. A draft with nothing changed in it follows
    /// its file; one with changes is the reader's and is left alone.
    pub fn palettes_changed(&mut self, palettes: Vec<crate::palettes::HighlightPalette>) {
        self.store.set_palettes(palettes);
        if self.palette_draft.is_some() && !self.palette_unsaved() {
            self.begin_palette();
        }
    }

    /// The six colours the popover offers, in the order the swatches show
    /// them: the palette being edited, else the one in use. Each as it goes
    /// into the file — see [`crate::palette::offered`].
    pub fn markup_colors(&self) -> Vec<String> {
        self.palette_draft
            .as_ref()
            .unwrap_or_else(|| self.store.highlight_palette())
            .colors
            .iter()
            .map(|colour| crate::palette::offered(colour))
            .collect()
    }

    /* ------------------------------------------------------ highlighting */

    /// Mark what is selected, in this colour.
    ///
    /// The whole gesture in order: the quads come off the selection, the
    /// document is let go of, the write goes in, and the document is reopened
    /// through the path a recompile uses. There is no pending layer and no
    /// markup revision in any cache key — saving immediately makes the
    /// deferred machinery unnecessary rather than merely delayed, and a reload
    /// rebuilds every cache there is.
    ///
    /// The write and the reopen are on a thread of their own, and the second
    /// half of this runs when they land. See [`Viewer::write_step`].
    pub fn mark_selection(&mut self, color: &str) {
        self.mark_noted(color, "");
    }

    /// [`Viewer::mark_selection`], with a comment on the mark.
    fn mark_noted(&mut self, color: &str, note: &str) {
        if self.busy() {
            return;
        }
        let offered = self.markup_at.take();
        let Some(sweep) = self.selection.filter(|sweep| !sweep.is_empty()) else {
            // Two different sentences, and the difference is the point. A
            // scan has no text in it at all, so there is nothing this gesture
            // could ever mark and no amount of selecting will help.
            self.notice = if self.text_on(self.page()).is_empty() {
                "There is no text on this page to highlight.".into()
            } else {
                "Select something first, and this highlights it.".into()
            };
            return;
        };
        let runs: Vec<(usize, Vec<Rect>)> = sweep
            .pages()
            .filter_map(|page| {
                let text = self.text_on(page);
                let (from, to) = sweep.range_on(page, text.chars.len())?;
                let quads = text.quads(from, to);
                (!quads.is_empty()).then_some((page, quads))
            })
            .collect();
        if runs.is_empty() {
            self.notice = "There is nothing there to highlight.".into();
            return;
        }
        let quote = self.selected_text();
        if !self.standing.into_file {
            let kept = self.beside(&runs, &quote);
            return self.keep_beside(kept, color);
        }
        // Signed, and asked once, *before* the write: it is their document,
        // and a rewrite is exactly the thing a signature is there to detect.
        // This said so after the file was already rewritten. The selection
        // and the swatches stay, for "Highlight anyway" and for Cancel.
        if self.standing.signed && !self.said_standing {
            self.markup_at = offered;
            let (color, note) = (color.to_string(), note.to_string());
            self.ask_to_break(
                CHANGING_BREAKS_A_SIGNATURE,
                "Highlight anyway",
                move |viewer| {
                    viewer.said_standing = true;
                    viewer.mark_noted(&color, &note);
                },
            );
            return;
        }
        // **Let go of the file before writing it, and reopen whatever
        // happens.** pdfium keeps the file open for the life of the document,
        // and on Windows nothing can rename over it or truncate it while it
        // does. The reopen is unconditional because a released document draws
        // nothing, so a failed write must still leave the reader looking at
        // their document. See [`crate::render::PageSource::release`], which
        // [`Viewer::write_step`] calls.
        self.selection = None;
        // Worked out now, off the document the passage was chosen in: a
        // refused write lands after the reopen, and a draft that has since
        // lost pages made this an index past the end of it.
        let kept = self.beside(&runs, &quote);
        if let Some(rgb) = crate::palette::read_colour(color) {
            self.marking = runs
                .iter()
                .map(|(page, quads)| (*page, quads.clone(), None, Some(rgb)))
                .collect();
        }
        let (writing, color) = (runs, color.to_string());
        let (colour, note, author) = (color.clone(), note.to_string(), self.author());
        let lost = if note.is_empty() {
            ""
        } else {
            " The comment could not go with it."
        };
        self.write_step(
            self.store.journal().to_vec(),
            move |path| crate::markup::add_noted(path, &writing, &colour, &author, &note),
            move |viewer, written| {
                viewer.show_markup_panel();
                match written {
                    // Nothing said: the mark on the page is the answer.
                    Ok(()) => {}
                    Err(refused) => {
                        // The file is as it was, so there is nothing to put
                        // back. The mark is kept beside the document instead,
                        // which is the answer a read-only file gets and for
                        // the same reason: a passage the reader marked is not
                        // lost because the disk said no.
                        viewer.keep_beside(kept, &color);
                        viewer.notice =
                            format!("{refused} The highlight is kept beside the document.{lost}");
                    }
                }
            },
        );
    }

    /// Keep a mark beside the document rather than in it, and say so once.
    /// A passage as the journal holds it: per page, its quads flattened
    /// against that page's height, and the words on it.
    fn beside(&self, runs: &[(usize, Vec<Rect>)], quote: &str) -> Vec<(usize, Vec<f64>, String)> {
        runs.iter()
            .map(|(page, quads)| {
                let height = self.document.size_of(page.saturating_sub(1)).height;
                // Each page keeps the words on it: a passage is looked for a
                // page at a time, and the whole of one that runs over two is
                // on neither.
                let own = if runs.len() > 1 {
                    crate::markup::quote_under(&self.text_on(*page), quads)
                } else {
                    quote.to_string()
                };
                (*page, crate::markup::flat(quads, height), own)
            })
            .collect()
    }

    fn keep_beside(&mut self, kept: Vec<(usize, Vec<f64>, String)>, color: &str) {
        let journal = self.store.journal().to_vec();
        self.did(Step {
            journal,
            file: None,
        });
        for (page, flat, own) in kept {
            self.store.keep_markup(page, &flat, color, &own);
        }
        self.selection = None;
        self.show_markup_panel();
        let why = self.standing.refused.clone();
        self.notice = if self.said_standing || why.is_empty() {
            "Highlighted, beside the document.".into()
        } else {
            self.said_standing = true;
            format!("Highlighted. {why}, so the highlight is kept beside it rather than in it.")
        };
    }

    /// A document with a passage marked has something to show in the Contents
    /// panel after all — [`Viewer::mark_page`]'s rule, and as narrow: only
    /// when the panel is already open and the tab it is on has nothing on it.
    /// Taking a reader off the thumbnails they were looking at is the panel
    /// arguing.
    fn show_markup_panel(&mut self) {
        if self.sidebar_open && self.tab == Tab::Pages && self.headings.is_empty() {
            self.tab = Tab::Contents;
        }
    }

    /// **A removal is two presses.** The first turns the small × or bin into
    /// the word for what it does, and the second does it: a highlight taken
    /// out of the file or a signature deleted is gone for good, and the
    /// buttons are small and sit right beside the row they belong to.
    /// Answers whether this press is the one that acts.
    pub fn arm(&mut self, what: Arming) -> bool {
        if self.arming.as_ref() == Some(&what) {
            self.arming = None;
            true
        } else {
            self.arming = Some(what);
            false
        }
    }

    /// Take one mark out, wherever it is being kept.
    ///
    /// **A mark in the document comes out of the document**:
    /// `FPDFPage_RemoveAnnot`, a reopen, and it is gone from the file for
    /// every reader of it. The app cannot say that — see [`crate::markup`].
    ///
    /// Answers whether it stopped to ask first — a signed document — so that
    /// the popover the question came from can stay for the answer.
    pub fn remove_markup(&mut self, key: &MarkKey) -> bool {
        match key {
            MarkKey::Beside(id) => {
                let journal = self.store.journal().to_vec();
                self.did(Step {
                    journal,
                    file: None,
                });
                // Nothing said: the mark going from the page is the answer.
                self.store.drop_markup(id);
            }
            MarkKey::InFile(page, index) => {
                if self.busy() {
                    return false;
                }
                // Asked here as it is asked before a mark goes in: an
                // encrypted or read-only document cannot have one taken out
                // either, and finding that out from a failed write costs the
                // reader the document on screen.
                if !self.standing.into_file {
                    self.notice = format!(
                        "{}, so the highlight cannot be taken out of it.",
                        self.standing.refused
                    );
                    return false;
                }
                // Asked as marking asks, and before the journal is touched:
                // see [`Viewer::mark_selection`].
                if self.standing.signed && !self.said_standing {
                    let key = key.clone();
                    self.ask_to_break(
                        CHANGING_BREAKS_A_SIGNATURE,
                        "Remove anyway",
                        move |viewer| {
                            viewer.said_standing = true;
                            if !viewer.remove_markup(&key) {
                                viewer.close_mark();
                            }
                        },
                    );
                    return true;
                }
                // **The journal has to be told first**, or the reload cannot
                // tell "the reader took this off" from "a rebuild lost it" —
                // the mark is gone from the file either way, and the second
                // reading offers it straight back.
                // By the annotation it is the reading of, which `sync_journal`
                // wrote down: the same words in the same colour kept beside
                // the document is another mark, and stays.
                let taking = format!("{page}:{index}");
                let journal = self.store.journal().to_vec();
                let keeping: Vec<crate::library::Highlight> = self
                    .store
                    .journal()
                    .iter()
                    .filter(|held| held.annotation_id.as_deref() != Some(taking.as_str()))
                    .cloned()
                    .collect();
                self.store.set_journal(keeping);
                let (page, index) = (*page, *index);
                // The comment goes with it, and nothing on the page shows
                // that it went: said, with the way back.
                let noted = !self.note_of(page, index).is_empty();
                // Off the page this frame, not when the rewrite lands. See
                // [`crate::page::Ramped::marking`].
                self.marking = self
                    .markup
                    .iter()
                    .filter(|mark| mark.page == page && mark.index == index)
                    .filter_map(|mark| {
                        let rgb = crate::palette::read_colour(&mark.color)?;
                        Some((page, mark.quads.clone(), Some(rgb), None))
                    })
                    .collect();
                self.write_step(
                    journal,
                    move |path| crate::markup::remove(path, page, index),
                    move |viewer, taken| match taken {
                        Err(refused) => viewer.notice = refused,
                        Ok(()) if noted => {
                            let undo = viewer.chord_for(Action::Undo);
                            viewer.notice = if undo.is_empty() {
                                "Comment also removed.".into()
                            } else {
                                format!("Comment also removed.\nPress {undo} to undo.")
                            };
                        }
                        Ok(()) => {}
                    },
                );
            }
        }
        false
    }

    /// Give one mark another colour, wherever it is being kept. The popover
    /// goes: the colour on the page is the answer.
    ///
    /// Answers whether it stopped to ask first, as [`Viewer::remove_markup`]
    /// does and for the popover's sake.
    pub fn recolour_markup(&mut self, key: &MarkKey, colour: &str) -> bool {
        match key {
            MarkKey::Beside(id) => {
                let journal = self.store.journal().to_vec();
                let changed = journal
                    .iter()
                    .cloned()
                    .map(|mut held| {
                        if held.id == *id {
                            held.color = colour.to_string();
                        }
                        held
                    })
                    .collect();
                self.did(Step {
                    journal,
                    file: None,
                });
                self.store.set_journal(changed);
            }
            MarkKey::InFile(page, index) => {
                if self.busy() {
                    return false;
                }
                let Some(mark) = self
                    .markup
                    .iter()
                    .find(|mark| mark.page == *page && mark.index == *index)
                    .cloned()
                else {
                    return false;
                };
                if mark.color.eq_ignore_ascii_case(colour) {
                    self.mark_open = None;
                    return false;
                }
                if !self.standing.into_file {
                    self.notice = format!(
                        "{}, so the highlight cannot be changed in it.",
                        self.standing.refused
                    );
                    return false;
                }
                if self.standing.signed && !self.said_standing {
                    let (key, colour) = (key.clone(), colour.to_string());
                    self.ask_to_break(
                        CHANGING_BREAKS_A_SIGNATURE,
                        "Change anyway",
                        move |viewer| {
                            viewer.said_standing = true;
                            if !viewer.recolour_markup(&key, &colour) {
                                viewer.close_mark();
                            }
                        },
                    );
                    return true;
                }
                // The journal needs nothing: the old mark's entry is last
                // time's reading of a file that still carries marks, which
                // `sync_journal` lets go of, and the new mark is read in.
                self.marking = vec![(
                    mark.page,
                    mark.quads.clone(),
                    crate::palette::read_colour(&mark.color),
                    crate::palette::read_colour(colour),
                )];
                let (page, index, colour) = (*page, *index, colour.to_string());
                self.write_step(
                    self.store.journal().to_vec(),
                    move |path| crate::markup::recolour(path, page, index, &colour),
                    |viewer, changed| {
                        if let Err(refused) = changed {
                            viewer.notice = refused;
                        }
                    },
                );
            }
        }
        self.mark_open = None;
        false
    }

    /// "Remove all highlights", asked first: every mark on this document,
    /// the ones kept beside it too.
    pub fn ask_remove_all_markup(&mut self) {
        self.menu = None;
        let beside = self
            .store
            .journal()
            .iter()
            .filter(|held| held.annotation_id.is_none())
            .count();
        let count = self.markup.len() + beside;
        if count == 0 {
            self.notice = "There are no highlights in this document.".into();
            return;
        }
        // Said now rather than after a yes that could not be kept.
        if !self.markup.is_empty() && !self.standing.into_file {
            self.notice = format!(
                "{}, so its highlights cannot be taken out of it.",
                self.standing.refused
            );
            return;
        }
        let mut says = if count == 1 {
            "The one highlight in this document is removed.".to_string()
        } else {
            format!("All {count} highlights in this document are removed.")
        };
        // One question, not this one and then the signature's.
        if self.standing.signed && !self.markup.is_empty() {
            says = format!("{says} {CHANGING_BREAKS_A_SIGNATURE}");
        }
        self.asking = Some(Asking {
            title: "Remove all highlights?".into(),
            says,
            keep: "Keep them",
            go: "Remove all",
            then: Box::new(|viewer| viewer.remove_all_markup()),
        });
    }

    /// The yes to [`Viewer::ask_remove_all_markup`]: the journal emptied and,
    /// if the file has any, one write that takes them all out of it.
    fn remove_all_markup(&mut self) {
        if !self.markup.is_empty() && self.busy() {
            return;
        }
        // The journal first, as [`Viewer::remove_markup`] says why.
        let journal = self.store.journal().to_vec();
        self.store.set_journal(Vec::new());
        if self.markup.is_empty() {
            return self.did(Step {
                journal,
                file: None,
            });
        }
        self.said_standing = true;
        let mut pages: Vec<usize> = self.markup.iter().map(|mark| mark.page).collect();
        pages.sort_unstable();
        pages.dedup();
        self.marking = self
            .markup
            .iter()
            .filter_map(|mark| {
                let rgb = crate::palette::read_colour(&mark.color)?;
                Some((mark.page, mark.quads.clone(), Some(rgb), None))
            })
            .collect();
        self.write_step(
            journal,
            move |path| crate::markup::remove_all(path, &pages),
            |viewer, taken| {
                if let Err(refused) = taken {
                    viewer.notice = refused;
                }
            },
        );
    }

    /* ------------------------------------------------------ undo and redo */

    /// A write of our own made: kept to be taken back, and whatever was taken
    /// back before it is no longer there to redo.
    pub(super) fn did(&mut self, step: Step) {
        self.redo.clear();
        self.undo.push(step);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
    }

    pub(super) fn forget_steps(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// ⌘Z: the last change to the file taken back — a signature, or a mark made, taken off,
    /// recoloured, or every mark taken off at once.
    pub fn undo(&mut self) {
        self.step_back(false);
    }

    /// ⌘⇧Z: the last one taken back, made again.
    pub fn redo(&mut self) {
        self.step_back(true);
    }

    /// **The file is put back whole**, from the copy taken before the change
    /// went in, rather than the change worked backwards: a mark taken off and
    /// made again would lose its note, its author and its replies. A change
    /// to the file by anybody else forgets every step (see
    /// [`Viewer::document_changed`]), and the stamp
    /// check in [`Viewer::write_file`] catches the one that lands meanwhile.
    fn step_back(&mut self, redoing: bool) {
        if self.busy() {
            return;
        }
        let taking = if redoing {
            &mut self.redo
        } else {
            &mut self.undo
        };
        let Some(step) = taking.pop() else {
            self.notice = if redoing {
                "Nothing to redo.".into()
            } else {
                "Nothing to undo.".into()
            };
            return;
        };
        self.mark_open = None;
        self.markup_at = None;
        // Told first, as [`Viewer::remove_markup`] says why.
        let now = self.store.journal().to_vec();
        // Marked before the step, after it, or now: the pages the file put
        // back can hold a mark on.
        let touched = self.marked_pages(now.iter().chain(&step.journal));
        self.store.set_journal(step.journal);
        let Some(before) = step.file else {
            return self.opposite(redoing, now, None);
        };
        let after = crate::markup::Before::make();
        let keeping = after.path().to_path_buf();
        self.write_file(
            Some(touched),
            move |path| {
                crate::markup::Before::take_to(&keeping, path)?;
                before.put_back(path)
            },
            move |viewer, written| match written {
                Ok(()) => viewer.opposite(redoing, now, Some(after)),
                Err(refused) => {
                    viewer.store.set_journal(now);
                    viewer.forget_steps();
                    viewer.notice = refused;
                }
            },
        );
    }

    /// What undoing leaves to redo, and redoing to undo.
    fn opposite(
        &mut self,
        redoing: bool,
        journal: Vec<crate::library::Highlight>,
        file: Option<crate::markup::Before>,
    ) {
        let other = if redoing {
            &mut self.undo
        } else {
            &mut self.redo
        };
        other.push(Step { journal, file });
    }
}
