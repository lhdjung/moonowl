//! The window with nothing in it: the start screen, the shelf, and dropping a
//! document on it.
//!
//! The largest thing the interface did not have, and the one whose absence
//! reached furthest: `PROGRESS.md`'s own account of ⌘N, of `Handover::Fill`
//! and of there being no way to close a document all named the missing start
//! screen as the reason. So the tests here are about that reach as much as
//! about the screen — what a window with nothing in it *is*, and what the
//! rest of the reader does differently once one can exist.
//!
//! Everything is asked of the interface, which is this suite's own rule: the
//! shelf is read off the rows somebody would click, and the state of the
//! window off whether the start screen is on it.

use std::path::{Path, PathBuf};

use moonowl::harness::{Options, Reader};
use moonowl::windows::{Desk, Handover};
use moonowl::{fixture, store};

/// A settings directory nothing else is using.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("moonowl-shelf-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn reader_at(path: &str, dir: &Path) -> Reader {
    Reader::open_with(
        path,
        Options {
            config: dir.to_path_buf(),
            // 1280 rather than the harness's 1100: the Document menu hangs
            // off the document's name, which at 1100 is squeezed to its floor
            // on `.bar-left` — and on a machine whose `ui-sans-serif` is wider
            // than SF Pro, out of its own group, where Blitz does not
            // hit-test it. See `tests/menus.rs`.
            width: 1280,
            ..Options::default()
        },
    )
}

/// Put the document down: the window stays, and what is in it is the shelf.
#[test]
fn closing_a_document_leaves_the_start_screen() {
    let dir = scratch("close");
    let mut reader = reader_at(&Reader::book(), &dir);
    assert!(!reader.state().empty, "a document is open to begin with");

    reader.click("[data-item=\"close-document\"]");

    let state = reader.state();
    assert!(state.empty, "the start screen is what is left");
    assert_eq!(state.pages, 0, "and there is no document behind it");
    // The bar keeps what is not about a document and loses what is. The
    // document's own name is what goes: "Open…" is a chip of its own now and
    // stands whether or not there is anything open — see `Menu::Open`, which
    // is the split this used to predate.
    assert!(state.toolbar, "the toolbar is still there");
    assert_eq!(state.title, "", "the document's name is gone with it");
    assert_eq!(
        reader.harness.text_content(".chip.open"),
        "Open…",
        "and the bar still offers to open something",
    );
    assert!(
        reader.harness.query(".chip.contents").is_none(),
        "nothing to show the contents of",
    );
    assert!(
        reader.harness.query(".chip.find").is_none(),
        "and nothing to search",
    );
    assert!(
        reader.harness.query(".chip.theme").is_some(),
        "the theme is not about a document and stays",
    );
}

/// And the document that was put down is on the shelf, with the page it was
/// left on.
#[test]
fn the_shelf_says_where_you_stopped() {
    let dir = scratch("shelf");
    let mut reader = reader_at(&Reader::book(), &dir);
    reader.press("End");
    let stopped = reader.state().page;
    assert!(stopped > 1, "the end of the book is not page one");

    reader.click("[data-item=\"close-document\"]");

    let recents = reader.state().recents;
    assert_eq!(recents.len(), 1, "one document has been read: {recents:?}");
    assert!(
        recents[0].starts_with("book.pdf"),
        "called what it is called: {recents:?}",
    );
    assert!(
        recents[0].ends_with(&format!("p. {stopped}")),
        "and left where it was left: {recents:?}",
    );
}

/// A row on the shelf opens that document, in this window.
#[test]
fn a_row_on_the_shelf_opens_it() {
    let dir = scratch("row");
    // Two documents, so that the one being opened is a choice rather than the
    // only thing there.
    let other = fixture::titled_pdf("A Paper With A Name");
    {
        let mut first = reader_at(&other, &dir);
        first.press("j");
    }
    let mut reader = reader_at(&Reader::book(), &dir);
    reader.click("[data-item=\"close-document\"]");
    assert_eq!(reader.state().recents.len(), 2, "both are on the shelf");

    reader.click(".recent-open");
    let state = reader.state();
    assert!(!state.empty, "the shelf is gone and a document is open");
    assert!(state.pages > 0);
    // The most recently read is first, and that is the one that was clicked:
    // `book.pdf` was open a moment ago, so it is at the front of the list.
    assert_eq!(state.title, "book.pdf");
}

