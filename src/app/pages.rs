//! The Viewer and page edits: saving pages as a new file, combining files,
//! and the edits written into the open document. `crate::arrange` writes
//! them; this decides when, where, and what the reader is told.

use super::*;
use crate::arrange::Edit;

impl Viewer {
    /* ------------------------------------------- new files from old ones */

    /// The pages the Document menu's range field offers before anything is
    /// typed into it, as they are called on screen: the pages selected in
    /// the thumbnails, or else the page being read.
    pub fn pages_offered(&self) -> String {
        let label = |page| self.label(page);
        match &self.pages_typed {
            Some(typed) => typed.clone(),
            None if !self.picked.is_empty() => crate::arrange::typed(&self.picked_pages(), label),
            None => crate::arrange::typed(&[self.page()], label),
        }
    }

    pub fn type_pages(&mut self, typed: &str) {
        self.pages_typed = Some(typed.to_string());
    }

    /// "Save pages as a new file…": the pages typed in the Document menu,
    /// saved where the reader says.
    pub fn save_typed_pages(&mut self) {
        let offered = self.pages_offered();
        match crate::arrange::parse_pages(&offered, |typed| self.page_for_label(typed)) {
            Ok(pages) => {
                self.close_menu();
                self.save_pages_as(pages);
            }
            Err(said) => self.notice = said,
        }
    }

    /// "Select pages…": the thumbnails, with the page being read selected
    /// if nothing is, so that the bar under them is up from the start.
    pub fn select_pages(&mut self) {
        self.close_menu();
        // The reader asked for the panel, so it stays open as a setting
        // would. See `toggle_sidebar`.
        self.results_borrowed = false;
        self.set_sidebar(true, true);
        self.show_tab(Tab::Pages);
        if self.picked.is_empty() {
            self.picked = std::iter::once(self.page()).collect();
            self.picked_from = Some(self.page());
        }
        // Delete and ⌘A act on the thumbnails, as after a click on one.
        self.column_last = true;
    }

    /// No page selected in the thumbnails, so the bar under them goes, and
    /// Delete and ⌘A act on the document again.
    pub fn deselect_pages(&mut self) {
        self.close_menu();
        self.picked.clear();
        self.picked_from = None;
        self.column_last = false;
    }

    /// Ask where `pages` go as a new file. The document is not changed.
    pub fn save_pages_as(&mut self, pages: Vec<usize>) {
        if self.empty() || pages.is_empty() {
            return;
        }
        let how = file_safe(&crate::arrange::named(&pages, |page| self.label(page)));
        let suggested = crate::arrange::beside(self.document.path(), &how);
        self.choose.ask(Choosing::SavePages(pages), Some(suggested));
    }

    /// "Insert pages from a file…": the pages of a file the reader picks,
    /// put in before `before`. The other file is not changed.
    pub fn insert_from_file(&mut self, before: usize) {
        if self.empty() {
            return;
        }
        self.close_menu();
        self.choose.ask(Choosing::InsertFrom(before), None);
    }

    /// "Combine files into a new file…": several files, in the order picked.
    pub fn combine_files(&mut self) {
        self.close_menu();
        self.choose.ask(Choosing::Combine, None);
    }

    /// A file dialog a page edit opened has answered.
    pub fn chosen(&mut self, choosing: Choosing, paths: Vec<String>) {
        let Some(first) = paths.first().cloned() else {
            return;
        };
        match choosing {
            Choosing::SavePages(pages) => self.write_pages(pages, first),
            Choosing::Combine => {
                let suggested = crate::arrange::beside(&first, "combined");
                self.choose
                    .ask(Choosing::CombineInto(paths), Some(suggested));
            }
            Choosing::CombineInto(sources) => self.write_combined(sources, first),
            Choosing::Copy(edit) => self.edit_into_copy(edit, first),
            Choosing::InsertFrom(before) => self.edit_pages(Edit::Insert(first, before)),
            Choosing::Split => self.write_chapters(first),
            Choosing::ExportHighlights(format) => self.write_highlights(format, first),
            Choosing::SavePicture(page) => self.write_picture(page, first),
            Choosing::SaveText => self.write_text(first),
        }
    }

