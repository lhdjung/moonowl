//! The three windows over the reader that ask or tell one thing: the
//! document's details, the Sign window, and the password prompt.
//!
//! Each is shown only while it is open, and reads the viewer itself.

use super::*;

/// The close button every one of these windows has in its bar: the quiet
/// cross, and the red one for while the pointer is over it. An icon's stroke
/// is an attribute, never the cascade (see [`Icon`]), so both crosses are
/// drawn and `:hover` picks one — `.icon.hot` in `styles.rs`. A flag set on
/// `mouseenter` misses the button that appears under a pointer that has not
/// moved.
fn shades(held: &Viewer) -> (String, String) {
    let wearing = held.palette();
    (
        crate::palette::hex(wearing.muted()),
        crate::palette::hex(wearing.negative()),
    )
}

#[component]
pub(super) fn DetailsWindow(viewer: Signal<Viewer>) -> Element {
    let held = viewer.read();
    let (ink, danger) = shades(&held);
    let shelf_name = held.store.title().to_string();
    let details_rows = held.details();
    drop(held);
    rsx! {
        div {
            class: "window-scrim",
            onmousedown: move |event| {
                event.stop_propagation();
                viewer.write().close_details();
            },
            div {
                class: "window details-window",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "Information",
                onmousedown: move |event| {
                    event.stop_propagation();
                    window_menu(viewer, &event);
                },
                div { class: "window-bar",
                    span { class: "window-title", "Information" }
                    button {
                        class: "chip window-close",
                        "aria-label": "Close",
                        onclick: move |_| { viewer.write().close_details(); },
                        Icon { name: "close", stroke: ink.clone(), class: "rest" }
                        Icon { name: "close", stroke: danger.clone(), class: "hot" }
                    }
                }
                // `.window-pane`, not `.note-body`: this is rows that
                // can outrun the window, where a note is a paragraph
                // that cannot. The padding and the scroll come with the
                // class.
                div { class: "window-pane details-body",
                    h2 { class: "pane-title details-name", "{shelf_name}" }
                    for (label, value) in details_rows {
                        div { class: "details-row", key: "{label}",
                            span { class: "details-label", "{label}" }
                            span { class: "details-value", "{value}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub(super) fn SignWindow(viewer: Signal<Viewer>) -> Element {
    let held = viewer.read();
    let Some(pad) = held.signing.clone() else {
        return rsx! {};
    };
    let (ink, danger) = shades(&held);
    // A signature, drawn as it will look on the page: `sign::INK`, the navy
    // that goes into the file, where the theme leaves pages alone, and the
    // theme's own ink where it recolours them — on a dark theme's pad the
    // navy could hardly be seen.
    let wearing = held.palette();
    let pen = if wearing.recolor {
        crate::palette::hex(wearing.text)
    } else {
        crate::sign::INK.to_string()
    };
    let arming = held.arming.clone();
    drop(held);
    // The three lists, read when the window opened, not here, because one of
    // them opens the file. See `Signing::kept`.
    let (kept, signed_here, seals) = (pad.kept.clone(), pad.signed_here.clone(), pad.seals.clone());
    // Asked before the list is consumed by the rows below it.
    let nothing_kept = kept.is_empty();
    rsx! {
        div {
            class: "window-scrim",
            onmousedown: move |event| {
                event.stop_propagation();
                viewer.write().close_signing_unless_drawn();
            },
            div {
                class: "window sign-window",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "Sign this document",
                onmousedown: move |event| {
                    event.stop_propagation();
                    window_menu(viewer, &event);
                },
                div { class: "window-bar",
                    span { class: "window-title", "Sign this document" }
                    button {
                        class: "chip window-close",
                        "aria-label": "Close",
                        onclick: move |_| { viewer.write().close_signing(); },
                        Icon { name: "close", stroke: ink.clone(), class: "rest" }
                        Icon { name: "close", stroke: danger.clone(), class: "hot" }
                    }
                }
                div { class: "sign-body",
                    // **The sentence that keeps this honest**, and it
                    // is the first thing in the window rather than a
                    // footnote under it. A reader who wanted the other
                    // kind of signing should find that out before they
                    // have drawn anything.
                    p { class: "pane-lede",
                        "Ink on the page, the way a pen is. It is written into the document and any reader can see it — and it is not a digital signature: it proves nothing about who signed or whether the file has changed since."
                    }
                    // **What the document is already signed with, in
                    // the other sense of the word.** First, because a
                    // reader who came here wanting the green tick should
                    // meet the document's own signatures before the
                    // pad.
                    if !seals.is_empty() {
                        h3 { class: "pane-group", "Digital signatures" }
                        div { class: "sign-list",
                            for (nth, seal) in seals.iter().enumerate() {
                                div { key: "{nth}", class: "sign-row sign-seal",
                                    span { class: "sign-placed",
                                        span { class: "sign-name",
                                            {if seal.filled { "Signed" } else { "Signature field" }}
                                        }
                                        span { class: "sign-where", "{seal.says()}" }
                                    }
                                }
                            }
                        }
                    }
                    // **What is already on this document**, first,
                    // because a reader who opens this window on a
                    // document they have signed is at least as likely
                    // to be here to take one off as to add another.
                    if !signed_here.is_empty() {
                        h3 { class: "pane-group", "On this document" }
                        div { class: "sign-list",
                            for placed in signed_here {
                                div {
                                    key: "{placed.page}-{placed.index}",
                                    class: "sign-row",
                                    span { class: "sign-placed",
                                        span { class: "sign-name",
                                            // The words for a line of
                                            // type, the name for a
                                            // hand — and for either
                                            // one this reader did not
                                            // write, what it is.
                                            {if !placed.by.is_empty() {
                                                placed.by.clone()
                                            } else if placed.kind == crate::sign::Written::Hand {
                                                "Ink".to_string()
                                            } else {
                                                "A stamp".to_string()
                                            }}
                                        }
                                        span { class: "sign-where", {format!("page {}", viewer.read().label(placed.page))} }
                                    }
                                    if arming == Some(crate::app::Arming::Placed(placed.page, placed.index)) {
                                        button {
                                            class: "sign-forget armed",
                                            onclick: {
                                                let (page, index) = (placed.page, placed.index);
                                                let kind = placed.kind;
                                                move |_| {
                                                    viewer.write().arming = None;
                                                    viewer.write().unsign(page, index, kind);
                                                }
                                            },
                                            "Take off"
                                        }
                                    } else {
                                        button {
                                            class: "sign-forget",
                                            "aria-label": "Take this off the document",
                                            onclick: {
                                                let (page, index) = (placed.page, placed.index);
                                                move |_| { viewer.write().arm(crate::app::Arming::Placed(page, index)); }
                                            },
                                            Icon { name: "trash", stroke: ink.clone() }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if !nothing_kept {
                        h3 { class: "pane-group", "Kept" }
                        div { class: "sign-list",
                            for entry in kept {
                                div { key: "{entry.id}", class: "sign-row",
                                    button {
                                        class: "sign-use",
                                        onclick: {
                                            let entry = entry.clone();
                                            move |_| viewer.write().sign_with(entry.clone())
                                        },
                                        // The signature itself, drawn
                                        // from the strokes on disk, by
                                        // the same arithmetic
                                        // `sign::place` uses onto a
                                        // page.
                                        Scrawl { signature: entry.clone(), width: 132.0, height: 44.0, ink: pen.clone() }
                                        span { class: "sign-name", "{entry.name}" }
                                    }
                                    if arming == Some(crate::app::Arming::Kept(entry.id.clone())) {
                                        button {
                                            class: "sign-forget armed",
                                            onclick: {
                                                let id = entry.id.clone();
                                                move |_| {
                                                    viewer.write().arming = None;
                                                    viewer.write().forget_signature(&id);
                                                }
                                            },
                                            "Delete"
                                        }
                                    } else {
                                        button {
                                            class: "sign-forget",
                                            "aria-label": "Delete this signature",
                                            onclick: {
                                                let id = entry.id.clone();
                                                move |_| { viewer.write().arm(crate::app::Arming::Kept(id.clone())); }
                                            },
                                            Icon { name: "trash", stroke: ink.clone() }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    h3 { class: "pane-group",
                        {if nothing_kept { "Draw one" } else { "Or draw another" }}
                    }
                    // **The pad.** A press begins a stroke; the moves
                    // and the release are heard on the root, because a
                    // hand signing a name leaves the box it started in
                    // more often than not.
                    div {
                        class: "sign-pad",
                        // **The size is written here and nowhere
                        // else.** The stylesheet is a `const &str` and
                        // cannot interpolate, so a pad sized in CSS and
                        // read in Rust would be two numbers that have to
                        // agree — and the handler is wrong by exactly
                        // their difference.
                        style: "width: {PAD_WIDTH}px; height: {PAD_HEIGHT}px;",
                        onmousedown: move |event| {
                            event.stop_propagation();
                            let on = event.element_coordinates();
                            let client = event.client_coordinates();
                            viewer.write().draw_from(
                                (on.x, on.y),
                                (client.x, client.y),
                                (PAD_WIDTH, PAD_HEIGHT),
                            );
                        },
                        Scrawl {
                            ink: pen.clone(),
                            signature: crate::sign::Signature {
                                name: String::new(),
                                id: String::new(),
                                // In the pad's own pixels, which is
                                // what `literal` below draws in.
                                strokes: pad.strokes.clone(),
                            },
                            width: PAD_WIDTH,
                            height: PAD_HEIGHT,
                            // The pad draws what was drawn, at the place
                            // it was drawn — not stretched to its own
                            // extent, which is what the saved ones are
                            // shown at and would make the ink jump
                            // under the hand drawing it.
                            literal: true,
                        }
                        if pad.strokes.iter().all(|stroke| stroke.is_empty()) {
                            span { class: "sign-pad-hint", "Draw your name here" }
                        }
                    }
                    crate::prefs::Field { label: "Name",
                        crate::prefs::TextField {
                            value: pad.name.clone(),
                            onchange: move |value| {
                                if let Some(signing) = viewer.write().signing.as_mut() {
                                    signing.name = value;
                                }
                            },
                        }
                    }
                    div { class: "pane-actions",
                        button {
                            class: "chip action",
                            onclick: move |_| viewer.write().clear_pad(),
                            "Clear"
                        }
                        button {
                            class: "chip action primary",
                            onclick: move |_| { viewer.write().keep_signature(); },
                            "Keep this signature"
                        }
                    }
                    // **The other half of signing something**: a date,
                    // a place, a printed version of what was just drawn.
                    // Here rather than in a window of its own because it
                    // is the same errand, and last because the signature
                    // is the thing being asked for.
                    h3 { class: "pane-group", "Or a date, or a line of text" }
                    crate::prefs::Field { label: "Text",
                        crate::prefs::TextField {
                            value: pad.line.clone(),
                            onchange: move |value| {
                                if let Some(signing) = viewer.write().signing.as_mut() {
                                    signing.line = value;
                                }
                            },
                        }
                    }
                    div { class: "pane-actions",
                        button {
                            class: "chip action sign-today",
                            // Fills the field rather than placing
                            // anything, so a reader who wants a
                            // different date can edit it and one who
                            // wants today's is one press from it.
                            onclick: move |_| {
                                if let Some(signing) = viewer.write().signing.as_mut() {
                                    signing.line = crate::sign::today();
                                }
                            },
                            "Today"
                        }
                        button {
                            class: "chip action primary sign-place-text",
                            onclick: {
                                let line = pad.line.clone();
                                move |_| viewer.write().type_with(line.clone())
                            },
                            "Place this on the page"
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub(super) fn PasswordWindow(viewer: Signal<Viewer>, frame: Frame) -> Element {
    let held = viewer.read();
    let Some(asking) = held.locked.clone() else {
        return rsx! {};
    };
    let (ink, danger) = shades(&held);
    drop(held);
    // The bullets the field shows, counted here because a format hole in
    // `rsx!` cannot hold a string literal of its own. See the field.
    let locked_shown = "\u{2022}".repeat(asking.typed.chars().count());
    rsx! {
        div {
            class: "window-scrim",
            // A press outside is not "not now": the app's own window
            // has no light dismiss either, and a question with a field
            // in it is not something to lose by clicking beside it.
            onmousedown: move |event| event.stop_propagation(),
            div {
                class: "window ask-window",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "This document is locked",
                onmousedown: move |event| {
                    event.stop_propagation();
                    window_menu(viewer, &event);
                },
                div { class: "window-bar",
                    span { class: "window-title", "This document is locked" }
                    button {
                        class: "chip window-close",
                        "aria-label": "Close",
                        onclick: move |_| { viewer.write().stop_unlocking(); },
                        Icon { name: "close", stroke: ink.clone(), class: "rest" }
                        Icon { name: "close", stroke: danger.clone(), class: "hot" }
                    }
                }
                div { class: "ask-body",
                    p { class: "pane-lede",
                        {if asking.wrong {
                            "That password was not right. Try again."
                        } else {
                            "It needs a password before it can be opened."
                        }}
                    }
                    // **What is on screen is bullets and what is in the
                    // field is the password.** Blitz reads
                    // `type="password"`, builds a text editor and gives
                    // it the right accessibility role — and does not
                    // *mask* it, so a field left to itself shows
                    // somebody's password to the room.
                    //
                    // So the ink is taken away (`color: transparent`
                    // with its own `caret-color`) and a span above it
                    // holds one bullet a character. The attribute stays
                    // `password`, so the role is kept and the day Blitz
                    // masks it this is a bullet under a bullet rather
                    // than a fault.
                    //
                    // **The other way round was tried and is the one to
                    // know about.** Bullets in the field's own `value`
                    // works until a key is pressed twice: `set_text`
                    // touches the editor only when the string differs,
                    // and setting it collapses the selection to the
                    // front — so the caret is thrown to offset 0 after
                    // every keystroke and "moonowl" is typed in as "lwonoom".
                    // Backspace cannot be intercepted to work around it
                    // either: on macOS it is a `doCommandBySelector:`
                    // the editor answers directly.
                    //
                    // The cost is that the password is in the DOM while
                    // the question is up, which is where a browser keeps
                    // it too.
                    span { class: "ask-field-wrap",
                        input {
                            class: "text-field ask-field",
                            r#type: "password",
                            value: "{asking.typed}",
                            "aria-label": "Password",
                            "data-keyboard": "password",
                            onmounted: move |event| {
                                let node = event.data();
                                let task = node.set_focus(true);
                                spawn(async move { let _ = task.await; });
                            },
                            oninput: move |event| {
                                viewer.write().type_password(&event.value());
                            },
                            // [`field_keeps`], as every field has — a
                            // plain key would otherwise scroll the
                            // document behind the window — plus the
                            // two keys this one is for.
                            onkeydown: {
                                let frame = frame.clone();
                                move |event: KeyboardEvent| {
                                    match event.key() {
                                        Key::Enter => {
                                            event.stop_propagation();
                                            event.prevent_default();
                                            if viewer.write().unlock() {
                                                let path = viewer
                                                    .read()
                                                    .document
                                                    .path()
                                                    .to_string();
                                                let title = viewer
                                                    .read()
                                                    .store
                                                    .title()
                                                    .to_string();
                                                frame.ask(Ask::Showing { path, title });
                                            }
                                        }
                                        Key::Escape => {
                                            event.stop_propagation();
                                            viewer.write().stop_unlocking();
                                        }
                                        _ => field_keeps(&event),
                                    }
                                }
                            },
                        }
                        span { class: "ask-bullets", "{locked_shown}" }
                    }
                    div { class: "pane-actions ask-actions",
                        button {
                            class: "chip action",
                            "data-item": "not-now",
                            onclick: move |_| { viewer.write().stop_unlocking(); },
                            "Not now"
                        }
                        button {
                            class: "chip action primary",
                            "data-item": "unlock",
                            onclick: {
                                let frame = frame.clone();
                                move |_| {
                                    if viewer.write().unlock() {
                                        let path =
                                            viewer.read().document.path().to_string();
                                        let title =
                                            viewer.read().store.title().to_string();
                                        frame.ask(Ask::Showing { path, title });
                                    }
                                }
                            },
                            "Open"
                        }
                    }
                }
            }
        }
    }
}
