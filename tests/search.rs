//! The find bar, through the interface.
//!
//! `search.rs`'s own tests are about folding and locating and need no
//! document; these are about the other half — that a keystroke opens the bar,
//! that what is typed into it reaches the scan, that the reader is taken to
//! the match, that the rectangles land on the page, and that the three
//! switches do what they say. Everything is read off the interface the way
//! somebody looking at the screen would read it, which is the rule
//! `PROGRESS.md` sets for `state()`.

use moonowl::fixture;
use moonowl::harness::{Options, Reader};

/// A reader over the six pages of prose, with the find bar already up.
/// The same, with the search opening the panel on its results — a setting,
/// and off unless the reader turns it on.
fn searching_into_the_panel() -> Reader {
    let mut reader = Reader::open_with(
        &fixture::prose_pdf(),
        Options {
            settings: vec![("search_shows_sidebar".into(), serde_json::json!(true))],
            ..Options::default()
        },
    );
    reader.press_chord("mod+f");
    reader
}

fn searching() -> Reader {
    let mut reader = Reader::open_with(&fixture::prose_pdf(), Options::default());
    reader.press_chord("mod+f");
    reader
}

/// Type a query and let the scan finish.
fn look_for(reader: &mut Reader, query: &str) {
    reader.type_text(query);
    reader.scan_out();
}

#[test]
fn the_find_bar_opens_on_the_shortcut_and_closes_on_escape() {
    let mut reader = Reader::open_with(&fixture::prose_pdf(), Options::default());
    assert_eq!(reader.state().find, None, "the bar is not up to begin with");
    reader.press_chord("mod+f");
    assert_eq!(
        reader.state().find,
        Some(String::new()),
        "up, and saying nothing until there is something to say",
    );
    reader.press("Escape");
    assert_eq!(reader.state().find, None);
}

#[test]
fn what_is_typed_is_searched_for_and_counted() {
    let mut reader = searching();
    look_for(&mut reader, "needle");
    let state = reader.state();
    assert_eq!(state.query, "needle");
    // Three: one on page 1 and two on page 3.
    assert_eq!(state.find.as_deref(), Some("1 of 3"));
}

/// The reader is taken to the first match, and the page it lands on is the
/// page the match is on.
#[test]
fn the_first_match_brings_the_reader_to_it() {
    let mut reader = searching();
    // Start on page 3, so that "first" cannot mean "the top of the document".
    look_for(&mut reader, "beside");
    assert_eq!(reader.state().page, 3);
    assert_eq!(reader.state().find.as_deref(), Some("1 of 1"));
}

/// And the scan starts at the page being read, so the match it settles on is
/// the one under the reader's eyes rather than the first in the book.
#[test]
fn the_scan_starts_where_the_reader_is() {
    let mut reader = searching();
    look_for(&mut reader, "needle");
    // From the top, the first of three is on page 1.
    assert_eq!(reader.state().page, 1);
    reader.press("Escape");
    // From page 3 it is the one on page 3 — the same three matches, a
    // different one to settle on, because the scan starts under the reader's
    // eyes rather than at the front of the book.
    reader.press("l");
    reader.press("l");
    assert_eq!(reader.state().page, 3);
    // The bar comes back holding "needle" and looks for it again — see
    // `reopening_the_bar_brings_the_query_back` — so the scan starts here.
    reader.press_chord("mod+f");
    reader.scan_out();
    assert_eq!(reader.state().find.as_deref(), Some("2 of 3"));
    assert_eq!(reader.state().page, 3);
}

#[test]
fn stepping_walks_the_matches_and_wraps() {
    let mut reader = searching();
    look_for(&mut reader, "needle");
    assert_eq!(reader.state().find.as_deref(), Some("1 of 3"));
    reader.press_chord("mod+g");
    assert_eq!(reader.state().find.as_deref(), Some("2 of 3"));
    reader.press_chord("mod+g");
    assert_eq!(reader.state().find.as_deref(), Some("3 of 3"));
    reader.press_chord("mod+g");
    assert_eq!(reader.state().find.as_deref(), Some("1 of 3"), "and round");
    reader.press_chord("mod+shift+g");
    assert_eq!(reader.state().find.as_deref(), Some("3 of 3"));
}