/// The × takes a document off the shelf, and off the shelf for good.
#[test]
fn a_document_can_be_taken_off_the_shelf() {
    let dir = scratch("forget");
    let other = fixture::titled_pdf("Something Else Entirely");
    {
        let _ = reader_at(&other, &dir);
    }
    let mut reader = reader_at(&Reader::book(), &dir);
    reader.click("[data-item=\"close-document\"]");
    assert_eq!(reader.state().recents.len(), 2);

    reader.click(".recent-forget");
    assert_eq!(reader.state().recents.len(), 1, "one row went");

    // And it is gone from the file, not merely from the screen: a second
    // reader over the same directory reads the library rather than this one's
    // memory of it.
    let after = Reader::empty(Options {
        config: dir.clone(),
        ..Options::default()
    });
    assert_eq!(after.state().recents.len(), 1, "gone from the library too");
}

/// A window made with nothing in it is the start screen, with the shelf on it.
#[test]
fn an_empty_window_shows_the_shelf() {
    let dir = scratch("empty");
    {
        let _ = reader_at(&Reader::book(), &dir);
    }
    let reader = Reader::empty(Options {
        config: dir.clone(),
        ..Options::default()
    });
    let state = reader.state();
    assert!(state.empty);
    assert_eq!(state.recents.len(), 1, "what was read last is offered");
}

/// A document handed to a window that is showing nothing lands in it.
///
/// This is `Handover::Fill` arriving for the first time — the arm `windows.rs`
/// carried a comment on for two phases saying it could not be reached until
/// there was a start screen.
#[test]
fn a_document_handed_to_an_empty_window_lands_in_it() {
    let dir = scratch("handover");
    let mut reader = Reader::empty(Options {
        config: dir.clone(),
        ..Options::default()
    });
    assert!(reader.state().empty);

    reader.hand_over(&Reader::book());

    let state = reader.state();
    assert!(!state.empty, "the document arrived");
    assert_eq!(state.title, "book.pdf");
    assert_eq!(state.pages, 400);
}

/// And the desk sends it there rather than making a second window.
#[test]
fn the_desk_fills_a_window_that_is_showing_nothing() {
    let desk = Desk::new();
    let one = desk.name();
    desk.set(&one, Some("/papers/one.pdf"));
    let empty = desk.name();
    // Not the one in front, so this is the walk rather than the shortcut.
    desk.focused(Some(&one));

    assert_eq!(
        desk.hand_over("/papers/two.pdf"),
        Handover::Fill(empty),
        "the empty window takes it",
    );
}

/// A dragged document says what will happen before it is let go.
#[test]
fn a_document_dragged_over_the_window_says_so() {
    let dir = scratch("drag");
    let mut reader = reader_at(&Reader::book(), &dir);
    assert_eq!(reader.state().dragging, None, "nothing is over the window");

    reader.drag_over(true);
    assert_eq!(reader.state().dragging.as_deref(), Some("Drop to open"));

    reader.drag_left();
    assert_eq!(reader.state().dragging, None, "and it came back down");

    // Something this reader will not open says so instead, rather than
    // promising to catch it.
    reader.drag_over(false);
    assert_eq!(
        reader.state().dragging.as_deref(),
        Some("That is not a PDF"),
    );
}

/// Letting one go on a document opens it beside it; on nothing, here.
#[test]
fn a_document_let_go_is_opened() {
    let dir = scratch("drop");
    let named = fixture::titled_pdf("The Dropped One");
    let mut reader = reader_at(&Reader::book(), &dir);
    reader.drag_over(true);
    reader.drop_document(&named);

    let state = reader.state();
    assert_eq!(state.dragging, None, "the hint went with the drop");
    assert_ne!(state.title, "The Dropped One", "the open document stays");
    assert!(
        reader
            .asks()
            .contains(&moonowl::app::Ask::SendOn(named.clone())),
        "and the dropped one goes beside it: {:?}",
        reader.asks()
    );

    // Into a window showing nothing, it is opened there.
    let mut empty = Reader::empty(Options {
        config: dir.clone(),
        ..Options::default()
    });
    empty.drop_document(&named);
    assert_eq!(empty.state().title, "The Dropped One");
}

/// Letting go of something else says why nothing happened.
#[test]
fn something_that_is_not_a_document_is_refused_out_loud() {
    let dir = scratch("refused");
    let mut reader = reader_at(&Reader::book(), &dir);
    reader.drag_over(true);
    reader.drag_refused();

    let state = reader.state();
    assert_eq!(state.dragging, None);
    assert_eq!(state.notice, "That is not a PDF.");
    assert_eq!(state.title, "book.pdf", "and nothing was opened");
}

