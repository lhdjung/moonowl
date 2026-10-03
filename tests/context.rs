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
fn over_a_selection_on_a_mark_it_is_one_menu_with_both() {
    let path = readable("both");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));

    reader.right_click_on_page(1, (0.30, LINE));
    assert!(reader.harness.query(".menu.context").is_none());
    assert!(reader.harness.query(".markup-popover").is_none());
    assert_eq!(
        reader.attribute_all(".mark-popover .menu-item", "data-item"),
        vec![
            "copy",
            "copy-quote",
            "highlight",
            "comment",
            "recolour",
            "remove",
            "find"
        ]
    );
    reader.click("[data-item='find']");
    assert!(reader
        .state()
        .query
        .starts_with("A needle in the first page"));
    assert!(reader.state().find.is_some(), "the find bar is up");
    assert!(reader.harness.query(".mark-popover").is_none());
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

/// **A mark's menu opens at the pointer**, under the line it is about, and
/// is kept inside the window as the right-click menu is.
#[test]
fn a_marks_menu_opens_at_the_pointer_and_inside_the_window() {
    let path = readable("placed");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    let (x, y) = reader.point_on(1, (0.45, LINE));
    reader.right_click_on_page(1, (0.45, LINE));
    let menu = reader.harness.layout_rect(".mark-popover");
    assert!((menu.x - x).abs() < 2.0, "at the pointer: {menu:?} for {x}");
    assert!(menu.y > y, "under the line: {menu:?} for {y}");

    // A window too short for it below the line, and too narrow to its right.
    let options = Options {
        width: 400,
        height: 320,
        ..Options::default()
    };
    let mut reader = Reader::open_with(&path, options);
    let (x, _) = reader.point_on(1, (0.45, LINE));
    reader.right_click_on_page(1, (0.45, LINE));
    let menu = reader.harness.layout_rect(".mark-popover");
    assert!(
        x + menu.width > 400.0,
        "a menu at the pointer would run off: {menu:?}"
    );
    assert!(
        menu.x + menu.width <= 400.0,
        "inside on the right: {menu:?}"
    );
    assert!(
        menu.y + menu.height <= 320.0,
        "inside at the bottom: {menu:?}"
    );
}

/// **A comment has a menu of its own**, over its card: the colours, then
/// what can be done to the words.
#[test]
fn over_a_comment_it_is_the_comments_menu() {
    let (path, mut reader, at) = commented("comment");
    reader.right_click_at(at.0, at.1);
    assert!(reader.harness.query(".menu.context").is_none());
    assert_eq!(
        reader
            .attribute_all(".mark-popover .mark-swatch", "data-colour")
            .len(),
        6
    );
    assert_eq!(
        reader.attribute_all(".mark-popover .menu-item", "data-item"),
        vec!["edit", "copy-comment", "recolour", "uncomment"]
    );
    reader.click("[data-item='copy-comment']");
    assert_eq!(reader.copied(), vec!["Worth a second look"]);

    reader.right_click_at(at.0, at.1);
    reader.click("[data-item='uncomment']");
    let marks = render::open(&path).expect("reopens").markup();
    assert_eq!(marks.len(), 1, "the passage keeps its mark");
    assert_eq!(marks[0].note, "");
}

/// A comment written onto a fresh mark, and the middle of its card.
fn commented(name: &str) -> (String, Reader, (f32, f32)) {
    let path = readable(name);
    // Fit page, for room beside it: at fit width a comment is a badge.
    let options = Options {
        settings: vec![("fit_mode".into(), serde_json::json!("page"))],
        ..Options::default()
    };
    let mut reader = Reader::open_with(&path, options);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-comment");
    reader.type_text("Worth a second look");
    reader.press_chord("mod+enter");
    let card = reader.harness.layout_rect(".note-card");
    let at = (card.x + card.width / 2.0, card.y + card.height / 2.0);
    (path, reader, at)
}

