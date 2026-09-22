//! The toolbar's menus, and the door they opened.
//!
//! Two of the four things a reader looking at this experiment beside the app
//! notices first: there are no dropdown menus, and there is no way to open
//! another document. They are one piece of work — the picker is a menu item
//! before it is anything else — and this is what says so.
//!
//! What is *not* here is anything about the picker itself. `Pick` is a
//! context holding one closure for the reason `Clip` is one: the real answer
//! is a modal window belonging to the operating system, and a suite that
//! opened one would sit there until somebody clicked it. So the harness
//! answers with a path and what is tested is everything downstream of the
//! answer.

use moonowl::app::Ask;
use moonowl::fixture;
use moonowl::harness::{Options, Reader};

fn reader() -> Reader {
    // **1280, not the harness's 1100.** The Document menu hangs off the
    // document's *name*, and the name is on `.bar-left` — the side that gives
    // way. At 1100 there is no bar left to give it: it is squeezed to its
    // floor and, on a machine whose `ui-sans-serif` is wider than SF Pro,
    // outside its own group, where Blitz does not hit-test it. 1280 is the
    // width the app's own inventory was taken at, and there is room in it on
    // all three platforms.
    Reader::open_with(
        &fixture::contents_pdf(),
        Options {
            width: 1280,
            ..Options::default()
        },
    )
}

/// Deleting a theme from the menu asks first, in a window of its own.
#[test]
fn deleting_a_theme_from_the_menu_asks_first() {
    let dir = std::env::temp_dir().join(format!("moonowl-menu-delete-{}", std::process::id()));
    let themes = dir.join("themes");
    std::fs::create_dir_all(&themes).expect("a themes directory");
    let file = themes.join("Mine.toml");
    std::fs::write(
        &file,
        "name = \"Mine\"\ntext = \"#e8e8e8\"\nbackground = \"#101018\"\n",
    )
    .expect("write a theme");
    let mut reader = Reader::open_with(
        &fixture::contents_pdf(),
        Options {
            width: 1280,
            config: dir.clone(),
            settings: vec![("theme".into(), serde_json::json!("Mine"))],
            ..Options::default()
        },
    );
    reader.click(".chip.theme");
    reader.click("[data-item=delete-theme]");
    assert!(
        file.exists(),
        "the menu item deleted the theme without asking"
    );
    assert!(reader
        .harness
        .query("[aria-label='Delete theme']")
        .is_some());
    reader.press("Escape");
    assert!(
        file.exists()
            && reader
                .harness
                .query("[aria-label='Delete theme']")
                .is_none()
    );

    reader.click(".chip.theme");
    reader.click("[data-item=delete-theme]");
    reader.click("[aria-label='Delete theme'] .danger");
    assert!(!file.exists(), "Delete theme deletes it");
    let _ = std::fs::remove_dir_all(&dir);
}

/// One at a time, and every way out of one.
#[test]
fn a_menu_opens_and_closes() {
    let mut reader = reader();
    assert_eq!(reader.state().menu, None, "nothing is down to start with");

    reader.click(".chip.theme");
    assert_eq!(reader.state().menu.as_deref(), Some("theme"));

    // A second menu replaces the first rather than joining it. Nothing inside
    // one menu knows the others exist, which is why the state is the
    // reader's — see `app::Menu`.
    reader.click(".chip.fit");
    assert_eq!(reader.state().menu.as_deref(), Some("view"));

    // Its own button closes it, which is the gesture that would have been
    // broken by dismissing on every press: the press would close it on the
    // way down and the click would open it straight back up.
    reader.click(".chip.fit");
    assert_eq!(reader.state().menu, None);

    reader.click(".chip.theme");
    reader.press("Escape");
    assert_eq!(reader.state().menu, None, "Escape closes the menu");

    // And a press anywhere the menu is not. The document is the ordinary
    // "anywhere" and is also the one place a press does something else, so it
    // is the one worth pressing.
    reader.click(".chip.theme");
    reader.click(".viewer");
    assert_eq!(reader.state().menu, None, "a press elsewhere closes it");
}

