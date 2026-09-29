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

/// Too narrow to keep a column for the comments beside the page.
fn narrow() -> Reader {
    Reader::open_with(
        &fixture::notes_pdf(),
        Options {
            width: 600,
            ..Options::default()
        },
    )
}

/// What counts as a note: anything with words in it, whatever its subtype —
/// and not a link, whose text is where it goes, nor an annotation with
/// nothing to read.
#[test]
fn a_note_is_any_annotation_with_words_in_it() {
    let reader = narrow();
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

    reader.press("Escape");
    assert!(
        reader.harness.query(".note-window").is_none(),
        "Escape closes it, like every other window over the reader",
    );
}

/// **A comment is read where it is**: in a column beside the page, which a
/// document with comments keeps even at fit width — who, when, and what —
/// and the page whose comments are there in words has no badge.
#[test]
fn the_comments_are_beside_the_page_in_words() {
    let reader = annotated();
    let card = reader.harness.layout_rect(".note-card");
    let page = reader.harness.layout_rect(".page");
    assert!(
        card.x > page.x + page.width,
        "beside the page: {card:?} {page:?}"
    );
    let cards = reader.text_all(".note-card-text");
    assert!(
        cards
            .iter()
            .any(|card| card.contains("This is the sentence the whole argument turns on.")),
        "{cards:?}"
    );
    let said = reader.text_all(".note-card-by");
    assert!(
        said.iter()
            .any(|said| said == "Unknown author · 28 Sep 2026, 21:09"),
        "who, where nobody is named, and when: {said:?}"
    );
    assert!(
        reader.harness.query(".note-badge").is_none(),
        "no badge where the words are"
    );
}

/// **A window too narrow for the column has the badge**, at the page's right
/// edge and level with its line, and pressing it opens the comment.
#[test]
fn a_narrow_window_has_the_badge() {
    let mut reader = narrow();
    assert!(reader.harness.query(".note-card").is_none());
    let badge = reader.harness.layout_rect(".note-badge");
    let page = reader.harness.layout_rect(".page");
    assert!(badge.width < 30.0, "a badge, not a cover: {badge:?}");
    let right = page.x + page.width - (badge.x + badge.width);
    assert!(
        (0.0..12.0).contains(&right),
        "at the page's right edge: {badge:?} on {page:?}"
    );
    reader.click(".note-badge");
    let window = reader.harness.text_content(".note-window");
    assert!(
        window.contains("This is the sentence the whole argument turns on."),
        "{window:?}"
    );
}

/// **Zoomed past the window, the comment is still there in words**: the
/// column is the window's, and the page pans under it.
#[test]
fn a_page_zoomed_past_the_window_keeps_its_comments() {
    let mut reader = annotated();
    for _ in 0..8 {
        reader.press_action(moonowl::keymap::Action::ZoomIn);
    }
    let page = reader.harness.layout_rect(".page");
    let column = reader.harness.layout_rect(".comment-column");
    assert!(
        page.x + page.width > column.x,
        "the page runs under the column: {page:?} {column:?}"
    );
    let card = reader.harness.layout_rect(".note-card");
    assert!(
        card.x >= column.x && card.x + card.width <= column.x + column.width,
        "the card is in the column: {card:?} {column:?}"
    );
    let said = reader.text_all(".note-card-by");
    assert!(
        said.iter().any(|said| said.starts_with("Unknown author")),
        "{said:?}"
    );
}

/// **A card rings its passage**, so it is never read against the lines
/// another column puts level with it; the passage lights its card but is not
/// boxed in itself. The ring is the highlight's colour, as the card's border
/// is.
#[test]
fn a_card_and_its_passage_light_up_together() {
    let mut reader = annotated();
    assert!(reader.harness.query(".note-passage").is_none());
    // The comment's card, not the sticky note's beside it.
    let card = reader
        .harness
        .query_all(".note-card")
        .into_iter()
        .map(|node| reader.harness.layout_rect_of(node))
        .max_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap();
    reader.point_to(card.x + card.width / 2.0, card.y + 10.0);
    let ring = reader.harness.layout_rect(".note-passage");
    let page = reader.harness.layout_rect(".page");
    assert!(
        ring.x < page.x + page.width && ring.width > 100.0,
        "the passage: {ring:?}"
    );
    assert!(!reader.harness.query_all(".note-card.hot").is_empty());

    // Away, and back by the passage itself.
    reader.point_to(4.0, card.y + card.height + 200.0);
    assert!(reader.harness.query(".note-passage").is_none());
    reader.point_to(ring.x + ring.width / 2.0, ring.y + ring.height / 2.0);
    assert!(!reader.harness.query_all(".note-card.hot").is_empty());
    assert!(
        reader.harness.query(".note-passage").is_none(),
        "no ring from the passage"
    );
}

