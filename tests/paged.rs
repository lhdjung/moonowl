//! One page at a time — the other half of Phase 3 item 6.
//!
//! **There is no key for this and no chip, on purpose.** The brief calls
//! continuous scrolling a strong default that may only ever change if the
//! reader explicitly opts into it, and says in as many words that a shortcut
//! for it would be a thing to hit by accident. So the whole interface for it
//! is a line in `settings.toml`, and every test here opens a reader that has
//! one — which is also what `Options.settings` was added for.

use moonowl::harness::{Options, Reader};
use serde_json::json;

fn paged() -> Reader {
    Reader::open_with(
        &Reader::book(),
        Options {
            settings: vec![("scroll_mode".into(), json!("paged"))],
            ..Options::default()
        },
    )
}

#[test]
fn one_page_is_laid_out_and_the_rest_of_the_book_is_not() {
    let reader = paged();
    let state = reader.state();
    assert_eq!(state.page, 1);
    assert_eq!(state.mounted, vec![1], "one page, and it is the first");

    // The same document read continuously has its neighbours in the document
    // as well, which is the whole difference between the two modes. Read a
    // little way in first: at the very top of a book fitted to the width,
    // one page fills the overscan band in either mode.
    let mut continuous = Reader::open(&Reader::book());
    continuous.press(" ");
    continuous.settle();
    assert!(
        continuous.state().mounted.len() > 1,
        "{:?}",
        continuous.state().mounted
    );

    // …and the same distance down the paged reader is still one page.
    let mut reader = paged();
    reader.press(" ");
    reader.settle();
    assert_eq!(reader.state().mounted.len(), 1);
}

#[test]
fn a_page_turn_is_a_page_turn_and_not_a_scroll() {
    let mut reader = paged();
    reader.press("l");
    reader.settle();
    let state = reader.state();
    assert_eq!(state.page, 2);
    assert_eq!(state.mounted, vec![2]);
    // The strip holds one page, so arriving at it starts at the top of it
    // rather than wherever the last page had been scrolled to.
    assert_eq!(state.scroll, 0.0);

    reader.press("h");
    reader.settle();
    assert_eq!(reader.state().page, 1);
}

#[test]
fn scrolling_past_the_end_of_a_page_turns_it() {
    let mut reader = paged();
    // A page fitted to the width is taller than the window, so the first
    // screen down is a scroll and stays on page one.
    reader.press(" ");
    reader.settle();
    assert_eq!(reader.state().page, 1);
    assert!(reader.state().scroll > 0.0);

    // …and pushing on from the bottom of it is what turns the page. Down the
    // page a screen at a time until it does, which is how a reader would meet
    // it and is not a fixed number of presses: how many screens a page is
    // depends on the window.
    let mut turned = None;
    for _ in 0..10 {
        reader.press(" ");
        reader.settle();
        if reader.state().page != 1 {
            turned = Some(reader.state());
            break;
        }
    }
    let state = turned.expect("a page that is scrolled to its end turns");
    assert_eq!(state.page, 2, "{state:?}");
    assert_eq!(state.scroll, 0.0, "and at the top of it");

    // Backwards is the bottom of the page arrived at, because that is where
    // the reader was reading.
    reader.press("ArrowUp");
    reader.settle();
    let state = reader.state();
    assert_eq!(state.page, 1, "{state:?}");
    assert!(state.scroll > 0.0, "at the bottom of page one: {state:?}");
}

/// **One flick of a trackpad is one page.** A swipe is dozens of events and
/// then its momentum, and at the foot of a page every one of them turned one.
#[test]
fn a_swipe_at_the_foot_of_a_page_turns_one_page() {
    let mut reader = paged();
    for _ in 0..20 {
        reader.wheel(600.0);
    }
    let page = reader.state().page;
    assert!(
        (1..=2).contains(&page),
        "one flick ran through the book: page {page}"
    );
}

#[test]
fn the_ends_of_the_document_are_the_first_and_last_pages() {
    let mut reader = paged();
    reader.press("End");
    reader.settle();
    let state = reader.state();
    assert_eq!(state.page, state.pages, "{state:?}");
    assert_eq!(state.mounted, vec![state.pages]);

    reader.press("Home");
    reader.settle();
    assert_eq!(reader.state().page, 1);
}

#[test]
fn a_page_typed_into_the_field_is_turned_to() {
    let mut reader = paged();
    reader.press("p");
    reader.type_text("50");
    reader.press("Enter");
    reader.settle();
    let state = reader.state();
    assert_eq!(state.page, 50);
    assert_eq!(state.mounted, vec![50]);

    // And back, through the history, which is the same door.
    reader.press_chord("mod+[");
    reader.settle();
    assert_eq!(reader.state().page, 1);
}

#[test]
fn a_match_on_another_page_turns_to_it() {
    let mut reader = paged();
    reader.press_chord("mod+f");
    reader.type_text("page 40");
    reader.scan_out();
    reader.settle();
    let state = reader.state();
    assert!(
        state.page > 1,
        "the reader was taken to the match: {state:?}"
    );
    assert_eq!(
        state.mounted,
        vec![state.page],
        "and that page is the one page laid out"
    );
    assert!(state.hits > 0, "with the match painted on it: {state:?}");
}

#[test]
fn the_mode_is_a_setting_and_nothing_else_can_reach_it() {
    // Every action in the app's table, pressed at a reader in paged mode:
    // none of them may put it back into continuous scrolling. This is the
    // brief's requirement, and the only way to hold a *keymap* to it is to
    // press the whole of it.
    let mut reader = paged();
    for chord in [
        "s",
        "t",
        "j",
        "k",
        "d",
        "u",
        "h",
        "l",
        "g",
        "G",
        "p",
        "mod+0",
        "mod+1",
        "mod+2",
        "mod+b",
        "mod+r",
        "mod+l",
        "mod+shift+p",
        "mod+shift+f",
        "escape",
    ] {
        reader.press_chord(chord);
        reader.settle();
        let state = reader.state();
        // At most two, because `s` turns spreads on and a row of a spread is
        // two pages side by side — which is one row still, and one row is
        // what paged mode lays out.
        assert!(
            state.mounted.len() <= 2,
            "{chord:?} left {} pages laid out",
            state.mounted.len()
        );
    }
}