    /// The document's chapters, read off its contents: each heading at the
    /// top level, with the pages from its own to the page before the next
    /// one's. Where the top level has one heading, usually the document's
    /// title, the headings under it are the chapters. Pages before the
    /// first chapter are one more, "Front matter". Whether a chapter also
    /// takes the page where the next one begins is decided when it is
    /// saved; see [`crate::arrange::ends_there`].
    pub fn chapters(&self) -> Vec<crate::arrange::Chapter> {
        let top = match self.headings.iter().filter(|h| h.depth == 0).count() {
            1 => 1,
            _ => 0,
        };
        let mut starts: Vec<(usize, &str, Option<crate::render::Pin>)> = (self.headings.iter())
            .filter(|heading| heading.depth == top)
            .filter_map(|heading| Some((heading.page?, heading.title.as_str(), heading.spot)))
            .collect();
        // In page order, and one chapter to a page: two headings on one
        // page are one chapter, under the first one's name.
        starts.sort_by_key(|(page, _, _)| *page);
        starts.dedup_by_key(|(page, _, _)| *page);
        if starts.first().is_some_and(|(page, _, _)| *page > 1) {
            starts.insert(0, (1, "Front matter", None));
        }
        let end = self.document.pages() + 1;
        (starts.iter().enumerate())
            .map(|(at, (from, title, _))| {
                let next = starts.get(at + 1);
                crate::arrange::Chapter {
                    title: title.to_string(),
                    pages: (*from..next.map_or(end, |(next, _, _)| *next)).collect(),
                    next: next.and_then(|(page, _, spot)| Some((*page, (*spot)?))),
                }
            })
            .collect()
    }

    /// "Save the page as a picture…": asks where page `page` (one-based)
    /// goes.
    pub fn save_picture(&mut self, page: usize) {
        self.close_menu();
        let suggested =
            crate::arrange::beside(self.document.path(), &format!("page {}", self.label(page)));
        let suggested = std::path::Path::new(&suggested).with_extension("png");
        self.choose.ask(
            Choosing::SavePicture(page),
            Some(suggested.to_string_lossy().into_owned()),
        );
    }

    fn write_picture(&mut self, page: usize, to: String) {
        let document = self.document.clone();
        let label = self.label(page);
        let taken = self.on_thread(
            {
                let to = to.clone();
                move || crate::export::save_picture(&*document, page - 1, &to)
            },
            move |viewer, saved| match saved {
                Ok(()) => {
                    let name = crate::store::file_name(&to);
                    viewer.notice = format!("Saved page {label} as {name}.");
                }
                Err(said) => viewer.notice = said,
            },
        );
        if !taken {
            self.notice = "Still saving the last file. Try again in a moment.".into();
        }
    }

    /// "Save the text as a file…": asks where the text goes.
    pub fn save_text(&mut self) {
        self.close_menu();
        let suggested = crate::arrange::beside(self.document.path(), "text");
        let suggested = std::path::Path::new(&suggested).with_extension("txt");
        self.choose.ask(
            Choosing::SaveText,
            Some(suggested.to_string_lossy().into_owned()),
        );
    }

    /// The text of every page written to `to`. It is read on the thread
    /// that writes it, since reading a long book's text takes a while.
    fn write_text(&mut self, to: String) {
        let document = self.document.clone();
        let taken = self.on_thread(
            {
                let to = to.clone();
                move || {
                    let text = crate::export::text(&*document)?;
                    if text.is_empty() {
                        return Err(NO_TEXT.to_string());
                    }
                    crate::atomic_write(std::path::Path::new(&to), text.as_bytes())
                }
            },
            move |viewer, saved| match saved {
                Ok(()) => {
                    let name = crate::store::file_name(&to);
                    viewer.notice = format!("Saved the text as {name}.");
                }
                Err(said) => viewer.notice = said,
            },
        );
        if !taken {
            self.notice = "Still saving the last file. Try again in a moment.".into();
        }
    }

    /// "Save each chapter as a file…": asks for the folder they go into.
    pub fn split_by_chapter(&mut self) {
        self.close_menu();
        if self.chapters().len() < 2 {
            self.notice = NO_CHAPTERS.into();
            return;
        }
        self.choose.ask(Choosing::Split, None);
    }

