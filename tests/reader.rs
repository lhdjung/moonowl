//! The reader, driven: the file `reader.test.mjs` is in this tree.
//!
//! Everything here goes through `harness::Reader`, which is a real
//! `DioxusDocument` with real style, real layout and the real event pipeline,
//! and no window anywhere. Assertions are on what the interface says about
//! itself — the pill, the chips, the notice — rather than on the `Viewer`
//! behind them, for the same reason the app's own harness reads the DOM: a
//! test that reaches past the interface cannot tell you the interface is
//! wired up.

use moonowl::harness::{Options, Reader};
use moonowl::keymap::Action;

fn book() -> Reader {
    Reader::open(&Reader::book())
}

#[test]
fn a_document_opens_on_its_first_page() {
    let reader = book();
    let state = reader.state();
    assert_eq!(state.pages, 400);
    assert_eq!(state.page, 1);
    assert_eq!(state.scroll, 0.0);
    assert_eq!(state.zoom, "Fit width");
    assert_eq!(state.mounted, vec![1]);
}

#[test]
fn the_wheel_moves_the_document() {
    let mut reader = book();
    reader.wheel_screen();
    let after = reader.state();
    assert!(after.scroll > 700.0, "{after:?}");

    // And it stops at the end rather than running past it.
    for _ in 0..3 {
        reader.press("End");
    }
    let end = reader.state();
    assert_eq!(end.page, 400);
    reader.wheel_screen();
    assert_eq!(reader.state().scroll, end.scroll, "the end is the end");
}

/// **The stationary scroll**, which the middle button starts: the anchor goes
/// down, the document runs under it the further the pointer is carried away,
/// and the next press puts it away.
///
/// The speed is a real clock rather than a pump, so this waits for the offset
/// to move rather than counting ticks. See `Viewer::still_speed`.
#[test]
fn the_middle_button_scrolls_from_where_it_was_pressed() {
    let mut reader = book();
    let (width, height) = reader.window();
    let (middle_x, middle_y) = (width as f32 / 2.0, height as f32 / 2.0);

    reader.middle_click_at(middle_x, middle_y);
    assert!(
        reader.box_of(".still-anchor").is_some(),
        "the anchor says the gesture is running"
    );

    // Parked on the anchor, the document does not move at all: the dead zone
    // is the whole of what keeps a resting hand from creeping down the page.
    assert!(
        !reader.wait_until(0.4, |reader| reader.state().scroll > 0.0),
        "nothing moves inside the dead zone"
    );

    // Carried well below it, it runs.
    reader.point_to(middle_x, middle_y + 150.0);
    assert!(
        reader.wait_until(2.0, |reader| reader.state().scroll > 20.0),
        "the document runs under the anchor"
    );

    // And a press — any press — puts it away and leaves the document where
    // it stopped.
    reader.click_at(middle_x, middle_y + 150.0);
    assert!(
        reader.box_of(".still-anchor").is_none(),
        "the anchor is gone"
    );
    let stopped = reader.state().scroll;
    assert!(
        !reader.wait_until(0.4, |reader| reader.state().scroll > stopped),
        "and it stays stopped"
    );
}

/// **The whole marker is always on screen.** Pressed hard against the corner
/// of the window, a circle centred on the pointer would be a quarter of a
/// circle — so it is held back by its own radius and its edge comes to rest
/// against the window's. See `Viewer::still_anchor`.
#[test]
fn the_anchor_never_hangs_off_the_edge_of_the_window() {
    let mut reader = book();
    let (width, height) = reader.window();

    reader.middle_click_at(width as f32 - 1.0, height as f32 - 1.0);
    let (left, top, mark, _) = reader.box_of(".still-anchor").expect("bottom right");
    assert!(
        left + mark <= width as f32 && top + mark <= height as f32,
        "the far corner: {left} + {mark} against {width}, {top} + {mark} against {height}"
    );

    reader.middle_click_at(width as f32 - 1.0, height as f32 - 1.0);
    reader.middle_click_at(1.0, 60.0);
    let (left, top, _, _) = reader.box_of(".still-anchor").expect("top left");
    assert!(left >= 0.0 && top >= 0.0, "the near corner: {left}, {top}");
}

