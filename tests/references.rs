//! References with no link behind them, such as "[3]" or "Figure 2",
//! followed with ⌘-click (Ctrl-click elsewhere) to the line that begins
//! with them. See `Viewer::follow_reference` and `refs.rs`.

use dioxus::html::Modifiers;
use moonowl::fixture;
use moonowl::harness::Reader;

/// ⌘ on a Mac, Ctrl elsewhere.
fn command() -> Modifiers {
    if cfg!(target_os = "macos") {
        Modifiers::META
    } else {
        Modifiers::CONTROL
    }
}

/// Click with the command key at a point in the page's own points, from
/// its top left.
fn follow(reader: &mut Reader, page: usize, x: f32, y: f32) {
    let (x, y) = reader.point_on(page, (x / 612.0, y / 792.0));
    reader.click_at_with(x, y, command());
}

#[test]
fn a_number_in_brackets_leads_to_its_entry_in_the_bibliography() {
    let mut reader = Reader::open(&fixture::references_pdf());
    // "[3]" begins its own line on page 1, which is not where it leads.
    follow(&mut reader, 1, 78.0, 188.0);
    assert!(
        reader.wait_until(5.0, |reader| reader.state().page == 5),
        "on page {}, told {:?}",
        reader.state().page,
        reader.state().notice
    );
}

#[test]
fn a_figure_leads_to_its_caption_and_not_to_a_sentence() {
    let mut reader = Reader::open(&fixture::references_pdf());
    follow(&mut reader, 1, 80.0, 228.0);
    assert!(reader.wait_until(5.0, |reader| reader.state().page == 3));
    // The caption, at 492pt down the page and not the sentence at 292pt,
    // is two thirds of the way down the window, with the figure above it.
    let (_, top) = reader.point_on(3, (0.0, 492.0 / 792.0));
    let (_, height) = reader.window();
    let down = top / height as f32;
    assert!(
        (0.6..0.75).contains(&down),
        "the caption is {down} of the way down"
    );
}

#[test]
fn a_reference_that_leads_nowhere_is_said() {
    let mut reader = Reader::open(&fixture::references_pdf());
    follow(&mut reader, 1, 100.0, 268.0);
    assert!(reader.wait_until(5.0, |reader| !reader.state().notice.is_empty()));
    assert_eq!(
        reader.state().notice,
        "No line in this document begins with Table 7."
    );
    assert_eq!(reader.state().page, 1);
}

#[test]
fn a_click_without_the_command_key_follows_nothing() {
    let mut reader = Reader::open(&fixture::references_pdf());
    reader.click_on_page(1, (78.0 / 612.0, 188.0 / 792.0));
    reader.later(1.0);
    assert_eq!(reader.state().page, 1);
}
