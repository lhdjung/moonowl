//! Windows, presenting, and the keys that ask for them.
//!
//! Phase 3 item 9. The rules about *which* window gets what are unit tests at
//! the bottom of `src/windows.rs`, because they have no window in them; this
//! file is the reader's half — what a key does, what the chrome does when it
//! is taken away, and what the window is asked for.
//!
//! **None of this can be tested in the app**, which is the point worth making.
//! `AGENTS.md` says so about the whole of its window story: "None of this can
//! be tested in the harness, which has no Rust behind it and no windows", and
//! what stands in for it there is a list of things somebody checked by hand in
//! a running app — including "Escape leaves full screen", which is
//! specifically called out as a real-app check because a browser in full
//! screen keeps the key. Here the reader asks its window for things through
//! one door ([`moonowl::app::Frame`]) and the harness writes the asks
//! down, so the asking is a test even though the window is not.
//!
//! What is still a real-app check is what the *shell* does with an ask, and
//! that is one file away: `shell.rs` turns each of these into a winit call.

use std::collections::BTreeMap;
use std::path::PathBuf;

use moonowl::app::Ask;
use moonowl::harness::{Options, Reader};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("moonowl-windows-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn reader(name: &str) -> Reader {
    reader_with(name, Vec::new())
}

fn reader_with(name: &str, settings: Vec<(String, serde_json::Value)>) -> Reader {
    Reader::open_with(
        &Reader::book(),
        Options {
            config: scratch(name),
            settings,
            // ⌘W and ⌘Q ship bound on every platform *but* macOS, where the
            // menu bar answers them before the page ever sees them — so on
            // the machine this is developed on they are bound to nothing at
            // all. Bound here, because what is being tested is what the
            // reader does with the action and not which key asks for it;
            // `tests/keys.rs` is where the defaults are held to.
            keys: BTreeMap::from([
                ("close-window".to_string(), vec!["mod+w".to_string()]),
                ("quit".to_string(), vec!["mod+q".to_string()]),
            ]),
            ..Options::default()
        },
    )
}

/// The height of the box the document is drawn in, which is the window minus
/// whatever chrome is on screen.
fn viewport(reader: &Reader) -> f64 {
    let node = reader.harness.query(".viewer").expect("a viewer");
    reader.harness.layout_rect_of(node).height as f64
}

#[test]
fn a_second_window_is_asked_for_rather_than_made() {
    let mut reader = reader("new");
    reader.press_chord("mod+n");
    assert_eq!(reader.asks(), vec![Ask::NewWindow]);
    // And nothing about this window moved: a new window is the *other*
    // window's business from here on, which is the whole reason a second one
    // costs this reader nothing.
    assert_eq!(reader.state().page, 1);
}

/// Two actions rather than one, because there is more than one window now and
/// a key that closed whichever happened to have the keyboard would be a
/// strange thing for Quit to do. The app's own reasoning, in `keys.ts`.
#[test]
fn closing_a_window_and_leaving_are_not_the_same_ask() {
    let mut reader = reader("close");
    reader.press_chord("mod+w");
    reader.press_chord("mod+q");
    assert_eq!(reader.asks(), vec![Ask::Close, Ask::Quit]);
}

#[test]
fn full_screen_is_asked_for_and_escape_leaves_it() {
    let mut reader = reader("full");
    reader.press_chord("mod+shift+f");
    assert_eq!(reader.asks(), vec![Ask::FullScreen(true)]);
    // Nothing on screen changes: full screen is a bigger window, and a bigger
    // window is a resize like any other.
    assert!(reader.state().toolbar);
    reader.press("Escape");
    assert_eq!(
        reader.asks(),
        vec![Ask::FullScreen(true), Ask::FullScreen(false)]
    );
}

#[test]
fn presenting_takes_everything_off_the_screen() {
    let mut reader = reader_with(
        "present",
        vec![("show_sidebar".into(), serde_json::json!(true))],
    );
    let before = viewport(&reader);
    assert!(reader.state().toolbar);
    assert!(reader.state().sidebar.is_some());

    reader.press_chord("mod+shift+p");
    let state = reader.state();
    assert!(state.presenting, "{state:?}");
    assert!(!state.toolbar, "the toolbar is still there");
    assert!(state.sidebar.is_none(), "the panel is still there");
    // Nothing is left on screen but the way out.
    assert!(state.notice.contains("Escape"), "{state:?}");
    assert_eq!(reader.asks(), vec![Ask::FullScreen(true)]);
    // And the document has the room they were using.
    let during = viewport(&reader);
    assert!(
        during > before + 40.0,
        "{during} against {before}: the document did not get the room"
    );
}

