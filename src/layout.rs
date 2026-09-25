//! The layout, ported from `viewer.ts`.
//!
//! This is the part of the app that decides where every page is, which pages
//! are worth having in the document at all, and which page the reader is
//! looking at. In the app it is about six hundred lines of TypeScript spread
//! through a three-thousand-line file, entangled with the DOM it is placing
//! things in. Here it is a plain struct with no renderer, no widget and no
//! window in it, which is the first thing this port buys: `cargo test` can ask
//! it every question the harness had to open a browser to ask.
//!
//! Everything it knows is from `relayout()` in `viewer.ts`, and the comments
//! that explain *why* a line is the way it is are carried over with the line,
//! because those are the parts that were paid for.

use crate::render::Rect;

/// A page's size, in PDF points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

/// Where a page is, and how big: the layout's whole output.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageBox {
    pub top: f64,
    pub left: f64,
    pub width: f64,
    pub height: f64,
    /// Points to pixels, for this page in this layout.
    pub scale: f64,
    /// The empty space directly above this page — the gap from the row before,
    /// or `PAD_Y` at the start of the document, and they are not the same
    /// number. Landing on a page means landing on that space, and it is
    /// recorded here rather than read back off the page before, which is right
    /// until two pages stand side by side and the box before this one is its
    /// neighbour, sharing its top exactly.
    pub above: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Width,
    Page,
    Actual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spread {
    Single,
    Two,
    Cover,
}

/// Continuous scrolling, or one page at a time.
///
/// **The brief calls continuous a strong default that can only ever change if
/// the reader explicitly opts into it**, and that is why there is no action
/// for this in [`crate::keymap`] and no chip in the toolbar: it is a line in
/// `settings.toml` and nothing else, exactly as the app has it. A key that
/// turns continuous scrolling off by accident is the failure the brief names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Continuous,
    Paged,
}

/// The part of a page worth showing: an origin and a size, both as fractions
/// of the whole page. `None` is the whole of it.
///
/// `Crop` in `viewer.ts`, and the same decision behind it: **one crop for the
/// whole document rather than one per page**, because a per-page crop changes
/// the scale from page to page, and in continuous scrolling that is a
/// document that breathes as you read it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Crop {
    /// The same rectangle on a page turned a quarter clockwise.
    ///
    /// A crop is a rectangle on the page *as the reader sees it*, so it turns
    /// with the page: `(x, y, w, h)` becomes `(1 − y − h, x, h, w)`. Turning
    /// it is exact and free; measuring it again would be eight renders for an
    /// answer already in hand — which is `rotate()` in `viewer.ts`, line for
    /// line.
    pub fn turned(self) -> Crop {
        Crop {
            x: 1.0 - self.y - self.height,
            y: self.x,
            width: self.height,
            height: self.width,
        }
    }
}

/// How a page is to be drawn: turned by so many degrees, and only this much
/// of it.
///
/// The renderer's whole instruction beyond a size, and it is one value rather
/// than two arguments because the two are decided together and are wrong
/// apart: a crop is a rectangle on a *turned* page, so a renderer handed one
/// without the other draws the wrong corner of the paper.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Clockwise, in degrees, and always one of 0, 90, 180, 270.
    pub rotation: u32,
    pub crop: Option<Crop>,
}

impl View {
    /// The page as the document has it: no turn, no trim. What a thumbnail
    /// gets, and what a renderer with nothing to say about either does.
    pub const WHOLE: View = View {
        rotation: 0,
        crop: None,
    };
}

/// What sets a page off from the window when it is narrower than one. Fit
/// width is the mode whose whole point is that it is not, so it does not get
/// one: charging it for the margin left forty pixels of ground either side of
/// a page that had supposedly reached both edges. `PAD_Y` is not conditional,
/// because there is always something above a page.
pub const PAD_X: f64 = 20.0;
pub const PAD_Y: f64 = 20.0;

/// How much beyond the viewport is kept in the document, as a fraction of a
/// screen either way. Under Blitz this matters more than it did under a
/// webview: every widget in the document is painted every frame whether or not
/// it is on screen, so a page that is not near the viewport must be genuinely
/// absent rather than merely invisible.
pub const OVERSCAN: f64 = 0.6;

/// The ceiling on one page's bitmap. A canvas had this to stay inside what a
/// browser would allocate; a texture has it because a page drawn at more
/// pixels than the screen can show is bytes nobody reads.
pub const MAX_PIXELS: f64 = 12_000_000.0;

/// pdf.js's own points-to-pixels, kept so that "100%" means here what it means
/// in the app.
pub const PDF_TO_CSS_UNITS: f64 = 96.0 / 72.0;

/// Where the reader is, described so that it survives a change of scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    /// One-based, as everything the reader sees is.
    pub page: usize,
    /// How far down that page, as a fraction of its height.
    pub offset: f64,
}

