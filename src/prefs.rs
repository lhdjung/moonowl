//! The Settings window: five pages in a frame over the reader.
//!
//! This is `settings.ts` and the `showWindow` half of `ui.ts`, and the shape
//! is the app's own: a scrim and a frame in the same document rather than a
//! window of the system's. It matters more here than there. A second winit
//! window would be a second [`Viewer`] over a second `Store`, and every
//! setting changed in it would reach the reader on its next launch —
//! `AGENTS.md` describes exactly that staleness between two reader windows,
//! and it is tolerable between two documents and not between a switch and the
//! thing the switch is about.
//!
//! What is not here, and is not pretended: the theme editor, which is a page
//! of its own. `follow_system_theme` used to be beside it for want of a
//! signal; the signal is `WindowEvent::ThemeChanged` and the switch is on the
//! Appearance page now — see [`crate::app::Appearance`].

use dioxus::prelude::*;

use crate::app::{Icon, Pane, Viewer};
use crate::keymap::{self, Action};
use crate::layout::{Fit, Mode, Spread};

/// The whole window, or nothing at all.
#[component]
pub fn Settings(viewer: Signal<Viewer>, frame: crate::app::Frame) -> Element {
    let held = viewer.read();
    let Some(pane) = held.pane else {
        return rsx! {};
    };
    let wearing = held.palette();
    let (ink, ink_on) = (
        crate::palette::hex(wearing.muted()),
        crate::palette::hex(wearing.accent),
    );
    drop(held);

    rsx! {
        // The scrim takes the press, which is how clicking beside the window
        // closes it — and the frame stops it, which is how clicking inside
        // does not. Blitz has no `position: fixed`, so this is absolute
        // against the root; see `.window-scrim` in `styles.rs`.
        div {
            class: "window-scrim",
            onmousedown: move |event| {
                event.stop_propagation();
                viewer.write().close_settings();
            },
            div {
                class: "window",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "Settings",
                // See the highlight colours window: a press anywhere a picker
                // is not puts the picker away.
                onmousedown: move |event| {
                    event.stop_propagation();
                    viewer.write().close_picker();
                },
                div { class: "window-bar",
                    span { class: "window-title", "Settings" }
                    button {
                        class: "chip window-close",
                        "aria-label": "Close",
                        onclick: move |_| { viewer.write().close_settings(); },
                        Icon { name: "close", stroke: ink.clone() }
                    }
                }
                div { class: "window-body",
                    nav { class: "window-nav", "aria-label": "Settings",
                        for page in Pane::ALL {
                            button {
                                key: "{page.label()}",
                                class: if page == pane { "nav-item on" } else { "nav-item" },
                                onclick: move |_| viewer.write().show_pane(page),
                                Icon {
                                    name: page.icon(),
                                    stroke: if page == pane { ink_on.clone() } else { ink.clone() },
                                }
                                "{page.label()}"
                            }
                        }
                    }
                    // **A keyed list of one, so that changing the page changes
                    // the element.** The pane scrolls — Reading is taller than
                    // 600px and says so where `.window-pane` is styled — and a
                    // scroll offset belongs to the node, not to what is in it.
                    // With one node for all five pages, leaving a scrolled
                    // Reading for About kept the 248px it had been scrolled by
                    // and About, which is four short paragraphs, was entirely
                    // above the top of the box: the reader saw an empty page. A
                    // key here is the one way to say "a different node" — a key
                    // on a lone child is not diffed.
                    for page in [pane] {
                        div { key: "{page.label()}", class: "window-pane",
                            match page {
                                Pane::Reading => rsx! { Reading { viewer } },
                                Pane::Appearance => rsx! { Appearance { viewer } },
                                Pane::Window => rsx! { WindowPage { viewer, frame: frame.clone() } },
                                Pane::Keyboard => rsx! { Keyboard { viewer } },
                                Pane::About => rsx! { About { viewer } },
                            }
                        }
                    }
                }
            }
        }
    }
}

/* ------------------------------------------------------------ the pieces */

/// One setting: what it is called, the control, and the sentence under it.
///
/// `ui.field` in the app, and the note is the part worth keeping — every
/// switch in that window says what it is for in a sentence, which is most of
/// why the window reads as calm rather than as a form.
#[component]
pub(crate) fn Field(
    label: String,
    #[props(default)] note: Option<String>,
    children: Element,
) -> Element {
    rsx! {
        div { class: "field",
            div { class: "field-head",
                span { class: "field-label", "{label}" }
                div { class: "field-control", {children} }
            }
            if let Some(note) = note {
                p { class: "field-note", "{note}" }
            }
        }
    }
}

/// A switch. `role="switch"` and `aria-checked`, because it is a button that
/// answers a yes-or-no question and the shape of it says nothing.
#[component]
pub(crate) fn Toggle(on: bool, onchange: EventHandler<bool>) -> Element {
    rsx! {
        button {
            class: if on { "switch on" } else { "switch" },
            role: "switch",
            "aria-checked": if on { "true" } else { "false" },
            onclick: move |_| onchange.call(!on),
            span { class: "switch-knob" }
        }
    }
}

/// A row of choices, one of which is in force. `ui.segmented`.
#[component]
fn Segmented(
    options: Vec<(String, String)>,
    chosen: String,
    onchange: EventHandler<String>,
) -> Element {
    rsx! {
        div { class: "segmented",
            for (value, label) in options {
                button {
                    key: "{value}",
                    class: if value == chosen { "segment on" } else { "segment" },
                    "aria-pressed": if value == chosen { "true" } else { "false" },
                    onclick: {
                        let value = value.clone();
                        move |_| onchange.call(value.clone())
                    },
                    "{label}"
                }
            }
        }
    }
}

