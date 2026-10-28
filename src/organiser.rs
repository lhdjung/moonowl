//! The page organiser: pages from one file or several, laid out in a grid,
//! arranged, and written out as one file or several. `plans/page-edits.md`
//! is the plan it follows.
//!
//! This module holds the draft and the grid. `app/organise.rs` is the
//! Viewer's half: opening it, the keys, the drag, and saving.

use std::collections::BTreeSet;
use std::sync::Arc;

use dioxus::html::geometry::WheelDelta;
use dioxus::prelude::*;
use dioxus_native::CustomWidgetAttr;

use crate::app::{Icon, Viewer};
use crate::layout::{Size, View};
use crate::page::{Chosen, PageWidget};
use crate::render::PageSource;

/// How tall the organiser's bar is.
pub const BAR: f64 = 52.0;
/// The space around the grid.
const PAD: f64 = 24.0;
/// The space between two pages in a row.
const GAP: f64 = 20.0;
/// The line under each page that says what it is.
const LABEL: f64 = 26.0;
/// The space between two rows.
const ROW_GAP: f64 = 10.0;
/// The heading over each file that will be written.
const HEAD: f64 = 44.0;
/// How much of a screen of rows is kept drawn above and below the screen.
const OVERSCAN: f64 = 0.4;
/// The smallest and largest pages, and the step between them.
pub const SMALLEST: f64 = 80.0;
pub const LARGEST: f64 = 480.0;
const STEP: f64 = 40.0;
/// How many steps of the draft are kept for undo.
const STEPS: usize = 100;

/// A file the organiser takes pages from.
pub struct Source {
    /// The file that was added.
    pub file: String,
    /// Where its pages are read from: the file itself, or for a picture, a
    /// PDF of one page made from it.
    pub path: String,
    /// What it is called: the file's own name.
    pub name: String,
    /// Whether it is the document the window has open, whose pages are
    /// written into it by "Save into".
    pub own: bool,
    pub password: Option<String>,
    pub document: Arc<dyn PageSource>,
    /// The widget's view of it: its pages are drawn by the widget the
    /// thumbnails use.
    pub chosen: Chosen,
    labels: Vec<String>,
    /// The PDF made from a picture, which goes when the organiser does.
    made: bool,
}

impl Source {
    pub fn new(
        file: String,
        path: String,
        own: bool,
        password: Option<String>,
        document: Arc<dyn PageSource>,
        chosen: Chosen,
        made: bool,
    ) -> Source {
        chosen.show(document.clone());
        let labels = document.labels();
        Source {
            name: crate::store::file_name(&file),
            file,
            path,
            own,
            password,
            document,
            chosen,
            labels,
            made,
        }
    }

    /// What page `index` (zero-based) is called in its file.
    pub fn label(&self, index: usize) -> String {
        (self.labels.get(index))
            .filter(|label| !label.is_empty())
            .cloned()
            .unwrap_or_else(|| (index + 1).to_string())
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        if self.made {
            self.document.release();
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// A page of the draft.
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    /// Which source, by index.
    pub source: usize,
    /// Which page of it, zero-based.
    pub page: usize,
    /// How many clockwise quarter turns are added to it, from 0 to 3.
    pub turn: i32,
    /// Whether a new file begins with this page, and what that file is
    /// called. An empty name is a file numbered after the first. The first
    /// page always begins a file; its name is used and nothing else.
    pub begins: Option<String>,
}

/// Where a page is drawn in the grid, from the top of the grid.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Place {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A heading over the pages of one file that will be written.
#[derive(Clone, Debug, PartialEq)]
pub struct Head {
    /// The first page of the file, by index into the sheets.
    pub first: usize,
    pub pages: usize,
    pub top: f64,
}

/// A row of pages: its top, its bottom, and its pages.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    top: f64,
    bottom: f64,
    pages: std::ops::Range<usize>,
}

/// Where every page sits, laid out once per change and searched while
/// scrolling, as the thumbnail column is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grid {
    pub places: Vec<Place>,
    pub heads: Vec<Head>,
    rows: Vec<Row>,
    pub total: f64,
    /// The size of the square each page is fitted into.
    pub size: f64,
}

