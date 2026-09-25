//! Where you were, what the document is called, and what was open last.
//!
//! Phase 3 item 7. The library itself is `src-tauri/src/library.rs`, mounted
//! into this crate unchanged and carrying its own eight tests — this file is
//! about the half that is the *reader's*: that a document opened again opens
//! where it was left, that it is called what it calls itself when that is
//! worth having, and that a launch with nothing named comes back to what was
//! being read.
//!
//! Everything here is asked of the interface or of the next run, because
//! those are the two places the feature actually shows: the toolbar says the
//! name, and a second reader over the same directory is what "the next time
//! you open it" means with no window to close.

use std::path::{Path, PathBuf};

use moonowl::harness::{Options, Reader};
use moonowl::{fixture, store};

/// A settings directory nothing else is using, and a reader over it.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("moonowl-library-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn reader_at(path: &str, dir: &Path, settings: Vec<(String, serde_json::Value)>) -> Reader {
    Reader::open_with(
        path,
        Options {
            config: dir.to_path_buf(),
            settings,
            ..Options::default()
        },
    )
}

/// The whole of the feature, in the order a reader meets it: read some of a
/// book, put it down, pick it up again.
#[test]
fn a_document_opens_where_it_was_left() {
    let dir = scratch("place");
    let book = Reader::book();

    let left = {
        let mut reader = reader_at(&book, &dir, Vec::new());
        // Not the very end, because the end of a document is where a reader
        // lands by clamping as well as by remembering, and a test that cannot
        // tell those apart is not a test.
        for _ in 0..6 {
            reader.wheel_screen();
        }
        let state = reader.state();
        assert!(state.page > 1, "the reader did not move: {state:?}");
        assert!(state.scroll > 0.0);
        // A position is written when the scrolling stops, on a thread of its
        // own; quitting is what does not wait for that, and this is the same
        // call quitting makes.
        reader.flush();
        state
    };

    let back = reader_at(&book, &dir, Vec::new()).state();
    assert_eq!(back.page, left.page, "{back:?} against {left:?}");
    assert!(
        (back.scroll - left.scroll).abs() < 2.0,
        "{} against {}",
        back.scroll,
        left.scroll
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Straight back to it, inside the scribe's settle: the place was read off
/// the disk before the scribe had written it.
#[test]
fn a_document_returned_to_at_once_opens_where_it_was_left() {
    let dir = scratch("return");
    let book = Reader::book();
    let mut reader = reader_at(&book, &dir, Vec::new());
    for _ in 0..6 {
        reader.wheel_screen();
    }
    let left = reader.state();
    assert!(left.page > 1, "{left:?}");
    for path in [moonowl::fixture::prose_pdf(), book] {
        reader.deliver(moonowl::emit::News {
            event: "open-document".into(),
            target: None,
            payload: moonowl::emit::Payload::Text(path),
        });
    }
    let back = reader.state();
    assert_eq!(back.page, left.page, "{back:?} against {left:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// And it opens at the top when the reader has said they would rather it did.
/// The switch is the app's own, out of the `settings.rs` this crate mounts.
#[test]
fn remembering_where_you_were_can_be_turned_off() {
    let dir = scratch("forget");
    let book = Reader::book();
    let off = vec![("remember_position".to_string(), serde_json::json!(false))];

    {
        let mut reader = reader_at(&book, &dir, off.clone());
        for _ in 0..6 {
            reader.wheel_screen();
        }
        assert!(reader.state().page > 1);
        reader.flush();
    }

    let back = reader_at(&book, &dir, off).state();
    assert_eq!(back.page, 1, "{back:?}");
    assert_eq!(back.scroll, 0.0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A page turned in paged mode never moves the scroll offset — both sides of
/// the turn sit at the top of their own page — so the write cannot be left to
/// the scroller. It is the one case the continuous test above cannot reach.
#[test]
fn one_page_at_a_time_remembers_which_page() {
    let dir = scratch("paged");
    let book = Reader::book();
    let paged = vec![("scroll_mode".to_string(), serde_json::json!("paged"))];

    {
        let mut reader = reader_at(&book, &dir, paged.clone());
        for _ in 0..4 {
            reader.press("l");
        }
        assert_eq!(reader.state().page, 5, "{:?}", reader.state());
        reader.flush();
    }

    assert_eq!(reader_at(&book, &dir, paged).state().page, 5);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `2310.06825v3.pdf` is not a name, and the file usually knows better.
#[test]
fn a_document_is_called_what_it_calls_itself() {
    let dir = scratch("titled");
    let path = fixture::titled_pdf("The Structure of Scientific Revolutions");
    let reader = reader_at(&path, &dir, Vec::new());
    assert_eq!(
        reader.state().title,
        "The Structure of Scientific Revolutions"
    );

    // And the library has it too, which is what a shelf would be drawn from.
    let library = moonowl::library::load(&dir);
    assert_eq!(
        library.files[0].title,
        "The Structure of Scientific Revolutions"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The half that is doing the work: a great many documents carry a title the
/// program that made them filled in, and every one of those is worse than the
/// file name because it looks deliberate.
#[test]
fn a_title_the_producer_filled_in_is_not_a_name() {
    for said in [
        "Microsoft Word - report.doc",
        "untitled",
        "Document1",
        "thesis.tex",
    ] {
        let dir = scratch(&format!("junk-{}", said.len()));
        let path = fixture::titled_pdf(said);
        let called = reader_at(&path, &dir, Vec::new()).state().title;
        assert!(
            called.ends_with(".pdf") && called != said,
            "{said:?} was taken as a name: {called:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// What a launch with nothing named comes back to.
#[test]
fn what_was_open_is_what_opens_next_time() {
    let dir = scratch("open");
    let path = fixture::titled_pdf("A Paper About Something");
    reader_at(&path, &dir, Vec::new());
    assert_eq!(store::reopening(&dir).as_deref(), Some(path.as_str()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Unless the reader would rather start fresh, which is the app's own setting
/// and is asked where the answer is used rather than by the caller.
#[test]
fn a_reader_who_would_rather_start_fresh_is_not_argued_with() {
    let dir = scratch("fresh");
    let path = fixture::titled_pdf("Another Paper Entirely");
    reader_at(
        &path,
        &dir,
        vec![("reopen_last_document".to_string(), serde_json::json!(false))],
    );
    assert_eq!(store::reopening(&dir), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// And a document that has been moved or deleted is not offered again, which
/// is `prune` doing its job: without it a launch fails on the same missing
/// file for ever.
#[test]
fn a_document_that_is_gone_is_not_reopened() {
    let dir = scratch("gone");
    let source = fixture::titled_pdf("A Paper That Will Vanish");
    let copy = dir.join("vanishing.pdf");
    std::fs::create_dir_all(&dir).expect("scratch");
    std::fs::copy(&source, &copy).expect("copy the fixture");
    let copy = copy.to_string_lossy().into_owned();

    reader_at(&copy, &dir, Vec::new());
    assert_eq!(store::reopening(&dir).as_deref(), Some(copy.as_str()));

    std::fs::remove_file(&copy).expect("take it away");
    assert_eq!(store::reopening(&dir), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **A launch is one window, on the document read most recently.** `open` is
/// still a list — two windows put two paths in it — and the order of that list
/// is the order the *windows* were made, which says nothing about which
/// document the reader was actually in. `opened_at` does.
#[test]
fn a_session_of_two_windows_comes_back_as_the_document_read_last() {
    use moonowl::library;

    let dir = scratch("two-windows");
    std::fs::create_dir_all(&dir).expect("scratch");
    let first = fixture::titled_pdf("The One Opened First");
    let second = fixture::titled_pdf("The One Opened Second");

    library::touch(&dir, &first, "first", 100).expect("first");
    library::touch(&dir, &second, "second", 200).expect("second");
    // The window on `first` was made first, so it is first in the list.
    library::set_open(&dir, &[first.clone(), second.clone()]).expect("set open");

    assert_eq!(store::reopening(&dir).as_deref(), Some(second.as_str()));
    // Opened second, but the first is where the reader went on reading.
    library::remember(&dir, &first, 3, 0.5, "3").expect("remember");
    assert_eq!(store::reopening(&dir).as_deref(), Some(first.as_str()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// **Recently read says what the toolbar said**: the printed page, "ii",
/// rather than its place in the file.
#[test]
fn the_place_remembered_carries_the_pages_own_name() {
    let mut reader = Reader::open(&moonowl::fixture::links_pdf());
    reader.press("ArrowRight");
    assert_eq!(reader.state().label, "ii");
    reader.flush();
    let library = moonowl::library::load(&reader.config);
    let entry = library.files.first().expect("an entry");
    assert_eq!((entry.page, entry.label.as_str()), (2, "ii"));
}