/// A match is painted where it is, and "Highlight all" is the only thing that
/// changes how many of them are.
#[test]
fn matches_are_painted_on_the_page_and_the_switch_says_how_many() {
    let mut reader = searching();
    look_for(&mut reader, "needle");
    // Page 3 has two of them, and it is one page: go there.
    reader.press_chord("mod+g");
    let all = reader.state().hits;
    assert!(all >= 2, "two matches on the page, {all} painted");
    reader.click(".find-all");
    let one = reader.state().hits;
    assert_eq!(one, 1, "only the match the reader is on");
    reader.click(".find-all");
    assert_eq!(reader.state().hits, all, "and back");
}

/// Where a highlight is drawn is the question the whole of `Rect` exists
/// for, so it is asked directly: the rectangle is inside the page it belongs
/// to, and it is not the whole of it.
#[test]
fn a_highlight_is_a_rectangle_inside_its_page() {
    let mut reader = searching();
    look_for(&mut reader, "needle");
    let page = reader.harness.layout_rect(".page");
    let hit = reader.harness.layout_rect(".hit");
    assert!(hit.width > 0.0 && hit.height > 0.0, "{hit:?}");
    assert!(
        hit.width < page.width / 2.0,
        "a word is not half the page: {hit:?} in {page:?}",
    );
    assert!(hit.height < page.height / 10.0, "{hit:?}");
}

/// The three things the fold is for, through the renderer rather than in
/// isolation — see `fixture::prose_pdf`, which found that two of the three
/// answers are not the ones the app gets from pdf.js.
#[test]
fn an_accent_a_ligature_and_a_soft_hyphen_are_all_findable_by_typing_the_word() {
    for (query, page) in [("resume", 5), ("find", 4), ("typography", 6)] {
        let mut reader = searching();
        look_for(&mut reader, query);
        let state = reader.state();
        assert!(
            state
                .find
                .as_deref()
                .is_some_and(|said| said.starts_with("1 of ")),
            "{query}: {:?}",
            state.find,
        );
        assert_eq!(state.page, page, "{query} is on page {page}");
    }
}

/// "Match case" and "Whole words" change what is found; both are settings and
/// both outlive the bar they are set from.
#[test]
fn the_two_switches_change_what_is_found_and_are_remembered() {
    let config = std::env::temp_dir().join(format!("moonowl-switches-{}", std::process::id()));
    let mut reader = Reader::open_with(
        &fixture::prose_pdf(),
        Options {
            config: config.clone(),
            ..Options::default()
        },
    );
    reader.press_chord("mod+f");
    // "in" is in "in the first page" and inside "Nothing", "again", "find"
    // and "line" — so the whole-words switch has something to take away.
    look_for(&mut reader, "in");
    let loose = reader.state().find.expect("a count");
    reader.click(".find-words");
    reader.scan_out();
    let whole = reader.state().find.expect("a count");
    assert_ne!(loose, whole, "whole words changed nothing: {loose}");
    assert_eq!(whole, "1 of 1");

    // And "The" is capitalised on three pages while "the" is lower case on
    // one, which is what the case switch has to see.
    reader.press("Escape");
    reader.press_chord("mod+f");
    // The bar came back holding the last query: over it, as a reader would.
    reader.press_chord("mod+a");
    look_for(&mut reader, "The");
    let insensitive = reader.state().find.expect("a count");
    reader.click(".find-case");
    reader.scan_out();
    let cased = reader.state().find.expect("a count");
    assert_ne!(
        insensitive, cased,
        "match case changed nothing: {insensitive}"
    );

    // Both survive the reader being closed and opened again, which is what
    // makes them settings rather than a state of the bar.
    drop(reader);
    let mut again = Reader::open_with(
        &fixture::prose_pdf(),
        Options {
            config,
            ..Options::default()
        },
    );
    again.press_chord("mod+f");
    look_for(&mut again, "in");
    assert_eq!(
        again.state().find.as_deref(),
        Some(whole.as_str()),
        "the switches did not survive the restart",
    );
}

