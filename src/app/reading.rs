//! The reading line: a band under the line being read, which helps the eye
//! keep its place. It is off by default, and switched on in Settings or the
//! View menu. While it is on, the keys that scroll a little (↓ and ↑, j and
//! k) move it a line at a time, and the page follows it.

use super::*;

/// How far down the window the band goes before the page follows it.
const READING_REACH: f64 = 0.75;

/// How many pages are looked through for the next line, so that a page
/// with no text on it, such as a figure, is passed over.
const READING_PAGES: usize = 3;

/// A page's text, and its lines: where each begins and the band under it.
pub(super) type ReadingBands = (Rc<PageText>, Rc<Vec<(usize, Rect)>>);

impl Viewer {
    pub fn reading_line_on(&self) -> bool {
        self.store.flag("reading_line")
    }

    /// Switch the reading line on or off. Switched on, it goes under the
    /// first line in the window.
    pub fn set_reading_line(&mut self, on: bool) {
        self.set_flag("reading_line", on);
        self.reading = on.then(|| self.first_line_in_view(true)).flatten();
    }

    /// Where the reading line is, as a page (one-based) and the character
    /// its line begins at, if it is on. Until a key has moved it, it is
    /// under the first line in the window. Asked once a frame, since the
    /// first line in the window is looked for line by line.
    pub fn reading_place(&self) -> Option<(usize, usize)> {
        if !self.reading_line_on() {
            return None;
        }
        self.reading.or_else(|| self.first_line_in_view(false))
    }

    /// The band on page `page`, in CSS pixels from the top left of its box,
    /// if the reading line at `place` is on that page.
    pub fn reading_band(&self, page: usize, place: Option<(usize, usize)>) -> Option<Rect> {
        let (on, from) = place?;
        (on == page).then(|| self.band_at(on, from)).flatten()
    }

    /// Move the reading line a line down or up. A line out of the window is
    /// not moved from: the band goes under the first line in the window
    /// instead. Answers whether there was a line to go to, so that a page
    /// with no text on it still scrolls.
    pub fn step_reading_line(&mut self, forwards: bool) -> bool {
        let next = match self.reading {
            Some((page, from)) if !self.in_view(page, from) => self.first_line_in_view(true),
            here => (here.or_else(|| self.first_line_in_view(true)))
                .and_then(|here| self.line_beside(here, forwards)),
        };
        let Some((page, from)) = next else {
            return false;
        };
        self.reading = Some((page, from));
        let band = |viewer: &Self| viewer.band_at(page, from);
        self.show_on(page, band, READING_REACH, REVEAL);
        true
    }

    /// The band under the line that begins at character `from` of page
    /// `page`: the line's letters, with a little room around them.
    fn band_at(&self, page: usize, from: usize) -> Option<Rect> {
        self.layout.box_of(page - 1)?;
        let text = self.text_on(page);
        let (line, _) = crate::select::extent(&text, crate::select::line_around(&text, from))?;
        Some(self.layout.place_on(page - 1, with_room(line)))
    }

    /// Forget the text read off the pages, and the reading line's lines
    /// with it, when the pages it was read from are gone.
    pub(super) fn forget_texts(&self) {
        self.texts.borrow_mut().clear();
        self.reading_bands.borrow_mut().clear();
    }

    /// Every line of `text` with its band, in the page's own space. The
    /// first line in the window is looked for on every frame while the
    /// reading line is on, so the lines of the last few pages are kept, for
    /// as long as their text is the one the window has.
    fn bands_of(&self, text: &Rc<PageText>) -> Rc<Vec<(usize, Rect)>> {
        let mut kept = self.reading_bands.borrow_mut();
        if let Some((_, bands)) = kept.iter().find(|(of, _)| Rc::ptr_eq(of, text)) {
            return bands.clone();
        }
        let bands: Rc<Vec<(usize, Rect)>> = Rc::new(
            (crate::select::lines(text).into_iter())
                .filter_map(|line| Some((line.0, with_room(crate::select::extent(text, line)?.0))))
                .collect(),
        );
        if kept.len() >= READING_KEPT {
            kept.remove(0);
        }
        kept.push((text.clone(), bands.clone()));
        bands
    }

    /// Whether the whole of the band at this line is in the window.
    fn in_view(&self, page: usize, from: usize) -> bool {
        let (Some(area), Some(band)) = (self.layout.box_of(page - 1), self.band_at(page, from))
        else {
            return false;
        };
        let top = area.top + band.top;
        top >= self.scroll_top && top + band.height <= self.scroll_top + self.layout.viewport.height
    }

    /// The first line whose top is in the window. Without `wait`, as on
    /// every frame, a page whose text cannot be read without waiting for
    /// the renderer is passed over until it has been read.
    fn first_line_in_view(&self, wait: bool) -> Option<(usize, usize)> {
        let first = self.layout.anchor(self.scroll_top).page;
        (first..=(first + READING_PAGES).min(self.pages())).find_map(|page| {
            let area = self.layout.box_of(page - 1)?;
            let text = self.text_reading(page, wait)?;
            self.bands_of(&text).iter().find_map(|&(from, band)| {
                let band = self.layout.place_on(page - 1, band);
                (area.top + band.top >= self.scroll_top).then_some((page, from))
            })
        })
    }

    /// The line after or before the one beginning at `here`, on this page
    /// or on the next pages that have text.
    fn line_beside(&self, (page, from): (usize, usize), forwards: bool) -> Option<(usize, usize)> {
        let lines = crate::select::lines(&self.text_on(page));
        let at = lines.iter().position(|&(a, b)| (a..b).contains(&from))?;
        let on_page = match forwards {
            true => lines.get(at + 1),
            false => at.checked_sub(1).and_then(|at| lines.get(at)),
        };
        if let Some(&(from, _)) = on_page {
            return Some((page, from));
        }
        let pages: Vec<usize> = match forwards {
            true => (page + 1..=(page + READING_PAGES).min(self.pages())).collect(),
            false => (page.saturating_sub(READING_PAGES).max(1)..page)
                .rev()
                .collect(),
        };
        pages.into_iter().find_map(|page| {
            let lines = crate::select::lines(&self.text_on(page));
            let line = if forwards {
                lines.first()
            } else {
                lines.last()
            };
            line.map(|&(from, _)| (page, from))
        })
    }
}

/// How many pages' lines [`Viewer::bands_of`] keeps: the window's pages and
/// the few after them that are looked through.
const READING_KEPT: usize = 8;

/// A line's letters with a little room around them.
fn with_room(line: Rect) -> Rect {
    let room = line.height * 0.2;
    Rect {
        left: line.left - room,
        top: line.top - room,
        width: line.width + 2.0 * room,
        height: line.height + 2.0 * room,
    }
}
