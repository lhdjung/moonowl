//! The Viewer and page edits: saving pages as a new file, combining files,
//! and the edits written into the open document. `crate::arrange` writes
//! them; this decides when, where, and what the reader is told.

use super::*;

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

/// Whether two paths name the same file, as far as the disk can say.
fn same_file(one: &str, other: &str) -> bool {
    let real = |path: &str| std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
    real(one) == real(other)
}
