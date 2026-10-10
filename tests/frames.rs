//! **A scroll frame is one render, and not one of the toolbar.**
//!
//! Its own file, as `settle.rs` is, because the counter it reads is the
//! process's.

use moonowl::harness::{Options, Reader};
use moonowl::stats;

/// One at a time: the counters are the process's, and two readers scrolling
/// at once count each other's frames.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn alone() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

/// The scroll writes the reader once, and the bar it brings up is already up
/// on every frame after the first: putting it up again only restarts its
/// clock, which is no reason to render. The toolbar shows nothing a scroll
/// within a page changes, so it does not render at all.
#[test]
fn a_scroll_frame_renders_once() {
    let _alone = alone();
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

/// The same with the toolbar away, which is when the page pill is up: the
/// pill already up restarts its clock without a write, as the bar does.
#[test]
fn a_scroll_frame_with_the_pill_up_renders_once() {
    let _alone = alone();
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            width: 700,
            height: 560,
            settings: vec![("show_page_pill".into(), serde_json::json!(true))],
            ..Default::default()
        },
    );
    reader.press_action(moonowl::keymap::Action::Toolbar);
    reader.settle();
    // Past the point where the second page mounts: its text landing is a
    // render of its own, and not a scroll frame's.
    reader.wheel(60.0);
    assert!(
        reader.harness.query(".page-pill").is_some(),
        "the pill is not up with the toolbar away"
    );
    let before = stats::get(&stats::RENDERS);
    for _ in 0..5 {
        reader.wheel(10.0);
    }
    let renders = stats::get(&stats::RENDERS) - before;
    assert_eq!(renders, 5, "five scroll frames took {renders} renders");
}