    /// Every chapter saved into `folder` as a file named after it, in the
    /// order of the document. Nothing in the folder is written over.
    fn write_chapters(&mut self, folder: String) {
        let chapters = self.chapters();
        if chapters.len() < 2 {
            self.notice = NO_CHAPTERS.into();
            return;
        }
        let path = self.document.path().to_string();
        let digits = chapters.len().to_string().len();
        let files: Vec<(crate::arrange::Chapter, String)> = (chapters.into_iter().enumerate())
            .map(|(at, chapter)| {
                let mut title = file_safe(&chapter.title);
                if title.is_empty() {
                    title = "Chapter".into();
                }
                let name = format!("{:0digits$} {title}.pdf", at + 1);
                (
                    chapter,
                    std::path::Path::new(&folder)
                        .join(name)
                        .to_string_lossy()
                        .into_owned(),
                )
            })
            .collect();
        let password = self.document.password().map(str::to_string);
        let count = files.len();
        let into = crate::store::file_name(&folder);
        let document = self.document.clone();
        let taken = self.on_thread(
            move || {
                if let Some((_, taken)) = files
                    .iter()
                    .find(|(_, to)| std::path::Path::new(to).exists())
                {
                    return Err((
                        0,
                        format!(
                            "{} is already in that folder. Choose another folder.",
                            crate::store::file_name(taken)
                        ),
                    ));
                }
                let mut lost = false;
                for (saved, (chapter, to)) in files.iter().enumerate() {
                    // Read here rather than on the thread that draws the
                    // window, since it reads the text of several pages.
                    let mut pages = chapter.pages.clone();
                    if let Some((page, spot)) = chapter.next {
                        if crate::arrange::ends_there(&*document, page, spot) {
                            pages.push(page);
                        }
                    }
                    lost |= crate::arrange::save_pages(&path, password.as_deref(), &pages, to)
                        .map_err(|said| (saved, said))?;
                }
                Ok::<bool, (usize, String)>(lost)
            },
            move |viewer, saved| match saved {
                Ok(lost) => {
                    let lost = if lost { LINKS_INTO_OTHER_CHAPTERS } else { "" };
                    viewer.notice = format!("Saved {count} chapters as files in {into}.{lost}");
                }
                Err((0, said)) => viewer.notice = said,
                // The files already written stay, and the reader is told
                // how many there are.
                Err((saved, said)) => {
                    viewer.notice =
                        format!("Saved {saved} of {count} chapters as files in {into}. {said}")
                }
            },
        );
        if !taken {
            self.notice = "Still saving the last file. Try again in a moment.".into();
        }
    }

    fn write_pages(&mut self, pages: Vec<usize>, to: String) {
        let path = self.document.path().to_string();
        let password = self.document.password().map(str::to_string);
        let named = crate::arrange::named(&pages, |page| self.label(page));
        let taken = self.on_thread(
            {
                let to = to.clone();
                move || {
                    if same_file(&path, &to) {
                        return Err("Choose a new name. The pages cannot be saved over the \
                                    document they come from."
                            .into());
                    }
                    crate::arrange::save_pages(&path, password.as_deref(), &pages, &to)
                }
            },
            move |viewer, saved| match saved {
                Ok(lost) => {
                    let name = crate::store::file_name(&to);
                    let lost = if lost { LINKS_INTO_THE_DOCUMENT } else { "" };
                    viewer.offer_open(format!("Saved {named} as {name}.{lost}"), to);
                }
                Err(said) => viewer.notice = said,
            },
        );
        if !taken {
            self.notice = "Still saving the last file. Try again in a moment.".into();
        }
    }

    fn write_combined(&mut self, sources: Vec<String>, to: String) {
        let count = sources.len();
        let taken = self.on_thread(
            {
                let to = to.clone();
                move || {
                    if sources.iter().any(|source| same_file(source, &to)) {
                        return Err("Choose a new name. The combined file cannot be saved over \
                                    one of the files in it."
                            .into());
                    }
                    crate::arrange::combine(&sources, &to)
                }
            },
            move |viewer, combined| match combined {
                Ok((pages, lost)) => {
                    let name = crate::store::file_name(&to);
                    viewer.notice = format!(
                        "Combined {count} files into {name}, {pages} {}.{}",
                        if pages == 1 { "page" } else { "pages" },
                        if lost { LINKS_BETWEEN_PAGES } else { "" }
                    );
                    if viewer.empty() {
                        viewer.open_here(&to);
                    } else {
                        viewer.frame.ask(Ask::NewWindowOn(to));
                    }
                }
                Err(said) => viewer.notice = said,
            },
        );
        if !taken {
            self.notice = "Still saving the last file. Try again in a moment.".into();
        }
    }

    /// Say `notice`, with a button beside it that opens `path` in a window
    /// of its own.
    fn offer_open(&mut self, notice: String, path: String) {
        self.notice = notice.clone();
        self.notice_opens = Some((notice, path));
    }