impl Grid {
    /// Lay `shapes` (each page's width and height as shown) out for a grid
    /// this wide, with pages fitted into squares of `size`. A file begins
    /// at every page `begins` says.
    pub fn new(shapes: &[Size], begins: &[bool], width: f64, size: f64) -> Grid {
        let across = (((width - PAD * 2.0 + GAP) / (size + GAP)).floor() as usize).max(1);
        let mut grid = Grid {
            size,
            ..Grid::default()
        };
        let mut y = PAD;
        let mut index = 0;
        while index < shapes.len() {
            let end = (index + 1..shapes.len())
                .find(|&at| begins[at])
                .unwrap_or(shapes.len());
            grid.heads.push(Head {
                first: index,
                pages: end - index,
                top: y,
            });
            y += HEAD;
            for row in (index..end).step_by(across) {
                let pages = row..(row + across).min(end);
                for (column, at) in pages.clone().enumerate() {
                    let shape = shapes[at];
                    let ratio = if shape.width > 0.0 {
                        shape.height / shape.width
                    } else {
                        1.414
                    };
                    let (w, h) = if ratio >= 1.0 {
                        (size / ratio, size)
                    } else {
                        (size, size * ratio)
                    };
                    grid.places.push(Place {
                        x: PAD + column as f64 * (size + GAP) + (size - w) / 2.0,
                        y: y + size - h,
                        width: w.max(1.0),
                        height: h.max(1.0),
                    });
                }
                grid.rows.push(Row {
                    top: y,
                    bottom: y + size + LABEL,
                    pages,
                });
                y += size + LABEL + ROW_GAP;
            }
            index = end;
        }
        grid.total = y + PAD;
        grid
    }

    /// The pages drawn for a grid scrolled to `scroll` and `height` tall:
    /// those in view and some way either side, by index, in order.
    pub fn mounted(&self, scroll: f64, height: f64) -> Vec<usize> {
        let margin = height * OVERSCAN;
        let (top, bottom) = (scroll - margin, scroll + height + margin);
        let first = self.rows.partition_point(|row| row.bottom < top);
        (self.rows[first..].iter())
            .take_while(|row| row.top <= bottom)
            .flat_map(|row| row.pages.clone())
            .collect()
    }

    /// Where pages let go of at `(x, y)` in the grid would land: before the
    /// page at the index answered, which is one past the last page for the
    /// end. `true` beside it says they would begin the file whose first page
    /// that is, rather than end the file before it.
    pub fn landing(&self, x: f64, y: f64) -> (usize, bool) {
        let count = self.places.len();
        // A heading, or the space above a file's first row: the start of
        // that file.
        if let Some(head) = (self.heads.iter()).rfind(|head| y >= head.top) {
            let first_row = self.rows.iter().find(|row| row.pages.start == head.first);
            if first_row.is_some_and(|row| y < row.top) {
                return (head.first, head.first > 0);
            }
        }
        let Some(row) = (self.rows.iter())
            .rfind(|row| y >= row.top - ROW_GAP)
            .or(self.rows.first())
        else {
            return (count, false);
        };
        let at = (row.pages.clone())
            .find(|&at| x < self.places[at].x + self.places[at].width / 2.0)
            .unwrap_or(row.pages.end);
        let starts = (self.heads.iter()).any(|head| head.first == at && at == row.pages.start);
        (at, starts && at > 0)
    }

    /// Where the line that shows a landing is drawn: its left, top and
    /// height, in the grid.
    pub fn line(&self, before: usize) -> Option<(f64, f64, f64)> {
        let row = (self.rows.iter())
            .find(|row| row.pages.contains(&before))
            .or_else(|| self.rows.iter().rfind(|row| row.pages.end == before))?;
        let x = match row.pages.contains(&before) {
            true => PAD + (before - row.pages.start) as f64 * (self.size + GAP) - GAP / 2.0,
            false => PAD + row.pages.len() as f64 * (self.size + GAP) - GAP / 2.0,
        };
        Some((x - 1.5, row.top, self.size))
    }

    /// The scroll that brings page `index` into a view `height` tall, or
    /// `None` when it is in view already.
    pub fn reveal(&self, index: usize, scroll: f64, height: f64) -> Option<f64> {
        let row = self.rows.iter().find(|row| row.pages.contains(&index))?;
        if row.top >= scroll && row.bottom <= scroll + height {
            return None;
        }
        Some((row.top - HEAD).max(0.0))
    }
}

/// The draft and everything about it that the window shows.
pub struct Organiser {
    pub sources: Vec<Source>,
    pub sheets: Vec<Sheet>,
    /// The pages selected, by index into the sheets.
    pub picked: BTreeSet<usize>,
    /// The page a ⇧-click selects from.
    picked_from: Option<usize>,
    undo: Vec<Vec<Sheet>>,
    redo: Vec<Vec<Sheet>>,
    /// The sheets as the organiser opened, to tell whether anything changed.
    opened: Vec<Sheet>,
    pub scroll: f64,
    pub grid: Grid,
    /// How wide and tall the grid is.
    pub width: f64,
    pub height: f64,
    /// A press on a page, which becomes a drag once it has moved: the page,
    /// and where the press was in the window.
    pub press: Option<(usize, (f64, f64))>,
    pub drag: Option<Drag>,
    /// A drag just ended, so the click that comes with its release is not a
    /// click on a page.
    pub dragged: bool,
    /// The clock that scrolls the grid under a drag held at its edge.
    pub roll: Option<u64>,
    /// The Split menu is open.
    pub splitting: bool,
    /// What is typed in "Every … pages".
    pub every: String,
    /// The chapters' text is being read, or the files written.
    pub busy: bool,
}