/// A document with nothing to search says so, which is a different sentence
/// from a document that does not contain the word.
#[test]
fn a_word_that_is_not_there_and_a_document_with_no_words_say_different_things() {
    let mut reader = searching();
    look_for(&mut reader, "aardvark");
    assert_eq!(reader.state().find.as_deref(), Some("None"));
    assert_eq!(reader.state().hits, 0);
}

/// The results tab is there while the bar is, and a row goes to its match.
#[test]
fn the_results_tab_lists_the_matches_and_a_row_goes_to_one() {
    let mut reader = Reader::open_with(&fixture::prose_pdf(), Options::default());
    reader.press_chord("mod+b");
    // The prose fixture carries no outline, so the panel opens on the pages —
    // which is `setDocument`'s own rule and the difference between a panel
    // and an empty box.
    assert_eq!(reader.state().sidebar.as_deref(), Some("pages"));
    reader.press_chord("mod+f");
    look_for(&mut reader, "needle");
    assert_eq!(
        reader.state().sidebar.as_deref(),
        Some("results"),
        "searching shows the results",
    );
    assert_eq!(reader.state().results, vec![0, 1, 2]);
    reader.click_nth(".result", 2);
    assert_eq!(reader.state().find.as_deref(), Some("3 of 3"));
    assert_eq!(reader.state().page, 3);

    // And it goes when the bar does, rather than sitting there empty.
    reader.click(".find-close");
    assert_eq!(reader.state().find, None);
    assert_eq!(reader.state().sidebar.as_deref(), Some("pages"));
    assert!(reader.state().results.is_empty());
}

/// **A key typed into the field is not a shortcut.** Every keystroke in the
/// field also bubbles to the root, which is where this reader turns keys into
/// actions — so without the field stopping them, typing "just" would scroll
/// the document four times on the way to searching for it.
#[test]
fn typing_into_the_field_does_not_drive_the_document() {
    let mut reader = searching();
    let before = reader.state().scroll;
    // "j" scrolls a line, "g g" goes to the top, "t" changes the theme.
    let theme = reader.state().theme;
    reader.type_text("jggt");
    reader.scan_out();
    assert_eq!(reader.state().query, "jggt");
    assert_eq!(reader.state().scroll, before, "the document moved");
    assert_eq!(reader.state().theme, theme, "the theme changed");
}

/// And a click on one of the bar's own buttons leaves the keyboard in the
/// field, which is the rule `give_keyboard_back` follows: the innermost
/// element that asks for it wins.
#[test]
fn a_click_in_the_find_bar_leaves_the_keyboard_in_the_field() {
    let mut reader = searching();
    look_for(&mut reader, "need");
    reader.click(".find-all");
    reader.type_text("le");
    reader.scan_out();
    assert_eq!(reader.state().query, "needle");
}

/// Closing the bar puts the index down, which is the whole memory policy —
/// and the highlights go with it, because a mark on the page that outlives
/// the bar that made it is a mark nobody asked for.
#[test]
fn closing_the_bar_takes_the_highlights_with_it() {
    let mut reader = searching();
    look_for(&mut reader, "needle");
    assert!(reader.state().hits > 0);
    reader.press("Escape");
    assert_eq!(reader.state().hits, 0);
    assert_eq!(reader.state().find, None);
}

