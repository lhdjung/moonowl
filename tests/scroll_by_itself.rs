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
