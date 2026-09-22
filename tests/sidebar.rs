//! The panel on the left, driven headlessly.
//!
//! `sidebar.test.mjs` in the app is five tests and every one of them is about
//! the thumbnail cache: what is drawn, what is given back, what a theme change
//! redraws, what a closed panel does not draw. None of those five has anything
//! to assert here, because there is no cache — a thumbnail belongs to its row
//! and a row exists only while it is in view. What replaces them is the one
//! question underneath all five: *is the column's mounting window doing its
//! job*, which `src/sidebar.rs` asks of the geometry directly and this file
//! asks of the DOM.
//!
//! The rest is the half the app's own file never covered: the table of
//! contents, the marks, and the document getting narrower when the panel
//! opens.

use moonowl::fixture;
use moonowl::harness::{Options, Reader};

/// A twelve-page document that carries its own table of contents. See
/// `src/fixture.rs`: written in Rust so that `cargo test` needs cargo and
/// nothing else.
fn with_contents() -> Reader {
    Reader::open(&fixture::contents_pdf())
}

/// Four hundred pages of plain text, and no outline at all — which is the
/// commoner document of the two.
fn book() -> Reader {
    Reader::open(&Reader::book())
}

#[test]
fn the_panel_opens_and_shuts_and_is_remembered() {
    let config;
    {
        let mut reader = with_contents();
        assert_eq!(reader.state().sidebar, None, "shut, which is the default");
        reader.press_chord("mod+b");
        assert_eq!(reader.state().sidebar.as_deref(), Some("contents"));
        reader.press_chord("mod+b");
        assert_eq!(reader.state().sidebar, None);
        reader.press_chord("mod+b");
        config = reader.config.clone();
    }
    // A setting, like every other: the next run opens on it.
    let again = Reader::open_with(
        &fixture::contents_pdf(),
        Options {
            config,
            ..Default::default()
        },
    );
    assert_eq!(again.state().sidebar.as_deref(), Some("contents"));
}

#[test]
fn a_document_that_carries_its_contents_lists_them() {
    let mut reader = with_contents();
    reader.press_chord("mod+b");
    let rows = reader.harness.query_all(".outline-item");
    let expected = fixture::expected_headings();
    assert_eq!(rows.len(), expected.len(), "one row per heading");
    // The titles, in the document's own order.
    let titles: Vec<String> = rows
        .iter()
        .map(|node| {
            reader
                .harness
                .base()
                .get_node(*node)
                .map(|node| node.text_content())
                .unwrap_or_default()
        })
        .collect();
    let wanted: Vec<String> = expected.iter().map(|(title, ..)| title.clone()).collect();
    assert_eq!(titles, wanted);
    // And a nested heading is indented further than its parent, which is the
    // whole of what `depth` is for. Measured off the pixels rather than off
    // the box: the rows are all the width of the panel and the indent is
    // padding, so their rectangles are identical and only the ink moves.
    let bands: Vec<(u32, u32, u32, u32)> = rows[..2]
        .iter()
        .map(|node| {
            let rect = reader.harness.layout_rect_of(*node);
            (
                rect.x as u32,
                rect.y as u32 + 2,
                (rect.x + rect.width) as u32,
                (rect.y + rect.height) as u32 - 2,
            )
        })
        .collect();
    let shot = reader.screenshot();
    let parent = shot
        .leftmost_ink(bands[0])
        .expect("\"Front matter\" is drawn");
    let child = shot.leftmost_ink(bands[1]).expect("\"Preface\" is drawn");
    assert!(
        child > parent,
        "a heading under another starts further in: {child} against {parent}",
    );
}

#[test]
fn a_document_with_no_contents_says_so() {
    let mut reader = book();
    reader.press_chord("mod+b");
    // It opens on the pages rather than on an empty box — and the sentence is
    // there behind the other tab rather than nowhere.
    assert_eq!(reader.state().sidebar.as_deref(), Some("pages"));
    reader.click(".tab[data-tab='contents']");
    assert_eq!(
        reader.harness.text_content(".sidebar-empty"),
        "This document has no table of contents."
    );
}