/// The scan is sliced, so a long document is searched over several turns of
/// the event loop rather than one — which is the only thing standing between
/// the reader and half a second of a window that does not answer.
///
/// Asked of the `Viewer` rather than through the harness, and that is the
/// point: driving it through the interface means `settle()`, and a settle is
/// several turns of the loop, so what a test would be measuring is how many
/// slices a settle happens to run rather than what a slice does. Here the
/// slices are counted.
#[test]
fn one_slice_of_the_scan_does_not_read_the_whole_book() {
    use moonowl::app::Viewer;
    use moonowl::page::Chosen;
    use moonowl::palette::FALLBACK;
    use moonowl::store::Store;

    let config = std::env::temp_dir().join(format!("moonowl-slices-{}", std::process::id()));
    let document = moonowl::render::open(&Reader::book()).expect("the fixture");
    let pages = document.pages();
    let mut viewer = Viewer::new(document, Chosen::new(FALLBACK), Store::at(&config));
    viewer.restore();
    viewer.resize(1100.0, 800.0);
    viewer.open_find();
    // "quick" is on every one of the four hundred pages.
    let token = viewer.find("quick").expect("something to scan");

    assert!(
        viewer.scan_slice(token),
        "one slice read the whole of a {pages}-page book",
    );
    let first = viewer.search.state().total;
    assert!(first > 0, "a slice that found nothing is not a slice");

    let mut slices = 1;
    while viewer.scan_slice(token) {
        slices += 1;
        assert!(slices < pages, "a slice that reads no pages is a loop");
    }
    assert!(slices > 1, "the whole book went in one slice after all");
    assert!(
        viewer.search.state().total > first,
        "the count did not climb: {first} after one slice of {slices}",
    );
    assert!(!viewer.search.state().scanning, "the scan did not finish");

    // And a scan that has been replaced stops at its next slice rather than
    // running to the end of a document nobody is searching any more.
    let stale = token;
    viewer.find("fox");
    assert!(!viewer.scan_slice(stale));
}

/* ------------------------------------------- the list behind the count */

/// **The count is the way through to the results, and the panel it opens is
/// borrowed rather than kept.** "3 of 128" answers *is it in here* and the
/// list answers *which one did I mean* — `el.findStatus` in `main.ts` is the
/// same button for the same reason. What the app does not do is give the
/// panel back, and this reader does: it came up on its own, so one Escape
/// takes it down along with the bar that opened it.
#[test]
fn the_count_opens_the_results_and_escape_puts_them_away() {
    let mut reader = searching();
    assert_eq!(
        reader.state().sidebar,
        None,
        "the panel is shut to start with"
    );
    look_for(&mut reader, "needle");

    reader.click(".find-count");
    assert_eq!(
        reader.state().sidebar.as_deref(),
        Some("results"),
        "the count opened the panel on the list",
    );

    reader.press("Escape");
    assert_eq!(reader.state().find, None, "one Escape takes the bar down");
    assert_eq!(reader.state().sidebar, None, "and the panel it borrowed");
}

/// A panel the reader had open before any of this is a panel the reader
/// keeps. Closing something somebody can see is the sort of tidying that
/// loses people their place.
#[test]
fn a_panel_the_reader_opened_is_not_taken_away() {
    let mut reader = Reader::open_with(&fixture::contents_pdf(), Options::default());
    reader.press_chord("mod+b");
    assert!(reader.state().sidebar.is_some(), "open before the search");
    reader.press_chord("mod+f");
    look_for(&mut reader, "the");
    reader.click(".find-count");

    reader.press("Escape");
    assert!(
        reader.state().sidebar.is_some(),
        "the panel stayed: {:?}",
        reader.state().sidebar,
    );
}

/// **And it goes back to the tab it was on**, not to whichever one the
/// document has: Pages, searched and put away, came back as Contents.
#[test]
fn putting_a_search_away_leaves_the_panel_on_the_tab_it_had() {
    let mut reader = Reader::open_with(&fixture::contents_pdf(), Options::default());
    reader.press_chord("mod+b");
    reader.click(".tab[data-tab='pages']");
    assert_eq!(reader.state().sidebar.as_deref(), Some("pages"));
    reader.press_chord("mod+f");
    look_for(&mut reader, "the");
    reader.click(".find-count");
    assert_eq!(reader.state().sidebar.as_deref(), Some("results"));

    reader.press("Escape");
    assert_eq!(reader.state().sidebar.as_deref(), Some("pages"));
}

