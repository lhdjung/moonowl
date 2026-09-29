//! The right-click menu: what it offers over a selection, a mark, a link and
//! a plain page, and that each row does what it says.

use moonowl::app::Ask;
use moonowl::fixture;
use moonowl::harness::{Options, Reader};
use moonowl::render;

/// A copy of the prose fixture — one line of type near the top of each of six
/// pages — in a directory of this test's own, because marking writes to it.
fn readable(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("moonowl-context-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory to write in");
    let path = dir.join("prose.pdf");
    std::fs::copy(fixture::prose_pdf(), &path).expect("a copy of the fixture");
    path.to_string_lossy().into_owned()
}

/// Where the one line of a `prose_pdf` page sits inside its box.
const LINE: f32 = 0.108;

fn open(path: &str) -> Reader {
    Reader::open_with(path, Options::default())
}

fn rows(reader: &Reader) -> Vec<String> {
    reader.attribute_all(".menu.context .menu-item", "data-item")
}

#[test]
fn a_right_click_on_a_page_offers_the_way_out_of_full_screen_first() {
    let mut reader = open(&readable("page"));
    reader.right_click_on_page(1, (0.5, 0.3));
    let offered = rows(&reader);
    assert_eq!(offered.first().map(String::as_str), Some("full-screen"));
    assert!(
        offered.contains(&"print".to_string()),
        "the document's own rows: {offered:?}"
    );
    assert!(
        !offered.contains(&"unmark-all".to_string()),
        "every mark in the document is not a misplaced click away",
    );
    reader.click("[data-item='full-screen']");
    assert_eq!(reader.asks(), vec![Ask::FullScreen(true)]);
    assert!(reader.harness.query(".menu.context").is_none());
}

#[test]
fn escape_and_a_press_elsewhere_put_it_away() {
    let mut reader = open(&readable("away"));
    reader.right_click_on_page(1, (0.5, 0.3));
    assert!(reader.harness.query(".menu.context").is_some());
    reader.press("Escape");
    assert!(reader.harness.query(".menu.context").is_none());

    reader.right_click_on_page(1, (0.5, 0.3));
    reader.click_on_page(1, (0.8, 0.2));
    assert!(reader.harness.query(".menu.context").is_none());
    assert!(
        reader.harness.query_all(".selected").is_empty(),
        "and the press that put it away began no sweep",
    );
}

#[test]
fn it_bookmarks_the_page_it_was_opened_on() {
    // Two across, so that the page right-clicked is not the page being read.
    let options = Options {
        settings: vec![
            ("fit_mode".into(), serde_json::json!("page")),
            ("spread_mode".into(), serde_json::json!("two")),
        ],
        ..Options::default()
    };
    let mut reader = Reader::open_with(&readable("bookmark"), options);
    assert_eq!(reader.state().page, 1);
    reader.right_click_on_page(2, (0.5, 0.3));
    reader.click("[data-item='mark']");
    assert_eq!(reader.state().notice, "Bookmarked page 2");
}

#[test]
fn over_a_selection_it_copies_and_finds_it() {
    let mut reader = open(&readable("selected"));
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.right_click_on_page(1, (0.30, LINE));
    assert_eq!(
        rows(&reader),
        vec!["copy", "copy-quote", "highlight", "find"]
    );
    reader.click("[data-item='copy']");
    assert_eq!(reader.copied(), vec!["A needle in the first page."]);

    reader.right_click_on_page(1, (0.30, LINE));
    reader.click("[data-item='find']");
    assert_eq!(reader.state().query, "A needle in the first page.");
    assert!(reader.state().find.is_some(), "the find bar is up");
}

#[test]
fn highlight_puts_the_swatches_up() {
    let options = Options {
        settings: vec![("offer_highlight_on_select".into(), serde_json::json!(false))],
        ..Options::default()
    };
    let mut reader = Reader::open_with(&readable("swatches"), options);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    assert!(reader.harness.query(".markup-popover").is_none());
    reader.right_click_on_page(1, (0.30, LINE));
    reader.click("[data-item='highlight']");
    assert!(reader.harness.query(".markup-popover").is_some());
}

#[test]
fn a_colour_chosen_from_a_marks_menu_is_the_marks_too() {
    let path = readable("chosen");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");

    reader.right_click_on_page(1, (0.30, LINE));
    reader.click("[data-item='recolour']");
    reader.click_nth(".colours-window .color-hex", 0);
    reader.press("End");
    for _ in 0..7 {
        reader.press("Backspace");
    }
    reader.type_text("#123456");
    reader.click(".colours-window .window-close");
    let marks = render::open(&path).expect("reopens").markup();
    assert!(
        marks[0].color.eq_ignore_ascii_case("#123456"),
        "{}",
        marks[0].color
    );
    assert_eq!(
        reader.attribute_all(".mark-swatch", "data-colour").len(),
        0,
        "and the menu is put away"
    );
}

#[test]
fn over_a_mark_it_is_the_marks_own_menu() {
    let path = readable("marked");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    let first = render::open(&path).expect("reopens").markup()[0]
        .color
        .clone();

    // The mark's own menu, the one a click opens: one menu per highlight.
    reader.right_click_on_page(1, (0.30, LINE));
    assert!(reader.harness.query(".menu.context").is_none());
    assert_eq!(
        reader.attribute_all(".mark-popover .menu-item", "data-item"),
        vec!["copy", "comment", "recolour", "remove"]
    );
    // Change colour… is the window of the six, over the menu.
    reader.click("[data-item='recolour']");
    assert!(reader.harness.query(".colours-window").is_some());
    assert!(reader.harness.query(".mark-popover").is_some());
    reader.click(".colours-window .window-close");
    reader.click("[data-item='copy']");
    assert_eq!(reader.copied(), vec!["A needle in the first page."]);
    assert!(reader.harness.query(".mark-popover").is_none());

    // With the six in it.
    reader.right_click_on_page(1, (0.30, LINE));
    assert!(reader.harness.query(".mark-popover").is_some());
    assert_eq!(
        reader
            .harness
            .attr(".mark-swatch.on", "data-colour")
            .as_deref(),
        Some(first.as_str()),
        "the colour it is in is the one ringed",
    );
    let second = reader.attribute_all(".mark-swatch", "data-colour")[1].clone();
    reader.click_nth(".mark-swatch", 1);
    assert!(reader.harness.query(".mark-popover").is_none());
    let marks = render::open(&path).expect("reopens").markup();
    assert_eq!(marks.len(), 1, "one mark, in another colour");
    assert!(
        marks[0].color.eq_ignore_ascii_case(&second),
        "{} is not {second}",
        marks[0].color
    );

    reader.right_click_on_page(1, (0.30, LINE));
    reader.click("[data-item='remove']");
    assert!(render::open(&path).expect("reopens").markup().is_empty());
}

#[test]
fn over_a_link_out_it_opens_and_copies_it() {
    let mut reader = Reader::open(&fixture::links_pdf());
    let at = reader
        .attribute_all(".link", "aria-label")
        .iter()
        .position(|name| name == "https://example.com/paper")
        .expect("the fixture links out");
    let node = reader.harness.query_all(".link")[at];
    let rect = reader.harness.layout_rect_of(node);
    let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);

    reader.right_click_at(x, y);
    assert_eq!(
        rows(&reader)[..3],
        ["open-link", "copy-link", "full-screen"]
    );
    reader.click("[data-item='copy-link']");
    assert_eq!(reader.copied(), vec!["https://example.com/paper"]);

    reader.right_click_at(x, y);
    reader.click("[data-item='open-link']");
    assert_eq!(reader.opened(), vec!["https://example.com/paper"]);
}
