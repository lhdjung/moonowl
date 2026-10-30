//! The Viewer and SyncTeX: from a place on the page to the line of the
//! source that made it, and back. `crate::synctex` reads the file; this
//! decides what the reader sees.

use super::*;

impl Viewer {
    /// A click with ⇧⌘ (Ctrl and Shift elsewhere) at `on`, in CSS pixels
    /// from the top left of page `page`'s box: the editor opens the source
    /// file at the line that made this place.
    pub fn source_at(&mut self, page: usize, on: (f64, f64)) {
        let template = self.store.text("synctex_editor");
        if template.trim().is_empty() {
            self.notice = "Choose the editor SyncTeX opens in Settings, under Reading.".into();
            return;
        }
        let (x, y) = self.layout.unplace_on(page - 1, on.0, on.1);
        let pdf = self.document.path().to_string();
        let taken = self.on_thread(
            move || crate::synctex::line_at(&pdf, page, x, y),
            move |viewer, found| {
                let opened = found.and_then(|(file, line)| {
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
    /// selected so that it can be found on it.
    pub fn take_forward(&mut self) {
        let Some(forward) = crate::synctex::take_for(self.document.path()) else {
            return;
        };
        let pdf = self.document.path().to_string();
        let crate::synctex::Forward { tex, line, .. } = forward;
        let name = crate::store::file_name(&tex);
        let taken = self.on_thread(
            move || crate::synctex::place_of(&pdf, &tex, line),
            move |viewer, found| match found {
                Ok(Some((page, x, y))) => viewer.show_source_line(page, x, y),
                Ok(None) => viewer.notice = format!("{name} made nothing in this document."),
                Err(said) => viewer.notice = said,
            },
        );
        if !taken {
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