/// And a count with nothing behind it is not a way to anything.
#[test]
fn an_empty_count_opens_nothing() {
    let mut reader = searching();
    reader.click(".find-count");
    assert_eq!(
        reader.state().sidebar,
        None,
        "nothing has been searched for"
    );
    look_for(&mut reader, "zzzzz");
    reader.click(".find-count");
    assert_eq!(reader.state().sidebar, None, "and nothing was found");
}

/// **Taking a letter back out again**, which is not the same key everywhere.
///
/// AppKit does not deliver Backspace as a keystroke: it reads the editing keys
/// against the standard key bindings and calls `doCommandBySelector:` with a
/// name, and `blitz-dom`'s own `Key::Backspace` arm is
/// `#[cfg(not(target_os = "macos"))]` because of it. `Shell` was not
/// forwarding that callback, so on a Mac a query could be typed and could not
/// be corrected — the find bar, the go-to-page field and every field in the
/// settings window, all of them write-only. See
/// `ApplicationHandlerExtMacOS for Shell` and `Reader::apple_binding`.
#[test]
fn a_query_can_be_corrected_as_well_as_typed() {
    let mut reader = searching();
    look_for(&mut reader, "needles");
    assert_eq!(reader.state().find.as_deref(), Some("None"), "no such word");

    reader.press("Backspace");
    reader.scan_out();
    assert_eq!(reader.state().query, "needle", "the letter came back off");
    assert_eq!(reader.state().find.as_deref(), Some("1 of 3"));
}

/* ------------------------------------------- the results, without asking */

/// **A search shows its matches**, rather than keeping them behind the count.
/// The panel comes up with the first hit, on the Results tab, and it is
/// borrowed: closing the bar takes it back down, so one Escape undoes the
/// whole of what one search did.
#[test]
fn searching_opens_the_panel_on_the_results_and_closing_it_puts_the_panel_away() {
    let mut reader = searching_into_the_panel();
    assert_eq!(
        reader.state().sidebar,
        None,
        "shut before anything is typed"
    );
    look_for(&mut reader, "needle");
    assert_eq!(
        reader.state().sidebar.as_deref(),
        Some("results"),
        "the matches are on screen without being asked for",
    );
    assert_eq!(reader.state().results, vec![0, 1, 2]);

    reader.click(".find-close");
    assert_eq!(reader.state().find, None);
    assert_eq!(reader.state().sidebar, None, "and the panel it borrowed");
}

/// **Reaching for another tab keeps the panel.** A panel the search opened
/// goes with the bar; one the reader has asked Contents or Pages of is theirs,
/// and the press that closes the bar leaves it up.
#[test]
fn another_tab_of_a_borrowed_panel_keeps_it() {
    let mut reader = searching_into_the_panel();
    look_for(&mut reader, "needle");
    reader.click(".tab[data-tab=pages]");
    assert_eq!(reader.state().find, None, "the press was past the bar");
    assert_eq!(
        reader.state().sidebar.as_deref(),
        Some("pages"),
        "and the panel stayed"
    );
}

/// And the search opening the panel at all is a setting, off by default: a
/// panel nobody asked for, and the page reflowing under the reader as it
/// came up.
#[test]
fn a_search_opens_no_panel_unless_told_to() {
    let mut reader = Reader::open_with(&fixture::prose_pdf(), Options::default());
    reader.press_chord("mod+f");
    look_for(&mut reader, "needle");
    assert_eq!(reader.state().sidebar, None);
    reader.click(".find-count");
    assert_eq!(
        reader.state().sidebar.as_deref(),
        Some("results"),
        "the count still does"
    );
}

/// A search that finds nothing opens nothing: a panel that comes up to say
/// "No matches." is a panel saying what the bar has already said.
#[test]
fn a_search_that_finds_nothing_opens_no_panel() {
    let mut reader = searching();
    look_for(&mut reader, "zzzzz");
    assert_eq!(reader.state().sidebar, None);
}

