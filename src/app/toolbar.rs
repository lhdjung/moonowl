//! The toolbar, and the find card that hangs off its Search chip.
//!
//! **A component of its own so that a scroll frame is not a toolbar render.**
//! `Reader` renders on every write to the viewer, which is every frame of a
//! scroll; the toolbar reads [`Bar`], a memo of the few dozen things it shows,
//! and renders only when one of them changes. Its hooks are `Reader`'s, handed
//! down: a door taken from context here would miss `Reader`'s fallbacks.

use super::*;

/// Everything the toolbar shows, read off the viewer in one go. Compared
/// after every write, so a write that changes none of it renders nothing.
#[derive(Clone, PartialEq)]
pub(super) struct Bar {
    empty: bool,
    sidebar_open: bool,
    find_open: bool,
    /// Whether the chips have lost their words — the first `@media` step in
    /// `styles.rs`, whose 1200 this is — and the find card would run out of
    /// the window if it hung off its chip.
    bar_tight: bool,
    menu: Option<Menu>,
    /// How tall the long menus may be: the window's own height, handed down,
    /// rather than `100vh`, which Stylo can leave stale after a move to a
    /// screen of another density (see `html, body, #main` in the sheet).
    menu_reach: f64,
    /// The shelf. It is a file on the disk, so it is read when the Open menu
    /// is open and not otherwise.
    recents: Vec<crate::store::Recent>,
    /// The title button exists only where there is a document, so this is the
    /// document's name and nothing else.
    shelf_name: String,
    typing_page: bool,
    page_field: String,
    page_box: f64,
    page_pad: f64,
    own_numbering: bool,
    /// What follows "of", which is not always the number of pages. See
    /// [`Viewer::pages_text`].
    pages_text: String,
    numbering_printed: bool,
    numbering_as_printed: String,
    numbering_by_position: String,
    fit: Fit,
    zoom: String,
    zoom_now: f64,
    shown_percent: f64,
    actual_100: bool,
    theme_name: String,
    /// Each theme's name, the two colours its swatch is drawn in, and whether
    /// it is one the reader wrote. Read through `palette` rather than handed
    /// raw to CSS: a swatch that shows a colour the renderer cannot read is
    /// the one place in the app meant to show what you are about to get
    /// lying about it — the app's own finding, in `ui.swatch`.
    theme_rows: Vec<(String, String, String, bool)>,
    theme_index: usize,
    worn_built_in: bool,
    dark_now: bool,
    following: bool,
    /// A machine that reports no appearance leaves the switch nothing to do,
    /// and says so rather than looking broken.
    unfollowable: bool,
    /// The switch in the Settings menu, which has to keep saying what the
    /// reader chose while the page field borrows the bar.
    toolbar_set: bool,
    full_screen: bool,
    scroll_mode: crate::layout::Mode,
    page_pill: bool,
    /// What an icon is drawn in: the quiet shade, the accent for a chip whose
    /// thing is in force, the faint shade, and the cross on Close under the
    /// pointer. Strings rather than classes — see [`Icon`].
    ink: String,
    ink_on: String,
    faint: String,
    danger: String,
    key_open: String,
    key_new_window: String,
    key_new_tab: String,
    key_mark: String,
    key_print: String,
    key_fit_width: String,
    key_fit_page: String,
    key_actual: String,
    key_dark: String,
    key_toolbar: String,
    key_fullscreen: String,
    key_present: String,
    key_settings: String,
    key_help: String,
    key_rotate_left: String,
    key_rotate_right: String,
}

impl Bar {
    fn of(held: &Viewer) -> Bar {
        let wearing = held.palette();
        let page_field = held.page_field();
        let digits = page_field.chars().count() as f64;
        // How wide the page box is: padding, border and the number in it,
        // with a floor so page 1 of a pamphlet is not a slot. Blitz cannot
        // centre an input's text, so the box is made to fit rather than the
        // text made to sit in the middle of it.
        //
        // **The floor is the app's own 44px**, not the smallest box a digit
        // fits in: a narrower floor makes page 1 a slot half the size of the
        // count beside it. Growing past 44 only happens at four digits.
        let page_box = (14.0 + 9.1 * digits).max(44.0);
        // **And where the number sits inside that box, which the box's width
        // alone does not settle.** `.page-field` says `text-align: center`
        // and Blitz ignores it on an input — `create_text_editor` calls
        // `editor.set_width(None)`, so the run starts at the leading edge.
        // The slack the floor leaves is split in half and paid as left
        // padding, which is the one thing Blitz does honour — so a one-digit
        // field is centred and a four-digit one, already fitted, is unchanged.
        let page_pad = 6.0 + ((page_box - 14.0 - 9.1 * digits) / 2.0).max(0.0);
        let (numbering_as_printed, numbering_by_position) = held.numbering_choices();
        // The remembered zoom, which is what the presets tick against, and
        // what is actually on screen, which is where the stepper starts. In a
        // fit mode those are different numbers — see [`Viewer::zoom_percent`].
        let zoom_now = held.layout.zoom * 100.0;
        Bar {
            empty: held.empty(),
            sidebar_open: held.sidebar_open,
            find_open: held.find_open,
            bar_tight: held.window_width <= 1200.0,
            menu: held.menu,
            menu_reach: (held.window_height - 62.0).max(120.0),
            recents: if held.menu == Some(Menu::Open) {
                held.recents()
            } else {
                Vec::new()
            },
            shelf_name: held.store.title().to_string(),
            typing_page: held.typing_page,
            page_field,
            page_box,
            page_pad,
            own_numbering: held.has_own_numbering(),
            pages_text: held.pages_text(),
            numbering_printed: held.numbering_printed(),
            numbering_as_printed,
            numbering_by_position,
            fit: held.layout.fit,
            zoom: match held.layout.fit {
                Fit::Width => "Fit width".to_string(),
                Fit::Page => "Fit page".to_string(),
                Fit::Actual => format!("{:.0}%", held.layout.zoom * 100.0),
            },
            zoom_now,
            // Whole percent, as the zoom notice says it: a pinch leaves
            // fractions nobody asked to read.
            shown_percent: held.zoom_percent().round(),
            // "Actual size" is a fit mode *and* a zoom of 1, so it is ticked
            // only when both are true — `showZoomMenu` asks the same two
            // questions.
            actual_100: held.layout.fit == Fit::Actual && (zoom_now - 100.0).abs() < 0.5,
            theme_name: held.theme_name(),
            theme_rows: held
                .store
                .themes()
                .iter()
                .map(|theme| {
                    let ink = crate::palette::read_colour(&theme.text).unwrap_or([0, 0, 0]);
                    let paper = if theme.recolor {
                        crate::palette::read_colour(&theme.background).unwrap_or([255, 255, 255])
                    } else {
                        [255, 255, 255]
                    };
                    (
                        theme.name.clone(),
                        crate::palette::hex(ink),
                        crate::palette::hex(paper),
                        !theme.built_in,
                    )
                })
                .collect(),
            theme_index: held.store.theme_index(),
            worn_built_in: held.store.theme().built_in,
            dark_now: held.store.dark_now(),
            following: held.store.flag("follow_system_theme"),
            unfollowable: held.store.outside().is_none(),
            toolbar_set: held.toolbar,
            full_screen: held.full_screen,
            scroll_mode: held.layout.mode,
            page_pill: held.page_pill(),
            ink: crate::palette::hex(wearing.muted()),
            ink_on: crate::palette::hex(wearing.accent),
            faint: crate::palette::hex(wearing.faint()),
            danger: crate::palette::hex(wearing.negative()),
            key_open: held.chord_for(Action::Open),
            key_new_window: held.chord_for(Action::NewWindow),
            key_new_tab: held.chord_for(Action::NewTab),
            key_mark: held.chord_for(Action::Mark),
            key_print: held.chord_for(Action::Print),
            key_fit_width: held.chord_for(Action::FitWidth),
            key_fit_page: held.chord_for(Action::FitPage),
            key_actual: held.chord_for(Action::ActualSize),
            key_dark: held.chord_for(Action::Dark),
            key_toolbar: held.chord_for(Action::Toolbar),
            key_fullscreen: held.chord_for(Action::Fullscreen),
            key_present: held.chord_for(Action::Present),
            key_settings: held.chord_for(Action::Settings),
            key_help: held.chord_for(Action::Help),
            key_rotate_left: held.chord_for(Action::RotateLeft),
            key_rotate_right: held.chord_for(Action::RotateRight),
        }
    }
}

