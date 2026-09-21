//! The keyboard, as a table rather than as a tower of branches — `keys.ts`,
//! in Rust.
//!
//! Everything the reader listens for is an **action** with a name, and a chord
//! is only ever a way of asking for one. An event is turned into a chord and
//! the chord is looked up, so two actions cannot both answer to ⌘F: that is
//! one key in one map, and a collision the reader is told about rather than a
//! bug that depends on which arm ran first.
//!
//! # The split
//!
//! [`crate::keys`] is `src-tauri/src/keys.rs`, mounted by path: it owns the
//! *file* — reading `keys.toml` and saying which lines are not a table entry
//! of the right shape — and deliberately not the meaning of a line. This
//! module owns the action list and the grammar of a chord, because it is the
//! side that turns a keystroke into one. Validating in both would be the same
//! parser written twice.
//!
//! # A chord
//!
//! Written `mod+shift+f`. Modifiers in the order a canonical chord spells
//! them:
//!
//! * `mod` — ⌘ on a Mac, Ctrl everywhere else. The only one that changes
//!   meaning between platforms.
//! * `ctrl` — the Control key itself. On Windows and Linux that *is* `mod`,
//!   so it is normalised to it: `ctrl+d` and `mod+d` are one chord there and
//!   two on a Mac.
//! * `alt` — ⌥ / Alt.
//! * `shift`
//!
//! Chords separated by a space are pressed one after the other, which is how
//! `g g` can be a binding at all.
//!
//! # `mac` is a parameter, not a `cfg!`
//!
//! `mod` is the one thing in a chord that means something different on each
//! machine, and a `cfg!(target_os = "macos")` would make the half of this file
//! that Windows and Linux run the half that never runs under `cargo test`
//! here. [`this_machine`] is what the binary passes.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use dioxus::html::{Code, Key, Modifiers};

/* --------------------------------------------------------------- actions */

macro_rules! actions {
    ($($variant:ident => $name:literal),* $(,)?) => {
        /// Everything the app can be asked to do. The name is what
        /// `keys.toml` writes and what a problem message quotes.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Action { $($variant),* }

        impl Action {
            pub fn as_str(self) -> &'static str {
                match self { $(Action::$variant => $name),* }
            }

            pub fn from_name(name: &str) -> Option<Action> {
                match name { $($name => Some(Action::$variant),)* _ => None }
            }
        }
    };
}

actions! {
    // Documents
    Open => "open",
    NewWindow => "new-window",
    NewTab => "new-tab",
    Print => "print",
    Settings => "settings",
    Help => "help",
    CloseWindow => "close-window",
    GoToTab => "go-to-tab",
    Quit => "quit",
    Find => "find",
    FindNext => "find-next",
    FindPrevious => "find-previous",
    SelectPage => "select-page",
    CopyQuote => "copy-quote",
    Mark => "mark",
    Markup => "markup",
    Dismiss => "dismiss",
    // Moving around
    NextPage => "next-page",
    PreviousPage => "previous-page",
    ScrollDown => "scroll-down",
    ScrollUp => "scroll-up",
    HalfScreenDown => "half-screen-down",
    HalfScreenUp => "half-screen-up",
    ScreenDown => "screen-down",
    ScreenUp => "screen-up",
    FirstPage => "first-page",
    LastPage => "last-page",
    GoToPage => "go-to-page",
    Back => "back",
    Forward => "forward",
    // Looking at it
    ZoomIn => "zoom-in",
    ZoomOut => "zoom-out",
    FitWidth => "fit-width",
    ActualSize => "actual-size",
    FitPage => "fit-page",
    RotateRight => "rotate-right",
    RotateLeft => "rotate-left",
    Dark => "dark",
    Sidebar => "sidebar",
    Toolbar => "toolbar",
    Fullscreen => "fullscreen",
    Present => "present",
    // …and the three this experiment has that the app does not. See `EXTRA`.
    NextTheme => "next-theme",
    Spread => "spread",
    Copy => "copy",
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Documents,
    MovingAround,
    LookingAtIt,
}

impl Group {
    pub fn as_str(self) -> &'static str {
        match self {
            Group::Documents => "Documents",
            Group::MovingAround => "Moving around",
            Group::LookingAtIt => "Looking at it",
        }
    }
}

pub const GROUPS: [Group; 3] = [Group::Documents, Group::MovingAround, Group::LookingAtIt];