pub struct Layout {
    sizes: Vec<Size>,
    /// Where every page is — and `None` for a page that is not laid out at
    /// all, which is every page but one in paged mode.
    ///
    /// **An `Option` rather than a zeroed box, and that is the whole of what
    /// this port does differently.** `viewer.ts` keeps a sparse JavaScript
    /// array here, and five things downstream — two binary searches,
    /// `trackCurrentPage`, `pointAt` and `mount` — each carry a comment
    /// explaining that they must check for a hole. `AGENTS.md` calls that
    /// "the correct amount of defence for the shape". In Rust the shape says
    /// it itself: nothing can read a box without answering the question, and
    /// [`Layout::box_of`] was already returning an `Option` for the
    /// out-of-range case.
    boxes: Vec<Option<PageBox>>,
    pub fit: Fit,
    /// The zoom factor, which only `Fit::Actual` reads.
    pub zoom: f64,
    pub spread: Spread,
    /// Continuous or one page at a time. See [`Mode`].
    pub mode: Mode,
    /// The page the reader is on, one-based.
    ///
    /// A *derived* number in continuous mode — [`Layout::page_at`] reads it
    /// off the scroll offset — and an authoritative one in paged mode, where
    /// it decides which page is laid out at all. It lives here rather than in
    /// the viewer because `relayout` is what needs it, and a layout that has
    /// to be told which page to lay out by whoever calls it is a layout with
    /// two sources of truth.
    pub current: usize,
    /// Quarter turns the reader has asked for, clockwise, in degrees.
    ///
    /// A way of looking rather than a property of the file, so it is not
    /// written down and does not survive the run — which is what `viewer.ts`
    /// says of it and what Preview, Acrobat and Sumatra all do. Within a run
    /// it stays with the *window*, like the fit and the zoom beside it: a
    /// document opened into a turned window is turned. See
    /// [`crate::app::Viewer::open_here`]. It is added to the page's own rotation by the renderer, because a
    /// page that says it is printed sideways has already been turned once and
    /// the reader is asking for one more.
    pub rotation: u32,
    /// What is left of a page once its margins are taken off, or `None` for
    /// all of it. See [`Crop`] and [`crate::crop`].
    pub crop: Option<Crop>,
    /// The distance between two rows, and between two pages of a spread.
    pub gap: f64,
    /// The scroller's own size, in CSS pixels.
    pub viewport: Size,
    content_width: f64,
    content_height: f64,
}

impl Layout {
    pub fn new(sizes: Vec<Size>) -> Self {
        let mut layout = Layout {
            sizes,
            boxes: Vec::new(),
            fit: Fit::Width,
            zoom: 1.0,
            spread: Spread::Single,
            mode: Mode::Continuous,
            current: 1,
            rotation: 0,
            crop: None,
            gap: 16.0,
            viewport: Size {
                width: 900.0,
                height: 900.0,
            },
            content_width: 0.0,
            content_height: 0.0,
        };
        layout.relayout();
        layout
    }

    pub fn pages(&self) -> usize {
        self.sizes.len()
    }

    /// The same layout over a different document.
    ///
    /// What a recompile leaves behind: the paper has been rewritten and every
    /// page in it may be a different shape or gone altogether, while the fit,
    /// the zoom, the spread, the rotation and the window are the reader's and
    /// have not changed at all. Rebuilding the whole [`Layout`] would take
    /// those with it, so the sizes are replaced and everything else stays.
    ///
    /// `current` is clamped, because a draft that lost its last chapter is
    /// exactly the case this has to survive: paged mode lays out `current`
    /// and nothing else, so a page number past the end is a window with
    /// nothing in it.
    pub fn replace_sizes(&mut self, sizes: Vec<Size>) {
        self.sizes = sizes;
        self.current = self.current.clamp(1, self.sizes.len().max(1));
        self.relayout();
    }

    /// A page's size as the document has it, before anything is done to it.
    pub fn size_of(&self, index: usize) -> Size {
        self.sizes[index]
    }

    /// The whole page, turned but not cropped.
    ///
    /// What a rectangle *on* the page is measured against — a link's area, a
    /// match's quad — because those are fractions of a whole page whatever is
    /// being shown of it. `wholeSizeOf` in `viewer.ts`, and the same reason
    /// it is a second function rather than a flag on the first.
    pub fn whole_size_of(&self, index: usize) -> Size {
        let size = self.sizes[index];
        if self.rotation.is_multiple_of(180) {
            size
        } else {
            Size {
                width: size.height,
                height: size.width,
            }
        }
    }

    /// The page as the layout has to place it: turned, and then only the part
    /// of it the crop keeps.
    fn effective(&self, index: usize) -> Size {
        let turned = self.whole_size_of(index);
        match self.crop {
            None => turned,
            Some(crop) => Size {
                width: turned.width * crop.width,
                height: turned.height * crop.height,
            },
        }
    }

    /// How every page is to be drawn, which is one value the whole reader
    /// agrees on. See [`View`].
    pub fn view(&self) -> View {
        View {
            rotation: self.rotation,
            crop: self.crop,
        }
    }

    /// Turn the document a quarter at a time, taking the crop with it.
    ///
    /// The crop turns rather than being measured again — see [`Crop::turned`]
    /// — and nothing else has to be told: a link and a match are both kept in
    /// the page's own unturned points here, so what changes is where they are
    /// *put*, which [`Layout::whole_size_of`] already answers.
    pub fn turn(&mut self, quarter_turns: i32) {
        let before = self.rotation;
        self.rotation = (self.rotation as i32 + quarter_turns * 90).rem_euclid(360) as u32;
        if self.rotation == before {
            return;
        }
        let mut turns = quarter_turns.rem_euclid(4);
        while turns > 0 {
            self.crop = self.crop.map(Crop::turned);
            turns -= 1;
        }
    }