/// Pages being dragged to a new place.
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    /// Where the pointer is, in the window.
    pub at: (f64, f64),
    /// Where they would land ([`Grid::landing`]), or `None` with the pointer
    /// outside the grid.
    pub landing: Option<(usize, bool)>,
}

impl Organiser {
    pub fn new(sources: Vec<Source>, width: f64, height: f64, size: f64) -> Organiser {
        let sheets: Vec<Sheet> = (sources.iter().enumerate())
            .flat_map(|(source, file)| {
                (0..file.document.pages()).map(move |page| Sheet {
                    source,
                    page,
                    turn: 0,
                    begins: None,
                })
            })
            .collect();
        let mut organiser = Organiser {
            sources,
            opened: sheets.clone(),
            sheets,
            picked: BTreeSet::new(),
            picked_from: None,
            undo: Vec::new(),
            redo: Vec::new(),
            scroll: 0.0,
            grid: Grid::default(),
            width,
            height,
            press: None,
            drag: None,
            dragged: false,
            roll: None,
            splitting: false,
            every: String::new(),
            busy: false,
        };
        organiser.relayout(size);
        organiser
    }

    /// Lay the grid out again, for pages fitted into squares of `size`.
    pub fn relayout(&mut self, size: f64) {
        let shapes: Vec<Size> = (self.sheets.iter())
            .map(|sheet| {
                let size = self.sources[sheet.source].document.size_of(sheet.page);
                match sheet.turn % 2 {
                    0 => size,
                    _ => Size {
                        width: size.height,
                        height: size.width,
                    },
                }
            })
            .collect();
        let begins: Vec<bool> = (self.sheets.iter().enumerate())
            .map(|(at, sheet)| at > 0 && sheet.begins.is_some())
            .collect();
        self.grid = Grid::new(&shapes, &begins, self.width, size);
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
    }

    pub fn resize(&mut self, width: f64, height: f64) {
        self.width = width;
        self.height = height;
        self.relayout(self.grid.size);
    }

    pub fn max_scroll(&self) -> f64 {
        (self.grid.total - self.height).max(0.0)
    }

    pub fn scroll_by(&mut self, delta: f64) {
        self.scroll = (self.scroll + delta).clamp(0.0, self.max_scroll());
    }

    /// Bring page `index` into view.
    pub fn reveal(&mut self, index: usize) {
        if let Some(to) = self.grid.reveal(index, self.scroll, self.height) {
            self.scroll = to.clamp(0.0, self.max_scroll());
        }
    }

    /// Whether the draft differs from what the organiser opened with.
    pub fn changed(&self) -> bool {
        self.sheets != self.opened
    }

    /// The files that will be written: each a run of sheets.
    pub fn parts(&self) -> Vec<std::ops::Range<usize>> {
        (self.grid.heads.iter())
            .map(|head| head.first..head.first + head.pages)
            .collect()
    }

    /// The source that is the window's document, if one is.
    pub fn own(&self) -> Option<usize> {
        self.sources.iter().position(|source| source.own)
    }

    /// Whether the draft can be written into the window's document: it was
    /// opened on one, it is one file, and no page of the document is in it
    /// twice.
    pub fn fits_document(&self) -> bool {
        let Some(own) = self.own() else {
            return false;
        };
        let mut seen = BTreeSet::new();
        self.parts().len() == 1
            && (self.sheets.iter())
                .filter(|sheet| sheet.source == own)
                .all(|sheet| seen.insert(sheet.page))
    }

    /// The draft as `arrange` writes it.
    pub fn arranged(&self, range: std::ops::Range<usize>) -> Vec<crate::arrange::Sheet> {
        self.sheets[range]
            .iter()
            .map(|sheet| {
                let source = &self.sources[sheet.source];
                crate::arrange::Sheet {
                    from: (!source.own).then(|| source.path.clone()),
                    page: sheet.page + 1,
                    turn: sheet.turn,
                }
            })
            .collect()
    }

    /// What page `index` of the draft is called under its picture.
    pub fn label(&self, index: usize) -> String {
        let sheet = &self.sheets[index];
        let source = &self.sources[sheet.source];
        let label = source.label(sheet.page);
        match self.sources.len() {
            1 => label,
            _ => format!("{} · {label}", source.name),
        }
    }