pub struct Spec {
    pub id: Action,
    /// What it does, in the words the Keyboard page uses.
    pub label: &'static str,
    pub group: Group,
    /// Needs a document open. Everything else answers on the start screen too.
    ///
    /// It was carried unread for two phases, on the grounds that dropping a
    /// column from a ported table is how a port starts drifting — and it is
    /// read now, because there is a start screen. `Reader`'s key handler in
    /// `app.rs` is the one place that asks, and `tests/keys.rs` checks every
    /// flag against `src/keys.ts` rather than four of them, because a wrong
    /// answer here is a key that is silently ignored.
    pub needs_document: bool,
    /// The keys it ships with, on every platform.
    pub keys: &'static [&'static str],
    /// …and these as well, on a Mac.
    pub mac_keys: &'static [&'static str],
    /// …or these, on Windows and Linux.
    pub other_keys: &'static [&'static str],
}

/// A table entry, with the two lists that are usually empty defaulted away.
macro_rules! spec {
    ($id:expr, $label:literal, $group:expr, $keys:expr) => {
        Spec {
            id: $id,
            label: $label,
            group: $group,
            needs_document: false,
            keys: &$keys,
            mac_keys: &[],
            other_keys: &[],
        }
    };
    ($id:expr, $label:literal, $group:expr, $keys:expr, doc) => {
        Spec {
            id: $id,
            label: $label,
            group: $group,
            needs_document: true,
            keys: &$keys,
            mac_keys: &[],
            other_keys: &[],
        }
    };
    ($id:expr, $label:literal, $group:expr, $keys:expr, mac $mac:expr) => {
        Spec {
            id: $id,
            label: $label,
            group: $group,
            needs_document: false,
            keys: &$keys,
            mac_keys: &$mac,
            other_keys: &[],
        }
    };
    ($id:expr, $label:literal, $group:expr, $keys:expr, other $other:expr) => {
        Spec {
            id: $id,
            label: $label,
            group: $group,
            needs_document: false,
            keys: &$keys,
            mac_keys: &[],
            other_keys: &$other,
        }
    };
    ($id:expr, $label:literal, $group:expr, $keys:expr, mac $mac:expr, other $other:expr) => {
        Spec {
            id: $id,
            label: $label,
            group: $group,
            needs_document: false,
            keys: &$keys,
            mac_keys: &$mac,
            other_keys: &$other,
        }
    };
}

use Action as A;
use Group::{Documents as D, LookingAtIt as L, MovingAround as M};