/// **A menu and the find bar are never both up**, so Escape has one thing to
/// close and closes it.
///
/// This used to assert the other thing — that Escape takes the menu first and
/// leaves the bar — and it was testing a state the app cannot reach. `wire()`
/// in `main.ts` wraps every control in the bar that opens something of its own
/// in `opens(…)`, which closes the search on the way, and its reason is worth
/// keeping: two panels claiming the same corner of the screen, one of them
/// still holding the keyboard, is not a place anybody meant to be. `show_menu`
/// does the same for all five menus at once. The ordering in `dismiss` is
/// still menu-before-bar and still right; there is simply nothing left that
/// can put a reader in front of both.
#[test]
fn opening_a_menu_puts_the_find_bar_away_and_escape_closes_the_menu() {
    let mut reader = reader();
    reader.press_chord("mod+f");
    assert!(reader.state().find.is_some());

    reader.click(".chip.theme");
    assert_eq!(reader.state().menu.as_deref(), Some("theme"));
    assert!(
        reader.state().find.is_none(),
        "the search stayed up behind the menu",
    );

    reader.press("Escape");
    assert_eq!(reader.state().menu, None);
}

/// Fifteen themes reached by pressing `t` fifteen times is what a menu is
/// for. The list is every theme installed, the one in use is ticked, and
/// choosing one wears it.
#[test]
fn the_theme_menu_is_the_whole_list() {
    let mut reader = reader();
    reader.click(".chip.theme");
    // The themes themselves carry a swatch; the three items under them —
    // New theme…, Make a copy…, All appearance settings… — do not, which is
    // what tells the two apart without counting from the end.
    let names = reader
        .harness
        .query_all(".menu.theme .menu-item .swatch")
        .len();
    assert_eq!(names, 15, "every shipped theme is in the menu");
    assert_eq!(
        reader.harness.query_all(".menu.theme .menu-item.on").len(),
        1,
        "exactly one is ticked",
    );

    reader.click_nth(".menu.theme .menu-item", 2);
    assert_eq!(reader.state().theme, "Tokyo Night");
    // **And the menu stays.** A theme is something you try on, so the tick
    // moves and the list is still there — `showThemeMenu` in `main.ts` puts
    // the menu away only for the items that take you somewhere else.
    assert_eq!(reader.state().menu.as_deref(), Some("theme"));
    reader.press("Escape");
    assert_eq!(reader.state().menu, None);
}

/// **The spreads are under the cog, which is where the app keeps them.**
/// `showSettingsMenu` in `main.ts` has them under "Pages side by side"; they
/// were in the zoom menu here, which is a menu about how big a page is.
#[test]
fn the_settings_menu_chooses_a_spread() {
    let mut reader = reader();
    let one = reader.state().mounted.len();
    reader.click(".chip.settings");
    // Continuous, one page at a time, then the three spreads.
    reader.click_nth(".menu.settings .menu-item", 3);
    assert_eq!(reader.state().menu, None);
    assert!(
        reader.state().mounted.len() > one,
        "two pages side by side mount more than one did",
    );
}

/// And the zoom menu is the app's: the three fits, a number to type, and the
/// presets under it. It puts nothing away — a zoom is something you try on.
#[test]
fn the_zoom_menu_offers_a_number_and_the_presets() {
    let mut reader = reader();
    reader.click(".chip.fit");
    assert!(
        reader.harness.query(".menu.view .stepper").is_some(),
        "a number to type",
    );
    // Fit width, fit page, actual size, then 50%.
    reader.click_nth(".menu.view .menu-item", 3);
    assert_eq!(reader.state().zoom, "50%");
    assert_eq!(
        reader.state().menu.as_deref(),
        Some("view"),
        "and the menu is still up",
    );
}

/// **A number typed into the zoom field is applied when it is typed out.**
/// Live, "150" passed through 1 and 15 — each clamped to 25% and each a
/// relayout of the document, at four times the pages.
#[test]
fn the_zoom_field_waits_for_the_whole_number() {
    let mut reader = reader();
    reader.click(".chip.fit");
    reader.click(".menu.view .step-field");
    reader.type_text("1");
    assert_ne!(
        reader.state().zoom,
        "25%",
        "the 1 of 150 was applied, clamped to the smallest zoom there is"
    );
    reader.type_text("50");
    reader.press("Enter");
    assert_eq!(reader.state().zoom, "150%");
}