    pub fn boxes(&self) -> &[Option<PageBox>] {
        &self.boxes
    }

    /// Where a page is, or `None` for a page outside the document or one this
    /// layout has not placed. See [`Layout::boxes`].
    pub fn box_of(&self, index: usize) -> Option<PageBox> {
        self.boxes.get(index).copied().flatten()
    }

    pub fn content_width(&self) -> f64 {
        self.content_width
    }

    pub fn content_height(&self) -> f64 {
        self.content_height
    }

    /// The pages that stand together, in order.
    ///
    /// One each in single mode. In `Cover`, page one is alone and every pair
    /// after it is (even, odd) — which is how a book falls open, page one
    /// being a right-hand page. In `Two` the pairs start from the first page,
    /// which is what a document of slides or a scan of two-up photocopies
    /// wants.
    pub fn rows(&self) -> Vec<Vec<usize>> {
        let count = self.sizes.len();
        if self.spread == Spread::Single {
            return (0..count).map(|index| vec![index]).collect();
        }
        let mut rows = Vec::new();
        let mut index = 0;
        if self.spread == Spread::Cover && count > 0 {
            rows.push(vec![0]);
            index = 1;
        }
        while index < count {
            if index + 1 < count {
                rows.push(vec![index, index + 1]);
            } else {
                rows.push(vec![index]);
            }
            index += 2;
        }
        rows
    }

    /// The row a page is standing in, and the pages standing with it.
    pub fn row_of(&self, index: usize) -> Vec<usize> {
        let count = self.sizes.len();
        match self.spread {
            Spread::Single => vec![index],
            Spread::Cover => {
                if index == 0 {
                    return vec![0];
                }
                let first = if index % 2 == 1 { index } else { index - 1 };
                if first + 1 < count {
                    vec![first, first + 1]
                } else {
                    vec![first]
                }
            }
            Spread::Two => {
                let first = index - (index % 2);
                if first + 1 < count {
                    vec![first, first + 1]
                } else {
                    vec![first]
                }
            }
        }
    }

    /// Work out where every page is. The one place a page's position is
    /// decided, and the only thing that writes `boxes`.
    pub fn relayout(&mut self) {
        self.boxes.clear();
        if self.sizes.is_empty() {
            self.content_width = 0.0;
            self.content_height = 0.0;
            return;
        }

        let pad_x = if self.fit == Fit::Width { 0.0 } else { PAD_X };
        let available_width = (self.viewport.width - pad_x * 2.0).max(120.0);
        let available_height = (self.viewport.height - PAD_Y * 2.0).max(120.0);

        // One row in paged mode: the row the reader is on, and nothing else
        // laid out at all. Everything downstream works in rows already, so
        // this is the whole of the difference between the two modes.
        let rows = match self.mode {
            Mode::Continuous => self.rows(),
            Mode::Paged => vec![self.row_of(self.current.clamp(1, self.sizes.len()) - 1)],
        };

        // Turned and trimmed once, here, rather than at every one of the six
        // places below that asks a page how big it is. A crop and a rotation
        // are the same kind of fact as a page's size and this is where the
        // layout is allowed to know about them; everything downstream works
        // in these.
        let sizes: Vec<Size> = (0..self.sizes.len())
            .map(|index| self.effective(index))
            .collect();

        // The gap between two pages of a spread is a distance on the screen,
        // like the gap between rows — it is not part of the page and does not
        // grow with the zoom. So it comes off the room available before the
        // scale is worked out, rather than being scaled along with the paper.
        let gaps_in = |row: &[usize]| (row.len() as f64 - 1.0) * self.gap;
        let paper_width = |row: &[usize]| row.iter().map(|&index| sizes[index].width).sum::<f64>();
        let row_height = |row: &[usize]| {
            row.iter()
                .map(|&index| sizes[index].height)
                .fold(0.0f64, f64::max)
        };
        let scale_for = |size: Size, room: f64| -> f64 {
            match self.fit {
                Fit::Width => room / size.width,
                Fit::Page => (room / size.width).min(available_height / size.height),
                Fit::Actual => PDF_TO_CSS_UNITS * self.zoom,
            }
        };
        let scale_for_row = |row: &[usize]| {
            scale_for(
                Size {
                    width: paper_width(row),
                    height: row_height(row),
                },
                (available_width - gaps_in(row)).max(120.0),
            )
        };
        let row_span = |row: &[usize], scale: f64| {
            row.iter()
                .map(|&index| (sizes[index].width * scale).round())
                .sum::<f64>()
                + gaps_in(row)
        };

        let mut width = 0.0f64;
        for row in &rows {
            width = width.max(row_span(row, scale_for_row(row)));
        }
        self.content_width = width.max(available_width) + pad_x * 2.0;

        let mut boxes: Vec<Option<PageBox>> = vec![None; self.sizes.len()];
        let mut top = PAD_Y;
        let mut above = PAD_Y;
        for row in &rows {
            let scale = scale_for_row(row);
            let across = row_span(row, scale);
            let mut left = ((self.content_width - across) / 2.0).round();
            let mut tallest = 0.0f64;
            for &index in row {
                let size = sizes[index];
                let page_width = (size.width * scale).round();
                let page_height = (size.height * scale).round();
                boxes[index] = Some(PageBox {
                    top,
                    left,
                    width: page_width,
                    height: page_height,
                    scale,
                    above,
                });
                left += page_width + self.gap;
                tallest = tallest.max(page_height);
            }
            top += tallest + self.gap;
            above = self.gap;
        }
        self.boxes = boxes;
        self.content_height = (top - self.gap + PAD_Y).max(0.0);
    }

