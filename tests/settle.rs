//! **The reader stops rendering when nothing is happening.**
//!
//! Its own file because the counter it reads is the process's, and the two
//! tests in `cost.rs` drive a reader of their own in parallel with it.

use moonowl::harness::{Options, Reader};
use moonowl::stats;

/// A window small enough that the rasteriser is not the slow part.
fn options() -> Options {
    Options {
        width: 700,
        height: 560,
        ..Default::default()
    }
}

/// **A component that dirties itself as it renders costs a core, for ever.**
///
/// Nothing else in the suite would notice: `settle()` pumps a fixed three
/// times and every assertion after it passes either way. In the real app it is
/// a redraw per frame — a whole style, layout and paint pass at the display's
/// refresh rate, with nobody touching the window.
/// The tests here count the process's renders, so they take turns.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn the_reader_stops_rendering_when_nothing_is_happening() {
    let _turn = ONE_AT_A_TIME.lock();
    let mut reader = Reader::open_with(&Reader::book(), options());
    reader.settle();
    let before = stats::get(&stats::RENDERS);
    for _ in 0..20 {
        reader.settle();
    }
    let after = stats::get(&stats::RENDERS);
    assert_eq!(
        before,
        after,
        "the reader rendered {} more times with nothing happening",
        after - before
    );
}

/// **The stationary scroll pressed against the end of the document does not
/// render.** It steps every frame, and a step that moves nothing is not a
/// render.
#[test]
fn the_stationary_scroll_against_an_edge_does_not_render() {
    let _turn = ONE_AT_A_TIME.lock();
    let mut reader = Reader::open_with(&Reader::book(), options());
    reader.press("End");
    reader.middle_click_at(350.0, 280.0);
    assert!(reader.harness.query(".still-anchor").is_some());
    reader.point_to(350.0, 550.0);
    reader.later(0.5);
    let before = stats::get(&stats::RENDERS);
    reader.later(1.0);
    let after = stats::get(&stats::RENDERS);
    assert!(reader.harness.query(".still-anchor").is_some());
    assert_eq!(
        before,
        after,
        "the reader rendered {} times against the edge",
        after - before
    );
}