/// **A menu item says which key asks for the same thing, and it reads that
/// off the keymap.** A hand-written chord beside a menu item cannot show a
/// rebound one, which is the drift the app's Keyboard page was rewritten to
/// stop — see `Viewer::chord_for`.
#[test]
fn a_menu_item_shows_the_key_that_is_actually_bound() {
    let mut reader = Reader::open_with(
        &fixture::contents_pdf(),
        Options {
            keys: [("open".to_string(), vec!["mod+shift+o".to_string()])]
                .into_iter()
                .collect(),
            ..Options::default()
        },
    );
    // Open is a menu of its own now, beside the document's rather than
    // inside it — see `Menu::Open`. The first key in it is Open's, which is
    // the item that was rebound.
    reader.click(".chip.open");
    let shown = reader.harness.text_content(".menu.open .menu-key");
    assert!(
        shown.contains('O') && (shown.contains('⇧') || shown.contains("Shift")),
        "the rebound chord is what the menu shows, not the shipped one: {shown:?}",
    );
}

/// ⌘O, and the whole of what opening a document in this window means.
#[test]
fn open_puts_a_different_document_in_this_window() {
    let second = fixture::titled_pdf("The Second One");
    let mut reader = Reader::open_with(
        &fixture::contents_pdf(),
        Options {
            picks: vec![second.clone()],
            ..Options::default()
        },
    );
    let before = reader.state();
    assert_eq!(before.pages, 12);

    reader.press_chord("mod+o");
    let after = reader.state();
    assert_eq!(after.pages, 3, "the new document's pages");
    assert_eq!(after.title, "The Second One", "and its name");
    assert_eq!(after.page, 1, "opened at the front");

    // What the process has to be told, and the only route to it: the desk and
    // the watch are neither of them the window's. See `Ask::Showing`.
    assert!(
        reader.asks().iter().any(|ask| matches!(
            ask,
            Ask::Showing { path, title } if path == &second && title == "The Second One"
        )),
        "the window said what it is showing now: {:?}",
        reader.asks(),
    );
}

/// A picker the reader closed is not a document, and nothing moves.
#[test]
fn a_cancelled_picker_changes_nothing() {
    let mut reader = reader();
    let before = reader.state();
    reader.press_chord("mod+o");
    assert_eq!(reader.state().pages, before.pages);
    assert_eq!(reader.state().title, before.title);
    assert!(
        !reader
            .asks()
            .iter()
            .any(|ask| matches!(ask, Ask::Showing { .. })),
        "nothing was said about a document that was never chosen",
    );
}

/// The same picker, the other door: a window of its own, and this one left
/// exactly as it was. This is the app's "Open document in new window…", which
/// is a menu item there and not a key — so it is a menu item here too.
#[test]
fn open_in_a_new_window_leaves_this_one_alone() {
    let second = fixture::titled_pdf("Beside It");
    let mut reader = Reader::open_with(
        &fixture::contents_pdf(),
        Options {
            picks: vec![second.clone()],
            ..Options::default()
        },
    );
    let before = reader.state();

    reader.click(".chip.open");
    reader.click_nth(".menu.open .menu-item", 1);

    assert_eq!(
        reader.state().pages,
        before.pages,
        "this window is untouched"
    );
    assert_eq!(reader.state().title, before.title);
    assert_eq!(
        reader.asks().last(),
        Some(&Ask::NewWindowOn(second)),
        "and a window was asked for on the other document",
    );
}

/// Opening the document that is already here is not an open at all.
#[test]
fn opening_what_is_already_open_says_so() {
    let here = fixture::contents_pdf();
    let mut reader = Reader::open_with(
        &here,
        Options {
            picks: vec![here.clone()],
            ..Options::default()
        },
    );
    reader.press_chord("mod+o");
    assert!(
        reader.state().notice.contains("already open"),
        "said so: {:?}",
        reader.state().notice,
    );
    assert!(
        !reader
            .asks()
            .iter()
            .any(|ask| matches!(ask, Ask::Showing { .. })),
        "and told nobody anything had changed",
    );
}

