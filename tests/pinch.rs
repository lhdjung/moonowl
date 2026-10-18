//! What a pinch costs: the page under the fingers is stretched, not drawn
//! again at every step. Alone in its file, because it counts every page the
//! process draws.

use moonowl::harness::{Options, Reader};
use moonowl::stats;

/// **Links do not make a pinch draw the page again.** In a theme that
/// recolours, the links are painted into the page, and their places are
/// part of what a drawn page is kept for. Measured against the box rounded
/// to whole pixels, they moved a little at every step of the pinch, and
/// every step drew the page again.
#[test]
fn a_pinch_over_links_draws_nothing_new() {
    let mut reader = Reader::open_with(
        &moonowl::fixture::links_pdf(),
        Options {
            // Moonowl Dark, which recolours.
            theme: Some(1),
            ..Options::default()
        },
    );
    reader.screenshot();
    let before = stats::get(&stats::DRAWN);
    for step in 0..12 {
        reader.pinch(0.013 + 0.001 * step as f64);
        reader.screenshot();
    }
    assert_eq!(
        stats::get(&stats::DRAWN),
        before,
        "the pages were stretched, not drawn again"
    );
}
