//! The menus: the Document menu, a right-click on a page or a mark, and Copy
//! over a window's own words.

use super::*;

/// What can be done with the document: the Document menu under its name, and
/// the lower half of a right-click on a page.
///
/// `here` is where on `page` the right-click was, and nothing for the
/// Document menu. **The two differ in three rows**: a right-click bookmarks
/// and signs the page it was on, where the menu has the page being read, and
/// it leaves out "Remove all highlights" — every mark in the document is not
/// a thing to have one misplaced click away.
/// What the document items show, read off the viewer by whoever renders
/// them — the toolbar's memo, or the context menu — so that
/// [`document_items`] itself reads nothing while the toolbar renders.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct DocumentFacts {
    accent: String,
    page: usize,
    marked: bool,
    bookmark: String,
}

impl DocumentFacts {
    pub(super) fn of(held: &Viewer, here: Option<(usize, Option<(f64, f64)>)>) -> Self {
        let page = here.map_or_else(|| held.page(), |(page, _)| page);
        DocumentFacts {
            accent: crate::palette::hex(held.palette().accent),
            page,
            marked: held.store.is_marked(page),
            bookmark: match here {
                Some(_) => format!("Bookmark page {}", held.label(page)),
                None => "Bookmark this page".to_string(),
            },
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn document_items(
    mut viewer: Signal<Viewer>,
    reveal: &Reveal,
    printer: &Printer,
    clip: &Clip,
    ink: &str,
    key_mark: &str,
    key_print: &str,
    here: Option<(usize, Option<(f64, f64)>)>,
    facts: &DocumentFacts,
) -> Element {
    let ink = ink.to_string();
    let DocumentFacts {
        accent,
        page,
        marked,
        bookmark,
    } = facts.clone();
    rsx! {
        // Where the document lives, which is the app's own first item — and
        // the one thing in this menu that is about the file rather than about
        // what is in it.
        button {
            class: "menu-item",
            "data-item": "reveal",
            onclick: {
                let reveal = reveal.clone();
                move |_| {
                    viewer.write().close_menu();
                    let path = viewer.read().document.path().to_string();
                    if path.is_empty() {
                        return;
                    }
                    if let Err(said) = reveal.show(&path) {
                        viewer.write().notice = said;
                    }
                }
            },
            Icon { name: "folder", stroke: ink.clone() }
            span { class: "menu-label", "Show in {crate::app::file_manager_name()}" }
        }
        // The page marked. Not a chip in the bar: a mark is set once and read
        // from the Contents panel, so a permanent button for it is one nobody
        // presses twice in an hour. It ticks, which is what the chip's "on"
        // state said.
        button {
            class: "menu-item",
            "data-item": "mark",
            onclick: move |_| {
                viewer.write().close_menu();
                viewer.write().mark_page(page);
            },
            Icon { name: "mark", stroke: ink.clone() }
            span { class: "menu-label", "{bookmark}" }
            span { class: "menu-tick", if marked { Icon { name: "check", stroke: accent.clone() } } }
            span { class: "menu-key", "{key_mark}" }
        }
        if here.is_none() {
            button {
                class: "menu-item",
                "data-item": "unmark-all",
                onclick: move |_| viewer.write().ask_remove_all_markup(),
                Icon { name: "trash", stroke: ink.clone() }
                span { class: "menu-label", "Remove all highlights" }
            }
        }
        // **The one item in this menu the app has no counterpart for** — see
        // [`crate::sign`]. It sits beside the bookmark because both are done
        // *to* the document, as against the three below, which take it
        // somewhere else. From a right-click it goes where the click was.
        button {
            class: "menu-item",
            "data-item": "sign",
            onclick: move |_| match here {
                Some((page, Some(on))) => viewer.write().open_signing_at(page, on),
                _ => {
                    viewer.write().open_signing();
                }
            },
            Icon { name: "sign", stroke: ink.clone() }
            span { class: "menu-label", if matches!(here, Some((_, Some(_)))) { "Sign here…" } else { "Sign…" } }
        }
        // Printing prints nothing: the document goes to a program that does.
        // See [`Printer`].
        button {
            class: "menu-item",
            "data-item": "print",
            onclick: {
                let printer = printer.clone();
                move |_| {
                    viewer.write().close_menu();
                    let path = viewer.read().document.path().to_string();
                    if path.is_empty() {
                        return;
                    }
                    if let Err(said) = printer.print(&path) {
                        viewer.write().notice = said;
                    }
                }
            },
            Icon { name: "print", stroke: ink.clone() }
            span { class: "menu-label", "Print…" }
            span { class: "menu-key", "{key_print}" }
        }
        // Two ways of taking the document with you, which is the app's own
        // pair. The name is what the toolbar shows; the path is what another
        // program will want.
        button {
            class: "menu-item",
            "data-item": "copy-name",
            onclick: {
                let clip = clip.clone();
                move |_| {
                    viewer.write().close_menu();
                    let name = viewer.read().store.title().to_string();
                    viewer.write().notice = clip.copy(&name, "Name copied.");
                }
            },
            Icon { name: "copy", stroke: ink.clone() }
            span { class: "menu-label", "Copy name" }
        }
        button {
            class: "menu-item",
            "data-item": "copy-path",
            onclick: {
                let clip = clip.clone();
                move |_| {
                    viewer.write().close_menu();
                    let path = viewer.read().document.path().to_string();
                    viewer.write().notice = clip.copy(&path, "Path copied.");
                }
            },
            Icon { name: "copy", stroke: ink.clone() }
            span { class: "menu-label", "Copy path" }
        }
        div { class: "menu-rule" }
        // What the document says about itself. Last, and behind a rule,
        // because it is the one item here that opens something rather than
        // doing something.
        button {
            class: "menu-item",
            "data-item": "information",
            onclick: move |_| {
                viewer.write().close_menu();
                viewer.write().open_details();
            },
            Icon { name: "info", stroke: ink.clone() }
            span { class: "menu-label", "Information" }
        }
    }
}

thread_local! {
    /// What was selected in a window's own words when the press being handled
    /// began. Blitz puts a selection down on any press, so the shell reads it
    /// first — see [`note_selection`] — and [`window_menu`] takes it.
    static SELECTED_AT_PRESS: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// What the shell does before it hands a press to Blitz.
pub fn note_selection(doc: &blitz_dom::BaseDocument) {
    let selected = doc
        .has_text_selection()
        .then(|| doc.get_selected_text())
        .flatten()
        .filter(|text| !text.trim().is_empty());
    SELECTED_AT_PRESS.with(|cell| *cell.borrow_mut() = selected);
}

/// **A right-click over a window with some of its words selected offers to
/// copy them**, as ⌘C already did. Called first by every window's press;
/// answers whether the menu opened, and then the press is kept from Blitz,
/// which would put the selection down.
pub(crate) fn window_menu(mut viewer: Signal<Viewer>, event: &MouseEvent) -> bool {
    if !asks_for_context(event) {
        return false;
    }
    let Some(text) = SELECTED_AT_PRESS.with(|cell| cell.borrow_mut().take()) else {
        return false;
    };
    event.prevent_default();
    let at = event.client_coordinates();
    viewer.write().copy_menu = Some(((at.x, at.y), text));
    true
}

/// The menu [`window_menu`] opens: one row, at the pointer. A press anywhere
/// else puts it away and goes no further, as a menu's does.
#[component]
pub(super) fn CopyMenu(viewer: Signal<Viewer>, clip: Clip) -> Element {
    let held = viewer.read();
    let Some(((x, y), text)) = held.copy_menu.clone() else {
        return rsx! {};
    };
    let ink = crate::palette::hex(held.palette().muted());
    let key = held.chord_for(Action::Copy);
    let (wide, high) = (held.window_width, held.window_height);
    drop(held);
    let (left, top) = (
        (x + 2.0).min(wide - 240.0).max(8.0),
        (y + 2.0).min(high - MENU_ROW - 22.0).max(8.0),
    );
    rsx! {
        div {
            class: "menu-catch",
            onmousedown: move |event| {
                event.stop_propagation();
                viewer.write().copy_menu = None;
            },
            div {
                class: "menu copy-menu",
                role: "menu",
                style: "left: {left}px; top: {top}px;",
                onmousedown: move |event| {
                    event.stop_propagation();
                    event.prevent_default();
                },
                button {
                    class: "menu-item",
                    "data-item": "copy",
                    onclick: move |_| {
                        let said = clip.copy(&text, "Copied.");
                        let mut held = viewer.write();
                        held.copy_menu = None;
                        held.notice = said;
                    },
                    Icon { name: "copy", stroke: ink.clone() }
                    span { class: "menu-label", "Copy" }
                    span { class: "menu-key", "{key}" }
                }
            }
        }
    }
}

/// Whether a press is a right-click: the right button, or ⌃ and the left on
/// a Mac, which is the same thing there.
pub(super) fn asks_for_context(event: &MouseEvent) -> bool {
    use dioxus::html::input_data::MouseButton;
    match event.trigger_button() {
        Some(MouseButton::Secondary) => true,
        Some(MouseButton::Primary) => cfg!(target_os = "macos") && event.modifiers().ctrl(),
        _ => false,
    }
}

/// A menu row's height and a rule's, as `.menu-item` and `.menu-rule` come
/// out, for keeping the right-click menu inside the window.
const MENU_ROW: f64 = 35.0;
const MENU_RULE: f64 = 11.0;
/// As wide as the right-click menu is allowed to come out, which is what it
/// is kept clear of the window's right edge by.
const CONTEXT_WIDTH: f64 = 280.0;
/// The comment card at the top of a mark's menu: the menu's width inside its
/// border and padding, and how many lines of it show before they scroll.
const MENU_NOTE_WIDTH: f64 = CONTEXT_WIDTH - 14.0;
const MENU_NOTE_LINES: usize = 8;

/// That card's height at `MENU_NOTE_LINES`: its border and padding, who
/// wrote it, the lines, and Edit's row.
fn menu_note_tallest() -> f64 {
    24.0 + 16.0 + MENU_NOTE_LINES as f64 * 22.0 + 30.0
}

/// The longest a selection is quoted in "Search for “…”".
pub(super) const QUOTED: usize = 24;

/// A mark's menu, placed in `.pages`, which scrolls with the pages. See
/// [`Viewer::mark_open`].
pub(super) struct MarkMenu {
    top: f64,
    left: f64,
    key: MarkKey,
    colour: String,
    note: String,
    /// Who wrote it and when. See [`byline`].
    said: String,
    /// What is selected, as a Find row names it, when the menu was asked
    /// for inside a selection: the selection's rows join the mark's.
    selected: Option<String>,
    /// Asked for over its comment. See [`Viewer::comment_menu`].
    comment: bool,
    commenting: Option<String>,
    /// Whether a comment can go on it: in the file, and the file takes one.
    commentable: bool,
}

impl MarkMenu {
    /// **At the pointer and inside the window**, as the right-click menu is:
    /// under the line it is about, or over it where it would run off the
    /// bottom, and pulled left where it would run off the right. How tall it
    /// comes out is counted from its rows, as that menu's is.
    pub(super) fn placed(held: &Viewer, at: Rect, key: &MarkKey, colour: &str) -> Self {
        let (note, said) = match key {
            MarkKey::InFile(page, index) => {
                (held.note_of(*page, *index), held.byline_of(*page, *index))
            }
            MarkKey::Beside(_) => (String::new(), String::new()),
        };
        let selected = held.has_selection().then(|| held.find_label());
        let comment = held.comment_menu;
        let commenting = held.commenting.clone();
        let commentable = held.standing.into_file && matches!(key, MarkKey::InFile(..));
        let (rows, rules, card) = if comment {
            (4.0, 1.0, 0.0)
        } else {
            let with = f64::from(u8::from(selected.is_some()));
            // Its card, counted as [`comment_cards`] counts one, and its
            // margin: who, the words up to `MENU_NOTE_LINES`, and Edit.
            let card = if note.is_empty() {
                0.0
            } else {
                let lines = note_lines(&note, MENU_NOTE_WIDTH - 24.0).min(MENU_NOTE_LINES);
                (18.0 + 17.0 + 30.0 + lines as f64 * 22.0).min(menu_note_tallest()) + 6.0
            };
            // Comment, or Remove comment, is the one row that can be missing.
            let comment_row = f64::from(u8::from(commentable || !note.is_empty()));
            (3.0 + comment_row + 3.0 * with, 1.0 + with, card)
        };
        // The menu's height even while its comment is written, so the card
        // being written lands on the one that was read rather than moving.
        // The swatches' row is the 30.
        let tall = rows * MENU_ROW + rules * MENU_RULE + 14.0 + 30.0 + card;
        let gap = if at.height > 0.0 { 8.0 } else { 2.0 };
        let (wide, high) = (held.document_width(), held.layout.viewport.height);
        let (x, y) = (at.left - held.scroll_left(), at.top - held.scroll_top);
        let below = y + at.height + gap;
        let top = if below + tall > high - 8.0 {
            (y - tall - gap).max(8.0)
        } else {
            below
        };
        // On whole pixels, so the ring round a swatch is as thick on every
        // side: a two-pixel border begun a third of the way into a pixel is
        // two different-looking edges.
        MarkMenu {
            top: top.round(),
            left: x.min(wide - CONTEXT_WIDTH - 8.0).max(8.0).round(),
            key: key.clone(),
            colour: colour.to_string(),
            note,
            said,
            selected,
            comment,
            commenting,
            commentable,
        }
    }
}

/// What a mark's menu offers: its colours, then what can be done to the
/// passage — or, over its comment, to the comment. See [`MarkMenu`].
pub(super) fn mark_menu_rows(
    mut viewer: Signal<Viewer>,
    menu: MarkMenu,
    colours: &[String],
    worn: crate::palette::Palette,
    clip: &Clip,
    ink: &str,
) -> Element {
    let ink = ink.to_string();
    let MarkMenu {
        top,
        left,
        key,
        colour,
        note,
        said,
        selected,
        comment,
        commenting,
        commentable,
    } = menu;
    let on_page = move |colour: &String| {
        crate::palette::read_colour(colour)
            .map(|rgb| crate::palette::hex(worn.on_page(rgb)))
            .unwrap_or_else(|| colour.clone())
    };
    let colours = colours.to_vec();
    let clip = clip.clone();
    // A comment with no card beside the page is written in one where the
    // menu was: over the menu's own card, where there is one, so that only
    // the field comes and the rest of the menu goes.
    if let Some(draft) = commenting {
        let stripe = on_page(&colour);
        return rsx! {
            div {
                class: "note-card editing",
                style: "position: absolute; top: {top + 7.0}px; left: {left + 7.0}px; width: {MENU_NOTE_WIDTH}px; border-color: {stripe};",
                onmousedown: move |event| event.stop_propagation(),
                if !said.is_empty() {
                    div { class: "note-card-by", "{said}" }
                }
                NoteField { viewer, draft, width: MENU_NOTE_WIDTH, lines: MENU_NOTE_LINES }
            }
        };
    }
    // Its words scroll past `MENU_NOTE_LINES`, as a card's do past the
    // window: the card is held to that height and its words give way, which
    // is [`Card::capped`]'s way. A height on the words alone left the card as
    // tall as all of them, and the rest of the menu off the window.
    let capped = note_lines(&note, MENU_NOTE_WIDTH - 24.0) > MENU_NOTE_LINES;
    let (text_class, tallest) = if capped {
        (
            "note-card-text scrolls",
            format!(" max-height: {}px;", menu_note_tallest()),
        )
    } else {
        ("note-card-text", String::new())
    };
    // As wide as the card in it makes it, so the field lands on the card.
    let wide = if note.is_empty() || comment {
        String::new()
    } else {
        format!(" width: {CONTEXT_WIDTH}px; box-sizing: border-box;")
    };
    rsx! {
    div {
        // **The mark's one menu**, whichever button opened it,
        // so it is dressed as the right-click menu is: what can
        // be done to it, one row under another.
        class: "menu mark-popover",
        role: "menu",
        // The same rule the swatches have, and for the same
        // reason: a press in here must not reach the page and
        // begin a sweep of its own — which would take this very
        // popover down again on the way.
        onmousedown: move |event| event.stop_propagation(),
        style: "position: absolute; top: {top}px; left: {left}px;{wide}",
        // **What it says, as its card beside the page says it**, above what
        // can be done to it — unless it was asked over that card. A click
        // on it writes in it, as Edit does.
        if !note.is_empty() && !comment {
            div {
                class: "note-card mark-note",
                style: "border-color: {on_page(&colour)};{tallest}",
                onclick: move |_| viewer.write().begin_comment(),
                onwheel: move |event| if capped { event.stop_propagation() },
                if !said.is_empty() {
                    div { class: "note-card-by", "{said}" }
                }
                div { class: text_class, "{note}" }
                div { class: "note-card-actions",
                    button {
                        class: "chip action primary note-card-edit",
                        onclick: move |event| {
                            event.stop_propagation();
                            viewer.write().begin_comment();
                        },
                        "Edit"
                    }
                }
            }
        }
        // **The six again, the one it is in ringed**: a mark in
        // the wrong colour was a removal and a new sweep.
        div { class: "mark-row",
        for choice in colours.iter() {
            button {
                key: "{choice}",
                class: if choice.eq_ignore_ascii_case(&colour) { "mark-swatch on" } else { "mark-swatch" },
                "data-colour": "{choice}",
                "aria-label": "Change to {choice}",
                style: "background: {on_page(choice)};",
                onclick: {
                    let (key, choice) = (key.clone(), choice.clone());
                    move |_| {
                        viewer.write().recolour_markup(&key, &choice);
                    }
                },
            }
        }
        }
        div { class: "menu-rule" }
        if comment {
        button {
            class: "menu-item",
            "data-item": "edit",
            onclick: move |_| viewer.write().begin_comment(),
            Icon { name: "comment", stroke: ink.clone() }
            span { class: "menu-label", "Edit…" }
        }
        button {
            class: "menu-item",
            "data-item": "copy-comment",
            onclick: {
                let (clip, note) = (clip.clone(), note.clone());
                move |_| {
                    viewer.write().close_mark();
                    viewer.write().notice = clip.copy(&note, "Comment copied.");
                }
            },
            Icon { name: "copy", stroke: ink.clone() }
            span { class: "menu-label", "Copy comment" }
        }
        button {
            class: "menu-item",
            "data-item": "recolour",
            onclick: move |_| viewer.write().open_markup_colours(),
            Icon { name: "theme", stroke: ink.clone() }
            span { class: "menu-label", "Change colour…" }
        }
        button {
            class: "menu-item",
            "data-item": "uncomment",
            onclick: move |_| viewer.write().remove_comment(),
            Icon { name: "close", stroke: ink.clone() }
            span { class: "menu-label", "Remove comment" }
        }
        } else {
        // Copy is the selection's where there is one, and the
        // whole mark's where there is not.
        button {
            class: "menu-item",
            "data-item": "copy",
            onclick: {
                let (clip, key, selecting) = (clip.clone(), key.clone(), selected.is_some());
                move |_| {
                    viewer.write().close_mark();
                    if selecting {
                        copy_selection(viewer, &clip);
                    } else {
                        let quote = viewer.read().mark_quote(&key);
                        viewer.write().notice = clip.copy(&quote, "Copied.");
                    }
                }
            },
            Icon { name: "copy", stroke: ink.clone() }
            span { class: "menu-label", "Copy" }
        }
        if selected.is_some() {
            button {
                class: "menu-item",
                "data-item": "copy-quote",
                onclick: {
                    let clip = clip.clone();
                    move |_| {
                        viewer.write().close_mark();
                        copy_quote(viewer, &clip);
                    }
                },
                Icon { name: "copy", stroke: ink.clone() }
                span { class: "menu-label", "Copy with page number" }
            }
            button {
                class: "menu-item",
                "data-item": "highlight",
                onclick: move |_| { viewer.write().open_markup(); },
                Icon { name: "edit", stroke: ink.clone() }
                span { class: "menu-label", "Highlight…" }
            }
        }
        // A comment is edited in its card, above.
        if note.is_empty() && commentable {
            button {
                class: "menu-item mark-comment",
                "data-item": "comment",
                onclick: move |_| viewer.write().begin_comment(),
                Icon { name: "comment", stroke: ink.clone() }
                span { class: "menu-label", "Comment…" }
            }
        }
        // The window that edits the six, over this menu, which
        // stays up to take whichever of them is wanted.
        button {
            class: "menu-item",
            "data-item": "recolour",
            onclick: move |_| viewer.write().open_markup_colours(),
            Icon { name: "theme", stroke: ink.clone() }
            span { class: "menu-label", "Change colour…" }
        }
        if !note.is_empty() {
            button {
                class: "menu-item mark-uncomment",
                "data-item": "uncomment",
                onclick: move |_| viewer.write().remove_comment(),
                Icon { name: "close", stroke: ink.clone() }
                span { class: "menu-label", "Remove comment" }
            }
        }
        button {
            class: "menu-item mark-remove",
            "data-item": "remove",
            onclick: move |_| {
                if !viewer.write().remove_markup(&key) {
                    viewer.write().close_mark();
                }
            },
            Icon { name: "trash", stroke: ink.clone() }
            span { class: "menu-label", "Remove highlight" }
        }
        if let Some(quoted) = selected.clone() {
            div { class: "menu-rule" }
            button {
                class: "menu-item",
                "data-item": "find",
                onclick: move |_| {
                    viewer.write().close_mark();
                    let token = viewer.write().find_selected();
                    rescan(viewer, token);
                },
                Icon { name: "search", stroke: ink.clone() }
                span { class: "menu-label", "Search for “{quoted}”" }
            }
        }
        }
    }
    }
}

/// The menu a right-click on the document puts up, at the pointer. See
/// [`Viewer::open_context`] and [`Over`].
///
/// **Leaving full screen comes first** where there is no selection or mark
/// to be about: with nothing else on screen, a right-click is what a reader
/// tries, and the way out is the thing they came for.
#[allow(clippy::too_many_arguments)]
pub(super) fn context_menu(
    mut viewer: Signal<Viewer>,
    context: Context,
    reveal: &Reveal,
    printer: &Printer,
    clip: &Clip,
    frame: &Frame,
    pick: &Pick,
    away: &Away,
    ink: &str,
) -> Element {
    let ink = ink.to_string();
    let Context { at, page, on, over } = context;
    let held = viewer.read();
    let key = |action| held.chord_for(action);
    let (key_copy, key_quote, key_markup, key_full, key_mark, key_print) = (
        key(Action::Copy),
        key(Action::CopyQuote),
        key(Action::Markup),
        key(Action::Fullscreen),
        key(Action::Mark),
        key(Action::Print),
    );
    let leave = held.presenting || held.full_screen;
    let commentable = held.standing.into_file;
    let quoted = if over == Over::Selection {
        held.find_label()
    } else {
        String::new()
    };
    // Kept inside the window: flipped above the pointer where it would run
    // off the bottom, pulled left where it would run off the right. In
    // `.body`'s space, which starts under the chrome.
    let (rows, rules) = match over {
        Over::Selection => (4.0 + f64::from(u8::from(commentable)), 1.0),
        Over::Link(_) => (10.0, 3.0),
        Over::Page => (8.0, 2.0),
    };
    let tall = rows * MENU_ROW + rules * MENU_RULE + 14.0;
    let (wide, high) = (held.window_width, held.layout.viewport.height);
    // Two pixels off the pointer, so that the release of a ⌃-click is not
    // a click on the first row.
    let (x, y) = (at.0 + 2.0, at.1 - held.chrome() + 2.0);
    let facts = DocumentFacts::of(&held, Some((page, on)));
    drop(held);
    let left = x.min(wide - CONTEXT_WIDTH - 8.0).max(8.0);
    let top = if y + tall > high - 8.0 {
        (y - tall - 4.0).max(8.0)
    } else {
        y
    };
    let doing = {
        let (frame, clip, pick, printer) =
            (frame.clone(), clip.clone(), pick.clone(), printer.clone());
        move |action| {
            let mut closing = viewer;
            closing.write().close_menu();
            perform(viewer, action, 0.0, &frame, &clip, &pick, &printer);
        }
    };
    let items = match over {
        Over::Selection => rsx! {
            button {
                class: "menu-item",
                "data-item": "copy",
                onclick: {
                    let clip = clip.clone();
                    move |_| {
                        viewer.write().close_menu();
                        copy_selection(viewer, &clip);
                    }
                },
                Icon { name: "copy", stroke: ink.clone() }
                span { class: "menu-label", "Copy" }
                span { class: "menu-key", "{key_copy}" }
            }
            button {
                class: "menu-item",
                "data-item": "copy-quote",
                onclick: {
                    let doing = doing.clone();
                    move |_| doing(Action::CopyQuote)
                },
                Icon { name: "copy", stroke: ink.clone() }
                span { class: "menu-label", "Copy with page number" }
                span { class: "menu-key", "{key_quote}" }
            }
            button {
                class: "menu-item",
                "data-item": "highlight",
                onclick: move |_| {
                    viewer.write().close_menu();
                    viewer.write().open_markup();
                },
                Icon { name: "edit", stroke: ink.clone() }
                span { class: "menu-label", "Highlight…" }
                span { class: "menu-key", "{key_markup}" }
            }
            // Written in the popover the swatches come up in, as its own
            // Comment is: the passage becomes a mark when the words are done.
            if commentable {
                button {
                    class: "menu-item",
                    "data-item": "comment",
                    onclick: move |_| {
                        let mut held = viewer.write();
                        held.close_menu();
                        if held.open_markup() {
                            held.begin_comment();
                        }
                    },
                    Icon { name: "comment", stroke: ink.clone() }
                    span { class: "menu-label", "Comment…" }
                }
            }
            div { class: "menu-rule" }
            button {
                class: "menu-item",
                "data-item": "find",
                onclick: move |_| {
                    let token = viewer.write().find_selected();
                    rescan(viewer, token);
                },
                Icon { name: "search", stroke: ink.clone() }
                span { class: "menu-label", "Search for “{quoted}”" }
            }
        },
        Over::Link(_) | Over::Page => {
            let link = match &over {
                Over::Link(url) => Some(url.clone()),
                _ => None,
            };
            rsx! {
                if let Some(url) = link {
                    button {
                        class: "menu-item",
                        "data-item": "open-link",
                        onclick: {
                            let (away, url) = (away.clone(), url.clone());
                            move |_| {
                                viewer.write().close_menu();
                                let away_to = viewer.write().follow(&Target::Away(url.clone()));
                                if let Some(url) = away_to {
                                    away.open(&url);
                                }
                            }
                        },
                        Icon { name: "link", stroke: ink.clone() }
                        span { class: "menu-label", "Open link" }
                    }
                    button {
                        class: "menu-item",
                        "data-item": "copy-link",
                        onclick: {
                            let clip = clip.clone();
                            move |_| {
                                viewer.write().close_menu();
                                viewer.write().notice = clip.copy(&url, "Link copied.");
                            }
                        },
                        Icon { name: "copy", stroke: ink.clone() }
                        span { class: "menu-label", "Copy link" }
                    }
                    div { class: "menu-rule" }
                }
                button {
                    class: "menu-item",
                    "data-item": "full-screen",
                    onclick: {
                        let doing = doing.clone();
                        move |_| doing(Action::Fullscreen)
                    },
                    Icon { name: "fullscreen", stroke: ink.clone() }
                    span { class: "menu-label", if leave { "Exit full screen" } else { "Full screen" } }
                    span { class: "menu-key", "{key_full}" }
                }
                div { class: "menu-rule" }
                {document_items(viewer, reveal, printer, clip, &ink, &key_mark, &key_print, Some((page, on)), &facts)}
            }
        }
    };
    rsx! {
        div { class: "menu context", role: "menu", "aria-label": "Actions",
            onmousedown: move |event| event.stop_propagation(),
            style: "top: {top}px; left: {left}px;",
            {items}
        }
    }
}

/// ⌘C, and the Copy in the popover under a selection.
pub(super) fn copy_selection(mut viewer: Signal<Viewer>, clip: &Clip) {
    let copied = viewer.read().selected_text();
    if copied.is_empty() {
        viewer.write().notice = "Select something first, and this copies it.".into();
    } else {
        viewer.write().notice = clip.copy(&copied, "Copied.");
    }
}

pub(super) fn copy_quote(mut viewer: Signal<Viewer>, clip: &Clip) {
    let quoted = viewer.read().quoted();
    viewer.write().notice = match quoted {
        Some((quote, where_from)) => clip.copy(&quote, &format!("Copied, with {where_from}.")),
        None => "Select something first, and this copies it with its page number.".into(),
    };
}