/// **The app's own table, carried across entry for entry**, because the point
/// of the port is that `keys.toml` means the same thing on both sides: a table
/// missing rows would report the other half as things Moonowl cannot do.
/// `tests/keys.rs` holds the two tables against the shipped `keys.toml`.
///
/// The comments explaining *why* a key is what it is live in `src/keys.ts`
/// beside the same rows and are not restated — one copy of a reason is the
/// argument for mounting the app's modules rather than copying them, and this
/// is the nearest a file with no `#[path]` can get.
pub const ACTIONS: &[Spec] = &[
    spec!(A::Open, "Open a document", D, ["mod+o"]),
    spec!(A::NewWindow, "New window", D, ["mod+n"]),
    // **Unbound, and in the table so that it can be bound.** A tab is macOS's
    // and the key every Mac application uses for one is ⌘T, which this reader
    // spends on the toolbar — so the gesture is the item under Open… and this
    // row is what lets somebody who wants the key give it one. See `tabs.rs`.
    spec!(A::NewTab, "New tab — macOS only", D, []),
    spec!(A::Print, "Print", D, ["mod+p"]),
    spec!(A::Settings, "Settings", D, ["mod+,"]),
    spec!(A::Help, "This list", D, ["f1", "mod+/"]),
    // **⌘W is ours on a Mac too, and it had been nobody's.** The app this was
    // ported from left it to the menu bar Tauri installs; winit installs an
    // application menu and no Window menu, so ⌘W reached nothing at all —
    // which is also what closes a *tab*, there being no other way to shut
    // one. ⌘Q is still the menu's, because that menu does have Quit in it.
    spec!(A::CloseWindow, "Close this window", D, ["mod+w"]),
    // **Nine chords, one action.** Which tab was asked for is the digit that
    // was pressed, and the key handler in `app.rs` reads it off the event
    // rather than off the action — nine near-identical entries here would be
    // nine rows on the Keyboard page saying the same thing. Tabs are macOS's
    // and exist nowhere else, which is why the keys are, and why ⌘1 and ⌘2
    // still mean what they meant on the platforms with no tabs to go to.
    spec!(A::GoToTab, "Go to a tab by number", D, [],
        mac ["mod+1", "mod+2", "mod+3", "mod+4", "mod+5", "mod+6", "mod+7", "mod+8", "mod+9"]),
    spec!(A::Quit, "Close Moonowl", D, [], other["mod+q"]),
    spec!(A::Find, "Search this document", D, ["mod+f"]),
    spec!(A::FindNext, "Next match", D, ["mod+g"]),
    spec!(A::FindPrevious, "Previous match", D, ["mod+shift+g"]),
    spec!(
        A::SelectPage,
        "Select the text of this page",
        D,
        ["mod+a"],
        doc
    ),
    spec!(
        A::CopyQuote,
        "Copy selection, with its page number",
        D,
        ["mod+shift+c"]
    ),
    spec!(
        A::Mark,
        "Mark this page, or take the mark off",
        D,
        ["mod+shift+b"]
    ),
    spec!(
        A::Markup,
        "Mark the selection — opens the colour popover",
        D,
        ["mod+shift+h"],
        doc
    ),
    spec!(
        A::Dismiss,
        "Close the search bar, leave full screen, stop presenting",
        D,
        ["escape"]
    ),
    spec!(A::NextPage, "Next page", M, ["right", "l"], doc),
    spec!(A::PreviousPage, "Previous page", M, ["left", "h"], doc),
    spec!(A::ScrollDown, "A little down", M, ["down", "j"], doc),
    spec!(A::ScrollUp, "A little up", M, ["up", "k"], doc),
    spec!(A::HalfScreenDown, "Half a screen down", M, ["d"], doc),
    spec!(A::HalfScreenUp, "Half a screen up", M, ["u"], doc),
    spec!(
        A::ScreenDown,
        "Down a screen",
        M,
        ["space", "pagedown"],
        doc
    ),
    spec!(
        A::ScreenUp,
        "Up a screen",
        M,
        ["shift+space", "pageup"],
        doc
    ),
    spec!(A::FirstPage, "First page", M, ["home", "g g"], doc),
    spec!(A::LastPage, "Last page", M, ["end", "shift+g"], doc),
    spec!(
        A::GoToPage,
        "Go to page: type the number, press Enter",
        M,
        ["mod+alt+g", "p"],
        doc
    ),
    spec!(
        A::Back,
        "Back to where you jumped from",
        M,
        ["mod+[", "alt+left"]
    ),
    spec!(A::Forward, "Forward again", M, ["mod+]", "alt+right"]),
    spec!(A::ZoomIn, "Zoom in", L, ["mod++", "mod+="]),
    spec!(A::ZoomOut, "Zoom out", L, ["mod+-"]),
    spec!(A::FitWidth, "Fit the width of the window", L, ["mod+0"]),
    // ⌘1 and ⌘2 belong to the tabs on a Mac, which is what a reader with four
    // documents open in one window reaches for and is not what a zoom mode
    // is: fit width, actual size and fit the page are three buttons in the
    // toolbar and one of them is already on ⌘0.
    spec!(
        A::ActualSize,
        "Actual size",
        L,
        [],
        mac["mod+alt+1"],
        other["mod+1"]
    ),
    spec!(
        A::FitPage,
        "Fit the whole page",
        L,
        [],
        mac["mod+alt+2"],
        other["mod+2"]
    ),
    spec!(A::RotateRight, "Turn the page right", L, ["mod+r"]),
    spec!(A::RotateLeft, "Turn the page left", L, ["mod+l"]),
    spec!(A::Dark, "Dark mode", L, ["mod+d"]),
    spec!(A::Sidebar, "Contents sidebar", L, ["mod+b"]),
    spec!(A::Toolbar, "Toolbar", L, ["mod+t"]),
    spec!(
        A::Fullscreen,
        "Full screen",
        L,
        ["f11", "mod+shift+f"],
        mac["mod+ctrl+f"]
    ),
    spec!(
        A::Present,
        "Presenting — full screen, nothing else on it",
        L,
        ["mod+shift+p"]
    ),
];

/// The three actions this experiment has and the app does not, kept in a list
/// of their own so that [`ACTIONS`] stays exactly the app's and the test which
/// says so stays exact.
///
/// The first two were built because this reader had no menus and have outlived
/// that — the themes and spread modes are in the Theme and View menus now. They
/// are kept because stepping to the next of something is a different gesture
/// from a list, and they cost a row each. On a merge they go away rather than
/// joining the table.
///
/// **`copy` is the other kind of extra, and it would have to join it.** ⌘C is
/// not the app's key because the webview owns the selection and so owns copying
/// it; `main.ts` reaches for the clipboard only for ⌘⇧C. Here the selection is
/// the reader's own (see [`crate::select`]), so plain copying is an action like
/// everything else — the clearest thing found so far that leaving the webview
/// costs: a key nobody ever had to write down.
pub const EXTRA: &[Spec] = &[
    spec!(A::NextTheme, "The next theme in the list", L, ["t"], doc),
    spec!(A::Spread, "One page or two side by side", L, ["s"], doc),
    spec!(A::Copy, "Copy the selection", D, ["mod+c"], doc),
];