/// A number with a step either side of it, and the number can be typed.
///
/// **The unit is beside the field and not in it**, which the app learned the
/// hard way: a field reading "16 px" puts the caret wherever the pointer
/// landed, so typing 30 gives "3016 px" and a setting at its maximum. And the
/// box is the width of what is in it, because Blitz cannot centre an input's
/// text — see the comment on `.pill` in `app.rs`, which is the same finding
/// one window along.
#[component]
pub(crate) fn Stepper(
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    #[props(default)] unit: Option<String>,
    // How many places after the point the field shows and keeps; none unless
    // asked for, because a dragged sidebar is 237.4px and nobody wants to read it.
    #[props(default)] decimals: u32,
    onchange: EventHandler<f64>,
) -> Element {
    let root: crate::app::RootFocus = use_context();
    let scale = 10f64.powi(decimals as i32);
    let shown = format!("{}", (value * scale).round() / scale);
    // **What the field is showing, which is the number until somebody types
    // into it.** A typed number is clamped on the way out, so a field being
    // typed into can disagree with the setting for a keystroke or two — "9"
    // on its way to "90" in a stepper whose maximum is 64 — and echoing the
    // clamped number back would rewrite the editor's text under the caret.
    // Leaving the field puts the setting's own number back.
    let mut typed = use_signal(|| None::<String>);
    let showing = typed.read().clone().unwrap_or_else(|| shown.clone());
    let width = (14.0 + 8.5 * showing.chars().count() as f64).max(34.0);
    rsx! {
        div { class: "stepper",
            button {
                class: "chip step-down",
                "aria-label": "Less",
                onclick: move |_| {
                    typed.set(None);
                    onchange.call((value - step).clamp(min, max));
                },
                "−"
            }
            // **The caret goes where the pointer put it**, which Blitz does by
            // itself on the press, and to the end when the field is reached by
            // Tab — `app::caret_on_arrival`. It does not ask for the keyboard
            // (`data-keyboard`): the innermost element asking wins every event,
            // so a stepper asking took the focus from every other field on the
            // page and would not let go of it on a click elsewhere.
            input {
                class: "step-field",
                style: "width: {width}px;",
                r#type: "text",
                value: "{showing}",
                // A typed value is clamped to the range and never snapped to
                // the step: the step is how far one press moves, not a list
                // of the answers allowed. `ui.stepper` in the app says so.
                oninput: move |event| {
                    let text = event.value();
                    typed.set(Some(text.clone()));
                    // A decimal comma is read as a point, which is how half the
                    // readers of this app write a half.
                    if let Some(number) = text.trim().replace(',', ".").parse::<f64>().ok().filter(|n| !n.is_nan()) {
                        onchange.call(((number * scale).round() / scale).clamp(min, max));
                    }
                },
                onblur: move |_| typed.set(None),
                onkeydown: move |event: KeyboardEvent| {
                    if event.key() == Key::Escape {
                        typed.set(None);
                    }
                    typing_is_not_a_shortcut(&event, root);
                },
            }
            if let Some(unit) = unit {
                span { class: "step-unit", "{unit}" }
            }
            button {
                class: "chip step-up",
                "aria-label": "More",
                onclick: move |_| {
                    typed.set(None);
                    onchange.call((value + step).clamp(min, max));
                },
                "+"
            }
        }
    }
}

/// A heading inside a page, and a paragraph that is not about one setting.
#[component]
fn Note(text: String) -> Element {
    rsx! { p { class: "pane-note", "{text}" } }
}

/* ------------------------------------------------------------- the pages */

#[component]
fn Reading(viewer: Signal<Viewer>) -> Element {
    let held = viewer.read();
    let mode = held.layout.mode;
    let spread = held.layout.spread;
    let gap = held.layout.gap;
    let fit = held.layout.fit;
    let zoom = held.layout.zoom;
    let trimming = held.trims_margins();
    let (remember, reopen, pill, hide_cursor, offer) = (
        held.store.flag("remember_position"),
        held.store.flag("reopen_last_document"),
        held.store.flag("show_page_pill"),
        held.store.flag("hide_cursor"),
        held.store.flag("offer_highlight_on_select"),
    );
    let tabs = held.store.flag("open_in_tabs");
    let key_mark = held.chord_for(Action::Markup);
    let rest = held.store.number("hide_cursor_after");
    let printed = held.numbering_printed();
    drop(held);

    rsx! {
        h2 { class: "pane-title", "Reading" }
        Field {
            label: "Page progression",
            Segmented {
                options: vec![
                    ("continuous".into(), "Continuous (default)".into()),
                    ("paged".into(), "One page at a time".into()),
                ],
                chosen: if mode == Mode::Paged { "paged".to_string() } else { "continuous".to_string() },
                onchange: move |value: String| {
                    let mode = if value == "paged" { Mode::Paged } else { Mode::Continuous };
                    viewer.write().set_scroll_mode(mode);
                },
            }
        }
        Field {
            label: "Pages side by side",
            note: "Two pages across reads like a book. \u{201c}Two, cover alone\u{201d} shows page 1 alone but two pages afterwards.",
            Segmented {
                options: vec![
                    ("single".into(), "One (default)".into()),
                    ("two".into(), "Two".into()),
                    ("cover".into(), "Two, cover alone".into()),
                ],
                chosen: match spread {
                    Spread::Single => "single".to_string(),
                    Spread::Two => "two".to_string(),
                    Spread::Cover => "cover".to_string(),
                },
                onchange: move |value: String| {
                    let spread = match value.as_str() {
                        "two" => Spread::Two,
                        "cover" => Spread::Cover,
                        _ => Spread::Single,
                    };
                    viewer.write().set_spread(spread);
                },
            }
        }
        Field {
            label: "Space between pages",
            note: "How much room to leave between one page and the next.",
            Stepper {
                value: gap, min: 0.0, max: 64.0, step: 4.0, unit: "px",
                onchange: move |value| viewer.write().set_page_gap(value),
            }
        }
        Field {
            label: "Trim the margins",
            note: "Remove whitespace on the left and right.",
            Toggle { on: trimming, onchange: move |on| viewer.write().set_trim(on) }
        }
        Field {
            label: "Zoom",
            note: "Fit width follows the window; a fixed zoom stays where you put it.",
            Segmented {
                options: vec![
                    ("width".into(), "Fit width (default)".into()),
                    ("page".into(), "Fit page".into()),
                    ("actual".into(), "Fixed".into()),
                ],
                chosen: match fit {
                    Fit::Width => "width".to_string(),
                    Fit::Page => "page".to_string(),
                    Fit::Actual => "actual".to_string(),
                },
                onchange: move |value: String| {
                    match value.as_str() {
                        "page" => viewer.write().set_fit(Fit::Page),
                        "actual" => viewer.write().actual_size(),
                        _ => viewer.write().set_fit(Fit::Width),
                    }
                },
            }
        }
        // Only where there is a fixed zoom to set, which is what `zoomField`
        // hides itself for in the app.
        if fit == Fit::Actual {
            Field {
                label: "Fixed zoom",
                Stepper {
                    value: (zoom * 100.0).round(), min: 25.0, max: 600.0, step: 25.0, unit: "%",
                    onchange: move |value: f64| viewer.write().set_zoom(value / 100.0),
                }
            }
        }
        Field {
            label: "Come back to where I stopped",
            note: "Each document reopens on the page you left it on.",
            Toggle { on: remember, onchange: move |on| viewer.write().set_flag("remember_position", on) }
        }
        Field {
            label: "Open what I was reading",
            note: "Start on the document you were reading when you last quit. Closing a document yourself means you are done with it, and it is not reopened.",
            Toggle { on: reopen, onchange: move |on| viewer.write().set_flag("reopen_last_document", on) }
        }
        // macOS alone: nowhere else has tabs to open into.
        if cfg!(target_os = "macos") {
            Field {
                label: "Open documents in tabs",
                note: "Documents opened from outside of Moonowl appear as tabs. Off, they open in new windows.",
                Toggle { on: tabs, onchange: move |on| viewer.write().set_flag("open_in_tabs", on) }
            }
        }
        Field {
            label: "Page numbers",
            note: "\u{201c}As printed\u{201c} uses any page counts from the document itself, like 407 to 425 or i, ii, iii. \u{201c}Count from 1\u{201c} counts from 1 to the end.",
            Segmented {
                options: vec![
                    ("printed".into(), "As printed (default)".into()),
                    ("position".into(), "Count from 1".into()),
                ],
                chosen: if printed { "printed".to_string() } else { "position".to_string() },
                onchange: move |value: String| viewer.write().set_page_numbering(value == "printed"),
            }
        }
        Field {
            label: "Show page count while scrolling",
            note: "A brief \u{201c}page 23 of 197\u{201d} while you scroll with the toolbar hidden.",
            Toggle { on: pill, onchange: move |on| viewer.write().set_flag("show_page_pill", on) }
        }
        Field {
            label: "Offer highlight colours on selecting",
            note: format!("The colours appear as soon as you finish selecting text. Off, they wait to be asked for. {key_mark}"),
            Toggle { on: offer, onchange: move |on| viewer.write().set_flag("offer_highlight_on_select", on) }
        }
        Field {
            label: "Hide the pointer while you read",
            note: "The pointer goes away once it has sat still for a while, and comes back the moment you move it.",
            Toggle { on: hide_cursor, onchange: move |on| viewer.write().set_flag("hide_cursor", on) }
        }
        // Only where there is a wait to set, which is what the fixed zoom
        // above does and for the same reason: a number that does nothing is
        // a question the reader has to work out the answer to.
        if hide_cursor {
            Field {
                label: "Wait before hiding it",
                Stepper {
                    value: rest, min: 0.0, max: 30.0, step: 1.0, unit: "s", decimals: 1,
                    onchange: move |value: f64| viewer.write().set_cursor_rest(value),
                }
            }
        }
    }
}

