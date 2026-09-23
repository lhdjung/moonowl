//! The document's own links, the pages it numbers its own way, and the field
//! that takes a number.
//!
//! Phase 3 item 5. The app has no test file for any of this — links are
//! covered by `reader.test.mjs` only as far as "a link layer exists", and
//! labels by `labels.test.mjs`, which asserts on a book that numbers its front
//! matter i, ii, iii. That book is a fixture the app generates with Node;
//! `fixture::links_pdf` is the same shape written in Rust, with the links in
//! it as well, so that `cargo test` needs cargo and nothing else.
//!
//! Everything here is asked of the interface: a link is a node with an area,
//! a jump is the page the toolbar then says, and an address handed to the
//! system is one the harness wrote down rather than one a browser opened.

use moonowl::fixture;
use moonowl::harness::{Options, Reader};

/// Six pages: two links on the first, a `/GoTo` action on the second, a link
/// that points nowhere on the third, and `/PageLabels` over all of them.
fn linked() -> Reader {
    Reader::open(&fixture::links_pdf())
}

/// The links the interface is showing, as (left, top, width, height) in CSS
/// pixels relative to the window — which is what a reader would point at.
fn link_areas(reader: &Reader) -> Vec<(f32, f32, f32, f32)> {
    reader
        .harness
        .query_all(".link")
        .into_iter()
        .map(|node| {
            let rect = reader.harness.layout_rect_of(node);
            (rect.x, rect.y, rect.width, rect.height)
        })
        .collect()
}

#[test]
fn a_link_is_a_node_where_the_document_says_it_is() {
    let reader = linked();
    let areas = link_areas(&reader);
    // Two on page one, and the pages after it are in the mounting window: the
    // `/GoTo` on page two is there as well, and the one that points nowhere is
    // not, whether or not page three is mounted.
    assert!(
        areas.len() >= 2,
        "page one's two links, at least: {areas:?}"
    );

    // The first link's area, from the file: 128 × 20 points, 72 from the left,
    // 72 from the top of a page 792 points tall. What it is on screen is that
    // multiplied by the page's own scale, so the shape is what to assert on —
    // a ratio the window size cannot move.
    let (left, _top, width, height) = areas[0];
    assert!(
        (width / height - 128.0 / 20.0).abs() < 0.05,
        "the link keeps the shape the file gives it: {width} × {height}"
    );
    // And it starts where the page's own left margin does, which is the same
    // 72 points the text is set at.
    let page = reader.harness.query(".page").expect("a page is mounted");
    let page_rect = reader.harness.layout_rect_of(page);
    let scale = page_rect.width / 612.0;
    assert!(
        ((left - page_rect.x) / scale - 72.0).abs() < 1.5,
        "72 points in from the page's left edge, not the window's"
    );
}

#[test]
fn a_link_that_points_nowhere_is_not_a_link() {
    let reader = linked();
    // Asked of the renderer rather than of the DOM, because this is a question
    // about the *document*: page three carries a `/Link` annotation with
    // neither an action nor a destination, and what happens to it happens
    // before anything is mounted. A rectangle that does nothing when it is
    // clicked reads as the app being broken rather than as the document being
    // odd, so it is dropped.
    assert_eq!(reader.document.links_of(0).len(), 2, "page one's two");
    assert_eq!(reader.document.links_of(1).len(), 1, "page two's `/GoTo`");
    assert!(
        reader.document.links_of(2).is_empty(),
        "page three's points nowhere"
    );
}

/// Where each mounted link says it goes, off the name it gives a screen
/// reader — which is the only thing on a bare rectangle that says so, and is
/// therefore also how a test tells two of them apart.
fn destinations(reader: &Reader) -> Vec<String> {
    reader.attribute_all(".link", "aria-label")
}

/// Click the link that goes to `where_to`, whichever page it is on.
fn follow(reader: &mut Reader, where_to: &str) {
    let at = destinations(reader)
        .iter()
        .position(|name| name == where_to)
        .unwrap_or_else(|| panic!("no link to {where_to}: {:?}", destinations(reader)));
    reader.click_nth(".link", at);
}

