//! **A scroll frame is one render, and not one of the toolbar.**
//!
//! Its own file, as `settle.rs` is, because the counter it reads is the
//! process's.

use moonowl::harness::{Options, Reader};
use moonowl::stats;

/// The scroll writes the reader once, and the bar it brings up is already up
/// on every frame after the first: putting it up again only restarts its
/// clock, which is no reason to render. The toolbar shows nothing a scroll
/// within a page changes, so it does not render at all.
#[test]
fn a_scroll_frame_renders_once() {
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            width: 700,
            height: 560,
            ..Default::default()
        },
    );
    reader.settle();
    reader.wheel(10.0);
    let before = stats::get(&stats::RENDERS);
    let toolbar_before = stats::get(&stats::TOOLBAR_RENDERS);
    for _ in 0..5 {
        reader.wheel(10.0);
    }
    let renders = stats::get(&stats::RENDERS) - before;
    assert_eq!(renders, 5, "five scroll frames took {renders} renders");
    let toolbar = stats::get(&stats::TOOLBAR_RENDERS) - toolbar_before;
    assert_eq!(
        toolbar, 0,
        "five scroll frames rendered the toolbar {toolbar} times"
    );
}
