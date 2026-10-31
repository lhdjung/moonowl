//! The Viewer and the edits written into the open document, and the
//! document saved in other forms: a page as a picture, its text.
//! `crate::arrange` writes the edits; this decides when, and what the
//! reader is told. The organiser's own saving is `organise.rs`.

use super::*;
use crate::arrange::Edit;

impl Viewer {
    /* ------------------------------------------- new files from old ones */

    /// A file dialog has answered: one the organiser or an export opened.
    pub fn chosen(&mut self, choosing: Choosing, paths: Vec<String>) {
        let Some(first) = paths.first().cloned() else {
            return;
        };
        match choosing {
            Choosing::ExportHighlights => self.write_highlights(first),
            Choosing::SavePicture(page) => self.write_picture(page, first),
            Choosing::SaveText => self.write_text(first),
            Choosing::SaveFlattened => self.write_flattened(first),
            Choosing::SaveAttachment(index) => self.write_attachment(index, first),
            Choosing::AddFiles => self.take_files(paths),
            Choosing::SaveArranged => self.write_arranged(first),
            Choosing::SaveParts => self.write_parts(first),
            Choosing::Compare => self.compare(first),
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
            self.notice = Self::BUSY.into();
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
            self.notice = Self::BUSY.into();
        }
    }

    /// The Document menu's "Flattened": where to save a copy with its
    /// annotations drawn into the pages.
    pub fn save_flattened(&mut self) {
        self.close_menu();
        if self.document.encrypted() {
            self.notice = crate::export::ENCRYPTED_FLAT.into();
            return;
        }
        let suggested = crate::arrange::beside(self.document.path(), "flattened");
        self.choose.ask(Choosing::SaveFlattened, Some(suggested));
    }

    /// The flattened copy written to `to`, on a thread.
    fn write_flattened(&mut self, to: String) {
        let document = self.document.clone();
        let taken = self.on_thread(
            {
                let to = to.clone();
                move || {
                    let bytes = crate::export::flattened(&*document)?;
                    crate::atomic_write(std::path::Path::new(&to), &bytes)
                }
            },
            move |viewer, saved| match saved {
                Ok(()) => {
                    let name = crate::store::file_name(&to);
                    viewer.offer_open(format!("Saved a flattened copy as {name}."), to.clone());
                }
                Err(said) => viewer.notice = said,
            },
        );
        if !taken {
            self.notice = Self::BUSY.into();
        }
    }

    /// A file the document carries, from the sidebar: where to save it. The
    /// dialog suggests its own name, beside the document.
    pub fn save_attachment(&mut self, index: usize) {
        let Some(name) = self.document.attachments().get(index) else {
            return;
        };
        // Only the name: a path inside it is the document's, not the disk's.
        let name = std::path::Path::new(name)
            .file_name()
            .map_or_else(|| "attachment".into(), |name| name.to_string_lossy());
        let folder = std::path::Path::new(self.document.path())
            .parent()
            .unwrap_or(std::path::Path::new(""));
        let suggested = folder.join(&*name).to_string_lossy().into_owned();
        self.choose
            .ask(Choosing::SaveAttachment(index), Some(suggested));
    }

    /// The file at `index` written to `to`, on a thread.
    fn write_attachment(&mut self, index: usize, to: String) {
        let document = self.document.clone();
        let taken = self.on_thread(
            {
                let to = to.clone();
                move || {
                    let bytes = document.attachment(index)?;
                    crate::atomic_write(std::path::Path::new(&to), &bytes)
                }
            },
            move |viewer, saved| match saved {
                Ok(()) => {
                    let name = crate::store::file_name(&to);
                    let said = format!("Saved {name}.");
                    if to.to_lowercase().ends_with(".pdf") {
                        viewer.offer_open(said, to.clone());
                    } else {
                        viewer.notice = said;
                    }
                }
                Err(said) => viewer.notice = said,
            },
        );
        if !taken {
            self.notice = Self::BUSY.into();
        }
    }