    /// The file the notice offers to open, while that notice is up.
    pub fn notice_offers(&self) -> Option<String> {
        (self.notice_opens.as_ref())
            .filter(|(said, _)| *said == self.notice)
            .map(|(_, path)| path.clone())
    }

    /// The notice's Open button.
    pub fn open_offered(&mut self) {
        if let Some(path) = self.notice_offers() {
            self.notice.clear();
            self.notice_opens = None;
            self.frame.ask(Ask::NewWindowOn(path));
        }
    }
}

impl Viewer {
    /* ------------------------------------------- edits of the document */

    /// A click on a thumbnail. A plain click goes to the page and selects it
    /// alone; ⌘-click (Ctrl elsewhere) adds or takes away a page without
    /// going to it; ⇧-click selects every page from the last one clicked.
    pub fn click_thumb(&mut self, page: usize, adding: bool, extending: bool) {
        // The release of a drag, which moved the pages or put them back.
        if std::mem::take(&mut self.dragged) {
            return;
        }
        self.column_last = true;
        if extending {
            let from = self.picked_from.unwrap_or_else(|| self.page());
            self.picked = (from.min(page)..=from.max(page)).collect();
            return;
        }
        if adding {
            if !self.picked.remove(&page) {
                self.picked.insert(page);
            }
        } else {
            self.picked = std::iter::once(page).collect();
            self.go_to_page(page);
        }
        self.picked_from = Some(page);
    }

    /* ----------------------------------------------- dragging thumbnails */

    /// A press on a thumbnail with the main button: a drag, if it moves far
    /// enough before it is let go of. A click never moves a page.
    pub fn press_thumb(&mut self, page: usize, at: (f64, f64)) {
        self.dragged = false;
        self.thumb_press = Some((page, at));
    }

    /// Whether a thumbnail is pressed, and so possibly being dragged.
    pub fn pressed_on_thumb(&self) -> bool {
        self.thumb_press.is_some()
    }

    /// The pointer carried with a thumbnail pressed. Past a few pixels the
    /// pages lift, and where they would land is worked out from the rows;
    /// near the top or bottom of the column, the column scrolls.
    pub fn drag_thumbs(&mut self, at: (f64, f64)) {
        let Some((page, from)) = self.thumb_press else {
            return;
        };
        if self.thumb_drag.is_none() {
            if (at.0 - from.0).hypot(at.1 - from.1) < DRAG_THRESHOLD {
                return;
            }
            if !self.picked.contains(&page) {
                self.picked = std::iter::once(page).collect();
                self.picked_from = Some(page);
            }
            self.column_last = true;
            self.thumb_drag = Some(ThumbDrag {
                pages: self.picked_pages(),
                at,
                before: None,
            });
        }
        let (top, bottom) = self.column_span();
        let inside = at.0 >= 0.0 && at.0 <= self.sidebar_width && at.1 >= top && at.1 <= bottom;
        let before = inside.then(|| {
            let down = at.1 - top + self.thumb_scroll;
            let pages = self.document.pages();
            (0..pages)
                .find(|&index| {
                    self.column
                        .row(index)
                        .is_some_and(|(row, tall)| down < row + tall / 2.0)
                })
                .map_or(pages + 1, |index| index + 1)
        });
        if let Some(drag) = &mut self.thumb_drag {
            drag.at = at;
            drag.before = before;
        }
        // Held near an edge, the column scrolls on a clock of its own, so it
        // goes on scrolling under a pointer that has stopped.
        if self.thumb_roll.is_none() && self.thumb_speed().is_some() {
            self.thumb_rolls += 1;
            self.thumb_roll = Some(self.thumb_rolls);
            crate::emit::after(
                STILL_TICK,
                self.post.clone(),
                crate::emit::News {
                    event: crate::emit::Event::ThumbTick(self.thumb_rolls),
                    target: None,
                },
            );
        }
    }

    /// Where the column is in the window, from top to bottom.
    fn column_span(&self) -> (f64, f64) {
        let top = self.chrome() + crate::sidebar::TABS;
        (top, top + self.thumb_panel())
    }

    /// How far the column scrolls in one step under a drag held at its
    /// edge: faster the further past the edge, and nothing away from it.
    fn thumb_speed(&self) -> Option<f64> {
        let y = self.thumb_drag.as_ref()?.at.1;
        let (top, bottom) = self.column_span();
        let past = if y < top + EDGE {
            y - top - EDGE
        } else if y > bottom - EDGE {
            y - bottom + EDGE
        } else {
            return None;
        };
        Some((past * 0.4).clamp(-24.0, 24.0))
    }

