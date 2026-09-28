//! The notes a document already carries, made readable.
//!
//! `renderNotes` in `viewer.ts` and `showNote` in `main.ts`, ported: pdfium
//! paints an annotation's own appearance into the page, so a sticky note
//! arrives as the little icon it was drawn as and a comment arrives
//! highlighted — and the words behind either of them live in the annotation,
//! where nothing here was reading them. The icon sat there looking like a
//! button and was not one.

use moonowl::fixture;
use moonowl::harness::{Options, Reader};

fn annotated() -> Reader {
    Reader::open_with(&fixture::notes_pdf(), Options::default())
}

/// What counts as a note: anything with words in it, whatever its subtype —
/// and not a link, whose text is where it goes, nor an annotation with
/// nothing to read.
#[test]
fn a_note_is_any_annotation_with_words_in_it() {
    let reader = annotated();
    let spots = reader.harness.query_all(".note-spot").len();
    let badges = reader.harness.query_all(".note-badge").len();
    assert_eq!(spots, 1, "the sticky note, which is a marker");
    assert_eq!(
        badges, 1,
        "and the comment over a passage, which is a badge"
    );
    // Two, and only two: the `/Square` with no `/Contents` has nothing to
    // read, and the link — which carries `/Contents` in this fixture on
    // purpose — is a link, whose text is where it goes. Either of them
    // counting would show up as a third spot here.
}

/// Pressing one opens it, and what it says is what the document says.
#[test]
fn pressing_a_note_opens_what_it_says() {
    let mut reader = annotated();
    assert!(reader.harness.query(".note-window").is_none());

    reader.click(".note-spot");
    let window = reader.harness.text_content(".note-window");
    assert!(
        window.contains("Check this against the second edition."),
        "the note's own words: {window:?}",
    );
    assert!(window.contains("A Reader"), "and who left it: {window:?}");
    assert!(window.contains("page 1"), "and where it is: {window:?}");

    reader.press("Escape");
    assert!(
        reader.harness.query(".note-window").is_none(),
        "Escape closes it, like every other window over the reader",
    );
}

/// A comment over a passage is a badge in the page's right margin, level
/// with its line: seen at a glance, and the words underneath stay in reach of
/// a pointer that wants to select them.
#[test]
fn a_comment_over_a_passage_is_a_badge_in_the_margin() {
    let reader = annotated();
    let badge = reader.harness.layout_rect(".note-badge");
    let page = reader.harness.layout_rect(".page");
    assert!(badge.width < 30.0, "a badge, not a cover: {badge:?}");
    let right = page.x + page.width - (badge.x + badge.width);
    assert!(
        (0.0..12.0).contains(&right),
        "at the page's right edge: {badge:?} on {page:?}"
    );
}

/// **Where the window has room beside the page, the comments are there in
/// words**, as a word processor shows them — and a page the width of the
/// window has only the badges.
#[test]
fn the_comments_are_beside_the_page_where_there_is_room() {
    let mut reader = annotated();
    assert!(
        reader.harness.query(".note-card").is_none(),
        "fit width leaves no room"
    );

    reader.press_action(moonowl::keymap::Action::FitPage);
    let cards = reader.text_all(".note-card-text");
    assert!(
        cards
            .iter()
            .any(|card| card.contains("Check this against the second edition.")),
        "{cards:?}"
    );
    let card = reader.harness.layout_rect(".note-card");
    let page = reader.harness.layout_rect(".page");
    assert!(
        card.x > page.x + page.width,
        "beside the page: {card:?} {page:?}"
    );

    reader.click(".note-card");
    assert!(
        reader.harness.query(".note-window").is_some(),
        "and a card opens the whole note"
    );
}