#[component]
fn Appearance(viewer: Signal<Viewer>) -> Element {
    let post =
        use_hook(|| dioxus_core::try_consume_context::<crate::emit::Post>().unwrap_or_default());
    let held = viewer.read();
    let editing = held.editing.clone();
    let chosen = held.store.theme_index();
    let worn = held.store.theme().clone();
    let themes: Vec<(usize, String, [String; 3])> = held
        .store
        .themes()
        .iter()
        .enumerate()
        .map(|(index, theme)| {
            // Resolved rather than handed over raw. Nothing may show a
            // theme's colour without going through `parseColor` — a swatch
            // that shows a colour the renderer cannot read is the picker
            // lying about the page. See `AGENTS.md`.
            let colours = crate::palette::resolve(theme, true);
            // **A theme that leaves the document alone leaves its links alone
            // too**, so the card shows what such a page really looks like
            // rather than the theme's own three colours — `themeCard`'s own
            // branch and its own two fallbacks.
            let card = if colours.recolor {
                [
                    crate::palette::hex(colours.background),
                    crate::palette::hex(colours.text),
                    crate::palette::hex(colours.link),
                ]
            } else {
                [
                    "#ffffff".to_string(),
                    "#2f3237".to_string(),
                    "#1a5fb4".to_string(),
                ]
            };
            (index, theme.name.clone(), card)
        })
        .collect();
    let dark = held.store.dark_now();
    let following = held.store.flag("follow_system_theme");
    let recolor_images = held.store.flag("recolor_images");
    // What the machine says, which is the difference between a switch that
    // does something and a switch that cannot: a platform reporting nothing
    // has no light and dark to follow, and saying so is better than leaving
    // an inert switch on the page.
    let machine = held.store.outside();
    let folder = held.store.themes_dir().display().to_string();
    let key_dark = held.chord_for(Action::Dark);
    drop(held);

    rsx! {
        h2 { class: "pane-title", "Appearance" }
        // The three switches, in `appearancePage`'s own order.
        Field {
            label: "Follow the system",
            note: match machine {
                Some(_) => "Take the light theme when the machine is light and the dark one when it is dark. Choosing a theme that disagrees turns this off.".to_string(),
                None => "This machine does not report an appearance, so there is nothing to follow.".to_string(),
            },
            Toggle { on: following, onchange: move |on| viewer.write().set_follow_system(on) }
        }
        Field {
            label: "Dark mode",
            note: format!("Switches between the light theme and the dark theme you last chose. {key_dark}"),
            Toggle { on: dark, onchange: move |on| viewer.write().set_dark(on) }
        }
        Field {
            label: "Recolour pictures too",
            note: "On, pictures take the theme along with the rest of the page. Off, they stay exactly as printed.".to_string(),
            Toggle { on: recolor_images, onchange: move |on| viewer.write().set_recolor_images(on) }
        }
        if let Some(draft) = editing {
            ThemeEditor { viewer, draft }
        } else {
            h3 { class: "pane-group", "Themes" }
            div { class: "theme-grid",
                for (index, name, colours) in themes {
                    button {
                        key: "{index}",
                        class: if index == chosen { "theme-card on" } else { "theme-card" },
                        "aria-pressed": if index == chosen { "true" } else { "false" },
                        onclick: move |_| viewer.write().set_theme(index),
                        // **A page, not a palette.** Two bars of colour said
                        // what a theme was made of; the app's card says what
                        // reading under it looks like — a word of body text and
                        // a link, in the ink each would really be drawn in. The
                        // link is the half that was missing entirely: nothing
                        // in this window showed a theme's link colour, which is
                        // the one colour a reader picks a theme for and cannot
                        // otherwise see until they meet a link.
                        div {
                            class: "theme-swatch",
                            style: "background: {colours[0]}; color: {colours[1]};",
                            span { class: "swatch-body", "Aa" }
                            span { class: "swatch-link", style: "color: {colours[2]};", "Link" }
                        }
                        span { class: "theme-name", "{name}" }
                    }
                }
            }
            div { class: "pane-actions",
                button {
                    class: "chip action",
                    onclick: move |_| viewer.write().begin_theme(None),
                    "New theme…"
                }
                button {
                    class: "chip action",
                    onclick: {
                        let worn = worn.clone();
                        move |_| viewer.write().begin_theme(Some(worn.clone()))
                    },
                    // A built-in is copied rather than edited, which is the
                    // app's own rule and its own wording: a shipped theme is
                    // written back on every run, so an edit in place would be
                    // silently reverted.
                    {if worn.built_in {
                        format!("Copy {}…", worn.name)
                    } else {
                        format!("Edit {}…", worn.name)
                    }}
                }
                button {
                    class: "chip action",
                    onclick: {
                        let post = post.clone();
                        move |_| theme_file_dialog(post.clone(), None)
                    },
                    "Import theme…"
                }
                button {
                    class: "chip action",
                    onclick: {
                        let post = post.clone();
                        let file = format!("{}.toml", worn.id);
                        move |_| theme_file_dialog(post.clone(), Some(file.clone()))
                    },
                    "Export theme…"
                }
                if !worn.built_in {
                    button {
                        class: "chip action danger",
                        onclick: {
                            let worn = worn.clone();
                            move |_| viewer.write().ask_delete_theme(worn.clone())
                        },
                        "Delete {worn.name}…"
                    }
                }
            }
            Note { text: format!("Theme files live in {folder}. They are plain text — a theme can be written by hand, or copied to another computer.") }
        }
    }
}

/// The system's file dialog for a theme: `None` asks which file to import,
/// `Some(name)` where to export the worn theme under that name. On a thread
/// of its own and answered into the mailbox, for `Pick`'s reason — a modal
/// dialog re-enters winit's handler. Cancelling sends nothing.
fn theme_file_dialog(post: crate::emit::Post, export: Option<String>) {
    std::thread::spawn(move || {
        let dialog = rfd::FileDialog::new().add_filter("Moonowl theme", &["toml"]);
        let (event, chosen) = match export {
            Some(name) => ("export-theme", dialog.set_file_name(name).save_file()),
            None => ("import-theme", dialog.pick_file()),
        };
        if let Some(path) = chosen {
            post.send(crate::emit::News {
                event: event.into(),
                target: None,
                payload: crate::emit::Payload::Text(path.to_string_lossy().into_owned()),
            });
        }
    });
}