#[test]
fn clicking_a_heading_goes_to_its_page() {
    let mut reader = with_contents();
    reader.press_chord("mod+b");
    assert_eq!(reader.state().page, 1);
    // "Chapter Two", which the fixture puts on page 8.
    let rows = reader.harness.query_all(".outline-item");
    let (title, _, page) = fixture::expected_headings()[6].clone();
    assert_eq!(title, "Chapter Two");
    let rect = reader.harness.layout_rect_of(rows[6]);
    let (x, y) = rect.center();
    reader.click_at(x, y);
    assert_eq!(reader.state().page, page);
}

#[test]
fn the_heading_the_reader_is_under_is_the_one_marked() {
    let mut reader = with_contents();
    reader.press_chord("mod+b");
    // Page 1 is under the first heading.
    assert_eq!(
        reader.harness.text_content(".outline-item.current"),
        "Front matter"
    );
    // Page 8 is "Chapter Two", and page 9 is still under it — a heading holds
    // until the next one starts.
    reader.press_chord("mod+b");
    for _ in 0..8 {
        reader.press("l");
    }
    reader.press_chord("mod+b");
    assert_eq!(reader.state().page, 9);
    assert_eq!(
        reader.harness.text_content(".outline-item.current"),
        "Chapter Two"
    );
}

/// **A thumbnail is the page, and nothing the reader has done to it.** The
/// selection and the link tint are fractions of the page *as shown* — turned,
/// trimmed — and a thumbnail is the whole page, so they landed somewhere else
/// on it, and only on the pages the document had mounted.
#[test]
fn a_selection_is_not_painted_into_the_thumbnails() {
    let mut reader = Reader::open(&fixture::prose_pdf());
    reader.press_chord("mod+b");
    reader.click(".tab[data-tab='pages']");
    let rect = reader.harness.layout_rect(".thumb-picture");
    let thumb = (
        rect.x as u32 + 4,
        rect.y as u32 + 4,
        (rect.x + rect.width) as u32 - 4,
        (rect.y + rect.height) as u32 - 4,
    );
    let before = reader.screenshot().mean(thumb);

    reader.sweep_page(1, (0.10, 0.108), (0.55, 0.108));
    assert!(!reader.state().empty, "the sweep selected something");
    let after = reader.screenshot().mean(thumb);
    assert_eq!(
        before.map(|c| c.round()),
        after.map(|c| c.round()),
        "the selection was painted into the thumbnail"
    );
}

#[test]
fn the_column_mounts_what_is_in_view_and_nothing_else() {
    let mut reader = book();
    reader.press_chord("mod+b");
    reader.click(".tab[data-tab='pages']");
    let first = reader.state().thumbs;
    assert!(!first.is_empty(), "the column is drawn");
    assert!(
        first.len() < 20,
        "four hundred pages, {} thumbnails: the column is a window, not a list",
        first.len()
    );
    assert_eq!(first[0], 1, "and it starts at the top");

    // Scrolling it moves the window rather than adding to it. This is the
    // whole of what `THUMB_CACHE` was for in the app, and there is nothing to
    // cap because nothing accumulates.
    reader.wheel_over(".panel.thumb-column", 3_000.0);
    let later = reader.state().thumbs;
    assert!(!later.is_empty());
    assert!(
        later.len() < 20,
        "{} thumbnails after scrolling",
        later.len()
    );
    assert!(
        later[0] > first[0],
        "the column moved: {} to {}",
        first[0],
        later[0]
    );
    assert!(
        later.windows(2).all(|pair| pair[1] == pair[0] + 1),
        "and it is still a band"
    );
}

#[test]
fn the_column_follows_the_document_and_stops_there() {
    let mut reader = book();
    reader.press_chord("mod+b");
    reader.click(".tab[data-tab='pages']");
    assert!(reader.state().thumbs.contains(&1));
    // Read a long way into the book: the column comes with.
    for _ in 0..40 {
        reader.press(" ");
    }
    let page = reader.state().page;
    assert!(page > 8, "the reader has moved: page {page}");
    assert!(
        reader.state().thumbs.contains(&page),
        "the thumbnail for page {page} is in the column: {:?}",
        reader.state().thumbs
    );
}

