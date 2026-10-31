//! The Viewer and SyncTeX: from a place on the page to the line of the
//! source that made it, and back. `crate::synctex` reads the file; this
//! decides what the reader sees.

use super::*;

impl Viewer {
    /// A click with ⇧⌘ (Ctrl and Shift elsewhere) at `on`, in CSS pixels
    /// from the top left of page `page`'s box: the editor opens the source
    /// file at the line that made this place.
    ///
    /// With no SyncTeX file beside the document, the reader is told that
    /// first, since choosing an editor would not help; with no editor
    /// chosen, they are sent to Settings.
    pub fn source_at(&mut self, page: usize, on: (f64, f64)) {
        let template = self.store.text("synctex_editor");
        let (x, y) = self.layout.unplace_on(page - 1, on.0, on.1);
        let pdf = self.document.path().to_string();
        let taken = self.on_thread_without_document(
            move || crate::synctex::line_at(&pdf, page, x, y),
            move |viewer, found| {
                let opened = found.and_then(|(file, line)| {
                    if template.trim().is_empty() {
                        return Err(
                            "Choose the editor SyncTeX opens in Settings, under Reading.".into(),
                        );
                    }
                    viewer
                        .launch
                        .clone()
                        .open(&crate::synctex::command(&template, &file, line))
                });
                if let Err(said) = opened {
                    viewer.notice = said;
                }
            },
        );
        if !taken {
            self.notice = Self::STILL_BUSY.into();
        }
    }

    /// Show the line an editor asked for, if the request is for the
    /// document in this window: its page, with the line of text there
    /// selected so that it can be found on it. `asked` is true when the
    /// request was sent to this window because it shows the document.
    pub fn take_forward(&mut self, asked: bool) {
        if !crate::synctex::waiting() {
            return;
        }
        let pdf = self.document.path().to_string();
        // Whether the request is for this document is asked on the thread,
        // since it compares the two paths as the disk resolves them.
        let taken = self.on_thread_without_document(
            move || {
                let crate::synctex::Forward { tex, line, .. } = crate::synctex::take_for(&pdf)?;
                let name = crate::store::file_name(&tex);
                Some((name, crate::synctex::place_of(&pdf, &tex, line)))
            },
            move |viewer, found| match found {
                None => {}
                Some((_, Ok(Some((page, x, y))))) => viewer.show_source_line(page, x, y),
                Some((name, Ok(None))) => {
                    viewer.notice = format!("{name} made nothing in this document.")
                }
                Some((_, Err(said))) => viewer.notice = said,
            },
        );
        // A window that has only opened a document may be busy while a
        // request waits for another window's document, which is not its
        // to let go of. The paths are compared as they are written, since
        // asking the disk belongs off this thread.
        let path = self.document.path().to_string();
        if !taken && crate::synctex::forget((!asked).then_some(path.as_str())) {
            self.notice = Self::STILL_BUSY.into();
        }
    }

    /// Go to the place a line of the source made, at `x`, `y` on page
    /// `page` in points from its top left, and select the line of text
    /// there.
    fn show_source_line(&mut self, page: usize, x: f64, y: f64) {
        if page == 0 || page > self.pages() {
            return;
        }
        let at = Rect {
            left: x,
            top: y,
            width: 0.0,
            height: 0.0,
        };
        self.reveal_line(page, at, false);
        let text = self.text_on(page);
        if text.is_empty() {
            return;
        }
        // Just above the baseline, which is where the letters are.
        let caret = crate::select::caret_at(&text, x + 1.0, y - 2.0);
        let (from, to) = crate::select::line_around(&text, caret);
        if from < to {
            self.selection = Some(Selection {
                anchor: Spot { page, index: from },
                head: Spot { page, index: to },
            });
        }
    }

    pub fn set_synctex_editor(&mut self, command: String) {
        self.store
            .set_soon(vec![("synctex_editor".into(), json!(command))]);
    }
}