/// Escape closes the Information window, which it did not.
///
/// The note window and this one are the same kind of thing in the same place
/// — a window over the reader, read once and dismissed — and `Action::Dismiss`
/// worked outward through the menus, the popovers, Settings and the note and
/// then stepped straight over this one. So a reader who opened it with the
/// pointer had to close it with the pointer, in an app where Escape closes
/// everything else that floats.
#[test]
fn escape_closes_the_information_window() {
    let mut reader = reader();
    reader.click(".chip.title");
    reader.click("[data-item='information']");
    assert!(
        reader.harness.query(".details-window").is_some(),
        "the window is up",
    );
    reader.press("Escape");
    assert!(
        reader.harness.query(".details-window").is_none(),
        "and Escape puts it away, like every other window over the reader",
    );
}

/// **The Information window fits what is in it, and its values are ranged
/// right.**
///
/// Both are `ui.field` under `.window[data-size="tall"]` in the app, and this
/// window had neither: it kept `.window`'s fixed 600px height, so a paper with
/// four facts came up in a frame with a hand's breadth of nothing under the
/// last of them, and its rows were a two-column grid with the value ranged left
/// after a label in a third of the width. A column of right-ranged values has
/// an edge to read down; a column of left-ranged ones does not.
#[test]
fn the_information_window_fits_its_rows_and_ranges_them_right() {
    let mut reader = reader();
    reader.click(".chip.title");
    reader.click("[data-item='information']");

    let window = reader.harness.layout_rect(".details-window");
    let rows = reader.harness.query_all(".details-row").len();
    assert!(rows >= 3, "the fixture names a few facts: {rows}");
    assert!(
        window.height < 500.0,
        "a window of {rows} rows was {}px tall — it is wearing the fixed \
         frame Settings needs rather than fitting what is in it",
        window.height,
    );

    // The last row's value ends where its row ends, give or take the pane's own
    // padding. The label starts where the row starts.
    let row = reader.harness.layout_rect(".details-row");
    let label = reader.harness.layout_rect(".details-label");
    let value = reader.harness.layout_rect(".details-value");
    assert!(
        (label.x - row.x).abs() < 2.0,
        "the label is at the left of its row: {} against {}",
        label.x,
        row.x,
    );
    assert!(
        ((row.x + row.width) - (value.x + value.width)).abs() < 2.0,
        "the value is at the right of its row: {} against {}",
        value.x + value.width,
        row.x + row.width,
    );
}

/// **Fourteen themes, all on screen at once.** The list was capped at 60% of
/// the window and the last few were behind a scroll — a scroll to reach a
/// thing the reader is choosing between. The cap is now the window less the
/// toolbar and a margin, which is where the app's own overflow correction
/// leaves it.
#[test]
fn the_theme_menu_shows_every_theme_without_a_scroll() {
    let mut reader = reader();
    reader.click(".chip.theme");
    let menu = reader.harness.layout_rect(".menu.theme");
    let rows = reader.harness.query_all(".menu.theme .menu-item");
    let last = reader.harness.layout_rect_of(*rows.last().expect("themes"));
    assert!(
        last.y + last.height <= menu.y + menu.height,
        "the last row is inside the menu: {last:?} in {menu:?}",
    );
    let window = reader.harness.layout_rect(".root");
    assert!(
        menu.y + menu.height <= window.y + window.height,
        "and the menu is inside the window: {menu:?} in {window:?}",
    );
}

/// **A row's note goes under its label, not beside it.** Side by side the two
/// shared the line, so "Recolour pictures too" with "Off leaves them as
/// printed." beside it came out as two lines of label and two of note in a
/// menu wide enough for both on one.
#[test]
fn a_settings_row_keeps_its_label_and_its_note_each_on_one_line() {
    let mut reader = reader();
    reader.click(".chip.settings");
    // The first row's label is a short one and fits whatever the layout does,
    // so it is the height of one line to measure the rest against.
    let line = reader.harness.layout_rect(".menu.settings .menu-row-label");
    let height = line.height;
    for selector in [
        ".menu.settings .menu-row-label",
        ".menu.settings .menu-row-note",
    ] {
        for node in reader.harness.query_all(selector) {
            let rect = reader.harness.layout_rect_of(node);
            assert!(
                rect.height <= height * 1.5,
                "one line: {selector} at {rect:?} against {height}",
            );
        }
    }
}