/// **And the foot of the last one can be reached.** The column was scrolled
/// against `layout.viewport.height`, which is the whole height of the row the
/// panel and the document share — so the end of the list was the tab strip's
/// forty-two pixels short, and the last thumbnail's lower edge and its number
/// were cut off on every document long enough to scroll. See
/// `sidebar::TABS`.
#[test]
fn the_last_thumbnail_is_whole_at_the_end_of_the_column() {
    let mut reader = book();
    reader.press_chord("mod+b");
    reader.click(".tab[data-tab='pages']");
    // As far as it goes: `scroll_thumbs` clamps, so one wheel is enough.
    reader.wheel_over(".panel.thumb-column", 1_000_000.0);
    let last = *reader.state().thumbs.last().expect("a thumbnail");
    assert_eq!(
        last,
        reader.state().pages,
        "the column reaches the end of the book"
    );

    let panel = reader.harness.layout_rect(".panel.thumb-column");
    let number = *reader
        .harness
        .query_all(".thumb-number")
        .last()
        .expect("a page number under the last thumbnail");
    let rect = reader.harness.layout_rect_of(number);
    assert!(
        rect.y + rect.height <= panel.y + panel.height + 0.5,
        "the last number ends at {}, the panel at {}",
        rect.y + rect.height,
        panel.y + panel.height,
    );
}

#[test]
fn a_thumbnail_is_a_picture_of_its_page() {
    let mut reader = book();
    reader.press_chord("mod+b");
    reader.click(".tab[data-tab='pages']");
    let rect = reader.harness.layout_rect(".thumb-picture");
    let shot = reader.screenshot();
    let band = (
        rect.x as u32 + 4,
        rect.y as u32 + 4,
        (rect.x + rect.width) as u32 - 4,
        (rect.y + rect.height) as u32 - 4,
    );
    // Paper, with something on it. The fixture is one line of text near the
    // top of a page, so a thumbnail of it is mostly white and not entirely.
    let mean = shot.mean(band);
    assert!(mean[0] > 200.0, "a thumbnail is paper: {mean:?}");
    let ink = shot.unlike([255, 255, 255], band);
    assert!(ink > 0.0005, "…with ink on it: {ink:.4} of the picture");
}

#[test]
fn the_document_gets_narrower_when_the_panel_opens() {
    let mut reader = book();
    let wide = reader.harness.layout_rect(".page").width;
    reader.press_chord("mod+b");
    let narrow = reader.harness.layout_rect(".page").width;
    let panel = reader.harness.layout_rect(".sidebar").width;
    assert!(panel > 100.0, "the panel has a width: {panel}");
    assert!(
        (wide - narrow - panel).abs() < 2.0,
        "the page gives up exactly what the panel takes: {wide} - {narrow} against {panel}",
    );
}

