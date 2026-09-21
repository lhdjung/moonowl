//! The panel on the left: the document's own table of contents, the pages the
//! reader has pinned, a column of thumbnails, and — while the find bar is up —
//! the search results.
//!
//! **The thumbnail cache is the mounting window**, which is why the half of
//! `sidebar.ts` that is `THUMB_CACHE`, `drawn`, `tasks`, `flights`, `trim()`
//! and an `IntersectionObserver` is not here. A thumbnail in the app is a
//! `<canvas>` living as long as the column does, so drawing one is a
//! commitment and the cap bounds it. Here it is a [`crate::page::PageWidget`]
//! on a node, so it lives as long as the node, and the node exists only while
//! its row is in view. Scrolling away gives the texture back through `Drop`.
//!
//! It is not a saving so much as the only design available: every widget in
//! the document is painted every frame whether it is on screen or not, so an
//! unmounted row is the difference between a column that costs nothing and one
//! that costs four hundred pdfium renders. Caching would buy little anyway —
//! a thumbnail is a fiftieth of a page's pixels.
//!
//! A thumbnail wears the theme for free: it is the same widget reading the
//! same [`crate::page::Chosen`], so the column and the page cannot disagree
//! about what theme is on.

use dioxus::html::geometry::WheelDelta;
use dioxus::prelude::*;
use dioxus_native::CustomWidgetAttr;

use crate::app::{Icon, Viewer};
use crate::layout::Size;
use crate::page::{Chosen, PageWidget};

/// What the panel can be showing.
///
/// Three, as the app has: Contents, Pages, and — only while the find bar is
/// up — Results. The third comes and goes with the bar rather than sitting
/// there empty, which is why `sidebar_width`'s default is wide enough for
/// three words and why the panel has to be able to fall back to one of the
/// other two when it goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Contents,
    Pages,
    Results,
}

/// How narrow and how wide the panel may be dragged. The app's own numbers,
/// and its default of 252 sits between them.
pub const MIN_WIDTH: f64 = 160.0;
pub const MAX_WIDTH: f64 = 480.0;

/// The panel width at which the tabs still have room for their words.
///
/// Three tabs of a 15px icon, a 6px gap, a word of about fifty and twelve of
/// padding, with 4px between them and eight either side: 250 is where
/// "Contents" stops fitting, which is why the default width is 252. Narrower
/// than this and the strip is three icons — see the note on `.tabs` in
/// `styles.rs` for why fading the word out instead was worse.
const TAB_LABELS_FIT: f64 = 250.0;

/// …and the width at which all *three* of them do, which is the same sum with
/// one more tab in it.
///
/// This is not what decides whether the words are shown — [`TAB_LABELS_FIT`]
/// is, and it is deliberately the two-tab number, because a Results tab that
/// arrives should not take the words off the two that were already there.
/// It decides whether the word is *faded at its end*, which is this reader's
/// stand-in for the `text-overflow: ellipsis` Blitz has not got. A fade said
/// unconditionally is a fade over every word that fits — the fault the
/// document's name in the toolbar had — and here it showed on `Contents` in a
/// panel with room for it twice over.
const TAB_LABELS_ROOMY: f64 = 273.0;

/// What is left for a picture once the column has its padding, and the space
/// under one for its page number.
pub const PAD: f64 = 10.0;
const LABEL: f64 = 18.0;
const GAP: f64 = 10.0;

/// How tall the tab strip is: `.tabs` is 8px above a 28px `.tab` and 6px
/// below it.
///
/// **The panel is the sidebar less this, and that is what the column has to
/// be scrolled against.** It was scrolled against `layout.viewport.height`,
/// which is the whole height of the row the sidebar and the document share —
/// so `max_scroll` was forty-two pixels short and the last thumbnail's foot
/// and its number could not be reached, on any document long enough to
/// scroll. Stated rather than measured because `get_client_rect` panics
/// inside the borrow every handler holds; see `Reader`'s own note on the
/// viewport.
pub const TABS: f64 = 42.0;

/// How much of a screen of thumbnails is kept mounted either side of the one
/// being looked at. `OVERSCAN` in `layout.rs` is 0.6 of a viewport and does
/// the same job for the document; this is smaller because a row is cheap to
/// draw and there are a great many of them in a screen.
const OVERSCAN: f64 = 0.4;

