//! Scrolling by itself: started from the View menu, at the speed Settings
//! gives, and stopped by any key, click or scroll.

use moonowl::harness::{Options, Reader};

fn started() -> Reader {
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            settings: vec![("scroll_by_itself_speed".into(), 100.0.into())],
            ..Default::default()
        },
    );
    reader.click(".chip.fit");
    reader.click(".scroll-by-itself");
    reader.settle();
    reader
}

#[test]
fn it_moves_at_the_speed_settings_give() {
    let mut reader = started();
    let before = reader.state().scroll;
    reader.later(1.0);
    let moved = reader.state().scroll - before;
    assert!(
        (90.0..=110.0).contains(&moved),
        "moved {moved}px in a second"
    );
}

/// A speed written into the settings file by hand past the fastest is the
/// fastest, both on the page and in Settings, which shows the one speed.
#[test]
fn a_speed_past_the_range_is_the_fastest() {
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            settings: vec![("scroll_by_itself_speed".into(), 1000.0.into())],
            ..Default::default()
        },
    );
    reader.press_chord("mod+,");
    let shown = reader.fields(".step-field");
    assert!(
        shown.contains(&"400".to_string()) && !shown.contains(&"1000".to_string()),
        "{shown:?}"
    );
    reader.press("Escape");
    reader.click(".chip.fit");
    reader.click(".scroll-by-itself");
    reader.settle();
    let before = reader.state().scroll;
    reader.later(1.0);
    let moved = reader.state().scroll - before;
    assert!(
        (380.0..=420.0).contains(&moved),
        "moved {moved}px in a second"
    );
}

/// A middle press stops it, and does not start the stationary scroll.
#[test]
fn a_middle_press_stops_it_and_only_that() {
    let mut reader = started();
    reader.later(0.5);
    let page = reader.harness.layout_rect(".page");
    reader.middle_click_at(page.x + page.width / 2.0, page.y + page.height / 2.0);
    assert!(reader.harness.query(".still-anchor").is_none());
    let stopped = reader.state().scroll;
    reader.later(1.0);
    assert_eq!(reader.state().scroll, stopped);
}

/// A wheel over the sidebar stops it too.
#[test]
fn a_wheel_over_the_sidebar_stops_it() {
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            settings: vec![
                ("scroll_by_itself_speed".into(), 100.0.into()),
                ("show_sidebar".into(), true.into()),
            ],
            ..Default::default()
        },
    );
    reader.click(".chip.fit");
    reader.click(".scroll-by-itself");
    reader.settle();
    reader.later(0.5);
    let side = reader.harness.layout_rect(".sidebar");
    let (x, y) = (side.x + side.width / 2.0, side.y + side.height / 2.0);
    reader.harness.wheel_at(x, y, 0.0, -10.0);
    reader.settle();
    let stopped = reader.state().scroll;
    reader.later(1.0);
    assert_eq!(reader.state().scroll, stopped);
}

/// A click on the page stops it and selects nothing, and a right-click
/// stops it and opens no menu.
#[test]
fn a_press_on_the_page_stops_it_and_only_that() {
    let mut reader = started();
    reader.later(0.5);
    reader.sweep_page(1, (0.1, 0.1), (0.6, 0.3));
    let stopped = reader.state().scroll;
    reader.later(1.0);
    assert_eq!(reader.state().scroll, stopped);
    reader.press_chord("mod+c");
    assert!(reader.copied().is_empty(), "{:?}", reader.copied());

    reader.click(".chip.fit");
    reader.click(".scroll-by-itself");
    reader.settle();
    reader.later(0.5);
    reader.right_click_on_page(1, (0.5, 0.5));
    assert!(reader.harness.query(".menu.context").is_none());
    let stopped = reader.state().scroll;
    reader.later(1.0);
    assert_eq!(reader.state().scroll, stopped);
}

/// The window losing the keyboard stops it: nobody is watching, and the
/// press that would stop it goes to another window.
#[test]
fn losing_the_keyboard_stops_it() {
    let mut reader = started();
    reader.later(0.5);
    reader.deliver(moonowl::emit::News {
        event: moonowl::emit::Event::Focused(false),
        target: None,
    });
    let stopped = reader.state().scroll;
    reader.later(1.0);
    assert_eq!(reader.state().scroll, stopped);
}

/// A wheel over the sidebar ends the stationary scroll as well.
#[test]
fn a_wheel_over_the_sidebar_ends_the_stationary_scroll() {
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            settings: vec![("show_sidebar".into(), true.into())],
            ..Default::default()
        },
    );
    let page = reader.harness.layout_rect(".page");
    reader.middle_click_at(page.x + page.width / 2.0, page.y + page.height / 2.0);
    assert!(reader.harness.query(".still-anchor").is_some());
    let side = reader.harness.layout_rect(".sidebar");
    let (x, y) = (side.x + side.width / 2.0, side.y + side.height / 2.0);
    reader.harness.wheel_at(x, y, 0.0, -10.0);
    reader.settle();
    assert!(reader.harness.query(".still-anchor").is_none());
}

#[test]
fn a_key_stops_it() {
    let mut reader = started();
    reader.later(0.5);
    reader.press("Shift");
    let stopped = reader.state().scroll;
    reader.later(1.0);
    assert_eq!(reader.state().scroll, stopped);
}

#[test]
fn a_wheel_stops_it() {
    let mut reader = started();
    reader.later(0.5);
    reader.wheel(10.0);
    let stopped = reader.state().scroll;
    reader.later(1.0);
    assert_eq!(reader.state().scroll, stopped);
}

#[test]
fn a_click_on_the_toolbar_stops_it() {
    let mut reader = started();
    reader.later(0.5);
    reader.click(".chip.fit");
    let stopped = reader.state().scroll;
    reader.later(1.0);
    assert_eq!(reader.state().scroll, stopped);
}

#[test]
fn another_document_opened_stops_it() {
    let mut reader = started();
    reader.later(0.5);
    // Handed over from outside, as a second launch or the Finder does, so no
    // key or click on the way stops it first.
    reader.hand_over(&moonowl::fixture::contents_pdf());
    assert_eq!(reader.state().pages, 12, "the new document is open");
    let opened = reader.state().scroll;
    reader.later(1.0);
    assert_eq!(reader.state().scroll, opened);
}

/// A click on a link stops it and does not follow the link.
#[test]
fn a_click_on_a_link_stops_it_and_only_that() {
    let mut reader = Reader::open_with(
        &moonowl::fixture::links_pdf(),
        Options {
            settings: vec![("scroll_by_itself_speed".into(), 20.0.into())],
            ..Default::default()
        },
    );
    reader.click(".chip.fit");
    reader.click(".scroll-by-itself");
    reader.settle();
    reader.later(0.2);
    let page = reader.state().page;
    reader.click(".link");
    let stopped = reader.state().scroll;
    assert_eq!(reader.state().page, page, "the link was not followed");
    assert!(reader.opened().is_empty(), "{:?}", reader.opened());
    reader.later(1.0);
    assert_eq!(reader.state().scroll, stopped);
}