    /* ------------------------------------------------------------ steps */

    /// Keep the draft as it is for undo, before it changes.
    fn step(&mut self) {
        self.undo.push(self.sheets.clone());
        if self.undo.len() > STEPS {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// The sheets changed: the selection is checked against them and the
    /// grid laid out again.
    fn changed_to(&mut self, picked: BTreeSet<usize>) {
        self.picked = picked
            .into_iter()
            .filter(|&at| at < self.sheets.len())
            .collect();
        self.picked_from = self.picked.first().copied();
        self.relayout(self.grid.size);
    }

    pub fn undo(&mut self) -> bool {
        let Some(sheets) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.sheets, sheets));
        self.changed_to(BTreeSet::new());
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(sheets) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.sheets, sheets));
        self.changed_to(BTreeSet::new());
        true
    }

    /* -------------------------------------------------------- selecting */

    /// A click on a page: alone, added or taken away (⌘), or every page
    /// from the last one clicked (⇧).
    pub fn click(&mut self, index: usize, adding: bool, extending: bool) {
        if std::mem::take(&mut self.dragged) {
            return;
        }
        if extending {
            let from = self.picked_from.unwrap_or(index);
            self.picked = (from.min(index)..=from.max(index)).collect();
            return;
        }
        if adding {
            if !self.picked.remove(&index) {
                self.picked.insert(index);
            }
        } else {
            self.picked = std::iter::once(index).collect();
        }
        self.picked_from = Some(index);
    }

    /// These pages selected, and no others.
    pub fn pick(&mut self, range: std::ops::Range<usize>) {
        self.picked = range.filter(|&at| at < self.sheets.len()).collect();
        self.picked_from = self.picked.first().copied();
    }

    pub fn pick_all(&mut self) {
        self.picked = (0..self.sheets.len()).collect();
    }

    /* ---------------------------------------------------------- editing */

    /// The selected pages taken out. A file that began with one of them
    /// begins with the next page left, unless that page begins one itself.
    /// Answers `false` when that would leave no page.
    pub fn remove(&mut self) -> bool {
        if self.picked.is_empty() {
            return true;
        }
        if self.picked.len() >= self.sheets.len() {
            return false;
        }
        self.step();
        let mut kept = Vec::new();
        let mut owed: Option<Option<String>> = None;
        for (at, sheet) in std::mem::take(&mut self.sheets).into_iter().enumerate() {
            if self.picked.contains(&at) {
                if sheet.begins.is_some() && owed.is_none() {
                    owed = Some(sheet.begins);
                }
                continue;
            }
            let mut sheet = sheet;
            if let Some(begins) = owed.take() {
                if sheet.begins.is_none() {
                    sheet.begins = begins;
                }
            }
            kept.push(sheet);
        }
        self.sheets = kept;
        self.changed_to(BTreeSet::new());
        true
    }

    /// The selected pages turned by `quarters` clockwise quarter turns.
    pub fn turn(&mut self, quarters: i32) {
        if self.picked.is_empty() {
            return;
        }
        self.step();
        for &at in &self.picked {
            let sheet = &mut self.sheets[at];
            sheet.turn = (sheet.turn + quarters).rem_euclid(4);
        }
        self.relayout(self.grid.size);
    }

    /// The selected pages moved to just before page `before`; with `leads`,
    /// they begin the file that page begins. A moved page carries no
    /// beginning of its own.
    pub fn move_to(&mut self, before: usize, leads: bool) {
        let moving: Vec<usize> = self.picked.iter().copied().collect();
        if moving.is_empty() {
            return;
        }
        let at = (0..before.min(self.sheets.len()))
            .filter(|index| !self.picked.contains(index))
            .count();
        let mut staying = Vec::new();
        let mut carried = Vec::new();
        for (index, sheet) in self.sheets.iter().cloned().enumerate() {
            match self.picked.contains(&index) {
                true => carried.push(Sheet {
                    begins: None,
                    ..sheet
                }),
                false => staying.push(sheet),
            }
        }
        if leads {
            if let Some(next) = staying.get_mut(at) {
                carried[0].begins = next.begins.take();
            }
        }
        let mut sheets = staying[..at].to_vec();
        sheets.extend(carried);
        sheets.extend_from_slice(&staying[at..]);
        // The first page always begins a file; a page that came before the
        // one that was first keeps that file's name.
        if sheets == self.sheets {
            return;
        }
        self.step();
        self.sheets = sheets;
        let count = moving.len();
        self.changed_to((at..at + count).collect());
    }

    /// Every page of `source` put in after the last selected page, or at
    /// the end, and selected.
    pub fn add(&mut self, source: Source) {
        self.step();
        let index = self.sources.len();
        let pages = source.document.pages();
        self.sources.push(source);
        let at = self
            .picked
            .last()
            .map_or(self.sheets.len(), |last| last + 1);
        let added = (0..pages).map(|page| Sheet {
            source: index,
            page,
            turn: 0,
            begins: None,
        });
        self.sheets.splice(at..at, added);
        self.changed_to((at..at + pages).collect());
    }

    /// A new file begins at the first selected page, or that page no longer
    /// begins one.
    pub fn cut(&mut self) {
        let Some(&first) = self.picked.first().filter(|&&first| first > 0) else {
            return;
        };
        self.step();
        let sheet = &mut self.sheets[first];
        sheet.begins = match sheet.begins {
            Some(_) => None,
            None => Some(String::new()),
        };
        self.relayout(self.grid.size);
    }

    /// The file that begins at page `first` joined to the one before it.
    pub fn join(&mut self, first: usize) {
        if first == 0
            || self
                .sheets
                .get(first)
                .is_none_or(|sheet| sheet.begins.is_none())
        {
            return;
        }
        self.step();
        self.sheets[first].begins = None;
        self.relayout(self.grid.size);
    }

    /// A new file every `every` pages, in place of the files there were.
    pub fn split_every(&mut self, every: usize) {
        if every == 0 {
            return;
        }
        self.step();
        for (at, sheet) in self.sheets.iter_mut().enumerate() {
            sheet.begins = (at > 0 && at % every == 0).then(String::new);
        }
        self.relayout(self.grid.size);
    }

    /// A new file at each chapter of the window's document, in place of the
    /// files there were. Each chapter is its title, its first page (one-
    /// based), and whether its last page is also the next chapter's first,
    /// which then goes into both files.
    pub fn split_at(&mut self, chapters: &[(String, usize, bool)]) {
        let Some(own) = self.own() else {
            return;
        };
        self.step();
        for sheet in &mut self.sheets {
            sheet.begins = None;
        }
        for (title, first, _) in chapters {
            let found = (self.sheets.iter())
                .position(|sheet| sheet.source == own && sheet.page + 1 == *first);
            if let Some(at) = found {
                self.sheets[at].begins = Some(title.clone());
            }
        }
        // A page shared by two chapters goes into the first one too, as a
        // second copy at its end.
        for (_, first, shared) in chapters.iter().rev() {
            if !shared {
                continue;
            }
            let Some(at) = (self.sheets.iter())
                .position(|sheet| sheet.source == own && sheet.page + 1 == *first)
            else {
                continue;
            };
            if at > 0 {
                let copy = Sheet {
                    begins: None,
                    ..self.sheets[at].clone()
                };
                self.sheets.insert(at, copy);
            }
        }
        self.changed_to(BTreeSet::new());
    }

    /// A step towards larger or smaller pages. Answers the new size.
    pub fn grow(&mut self, larger: bool) -> f64 {
        let size = match larger {
            true => self.grid.size + STEP,
            false => self.grid.size - STEP,
        };
        self.relayout(size.clamp(SMALLEST, LARGEST));
        self.grid.size
    }

    /// What the first file is called when it is saved as one of several,
    /// and what each part is called: its chapter, or "part n".
    pub fn part_names(&self) -> Vec<String> {
        self.parts()
            .iter()
            .enumerate()
            .map(|(at, range)| {
                let named = self.sheets[range.start].begins.clone().unwrap_or_default();
                match named.is_empty() {
                    true => format!("part {}", at + 1),
                    false => named,
                }
            })
            .collect()
    }
}

