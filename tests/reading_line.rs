//! The reading line: a band under the line being read, off by default,
//! moved a line at a time with ↓ and ↑. See `app/reading.rs`.

use moonowl::fixture;
use moonowl::harness::{Options, Reader};

/// Page 1 of the fixture has a heading at 700pt from the bottom and lines
/// at 600, 560 and 520.
fn reader(on: bool) -> Reader {
    Reader::open_with(
        &fixture::references_pdf(),
        Options {
            settings: vec![("reading_line".into(), on.into())],
            ..Default::default()
        },
    )
}

/// How much of the strip behind the line set on `baseline` (points from
/// the bottom of page 1) is not white: a little where only letters are,
/// nearly all of it under the band.
fn covered(reader: &mut Reader, baseline: f32) -> f64 {
    let top = 792.0 - baseline;
    let (left, upper) = reader.point_on(1, (66.0 / 612.0, (top - 9.0) / 792.0));
    let (right, lower) = reader.point_on(1, (180.0 / 612.0, (top + 1.0) / 792.0));
    let shot = reader.screenshot();
    shot.unlike(
        [255, 255, 255],
        (left as u32, upper as u32, right as u32, lower as u32),
    )
}

#[test]
fn the_reading_line_is_off_and_the_arrows_scroll() {
    let mut reader = reader(false);
    assert!(covered(&mut reader, 600.0) < 0.5);
    let before = reader.state().scroll;
    reader.press("ArrowDown");
    assert!(reader.state().scroll > before, "↓ scrolls");
}

#[test]
fn the_arrows_move_the_band_a_line_at_a_time() {
    let mut reader = reader(true);
    // Under the first line in the window, the page's heading.
    assert!(covered(&mut reader, 700.0) > 0.8);
    assert!(covered(&mut reader, 600.0) < 0.5);

    let scroll = reader.state().scroll;
    reader.press("ArrowDown");
    assert!(
        covered(&mut reader, 600.0) > 0.8,
        "the band is on the next line"
    );
    assert!(covered(&mut reader, 700.0) < 0.5);
    assert_eq!(
        reader.state().scroll,
        scroll,
        "a line in the window is not scrolled to"
    );

    reader.press("ArrowDown");
    assert!(covered(&mut reader, 560.0) > 0.8);
    reader.press("ArrowUp");
    assert!(covered(&mut reader, 600.0) > 0.8, "↑ goes back a line");
}

#[test]
fn the_view_menu_switches_it_on() {
    let mut reader = reader(false);
    reader.click(".chip.fit");
    reader.click(".reading-line");
    reader.press("Escape");
    assert!(covered(&mut reader, 700.0) > 0.8);
}

#[test]
fn the_page_follows_the_band_onto_the_next_pages() {
    // One line on each page, so each ↓ is a page further on.
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            settings: vec![("reading_line".into(), true.into())],
            ..Default::default()
        },
    );
    for _ in 0..4 {
        reader.press("ArrowDown");
    }
    assert_eq!(reader.state().page, 5);
    reader.press("ArrowUp");
    assert_eq!(reader.state().page, 4);
}
