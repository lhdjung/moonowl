//! Type is the same width on every screen.
//!
//! parley shapes at the CSS size times the display scale, and harfrust reads
//! SF's `trak` table at whatever size it is handed — so on a 2x screen every
//! word came out tracked as type twice its size, 5-7% wider than at 1x and
//! than WebKit. `vendor/parley` divides the scale back out; this is the
//! assertion that it stays divided out.

use moonowl::harness::{Options, Reader};

/// The one filled button on the start screen is as wide as its words.
fn button_width(scale: f32) -> f64 {
    let mut reader = Reader::empty(Options {
        width: 600,
        height: 520,
        scale,
        ..Options::default()
    });
    reader.settle();
    reader
        .width_of(".start-open")
        .expect("the start screen's button")
}

#[test]
fn a_word_is_as_wide_at_2x_as_at_1x() {
    let at_1x = button_width(1.0);
    let at_2x = button_width(2.0);
    // The fault this guards against is 5-7% of the word; the allowance is
    // 1%, because Segoe UI on Windows lands one pixel apart at the two
    // scales from rounding alone and has no `trak` table to be wrong by.
    assert!(
        (at_1x - at_2x).abs() < at_1x * 0.01,
        "the button is {at_1x}px wide at 1x and {at_2x}px at 2x"
    );
}