#[test]
fn the_keys_move_the_reader() {
    let mut reader = book();
    let screen = reader.state();

    reader.press("j");
    let line = reader.state().scroll;
    assert!(line > 0.0 && line < 100.0, "a line is a line: {line}");

    reader.press("k");
    assert_eq!(reader.state().scroll, screen.scroll);

    reader.press("d");
    let half = reader.state().scroll;
    reader.press("Home");
    reader.press(" ");
    let whole = reader.state().scroll;
    assert!(
        whole > half * 1.5,
        "a screen is more than half of one: {whole} against {half}"
    );

    reader.press("End");
    assert_eq!(reader.state().page, 400);
    reader.press("Home");
    assert_eq!(reader.state().scroll, 0.0);

    // A page at a time. Left and right turn pages in every scroll mode,
    // which is the app's binding and the reason to reach for them rather than
    // for the keys that move by a screen: landing on the top of a page.
    reader.press("ArrowRight");
    assert_eq!(reader.state().page, 2);
    reader.press("l");
    assert_eq!(reader.state().page, 3);
    reader.press("h");
    assert_eq!(reader.state().page, 2);
}

#[test]
fn only_the_pages_near_the_viewport_are_in_the_document() {
    let mut reader = book();
    for _ in 0..12 {
        reader.wheel_screen();
    }
    let state = reader.state();
    assert!(
        state.mounted.len() <= 3,
        "a 400-page book holds a handful of pages: {state:?}"
    );
    assert!(
        state.mounted.contains(&state.page),
        "the page being read is one of them: {state:?}"
    );
    // Contiguous, in order — the mounting window is a band, not a set.
    for pair in state.mounted.windows(2) {
        assert_eq!(pair[1], pair[0] + 1, "{state:?}");
    }
}

#[test]
fn fit_and_zoom_say_what_they_did() {
    let mut reader = book();
    let wide = reader.harness.layout_rect(".page");
    assert!(
        (wide.width - 1100.0).abs() < 1.0,
        "fit width fills it: {wide:?}"
    );

    reader.press_action(Action::FitPage);
    assert_eq!(reader.state().zoom, "Fit page");
    let fitted = reader.harness.layout_rect(".page");
    let viewer = reader.harness.layout_rect(".viewer");
    assert!(
        fitted.height <= viewer.height + 0.5,
        "a fitted page is on the screen: {fitted:?} in {viewer:?}"
    );

    reader.press_chord("mod++");
    let closer = reader.state();
    assert!(closer.zoom.ends_with('%'), "{closer:?}");
    // The chip says it; the notice stays quiet while the bar is up.
    assert_ne!(closer.notice, closer.zoom);
    let bigger = reader.harness.layout_rect(".page");
    reader.press_chord("mod+-");
    let smaller = reader.harness.layout_rect(".page");
    assert!(smaller.width < bigger.width, "{smaller:?} {bigger:?}");
}

#[test]
fn zooming_keeps_the_reader_where_they_were() {
    let mut reader = book();
    for _ in 0..5 {
        reader.wheel_screen();
    }
    let before = reader.state().page;
    reader.press_chord("mod++");
    reader.press_chord("mod++");
    assert_eq!(reader.state().page, before, "a zoom is not a page turn");
}

#[test]
fn the_toolbar_is_clickable() {
    let mut reader = book();
    // By what each chip is rather than by where it sits: the toolbar grew two
    // more when the sidebar arrived, and a chip addressed by its position is
    // a test that quietly starts clicking something else.
    reader.click(".chip.zoom-in");
    assert!(reader.state().zoom.ends_with('%'));
    // The fit and the theme are menus now rather than a step and a cycle —
    // see `app::Menu`. The chip still says what is in force, which is what
    // `state()` reads off it; clicking it shows the choices.
    reader.click(".chip.fit");
    assert_eq!(reader.state().menu.as_deref(), Some("view"));
    reader.click_nth(".menu.view .menu-item", 0);
    assert_eq!(reader.state().zoom, "Fit width");
    // Neither menu closes on a choice: a zoom and a theme are both things you
    // try on, which is the app's own rule for these two — see `showZoomMenu`
    // and `showThemeMenu`. Escape is the way out of either.
    assert_eq!(reader.state().menu.as_deref(), Some("view"));
    reader.press("Escape");
    reader.click(".chip.theme");
    reader.click_nth(".menu.theme .menu-item", 1);
    assert_eq!(reader.state().theme, "Moonowl Dark");
}