/// **The click that puts a menu away is spent on that.** The View menu stays
/// up on purpose, and the press that closed it landed on a cross-reference
/// and jumped.
#[test]
fn a_click_that_closes_a_menu_does_not_follow_a_link() {
    let mut reader = linked();
    reader.click(".chip.fit");
    assert_eq!(reader.state().menu.as_deref(), Some("view"));
    follow(&mut reader, "Page 5 of this document");
    assert_eq!(reader.state().menu, None, "the menu went");
    assert_eq!(reader.state().label, "i", "and nothing else happened");

    follow(&mut reader, "Page 5 of this document");
    assert_eq!(reader.state().label, "2", "the next click is a click");
}

#[test]
fn following_a_link_goes_where_the_destination_says() {
    let mut reader = linked();
    assert_eq!(
        reader.state().label,
        "i",
        "the first page, as it is printed"
    );

    // The second link on page one: `/Dest [page five /XYZ null 400 null]`.
    follow(&mut reader, "Page 5 of this document");
    assert_eq!(
        reader.state().label,
        "2",
        "page five of the file, which is printed 2"
    );

    // And it lands where the destination says rather than at the top of the
    // page. `/XYZ null 400 null` on a page 792 points tall is 392 points down,
    // which is [`fixture::LINK_OFFSET`] of the way through it — so page five's
    // box has to start that far *above* the top of the document area.
    let mounted = reader.state().mounted;
    let at = mounted
        .iter()
        .position(|&page| page == 5)
        .expect("page five is mounted after jumping to it");
    let page = reader
        .harness
        .layout_rect_of(reader.harness.query_all(".page")[at]);
    let viewer = reader
        .harness
        .layout_rect_of(reader.harness.query(".viewer").expect("the document area"));
    let landed = (viewer.y - page.y) / page.height;
    assert!(
        (landed as f64 - fixture::LINK_OFFSET).abs() < 0.02,
        "{landed} of the way down page five, against {}",
        fixture::LINK_OFFSET
    );
}

#[test]
fn a_link_written_as_an_action_goes_the_same_place() {
    let mut reader = linked();
    // Page two's link is a `/GoTo` action rather than a `/Dest`, which is the
    // other of the two routes `links_of` follows. Page two has to be on screen
    // for it to be a node at all — the link layer is the mounting window's,
    // like everything else drawn over a page.
    reader.press("l");
    follow(&mut reader, "Page 6 of this document");
    assert_eq!(
        reader.state().label,
        "3",
        "page six of the file, which is printed 3"
    );
}

#[test]
fn a_link_out_of_the_document_is_handed_to_the_system() {
    let mut reader = linked();
    let before = reader.state().scroll;
    follow(&mut reader, "https://example.com/paper");
    assert_eq!(
        reader.opened(),
        vec!["https://example.com/paper".to_string()],
        "the address, once, and nothing opened"
    );
    assert_eq!(
        reader.state().scroll,
        before,
        "and the document did not move"
    );
    assert!(
        reader.state().notice.contains("example.com"),
        "the reader is told where it went: {:?}",
        reader.state().notice
    );
}

#[test]
fn back_returns_to_where_the_jump_started_and_forward_returns_again() {
    let mut reader = linked();
    // Somewhere that is not the top of the document, so that "back" has an
    // offset to put right as well as a page — but not a whole screenful,
    // which now carries page one's links off the top of the window. The
    // document area grew when the notice line stopped being a row of the
    // window and became a pill over it, and a screenful grew with it.
    reader.wheel(150.0);
    let started = reader.state().scroll;

    follow(&mut reader, "Page 5 of this document");
    assert_eq!(reader.state().label, "2");

    reader.press_chord("mod+[");
    assert_eq!(reader.state().label, "i", "back to the page we jumped from");
    assert!(
        (reader.state().scroll - started).abs() < 2.0,
        "and to the place on it: {} against {started}",
        reader.state().scroll
    );

    reader.press_chord("mod+]");
    assert_eq!(reader.state().label, "2", "forward again");
}

