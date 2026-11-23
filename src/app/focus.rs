//! The keyboard and the focus, which Blitz leaves to whoever owns the
//! window: which element gets the keys back after a click, what Enter, Space
//! and the arrows do on a focused control, and where a field's caret goes
//! when the field is reached. `shell.rs` and the harness both ask these, so
//! the tests see what the window does.

use super::*;

/// The element that wants the keyboard, as a selector.
///
/// **A click takes the focus away from the reader, and the reader cannot take
/// it back.** Blitz walks up from whatever was clicked looking for something
/// it knows how to focus, and a plain `<button>` is not on that list, so the
/// focus ends up on `<html>` — which is above anything a component can put a
/// handler on. Without the steps below, every shortcut would do nothing from
/// the first click onwards.
///
/// It cannot be answered from inside the reader either: `MountedData::set_focus`
/// takes `doc_mut()`, and every place a component could call it from is already
/// inside a borrow of the document — a task spawned from one included.
///
/// So the element that wants the keyboard says so and whoever owns the window
/// hands it back: `shell.rs` after a real click, the harness after a
/// synthesised one.
pub const KEYBOARD: &str = "[data-keyboard]";

/// Give the keyboard back to the element that asked for it, unless something
/// inside it has taken the focus.
///
/// Focus that lands *inside* the reader belongs to whatever took it — a field
/// in the find bar. Focus anywhere else is focus nobody wanted, which is what
/// a click on a button leaves behind.
pub fn give_keyboard_back(doc: &mut blitz_dom::BaseDocument) {
    // **The innermost element that asks for it wins**, which is why this is
    // `query_selector_all` and takes the last: the root always asks, and the
    // find bar's field is a descendant, so it comes later in document order.
    // Otherwise a click on "Match case" hands the keyboard to the root and the
    // next thing typed scrolls the document.
    let Ok(wants) = doc.query_selector_all(KEYBOARD) else {
        return;
    };
    let Some(wants) = wants.last().copied() else {
        return;
    };
    if let Some(focused) = doc.get_focussed_node_id() {
        let mut node = Some(focused);
        while let Some(id) = node {
            if id == wants {
                return;
            }
            node = doc.get_node(id).and_then(|node| node.parent);
        }
    }
    doc.set_focus_to(wants);
}

/// [`give_keyboard_back`], answering whether the focus moved.
///
/// A key or a click that removes the focused field is followed by
/// `give_keyboard_back`, but news can remove a field too: a recompile that
/// takes away a comment's passage takes the comment field with it. Without
/// this, the next key would go nowhere. The shell asks on every event and the
/// harness in `settle`. A removed field leaves the focus on `html` or on
/// nothing.
pub fn keyboard_if_lost(doc: &mut blitz_dom::BaseDocument) -> bool {
    // Inside an element that asks for the keyboard, it is not lost. That is
    // a walk up from one node, where the search below reads the whole page.
    let asks = blitz_dom::LocalName::from("data-keyboard");
    let mut node = doc.get_focussed_node_id();
    while let Some(id) = node {
        let Some(here) = doc.get_node(id) else { break };
        if here
            .element_data()
            .is_some_and(|el| el.has_attr(asks.clone()))
        {
            return false;
        }
        node = here.parent;
    }
    let before = doc.get_focussed_node_id();
    give_keyboard_back(doc);
    doc.get_focussed_node_id() != before
}

/// A key that the controls answer before the keymap hears it, pressed with
/// no modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKey {
    Enter,
    Space,
    Up,
    Down,
}

/// What [`keys_on_controls`] did with a key.
pub enum OnControls {
    /// Nothing. The key goes on to the document as usual.
    Pass,
    /// The key moved the focus within an open menu, and goes no further.
    Moved,
    /// Enter or Space on a button that has the focus. The caller sends these
    /// two events, a press and a release over the middle of the button, so
    /// that the key does exactly what a click there does, and then calls
    /// [`after_press`].
    Press(blitz_dom::NodeId, Box<[blitz_traits::events::UiEvent; 2]>),
}