/// Where every thumbnail sits in the column.
///
/// The document's layout in miniature, and deliberately not the same code:
/// `Layout` is about fit modes, spreads, zoom and a page gap in CSS pixels,
/// and a column of thumbnails has none of those. What it does share is the
/// shape that matters — positions worked out once and binary-searched, rather
/// than a walk over four hundred rows on every frame of a scroll.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Column {
    /// How wide a picture is drawn, in CSS pixels.
    pub width: f64,
    /// The top of each row, in order.
    tops: Vec<f64>,
    /// How tall each picture is. The row is this plus [`LABEL`].
    heights: Vec<f64>,
    total: f64,
}

impl Column {
    /// Lay a column of `sizes` out for a panel this wide.
    pub fn new(sizes: &[Size], panel_width: f64) -> Column {
        let width = (panel_width - PAD * 2.0).max(40.0);
        let mut tops = Vec::with_capacity(sizes.len());
        let mut heights = Vec::with_capacity(sizes.len());
        let mut y = PAD;
        for size in sizes {
            // A page with no height in it is a document being awkward rather
            // than a row that should be zero pixels tall.
            let ratio = if size.width > 0.0 {
                size.height / size.width
            } else {
                1.414
            };
            let height = (width * ratio).max(1.0);
            tops.push(y);
            heights.push(height);
            y += height + LABEL + GAP;
        }
        Column {
            width,
            tops,
            heights,
            total: y - GAP + PAD,
        }
    }

    pub fn pages(&self) -> usize {
        self.tops.len()
    }

    pub fn total(&self) -> f64 {
        self.total
    }

    /// The top of a row and the height of its picture, by zero-based index.
    pub fn row(&self, index: usize) -> Option<(f64, f64)> {
        Some((*self.tops.get(index)?, self.heights[index]))
    }

    pub fn max_scroll(&self, height: f64) -> f64 {
        (self.total - height).max(0.0)
    }

    /// Which rows are in the DOM: everything in view, plus [`OVERSCAN`] of a
    /// panel either side of it. Zero-based, in order.
    ///
    /// The band is asked for by binary search rather than by scanning, for the
    /// same reason `first_box_ending_after` is a binary search: this runs on
    /// every frame of a scroll and a book has as many rows as it has pages.
    pub fn mounted(&self, scroll: f64, height: f64) -> Vec<usize> {
        if self.tops.is_empty() {
            return Vec::new();
        }
        let margin = height * OVERSCAN;
        let top = scroll - margin;
        let bottom = scroll + height + margin;
        let first = self.first_row_ending_after(top);
        let mut mounted = Vec::new();
        for index in first..self.tops.len() {
            if self.tops[index] > bottom {
                break;
            }
            mounted.push(index);
        }
        mounted
    }

