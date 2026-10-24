//! The Viewer and page edits: saving pages as a new file, combining files,
//! and the edits written into the open document. `crate::arrange` writes
//! them; this decides when, where, and what the reader is told.

use super::*;
use crate::arrange::Edit;

impl Viewer {
    /* ------------------------------------------- new files from old ones */

    /// The pages the Document menu's range field offers before anything is
    /// typed into it: the pages selected in the thumbnails, or the page
    /// being read.
    pub fn pages_offered(&self) -> String {
        match &self.pages_typed {
            Some(typed) => typed.clone(),
            None => crate::arrange::typed(&[self.page()]),
        }
    }

    pub fn type_pages(&mut self, typed: &str) {
        self.pages_typed = Some(typed.to_string());
    }

    /// "Save pages as a new file…": the pages typed in the Document menu,
    /// saved where the reader says.
    pub fn save_typed_pages(&mut self) {
        let offered = self.pages_offered();
        match crate::arrange::parse_pages(&offered, self.document.pages()) {
            Ok(pages) => {
                self.close_menu();
                self.save_pages_as(pages);
            }
            Err(said) => self.notice = said,
        }
    }

    /// Ask where `pages` go as a new file. The document is not changed.
    pub fn save_pages_as(&mut self, pages: Vec<usize>) {
        if self.empty() || pages.is_empty() {
            return;
        }
        let how = crate::arrange::named(&pages, |page| page.to_string());
        let suggested = crate::arrange::beside(self.document.path(), &how);
        self.choose.ask(Choosing::SavePages(pages), Some(suggested));
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
        }
    }

    fn write_pages(&mut self, pages: Vec<usize>, to: String) {
        let path = self.document.path().to_string();
        if same_file(&path, &to) {
            self.notice =
                "Choose a new name. The pages cannot be saved over the document they come from."
                    .into();
            return;
        }
        let password = self.document.password().map(str::to_string);
        let named = crate::arrange::named(&pages, |page| self.label(page));
        let taken = self.on_disk(
            {
                let to = to.clone();
                move || crate::arrange::save_pages(&path, password.as_deref(), &pages, &to)
            },
            move |viewer, saved| match saved {
                Ok(()) => {
                    let name = crate::store::file_name(&to);
                    viewer.offer_open(format!("Saved {named} as {name}."), to);
                }
                Err(said) => viewer.notice = said,
            },
        );
        if !taken {
            self.notice = "Still saving the last file. Try again in a moment.".into();
        }
    }

    fn write_combined(&mut self, sources: Vec<String>, to: String) {
        if sources.iter().any(|source| same_file(source, &to)) {
            self.notice =
                "Choose a new name. The combined file cannot be saved over one of the files in it."
                    .into();
            return;
        }
        let count = sources.len();
        let taken = self.on_disk(
            {
                let to = to.clone();
                move || crate::arrange::combine(&sources, &to)
            },
            move |viewer, combined| match combined {
                Ok(pages) => {
                    let name = crate::store::file_name(&to);
                    viewer.notice = format!(
                        "Combined {count} files into {name}, {pages} {}.",
                        if pages == 1 { "page" } else { "pages" }
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

    /// The pages selected in the column, in order.
    pub fn picked_pages(&self) -> Vec<usize> {
        self.picked.iter().copied().collect()
    }

    /// Every page selected.
    pub fn pick_all_pages(&mut self) {
        self.picked = (1..=self.document.pages()).collect();
    }

    /// Delete: the column's selection taken out, when the column was the last
    /// place clicked. Anywhere else it does nothing.
    pub fn remove_picked(&mut self) {
        if self.column_last && !self.picked.is_empty() {
            self.edit_pages(Edit::Remove(self.picked_pages()));
        }
    }

    /// The column's selection turned in the file, when the column was the
    /// last place clicked.
    pub fn turn_picked(&mut self, quarters: i32) {
        if self.column_last && !self.picked.is_empty() {
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
        let Some(((_, pages), typed)) = self.pages_menu.clone().zip(self.moving_to.clone()) else {
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
        self.pages_menu = Some((at, self.picked_pages()));
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
        // Not into a copy either: the copy is the same rewrite.
        let large = std::fs::metadata(self.document.path())
            .is_ok_and(|file| file.len() > crate::markup::IN_FILE_LIMIT);
        if self.document.encrypted() || large {
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
        let mut says = format!("Page edits go into a copy, so {name} stays as it is.");
        if self.standing.signed {
            says += " The copy will not carry the signature.";
        }
        let suggested = crate::arrange::beside(&path, "edited");
        let in_place = edit.clone();
        self.asking = Some(Asking {
            title: "Where should page edits go?".into(),
            says,
            keep: "Cancel",
            go: "Edit a copy…",
            of_document: true,
            then: Box::new(move |viewer| {
                viewer.choose.ask(Choosing::Copy(edit), Some(suggested));
            }),
            also: Some((
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
            )),
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
        if self.standing.signed && !self.said_standing {
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
                    keep: "Cancel",
                    go: "Remove pages",
                    of_document: true,
                    then: Box::new(move |viewer| viewer.edit_in_place(edit, true)),
                    also: None,
                });
                return;
            }
        }
        self.write_edit(edit, None);
    }

    /// The write: into the document this window shows, or into the copy at
    /// `into` for the first edit.
    fn write_edit(&mut self, edit: Edit, into: Option<String>) {
        let (done, also) = self.what_edit_did(&edit);
        let said = match &into {
            Some(copy) => format!(
                "Saved the edit as {}. From now on, page edits go into this copy.{also}",
                crate::store::file_name(copy)
            ),
            None => format!("{done}{also}"),
        };
        let marks = self.store.marks().to_vec();
        let writing = (edit.clone(), into.clone());
        self.write_step_on(
            None,
            self.store.journal().to_vec(),
            Some(marks),
            Paging::Edit(edit, into),
            move |path| {
                let (edit, into) = writing;
                edit.write(path, into.as_deref().unwrap_or(path))
            },
            move |viewer, written| {
                viewer.notice = match written {
                    Ok(()) => said,
                    Err(refused) => refused,
                };
            },
        );
    }

    /// What the notice says once `edit` is written: what happened and how to
    /// take it back, and then, each with a space before it, what did not
    /// move with the pages.
    fn what_edit_did(&self, edit: &Edit) -> (String, String) {
        let pages = edit.pages();
        let named = crate::arrange::named(&pages, |page| self.label(page));
        let them = if pages.len() == 1 { "it" } else { "them" };
        let undo = self.chord_for(Action::Undo);
        let said = match edit {
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
            Edit::Insert(other, _) => {
                format!("Put in the pages of {}.", crate::store::file_name(other))
            }
        };
        let mut also = String::new();
        if let Edit::Remove(gone) = edit {
            let lost = (self.headings.iter())
                .filter(|heading| heading.page.is_some_and(|page| gone.contains(&page)))
                .count();
            match lost {
                0 => {}
                1 => also += &format!(" A heading pointed at {them} and now leads nowhere."),
                n => also += &format!(" {n} headings pointed at {them} and now lead nowhere."),
            }
        }
        if !matches!(edit, Edit::Turn(..)) && self.has_own_numbering() {
            also += " Page labels in this document did not move with the pages.";
        }
        (said, also)
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

/// Whether two paths name the same file, as far as the disk can say.
fn same_file(one: &str, other: &str) -> bool {
    let real = |path: &str| std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
    real(one) == real(other)
}