/// **Tab moves the focus from button to button, and these keys act on it.**
/// Blitz moves the focus on Tab but does nothing with Enter or Space on a
/// button, and gives the menus no arrow keys. The shell and the harness ask
/// this before they hand a key to the document.
pub fn keys_on_controls(doc: &mut blitz_dom::BaseDocument, key: ControlKey) -> OnControls {
    use blitz_traits::events::{MouseEventButtons, UiEvent};
    let focused = doc.get_focussed_node_id();
    let is = |doc: &blitz_dom::BaseDocument, id: blitz_dom::NodeId, tag: &str| {
        doc.get_node(id)
            .and_then(|node| node.element_data())
            .is_some_and(|el| &*el.name.local == tag)
    };
    let enabled = |doc: &blitz_dom::BaseDocument, id: blitz_dom::NodeId| {
        doc.get_node(id)
            .and_then(|node| node.attr(blitz_dom::local_name!("disabled")))
            .is_none()
    };
    let inside =
        |doc: &blitz_dom::BaseDocument, id: blitz_dom::NodeId, outer: blitz_dom::NodeId| {
            let mut node = Some(id);
            while let Some(at) = node {
                if at == outer {
                    return true;
                }
                node = doc.get_node(at).and_then(|node| node.parent);
            }
            false
        };
    match key {
        ControlKey::Enter | ControlKey::Space => {
            let Some(button) = focused.filter(|&id| is(doc, id, "button") && enabled(doc, id))
            else {
                return OnControls::Pass;
            };
            let Some(node) = doc.get_node(button) else {
                return OnControls::Pass;
            };
            let up = node.synthetic_click_event_data(Default::default());
            // Only if the button is what a click at its middle would land on:
            // anything over it would take the click instead.
            let landing = doc.hit(up.page_x(), up.page_y()).map(|hit| hit.node_id);
            if !landing.is_some_and(|hit| inside(doc, hit, button)) {
                return OnControls::Pass;
            }
            let down = blitz_traits::events::BlitzPointerEvent {
                buttons: MouseEventButtons::Primary,
                ..up.clone()
            };
            OnControls::Press(
                button,
                Box::new([UiEvent::PointerDown(down), UiEvent::PointerUp(up)]),
            )
        }
        ControlKey::Up | ControlKey::Down => {
            // Not a highlight's colours: that popover stays up while the
            // page scrolls, and the arrows go on scrolling it.
            let Ok(menus) = doc.query_selector_all(".menu:not(.mark-popover)") else {
                return OnControls::Pass;
            };
            // The menu that has the focus, or else the one opened last.
            let menu = menus
                .iter()
                .copied()
                .find(|&menu| focused.is_some_and(|id| inside(doc, id, menu)))
                .or_else(|| menus.last().copied());
            let Some(menu) = menu else {
                return OnControls::Pass;
            };
            // A field in a menu keeps its own arrow keys.
            if focused.is_some_and(|id| is(doc, id, "input") || is(doc, id, "textarea")) {
                return OnControls::Pass;
            }
            let Ok(buttons) = doc.query_selector_all("button") else {
                return OnControls::Pass;
            };
            let items: Vec<_> = buttons
                .into_iter()
                .filter(|&id| inside(doc, id, menu) && enabled(doc, id))
                .collect();
            if items.is_empty() {
                return OnControls::Pass;
            }
            let at = focused.and_then(|id| items.iter().position(|&item| item == id));
            let last = items.len() - 1;
            let next = match (key, at) {
                (ControlKey::Down, Some(at)) if at < last => at + 1,
                (ControlKey::Down, _) => 0,
                (_, Some(at)) if at > 0 => at - 1,
                _ => last,
            };
            doc.set_focus_to(items[next]);
            in_view(doc, scroller_of(doc, menu, items[next]), items[next]);
            OnControls::Moved
        }
    }
}

