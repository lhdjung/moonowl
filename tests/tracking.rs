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
        system_font: cfg!(target_os = "macos"),
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

/// **The number is the same on every machine, which is the point of it.**
/// The harness lays out in `tests/fonts/DejaVuSans.ttf` unless asked not to,
/// so an assertion with a width in it that passes on one platform passes on
/// all three. If this fails on one of them, the font is not pinned there.
#[test]
fn a_word_is_as_wide_on_every_machine() {
    let mut reader = Reader::empty(Options {
        width: 600,
        height: 520,
        ..Options::default()
    });
    reader.settle();
    let width = reader.width_of(".start-open").expect("the button");
    assert_eq!(width.round(), 185.0);
}