/// A theme being written, field by field. `themeEditor` in `settings.ts`.
///
/// **One difference from the app, and it is the platform's.** There, each
/// colour is an `<input type="color">` beside a hex field — the operating
/// system's own picker. Blitz has no colour input, so what is here is the
/// swatch and the hex field, and the swatch is a preview rather than a way in.
/// Everything else is the app's: the same seven fields in the same order with
/// the same sentences under them, the draft worn while it is being written,
/// and Cancel, Save and Delete at the foot.
#[component]
fn ThemeEditor(viewer: Signal<Viewer>, draft: crate::theme::Theme) -> Element {
    let post =
        use_hook(|| dioxus_core::try_consume_context::<crate::emit::Post>().unwrap_or_default());
    // Import and export stand still while there is something unsaved: export
    // writes the theme as saved, and import would put the draft down.
    let unsaved = viewer.read().draft_unsaved();
    let file = format!("{}.toml", draft.id);
    // What the page will actually use, which is what the fields have to show:
    // four of the seven are derived when the file does not name them, and a
    // field standing in with something else is the picker lying again.
    let shown = crate::palette::resolve(&draft, true);
    let hex = crate::palette::hex;
    let fresh = draft.id.trim().is_empty();

    // Enter, from any field in the editor: the theme is saved and the window
    // goes. It is what Enter means in every other window with a form in it,
    // and without it the only way out of the editor was the pointer.
    let done = move |_| {
        viewer.write().save_theme();
        viewer.write().close_settings();
    };

    rsx! {
        h3 { class: "pane-group", {if fresh { "New theme" } else { "Edit theme" }} }
        Field { label: "Name",
            TextField {
                value: draft.name.clone(),
                onchange: move |value| viewer.write().draft_set("name", value),
                onsubmit: done,
            }
        }
        Field {
            label: "Text",
            ColorField {
                viewer,
                field: "text",
                value: hex(shown.text),
                onsubmit: done,
            }
        }
        Field {
            label: "Background",
            ColorField {
                viewer,
                field: "background",
                value: hex(shown.background),
                onsubmit: done,
            }
        }
        Field {
            label: "Accent",
            note: "Marks around things that need to stand out.".to_string(),
            ColorField {
                viewer,
                field: "accent",
                value: hex(shown.accent),
                onsubmit: done,
            }
        }
        // Only while the document is recoloured: otherwise links keep the
        // colour they were printed in, and a field for one would do nothing.
        if draft.recolor {
            Field {
                label: "Links",
                note: "Links within the document, like to the references section, count just like web links.".to_string(),
                ColorField {
                    viewer,
                    field: "link",
                    value: hex(shown.link),
                    onsubmit: done,
                }
            }
        }
        Field {
            label: "Selection area",
            note: "The color behind text you selected. By default, it follows the accent.".to_string(),
            ColorField {
                viewer,
                field: "selection_area",
                value: hex(shown.selection_area),
                onsubmit: done,
            }
        }
        Field {
            label: "Selected text",
            note: "The color of the words you selected. By default, the inverse of the area color.".to_string(),
            ColorField {
                viewer,
                field: "selection_text",
                value: hex(shown.selection_text),
                onsubmit: done,
            }
        }
        Field {
            label: "Recolour the document",
            note: "Off leaves every page exactly as it was printed.".to_string(),
            Toggle {
                on: draft.recolor,
                onchange: move |on| viewer.write().draft_recolor(on),
            }
        }
        div { class: "pane-actions",
            button {
                class: "chip action",
                onclick: move |_| viewer.write().cancel_theme(),
                "Cancel"
            }
            button {
                class: "chip action primary",
                onclick: move |_| viewer.write().save_theme(),
                "Save theme"
            }
            button {
                class: "chip action",
                // Absent rather than "false": Blitz disables on the attribute alone.
                disabled: unsaved.then_some("true"),
                onclick: {
                    let post = post.clone();
                    move |_| if !unsaved { theme_file_dialog(post.clone(), None) }
                },
                "Import theme…"
            }
            button {
                class: "chip action",
                // Absent rather than "false": Blitz disables on the attribute alone.
                disabled: unsaved.then_some("true"),
                onclick: {
                    let post = post.clone();
                    let file = file.clone();
                    move |_| if !unsaved { theme_file_dialog(post.clone(), Some(file.clone())) }
                },
                "Export theme…"
            }
            // Only a theme already on disk can be deleted: "New theme…" and a
            // copy of a built-in have not been saved yet.
            if !fresh {
                button {
                    class: "chip action danger",
                    onclick: move |_| viewer.write().ask_delete_theme(draft.clone()),
                    "Delete this theme…"
                }
            }
        }
    }
}

/// "Delete this theme?", asked in a small window of its own over whatever
/// opened it — the theme menu, the Appearance page or the editor.
#[component]
pub(crate) fn ConfirmDeleteTheme(viewer: Signal<Viewer>) -> Element {
    let held = viewer.read();
    let Some(theme) = held.deleting_theme.clone() else {
        return rsx! {};
    };
    let ink = crate::palette::hex(held.palette().muted());
    drop(held);
    rsx! {
        div {
            class: "window-scrim",
            onmousedown: move |event| {
                event.stop_propagation();
                viewer.write().close_delete_theme();
            },
            div {
                class: "window ask-window",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "Delete theme",
                onmousedown: move |event| event.stop_propagation(),
                div { class: "window-bar",
                    span { class: "window-title", "Delete {theme.name}?" }
                    button {
                        class: "chip window-close",
                        "aria-label": "Close",
                        onclick: move |_| { viewer.write().close_delete_theme(); },
                        Icon { name: "close", stroke: ink.clone() }
                    }
                }
                div { class: "ask-body",
                    p { class: "pane-lede",
                        "Its file is removed from the themes folder, and this cannot be undone."
                    }
                    div { class: "pane-actions ask-actions",
                        button {
                            class: "chip action",
                            onclick: move |_| { viewer.write().close_delete_theme(); },
                            "Keep it"
                        }
                        button {
                            class: "chip action danger",
                            onclick: move |_| viewer.write().confirm_delete_theme(),
                            "Delete theme"
                        }
                    }
                }
            }
        }
    }
}