#[test]
fn the_panel_can_be_dragged_wider_and_narrower_and_the_width_survives() {
    let config;
    {
        let mut reader = book();
        reader.press_chord("mod+b");
        let start = reader.state().sidebar_width;
        // Border-box, so one pixel over the setting for the border itself.
        assert!((start - 252.0).abs() < 2.0, "the shipped default: {start}");

        reader.drag_sidebar_edge(100.0);
        let wider = reader.state().sidebar_width;
        assert!(
            (wider - (start + 100.0)).abs() < 2.0,
            "widened by exactly what the pointer moved: {start} -> {wider}",
        );

        // Past `MAX_WIDTH`, which is `sidebar::MAX_WIDTH` and not repeated
        // here so the test cannot drift from the number that actually
        // clamps. Not dragged past the harness's own 1100px window: a
        // pointer that leaves the window stops being tracked at all — real
        // windows have the same limit, and the window is wide enough that
        // `MAX_WIDTH` is reached well inside it.
        reader.drag_sidebar_edge(500.0);
        assert!(
            (reader.state().sidebar_width - moonowl::sidebar::MAX_WIDTH).abs() < 2.0,
            "clamped at the wide end: {}",
            reader.state().sidebar_width,
        );

        // And the narrow end, from the other direction: picked up at the
        // edge's *current* position, which `drag_sidebar_edge` finds itself
        // rather than assuming where the last drag left it.
        reader.drag_sidebar_edge(-400.0);
        assert!(
            (reader.state().sidebar_width - moonowl::sidebar::MIN_WIDTH).abs() < 2.0,
            "clamped at the narrow end: {}",
            reader.state().sidebar_width,
        );

        reader.drag_sidebar_edge(60.0);
        config = reader.config.clone();
    }
    let width = Reader::open_with(
        &Reader::book(),
        Options {
            config,
            ..Default::default()
        },
    )
    .state()
    .sidebar_width;
    let expected = moonowl::sidebar::MIN_WIDTH + 60.0;
    assert!(
        (width - expected).abs() < 2.0,
        "the dragged width is a setting too: {width}, wanted near {expected}",
    );
}

/// `drag_sidebar` moves the panel's own width and nothing else — see its own
/// doc comment on why. Every pixel a full relayout passed through used to be
/// a fresh render and texture upload for every mounted page, with a blank
/// `.page` (white, whatever the theme) to show in between: the flicker a
/// reader watching the drag actually saw.
#[test]
fn the_document_does_not_relayout_until_the_drag_ends() {
    let mut reader = book();
    reader.press_chord("mod+b");
    let before = reader.harness.layout_rect(".page");

    let (x, y) = reader.harness.center_of(".sidebar-resize");
    reader.harness.mouse_down_at(x, y);
    reader.carry(x + 150.0, y);
    reader.settle();
    let mid_drag = reader.harness.layout_rect(".page");
    assert_eq!(
        (mid_drag.width, mid_drag.height),
        (before.width, before.height),
        "the page does not move while the pointer is still down",
    );

    reader.harness.mouse_up_at(x + 150.0, y);
    reader.settle();
    let after = reader.harness.layout_rect(".page");
    assert!(
        after.width < before.width,
        "and relays out exactly once the pointer lets go: {} -> {}",
        before.width,
        after.width,
    );
}

/// **The thumbnails are the exception, and they follow the pointer.** The
/// document's relayout is deferred because it is a pdfium render and a
/// texture upload per mounted page; a thumbnail is a twenty-fifth of a page
/// in area, and it is the thing directly under the pointer while the edge is
/// being dragged. A column whose pictures stay the size they were while its
/// own edge moves is the one place the deferral reads as a fault.
#[test]
fn the_thumbnails_follow_the_drag() {
    let mut reader = book();
    reader.press_chord("mod+b");
    reader.click(".tab[data-tab='pages']");
    let before = reader.harness.layout_rect(".thumb-picture");

    let (x, y) = reader.harness.center_of(".sidebar-resize");
    reader.harness.mouse_down_at(x, y);
    reader.carry(x + 120.0, y);
    reader.settle();
    let mid_drag = reader.harness.layout_rect(".thumb-picture");
    assert!(
        mid_drag.width > before.width + 60.0,
        "the thumbnail is wider while the pointer is still down: {} -> {}",
        before.width,
        mid_drag.width,
    );

    reader.harness.mouse_up_at(x + 120.0, y);
    reader.settle();
    let after = reader.harness.layout_rect(".thumb-picture");
    assert_eq!(
        (after.width, after.height),
        (mid_drag.width, mid_drag.height),
        "and letting go changes nothing further",
    );
}