    /* Both searches below assume `boxes` runs in order down the page and has
    no holes, which is true in continuous mode and is why neither is used
    in paged mode — there, one page is laid out and the rest of the array
    is empty. A hole stops the search where `viewer.ts` breaks out of the
    same loop, so a caller that reaches one anyway gets an answer rather
    than a panic. */

    /// The first page whose bottom edge is at or below `y`.
    pub fn first_box_ending_after(&self, y: f64) -> usize {
        let mut low = 0isize;
        let mut high = self.boxes.len() as isize - 1;
        let mut found = self.boxes.len();
        while low <= high {
            let middle = ((low + high) / 2) as usize;
            if self.boxes[middle].is_none() {
                break;
            }
            // The *row's* bottom, which is what runs in order: side by side,
            // a short page ends above the tall one before it, and a search on
            // the page's own bottom stepped past a tall page still on screen.
            let bottom = self
                .row_of(middle)
                .into_iter()
                .filter_map(|index| self.boxes.get(index).copied().flatten())
                .map(|page| page.top + page.height)
                .fold(f64::MIN, f64::max);
            if bottom >= y {
                found = middle;
                high = middle as isize - 1;
            } else {
                low = middle as isize + 1;
            }
        }
        found
    }

    /// The last page whose top edge is at or above `y`; page one if none is.
    pub fn last_box_starting_above(&self, y: f64) -> usize {
        let mut low = 0isize;
        let mut high = self.boxes.len() as isize - 1;
        let mut found = 0usize;
        while low <= high {
            let middle = ((low + high) / 2) as usize;
            let Some(page) = self.boxes[middle] else {
                break;
            };
            if page.top <= y {
                found = middle;
                low = middle as isize + 1;
            } else {
                high = middle as isize - 1;
            }
        }
        found
    }

    /// The pages that should be in the document at this scroll position.
    ///
    /// Boxes are in order down the page, so the visible run is found rather
    /// than looked for: scanning all of them cost a pass over the whole
    /// document on every frame of every scroll — nine hundred pages of work to
    /// discover that three of them are on screen.
    pub fn mounted(&self, scroll_top: f64) -> Vec<usize> {
        if self.boxes.is_empty() {
            return Vec::new();
        }
        // One page is laid out and the rest of `boxes` is empty, so there is
        // nothing to search for.
        if self.mode == Mode::Paged {
            let index = self.current.clamp(1, self.sizes.len()) - 1;
            return self
                .row_of(index)
                .into_iter()
                .filter(|&index| self.boxes[index].is_some())
                .collect();
        }
        let height = self.viewport.height;
        let from = scroll_top - height * OVERSCAN;
        let to = scroll_top + height * (1.0 + OVERSCAN);
        let mut wanted = Vec::new();
        let mut index = self.first_box_ending_after(from);
        // From the start of its row: the search answers with any page of it.
        if index < self.boxes.len() {
            index = self.row_of(index)[0];
        }
        while index < self.boxes.len() {
            let Some(page) = self.boxes[index] else { break };
            if page.top > to {
                break;
            }
            wanted.push(index);
            index += 1;
        }
        wanted
    }

    /// Which page the reader is on: the row a third of the way down the
    /// window, and then the left-hand page of it — two pages standing side by
    /// side share a top, so the search finds the right-hand one, and a reader
    /// looking at a spread is on the page it opens at. One-based.
    pub fn page_at(&self, scroll_top: f64) -> usize {
        if self.boxes.is_empty() {
            return 1;
        }
        // In paged mode nothing is derived from the scroll offset: the page
        // the reader is on is the page that is laid out, and it changes by
        // being turned.
        if self.mode == Mode::Paged {
            return self.row_of(self.current.clamp(1, self.sizes.len()) - 1)[0] + 1;
        }
        // **The end of the document is on its last page**, however short the
        // last row: a probe a third of the way down stops above a two-up
        // deck's final row, and the count never reached the end.
        let max = self.max_scroll();
        if max > 0.0 && scroll_top >= max - 1.0 {
            return self.row_of(self.sizes.len() - 1)[0] + 1;
        }
        let probe = scroll_top + self.viewport.height * 0.35;
        self.row_of(self.last_box_starting_above(probe))[0] + 1
    }

