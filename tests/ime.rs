//! Composed input, which both of this experiment's documents called the one
//! thing it was blocked on.
//!
//! `dioxus-assessment.md` said "IME does not exist — no `compositionstart` /
//! `update` / `end`", and `PROGRESS.md` carried it as the single item that
//! needed a decision rather than a workaround: a reader who writes Japanese,
//! Chinese or Korean composes every word in a candidate window, and a find bar
//! that cannot take a composition cannot be searched in those languages at
//! all. That was true of Blitz once and is not true of the revision this tree
//! is pinned to. This file is the evidence, and it is here rather than in
//! `tests/search.rs` because what is being asked about is the *input path*
//! and the find field is only the one place this reader currently has to ask
//! it from.
//!
//! What arrives is not a DOM `CompositionEvent` and never will be — the DOM's
//! composition events are a *notification* that a composition is under way,
//! and Blitz instead applies the composition to the focused element's editor
//! through Parley and tells the application about the result. The reader wants
//! the result. Nothing in `app.rs` had to change for any of this to work,
//! which is why there is no source change beside these tests: the find field
//! is an ordinary `<input>` with an `oninput` handler, and a committed
//! composition is an `input` event like any other.

use moonowl::fixture;
use moonowl::harness::{Options, Reader};

/// A reader over the six pages of prose with the find bar up, which is
/// `tests/search.rs`'s own opening and is repeated rather than shared because
/// a test file that reaches into another one is a test file that cannot be
/// read on its own.
fn searching() -> Reader {
    let mut reader = Reader::open_with(&fixture::prose_pdf(), Options::default());
    reader.press_chord("mod+f");
    reader
}

/// The whole of what was in doubt: a word nobody can type a keystroke at a
/// time reaches the field, and the reader searches for it.
#[test]
fn a_composed_word_reaches_the_field_and_is_searched_for() {
    let mut reader = searching();
    // What a Japanese input method shows in its candidate window on the way to
    // 日本語, and then the word itself.
    reader.compose(&["に", "にほん", "にほんご"], "日本語");
    reader.scan_out();

    let state = reader.state();
    assert_eq!(state.query, "日本語", "the field holds what was committed");
    // The fixture is set in Helvetica and has no Japanese in it, so "No matches" is
    // the right answer and is a different answer from silence: it says the
    // scan ran over the whole document and came back.
    assert_eq!(state.find.as_deref(), Some("No matches"));
}

/// And a composed word that *is* in the document is found, which is the half
/// the assertion above cannot make with a fixture written in Helvetica.
///
/// `résumé` is the case: two characters nobody has a key for, composed on a
/// Mac as ⌥e then e, and on the page because `build_prose` puts it there.
#[test]
fn a_composed_word_that_is_in_the_document_is_found() {
    let mut reader = searching();
    reader.compose(&["r", "ré", "résum"], "résumé");
    reader.scan_out();

    let state = reader.state();
    assert_eq!(state.query, "résumé");
    assert_eq!(state.find.as_deref(), Some("1 of 1"));
    assert_eq!(
        state.page, 5,
        "and the reader is taken to the page it is on"
    );
}

/// **A preedit is not a query.** What is in the candidate window is a guess
/// the input method has not been told is right, and searching for it would
/// mean a scan of the whole document per keystroke of romaji — every one of
/// them for a string the reader never asked for.
///
/// This is upstream's behaviour rather than this reader's care:
/// `apply_generated_text_input_event` answers `PreEditChange` with a redraw
/// and no `input` event, so the handler in `app.rs` never runs. A browser
/// *does* fire `input` mid-composition and sets `isComposing` for the
/// application to check — and `main.ts` does not check it, so the app searches
/// for every intermediate guess and this reader does not. It is a small thing
/// and it is the second time the port has come out ahead by inheriting a
/// stricter substrate.
#[test]
fn a_preedit_is_not_searched_for_until_it_is_committed() {
    let mut reader = searching();
    reader.preedit("needl");
    assert_eq!(
        reader.state().find.as_deref(),
        Some(""),
        "the bar says nothing, because nothing has been searched for",
    );

    reader.compose(&[], "needle");
    reader.scan_out();
    assert_eq!(reader.state().find.as_deref(), Some("1 of 3"));
}

/// The empty preedit before a commit is winit's contract and not a nicety.
///
/// Without it the commit lands *beside* the composing region rather than in
/// place of it, and the field ends up holding both — にほん日本語. It is worth
/// a test because the failure looks exactly like a Blitz fault and is not one,
/// and because `Reader::compose` sending that empty preedit is the only reason
/// no test above has to know about it.
#[test]
fn a_commit_replaces_the_composition_rather_than_following_it() {
    let mut reader = searching();
    reader.compose(&["に", "にほん"], "日本語");
    assert_eq!(reader.state().query, "日本語");

    // And the other way round, which is the half worth writing down: a commit
    // that is not preceded by the empty preedit is inserted at the selection
    // and leaves the composing region where it was. `Reader::compose` is the
    // only reason no other test here has to know that.
    let mut raw = searching();
    raw.preedit("にほん");
    raw.commit("日本語");
    assert_eq!(
        raw.state().query,
        "にほん日本語",
        "the composition is still there, with the commit after it",
    );
}

/// A composition put into the field does not reach the document behind it.
///
/// The find field lets a chord with a modifier past and swallows everything
/// else — `tests/search.rs` asserts that for typing, and a composition is the
/// case that arrives by another door entirely: it is applied to the focused
/// editor rather than dispatched as keys, so the root's handler is not even on
/// its path. The page the reader was on is the page they are still on.
#[test]
fn composing_into_the_field_does_not_drive_the_document() {
    let mut reader = searching();
    let before = reader.state().page;
    reader.compose(&["に", "にほん", "にほんご"], "日本語");
    assert_eq!(reader.state().page, before);
}