/// Every action, the app's and this experiment's, in the order they are shown.
pub fn every() -> impl Iterator<Item = &'static Spec> {
    ACTIONS.iter().chain(EXTRA.iter())
}

fn spec_of(action: Action) -> Option<&'static Spec> {
    every().find(|spec| spec.id == action)
}

/// What an action answers to out of the box, on this kind of machine.
pub fn default_keys(spec: &Spec, mac: bool) -> Vec<&'static str> {
    let extra = if mac { spec.mac_keys } else { spec.other_keys };
    spec.keys.iter().chain(extra.iter()).copied().collect()
}

/// What an action is called on the Keyboard page.
pub fn label(action: Action) -> &'static str {
    spec_of(action).map(|spec| spec.label).unwrap_or("That")
}

/// Whether this action needs a document open.
pub fn needs_document(action: Action) -> bool {
    spec_of(action)
        .map(|spec| spec.needs_document)
        .unwrap_or(false)
}

/// What kind of machine this is, asked once. Every function here takes the
/// answer rather than asking; see the module comment.
pub fn this_machine() -> bool {
    cfg!(target_os = "macos")
}

/* ---------------------------------------------------------------- chords */

/// Named keys, as they are spelled in a chord. Anything else is a single
/// character: `f`, `7`, `[`, `+`.
const NAMES: &[(&str, &str)] = &[
    ("Escape", "escape"),
    (" ", "space"),
    ("Enter", "enter"),
    ("Tab", "tab"),
    ("Backspace", "backspace"),
    ("Delete", "delete"),
    ("ArrowLeft", "left"),
    ("ArrowRight", "right"),
    ("ArrowUp", "up"),
    ("ArrowDown", "down"),
    ("PageUp", "pageup"),
    ("PageDown", "pagedown"),
    ("Home", "home"),
    ("End", "end"),
];

/// Spellings a person might reasonably reach for, mapped onto the one this
/// module uses. A chord is written by hand, so the notation is exactly what
/// will be got wrong.
const ALIASES: &[(&str, &str)] = &[
    ("esc", "escape"),
    ("return", "enter"),
    ("spacebar", "space"),
    ("arrowleft", "left"),
    ("arrowright", "right"),
    ("arrowup", "up"),
    ("arrowdown", "down"),
    ("page-up", "pageup"),
    ("page-down", "pagedown"),
    ("pgup", "pageup"),
    ("pgdn", "pagedown"),
    ("del", "delete"),
    ("plus", "+"),
    ("minus", "-"),
    ("equals", "="),
    ("equal", "="),
    ("slash", "/"),
    ("comma", ","),
    ("period", "."),
    ("dot", "."),
    ("semicolon", ";"),
    ("backslash", "\\"),
    ("backquote", "`"),
    ("grave", "`"),
];

/// Whether the modifier this app calls `mod` is down: ⌘ on a Mac.
///
/// **`meta()` alone is not the question, and asking it was the whole of why
/// every ⌘ chord was dead in the real app.** `keyboard_types::Modifiers` has
/// both `META` and `SUPER`, `meta()` asks for `META`, and
/// `winit_modifiers_to_kbt_modifiers` in `blitz-shell` answers winit's
/// `meta_key()` — which is Command on macOS — with `SUPER`. So ⌘T arrived as
/// a bare `t` and cycled the theme, ⌘F never opened the find bar, and the
/// harness saw none of it because `spell_out` was writing `META` by hand.
/// The harness spells it the way the shell does now; both bits are read here,
/// because which one a keystroke carries is the window system's business and
/// not this app's.
pub fn command(modifiers: Modifiers) -> bool {
    modifiers.meta() || modifiers.contains(Modifiers::SUPER)
}

/// Nothing held but Shift: a keystroke that is typing rather than a chord.
/// The three fields that own the keyboard ask this before they let a key
/// through to the root — see the `onkeydown` handlers in `app.rs`.
///
/// **Alt is typing.** On a Mac Option is how `@`, `[` and `{` are reached on
/// most keyboards that are not American, and on Windows AltGr arrives as Ctrl
/// and Alt together; counted as a chord, none of them could be typed into a
/// search or a password. No default chord is Alt alone.
pub fn plain(modifiers: Modifiers) -> bool {
    !command(modifiers) && (!modifiers.ctrl() || modifiers.alt())
}