/// The box that scrolls a row of a menu: the theme menu's list for a theme,
/// and otherwise the menu itself.
fn scroller_of(
    doc: &blitz_dom::BaseDocument,
    menu: blitz_dom::NodeId,
    row: blitz_dom::NodeId,
) -> blitz_dom::NodeId {
    let mut at = doc.get_node(row).and_then(|node| node.parent);
    while let Some(id) = at.filter(|&id| id != menu) {
        let node = doc.get_node(id);
        if node.is_some_and(|node| {
            node.attr(blitz_dom::LocalName::from("class"))
                .is_some_and(|class| class.split(' ').any(|word| word == "theme-list"))
        }) {
            return id;
        }
        at = node.and_then(|node| node.parent);
    }
    menu
}

/// Scroll a menu taller than the window so that its focused row can be seen.
/// Enter presses a row by clicking its middle, which misses a row that is
/// out of sight.
fn in_view(doc: &mut blitz_dom::BaseDocument, menu: blitz_dom::NodeId, row: blitz_dom::NodeId) {
    let (Some(outer), Some(inner)) = (
        doc.get_client_bounding_rect(menu),
        doc.get_client_bounding_rect(row),
    ) else {
        return;
    };
    let Some(node) = doc.get_node(menu) else {
        return;
    };
    // Blitz places a scrolled box as if its own scrolling moved it too.
    let top = outer.y + node.scroll_offset().y + node.final_layout().border.top as f64;
    let bottom =
        outer.y + node.scroll_offset().y + outer.height - node.final_layout().border.bottom as f64;
    let by = if inner.y < top {
        inner.y - top
    } else if inner.y + inner.height > bottom {
        inner.y + inner.height - bottom
    } else {
        return;
    };
    doc.scroll_by(menu, 0.0, by, blitz_dom::ScrollBehavior::Instant);
}

/// After a button was pressed from the keyboard: the focus goes back to it, so
/// that Tab carries on from there. Blitz moves the focus off a button that is
/// clicked. A button the press took away leaves the focus to
/// [`give_keyboard_back`].
pub fn after_press(doc: &mut blitz_dom::BaseDocument, button: blitz_dom::NodeId) {
    let tag = |doc: &blitz_dom::BaseDocument, id: blitz_dom::NodeId| {
        doc.get_node(id)
            .and_then(|node| node.element_data())
            .map(|el| el.name.local.to_string())
    };
    let still_there = tag(doc, button).as_deref() == Some("button");
    // Unless a field has it: a press that opens a field gives the field the
    // keyboard.
    let in_field = doc
        .get_focussed_node_id()
        .and_then(|id| tag(doc, id))
        .is_some_and(|tag| tag == "input" || tag == "textarea");
    if still_there && !in_field {
        doc.set_focus_to(button);
    }
}

/// A field that wants its caret at the end, once: `data-caret="end"`; or its
/// whole contents selected, once: `data-caret="all …"`, which the find field
/// asks for on every ⌘F with a new number after the word.
///
/// The find field comes back holding the query it went down with, and the
/// editor Blitz builds for it puts the caret at the front, so typing would
/// land in front of the old query. A component has no door to the editor — it is
/// built by the layout *after* the mount — so this is answered where
/// [`give_keyboard_back`] is: by the shell after every event, the redraw that
/// laid the field out included, and by the harness in `settle`. The attribute
/// comes off once the caret has moved, so nothing the reader does with it
/// afterwards is undone. Dioxus never rewrites an attribute whose value has
/// not changed, which is what lets the DOM take it away.
pub const CARET: &str = "[data-caret]";

/// Move every asking caret to the end of its field. `true` when one moved,
/// which is a frame to draw.
pub fn place_carets(doc: &mut blitz_dom::BaseDocument) -> bool {
    let Ok(wants) = doc.query_selector_all(CARET) else {
        return false;
    };
    let mut moved = false;
    for id in wants {
        let all = doc
            .get_node(id)
            .and_then(|node| node.attr(blitz_dom::LocalName::from("data-caret")))
            .is_some_and(|asked| asked.starts_with("all"));
        let mut placed = false;
        doc.with_text_input(id, |mut driver| {
            if all {
                driver.select_all();
            } else {
                driver.move_to_text_end();
            }
            placed = true;
        });
        if !placed {
            continue;
        }
        if let Some(element) = doc
            .get_node_mut(id)
            .and_then(|node| node.element_data_mut())
        {
            let name = element
                .attrs
                .iter()
                .find(|attr| &*attr.name.local == "data-caret")
                .map(|attr| attr.name.clone());
            if let Some(name) = name {
                element.attrs.remove(&name);
            }
        }
        moved = true;
    }
    moved
}