/// The six highlight colours, each with the full picker, and a way back to
/// what a fresh install has. Opened from the … on the swatches a selection
/// brings up; the swatches stay under it and show the change at once.
#[component]
pub(crate) fn MarkupColours(viewer: Signal<Viewer>) -> Element {
    let held = viewer.read();
    let colours: Vec<String> = crate::app::MARKUP_COLOR_KEYS
        .iter()
        .map(|key| held.store.text(key))
        .collect();
    let worn = held.palette();
    let ink = crate::palette::hex(worn.muted());
    drop(held);
    // Resetting throws six settings away, so the button asks once before
    // it does — in place, rather than in a window over a window.
    let mut confirming = use_signal(|| false);
    rsx! {
        div {
            class: "window-scrim",
            onmousedown: move |event| {
                event.stop_propagation();
                viewer.write().close_markup_colours();
            },
            div {
                class: "window colours-window",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "Highlight colours",
                // A press anywhere in the window a picker is not puts the
                // picker away; the field stops the press itself.
                onmousedown: move |event| {
                    event.stop_propagation();
                    viewer.write().close_picker();
                },
                div { class: "window-bar",
                    span { class: "window-title", "Highlight colours" }
                    button {
                        class: "chip window-close",
                        "aria-label": "Close",
                        onclick: move |_| { viewer.write().close_markup_colours(); },
                        Icon { name: "close", stroke: ink.clone() }
                    }
                }
                div { class: "colours-body",
                    p { class: "field-note",
                        if worn.recolor {
                            "The six colours a selection offers. Press a swatch for the full picker, or type a colour. The second swatch is how the colour comes out on this theme's page."
                        } else {
                            "The six colours a selection offers. Press a swatch for the full picker, or type a colour."
                        }
                    }
                    for (index, (key, colour)) in crate::app::MARKUP_COLOR_KEYS.iter().zip(colours).enumerate() {
                        div { key: "{key}", class: "colours-row",
                            span { class: "colours-label", "Colour {index + 1}" }
                            ColorField {
                                viewer,
                                field: *key,
                                value: colour.clone(),
                                onchange: move |hex: String| viewer.write().set_markup_color(index + 1, hex),
                            }
                            if worn.recolor {
                                if let Some(rgb) = crate::palette::read_colour(&colour) {
                                    span {
                                        class: "colours-on-page",
                                        title: "On the page",
                                        style: "background: {crate::palette::hex(worn.on_page(rgb))};",
                                    }
                                }
                            }
                        }
                    }
                    div { class: "pane-actions",
                        if *confirming.read() {
                            span { class: "colours-ask", "Put all six back to the defaults? Your own colours will be lost." }
                            button {
                                class: "chip action danger",
                                onclick: move |_| {
                                    confirming.set(false);
                                    viewer.write().reset_markup_colors();
                                },
                                "Reset"
                            }
                            button {
                                class: "chip action",
                                onclick: move |_| confirming.set(false),
                                "Keep them"
                            }
                        } else {
                            button {
                                class: "chip action",
                                onclick: move |_| confirming.set(true),
                                "Reset all colours…"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// **A plain key typed into a field is not a shortcut**, and every field in
/// this reader has to say so for itself.
///
/// The root's `onkeydown` hears every key in the window, because that is how a
/// key with nothing focused reaches the reader at all — see the root element in
/// `app.rs`. A field with the focus therefore gets its keystroke *and* so does
/// the keymap, so typing a name into the theme editor scrolled the document
/// behind it: `space` is a screen down and `d` is half of one. The password
/// field carries this rule inline and named it "the same two rules every field
/// in this file has", which was true of that field and of no other.
///
/// A key with a modifier is let through, deliberately, so that ⌘A, ⌘C, ⌘V and
/// ⌘Z still mean what they mean in a field — and so that ⌘, still closes
/// Settings from inside one.
fn typing_is_not_a_shortcut(event: &KeyboardEvent, root: crate::app::RootFocus) {
    // **Escape leaves the field, and that is all it does here.** It used to be
    // let straight through, on the reasoning that Escape is the way out of the
    // window the field is in — which it is, once there is nothing nearer to
    // leave. A field is nearer. Typed into one it reached the root's handler
    // and closed the window, so thinking better of six hexadecimal digits took
    // the whole theme editor with it.
    //
    // So the keyboard goes back to the reader instead, which is what makes the
    // *next* Escape mean the picker and the one after it mean the window —
    // `Action::Dismiss` outward in the order the reader arrived at them, with
    // the field as its innermost step. The stepper says the same thing by
    // handling Escape itself.
    if event.key() == Key::Escape {
        event.stop_propagation();
        crate::app::leave_field(root);
        return;
    }
    let modified = event.modifiers().meta() || event.modifiers().ctrl() || event.modifiers().alt();
    if !modified {
        event.stop_propagation();
    }
}

/// A line of text somebody types. The app's `ui.textField`.
///
/// `onsubmit` is Enter, and is what makes a field a way of finishing rather
/// than only a way of typing: the theme editor saves on it. A field with none
/// swallows Enter as it swallows every other plain key.
#[component]
pub(crate) fn TextField(
    value: String,
    onchange: EventHandler<String>,
    #[props(default)] onsubmit: Option<EventHandler<()>>,
) -> Element {
    let root: crate::app::RootFocus = use_context();
    rsx! {
        input {
            class: "text-field",
            r#type: "text",
            value: "{value}",
            oninput: move |event| onchange.call(event.value()),
            onkeydown: move |event: KeyboardEvent| {
                if finished(&event, onsubmit.as_ref()) {
                    return;
                }
                typing_is_not_a_shortcut(&event, root);
            },
        }
    }
}

/// Enter, in a field that has somewhere to go with it.
///
/// True when the key was Enter and the handler was called, so that the caller
/// stops there. Plain Enter only: ⌘Enter and the rest are nobody's here.
fn finished(event: &KeyboardEvent, onsubmit: Option<&EventHandler<()>>) -> bool {
    if event.key() != Key::Enter || !crate::keymap::plain(event.modifiers()) {
        return false;
    }
    event.stop_propagation();
    if let Some(onsubmit) = onsubmit {
        onsubmit.call(());
    }
    true
}

/// The colours the picker keeps ready to hand, in the order they are laid out:
/// eight greys, then ten hues in four steps each from pale to deep.
///
/// **A shortcut, not the picker.** Forty colours is enough to land on something
/// close in one press, and the square and the strip above them are how any
/// other colour is reached. It used to be the whole of the way in, which made
/// every colour outside the forty a question of six hexadecimal digits.
const SWATCHES: &[&str] = &[
    "#ffffff", "#f2f3f5", "#d9dce1", "#b4b9c1", "#868d99", "#575d68", "#2f3237", "#000000",
    "#fde2e2", "#f2a2a2", "#d64545", "#8f2020", "#fdeada", "#f4bc80", "#d97a1c", "#8c4a0a",
    "#fdf6d3", "#f0dd8a", "#c9a227", "#7d6410", "#e3f3dd", "#a8d99a", "#4f9e3f", "#2c5f22",
    "#daf1ee", "#8fd3cb", "#2f9c8e", "#175c53", "#dcecfa", "#9ec9ee", "#2f7fc4", "#174d7a",
    "#e2e2f7", "#a9a9e4", "#5a5ac0", "#333376", "#f2dcf3", "#d9a4dd", "#a44eb0", "#6b2f72",
];

/// The saturation/value square and the hue strip, in CSS pixels.
///
/// Numbers rather than a measurement, because the measurement is not
/// available: a pointer's position has to become a fraction of the box, and
/// the box cannot be measured from inside an event handler. So the size is
/// written here and nowhere else — inlined into the element's own `style`,
/// exactly as the Sign window's pad is, because a box sized in the stylesheet
/// and read in Rust is two numbers that have to agree and a handler that is
/// wrong by their difference. Border-box sizes, which is what
/// `element_coordinates` measures against. The width is the swatch grid's
/// own: eight of 22 with 4 between them.
const SQUARE_W: f64 = 204.0;
const SQUARE_H: f64 = 116.0;
const STRIP_H: f64 = 18.0;

/// The three `background` lists that draw one position marker.
///
/// Kept together because they are only correct together: the images, their
/// positions and their sizes are three parallel lists, and one layer is the
/// same index in each.
struct Marker {
    image: String,
    position: String,
    size: String,
}

/// A marker on the square or the strip, as three stacked background layers —
/// a dark outline, a white ring inside it, and the picked colour in the
/// middle, so that it reads on a white corner and a black one alike.
///
/// **Background layers rather than child elements**, and that is the one
/// non-obvious decision here: a child on top of the square is what the pointer
/// hits, and Blitz measures element coordinates against the node that was hit
/// — so the square's arithmetic would be reading offsets from the marker.
///
/// **Not a `radial-gradient`.** Blitz resolves a radial gradient's centre in
/// CSS pixels and adds it to a rectangle already in device pixels, so on a 2×
/// display the ring lands at half the offset. `background-position` and
/// `background-size` are both scaled before use, and `linear-gradient(c, c)`
/// is a flat fill, so a marker built from those is drawn where it was put at
/// any scale.
///
/// The centre is held half a marker inside the box: a layer is clipped to its
/// element, so an unclamped marker on a fully black or fully saturated colour
/// would be a sliver against the edge — which is exactly when somebody is
/// looking for it.
fn marker(x: f64, y: f64, width: f64, height: f64, fill: crate::palette::Rgb) -> Marker {
    /// Half the outermost square, which is how far in the centre is held.
    const REACH: f64 = 8.0;

    let x = x.clamp(REACH, width - REACH);
    let y = y.clamp(REACH, height - REACH);
    let fill = crate::palette::hex(fill);
    let corner = |inset: f64| format!("{:.1}px {:.1}px", x - inset, y - inset);

    Marker {
        image: format!(
            "linear-gradient({fill}, {fill}), \
             linear-gradient(#ffffff, #ffffff), \
             linear-gradient(rgba(0,0,0,0.55), rgba(0,0,0,0.55))"
        ),
        position: format!("{}, {}, {}", corner(4.0), corner(6.0), corner(REACH)),
        size: "8px 8px, 12px 12px, 16px 16px".to_string(),
    }
}

/// A colour as the picker holds it: hue in degrees, the rest on 0…1.
#[derive(Clone, Copy, PartialEq)]
struct Hsv {
    hue: f64,
    saturation: f64,
    value: f64,
}

fn to_hsv(rgb: crate::palette::Rgb) -> Hsv {
    let (r, g, b) = (
        f64::from(rgb[0]) / 255.0,
        f64::from(rgb[1]) / 255.0,
        f64::from(rgb[2]) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let span = max - min;
    let hue = if span == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / span) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / span + 2.0)
    } else {
        60.0 * ((r - g) / span + 4.0)
    };
    Hsv {
        hue: if hue < 0.0 { hue + 360.0 } else { hue },
        saturation: if max == 0.0 { 0.0 } else { span / max },
        value: max,
    }
}

fn from_hsv(hsv: Hsv) -> crate::palette::Rgb {
    let Hsv {
        hue,
        saturation,
        value,
    } = hsv;
    let sector = hue.rem_euclid(360.0) / 60.0;
    let span = value * saturation;
    let middle = span * (1.0 - (sector % 2.0 - 1.0).abs());
    let base = value - span;
    let (r, g, b) = match sector as u32 {
        0 => (span, middle, 0.0),
        1 => (middle, span, 0.0),
        2 => (0.0, span, middle),
        3 => (0.0, middle, span),
        4 => (middle, 0.0, span),
        _ => (span, 0.0, middle),
    };
    let channel = |part: f64| ((part + base) * 255.0).round().clamp(0.0, 255.0) as u8;
    [channel(r), channel(g), channel(b)]
}

/// A colour: what it looks like, the six digits that say so, and a grid of
/// colours to point at instead.
///
/// **The field takes anything, and complains rather than correcting.** It used
/// to show the theme's colour and pass on only what parsed, which meant every
/// keystroke that did not parse was rewritten under the caret: a Backspace
/// took a character out and the old value put it straight back, and typing
/// over `#2f3237` was a fight. What is typed now stays typed — marked as
/// unreadable while it is — and Enter or leaving the field puts the last
/// readable colour back if what is in the box is not one. A colour that *does*
/// parse is passed on as it is typed, so the window recolours under the hand.
///
/// The hex field takes every notation the renderer reads rather than only the
/// long one — a theme file may perfectly well say `#fff` — and what leaves
/// here is always the six-digit form, because that is what is written back to
/// the file.
#[component]
pub(crate) fn ColorField(
    viewer: Signal<Viewer>,
    /// The theme file's own name for this colour, which is both what a change
    /// is written to and which picker is open. See [`Viewer::draft_set`] and
    /// [`Viewer::picking`].
    field: &'static str,
    value: String,
    #[props(default)] onsubmit: Option<EventHandler<()>>,
    /// Where a change goes, when it is not a theme draft — the highlight
    /// colours write straight to the settings.
    #[props(default)]
    onchange: Option<EventHandler<String>>,
) -> Element {
    let root: crate::app::RootFocus = use_context();
    let open = viewer.read().picking == Some(field);
    let mut change = move |hex: String| match onchange.as_ref() {
        Some(onchange) => onchange.call(hex),
        None => viewer.write().draft_set(field, hex),
    };
    // What is in the box while it is being typed in, and nothing when it is
    // not: the same shape as the stepper's, one field along, and for the same
    // reason — Blitz's `set_text` moves no caret, so a value written back
    // under the caret puts it at the front.
    let mut typed = use_signal(|| None::<String>);
    let showing = typed.read().clone().unwrap_or_else(|| value.clone());
    let unreadable = crate::palette::read_colour(&showing).is_none();
    // Leaving the field, whether by Enter or by pressing elsewhere: what is
    // readable is kept, and what is not is dropped for what the theme has.
    let mut settle = move || typed.set(None);

    // **Where the square and the strip are pointed, which is not simply read
    // off the colour.** The round trip through hue, saturation and value is
    // lossy exactly where a picker is used — a grey has no hue, black has
    // neither hue nor saturation — so a square dragged into its own left edge
    // would snap the strip to red and strand whoever was dragging it.
    //
    // So the picker remembers what it last wrote, and that stands wherever the
    // colour cannot say. A colour arriving from anywhere else — the hex field,
    // a swatch, another theme opened into the same fields — is told apart by
    // not being what the remembered one would produce, and is read afresh.
    let picked = crate::palette::read_colour(&value).unwrap_or([0, 0, 0]);
    let mut kept = use_signal(|| to_hsv(picked));
    let hsv = if from_hsv(*kept.peek()) == picked {
        *kept.peek()
    } else {
        to_hsv(picked)
    };
    // A press takes hold and a release lets go; `peek` rather than a read,
    // because a drag must not re-render the window on every move.
    let mut dragging = use_signal(|| false);

    let mut apply = move |next: Hsv| {
        kept.set(next);
        typed.set(None);
        change(crate::palette::hex(from_hsv(next)));
    };
    let mut pick_square = move |event: MouseEvent| {
        let on = event.element_coordinates();
        apply(Hsv {
            saturation: (on.x / SQUARE_W).clamp(0.0, 1.0),
            value: 1.0 - (on.y / SQUARE_H).clamp(0.0, 1.0),
            ..hsv
        });
    };
    let mut pick_strip = move |event: MouseEvent| {
        let on = event.element_coordinates();
        apply(Hsv {
            hue: (on.x / SQUARE_W).clamp(0.0, 1.0) * 360.0,
            ..hsv
        });
    };

    let square_mark = marker(
        hsv.saturation * SQUARE_W,
        (1.0 - hsv.value) * SQUARE_H,
        SQUARE_W,
        SQUARE_H,
        picked,
    );
    // The hue itself, at full saturation and value: what the square washes
    // white and black over, and what the strip's own marker is filled with.
    let pure = from_hsv(Hsv {
        hue: hsv.hue,
        saturation: 1.0,
        value: 1.0,
    });
    let strip_mark = marker(
        hsv.hue / 360.0 * SQUARE_W,
        STRIP_H / 2.0,
        SQUARE_W,
        STRIP_H,
        pure,
    );
    let pure = crate::palette::hex(pure);

    rsx! {
        span { class: "color-field",
            button {
                class: "color-swatch",
                "aria-label": "Choose a colour",
                style: "background: {value};",
                // The window closes a picker on any press; this press is the
                // one that opens or closes it, so it must not reach the window.
                onmousedown: move |event| event.stop_propagation(),
                onclick: move |_| viewer.write().toggle_picker(field),
            }
            input {
                class: if unreadable { "text-field color-hex unreadable" } else { "text-field color-hex" },
                r#type: "text",
                value: "{showing}",
                onkeydown: move |event: KeyboardEvent| {
                    if !crate::keymap::plain(event.modifiers()) {
                        return;
                    }
                    match event.key() {
                        // Done with this field: the colour stands or the last
                        // one comes back, and the editor around it hears the
                        // Enter — which is what saves the theme.
                        Key::Enter => {
                            settle();
                            event.stop_propagation();
                            if let Some(onsubmit) = onsubmit.as_ref() {
                                onsubmit.call(());
                            }
                        }
                        // What was typed and is not a colour goes, and the
                        // key stops here: leaving the field is what Escape
                        // means while the caret is in one. The picker below is
                        // the next Escape's business — see
                        // `typing_is_not_a_shortcut`.
                        Key::Escape => {
                            settle();
                            typing_is_not_a_shortcut(&event, root);
                        }
                        _ => typing_is_not_a_shortcut(&event, root),
                    }
                },
                onblur: move |_| settle(),
                oninput: move |event| {
                    let text = event.value();
                    if let Some(read) = crate::palette::read_colour(&text) {
                        change(crate::palette::hex(read));
                    }
                    typed.set(Some(text));
                },
            }
            if open {
                div { class: "color-picker",
                    onmousedown: move |event| event.stop_propagation(),
                    // **Four background layers**, which is what a saturation/
                    // value square is with no `<canvas>` to draw one in: the
                    // marker, black washed up from the foot, white washed in
                    // from the left, and the hue itself as a gradient from one
                    // colour to the same colour — a flat fill, written that way
                    // so every layer is an image and the three lists line up
                    // entry for entry.
                    div {
                        class: "color-square",
                        "data-square": "true",
                        style: "width: {SQUARE_W}px; height: {SQUARE_H}px; \
                            background-image: {square_mark.image}, \
                            linear-gradient(to top, #000000, rgba(0,0,0,0)), \
                            linear-gradient(to right, #ffffff, rgba(255,255,255,0)), \
                            linear-gradient({pure}, {pure}); \
                            background-position: {square_mark.position}, 0px 0px, 0px 0px, 0px 0px; \
                            background-size: {square_mark.size}, auto, auto, auto; \
                            background-repeat: no-repeat;",
                        onmousedown: move |event: MouseEvent| {
                            event.stop_propagation();
                            dragging.set(true);
                            pick_square(event);
                        },
                        onmousemove: move |event: MouseEvent| {
                            if *dragging.peek() {
                                pick_square(event);
                            }
                        },
                        onmouseup: move |_| dragging.set(false),
                        onmouseleave: move |_| dragging.set(false),
                    }
                    div {
                        class: "color-strip",
                        "data-strip": "true",
                        style: "width: {SQUARE_W}px; height: {STRIP_H}px; \
                            background-image: {strip_mark.image}, \
                            linear-gradient(to right, #ff0000, #ffff00, #00ff00, #00ffff, #0000ff, #ff00ff, #ff0000); \
                            background-position: {strip_mark.position}, 0px 0px; \
                            background-size: {strip_mark.size}, auto; \
                            background-repeat: no-repeat;",
                        onmousedown: move |event: MouseEvent| {
                            event.stop_propagation();
                            dragging.set(true);
                            pick_strip(event);
                        },
                        onmousemove: move |event: MouseEvent| {
                            if *dragging.peek() {
                                pick_strip(event);
                            }
                        },
                        onmouseup: move |_| dragging.set(false),
                        onmouseleave: move |_| dragging.set(false),
                    }
                    div { class: "color-grid", role: "listbox", "aria-label": "Colours",
                        for swatch in SWATCHES.iter().copied() {
                            button {
                                key: "{swatch}",
                                class: if swatch.eq_ignore_ascii_case(&value) { "color-choice on" } else { "color-choice" },
                                "aria-label": "{swatch}",
                                style: "background: {swatch};",
                                onclick: move |_| {
                                    viewer.write().close_picker();
                                    if let Some(rgb) = crate::palette::read_colour(swatch) {
                                        apply(to_hsv(rgb));
                                    }
                                },
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn WindowPage(viewer: Signal<Viewer>, frame: crate::app::Frame) -> Element {
    let held = viewer.read();
    let toolbar = held.toolbar;
    let sidebar = held.sidebar_open;
    let search_sidebar = held.store.flag("search_shows_sidebar");
    let width = held.sidebar_width;
    let full = held.full_screen;
    let presenting = held.presenting;
    // Read off the keymap, as every menu does, so a rebound key shows the
    // key it was bound to rather than the one it shipped with.
    let key_toolbar = held.chord_for(Action::Toolbar);
    let key_sidebar = held.chord_for(Action::Sidebar);
    let key_full = held.chord_for(Action::Fullscreen);
    let key_present = held.chord_for(Action::Present);
    drop(held);

    rsx! {
        h2 { class: "pane-title", "Window" }
        Field {
            label: "Show toolbar",
            note: format!("The bar along the top. Hidden, the page number appears briefly as you scroll, and the top edge of the window brings the bar back. {key_toolbar}"),
            Toggle { on: toolbar, onchange: move |_| viewer.write().toggle_toolbar() }
        }
        Field {
            label: "Show sidebar",
            note: format!("Chapters and page thumbnails, down the left. {key_sidebar}"),
            Toggle { on: sidebar, onchange: move |_| viewer.write().toggle_sidebar() }
        }
        Field {
            label: "Make search show sidebar",
            note: "The results open in the sidebar as soon as there are any, and close with the find bar. Off, the count in the find bar still opens them.",
            Toggle { on: search_sidebar, onchange: move |on| viewer.write().set_flag("search_shows_sidebar", on) }
        }
        Field {
            label: "Sidebar width",
            note: "It can also be dragged by its edge.",
            Stepper {
                value: width, min: crate::sidebar::MIN_WIDTH, max: crate::sidebar::MAX_WIDTH, step: 8.0, unit: "px",
                onchange: move |value| viewer.write().set_sidebar_width(value),
            }
        }
        // Both of these are the *window's* rather than the page's, so throwing
        // one is an `Ask` — which is why this component takes the `Frame` the
        // reader holds. They were a sentence here until it did.
        Field {
            label: "Full screen",
            note: format!("The window fills the screen. {key_full} — and Escape leaves again."),
            Toggle {
                on: full,
                onchange: {
                    let frame = frame.clone();
                    move |on| {
                        viewer.write().set_full_screen(on);
                        frame.ask(crate::app::Ask::FullScreen(on));
                    }
                },
            }
        }
        Field {
            label: "Presenting",
            note: format!("Full screen with nothing else on it: the two switches above, thrown together, and Escape puts both back. {key_present}"),
            Toggle {
                on: presenting,
                onchange: {
                    let frame = frame.clone();
                    move |on| {
                        let full = viewer.write().present(on);
                        frame.ask(crate::app::Ask::FullScreen(full));
                    }
                },
            }
        }
    }
}

#[component]
fn Keyboard(viewer: Signal<Viewer>) -> Element {
    // **Drawn from the keymap, never from a list of its own.** The app's page
    // was a hand-written table once and it had already drifted: it named ⌘T
    // twice and could not have known about a key the reader rebound. Every
    // row here is an action out of `keymap.rs` with whatever `keys.toml` gave
    // it, so the page is a view of what the reader will actually get.
    let held = viewer.read();
    let keymap = held.keymap.clone();
    let keys_file = held.store.dir().join("keys.toml").display().to_string();
    drop(held);
    let mac = keymap.mac();

    rsx! {
        h2 { class: "pane-title", "Keyboard" }
        // What could not be read comes first: a key that does nothing is
        // otherwise found out about by pressing it.
        if !keymap.problems.is_empty() {
            h3 { class: "pane-group", "In your keys.toml" }
            for problem in keymap.problems.clone() {
                Note { text: problem }
            }
        }
        for group in keymap::GROUPS {
            {
                let rows: Vec<(String, String)> = keymap::every()
                    .filter(|spec| spec.group == group)
                    .filter_map(|spec| {
                        let chords = keymap.by_action.get(&spec.id)?;
                        if chords.is_empty() {
                            return None;
                        }
                        let shown = chords
                            .iter()
                            .map(|chord| keymap::shown(chord, mac))
                            .collect::<Vec<_>>()
                            .join("  or  ");
                        Some((spec.label.to_string(), shown))
                    })
                    .collect();
                rsx! {
                    if !rows.is_empty() {
                        h3 { class: "pane-group", "{group.as_str()}" }
                        div { class: "keys",
                            for (what, chord) in rows {
                                span { key: "{what}", class: "key-what", "{what}" }
                                span { class: "key-chord", "{chord}" }
                            }
                        }
                    }
                }
            }
        }
        h3 { class: "pane-group", "Without the keyboard" }
        div { class: "keys",
            span { class: "key-what", "Bring the toolbar back when it is hidden" }
            span { class: "key-chord", "The top edge of the window" }
            span { class: "key-what", "Open something else you have been reading" }
            span { class: "key-chord", "The document's name in the bar" }
            span { class: "key-what", "Move a page that is wider than the window" }
            span { class: "key-chord", "Two fingers across, or ⇧ and the wheel" }
        }
        h3 { class: "pane-group", "Changing keybinds" }
        Note { text: "Every keybind is in keys.toml in your config folder, commented out. Uncomment a line to change its keys, then Reload — the file is deliberately not watched, because the app writes to that folder several times a minute while you are scrolling." }
        div { class: "pane-actions",
            // The file itself, opened in whatever edits text here. The app's
            // own first button, and the reason it is beside Reload: the two
            // are one gesture — change the file, then say so.
            OpenPath {
                viewer,
                label: "Open keys file".to_string(),
                path: keys_file,
            }
            button {
                class: "chip action",
                onclick: move |_| viewer.write().reload_keys(),
                "Reload"
            }
        }
    }
}

#[component]
fn About(viewer: Signal<Viewer>) -> Element {
    let held = viewer.read();
    let config = held.store.dir().display().to_string();
    let themes = held.store.themes_dir().display().to_string();
    let settings_file = held.store.dir().join("settings.toml").display().to_string();
    drop(held);
    let licenses = licenses_dir();

    rsx! {
        h2 { class: "pane-title", "Moonowl" }
        p { class: "pane-lede", "A calm place to read." }
        Note { text: "Your settings and themes are stored in plain text on this computer and not sent anywhere else." }
        div { class: "keys",
            span { class: "key-what", "Settings and keys" }
            span { class: "key-chord", "{config}" }
            span { class: "key-what", "Themes" }
            span { class: "key-chord", "{themes}" }
        }
        div { class: "pane-actions",
            OpenPath {
                viewer,
                label: "Open settings file".to_string(),
                path: settings_file,
            }
            OpenPath {
                viewer,
                label: "Open themes folder".to_string(),
                path: themes.clone(),
            }
            if let Some(path) = licenses {
                OpenPath {
                    viewer,
                    label: "Open licences folder".to_string(),
                    path,
                }
            }
        }
    }
}

/// Where the installer put the licence notices — pdfium's and its libraries',
/// and the app's own — or `None` from a bare `cargo run`, which has none.
///
/// The same three places `pdfium.rs` looks for the library, one level over:
/// `Contents/Resources` in the `.app`, `/usr/lib/Moonowl` beside `/usr/bin`,
/// and the executable's own directory on Windows. `Cargo.toml` says why the
/// folder exists.
fn licenses_dir() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [
        dir.join("../Resources/licenses"),
        dir.join("../lib/Moonowl/licenses"),
        dir.join("licenses"),
    ]
    .into_iter()
    .find(|d| d.is_dir())
    .map(|d| d.display().to_string())
}

/// A button that hands a path to whatever the platform opens it with.
///
/// Three of these, and all three are about the same thing: the files this
/// reader keeps are plain text, and the point of saying so is that they can be
/// opened. Through [`crate::app::Reveal`], which is the one door in this crate
/// that starts another program — so a test writes the path down instead.
#[component]
fn OpenPath(viewer: Signal<Viewer>, label: String, path: String) -> Element {
    let reveal = use_hook(|| {
        dioxus_core::try_consume_context::<crate::app::Reveal>()
            .unwrap_or_else(crate::app::Reveal::to_the_system)
    });
    rsx! {
        button {
            class: "chip action",
            onclick: move |_| {
                if let Err(said) = reveal.show(&path) {
                    viewer.write().notice = said;
                }
            },
            "{label}"
        }
    }
}