/// `event.code` for the keys whose `event.key` a modifier can take away.
///
/// Option is not a letter on a Mac: ⌥G arrives as ©, and the G is simply not
/// there to compare any more. So every event offers a second spelling taken
/// from its physical key, and a chord matches if either spelling does.
const CODES: &[(&str, &str)] = &[
    ("Minus", "-"),
    ("Equal", "="),
    ("BracketLeft", "["),
    ("BracketRight", "]"),
    ("Backslash", "\\"),
    ("Semicolon", ";"),
    ("Quote", "'"),
    ("Backquote", "`"),
    ("Comma", ","),
    ("Period", "."),
    ("Slash", "/"),
    ("Space", "space"),
];

fn look_up(table: &'static [(&'static str, &'static str)], name: &str) -> Option<&'static str> {
    table
        .iter()
        .find(|(from, _)| *from == name)
        .map(|(_, to)| *to)
}

fn is_letter(name: &str) -> bool {
    name.len() == 1 && name.as_bytes()[0].is_ascii_lowercase()
}

/// `f1` … `f12`, and nothing above it: a chord naming `f13` is a chord this
/// app cannot read rather than one it silently ignores.
fn is_function_key(name: &str) -> bool {
    let Some(number) = name.strip_prefix('f') else {
        return false;
    };
    matches!(number.parse::<u32>(), Ok(n) if (1..=12).contains(&n) && !number.starts_with('0'))
}

#[derive(Clone, Copy, Default)]
struct Mods {
    mod_: bool,
    ctrl: bool,
    alt: bool,
    shift: bool,
}

fn spell(mods: Mods, key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 16);
    if mods.mod_ {
        out.push_str("mod+");
    }
    if mods.ctrl {
        out.push_str("ctrl+");
    }
    if mods.alt {
        out.push_str("alt+");
    }
    if mods.shift {
        out.push_str("shift+");
    }
    out.push_str(key);
    out
}

/// A chord as it should be *shown* to somebody, which is not how it is
/// written down.
///
/// `mod+t` is the spelling a file uses and ⌘T is the thing on the keyboard, and
/// the difference is not only the symbol: `mod` is Command on a Mac and
/// Control everywhere else, which is the whole reason the file does not spell
/// it. One notice needs this today — the one that says how to get the toolbar
/// back, whose entire job is to name a key — and the Keyboard page will need
/// it for every row when it is built.
pub fn shown(chord: &str, mac: bool) -> String {
    let mut out = String::new();
    let mut rest = chord;
    loop {
        // The same peeling `parse_chord` does, and for the same reason: `+`
        // is a key as well as a separator, so `mod++` is the zoom.
        let taken = [
            ("mod+", if mac { "⌘" } else { "Ctrl+" }),
            ("ctrl+", if mac { "⌃" } else { "Ctrl+" }),
            ("alt+", if mac { "⌥" } else { "Alt+" }),
            ("shift+", if mac { "⇧" } else { "Shift+" }),
        ]
        .into_iter()
        .find(|(prefix, _)| rest.starts_with(prefix));
        match taken {
            Some((prefix, symbol)) => {
                out.push_str(symbol);
                rest = &rest[prefix.len()..];
            }
            None => break,
        }
    }
    // A letter is shown as a capital, because that is what is printed on the
    // key — and not because Shift is held, which is why this is not `shift+`.
    let mut key = rest.to_string();
    if is_letter(&key) {
        key = key.to_uppercase();
    }
    out.push_str(&key);
    out
}

/// A chord's canonical spelling: modifiers in one fixed order, key lowercased,
/// aliases resolved. `None` when it is not a chord this app can read.
pub fn parse_chord(text: &str, mac: bool) -> Option<String> {
    let lowered = text.trim().to_lowercase();
    let mut rest = lowered.as_str();
    if rest.is_empty() {
        return None;
    }
    let mut mods = Mods::default();
    // Peeled from the front rather than split on `+`, because `+` is also a
    // key: `mod++` is the zoom, and splitting would read it as two empty
    // names. Longest first, so `control+` is not read as `ctrl` with a stray
    // `rol+` after it.
    const MODIFIERS: &[&str] = &[
        "command+", "control+", "option+", "super+", "shift+", "meta+", "cmd+", "ctrl+", "opt+",
        "alt+", "mod+", "win+",
    ];
    while let Some(found) = MODIFIERS.iter().find(|name| rest.starts_with(**name)) {
        match found.trim_end_matches('+') {
            "mod" | "cmd" | "command" | "meta" | "super" | "win" => mods.mod_ = true,
            // On Windows and Linux, Control *is* the modifier this app uses,
            // so there is no third thing for a literal `ctrl` to mean.
            "ctrl" | "control" => {
                if mac {
                    mods.ctrl = true
                } else {
                    mods.mod_ = true
                }
            }
            "alt" | "opt" | "option" => mods.alt = true,
            _ => mods.shift = true,
        }
        rest = &rest[found.len()..];
    }
    let key = look_up(ALIASES, rest).unwrap_or(rest);
    let known = key.chars().count() == 1
        || is_function_key(key)
        || NAMES.iter().any(|(_, name)| *name == key);
    if !known {
        return None;
    }
    Some(spell(mods, key))
}