    /// One step of that clock: the column scrolls and the line follows.
    /// Answers whether the clock goes on.
    pub fn roll_thumbs(&mut self, token: u64) -> bool {
        let speed = (self.thumb_roll == Some(token))
            .then(|| self.thumb_speed())
            .flatten();
        let (Some(speed), Some(at)) = (speed, self.thumb_drag.as_ref().map(|drag| drag.at)) else {
            if self.thumb_roll == Some(token) {
                self.thumb_roll = None;
            }
            return false;
        };
        self.scroll_thumbs(speed);
        self.drag_thumbs(at);
        true
    }

    /// The thumbnails let go of: the pages move to where the line is. Let go
    /// outside the column, they stay where they were.
    pub(super) fn drop_thumbs(&mut self) {
        self.thumb_press = None;
        self.thumb_roll = None;
        let Some(drag) = self.thumb_drag.take() else {
            return;
        };
        self.dragged = true;
        if let Some(before) = drag.before {
            self.edit_pages(Edit::Move(drag.pages, before));
        }
    }

    /// Escape during a drag: the pages stay where they were.
    pub fn cancel_thumb_drag(&mut self) -> bool {
        self.thumb_press = None;
        self.thumb_roll = None;
        let cancelled = self.thumb_drag.take().is_some();
        self.dragged |= cancelled;
        cancelled
    }

    /// The pages selected in the column, in order.
    pub fn picked_pages(&self) -> Vec<usize> {
        self.picked.iter().copied().collect()
    }

    /// Every page selected.
    pub fn pick_all_pages(&mut self) {
        self.picked = (1..=self.document.pages()).collect();
    }

    /// Whether keys act on the column's selection: the column was the last
    /// place clicked, and the reader can still see it.
    pub fn column_has_keys(&self) -> bool {
        self.column_last && self.sidebar_open && !self.presenting && self.tab == Tab::Pages
    }

    /// Delete: the column's selection taken out, when the column was the last
    /// place clicked. Anywhere else it does nothing.
    pub fn remove_picked(&mut self) {
        if self.column_has_keys() && !self.picked.is_empty() {
            self.edit_pages(Edit::Remove(self.picked_pages()));
        }
    }

    /// The column's selection turned in the file, when the column was the
    /// last place clicked.
    pub fn turn_picked(&mut self, quarters: i32) {
        if self.column_has_keys() && !self.picked.is_empty() {
            self.edit_pages(Edit::Turn(self.picked_pages(), quarters));
        }
    }

    /// The thumbnail menu's "Move to page…": the field it opens.
    pub fn open_move_field(&mut self) {
        self.moving_to = Some(String::new());
    }

    pub fn type_move(&mut self, typed: &str) {
        self.moving_to = Some(typed.to_string());
    }

    /// Enter in that field: the menu's pages moved to just before the page
    /// typed. A number one past the last page is the end.
    pub fn commit_move(&mut self) {
        let Some(((_, pages, _), typed)) = self.pages_menu.clone().zip(self.moving_to.clone())
        else {
            return;
        };
        let end = self.document.pages() + 1;
        let before = self
            .page_for_label(&typed)
            .or_else(|| (typed.trim().parse::<usize>().ok()).filter(|&page| page == end));
        let Some(before) = before else {
            self.notice = format!("Type a page from 1 to {end}. {end} is the end.");
            return;
        };
        self.close_menu();
        self.edit_pages(Edit::Move(pages, before));
    }

    /// The thumbnail menu's "Number pages from here…": the field it opens,
    /// holding what the page clicked is called now. That is its label even
    /// while positions are shown, since the label is what the field changes.
    pub fn open_number_field(&mut self) {
        let clicked = self.pages_menu.as_ref().map(|(_, _, page)| *page);
        self.numbering = clicked.map(|page| self.own_label(page));
    }

    /// What `page` (one-based) is called by the document, or by the numbers
    /// printed on its pages, and its position where neither says.
    fn own_label(&self, page: usize) -> String {
        (self.labels.get(page - 1))
            .filter(|label| !label.is_empty())
            .cloned()
            .unwrap_or_else(|| page.to_string())
    }

    pub fn type_number(&mut self, typed: &str) {
        self.numbering = Some(typed.to_string());
    }