#[test]
fn a_mark_is_a_toggle_and_survives_being_closed() {
    let config;
    let page;
    {
        let mut reader = with_contents();
        for _ in 0..3 {
            reader.press("l");
        }
        page = reader.state().page;
        assert_eq!(page, 4);
        reader.press_chord("mod+shift+b");
        assert_eq!(reader.state().notice, "Marked page 4");
        reader.press_chord("mod+b");
        // Named for the section it falls in, which the fixture calls
        // "A section" — a mark named "Page 4" is worth a great deal less.
        assert_eq!(reader.harness.text_content(".mark-go"), "A section");
        // The same gesture, doing the same thing.
        reader.press_chord("mod+shift+b");
        assert_eq!(reader.state().notice, "Took the mark off page 4");
        assert_eq!(reader.harness.query(".mark-go"), None);
        reader.press_chord("mod+shift+b");
        config = reader.config.clone();
    }
    let again = Reader::open_with(
        &fixture::contents_pdf(),
        Options {
            config,
            ..Default::default()
        },
    );
    assert_eq!(again.harness.text_content(".mark-go"), "A section");
}

#[test]
fn a_mark_goes_back_to_where_it_was_put() {
    let mut reader = with_contents();
    for _ in 0..5 {
        reader.press("l");
    }
    assert_eq!(reader.state().page, 6);
    reader.press_chord("mod+shift+b");
    reader.press("Home");
    assert_eq!(reader.state().page, 1);
    reader.press_chord("mod+b");
    reader.click(".mark-go");
    assert_eq!(reader.state().page, 6);
}

#[test]
fn a_mark_can_be_taken_off_from_the_panel() {
    let mut reader = with_contents();
    reader.press_chord("mod+shift+b");
    reader.press_chord("mod+b");
    assert!(reader.harness.query(".mark-go").is_some());
    reader.click(".mark-drop");
    assert_eq!(reader.harness.query(".mark-go"), None);
}

/* ------------------------------------------ the strip across the top */

/// **Everything in the panel stays inside the panel, at every width it can be
/// dragged to.** This is not a matter of taste: Blitz hit-tests a box where it
/// *is*, and the document's layer is over the panel's out there — so a tab
/// drawn past the panel's edge could be seen and could not be pressed, which
/// read as "I can't get back to the results". Three tabs that would not shrink
/// were the cause, and a result row that would not shrink was the same fault
/// one panel down, showing as a page number that had gone missing off the
/// left.
#[test]
fn nothing_in_the_panel_hangs_over_the_document() {
    let mut reader = Reader::open_with(&fixture::prose_pdf(), Options::default());
    reader.press_chord("mod+b");
    // **The narrowing comes first, and it has to.** The panel's edge is below
    // the toolbar, so picking it up is reaching past the find bar and puts the
    // search away — in this reader and, measured, in the app, where the grip
    // is not in `FIND_KEEPS_OPEN` either. Search after dragging and there is
    // still a list to look at.
    reader.drag_sidebar_edge(-100.0);
    reader.press_chord("mod+f");
    reader.type_text("the");
    reader.scan_out();

    let panel = reader.harness.layout_rect(".sidebar");
    let edge = panel.x + panel.width;
    for tab in reader.harness.query_all(".tab") {
        let rect = reader.harness.layout_rect_of(tab);
        assert!(
            rect.x + rect.width <= edge,
            "a tab runs to {} past a panel ending at {edge}",
            rect.x + rect.width,
        );
        // …and it can be pressed, which is the half the geometry is for.
        let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        assert!(
            reader.harness.hit(x, y).is_some_and(|hit| {
                let mut node = Some(hit.node_id);
                while let Some(id) = node {
                    if id == tab {
                        return true;
                    }
                    node = reader.harness.base().get_node(id).and_then(|n| n.parent);
                }
                false
            }),
            "the tab at ({x}, {y}) is not what a press there lands on",
        );
    }

    // The page a match is on is the first thing in its row and is still there.
    let row = reader.harness.query_all(".result");
    assert!(!row.is_empty(), "there are matches to list");
    for (number, row) in reader.harness.query_all(".result-page").iter().zip(&row) {
        let rect = reader.harness.layout_rect_of(*number);
        let outer = reader.harness.layout_rect_of(*row);
        assert!(
            rect.x >= outer.x && rect.x + rect.width <= edge,
            "a match's page number is outside its row: {rect:?} against {outer:?}",
        );
    }
}