/// **A click on a comment is a right-click**, and a double click writes in
/// the card itself: no second field anywhere else.
#[test]
fn a_comment_is_clicked_for_its_menu_and_double_clicked_to_edit() {
    let (path, mut reader, at) = commented("click-comment");
    let note = || {
        render::open(&path).expect("reopens").markup()[0]
            .note
            .clone()
    };
    reader.click_at(at.0, at.1);
    assert_eq!(
        reader.attribute_all(".mark-popover .menu-item", "data-item"),
        vec!["edit", "copy-comment", "recolour", "uncomment"]
    );
    reader.press("Escape");

    reader.double_click_at(at.0, at.1);
    assert!(
        reader.harness.query(".mark-popover").is_none(),
        "no menu while editing"
    );
    assert!(reader.harness.query(".note-card.editing").is_some());
    assert_eq!(reader.field(".note-card-field"), "Worth a second look");

    // A press outside the card is done with it, as Done and Enter are.
    reader.type_text(", twice");
    assert_eq!(
        reader.field(".note-card-field"),
        "Worth a second look, twice"
    );
    reader.click_on_page(1, (0.5, 0.6));
    assert_eq!(note(), "Worth a second look, twice");
    assert!(reader.harness.query(".note-card-field").is_none());

    let card = reader.harness.layout_rect(".note-card");
    let at = (card.x + card.width / 2.0, card.y + card.height / 2.0);
    reader.click_at(at.0, at.1);
    reader.click("[data-item='edit']");
    reader.type_text("!");
    reader.click(".note-card-done");
    assert_eq!(note(), "Worth a second look, twice!");
    assert!(
        reader.harness.query(".mark-popover").is_none(),
        "and the menu with it"
    );

    reader.click(".note-card-edit");
    assert!(
        reader.harness.query(".mark-popover").is_none(),
        "Edit is no click on the card"
    );
    reader.type_text("!");
    reader.press_chord("mod+enter");
    assert_eq!(note(), "Worth a second look, twice!!", "Enter is Done");

    // Escape leaves it as it was.
    let card = reader.harness.layout_rect(".note-card");
    let at = (card.x + card.width / 2.0, card.y + card.height / 2.0);
    reader.click_at(at.0, at.1);
    reader.click("[data-item='edit']");
    reader.type_text(" never mind");
    // ⌘A is the field's, not the document's.
    reader.press_chord("mod+a");
    assert!(
        reader.harness.query(".selected").is_none(),
        "nothing selected on the page"
    );
    reader.press("Escape");
    assert_eq!(note(), "Worth a second look, twice!!");
    assert!(reader.harness.query(".note-card-field").is_none());
}

/// **"Change colour…" changes the colour**: the window it opens offers each
/// of the six for the highlight, the one it is in said to be in use.
#[test]
fn the_colours_window_opened_over_a_mark_can_use_one() {
    let path = readable("use-colour");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    reader.right_click_on_page(1, (0.30, LINE));
    reader.click("[data-item='recolour']");
    assert_eq!(reader.harness.query_all(".colours-use.in-use").len(), 1);
    assert_eq!(reader.harness.query_all("button.colours-use").len(), 5);
    let in_use = reader.harness.layout_rect(".colours-use.in-use");
    let apply = reader.harness.layout_rect("button.colours-use");
    assert!(
        (in_use.x - apply.x).abs() < 0.5,
        "{} against {}",
        in_use.x,
        apply.x
    );

    let colours = reader.attribute_all(".mark-swatch", "data-colour");
    let was = render::open(&path).expect("reopens").markup()[0]
        .color
        .clone();
    reader.click_nth("button.colours-use", 2);
    assert!(
        reader.harness.query(".colours-window").is_none(),
        "used, and gone"
    );
    let now = render::open(&path).expect("reopens").markup()[0]
        .color
        .clone();
    assert!(!now.eq_ignore_ascii_case(&was), "{now} was {was}");
    assert!(
        colours.iter().any(|c| c.eq_ignore_ascii_case(&now)),
        "{now} in {colours:?}"
    );
}