/// The one already open is not offered on its own shelf.
///
/// The app's own rule for the same list: reopening it here is a no-op, and
/// opening it in a second window is what its own title menu is for.
#[test]
fn the_document_in_front_of_you_is_not_on_the_shelf() {
    let dir = scratch("self");
    let mut reader = reader_at(&Reader::book(), &dir);
    reader.click(".chip.title");
    let rows = reader.text_all(".menu.document .menu-label");
    assert!(
        !rows.iter().any(|row| row == "book.pdf"),
        "the open document is not offered again: {rows:?}",
    );
}

/// Closing a document empties the restore list, which is the gesture that
/// says "I have finished with this".
#[test]
fn closing_a_document_tells_the_process_the_window_is_empty() {
    let dir = scratch("showing");
    let mut reader = reader_at(&Reader::book(), &dir);
    reader.click("[data-item=\"close-document\"]");

    let asked = reader.asks();
    assert!(
        asked.iter().any(|ask| matches!(
            ask,
            moonowl::app::Ask::Showing { path, .. } if path.is_empty()
        )),
        "an empty path is how a window says it is showing nothing: {asked:?}",
    );
}

/// And the store stops pointing at the document, so nothing written
/// afterwards lands on it.
#[test]
fn the_shelf_is_not_touched_by_closing() {
    let dir = scratch("untouched");
    {
        let mut reader = reader_at(&Reader::book(), &dir);
        reader.click("[data-item=\"close-document\"]");
    }
    // One entry, keyed by a real path. A close that went through `opened`
    // would have written a second, keyed by the empty string, and put it at
    // the front — a row nobody can open where the last thing read should be.
    let shelf = store::Store::at(&dir).recents();
    assert_eq!(shelf.len(), 1, "{shelf:?}");
    assert!(!shelf[0].path.is_empty());
}

/// A key that is about a document does nothing when there is none, and a key
/// that is not still works.
#[test]
fn the_keyboard_knows_there_is_no_document() {
    let dir = scratch("keys");
    let mut reader = Reader::empty(Options {
        config: dir.clone(),
        ..Options::default()
    });

    // Every one of these is `needsDocument` in the app's own table: moving
    // around inside a document, marking a page, highlighting a passage.
    for key in ["j", "End", "n"] {
        reader.press(key);
    }
    for chord in ["mod+shift+b", "mod+shift+h", "mod+g"] {
        reader.press_chord(chord);
    }
    let state = reader.state();
    assert!(state.empty, "still the start screen");
    assert_eq!(state.notice, "", "and nothing said about any of it");
    // ⌘F is the one key this reader guards that the app does not — see
    // `Viewer::open_find`, which says why.
    reader.press_chord("mod+f");
    assert_eq!(reader.state().find, None, "no find bar over nothing");

    // …and one that is not about a document. ⌘N asks for a window, which is
    // the whole of what it does from here.
    reader.press_chord("mod+n");
    assert!(
        reader
            .asks()
            .iter()
            .any(|ask| matches!(ask, moonowl::app::Ask::NewWindow)),
        "⌘N still asks for a window: {:?}",
        reader.asks(),
    );
}

/// **The shelf is under the Open… menu too**, which is where a reader with a
/// document already open can reach it: the start screen is behind that document
/// and unreachable without first putting it down. The rows were written and
/// never drawn — the list was read while the *Document* menu was open and shown
/// under the Open menu, which are two different menus.
#[test]
fn the_open_menu_carries_the_shelf() {
    let dir = scratch("open-menu");
    let other = fixture::titled_pdf("A Paper With A Name");
    {
        let mut first = reader_at(&other, &dir);
        first.press("j");
    }
    let mut reader = reader_at(&Reader::book(), &dir);

    reader.click(".chip.open");
    let rows = reader.harness.query_all("[data-item='recent']").len();
    assert_eq!(rows, 1, "the document that was read before this one");
    let listed = reader.text_all(".menu.open .menu-label");
    assert!(
        listed
            .iter()
            .any(|name| name.contains("A Paper With A Name")),
        "named on its own row: {listed:?}",
    );

    // And the row opens it, in this window.
    reader.click("[data-item='recent']");
    assert_eq!(reader.state().title, "A Paper With A Name");
}