    /// Say `notice`, with a button beside it that opens `path` in a window
    /// of its own.
    pub(super) fn offer_open(&mut self, notice: String, path: String) {
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

    /// The thumbnail menu's "Number pages from here…": the field it opens,
    /// holding what the page clicked is called now. That is its label even
    /// while positions are shown, since the label is what the field changes.
    pub fn open_number_field(&mut self) {
        let clicked = self.pages_menu.as_ref().map(|(_, page)| *page);
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
        let clicked = self.pages_menu.as_ref().map(|(_, page)| *page);
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
        let edit = Edit::Number(page, typed, printed);
        if self.may_edit(&edit) {
            self.edit_in_place(edit, false);
        }
    }

    /// A right-click on a thumbnail: its menu, at the pointer.
    pub fn open_pages_menu(&mut self, page: usize, at: (f64, f64)) {
        if self.empty() {
            return;
        }
        self.close_menu();
        self.pages_menu = Some((at, page));
        self.menu = Some(Menu::Pages);
    }

    /// Whether `edit` can be written at all, said when it cannot.
    pub(super) fn may_edit(&mut self, edit: &Edit) -> bool {
        if self.empty() || self.busy() {
            return false;
        }
        if edit.moves_nothing(self.document.pages()) {
            return false;
        }
        // No certification allows a page edit, so nothing is asked first.
        if self.document.certified().is_some() {
            self.notice = format!(
                "{}, so its pages cannot be changed.",
                crate::markup::CERTIFIED_BY
            );
            return false;
        }
        if self.document.encrypted() || self.standing.large {
            self.notice = format!("{}, so its pages cannot be changed.", self.standing.refused);
            return false;
        }
        true
    }

    /// An edit into the document itself, with what is asked first: whether
    /// to break a signature, and whether to take out many pages at once,
    /// since undo lasts only until the window closes.
    pub(super) fn edit_in_place(&mut self, edit: Edit, asked: bool) {
        if !self.standing.into_file {
            self.notice = format!("{}, so its pages cannot be changed.", self.standing.refused);
            return;
        }
        // Numbering changes no page, and is added after the signed bytes as
        // a highlight is, so it asks nothing.
        let numbering = matches!(edit, Edit::Number(..));
        let breaks = self.standing.signed && !self.said_standing && !numbering;
        let removed = match &edit {
            Edit::Arrange(sheets) => {
                // A page of the document kept twice is one page kept.
                let kept: std::collections::BTreeSet<usize> = (sheets.iter())
                    .filter(|sheet| sheet.from.is_none())
                    .map(|sheet| sheet.page)
                    .collect();
                self.document.pages().saturating_sub(kept.len())
            }
            Edit::Number(..) => 0,
        };
        let many = removed > crate::arrange::ASK_BEYOND && !asked;
        // Both things to ask are asked in one question.
        if breaks || many {
            let mut says = String::new();
            if breaks {
                says += CHANGING_BREAKS_A_SIGNATURE;
            }
            if many {
                let (name, undo) = (self.store.title(), self.chord_for(Action::Undo));
                if breaks {
                    says += " ";
                }
                says += &format!(
                    "The pages are taken out of {name} itself. {undo} puts them back until this window is closed."
                );
            }
            let (title, go) = match (breaks, many) {
                (true, true) => (
                    format!("Remove {removed} pages from a signed document?"),
                    "Remove pages",
                ),
                (true, false) => (
                    "Change the pages of a signed document?".to_string(),
                    "Change anyway",
                ),
                _ => (format!("Remove {removed} pages?"), "Remove pages"),
            };
            self.asking = Some(Asking {
                title,
                says,
                other: Other::Keep("Cancel"),
                go,
                of_document: true,
                then: Box::new(move |viewer| {
                    viewer.said_standing |= breaks;
                    viewer.edit_in_place(edit, true);
                }),
            });
            return;
        }
        self.write_edit(edit);
    }

    /// The write into the document this window shows.
    fn write_edit(&mut self, edit: Edit) {
        let said = self.what_edit_did(&edit);
        let headings = self.headings_on(&edit);
        let labels = matches!(edit, Edit::Arrange(_)) && self.has_own_numbering();
        let them = "removed pages";
        let marks = self.store.marks().to_vec();
        if let Some(organiser) = self.organiser.as_mut() {
            organiser.busy = true;
        }
        let writing = edit.clone();
        let wrote = Arc::new(Mutex::new(crate::arrange::Wrote::default()));
        let writes = Arc::clone(&wrote);
        // Numbering moves no page, so the reopen reads marks off only the
        // pages that have one, as a highlight's does. An arrangement moves
        // pages and brings new ones, so every page is read.
        let touched = match &edit {
            Edit::Number(..) => Some(self.marked_pages(std::iter::empty())),
            Edit::Arrange(_) => None,
        };
        self.write_step_on(
            touched,
            self.store.journal().to_vec(),
            Some(marks),
            Paging::Edit(edit),
            move |path| {
                *writes.lock().unwrap() = writing.write(path)?;
                Ok(())
            },
            move |viewer, written| {
                let wrote = *wrote.lock().unwrap();
                // The organiser that saved this goes once it is written.
                match &written {
                    Ok(()) => viewer.organiser = None,
                    Err(_) => {
                        if let Some(organiser) = viewer.organiser.as_mut() {
                            organiser.busy = false;
                        }
                    }
                }
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
                        if wrote.links_left_behind {
                            said += LINKS_TO_PAGES_LEFT_OUT;
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
        let undo = self.chord_for(Action::Undo);
        match edit {
            Edit::Arrange(sheets) => {
                let count = self.document.pages();
                let kept = sheets.iter().filter(|sheet| sheet.from.is_none()).count();
                let turned = (sheets.iter()).filter(|sheet| sheet.turn % 4 != 0).count();
                let added = sheets.len() - kept;
                let pages = |n: usize| {
                    if n == 1 {
                        "1 page".to_string()
                    } else {
                        format!("{n} pages")
                    }
                };
                let mut parts = Vec::new();
                if kept < count {
                    parts.push(format!("removed {}", pages(count - kept)));
                }
                if turned > 0 {
                    parts.push(format!("turned {}", pages(turned)));
                }
                if added > 0 {
                    parts.push(format!("added {} from other files", pages(added)));
                }
                let did = match parts.as_slice() {
                    [] => "put its pages in a new order".to_string(),
                    [one] => one.clone(),
                    [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
                };
                format!(
                    "Arranged {}: {did}. {undo} puts it back.",
                    crate::store::file_name(self.document.path())
                )
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
        let Edit::Arrange(sheets) = edit else {
            return 0;
        };
        let gone: Vec<usize> = (1..=self.document.pages())
            .filter(|&page| {
                !(sheets.iter()).any(|sheet| sheet.from.is_none() && sheet.page == page)
            })
            .collect();
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

/// Said when pages put in from another file link to pages of it that were
/// left out.
const LINKS_TO_PAGES_LEFT_OUT: &str =
    " Links on the pages put in that lead to pages left out do not work.";

/// Said when the contents give no chapters to save as files.
pub(super) const NO_CHAPTERS: &str =
    "The contents of this document have no chapters to split it by.";

/// What "Save the text as a file…" says about a document with no text.
const NO_TEXT: &str = "This document has no text to save. A scan has none until it has \
                       been through text recognition.";

/// A heading as part of a file name: without the characters some systems
/// do not allow in one, and not too long. A name may have at most 255
/// bytes on most systems, so 80 characters are also held to 200 bytes,
/// which leaves room for the rest of the name.
pub(super) fn file_safe(title: &str) -> String {
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

/// Whether two paths name the same file, as far as the disk can say.
pub(super) fn same_file(one: &str, other: &str) -> bool {
    let real = |path: &str| std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
    real(one) == real(other)
}

impl Viewer {
    /// "Recognise text": the words on every page with no text of its own,
    /// found on a thread and then written into the document as one step,
    /// so ⌘Z takes them out again. See `crate::ocr`.
    ///
    /// ponytail: one notice for the whole document; say how far it has got
    /// if long scans make it look stuck.
    pub fn recognise_text(&mut self) {
        self.close_menu();
        if self.markup_reading.is_some() {
            self.notice = "Still reading this document. Try again in a moment.".into();
            return;
        }
        if !self.standing.into_file {
            self.notice = format!(
                "{}, so recognised text cannot be written into it.",
                self.standing.refused
            );
            return;
        }
        if self.standing.signed {
            self.notice = "This document is signed, and writing text into its pages \
                           would break the signature."
                .into();
            return;
        }
        let document = self.document.clone();
        let taken = self.on_thread_reading(
            move || crate::ocr::recognise(&*document),
            move |viewer, found| match found {
                // A reload under way, which says so.
                Ok(_) if viewer.busy() => {}
                Err(said) => viewer.notice = said,
                Ok(found) if found.is_empty() => {
                    viewer.notice =
                        "Every page has text already, so there is nothing to recognise.".into()
                }
                Ok(found) if found.iter().all(|(_, words)| words.is_empty()) => {
                    viewer.notice = "No words were found on the pages without text.".into()
                }
                Ok(found) => {
                    let pages = found.iter().filter(|(_, words)| !words.is_empty()).count();
                    let done = match pages {
                        1 => "Recognised the text of 1 page.".to_string(),
                        n => format!("Recognised the text of {n} pages."),
                    };
                    viewer.write_step(
                        viewer.store.journal().to_vec(),
                        move |path| crate::ocr::write(path, &found),
                        move |viewer, written| {
                            viewer.notice = match written {
                                Ok(()) => done,
                                Err(refused) => refused,
                            }
                        },
                    );
                }
            },
        );
        self.notice = match taken {
            true => "Recognising the text of the pages that have none…".into(),
            false => Self::STILL_BUSY.into(),
        };
    }
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