    /// Enter in that field: the pages numbered from the page clicked on, as
    /// the label typed begins. A label left as it was changes nothing.
    pub fn commit_number(&mut self) {
        let clicked = self.pages_menu.as_ref().map(|(_, _, page)| *page);
        let (Some(page), Some(typed)) = (clicked, self.numbering.clone()) else {
            return;
        };
        let typed = typed.trim().to_string();
        if typed.is_empty() {
            self.notice = "Type what this page is called, like 1, iv or A-1.".into();
            return;
        }
        self.close_menu();
        if typed == self.own_label(page) {
            return;
        }
        let printed = self.labels.first().and_then(|label| label.parse().ok());
        self.edit_pages(Edit::Number(page, typed, printed));
    }

    /// A right-click on a thumbnail: the page edits, at the pointer, for the
    /// selection, which the page clicked joins unless it is already in it.
    pub fn open_pages_menu(&mut self, page: usize, at: (f64, f64)) {
        if self.empty() {
            return;
        }
        self.close_menu();
        self.column_last = true;
        if !self.picked.contains(&page) {
            self.picked = std::iter::once(page).collect();
            self.picked_from = Some(page);
        }
        self.pages_menu = Some((at, self.picked_pages(), page));
        self.menu = Some(Menu::Pages);
    }

    /// A page edit of the open document. The first one in a document asks
    /// where edits go — into a copy, or into the document itself — and every
    /// later one goes there. Every edit can be undone.
    pub fn edit_pages(&mut self, edit: Edit) {
        if self.empty() || self.busy() {
            return;
        }
        if let Edit::Remove(pages) = &edit {
            if pages.len() >= self.document.pages() {
                self.notice = "A document needs at least one page.".into();
                return;
            }
        }
        if edit.moves_nothing(self.document.pages()) {
            return;
        }
        // No certification allows a page edit, and a copy is the same update
        // to the same signed bytes, so nothing is asked first.
        if self.document.certified().is_some() {
            self.notice = format!(
                "{}, so its pages cannot be changed.",
                crate::markup::CERTIFIED_BY
            );
            return;
        }
        // Not into a copy either: the copy is the same rewrite.
        if self.document.encrypted() || self.standing.large {
            self.notice = format!("{}, so its pages cannot be changed.", self.standing.refused);
            return;
        }
        match self.edits {
            Edits::Ask => self.ask_where_edits_go(edit),
            Edits::Copy => self.write_edit(edit, None),
            Edits::InPlace => self.edit_in_place(edit, false),
        }
    }

    /// The first page edit's question: a copy the reader names, which is
    /// what Enter chooses, or the document itself.
    fn ask_where_edits_go(&mut self, edit: Edit) {
        let path = self.document.path().to_string();
        let name = crate::store::file_name(&path);
        let says = format!("Page edits go into a copy, so {name} stays as it is.");
        let suggested = crate::arrange::beside(&path, "edited");
        let in_place = edit.clone();
        self.asking = Some(Asking {
            title: "Where should page edits go?".into(),
            says,
            go: "Edit a copy…",
            of_document: true,
            then: Box::new(move |viewer| {
                viewer.choose.ask(Choosing::Copy(edit), Some(suggested));
            }),
            other: Other::Also(
                "Change this file",
                Box::new(move |viewer| {
                    if !viewer.standing.into_file {
                        viewer.notice = format!(
                            "{}, so page edits can only go into a copy.",
                            viewer.standing.refused
                        );
                        return;
                    }
                    viewer.edits = Edits::InPlace;
                    viewer.edit_in_place(in_place, false);
                }),
            ),
        });
    }

    /// The copy's name has come back from the save dialog: the edit is
    /// written into it, and the window reads on in it.
    pub(super) fn edit_into_copy(&mut self, edit: Edit, to: String) {
        if self.empty() || self.busy() {
            return;
        }
        // The document itself, chosen in the dialog: the edit goes into it.
        if same_file(&to, self.document.path()) {
            if !self.standing.into_file {
                self.notice = format!("{}, so it cannot be changed.", self.standing.refused);
                return;
            }
            self.edits = Edits::InPlace;
            return self.edit_in_place(edit, false);
        }
        self.write_edit(edit, Some(to));
    }