/// **On a two-column page, a comment on the left column is left of the
/// page**, beside its own lines rather than past the other column's, and one
/// on the right column is right of it.
#[test]
fn a_comment_on_the_left_column_is_left_of_the_page() {
    let reader = Reader::open_with(&fixture::columns_pdf(), Options::default());
    let page = reader.harness.layout_rect(".page");
    let cards: Vec<_> = reader
        .harness
        .query_all(".note-card")
        .into_iter()
        .map(|node| reader.harness.layout_rect_of(node))
        .collect();
    assert_eq!(cards.len(), 2, "{cards:?}");
    assert!(
        cards.iter().any(|card| card.x + card.width <= page.x),
        "one left of the page: {cards:?} {page:?}"
    );
    assert!(
        cards.iter().any(|card| card.x >= page.x + page.width),
        "and one right of it: {cards:?} {page:?}"
    );
    let text = reader.text_all(".note-card-text");
    let left = cards.iter().position(|card| card.x < page.x).unwrap();
    assert_eq!(text[left], "On the left column.");
}

/// And a window without room for two columns keeps one, on the right.
#[test]
fn without_room_for_two_columns_every_comment_goes_right() {
    let reader = Reader::open_with(
        &fixture::columns_pdf(),
        Options {
            width: 900,
            ..Options::default()
        },
    );
    let page = reader.harness.layout_rect(".page");
    let cards: Vec<_> = reader
        .harness
        .query_all(".note-card")
        .into_iter()
        .map(|node| reader.harness.layout_rect_of(node))
        .collect();
    assert_eq!(cards.len(), 2, "{cards:?}");
    assert!(
        cards.iter().all(|card| card.x >= page.x + page.width),
        "{cards:?}"
    );
}

/// Zoomed past the window, the left column holds its comments as the right
/// one does.
#[test]
fn the_left_column_keeps_its_comments_zoomed_in() {
    let mut reader = Reader::open_with(&fixture::columns_pdf(), Options::default());
    for _ in 0..8 {
        reader.press_action(moonowl::keymap::Action::ZoomIn);
    }
    let columns: Vec<_> = reader
        .harness
        .query_all(".comment-column")
        .into_iter()
        .map(|node| reader.harness.layout_rect_of(node))
        .collect();
    let left = columns
        .iter()
        .min_by(|a, b| a.x.total_cmp(&b.x))
        .copied()
        .unwrap();
    let text = reader.text_all(".note-card-text");
    let cards: Vec<_> = reader
        .harness
        .query_all(".note-card")
        .into_iter()
        .map(|node| reader.harness.layout_rect_of(node))
        .collect();
    let at = text
        .iter()
        .position(|t| t == "On the left column.")
        .expect("shown");
    let card = cards[at];
    assert!(
        card.x >= left.x && card.x + card.width <= left.x + left.width,
        "{card:?} in {left:?}"
    );
}

/// **Two pages side by side**: the left page has the other at its right, so
/// its comments are left of it — as cards, not badges — and they go back
/// right when the spread goes.
#[test]
fn the_left_page_of_a_spread_has_its_comments_at_the_left() {
    let mut reader = Reader::open_with(
        &fixture::notes_pdf(),
        Options {
            settings: vec![("spread_mode".into(), "two".into())],
            keys: [("spread".to_string(), vec!["shift+b".to_string()])].into(),
            ..Options::default()
        },
    );
    let cards = |reader: &Reader| {
        let page = reader.harness.layout_rect(".page");
        let cards: Vec<_> = reader
            .harness
            .query_all(".note-card")
            .into_iter()
            .map(|node| reader.harness.layout_rect_of(node))
            .collect();
        assert!(!cards.is_empty());
        (cards, page)
    };
    let (left, page) = cards(&reader);
    assert!(
        left.iter().all(|card| card.x + card.width <= page.x),
        "{left:?} {page:?}"
    );
    reader.press_chord("shift+b");
    let (right, page) = cards(&reader);
    assert!(
        right.iter().all(|card| card.x >= page.x + page.width),
        "{right:?} {page:?}"
    );
    reader.press_chord("shift+b");
    let (left, page) = cards(&reader);
    assert!(
        left.iter().all(|card| card.x + card.width <= page.x),
        "{left:?} {page:?}"
    );
}