/// The organiser, over the whole window.
#[component]
pub fn OrganiserView(mut viewer: Signal<Viewer>) -> Element {
    let held = viewer.read();
    let Some(organiser) = held.organiser.as_ref() else {
        return rsx! {};
    };
    let palette = held.palette();
    for source in &organiser.sources {
        if source.chosen.get() != palette {
            source.chosen.set(palette);
        }
    }
    let ink = crate::palette::hex(palette.muted());
    let danger = crate::palette::hex(palette.negative());
    let on_accent = crate::palette::hex(palette.accent_contrast());
    let surround_ink = crate::palette::hex(palette.on_surround().muted());
    let picked = organiser.picked.len();
    let count = organiser.sheets.len();
    let parts = organiser.parts().len();
    let scroll = organiser.scroll;
    let grid = organiser.grid.clone();
    let mounted = grid.mounted(scroll, organiser.height);
    let heads: Vec<(usize, usize, f64, String)> = (grid.heads.iter().enumerate())
        .filter(|(_, head)| {
            head.top + HEAD >= scroll - organiser.height
                && head.top <= scroll + organiser.height * 2.0
        })
        .map(|(at, head)| {
            let pages = match head.pages {
                1 => "1 page".to_string(),
                n => format!("{n} pages"),
            };
            let name = organiser.sheets[head.first]
                .begins
                .clone()
                .unwrap_or_default();
            let said = match (parts, name.is_empty()) {
                (1, _) => pages,
                (_, true) => format!("File {} · {pages}", at + 1),
                (_, false) => format!("{name} · {pages}"),
            };
            (at, head.first, head.top, said)
        })
        .collect();
    // Each page drawn: where it is in the draft, which page of which file,
    // where it goes, what it is called, whether it is selected, its key,
    // and how it is turned.
    #[allow(clippy::type_complexity)]
    let pages: Vec<(usize, usize, Place, String, bool, String, usize, View)> = mounted
        .iter()
        .map(|&at| {
            let sheet = &organiser.sheets[at];
            // Keyed by the page and how often it came before, so that a
            // page that moves keeps its picture.
            let copy = (organiser.sheets[..at].iter())
                .filter(|other| other.source == sheet.source && other.page == sheet.page)
                .count();
            (
                at,
                sheet.page,
                grid.places[at],
                organiser.label(at),
                organiser.picked.contains(&at),
                format!("{}:{}:{copy}", sheet.source, sheet.page),
                sheet.source,
                View {
                    rotation: (sheet.turn.rem_euclid(4) * 90) as u32,
                    crop: None,
                },
            )
        })
        .collect();
    let chosen: Vec<Chosen> = organiser.sources.iter().map(|s| s.chosen.clone()).collect();
    let line = (organiser.drag.as_ref())
        .and_then(|drag| drag.landing)
        .and_then(|(before, _)| grid.line(before));
    let lifted = (organiser.drag.as_ref()).map(|drag| (drag.at, picked.max(1)));
    let into = (organiser.own())
        .filter(|_| organiser.fits_document())
        .map(|own| organiser.sources[own].name.clone());
    let splitting = organiser.splitting;
    let every = organiser.every.clone();
    let chapters = organiser.own().is_some() && held.chapters().len() > 1;
    let busy = organiser.busy;
    let empty = count == 0;
    let total = grid.total;
    let save_label = match parts {
        1 => "Save as…".to_string(),
        n => format!("Save as {n} files…"),
    };
    let none = picked == 0;
    let counted = match (picked, count) {
        (0, 1) => "1 page".to_string(),
        (0, n) => format!("{n} pages"),
        (p, n) => format!("{p} of {n} selected"),
    };
    drop(held);

    rsx! {
        div {
            class: "organiser",
            role: "dialog",
            "aria-label": "Arrange pages",
            onmousedown: move |event| {
                event.stop_propagation();
                if viewer.read().organiser.as_ref().is_some_and(|o| o.splitting) {
                    if let Some(organiser) = viewer.write().organiser.as_mut() {
                        organiser.splitting = false;
                    }
                }
            },
            div { class: "organiser-bar",
                span { class: "organiser-title", "Arrange pages" }
                span { class: "organiser-count", "{counted}" }
                div { class: "organiser-tools",
                    button {
                        class: "chip",
                        "data-item": "add-files",
                        onclick: move |_| viewer.write().add_files(),
                        Icon { name: "plusCircle", stroke: ink.clone() }
                        span { class: "chip-label", "Add files…" }
                    }
                    button {
                        class: "chip",
                        "data-item": "turn-left",
                        "aria-label": "Turn left",
                        disabled: none.then_some("true"),
                        onclick: move |_| viewer.write().turn_sheets(-1),
                        Icon { name: "rotateLeft", stroke: ink.clone() }
                        span { class: "chip-label", "Left" }
                    }
                    button {
                        class: "chip",
                        "data-item": "turn-right",
                        "aria-label": "Turn right",
                        disabled: none.then_some("true"),
                        onclick: move |_| viewer.write().turn_sheets(1),
                        Icon { name: "rotateRight", stroke: ink.clone() }
                        span { class: "chip-label", "Right" }
                    }
                    button {
                        class: "chip organiser-remove",
                        "data-item": "remove-pages",
                        "aria-label": "Remove",
                        disabled: none.then_some("true"),
                        onclick: move |_| viewer.write().remove_sheets(),
                        Icon { name: "trash", stroke: ink.clone(), class: "rest" }
                        Icon { name: "trash", stroke: danger.clone(), class: "hot" }
                        span { class: "chip-label", "Remove" }
                    }
                    button {
                        class: "chip",
                        "data-item": "cut",
                        "aria-label": "New file here",
                        disabled: none.then_some("true"),
                        onclick: move |_| viewer.write().cut_sheets(),
                        Icon { name: "pages", stroke: ink.clone() }
                        span { class: "chip-label", "New file here" }
                    }
                    div { class: "organiser-split",
                        onmousedown: move |event| event.stop_propagation(),
                        button {
                            class: if splitting { "chip on" } else { "chip" },
                            "data-item": "split",
                            disabled: empty.then_some("true"),
                            onclick: move |_| {
                                if let Some(organiser) = viewer.write().organiser.as_mut() {
                                    organiser.splitting = !organiser.splitting;
                                }
                            },
                            Icon { name: "pages", stroke: ink.clone() }
                            span { class: "chip-label", "Split" }
                        }
                        if splitting {
                            div { class: "menu organiser-split-menu", role: "menu",
                                if chapters {
                                    button {
                                        class: "menu-item",
                                        "data-item": "split-chapters",
                                        onclick: move |_| viewer.write().split_at_chapters(),
                                        span { class: "menu-label", "At each chapter" }
                                    }
                                }
                                div { class: "menu-row menu-pages",
                                    span { class: "menu-row-label", "Every" }
                                    input {
                                        class: "menu-pages-field organiser-every",
                                        r#type: "text",
                                        value: "{every}",
                                        "aria-label": "Pages in each file",
                                        "data-keyboard": "pages",
                                        oninput: move |event| {
                                            if let Some(organiser) = viewer.write().organiser.as_mut() {
                                                organiser.every = event.value();
                                            }
                                        },
                                        onkeydown: move |event| match event.key() {
                                            Key::Enter => {
                                                event.stop_propagation();
                                                viewer.write().split_every();
                                            }
                                            Key::Escape => {
                                                event.stop_propagation();
                                                if let Some(organiser) = viewer.write().organiser.as_mut() {
                                                    organiser.splitting = false;
                                                }
                                            }
                                            _ => crate::app::field_keeps(&event),
                                        },
                                    }
                                    span { class: "menu-row-label", "pages" }
                                    button {
                                        class: "chip action menu-pages-save",
                                        "data-item": "split-every",
                                        onclick: move |_| viewer.write().split_every(),
                                        "Split"
                                    }
                                }
                            }
                        }
                    }
                    div { class: "zoom-group organiser-size",
                        button {
                            class: "chip",
                            "data-item": "smaller",
                            "aria-label": "Smaller pages",
                            onclick: move |_| viewer.write().grow_sheets(false),
                            "−"
                        }
                        button {
                            class: "chip",
                            "data-item": "larger",
                            "aria-label": "Larger pages",
                            onclick: move |_| viewer.write().grow_sheets(true),
                            "+"
                        }
                    }
                }
                div { class: "organiser-ends",
                    button {
                        class: "chip action",
                        "data-item": "close",
                        onclick: move |_| viewer.write().close_organiser(false),
                        "Close"
                    }
                    if let Some(name) = into {
                        button {
                            class: "chip action",
                            "data-item": "save-into",
                            disabled: busy.then_some("true"),
                            onclick: move |_| viewer.write().save_into_document(),
                            "Save into {name}"
                        }
                    }
                    button {
                        class: "chip action primary",
                        "data-item": "save-as",
                        disabled: (busy || empty).then_some("true"),
                        onclick: move |_| viewer.write().save_arranged(),
                        "{save_label}"
                    }
                }
            }
            div {
                class: "organiser-grid",
                "data-scroll": "{scroll}",
                onwheel: move |event| {
                    let height = viewer.read().organiser.as_ref().map_or(0.0, |o| o.height);
                    let delta = -match event.delta() {
                        WheelDelta::Pixels(delta) => delta.y,
                        WheelDelta::Lines(delta) => delta.y * 60.0,
                        WheelDelta::Pages(delta) => delta.y * height,
                    };
                    viewer.write().scroll_organiser(delta);
                },
                div {
                    class: "organiser-sheets",
                    style: "height: {total}px; top: {-scroll}px;",
                    if empty {
                        p { class: "organiser-empty", "Add files to combine their pages into one." }
                    }
                    for (at, first, top, said) in heads {
                        div {
                            key: "h{at}",
                            class: "organiser-head",
                            style: "top: {top}px;",
                            span { class: "organiser-head-name", "{said}" }
                            if first > 0 {
                                button {
                                    class: "chip organiser-join",
                                    "data-item": "join",
                                    onmousedown: move |event| event.stop_propagation(),
                                    onclick: move |_| viewer.write().join_sheets(first),
                                    Icon { name: "pages", stroke: surround_ink.clone() }
                                    span { "Join with the file above" }
                                }
                            }
                        }
                    }
                    for (at, page, place, label, is_picked, key, source, view) in pages {
                        Leaf {
                            key: "{key}",
                            viewer,
                            chosen: chosen[source].clone(),
                            at,
                            page,
                            place,
                            label,
                            picked: is_picked,
                            view,
                            on_accent: on_accent.clone(),
                        }
                    }
                    if let Some((x, y, height)) = line {
                        div { class: "organiser-drop", style: "left: {x}px; top: {y}px; height: {height}px;" }
                    }
                }
            }
            if let Some(((x, y), many)) = lifted {
                div {
                    class: "organiser-lift",
                    style: "left: {x + 12.0}px; top: {y + 12.0}px;",
                    if many == 1 { "1 page" } else { "{many} pages" }
                }
            }
        }
    }
}