/// And a panel the reader shuts stays shut, for as long as that search does.
/// The panel is opened once per search and not on every slice of the scan —
/// see `Viewer::show_the_matches`.
#[test]
fn a_panel_shut_during_a_search_stays_shut() {
    let mut reader = searching_into_the_panel();
    look_for(&mut reader, "needle");
    reader.press_chord("mod+b");
    assert_eq!(reader.state().sidebar, None, "the reader shut it");
    reader.scan_out();
    assert_eq!(reader.state().sidebar, None, "and it stayed shut");
}

/* --------------------------------------------- the bar is a card, not a row */

/// **The bar hangs over the document rather than taking a row from it.**
/// `styles.css` puts it under the toolbar at the right; here it had been a
/// row of the flex column, which took forty pixels off the viewport for as
/// long as it was up — so opening the search moved the page being read.
#[test]
fn the_bar_hangs_over_the_document_and_does_not_shorten_it() {
    // Wide enough for the chips to have their words: under 1200px they lose
    // them and the card goes to the window's edge, which is the end of this.
    let mut reader = Reader::open_with(
        &fixture::prose_pdf(),
        Options {
            width: 1400,
            ..Options::default()
        },
    );
    let before = reader.harness.layout_rect(".viewer");
    reader.press_chord("mod+f");
    let after = reader.harness.layout_rect(".viewer");
    assert_eq!(
        (after.width, after.height),
        (before.width, before.height),
        "the document is the size it was",
    );
    let bar = reader.harness.layout_rect(".find-bar");
    let chip = reader.harness.layout_rect(".chip.find");
    assert!(bar.y > chip.y, "under the toolbar: {bar:?}");
    // And under the button that opened it, flush with the bar's lower edge,
    // which is where every other panel in the toolbar comes down. It used to
    // hang at the window's right edge twelve pixels below the bar, belonging
    // to nothing.
    assert!(
        (bar.x - chip.x).abs() <= 1.0,
        "and under the Search chip: {bar:?} against {chip:?}",
    );
    assert!(
        bar.x + bar.width < after.width,
        "with room for it there: {bar:?} in a window {} wide",
        after.width,
    );
    // A narrower bar is a bar of symbols, and the card is wider than what is
    // left of it to the chip's right — so it stands at the window's edge
    // rather than running out of the window.
    reader.resize(1100, 900);
    reader.settle();
    let bar = reader.harness.layout_rect(".find-bar");
    assert!(bar.x >= 0.0 && bar.x + bar.width <= 1100.0, "{bar:?}");
    assert!(bar.y >= 46.0, "and still under the toolbar: {bar:?}");
}

/// And it can be pressed with the document scrolled under it, which is the
/// trap in `tests/upstream.rs`: a page is placed at `top - scroll`, so a
/// scrolled document is hit-tested over the whole window whatever is drawn
/// on top. The bar's `z-index` is what settles it.
#[test]
fn the_bar_can_be_pressed_over_a_scrolled_document() {
    let mut reader = searching();
    look_for(&mut reader, "needle");
    // By the wheel rather than by a key: the field owns the keyboard while
    // the bar is up, which is the whole of why `j` does not scroll here.
    reader.wheel_over(".viewer", 2_000.0);
    assert!(
        reader.state().scroll > 0.0,
        "the document has been scrolled"
    );
    reader.click(".find-close");
    assert_eq!(reader.state().find, None, "the bar closed");
}

/* ------------------------------- and the four ways it goes away by itself */