/// A binding: one chord, or several pressed in turn. `None` when any part of
/// it is unreadable, because half a sequence is not a shorter one.
pub fn parse_binding(text: &str, mac: bool) -> Option<String> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }
    let mut chords = Vec::with_capacity(parts.len());
    for part in parts {
        chords.push(parse_chord(part, mac)?);
    }
    Some(chords.join(" "))
}

/// Every spelling of the key that was just pressed, best first.
///
/// Best first matters: ⇧Space offers `shift+space` before `space`, so a
/// binding that wants the shifted one gets it and a binding that does not
/// still answers. Shift is only dropped for keys that are not letters — G and
/// g are two different keys to a reader, and `shift+g` must never fall through
/// to `g`.
pub fn chords_of(key: &Key, code: Code, modifiers: Modifiers, mac: bool) -> Vec<String> {
    if matches!(
        key,
        Key::Shift | Key::Control | Key::Alt | Key::Meta | Key::CapsLock
    ) {
        return Vec::new();
    }
    // The Windows and Super keys are not bound to anything, and reading one
    // as no modifier at all would turn ⊞J into a scroll.
    if !mac && command(modifiers) {
        return Vec::new();
    }
    let mods = Mods {
        mod_: if mac {
            command(modifiers)
        } else {
            modifiers.ctrl()
        },
        ctrl: mac && modifiers.ctrl(),
        alt: modifiers.alt(),
        shift: modifiers.shift(),
    };

    let spelled = key.to_string();
    let mut names: Vec<String> = Vec::new();
    let mut push = |name: Option<String>| {
        if let Some(name) = name {
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        }
    };
    push(match look_up(NAMES, &spelled) {
        Some(name) => Some(name.to_string()),
        None if spelled.chars().count() == 1 => Some(spelled.to_lowercase()),
        None => None,
    });
    if is_function_key(&spelled.to_lowercase()) {
        push(Some(spelled.to_lowercase()));
    }
    // `Code`'s `Display` is the DOM's own `event.code` string — "KeyG",
    // "Digit0", "Minus" — which is what makes this the same three lines it is
    // in `keys.ts` rather than a match over two hundred variants.
    let hit = code.to_string();
    if let Some(letter) = hit.strip_prefix("Key").filter(|rest| rest.len() == 1) {
        push(Some(letter.to_lowercase()));
    } else if let Some(digit) = hit.strip_prefix("Digit").filter(|rest| rest.len() == 1) {
        push(Some(digit.to_string()));
    } else {
        push(look_up(CODES, &hit).map(str::to_string));
    }

    let mut out: Vec<String> = names.iter().map(|name| spell(mods, name)).collect();
    if mods.shift {
        for name in &names {
            if is_letter(name) {
                continue;
            }
            let without = spell(
                Mods {
                    shift: false,
                    ..mods
                },
                name,
            );
            if !out.contains(&without) {
                out.push(without);
            }
        }
    }
    out
}

/* --------------------------------------------------------------- keymaps */

/// The bindings in force: the defaults, with the reader's file over the top.
///
/// `Clone`, because the Keyboard page is drawn from it and drawing happens
/// outside the borrow the reader is read through.
#[derive(Clone)]
pub struct Keymap {
    /// Binding → the action it asks for.
    pub by_binding: HashMap<String, Action>,
    /// Every binding of every action, in the order they are offered.
    pub by_action: BTreeMap<Action, Vec<String>>,
    /// Chords that begin a longer binding and mean nothing on their own.
    pub prefixes: BTreeSet<String>,
    /// Lines of `keys.toml` this app could not use, in the words the reader
    /// needs to fix them. Empty is the normal case.
    pub problems: Vec<String>,
    mac: bool,
}

/// What a keystroke came to.
#[derive(Clone, Debug, PartialEq)]
pub enum Press {
    /// Do this.
    Act(Action),
    /// The first half of a sequence: hold it and see what comes next.
    Wait(String),
    /// Nothing is bound to it, and whatever was pending is dropped.
    Nothing,
}

impl Keymap {
    /// What the app ships with, on this kind of machine.
    pub fn shipped(mac: bool) -> Keymap {
        Keymap::build(mac, &BTreeMap::new())
    }