/// **And a word that fits is not faded.**
///
/// The mask over a tab's last ten pixels is this reader's stand-in for the
/// `text-overflow: ellipsis` Blitz has not got, and it was on the label
/// itself — which is `flex: 0 1 auto`, so its box *is* its word. Every tab in
/// every panel was therefore faded at its end, including a two-tab strip with
/// a hundred and fifteen pixels a tab. It is the same fault the document's
/// name in the toolbar had, and the same fix: `sidebar.rs` says `tight` only
/// when there are three tabs and the panel is narrower than all three of them
/// labelled.
#[test]
fn a_tab_is_only_faded_when_its_word_might_not_fit() {
    let mut reader = Reader::open_with(&fixture::prose_pdf(), Options::default());
    reader.press_chord("mod+b");
    assert!(
        reader.harness.query(".tab-label").is_some(),
        "the words are shown at the default width",
    );
    assert!(
        reader.harness.query(".tabs.tight").is_none(),
        "two tabs at 252px have room and were faded anyway",
    );

    // A third tab arrives with the find bar, and now they do not all fit.
    reader.press_chord("mod+f");
    assert!(
        reader.harness.query(".tabs.tight").is_some(),
        "three tabs at 252px are cut and were not faded",
    );
}

/// And the word gives way before the drawing does — three tabs reading "C",
/// "P", "R" are three tabs nobody can tell apart, so below the width a word
/// fits in, the strip is icons.
#[test]
fn a_narrow_panel_keeps_the_drawings_and_drops_the_words() {
    let mut reader = Reader::open_with(&fixture::contents_pdf(), Options::default());
    reader.press_chord("mod+b");
    assert!(
        reader.harness.query(".tab-label").is_some(),
        "the default width has room for the words",
    );
    // Past the minimum, which is where it stops: a pointer carried off the
    // left of the window sends no move at all.
    reader.drag_sidebar_edge(-100.0);
    assert!(
        reader.harness.query(".tab-label").is_none(),
        "and the narrowest does not",
    );
    assert!(
        reader.harness.query(".tab .icon").is_some(),
        "the drawings stay at every width",
    );
}

/// **The tab strip has to win the hit test against a scrolled column.**
/// A thumbnail is placed at `top - thumb_scroll`, so a column scrolled at all
/// puts rows at negative offsets — painted clipped by `.panel`'s
/// `overflow: hidden`, and hit-tested where their boxes say they are, which
/// is over the tabs. Without a `z-index` on `.tabs`, clicking Contents from a
/// scrolled Pages tab landed on a thumbnail nobody could see; at the top of
/// the column, which is where anyone would check it, it worked.
#[test]
fn the_tabs_can_be_clicked_over_a_scrolled_column() {
    let mut reader = with_contents();
    reader.press_chord("mod+b");
    reader.click(".tab[data-tab='pages']");
    reader.wheel_over(".panel.thumb-column", 3_000.0);
    assert!(
        reader.state().thumbs[0] > 1,
        "the column has been scrolled: {:?}",
        reader.state().thumbs
    );
    reader.click(".tab[data-tab='contents']");
    assert_eq!(reader.state().sidebar.as_deref(), Some("contents"));
}

/// **A thumbnail is a button, and the picture is most of it.** Pressing the
/// page rather than the number under it is the gesture everybody makes, and it
/// did nothing at all: the picture is a custom widget, and Blitz makes no click
/// out of a press that lands on one. Scrolled first, because that is where a
/// reader is by the time they pick a page.
#[test]
fn clicking_a_thumbnail_goes_to_its_page() {
    let mut reader = book();
    reader.press_chord("mod+b");
    reader.click(".tab[data-tab='pages']");
    reader.wheel_over(".panel.thumb-column", 900.0);
    let picture = reader
        .harness
        .layout_rect(".thumb[data-thumb='4'] .thumb-picture");
    reader.click_at(
        picture.x + picture.width / 2.0,
        picture.y + picture.height / 2.0,
    );
    assert_eq!(reader.state().page, 4, "the picture is the button");
}