/// **Reaching past the bar puts it away**, which is `onFindOutside` in
/// `main.ts`: a reader who has found what they came for and gone back to
/// reading should not have a card sitting over the corner of the page for
/// the rest of the session — nor the panel the search borrowed, which most
/// readers would otherwise have to close by hand after every search.
///
/// **And the press is spent on that.** The page had begun a sweep at it,
/// measured against a layout the panel's closing then changed, and carried
/// on it selected a stray patch near the pointer.
#[test]
fn a_press_in_the_document_puts_the_find_bar_and_its_panel_away() {
    let mut reader = searching();
    look_for(&mut reader, "needle");
    reader.click(".find-count");
    assert!(reader.state().find.is_some(), "the bar is up");
    assert_eq!(reader.state().sidebar.as_deref(), Some("results"));
    reader.click(".viewer");
    assert_eq!(reader.state().find, None, "and reading closed it");
    assert_eq!(reader.state().sidebar, None, "and the panel it borrowed");
    assert!(
        reader.harness.query_all(".selected").is_empty(),
        "and nothing was selected by it",
    );
}

/// And opening any of the five menus does too, which is `opens(…)` in
/// `wire()`: two panels claiming the same corner of the screen, one of them
/// still holding the keyboard, is not a place anybody meant to be.
#[test]
fn opening_a_menu_puts_the_find_bar_away() {
    for chip in [".chip.theme", ".chip.settings", ".chip.open", ".chip.fit"] {
        let mut reader = searching();
        look_for(&mut reader, "needle");
        reader.click(chip);
        assert_eq!(reader.state().find, None, "{chip} left the bar up");
    }
}

/// Contents is one of them, and it is the one that is not a menu: it opens a
/// panel rather than a popover, and the app wraps it in the same `opens(…)`
/// for the same reason.
#[test]
fn the_contents_button_puts_the_find_bar_away() {
    let mut reader = searching();
    look_for(&mut reader, "needle");
    reader.click(".chip.contents");
    assert_eq!(reader.state().find, None);
}

/// **But the bar's own switches, the toolbar, and the list of results do
/// not.** The three that would each have been a bug of their own: a switch
/// that closes the thing it is about, a rotation that ends a search, and a
/// result that closes the list it was picked from.
#[test]
fn the_bar_its_own_toolbar_and_its_results_all_keep_it_open() {
    let mut reader = searching();
    look_for(&mut reader, "needle");

    reader.click(".find-option");
    assert!(
        reader.state().find.is_some(),
        "a switch on the bar closed the bar",
    );

    reader.click(".chip.rotate-left");
    assert!(
        reader.state().find.is_some(),
        "turning the page is reading, not leaving",
    );

    // The list is this search seen larger — `#results-panel` and
    // `#tab-results` in `FIND_KEEPS_OPEN`.
    reader.click(".find-count");
    assert_eq!(
        reader.state().sidebar.as_deref(),
        Some("results"),
        "the count opens the list behind it",
    );
    reader.click_nth(".result", 0);
    assert!(
        reader.state().find.is_some(),
        "picking a result closed the search that found it",
    );
}

/// ⌘A in the field selects the query, not the page under it. The root hears
/// every key the field lets past, and it let the editing chords past.
#[test]
fn select_all_in_the_field_selects_nothing_on_the_page() {
    let mut reader = searching();
    look_for(&mut reader, "needle");
    reader.press_chord("mod+a");
    assert!(
        reader.harness.query_all(".selected").is_empty(),
        "⌘A in the find bar selected the page",
    );
}

/// Stepping to a match that is already on screen leaves the document where it
/// is; only a match out of view moves it.
#[test]
fn a_match_on_screen_does_not_move_the_document() {
    let mut reader = searching();
    look_for(&mut reader, "needle");
    // 1 of 3 is on page 1, 2 and 3 of 3 share a line on page 3.
    let first = reader.state().scroll;
    reader.press_chord("mod+g");
    let on_page_three = reader.state().scroll;
    assert_ne!(on_page_three, first, "page 3 was out of view and stayed");
    reader.press_chord("mod+g");
    assert_eq!(reader.state().find.as_deref(), Some("3 of 3"));
    assert_eq!(reader.state().scroll, on_page_three, "the document jumped");
}

/// A query survives the bar going down, and reopening looks for it again.
#[test]
fn reopening_the_bar_brings_the_query_back() {
    let mut reader = searching();
    look_for(&mut reader, "needle");
    reader.press("Escape");
    assert_eq!(reader.state().find, None);
    reader.press_chord("mod+f");
    reader.scan_out();
    assert_eq!(reader.state().query, "needle");
    assert_eq!(reader.state().find.as_deref(), Some("1 of 3"));
}

