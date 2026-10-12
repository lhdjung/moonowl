//! The keyboard on the controls: Tab moves the focus from button to button,
//! Enter or Space presses the one it is on, and the arrows move through an
//! open menu. See `app::keys_on_controls`.

use moonowl::harness::{Options, Reader};

fn reader() -> Reader {
    Reader::open_with(&Reader::book(), Options::default())
}

/// Tab until the focus is on `selector`, as somebody without a mouse would.
fn tab_to(reader: &mut Reader, selector: &str) {
    let target = reader.harness.query(selector).expect("on screen");
    for _ in 0..60 {
        if reader.focused() == Some(target) {
            return;
        }
        reader.press("Tab");
    }
    panic!("Tab never reached {selector}");
}

/// The button with the focus is outlined. Blitz never matches
/// `:focus-visible`, and an outline asked for there was never drawn.
#[test]
fn the_focused_button_is_outlined() {
    let mut reader = reader();
    let (x, y, _, height) = reader.box_of(".chip.open").expect("on screen");
    let beside = |reader: &mut Reader| {
        let shot = reader.screenshot();
        let pixel = shot.at((x - 2.0) as u32, (y + height / 2.0) as u32);
        [pixel[0], pixel[1], pixel[2]]
    };
    let plain = beside(&mut reader);
    tab_to(&mut reader, ".chip.open");
    assert_ne!(beside(&mut reader), plain, "an outline is drawn round it");
}

#[test]
fn enter_and_space_press_the_button_that_has_the_focus() {
    let mut reader = reader();
    tab_to(&mut reader, ".chip.settings");
    reader.press("Enter");
    assert_eq!(reader.state().menu.as_deref(), Some("settings"));
    assert_eq!(
        reader.focused(),
        reader.harness.query(".chip.settings"),
        "and the focus stays on the button, so Tab carries on from there",
    );
    reader.press(" ");
    assert_eq!(reader.state().menu, None, "Space presses it too");
}

#[test]
fn the_arrows_move_through_a_menu_and_enter_chooses() {
    let mut reader = reader();
    reader.click(".chip.theme");
    let enabled: Vec<_> = reader
        .harness
        .query_all(".menu.theme button")
        .into_iter()
        .zip(reader.attribute_all(".menu.theme button", "disabled"))
        .filter(|(_, disabled)| disabled.is_empty())
        .map(|(node, _)| node)
        .collect();
    let want = reader.harness.query_all(".menu.theme .menu-item")[2];
    let steps = enabled
        .iter()
        .position(|&node| node == want)
        .expect("in the menu")
        + 1;
    for _ in 0..steps {
        reader.press("ArrowDown");
    }
    assert_eq!(reader.focused(), Some(want));
    reader.press("Enter");
    assert_eq!(reader.state().theme, "Professional");

    // Up from the first row goes round to the last.
    for _ in 0..steps - 1 {
        reader.press("ArrowUp");
    }
    assert_eq!(reader.focused(), enabled.first().copied());
    reader.press("ArrowUp");
    assert_eq!(reader.focused(), enabled.last().copied());
}

/// With no menu open, the arrows are the document's, as they always were.
#[test]
fn without_a_menu_the_arrows_scroll() {
    let mut reader = reader();
    let top = reader.state().scroll;
    reader.press("ArrowDown");
    assert!(reader.state().scroll > top);
}