#[test]
fn the_end_of_the_history_says_so() {
    let mut reader = linked();
    reader.press_chord("mod+[");
    assert!(
        reader.state().notice.contains("further back"),
        "a shortcut that did nothing is indistinguishable from one that is \
         not bound: {:?}",
        reader.state().notice
    );
    reader.press_chord("mod+]");
    assert!(reader.state().notice.contains("further forward"));
}

#[test]
fn scrolling_is_not_a_jump() {
    let mut reader = linked();
    // The distinction the whole history rests on: moving *through* a document
    // leaves no trace. Four screenfuls and a page turn, and there is still
    // nowhere to go back to.
    reader.wheel_screen();
    reader.wheel_screen();
    reader.press("l");
    reader.press_chord("mod+[");
    assert!(
        reader.state().notice.contains("further back"),
        "scrolling and turning pages are not places to come back to"
    );
}

#[test]
fn a_document_numbers_its_own_pages() {
    let mut reader = linked();
    assert_eq!(reader.state().label, "i");
    assert_eq!(
        reader.state().pages,
        6,
        "six pages, whatever they are called"
    );

    // Down to the body, which starts again at 1.
    for _ in 0..3 {
        reader.press("l");
    }
    assert_eq!(reader.state().label, "1", "the fourth page is printed 1");
    assert_eq!(
        fixture::LABELS,
        ["i", "ii", "iii", "1", "2", "3"],
        "the fixture's own list, so this test names what it is asserting"
    );
}

#[test]
fn the_field_takes_a_label_first_and_a_position_second() {
    let mut reader = linked();
    // "3" is the label of page six and also the position of page three. The
    // label wins, which is what makes a number off an index find what the
    // index meant.
    reader.press("p");
    reader.type_text("3");
    reader.press("Enter");
    assert_eq!(
        reader.state().label,
        "3",
        "page six of the file, whose label is 3 — not page three, whose label \
         is iii"
    );
    assert!(reader.state().mounted.contains(&6));

    // "iii" is a label and nothing else, so there is only one answer.
    reader.press("p");
    reader.type_text("iii");
    reader.press("Enter");
    assert_eq!(reader.state().label, "iii");
    assert!(reader.state().mounted.contains(&3));
}

/// **A number past the end goes to the end, and says nothing about it.**
/// This is ⌘9 in a browser with four tabs open: it goes to the fourth. The
/// notice is kept for what cannot be clamped — text that is neither a label
/// this document uses nor a number at all.
#[test]
fn a_number_past_the_end_goes_to_the_end_and_a_word_is_said() {
    let mut reader = linked();
    reader.press("p");
    reader.type_text("99");
    reader.press("Enter");
    assert_eq!(
        reader.state().label,
        "3",
        "the sixth page of six, which this document calls 3",
    );
    assert!(reader.state().mounted.contains(&6));
    assert_eq!(reader.state().notice, "", "and nothing was said about it");

    // The other end, for the same reason: 0 is the first page.
    reader.press("p");
    reader.type_text("0");
    reader.press("Enter");
    assert_eq!(reader.state().label, "i");

    // And what cannot be clamped is still said out loud.
    reader.press("p");
    reader.type_text("xii");
    reader.press("Enter");
    assert!(
        reader.state().notice.contains("no page xii"),
        "{:?}",
        reader.state().notice
    );
    assert_eq!(
        reader.state().label,
        "i",
        "and the field goes back to naming the page the reader is on"
    );
}

#[test]
fn typing_in_the_field_does_not_drive_the_document() {
    let mut reader = linked();
    let before = reader.state().scroll;
    reader.press("p");
    // "j" is scroll down and " " is a screen down, everywhere else in this
    // reader. In the field they are two characters.
    reader.type_text("j 2");
    assert_eq!(reader.state().scroll, before, "the document stayed put");
    reader.press("Escape");
    assert_eq!(
        reader.state().label,
        "i",
        "and Escape puts the current page back in the field"
    );
    // The keyboard comes back with it.
    reader.press("j");
    assert!(
        reader.state().scroll > before,
        "the reader has its keys back"
    );
}