    /// An edit into the document itself, with what is asked first: whether
    /// to break a signature, and whether to take out many pages at once,
    /// since undo lasts only until the window closes.
    fn edit_in_place(&mut self, edit: Edit, asked: bool) {
        if !self.standing.into_file {
            self.notice = format!("{}, so its pages cannot be changed.", self.standing.refused);
            return;
        }
        // Numbering changes no page, and is added after the signed bytes as
        // a highlight is, so it asks nothing.
        let numbering = matches!(edit, Edit::Number(..));
        if self.standing.signed && !self.said_standing && !numbering {
            self.ask_to_break(
                CHANGING_BREAKS_A_SIGNATURE,
                "Change anyway",
                move |viewer| {
                    viewer.said_standing = true;
                    viewer.edit_in_place(edit, asked);
                },
            );
            return;
        }
        if let Edit::Remove(pages) = &edit {
            if pages.len() > crate::arrange::ASK_BEYOND && !asked {
                let (count, name) = (pages.len(), self.store.title().to_string());
                let undo = self.chord_for(Action::Undo);
                self.asking = Some(Asking {
                    title: format!("Remove {count} pages?"),
                    says: format!(
                        "They are taken out of {name} itself. {undo} puts them back until this window is closed."
                    ),
                    other: Other::Keep("Cancel"),
                    go: "Remove pages",
                    of_document: true,
                    then: Box::new(move |viewer| viewer.edit_in_place(edit, true)),
                });
                return;
            }
        }
        self.write_edit(edit, None);
    }

    /// The write: into the document this window shows, or into the copy at
    /// `into` for the first edit.
    fn write_edit(&mut self, edit: Edit, into: Option<String>) {
        let done = self.what_edit_did(&edit);
        let headings = self.headings_on(&edit);
        let labels = !matches!(edit, Edit::Turn(..) | Edit::Number(..)) && self.has_own_numbering();
        let them = if edit.pages().len() == 1 {
            "it"
        } else {
            "them"
        };
        let said = match &into {
            Some(copy) => format!(
                "Saved the edit as {}. From now on, page edits go into this copy.",
                crate::store::file_name(copy)
            ),
            None => done,
        };
        let marks = self.store.marks().to_vec();
        let writing = (edit.clone(), into.clone());
        let wrote = Arc::new(Mutex::new(crate::arrange::Wrote::default()));
        let writes = Arc::clone(&wrote);
        self.write_step_on(
            None,
            self.store.journal().to_vec(),
            Some(marks),
            Paging::Edit(edit, into),
            move |path| {
                let (edit, into) = writing;
                *writes.lock().unwrap() = edit.write(path, into.as_deref().unwrap_or(path))?;
                Ok(())
            },
            move |viewer, written| {
                let wrote = *wrote.lock().unwrap();
                viewer.notice = match written {
                    Ok(()) => {
                        let mut said = said;
                        // The headings taken out are counted as they are
                        // written. Where none could be, the headings the
                        // reader saw on those pages now lead nowhere.
                        said += &match (headings, wrote.forgot) {
                            (0, 0) => String::new(),
                            (_, 1) => format!(
                                " The heading that pointed at {them} is taken out of the contents."
                            ),
                            (1, 0) => {
                                format!(" A heading pointed at {them} and now leads nowhere.")
                            }
                            (n, 0) => {
                                format!(" {n} headings pointed at {them} and now lead nowhere.")
                            }
                            (_, taken) => format!(
                                " The {taken} headings that pointed at {them} are taken out of the contents."
                            ),
                        };
                        if labels && !wrote.labels_kept {
                            said += " Page labels in this document did not move with the pages.";
                        }
                        if wrote.links_lost {
                            said += LINKS_BETWEEN_PAGES;
                        }
                        said
                    }
                    Err(refused) => refused,
                };
            },
        );
    }