    /// The first row whose bottom edge is below `y`.
    fn first_row_ending_after(&self, y: f64) -> usize {
        let (mut low, mut high) = (0usize, self.tops.len());
        while low < high {
            let middle = (low + high) / 2;
            if self.tops[middle] + self.heights[middle] + LABEL < y {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low.min(self.tops.len().saturating_sub(1))
    }

    /// Where to scroll the column so that a page is on screen — and `None`
    /// when it already is.
    ///
    /// Returning nothing when there is nothing to do is the whole behaviour:
    /// the column follows the document as it is read, and a column that
    /// re-centred on every page turn would take a reader who had scrolled it
    /// somewhere else straight back. `revealCurrentThumb` in `sidebar.ts`
    /// asks the same question of two rectangles.
    pub fn reveal(&self, index: usize, scroll: f64, height: f64) -> Option<f64> {
        let (top, picture) = self.row(index)?;
        let bottom = top + picture + LABEL;
        if top >= scroll && bottom <= scroll + height {
            return None;
        }
        // Centred, which is what the app asks the browser for, and clamped.
        let wanted = top - (height - (bottom - top)) / 2.0;
        Some(wanted.clamp(0.0, self.max_scroll(height)))
    }
}

/// The panel itself.
///
/// It takes the whole `Viewer` signal rather than fifteen props, which the
/// reader's other components do not need to and this one does: it draws four
/// lists that are each a different slice of the same state and changes six
/// things about it. Passing them one at a time would be the same coupling
/// written out longer.
#[component]
pub fn Sidebar(mut viewer: Signal<Viewer>, chosen: Chosen) -> Element {
    let held = viewer.read();
    let tab = held.tab;
    let width = held.sidebar_width;
    let page = held.page();
    // In every thumbnail's key, with which document it is of: the colours it
    // wears. A new draft of the same document is drawn in place — see
    // `Chosen::show` in `page.rs`.
    let worn = chosen.get().key();
    let opened = held.opened;
    // What a tab's icon is drawn in. See `Icon` in `app.rs`: an inline `<svg>`
    // reaches usvg with no cascade behind it, so the shade has to travel with
    // it rather than being inherited from the button.
    let wearing = held.palette();
    let (ink, ink_on) = (
        crate::palette::hex(wearing.muted()),
        crate::palette::hex(wearing.text),
    );
    let headings = held.headings.clone();
    let marks: Vec<(usize, String)> = held
        .store
        .marks()
        .iter()
        .map(|mark| {
            let page = mark.page as usize;
            let title = if mark.title.is_empty() {
                format!("Page {page}")
            } else {
                mark.title.clone()
            };
            (page, title)
        })
        .collect();
    // Every mark in the document, and whatever the journal is holding beside
    // it. Read here rather than in the rows below because it costs a page of
    // text per mark — see `Viewer::markup_rows` — and the panel is redrawn on
    // every scroll frame.
    let markup = held.markup_rows();
    let marked_up = !markup.is_empty();
    // How many passages the journal is holding that the document itself has
    // lost — a paper recompiled by LaTeX is a new file and the annotations
    // went with it. The offer is a button and never a thing that happens on
    // its own. See [`crate::app::Viewer::restore_markup`].
    let adrift = held.restorable();
    let column = held.column.clone();
    // Worked out once rather than per row: the answer is the same for every
    // one of them, and a document's outline can be long.
    let current_heading = heading_for(&headings, page);
    let thumb_scroll = held.thumb_scroll;
    let panel_height = held.thumb_panel();
    let mounted = column.mounted(thumb_scroll, panel_height);
    // The third tab is here only while the find bar is: a Results tab with
    // nothing behind it is a tab that answers a question nobody asked.
    let searching = held.find_open;
    // See the note on `.tabs` below: an icon and a word, or an icon.
    let labelled = width >= TAB_LABELS_FIT;
    // Whether a word could be cut: three tabs in a panel narrow enough that
    // they do not all fit. See [`TAB_LABELS_ROOMY`].
    let tight = searching && width < TAB_LABELS_ROOMY;
    let results = if searching {
        held.search.results(crate::search::RESULT_LIMIT)
    } else {
        Vec::new()
    };
    // **What a page is called, not where it is in the file**, which is what
    // `this.viewer.label(result.page)` reads in `sidebar.ts` and is the whole
    // point of a document that numbers its own front matter: a match on the
    // third page of a book that calls it iii is cited as iii. Taken here
    // rather than in the row, because the rows are built after the reader is
    // let go of.
    let result_labels: Vec<String> = results
        .iter()
        .map(|result| held.label(result.page))
        .collect();
    let result_at = held.search.state().at;
    let result_total = held.search.state().total;
    let scanning = held.search.state().scanning;
    drop(held);

    let rows: Vec<(usize, f64, f64)> = mounted
        .iter()
        .filter_map(|&index| column.row(index).map(|(top, height)| (index, top, height)))
        .collect();

    rsx! {
        div { class: "sidebar", style: "width: {width}px;",
            // The edge, picked up to widen or narrow the panel. It cannot
            // track its own drag — widening moves the pointer out from under
            // it — so `onmousedown` only starts one; `app.rs` puts the
            // `onmousemove` and `onmouseup` on the root, which is the one
            // ancestor the pointer cannot leave.
            div {
                class: "sidebar-resize",
                onmousedown: move |event| {
                    let x = event.client_coordinates().x;
                    viewer.write().start_resize_sidebar(x);
                },
            }
            // **The word goes before the icon does**, which is the app's own
            // rule for this strip said with a number instead of with
            // `text-overflow`. Three tabs of an icon, a gap and a word need
            // about 250px between them; below that the mask meant to fade the
            // last few letters was fading the whole word, and three tabs
            // reading "C", "P", "R" are three tabs nobody can tell apart. An
            // icon on its own is still the thing it is a drawing of.
            div { class: if tight { "tabs tight" } else { "tabs" },
                // Before the root's own press, which closes the find bar and
                // a borrowed panel with it. The Results tab stops its press
                // on the way here, so only Contents and Pages adopt.
                onmousedown: move |_| viewer.write().adopt_sidebar(),
                button {
                    class: if tab == Tab::Contents { "tab on" } else { "tab" },
                    "data-tab": "contents",
                    onclick: move |_| viewer.write().show_tab(Tab::Contents),
                    Icon { name: "contents", stroke: if tab == Tab::Contents { ink_on.clone() } else { ink.clone() } }
                    if labelled { span { class: "tab-label", "Contents" } }
                }
                button {
                    class: if tab == Tab::Pages { "tab on" } else { "tab" },
                    "data-tab": "pages",
                    onclick: move |_| viewer.write().show_tab(Tab::Pages),
                    Icon { name: "pages", stroke: if tab == Tab::Pages { ink_on.clone() } else { ink.clone() } }
                    if labelled { span { class: "tab-label", "Pages" } }
                }
                if searching {
                    button {
                        class: if tab == Tab::Results { "tab on" } else { "tab" },
                        "data-tab": "results",
                        // `#tab-results` in `FIND_KEEPS_OPEN` — see the root's
                        // `onmousedown` in `app.rs`. This tab and the panel
                        // under it are the search seen larger, so reaching
                        // into either is not reaching past the bar.
                        onmousedown: move |event| event.stop_propagation(),
                        onclick: move |_| viewer.write().show_tab(Tab::Results),
                        Icon { name: "search", stroke: if tab == Tab::Results { ink_on.clone() } else { ink.clone() } }
                        if labelled { span { class: "tab-label", "Results" } }
                    }
                }
            }
            if tab == Tab::Results {
                div { class: "panel results",
                    // `#results-panel` in `FIND_KEEPS_OPEN` — see the Results
                    // tab above, and the root's `onmousedown` in `app.rs`.
                    // Picking a result would otherwise close the search that
                    // found it, and the list with it.
                    onmousedown: move |event| event.stop_propagation(),
                    if results.is_empty() {
                        p { class: "sidebar-empty",
                            if scanning { "Searching…" } else { "No matches." }
                        }
                    } else {
                        // The count is above the list rather than in it,
                        // because a list that is longer than it says is a
                        // list somebody scrolls to the end of to find out.
                        p { class: "results-count",
                            if result_total > results.len() {
                                "{results.len()} of {result_total} matches"
                            } else if result_total == 1 {
                                "1 match"
                            } else {
                                "{result_total} matches"
                            }
                        }
                        for (row, result) in results.iter().enumerate() {
                            {
                                let at = result.at;
                                let current = result_at == Some(at);
                                let (page, before, hit, after) = (
                                    result.page,
                                    result.before.clone(),
                                    result.hit.clone(),
                                    result.after.clone(),
                                );
                                let named = result_labels[row].clone();
                                rsx! {
                                    button {
                                        key: "{at}",
                                        class: if current { "result current" } else { "result" },
                                        "data-result": "{at}",
                                        "data-page": "{page}",
                                        onclick: move |_| viewer.write().go_to_result(at),
                                        span { class: "result-page", "{named}" }
                                        span { class: "result-line",
                                            span { class: "result-before", "{before}" }
                                            span { class: "result-hit", "{hit}" }
                                            span { class: "result-after", "{after}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            } else if tab == Tab::Contents {
                div { class: "panel contents",
                    // The reader's own marks, above the document's contents.
                    // Above rather than beside, and for the reason
                    // `showMarks` gives: there are never many, and a section
                    // of four over a chapter list of two hundred is the right
                    // way round.
                    if !marks.is_empty() {
                        div { class: "marks",
                            p { class: "marks-title", "Marked" }
                            for (marked, title) in marks {
                                div { class: "mark",
                                    button {
                                        class: "mark-go",
                                        onclick: move |_| viewer.write().go_to_page(marked),
                                        "{title}"
                                    }
                                    button {
                                        class: "mark-drop",
                                        "aria-label": "Remove the mark on page {marked}",
                                        onclick: move |_| { viewer.write().mark_page(marked); },
                                        "×"
                                    }
                                }
                            }
                        }
                    }
                    // The passages the reader has marked, under the pages
                    // they pinned and above the document's own contents.
                    // `showHighlights` in the app puts them in the same
                    // panel and in the same order, and for the reason the
                    // marks are up there: there are never many, and this is
                    // the reader's own account of the document rather than
                    // the document's.
                    if marked_up {
                        div { class: "markup",
                            p { class: "marks-title", "Marked up" }
                            if adrift > 0 {
                                button {
                                    class: "markup-restore",
                                    onclick: move |_| {
                                        viewer.write().restore_markup();
                                    },
                                    if adrift == 1 {
                                        "Put 1 passage back"
                                    } else {
                                        "Put {adrift} passages back"
                                    }
                                }
                            }
                            for row in markup {
                                {
                                    let (page, colour) = (row.page, row.color.clone());
                                    let quote = if row.quote.is_empty() {
                                        format!("Page {page}")
                                    } else {
                                        row.quote.clone()
                                    };
                                    let beside = matches!(row.key, crate::app::MarkKey::Beside(_));
                                    let key = row.key.clone();
                                    rsx! {
                                        div { class: "mark markup-row", key: "{row.key:?}",
                                            span {
                                                class: "markup-dot",
                                                style: "background: {colour};",
                                            }
                                            button {
                                                class: "mark-go",
                                                "data-page": "{page}",
                                                onclick: move |_| viewer.write().go_to_page(page),
                                                "{quote}"
                                                // What the file cannot carry
                                                // says so on the row rather
                                                // than in a section of its
                                                // own: to a reader it is one
                                                // kind of thing, and this is
                                                // the one fact about it worth
                                                // knowing.
                                                if beside {
                                                    span { class: "markup-beside", "beside the document" }
                                                }
                                            }
                                            button {
                                                class: "mark-drop",
                                                "aria-label": "Remove this mark",
                                                onclick: move |_| {
                                                    viewer.write().remove_markup(&key);
                                                },
                                                "×"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if headings.is_empty() && !marked_up {
                        p { class: "sidebar-empty", "This document has no table of contents." }
                    } else if headings.is_empty() {
                    } else {
                        for (at, heading) in headings.iter().enumerate() {
                            {
                                let indent = 8.0 + heading.depth as f64 * 14.0;
                                let target = heading.page;
                                let current = current_heading == Some(at);
                                let title = heading.title.clone();
                                rsx! {
                                    button {
                                        key: "{at}",
                                        class: if current { "outline-item current" } else { "outline-item" },
                                        style: "padding-left: {indent}px;",
                                        "data-page": "{target.unwrap_or(0)}",
                                        onclick: move |_| {
                                            if let Some(page) = target {
                                                viewer.write().go_to_page(page);
                                            }
                                        },
                                        "{title}"
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                div {
                    // Not `.pages`, which is the document's own container and
                    // is what the harness reads the scroll offset off. Two
                    // nodes answering one selector is a test that silently
                    // asks the wrong one.
                    class: "panel thumb-column",
                    "data-thumb-scroll": "{thumb_scroll}",
                    onwheel: move |event| {
                        let delta = -match event.delta() {
                            WheelDelta::Pixels(delta) => delta.y,
                            WheelDelta::Lines(delta) => delta.y * 60.0,
                            WheelDelta::Pages(delta) => delta.y * panel_height,
                        };
                        viewer.write().scroll_thumbs(delta);
                    },
                    div {
                        class: "thumbs",
                        style: "height: {column.total()}px;",
                        for (index, top, height) in rows {
                            Thumb {
                                // What `keyFor()` is, in miniature: the page,
                                // the size it is drawn at, and the theme. See
                                // `page.rs` — the key is what gives the old
                                // texture back.
                                key: "{index}:{column.width}x{height}:{worn}:{opened}",
                                chosen: chosen.clone(),
                                viewer,
                                index,
                                top: top - thumb_scroll,
                                left: PAD,
                                width: column.width,
                                height,
                                current: index + 1 == page,
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Which heading the reader is under: the last one at or before this page.
///
/// `setPage` in `sidebar.ts` walks the list for the same answer, and
/// `sectionFor` walks it again to name a mark. One function, asked twice.
pub fn heading_for(headings: &[crate::render::Heading], page: usize) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for (at, heading) in headings.iter().enumerate() {
        let Some(target) = heading.page else { continue };
        if target <= page && best.is_none_or(|(_, best)| target >= best) {
            best = Some((at, target));
        }
    }
    best.map(|(at, _)| at)
}

/// One thumbnail, in its place.
#[component]
fn Thumb(
    chosen: Chosen,
    mut viewer: Signal<Viewer>,
    index: usize,
    top: f64,
    left: f64,
    width: f64,
    height: f64,
    current: bool,
) -> Element {
    // The same write-once attribute the document's pages use, for the same
    // reason: a widget is handed to Blitz once and cannot be given new props,
    // so `use_hook` is what keeps a row that merely moved from building a
    // second one.
    let widget = use_hook(|| {
        let shell = dioxus_core::try_consume_context::<
            std::sync::Arc<dyn blitz_traits::shell::ShellProvider>,
        >();
        // `View::WHOLE`: a thumbnail is the page as the document has it,
        // not as the reader has turned or trimmed it. The app draws its
        // thumbnails through a viewport of their own for the same reason —
        // the column is a map of the file, and a map that turns with the
        // reader is one they have to re-learn.
        CustomWidgetAttr::new(PageWidget::new(
            index,
            crate::layout::View::WHOLE,
            chosen.clone(),
            shell,
        ))
    });
    let number = index + 1;

    rsx! {
        button {
            class: if current { "thumb current" } else { "thumb" },
            "data-thumb": "{number}",
            style: "position: absolute; top: {top}px; left: {left}px; width: {width}px;",
            onclick: move |_| viewer.write().go_to_page(number),
            div {
                class: "thumb-picture",
                style: "width: {width}px; height: {height}px;",
                object {
                    "data": widget,
                    // A widget laid out at 0×0 is a blank window with nothing
                    // to say why, which is what `display: block` costs to
                    // avoid. See `page.rs`.
                    //
                    // **And `pointer-events: none`, which is the whole of why
                    // a thumbnail could be clicked at all.** A press does land
                    // on a widget — that is how a sweep over a page begins —
                    // but Blitz never turns one into a click, so the button
                    // around the picture heard nothing and only the number
                    // under it answered: a target eighteen pixels tall in a
                    // row of three hundred. Made transparent to the pointer,
                    // the press lands on `.thumb-picture` and the click
                    // reaches the button, which is the whole row.
                    style: "display: block; pointer-events: none; width: {width}px; height: {height}px;",
                }
            }
            span { class: "thumb-number", "{number}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes(pages: usize) -> Vec<Size> {
        (0..pages)
            .map(|_| Size {
                width: 612.0,
                height: 792.0,
            })
            .collect()
    }

    /// The band that is mounted holds every row in view and nothing far from
    /// it — which is the whole of what replaces `THUMB_CACHE`, and so the one
    /// property in this file worth asserting directly.
    #[test]
    fn the_mounted_band_covers_the_view_and_stops() {
        let column = Column::new(&sizes(400), 252.0);
        let height = 800.0;
        for scroll in [0.0, 1_000.0, 40_000.0, column.max_scroll(height)] {
            let mounted = column.mounted(scroll, height);
            assert!(!mounted.is_empty(), "nothing mounted at {scroll}");
            // Every row that is actually in view is in the band.
            for index in 0..column.pages() {
                let (top, picture) = column.row(index).expect("a row");
                let visible = top < scroll + height && top + picture > scroll;
                if visible {
                    assert!(
                        mounted.contains(&index),
                        "row {index} is in view at {scroll}"
                    );
                }
            }
            // And nothing more than a screen of overscan either side of it.
            let reach = height * (1.0 + OVERSCAN * 2.0) + 200.0;
            for &index in &mounted {
                let (top, picture) = column.row(index).expect("a row");
                assert!(
                    top + picture > scroll - reach && top < scroll + reach,
                    "row {index} is nowhere near the view at {scroll}",
                );
            }
            // Contiguous and in order, which is what makes it a band.
            assert!(mounted.windows(2).all(|pair| pair[1] == pair[0] + 1));
        }
    }

    /// A column of four hundred rows mounts a few tens of them, whatever the
    /// document is. This is the memory claim in the module comment, stated as
    /// a number.
    #[test]
    fn a_long_document_mounts_a_screenful() {
        let column = Column::new(&sizes(400), 252.0);
        let mounted = column.mounted(20_000.0, 800.0);
        assert!(mounted.len() < 20, "{} rows mounted", mounted.len());
    }

    /// The column follows the document only when it has to.
    #[test]
    fn revealing_a_row_already_in_view_does_nothing() {
        let column = Column::new(&sizes(50), 252.0);
        let height = 800.0;
        assert_eq!(column.reveal(0, 0.0, height), None);
        let far = column
            .reveal(40, 0.0, height)
            .expect("row 40 is not in view");
        assert!(far > 0.0 && far <= column.max_scroll(height));
        // And once it is there, it stays.
        assert_eq!(column.reveal(40, far, height), None);
    }

    /// A page with nothing in it does not take the column apart, which a
    /// document being awkward is entitled to try.
    #[test]
    fn a_page_of_no_size_still_gets_a_row() {
        let column = Column::new(
            &[Size {
                width: 0.0,
                height: 0.0,
            }],
            252.0,
        );
        let (_, height) = column.row(0).expect("a row");
        assert!(height > 0.0);
        assert!(column.total() > 0.0);
    }
}