/// **A comment on a passage not yet marked is written in a card too**, beside
/// the page — no field of its own anywhere else.
#[test]
fn a_new_comment_is_written_in_its_card() {
    // Beside the page, where there is room…
    let (path, mut reader, _) = commented("first-card");
    reader.press("p");
    reader.type_text("2");
    reader.press("Enter");
    reader.sweep_page(2, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-comment");
    assert!(reader.harness.query(".markup-popover").is_none());
    let card = reader.harness.layout_rect(".note-card.editing");
    let (right, _) = reader.point_on(2, (1.0, 0.0));
    assert!(
        card.x >= right,
        "beside the page: {} against {right}",
        card.x
    );
    reader.type_text("And this");
    reader.click(".note-card-done");
    let notes: Vec<String> = render::open(&path)
        .expect("reopens")
        .markup()
        .into_iter()
        .map(|mark| mark.note)
        .collect();
    assert!(notes.contains(&"And this".to_string()), "{notes:?}");

    // …and under the line where there is no room there.
    let path = readable("first-under");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-comment");
    assert!(reader.harness.query(".markup-popover").is_none());
    assert!(reader
        .harness
        .query(".note-card.editing .note-card-field")
        .is_some());
    reader.type_text("First");
    reader.press_chord("mod+enter");
    assert_eq!(
        render::open(&path).expect("reopens").markup()[0].note,
        "First"
    );
}

/// **A press in the margin beside a page puts the colours away**, as one on
/// the page does.
#[test]
fn a_press_beside_the_page_puts_the_popovers_away() {
    let path = readable("margin");
    let mut reader = open(&path);
    reader.press_action(moonowl::keymap::Action::FitPage);
    let page = reader.harness.layout_rect(".page");
    let viewer = reader.harness.layout_rect(".viewer");
    assert!(page.x - viewer.x > 20.0, "a margin to press in");
    let margin = (viewer.x + (page.x - viewer.x) / 2.0, page.y + 40.0);

    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    assert!(reader.harness.query(".markup-popover").is_some());
    reader.click_at(margin.0, margin.1);
    assert!(reader.harness.query(".markup-popover").is_none());

    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    reader.right_click_on_page(1, (0.30, LINE));
    assert!(reader.harness.query(".mark-popover").is_some());
    reader.click_at(margin.0, margin.1);
    assert!(reader.harness.query(".mark-popover").is_none());
}

/// **Edit never shrinks a card**: the field is as tall as the words.
#[test]
fn edit_keeps_the_card_as_tall_as_it_was() {
    let (_, mut reader, _) = commented("edit-tall");
    reader.click(".note-card-edit");
    reader.type_text(&" and a longer thought to go with it".repeat(6));
    reader.press_chord("mod+enter");
    let shown = reader.harness.layout_rect(".note-card");
    reader.click(".note-card-edit");
    let editing = reader.harness.layout_rect(".note-card.editing");
    assert!(
        editing.height >= shown.height,
        "{} against {}",
        editing.height,
        shown.height
    );
}

/// **All of a long comment can be reached**: its card is no taller than the
/// window, and its words scroll inside it — under the wheel, which then
/// leaves the document where it is — and so do they while it is edited.
#[test]
fn a_long_comment_scrolls_in_its_card() {
    let (_, mut reader, _) = commented("long");
    reader.click(".note-card-edit");
    reader.type_text(&" and a longer thought to go with it".repeat(60));
    reader.press_chord("mod+enter");
    let window = reader.harness.layout_rect(".viewer");
    let card = reader.harness.layout_rect(".note-card");
    assert!(card.height <= window.height, "{card:?} in {window:?}");
    // And the document scrolls far enough to show all of it.
    reader.wheel((card.y + card.height - window.y - window.height + 40.0) as f64);
    let card = reader.harness.layout_rect(".note-card");
    assert!(
        card.y + card.height <= window.y + window.height,
        "{card:?} in {window:?}"
    );

    let text = reader.harness.node(".note-card-text.scrolls");
    let scroll = reader.state().scroll;
    reader.wheel_over(".note-card-text", 200.0);
    let moved = reader
        .harness
        .base()
        .get_node(text)
        .unwrap()
        .scroll_offset()
        .y;
    assert!(moved > 0.0, "the words scrolled");
    assert_eq!(reader.state().scroll, scroll, "the document did not");

    reader.click(".note-card-edit");
    let field = reader.harness.layout_rect(".note-card-field");
    assert!(
        field.y + field.height <= window.y + window.height,
        "{field:?} in {window:?}"
    );
    // Its lines wrap inside the field rather than run on past its edge (the
    // harness draws at a scale of 1, so the editor's pixels are CSS pixels).
    // A space at a line's end hangs past it, as CSS has it, and is not ink.
    let base = reader.harness.base();
    let node = base
        .get_node(reader.harness.node(".note-card-field"))
        .unwrap();
    let written = node
        .element_data()
        .and_then(|el| el.text_input_data())
        .and_then(|input| input.editor.try_layout())
        .map(|layout| {
            layout
                .lines()
                .map(|line| line.metrics().advance - line.metrics().hanging_advance)
                .fold(0.0, f32::max)
        })
        .unwrap();
    let inside = node.final_layout().content_box_width();
    assert!(written <= inside + 0.5, "{written} in {inside}");
}

/// **A long comment in a mark's menu is held to eight lines**, which scroll,
/// so the rest of the menu stays in the window — and written in, the card
/// stays where it was and as tall as it was.
#[test]
fn a_long_comment_in_a_marks_menu_keeps_the_menu_whole() {
    let path = readable("long-menu");
    let mut reader = Reader::open_with(
        &path,
        Options {
            width: 700,
            height: 800,
            ..Options::default()
        },
    );
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-comment");
    reader.type_text(&" and a longer thought to go with it".repeat(20));
    reader.press_chord("mod+enter");
    reader.click_on_page(1, (0.30, LINE));
    let window = reader.harness.layout_rect(".viewer");
    let last = reader
        .harness
        .layout_rect(".mark-popover [data-item='remove']");
    assert!(
        last.y + last.height <= window.y + window.height,
        "{last:?} in {window:?}"
    );
    assert!(reader
        .harness
        .query(".mark-note .note-card-text.scrolls")
        .is_some());
    let card = reader.harness.layout_rect(".mark-note");
    reader.click(".mark-note .note-card-edit");
    let editing = reader.harness.layout_rect(".note-card.editing");
    assert!(
        (editing.y - card.y).abs() < 1.0 && (editing.height - card.height).abs() < 2.0,
        "{editing:?} over {card:?}"
    );
}

/// **A comment is written in the card it was asked of**: from a click on the
/// passage, in the menu's card under it, not the one beside the page.
#[test]
fn a_comment_edited_from_the_passage_is_written_under_it() {
    let (_, mut reader, _) = commented("edit-from-passage");
    let page = reader.harness.layout_rect(".page");
    reader.click_on_page(1, (0.30, LINE));
    let card = reader.harness.layout_rect(".mark-note");
    reader.click(".mark-note .note-card-edit");
    assert_eq!(reader.harness.query_all(".note-card-field").len(), 1);
    let editing = reader.harness.layout_rect(".note-card.editing");
    assert!(editing.x < page.x + page.width, "{editing:?} on {page:?}");
    assert!(
        (editing.y - card.y).abs() < 1.0,
        "{editing:?} over {card:?}"
    );
    assert_eq!(reader.field(".note-card-field"), "Worth a second look");
}

/// **Enter is a new line in a comment**, as it is wherever words are
/// written, and ⌘Enter is Done.
#[test]
fn enter_is_a_new_line_in_a_comment() {
    let (path, mut reader, _) = commented("new-line");
    reader.click(".note-card-edit");
    reader.press("Enter");
    reader.type_text("A second line");
    assert!(
        reader.harness.query(".note-card-field").is_some(),
        "still writing"
    );
    reader.press_chord("mod+enter");
    assert!(reader.harness.query(".note-card-field").is_none(), "done");
    assert_eq!(
        render::open(&path).expect("reopens").markup()[0].note,
        "Worth a second look\nA second line"
    );
}