    /// The defaults with a `keys.toml` over the top.
    ///
    /// Naming an action in the file *replaces* its keys rather than adding to
    /// them, and naming it with an empty list unbinds it. Anything the file
    /// does not mention keeps what it shipped with — so a file that rebinds
    /// one key stays one line long, and a key added in a later version
    /// arrives rather than being frozen out by a file written before it
    /// existed.
    ///
    /// Nothing here fails. A file somebody wrote by hand is a file this app
    /// does not own, and the answer to a line it cannot read is to say so and
    /// carry on with the rest — the same answer `palette::unreadable` gives a
    /// theme.
    pub fn build(mac: bool, overrides: &BTreeMap<String, Vec<String>>) -> Keymap {
        let mut problems = Vec::new();
        let mut by_action: BTreeMap<Action, Vec<String>> = BTreeMap::new();

        for name in overrides.keys() {
            if Action::from_name(name).is_none() {
                problems.push(format!("{name} is not something Moonowl can do."));
            }
        }

        for spec in every() {
            let Some(given) = overrides.get(spec.id.as_str()) else {
                by_action.insert(
                    spec.id,
                    default_keys(spec, mac)
                        .into_iter()
                        .map(String::from)
                        .collect(),
                );
                continue;
            };
            let mut chords: Vec<String> = Vec::new();
            for text in given {
                match parse_binding(text, mac) {
                    Some(binding) if !chords.contains(&binding) => chords.push(binding),
                    Some(_) => {}
                    None => problems.push(format!(
                        "{}: \"{text}\" is not a key Moonowl can read.",
                        spec.id.as_str()
                    )),
                }
            }
            by_action.insert(spec.id, chords);
        }

        // One chord, one action. Which one is not a matter of the order the
        // arms happen to be written in any more, so a chord claimed twice is
        // a thing the reader is told about — and the one they wrote
        // themselves wins, because the other one is ours.
        let mut by_binding: HashMap<String, Action> = HashMap::new();
        let from_file: BTreeSet<&str> = overrides.keys().map(String::as_str).collect();
        for spec in every() {
            for binding in by_action.get(&spec.id).cloned().unwrap_or_default() {
                let Some(&taken) = by_binding.get(&binding) else {
                    by_binding.insert(binding, spec.id);
                    continue;
                };
                let mine = from_file.contains(spec.id.as_str());
                let theirs = from_file.contains(taken.as_str());
                let loser = if mine && !theirs { taken } else { spec.id };
                let winner = if loser == spec.id { taken } else { spec.id };
                if loser == taken {
                    by_binding.insert(binding.clone(), spec.id);
                }
                problems.push(format!(
                    "{} is asked to do two things — {} has it, {} does not.",
                    describe_binding(&binding, mac),
                    winner.as_str(),
                    loser.as_str()
                ));
                drop_binding(&mut by_action, loser, &binding);
            }
        }

        // A chord that both does something and begins something longer would
        // have to wait to find out which, and a key that waits is a key that
        // feels broken. The shorter one keeps it: acting the moment it is
        // pressed is the promise every other key here makes.
        let bindings: Vec<String> = by_binding.keys().cloned().collect();
        for binding in bindings {
            let chords: Vec<&str> = binding.split(' ').collect();
            for cut in 1..chords.len() {
                let head = chords[..cut].join(" ");
                if !by_binding.contains_key(&head) {
                    continue;
                }
                problems.push(format!(
                    "{} can never be pressed: {} already does something on its own.",
                    describe_binding(&binding, mac),
                    describe_binding(&head, mac)
                ));
                if let Some(&action) = by_binding.get(&binding) {
                    drop_binding(&mut by_action, action, &binding);
                }
                by_binding.remove(&binding);
                break;
            }
        }

        // What is left of the sequences: the chords that mean "wait, there is
        // more".
        let mut prefixes = BTreeSet::new();
        for binding in by_binding.keys() {
            let chords: Vec<&str> = binding.split(' ').collect();
            for cut in 1..chords.len() {
                prefixes.insert(chords[..cut].join(" "));
            }
        }

        Keymap {
            by_binding,
            by_action,
            prefixes,
            problems,
            mac,
        }
    }

    pub fn mac(&self) -> bool {
        self.mac
    }

    /// The action a chord asks for, if any.
    pub fn action_for(&self, binding: &str) -> Option<Action> {
        self.by_binding.get(binding).copied()
    }