    /// Where the reader is, in a form that survives a relayout.
    pub fn anchor(&self, scroll_top: f64) -> Anchor {
        if self.boxes.is_empty() {
            return Anchor {
                page: 1,
                offset: 0.0,
            };
        }
        let index = match self.mode {
            Mode::Paged => self.current.clamp(1, self.sizes.len()) - 1,
            // The tallest page of the row, so the offset is a fraction of
            // something the scroll position is actually inside: beside a
            // short page, two thirds down the tall one clamped to the short
            // one's bottom and came back well above where the reader was.
            Mode::Continuous => {
                let mut found = self.last_box_starting_above(scroll_top);
                // **In the space above a page is on that page**, because that
                // is where landing on it puts the scroll: read as the bottom of
                // the page before, going to page 5 twice was a jump from 4.
                let next = self.row_of(found).last().map_or(found, |&last| last + 1);
                if let Some(Some(page)) = self.boxes.get(next) {
                    if scroll_top >= page.top - page.above - 0.5 {
                        found = next;
                    }
                }
                self.row_of(found)
                    .into_iter()
                    .filter(|&index| self.boxes.get(index).copied().flatten().is_some())
                    .max_by(|&a, &b| {
                        let height = |i: usize| self.boxes[i].map_or(0.0, |p| p.height);
                        height(a).total_cmp(&height(b))
                    })
                    .unwrap_or(found)
            }
        };
        let Some(page) = self.boxes[index] else {
            return Anchor {
                page: index + 1,
                offset: 0.0,
            };
        };
        Anchor {
            page: index + 1,
            offset: ((scroll_top - page.top) / page.height.max(1.0)).clamp(0.0, 1.0),
        }
    }

    /// Where the scroller has to be for a page to be at the top of the window.
    ///
    /// Landing on a page means the space above it starts at the top of the
    /// window and the page follows.
    pub fn scroll_target(&self, anchor: Anchor) -> f64 {
        let index = anchor.page.clamp(1, self.pages().max(1)) - 1;
        let Some(page) = self.box_of(index) else {
            return 0.0;
        };
        let target = page.top + anchor.offset * page.height
            - if anchor.offset == 0.0 {
                page.above
            } else {
                0.0
            };
        target.clamp(0.0, self.max_scroll())
    }

    pub fn max_scroll(&self) -> f64 {
        (self.content_height - self.viewport.height).max(0.0)
    }

    /// How far there is to go across, which is nothing at all unless the
    /// reader has zoomed past the width of the window.
    ///
    /// `#viewer` in the app is `overflow: auto` and `#pages` is
    /// `margin: 0 auto`, which is these two facts in CSS: a page narrower
    /// than the window is centred in it, and a page wider than the window
    /// scrolls. Blitz has neither — the pages are placed absolutely against a
    /// box this file sizes — so both are arithmetic here. See
    /// [`crate::app::Viewer::across`].
    pub fn max_scroll_x(&self) -> f64 {
        (self.content_width - self.viewport.width).max(0.0)
    }

    /// Where a rectangle on a page lands on the screen: CSS pixels, from the
    /// top left of that page's box.
    ///
    /// **The one place a link, a match or a mark meets the rotation and the
    /// crop.** Everything above keeps its rectangles in the page's own
    /// unturned points, which is what the renderer answered in, and they stay
    /// good through a turn and a trim because nothing wrote the turn into
    /// them. `viewer.ts` cannot do this — its link and note caches hold
    /// fractions of a *turned* page, so `rotate()` there has to throw three
    /// caches away — and the difference is that a text layer measured in
    /// percentages has to know the shape it is a percentage of.
    pub fn place_on(&self, index: usize, rect: Rect) -> Rect {
        let Some(page) = self.box_of(index) else {
            return rect;
        };
        let whole = self.sizes[index];
        let Rect {
            left,
            top,
            width,
            height,
        } = rect;
        // Turned first: a quarter clockwise takes the left edge to the top,
        // which is the same rotation `Crop::turned` does in fractions.
        let turned = match self.rotation {
            90 => Rect {
                left: whole.height - top - height,
                top: left,
                width: height,
                height: width,
            },
            180 => Rect {
                left: whole.width - left - width,
                top: whole.height - top - height,
                width,
                height,
            },
            270 => Rect {
                left: top,
                top: whole.width - left - width,
                width: height,
                height: width,
            },
            _ => rect,
        };
        // And then moved by however much of the turned page the crop took off
        // its top and its left.
        let size = self.whole_size_of(index);
        let (offset_x, offset_y) = match self.crop {
            Some(crop) => (crop.x * size.width, crop.y * size.height),
            None => (0.0, 0.0),
        };
        Rect {
            left: (turned.left - offset_x) * page.scale,
            top: (turned.top - offset_y) * page.scale,
            width: turned.width * page.scale,
            height: turned.height * page.scale,
        }
    }