/// **A click used to cost the reader its keyboard**, and nothing said so for
/// two phases because no test had ever pressed a key after clicking
/// something. Blitz clears the focus when a click lands on nothing it knows
/// how to focus — a `<button>` is not on that list — and a key with nothing
/// focused goes to `<html>`, which is above every handler this app can put
/// anywhere. See `app::KEYBOARD`: the window is what gives it back, because a
/// component cannot.
#[test]
fn a_click_does_not_cost_the_reader_its_keyboard() {
    let mut reader = book();
    reader.click(".chip.zoom-in");
    let after = reader.state();
    reader.press("j");
    assert!(
        reader.state().scroll > after.scroll,
        "a key still moves the document after a click",
    );
    reader.press("t");
    assert_eq!(
        reader.state().theme,
        "Moonowl Dark",
        "…and still acts on it"
    );
}

#[test]
fn a_theme_is_named_where_it_is_changed() {
    let mut reader = book();
    assert_eq!(reader.state().theme, "Moonowl Light");
    reader.press("t");
    let dark = reader.state();
    assert_eq!(dark.theme, "Moonowl Dark");
    assert_eq!(dark.notice, "Moonowl Dark", "the notice says what happened");
}

#[test]
fn spreads_put_two_pages_side_by_side() {
    let mut reader = book();
    reader.press("ArrowRight");
    reader.press("s");
    let state = reader.state();
    assert!(state.mounted.len() >= 2, "a spread holds a pair: {state:?}");
    let pages = reader.harness.query_all(".page");
    let rects: Vec<_> = pages
        .iter()
        .map(|node| reader.harness.layout_rect_of(*node))
        .collect();
    assert!(
        rects.iter().any(|rect| rect.x > 1.0),
        "one of them is to the right of the other: {rects:?}"
    );
}

#[test]
fn the_window_can_be_a_different_size() {
    let small = Reader::open_with(
        &Reader::book(),
        Options {
            width: 640,
            height: 480,
            ..Default::default()
        },
    );
    let page = small.harness.layout_rect(".page");
    assert!(
        (page.width - 640.0).abs() < 1.0,
        "fit width fits the window it is given: {page:?}"
    );
}

/// **Everything the reader changes is still there the next time.** This is the
/// brief's first promise about settings and the first thing Phase 3 owed it:
/// a theme, a zoom and a spread chosen by hand, a reader closed, a reader
/// opened on the same settings directory, and the same document as it was
/// left.
///
/// It is driven entirely through the interface — keys pressed, answers read
/// off the toolbar — because that is the half that was never wired up before
/// and the half a unit test on `Store` cannot see.
#[test]
fn what_the_reader_changes_survives_being_closed() {
    let config = std::env::temp_dir().join(format!("moonowl-restart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&config);
    let open = || {
        Reader::open_with(
            &Reader::book(),
            Options {
                config: config.clone(),
                ..Default::default()
            },
        )
    };

    let (theme, zoom) = {
        let mut reader = open();
        assert_eq!(reader.state().theme, "Moonowl Light", "a fresh directory");
        reader.press("t");
        reader.press("s");
        reader.press_chord("mod++");
        reader.press_chord("mod++");
        let state = reader.state();
        assert_ne!(state.theme, "Moonowl Light");
        assert!(state.zoom.ends_with('%'), "a zoom, not a fit: {state:?}");
        (state.theme, state.zoom)
    };

    let reopened = open().state();
    assert_eq!(reopened.theme, theme, "the theme came back");
    assert_eq!(reopened.zoom, zoom, "and the zoom it was left at");
    // The spread is read off the file rather than off the screen, because at
    // 200% one page is as much as the window holds and the interface has no
    // readout for it yet. `spreads_put_two_pages_side_by_side` above is what
    // says the setting does anything.
    let written = std::fs::read_to_string(config.join("settings.toml")).expect("settings.toml");
    assert!(written.contains("spread_mode = \"cover\""), "{written}");
    // And it is a settings file a person could open, which is the promise the
    // format is for.
    assert!(written.starts_with("# Moonowl settings"), "{written}");

    let _ = std::fs::remove_dir_all(&config);
}

/// The theme list is the app's own fourteen files, not two hard-coded ones —
/// which is what makes `t` a poor gesture and a menu Phase 3's next item, and
/// is worth asserting because the reader is the only place the whole chain
/// (`build.rs` → `install_built_ins` → `load_all` → `resolve`) is exercised
/// end to end.
#[test]
fn the_whole_shipped_theme_set_is_wearable() {
    let mut reader = book();
    let mut seen = vec![reader.state().theme];
    for _ in 1..moonowl::theme::BUILT_IN.len() {
        reader.press("t");
        seen.push(reader.state().theme);
    }
    assert!(seen.len() >= 14, "{seen:?}");
    assert!(seen.contains(&"Moonowl Dark".to_string()), "{seen:?}");
    assert!(seen.contains(&"Nord".to_string()), "{seen:?}");
    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), seen.len(), "each one once: {seen:?}");

    // And round, back to where it started.
    reader.press("t");
    assert_eq!(reader.state().theme, seen[0]);
}