    /// What a keystroke means, given what was pressed before it.
    ///
    /// This is `wireKeyboard`'s inner loop from `main.ts`, lifted out as a
    /// function of its two inputs — which is the shape it always had and
    /// could not have there, because it was reading and writing four fields
    /// of the `App` object on the way through. Every branch of it is
    /// testable without a keyboard now.
    pub fn resolve(&self, chords: &[String], pending: &str) -> Press {
        for chord in chords {
            // Half way through a sequence: `g` has been pressed and this is
            // what came next. A chord that does not continue it is not a
            // mistake — `g` then ⌘F is a reader changing their mind — so the
            // pending prefix is dropped and the chord is tried on its own.
            let continued = if pending.is_empty() {
                String::new()
            } else {
                format!("{pending} {chord}")
            };
            let binding = if !continued.is_empty() && self.by_binding.contains_key(&continued) {
                continued.as_str()
            } else {
                chord.as_str()
            };
            if let Some(&action) = self.by_binding.get(binding) {
                return Press::Act(action);
            }
            let prefix = if !continued.is_empty() && self.prefixes.contains(&continued) {
                continued.as_str()
            } else {
                chord.as_str()
            };
            if self.prefixes.contains(prefix) {
                return Press::Wait(prefix.to_string());
            }
        }
        Press::Nothing
    }

    /// The same, straight off an event. What the reader's `onkeydown` calls.
    pub fn press(&self, key: &Key, code: Code, modifiers: Modifiers, pending: &str) -> Press {
        let chords = chords_of(key, code, modifiers, self.mac);
        if chords.is_empty() {
            // Not "nothing is bound to it": a bare Shift is not a keystroke
            // at all, and it must not clear a `g` that is waiting.
            return Press::Wait(pending.to_string());
        }
        self.resolve(&chords, pending)
    }

    /// One line for a reader who is not looking at the Keyboard page and
    /// would otherwise find out by pressing a key that does nothing.
    pub fn complaint(&self) -> Option<String> {
        match self.problems.len() {
            0 => None,
            1 => Some(format!("{}: {}", crate::keys::FILE, self.problems[0])),
            more => Some(format!(
                "{}: {} And {} more.",
                crate::keys::FILE,
                self.problems[0],
                more - 1
            )),
        }
    }
}

fn drop_binding(by_action: &mut BTreeMap<Action, Vec<String>>, action: Action, binding: &str) {
    if let Some(kept) = by_action.get_mut(&action) {
        kept.retain(|each| each != binding);
    }
}

/* -------------------------------------------------------------- for eyes */

const SHOWN: &[(&str, &str)] = &[
    ("left", "←"),
    ("right", "→"),
    ("up", "↑"),
    ("down", "↓"),
    ("pageup", "Page Up"),
    ("pagedown", "Page Down"),
    ("escape", "Escape"),
    ("space", "Space"),
    ("enter", "Enter"),
    ("tab", "Tab"),
    ("home", "Home"),
    ("end", "End"),
    ("backspace", "Backspace"),
    ("delete", "Delete"),
    ("-", "−"),
];

/// A chord as it should be read: ⌘⇧F on a Mac, Ctrl+Shift+F elsewhere.
pub fn describe_chord(chord: &str, mac: bool) -> String {
    fn eat(rest: &mut &str, name: &str) -> bool {
        match rest.strip_prefix(name) {
            Some(left) => {
                *rest = left;
                true
            }
            None => false,
        }
    }
    let mut rest = chord;
    let mod_ = eat(&mut rest, "mod+");
    let ctrl = eat(&mut rest, "ctrl+");
    let alt = eat(&mut rest, "alt+");
    let shift = eat(&mut rest, "shift+");
    let key = rest;
    // A bare ⇧ and a letter is the letter, capitalised: nobody reads G as ⇧G.
    if !mod_ && !ctrl && !alt && shift && is_letter(key) {
        return key.to_uppercase();
    }
    let name = if is_function_key(key) {
        key.to_uppercase()
    } else if let Some(shown) = look_up(SHOWN, key) {
        shown.to_string()
    } else if is_letter(key) && (mod_ || ctrl || alt) {
        key.to_uppercase()
    } else {
        key.to_string()
    };
    let mut out = String::new();
    if mac {
        // Apple's order, which is not the order a chord is written in.
        for (on, sign) in [(ctrl, "⌃"), (alt, "⌥"), (shift, "⇧"), (mod_, "⌘")] {
            if on {
                out.push_str(sign);
            }
        }
    } else {
        for (on, sign) in [
            (mod_, "Ctrl+"),
            (ctrl, "Ctrl+"),
            (alt, "Alt+"),
            (shift, "Shift+"),
        ] {
            if on {
                out.push_str(sign);
            }
        }
    }
    out.push_str(&name);
    out
}

/// A whole binding, sequences included: `g g` reads as "g then g".
pub fn describe_binding(binding: &str, mac: bool) -> String {
    binding
        .split(' ')
        .map(|chord| describe_chord(chord, mac))
        .collect::<Vec<_>>()
        .join(" ")
}