#[test]
fn the_field_is_a_jump() {
    let mut reader = linked();
    reader.wheel_screen();
    let started = reader.state().scroll;
    reader.press("p");
    reader.type_text("iii");
    reader.press("Enter");
    reader.press_chord("mod+[");
    assert!(
        (reader.state().scroll - started).abs() < 2.0,
        "typing a page number is a jump, so back comes back: {} against {started}",
        reader.state().scroll
    );
}

#[test]
fn the_numbers_printed_on_an_offprint_are_read_off_the_paper() {
    // No `/PageLabels`, and 407 at the foot of the first page: the label is
    // what the paper says, the count follows it, and a citation finds its
    // page.
    let mut reader = Reader::open_with(&fixture::offprint_pdf(), Options::default());
    assert_eq!(reader.state().label, "407");
    assert_eq!(reader.harness.text_content(".of").trim(), "of 425");
    reader.press("p");
    reader.type_text("412");
    reader.press("Enter");
    assert_eq!(reader.state().label, "412");
    assert!(
        reader.state().mounted.contains(&6),
        "412 is the sixth page of the file: {:?}",
        reader.state().mounted
    );
}

#[test]
fn the_count_is_a_menu_that_chooses_the_numbering() {
    let mut reader = Reader::open_with(&fixture::offprint_pdf(), Options::default());
    reader.click(".of.choice");
    let rows = reader.text_all(".menu.numbering .menu-item");
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(
        rows[0].contains("407 of 425") && rows[1].contains("1 of 19"),
        "{rows:?}"
    );
    reader.click_nth(".menu.numbering .menu-item", 1);
    assert_eq!(reader.state().label, "1");
    assert_eq!(reader.harness.text_content(".of").trim(), "of 19");
    // And typing a number now means a place in the file.
    reader.press("p");
    reader.type_text("6");
    reader.press("Enter");
    assert!(
        reader.state().mounted.contains(&6),
        "{:?}",
        reader.state().mounted
    );
    // A document with nothing to choose between offers no menu.
    let reader = Reader::open_with(&fixture::contents_pdf(), Options::default());
    assert!(reader.harness.query(".of.choice").is_none());
}

#[test]
fn a_document_that_numbers_its_pages_1_to_n_says_nothing() {
    // The commoner case by a long way, and the one the app drops the list
    // for: `state().page` is the position, and it is also the label.
    let reader = Reader::open_with(&fixture::contents_pdf(), Options::default());
    assert_eq!(reader.state().page, 1);
    assert_eq!(reader.state().label, "1");
    assert!(
        reader.document.labels().is_empty(),
        "a list that restates the position is not carried"
    );
}

/// **A link is ramped from the page as drawn, not as recoloured.** Page one's
/// second link is over nothing, so under a dark theme it is the theme's paper.
/// The software path used to ramp the recoloured page, where dark paper reads
/// as ink, and drew every link as a solid block of the link colour.
#[test]
fn a_link_over_nothing_is_paper_under_a_dark_theme() {
    let mut reader = Reader::open_with(
        &fixture::links_pdf(),
        Options {
            settings: vec![
                ("theme".into(), serde_json::json!("gruvbox")),
                ("follow_system_theme".into(), serde_json::json!(false)),
            ],
            ..Options::default()
        },
    );
    let (x, y, width, height) = link_areas(&reader)[1];
    let shot = reader.screenshot();
    let scale = shot.width as f32 / reader.window().0 as f32;
    let pixel = shot.at(
        ((x + width / 2.0) * scale) as u32,
        ((y + height / 2.0) * scale) as u32,
    );
    assert_eq!(&pixel[..3], &[0x28, 0x28, 0x28], "Gruvbox's paper");
}