/// **Two across has to mean two on screen.**
///
/// At a fixed zoom it did not. 175% is 175% whatever is beside it, so asking
/// for a spread at one laid two letter pages across 2,870 pixels of a window
/// half that wide and centred them: the reader got the inner half of each,
/// which is the single page they had been looking at with a seam down it.
/// Choosing Fit width by hand fixed it, which is what said what the fault was.
/// Fit modes cannot break this way, so the fallback only ever fires out of
/// actual size.
#[test]
fn a_spread_too_wide_for_the_window_falls_back_to_fitting_it() {
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            settings: vec![
                ("fit_mode".into(), "actual".into()),
                ("zoom".into(), 1.75.into()),
            ],
            ..Options::default()
        },
    );
    assert_eq!(reader.state().zoom, "175%", "actual size, as the file says");

    // `s` is the spread key: one page across, or a cover spread.
    reader.press("ArrowRight");
    reader.press("s");

    let state = reader.state();
    assert_eq!(state.zoom, "Fit width", "the fit gave way: {state:?}");
    assert_eq!(state.notice, "Fit width", "and said so");

    let window = reader.harness.layout_rect(".viewer").width;
    let rects: Vec<_> = reader
        .harness
        .query_all(".page")
        .iter()
        .map(|node| reader.harness.layout_rect_of(*node))
        .collect();
    assert!(rects.len() >= 2, "a pair is mounted: {rects:?}");
    for rect in &rects {
        assert!(
            rect.x >= -1.0 && rect.x + rect.width <= window + 1.0,
            "both pages are inside the window: {rect:?} in {window}",
        );
    }
}

/// And the way back is not the same journey. A single page as wide as the
/// reader's zoom makes it is the zoom's doing, not the spread's, so going back
/// to one across leaves the fit where it is rather than deciding for them
/// twice.
#[test]
fn going_back_to_one_page_across_leaves_the_fit_alone() {
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            settings: vec![
                ("fit_mode".into(), "actual".into()),
                ("zoom".into(), 6.0.into()),
            ],
            ..Options::default()
        },
    );
    assert_eq!(reader.state().zoom, "600%");
    reader.press("ArrowRight");
    reader.press("s");
    assert_eq!(reader.state().zoom, "Fit width", "two across fits the pair");

    // Back to one, and the fit stays: nothing about one page across says the
    // reader wants their 600% again, and putting it back would be a second
    // decision made for them.
    reader.press("s");
    assert_eq!(reader.state().zoom, "Fit width");
}

#[test]
fn the_page_arrows_turn_a_spread_by_the_row() {
    // The keys were turned by the row; the chips beside the page field still
    // stepped by the page, so Next on a pair landed on the same pair.
    let mut reader = book();
    reader.press("s"); // a cover spread: 1 alone, then (2, 3), (4, 5)…
    reader.click(".page-next");
    assert_eq!(reader.state().page, 2);
    reader.click(".page-next");
    assert_eq!(reader.state().page, 4, "the row after (2, 3)");
    reader.click(".page-previous");
    assert_eq!(reader.state().page, 2, "and back");
}