    /// What the notice says once `edit` is written: what happened and how to
    /// take it back.
    fn what_edit_did(&self, edit: &Edit) -> String {
        let pages = edit.pages();
        let named = crate::arrange::named(&pages, |page| self.label(page));
        let them = if pages.len() == 1 { "it" } else { "them" };
        let undo = self.chord_for(Action::Undo);
        match edit {
            Edit::Remove(_) => format!("Removed {named}. {undo} puts {them} back."),
            Edit::Turn(_, quarters) => {
                let way = if *quarters < 0 { "left" } else { "right" };
                let is = if pages.len() == 1 { "is" } else { "are" };
                let named = named[..1].to_uppercase() + &named[1..];
                format!("{named} {is} turned {way} in the file.")
            }
            Edit::Move(_, before) => {
                let count = match pages.len() {
                    1 => named,
                    n => format!("{n} pages"),
                };
                if *before > self.document.pages() {
                    format!("Moved {count} to the end.")
                } else {
                    format!("Moved {count} to before page {}.", self.label(*before))
                }
            }
            Edit::Insert(other, before) => {
                let name = crate::store::file_name(other);
                let place = if *before > self.document.pages() {
                    "at the end".to_string()
                } else {
                    format!("before page {}", self.label(*before))
                };
                format!("Put in the pages of {name} {place}. {undo} takes them out again.")
            }
            Edit::Number(page, typed, _) => {
                let mut said = format!("Page {} is now called {typed}", self.label(*page));
                said += match crate::labels::numbers_on(typed) {
                    true => ", and the pages after it are numbered on from there.",
                    false => ".",
                };
                if !self.numbering_printed() {
                    said += " To see it, set Page numbers to \u{201c}As printed\u{201d}.";
                }
                said
            }
        }
    }

    /// How many headings lead to the pages `edit` removes.
    fn headings_on(&self, edit: &Edit) -> usize {
        let Edit::Remove(gone) = edit else {
            return 0;
        };
        (self.headings.iter())
            .filter(|heading| heading.page.is_some_and(|page| gone.contains(&page)))
            .count()
    }

    /// The reader's own records of the pages, moved with them: bookmarks
    /// and the journal. Whatever was on a page that went goes with it.
    pub(super) fn follow_pages(&mut self, moved: &crate::arrange::Moved) {
        let journal = (self.store.journal().iter())
            .filter_map(|held| {
                let page = moved.page(held.page as usize)?;
                Some(crate::library::Highlight {
                    page: page as u32,
                    ..held.clone()
                })
            })
            .collect();
        self.store.set_journal(journal);
        let mut marks: Vec<crate::library::Mark> = (self.store.marks().iter())
            .filter_map(|mark| {
                let page = moved.page(mark.page as usize)?;
                Some(crate::library::Mark {
                    page: page as u32,
                    ..mark.clone()
                })
            })
            .collect();
        marks.sort_by_key(|mark| mark.page);
        self.store.set_marks(marks);
    }
}

/// Said after pages from another file come in: pdfium copies a page
/// without the pages its links point at.
const LINKS_BETWEEN_PAGES: &str = " Links from one of those pages to another do not work here.";

/// Said after pages are saved as a new file, for the same reason.
const LINKS_INTO_THE_DOCUMENT: &str =
    " Links in them to other pages of this document do not work in the new file.";

/// Said when the contents give no chapters to save as files.
const NO_CHAPTERS: &str = "The contents of this document have no chapters to split it by.";

/// What "Save the text as a file…" says about a document with no text.
const NO_TEXT: &str = "This document has no text to save. A scan has none until it has \
                       been through text recognition.";

/// Said after chapters are saved as files, for the same reason.
const LINKS_INTO_OTHER_CHAPTERS: &str =
    " Links in them to other chapters do not work in the new files.";

/// A heading as part of a file name: without the characters some systems
/// do not allow in one, and not too long. A name may have at most 255
/// bytes on most systems, so 80 characters are also held to 200 bytes,
/// which leaves room for the rest of the name.
fn file_safe(title: &str) -> String {
    let kept: String = (title.chars())
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let words: Vec<&str> = kept.split_whitespace().collect();
    let mut name = words.join(" ");
    if let Some((cut, _)) = name.char_indices().nth(80) {
        name.truncate(cut);
    }
    while name.len() > 200 {
        name.pop();
    }
    name.trim_end_matches(['.', ' ']).to_string()
}

/// How far a pressed thumbnail moves before it is a drag rather than a
/// click, in pixels.
const DRAG_THRESHOLD: f64 = 6.0;

/// How near the top or bottom of the column the pointer has to be for a drag
/// to scroll it, in pixels.
const EDGE: f64 = 40.0;

/// Whether two paths name the same file, as far as the disk can say.
fn same_file(one: &str, other: &str) -> bool {
    let real = |path: &str| std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
    real(one) == real(other)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_file_name_is_held_to_its_length_in_bytes() {
        let emoji = "\u{1f989}".repeat(80);
        let name = super::file_safe(&emoji);
        assert!(
            name.len() <= 200 && name.len().is_multiple_of(4),
            "{}",
            name.len()
        );
        assert_eq!(super::file_safe("A/B: c"), "A B c");
    }
}