/// Presenting hides the panel rather than closing it, which is the difference
/// between putting something away and turning it off.
#[test]
fn what_was_open_before_presenting_is_open_after() {
    let mut reader = reader_with(
        "restored",
        vec![("show_sidebar".into(), serde_json::json!(true))],
    );
    let before = viewport(&reader);
    reader.press_chord("mod+shift+p");
    reader.press("Escape");
    let state = reader.state();
    assert!(!state.presenting);
    assert!(state.toolbar);
    assert!(state.sidebar.is_some(), "the panel did not come back");
    assert!((viewport(&reader) - before).abs() < 1.0);
}

/// A reader who was in full screen, presented, and then stopped is still in
/// full screen — which is where they were. Presenting and full screen are two
/// switches, and stopping one puts the other back rather than turning it off.
#[test]
fn stopping_presenting_does_not_take_full_screen_with_it() {
    let mut reader = reader("both");
    reader.press_chord("mod+shift+f");
    reader.press_chord("mod+shift+p");
    reader.press("Escape");
    assert_eq!(
        reader.asks(),
        vec![
            Ask::FullScreen(true),
            Ask::FullScreen(true),
            Ask::FullScreen(true)
        ],
        "stopping presenting asked to leave full screen"
    );
    assert!(!reader.state().presenting);
    // And Escape again is the one that leaves.
    reader.press("Escape");
    assert_eq!(reader.asks().last(), Some(&Ask::FullScreen(false)));
}

/// Escape is the way out of four things and it takes them in the order the
/// reader arrived at them. The find bar is inside presenting, not beside it.
#[test]
fn escape_closes_the_find_bar_before_it_stops_presenting() {
    let mut reader = reader("escape");
    reader.press_chord("mod+shift+p");
    reader.press_chord("mod+f");
    assert!(reader.state().find.is_some());

    reader.press("Escape");
    assert!(reader.state().find.is_none(), "the bar is still open");
    assert!(reader.state().presenting, "it stopped presenting instead");

    reader.press("Escape");
    assert!(!reader.state().presenting);
}

/// With the toolbar gone there is nothing on screen that says how to get it
/// back, so the message names the key — and reads it off the keymap, because
/// what the key *is* is whatever `keys.toml` says it is.
#[test]
fn putting_the_toolbar_away_says_how_to_bring_it_back() {
    let mut reader = reader("toolbar");
    reader.press_chord("mod+t");
    let state = reader.state();
    assert!(!state.toolbar, "the toolbar is still there");
    // The notice line survives the toolbar, which is the whole reason it is
    // not taken away with it.
    assert!(
        state.notice.contains("brings it back"),
        "notice was {:?}",
        state.notice
    );
    reader.press_chord("mod+t");
    assert!(reader.state().toolbar);
}

/// A rebound key is named in the notice, which is what reading the keymap
/// rather than stating a chord buys.
#[test]
fn the_message_names_whatever_key_the_reader_bound() {
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            config: scratch("rebound"),
            keys: BTreeMap::from([("toolbar".to_string(), vec!["shift+b".to_string()])]),
            ..Options::default()
        },
    );
    reader.press_chord("shift+b");
    let notice = reader.state().notice;
    // Asked through the same function the notice is written with, because how
    // a chord reads is the platform's business: ⇧B on a Mac, `Shift+B` on
    // Windows and Linux. What is being tested is that the *rebound* key is the
    // one named, not the default.
    let want = moonowl::keymap::describe_binding("shift+b", cfg!(target_os = "macos"));
    assert!(
        notice.contains(&want),
        "notice was {notice:?}, wanted {want:?}"
    );
}

/// And it is a setting, so a reader who reads without one gets none next time.
#[test]
fn a_reader_who_reads_without_a_toolbar_gets_none_next_time() {
    let dir = scratch("remembered");
    let book = Reader::book();
    {
        let mut reader = Reader::open_with(
            &book,
            Options {
                config: dir.clone(),
                ..Options::default()
            },
        );
        reader.press_chord("mod+t");
        assert!(!reader.state().toolbar);
    }
    let back = Reader::open_with(
        &book,
        Options {
            config: dir.clone(),
            ..Options::default()
        },
    );
    assert!(!back.state().toolbar, "the toolbar came back");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_document_open_in_another_window_is_brought_forward_not_opened_again() {
    // The picker, the shelf and a drop all land in `open_here`; two windows
    // on one document each wrote the whole of its marks, and the last won.
    let desk = moonowl::windows::Desk::new();
    desk.set("main", Some(&Reader::book()));
    desk.set("reader-1", Some(&moonowl::fixture::prose_pdf()));
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            desk: Some(desk),
            config: scratch("elsewhere"),
            ..Default::default()
        },
    );
    reader.deliver(moonowl::emit::News {
        event: "open-document".into(),
        target: None,
        payload: moonowl::emit::Payload::Text(moonowl::fixture::prose_pdf()),
    });
    assert_eq!(reader.state().pages, 400, "this window keeps what it had");
    assert_eq!(
        reader.state().notice,
        "That document is open in another window."
    );
    assert_eq!(
        reader.asks(),
        vec![Ask::NewWindowOn(moonowl::fixture::prose_pdf())],
        "and that one is asked forward"
    );
}