/// A field a click moved the focus into has everything in it selected, so
/// that typing replaces it — the number in a stepper, the page, a colour.
/// Asked when the button comes up rather than when it goes down, because a
/// press that slides a pixel is a drag to Blitz and would undo it; and left
/// alone when that drag selected something of its own. A second click, into
/// a field that already has the focus, puts the caret where it landed.
pub fn select_on_arrival(
    doc: &mut blitz_dom::BaseDocument,
    before: Option<blitz_dom::NodeId>,
) -> bool {
    let Some(id) = doc.get_focussed_node_id().filter(|&id| Some(id) != before) else {
        return false;
    };
    let mut selected = false;
    doc.with_text_input(id, |mut driver| {
        if driver.editor.raw_selection().is_collapsed() {
            driver.select_all();
            selected = true;
        }
    });
    selected
}

/// A field the keyboard moved the focus into — Tab — puts its caret after
/// what is in it. A click selects it instead: [`select_on_arrival`].
/// `before` is whatever had the focus before the key, so a key typed into a
/// field that already had it moves nothing. `true` when a
/// caret moved, which is a frame to draw.
pub fn caret_on_arrival(
    doc: &mut blitz_dom::BaseDocument,
    before: Option<blitz_dom::NodeId>,
) -> bool {
    let Some(id) = doc.get_focussed_node_id().filter(|&id| Some(id) != before) else {
        return false;
    };
    let mut moved = false;
    doc.with_text_input(id, |mut driver| {
        driver.move_to_text_end();
        moved = true;
    });
    moved
}

/// The reader's own root element, kept from the moment it mounts so that
/// anything inside the window can hand the keyboard back to it.
///
/// **A component cannot take the focus for itself**: `set_focus_to` panics
/// from inside the document borrow every handler is already holding, which is
/// the whole of why [`KEYBOARD`] exists and why the root asks for the focus in
/// its own `onmounted` rather than anywhere else. So it keeps the handle it
/// asked with, and a field leaving on Escape spawns the same two lines against
/// it.
///
/// Without this, Escape in a field could not mean "out of this field" without
/// also meaning "out of this window": the key would reach the root's handler
/// and close Settings from under somebody correcting six hexadecimal digits.
pub type RootFocus = Signal<Option<std::rc::Rc<MountedData>>>;

/// A number typed into a stepper that applies only the finished number, and
/// not yet confirmed: what confirms it.
///
/// A press anywhere else confirms it before that press does anything, as a
/// browser does. Blitz moves the focus after a click's handlers have run, so
/// the field's blur comes too late: a typed zoom followed by a click on "Fit
/// width" would undo the click. The root's `onpointerdown` calls this, since
/// nothing in the reader stops a `pointerdown`. See `prefs::Stepper`.
pub type Unconfirmed = Signal<Option<Callback<()>>>;

/// Hand the keyboard back to the reader's root. See [`RootFocus`].
pub fn leave_field(root: RootFocus) {
    let Some(node) = root.peek().clone() else {
        return;
    };
    let task = node.set_focus(true);
    spawn(async move {
        let _ = task.await;
    });
}

/// What a field does with a key that is not its own Enter or Escape. Typing
/// and the field's editing chords stay in it — see
/// [`crate::keymap::edits_a_field`]: ⌘Z undoes the typing, never a
/// highlight — and every other chord goes on to the keymap alone, so ⌘+
/// still zooms while somebody types, and ⌘↑ goes to the top of the document
/// without also moving the caret.
pub fn field_keeps(event: &KeyboardEvent) {
    let modifiers = event.modifiers();
    if crate::keymap::edits_a_field(&event.key(), event.code(), modifiers)
        || crate::keymap::plain(modifiers)
    {
        event.stop_propagation();
    } else {
        event.prevent_default();
    }
}