    /// [`Layout::place_on`] backwards: a point on the screen, in CSS pixels
    /// from the top left of a page's box, said in the page's own unturned
    /// points.
    ///
    /// The pointer is the one thing that arrives in the wrong space. Everything
    /// else starts in the page's own points and goes *out* through `place_on`
    /// once; a click starts on the screen and has to come back through the same
    /// crop and rotation, or a page turned on its side selects the words that
    /// were under the pointer before it was turned.
    ///
    /// The inverse rather than a search for the rectangle containing the point:
    /// a character's box is eight points wide, so a point in the gap between
    /// words or the leading between lines is in none of them. Inverting gives
    /// every point an answer and leaves *which character* to
    /// [`crate::select::caret_at`], which has the whole page in hand.
    pub fn unplace_on(&self, index: usize, x: f64, y: f64) -> (f64, f64) {
        let Some(page) = self.box_of(index) else {
            return (x, y);
        };
        // Out of the crop first, because it was applied last.
        let size = self.whole_size_of(index);
        let (offset_x, offset_y) = match self.crop {
            Some(crop) => (crop.x * size.width, crop.y * size.height),
            None => (0.0, 0.0),
        };
        let scale = if page.scale == 0.0 { 1.0 } else { page.scale };
        let turned_x = x / scale + offset_x;
        let turned_y = y / scale + offset_y;
        // And then back the other way round the turn.
        let whole = self.sizes[index];
        match self.rotation {
            90 => (turned_y, whole.height - turned_x),
            180 => (whole.width - turned_x, whole.height - turned_y),
            270 => (whole.width - turned_y, turned_x),
            _ => (turned_x, turned_y),
        }
    }