#[component]
pub(super) fn Toolbar(
    viewer: Signal<Viewer>,
    frame: Frame,
    pick: Pick,
    printer: Printer,
    reveal: Reveal,
    clip: Clip,
) -> Element {
    crate::stats::add(&crate::stats::TOOLBAR_RENDERS, 1);
    let bar = use_memo(move || Bar::of(&viewer.read()));
    let Bar {
        empty,
        sidebar_open,
        find_open,
        bar_tight,
        menu,
        menu_reach,
        recents,
        shelf_name,
        typing_page,
        page_field,
        page_box,
        page_pad,
        own_numbering,
        pages_text,
        numbering_printed,
        numbering_as_printed,
        numbering_by_position,
        fit,
        zoom,
        zoom_now,
        shown_percent,
        actual_100,
        theme_name,
        theme_rows,
        theme_index,
        worn_built_in,
        dark_now,
        following,
        unfollowable,
        toolbar_set,
        full_screen,
        scroll_mode,
        page_pill,
        ink,
        ink_on,
        faint,
        danger,
        key_open,
        key_new_window,
        key_new_tab,
        key_mark,
        key_print,
        key_fit_width,
        key_fit_page,
        key_actual,
        key_dark,
        key_toolbar,
        key_fullscreen,
        key_present,
        key_settings,
        key_help,
        key_rotate_left,
        key_rotate_right,
    } = bar();
    // The window, for the two switches in the Settings menu that ask it to go
    // full screen.
    let full_screen_frame = frame.clone();
    let full_screen_label_frame = frame.clone();

    rsx! {
        div { class: "toolbar",
            // **Everything in this bar that is about a document is gone
            // when there is none**, which the app does by selector. What is
            // left is what is still true of an empty window: how to put
            // something in it, and what it looks like.
            //
            // **Three groups, and the middle one is why.** A flat row with
            // a spacer put the page readout wherever the row ran out of
            // chips, which was the far right. `.bar-left` gives way because
            // a title has somewhere to shrink to, `.bar-center` never gives
            // way, and `.bar-right` grows against the left so the two sides
            // share the slack.
            div { class: "bar-group bar-left",
                if !empty {
                button {
                    // Not `.sidebar`, which is the panel itself: a selector
                    // that matches the button *and* the thing the button
                    // opens is a test that cannot tell them apart.
                    class: if sidebar_open { "chip contents on" } else { "chip contents" },
                    // A name for when it is folded to its symbol: Blitz draws no
                    // tooltip, so `title` names nothing. The same on each chip
                    // below that folds.
                    "aria-label": "Sidebar",
                    // The sidebar is `opens(…)` in `main.ts` for the same
                    // reason the five menus are — see `show_menu`. The
                    // *keyboard* action is not, there or here: a shortcut
                    // asked for the panel and said nothing about the
                    // search. A panel the search borrowed goes down with
                    // the bar, which is the whole of what this press asked.
                    onclick: move |_| {
                        let mut viewer = viewer.write();
                        let borrowed = viewer.results_borrowed;
                        viewer.close_find();
                        if !borrowed {
                            viewer.toggle_sidebar();
                        }
                    },
                    Icon { name: "contents", stroke: if sidebar_open { ink_on.clone() } else { ink.clone() } }
                    span { class: "chip-label", "Sidebar" }
                }
                }
                // **The way to another document, which is not the same
                // question as what can be done with this one.** The picker,
                // a second window and the shelf all answer "open
                // something"; a mark, a highlight and a print belong to the
                // document already on screen. Both hanging off the title
                // meant a menu opened by pressing the name of the paper you
                // are reading, four of whose items are about papers you are
                // not.
                //
                // **Every menu hangs inside an anchor of its own, and that
                // is the whole of where a menu appears.** An absolutely
                // positioned child of a `position: relative` wrapper needs
                // no measurement, and the engine keeps it in step — where a
                // single layer pinned to the toolbar's ends did not. Out of
                // the flow, so the 46px row stays 46px whatever hangs off
                // it.
                div { class: "anchor",
                    button {
                        class: if menu == Some(Menu::Open) { "chip open on" } else { "chip open" },
                        "aria-label": "Open",
                        onmousedown: move |event| event.stop_propagation(),
                        onclick: move |_| viewer.write().show_menu(Menu::Open),
                        Icon {
                            name: "folder",
                            stroke: if menu == Some(Menu::Open) { ink_on.clone() } else { ink.clone() },
                        }
                        span { class: "chip-label", "Open…" }
                    }
                    if menu == Some(Menu::Open) {
                        div { class: "menu open", role: "menu", "aria-label": "Open",
                            style: "max-height: {menu_reach}px;",
                            // A press inside a menu is not a press outside it: the root
                            // puts the menu away, and the item's own click comes after
                            // the press. This was on the layer these three used to share.
                            onmousedown: move |event| event.stop_propagation(),
                            button {
                                class: "menu-item",
                                "data-item": "open",
                                onclick: {
                                    let pick = pick.clone();
                                    move |_| {
                                        viewer.write().close_menu();
                                        pick.ask(Opening::Here);
                                    }
                                },
                                Icon { name: "folder", stroke: ink.clone() }
                                span { class: "menu-label", "Open document…" }
                                span { class: "menu-key", "{key_open}" }
                            }
                            if cfg!(target_os = "macos") {
                                button {
                                    class: "menu-item",
                                    "data-item": "open-in-tab",
                                    onclick: {
                                        let pick = pick.clone();
                                        move |_| {
                                            viewer.write().close_menu();
                                            pick.ask(Opening::InTab);
                                        }
                                    },
                                    Icon { name: "tab", stroke: ink.clone() }
                                    span { class: "menu-label", "Open document in new tab…" }
                                }
                            }
                            // The two-documents-at-once route in one step:
                            // pick the second and it arrives beside the
                            // first rather than on top of it. A menu item
                            // and not a key, because it is not a key in
                            // `keys.ts` either.
                            button {
                                class: "menu-item",
                                "data-item": "open-beside",
                                onclick: {
                                    let pick = pick.clone();
                                    move |_| {
                                        viewer.write().close_menu();
                                        pick.ask(Opening::Beside);
                                    }
                                },
                                Icon { name: "windows", stroke: ink.clone() }
                                span { class: "menu-label", "Open document in new window…" }
                            }
                            // **What ⌘N used to do by accident.** macOS
                            // turns a new window into a tab of its own
                            // accord while the app is full screen, which
                            // is a good thing to be able to ask for and a
                            // bad thing to be given — so it is switched
                            // off (see `tabs.rs`) and said here instead.
                            // macOS alone has tabs, so the item is there
                            // alone.
                            if cfg!(target_os = "macos") {
                                button {
                                    class: "menu-item",
                                    "data-item": "new-tab",
                                    onclick: {
                                        let frame = frame.clone();
                                        move |_| {
                                            viewer.write().close_menu();
                                            frame.ask(Ask::NewTab);
                                        }
                                    },
                                    Icon { name: "tabPlus", stroke: ink.clone() }
                                    span { class: "menu-label", "New tab" }
                                    span { class: "menu-key", "{key_new_tab}" }
                                }
                            }
                            button {
                                class: "menu-item",
                                onclick: {
                                    let frame = frame.clone();
                                    move |_| {
                                        viewer.write().close_menu();
                                        frame.ask(Ask::NewWindow);
                                    }
                                },
                                Icon { name: "windowPlus", stroke: ink.clone() }
                                span { class: "menu-label", "New window" }
                                span { class: "menu-key", "{key_new_window}" }
                            }
                            // And the shelf, which is the same list the
                            // start screen shows. It is here for the reader
                            // who has a document open: the start screen is
                            // unreachable without first putting that one
                            // down.
                            if !recents.is_empty() {
                                div { class: "menu-rule" }
                                div { class: "menu-section", "Recently read" }
                                for entry in recents.iter().cloned() {
                                    button {
                                        key: "{entry.path}",
                                        class: "menu-item",
                                        "data-item": "recent",
                                        onclick: {
                                            let frame = frame.clone();
                                            let path = entry.path.clone();
                                            move |_| {
                                                let path = path.clone();
                                                viewer.write().close_menu();
                                                if viewer.write().open_here(&path) {
                                                    let title = viewer
                                                        .read()
                                                        .store
                                                        .title()
                                                        .to_string();
                                                    frame.ask(Ask::Showing { path, title });
                                                }
                                            }
                                        },
                                        // The middle button opens it
                                        // beside this one, a tab or a
                                        // window as "Open documents in
                                        // tabs" says. On the release, as
                                        // a browser's middle click is;
                                        // Blitz makes no `auxclick`.
                                        onmouseup: {
                                            let frame = frame.clone();
                                            let path = entry.path.clone();
                                            move |event: MouseEvent| {
                                                if event.trigger_button() != Some(dioxus::html::input_data::MouseButton::Auxiliary) {
                                                    return;
                                                }
                                                event.stop_propagation();
                                                viewer.write().close_menu();
                                                frame.ask(Ask::SendOn(path.clone()));
                                            }
                                        },
                                        // A drawing, quieter than the
                                        // ones above the rule, so the
                                        // section reads as a shelf rather
                                        // than as four more commands. One
                                        // drawing and not two: it used to
                                        // sit in a tick column of its own
                                        // beside a second icon, which drew
                                        // a faint sheet of paper and a
                                        // bright one saying the same thing
                                        // about the same file.
                                        Icon { name: "document", stroke: faint.clone() }
                                        span { class: "menu-label", "{entry.title}" }
                                        span { class: "menu-key", "p. {entry.label}" }
                                    }
                                }
                            }
                        }
                    }
                }
                // The two window verbs, in the bar only while there is
                // nothing else in it: a window with a document has better
                // things to spend the room on, and one with none has ⌘N
                // and ⌘W and no way of knowing it.
                if empty {
                button {
                    class: "chip new-window",
                    "aria-label": "New window",
                    onclick: {
                        let frame = frame.clone();
                        move |_| frame.ask(Ask::NewWindow)
                    },
                    Icon { name: "windowPlus", stroke: ink.clone() }
                    span { class: "chip-label", "New window" }
                }
                button {
                    class: "chip close-window",
                    "aria-label": "Close window",
                    onclick: {
                        let frame = frame.clone();
                        move |_| frame.ask(Ask::Close)
                    },
                    Icon { name: "close", stroke: ink.clone(), class: "rest" }
                    Icon { name: "close", stroke: danger.clone(), class: "hot" }
                    span { class: "chip-label", "Close window" }
                }
                }
                if !empty {
                // The gesture the app calls Close, and the one place a
                // document can be put down without the window going with
                // it. A button rather than a menu item: a reader looking
                // for how to put a document down does not look inside a
                // menu named after the document.
                button {
                    class: "chip close-doc",
                    "aria-label": "Close",
                    "data-item": "close-document",
                    onclick: {
                        let frame = frame.clone();
                        move |_| {
                            viewer.write().close_menu();
                            viewer.write().close_document();
                            // The desk, the restore list and the document
                            // watch all belong to the process, and an
                            // empty path is how this window says it is
                            // showing none. See [`Ask::Showing`].
                            frame.ask(Ask::Showing {
                                path: String::new(),
                                title: String::new(),
                            });
                        }
                    },
                    Icon { name: "close", stroke: ink.clone(), class: "rest" }
                    Icon { name: "close", stroke: danger.clone(), class: "hot" }
                    span { class: "chip-label", "Close" }
                }
                // What the document is called — its own `/Title` where
                // that is worth having and the file's name where it is not,
                // see `store::worth_calling` — and the button its menu
                // hangs off.
                // The extra air the app puts in front of the name lives on
                // the anchor rather than on the button, so that the menu
                // still comes down flush with the button it belongs to.
                div { class: "anchor titled",
                    button {
                        class: if menu == Some(Menu::Document) { "chip title on" } else { "chip title" },
                        onmousedown: move |event| event.stop_propagation(),
                        onclick: move |_| viewer.write().show_menu(Menu::Document),
                        // **No icon.** This is the one thing in the bar
                        // that is not a verb, and an icon in front of a
                        // file name says nothing the name does not — while
                        // costing it twenty-three pixels in a bar with none
                        // to spare.
                        span { class: "title-name", "{shelf_name}" }
                    }
                    if menu == Some(Menu::Document) {
                        div { class: "menu document", role: "menu", "aria-label": "Document",
                            style: "max-height: {menu_reach}px;",
                            onmousedown: move |event| event.stop_propagation(),
                            {document_items(viewer, &reveal, &printer, &clip, &ink, &key_mark, &key_print, None)}
                        }
                    }
                }
                }
            }
            // The middle of the bar, and the only thing in it: where you
            // are, with a step either side. A page is turned by a key or
            // by scrolling far more often than by pressing an arrow, but
            // the pair is what makes the number between them read as a
            // control rather than a readout.
            div { class: "bar-group bar-center",
                if !empty {
                button {
                    class: "chip page-previous",
                    "aria-label": "Previous page",
                    onclick: move |_| viewer.write().previous_page(),
                    Icon { name: "up", stroke: ink.clone() }
                }
                // The page field, which is a field rather than a readout for
                // the app's own reason: stepping is fine for nudging and
                // hopeless for arriving, and a reader with a citation in
                // front of them has a number to type. What it shows is the
                // page's *label* — see [`Viewer::label`].
                // **The box is the width of the number in it, and that is a
                // workaround wearing a design's clothes.** `text-align:
                // center` on an input does nothing in Blitz:
                // `create_text_editor` never gives parley an alignment and
                // calls `editor.set_width(None)`, so there is no box to
                // align within. Making the box fit its contents is the
                // available answer and the better one — the field grows as
                // digits are typed. The button and the field take the same
                // width, so opening the field moves nothing.
                div { class: "pill",
                    // **A readout that becomes a field, rather than a field
                    // that is always one** — Blitz's focus rule decides it,
                    // not taste. The keyboard goes to the innermost element
                    // asking for it, so a field always in the toolbar either
                    // always asks (and every keystroke goes into it) or stops
                    // asking while still holding the focus, which is the same
                    // dead keyboard. The find bar's field *stops existing*
                    // when the bar closes; this is that, borrowed.
                    if typing_page {
                    input {
                        class: "page-field",
                        style: "width: {page_box}px; padding-left: {page_pad}px;",
                        r#type: "text",
                        value: "{page_field}",
                        // Not the root's business: see its `onmousedown`.
                        onmousedown: move |event| event.stop_propagation(),
                        "aria-label": "Go to page",
                        "data-keyboard": "goto",
                        // Arrives with the number selected, so typing
                        // replaces it. See [`place_carets`].
                        "data-caret": "all",
                        onmounted: move |event| {
                            let node = event.data();
                            let task = node.set_focus(true);
                            spawn(async move { let _ = task.await; });
                        },
                        oninput: move |event| viewer.write().type_page(&event.value()),
                        // As the find field: a plain key typed here would
                        // otherwise bubble to the root and scroll the
                        // document. See [`field_keeps`].
                        onkeydown: move |event| {
                            match event.key() {
                                Key::Enter => {
                                    event.stop_propagation();
                                    viewer.write().commit_page();
                                }
                                // **A menu is outermost and this field owns
                                // the keyboard.** The app puts the ordering
                                // in one capturing handler; here the keyboard
                                // belongs to the innermost element asking, so
                                // a field that has it must defer to the menu
                                // itself. `Action::Dismiss` is the same list
                                // for when no field has the keyboard.
                                Key::Escape => {
                                    event.stop_propagation();
                                    if !viewer.write().close_menu() {
                                        viewer.write().cancel_page();
                                    }
                                }
                                _ => field_keeps(&event),
                            }
                        },
                    }
                    } else {
                    button {
                        class: "page-now",
                        style: "width: {page_box}px;",
                        "aria-label": "Go to page",
                        onclick: move |_| viewer.write().open_page_field(),
                        "{page_field}"
                    }
                    }
                    // **The count is a menu where the document numbers
                    // itself.** "407 of 425" and "1 of 19" are both true of
                    // an offprint, and which a reader wants depends on
                    // whether they hold a citation or a thumb: the choice
                    // hangs off the count, which is the thing it changes.
                    if own_numbering {
                        div { class: "anchor",
                            button {
                                class: if menu == Some(Menu::Numbering) { "of choice on" } else { "of choice" },
                                "aria-label": "Page numbers",
                                onmousedown: move |event| event.stop_propagation(),
                                onclick: move |_| viewer.write().show_menu(Menu::Numbering),
                                "of {pages_text}"
                            }
                            if menu == Some(Menu::Numbering) {
                                div { class: "menu numbering", role: "menu", "aria-label": "Page numbers",
                                    onmousedown: move |event| event.stop_propagation(),
                                    div { class: "menu-section", "Page numbers" }
                                    button {
                                        class: if numbering_printed { "menu-item on" } else { "menu-item" },
                                        onclick: move |_| { viewer.write().set_page_numbering(true); viewer.write().close_menu(); },
                                        span { class: "menu-tick", if numbering_printed { Icon { name: "check", stroke: ink_on.clone() } } }
                                        span { class: "menu-label", "{numbering_as_printed}" }
                                        span { class: "menu-key", "As printed" }
                                    }
                                    button {
                                        class: if !numbering_printed { "menu-item on" } else { "menu-item" },
                                        onclick: move |_| { viewer.write().set_page_numbering(false); viewer.write().close_menu(); },
                                        span { class: "menu-tick", if !numbering_printed { Icon { name: "check", stroke: ink_on.clone() } } }
                                        span { class: "menu-label", "{numbering_by_position}" }
                                        span { class: "menu-key", "Count from 1" }
                                    }
                                }
                            }
                        }
                    } else {
                        span { class: "of", "of {pages_text}" }
                    }
                }
                button {
                    class: "chip page-next",
                    "aria-label": "Next page",
                    onclick: move |_| viewer.write().next_page(),
                    Icon { name: "down", stroke: ink.clone() }
                }
                }
            }
            div { class: "bar-group bar-right",
                if !empty {
                // The app's `#find`, which this bar did not have: ⌘F was
                // the only way in, and a shortcut is not a way in for
                // somebody who does not already know it is there.
                // A second click puts the card away, as it does a menu.
                div { class: "anchor",
                    button {
                        class: if find_open { "chip find on" } else { "chip find" },
                        "aria-label": "Search",
                        onclick: move |_| {
                            if viewer.read().find_open {
                                viewer.write().close_find();
                            } else {
                                let token = viewer.write().open_find();
                                rescan(viewer, token);
                            }
                        },
                        Icon { name: "search", stroke: if find_open { ink_on.clone() } else { ink.clone() } }
                        span { class: "chip-label", "Search" }
                    }
                    if find_open && !bar_tight {
                        FindCard { viewer, place: "top: calc(100% + 8px); left: 0;" }
                    }
                }
                // **Left and Right, in the bar** and never behind a menu:
                // turning a page that came in sideways is one press, free,
                // which is a point about this reader — Acrobat Reader
                // meets the same wish with a paywall.
                button {
                    class: "chip rotate-left",
                    "aria-label": "Turn the page left",
                    title: "Turn the page left — {key_rotate_left}",
                    onclick: move |_| viewer.write().rotate(-1),
                    Icon { name: "rotateLeft", stroke: ink.clone() }
                    span { class: "chip-label", "Left" }
                }
                button {
                    class: "chip rotate-right",
                    "aria-label": "Turn the page right",
                    title: "Turn the page right — {key_rotate_right}",
                    onclick: move |_| viewer.write().rotate(1),
                    Icon { name: "rotateRight", stroke: ink.clone() }
                    span { class: "chip-label", "Right" }
                }
                }
                if !empty {
                // Two buttons that were a cycle and are now a list. The chip
                // still *says* what is in force, which is how the harness and
                // a reader both read it; what changed is that clicking shows
                // the choices instead of stepping to the next.
                //
                // Three in one sunk group, minus left and plus right. Three
                // quiet words in a row of quiet words read as three more
                // labels; a stepper with the readout between its ends reads as
                // one control, from the same three elements.
                div { class: "zoom-group",
                    button {
                        class: "chip zoom-out",
                        "aria-label": "Zoom out",
                        onclick: move |_| viewer.write().zoom(false),
                        Icon { name: "minus", stroke: ink.clone() }
                    }
                    div { class: "anchor",
                        button {
                            class: if menu == Some(Menu::View) { "chip fit on" } else { "chip fit" },
                            onmousedown: move |event| event.stop_propagation(),
                            onclick: move |_| viewer.write().show_menu(Menu::View),
                            "{zoom}"
                        }
                        if menu == Some(Menu::View) {
                            div { class: "menu view", role: "menu", "aria-label": "View",
                                style: "max-height: {menu_reach}px;",
                                // A press inside a menu is not a press outside it: the root
                                // puts the menu away, and the item's own click comes after
                                // the press. This was on the layer these three used to share.
                                onmousedown: move |event| event.stop_propagation(),
                                // **`showZoomMenu` in `main.ts`, item for
                                // item**: a number to type and the presets
                                // under it. Spread is in the settings menu
                                // and rotation is two buttons in the bar,
                                // which is where the app keeps them and is
                                // not what a reader pressing the zoom is
                                // asking about.
                                //
                                // Nothing in here puts the menu away: a zoom
                                // is something you try on, like a theme.
                                button {
                                    class: if fit == Fit::Width { "menu-item on" } else { "menu-item" },
                                    onclick: move |_| viewer.write().set_fit(Fit::Width),
                                    span { class: "menu-tick", if fit == Fit::Width { Icon { name: "check", stroke: ink_on.clone() } } }
                                    span { class: "menu-label", "Fit width" }
                                    span { class: "menu-key", "{key_fit_width}" }
                                }
                                button {
                                    class: if fit == Fit::Page { "menu-item on" } else { "menu-item" },
                                    onclick: move |_| viewer.write().set_fit(Fit::Page),
                                    span { class: "menu-tick", if fit == Fit::Page { Icon { name: "check", stroke: ink_on.clone() } } }
                                    span { class: "menu-label", "Fit page" }
                                    span { class: "menu-key", "{key_fit_page}" }
                                }
                                button {
                                    class: if actual_100 { "menu-item on" } else { "menu-item" },
                                    onclick: move |_| viewer.write().actual_size(),
                                    span { class: "menu-tick", if actual_100 { Icon { name: "check", stroke: ink_on.clone() } } }
                                    span { class: "menu-label", "Actual size" }
                                    span { class: "menu-key", "{key_actual}" }
                                }
                                div { class: "menu-rule" }
                                // The rest of the ladder, for the sizes the
                                // presets do not name. It starts from what is
                                // on the screen rather than the remembered
                                // zoom: in a fit mode those differ, and the
                                // one being looked at is the one to type
                                // over.
                                div { class: "menu-row",
                                    label { class: "menu-row-text",
                                        span { class: "menu-row-label", "Zoom to" }
                                    }
                                    crate::prefs::Stepper {
                                        value: shown_percent,
                                        min: 25.0,
                                        max: 600.0,
                                        step: 25.0,
                                        unit: "%".to_string(),
                                        // Applied once it is typed out rather
                                        // than on the way: 150 passes through
                                        // 1 and 15, and each is a relayout.
                                        live: false,
                                        onchange: move |value: f64| viewer.write().set_zoom(value / 100.0),
                                    }
                                }
                                for percent in [50.0_f64, 75.0, 100.0, 125.0, 150.0, 175.0, 200.0, 300.0] {
                                    {
                                        let on = fit == Fit::Actual && (zoom_now - percent).abs() < 0.5;
                                        rsx! {
                                            button {
                                                key: "{percent}",
                                                class: if on { "menu-item on" } else { "menu-item" },
                                                onclick: move |_| viewer.write().set_zoom(percent / 100.0),
                                                span { class: "menu-tick", if on { Icon { name: "check", stroke: ink_on.clone() } } }
                                                span { class: "menu-label", "{percent:.0}%" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    button {
                        class: "chip zoom-in",
                        "aria-label": "Zoom in",
                        onclick: move |_| viewer.write().zoom(true),
                        Icon { name: "plus", stroke: ink.clone() }
                    }
                }
                }
                div { class: "anchor",
                    button {
                        class: if menu == Some(Menu::Theme) { "chip theme on" } else { "chip theme" },
                        "aria-label": "Theme",
                        onmousedown: move |event| event.stop_propagation(),
                        onclick: move |_| viewer.write().show_menu(Menu::Theme),
                        // **"Theme", not the theme's name**: a button says
                        // what pressing it does, and the theme in force is a
                        // tick in the menu where the fourteen are. The
                        // harness reads it off `data-theme` instead, which
                        // is not a label anybody has to look at.
                        "data-theme": "{theme_name}",
                        Icon { name: "theme", stroke: if menu == Some(Menu::Theme) { ink_on.clone() } else { ink.clone() } }
                        span { class: "chip-label", "Theme" }
                    }
                    if menu == Some(Menu::Theme) {
                        div { class: "menu theme", role: "menu", "aria-label": "Theme",
                            style: "max-height: {menu_reach}px;",
                            // A press inside a menu is not a press outside it: the root
                            // puts the menu away, and the item's own click comes after
                            // the press. This was on the layer these three used to share.
                            onmousedown: move |event| event.stop_propagation(),
                            // **The two switches above the list.** Dark mode
                            // is a move between the pair the reader chose and
                            // following the machine does it for them, so both
                            // belong where the themes are rather than a
                            // window away on the Appearance page.
                            div { class: "menu-row",
                                label { class: "menu-row-text",
                                    onclick: move |_| viewer.write().set_dark(!dark_now),
                                    span { class: "menu-row-label", "Dark mode" }
                                    span { class: "menu-row-note", "{key_dark}" }
                                }
                                crate::prefs::Toggle {
                                    on: dark_now,
                                    onchange: move |on: bool| viewer.write().set_dark(on),
                                }
                            }
                            div { class: "menu-row",
                                label { class: "menu-row-text",
                                    onclick: move |_| viewer.write().set_follow_system(!following),
                                    span { class: "menu-row-label", "Follow the system" }
                                    if unfollowable {
                                        span { class: "menu-row-note", "This machine reports no appearance." }
                                    }
                                }
                                crate::prefs::Toggle {
                                    on: following,
                                    onchange: move |on: bool| viewer.write().set_follow_system(on),
                                }
                            }
                            div { class: "menu-rule" }
                            div { class: "menu-section", "Themes" }
                            // Nothing in here that only changes an
                            // appearance puts the menu away: a theme is
                            // something you try on, so the tick moves and
                            // the list stays. The app says the same in the
                            // same place.
                            for (index, theme) in theme_rows.iter().cloned().enumerate() {
                                {
                                    let (name, ink, paper, mine) = theme;
                                    rsx! {
                                        button {
                                            key: "{index}:{name}",
                                            class: if index == theme_index { "menu-item on" } else { "menu-item" },
                                            onclick: move |_| viewer.write().set_theme(index),
                                            span { class: "menu-tick", if index == theme_index { Icon { name: "check", stroke: ink_on.clone() } } }
                                            // Two letters of the theme, in
                                            // its own colours, read through
                                            // `parseColor`: a swatch that
                                            // hands a raw string to CSS shows
                                            // a colour the renderer cannot
                                            // read.
                                            span {
                                                class: "swatch",
                                                style: "background: {paper}; color: {ink};",
                                                "A"
                                            }
                                            span { class: "menu-label", "{name}" }
                                            if mine { span { class: "menu-key", "Yours" } }
                                        }
                                    }
                                }
                            }
                            // The three at the foot of `showThemeMenu`, and
                            // the whole of the way in to the theme editor
                            // from the bar. A built-in is copied rather than
                            // edited, because a shipped theme is written back
                            // on every run.
                            div { class: "menu-rule" }
                            button {
                                class: "menu-item",
                                "data-item": "new-theme",
                                onclick: move |_| {
                                    viewer.write().close_menu();
                                    viewer.write().begin_theme(None);
                                    viewer.write().show_pane(Pane::Appearance);
                                },
                                span { class: "menu-tick", "" }
                                Icon { name: "plusCircle", stroke: ink.clone() }
                                span { class: "menu-label", "New theme…" }
                            }
                            button {
                                class: "menu-item",
                                "data-item": "edit-theme",
                                onclick: move |_| {
                                    viewer.write().close_menu();
                                    let worn = viewer.read().store.theme().clone();
                                    viewer.write().begin_theme(Some(worn));
                                    viewer.write().show_pane(Pane::Appearance);
                                },
                                span { class: "menu-tick", "" }
                                Icon { name: "edit", stroke: ink.clone() }
                                span { class: "menu-label",
                                    {if worn_built_in {
                                        "Copy this theme…"
                                    } else {
                                        "Edit this theme…"
                                    }}
                                }
                            }
                            if !worn_built_in {
                                button {
                                    class: "menu-item",
                                    "data-item": "delete-theme",
                                    onclick: move |_| {
                                        let worn = viewer.read().store.theme().clone();
                                        viewer.write().ask_delete_theme(worn);
                                    },
                                    span { class: "menu-tick", "" }
                                    Icon { name: "trash", stroke: ink.clone() }
                                    span { class: "menu-label", "Delete this theme" }
                                }
                            }
                            div { class: "menu-rule" }
                            button {
                                class: "menu-item",
                                "data-item": "appearance-settings",
                                onclick: move |_| {
                                    viewer.write().close_menu();
                                    viewer.write().show_pane(Pane::Appearance);
                                },
                                span { class: "menu-tick", "" }
                                Icon { name: "settings", stroke: ink.clone() }
                                span { class: "menu-label", "All appearance settings…" }
                            }
                        }
                    }
                }
                // **The cog opens a menu, not the window**: the switches
                // somebody reaches for while reading, and "All settings…" at
                // the bottom for the rest. Going straight to the window meant
                // a window over the document for a switch that is one press.
                div { class: "anchor",
                    button {
                        class: if menu == Some(Menu::Settings) { "chip settings on" } else { "chip settings" },
                        "aria-label": "Settings",
                        onmousedown: move |event| event.stop_propagation(),
                        onclick: move |_| viewer.write().show_menu(Menu::Settings),
                        Icon { name: "settings", stroke: if menu == Some(Menu::Settings) { ink_on.clone() } else { ink.clone() } }
                        span { class: "chip-label", "Settings" }
                    }
                    if menu == Some(Menu::Settings) {
                        div { class: "menu settings", role: "menu", "aria-label": "Settings",
                            style: "max-height: {menu_reach}px;",
                            onmousedown: move |event| event.stop_propagation(),
                            div { class: "menu-section", "Window" }
                            div { class: "menu-row",
                                label { class: "menu-row-text",
                                    onclick: move |_| {
                                        viewer.write().toggle_toolbar();
                                        viewer.write().close_menu();
                                    },
                                    span { class: "menu-row-label", "Show menu bar" }
                                    span { class: "menu-row-note", "{key_toolbar}" }
                                }
                                // And then leave: this menu hangs off a
                                // button in the toolbar, so turning the
                                // toolbar off leaves it anchored to
                                // nothing. The app closes it for the same
                                // reason and says so in the same words.
                                crate::prefs::Toggle {
                                    on: toolbar_set,
                                    onchange: move |_| {
                                        viewer.write().toggle_toolbar();
                                        viewer.write().close_menu();
                                    },
                                }
                            }
                            div { class: "menu-row",
                                label { class: "menu-row-text",
                                    onclick: move |_| {
                                        viewer.write().set_full_screen(!full_screen);
                                        full_screen_label_frame.ask(Ask::FullScreen(!full_screen));
                                    },
                                    span { class: "menu-row-label", "Full screen" }
                                    span { class: "menu-row-note", "{key_fullscreen}" }
                                }
                                crate::prefs::Toggle {
                                    on: full_screen,
                                    onchange: move |on: bool| {
                                        viewer.write().set_full_screen(on);
                                        full_screen_frame.ask(Ask::FullScreen(on));
                                    },
                                }
                            }
                            // Presenting, from the one menu somebody looks
                            // in: the key and a pane of Settings were the
                            // only ways in. A switch, as it is in Settings
                            // and as full screen above it is — and off
                            // whenever this menu can be seen, presenting
                            // having put the bar away.
                            div { class: "menu-row",
                                label { class: "menu-row-text",
                                    onclick: {
                                        let frame = frame.clone();
                                        move |_| {
                                            viewer.write().close_menu();
                                            let full = viewer.write().present(true);
                                            frame.ask(Ask::FullScreen(full));
                                        }
                                    },
                                    span { class: "menu-row-label", "Presenting" }
                                    span { class: "menu-row-note", "{key_present}" }
                                }
                                crate::prefs::Toggle {
                                    on: false,
                                    onchange: {
                                        let frame = frame.clone();
                                        move |_| {
                                            viewer.write().close_menu();
                                            let full = viewer.write().present(true);
                                            frame.ask(Ask::FullScreen(full));
                                        }
                                    },
                                }
                            }
                            div { class: "menu-rule" }
                            div { class: "menu-section", "Reading" }
                            button {
                                class: if scroll_mode == crate::layout::Mode::Continuous { "menu-item on" } else { "menu-item" },
                                onclick: move |_| {
                                    viewer.write().set_scroll_mode(crate::layout::Mode::Continuous);
                                    viewer.write().close_menu();
                                },
                                span { class: "menu-tick", if scroll_mode == crate::layout::Mode::Continuous { Icon { name: "check", stroke: ink_on.clone() } } }
                                span { class: "menu-label", "Continuous scrolling" }
                                span { class: "menu-key", "Default" }
                            }
                            button {
                                class: if scroll_mode == crate::layout::Mode::Paged { "menu-item on" } else { "menu-item" },
                                onclick: move |_| {
                                    viewer.write().set_scroll_mode(crate::layout::Mode::Paged);
                                    viewer.write().close_menu();
                                },
                                span { class: "menu-tick", if scroll_mode == crate::layout::Mode::Paged { Icon { name: "check", stroke: ink_on.clone() } } }
                                span { class: "menu-label", "One page at a time" }
                            }
                            div { class: "menu-rule" }
                            div { class: "menu-row",
                                label { class: "menu-row-text",
                                    onclick: move |_| viewer.write().set_page_pill(!page_pill),
                                    span { class: "menu-row-label", "Show page number while scrolling" }
                                    span { class: "menu-row-note", "Only when the menu bar is hidden." }
                                }
                                crate::prefs::Toggle {
                                    on: page_pill,
                                    onchange: move |on: bool| viewer.write().set_page_pill(on),
                                }
                            }
                            div { class: "menu-rule" }
                            div { class: "menu-section", "Page numbers" }
                            button {
                                class: if numbering_printed { "menu-item on" } else { "menu-item" },
                                onclick: move |_| { viewer.write().set_page_numbering(true); viewer.write().close_menu(); },
                                span { class: "menu-tick", if numbering_printed { Icon { name: "check", stroke: ink_on.clone() } } }
                                span { class: "menu-label", "As printed on the page" }
                                span { class: "menu-key", "Default" }
                            }
                            button {
                                class: if !numbering_printed { "menu-item on" } else { "menu-item" },
                                onclick: move |_| { viewer.write().set_page_numbering(false); viewer.write().close_menu(); },
                                span { class: "menu-tick", if !numbering_printed { Icon { name: "check", stroke: ink_on.clone() } } }
                                span { class: "menu-label", "Count from 1" }
                            }
                            div { class: "menu-rule" }
                            button {
                                class: "menu-item",
                                "data-item": "settings",
                                onclick: move |_| {
                                    viewer.write().close_menu();
                                    viewer.write().open_settings();
                                },
                                span { class: "menu-tick", "" }
                                Icon { name: "settings", stroke: ink.clone() }
                                span { class: "menu-label", "All settings…" }
                                span { class: "menu-key", "{key_settings}" }
                            }
                            // The one list of what every key does, which
                            // was otherwise only F1 — a key nobody who
                            // needs the list knows about.
                            button {
                                class: "menu-item",
                                "data-item": "keyboard",
                                onclick: move |_| {
                                    viewer.write().close_menu();
                                    viewer.write().show_pane(Pane::Keyboard);
                                },
                                span { class: "menu-tick", "" }
                                Icon { name: "keyboard", stroke: ink.clone() }
                                span { class: "menu-label", "Keyboard shortcuts…" }
                                span { class: "menu-key", "{key_help}" }
                            }
                            button {
                                class: "menu-item",
                                "data-item": "about",
                                onclick: move |_| {
                                    viewer.write().close_menu();
                                    viewer.write().show_pane(Pane::About);
                                },
                                span { class: "menu-tick", "" }
                                Icon { name: "info", stroke: ink.clone() }
                                span { class: "menu-label", "About Moonowl…" }
                                span { class: "menu-key", "" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The search card. **It hangs off the Search chip** by its left edge, the
/// way every other panel in the bar hangs off the button that opens it — the
/// chips to its right are wider than the card. Placed twice: inside the
/// chip's own `.anchor` while the toolbar is up, and at the window's edge
/// when the toolbar is away, or once the chips have lost their words and the
/// card would run out of the window.
///
/// It reads the viewer directly, so it renders on every write while it is
/// up: its count moves as a search runs, and it is up only while one is.
#[component]
pub(super) fn FindCard(viewer: Signal<Viewer>, place: String) -> Element {
    let held = viewer.read();
    let wearing = held.palette();
    let (ink, ink_on, faint) = (
        crate::palette::hex(wearing.muted()),
        crate::palette::hex(wearing.accent),
        crate::palette::hex(wearing.faint()),
    );
    let find_query = held.find_query.clone();
    let find_asked = held.find_asked;
    let find_count = held.find_count();
    let find_options = held.search.options();
    let highlight_all = held.highlight_all;
    // The comment field is inside the pages, and before the find bar in the
    // document, so the find field stops asking while it is up.
    let commenting = held.commenting.is_some();
    let typing_page = held.typing_page;
    drop(held);
    let tick = |on: bool| if on { "boxChecked" } else { "box" };
    rsx! {
        div { class: "find-bar", style: "{place}",
        // The card sits *below* the toolbar, so without this the root's
        // "a press past the bar puts it away" fires on the bar's own
        // switches and Highlight all closes the search it was about to
        // change. Said once here rather than on all eight controls.
        onmousedown: move |event| event.stop_propagation(),
        div { class: "find-row",
            span { class: "find-icon", Icon { name: "search", stroke: faint.clone() } }
            input {
                class: "find-field",
                r#type: "text",
                value: "{find_query}",
                // Selected, every time ⌘F is pressed, so that
                // typing replaces it. See [`place_carets`].
                "data-caret": "all {find_asked}",
                placeholder: "Search this document",
                // While the bar is up, this is the element that wants
                // the keyboard, inside the one that otherwise does —
                // which is what makes `give_keyboard_back`'s rule "the
                // innermost one asking".
                //
                // Unless the page field is up: the two are siblings, so
                // "innermost" cannot separate them and document order
                // would hand ⌥⌘G's field to the find bar. Two fields
                // never both ask.
                "data-keyboard": if typing_page || commenting { None } else { Some("find") },
                onmounted: move |event| {
                    let node = event.data();
                    let task = node.set_focus(true);
                    spawn(async move { let _ = task.await; });
                },
                oninput: move |event| {
                    let typed = event.value();
                    let token = viewer.write().find(&typed);
                    rescan(viewer, token);
                },
                // Every key typed here also bubbles to the root, which
                // turns keys into actions — so without this, typing
                // "just" into the field scrolls the document four times.
                // See [`field_keeps`].
                onkeydown: move |event| {
                    let modifiers = event.modifiers();
                    match event.key() {
                        // Enter is the find bar's own, and is not in
                        // `keys.toml`: it means "the next one" here
                        // and nothing anywhere else.
                        Key::Enter => {
                            event.stop_propagation();
                            viewer.write().step_match(!modifiers.shift());
                        }
                        // The menu first, for the reason the page
                        // field gives above.
                        Key::Escape => {
                            event.stop_propagation();
                            if !viewer.write().close_menu() {
                                viewer.write().close_find();
                            }
                        }
                        // A one-line field has no pages to turn, so
                        // these go on to the document being searched.
                        Key::PageUp | Key::PageDown => {}
                        _ => field_keeps(&event),
                    }
                },
            }
            // The count, and the way to the list behind it — see
            // [`Viewer::show_results`]. A button rather than a
            // readout, and one that only looks pressable when it has
            // something to show, which is `.find-status:not(:empty)`
            // in the app said with a class because Blitz has no `:empty`.
            button {
                class: if find_count.is_empty() { "find-count" } else { "find-count ready" },
                "aria-label": "Show every match",
                onmousedown: move |event| event.stop_propagation(),
                onclick: move |_| viewer.write().show_results(),
                "{find_count}"
            }
            // Up and down, which is what the app draws: the matches
            // are a place in the document rather than a list to walk
            // left and right along.
            button {
                class: "chip icon-only find-previous",
                "aria-label": "Previous match",
                onclick: move |_| viewer.write().step_match(false),
                Icon { name: "up", stroke: ink.clone() }
            }
            button {
                class: "chip icon-only find-next",
                "aria-label": "Next match",
                onclick: move |_| viewer.write().step_match(true),
                Icon { name: "down", stroke: ink.clone() }
            }
            // A cross, not a word: closing the find bar finishes
            // nothing, it puts a thing away.
            button {
                class: "chip icon-only find-close",
                "aria-label": "Close search",
                onclick: move |_| viewer.write().close_find(),
                Icon { name: "close", stroke: ink.clone() }
            }
        }
        // **The three switches, in the app's order and under the field
        // they belong to.** Two change what is found; the first changes
        // only how much of it is painted, and is the one a reader
        // reaches for most. Each wears a box, ticked when on and empty
        // when off, as Firefox's do; the box is there either way, so
        // turning one on does not shuffle the others sideways.
        div { class: "find-options",
            button {
                class: if highlight_all { "find-option find-all on" } else { "find-option find-all" },
                onclick: move |_| viewer.write().toggle_highlight_all(),
                Icon { name: tick(highlight_all), stroke: if highlight_all { ink_on.clone() } else { faint.clone() } }
                "Highlight all"
            }
            button {
                class: if find_options.match_case { "find-option find-case on" } else { "find-option find-case" },
                onclick: move |_| {
                    let token = viewer.write().set_find_options(crate::search::Options {
                        match_case: !find_options.match_case,
                        whole_words: find_options.whole_words,
                    });
                    rescan(viewer, token);
                },
                Icon { name: tick(find_options.match_case), stroke: if find_options.match_case { ink_on.clone() } else { faint.clone() } }
                "Match case"
            }
            button {
                class: if find_options.whole_words { "find-option find-words on" } else { "find-option find-words" },
                onclick: move |_| {
                    let token = viewer.write().set_find_options(crate::search::Options {
                        match_case: find_options.match_case,
                        whole_words: !find_options.whole_words,
                    });
                    rescan(viewer, token);
                },
                Icon { name: tick(find_options.whole_words), stroke: if find_options.whole_words { ink_on.clone() } else { faint.clone() } }
                "Whole words"
            }
        }
        }
    }
}
