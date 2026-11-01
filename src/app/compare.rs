//! The Viewer and "Compare with…": another version of the document read
//! and compared on a thread, the changes painted on the pages, and the
//! Differences tab that lists them. `crate::compare` does the comparing.

use super::*;

/// The other file, and where the document's words differ from its words.
/// Neither is taken to be the earlier one: the other file may be a later
/// version, or not a version of the document at all.
pub struct Comparing {
    /// The other file's name, as the Differences tab says it.
    pub name: String,
    pub changes: Vec<crate::compare::Change>,
    /// The change the reader went to last, from the list.
    pub at: Option<usize>,
    /// What the Differences tab lists, made once, since the sidebar is worked
    /// out again on every scroll frame.
    pub rows: Rc<Vec<ChangeRow>>,
    /// Each page's changes, as (change, from, to), so that drawing a page
    /// looks only at its own.
    on_page: HashMap<usize, Vec<(usize, usize, usize)>>,
}

/// A row of the Differences tab: the page (one-based), what the page is
/// called, and the words removed and added, cut short.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeRow {
    pub page: usize,
    pub label: String,
    pub removed: String,
    pub added: String,
}

/// How many changes the Differences tab lists. The pages show every one.
const CHANGE_ROWS: usize = 500;

impl Viewer {
    /// "Compare with…" in the Document menu: asks for the other version.
    pub fn compare_with(&mut self) {
        self.close_menu();
        if !self.empty() {
            self.choose.ask(Choosing::Compare, None);
        }
    }

    /// Compare the document with the file at `path`, on a thread, and show
    /// the changes in the Differences tab.
    pub(super) fn compare(&mut self, path: String) {
        let path = crate::config::absolute(&path);
        if path == self.document.path() {
            self.notice = "That is this document. Choose another version of it.".into();
            return;
        }
        let document = self.document.clone();
        let name = crate::store::file_name(&path);
        let taken = self.on_thread_reading(
            move || {
                let other = crate::render::open(&path).map_err(|refused| refused.to_string())?;
                let changes = crate::compare::compare(&*document, &*other);
                other.release();
                changes
            },
            move |viewer, changes| match changes {
                Ok(changes) => viewer.show_changes(name, changes),
                Err(said) => viewer.notice = said,
            },
        );
        if !taken {
            self.notice = self.why_refused().into();
        }
    }

    fn show_changes(&mut self, name: String, changes: Vec<crate::compare::Change>) {
        self.notice = match changes.len() {
            0 => format!("The text is the same as in {name}."),
            1 => format!("1 difference from {name}."),
            n => format!("{n} differences from {name}."),
        };
        if changes.is_empty() {
            self.stop_comparing();
            return;
        }
        let rows = (changes.iter().take(CHANGE_ROWS))
            .map(|change| ChangeRow {
                page: change.page(),
                label: self.label(change.page()),
                removed: change.removed.clone(),
                added: change.added.clone(),
            })
            .collect();
        let mut on_page: HashMap<usize, Vec<(usize, usize, usize)>> = HashMap::new();
        for (number, change) in changes.iter().enumerate() {
            for &(page, from, to) in &change.places {
                on_page.entry(page).or_default().push((number, from, to));
            }
        }
        self.comparing = Some(Comparing {
            name,
            changes,
            at: None,
            rows: Rc::new(rows),
            on_page,
        });
        self.set_sidebar(true, false);
        self.tab = Tab::Changes;
    }

    /// Put the comparison away: the bands go, and so does the tab.
    pub fn stop_comparing(&mut self) {
        self.comparing = None;
        if self.tab == Tab::Changes {
            self.tab = if self.headings.is_empty() {
                Tab::Pages
            } else {
                Tab::Contents
            };
        }
    }

    /// What changed on page `page`, in CSS pixels from the top left of its
    /// box, each with its class: the words added, and a bar where words were
    /// removed. The change the reader went to last is `current` as well.
    pub fn change_areas(&self, page: usize) -> Vec<(Rect, &'static str)> {
        let Some(comparing) = &self.comparing else {
            return Vec::new();
        };
        if self.layout.box_of(page - 1).is_none() {
            return Vec::new();
        }
        let Some(here) = comparing.on_page.get(&page) else {
            return Vec::new();
        };
        let text = self.text_on(page);
        let mut out = Vec::new();
        for &(number, from, to) in here {
            let current = comparing.at == Some(number);
            if from < to {
                let class = if current { "added current" } else { "added" };
                for quad in text.quads(from, to) {
                    out.push((self.layout.place_on(page - 1, quad), class));
                }
            } else if let Some(bar) = removed_bar(&text, from) {
                let class = if current {
                    "removed current"
                } else {
                    "removed"
                };
                out.push((self.layout.place_on(page - 1, bar), class));
            }
        }
        out
    }

    /// Go to change number `number`, from the Differences tab.
    pub fn go_to_change(&mut self, number: usize) {
        let Some(comparing) = &mut self.comparing else {
            return;
        };
        let Some(change) = comparing.changes.get(number) else {
            return;
        };
        let first = comparing.at.is_none();
        comparing.at = Some(number);
        let &(page, from, to) = &change.places[0];
        let text = self.text_on(page);
        let line = text
            .quads(from, to)
            .first()
            .copied()
            .or_else(|| removed_bar(&text, from));
        let from_here = self.layout.anchor(self.scroll_top);
        let placed = |viewer: &Self| Some(viewer.layout.place_on(page - 1, line?));
        self.show_on(page, placed, 0.8, REVEAL);
        // The first change gone to is a jump, so that ⌘[ goes back to where
        // the reader was before they began going through them.
        if first {
            self.note_jump(from_here);
        }
    }
}

/// A thin bar where words were removed: at the left edge of the character
/// at `at`, or at the right edge of the one before it at the end of a line.
fn removed_bar(text: &crate::render::PageText, at: usize) -> Option<Rect> {
    let drawn = |i: usize| Some(text.glyph(i)).filter(|g| g.width > 0.0 && g.height > 0.0);
    let (left, glyph) = match (at < text.chars.len()).then(|| drawn(at)).flatten() {
        Some(glyph) => (glyph.left, glyph),
        None => {
            let glyph = (0..at.min(text.chars.len())).rev().find_map(drawn)?;
            (glyph.left + glyph.width, glyph)
        }
    };
    Some(Rect {
        left: left - 1.0,
        top: glyph.top,
        width: 2.0,
        height: glyph.height,
    })
}