    /// Which page a point in the content is on, and where on it — CSS pixels
    /// from the top left of that page's box.
    ///
    /// **The point is never outside a page.** A sweep that leaves the paper —
    /// into the gutter between two pages of a spread, into the grey either
    /// side, past the last page of the document — is still a sweep, and a
    /// reader dragging down the margin means "carry on down the text". So the
    /// nearest page is chosen and the point is clamped into it, which is what
    /// makes dragging into the space below a page select to the end of it.
    ///
    /// `None` only when there is nothing laid out at all.
    pub fn page_at_point(&self, x: f64, y: f64) -> Option<(usize, f64, f64)> {
        let mut nearest: Option<(f64, usize, PageBox)> = None;
        for (index, page) in self.boxes.iter().enumerate() {
            // A hole, which is every page but one in paged mode.
            let Some(page) = page else { continue };
            let dx = if x < page.left {
                page.left - x
            } else if x > page.left + page.width {
                x - page.left - page.width
            } else {
                0.0
            };
            let dy = if y < page.top {
                page.top - y
            } else if y > page.top + page.height {
                y - page.top - page.height
            } else {
                0.0
            };
            // Vertical first, as it is in `caret_at` and for the same reason:
            // two pages side by side are one line of the document, and a
            // point between them belongs to the one it is level with rather
            // than to the row above.
            let distance = dy * 1000.0 + dx;
            if nearest.is_none_or(|(best, _, _)| distance < best) {
                nearest = Some((distance, index, *page));
            }
        }
        let (_, index, page) = nearest?;
        Some((
            index,
            (x - page.left).clamp(0.0, page.width),
            (y - page.top).clamp(0.0, page.height),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letter(count: usize) -> Vec<Size> {
        (0..count)
            .map(|_| Size {
                width: 612.0,
                height: 792.0,
            })
            .collect()
    }

    fn reader(count: usize) -> Layout {
        let mut layout = Layout::new(letter(count));
        layout.viewport = Size {
            width: 900.0,
            height: 700.0,
        };
        layout.relayout();
        layout
    }

    #[test]
    fn fit_width_uses_the_whole_window() {
        let layout = reader(3);
        // No side margin in fit width: the content is exactly as wide as the
        // viewport, which is the fault the `padX` conditional exists to fix.
        assert_eq!(layout.content_width(), 900.0);
        assert_eq!(layout.box_of(0).unwrap().width, 900.0);
        assert_eq!(layout.box_of(0).unwrap().left, 0.0);
    }

    #[test]
    fn fit_page_keeps_the_margin_and_fits_the_height() {
        let mut layout = reader(3);
        layout.fit = Fit::Page;
        layout.relayout();
        let page = layout.box_of(0).unwrap();
        assert!(page.height <= 700.0 - PAD_Y * 2.0 + 0.5, "{page:?}");
        assert!(
            page.left >= PAD_X,
            "a fitted page keeps its margin: {page:?}"
        );
    }

    #[test]
    fn the_first_page_starts_below_the_top_margin() {
        let layout = reader(3);
        assert_eq!(layout.box_of(0).unwrap().top, PAD_Y);
        assert_eq!(layout.box_of(0).unwrap().above, PAD_Y);
        // And every page after it is a gap below the one before, which is the
        // other number `above` has to hold.
        assert_eq!(layout.box_of(1).unwrap().above, layout.gap);
    }

    #[test]
    fn landing_on_a_page_lands_on_the_space_above_it() {
        let layout = reader(5);
        for page in 1..=5 {
            let top = layout.scroll_target(Anchor { page, offset: 0.0 });
            let index = page - 1;
            let want = (layout.box_of(index).unwrap().top - layout.box_of(index).unwrap().above)
                .min(layout.max_scroll());
            assert!((top - want).abs() < 0.001, "page {page}: {top} vs {want}");
        }
    }

    #[test]
    fn the_searches_agree_with_a_scan() {
        let layout = reader(40);
        for y in (0..40_000).step_by(137).map(|y| y as f64) {
            let first = layout.first_box_ending_after(y);
            let scanned = layout
                .boxes()
                .iter()
                .position(|page| page.is_some_and(|page| page.top + page.height >= y))
                .unwrap_or(layout.pages());
            assert_eq!(first, scanned, "first ending after {y}");

            let last = layout.last_box_starting_above(y);
            let scanned = layout
                .boxes()
                .iter()
                .rposition(|page| page.is_some_and(|page| page.top <= y))
                .unwrap_or(0);
            assert_eq!(last, scanned, "last starting above {y}");
        }
    }

    /// Side by side, a tall page beside a short one: the short one ends far
    /// above the tall one, and the tall one is still what is on screen.
    #[test]
    fn a_tall_page_beside_a_short_one_stays_mounted() {
        let size = |width, height| Size { width, height };
        let mut layout = Layout::new(vec![
            size(300.0, 3000.0),
            size(300.0, 300.0),
            size(300.0, 300.0),
            size(300.0, 300.0),
        ]);
        layout.viewport = size(900.0, 700.0);
        layout.spread = Spread::Two;
        layout.relayout();
        let tall = layout.box_of(0).unwrap();
        let mounted = layout.mounted(tall.top + tall.height * 0.6);
        assert!(mounted.contains(&0), "{mounted:?}");
    }

    #[test]
    fn a_place_down_the_tall_page_of_a_spread_survives_a_relayout() {
        let size = |width, height| Size { width, height };
        // The tall page on the left: the last box starting above the scroll
        // position is the short one beside it.
        let mut layout = Layout::new(vec![size(300.0, 3000.0), size(300.0, 300.0)]);
        layout.viewport = size(900.0, 700.0);
        layout.spread = Spread::Two;
        layout.relayout();
        let tall = layout.box_of(0).unwrap();
        let at = tall.top + tall.height * 0.6;
        let anchor = layout.anchor(at);
        assert!(
            (layout.scroll_target(anchor) - at).abs() < 1.0,
            "{anchor:?}"
        );
    }

    #[test]
    fn only_the_pages_near_the_viewport_are_mounted() {
        let layout = reader(400);
        let mounted = layout.mounted(0.0);
        // A four-hundred page book is a handful of pages in the document, and
        // the number is bounded by the screen rather than by the book.
        assert!(mounted.len() < 8, "{} pages mounted", mounted.len());
        assert_eq!(mounted[0], 0);

        let deep = layout.scroll_target(Anchor {
            page: 200,
            offset: 0.0,
        });
        let mounted = layout.mounted(deep);
        assert!(mounted.contains(&199), "{mounted:?}");
        assert!(mounted.len() < 8, "{} pages mounted", mounted.len());

        // Every mounted page is within the overscan band, and no page outside
        // it is mounted: the two halves of the same claim.
        let from = deep - layout.viewport.height * OVERSCAN;
        let to = deep + layout.viewport.height * (1.0 + OVERSCAN);
        for (index, page) in layout.boxes().iter().flatten().enumerate() {
            let near = page.top + page.height >= from && page.top <= to;
            assert_eq!(near, mounted.contains(&index), "page {index}");
        }
    }

    #[test]
    fn the_page_being_read_is_the_one_a_third_down() {
        let layout = reader(10);
        assert_eq!(layout.page_at(0.0), 1);
        let at = layout.scroll_target(Anchor {
            page: 4,
            offset: 0.0,
        });
        assert_eq!(layout.page_at(at), 4);
    }

    /// Two-up at a small zoom: the last row is short, and the probe a third
    /// down stopped above it with the reader scrolled to the very end.
    #[test]
    fn the_end_of_the_document_is_its_last_page() {
        let mut layout = reader(40);
        layout.spread = Spread::Two;
        layout.fit = Fit::Actual;
        layout.zoom = 0.4;
        layout.relayout();
        assert_eq!(layout.page_at(layout.max_scroll()), 39);
    }

    /// Landing on a page is being on it, not at the bottom of the one before.
    #[test]
    fn landing_on_a_page_is_on_that_page() {
        let mut layout = reader(20);
        for spread in [Spread::Single, Spread::Cover] {
            layout.spread = spread;
            layout.relayout();
            for page in [2, 5, 7] {
                let landed = layout.anchor(layout.scroll_target(Anchor { page, offset: 0.0 }));
                let row = layout.row_of(page - 1);
                assert!(
                    row.contains(&(landed.page - 1)),
                    "{spread:?} {page} {landed:?}"
                );
                assert_eq!(landed.offset, 0.0, "{spread:?} {page}");
            }
        }
    }

    #[test]
    fn an_anchor_survives_a_change_of_scale() {
        let mut layout = reader(20);
        let before = layout.anchor(layout.scroll_target(Anchor {
            page: 7,
            offset: 0.25,
        }));
        assert_eq!(before.page, 7);
        layout.fit = Fit::Actual;
        layout.zoom = 2.0;
        layout.relayout();
        let after = layout.anchor(layout.scroll_target(before));
        assert_eq!(after.page, 7);
        assert!((after.offset - before.offset).abs() < 0.01, "{after:?}");
    }

    #[test]
    fn a_cover_spread_opens_the_way_a_book_does() {
        let mut layout = reader(6);
        layout.spread = Spread::Cover;
        layout.relayout();
        assert_eq!(
            layout.rows(),
            vec![vec![0], vec![1, 2], vec![3, 4], vec![5]]
        );
        // Two pages side by side share a top exactly, which is the case
        // `above` and `page_at` are both careful about.
        assert_eq!(layout.box_of(1).unwrap().top, layout.box_of(2).unwrap().top);
        assert_eq!(layout.row_of(2), vec![1, 2]);
        // The gap between them is a distance on the screen: the pair spans
        // both pages and one gap, and no more.
        let left = layout.box_of(1).unwrap();
        let right = layout.box_of(2).unwrap();
        assert_eq!(right.left - (left.left + left.width), layout.gap);
    }

    #[test]
    fn turning_the_document_turns_every_page() {
        let mut layout = reader(3);
        let upright = layout.box_of(0).unwrap();
        layout.turn(1);
        layout.relayout();
        let sideways = layout.box_of(0).unwrap();
        assert_eq!(layout.rotation, 90);
        // Fit width, so both are as wide as the window and the height is
        // what says the page turned: a letter page on its side is shorter
        // than it is wide by exactly the ratio it was taller.
        assert_eq!(sideways.width, upright.width);
        assert!(
            (sideways.height / sideways.width - 612.0 / 792.0).abs() < 0.01,
            "{sideways:?}"
        );
        // Four quarters is where it started, and the page's own size is never
        // touched: it is the document's, not the reader's.
        layout.turn(3);
        layout.relayout();
        assert_eq!(layout.rotation, 0);
        assert_eq!(layout.box_of(0).unwrap(), upright);
        assert_eq!(
            layout.size_of(0),
            Size {
                width: 612.0,
                height: 792.0
            }
        );
    }

    #[test]
    fn a_crop_takes_the_margins_off_the_layout() {
        let mut layout = reader(3);
        let whole = layout.box_of(0).unwrap();
        layout.crop = Some(Crop {
            x: 0.1,
            y: 0.2,
            width: 0.8,
            height: 0.6,
        });
        layout.relayout();
        let trimmed = layout.box_of(0).unwrap();
        // Fit width again, so the width is the window either way and the
        // shape is what moved: 612 × 0.8 by 792 × 0.6.
        assert_eq!(trimmed.width, whole.width);
        let want = (792.0 * 0.6) / (612.0 * 0.8);
        assert!(
            (trimmed.height / trimmed.width - want).abs() < 0.01,
            "{trimmed:?}"
        );
    }

    #[test]
    fn a_rectangle_on_the_page_goes_where_the_page_went() {
        let mut layout = reader(2);
        // A rectangle in the page's own points: 100 wide and 20 tall, 72 in
        // from the left and 72 down from the top. A link, in other words.
        let link = Rect {
            left: 72.0,
            top: 72.0,
            width: 100.0,
            height: 20.0,
        };

        let upright = layout.place_on(0, link);
        let scale = layout.box_of(0).unwrap().scale;
        assert!((upright.left - 72.0 * scale).abs() < 0.001, "{upright:?}");
        assert!((upright.width - 100.0 * scale).abs() < 0.001, "{upright:?}");

        // A quarter clockwise takes the left edge to the top, so what was 72
        // from the top is now 72 from the left, and the rectangle lies the
        // other way round.
        layout.turn(1);
        layout.relayout();
        let turned = layout.place_on(0, link);
        let scale = layout.box_of(0).unwrap().scale;
        assert!((turned.top - 72.0 * scale).abs() < 0.001, "{turned:?}");
        assert!((turned.width - 20.0 * scale).abs() < 0.001, "{turned:?}");
        assert!((turned.height - 100.0 * scale).abs() < 0.001, "{turned:?}");
        // And its distance from the left of the page it is now on is what its
        // distance from the *bottom* was: the page is 792 tall upright, the
        // rectangle ended 72 + 20 down it, and the turn brings the bottom
        // edge to the left.
        let page = layout.box_of(0).unwrap();
        assert!(
            (turned.left - (792.0 - 72.0 - 20.0) * scale).abs() < 0.001,
            "{turned:?} on {page:?}"
        );

        // Four quarters is where it started, to the pixel.
        layout.turn(3);
        layout.relayout();
        let back = layout.place_on(0, link);
        assert!((back.left - upright.left).abs() < 0.001, "{back:?}");
        assert!((back.top - upright.top).abs() < 0.001, "{back:?}");

        // And a crop moves it by however much came off the top and the left,
        // and by nothing else: a rectangle is not scaled by being cropped.
        layout.crop = Some(Crop {
            x: 0.1,
            y: 0.2,
            width: 0.8,
            height: 0.6,
        });
        layout.relayout();
        let cropped = layout.place_on(0, link);
        let scale = layout.box_of(0).unwrap().scale;
        assert!(
            (cropped.left - (72.0 - 0.1 * 612.0) * scale).abs() < 0.001,
            "{cropped:?}"
        );
        assert!(
            (cropped.top - (72.0 - 0.2 * 792.0) * scale).abs() < 0.001,
            "{cropped:?}"
        );
        assert!((cropped.width - 100.0 * scale).abs() < 0.001, "{cropped:?}");
    }
}
