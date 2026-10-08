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

/// Fit page, which leaves room beside the page: at fit width there is none,
/// and every comment is underlined.
fn fit_page(path: &str) -> Reader {
    Reader::open_with(
        path,
        Options {
            settings: vec![("fit_mode".into(), "page".into())],
            ..Options::default()
        },
    )
}

fn annotated() -> Reader {
    fit_page(&fixture::notes_pdf())
}

/// Too narrow for a card beside the page.
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
    let lines = reader.harness.query_all(".note-line").len();
    assert_eq!(spots, 1, "the sticky note, which is a marker");
    assert_eq!(
        lines, 1,
        "and the comment over a passage, which is underlined"
    );
    // Two, and only two: the `/Square` with no `/Contents` has nothing to
    // read, and the link — which carries `/Contents` in this fixture on
    // purpose — is a link, whose text is where it goes. Either of them
    // counting would show up as a third spot here.
}

/// **Pressing one opens it on the page**, under the marker rather than in a
/// window over the reader, and what it says is what the document says.
#[test]
fn pressing_a_note_opens_what_it_says() {
    let mut reader = narrow();
    assert!(reader.harness.query(".note-card").is_none());

    reader.click(".note-spot");
    let card = reader.harness.text_content(".note-card.read");
    assert!(
        card.contains("Check this against the second edition."),
        "the note's own words: {card:?}",
    );
    assert!(card.contains("A Reader"), "and who left it: {card:?}");
    let (spot, card) = (
        reader.harness.layout_rect(".note-spot"),
        reader.harness.layout_rect(".note-card.read"),
    );
    assert!(card.y > spot.y + spot.height, "under it: {card:?} {spot:?}");
    assert!(reader.harness.query(".window-scrim").is_none(), "no window");

    reader.press("Escape");
    assert!(
        reader.harness.query(".note-card").is_none(),
        "Escape puts it away"
    );
    reader.click(".note-spot");
    reader.click_at(4.0, 300.0);
    assert!(
        reader.harness.query(".note-card").is_none(),
        "and so does a press anywhere else"
    );
}

/// **A comment is read where it is**: beside the page, where the window has
/// room — who, when, and what — and a passage whose comment is there in
/// words is not underlined.
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
        said.iter().any(|said| said == "28 Sep 2026, 21:09"),
        "who, where nobody is named, and when: {said:?}"
    );
    assert!(
        reader.harness.query(".note-line").is_none(),
        "no line where the words are"
    );
}

/// **A window too narrow for a card underlines the passage**, under its
/// own words rather than off at an edge, and pressing the line opens the
/// comment where a click on a mark opens it: to read, and to edit in place.
#[test]
fn a_narrow_window_underlines_the_passage() {
    let mut reader = narrow();
    assert!(reader.harness.query(".note-card").is_none());
    let line = reader.harness.layout_rect(".note-line");
    let page = reader.harness.layout_rect(".page");
    assert!(line.height < 10.0, "a line, not a cover: {line:?}");
    assert!(
        line.x > page.x && line.x + line.width < page.x + page.width,
        "under the passage: {line:?} on {page:?}"
    );
    reader.click(".note-line");
    assert!(reader.harness.query(".note-window").is_none(), "no window");
    // At the top of the menu, as its card beside the page would say it.
    assert_eq!(
        reader.harness.text_content(".mark-note .note-card-text"),
        "This is the sentence the whole argument turns on."
    );
    assert_eq!(
        reader.harness.text_content(".mark-note .note-card-by"),
        "28 Sep 2026, 21:09"
    );
    // A click on the card writes in it, where it was.
    let card = reader.harness.layout_rect(".mark-note");
    reader.click(".mark-note .note-card-text");
    assert!(reader.harness.query(".mark-popover").is_none());
    let editing = reader.harness.layout_rect(".note-card.editing");
    assert!(
        (editing.x - card.x).abs() < 1.0 && (editing.y - card.y).abs() < 1.0,
        "over the card it was read in: {editing:?} {card:?}"
    );
    assert_eq!(
        reader.field(".note-card-field"),
        "This is the sentence the whole argument turns on."
    );
}

/// **Zoomed past the window, the comment is underlined**: the page is never
/// narrowed to make room for it.
#[test]
fn a_page_zoomed_past_the_window_underlines_the_passage() {
    let mut reader = annotated();
    for _ in 0..8 {
        reader.press_action(moonowl::keymap::Action::ZoomIn);
    }
    assert!(reader.harness.query(".note-card").is_none());
    assert!(reader.harness.query(".note-line").is_some());
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
    let reader = fit_page(&fixture::columns_pdf());
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

/// **Two pages side by side**: the left page has the other at its right, so
/// its comments are left of it — as cards, not badges — and they go back
/// right when the spread goes.
#[test]
fn the_left_page_of_a_spread_has_its_comments_at_the_left() {
    let mut reader = Reader::open_with(
        &fixture::notes_pdf(),
        Options {
            settings: vec![
                ("spread_mode".into(), "two".into()),
                ("fit_mode".into(), "page".into()),
            ],
            // Room beside two pages.
            width: 2000,
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