/// And what comes back is selected, as Firefox has it: typing replaces the
/// query, and one press of an arrow is the way to continue it instead.
#[test]
fn a_query_brought_back_is_selected() {
    let mut reader = searching();
    look_for(&mut reader, "needle");
    reader.press("Escape");
    reader.press_chord("mod+f");
    reader.scan_out();
    reader.press("ArrowRight");
    reader.type_text("s");
    reader.scan_out();
    assert_eq!(reader.state().query, "needles");
    reader.press("Backspace");
    reader.scan_out();
    assert_eq!(reader.state().query, "needle");
    assert_eq!(reader.state().find.as_deref(), Some("1 of 3"));
}

/// ⌘F with the bar already up selects the query, as Firefox and Chrome do, so
/// that what is typed next replaces it — and so does ⌘F bringing it back.
#[test]
fn find_again_selects_the_query() {
    let mut reader = searching();
    look_for(&mut reader, "needle");
    reader.press_chord("mod+f");
    look_for(&mut reader, "beside");
    assert_eq!(reader.state().query, "beside", "typed over, not after");

    reader.press("Escape");
    reader.press_chord("mod+f");
    look_for(&mut reader, "needle");
    assert_eq!(reader.state().query, "needle", "and on the way back in");
}

/// One press, one character. On a Mac AppKit sends `moveLeft:` as well as the
/// keystroke, and both used to move the caret.
#[test]
fn an_arrow_moves_the_caret_one_character() {
    let mut reader = searching();
    reader.type_text("abc");
    reader.press("ArrowLeft");
    reader.type_text("X");
    assert_eq!(reader.state().query, "abXc");
    reader.press("ArrowRight");
    reader.type_text("Y");
    assert_eq!(reader.state().query, "abXcY");
}

/// A selection in the field is the theme's, not Blitz's pale blue with the
/// field's ink on it (issue 4).
#[test]
fn a_selection_in_the_field_wears_the_theme() {
    let dark = moonowl::theme::BUILT_IN
        .iter()
        .position(|(id, _)| *id == moonowl::theme::DEFAULT_DARK)
        .unwrap();
    let parsed: moonowl::theme::Theme = toml::from_str(moonowl::theme::BUILT_IN[dark].1).unwrap();
    let area = moonowl::palette::resolve(&parsed, true).selection_area;
    let mut reader = Reader::open_with(
        &fixture::prose_pdf(),
        Options {
            theme: Some(dark),
            ..Default::default()
        },
    );
    reader.press_chord("mod+f");
    look_for(&mut reader, "needle");
    let field = reader.harness.layout_rect(".find-field");
    let rect = (
        field.x as u32,
        field.y as u32,
        (field.x + field.width) as u32,
        (field.y + field.height) as u32,
    );
    let before = 1.0 - reader.screenshot().unlike(area, rect);
    reader.press_chord("mod+f");
    let shot = reader.screenshot();
    let selected = 1.0 - shot.unlike(area, rect);
    assert!(
        selected > before + 0.05,
        "the selection is the theme's colour: {before:.3} → {selected:.3}"
    );
    assert!(
        shot.unlike([180, 213, 255], rect) > 0.999,
        "and there is no pale blue"
    );
}

/// **⌘G after Escape brings the search back** rather than saying "No
/// matches" for a word found a moment ago: the bar keeps its query.
#[test]
fn find_next_after_escape_looks_again() {
    let mut reader = searching();
    look_for(&mut reader, "needle");
    reader.press("Escape");
    reader.press("Escape");
    assert_eq!(reader.state().find, None, "the bar is down");
    reader.press_chord("mod+g");
    reader.scan_out();
    assert!(reader.state().find.is_some(), "and back up, searching");
    assert_ne!(reader.state().notice, "No matches");
}