/// One page in the grid.
#[component]
fn Leaf(
    mut viewer: Signal<Viewer>,
    chosen: Chosen,
    at: usize,
    page: usize,
    place: Place,
    label: String,
    picked: bool,
    view: View,
    on_accent: String,
) -> Element {
    // A widget is handed to Blitz once, so the turn is a cell it reads.
    let turned = use_hook(|| std::rc::Rc::new(std::cell::Cell::new(view)));
    turned.set(view);
    let widget = use_hook(|| {
        let shell = dioxus_core::try_consume_context::<
            std::sync::Arc<dyn blitz_traits::shell::ShellProvider>,
        >();
        CustomWidgetAttr::new(PageWidget::new(page, turned.clone(), chosen.clone(), shell).plain())
    });
    let Place {
        x,
        y,
        width,
        height,
    } = place;
    rsx! {
        button {
            class: if picked { "sheet picked" } else { "sheet" },
            "data-sheet": "{at}",
            style: "left: {x}px; top: {y}px; width: {width}px;",
            onclick: move |event| {
                let held = event.modifiers();
                viewer.write().click_sheet(at, crate::keymap::command(held), held.shift());
            },
            onmousedown: move |event| {
                event.stop_propagation();
                if event.trigger_button() == Some(dioxus::html::input_data::MouseButton::Primary) {
                    let at_pointer = event.client_coordinates();
                    viewer.write().press_sheet(at, (at_pointer.x, at_pointer.y));
                }
            },
            div {
                class: "sheet-picture",
                style: "width: {width}px; height: {height}px;",
                object {
                    "data": widget,
                    // See `Thumb` in `sidebar.rs`: the click has to reach
                    // the button around the picture.
                    style: "display: block; pointer-events: none; width: {width}px; height: {height}px;",
                }
                if picked {
                    span { class: "thumb-tick", Icon { name: "check", stroke: on_accent } }
                }
            }
            span { class: "sheet-label", "{label}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letters(count: usize) -> Vec<Size> {
        vec![
            Size {
                width: 612.0,
                height: 792.0,
            };
            count
        ]
    }

    #[test]
    fn a_new_file_starts_a_new_section_on_a_new_row() {
        let mut begins = vec![false; 7];
        begins[5] = true;
        let grid = Grid::new(
            &letters(7),
            &begins,
            2.0 * PAD + 3.0 * 100.0 + 2.0 * GAP,
            100.0,
        );
        assert_eq!(grid.heads.len(), 2);
        assert_eq!((grid.heads[1].first, grid.heads[1].pages), (5, 2));
        // Three to a row: 0–2, 3–4, then the second file's 5–6.
        assert_eq!(grid.places[3].y, grid.places[4].y);
        assert!(grid.places[5].y > grid.places[3].y + HEAD);
        assert_eq!(grid.places[5].x, grid.places[0].x);
    }

    #[test]
    fn a_landing_is_before_the_page_under_the_pointer_or_after_the_row() {
        let begins = vec![false; 5];
        let grid = Grid::new(
            &letters(5),
            &begins,
            2.0 * PAD + 3.0 * 100.0 + 2.0 * GAP,
            100.0,
        );
        let one = grid.places[1];
        assert_eq!(grid.landing(one.x + 1.0, one.y + 10.0), (1, false));
        assert_eq!(
            grid.landing(one.x + one.width - 1.0, one.y + 10.0),
            (2, false)
        );
        // Past the last page of the first row: before the first of the next.
        assert_eq!(grid.landing(1000.0, one.y + 10.0), (3, false));
        let last = grid.places[4];
        assert_eq!(grid.landing(1000.0, last.y + 10.0), (5, false));
    }

    #[test]
    fn only_the_rows_near_the_view_are_drawn() {
        let begins = vec![false; 300];
        let grid = Grid::new(&letters(300), &begins, 2.0 * PAD + 100.0, 100.0);
        let drawn = grid.mounted(5000.0, 500.0);
        assert!(drawn.len() < 12, "{}", drawn.len());
        assert!(drawn.contains(&(((5000.0 - PAD - HEAD) / (100.0 + LABEL + ROW_GAP)) as usize + 1)));
    }
}
