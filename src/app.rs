//! The reader, as components and one piece of state.
//!
//! A signal holding one `Viewer` is what `main.ts`'s `App` object was, and the
//! DOM that used to be built with `document.createElement` is an `rsx!` block
//! rebuilt from the state whenever the state moves.
//!
//! Four things about it are Blitz's doing rather than choices:
//!
//! *Nothing is `position: fixed`*, because Blitz has no such thing. The root
//! is a flex column — toolbar, viewer, notice — so nothing is over a scrolling
//! body and nothing needs to be. The assessment expected a workaround and it
//! is an improvement.
//!
//! *The keyboard is one handler on the root.* Blitz sends a key to the focused
//! node and falls back to the root when there is none, and events bubble — so
//! a `keydown` on the root is the app-level handler `main.ts` has, including
//! for a page, which cannot be focused. An event becomes a chord and the chord
//! is looked up in [`crate::keymap`]; `perform` at the bottom is one arm per
//! action, and a missing arm is a compile error. The winit route,
//! `use_window_event`, is closed to us: it takes its `WindowEventHandlers` out
//! of a context only `dioxus_native`'s own application provides, and the type
//! is private.
//!
//! *A page is a `<div>` with an `<object>` in it*, absolutely positioned
//! against the viewer at where-it-is minus where-the-reader-is. Blitz has no
//! `position: static`, so an absolutely positioned node is placed against its
//! immediate parent — which is what this layout wants anyway.
//!
//! *And the scrolling is ours.* `overflow: scroll` does not survive the second
//! half of scrolling, which is the app moving the document itself — a page
//! jump, Home, a zoom that keeps the reader's place. Every one of those goes
//! through `MountedData`, and **every `MountedData` call borrows the document
//! while it is already borrowed**: an event handler runs inside `EventDriver`'s
//! borrow, a mounted handler inside `flush_queued_mounted_events`'s, so
//! `scroll` and `get_client_rect` panic with "RefCell already borrowed" rather
//! than failing. So the scroll offset is a number in this file, the wheel
//! moves it, and the pages are placed against it — which is what `viewer.ts`
//! does in all but the last step. What is lost is the platform's fling, and
//! the scrollbar, which is drawn here instead: [`Viewer::bar_thumb`] and the
//! `.scrollbar` in the block below.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use dioxus::html::geometry::WheelDelta;
use dioxus::prelude::*;
use dioxus_native::CustomWidgetAttr;
use serde_json::json;

use crate::emit::Payload;
use crate::keymap::{Action, Keymap, Press};
use crate::layout::{Anchor, Fit, Layout, Mode, Size, Spread};
use crate::page::{Chosen, PageWidget};
use crate::palette::Palette;
use crate::render::{Heading, Link, PageSource, PageText, Rect, Target};
use crate::search::{Options as Find, Search};
use crate::select::{Selection, Spot, Unit};
use crate::sidebar::{Column, Sidebar, Tab};
use crate::store::Store;

/// A door out of this reader: one closure, held so a component can take it as
/// a prop.
///
/// Dioxus compares props to decide whether to render and there is nothing in a
/// closure to compare, so two of these are the same door when they are the
/// same `Rc` — which is what a shell handing the same one down every render
/// produces. Eight of them differed only in their name and in what they are
/// handed; each still carries its own defaults and its own way of being
/// called, in the `impl` block beneath it.
macro_rules! door {
    ($(#[$note:meta])* $name:ident ($($arg:ty),*) $(-> $ret:ty)?) => {
        $(#[$note])*
        #[derive(Clone)]
        pub struct $name(Rc<dyn Fn($($arg),*) $(-> $ret)?>);

        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                Rc::ptr_eq(&self.0, &other.0)
            }
        }

        impl $name {
            pub fn new(through: impl Fn($($arg),*) $(-> $ret)? + 'static) -> Self {
                $name(Rc::new(through))
            }
        }
    };
}

/// An open document, wrapped so that it can be a component's prop.
///
/// Two handles to the same open document are the same document, and no two
/// documents are ever equal: Dioxus wants this to decide whether a component's
/// props have changed, and there is nothing in an open file to compare.
#[derive(Clone)]
pub struct Handle(pub Arc<dyn PageSource>);

impl PartialEq for Handle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

door!(
    /// Whether the pointer is on the screen, asked of whatever owns the window.
    ///
    /// [`Screen`]'s sibling and for the same reason — a component that reaches
    /// into winit is a component that knows what it is running under — but this
    /// one is a *setter*, and there is no CSS for it. `cursor: none` reaches
    /// Blitz and Blitz reaches the window, but only when the hover target
    /// changes, which is precisely when the pointer is moving: the one moment
    /// this must not fire. So it goes to the window directly.
    ///
    /// `set_cursor_visible` rather than the shell provider's `set_cursor`,
    /// which is the other way in and takes the icon with it: showing the
    /// pointer again would have to name a shape, and the right shape is
    /// whatever the thing under it already asked for — an I-beam over a page,
    /// a hand over a link.
    Pointer(bool)
);

impl Pointer {
    /// The default: the window this reader is drawn in, or nothing at all in
    /// a harness, which has no window and no pointer.
    pub fn to_the_window(window: Option<Arc<dyn winit::window::Window>>) -> Self {
        Pointer::new(move |on| {
            // On a Mac, AppKit's own "hidden until the mouse moves": winit's
            // cursor rects sometimes never took the pointer back over the
            // sidebar, where no hover changes the icon to force them to.
            #[cfg(target_os = "macos")]
            {
                let _ = &window;
                use objc2::runtime::{AnyClass, Bool};
                if let Some(class) = AnyClass::get(c"NSCursor") {
                    let _: () = unsafe {
                        objc2::msg_send![class, setHiddenUntilMouseMoves: Bool::new(!on)]
                    };
                }
            }
            #[cfg(not(target_os = "macos"))]
            if let Some(window) = window.as_ref() {
                window.set_cursor_visible(on);
            }
        })
    }

    pub fn show(&self, on: bool) {
        (self.0)(on)
    }
}

/// What the pointer's rest is measured against: when it last moved, whether a
/// timer is already out for it, and whether it is off the screen now.
///
/// **A hook beside the handler, never a capture inside one** — see the note on
/// `use_effect` in `AGENTS.md`. None of it is in the [`Viewer`]: hiding the
/// pointer changes nothing the document looks like, and putting it in a signal
/// would be a render on every move of the mouse.
#[derive(Default)]
struct Resting {
    moved: Cell<Option<std::time::Instant>>,
    waiting: Cell<bool>,
    away: Cell<bool>,
}

door!(
    /// What handing the document to something that prints does. The program
    /// is **named** rather than left to the system's default handler, because
    /// that default may well be this reader and handing a document to
    /// ourselves to print is a loop. A test must not be able to open Preview.
    ///
    /// The answer is nothing, or a sentence for the notice line.
    Printer(&str) -> Result<(), String>
);

impl Printer {
    /// The path, if the document is still there — else the sentence that
    /// says it is not, which is the one thing every printer checks first.
    pub fn present(path: &str) -> Result<String, String> {
        let file = std::path::PathBuf::from(path);
        if file.exists() {
            return Ok(path.to_string());
        }
        let name = file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        Err(format!("{name} is no longer there."))
    }

    /// The hand-off: the platform's own program, exactly as the app names
    /// it. On macOS the shell provides a printer of its own — the system's
    /// print panel, see `print.rs` — and this is what it falls back to.
    pub fn to_the_system() -> Self {
        Printer::new(|path| {
            let file = std::path::PathBuf::from(Printer::present(path)?);

            #[cfg(target_os = "macos")]
            let mut command = {
                let mut command = std::process::Command::new("open");
                command.arg("-a").arg("Preview").arg(&file);
                command
            };

            // Edge by absolute path, because it is not on `PATH` and because
            // naming it is the whole point: `ShellExec_RunDLL` alone opens the
            // file with the *default* handler, which after installing this
            // reader may be this reader.
            #[cfg(target_os = "windows")]
            let mut command = {
                let edge = std::env::var("ProgramFiles(x86)")
                    .or_else(|_| std::env::var("ProgramFiles"))
                    .map(|root| {
                        std::path::PathBuf::from(root)
                            .join(r"Microsoft\Edge\Application\msedge.exe")
                    })
                    .ok()
                    .filter(|path| path.exists());
                match edge {
                    Some(edge) => {
                        let mut command = std::process::Command::new(edge);
                        command.arg(&file);
                        command
                    }
                    None => {
                        let mut command = std::process::Command::new("rundll32.exe");
                        command.arg("shell32.dll,ShellExec_RunDLL").arg(&file);
                        command
                    }
                }
            };

            // The default handler, unless the default is this reader — which
            // the .deb makes it, and then ⌘P came straight back here and
            // printed nothing. Then a viewer that has a print dialog.
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let mut command = {
                let ours = std::process::Command::new("xdg-mime")
                    .args(["query", "default", "application/pdf"])
                    .output()
                    .map(|out| {
                        String::from_utf8_lossy(&out.stdout)
                            .to_lowercase()
                            .contains("moonowl")
                    })
                    .unwrap_or(false);
                let on_path = |name: &str| {
                    std::env::var_os("PATH").is_some_and(|paths| {
                        std::env::split_paths(&paths).any(|dir| dir.join(name).is_file())
                    })
                };
                let program = if ours {
                    ["evince", "okular", "atril", "xreader", "papers"]
                        .into_iter()
                        .find(|name| on_path(name))
                        .ok_or_else(|| {
                            "Moonowl is this computer's PDF viewer, so there is no other to print \
                             from. Install one with a print dialog, such as Evince or Okular."
                                .to_string()
                        })?
                } else {
                    "xdg-open"
                };
                let mut command = std::process::Command::new(program);
                command.arg(&file);
                command
            };

            command
                .spawn()
                .map(|_| ())
                .map_err(|err| format!("Could not hand the document over to print: {err}"))
        })
    }

    pub fn print(&self, path: &str) -> Result<(), String> {
        (self.0)(path)
    }
}

door!(
    /// The document, shown where it lives — "Show in Finder", and its two other
    /// names on the two other platforms. A door beside [`Printer`] and for the
    /// same reason: it hands a path to a program outside this process.
    Reveal(&str) -> Result<(), String>
);

impl Reveal {
    pub fn to_the_system() -> Self {
        Reveal::new(|path| {
            let file = std::path::PathBuf::from(path);
            if !file.exists() {
                return Err(format!("{} is no longer there.", where_it_lives(&file)));
            }
            // A folder is shown by opening it, not by selecting it in its
            // parent: "Open themes folder" means the themes, not their
            // neighbours.
            let folder = file.is_dir();

            #[cfg(target_os = "macos")]
            let mut command = {
                let mut command = std::process::Command::new("open");
                if !folder {
                    command.arg("-R");
                }
                command.arg(&file);
                command
            };

            #[cfg(target_os = "windows")]
            let mut command = {
                let mut command = std::process::Command::new("explorer.exe");
                if folder {
                    command.arg(&file);
                } else {
                    command.arg(format!("/select,{}", file.display()));
                }
                command
            };

            // Nothing on Linux selects a file portably, so the folder it is in
            // is what opens. Saying the folder is the whole of what the item
            // promises anywhere.
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let mut command = {
                let mut command = std::process::Command::new("xdg-open");
                command.arg(if folder {
                    file.as_path()
                } else {
                    file.parent().unwrap_or(&file)
                });
                command
            };

            command
                .spawn()
                .map(|_| ())
                .map_err(|err| format!("Could not show the document: {err}"))
        })
    }

    pub fn show(&self, path: &str) -> Result<(), String> {
        (self.0)(path)
    }
}

fn where_it_lives(file: &std::path::Path) -> String {
    file.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.display().to_string())
}

/// What the file manager is called here. `fileManagerName` in `api.ts`.
pub fn file_manager_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Finder"
    } else if cfg!(target_os = "windows") {
        "File Explorer"
    } else {
        "the file manager"
    }
}

/// Where the reader's settings and themes live, and what to wear for this run.
///
/// A path rather than an open [`Store`], because a component's props have to
/// be `Clone` and `PartialEq` and a settings table is neither — and because
/// the store is this window's, made where the window's state is made.
#[derive(Clone, PartialEq)]
pub struct Config {
    pub dir: std::path::PathBuf,
    /// `--theme N`, which chooses for this run without writing it down.
    pub theme: Option<usize>,
    /// Whether to watch the themes directory and the open document — see
    /// [`crate::watch`].
    ///
    /// **Only the harness reads this now**: the binary provides one
    /// `Arc<Watching>` per process as a context and a window uses it whatever
    /// this says. Off with no context, because `Watching` has no way to stop —
    /// the thread outlives the handle, which is nothing at one per process and
    /// a hundred threads on a `cargo test`. A test posts news into the mailbox
    /// itself; one test asks for the real watcher.
    pub watch: bool,
    /// What this window is called — `main`, then `reader-1`, and so on.
    ///
    /// It is the name `watch.rs` reports a rewritten document to, and the
    /// name [`crate::emit::Exchange`] routes that report by, which is the
    /// whole of why a window needs one. See [`crate::windows`].
    pub window: String,
}

impl Config {
    /// The reader's own directory. See [`crate::config::config_dir`]: it is
    /// deliberately not the installed app's.
    pub fn here() -> Config {
        Config {
            dir: crate::config::config_dir(),
            theme: None,
            watch: true,
            window: crate::windows::MAIN.to_string(),
        }
    }

    pub fn at(dir: impl Into<std::path::PathBuf>) -> Config {
        Config {
            dir: dir.into(),
            theme: None,
            watch: false,
            window: crate::windows::MAIN.to_string(),
        }
    }
}

door!(
    /// How big the window is, asked of whatever knows: width and height in
    /// logical pixels, and the scale factor.
    ///
    /// A number rather than `use_window()`, which consumes an `Arc<dyn
    /// winit::Window>` a headless test cannot provide. The shell answers it out of
    /// the real window and the harness out of its own viewport — and a component
    /// that reaches into winit is a component that knows what it is running
    /// under.
    Screen() -> (f64, f64, f64)
);

impl Screen {
    /// A window of a fixed size, which is what a test has.
    pub fn fixed(width: f64, height: f64, scale: f64) -> Self {
        Screen::new(move || (width, height, scale))
    }

    pub fn get(&self) -> (f64, f64, f64) {
        (self.0)()
    }
}

door!(
    /// Whether the machine is in dark mode, asked of whatever knows. [`Screen`]'s
    /// sibling, for the same reason.
    ///
    /// `Option<bool>` where the app's `matchMedia` answers `bool`, because winit
    /// says `Option<Theme>` and the absence is real: a platform that does not
    /// report an appearance must leave the reader wearing what they chose rather
    /// than be read as "light". See [`crate::store::Store::outside`].
    Appearance() -> Option<bool>
);

impl Appearance {
    /// A machine that will not say, which is what a shell that has not
    /// provided one leaves behind and what most tests want.
    pub fn unknown() -> Self {
        Appearance::new(|| None)
    }

    pub fn get(&self) -> Option<bool> {
        (self.0)()
    }
}

/// Said beside a theme chosen against the machine while following it: the
/// choice holds, and the reader has to know for how long.
pub const UNTIL_THE_SYSTEM_SWITCHES: &str = "Until the system next switches light and dark.";

/// Said, instead of writing, the first time a signed document is marked.
pub const MARKING_BREAKS_A_SIGNATURE: &str =
    "This document is signed, and highlighting it breaks the signature. Highlight it again to go ahead.";

/// …and the same question for taking a mark out, which rewrites the file
/// just the same.
pub const REMOVING_BREAKS_A_SIGNATURE: &str =
    "This document is signed, and changing it breaks the signature. Remove it again to go ahead.";

/// The toolbar's height, the notice line's, and the hairline between them and
/// the document.
///
/// Stated rather than measured: there is no `ResizeObserver` and no
/// `get_client_rect` callable safely from an event. Three numbers rather than
/// one because either of the first two can be taken away — ⌘T puts the toolbar
/// down, presenting puts everything down. See [`Viewer::chrome`].
pub const TOOLBAR: f64 = 46.0;
/// The toolbar's own bottom border, and the only hairline left: the notice
/// line used to be a row of the flex column under it and had one too.
const HAIRLINE: f64 = 1.0;

/// What the chrome costs with all of it on screen, which is what a window
/// opens with.
pub const CHROME: f64 = TOOLBAR + HAIRLINE;

/// How far one press of an arrow moves the page.
const LINE: f64 = 60.0;

/// How wide the scrollbar is, and the shortest its thumb is allowed to be.
/// The width is in `styles.rs` as well, and has to be: the bar is drawn by
/// CSS and positioned by arithmetic — the page count sits to its left.
pub const BAR: f64 = 12.0;
const MIN_THUMB: f64 = 34.0;

/// What a screen keeps of itself when a screen is scrolled, so a paragraph
/// read across the join is not lost. `scrollByViewport` in `viewer.ts` is the
/// same number. Half a screen is half of *this* rather than half the window,
/// or `d` twice and Space once land in two different places.
const OVERLAP: f64 = 60.0;

/// How many places back a reader can step.
///
/// The app's number, and its reasoning: deep enough to walk out of a chain of
/// cross-references, shallow enough that it is a history rather than a log.
const HISTORY_LIMIT: usize = 50;

/// How much of the window a match is brought to when the reader is taken to
/// one: a third down, so there is something above it to read into.
const REVEAL: f64 = 0.3;

/// How many pages of text are kept for the sake of a selection.
///
/// A page is about 100KB of characters and boxes, and every mounted page a
/// selection touches asks for its text on every render — which at a quarter
/// size, or in a spread, is sixteen pages. Eight was a spread and its
/// neighbours, and a longer sweep then extracted every page again each frame.
/// Twenty-four is 2.4MB against the 23MB two page textures cost. See
/// [`Viewer::texts`], the one cache in this file that is bounded where the
/// links beside it are not.
pub const TEXT_CACHE: usize = 24;

/// How long after a press a second one in the same place is the same gesture.
/// Blitz's own number for the fields it owns, restated because a page cannot
/// be told about a double click and has to count one — see
/// [`Viewer::begin_sweep`].
/// The settings keys of the six highlight colours, in swatch order. Static
/// so that [`Viewer::picking`] can name one.
pub const MARKUP_COLOR_KEYS: [&str; 6] = [
    "markup_color_1",
    "markup_color_2",
    "markup_color_3",
    "markup_color_4",
    "markup_color_5",
    "markup_color_6",
];

/// How long a message stays on the notice line. `ui.notice` in the app.
/// How long the first half of a key sequence waits for the second.
const SEQUENCE_LASTS: std::time::Duration = std::time::Duration::from_millis(1200);

const NOTICE_LASTS: std::time::Duration = std::time::Duration::from_millis(4200);

/// How wide the strip at the right edge of a note that is a passage rather
/// than a marker. See [`crate::render::Note`].
const NOTE_EDGE: f64 = 14.0;

/// How far down the window the pointer counts as reaching for the toolbar.
///
/// **The handle's own band and everything above it, not the window's edge.**
/// It was the top eight pixels — the strip macOS slides its own title bar and
/// traffic lights over the moment a pointer arrives there, which put the one
/// control that gives the toolbar back under the one thing the system puts in
/// front of it. The handle sits lower now (see `.peek-line`), so the reach
/// runs from the edge down past the foot of it: wherever the pointer is in
/// that band it is either on the handle or on its way to it.
const PEEK_REACH: f64 = 130.0;

/// And how far down it stays once it is down. It must be below [`PEEK_REACH`]
/// with room to spare: a handle that goes away as the hand arrives is worse
/// than one that was never offered.
const PEEK_KEEP: f64 = 180.0;

/// How long the page pill stays up after a scroll. `flashPill` in `main.ts`.
const PILL_LASTS: std::time::Duration = std::time::Duration::from_millis(1100);

/// And how long the scrollbar stays up after one. Longer than the pill by
/// some way: the pill is a sentence, read once and done with, and the bar is
/// where the reader's hand goes next — a bar that leaves as fast as the pill
/// is a bar that cannot be caught.
const BAR_LASTS: std::time::Duration = std::time::Duration::from_millis(2500);

const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(500);

/// How long a zoom gesture goes on being one after its last event: long
/// enough that a slow drag is not cut into three, short enough that the page
/// comes back sharp before the reader has read a line. See
/// [`Viewer::settle_zoom`].
const ZOOM_SETTLES: std::time::Duration = std::time::Duration::from_millis(180);

/// How long a page turn asked for by a wheel keeps the next one waiting.
///
/// **A swipe is a stream and a keystroke is not.** One flick of a trackpad is
/// dozens of events and then its momentum, and at the foot of a page in paged
/// mode every one of them turned one — a swipe ran through a chapter. Only the
/// wheel is held back, and only at an edge; a key turns every time it is
/// pressed.
// ponytail: a plain gap, so a mouse wheel spun hard turns about three pages a
// second. If that is ever too few, the phase winit knows about — momentum or
// the reader's own fingers — is the thing to ask.
const TURN_GAP: std::time::Duration = std::time::Duration::from_millis(400);

/// How wide the stationary scroll's marker is, and so how far its own edge is
/// held back from the window's: half of itself, because it is centred on the
/// point the button went down on.
///
/// The only place the number is written. `.still-anchor` carries no size of
/// its own — the box and the drawing inside it are both set from here, which
/// is the whole reason the marker is an `<svg>` rather than a bordered div.
const STILL_MARK: f64 = 34.0;

/// The marker itself, on the 24px grid the rest of the icons are drawn on: a
/// ring in the theme's own surface and line, two chevrons, and the point the
/// distance is measured from.
///
/// **Drawn rather than bordered.** It was a div with `border-radius: 50%` and
/// a hairline border, and on a 2x screen the ring came out heavier down one
/// side than the other — a grey sliver on the left of a shape whose whole job
/// is to be symmetrical. A stroked circle goes through the same path
/// rasteriser as every other icon in the window and is round by construction.
fn still_mark(paper: &str, line: &str, ink: &str) -> String {
    format!(
        r#"<circle cx="12" cy="12" r="11.4" fill="{paper}" stroke="{line}" stroke-width="0.72"/><path d="M12 5.2l-3.4 3.7M12 5.2l3.4 3.7M12 18.8l-3.4-3.7M12 18.8l3.4-3.7" fill="none" stroke="{ink}" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/><circle cx="12" cy="12" r="1.4" fill="{ink}"/>"#
    )
}

/// The stationary scroll's dead zone: a pointer within this many pixels of
/// the anchor moves the document not at all. Firefox's own number, and the
/// whole of what keeps a hand resting on the button from creeping down the
/// page.
const STILL_DEAD: f64 = 12.0;

/// How often the stationary scroll steps — a frame — and the interval the
/// speed curve below is quoted in, which is not the same number. See
/// [`still_speed`].
const STILL_TICK: std::time::Duration = std::time::Duration::from_millis(16);
const STILL_STEP: f64 = 20.0;

/// The zoom ladder, in the app's own steps.
/// `ZOOM_LADDER` in `main.ts`, and the same sixteen steps: three of them —
/// 175%, 250% and 600% — had been dropped on the way across, so ⌘+ walked a
/// shorter ladder here and the top of it was 400%.
const ZOOMS: [f64; 16] = [
    0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 6.0,
];

/// How fast the stationary scroll runs when the pointer is `delta` pixels from
/// the anchor, in pixels per [`STILL_STEP`].
///
/// **Firefox's own curve**, `d^1.5 - 1` past the dead zone, because this is
/// Firefox's gesture: a reader who has it in their fingers should find it
/// here behaving exactly as it does there. It is why the pointer just outside
/// the marker creeps and the pointer at the edge of the window flies, with no
/// step between the two.
fn still_speed(delta: f64) -> f64 {
    let far = delta / STILL_DEAD;
    if far > 1.0 {
        far * far.sqrt() - 1.0
    } else if far < -1.0 {
        far * (-far).sqrt() + 1.0
    } else {
        0.0
    }
}

door!(
    /// What opening a link outside the document does.
    ///
    /// A context rather than a call, for the reason [`Screen`] is one.
    /// `webbrowser::open` is the default, so a shell providing nothing still opens
    /// links; a harness providing its own can watch where a link would have gone
    /// without a browser window arriving on somebody's screen mid-`cargo test`.
    /// A document's own links do not go through the DOM, so they need their own
    /// way out — `nav.rs` is the same door for an `<a href>` in the chrome.
    Away(&str)
);

/// A link's address as [`Away`] will open it, or `None` where it will not.
/// The two shapes documents write without a scheme get one: a bare `www.`
/// and a `doi:`.
fn openable(url: &str) -> Option<String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    if ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        Some(url.to_string())
    } else if lower.starts_with("www.") {
        Some(format!("https://{url}"))
    } else if lower.starts_with("doi:") {
        Some(format!("https://doi.org/{}", url[4..].trim_start()))
    } else {
        None
    }
}

impl Away {
    /// The default: hand the address to the system, with the same three
    /// schemes `nav.rs` allows and for the same reason — a `file:` or a
    /// `javascript:` in somebody's document is not a thing this app opens
    /// because the document asked.
    pub fn to_the_system() -> Self {
        Away::new(|url| {
            let lower = url.to_ascii_lowercase();
            if lower.starts_with("http://")
                || lower.starts_with("https://")
                || lower.starts_with("mailto:")
            {
                if let Err(err) = webbrowser::open(url) {
                    eprintln!("could not open {url}: {err}");
                }
            }
        })
    }

    pub fn open(&self, url: &str) {
        (self.0)(url);
    }
}

door!(
    /// Where a copied passage goes.
    ///
    /// A context holding one closure, for the reason [`Away`] is one: a suite that
    /// took the real clipboard would empty the clipboard of whoever is running
    /// `cargo test`.
    ///
    /// **The app has no equivalent and needs none**: there the webview owns the
    /// selection, so it owns copying it. Here the selection is the reader's own
    /// ([`crate::select`]), so copying it is too.
    Clip(&str)
);

impl Clip {
    /// The default: the system's clipboard, through the shell provider Blitz
    /// already hands every window. `None` for a shell that provided none,
    /// which says so once rather than silently copying nothing — a reader who
    /// presses ⌘C and pastes what they copied an hour ago has no way to tell
    /// that from a key that is not bound.
    pub fn to_the_system(shell: Option<Arc<dyn blitz_traits::shell::ShellProvider>>) -> Self {
        Clip::new(move |text| match &shell {
            Some(shell) => {
                if shell.set_clipboard_text(text.to_string()).is_err() {
                    eprintln!("the clipboard refused {} characters", text.len());
                }
            }
            None => eprintln!("there is no clipboard to copy into"),
        })
    }

    pub fn put(&self, text: &str) {
        (self.0)(text);
    }
}

door!(
    /// Which document the reader chose, when they were asked.
    ///
    /// A context holding one closure, for the reason [`Clip`] is one: the thing it
    /// stands for is a modal window belonging to the operating system, and a test
    /// that opened one would sit there until somebody clicked it. `blitz-shell`
    /// already carries the picker — `open_file_dialog` on the shell provider,
    /// behind its `file-dialog` feature, which is `rfd` — so this is a door in the
    /// shell like the clipboard beside it rather than a dependency of its own.
    ///
    /// **It asks and does not answer, and that is a crash rather than a taste.**
    /// `rfd` runs `NSOpenPanel` *modally*, which spins a nested run loop, which
    /// delivers events to winit while it is still inside `EventHandler::handle` —
    /// a click on a menu item is what got us here. That handler panics on
    /// re-entry, correctly. So the picker opens on a thread of its own, where
    /// `rfd` dispatches it back onto the main queue once the click has been
    /// handled, and the answer comes back as news in the mailbox like the document
    /// dropped on the window and the one handed over by a second launch.
    ///
    /// A picker the reader closed sends nothing, which is what cancelling means.
    Pick(Opening)
);

/// Which door a chosen document goes through: this window, or one of its own.
///
/// The two are a menu item apart in the interface and a whole window apart
/// underneath, and the choice has to travel with the ask because by the time
/// the answer arrives the menu it was chosen from is long shut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opening {
    /// Here, replacing what this window is showing. ⌘O and the start screen.
    Here,
    /// Beside it, in a window of its own — the app's "Open document in new
    /// window…", which is the two-documents-at-once route in one step.
    Beside,
}

impl Opening {
    /// The news a chosen document arrives as. Both are handled in `Reader`'s
    /// one mailbox task; `open-document` is the same event a drop and a second
    /// launch already send, and a picked document is the same thing happening
    /// for a different reason.
    fn event(self) -> &'static str {
        match self {
            Opening::Here => "open-document",
            Opening::Beside => "open-document-beside",
        }
    }
}

impl Pick {
    /// The default: the system's own picker, filtered to PDFs, opened on a
    /// thread and answered into the mailbox.
    pub fn from_the_system(
        shell: Option<Arc<dyn blitz_traits::shell::ShellProvider>>,
        post: crate::emit::Post,
    ) -> Self {
        Pick::new(move |opening| {
            // Said once rather than silently choosing nothing, which is
            // `Clip`'s rule and the same reason: a reader who presses ⌘O and
            // gets no window cannot tell a shell that provided no picker from
            // a picker they cancelled.
            let Some(shell) = shell.clone() else {
                eprintln!("there is no picker to choose a document with");
                return;
            };
            let post = post.clone();
            std::thread::spawn(move || {
                let filter = blitz_traits::shell::FileDialogFilter {
                    name: "PDF".to_string(),
                    extensions: vec!["pdf".to_string()],
                };
                let chosen = shell
                    .open_file_dialog(false, Some(filter))
                    .into_iter()
                    .next();
                if let Some(path) = chosen {
                    post.send(crate::emit::News {
                        event: opening.event().to_string(),
                        target: None,
                        payload: crate::emit::Payload::Text(path.to_string_lossy().into_owned()),
                    });
                }
            });
        })
    }

    /// Ask for a document. What comes back, comes back later.
    pub fn ask(&self, opening: Opening) {
        (self.0)(opening);
    }
}

/// What the *window* can be asked to do, which is not the page's business.
///
/// A shell answers these against winit; the harness writes them down, which is
/// how "⌘N asks for a window" is a test rather than something checked by hand.
/// One closure and an enum rather than five closures, because a test that
/// wants to know what the reader asked for wants the list in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    /// A window of its own. The document is chosen by whoever answers, which
    /// in this reader means a file picker: there is no start screen here, so
    /// there is no such thing as a window with nothing in it. See
    /// [`crate::windows::Desk::hand_over`].
    NewWindow,
    /// A *tab* of the window in front, which is macOS's own idea and is
    /// nothing anywhere else. It is asked for by name because the system will
    /// otherwise do it uninvited — see `tabs.rs` and `shell.rs`'s
    /// `can_create_surfaces` — and a reader who pressed ⌘N wanted a window.
    NewTab,
    /// The nth tab of this window's group, brought forward. One-based: ⌘1 is
    /// the first. A window that is not in a tab group has one tab, so this
    /// costs nothing where there are none.
    SelectTab(usize),
    /// This window, closed. On the last window that ends the app, which is
    /// how most people quit it.
    Close,
    /// All of them, and the app with them.
    Quit,
    /// This document, in a window of its own — ⇧⌘O, and the menu item under
    /// it. Answered by `hand_over`, so a document already open somewhere is
    /// brought forward rather than opened twice.
    NewWindowOn(String),
    /// A document handed to this window that it has no room for, sent on.
    /// Like [`Ask::NewWindowOn`], but whether it is a tab or a window is the
    /// setting's to say, as it is for a document arriving from outside.
    SendOn(String),
    /// This window is showing a different document now: where it is, and what
    /// to call it.
    ///
    /// Three things outside the reader have to be told and none is the
    /// reader's to reach: the desk (the restore list), the watch (still
    /// following the file open a moment ago) and the window's own title. The
    /// name travels with the path because the reader has already worked it out
    /// — `store::called` — and asking pdfium again means opening the file
    /// again.
    Showing { path: String, title: String },
    /// Full screen, on or off. Presenting is this and the chrome taken away,
    /// and the second half is the page's own — see [`Viewer::present`].
    FullScreen(bool),
}

door!(Frame(Ask));

impl Frame {
    /// What a window with nobody listening does, which is say so once. A
    /// reader who presses ⌘N and gets silence has no way to tell a shortcut
    /// that did nothing from one that is not bound.
    pub fn unanswered() -> Self {
        Frame::new(|ask| eprintln!("nothing is listening for {ask:?}"))
    }

    pub fn ask(&self, ask: Ask) {
        (self.0)(ask);
    }
}

/// What a mark says it was made by, which is what every other reader shows in
/// the margin beside it. The app writes nothing here, because pdf.js's
/// annotation editor does not offer to; this reader is writing the annotation
/// itself and there is no reason to leave it anonymous.
const AUTHOR: &str = "Moonowl";

/// How tall a signature is dropped, in the page's own points.
///
/// Fixed rather than a fraction of the page: a signature is a fact about a
/// hand and not about the paper, and a name on a postcard is the same size as
/// the same name on a poster. Forty points is about 14mm.
const HAND_HEIGHT: f64 = 40.0;

/// The drawing pad in the Sign window, in CSS pixels.
///
/// Written down rather than measured, because a stroke arrives as a point
/// inside the element and Blitz gives an event its coordinates and not its
/// target's box. Sized from these on the element itself rather than in
/// `styles.rs`, which is a `const &str` and cannot interpolate — two numbers
/// that have to agree is the copy this codebase keeps warning about.
pub const PAD_WIDTH: f64 = 440.0;
pub const PAD_HEIGHT: f64 = 150.0;

/// One row of the markup list, wherever the mark itself is being kept.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkRow {
    pub page: usize,
    pub color: String,
    /// The words under it — read off the page for a mark in the document,
    /// and remembered for one the journal is holding, because a mark that is
    /// beside a document may be beside a document that has been rewritten.
    pub quote: String,
    pub key: MarkKey,
}

/// How to find a mark again in order to take it out.
///
/// A mark in the file is named by where it sits and removed by removing it; a
/// mark beside the file is named by an id this reader made up and removed from
/// a TOML table. In the app every mark is the second kind for removal, because
/// the first kind cannot be removed at all — which is why its journal needs a
/// durable id and this one does not.
/// A control that takes something away for good, pressed once: the second
/// press on the same one is the one that acts. See [`Viewer::arm`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arming {
    /// A highlight's row in the panel.
    Mark(MarkKey),
    /// A signature on the document: its page and annotation index.
    Placed(usize, usize),
    /// A kept signature, by id.
    Kept(String),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MarkKey {
    /// In the document: the page, and where among that page's annotations.
    /// Good for as long as the document is the one it was read from, which
    /// is until the next write — and every write here is followed by a
    /// reopen and a re-read.
    InFile(usize, usize),
    /// Beside the document, in `library.toml`, by the id it was given.
    Beside(String),
}

/// A fit mode, spelled the way `settings.toml` spells it. One function rather
/// than two match arms in two places, because the value written and the value
/// read back have to be the same word and nothing else checks that they are.
fn name_of(fit: Fit) -> &'static str {
    match fit {
        Fit::Width => "width",
        Fit::Page => "page",
        Fit::Actual => "actual",
    }
}

/// Which page of the Settings window is open.
///
/// **A window in the flow rather than one of the system's**, as in the app. It
/// matters more here: a second winit window would be a second `Viewer` over a
/// second `Store`, and every setting changed in it would reach the reader on
/// its next launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Reading,
    Appearance,
    Window,
    Keyboard,
    About,
}

impl Pane {
    /// The five, in the order the nav column lists them.
    pub const ALL: [Pane; 5] = [
        Pane::Reading,
        Pane::Appearance,
        Pane::Window,
        Pane::Keyboard,
        Pane::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Pane::Reading => "Reading",
            Pane::Appearance => "Appearance",
            Pane::Window => "Window",
            Pane::Keyboard => "Keyboard",
            Pane::About => "About",
        }
    }

    /// The icon beside it, by the app's own name for the drawing.
    pub fn icon(self) -> &'static str {
        match self {
            Pane::Reading => "book",
            Pane::Appearance => "theme",
            Pane::Window => "sidebar",
            Pane::Keyboard => "keyboard",
            Pane::About => "info",
        }
    }
}

/// The toolbar's three menus: what document is open, what it looks like, and
/// how it is laid out — which is where the app's are and what they are about.
///
/// The chips stood in for these, and a list of choices with no room to show
/// itself is what `keymap::EXTRA`'s first two entries exist for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Menu {
    /// Under the document's name: what can be done with the document that is
    /// open — mark it, highlight in it, print it, copy what it is called.
    Document,
    /// Under "Open…": every way to bring a *different* document onto the
    /// screen, and the shelf of what was read before. Kept apart from
    /// [`Menu::Document`] for the app's own reason — a menu opened by pressing
    /// the name of the paper you are reading should not be four items about
    /// papers you are not.
    Open,
    /// Under the theme's name: every theme installed, the one in use ticked.
    Theme,
    /// Under the zoom: the fit modes, a stepper and the presets — which is
    /// `showZoomMenu` in `main.ts`, item for item.
    View,
    /// Under the cog: the switches somebody reaches for while reading, and
    /// the way to the window that holds all of them. `showSettingsMenu`.
    Settings,
    /// Under "of 425": whether a page is called by the number printed on it
    /// or by where it falls in the file. Only where the two differ.
    Numbering,
}

impl Menu {
    /// What the button that opens it is called, which is also what the menu
    /// is called to a screen reader.
    pub fn label(self) -> &'static str {
        match self {
            Menu::Document => "Document",
            Menu::Open => "Open",
            Menu::Theme => "Theme",
            Menu::View => "View",
            Menu::Settings => "Settings",
            Menu::Numbering => "Page numbers",
        }
    }
}

/// Everything the reader is looking at, and everything that changes it.
/// A document that will not open without a password, and what has been said
/// to it so far.
///
/// The app's `ui.askForPassword` is a promise the load waits on. There is
/// nothing to wait here: pdfium answers at open, so a locked document is a
/// piece of *state* — the window shows whatever it was showing, with a
/// question over it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Locked {
    /// The document being asked about. Not opened, and not the document this
    /// window is showing: a reader who declines keeps what they had.
    pub path: String,
    /// What has been typed, in the clear. What is on screen is bullets — see
    /// [`Viewer::type_password`].
    pub typed: String,
    /// Whether an answer has already been given and refused, which is the
    /// difference between the two sentences over the field.
    pub wrong: bool,
}

/// The Sign window: what is drawn on the pad, and what it will be called.
///
/// **The pad is the one place in this reader where the pointer draws**, so the
/// points are kept as they arrive, in the pad's own space, and normalised only
/// when the pad's size is in hand; see [`Viewer::draw_to`]. They are
/// deliberately not a `Signature` until then: a signature is a thing on disk
/// in a unit box, and these are pixels on a pad.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Signing {
    /// What the reader is calling it. A name is asked for because several are
    /// the ordinary case — a full name and a set of initials — and a list of
    /// rows all called the same thing is a list nobody can choose from.
    pub name: String,
    /// The strokes so far, in the pad's own pixels from its top left.
    pub strokes: Vec<Vec<[f64; 2]>>,
    /// Whether the pointer is down, which is what makes a move add a point to
    /// the last stroke rather than start one.
    pub drawing: bool,
    /// How big the pad was when it was last drawn on, so that the strokes can
    /// be put into the unit box without asking the DOM. Written by the pad's
    /// own press handler, which is the one place the size is known.
    pub pad: (f64, f64),
    /// Where the pad's top left corner is in the window, worked out at the
    /// press from the difference between the two coordinate systems the event
    /// carries. See [`Viewer::draw_from`].
    pub origin: (f64, f64),
    /// **The other half of a signature** — a date, a place, a printed name.
    /// Kept beside the strokes rather than in a window of its own because it
    /// is the same errand.
    pub line: String,
    /// What the window lists, read when it opens rather than on every render
    /// — each stroke on the pad is a render, and one of these opens the file.
    /// The signatures kept on disk, the ones already on the document's
    /// pages, and the digital signatures it carries.
    pub kept: Vec<crate::sign::Signature>,
    pub signed_here: Vec<crate::sign::Placed>,
    pub seals: Vec<crate::sign::Seal>,
}

/// What is armed and waiting for a click on a page.
///
/// **Signing is two gestures and this is the gap between them.** Choosing what
/// to put down and choosing where it goes are different questions, and a
/// window that answered both would have to contain the page. So the window
/// closes, this is armed, the pointer answers the second question, and Escape
/// disarms it like everything else in this reader that waits.
pub enum Placing {
    /// A name, drawn. Goes in as `/Ink`.
    Hand(crate::sign::Signature),
    /// A date or a line of text. Goes in as a `/Stamp` — see
    /// [`crate::sign::place_text`] for why not the annotation named for it.
    Line(String),
}

/// How a write went, and the document read again after it.
type Landed = (
    Result<(), String>,
    Result<Arc<dyn PageSource>, crate::render::Refusal>,
);
/// The second half of whatever asked for a write. See [`Viewer::write`].
type Done = Box<dyn FnOnce(&mut Viewer, Result<(), String>)>;

pub struct Viewer {
    pub document: Arc<dyn PageSource>,
    pub layout: Layout,
    pub scroll_top: f64,
    /// Where the window sits across the document, as the fraction of the
    /// content its middle is over. Half, always, until somebody pans.
    ///
    /// **A fraction rather than an offset, and that is what keeps the page
    /// centred.** The app gets centring from `margin: 0 auto`; here the pages
    /// are placed absolutely, so it is arithmetic — and an offset would have
    /// to be recomputed at every one of the dozen places that relay the
    /// document out. [`Viewer::scroll_left`] resolves a fraction against
    /// whatever the content is now, which is the answer for a zoom step and
    /// for a window made narrower alike.
    across: f64,
    /// The settings table and the themes, which is everything this reader
    /// remembers between runs. It is not a signal and does not need to be:
    /// every change to it goes through a method here, and this whole struct
    /// is behind one.
    pub store: Store,
    chosen: Chosen,
    /// One line at the bottom of the window, which is `notice()` in `ui.ts`.
    pub notice: String,
    /// A document is being dragged over this window, and whether it is one
    /// this reader would open — `#drop-hint` in the app, arriving from winit
    /// rather than from the DOM. `None` is nothing over the window, which is
    /// almost always.
    pub dragging: Option<bool>,
    /// Every key the reader can press, and what it asks for. Built once from
    /// the defaults with `keys.toml` over the top; see [`crate::keymap`].
    pub keymap: Keymap,
    /// The first half of a sequence, waiting to find out what follows it —
    /// `g`, on its way to `g g`. Empty almost always.
    pending: String,
    /// When `pending` was pressed.
    pending_at: std::time::Instant,
    /// Bumped whenever every page has to be drawn again. It is not in the
    /// texture's key — the widget compares sizes and themes itself — but the
    /// components have to be told that something they cannot see has moved.
    pub generation: u64,
    /// The panel on the left, and what it is showing. All three are settings,
    /// so a reader who reads with the contents open gets them back.
    pub sidebar_open: bool,
    /// **Whether the panel on screen is one the search borrowed.** The count
    /// in the find bar is the way through to the list, as in the app. What the
    /// app does not have is a way back: a panel that came up on its own goes
    /// away on its own, so one Escape takes the bar down and the panel with
    /// it — but only when the panel was shut to begin with.
    results_borrowed: bool,
    /// The one removal waiting for its second press, if any.
    pub arming: Option<Arming>,
    /// The two-page arrangement the spread key goes back to.
    pub last_pair: Spread,
    /// The panel's tab before a search took it over. See [`Viewer::to_results`].
    tab_before_results: Option<Tab>,
    /// The page pill: whether it is up, and which flash put it there.
    ///
    /// A token rather than a timer, as the notice line is: the thread that
    /// takes the pill down carries the number it was started for, so a scroll
    /// while it is up keeps it up rather than letting it vanish on the first
    /// one's clock.
    /// When a page was last turned at the foot of one, so that the rest of
    /// the flick that turned it does not turn another. See [`TURN_GAP`].
    turned_at: Option<std::time::Instant>,
    pill_up: bool,
    pill_token: u64,
    /// Where the last relayout — a zoom, a resize — left the scroll. The
    /// document moved there without the reader scrolling, so the pill, which
    /// answers a scroll, stays down. See [`Viewer::relaid`].
    relaid_at: f64,
    /// The scrollbar, which is the same thing again: up while the document is
    /// moving and for [`BAR_LASTS`] after it stops.
    ///
    /// **It is not drawn at all while it is away**, rather than drawn
    /// transparent: the track is twelve live pixels hard against the edge of
    /// the window, and an invisible control that jumps the document when it
    /// is pressed is worse than no control.
    bar_up: bool,
    bar_token: u64,
    /// Scrolls asked for. See [`Viewer::scroll_gesture`].
    scrolls: u64,
    /// Whether the handle that gives the toolbar back is down. See
    /// [`Viewer::reach_for_toolbar`].
    peek: bool,
    /// The toolbar is on screen for as long as the page field is in use, though
    /// the setting still says it is hidden. `toolbarPeeking` in `main.ts`. See
    /// [`Viewer::open_page_field`].
    borrowed_toolbar: bool,
    /// Whether this search has already put its results on screen. Once per
    /// search, like [`Viewer::revealed`] beside it and for the same reason:
    /// a scan is many slices and the reader must be able to close what one of
    /// them opened. See [`Viewer::show_the_matches`].
    offered_results: bool,
    pub sidebar_width: f64,
    /// The pointer's `client_x` and the panel's width when the edge was
    /// picked up — everything `drag_sidebar` needs and nothing it has to ask
    /// the DOM for. `None` outside a drag, which is most of the time and is
    /// what root's `onmousemove` checks before touching the signal at all.
    resize_from: Option<(f64, f64)>,
    /// The pointer's `client_y` and the offset the document was at when the
    /// scrollbar's thumb was picked up. `None` outside a drag, exactly as
    /// `resize_from` is, and checked in the same place for the same reason.
    bar_from: Option<(f64, f64)>,
    /// **The stationary scroll**, which the middle button starts: where it
    /// went down in the window, and where the pointer has got to since. The
    /// distance between the two is the speed — see [`still_speed`].
    ///
    /// `None` is the gesture not running, which is almost always, and is what
    /// every press, key and wheel checks before ending it.
    still_from: Option<(f64, f64)>,
    still_at: (f64, f64),
    /// Which run of it the clock is stepping, so that a gesture started and
    /// stopped twice in a second does not end up with two tickers driving one
    /// document. Zero before the first.
    still_token: u64,
    pub tab: Tab,
    /// Which toolbar menu is down, if any. `None` almost always.
    ///
    /// **One at a time, and the state is the reader's rather than the
    /// button's**, for `showPopover`'s reason: opening a second menu has to
    /// close the first, and nothing inside one menu can know about another.
    /// Escape closes it, and so does a press anywhere the menu is not.
    pub menu: Option<Menu>,
    /// Which page of Settings is up, if any. See [`Pane`].
    pub pane: Option<Pane>,
    /// And which page it was on when it was last put away, so that reopening
    /// it comes back to where the reader was.
    pane_last: Pane,
    /// Where the thumbnail column has been scrolled to. Ours for the reason
    /// the document's own scroll offset is ours — see the module comment.
    pub thumb_scroll: f64,
    /// Where every thumbnail sits, for the panel at its current width.
    pub column: Column,
    /// The document's own table of contents, read once when it was opened.
    pub headings: Vec<Heading>,
    /// The heading last clicked in Contents, which wins a tie with another
    /// at the same height — see [`crate::sidebar::heading_for`].
    pub picked_heading: Option<usize>,
    /// What the document calls its own pages, or empty when it calls them 1
    /// to n — see [`crate::render::PageSource::labels`]. Read once at open,
    /// like the outline, and for the same reason: it decides what the toolbar
    /// *says*, and a number that arrives late is a number that was wrong.
    labels: Vec<String>,
    /// The links on each page that has been asked about, kept.
    ///
    /// **Behind a `RefCell` because it is filled while the reader is being
    /// *read***: a page's links are wanted by the render that mounts it, and a
    /// render holds a `read()` of this struct. Filling it from outside would
    /// mean every method that changes which pages are mounted remembering to,
    /// and forgetting one is a page whose cross-references quietly do nothing.
    ///
    /// Not trimmed: a link is a rectangle and a target, and four hundred pages
    /// of them is a fiftieth of one page's texture. The caches worth watching
    /// hold *pixels*.
    links: RefCell<HashMap<usize, Rc<Vec<Link>>>>,
    /// And the notes on it, kept the same way and for the same reason: a
    /// document that carries none — which is most of them — asks pdfium once
    /// per page and gets an empty list it can hold on to.
    notes: RefCell<HashMap<usize, Rc<Vec<crate::render::Note>>>>,
    /// The note the reader has opened, if any. See the note window in
    /// [`Reader`] — `showNote` in `main.ts`.
    pub note_open: Option<(usize, crate::render::Note)>,
    /// Whether the window that edits the six highlight colours is up. See
    /// [`crate::prefs::MarkupColours`].
    pub colours_open: bool,
    /// The document waiting on a password, if one is. `ui.askForPassword` in
    /// the app, and see [`Locked`].
    pub locked: Option<Locked>,
    /// Whether the Information window is up. `showDocumentDetails` in
    /// `main.ts`: what the document says about itself, which nothing else in
    /// this reader shows.
    pub details_open: bool,
    /// The theme being written, if one is. `editing` in `settings.ts`: a
    /// draft is installed as the live theme while it is being made, which is
    /// how the app around you becomes the preview.
    pub editing: Option<crate::theme::Theme>,
    /// The draft as it stood when editing began, so the editor can tell
    /// whether anything is unsaved.
    pub editing_from: Option<crate::theme::Theme>,
    /// The Sign window, when it is up. See [`Signing`], and [`crate::sign`]
    /// for what the word does and does not mean here.
    pub signing: Option<Signing>,
    /// A signature chosen and waiting for somewhere to go. See [`Placing`].
    pub placing: Option<Placing>,
    /// Whether the reader has been told, for this document, that signing it
    /// rewrites the file and breaks the cryptographic signature it carries.
    /// Once, and before it happens — see [`crate::sign::BREAKS_A_SIGNATURE`].
    said_rewrites: bool,
    /// The text of the pages the pointer has touched, and where every
    /// character of it is.
    ///
    /// Behind a `RefCell` for the reason the links are, and **bounded where
    /// the links are not**: a page of text is a `char` and a `Rect` per
    /// character, about thirty-six bytes each, so a four-hundred-page book read
    /// end to end would be 40MB — a quarter of what this reader costs, held for
    /// a feature nobody may have used. [`TEXT_CACHE`] pages, oldest out first.
    ///
    /// The search keeps its own copy while the find bar is up and gives it back
    /// with [`Search::forget`]; the two do not share one, because a selection
    /// outlives the find bar and would otherwise stop being copyable for no
    /// reason the reader could see.
    texts: RefCell<Vec<(usize, Rc<PageText>)>>,
    /// What the reader has swept over, or nothing.
    ///
    /// One selection for the document rather than one per page: a sweep that
    /// runs off the bottom of a page carries on down the next one, which is
    /// what continuous scrolling is for. See [`crate::select`].
    selection: Option<Selection>,
    /// Every highlight in the document, read out of the file at open and
    /// again after every reload. See [`crate::markup`].
    ///
    /// **The file is the record and this is a reading of it**, so there is
    /// nothing to keep in step: a mark is written, the document is reopened
    /// through the path a recompile already uses, and this is filled from what
    /// came back. The app needs a journal reconciled on every open because its
    /// writes and reads are on opposite sides of a bridge; here they are the
    /// same call.
    pub markup: Vec<crate::markup::Mark>,
    /// Where a mark can go on this document, asked once when it opened.
    standing: crate::markup::Standing,
    /// Whether the reader has been told about the standing yet. Once per
    /// document, as the app says it: a sentence repeated on every gesture is
    /// a sentence nobody reads.
    said_standing: bool,
    /// The colour popover: which page it is over and where on that page, in
    /// the same space as a mark's own quads.
    ///
    /// `None` almost always. It opens where the selection ends rather than off
    /// a toolbar button, because nothing in the toolbar is what it is about.
    pub markup_at: Option<(usize, Rect)>,
    /// The theme a "Delete…" button is asking about, while its window is up.
    /// See [`crate::prefs::ConfirmDeleteTheme`].
    pub deleting_theme: Option<crate::theme::Theme>,
    /// The mark the pointer was last clicked on, and what to say about it:
    /// which page, where on it, what colour it is and how to take it out.
    ///
    /// A mark is a thing on a page, so the way to take it off is on the page —
    /// a × on a row in a panel behind a tab is reachable and is not findable.
    pub mark_open: Option<(usize, Rect, MarkKey, String)>,
    /// Which colour field of the theme editor has its picker down, named by
    /// the theme key it writes — `None` when none of them has.
    ///
    /// Here rather than in the component, and for [`Viewer::markup_at`]'s
    /// reason: a popover has to close on Escape, and Escape reaches the root's
    /// handler whenever the pointer has left the focus anywhere that is not a
    /// field — which is what pressing the swatch to open the picker does. Six
    /// fields, so it is *which* rather than whether.
    pub picking: Option<&'static str>,
    /// Where the last press landed, in the page's own space, so that letting
    /// go without having swept anywhere can ask what is under it.
    pressed_on: Option<(usize, f64, f64)>,
    /// Where the content's top left is, in the coordinates a mouse event
    /// arrives in, worked out from the press that began the sweep.
    ///
    /// **Not asked of the DOM, because it cannot be asked from here** — a
    /// `MountedData` call borrows the document and every place a component can
    /// call one from is already inside a borrow of it. The press arrives with
    /// both its client coordinates and its coordinates within the page it
    /// landed on, and the layout knows where that page is: subtract. Worked
    /// out once per sweep, with the scroll offset added back on every move, so
    /// scrolling mid-sweep extends the selection through the text that goes
    /// past rather than through the pixels the pointer is over.
    ///
    /// `None` outside a sweep, which root's `onmousemove` checks before
    /// touching the signal at all.
    sweep_from: Option<(f64, f64)>,
    /// When and where the pointer last went down on a page, and how many times
    /// in a row it has there, which is the whole of what tells a second click
    /// from a first one and a third from a second. See
    /// [`Viewer::begin_sweep`].
    pressed: Option<(std::time::Instant, f64, f64, u8)>,
    /// What the sweep under way takes hold of, and the unit it began on, so
    /// that dragging on from a double click extends by words from that word
    /// rather than by characters from wherever the pointer twitched to.
    sweep_seed: (Unit, Spot, Spot),
    /// The scale a zoom gesture began at, while one is under way.
    ///
    /// A pinch is a stream rather than a step, and this is what makes the
    /// stream one gesture: every page keeps the texture it was drawn at when
    /// the fingers went down and is stretched to whatever the layout says this
    /// frame. Dividing by it gets back to that size, which is what keeps the
    /// component key still. See [`crate::page::Chosen::holding`].
    zoom_from: Option<f64>,
    /// Whether a pinch is under way, which is what keeps the settle timer
    /// from ending it. See [`Viewer::end_pinch`].
    pinching: bool,
    /// Which gesture the settle timer is for; see [`Viewer::settle_zoom`].
    zoom_token: u64,
    /// Where the reader has jumped from, and where they came back from.
    ///
    /// Moving *through* a document leaves no trace — scrolling, turning a
    /// page, stepping through matches. Jumping *across* it does: a
    /// cross-reference, a chapter, a typed page number. Those three are what
    /// go through [`Viewer::jump_to`].
    past: Vec<Anchor>,
    future: Vec<Anchor>,
    /// Whether the reader is typing a page number into the field in the
    /// toolbar, and what they have typed.
    ///
    /// The field shows the current page's label when nobody is typing in it,
    /// and what has been typed when somebody is.
    ///
    /// **It arrives holding the page it is on, and the first thing typed
    /// replaces the lot**, which is what the app's `el.pageNumber.select()`
    /// comes to. There is no `select()` here — parley selects all only when
    /// *asked by a keystroke*, with no imperative door from a component — so
    /// the selection is emulated: `page_fresh` below is the "all of it is
    /// selected" state, spent in `Reader`'s keydown handler. An empty field
    /// would lose the reader the one thing the field was showing them.
    pub typing_page: bool,
    pub page_typed: String,
    /// Whether what is in the field is the page it opened on rather than
    /// anything the reader has typed — the emulated "all of it is selected".
    /// See [`Viewer::typing_page`].
    pub page_fresh: bool,
    /// The index, the matches, and where the reader is in them. See
    /// [`crate::search`]: it knows nothing about this struct, which is what
    /// lets the whole of it be tested with no document and no window.
    pub search: Search,
    /// Whether the find bar is up. The index is put down when it goes — see
    /// [`Search::forget`] — so this is a memory decision as well as a
    /// visible one.
    pub find_open: bool,
    /// What is in the field, which is not the same as what has been searched
    /// for: the field is ahead of the scan by however long a keystroke takes
    /// to reach it.
    pub find_query: String,
    /// How many times the find bar has been asked for. Every ⌘F selects the
    /// whole query, as Firefox and Chrome do — so pressing it again is the
    /// way to type a new search over the old one — and a new number is what
    /// makes Dioxus write the attribute that asks for it. See [`place_carets`].
    pub find_asked: u32,
    /// Whether every match is painted or only the one the reader is on. The
    /// third search switch, and the one that changes nothing about what is
    /// found — which is why it lives here and not in [`crate::search`].
    pub highlight_all: bool,
    /// Which scan is running. A keystroke starts a new one and the task
    /// driving the old one stops at its next slice, which is what `run` in
    /// `search.ts` does with the same two lines.
    scan: u64,
    /// Whether this search has already taken the reader to its first match.
    /// Once, and only for the first result to arrive — after that the reader
    /// is moved by asking, not by the scan catching up.
    revealed: bool,
    /// Whether the reader has asked for the margins to come off. Not the
    /// same question as whether any came off — see [`Viewer::trimmed`].
    trimming: bool,
    /// How wide the window is, which is not how wide the document is: the
    /// panel takes its share first. Kept because opening the panel has to
    /// relay out against the same window.
    window_width: f64,
    /// How tall the window is, which is not the same as how tall the document
    /// area is: the difference is the chrome, and the chrome comes and goes.
    /// See [`Viewer::chrome`] and [`Viewer::fit_window`].
    window_height: f64,
    /// Whether the toolbar is on screen. A setting, so a reader who reads
    /// without one gets none next time.
    pub toolbar: bool,
    /// Full screen, as this reader last asked for it. The window is the thing
    /// that is actually in it, and this is the asking — see [`Frame`]. It is
    /// deliberately not a setting: the app remembers it because the window
    /// state is Tauri's to restore, and a reader who quit in full screen and
    /// launched into it again with no chrome would have nothing to press.
    pub full_screen: bool,
    /// Presenting: full screen with nothing else on it. Full screen plus the
    /// chrome away, held apart from both so that leaving one puts the other
    /// back the way it was.
    pub presenting: bool,
    /// Whether the window has reported full screen since presenting began.
    /// See [`Viewer::window_full`].
    presented_full: bool,
    /// Where the last run left the reader, waiting for a window to put it
    /// back in.
    ///
    /// **A held place rather than a scroll offset, because there is no
    /// viewport yet.** `Viewer::new` runs before anything is mounted, so the
    /// layout is 0×0 and every page in it zero high — a place turned into an
    /// offset there comes back as page one the moment the window says how big
    /// it is. [`Viewer::resize`] spends it on the first layout with room in it.
    ///
    /// It is also what stops the restore writing over itself:
    /// [`Viewer::remember_place`] says nothing while one is pending.
    place: Option<Anchor>,
    /// Which draft of the document this is: a recompile, a mark written in.
    /// What the markup rows are cached against. Not in the page's key —
    /// see [`Viewer::opened`] and [`crate::page::Chosen::show`].
    pub edition: u64,
    /// Which document this is, counting from the first. **In the page's key,
    /// and it is the only thing that could be.** A page keeps its texture
    /// while its key does not move — page, size, theme, view — and a
    /// different document changes none of those while changing every pixel.
    /// `generation` cannot do it: opening the sidebar bumps that, and that
    /// must not throw a texture away. `edition` no longer does it either: a
    /// new draft of the same document is drawn in place, over the old one,
    /// which is what keeps a highlight from blanking the page.
    pub opened: u64,
    /// The process's watch and this window's name in it, so that a write of
    /// this window's own is not reported back to it as news. See
    /// [`crate::watch::Watching::wrote`]. Absent in a reader with no watch.
    pub watching: Option<Arc<crate::watch::Watching>>,
    pub window: String,
    /// This window's mailbox, which is where a write on a thread of its own
    /// says it has landed. See [`Viewer::write`].
    pub post: crate::emit::Post,
    /// Who is showing what, so that a document open in another window is
    /// brought forward rather than opened here too. Absent in a reader with
    /// one window and no process around it.
    pub desk: Option<crate::windows::Desk>,
    /// The window's own door, for asking that other window forward.
    pub frame: Frame,
    /// The write in flight: where its result lands, and what to do with it.
    writing: Option<(Arc<Mutex<Option<Landed>>>, Done)>,
    /// Whether what is in flight is a reload rather than a write of ours,
    /// which is what [`Viewer::busy`] says.
    reloading: bool,
    /// The file changed while that write or reload was in flight, after its
    /// thread may already have read it: one more reload when it lands.
    reload_owed: bool,
    /// Which measuring of the margins is the current one, so that a sample
    /// taken of the last document is not laid over this one. See
    /// [`Viewer::measure_crop`].
    crop_token: u64,
    /// Whether the reader asked for the trim just now and is owed a word
    /// about what was found.
    crop_asked: bool,
    /// The markup list as last built, with the edition and journal reading
    /// it was built for. See [`Viewer::markup_rows`].
    mark_rows: RefCell<Option<(u64, u64, Vec<MarkRow>)>>,
}

impl Viewer {
    pub fn new(document: Arc<dyn PageSource>, chosen: Chosen, store: Store) -> Self {
        let sizes = (0..document.pages())
            .map(|index| document.size_of(index))
            .collect();
        // The reader's file over the defaults, and its complaints in front of
        // the keymap's own: `keys.rs` reports the shapes TOML can describe and
        // this side cannot use, `keymap` reports the chords and the action
        // names — and to a reader looking at one line at the bottom of a
        // window they are all just things wrong with `keys.toml`.
        let file = store.keyboard();
        let mut keymap = Keymap::build(crate::keymap::this_machine(), &file.bindings);
        keymap.problems = file
            .problems
            .into_iter()
            .chain(keymap.problems.drain(..))
            .collect();
        chosen.show(document.clone());
        let mut viewer = Viewer {
            layout: Layout::new(sizes),
            scroll_top: 0.0,
            across: 0.5,
            sidebar_open: false,
            results_borrowed: false,
            arming: None,
            last_pair: Spread::Cover,
            tab_before_results: None,
            offered_results: false,
            note_open: None,
            colours_open: false,
            locked: None,
            details_open: false,
            editing: None,
            editing_from: None,
            signing: None,
            placing: None,
            said_rewrites: false,
            turned_at: None,
            pill_up: false,
            pill_token: 0,
            relaid_at: f64::NAN,
            bar_up: false,
            bar_token: 0,
            scrolls: 0,
            peek: false,
            borrowed_toolbar: false,
            sidebar_width: 252.0,
            resize_from: None,
            bar_from: None,
            still_from: None,
            still_at: (0.0, 0.0),
            still_token: 0,
            tab: Tab::Contents,
            menu: None,
            pane: None,
            pane_last: Pane::Reading,
            thumb_scroll: 0.0,
            column: Column::default(),
            headings: document.outline(),
            picked_heading: None,
            labels: document.labels(),
            links: RefCell::new(HashMap::new()),
            notes: RefCell::new(HashMap::new()),
            texts: RefCell::new(Vec::new()),
            selection: None,
            markup: Vec::new(),
            standing: crate::markup::Standing::default(),
            said_standing: false,
            markup_at: None,
            deleting_theme: None,
            mark_open: None,
            picking: None,
            pressed_on: None,
            sweep_from: None,
            sweep_seed: (
                Unit::Char,
                Spot { page: 0, index: 0 },
                Spot { page: 0, index: 0 },
            ),
            pressed: None,
            zoom_from: None,
            zoom_token: 0,
            pinching: false,
            past: Vec::new(),
            future: Vec::new(),
            typing_page: false,
            page_typed: String::new(),
            page_fresh: false,
            search: Search::new(),
            find_open: false,
            find_query: String::new(),
            find_asked: 0,
            highlight_all: true,
            scan: 0,
            revealed: false,
            trimming: false,
            window_width: 0.0,
            window_height: 0.0,
            toolbar: true,
            full_screen: false,
            presenting: false,
            presented_full: false,
            place: None,
            edition: 0,
            opened: 0,
            watching: None,
            window: String::new(),
            post: crate::emit::Post::default(),
            desk: None,
            frame: Frame::unanswered(),
            writing: None,
            reloading: false,
            reload_owed: false,
            crop_token: 0,
            crop_asked: false,
            mark_rows: RefCell::new(None),
            notice: String::new(),
            dragging: None,
            keymap,
            pending: String::new(),
            pending_at: std::time::Instant::now(),
            generation: 0,
            chosen,
            document,
            store,
        };
        // The document goes into `library.toml` before anything is restored,
        // because that is what gives a mark somewhere to live — see
        // `Store::opened`.
        let path = viewer.document.path().to_string();
        if !path.is_empty() {
            let declared = viewer.document.title();
            viewer.place = viewer.store.opened(&path, &declared);
        }
        viewer.read_markup();
        viewer
    }

    /// Put back what the last run left: settings survive, and are independent
    /// of each other.
    ///
    /// Read once and then never again — the table is this window's copy from
    /// here on, so a setting changed in another window is not seen until this
    /// one opens again.
    ///
    /// Called by `Reader` once the window's mailbox is in, because a trim
    /// restored here is measured on a thread that answers through it.
    /// The fit mode the reader chose, as the settings have it — which the
    /// layout's can differ from while a spread is fitted for the moment. See
    /// [`Viewer::set_spread`].
    fn stored_fit(&self) -> Fit {
        match self.store.text("fit_mode").as_str() {
            "page" => Fit::Page,
            "actual" => Fit::Actual,
            _ => Fit::Width,
        }
    }

    pub fn restore(&mut self) {
        self.layout.fit = self.stored_fit();
        // A zoom out of the range the ladder covers is a zoom nothing can step
        // away from, and `settings.rs` only promises the value is a number.
        self.layout.zoom = self
            .store
            .number("zoom")
            .clamp(ZOOMS[0], ZOOMS[ZOOMS.len() - 1]);
        self.layout.spread = match self.store.text("spread_mode").as_str() {
            "two" => Spread::Two,
            "cover" => Spread::Cover,
            _ => Spread::Single,
        };
        if self.layout.spread != Spread::Single {
            self.last_pair = self.layout.spread;
        }
        self.layout.gap = self.store.number("page_gap").clamp(0.0, 64.0);
        // Continuous unless the file says otherwise, and the file is the only
        // way to say otherwise: see [`Mode`].
        self.layout.mode = match self.store.text("scroll_mode").as_str() {
            "paged" => Mode::Paged,
            _ => Mode::Continuous,
        };
        self.sidebar_open = self.store.flag("show_sidebar");
        self.toolbar = self.store.flag("show_toolbar");
        // The switch is a setting and the crop is not, so a run that had it on
        // measures this document rather than putting back the last one's
        // rectangle. It is deferred to the end of `restore` because measuring
        // draws eight pages and the layout has not been built yet.
        self.trimming = self.store.flag("trim_margins");
        // Where a match is looked for is a way of reading rather than a
        // property of a document, so these outlive the find bar they are set
        // from and the session they were set in — which is the comment
        // `settings.rs` already carries above the three of them.
        self.highlight_all = self.store.flag("search_highlight_all");
        self.search.set_options(Find {
            match_case: self.store.flag("search_match_case"),
            whole_words: self.store.flag("search_whole_words"),
        });
        self.sidebar_width = self
            .store
            .number("sidebar_width")
            .clamp(crate::sidebar::MIN_WIDTH, crate::sidebar::MAX_WIDTH);
        // Which tab is showing is not a setting — the app has none either,
        // and adding one means adding a key to `settings.rs`, which this crate
        // mounts rather than edits. A document with no contents opens on the
        // pages, which is the difference between a panel and an empty box.
        if self.headings.is_empty() {
            self.tab = Tab::Pages;
        }
        self.relay_column();
        self.layout.relayout();
        if self.trimming {
            self.measure_crop();
        }
        self.chosen.set(self.store.palette());
        // Two things can be wrong with the reader's files at startup and there
        // is one line to say so in. The theme wins, because it is about what is
        // on screen right now; the keyboard's is still there at the next
        // keystroke that does nothing.
        if let Some(said) = self
            .store
            .complaint
            .clone()
            .or_else(|| self.keymap.complaint())
        {
            self.notice = said;
        }
    }

    pub fn page(&self) -> usize {
        self.layout.page_at(self.scroll_top)
    }

    pub fn pages(&self) -> usize {
        self.layout.pages()
    }

    /// The theme in use, as colours. The name beside it is the theme's, and
    /// the toolbar shows that rather than this.
    pub fn palette(&self) -> Palette {
        self.store.palette()
    }

    pub fn theme_name(&self) -> String {
        self.store.theme().name.clone()
    }

    /// The window changed size, or the sidebar did. The one entry point that
    /// both relays out and puts the reader back where they were.
    ///
    /// The width handed in is the *window's*; the document gets that minus the
    /// panel, which is why opening the sidebar is a resize.
    pub fn resize(&mut self, width: f64, height: f64) {
        self.window_width = width;
        let width = self.document_width();
        let settled = (self.layout.viewport.width - width).abs() < 0.5
            && (self.layout.viewport.height - height).abs() < 0.5;
        // A window that has not changed size still owes the reader their
        // place, so this is the one thing that gets past the early return.
        if settled && self.place.is_none() {
            return;
        }
        let anchor = self.layout.anchor(self.scroll_top);
        self.layout.viewport = Size { width, height };
        self.layout.relayout();
        self.scroll_top = self.layout.scroll_target(anchor);
        self.relaid_at = self.scroll_top;
        // The column is laid out for a panel of a width and clamped against a
        // panel of a height, and this is where both of those move.
        self.relay_column();
        self.reveal_thumb();
        // And where the last run left off, spent on the first window this
        // reader gets: a place is a page and a fraction of it, and turning
        // that into a scroll offset needs pages that have been laid out. It
        // goes through `go_to` rather than through `scroll_target` because in
        // paged mode arriving at a page is a relayout — see [`Viewer::go_to`].
        if let Some(place) = self.place.take() {
            self.go_to(place);
            self.relaid_at = self.scroll_top;
        }
    }

    /// How much of the window the document has: everything the panel is not
    /// standing on.
    fn document_width(&self) -> f64 {
        // Presenting takes the panel with the rest of the chrome, and it does
        // so here rather than by closing it: a reader who stops presenting
        // gets back the sidebar they had open, which is the whole difference
        // between hiding something and turning it off.
        let panel = if self.sidebar_open && !self.presenting {
            self.sidebar_width
        } else {
            0.0
        };
        (self.window_width - panel).max(120.0)
    }

    /* ------------------------------------------------------- the sidebar */

    /// Open or shut the panel, and remember which.
    pub fn toggle_sidebar(&mut self) {
        // A panel the reader has now taken hold of is theirs, whoever opened
        // it: closing the find bar must not take away something that was
        // asked for in between. See `results_borrowed`.
        self.results_borrowed = false;
        self.set_sidebar(!self.sidebar_open, true);
    }

    /// The panel, opened or shut. `remember` is the difference between a fact
    /// about the reader and a fact about what is on screen: a panel the reader
    /// opened is a setting, and one the search borrowed is not.
    fn set_sidebar(&mut self, open: bool, remember: bool) {
        if self.sidebar_open == open {
            return;
        }
        self.sidebar_open = open;
        if remember {
            self.store.set(vec![("show_sidebar".into(), json!(open))]);
        }
        // The document is now a different width, and the reader should be
        // looking at the same words afterwards — which is exactly what
        // `resize` promises, so this goes through it rather than around it.
        let (width, height) = (self.window_width, self.layout.viewport.height);
        self.layout.viewport.width = -1.0;
        self.resize(width, height);
        self.generation += 1;
        if self.sidebar_open {
            self.reveal_thumb();
        }
    }

    /// The panel's edge has been picked up, at this `client_x`.
    pub fn start_resize_sidebar(&mut self, client_x: f64) {
        self.resize_from = Some((client_x, self.sidebar_width));
    }

    /// The pointer has moved to `client_x`. A no-op outside a drag, which is
    /// what lets `onmousemove` sit on the root and fire on every move in the
    /// window without a drag costing a render it did not ask for.
    ///
    /// **Only the panel's own width moves here — the document does not.**
    /// `sidebar_width` is in `PageWidget`'s key, so relaying the document out
    /// at every width a drag passes through is a fresh pdfium render and
    /// texture upload for every mounted page, every frame, with nothing to
    /// show while either runs. `.sidebar`'s width is a plain style attribute,
    /// so the boundary still follows the pointer, and
    /// `finish_resize_sidebar` is the one deferred relayout.
    ///
    /// **The column of thumbnails is the exception.** A thumbnail is a
    /// twenty-fifth of a page in area, so the whole visible column costs less
    /// than one page — and it is directly under the pointer, which is the one
    /// place the deferral reads as a fault rather than a saving. So
    /// `relay_column` is live and the document's relayout is not.
    pub fn drag_sidebar(&mut self, client_x: f64) {
        let Some((start_x, start_width)) = self.resize_from else {
            return;
        };
        // Whole pixels, because a drag's arithmetic is 237.4px and nobody wants
        // to read it in the width's field. A typed width keeps its fraction.
        let width = (start_width + (client_x - start_x))
            .round()
            .clamp(crate::sidebar::MIN_WIDTH, crate::sidebar::MAX_WIDTH);
        if width == self.sidebar_width {
            return;
        }
        self.sidebar_width = width;
        self.relay_column();
    }

    /// The pointer let go: the one relayout the drag deferred, and the one
    /// write — same reasoning as `toggle_sidebar`, and there is no debounced
    /// write to reach for the way `main.ts`'s `setSoon` is for a zoom moving
    /// under a pinch.
    pub fn finish_resize_sidebar(&mut self) {
        if self.resize_from.take().is_some() {
            let (window_width, height) = (self.window_width, self.layout.viewport.height);
            self.layout.viewport.width = -1.0;
            self.resize(window_width, height);
            self.store
                .set(vec![("sidebar_width".into(), json!(self.sidebar_width))]);
        }
    }

    /* --------------------------------------------------------- the window */

    /// What the chrome costs on screen right now.
    ///
    /// **Only the toolbar costs anything.** The notice is a pill over the
    /// document, as in the app, and goes away by itself — as a row of the flex
    /// column it took 30px off the document whether or not it had anything to
    /// say. It still outlives the toolbar, because the message saying how to
    /// get the toolbar back is written on it. Presenting is where everything
    /// goes, which is what presenting *is*.
    pub fn chrome(&self) -> f64 {
        if self.presenting || !self.toolbar_up() {
            return 0.0;
        }
        TOOLBAR + HAIRLINE
    }

    /// A new zoom, said in the corner — only with the bar away, since with
    /// it up the zoom chip already says it, and only if the reader wants it.
    fn say_zoom(&mut self, zoom: f64) {
        if self.chrome() == 0.0 && self.store.flag("show_zoom_notice") {
            self.notice = format!("{:.0}%", zoom * 100.0);
        }
    }

    /// Whether the bar is on screen, which is not the same question as whether
    /// the reader wants it there: the page field borrows it for as long as it
    /// is in use. See [`Viewer::open_page_field`].
    pub fn toolbar_up(&self) -> bool {
        self.toolbar || self.borrowed_toolbar
    }

    /// The window is this big. What the document gets is the rest.
    ///
    /// The one place the chrome is subtracted, and a method rather than a
    /// constant because the chrome comes and goes: a subtraction at the call
    /// site only knows what was on screen when it was written.
    pub fn fit_window(&mut self, width: f64, height: f64) {
        self.window_height = height;
        self.resize(width, (height - self.chrome()).max(120.0));
    }

    /// The same window, with a different amount of it left for the document.
    fn refit(&mut self) {
        let (width, height) = (self.window_width, self.window_height);
        self.fit_window(width, height);
    }

    /// The toolbar, put away or brought back.
    ///
    /// With it gone there is nothing on screen saying how to get it back, so
    /// the notice says — reading the keymap rather than stating a chord,
    /// because `keys.toml` decides which key it is.
    pub fn toggle_toolbar(&mut self) {
        self.toolbar = !self.toolbar;
        // Asking for the bar by name ends the loan: otherwise ⌘T while the
        // page field is open reads as a no-op, and closing the field then
        // takes away a toolbar the reader had just asked for.
        self.borrowed_toolbar = false;
        // The page field goes with the bar it is in, or it held the keyboard
        // with nothing on screen and took an Escape to get out of.
        if !self.toolbar {
            self.cancel_page();
        }
        self.store
            .set(vec![("show_toolbar".into(), json!(self.toolbar))]);
        if !self.toolbar {
            let key = self
                .keymap
                .by_action
                .get(&Action::Toolbar)
                .and_then(|chords| chords.first())
                .map(|chord| crate::keymap::describe_binding(chord, crate::keymap::this_machine()));
            self.notice = match key {
                Some(key) => format!("Toolbar hidden.\n{key} brings it back"),
                // Unbound, which `keys.toml` can do: an empty list unbinds.
                // Then the sentence that names a key would be naming none.
                None => "Toolbar hidden.".to_string(),
            };
        }
        self.refit();
        self.generation += 1;
    }

    /// Full screen, as this reader is asking for it. The window is what is
    /// actually in it — see [`Frame`] — and this is the half that is the
    /// page's: nothing changes shape, because full screen is a bigger window
    /// and a bigger window is a resize like any other.
    pub fn set_full_screen(&mut self, on: bool) {
        self.full_screen = on;
    }

    /// The window says whether it is in full screen, having been resized.
    ///
    /// **Presenting ends with the full screen it lives in.** Left by the
    /// green button, presenting stayed on in an ordinary window with nothing
    /// on it. Only once presenting has been seen in full screen, because the
    /// resizes on the way *in* can still say it is not.
    pub fn window_full(&mut self, full: bool) {
        if self.presenting {
            if full {
                self.presented_full = true;
            } else if self.presented_full {
                self.present(false);
                self.full_screen = false;
            }
        } else if self.full_screen != full {
            // The green button or a tab born into full screen asks nobody:
            // without this Escape did not leave it and the switch said Off.
            self.full_screen = full;
        }
    }

    /// Presenting: full screen with nothing else on it. Answers what full
    /// screen should now be — presenting turns it on, and *stopping* puts it
    /// back to whatever the reader asked for themselves rather than turning it
    /// off, so somebody who was already in full screen stays there.
    pub fn present(&mut self, on: bool) -> bool {
        // See `toggle_toolbar`: the field is not on screen to be typed in.
        if on {
            self.cancel_page();
        }
        self.presenting = on;
        self.presented_full = false;
        self.notice = if on {
            "Presenting. Escape stops.".to_string()
        } else {
            String::new()
        };
        self.refit();
        self.generation += 1;
        on || self.full_screen
    }

    /// How to ask for this action from the keyboard, as it should be read —
    /// ⌘O on a Mac, Ctrl+O elsewhere — or nothing where it is not bound.
    ///
    /// **Read off the keymap rather than written beside the menu item**: a
    /// menu that states its own shortcut cannot show a rebound one, and a
    /// reader who unbinds ⌘O sees a menu item with no chord on it, which is
    /// true.
    pub fn chord_for(&self, action: Action) -> String {
        let bindings = self.keymap.by_action.get(&action);
        let shown = bindings.and_then(|bindings| {
            // **A bare function key is the system's on a Mac.** F11 is
            // Mission Control and F1 the brightness, so a menu naming either
            // is naming a key that will not arrive. Where an action has a
            // chord as well, that is the one to show.
            if self.keymap.mac() {
                if let Some(chord) = bindings.iter().find(|binding| binding.contains("mod+")) {
                    return Some(chord);
                }
            }
            bindings.first()
        });
        shown
            .map(|binding| crate::keymap::describe_binding(binding, self.keymap.mac()))
            .unwrap_or_default()
    }

    /// Put a menu down, or take the one that is down away. Asking for the
    /// menu that is already open closes it, which is what clicking its own
    /// button means.
    pub fn show_menu(&mut self, menu: Menu) {
        // One thing down at a time: the swatches are a menu in everything but
        // name, and two of them open at once is the thing `showPopover` in
        // `ui.ts` exists to prevent.
        self.markup_at = None;
        // **And the find bar is one of the things that go down**, because two
        // panels claiming the same corner with one of them still holding the
        // keyboard is not a place anybody meant to be. The menus are all of
        // it; the chips that merely move around the document — page arrows,
        // rotations, zoom steppers — leave the bar alone, there and here.
        //
        // **The index stays, though.** A menu opened mid-search is a zoom or a
        // theme tried on, not a search finished with, and ⌘F or ⌘G after it
        // is the same search again — read out of what is kept rather than
        // rescanned. Escape is what puts the index down.
        if self.find_open {
            self.put_find_away();
            self.search.clear();
        }
        self.menu = if self.menu == Some(menu) {
            None
        } else {
            Some(menu)
        };
    }

    /* ------------------------------------------------------- the settings */

    /// Put the Settings window up, on the page it was last left on.
    ///
    /// Coming back to the same page is what a window with a nav column is
    /// expected to do. It lives as long as the reader, not as long as the
    /// window.
    pub fn open_settings(&mut self) {
        self.close_menu();
        let was_shut = self.pane.is_none();
        self.pane = Some(self.pane_last);
        // A draft put down with the window is picked up with it.
        if was_shut {
            self.preview_draft();
        }
    }

    /// **A theme being edited goes out of sight with the window, and is kept.**
    /// Left worn, the half-made theme coloured the whole app and sat in the
    /// Theme menu; thrown away, a stray click beside the window lost the work.
    /// The draft waits for Settings to open again.
    pub fn close_settings(&mut self) -> bool {
        self.picking = None;
        if let Some(pane) = self.pane.take() {
            self.pane_last = pane;
            if self.editing.is_some() {
                self.reload_themes();
            }
            return true;
        }
        false
    }

    /// The colour picker under one of the theme editor's swatches, opened or
    /// put away. Pressing the swatch that opened it closes it, which is what
    /// every other popover in this app does.
    pub fn toggle_picker(&mut self, field: &'static str) {
        self.picking = (self.picking != Some(field)).then_some(field);
    }

    /// Take it down. `false` when it was not down, which is what lets Escape
    /// go on to the next thing it means — see [`Viewer::close_markup`], which
    /// is the same shape for the same reason.
    pub fn close_picker(&mut self) -> bool {
        self.picking.take().is_some()
    }

    pub fn show_pane(&mut self, pane: Pane) {
        self.pane = Some(pane);
        self.pane_last = pane;
    }

    /// A flag straight through to the settings file, for the switches whose
    /// whole effect is that they are written down — `remember_position` is
    /// read at the next open, `reopen_last_document` by the next launch.
    pub fn set_flag(&mut self, key: &str, on: bool) {
        self.store.set(vec![(key.to_string(), json!(on))]);
    }

    /// The gap between one page and the next, which is a distance on the
    /// screen and therefore a relayout.
    pub fn set_page_gap(&mut self, gap: f64) {
        let gap = gap.clamp(0.0, 64.0);
        if gap == self.layout.gap {
            return;
        }
        self.keeping_place(|layout| layout.gap = gap);
        self.store.set(vec![("page_gap".into(), json!(gap))]);
    }

    /// The panel's width, set from the field rather than dragged. Goes through
    /// the same relayout the drag's own ending does — see
    /// [`Viewer::finish_resize_sidebar`], and the comment there about why a
    /// whole number.
    pub fn set_sidebar_width(&mut self, width: f64) {
        let width = width.clamp(crate::sidebar::MIN_WIDTH, crate::sidebar::MAX_WIDTH);
        if width == self.sidebar_width {
            return;
        }
        self.sidebar_width = width;
        let (window_width, height) = (self.window_width, self.layout.viewport.height);
        self.layout.viewport.width = -1.0;
        self.resize(window_width, height);
        self.store.set(vec![("sidebar_width".into(), json!(width))]);
    }

    /// What the page on screen is actually drawn at, as a percentage.
    ///
    /// **Not the remembered zoom**, which in a fit mode is a number nobody is
    /// looking at. The stepper starts from what is on screen, because that is
    /// what somebody types over.
    pub fn zoom_percent(&self) -> f64 {
        self.layout
            .box_of(self.page().saturating_sub(1))
            .map(|page| page.scale / crate::layout::PDF_TO_CSS_UNITS * 100.0)
            .unwrap_or(self.layout.zoom * 100.0)
    }

    /// A fixed zoom, typed rather than stepped. The pair that never moves
    /// alone — see [`Viewer::zoom`].
    pub fn set_zoom(&mut self, zoom: f64) {
        // A zoom asked for outright ends any gesture that was running, or the
        // pages would hold a stale texture until its timer came round.
        self.zoom_token += 1;
        self.zoom_from = None;
        self.chosen.hold(false);
        let zoom = zoom.clamp(0.25, 6.0);
        self.keeping_place(|layout| {
            layout.fit = Fit::Actual;
            layout.zoom = zoom;
        });
        self.store.set(vec![
            ("zoom".into(), json!(zoom)),
            ("fit_mode".into(), json!(name_of(Fit::Actual))),
        ]);
    }

    /// Zoom by a proportion of where we are, which is what a pinch asks for.
    ///
    /// **A pinch and a ⌃-wheel cannot be steps on the ladder**: a trackpad
    /// sends a stream of small events and a mouse one large one, so stepping
    /// on each took 125% to 400% in one gesture. Leaving a fit mode starts
    /// from the size the fit had reached, so the first squeeze moves the page
    /// a little rather than jumping to the remembered zoom.
    ///
    /// Written through `set_soon`: a pinch produces one of these a frame and
    /// the file only needs the one it ends on.
    pub fn zoom_by(&mut self, factor: f64) {
        // A pinch arrives whatever the window holds, and with no page to
        // measure this made "actual size" of nothing and saved it.
        if self.empty() {
            return;
        }
        let current = if self.layout.fit == Fit::Actual {
            self.layout.zoom
        } else {
            self.layout
                .box_of(self.page() - 1)
                .map(|page| page.scale / crate::layout::PDF_TO_CSS_UNITS)
                .unwrap_or(1.0)
        };
        let next = (current * factor).clamp(0.25, 6.0);
        if (next - current).abs() < 0.0005 {
            return;
        }
        // **The gesture begins here and is what keeps the document on screen
        // while it lasts.** Every page holds the texture it has and is
        // stretched to the size the layout asks for, so the words grow under
        // the fingers instead of the document going blank until they stop.
        // The page key divides by `zoom_from` to stay one key for the whole
        // gesture — see [`crate::page::Chosen::holding`].
        if self.zoom_from.is_none() {
            self.zoom_from = Some(current);
            self.chosen.hold(true);
        }
        self.zoom_token += 1;
        self.keeping_place(|layout| {
            layout.fit = Fit::Actual;
            layout.zoom = next;
        });
        self.say_zoom(next);
        self.store.set_soon(vec![
            ("zoom".into(), json!(next)),
            ("fit_mode".into(), json!(name_of(Fit::Actual))),
        ]);
    }

    /// Which gesture is running, for the timer that ends it.
    /// How many scrolls have been asked for, moved or not.
    ///
    /// What the page pill watches, beside the offset. A wheel at either end of
    /// the document changes nothing and is still a gesture the reader made, and
    /// the app it was ported from flashed the pill for it — `flashPill` hung
    /// off the wheel handler rather than off the offset.
    pub fn scroll_gesture(&self) -> u64 {
        self.scrolls
    }

    pub fn zoom_gesture(&self) -> Option<u64> {
        self.zoom_from.map(|_| self.zoom_token)
    }

    /// How much smaller than the box a page is currently drawn, while a zoom
    /// gesture is under way: 1.0 when there is none. Every page's key is
    /// multiplied by it, so the key does not move while the gesture does.
    pub fn zoom_held_at(&self) -> f64 {
        match (self.zoom_from, self.layout.fit) {
            (Some(from), Fit::Actual) if self.layout.zoom > 0.0 => from / self.layout.zoom,
            _ => 1.0,
        }
    }

    /// The fingers stopped. Draw the pages again, at the size they are now.
    ///
    /// Guarded by the token for the reason [`Viewer::unflash_pill`] is: a
    /// gesture that went on past the settle is a second timer, and the first
    /// one must not end it. And not while a pinch is under way: two fingers
    /// resting on the trackpad send nothing, and a timer that ended the
    /// gesture in that pause redrew every page at the size the pause was at
    /// — and again when the fingers lifted. The pinch says when it ends.
    pub fn settle_zoom(&mut self, token: u64) {
        if self.zoom_token != token || self.pinching {
            return;
        }
        self.zoom_from = None;
        self.chosen.hold(false);
    }

    /// Two fingers moved on the trackpad. See [`Viewer::zoom_by`]; the
    /// difference is that the gesture ends with [`Viewer::end_pinch`] rather
    /// than with the timer.
    pub fn pinch_by(&mut self, factor: f64) {
        self.pinching = true;
        self.zoom_by(factor);
    }

    /// The fingers lifted.
    pub fn end_pinch(&mut self) {
        self.pinching = false;
        self.zoom_from = None;
        self.chosen.hold(false);
    }

    /// Whether a picture on a recoloured page is recoloured with it.
    ///
    /// It is in the *palette* rather than beside it — `resolve` takes it — so
    /// changing it is a new palette and every drawn page is stale. See
    /// `store.rs`, where the same flag is read.
    pub fn set_recolor_images(&mut self, on: bool) {
        self.store.set(vec![("recolor_images".into(), json!(on))]);
        self.chosen.set(self.store.palette());
        self.generation += 1;
    }

    /// …and what it is set to, which the settings menu shows.
    pub fn recolor_images(&self) -> bool {
        self.store.flag("recolor_images")
    }

    /// Whether the page pill appears while the reader scrolls with the
    /// toolbar away. See the pill in `Reader` — `show_page_pill` in the app.
    pub fn page_pill(&self) -> bool {
        self.store.flag("show_page_pill")
    }

    /// Whether the pointer goes away when it has been left alone. Off unless
    /// the reader asked for it — see the default in `settings.rs`.
    pub fn hides_cursor(&self) -> bool {
        self.store.flag("hide_cursor")
    }

    /// And how long it has to sit still first. Zero is allowed and means "as
    /// soon as it stops", which is a fifth of a second in practice: a pointer
    /// being moved slowly goes quiet between events for longer than one turn
    /// of the clock, and hiding it in those gaps would make it flicker.
    pub fn cursor_rests(&self) -> std::time::Duration {
        // A hand-edited 1e20 is past what a `Duration` holds, and panicked.
        std::time::Duration::from_secs_f64(
            self.store.number("hide_cursor_after").clamp(0.2, 3600.0),
        )
    }

    pub fn set_cursor_rest(&mut self, seconds: f64) {
        self.store
            .set(vec![("hide_cursor_after".into(), json!(seconds))]);
    }

    pub fn set_page_pill(&mut self, on: bool) {
        self.store.set(vec![("show_page_pill".into(), json!(on))]);
    }

    /// Whether a page is called by the number printed on it rather than by
    /// its place in the file. The setting is `page_numbering`.
    pub fn numbering_printed(&self) -> bool {
        self.store.text("page_numbering") != "position"
    }

    pub fn set_page_numbering(&mut self, printed: bool) {
        let value = if printed { "printed" } else { "position" };
        self.store
            .set(vec![("page_numbering".into(), json!(value))]);
    }

    /// Whether this document has numbers of its own to show — which is when
    /// the choice above is worth offering at all.
    pub fn has_own_numbering(&self) -> bool {
        !self.labels.is_empty()
    }

    /// The labels in force: the document's own, unless the reader has asked
    /// for positions, in which case there are none.
    fn labels(&self) -> &[String] {
        if self.numbering_printed() {
            &self.labels
        } else {
            &[]
        }
    }

    /// Read `keys.toml` again, exactly as the launch did.
    ///
    /// A button rather than a watcher: the config directory is written to
    /// several times a minute while somebody is scrolling, so a watch on it
    /// would be answering its own writes.
    pub fn reload_keys(&mut self) {
        let file = self.store.keyboard();
        let mut keymap = Keymap::build(crate::keymap::this_machine(), &file.bindings);
        keymap.problems = file
            .problems
            .into_iter()
            .chain(keymap.problems.drain(..))
            .collect();
        // The page redraws either way, so the line is what says it happened:
        // a file with nothing wrong in it redraws to exactly what was there.
        self.notice = match keymap.problems.len() {
            0 => "Keys reloaded.".to_string(),
            one => format!("Keys reloaded. {one} could not be used — below."),
        };
        self.keymap = keymap;
    }

    /// Whatever is down, put away. Answers whether there was anything, so
    /// that Escape can fall through to the next thing when there was not.
    pub fn close_menu(&mut self) -> bool {
        self.menu.take().is_some()
    }

    /// A panel the search borrowed becomes the reader's the moment they
    /// reach for one of its other tabs: that is asking for the sidebar, and
    /// the press that closes the find bar must not take it down with it.
    pub fn adopt_sidebar(&mut self) {
        if std::mem::take(&mut self.results_borrowed) {
            self.store.set(vec![("show_sidebar".into(), json!(true))]);
        }
    }

    pub fn show_tab(&mut self, tab: Tab) {
        self.tab = tab;
        if tab == Tab::Pages {
            self.reveal_thumb();
        }
    }

    /// How tall the thumbnail panel is: the row the sidebar shares with the
    /// document, less the tab strip above it. Everything that scrolls the
    /// column asks this rather than the viewport, or the foot of the last
    /// thumbnail is unreachable — see [`crate::sidebar::TABS`].
    pub fn thumb_panel(&self) -> f64 {
        (self.layout.viewport.height - crate::sidebar::TABS).max(1.0)
    }

    /// Lay the thumbnail column out for the panel as it stands.
    fn relay_column(&mut self) {
        let sizes: Vec<Size> = (0..self.layout.pages())
            .map(|index| self.layout.size_of(index))
            .collect();
        self.column = Column::new(&sizes, self.sidebar_width);
        self.thumb_scroll = self
            .thumb_scroll
            .clamp(0.0, self.column.max_scroll(self.thumb_panel()));
    }

    pub fn scroll_thumbs(&mut self, delta: f64) {
        let height = self.thumb_panel();
        self.thumb_scroll = (self.thumb_scroll + delta).clamp(0.0, self.column.max_scroll(height));
    }

    /// Bring the page the reader is on into the column, if it is not there
    /// already. See `Column::reveal`: doing nothing is most of the behaviour.
    fn reveal_thumb(&mut self) {
        if !self.sidebar_open || self.tab != Tab::Pages {
            return;
        }
        let height = self.thumb_panel();
        if let Some(to) = self
            .column
            .reveal(self.page() - 1, self.thumb_scroll, height)
        {
            self.thumb_scroll = to;
        }
    }

    /// Go to a page, one-based — what a row in either list does when it is
    /// clicked, and a *jump*: see [`Viewer::jump_to`].
    pub fn go_to_page(&mut self, page: usize) {
        self.jump_to(page, 0.0);
    }

    /// The row after this one, not the page: side by side, the page after
    /// the left one is on the same row, and "next" went nowhere. A step, not
    /// a jump — it leaves no history behind.
    pub fn next_page(&mut self) {
        let row = self.layout.row_of(self.page() - 1);
        let next = row.last().map_or(self.page() + 1, |last| last + 2);
        // Past the last row there is only the rest of the last page, and
        // `scroll_target` clamping to its top sent the reader back up it.
        if next > self.pages() {
            self.scroll_to(self.layout.max_scroll());
            return;
        }
        self.go_to(Anchor {
            page: next,
            offset: 0.0,
        });
    }

    /// The row before this one; see [`Viewer::next_page`].
    pub fn previous_page(&mut self) {
        let row = self.layout.row_of(self.page() - 1);
        let previous = row.first().copied().unwrap_or(0).max(1);
        self.go_to(Anchor {
            page: previous,
            offset: 0.0,
        });
    }

    /// Land at a place in the document, turning the page first if that is
    /// what landing means here.
    ///
    /// **The one place the two scroll modes differ to a caller.** In
    /// continuous mode a page is somewhere to scroll to; in paged mode it is
    /// the only page laid out, so arriving is a relayout. Everything that
    /// moves the reader goes through this, so nothing else has to know which
    /// mode is on.
    pub fn go_to(&mut self, anchor: Anchor) {
        if self.layout.mode == Mode::Paged {
            let page = anchor.page.clamp(1, self.pages().max(1));
            if page != self.layout.current {
                self.layout.current = page;
                self.layout.relayout();
                self.generation += 1;
                self.scroll_top = 0.0;
                self.reveal_thumb();
                // A page turn where both sides sit at the top of their page
                // does not move `scroll_top` at all, so the write cannot be
                // left to `scroll_to` — and the page is the whole of what
                // there is to remember in this mode.
                self.remember_place();
            }
        }
        let target = self.layout.scroll_target(anchor);
        self.scroll_to(target);
    }

    /// The start and the end of the document, which are not the same thing as
    /// the top and the bottom of what is laid out.
    pub fn to_start(&mut self) {
        match self.layout.mode {
            Mode::Paged => self.go_to(Anchor {
                page: 1,
                offset: 0.0,
            }),
            Mode::Continuous => {
                self.scroll_to(0.0);
            }
        }
    }

    pub fn to_end(&mut self) {
        match self.layout.mode {
            Mode::Paged => {
                let last = self.pages().max(1);
                self.go_to(Anchor {
                    page: last,
                    offset: 0.0,
                });
                let bottom = self.layout.max_scroll();
                self.scroll_to(bottom);
            }
            Mode::Continuous => {
                let bottom = self.layout.max_scroll();
                self.scroll_to(bottom);
            }
        }
    }

    /// Whether there is nowhere left to scroll the page this way, which in
    /// paged mode is where a page turn begins.
    fn at_edge(&self, delta: f64) -> bool {
        if delta > 0.0 {
            self.scroll_top >= self.layout.max_scroll() - 1.0
        } else {
            self.scroll_top <= 1.0
        }
    }

    /// The same, asked by a wheel: one flick is a stream of events and the
    /// page is turned by the first of them. See [`TURN_GAP`].
    pub fn wheel_nudge(&mut self, delta: f64) {
        if self.layout.mode == Mode::Paged
            && self.at_edge(delta)
            && self.turned_at.is_some_and(|then| then.elapsed() < TURN_GAP)
        {
            return;
        }
        self.nudge(delta);
    }

    /// Move by a distance, and turn the page when there is nowhere left to
    /// move. One function where the app has two, because a key and a wheel ask
    /// the same question.
    ///
    /// In paged mode the strip holds one page, so a page that fits cannot be
    /// scrolled and a taller one stops at its own bottom edge — either way the
    /// reader pushes and nothing happens, which is the gesture everybody tries
    /// first.
    pub fn nudge(&mut self, delta: f64) {
        if self.layout.mode == Mode::Continuous || delta == 0.0 {
            let to = self.scroll_by(delta);
            self.scroll_to(to);
            return;
        }
        if !self.at_edge(delta) {
            let to = self.scroll_by(delta);
            self.scroll_to(to);
            return;
        }
        let page = self.layout.current;
        let next = if delta > 0.0 {
            self.layout
                .row_of(page - 1)
                .last()
                .copied()
                .unwrap_or(page - 1)
                + 2
        } else {
            // The first of this row, counted from nought, is the number of
            // the page before it — and nought is "there is none". After a jump
            // `current` is as often the right-hand page, and the page before
            // *that* is the same spread.
            self.layout.row_of(page - 1).first().copied().unwrap_or(0)
        };
        if next < 1 || next > self.pages() {
            return;
        }
        self.turned_at = Some(std::time::Instant::now());
        self.go_to(Anchor {
            page: next,
            offset: 0.0,
        });
        // Backwards is the *bottom* of the page arrived at, because that is
        // where the reader was reading: turning back to the top of the
        // previous page skips everything they were about to re-read.
        if delta < 0.0 {
            let bottom = self.layout.max_scroll();
            self.scroll_to(bottom);
        }
    }

    /// One page at a time, or continuous.
    ///
    /// A setting and nothing else — there is no shortcut and no chip, which
    /// is the brief's own instruction: continuous is a strong default and a
    /// key that leaves it by accident is the failure it names. See [`Mode`].
    pub fn set_scroll_mode(&mut self, mode: Mode) {
        if mode == self.layout.mode {
            return;
        }
        let here = self.layout.anchor(self.scroll_top);
        self.layout.mode = mode;
        self.layout.current = here.page;
        self.layout.relayout();
        self.scroll_top = self.layout.scroll_target(here);
        self.relaid_at = self.scroll_top;
        self.generation += 1;
        self.store.set(vec![(
            "scroll_mode".into(),
            json!(match mode {
                Mode::Paged => "paged",
                Mode::Continuous => "continuous",
            }),
        )]);
    }

    /* ------------------------------------------------------ where you were */

    /// Go somewhere the reader asked to go, remembering where they were.
    ///
    /// The citation on page 12 that lands on page 190 is what the history
    /// exists for; the twenty keystrokes of scrolling that reached page 12 are
    /// not. And a jump that lands where the reader already is is not a jump —
    /// without that test, Home twice files the first page away as somewhere
    /// worth returning to.
    pub fn jump_to(&mut self, page: usize, offset: f64) {
        let from = self.layout.anchor(self.scroll_top);
        let to = page.clamp(1, self.pages().max(1));
        if to == from.page && (offset - from.offset).abs() < 0.01 {
            self.go_to(Anchor { page: to, offset });
            return;
        }
        self.past.push(from);
        if self.past.len() > HISTORY_LIMIT {
            self.past.remove(0);
        }
        // A jump made after stepping back throws away what was ahead, which
        // is what every back button does and what nobody is surprised by.
        self.future.clear();
        self.go_to(Anchor { page: to, offset });
    }

    /// Back to where the last jump started, or forward again.
    ///
    /// `false` when there is nowhere to go, so that the caller can say so:
    /// silence at the end of the history is indistinguishable from a key that
    /// does not work.
    pub fn go_back(&mut self) -> bool {
        let Some(place) = self.past.pop() else {
            return false;
        };
        self.future.push(self.layout.anchor(self.scroll_top));
        self.go_to(place);
        true
    }

    pub fn go_forward(&mut self) -> bool {
        let Some(place) = self.future.pop() else {
            return false;
        };
        self.past.push(self.layout.anchor(self.scroll_top));
        self.go_to(place);
        true
    }

    /* ------------------------------------------------------------- links */

    /// The links on a page, asked for once and kept. See [`Viewer::links`].
    pub fn links_on(&self, index: usize) -> Rc<Vec<Link>> {
        if let Some(known) = self.links.borrow().get(&index) {
            return known.clone();
        }
        let links = Rc::new(self.document.links_of(index));
        self.links.borrow_mut().insert(index, links.clone());
        links
    }

    /// The links on a mounted page, as rectangles in CSS pixels from the top
    /// left of its box — which is the space the page's own overlay is laid
    /// out in, and the same one [`Viewer::highlights`] answers in.
    pub fn link_areas(&self, page: usize) -> Vec<(Rect, Target)> {
        let Some(index) = page.checked_sub(1) else {
            return Vec::new();
        };
        if self.layout.box_of(index).is_none() {
            return Vec::new();
        }
        self.links_on(index)
            .iter()
            .map(|link| (self.layout.place_on(index, link.rect), link.target.clone()))
            .collect()
    }

    /* ------------------------------------------------------------- notes */

    /// The notes on a page, asked for once and kept.
    pub fn notes_on(&self, index: usize) -> Rc<Vec<crate::render::Note>> {
        if let Some(known) = self.notes.borrow().get(&index) {
            return known.clone();
        }
        let notes = Rc::new(self.document.notes_of(index));
        self.notes.borrow_mut().insert(index, notes.clone());
        notes
    }

    /// The notes on a mounted page, placed in the same space as the links.
    pub fn note_areas(&self, page: usize) -> Vec<(Rect, crate::render::Note)> {
        let Some(index) = page.checked_sub(1) else {
            return Vec::new();
        };
        if self.layout.box_of(index).is_none() {
            return Vec::new();
        }
        self.notes_on(index)
            .iter()
            .map(|note| (self.layout.place_on(index, note.rect), note.clone()))
            .collect()
    }

    /// Open one, which is a window rather than a tooltip: a note can be a
    /// paragraph, and `title` is a tooltip's whole vocabulary.
    pub fn open_note(&mut self, page: usize, note: crate::render::Note) {
        self.note_open = Some((page, note));
    }

    pub fn close_note(&mut self) -> bool {
        self.note_open.take().is_some()
    }

    /// What has been typed into the password field so far.
    ///
    /// Kept here because the field does not hold it: Blitz renders
    /// `type="password"` in the clear, so what is on screen is a row of
    /// bullets and this is the string they stand for. [`Reader`] keeps the two
    /// in step.
    pub fn type_password(&mut self, typed: &str) {
        if let Some(locked) = self.locked.as_mut() {
            locked.typed = typed.to_string();
        }
    }

    /// "Not now": the question is withdrawn and the reader is left with
    /// whatever they had — at launch, the start screen.
    ///
    /// **Declining is not answering with an empty password.** A reader who has
    /// decided not to open this document must not be asked again on the way
    /// out of the question.
    pub fn stop_unlocking(&mut self) -> bool {
        if self.locked.take().is_none() {
            return false;
        }
        // The desk was told this window shows the document it was asking
        // about; it shows what it showed before, or nothing.
        self.frame.ask(Ask::Showing {
            path: if self.empty() {
                String::new()
            } else {
                self.document.path().to_string()
            },
            title: self.store.title().to_string(),
        });
        true
    }

    /// The reader has answered. Answers whether the document opened, because
    /// the caller has the same bookkeeping to do as [`Self::open_here`]'s.
    pub fn unlock(&mut self) -> bool {
        let Some(locked) = self.locked.clone() else {
            return false;
        };
        self.open_here_with(&locked.path, Some(&locked.typed))
    }

    /// What the document says about itself, in the app's own order — see
    /// [`crate::render::PageSource::details`]. The name and the file are the
    /// reader's rather than the document's, so they are added here.
    pub fn details(&self) -> Vec<(String, String)> {
        let mut rows = self.document.details();
        if !self.document.path().is_empty() {
            rows.push(("File".into(), self.document.path().to_string()));
        }
        rows
    }

    /* --------------------------------------------------------- the editor */

    /// Begin a theme: a new one, or a copy of the one being worn.
    ///
    /// **The draft is installed as the live theme**, which is what makes the
    /// window around it the preview. A built-in keeping its own id would be
    /// overwritten on save, so a copy gets an empty id and `theme::save` mints
    /// one.
    pub fn begin_theme(&mut self, from: Option<crate::theme::Theme>) {
        let draft = match from {
            Some(theme) if theme.built_in => crate::theme::Theme {
                id: String::new(),
                name: format!("{} copy", theme.name),
                built_in: false,
                ..theme
            },
            Some(theme) => theme,
            None => {
                let worn = self.store.theme().clone();
                crate::theme::Theme {
                    id: String::new(),
                    name: "New theme".into(),
                    built_in: false,
                    ..worn
                }
            }
        };
        self.editing_from = Some(draft.clone());
        self.editing = Some(draft);
        self.preview_draft();
    }

    /// Whether the draft differs from what is on disk. A theme with no id has
    /// never been saved, so it always does.
    pub fn draft_unsaved(&self) -> bool {
        match &self.editing {
            Some(draft) => draft.id.trim().is_empty() || self.editing_from.as_ref() != Some(draft),
            None => false,
        }
    }

    /// What is in the draft, worn without being remembered — `wear_for_now`,
    /// which is the same door the harness's theme override uses.
    pub fn preview_draft(&mut self) {
        let Some(draft) = self.editing.clone() else {
            return;
        };
        // **Without the draft that was previewed a keystroke ago.** A theme
        // that is not yet on disk has no id, so the search below never finds
        // the copy of it left by the last press of a key and appended another —
        // and a reader naming a theme Brownie watched B, Br, Bro and four more
        // pile up in the theme list, all of them the same unsaved theme. An id
        // is what a saved theme has; nothing else belongs in that list.
        let mut themes: Vec<crate::theme::Theme> = self
            .store
            .themes()
            .iter()
            .filter(|theme| !theme.id.trim().is_empty())
            .cloned()
            .collect();
        // The draft stands in for the theme it is a version of, or is added
        // to the end when it is a new one.
        match themes
            .iter()
            .position(|theme| theme.id == draft.id && !draft.id.is_empty())
        {
            Some(at) => themes[at] = draft,
            None => themes.push(draft),
        }
        let at = themes.len() - 1;
        let at = self
            .editing
            .as_ref()
            .and_then(|draft| {
                themes
                    .iter()
                    .position(|theme| theme.id == draft.id && !draft.id.is_empty())
            })
            .unwrap_or(at);
        self.store.set_themes(themes);
        self.store.wear_for_now(at);
        self.chosen.set(self.store.palette());
        self.generation += 1;
    }

    /// Change one field of the draft and show it. The field's name is the
    /// theme file's own, which is what keeps this one function rather than
    /// seven.
    pub fn draft_set(&mut self, field: &str, value: String) {
        let Some(draft) = self.editing.as_mut() else {
            return;
        };
        let some = |value: String| (!value.trim().is_empty()).then_some(value);
        match field {
            "name" => draft.name = value,
            "text" => draft.text = value,
            "background" => draft.background = value,
            "accent" => draft.accent = some(value),
            "link" => draft.link = some(value),
            "selection_area" => draft.selection_area = some(value),
            "selection_text" => draft.selection_text = some(value),
            _ => return,
        }
        self.preview_draft();
    }

    pub fn draft_recolor(&mut self, on: bool) {
        if let Some(draft) = self.editing.as_mut() {
            draft.recolor = on;
        }
        self.preview_draft();
    }

    /// Put the draft down and go back to what was on before it.
    pub fn cancel_theme(&mut self) {
        self.editing = None;
        self.picking = None;
        self.reload_themes();
    }

    /// Write the draft to its file, wear it for real, and close the editor.
    pub fn save_theme(&mut self) {
        let Some(draft) = self.editing.clone() else {
            return;
        };
        let dir = self.store.themes_dir().to_path_buf();
        match crate::theme::save(&dir, &draft) {
            Ok(saved) => {
                // **A theme that moved takes its references with it.** The
                // file is named for the theme, so renaming one renames the
                // file — see `theme::save` — and the two remembered halves of
                // the light/dark pair name themes by that id. Left alone, a
                // renamed dark theme stops being the dark theme.
                if saved.id != draft.id && !draft.id.trim().is_empty() {
                    let moving: Vec<(String, serde_json::Value)> =
                        ["theme", "light_theme", "dark_theme"]
                            .into_iter()
                            .filter(|key| self.store.text(key) == draft.id)
                            .map(|key| (key.to_string(), serde_json::json!(saved.id)))
                            .collect();
                    if !moving.is_empty() {
                        self.store.set(moving);
                    }
                }
                self.editing = None;
                self.reload_themes();
                let at = self
                    .store
                    .themes()
                    .iter()
                    .position(|theme| theme.id == saved.id);
                self.notice.clear();
                if let Some(at) = at {
                    self.set_theme(at);
                }
                // Worn against the system, it holds only until the system
                // switches, and that is the half of the news they cannot see.
                let also = if self.notice.ends_with(UNTIL_THE_SYSTEM_SWITCHES) {
                    format!(" {UNTIL_THE_SYSTEM_SWITCHES}")
                } else {
                    String::new()
                };
                self.notice = format!("Saved {}.{also}", saved.name);
            }
            Err(said) => self.notice = said,
        }
    }

    /// A theme file the reader chose, added to their own themes and worn.
    pub fn import_theme(&mut self, path: &str) {
        // Imported from the editor, the draft — saved, or the button would not
        // have been live — is put down so the imported theme can be worn.
        if self.editing.is_some() {
            self.cancel_theme();
        }
        let dir = self.store.themes_dir().to_path_buf();
        let imported = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|source| crate::theme::import(&dir, &source));
        match imported {
            Ok(theme) => {
                self.reload_themes();
                let at = self
                    .store
                    .themes()
                    .iter()
                    .position(|worn| worn.id == theme.id);
                self.notice.clear();
                if let Some(at) = at {
                    self.set_theme(at);
                }
                // Worn against the system, it holds only until the system
                // switches, and that is the half of the news they cannot see.
                let also = if self.notice.ends_with(UNTIL_THE_SYSTEM_SWITCHES) {
                    format!(" {UNTIL_THE_SYSTEM_SWITCHES}")
                } else {
                    String::new()
                };
                self.notice = format!("Imported {}.{also}", theme.name);
            }
            Err(said) => self.notice = said,
        }
    }

    /// The theme being worn, written where the reader chose.
    pub fn export_theme(&mut self, path: &str) {
        let theme = self.store.theme().clone();
        self.notice = match crate::theme::to_toml(&theme)
            .and_then(|body| crate::atomic_write(std::path::Path::new(path), body.as_bytes()))
        {
            Ok(()) => format!("Exported {}.", theme.name),
            Err(said) => said,
        };
    }

    /// Ask whether to delete `theme`, in a window of its own.
    pub fn ask_delete_theme(&mut self, theme: crate::theme::Theme) {
        self.menu = None;
        self.deleting_theme = Some(theme);
    }

    /// Put the question away without deleting anything.
    pub fn close_delete_theme(&mut self) -> bool {
        self.deleting_theme.take().is_some()
    }

    /// The answer was yes.
    pub fn confirm_delete_theme(&mut self) {
        if let Some(theme) = self.deleting_theme.take() {
            self.begin_theme(Some(theme));
            self.delete_theme();
        }
    }

    /// Delete the theme being edited, which is only ever one that is on disk.
    pub fn delete_theme(&mut self) {
        let Some(draft) = self.editing.clone() else {
            return;
        };
        if draft.id.trim().is_empty() {
            self.cancel_theme();
            return;
        }
        let dir = self.store.themes_dir().to_path_buf();
        match crate::theme::delete(&dir, &draft.id) {
            Ok(()) => {
                self.editing = None;
                self.reload_themes();
                let replacement = self.store.replacement_for(&draft).unwrap_or(0);
                self.set_theme(replacement);
                self.notice = format!("Deleted {}.", draft.name);
            }
            Err(said) => self.notice = said,
        }
    }

    /// The themes directory, read again. What every one of the four above
    /// ends with, because each has changed what is in it or what is worn.
    fn reload_themes(&mut self) {
        let dir = self.store.themes_dir().to_path_buf();
        self.store.set_themes(crate::theme::load_all(&dir));
        let worn = self.store.theme_index();
        self.store.wear_for_now(worn);
        self.chosen.set(self.store.palette());
        self.generation += 1;
    }

    pub fn open_details(&mut self) {
        self.details_open = true;
    }

    pub fn close_details(&mut self) -> bool {
        std::mem::take(&mut self.details_open)
    }

    /// Follow a link, and say what the window has to do about it.
    ///
    /// A place in this document is a jump and is done here. An address is not
    /// this struct's to open — there is no browser in a `Viewer` and none in
    /// the harness — so it is handed back, and one place decides what opening
    /// a link means.
    pub fn follow(&mut self, target: &Target) -> Option<String> {
        match target {
            Target::Place { page, offset } => {
                self.jump_to(*page, *offset);
                None
            }
            Target::Away(url) => match openable(url) {
                Some(url) => {
                    self.notice = format!("Opened {url}");
                    Some(url)
                }
                // [`Away`] drops these, and the notice said "Opened" anyway.
                None => {
                    self.notice =
                        format!("{url} is not a web or mail address, so it was not opened.");
                    None
                }
            },
        }
    }

    /* --------------------------------------------------------- selecting */

    /// A page's text, asked for once and kept — see [`Viewer::texts`].
    /// One-based, like everything that says "page" in this file.
    pub fn text_on(&self, page: usize) -> Rc<PageText> {
        if let Some(known) = self
            .texts
            .borrow()
            .iter()
            .find(|(at, _)| *at == page)
            .map(|(_, text)| text.clone())
        {
            return known;
        }
        let Some(index) = page.checked_sub(1) else {
            return Rc::new(PageText::default());
        };
        let text = Rc::new(self.document.text_of(index));
        let mut held = self.texts.borrow_mut();
        held.push((page, text.clone()));
        if held.len() > TEXT_CACHE {
            held.remove(0);
        }
        crate::stats::set(&crate::stats::TEXT_PAGES, held.len() as u64);
        text
    }

    /// The pointer went down on a page: where the sweep starts, and where the
    /// content is in the coordinates the pointer arrives in.
    ///
    /// `on` is the point within the page's own box, in CSS pixels from its top
    /// left, which is what the event carries; `client` is the same press said
    /// in the window's coordinates. The two together are the origin — see
    /// [`Viewer::sweep_from`].
    ///
    /// **A second press in the same place is a double click and takes the word
    /// under it, a third takes the line**, counted here rather than heard
    /// about: `dblclick` is a default action of `pointerup`, and a default
    /// action never runs over a custom widget. The numbers are Blitz's own, so
    /// a page and a text field in one window answer a double click alike.
    ///
    /// The unit is kept for the moves that follow: a mouse held still is not
    /// still, and the half-pixel twitch between the second press and its
    /// release used to arrive as a sweep that cut the word back to the letters
    /// before the pointer.
    pub fn begin_sweep(&mut self, page: usize, on: (f64, f64), client: (f64, f64)) {
        let Some(index) = page.checked_sub(1) else {
            return;
        };
        let Some(area) = self.layout.box_of(index) else {
            return;
        };
        self.sweep_from = Some((
            client.0 - on.0 - area.left,
            client.1 - on.1 - area.top + self.scroll_top,
        ));
        let count = match self.pressed {
            Some((when, x, y, count))
                if when.elapsed() < DOUBLE_CLICK
                    && (x - client.0).abs() <= 2.0
                    && (y - client.1).abs() <= 2.0 =>
            {
                count.saturating_add(1)
            }
            _ => 1,
        };
        self.pressed = Some((std::time::Instant::now(), client.0, client.1, count));
        // Where the press landed on the page, for [`Viewer::end_sweep`] to
        // ask what is under it when nothing was swept.
        self.pressed_on = Some((page, on.0, on.1));
        // A press anywhere puts away whatever the last one opened.
        self.mark_open = None;
        self.arming = None;
        let unit = match count {
            1 => Unit::Char,
            2 => Unit::Word,
            _ => Unit::Line,
        };
        self.sweep_unit(page, on, unit);
        // A new sweep is a new passage; the swatches offered for the last one
        // go with it.
        self.markup_at = None;
    }

    /// The pointer moved with the button down. A no-op outside a sweep, which
    /// is what lets this sit on the root and fire on every move in the window.
    pub fn sweep_to(&mut self, client: (f64, f64)) {
        let Some((left, top)) = self.sweep_from else {
            return;
        };
        let Some(mut sweep) = self.selection else {
            return;
        };
        let x = client.0 - left;
        let y = client.1 - top + self.scroll_top;
        let Some((index, on_x, on_y)) = self.layout.page_at_point(x, y) else {
            return;
        };
        let head = self.spot_on(index, on_x, on_y);
        // Whole units, from the one the sweep began on: past its start the
        // selection runs back from the unit's end, otherwise on from its start.
        let (unit, from, to) = self.sweep_seed;
        let text = self.text_on(index + 1);
        let (before, after) = crate::select::unit_around(&text, head.index, unit);
        let (anchor, head) = if head < from {
            (
                to,
                Spot {
                    page: head.page,
                    index: before,
                },
            )
        } else {
            (
                from,
                Spot {
                    page: head.page,
                    index: after,
                },
            )
        };
        if head == sweep.head && anchor == sweep.anchor {
            return;
        }
        sweep.anchor = anchor;
        sweep.head = head;
        self.selection = Some(sweep);
    }

    /// The pointer let go. A sweep that covered nothing is a click, and a
    /// click puts the selection down rather than leaving a caret nobody can
    /// see blinking in a document nobody can type into.
    pub fn end_sweep(&mut self) {
        self.sweep_from = None;
        if self.selection.is_some_and(|sweep| sweep.is_empty()) {
            self.selection = None;
        }
        // **A click on a mark is a question about that mark**, asked here
        // rather than on the press because a press is also where a sweep
        // begins — and an already-marked passage is exactly the one a reader
        // wants to copy. So the mark answers only when nothing was swept.
        if self.selection.is_none() {
            if let Some((page, x, y)) = self.pressed_on {
                self.mark_open = self.mark_under(page, x, y);
            }
        }
        self.pressed_on = None;
    }

    /// The mark under a point on a page, if there is one, ready to be shown.
    ///
    /// The rectangle handed back is the one that was hit, so the popover opens
    /// under the line that was clicked rather than under the first line of a
    /// mark that runs over three.
    fn mark_under(&self, page: usize, x: f64, y: f64) -> Option<(usize, Rect, MarkKey, String)> {
        let index = page.checked_sub(1)?;
        self.layout.box_of(index)?;
        self.markup
            .iter()
            .filter(|mark| mark.page == page)
            .find_map(|mark| {
                mark.quads
                    .iter()
                    .map(|quad| self.layout.place_on(index, *quad))
                    .find(|area| {
                        x >= area.left
                            && x <= area.left + area.width
                            && y >= area.top
                            && y <= area.top + area.height
                    })
                    .map(|area| {
                        (
                            page,
                            area,
                            MarkKey::InFile(mark.page, mark.index),
                            mark.color.clone(),
                        )
                    })
            })
    }

    /// Put the mark's own popover away. `false` when it was not up, which is
    /// what lets Escape go on to the next thing it means — the same shape
    /// [`Viewer::close_markup`] has.
    pub fn close_mark(&mut self) -> bool {
        self.mark_open.take().is_some()
    }

    /// The button came up: whatever was being dragged is put down.
    pub fn let_go(&mut self) {
        if self.dragging_bar() {
            self.drop_bar();
        }
        self.draw_done();
        if self.resize_from.is_some() {
            self.finish_resize_sidebar();
        }
        if self.sweeping() {
            self.end_sweep();
            // **A sweep that covered something offers to mark it.**
            // Reachable only by ⌘⇧H, nothing on screen ever pointed at
            // highlighting and nobody found the feature after it was
            // built. Letting go of a selection is the moment the
            // reader is looking at the passage. A setting, because a
            // reader who selects to copy has not asked to mark — and only
            // after a drag: a word or a line taken by a second or third
            // click is most often a word to copy or look up, and a popover
            // over it is in the way.
            let dragged = self.pressed.is_some_and(|(_, _, _, count)| count == 1);
            if dragged && self.store.flag("offer_highlight_on_select") {
                self.open_markup();
            }
        }
    }

    /// True while the pointer is down on a page.
    pub fn sweeping(&self) -> bool {
        self.sweep_from.is_some()
    }

    /// The unit under a point — the word a second click means, the line a
    /// third does, or the caret itself for a first.
    ///
    /// The anchor is left at the *start* of the unit and the head at its end,
    /// so a reader who goes on dragging extends from the word rather than from
    /// wherever inside it they happened to press.
    fn sweep_unit(&mut self, page: usize, on: (f64, f64), unit: Unit) {
        let Some(index) = page.checked_sub(1) else {
            return;
        };
        let (x, y) = self.layout.unplace_on(index, on.0, on.1);
        let text = self.text_on(index + 1);
        let (from, to) =
            crate::select::unit_around(&text, crate::select::caret_at(&text, x, y), unit);
        let (anchor, head) = (Spot { page, index: from }, Spot { page, index: to });
        self.sweep_seed = (unit, anchor, head);
        self.selection = Some(Selection { anchor, head });
    }

    /// Where a caret goes for a point in a page's box.
    fn spot_on(&self, index: usize, on_x: f64, on_y: f64) -> Spot {
        let (x, y) = self.layout.unplace_on(index, on_x, on_y);
        let text = self.text_on(index + 1);
        Spot {
            page: index + 1,
            index: crate::select::caret_at(&text, x, y),
        }
    }

    /// Everything on the page the reader is on, which is ⌘A.
    ///
    /// The *page* rather than the document, which is the app's label for this
    /// key: a reader who means the whole document means a file.
    pub fn select_page(&mut self) -> bool {
        let page = self.page();
        let text = self.text_on(page);
        if text.is_empty() {
            self.notice = "There is no text on this page to select.".into();
            return false;
        }
        self.selection = Some(Selection {
            anchor: Spot { page, index: 0 },
            head: Spot {
                page,
                index: text.chars.len(),
            },
        });
        true
    }

    /// Put the selection down. `false` when there was none, so that whatever
    /// asked can go on to the next thing Escape means.
    pub fn clear_selection(&mut self) -> bool {
        self.sweep_from = None;
        self.markup_at = None;
        self.selection.take().is_some()
    }

    pub fn has_selection(&self) -> bool {
        self.selection.is_some_and(|sweep| !sweep.is_empty())
    }

    /// What is selected on one mounted page, as rectangles in CSS pixels from
    /// the top left of its box — the space [`Viewer::highlights`] and
    /// [`Viewer::link_areas`] answer in.
    pub fn selected_areas(&self, page: usize) -> Vec<Rect> {
        let Some(sweep) = self.selection else {
            return Vec::new();
        };
        let Some(index) = page.checked_sub(1) else {
            return Vec::new();
        };
        if self.layout.box_of(index).is_none() {
            return Vec::new();
        }
        if !sweep.pages().contains(&page) {
            return Vec::new();
        }
        let text = self.text_on(page);
        let Some((from, to)) = sweep.range_on(page, text.chars.len()) else {
            return Vec::new();
        };
        text.quads(from, to)
            .into_iter()
            .map(|quad| self.layout.place_on(index, quad))
            .collect()
    }

    /// The selected words, in reading order, over as many pages as the sweep
    /// covers.
    pub fn selected_text(&self) -> String {
        let Some(sweep) = self.selection else {
            return String::new();
        };
        let mut out = String::new();
        for page in sweep.pages() {
            let text = self.text_on(page);
            let Some((from, to)) = sweep.range_on(page, text.chars.len()) else {
                continue;
            };
            let part = crate::select::quote(&text, from, to);
            if part.is_empty() {
                continue;
            }
            if !out.is_empty() {
                // A page break is a paragraph break, which is what a reader
                // pasting two pages of a paper into their notes means by it.
                out.push('\n');
            }
            out.push_str(&part);
        }
        out
    }

    /// The selected words with where they came from, which is ⌘⇧C.
    ///
    /// The app's format and its reason: going back to find the page a quote
    /// was on is the constant tax of reading for work. The page is the one the
    /// selection *began* on, not the one in the toolbar.
    ///
    /// Returns the text to copy and the words for the notice line.
    pub fn quoted(&self) -> Option<(String, String)> {
        let sweep = self.selection?;
        let quoted = self.selected_text();
        if quoted.is_empty() {
            return None;
        }
        let name = self.store.title().to_string();
        let where_from = format!(
            "{}p. {}",
            if name.is_empty() {
                String::new()
            } else {
                format!("{name}, ")
            },
            self.label(sweep.span().0.page)
        );
        Some((format!("“{quoted}” — {where_from}"), where_from))
    }

    /* --------------------------------------------------------------- ink */

    /// Open the Sign window, or say why it cannot be.
    ///
    /// The refusals are asked *here* rather than when the signature is placed,
    /// which is the opposite of markup and right for the opposite reason: a
    /// mark is one gesture and may as well try and report, where signing is a
    /// window, a drawing and a click.
    pub fn open_signing(&mut self) -> bool {
        // The menu comes down whichever way this goes: a refusal is a notice,
        // and a notice under an open menu is one nobody reads.
        self.menu = None;
        if self.empty() {
            return false;
        }
        let standing = crate::sign::standing(
            self.document.path(),
            self.document.encrypted(),
            self.document.sealed(),
        );
        if !standing.into_file {
            self.notice = format!("{} — so it cannot be signed.", standing.refused);
            return false;
        }
        self.signing = Some(Signing {
            kept: self.signatures(),
            signed_here: self.signed_here(),
            seals: self.seals(),
            // The pad opens empty and the name opens empty with it. A default
            // of "Signature" is what `sign::save` falls back to, and putting
            // it in the field would mean a reader who types their own name has
            // to clear somebody else's word out of the way first.
            ..Default::default()
        });
        true
    }

    /// Take it down. `false` when it was not up, which is what lets Escape go
    /// on to the next thing it means.
    pub fn close_signing(&mut self) -> bool {
        self.signing.take().is_some()
    }

    /// A press beside the window: it closes one with nothing in it, and
    /// leaves one holding a drawing or words — a hand signing a name runs off
    /// the pad more often than not, and one stray press lost the lot. Escape
    /// and the window's own × still close it.
    pub fn close_signing_unless_drawn(&mut self) {
        let started = self.signing.as_ref().is_some_and(|pad| {
            !pad.strokes.is_empty() || !pad.name.trim().is_empty() || !pad.line.trim().is_empty()
        });
        if !started {
            self.close_signing();
        }
    }

    /// Where the signatures are kept, which is the config directory this
    /// reader was given rather than the ambient one. See [`crate::sign::dir`].
    pub fn signatures(&self) -> Vec<crate::sign::Signature> {
        crate::sign::load_all(self.store.dir())
    }

    /// A press on the pad: begin a stroke, and note where the pad is.
    ///
    /// The difference between `on` (inside the pad) and `client` (in the
    /// window) is the pad's top left corner, which a later move needs and
    /// cannot ask for. `begin_sweep` records the same number for the same
    /// reason.
    pub fn draw_from(&mut self, on: (f64, f64), client: (f64, f64), pad: (f64, f64)) {
        let Some(signing) = self.signing.as_mut() else {
            return;
        };
        signing.pad = pad;
        signing.origin = (client.0 - on.0, client.1 - on.1);
        signing.drawing = true;
        signing.strokes.push(vec![[on.0, on.1]]);
    }

    /// The pointer moved in the window with the button down, while a stroke is
    /// running. Taken into the pad's own space and clamped to it: a hand that
    /// runs off the edge should stop at the edge rather than write a signature
    /// whose box is the whole window.
    pub fn draw_on_pad(&mut self, client: (f64, f64)) {
        let Some((origin, pad)) = self
            .signing
            .as_ref()
            .filter(|signing| signing.drawing)
            .map(|signing| (signing.origin, signing.pad))
        else {
            return;
        };
        self.draw_to((
            (client.0 - origin.0).clamp(0.0, pad.0),
            (client.1 - origin.1).clamp(0.0, pad.1),
        ));
    }

    /// One point onto the stroke that is running, in the pad's own space.
    pub fn draw_to(&mut self, at: (f64, f64)) {
        let Some(signing) = self.signing.as_mut() else {
            return;
        };
        if !signing.drawing {
            return;
        }
        let Some(stroke) = signing.strokes.last_mut() else {
            return;
        };
        // A point that has not moved is a point that says nothing, and a
        // signature drawn slowly would otherwise be a thousand of them. Half a
        // pixel is below what anybody can see and above what a jittering
        // trackpad reports while a finger rests.
        if stroke
            .last()
            .is_some_and(|last| (last[0] - at.0).abs() < 0.5 && (last[1] - at.1).abs() < 0.5)
        {
            return;
        }
        stroke.push([at.0, at.1]);
    }

    /// And the button let go. A stroke of one point is kept: a dot over an i
    /// is a stroke of one point, and [`crate::sign::place`] knows what to do
    /// with one.
    pub fn draw_done(&mut self) {
        if let Some(signing) = self.signing.as_mut() {
            signing.drawing = false;
        }
    }

    /// Throw away what is on the pad, leaving the window up. The thing anybody
    /// wants after the first attempt at signing with a trackpad.
    pub fn clear_pad(&mut self) {
        if let Some(signing) = self.signing.as_mut() {
            signing.strokes.clear();
            signing.drawing = false;
        }
    }

    /// Keep what is on the pad, and answer whether it was kept.
    ///
    /// **The strokes go across as drawn**, in the pad's own pixels, and
    /// `sign::save` normalises them — one division rather than two. Two was
    /// the bug: dividing x by the pad's width and y by its height scales the
    /// axes differently, so a wide name arrived narrow. A pad pixel is square,
    /// so handing over pixels loses nothing.
    pub fn keep_signature(&mut self) -> bool {
        let Some(signing) = self.signing.clone() else {
            return false;
        };
        if signing.strokes.iter().all(|stroke| stroke.is_empty()) {
            self.notice = "Draw a signature first, and this keeps it.".into();
            return false;
        }
        let drawn = crate::sign::Signature {
            name: signing.name.trim().to_string(),
            id: String::new(),
            strokes: signing.strokes.clone(),
        };
        // Named here and written by the scribe: the answer — what it was
        // stored as — is wanted now, and the file is not on this thread's
        // account. See [`crate::store::later`].
        match crate::sign::named(self.store.dir(), &drawn) {
            Ok(stored) => {
                let (dir, writing) = (self.store.dir().to_path_buf(), stored.clone());
                crate::store::later(move || {
                    let _ = crate::sign::write(&dir, &writing);
                });
                self.notice = format!("Kept {}.", stored.name);
                // The pad is cleared rather than the window closed: keeping a
                // signature and using one are two things, and a reader who has
                // just drawn one very often wants to draw the initials too.
                self.clear_pad();
                // The list with it in, rather than the directory read again:
                // the file is the scribe's to write and is not there yet.
                let mut kept = self.signatures();
                if !kept.iter().any(|kept| kept.id == stored.id) {
                    kept.push(stored.clone());
                    kept.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
                }
                if let Some(signing) = self.signing.as_mut() {
                    signing.name.clear();
                    signing.kept = kept;
                }
                true
            }
            Err(why) => {
                self.notice = why;
                false
            }
        }
    }

    /// Take one off the list, and off the disk.
    pub fn forget_signature(&mut self, id: &str) {
        match crate::sign::named_file(self.store.dir(), id) {
            Ok(file) => crate::store::later(move || {
                let _ = std::fs::remove_file(file);
            }),
            Err(why) => {
                self.notice = why;
                return;
            }
        }
        // The list without it, rather than the directory read again: the file
        // is the scribe's to remove and has not gone yet.
        let kept: Vec<crate::sign::Signature> = self
            .signatures()
            .into_iter()
            .filter(|kept| kept.id != id)
            .collect();
        if let Some(signing) = self.signing.as_mut() {
            signing.kept = kept;
        }
    }

    /// Every signature already in the document this window is showing.
    pub fn signed_here(&self) -> Vec<crate::sign::Placed> {
        self.document.signatures()
    }

    /// **What the document is already *digitally* signed with**, which is a
    /// different question from the one above and is answered by reading the
    /// file rather than the page. See [`crate::sign::Seal`], which is also
    /// where the four things that can honestly be said about one are.
    pub fn seals(&self) -> Vec<crate::sign::Seal> {
        self.document.seals()
    }

    /// **Take a signature back out of the document.**
    ///
    /// The assessment's one caveat was *it cannot be removed afterwards*, and
    /// that is true of the app alone: here it is `FPDFPage_RemoveAnnot`, the
    /// same call a highlight comes out through. A signature on the wrong page
    /// is the ordinary case, not the corner.
    ///
    /// Written off the main thread, as everything that writes the document
    /// is. See [`Viewer::write`].
    pub fn unsign(&mut self, page: usize, index: usize, kind: crate::sign::Written) {
        if self.busy() {
            return;
        }
        if !self.standing.into_file {
            self.notice = format!(
                "{} — so nothing can be taken out of it.",
                self.standing.refused
            );
            return;
        }
        // Asked once, as signing asks: see [`Viewer::sign_at`].
        if self.standing.signed && !self.said_rewrites {
            self.said_rewrites = true;
            self.notice = REMOVING_BREAKS_A_SIGNATURE.into();
            return;
        }
        self.write(
            move |path| crate::markup::remove(path, page, index),
            move |viewer, taken| match taken {
                Ok(()) => {
                    viewer.notice = match kind {
                        crate::sign::Written::Hand => format!("Signature taken off page {page}."),
                        crate::sign::Written::Line => format!("Text taken off page {page}."),
                    }
                }
                Err(refused) => viewer.notice = refused,
            },
        );
    }

    /// Choose a signature and go looking for somewhere to put it.
    ///
    /// The window closes, because the next question is *where* and the answer
    /// to it is on the page the window is covering.
    pub fn sign_with(&mut self, signature: crate::sign::Signature) {
        self.signing = None;
        self.placing = Some(Placing::Hand(signature));
        self.notice = "Click on the page where the signature should go.".into();
    }

    /// The same, for a date or a line of text. One gesture and one armed
    /// slot, because a reader placing something on a page is doing one thing
    /// whichever of the two it is.
    pub fn type_with(&mut self, line: String) {
        if line.trim().is_empty() {
            self.notice = "There is nothing typed to put on the page.".into();
            return;
        }
        self.signing = None;
        self.placing = Some(Placing::Line(line));
        self.notice = "Click on the page where the text should go.".into();
    }

    /// Put it down again unsigned. `false` when nothing was armed, which is
    /// what lets Escape go on to the next thing it means.
    pub fn put_down(&mut self) -> bool {
        if self.placing.take().is_some() {
            self.notice = "Signing cancelled.".into();
            true
        } else {
            false
        }
    }

    /// **The click that signs.** `on` is where the pointer landed on the page,
    /// in the screen pixels a page is laid out in; `unplace_on` takes it the
    /// rest of the way, through the crop and the rotation, into the page's own
    /// points.
    ///
    /// The point is the *middle of the left edge* of the signature, not its
    /// top left: what a reader clicking on a line is aiming at is the line, so
    /// the signature sits on it rather than hanging below it.
    ///
    /// Written off the main thread. See [`Viewer::write`].
    pub fn sign_at(&mut self, page: usize, on: (f64, f64)) {
        if self.placing.is_none() || self.busy() {
            return;
        }
        let (Some(placing), Some(index)) = (self.placing.take(), page.checked_sub(1)) else {
            return;
        };
        let (x, y) = self.layout.unplace_on(index, on.0, on.1);
        // A hand is drawn to a height it chose and a line of type to a
        // smaller one: the name is the thing being said, and a date under it
        // is a note about the name.
        let height = match &placing {
            Placing::Hand(_) => HAND_HEIGHT,
            Placing::Line(_) => crate::sign::LINE_HEIGHT,
        };
        let at = Rect {
            left: x,
            top: y - height / 2.0,
            width: 0.0,
            height,
        };
        // Said once, *before* it happens, and only for the document that has
        // something to lose: the signature stays armed, and the next click
        // is the reader's answer. See `sign::BREAKS_A_SIGNATURE`.
        if self.standing.signed && !self.said_rewrites {
            self.said_rewrites = true;
            self.placing = Some(placing);
            self.notice = format!(
                "{} Click again to sign anyway.",
                crate::sign::BREAKS_A_SIGNATURE
            );
            return;
        }

        let done = match &placing {
            Placing::Hand(_) => format!("Signed on page {page}."),
            Placing::Line(_) => format!("Written on page {page}."),
        };
        self.write(
            move |path| match &placing {
                Placing::Hand(signature) => {
                    crate::sign::place(path, page, at, signature, crate::sign::INK)
                }
                Placing::Line(line) => {
                    crate::sign::place_text(path, page, at, line, crate::sign::INK)
                }
            },
            move |viewer, written| match written {
                Ok(()) => viewer.notice = done,
                // Nothing is kept beside the document, where a mark would be.
                // A highlight in the journal is still a passage the reader
                // marked; a signature that did not reach the file is not a
                // signature, and a list of them would be a promise this
                // reader cannot keep.
                Err(refused) => viewer.notice = refused,
            },
        );
    }

    /* ------------------------------------------------------------- markup */

    /// Read the document's own markup, ask the disk where a new mark could
    /// go, and bring the journal into line with both. Called at open and
    /// after every reload.
    fn read_markup(&mut self) {
        self.markup = self.document.markup();
        let path = self.document.path().to_string();
        self.standing = if path.is_empty() {
            crate::markup::Standing::default()
        } else {
            crate::markup::standing(&path, self.document.encrypted(), self.document.sealed())
        };
        self.sync_journal();
    }

    /// The journal, rebuilt from what the file says.
    ///
    /// **This is where a recompile is survived**, and the one job the journal
    /// has that a document cannot do for itself: a paper rebuilt by LaTeX is a
    /// new file and every annotation went with it, but the words are usually
    /// still there. So each mark is written down with the passage it covers,
    /// and [`Viewer::restore_markup`] looks the quotes up again.
    ///
    /// The rule is **everything is thrown away and rebuilt from the file**,
    /// and what survives is only what the file cannot carry: a mark beside a
    /// document that could not be written, and a mark a rebuild lost.
    ///
    /// A mark is the same mark when its colour and its words are the same —
    /// not its page and not its index, because a rebuild moves a passage
    /// (which is the case this exists for) and an index shifts whenever an
    /// earlier annotation is added or taken away.
    fn sync_journal(&mut self) {
        let inside: Vec<(String, String, crate::markup::Mark)> = self
            .markup
            .iter()
            .map(|mark| {
                // Read once: the folded copy compares, the plain one is
                // written. With marks on more pages than the text cache
                // holds, a second read here was a second pdfium extraction
                // per mark at every open.
                let quote = crate::markup::quote_under(&self.text_on(mark.page), &mark.quads);
                (mark.color.to_lowercase(), quote, mark.clone())
            })
            .collect();
        let mut next = Vec::new();
        for held in self.store.journal() {
            let known = inside.iter().any(|(colour, quote, _)| {
                *colour == held.color.to_lowercase() && folded(quote) == folded(&held.quote)
            });
            if known {
                // In the file, so the file's own entry below is the one to
                // keep — this copy is last time's reading of it.
                continue;
            }
            // Not in the file. Either it never was, or it went with a
            // rebuild, and **`annotation_id` is what says so**: `None` is the
            // app's own mark for a highlight the document is not carrying,
            // and it is what the panel reads to know which rows to list and
            // which passages it can offer to put back.
            let mut lost = held.clone();
            lost.annotation_id = None;
            next.push(lost);
        }
        for (_, quote, mark) in &inside {
            let height = self.document.size_of(mark.page.saturating_sub(1)).height;
            next.push(crate::store::Store::markup_entry(
                mark.page,
                crate::markup::flat(&mark.quads, height),
                &mark.color,
                quote,
                Some(format!("{}:{}", mark.page, mark.index)),
            ));
        }
        self.store.set_journal(next);
    }

    /// The marks the journal is holding that are not in the document: the
    /// ones a rebuild lost, and the ones a document that cannot be written
    /// never took.
    pub fn markup_adrift(&self) -> Vec<&crate::library::Highlight> {
        self.store
            .journal()
            .iter()
            .filter(|held| held.annotation_id.is_none())
            .collect()
    }

    /// How many of those could be put back, which is what the offer says.
    pub fn restorable(&self) -> usize {
        if !self.standing.into_file {
            return 0;
        }
        self.markup_adrift()
            .iter()
            .filter(|held| !held.quote.trim().is_empty())
            .count()
    }

    /// Look the lost passages up again and write back the ones still there.
    ///
    /// **A guess, and never a thing that happens on its own** — a button,
    /// because this reader does not write to somebody's file without being
    /// asked.
    ///
    /// The lookup starts on the page the passage used to be on and works
    /// outwards. What it does not find is left in the journal and counted out
    /// loud: a passage that was rewritten is not a passage that moved.
    pub fn restore_markup(&mut self) {
        let wanted: Vec<crate::library::Highlight> = self
            .markup_adrift()
            .into_iter()
            .filter(|held| !held.quote.trim().is_empty())
            .cloned()
            .collect();
        if wanted.is_empty() || !self.standing.into_file || self.busy() {
            return;
        }
        // Asked first, as marking asks: see [`Viewer::mark_selection`].
        if self.standing.signed && !self.said_standing {
            self.said_standing = true;
            self.notice = MARKING_BREAKS_A_SIGNATURE.into();
            return;
        }
        // Looked up on the thread, off the document as it is on disk: a
        // passage that was rewritten is a read of every page's text, which
        // is the stall `Viewer::write` exists to keep out of the window. The
        // journal is left as it is — the reload the write causes reads the
        // file, and a passage back in it is a row `sync_journal` replaces
        // with the file's own; one that was not found stays adrift.
        let password = self.document.password().map(str::to_string);
        let counted = Arc::new(Mutex::new((0usize, 0usize)));
        let counting = Arc::clone(&counted);
        self.write(
            move |path| {
                let document = crate::render::open_with(path, password.as_deref())
                    .map_err(|e| e.to_string())?;
                let mut found = Vec::new();
                let mut lost = 0;
                for held in &wanted {
                    match find_quote(&*document, held.page as usize, &held.quote) {
                        Some((page, quads)) => found.push((page, quads, held.color.clone())),
                        None => lost += 1,
                    }
                }
                drop(document);
                // Written, not found: counted as each colour lands, so a
                // failure part of the way says what did make it in.
                *counting.lock().unwrap_or_else(|e| e.into_inner()) = (0, lost);
                if found.is_empty() {
                    return Err(format!(
                        "{} could not be found in this document.",
                        said_of(lost, "passage", "passages"),
                    ));
                }
                // One rewrite per colour, not per passage: `add` takes a
                // run of pages, and each call is the whole file saved again.
                let mut by_colour: std::collections::BTreeMap<String, Vec<(usize, Vec<Rect>)>> =
                    Default::default();
                for (page, quads, color) in found {
                    by_colour.entry(color).or_default().push((page, quads));
                }
                for (color, runs) in &by_colour {
                    crate::markup::add(path, runs, color, AUTHOR)?;
                    counting.lock().unwrap_or_else(|e| e.into_inner()).0 += runs.len();
                }
                Ok(())
            },
            move |viewer, written| {
                let (wrote, lost) = *counted.lock().unwrap_or_else(|e| e.into_inner());
                viewer.notice = match written {
                    Err(refused) if wrote > 0 => format!(
                        "{} put back, and then: {refused}",
                        said_of(wrote, "passage", "passages"),
                    ),
                    Err(refused) => refused,
                    Ok(()) if lost == 0 => {
                        format!("{} put back.", said_of(wrote, "passage", "passages"))
                    }
                    Ok(()) => format!(
                        "{} put back. {} could not be found in this document.",
                        said_of(wrote, "passage", "passages"),
                        said_of(lost, "passage", "passages"),
                    ),
                };
            },
        );
    }

    /// Every mark the panel lists: the document's own first, in reading
    /// order, then whatever the journal is holding.
    ///
    /// One list, because to a reader they are one thing. They are told apart
    /// by a word on the row rather than by a section of their own.
    pub fn markup_rows(&self) -> Vec<MarkRow> {
        // Built once per reading of the document and of the journal, because
        // the panel asks on every scroll frame and a row costs a page of text.
        let (edition, journal) = (self.edition, self.store.journal_rev());
        if let Some((for_edition, for_journal, rows)) = self.mark_rows.borrow().as_ref() {
            if *for_edition == edition && *for_journal == journal {
                return rows.clone();
            }
        }
        let rows = self.build_markup_rows();
        *self.mark_rows.borrow_mut() = Some((edition, journal, rows.clone()));
        rows
    }

    fn build_markup_rows(&self) -> Vec<MarkRow> {
        let mut rows: Vec<MarkRow> = self
            .markup
            .iter()
            .map(|mark| MarkRow {
                page: mark.page,
                color: mark.color.clone(),
                quote: crate::markup::quote_under(&self.text_on(mark.page), &mark.quads),
                key: MarkKey::InFile(mark.page, mark.index),
            })
            .collect();
        rows.sort_by(|a, b| {
            let (first, second) = (
                self.markup
                    .iter()
                    .find(|mark| MarkKey::InFile(mark.page, mark.index) == a.key)
                    .map(|mark| (mark.page, mark.begins())),
                self.markup
                    .iter()
                    .find(|mark| MarkKey::InFile(mark.page, mark.index) == b.key)
                    .map(|mark| (mark.page, mark.begins())),
            );
            first
                .partial_cmp(&second)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        // Only the ones the file is not already showing: the journal mirrors
        // every mark in the document as well, so that a rebuild has the
        // quotes to look up — see [`Viewer::sync_journal`] — and listing both
        // copies would list every mark twice.
        rows.extend(self.markup_adrift().into_iter().map(|held| MarkRow {
            page: held.page as usize,
            color: held.color.clone(),
            quote: held.quote.clone(),
            key: MarkKey::Beside(held.id.clone()),
        }));
        rows
    }

    /// Put the colour popover up over the selection, or say why not.
    ///
    /// Answers whether it opened, so that the gesture that opens it by
    /// itself — letting go of a sweep — can stay silent while the key says
    /// something.
    pub fn open_markup(&mut self) -> bool {
        let Some(sweep) = self.selection.filter(|sweep| !sweep.is_empty()) else {
            return false;
        };
        // Where the sweep *ends*, which is where the reader's pointer is and
        // is what the app anchors to as well — the last rectangle of the
        // range, not the first.
        let (_, last) = sweep.span();
        let areas = self.selected_areas(last.page);
        let Some(area) = areas.last().copied() else {
            return false;
        };
        self.menu = None;
        self.markup_at = Some((last.page, area));
        true
    }

    /// Take it down again. `false` when it was not up, which is what lets
    /// Escape go on to the next thing it means.
    pub fn close_markup(&mut self) -> bool {
        self.markup_at.take().is_some()
    }

    /// The window that edits the six colours, over whatever is open. The
    /// swatches stay up underneath so that a colour chosen in the window can
    /// be used on the passage the reader has just swept.
    pub fn open_markup_colours(&mut self) {
        self.colours_open = true;
    }

    /// Take it down. `false` when it was not up, which is what lets Escape
    /// go on to the next thing it means.
    pub fn close_markup_colours(&mut self) -> bool {
        if !self.colours_open {
            return false;
        }
        // A picker open inside it goes first: Escape means the thing on top.
        if self.picking.take().is_some() {
            return true;
        }
        self.colours_open = false;
        true
    }

    /// One of the six, changed. `at` is one-based, as the keys are.
    pub fn set_markup_color(&mut self, at: usize, hex: String) {
        if crate::palette::read_colour(&hex).is_none() {
            return;
        }
        self.store
            .set(vec![(format!("markup_color_{at}"), json!(hex))]);
    }

    /// All six back to what a fresh install has. This throws a reader's own
    /// colours away, which is why the window asks first.
    pub fn reset_markup_colors(&mut self) {
        let defaults = crate::settings::defaults();
        let entries = MARKUP_COLOR_KEYS
            .iter()
            .filter_map(|key| Some((key.to_string(), defaults.get(*key)?.clone())))
            .collect();
        self.store.set(entries);
        self.notice = "Highlight colours reset.".into();
    }

    /// The six colours the popover offers, in the order the swatches show
    /// them.
    ///
    /// Six independent settings rather than a list, because `settings.rs` has
    /// no list type and a palette is not worth adding one for. They are the
    /// app's own keys, so `markup_color_3` changes the third swatch in both.
    pub fn markup_colors(&self) -> Vec<String> {
        (1..=6)
            .map(|at| self.store.text(&format!("markup_color_{at}")))
            .filter(|colour| crate::palette::read_colour(colour).is_some())
            .collect()
    }

    /// Mark what is selected, in this colour.
    ///
    /// The whole gesture in order: the quads come off the selection, the
    /// document is let go of, the write goes in, and the document is reopened
    /// through the path a recompile uses. There is no pending layer and no
    /// markup revision in any cache key — saving immediately makes the
    /// deferred machinery unnecessary rather than merely delayed, and a reload
    /// rebuilds every cache there is.
    ///
    /// The write and the reopen are on a thread of their own, and the second
    /// half of this runs when they land. See [`Viewer::write`].
    pub fn mark_selection(&mut self, color: &str) {
        if self.busy() {
            return;
        }
        let offered = self.markup_at.take();
        let Some(sweep) = self.selection.filter(|sweep| !sweep.is_empty()) else {
            // Two different sentences, and the difference is the point. A
            // scan has no text in it at all, so there is nothing this gesture
            // could ever mark and no amount of selecting will help.
            self.notice = if self.text_on(self.page()).is_empty() {
                "There is no text on this page to highlight.".into()
            } else {
                "Select something first, and this highlights it.".into()
            };
            return;
        };
        let runs: Vec<(usize, Vec<Rect>)> = sweep
            .pages()
            .filter_map(|page| {
                let text = self.text_on(page);
                let (from, to) = sweep.range_on(page, text.chars.len())?;
                let quads = text.quads(from, to);
                (!quads.is_empty()).then_some((page, quads))
            })
            .collect();
        if runs.is_empty() {
            self.notice = "There is nothing there to highlight.".into();
            return;
        }
        let quote = self.selected_text();
        if !self.standing.into_file {
            let kept = self.beside(&runs, &quote);
            return self.keep_beside(kept, color);
        }
        // Signed, and asked once, *before* the write: it is their document,
        // and a rewrite is exactly the thing a signature is there to detect.
        // The selection and the swatches stay, so the answer is the same
        // click again. This said so after the file was already rewritten.
        if self.standing.signed && !self.said_standing {
            self.said_standing = true;
            self.markup_at = offered;
            self.notice = MARKING_BREAKS_A_SIGNATURE.into();
            return;
        }
        // **Let go of the file before writing it, and reopen whatever
        // happens.** pdfium keeps the file open for the life of the document,
        // and on Windows nothing can rename over it or truncate it while it
        // does. The reopen is unconditional because a released document draws
        // nothing, so a failed write must still leave the reader looking at
        // their document. See [`crate::render::PageSource::release`], which
        // [`Viewer::write`] calls.
        self.selection = None;
        // Worked out now, off the document the passage was chosen in: a
        // refused write lands after the reopen, and a draft that has since
        // lost pages made this an index past the end of it.
        let kept = self.beside(&runs, &quote);
        let (writing, color) = (runs, color.to_string());
        let colour = color.clone();
        self.write(
            move |path| crate::markup::add(path, &writing, &colour, AUTHOR),
            move |viewer, written| {
                viewer.show_markup_panel();
                match written {
                    // Nothing said: the mark on the page is the answer.
                    Ok(()) => {}
                    Err(refused) => {
                        // The file is as it was, so there is nothing to put
                        // back. The mark is kept beside the document instead,
                        // which is the answer a read-only file gets and for
                        // the same reason: a passage the reader marked is not
                        // lost because the disk said no.
                        viewer.keep_beside(kept, &color);
                        viewer.notice =
                            format!("{refused} The highlight is kept beside the document.");
                    }
                }
            },
        );
    }

    /// Keep a mark beside the document rather than in it, and say so once.
    /// A passage as the journal holds it: per page, its quads flattened
    /// against that page's height, and the words on it.
    fn beside(&self, runs: &[(usize, Vec<Rect>)], quote: &str) -> Vec<(usize, Vec<f64>, String)> {
        runs.iter()
            .map(|(page, quads)| {
                let height = self.document.size_of(page.saturating_sub(1)).height;
                // Each page keeps the words on it: a passage is looked for a
                // page at a time, and the whole of one that runs over two is
                // on neither.
                let own = if runs.len() > 1 {
                    crate::markup::quote_under(&self.text_on(*page), quads)
                } else {
                    quote.to_string()
                };
                (*page, crate::markup::flat(quads, height), own)
            })
            .collect()
    }

    fn keep_beside(&mut self, kept: Vec<(usize, Vec<f64>, String)>, color: &str) {
        for (page, flat, own) in kept {
            self.store.keep_markup(page, &flat, color, &own);
        }
        self.selection = None;
        self.show_markup_panel();
        let why = self.standing.refused.clone();
        self.notice = if self.said_standing || why.is_empty() {
            "Highlighted, beside the document.".into()
        } else {
            self.said_standing = true;
            format!("Highlighted — but {why}, so it is kept beside the document rather than in it.")
        };
    }

    /// A document with a passage marked has something to show in the Contents
    /// panel after all — [`Viewer::mark_page`]'s rule, and as narrow: only
    /// when the panel is already open and the tab it is on has nothing on it.
    /// Taking a reader off the thumbnails they were looking at is the panel
    /// arguing.
    fn show_markup_panel(&mut self) {
        if self.sidebar_open && self.tab == Tab::Pages && self.headings.is_empty() {
            self.tab = Tab::Contents;
        }
    }

    /// **A removal is two presses.** The first turns the small × or bin into
    /// the word for what it does, and the second does it: a highlight taken
    /// out of the file or a signature deleted is gone for good, and the
    /// buttons are small and sit right beside the row they belong to.
    /// Answers whether this press is the one that acts.
    pub fn arm(&mut self, what: Arming) -> bool {
        if self.arming.as_ref() == Some(&what) {
            self.arming = None;
            true
        } else {
            self.arming = Some(what);
            false
        }
    }

    /// Take one mark out, wherever it is being kept.
    ///
    /// **A mark in the document comes out of the document**:
    /// `FPDFPage_RemoveAnnot`, a reopen, and it is gone from the file for
    /// every reader of it. The app cannot say that — see [`crate::markup`].
    ///
    /// Answers whether it stopped to ask first — a signed document — so that
    /// the popover the question came from can stay for the answer.
    pub fn remove_markup(&mut self, key: &MarkKey) -> bool {
        match key {
            MarkKey::Beside(id) => {
                // Nothing said: the mark going from the page is the answer.
                self.store.drop_markup(id);
            }
            MarkKey::InFile(page, index) => {
                if self.busy() {
                    return false;
                }
                // Asked here as it is asked before a mark goes in: an
                // encrypted or read-only document cannot have one taken out
                // either, and finding that out from a failed write costs the
                // reader the document on screen.
                if !self.standing.into_file {
                    self.notice = format!(
                        "{} — so the highlight cannot be taken out of it.",
                        self.standing.refused
                    );
                    return false;
                }
                // Asked as marking asks, and before the journal is touched:
                // see [`Viewer::mark_selection`].
                if self.standing.signed && !self.said_standing {
                    self.said_standing = true;
                    self.notice = REMOVING_BREAKS_A_SIGNATURE.into();
                    return true;
                }
                // **The journal has to be told first**, or the reload cannot
                // tell "the reader took this off" from "a rebuild lost it" —
                // the mark is gone from the file either way, and the second
                // reading offers it straight back.
                // By the annotation it is the reading of, which `sync_journal`
                // wrote down: the same words in the same colour kept beside
                // the document is another mark, and stays.
                let taking = format!("{page}:{index}");
                let keeping: Vec<crate::library::Highlight> = self
                    .store
                    .journal()
                    .iter()
                    .filter(|held| held.annotation_id.as_deref() != Some(taking.as_str()))
                    .cloned()
                    .collect();
                self.store.set_journal(keeping);
                let (page, index) = (*page, *index);
                self.write(
                    move |path| crate::markup::remove(path, page, index),
                    |viewer, taken| {
                        if let Err(refused) = taken {
                            viewer.notice = refused;
                        }
                    },
                );
            }
        }
        false
    }

    /* ------------------------------------------------- what a page is called */

    /// Whether this document numbers its pages its own way.
    pub fn has_labels(&self) -> bool {
        !self.labels().is_empty()
    }

    /* ---------------------------------------------------- the toolbar peek */

    /// **With the toolbar away, the top edge of the window stands in for
    /// it.** `wireToolbarPeek` in `main.ts`: a handle drops in when the
    /// pointer arrives at the edge and puts the bar back, and nothing is on
    /// screen until somebody reaches for it. Without this the only way back
    /// was the key the notice names, which is a sentence that has to be read
    /// and remembered.
    ///
    /// The handle sits below the band the system reserves for itself rather
    /// than at the very top, in full screen and out of it alike — see
    /// [`PEEK_REACH`] — so there is one reach here and not two.
    pub fn reach_for_toolbar(&mut self, y: f64) {
        if self.toolbar_up() || self.presenting {
            self.peek = false;
            return;
        }
        let reach = PEEK_REACH;
        if y <= reach {
            self.peek = true;
        } else if y > PEEK_KEEP {
            // Going away while it is being reached for is the one thing it
            // must not do, so it stays for a good way below where it appears.
            self.peek = false;
        }
    }

    /// Whether a move at this height would change anything, asked before the
    /// signal is written to: `onmousemove` fires on every move in the window
    /// and a write is a render. The same guard `resize_from` gets.
    pub fn peek_changes(&self, y: f64) -> bool {
        if self.toolbar_up() || self.presenting {
            return self.peek;
        }
        let reach = PEEK_REACH;
        (y <= reach) != self.peek && (y <= reach || y > PEEK_KEEP)
    }

    pub fn peeking(&self) -> bool {
        self.peek
    }

    /* ------------------------------------------------------- the page pill */

    /// The reader scrolled: put the pill up, if there is any reason to.
    ///
    /// Two conditions: **only while the toolbar is away**, because with the
    /// bar up the number is already on screen, and only if the reader wants
    /// it. Returns the token of the flash, which the thread that takes it down
    /// carries.
    pub fn flash_pill(&mut self) -> Option<u64> {
        if self.toolbar_up() || self.presenting || self.empty() || !self.page_pill() {
            return None;
        }
        self.pill_token += 1;
        self.pill_up = true;
        Some(self.pill_token)
    }

    /// Whether the scroll is where a relayout put it, rather than where the
    /// reader scrolled to.
    pub fn relaid(&self) -> bool {
        self.scroll_top == self.relaid_at
    }

    /// …and the end of that second, if nothing has happened since.
    pub fn unflash_pill(&mut self, token: u64) {
        if self.pill_token == token {
            self.pill_up = false;
        }
    }

    /* -------------------------------------------------------- the scrollbar */

    /// The document moved: put the bar up. See [`Viewer::bar_shown`].
    ///
    /// Unconditional where [`Viewer::flash_pill`] has three conditions, and
    /// a token for the same reason: the thread that takes the bar down
    /// carries the number it was started for, so a scroll while it is up
    /// keeps it up rather than letting it go on the first one's clock.
    pub fn flash_bar(&mut self) -> u64 {
        self.bar_token += 1;
        self.bar_up = true;
        self.bar_token
    }

    /// …and [`BAR_LASTS`] later, if nothing has moved since.
    pub fn unflash_bar(&mut self, token: u64) {
        if self.bar_token == token {
            self.bar_up = false;
        }
    }

    /// Whether there is a bar on screen at all. A hand on it keeps it there
    /// however long the drag takes, which is the one gesture no timer should
    /// be able to interrupt.
    pub fn bar_shown(&self) -> bool {
        self.bar_up || self.dragging_bar()
    }

    /// **A hand on the scrollbar always gets the count, setting or no.** The
    /// setting is about scrolling — a message that appears of its own accord
    /// while somebody reads — and dragging a thumb through four hundred pages
    /// is the one gesture with nothing else on screen to say where it has
    /// arrived. That is why the setting is off by default now: what it was
    /// there for is answered by the bar.
    pub fn pill_shown(&self) -> bool {
        self.pill_up || self.dragging_bar()
    }

    /// What the pill says: the page, and how many there are. A document that
    /// numbers its own pages says both — "iii (3 of 400)" — because the label
    /// is what is printed on the page and the position is what says how far
    /// through it is.
    pub fn pill_text(&self) -> String {
        let (page, pages) = (self.page(), self.pages());
        if pages == 0 {
            return String::new();
        }
        if self.has_labels() {
            format!("{} ({page} of {pages})", self.label(page))
        } else {
            format!("{page} of {pages}")
        }
    }

    /// What to call a page, one-based, when showing it to a reader.
    pub fn label(&self, page: usize) -> String {
        match self.labels().get(page.wrapping_sub(1)) {
            Some(label) if !label.is_empty() => label.clone(),
            _ => page.to_string(),
        }
    }

    /// The page a reader means by what they typed.
    ///
    /// A label first, because that is what is printed on the page and what an
    /// index cites; the position second, so "page 7" still finds something in
    /// a document whose seventh page is called "vii", and so a page with a
    /// blank label is reachable at all.
    pub fn page_for_label(&self, text: &str) -> Option<usize> {
        let wanted = text.trim();
        if wanted.is_empty() {
            return None;
        }
        let folded = wanted.to_lowercase();
        if let Some(at) = self
            .labels()
            .iter()
            .position(|label| label.to_lowercase() == folded)
        {
            return Some(at + 1);
        }
        let number: usize = wanted.parse().ok()?;
        (number >= 1 && number <= self.pages()).then_some(number)
    }

    /// What stands after "of" in the toolbar, which is the number of pages
    /// unless the document numbers its own straight through.
    ///
    /// A journal offprint numbers its pages from where they fall in the issue,
    /// so the first of eighteen is printed 2669 — and the count beside it, read
    /// off the file rather than off the page, made that "2669 of 18". The two
    /// halves have to be speaking about the same thing, and what the reader can
    /// see is the number on the paper, so the count follows it: "2669 of 2686".
    ///
    /// **Only where the labels are one ascending run of numbers**, which is
    /// what "numbers its own straight through" means and is the whole of the
    /// case this is for. A book whose front matter is i, ii, iii before a body
    /// that starts again at 1 has no last number to count to — its sixth page
    /// is called 3 — so that one says how many pages there are, as before.
    pub fn pages_text(&self) -> String {
        let pages = self.pages();
        if pages == 0 {
            return "0".to_string();
        }
        straight_run(&self.label(1), &self.label(pages), pages).unwrap_or_else(|| pages.to_string())
    }

    /// What the readout would say under each numbering — "407 of 425" and
    /// "1 of 19" — for the menu that chooses between them.
    pub fn numbering_choices(&self) -> (String, String) {
        let (page, pages) = (self.page(), self.pages());
        let printed = match self.labels.get(page.wrapping_sub(1)) {
            Some(label) if !label.is_empty() => label.clone(),
            _ => page.to_string(),
        };
        let first = self.labels.first().cloned().unwrap_or_default();
        let last = self.labels.last().cloned().unwrap_or_default();
        let count = straight_run(&first, &last, pages).unwrap_or_else(|| pages.to_string());
        (
            format!("{printed} of {count}"),
            format!("{page} of {pages}"),
        )
    }

    /// What the field in the toolbar has in it.
    pub fn page_field(&self) -> String {
        if self.typing_page {
            self.page_typed.clone()
        } else {
            self.label(self.page())
        }
    }

    /// Put the reader in the page field, holding the page they are on with
    /// all of it selected. `focusPageNumber` in `main.ts`, which is
    /// `focus()` and then `select()`; see [`Viewer::typing_page`] for what
    /// stands in for the second half.
    pub fn open_page_field(&mut self) {
        // Nothing to go to, and nowhere to type it: the bar's document half is
        // not there on an empty window, and presenting is where the whole bar
        // goes. Without the second the field went into a state with no field in
        // it — `typing_page` true and nothing on screen holding the keyboard —
        // which took an Escape to leave. `focusPageNumber` returns on the first
        // of these too.
        if self.empty() || self.presenting {
            return;
        }
        // **There is nowhere to put the cursor when the toolbar is away**, so
        // the shortcut brings the bar in itself rather than making the reader
        // do that first — and only for as long as the field is in use. Unlike
        // the toolbar key this does not change the setting; committing or
        // abandoning the jump puts the bar back into hiding. `focusPageNumber`
        // in `main.ts`, and the same loan.
        if !self.toolbar {
            self.borrowed_toolbar = true;
            self.refit();
            self.generation += 1;
        }
        self.typing_page = true;
        self.page_typed = self.label(self.page());
        self.page_fresh = true;
    }

    /// Give the toolbar back, if the page field had borrowed it.
    ///
    /// Before the jump rather than after: `go_to_page` resolves a scroll offset
    /// against the window the document is about to have, and a bar that goes
    /// away afterwards leaves the reader a toolbar's height off.
    fn return_toolbar(&mut self) {
        if self.borrowed_toolbar {
            self.borrowed_toolbar = false;
            self.refit();
            self.generation += 1;
        }
    }

    /// The reader typed into the field: whatever is in it now.
    pub fn type_page(&mut self, text: &str) {
        self.typing_page = true;
        self.page_typed = text.to_string();
        self.page_fresh = false;
    }

    /// Go where the field says, or say that it says nowhere.
    ///
    /// Either way the field stops being typed in and goes back to naming the
    /// page the reader is on, which is what the app does by putting the
    /// current label back into it.
    pub fn commit_page(&mut self) {
        self.return_toolbar();
        let typed = std::mem::take(&mut self.page_typed);
        self.typing_page = false;
        // Nothing was typed: the field is holding the page it opened on, and
        // Enter on that means "never mind" rather than "go where I already
        // am" — which would otherwise put an entry in the history for a jump
        // that went nowhere.
        let untouched = std::mem::take(&mut self.page_fresh);
        if untouched || typed.trim().is_empty() {
            return;
        }
        if let Some(page) = self.page_for_label(&typed) {
            self.go_to_page(page);
            return;
        }
        // **A number past the end is the last page, not a complaint** — ⌘9 in
        // a browser with four tabs open. Only text that is neither a label nor
        // a number is worth a word: "xii" in a document numbered 1, 2, 3 is a
        // reader looking at the wrong book, and there is nowhere to clamp it
        // to.
        if let Ok(number) = typed.trim().parse::<usize>() {
            let pages = self.pages();
            if pages > 0 {
                self.go_to_page(number.clamp(1, pages));
            }
            return;
        }
        self.notice = format!("There is no page {} in this document", typed.trim());
    }

    pub fn cancel_page(&mut self) {
        self.return_toolbar();
        self.typing_page = false;
        self.page_typed.clear();
        self.page_fresh = false;
    }

    /// What a mark on this page would be called: the section it falls in.
    ///
    /// `sectionFor` in `sidebar.ts`, and for its reason — a mark named for
    /// the chapter it sits in is worth a great deal more than one named
    /// "Page 214", and the outline has already been walked.
    pub fn section_for(&self, page: usize) -> String {
        crate::sidebar::heading_for(&self.headings, page, 1.0, None)
            .map(|at| self.headings[at].title.clone())
            .unwrap_or_default()
    }

    /// Put a pin in a page or take it out — the same gesture doing the same
    /// thing, which is what `toggle_mark` is.
    pub fn mark_page(&mut self, page: usize) {
        // No page to put a pin in. `open_find`'s note again, and this is the
        // third of the four keys the app leaves unflagged that are plainly
        // about a document — `mark`, `find`, `find-next`, `find-previous`.
        // Without it ⌘⇧B on the start screen says "Marked page 0", and says it
        // about the document whose entry the store has just let go of.
        if self.empty() {
            return;
        }
        let title = self.section_for(page);
        let marked = self.store.toggle_mark(page, &title);
        let called = self.label(page);
        self.notice = if marked {
            format!("Bookmarked page {called}")
        } else {
            format!("Took the bookmark off page {called}")
        };
        // The panel opens on the pages when a document has no contents, and a
        // document with a mark in it has something to show there after all —
        // which is the rule `showMarks` follows, and the reason it is that
        // narrow: taking the reader off the thumbnails they were looking at,
        // for a list they can see is there, is the panel arguing.
        if marked && self.sidebar_open && self.tab == Tab::Pages && self.headings.is_empty() {
            self.tab = Tab::Contents;
        }
    }

    /* ------------------------------------------------------- the search */

    /// Put the find bar up. Nothing is searched for until something is typed —
    /// unless the bar went down with a query in it, which comes back and is
    /// looked for again; the token is the scan's, for [`rescan`].
    pub fn open_find(&mut self) -> Option<u64> {
        // Nothing to search. **One line more than the app has**, deliberately:
        // `find` is not `needsDocument` in `keys.ts`, so ⌘F on the app's start
        // screen puts up a bar that will never find anything. The flag is left
        // agreeing with the app, because that table is a port and this is a
        // judgement about one key.
        if self.empty() {
            return None;
        }
        // The colour popover is about a passage, and somebody opening the
        // find bar has moved on from it. Closing it here rather than leaving
        // Escape to do it keeps that key's order the one it claims to be:
        // outward, in the order the reader arrived — and the bar was arrived
        // at second.
        self.markup_at = None;
        self.find_open = true;
        self.find_asked += 1;
        if self.sidebar_open && !self.search.query().is_empty() {
            self.to_results();
        }
        if self.find_query.is_empty() {
            return None;
        }
        let query = self.find_query.clone();
        self.find(&query)
    }

    /// Show the list behind the count.
    ///
    /// **The count in the find bar is the way through to the results**: "3 of
    /// 128" answers *is it in here* and not *which one did I mean*, and the
    /// second is what somebody searching a long document is usually asking. An
    /// empty count leads nowhere and does not look pressable.
    pub fn show_results(&mut self) {
        if self.search.state().total == 0 {
            return;
        }
        if !self.sidebar_open {
            self.set_sidebar(true, false);
            self.results_borrowed = true;
        }
        self.to_results();
    }

    /// Take the find bar down, and the index with it.
    ///
    /// **The index goes when the bar does**: every page scanned is kept, so a
    /// long book costs tens of megabytes for as long as it is open — a fair
    /// trade while somebody is searching and none once they have stopped.
    /// Reopening rescans, in under half a second. See [`Search::forget`].
    /// The query itself stays, so that reopening looks for the same thing.
    pub fn close_find(&mut self) {
        self.put_find_away();
        self.search.forget();
    }

    /// The bar down and its scan stopped, and the pages it has read kept.
    fn put_find_away(&mut self) {
        self.find_open = false;
        self.scan += 1;
        // A panel that came up to hold the results goes back down with them,
        // so that one Escape undoes the whole of what one search did. See
        // `results_borrowed`, which is what keeps this from shutting a panel
        // the reader had open before any of this started.
        if self.results_borrowed {
            self.results_borrowed = false;
            self.set_sidebar(false, false);
        }
        // Back to the tab the reader had, not to whichever one the document
        // has: Pages, searched and put away, came back as Contents.
        if self.tab == Tab::Results {
            self.tab = self
                .tab_before_results
                .take()
                .unwrap_or(if self.headings.is_empty() {
                    Tab::Pages
                } else {
                    Tab::Contents
                });
        }
    }

    /// The panel on its results, remembering the tab it was on.
    fn to_results(&mut self) {
        if self.tab != Tab::Results {
            self.tab_before_results = Some(self.tab);
        }
        self.tab = Tab::Results;
    }

    /// Look for what is in the field. Returns the token of the scan it
    /// started, or `None` when there is nothing to scan — which is what the
    /// caller needs to know before spawning a task to drive it.
    pub fn find(&mut self, query: &str) -> Option<u64> {
        self.find_query = query.to_string();
        self.scan += 1;
        self.revealed = false;
        self.offered_results = false;
        let (page, pages) = (self.page(), self.pages());
        if !self.search.find(query, page, pages) {
            return None;
        }
        if self.sidebar_open {
            self.to_results();
        }
        Some(self.scan)
    }

    /// Read pages until the slice is up. Returns whether there is more to do.
    ///
    /// The whole of the streaming, here rather than in [`crate::search`]
    /// because it is the only part needing a clock and a document. The reason
    /// there is a slice at all is a book and not a page: pdfium reads a page
    /// in 0.18-1.3ms, so what is worth hiding is the 498ms a 376-page book of
    /// typeset mathematics takes.
    pub fn scan_slice(&mut self, token: u64) -> bool {
        if token != self.scan {
            return false;
        }
        let began = std::time::Instant::now();
        while let Some(page) = self.search.wants() {
            let document = self.document.clone();
            self.search.feed(page, || document.text_of(page - 1));
            if began.elapsed().as_secs_f64() * 1000.0 > crate::search::SLICE_MS {
                break;
            }
        }
        self.search.publish();
        // The first result to arrive is the one the reader is taken to, and
        // only the first: after that they are moved by asking rather than by
        // the scan catching up with them somewhere else in the book.
        if !self.revealed && self.search.current().is_some() {
            self.revealed = true;
            self.reveal_match();
        }
        self.show_the_matches();
        self.search.wants().is_some()
    }

    /// **A search puts its results in the panel, as soon as there are any.**
    ///
    /// With the first match rather than with the bar, so a search for
    /// something that is not in the document does not open a panel to say so.
    /// [`Viewer::show_results`] is the same door opened by hand.
    ///
    /// The panel is *borrowed*: it goes down with the bar that opened it, and
    /// one the reader had open before any of this is one they keep. See
    /// [`Viewer::close_find`].
    fn show_the_matches(&mut self) {
        // Once per search, and that is what the flag is for rather than
        // tidiness: a scan is dozens of slices, and a panel reopened on every
        // one of them is a panel the reader cannot close while the book is
        // still being read.
        if self.offered_results || !self.find_open || self.search.state().total == 0 {
            return;
        }
        // The reader's to turn off: the count in the bar still opens it.
        if !self.store.flag("search_shows_sidebar") {
            return;
        }
        self.offered_results = true;
        if !self.sidebar_open {
            self.set_sidebar(true, false);
            self.results_borrowed = true;
        }
        self.to_results();
    }

    /// Move to the next match, or the one before, and go there.
    pub fn step_match(&mut self, forwards: bool) {
        // Nothing to step through, and nothing to say about it: "No matches"
        // on a start screen is an answer to a question nobody asked. The
        // second half of `open_find`'s note — the app leaves ⌘G, ⌘⇧G and ⌘F
        // unflagged, and all three of them are about a document.
        if self.empty() {
            return;
        }
        if self.search.matches().is_empty() {
            // Not yet is not none: the count says "Searching…", and the first
            // match to arrive is where the scan takes the reader anyway.
            if self.search.scanning() {
                return;
            }
            self.notice = if self.search.state().textless {
                "There is no text in this document to search".into()
            } else {
                "No matches".into()
            };
            return;
        }
        self.search.step(forwards);
        self.reveal_match();
    }

    /// Go to one result by its place in the list — a row of the Results tab.
    pub fn go_to_result(&mut self, at: usize) {
        self.search.go_to(at);
        self.reveal_match();
    }

    /// Bring the match the reader is on into view.
    ///
    /// A match is a range of characters and a character knows its box, so this
    /// is arithmetic: the top of the page, plus where the match is on it, less
    /// a third of a screen so there is something above it to read into.
    pub fn reveal_match(&mut self) {
        let Some(hit) = self.search.current() else {
            return;
        };
        let page = match self.layout.box_of(hit.page - 1) {
            Some(page) => page,
            // Paged mode lays out one page and leaves the rest of `boxes`
            // empty, so a match anywhere else is a page to turn to first.
            // `revealMatch` in `viewer.ts` says the same thing: the other
            // pages have no place on the strip at all. **And then it is a
            // place on the page** — turning to it and stopping left a match
            // at the foot of a tall page off the bottom of the window.
            None => {
                self.go_to(Anchor {
                    page: hit.page,
                    offset: 0.0,
                });
                match self.layout.box_of(hit.page - 1) {
                    Some(page) => page,
                    None => return,
                }
            }
        };
        // Where the match lands on the *page as it is being shown*, which a
        // quad in the page's own points is not once the reader has turned or
        // trimmed it. One call rather than a multiplication by the scale —
        // see [`Layout::place_on`].
        let (top, bottom) = self
            .search
            .quads_on(hit.page)
            .into_iter()
            .filter(|(_, current)| *current)
            .map(|(quad, _)| self.layout.place_on(hit.page - 1, quad))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(top, bottom), rect| {
                (top.min(rect.top), bottom.max(rect.top + rect.height))
            });
        // A match already on screen stays where it is: stepping through a
        // paragraph of them jumped the page for every one, and each jump is
        // the reader finding their place again.
        let height = self.layout.viewport.height;
        if top.is_finite()
            && page.top + top >= self.scroll_top
            && page.top + bottom <= self.scroll_top + height
        {
            return;
        }
        let target = if top.is_finite() {
            page.top + top - height * REVEAL
        } else {
            // A match nothing drew — pdfium generates characters the printer
            // never put on the page — is still on a page.
            page.top
        };
        self.scroll_to(target);
    }

    /// Change one of the two switches that decide what is found, and look
    /// again with it.
    ///
    /// The extracted text stays: only the fold and the boundary test depend on
    /// these, so a rescan asks the renderer for nothing —
    /// `changing_the_case_setting_does_not_go_back_to_the_renderer` says so.
    pub fn set_find_options(&mut self, options: Find) -> Option<u64> {
        self.search.set_options(options);
        self.store.set(vec![
            ("search_match_case".into(), json!(options.match_case)),
            ("search_whole_words".into(), json!(options.whole_words)),
        ]);
        let query = self.find_query.clone();
        self.find(&query)
    }

    /// Paint every match, or only the one the reader is on.
    pub fn toggle_highlight_all(&mut self) {
        self.highlight_all = !self.highlight_all;
        self.store.set(vec![(
            "search_highlight_all".into(),
            json!(self.highlight_all),
        )]);
    }

    /// What to paint over one page, in CSS pixels from its top left.
    ///
    /// `(rectangle, is the one the reader is on)`. Empty when the find bar is
    /// down, because a highlight that outlives the bar that made it is a mark
    /// on the page nobody asked for.
    pub fn highlights(&self, page: usize) -> Vec<(Rect, bool)> {
        if !self.find_open {
            return Vec::new();
        }
        if self.layout.box_of(page - 1).is_none() {
            return Vec::new();
        }
        self.search
            .quads_on(page)
            .into_iter()
            .filter(|(_, current)| self.highlight_all || *current)
            .map(|(quad, current)| (self.layout.place_on(page - 1, quad), current))
            .collect()
    }

    /// What the find bar says beside the field: where the reader is in the
    /// matches, or why there are none.
    pub fn find_count(&self) -> String {
        let state = self.search.state();
        if state.query.trim().is_empty() {
            return String::new();
        }
        if state.total == 0 {
            return if state.scanning {
                "Searching…".into()
            } else if state.textless {
                "No text to search".into()
            } else {
                "None".into()
            };
        }
        let at = state.at.map(|at| at + 1).unwrap_or(0);
        let more = if state.capped {
            "+"
        } else if state.scanning {
            "…"
        } else {
            ""
        };
        format!("{at} of {}{more}", state.total)
    }

    /// Relay out around the page the reader is on, which is what every change
    /// of fit, zoom or spread has to do.
    fn keeping_place(&mut self, change: impl FnOnce(&mut Layout)) {
        let anchor = self.layout.anchor(self.scroll_top);
        change(&mut self.layout);
        self.layout.relayout();
        self.scroll_top = self.layout.scroll_target(anchor);
        self.relaid_at = self.scroll_top;
        self.generation += 1;
    }

    pub fn set_fit(&mut self, fit: Fit) {
        // See `set_zoom`: choosing a fit mode ends a gesture too.
        self.zoom_token += 1;
        self.zoom_from = None;
        self.chosen.hold(false);
        self.keeping_place(|layout| layout.fit = fit);
        self.notice = match fit {
            Fit::Width => "Fit width".into(),
            Fit::Page => "Fit page".into(),
            Fit::Actual => "Actual size".into(),
        };
        self.store
            .set(vec![("fit_mode".into(), json!(name_of(fit)))]);
    }

    /// Actual size, which is a fit mode *and* a zoom of 1.
    ///
    /// The pair that never moves alone, and the reason `store.set` takes a
    /// group: written one at a time, a zoom of 1 lands under a fit mode that
    /// ignores it and the next run comes back fitted to the width.
    pub fn actual_size(&mut self) {
        self.keeping_place(|layout| {
            layout.fit = Fit::Actual;
            layout.zoom = 1.0;
        });
        self.notice = "Actual size".into();
        self.store.set(vec![
            ("zoom".into(), json!(1.0)),
            ("fit_mode".into(), json!(name_of(Fit::Actual))),
        ]);
    }

    pub fn zoom(&mut self, closer: bool) {
        // ⌘+ on the start screen, as a pinch there: see [`Viewer::zoom_by`].
        if self.empty() {
            return;
        }
        let current = if self.layout.fit == Fit::Actual {
            self.layout.zoom
        } else {
            // Leaving a fit mode starts from where the fit had got to, so the
            // first press changes the size by one step rather than jumping.
            self.layout
                .box_of(self.page() - 1)
                .map(|page| page.scale / crate::layout::PDF_TO_CSS_UNITS)
                .unwrap_or(1.0)
        };
        let next = if closer {
            ZOOMS.iter().copied().find(|&step| step > current + 0.001)
        } else {
            ZOOMS
                .iter()
                .copied()
                .rev()
                .find(|&step| step < current - 0.001)
        };
        let Some(next) = next else { return };
        self.keeping_place(|layout| {
            layout.fit = Fit::Actual;
            layout.zoom = next;
        });
        self.say_zoom(next);
        // The pair that never moves alone, and the reason `set` takes a group:
        // a zoom without the fit mode it left comes back as a fit width that
        // ignores it.
        self.store.set(vec![
            ("zoom".into(), json!(next)),
            ("fit_mode".into(), json!(name_of(Fit::Actual))),
        ]);
    }

    /// Take the margins off, or put them back.
    ///
    /// The switch is remembered and the measurement is not: the crop is a fact
    /// about a document rather than about the reader, so a run that opens a
    /// different document measures that one.
    ///
    /// **Left on when there is nothing to trim.** [`Viewer::trimmed`] is how
    /// the interface tells "off" from "on, and there was nothing there".
    pub fn set_trim(&mut self, on: bool) {
        self.trimming = on;
        self.store.set(vec![("trim_margins".into(), json!(on))]);
        if !on {
            self.keeping_place(|layout| layout.crop = None);
            self.notice = "Margins put back".into();
            return;
        }
        self.crop_asked = true;
        self.measure_crop();
    }

    pub fn trims_margins(&self) -> bool {
        self.trimming
    }

    /// Whether anything was actually found to trim.
    pub fn trimmed(&self) -> bool {
        self.layout.crop.is_some()
    }

    /// Measure the document and lay it out again against what was found.
    ///
    /// Measured on the page as the *document* has it and then turned to match
    /// the page as the reader has it, which is [`Layout::turn`]'s order:
    /// measuring after a rotation would draw eight pages sideways for an
    /// answer one transposition away from the one in hand.
    ///
    /// **Measured on a thread**: the sample is eight pages drawn through
    /// pdfium, which is a visible stall on a scan, and it was taken at every
    /// open and every rebuild. The answer comes back as `crop-measured` with
    /// the token it was asked under, and [`Viewer::measured`] lays it in.
    ///
    /// **The crop in force stays until the answer.** Cleared here, after the
    /// caller had laid the pages out under it, every box kept the cropped
    /// shape while each page was drawn whole into it — stretched, with clicks
    /// and links in the wrong place — once per highlight written. A new
    /// document clears it before its pages are laid out: see
    /// [`Viewer::take_up`].
    fn measure_crop(&mut self) {
        self.crop_token += 1;
        let (token, document, post) = (self.crop_token, self.document.clone(), self.post.clone());
        let working = crate::stats::Writing::begin();
        std::thread::spawn(move || {
            let _working = working;
            let crop = crate::crop::measure(&document);
            post.send(crate::emit::News {
                event: "crop-measured".into(),
                target: None,
                payload: crate::emit::Payload::Measured(crop, token),
            });
        });
    }

    /// The margins, measured: laid over the document if it is still the one
    /// they were measured off and the reader still wants them trimmed.
    pub fn measured(&mut self, mut crop: Option<crate::layout::Crop>, token: u64) {
        if token != self.crop_token || !self.trimming {
            return;
        }
        let mut turns = (self.layout.rotation / 90) % 4;
        while turns > 0 {
            crop = crop.map(crate::layout::Crop::turned);
            turns -= 1;
        }
        self.keeping_place(|layout| layout.crop = crop);
        if std::mem::take(&mut self.crop_asked) {
            self.notice = if self.trimmed() {
                "Margins trimmed".into()
            } else {
                "There are no margins to trim on this document".into()
            };
        }
    }

    /// Turn the document a quarter at a time.
    ///
    /// Nothing is written down: a rotation is a way of looking rather than a
    /// property of the file, as in Preview, Acrobat and Sumatra.
    ///
    /// **And no cache is thrown away**, where the app clears three: its links,
    /// notes and markup are held as fractions of a page that has just changed
    /// shape. Here they are in the page's own unturned points and
    /// [`Layout::place_on`] does the turning where they are drawn.
    pub fn rotate(&mut self, quarter_turns: i32) {
        // On the start screen there is nothing to turn, and the turn stayed
        // with the window: the next document opened sideways.
        if self.empty() {
            return;
        }
        self.keeping_place(|layout| layout.turn(quarter_turns));
        self.notice = match self.layout.rotation {
            0 => "Upright".into(),
            degrees => format!("Turned {degrees}°"),
        };
    }

    pub fn set_spread(&mut self, spread: Spread) {
        if spread != Spread::Single {
            self.last_pair = spread;
        }
        self.keeping_place(|layout| layout.spread = spread);
        self.store.set(vec![(
            "spread_mode".into(),
            json!(match spread {
                Spread::Single => "single",
                Spread::Two => "two",
                Spread::Cover => "cover",
            }),
        )]);
        // **Two across has to mean two on screen.** At a fixed zoom it does
        // not: 175% is 175% whatever is beside it, so asking for a spread at
        // one put two pages of a letter book across 2,870 pixels of a window
        // half that wide, and centred them — the reader got the inner half of
        // each, which is the single page they had been looking at with a seam
        // down it. So the pair is fitted to the width — **for the moment, and
        // not written down**: the zoom is a setting of its own, and choosing a
        // spread does not change another setting. Back to one page across,
        // the reader's own fit and zoom come back with it.
        //
        // Only out of actual size, because the two fit modes cannot overflow.
        if spread != Spread::Single && self.layout.max_scroll_x() > 0.0 {
            self.keeping_place(|layout| layout.fit = Fit::Width);
            self.notice = "Fit width, to show the pair".into();
        } else if spread == Spread::Single && self.layout.fit != self.stored_fit() {
            let fit = self.stored_fit();
            self.keeping_place(|layout| layout.fit = fit);
        }
    }

    /// Wear the theme at `index` in the list, and remember it.
    pub fn set_theme(&mut self, index: usize) {
        let worn = self.store.wear(index);
        if worn.name.is_empty() {
            return;
        }
        // Every mounted page reads this on its next paint, and the next paint
        // is the frame this change causes. The theme is in each page's key,
        // so every page is a new node drawn afresh — see the header of
        // `page.rs` for why nothing on the GPU is recoloured in place.
        self.chosen.set(self.store.palette());
        // Three things could be said and one line says one, in the order of
        // how much the reader needs to know: a colour the renderer cannot
        // read, then a switch that moved without being touched, then the name
        // of what is now being read in.
        self.notice = match (self.store.complaint.clone(), worn.overruled) {
            (Some(complaint), _) => complaint,
            (None, true) => format!("{}. {UNTIL_THE_SYSTEM_SWITCHES}", worn.name),
            (None, false) => worn.name,
        };
        // The system switching light and dark, or ⌘D, while a theme is being
        // edited changes what Cancel goes back to — not what is on screen,
        // which is the draft until the editor is done with it.
        if self.editing.is_some() && self.pane.is_some() {
            self.preview_draft();
        }
        self.generation += 1;
    }

    /// ⌘D, and the switch on the Appearance page.
    ///
    /// The theme moves to the other half of the pair the reader chose — see
    /// [`Store::other_half`]. ⌘D at noon is a reader wanting the dark theme
    /// *now*, and it holds until the machine next switches — see
    /// [`Store::wear`].
    pub fn toggle_dark(&mut self) {
        self.set_dark(!self.store.dark_now());
    }

    pub fn set_dark(&mut self, on: bool) {
        match self.store.other_half(on) {
            Some(index) => self.set_theme(index),
            // It takes deleting half the themes directory to reach this, and
            // saying nothing would read as a broken key.
            None => {
                self.notice = format!(
                    "There is no {} theme in your themes folder.",
                    if on { "dark" } else { "light" }
                );
            }
        }
    }

    /// The machine's light or dark, at startup and whenever it changes.
    ///
    /// At startup because the machine can have changed its mind while the app
    /// was shut. `None` is not the app's case — a webview always answers this
    /// and a window does not, so a platform that will not say leaves the
    /// reader wearing what they chose. See [`Store::outside`].
    pub fn follow_system(&mut self, dark: Option<bool>) {
        self.store.set_outside(dark);
        if let Some(index) = self.store.following() {
            self.set_theme(index);
        }
    }

    /// The switch itself. Turning it on takes effect at once rather than at
    /// the machine's next change, because a switch that says "follow the
    /// system" and leaves a light theme up on a dark machine has not been
    /// believed by anybody.
    pub fn set_follow_system(&mut self, on: bool) {
        self.store
            .set(vec![("follow_system_theme".into(), serde_json::json!(on))]);
        if on {
            self.store.stop_overruling();
            if let Some(index) = self.store.following() {
                self.set_theme(index);
            }
        }
    }

    /* ------------------------------------- what changed on the disk */

    /// A theme file was written — by hand, by an LLM, or by this app.
    ///
    /// The set is replaced and whatever is in use is put back on from the new
    /// files, so editing a theme beside the reader shows up in it. Nothing is
    /// written down: nobody chose a theme, and an editor saving every few
    /// seconds must not rewrite `settings.toml` every few seconds.
    ///
    /// A theme whose file has *gone* takes the reader somewhere else, and that
    /// one *is* remembered, because it is a choice made on their behalf.
    ///
    /// A theme being composed in the editor is the live theme and has no id,
    /// so every save would otherwise read as "the theme you are reading in has
    /// been deleted" — `isEditingTheme()` is the guard.
    pub fn themes_changed(&mut self, themes: Vec<crate::theme::Theme>) {
        // The guard that comment promises. Without it a draft — which is in
        // no file — read as deleted, and the reader was moved to another
        // theme *and that was written down*, from under the open editor.
        if self.editing.is_some() && self.pane.is_some() {
            self.store.set_themes(themes);
            self.preview_draft();
            return;
        }
        let before = self.store.theme().clone();
        let mut themes = themes;
        // **Missing is not gone.** A theme saved from an editor with a typo in
        // it does not parse, and is not in the list — but its file is there,
        // and treating it as deleted moved the reader to another theme and
        // wrote that down, so fixing the typo brought the theme back with
        // nothing pointing at it. The last good copy stays on until the file
        // reads again.
        let unreadable = !before.id.is_empty()
            && !themes.iter().any(|theme| theme.id == before.id)
            && self
                .store
                .themes_dir()
                .join(format!("{}.toml", before.id))
                .exists();
        if unreadable {
            self.notice = format!(
                "{}.toml could not be read, so the last version that could is still on.",
                before.id
            );
            themes.push(before.clone());
        }
        self.store.set_themes(themes);
        let still_there = self
            .store
            .themes()
            .iter()
            .any(|theme| theme.id == before.id);
        if still_there {
            // Unconditional, as the app's is: a theme that came back
            // unchanged costs one comparison in the widget and nothing else.
            self.chosen.set(self.store.palette());
            // Said only when there is something to say: a theme file saved
            // elsewhere wiped whatever the notice line was holding.
            if let Some(complaint) = self.store.complaint.clone() {
                self.notice = complaint;
            }
        } else if let Some(index) = self
            .renamed_elsewhere()
            .or_else(|| self.store.replacement_for(&before))
        {
            let worn = self.store.wear(index);
            self.chosen.set(self.store.palette());
            self.notice = format!("{} is gone. Now reading in {}.", before.name, worn.name);
        }
        self.generation += 1;
    }

    /// Where the worn theme went if another window renamed it: that window
    /// moved the settings to the new name a moment before this news arrived,
    /// and replacing the theme here wrote the replacement over its choice.
    /// One small read, and only when the worn theme has gone.
    fn renamed_elsewhere(&self) -> Option<usize> {
        let settings = crate::settings::load(self.store.dir());
        let id = settings.get("theme")?.as_str()?;
        self.store.themes().iter().position(|theme| theme.id == id)
    }

    /// The open document was rewritten underneath the reader.
    ///
    /// A paper recompiled by LaTeX is the case this exists for, and the reader
    /// stays where they were. Where that is comes off the layout rather than
    /// out of the library: the library has the last position *written down*,
    /// and this is the one moment the two can differ by a whole scroll.
    ///
    /// What goes is everything read out of the old file — outline, labels,
    /// links, index, crop — and everything pointing into it, which is the
    /// history. What stays is the reader's own: fit, zoom, spread, rotation,
    /// panel, theme.
    ///
    /// Answers the token of the scan it restarted, or `None`; a task is the
    /// caller's to spawn.
    pub fn document_changed(&mut self, path: &str) -> Option<u64> {
        if path != self.document.path() {
            return None;
        }
        // A write or reload in flight may have read the file before this
        // draft arrived, so it is owed another when it lands: dropped, a
        // compiler's second draft stayed off screen until its third.
        if self.writing.is_some() {
            self.reload_owed = true;
            return None;
        }
        // On a thread, like a write: the reopen loads every page for its
        // size, which is seconds on a scanned volume. The document in hand is
        // kept until the new one is ready, so nothing goes blank meanwhile.
        self.offload(
            false,
            |_| Ok(()),
            |viewer, _| {
                // Still asked, because asking is what *renames* the document.
                // What is not done with the answer is announce it: a reader
                // watching a paper recompile sees the page redraw and the title
                // change, and "Reloaded — the document changed on disk" only
                // worries somebody who did not know what a reload is.
                if viewer.store.renamed(&viewer.document.title()) {
                    // The window's own title is the shell's to set, and it is
                    // only ever told through `Showing`. Same path: the desk,
                    // the restore list and the watch all find nothing to do.
                    viewer.frame.ask(Ask::Showing {
                        path: viewer.document.path().to_string(),
                        title: viewer.store.title().to_string(),
                    });
                }
            },
        );
        None
    }

    /// Whether a write is still in flight, said if so. One at a time: the
    /// second would be editing a file the first is about to replace.
    fn busy(&mut self) -> bool {
        if self.writing.is_some() {
            self.notice = if self.reloading {
                "The document is being reloaded. Try again in a moment.".into()
            } else {
                "Still writing the last change into the document.".into()
            };
        }
        self.writing.is_some()
    }

    /// Write the document on a thread of its own, and carry on when it lands.
    ///
    /// `work` is the whole file read, rewritten by `FPDF_SaveAsCopy` and
    /// written back, and the reopen after it loads every page for its size —
    /// invisible on a paper and seconds on a scanned volume, which used to be
    /// seconds of a window that would not move. Both are on the thread; `done`
    /// is the caller's second half, run by [`Viewer::landed`] once the
    /// mailbox says `document-written`.
    ///
    /// **The file is let go of first, and reopened whatever happens**: pdfium
    /// holds it open, and on Windows nothing can rename over it while it does.
    /// See [`crate::render::PageSource::release`].
    ///
    /// The watch is told the burst on its way is ours from the thread, right
    /// after the write and before the reopen — inside its settle window, or it
    /// would reload the document a second time. See
    /// [`crate::watch::Watching::wrote`]. **Not said on the way through
    /// `document_changed`**, where it used to be: `wrote` retakes the baseline
    /// from the disk, and a compiler's next draft landing in that moment
    /// became the baseline and was never reported.
    // ponytail: pdfium's one lock is held for the length of the save, so a
    // page mounted for the first time in that moment still waits for it.
    fn write(
        &mut self,
        work: impl FnOnce(&str) -> Result<(), String> + Send + 'static,
        done: impl FnOnce(&mut Viewer, Result<(), String>) + 'static,
    ) {
        // **Only into the draft on screen.** A mark is a page and an index or
        // a set of quads in *this* file; applied to one Zotero or a compiler
        // wrote since, it takes out or covers something else. Refused, the
        // reopen below still runs and brings the new draft in.
        let expected = self.document.stamp();
        let work = move |path: &str| {
            if expected.is_some() && crate::render::stamp_of(path) != expected {
                return Err("The document changed on disk. Try again now it has reloaded.".into());
            }
            work(path)
        };
        self.document.release();
        self.offload(true, work, done);
    }

    /// The thread under [`Viewer::write`], which [`Viewer::document_changed`]
    /// shares for the reopen alone: `ours` is whether the watch is to be told
    /// the burst on its way is this reader's.
    fn offload(
        &mut self,
        ours: bool,
        work: impl FnOnce(&str) -> Result<(), String> + Send + 'static,
        done: impl FnOnce(&mut Viewer, Result<(), String>) + 'static,
    ) {
        let path = self.document.path().to_string();
        let password = self.document.password().map(str::to_string);
        let landing = Arc::new(Mutex::new(None));
        self.writing = Some((Arc::clone(&landing), Box::new(done)));
        self.reloading = !ours;
        let (watching, window, post) = (
            self.watching.clone(),
            self.window.clone(),
            self.post.clone(),
        );
        let writing = crate::stats::Writing::begin();
        std::thread::spawn(move || {
            let _writing = writing;
            let written = work(&path);
            // Only a write that happened is a burst of ours: a refused one
            // left the disk to whoever wrote it last, and taking the baseline
            // from that would swallow their next draft.
            if let Some(watching) = watching.filter(|_| ours && written.is_ok()) {
                watching.wrote(&window, std::path::Path::new(&path));
            }
            let reopened = crate::render::open_with(&path, password.as_deref());
            *landing.lock().unwrap_or_else(|e| e.into_inner()) = Some((written, reopened));
            post.send(crate::emit::News {
                event: "document-written".into(),
                target: None,
                payload: crate::emit::Payload::Nothing,
            });
        });
    }

    /// A write of this reader's own has landed: take up the document it left
    /// and run the second half of whatever asked for it.
    ///
    /// Nothing happens for a write nobody is waiting for any more — the
    /// reader opened another document in the meantime, which forgot it.
    ///
    /// Answers the token of the scan it restarted, as `document_changed` does.
    pub fn landed(&mut self) -> Option<u64> {
        let (landing, _) = self.writing.as_ref()?;
        let (written, reopened) = landing.lock().unwrap_or_else(|e| e.into_inner()).take()?;
        let (_, done) = self.writing.take()?;
        let restarted = self.adopt(reopened);
        done(self, written);
        if std::mem::take(&mut self.reload_owed) {
            let path = self.document.path().to_string();
            self.document_changed(&path);
        }
        restarted
    }

    /// The second half of a reopen, for a document [`Viewer::offload`]'s
    /// thread opened: a write of this reader's own, or a rebuild's.
    fn adopt(
        &mut self,
        reopened: Result<Arc<dyn PageSource>, crate::render::Refusal>,
    ) -> Option<u64> {
        let at = self.layout.anchor(self.scroll_top);
        let reopened = match reopened {
            Ok(document) => document,
            // A compiler that is still writing is what `whole()` in `watch.rs`
            // is there to rule out, so this is the genuinely broken file —
            // and the document already open is the better thing to be looking
            // at than an empty window. If it was let go of for a write that
            // then failed, it is taken up again, or every page is blank until
            // ⌘O.
            Err(refused) => {
                self.document.retake();
                // Whatever was asked of it while it was let go of was
                // answered with nothing, and cached.
                self.links.borrow_mut().clear();
                self.notes.borrow_mut().clear();
                self.texts.borrow_mut().clear();
                self.notice = format!("The document could not be reopened: {refused}");
                return None;
            }
        };
        self.document = reopened;
        self.chosen.show(self.document.clone());
        self.headings = self.document.outline();
        self.picked_heading = None;
        self.labels = self.document.labels();
        self.links.borrow_mut().clear();
        self.notes.borrow_mut().clear();
        // And the text with them, along with whatever was selected: both are
        // indices into a document that no longer exists. The markup journal is
        // where a passage *does* survive a rebuild, and it survives as a quote
        // to be looked up again rather than as a range.
        //
        // Before the markup is read, which reads its quotes off this text.
        self.texts.borrow_mut().clear();
        self.read_markup();
        // An annotation's index is its place in a list that was just
        // rewritten: a popover or a Sign window still holding one would take
        // the wrong annotation out of the file.
        self.mark_open = None;
        self.markup_at = None;
        if self.signing.is_some() {
            let (signed_here, seals) = (self.signed_here(), self.seals());
            if let Some(signing) = self.signing.as_mut() {
                signing.signed_here = signed_here;
                signing.seals = seals;
            }
        }
        self.selection = None;
        self.sweep_from = None;
        // A rebuild's pages are not the ones the history was taken on. A write
        // of our own changed a mark, never a page, and following a reference,
        // marking it and stepping back is the whole of reading one.
        if self.reloading {
            self.past.clear();
            self.future.clear();
        }
        let sizes = (0..self.document.pages())
            .map(|index| self.document.size_of(index))
            .collect();
        self.layout.replace_sizes(sizes);
        // The thumbnail column is laid out from those sizes, and a draft of a
        // different length leaves it a page short or a page long. `resize`
        // rebuilds it for a window that changed, and the window did not.
        self.relay_column();
        if self.trimming {
            // Measured again rather than kept: the margins are a fact about
            // the file, and this is a different file.
            self.measure_crop();
        }
        self.edition += 1;
        self.generation += 1;
        // Clamped by `go_to`, so a draft that lost its last chapter lands on
        // the end of what is left rather than nowhere.
        self.go_to(at);
        // A new draft under the reader is not the reader scrolling.
        self.relaid_at = self.scroll_top;
        if self.find_open {
            let query = self.find_query.clone();
            self.search.forget();
            self.find(&query)
        } else {
            None
        }
    }

    /// A different document, in this window. ⌘O, and the menu item under it.
    ///
    /// **⌘O replaces the document in the window it was pressed in** and ⇧⌘O
    /// asks for a window, which is the app's split: ⌘O is the reader saying
    /// *this one instead*.
    ///
    /// Everything [`Viewer::document_changed`] clears is cleared, **and the
    /// library entry with it**: a recompile is the same document and this is a
    /// different one, so the marks, the title and the remembered place all
    /// move. What stays is the reader's own — fit, zoom, spread, rotation,
    /// panel, theme — because a setting is not a property of a document.
    ///
    /// Answers whether it opened; a document that would not open leaves this
    /// window showing the one it had. The find bar is closed rather than
    /// searched again, which is where this parts company with
    /// `document_changed`: a query asked of a paper is not a query asked of
    /// the next book.
    pub fn open_here(&mut self, path: &str) -> bool {
        self.open_here_with(path, None)
    }

    /// The same, with the password for a document that wants one.
    ///
    /// **Locked is not an error here, it is a question**, and this is the one
    /// place that turns it into one: [`Locked`] goes up and the window keeps
    /// what it had. The answer comes back through [`Self::unlock`].
    ///
    /// Whether the sentence says "it needs a password" or "that one was not
    /// right" is decided here rather than by pdfium, which reports both as
    /// `FPDF_ERR_PASSWORD`: the difference is whether this call supplied
    /// one.
    fn open_here_with(&mut self, path: &str, password: Option<&str>) -> bool {
        // The picker, a drop and a handover all arrive here; the command line
        // and the socket were made absolute at the door. See `config::absolute`.
        let path = &crate::config::absolute(path);
        if path == self.document.path() {
            self.notice = "That document is already open here.".into();
            return false;
        }
        // Open in another window: that one comes forward, through the same
        // door a second launch uses. Two windows on one document each write
        // the whole of its marks, and the last one wins.
        if self.shown_elsewhere(path) {
            self.notice = "That document is open in another window.".into();
            self.frame.ask(Ask::NewWindowOn(path.clone()));
            return false;
        }
        let opened = match crate::render::open_with(path, password) {
            Ok(document) => document,
            Err(crate::render::Refusal::Locked) => {
                self.locked = Some(Locked {
                    path: path.to_string(),
                    typed: String::new(),
                    wrong: password.is_some(),
                });
                // **Asking is showing, as far as the desk is concerned.** A
                // window down as empty is where the next document handed
                // over goes, and this one would send it on — back to the
                // desk, back to this window, for ever. See `stop_unlocking`.
                self.frame.ask(Ask::Showing {
                    path: path.to_string(),
                    title: crate::store::called(path, ""),
                });
                return false;
            }
            Err(refused) => {
                // Through `stop_unlocking`, which tells the desk: a prompt
                // given up here left it believing this window showed the
                // locked document, and sent anybody opening it here.
                self.stop_unlocking();
                self.notice = format!("Could not open that document: {refused}");
                return false;
            }
        };
        self.locked = None;
        // Where the reader was in the document being put down, written before
        // the store stops pointing at it. `remember` hands it to the scribe,
        // which keeps one place per document — so this cannot be skipped on
        // the grounds that the scroll has not moved since the last one.
        self.store.remember(self.layout.anchor(self.scroll_top));
        let declared = opened.title();
        self.document = opened;
        let place = self.store.opened(path, &declared);
        self.take_up(place);
        true
    }

    /// The document is put down and this window is showing none.
    ///
    /// **The gesture the app calls Close**, which is not closing the window:
    /// the window stays, the toolbar keeps what is not about a document, and
    /// the start screen is what is in front of the reader. It is also the one
    /// gesture that empties the restore list — a window that goes because the
    /// app is quitting was open at the end, and a document the reader put down
    /// is one they have finished with — and that is the caller's to say,
    /// because the list belongs to the process.
    ///
    /// Everything [`Viewer::open_here`] clears is cleared, and what takes the
    /// document's place is [`crate::render::Nothing`].
    pub fn close_document(&mut self) {
        if self.empty() {
            return;
        }
        // Where they got to, written while the store still points at the file
        // it is about. Exactly as `open_here` does it, and for the same
        // reason: this is the last moment either half is true.
        self.store.remember(self.layout.anchor(self.scroll_top));
        // **Written now rather than eventually**, which is the one place in
        // this reader that waits for the scribe: the screen about to go up
        // says, on its first row, where the reader stopped in the document
        // they are putting down. Read before the file has caught up, that is
        // the page they were on when they last *opened* it — stale exactly
        // when it is most looked at.
        crate::store::flush();
        self.document = crate::render::nothing();
        self.store.closed();
        self.take_up(None);
        // Nothing to say. The screen that arrives says what it is.
        self.notice = String::new();
    }

    /// Whether this window has a document in it.
    ///
    /// Asked in the two places it decides something: what the toolbar carries,
    /// and whether the body is the document or the start screen. Everything
    /// else goes on being written for a document, because a document of no
    /// pages answers every question already — see [`crate::render::Nothing`].
    pub fn empty(&self) -> bool {
        self.document.pages() == 0
    }

    /// The last few documents read, for the start screen and the Open menu,
    /// with the one already open left out — reopening it here would be a
    /// no-op, and opening it in a *second* window is what the title menu is
    /// for. Six, which is the app's number; the menu takes what it wants.
    pub fn recents(&self) -> Vec<crate::store::Recent> {
        let open = self.document.path();
        self.store
            .recents()
            .into_iter()
            .filter(|entry| entry.path != open)
            .take(6)
            .collect()
    }

    /// Take a document off the recently-read list, and off the shelf.
    pub fn forget(&mut self, path: &str) {
        // Forgetting is losing the marks and the place, and a window reading
        // the document would go on writing them into an entry that is gone.
        if self.shown_elsewhere(path) {
            self.notice = "That document is open in another window.".into();
            return;
        }
        self.store.forget(path);
        self.generation += 1;
    }

    /// Whether another window of this process is showing the document.
    fn shown_elsewhere(&self, path: &str) -> bool {
        self.desk
            .as_ref()
            .and_then(|desk| desk.shown_by(path))
            .is_some_and(|label| label != self.window)
    }

    /// Everything a window has to put down when the document under it changes,
    /// and everything it has to pick up again.
    ///
    /// Shared by [`Viewer::open_here`] and [`Viewer::close_document`] because
    /// the list is the part that is easy to get wrong: a cache left pointing
    /// into the document that was there a moment ago is a rectangle drawn over
    /// the wrong page.
    fn take_up(&mut self, place: Option<crate::layout::Anchor>) {
        // A write still in flight was into the document put down, and what
        // it lands as is nothing this one wants. See [`Viewer::landed`].
        self.writing = None;
        self.reload_owed = false;
        self.headings = self.document.outline();
        // An index into the outline just replaced.
        self.picked_heading = None;
        self.labels = self.document.labels();
        // A different document has different markup, and its own answer to
        // whether it can be written — and `said_standing` goes with it,
        // because "said once" means once per document.
        self.links.borrow_mut().clear();
        self.notes.borrow_mut().clear();
        // Before the markup is read, which reads its quotes off this text.
        self.texts.borrow_mut().clear();
        self.read_markup();
        self.said_standing = false;
        self.said_rewrites = false;
        self.markup_at = None;
        self.mark_open = None;
        self.signing = None;
        // A signature armed for the last document would land on the first
        // click in this one, with nothing on screen saying it was armed.
        self.placing = None;
        self.note_open = None;
        self.selection = None;
        self.sweep_from = None;
        self.past.clear();
        self.future.clear();
        self.search.forget();
        self.close_find();
        // Nothing until the answer: a different document laid out under the
        // last one's margins is every page drawn once wrong and once right.
        self.layout.crop = None;
        let sizes = (0..self.document.pages())
            .map(|index| self.document.size_of(index))
            .collect();
        self.layout.replace_sizes(sizes);
        if self.trimming {
            self.measure_crop();
        }
        // A document with no contents opens on its pages, which is what
        // `restore` does at startup and what `setDocument` does in the app:
        // the difference between a panel and an empty box.
        self.tab = if self.headings.is_empty() {
            Tab::Pages
        } else {
            Tab::Contents
        };
        self.edition += 1;
        self.opened += 1;
        self.generation += 1;
        self.chosen.show(self.document.clone());
        self.scroll_top = 0.0;
        self.go_to(place.unwrap_or(crate::layout::Anchor {
            page: 1,
            offset: 0.0,
        }));
        self.relaid_at = self.scroll_top;
        self.relay_column();
        self.revealed = false;
        self.reveal_thumb();
        self.notice = String::new();
    }

    /// The next theme in the list, which is what `t` is bound to.
    ///
    /// Not the app's gesture — the Theme menu is — but the one keystroke that
    /// proves the whole list is loaded and wearable, which is what
    /// `the_whole_shipped_theme_set_is_wearable` presses.
    pub fn next_theme(&mut self) {
        let next = (self.store.theme_index() + 1) % self.store.themes().len().max(1);
        self.set_theme(next);
    }

    /// How far the document is scrolled across, in CSS pixels — nothing at
    /// all unless it is wider than the window. See [`Viewer::across`].
    pub fn scroll_left(&self) -> f64 {
        let room = self.layout.max_scroll_x();
        if room <= 0.0 {
            return 0.0;
        }
        (self.across * self.layout.content_width() - self.layout.viewport.width / 2.0)
            .clamp(0.0, room)
    }

    /// Move the document across under the window, which is what a trackpad's
    /// other axis and ⇧-wheel ask for. A no-op when there is nothing to move,
    /// and it puts the middle back in the middle when there stops being: a
    /// reader who zooms out to a page that fits should not find it off to one
    /// side because of where they had panned to.
    pub fn pan(&mut self, delta: f64) {
        let room = self.layout.max_scroll_x();
        if room <= 0.0 {
            self.across = 0.5;
            return;
        }
        if delta == 0.0 {
            return;
        }
        let to = (self.scroll_left() + delta).clamp(0.0, room);
        self.across = (to + self.layout.viewport.width / 2.0) / self.layout.content_width();
    }

    /// Where the reader would end up, clamped, in CSS pixels.
    pub fn scroll_by(&self, delta: f64) -> f64 {
        (self.scroll_top + delta).clamp(0.0, self.layout.max_scroll())
    }

    /// Move the document under the window. The one place `scroll_top` changes.
    pub fn scroll_to(&mut self, top: f64) -> bool {
        // Counted before the clamp, because a wheel at the top of the document
        // is still a scroll: the pill flashes for the gesture, not for the
        // offset. See [`Viewer::scroll_gesture`].
        self.scrolls = self.scrolls.wrapping_add(1);
        self.relaid_at = f64::NAN;
        let to = top.clamp(0.0, self.layout.max_scroll());
        if (to - self.scroll_top).abs() < 0.01 {
            return false;
        }
        self.scroll_top = to;
        // The column follows the document, and only when it has to — see
        // `Column::reveal`. `setPage` in `sidebar.ts` is this call.
        self.reveal_thumb();
        self.remember_place();
        true
    }

    /// Note where the reader is, for the next run.
    ///
    /// Called from the two places the reader actually moves — this one, and a
    /// page turned in paged mode, which does not go past [`Viewer::scroll_to`]
    /// when both sides of the turn sit at the top of their page. Costs a
    /// channel send: the disk is the scribe's. See [`Store::remember`].
    fn remember_place(&self) {
        // Not while the last run's place is still waiting for a window to be
        // put back in: the relayouts on the way to the first frame would
        // record page one over the place being restored.
        if self.place.is_some() {
            return;
        }
        self.store.remember(self.layout.anchor(self.scroll_top));
    }

    /* --------------------------------------------------------- the scrollbar

    The one piece of the platform's scrolling this reader had to build for
    itself. `overflow: scroll` is not what moves this document — see the note
    at the top of this file — so there is no scrollbar to inherit, and a
    reader nine hundred pages into a book had nothing on screen saying so.

    It is drawn over the document rather than beside it: a column that took
    twelve pixels of layout would move the page off centre every time a
    document became long enough to need one. */

    /// The thumb: where it starts and how tall it is, in the viewer's own
    /// pixels. `None` when the document fits, which is when there is nothing
    /// to draw.
    ///
    /// The floor is the whole of what makes it usable in a long book: the
    /// honest height for one page of four hundred is two pixels, which is not
    /// something a pointer can catch.
    pub fn bar_thumb(&self) -> Option<(f64, f64)> {
        let room = self.layout.max_scroll();
        let track = self.layout.viewport.height;
        if room <= 0.5 || track <= MIN_THUMB {
            return None;
        }
        let content = self.layout.content_height().max(1.0);
        let thumb = (track * track / content).clamp(MIN_THUMB, track);
        let top = (self.scroll_top / room) * (track - thumb);
        Some((top.clamp(0.0, track - thumb), thumb))
    }

    /// The bar was pressed at this `client_y`.
    ///
    /// One handler for both gestures the bar has: on the thumb it is a drag,
    /// anywhere else it is a jump — the thumb comes to the pointer and the
    /// drag begins from there, which is what every scrollbar built since the
    /// mouse wheel does and is one branch rather than two handlers.
    pub fn press_bar(&mut self, client_y: f64) {
        let Some((top, thumb)) = self.bar_thumb() else {
            return;
        };
        let y = client_y - self.chrome();
        if !(top..top + thumb).contains(&y) {
            let track = self.layout.viewport.height - thumb;
            let room = self.layout.max_scroll();
            if track > 0.0 {
                self.scroll_to((y - thumb / 2.0) / track * room);
            }
        }
        self.bar_from = Some((client_y, self.scroll_top));
    }

    /// The pointer has moved to `client_y`. A no-op outside a drag, which is
    /// what lets this hang off the root's `onmousemove` — `drag_sidebar`'s
    /// reason, and the same guard.
    pub fn drag_bar(&mut self, client_y: f64) {
        let Some((from, was)) = self.bar_from else {
            return;
        };
        let Some((_, thumb)) = self.bar_thumb() else {
            return;
        };
        let track = self.layout.viewport.height - thumb;
        if track <= 0.0 {
            return;
        }
        self.scroll_to(was + (client_y - from) * self.layout.max_scroll() / track);
    }

    pub fn drop_bar(&mut self) {
        self.bar_from = None;
    }

    /// Whether the reader is on the bar right now. The page count follows it
    /// while they are, whatever the setting says — see [`Viewer::pill_shown`].
    pub fn dragging_bar(&self) -> bool {
        self.bar_from.is_some()
    }

    /* -------------------------------------------------- the stationary scroll

    The middle button drops an anchor in the page and the document runs under
    it, the faster the further the pointer is carried from it. Firefox's
    gesture, down to the curve in [`still_speed`] and to the dead zone around
    the anchor, because half of what makes it good is that it is already in
    the reader's fingers.

    Nothing about it is a setting. It answers to a button nothing else in this
    reader uses, it says what it is doing with a marker on screen, and every
    press, key and wheel ends it — so there is nothing for a reader who does
    not want it to turn off. */

    /// The middle button went down at `at`, in the window's own coordinates.
    ///
    /// Pressed again while one is running it puts the gesture away instead,
    /// which is the other half of what Firefox answers to. Returns the token
    /// of the run to be stepped, or `None` when the press ended one.
    pub fn hold_still(&mut self, at: (f64, f64)) -> Option<u64> {
        if self.still_from.is_some() {
            self.stop_still();
            return None;
        }
        self.still_from = Some(at);
        self.still_at = at;
        self.still_token += 1;
        Some(self.still_token)
    }

    /// The pointer moved. A no-op outside the gesture, which is what lets this
    /// hang off the root's `onmousemove` — [`Viewer::drag_bar`]'s reason, and
    /// the same guard.
    pub fn steer_still(&mut self, at: (f64, f64)) {
        if self.still_from.is_some() {
            self.still_at = at;
        }
    }

    pub fn stop_still(&mut self) {
        self.still_from = None;
    }

    /// Whether one is running, and where its anchor is drawn.
    pub fn scrolling_still(&self) -> bool {
        self.still_from.is_some()
    }

    /// Where the marker is drawn — which is not quite where the button went
    /// down, at the edges of the window.
    ///
    /// **The whole of it is always on screen.** Pressed hard against the side
    /// of the window a circle centred on the pointer would be half a circle,
    /// and half a circle at the edge of the screen reads as a rendering
    /// fault rather than as a mark; so it is held back by its own radius and
    /// its edge comes to rest against the window's, which is what Firefox
    /// does. What the speed is measured from is still the press itself — see
    /// [`Viewer::still_speed`] — so the gesture does not move when the marker
    /// does.
    ///
    pub fn still_anchor(&self) -> Option<(f64, f64)> {
        let (x, y) = self.still_from?;
        let edge = STILL_MARK / 2.0;
        let top = self.chrome() + edge;
        Some((
            x.clamp(edge, (self.window_width - edge).max(edge)),
            y.clamp(top, (self.window_height - edge).max(top)),
        ))
    }

    /// How far to move this step, across and down — or `None` when this run is
    /// over and the clock should stop re-arming itself.
    ///
    /// **Asked before anything is written**, because the gesture spends most
    /// of its life parked in the dead zone: a step that wrote the viewer to
    /// move it nothing would be a render sixty times a second for as long as
    /// the anchor is up. See the note on `use_effect` in `AGENTS.md`.
    pub fn still_speed(&self, token: u64) -> Option<(f64, f64)> {
        let from = self.still_from.filter(|_| self.still_token == token)?;
        let step = STILL_TICK.as_secs_f64() * 1000.0 / STILL_STEP;
        Some((
            still_speed(self.still_at.0 - from.0) * step,
            still_speed(self.still_at.1 - from.1) * step,
        ))
    }

    /// And the step itself.
    ///
    /// [`Viewer::nudge`] is deliberately not used: it turns the page when
    /// there is nowhere left to scroll, and a gesture that steps sixty times a
    /// second would turn sixty pages. In paged mode this runs within the page
    /// and stops at its edges, which is what that mode means.
    pub fn drift(&mut self, across: f64, down: f64) {
        if across != 0.0 {
            self.pan(across);
        }
        if down != 0.0 {
            let to = self.scroll_by(down);
            self.scroll_to(to);
        }
    }
}

/// One mounted page as the `rsx!` block needs it: which page, where its box
/// is, and the two overlays that go on top of it.
///
/// A struct rather than a tuple at seven things: reading the fourth `f64` in a
/// row off its position is correct until somebody inserts a field.
struct Placed {
    index: usize,
    top: f64,
    left: f64,
    width: f64,
    height: f64,
    hits: Vec<(Rect, bool)>,
    links: Vec<(Rect, Target)>,
    /// The notes somebody else left on this page. See [`crate::render::Note`].
    notes: Vec<(Rect, crate::render::Note)>,
    /// What the reader has swept over, on this page, in the same space as the
    /// other two. See [`crate::select`].
    selected: Vec<Rect>,
    /// Where the colour popover goes, when it is over this page. See
    /// [`Viewer::markup_at`].
    swatches: Option<Rect>,
    /// The size the page's texture is keyed on, which is its box except under
    /// a zoom gesture. See [`Viewer::zoom_held_at`].
    drawn: (f64, f64),
    /// The mark the reader clicked on, when it is on this page. See
    /// [`Viewer::mark_open`].
    mark: Option<(Rect, MarkKey, String)>,
}

/// The element that wants the keyboard, as a selector.
///
/// **A click takes the focus away from the reader, and the reader cannot take
/// it back.** Blitz walks up from whatever was clicked looking for something
/// it knows how to focus, and a plain `<button>` is not on that list, so the
/// focus ends up on `<html>` — which is above anything a component can put a
/// handler on. From the first click onwards every shortcut did nothing, and it
/// was invisible for two phases because no test had pressed a key *after*
/// clicking something.
///
/// It cannot be answered from inside the reader either: `MountedData::set_focus`
/// takes `doc_mut()`, and every place a component could call it from is already
/// inside a borrow of the document — a task spawned from one included.
///
/// So the element that wants the keyboard says so and whoever owns the window
/// hands it back: `shell.rs` after a real click, the harness after a
/// synthesised one. `reclaimKeyboard()` in `main.ts` is the same division.
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

/// A field that wants its caret at the end, once: `data-caret="end"`; or its
/// whole contents selected, once: `data-caret="all …"`, which the find field
/// asks for on every ⌘F with a new number after the word.
///
/// The find field comes back holding the query it went down with, and the
/// editor Blitz builds for it puts the caret at the front, so typing landed in
/// front of the old query. A component has no door to the editor — it is
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
/// what is in it. A click selects it instead: [`select_on_arrival`]. `before` is whatever had the focus before the key, so a
/// key typed into a field that already had it moves nothing. `true` when a
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
/// Without this there was no way for Escape in a field to mean "out of this
/// field" that did not also mean "out of this window": the key reached the
/// root's handler and closed Settings from under somebody who was correcting
/// six hexadecimal digits.
pub type RootFocus = Signal<Option<std::rc::Rc<MountedData>>>;

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

/// The whole window.
///
#[component]
pub fn Reader(
    document: Handle,
    chosen: Chosen,
    config: Config,
    /// A document this window is to ask for the password to, if there is one.
    ///
    /// A prop rather than a message down the mailbox because it is true
    /// *before* the first frame, and a question posted to a window that has
    /// not rendered yet is a question about arrival order.
    #[props(default)]
    asking: Option<String>,
    /// Why the document this window was made for would not open, said on the
    /// notice line of the empty window made instead.
    #[props(default)]
    refused: Option<String>,
) -> Element {
    crate::stats::add(&crate::stats::RENDERS, 1);
    // The root's own handle on itself, for everything below that has to give
    // the keyboard back to it. Provided here so that the Settings window and
    // the fields in it can reach it. See [`RootFocus`].
    let mut root_focus: RootFocus = use_context_provider(|| Signal::new(None));
    // The viewport, taken from the window rather than from the element:
    // `get_client_rect` panics inside the document borrow every handler
    // already holds. The chrome above and below is a number this file knows,
    // and the scroll event carries the real client size, so the first scroll
    // corrects whatever this got wrong.
    let screen = use_hook(|| {
        dioxus_core::try_consume_context::<Screen>()
            // Nothing provided one, which is a shell that forgot rather than a
            // situation to cope with — so the number is stated rather than
            // guessed, and it is the one `main.rs` defaults to.
            .unwrap_or_else(|| Screen::fixed(1100.0, 900.0, 1.0))
    });
    // …and what it says about light and dark, which is asked in the same
    // breath and for the same reason. See [`Appearance`].
    let appearance = use_hook(|| {
        dioxus_core::try_consume_context::<Appearance>().unwrap_or_else(Appearance::unknown)
    });
    let mut viewer = use_signal(|| {
        let mut store = Store::at(&config.dir);
        if let Some(index) = config.theme {
            store.wear_for_now(index);
        }
        let mut viewer = Viewer::new(document.0.clone(), chosen.clone(), store);
        viewer.window = config.window.clone();
        viewer.watching = dioxus_core::try_consume_context::<Arc<crate::watch::Watching>>();
        viewer.post = dioxus_core::try_consume_context::<crate::emit::Post>().unwrap_or_default();
        viewer.desk = dioxus_core::try_consume_context::<crate::windows::Desk>();
        viewer.frame =
            dioxus_core::try_consume_context::<Frame>().unwrap_or_else(Frame::unanswered);
        viewer.restore();
        // **Before the first frame, like the viewport above it**, for the
        // reader's sake rather than the renderer's: a machine in dark mode
        // must never see a white page on the way in. One question of the
        // window, and nothing else on the ordinary launch.
        viewer.follow_system(appearance.get());
        // **Sized before the first frame, not on mount.** Laid out at a
        // default viewport and corrected by `onmounted`, every page was drawn,
        // re-keyed and drawn again — a round of pdfium renders thrown away on
        // every launch, and a frame in which *every* node is replaced at once.
        // That frame is the one that crashes: a texture registered while
        // something else is unregistered is one Vello cannot find at submit,
        // and the `fresh` flag only moves the collision a frame along. Asking
        // the window its size first takes the collision away.
        let (width, height, _scale) = screen.get();
        viewer.fit_window(width, height);
        if let Some(said) = refused.clone() {
            viewer.notice = said;
        }
        // And the question, if this window was made to ask one.
        if let Some(path) = asking.clone() {
            viewer.locked = Some(Locked {
                path,
                typed: String::new(),
                wrong: false,
            });
        }
        viewer
    });
    // Where a link out of the document goes. See [`Away`]: the default is the
    // system browser, and a harness provides its own.
    let away =
        use_hook(|| dioxus_core::try_consume_context::<Away>().unwrap_or_else(Away::to_the_system));
    // And what the window itself can be asked to do. See [`Frame`]: the shell
    // answers these against winit, and a harness writes them down.
    let frame =
        use_hook(|| dioxus_core::try_consume_context::<Frame>().unwrap_or_else(Frame::unanswered));
    // And what hands the document to something that prints. See [`Printer`]:
    // the default is the platform's own program, and a harness writes the
    // path down instead of opening one.
    let printer = use_hook(|| {
        dioxus_core::try_consume_context::<Printer>().unwrap_or_else(Printer::to_the_system)
    });
    // …and what shows the document where it lives. See [`Reveal`]: the default
    // asks the platform's file manager, and a harness writes the path down.
    let reveal = use_hook(|| {
        dioxus_core::try_consume_context::<Reveal>().unwrap_or_else(Reveal::to_the_system)
    });
    // And where a copied passage goes. See [`Clip`]: the default is the
    // system's clipboard through the shell provider Blitz hands every window,
    // and a harness provides its own so that `cargo test` does not empty
    // anybody's.
    let clip = use_hook(|| {
        dioxus_core::try_consume_context::<Clip>().unwrap_or_else(|| {
            Clip::to_the_system(dioxus_core::try_consume_context::<
                Arc<dyn blitz_traits::shell::ShellProvider>,
            >())
        })
    });
    // And which document the reader chooses when they are asked for one. See
    // [`Pick`]: the default is the system's picker through the same shell
    // provider, and a harness answers with a path of its own.
    let pick = use_hook(|| {
        dioxus_core::try_consume_context::<Pick>().unwrap_or_else(|| {
            Pick::from_the_system(
                dioxus_core::try_consume_context::<Arc<dyn blitz_traits::shell::ShellProvider>>(),
                dioxus_core::try_consume_context::<crate::emit::Post>().unwrap_or_default(),
            )
        })
    });

    // The window, for the switch in the settings menu — the keyboard's own
    // copy is moved into `on_key` below, and asking the window to go full
    // screen is the same `Ask` either way.
    let full_screen_frame = frame.clone();
    let full_screen_label_frame = frame.clone();

    let resize_from_window = {
        let screen = screen.clone();
        move |mut viewer: Signal<Viewer>| {
            let (width, height, _scale) = screen.get();
            viewer.write().fit_window(width, height);
        }
    };

    // Every key the reader can press is an *action*, and a chord is only a
    // way of asking for one. The event is turned into a chord and the chord is
    // looked up in `keymap.rs`, over `keys.toml`. What is left here is working
    // out what was asked for and doing it.
    let on_key = {
        let frame = frame.clone();
        let clip = clip.clone();
        let pick = pick.clone();
        let printer = printer.clone();
        move |event: KeyboardEvent| {
            // **Any key ends the stationary scroll**, the way any press
            // does — and the key itself still lands, because a reader who
            // has reached for ⌘F has finished with the anchor and not with
            // the keystroke.
            if viewer.read().scrolling_still() {
                viewer.write().stop_still();
            }
            // A half-pressed sequence goes stale as the app's did, after
            // 1200ms: read at the next key rather than by a timer, since
            // there is no `setTimeout` here. Without it a `g` pressed by
            // accident waited for ever, and the next `g`, ten minutes of
            // trackpad later, went to page one.
            if viewer.read().pending_at.elapsed() > SEQUENCE_LASTS {
                viewer.write().pending.clear();
            }
            let (press, screen) = {
                let held = viewer.read();
                (
                    held.keymap
                        .press(&event.key(), event.code(), event.modifiers(), &held.pending),
                    held.layout.viewport.height,
                )
            };
            match press {
                // `g`, on its way to `g g`. A sequence half pressed and then
                // abandoned is dropped by the next chord that continues
                // nothing, or by going stale — see above.
                Press::Wait(prefix) => {
                    let mut held = viewer.write();
                    held.pending = prefix;
                    held.pending_at = std::time::Instant::now();
                }
                Press::Nothing => viewer.write().pending.clear(),
                // **The one action that asks which key was pressed.** ⌘1
                // through ⌘9 are nine chords on one action — see
                // `keymap.rs` — so the digit is read off the event here
                // rather than being nine arms of `perform`.
                Press::Act(Action::GoToTab) => {
                    viewer.write().pending.clear();
                    // The physical key first: on AZERTY ⌘1 types "&", which
                    // the chord lookup matched by its key and this did not.
                    let code = event.code().to_string();
                    if let Some(at) = code
                        .strip_prefix("Digit")
                        .and_then(|digit| digit.parse::<u32>().ok())
                        .or_else(|| event.key().to_string().chars().next()?.to_digit(10))
                        .filter(|digit| *digit > 0)
                    {
                        frame.ask(Ask::SelectTab(at as usize));
                    }
                }
                Press::Act(action) => {
                    viewer.write().pending.clear();
                    // **An action about a document does nothing when there is
                    // none.** Without this, `j` on the start screen scrolls a
                    // layout of no pages, ⌘F puts up a find bar over nothing,
                    // and ⌘⇧H offers to highlight a selection that cannot
                    // exist.
                    if crate::keymap::needs_document(action) && viewer.read().empty() {
                        return;
                    }
                    // **Nor through a window over the reader.** Settings, a
                    // note, the Sign window, the colours, the details, the
                    // password prompt and "Delete this theme?" are
                    // what the reader is looking at; Space and `j` scrolled
                    // the document behind Settings, `t` changed its theme and
                    // ⌘F put a find bar under the scrim that took the typing.
                    let windowed = {
                        let held = viewer.read();
                        held.pane.is_some()
                            || held.note_open.is_some()
                            || held.signing.is_some()
                            || held.colours_open
                            || held.details_open
                            || held.locked.is_some()
                            || held.deleting_theme.is_some()
                    };
                    if windowed && !answers_over_a_window(action) {
                        return;
                    }
                    perform(viewer, action, screen, &frame, &clip, &pick, &printer);
                }
            }
        }
    };

    // What drives a scan: a task per search, stopped by its token going stale.
    // There is no timer and no `setTimeout` — a slice yields to the event
    // loop, is woken by dioxus's own scheduler, and comes back on the next
    // turn. See `Breathe` at the bottom of this file.
    let scan = move |token: Option<u64>| {
        let Some(token) = token else { return };
        let mut viewer = viewer;
        spawn(async move {
            while viewer.write().scan_slice(token) {
                Breathe::once().await;
            }
        });
    };

    // Two files the reader is looking at are not the reader's to change: the
    // themes, and the document. Both are watched by the app's own `watch.rs`,
    // mounted here, and both arrive as news in a mailbox.
    //
    // **The pointer, and what keeps it on the screen.** Both are declared
    // before the mailbox below, because the timer that takes it away is read
    // out of that mailbox and a hook cannot be declared inside another hook's
    // initialiser. See [`Pointer`] and [`Resting`].
    let pointer = use_hook(|| {
        dioxus_core::try_consume_context::<Pointer>().unwrap_or_else(|| {
            Pointer::to_the_window(dioxus_core::try_consume_context::<
                Arc<dyn winit::window::Window>,
            >())
        })
    });
    let resting = use_hook(|| Rc::new(Resting::default()));

    // **The task is the whole of the wiring on this side**, and it is a real
    // wait: the watcher thread wakes it, the wake marks the task ready, and
    // dioxus's waker takes it to the window. Nothing polls, and in the harness
    // the same wake makes the next `pump()` run it.
    let (watching, notifying) = use_hook(|| {
        // This window's mailbox, joined to the process's switchboard under
        // this window's name. Both come from whoever made the window; a
        // harness provides them and a window with neither watches nothing.
        let post = viewer.read().post.clone();
        let exchange = dioxus_core::try_consume_context::<crate::emit::Exchange>();
        if let Some(exchange) = exchange.as_ref() {
            exchange.join(&config.window, post.clone());
        }
        // **One watcher for the process, not one per window.** It follows one
        // themes directory and a document per window, which is the shape
        // `watch.rs` has: `follow` counts what wants a directory rather than
        // unwatching it with the document that named it, because two papers
        // recompiled in one folder is the ordinary case.
        let shared = dioxus_core::try_consume_context::<Arc<crate::watch::Watching>>();
        let held = match (shared, config.watch) {
            (Some(watching), _) => {
                let path = viewer.read().document.path().to_string();
                if !path.is_empty() {
                    watching.document(&config.window, Some(&path));
                }
                Some(watching)
            }
            // Nobody made one, and this window wants one: the harness's own
            // case, and it is a watcher of one window's own.
            (None, true) => {
                let held = viewer.read();
                let (themes, path) = (
                    held.store.themes_dir().to_path_buf(),
                    held.document.path().to_string(),
                );
                drop(held);
                let exchange = exchange.clone().unwrap_or_else(|| {
                    let exchange = crate::emit::Exchange::new();
                    exchange.join(&config.window, post.clone());
                    exchange
                });
                let watching = Arc::new(crate::watch::start(exchange, themes));
                if !path.is_empty() {
                    watching.document(&config.window, Some(&path));
                }
                viewer.write_unchecked().watching = Some(Arc::clone(&watching));
                Some(watching)
            }
            (None, false) => None,
        };
        let listening = post.clone();
        let pointing = pointer.clone();
        let resting = resting.clone();
        let sizing = screen.clone();
        let watching_appearance = appearance.clone();
        let opening = frame.clone();
        spawn(async move {
            loop {
                let news = listening.next().await;
                match news.event.as_str() {
                    // Four seconds after something was said, said by a
                    // thread of its own. It clears the line only if the line
                    // still carries what it was started for.
                    "notice-timeout" => {
                        if let Payload::Text(said) = &news.payload {
                            if viewer.read().notice == *said {
                                viewer.write().notice.clear();
                            }
                        }
                    }
                    // A second after the reader stopped scrolling, and only
                    // if nothing has scrolled since — see `Viewer::flash_pill`.
                    //
                    // Every timer but the last is stale, and a `write` is a
                    // render whether it changes anything or not: asked under
                    // `read` first, a scroll's hundred timers cost nothing.
                    "pill-timeout" => {
                        if let Payload::Token(token) = news.payload {
                            if viewer.read().pill_token == token {
                                viewer.write().unflash_pill(token);
                            }
                        }
                    }
                    // And a few seconds after it, the bar goes the same way.
                    "bar-timeout" => {
                        if let Payload::Token(token) = news.payload {
                            if viewer.read().bar_token == token {
                                viewer.write().unflash_bar(token);
                            }
                        }
                    }
                    // The pointer has been left alone for a while. It is
                    // asked for once per rest rather than once per move of
                    // the mouse — a timer armed by every move would be a
                    // hundred a second, each outliving what armed it — so
                    // what fires here may be early, and an early one arms
                    // the remainder rather than hiding anything.
                    "cursor-timeout" => {
                        resting.waiting.set(false);
                        if !viewer.read().hides_cursor() {
                            continue;
                        }
                        let Some(moved) = resting.moved.get() else {
                            continue;
                        };
                        let rests = viewer.read().cursor_rests();
                        let still_for = moved.elapsed();
                        if still_for >= rests {
                            resting.away.set(true);
                            pointing.show(false);
                        } else {
                            resting.waiting.set(true);
                            crate::emit::after(
                                rests - still_for,
                                listening.clone(),
                                crate::emit::News {
                                    event: "cursor-timeout".into(),
                                    target: None,
                                    payload: Payload::Nothing,
                                },
                            );
                        }
                    }
                    // One step of the stationary scroll, and the next one
                    // armed — a clock the gesture starts and stops rather
                    // than one that runs. See the block on it in `Viewer`.
                    //
                    // The speed is read before anything is written, because
                    // the pointer spends most of the gesture inside the dead
                    // zone: writing the viewer to move it nothing would be a
                    // render a frame for as long as the anchor is up.
                    "still-tick" => {
                        let Payload::Token(token) = news.payload else {
                            continue;
                        };
                        let Some((across, down)) = viewer.read().still_speed(token) else {
                            continue;
                        };
                        if (across, down) != (0.0, 0.0) {
                            viewer.write().drift(across, down);
                        }
                        crate::emit::after(
                            STILL_TICK,
                            listening.clone(),
                            crate::emit::News {
                                event: "still-tick".into(),
                                target: None,
                                payload: Payload::Token(token),
                            },
                        );
                    }
                    // The fingers stopped moving. See [`Viewer::settle_zoom`].
                    "zoom-settled" => {
                        if let Payload::Token(token) = news.payload {
                            if viewer.read().zoom_token == token {
                                viewer.write().settle_zoom(token);
                            }
                        }
                    }
                    "crop-measured" => {
                        if let Payload::Measured(crop, token) = news.payload {
                            viewer.write().measured(crop, token);
                        }
                    }
                    "themes-changed" => {
                        if let Payload::Themes(themes) = news.payload {
                            viewer.write().themes_changed(themes);
                        }
                    }
                    "document-changed" => {
                        let Payload::Text(path) = news.payload else {
                            continue;
                        };
                        let restarted = viewer.write().document_changed(&path);
                        scan(restarted);
                    }
                    // A write of this window's own, back from its thread.
                    "document-written" => {
                        let restarted = viewer.write().landed();
                        scan(restarted);
                    }
                    // A document is over the window, and whether it is one
                    // this reader would open. Both answers are worth having:
                    // a hint that says "drop to open" over a folder is a
                    // promise nothing keeps.
                    "drag-over" => {
                        // A shell that said nothing about it means yes, which
                        // is what the hint promised before there was an answer.
                        let takeable = match news.payload {
                            Payload::Takeable(takeable) => takeable,
                            _ => true,
                        };
                        viewer.write().dragging = Some(takeable);
                    }
                    "drag-left" => viewer.write().dragging = None,
                    // Let go on something that is not a document. The app's
                    // own sentence, said on the app's own line.
                    "drag-refused" => {
                        let mut held = viewer.write();
                        held.dragging = None;
                        held.notice = "That is not a PDF.".into();
                    }
                    // A document handed to this window by the process: a
                    // second launch, "Open with", a double-click. It arrives
                    // here rather than at a new window because this one is
                    // showing nothing — see `Desk::hand_over` — and the
                    // bookkeeping afterwards is ⌘O's, because this is ⌘O with
                    // somebody else choosing the file.
                    // Handed to this window because the desk had it down as
                    // empty. Trusted from the window, not the bookkeeping: if
                    // something got here first — or a password is being asked
                    // for — the document goes beside it, never over it.
                    "handed-over" | "open-document" => {
                        let Payload::Text(path) = news.payload else {
                            continue;
                        };
                        let full = !viewer.read().empty() || viewer.read().locked.is_some();
                        if news.event == "handed-over" && full {
                            if !path.is_empty() {
                                opening.ask(Ask::SendOn(path));
                            }
                            continue;
                        }
                        viewer.write().dragging = None;
                        if !path.is_empty() && viewer.write().open_here(&path) {
                            let title = viewer.read().store.title().to_string();
                            opening.ask(Ask::Showing { path, title });
                        }
                    }
                    // The same document, through the other door: the picker
                    // was opened by "Open document in new window…", so what
                    // it chose goes beside this window rather than into it.
                    // See `Pick` — a picker cannot answer where it was asked,
                    // so which door it was is carried in the event's name.
                    "open-document-beside" => {
                        let Payload::Text(path) = news.payload else {
                            continue;
                        };
                        if !path.is_empty() {
                            opening.ask(Ask::NewWindowOn(path));
                        }
                    }
                    // A theme file chosen under Appearance, answered here for
                    // `Pick`'s reason. See `theme_file_dialog` in `prefs.rs`.
                    "import-theme" => {
                        if let Payload::Text(path) = news.payload {
                            viewer.write().import_theme(&path);
                        }
                    }
                    "export-theme" => {
                        if let Payload::Text(path) = news.payload {
                            viewer.write().export_theme(&path);
                        }
                    }
                    // The window changed size, which nothing else in this
                    // process will tell the layout — Blitz resizes its own
                    // viewport and asks for a redraw, and a redraw of a
                    // layout computed for the old window is the old layout.
                    // See `Shell::on_resized`, which is the other half.
                    "window-resized" => {
                        let (width, height, _scale) = sizing.get();
                        viewer.write().fit_window(width, height);
                        // The window is what is in full screen. See
                        // [`Viewer::window_full`].
                        if let Payload::Full(full) = news.payload {
                            viewer.write().window_full(full);
                        }
                    }
                    // The machine went light or dark while the reader was
                    // reading. Nothing carries the answer — the event says
                    // only that there is a new one, exactly as a resize does,
                    // and it is asked of the window. See `Shell::on_theme`.
                    // Two fingers, moving apart or together. macOS gives the
                    // change since the last event as a fraction, so the
                    // gesture's whole scale is the product of them and each
                    // one is a proportion to zoom by. See `Viewer::zoom_by`.
                    "pinched" => {
                        if let Payload::Amount(delta) = news.payload {
                            viewer.write().pinch_by(1.0 + delta);
                        }
                    }
                    "pinch-ended" => viewer.write().end_pinch(),
                    "appearance-changed" => {
                        viewer.write().follow_system(watching_appearance.get());
                    }
                    // Nothing else is emitted, and an unknown event is a
                    // version of this crate that has not caught up rather
                    // than something to report.
                    _ => {}
                }
            }
        });
        // Held for the life of the reader. Dropping it stops nothing — see
        // `Config::watch` — but this is where it will be asked to. The
        // mailbox comes back out with it because the notice timer below
        // wants to post into it, and a hook cannot be declared inside
        // another hook's initialiser.
        (held, post)
    });
    // Read so that the handle is plainly alive rather than plainly unused.
    let _ = watching.is_some();

    // **The pointer moved, so it is on the screen and the clock starts over.**
    // One timer at a time: a fresh one per move of the mouse would be a
    // hundred a second, all of them outliving the move that armed them, so
    // the one that is out simply arms the remainder when it fires early. See
    // the "cursor-timeout" arm above.
    let stir_pointer = {
        let notifying = notifying.clone();
        let pointer = pointer.clone();
        let resting = resting.clone();
        move || {
            resting.moved.set(Some(std::time::Instant::now()));
            if resting.away.replace(false) {
                pointer.show(true);
            }
            if !viewer.read().hides_cursor() || resting.waiting.replace(true) {
                return;
            }
            crate::emit::after(
                viewer.read().cursor_rests(),
                notifying.clone(),
                crate::emit::News {
                    event: "cursor-timeout".into(),
                    target: None,
                    payload: Payload::Nothing,
                },
            );
        }
    };

    // What starts the stationary scroll's clock. The gesture is begun from a
    // handler in the tree below and stepped from the mailbox above, and this
    // is the one line they share: arm the first tick, and the arm above arms
    // every one after it.
    let start_still = {
        let notifying = notifying.clone();
        move |token: Option<u64>| {
            let Some(token) = token else { return };
            crate::emit::after(
                STILL_TICK,
                notifying.clone(),
                crate::emit::News {
                    event: "still-tick".into(),
                    target: None,
                    payload: Payload::Token(token),
                },
            );
        }
    };

    // **What an effect remembers between runs lives in a hook, never in the
    // closure.**
    //
    // The three effects below each compare what they see against what they saw
    // last time, and each of them held that in a `let mut` captured by `move`.
    // That is wrong, and it is not visibly wrong: `use_effect` hands its
    // closure to `use_callback`, which *replaces the stored closure on every
    // render* — so the capture is a fresh one every time and the comparison is
    // always against the initial value. The notice's timer was therefore
    // re-armed on every render, and the pill's effect, which also *writes* the
    // viewer, dirtied the component it had just been run by: a render, a
    // layout and a full paint per frame for as long as the window was open,
    // 100% of a core with nobody touching the app. `tests/cost.rs` asserts the
    // reader stops rendering, which is the only thing that would have said so.
    //
    // **The notice puts itself away after four seconds.**
    //
    // A thread rather than a timer, because nothing in this reader is async
    // except the mailbox: it sleeps and posts through the same door `watch.rs`
    // uses. The "notice-timeout" arm above throws the message away if the line
    // is no longer showing the message this timer was started for, so a second
    // notice does not vanish with the first one's four seconds. A message said
    // twice keeps the first timer, which is the one case this is imprecise
    // about.
    {
        let notifying = notifying.clone();
        let last = use_hook(|| Rc::new(RefCell::new(String::new())));
        use_effect(move || {
            let said = viewer.read().notice.clone();
            if said == *last.borrow() {
                return;
            }
            *last.borrow_mut() = said.clone();
            if said.is_empty() {
                return;
            }
            crate::emit::after(
                NOTICE_LASTS,
                notifying.clone(),
                crate::emit::News {
                    event: "notice-timeout".into(),
                    target: None,
                    payload: Payload::Text(said),
                },
            );
        });
    }

    // **And the pill puts itself away after a second**, the same way and for
    // the same reason: `flashPill` in the app is a `setTimeout` of 1100ms,
    // restarted by every scroll. The effect watches the scroll offset, so a
    // wheel, a key, a jump and a link all flash it without one of them having
    // to remember to.
    {
        let notifying = notifying.clone();
        let last = use_hook(|| Rc::new(Cell::new((f64::NAN, u64::MAX))));
        use_effect(move || {
            let now = {
                let held = viewer.read();
                (held.scroll_top, held.scroll_gesture())
            };
            if now == last.get() {
                return;
            }
            last.set(now);
            // The bar first, and with no condition on it: the pill is a
            // setting and this is the only thing on screen saying how far
            // into the book the reader is.
            let token = viewer.write().flash_bar();
            crate::emit::after(
                BAR_LASTS,
                notifying.clone(),
                crate::emit::News {
                    event: "bar-timeout".into(),
                    target: None,
                    payload: Payload::Token(token),
                },
            );
            // A zoom or a resize moves the scroll too, and is not a scroll.
            if viewer.read().relaid() {
                return;
            }
            let Some(token) = viewer.write().flash_pill() else {
                return;
            };
            crate::emit::after(
                PILL_LASTS,
                notifying.clone(),
                crate::emit::News {
                    event: "pill-timeout".into(),
                    target: None,
                    payload: Payload::Token(token),
                },
            );
        });
    }

    // **And a zoom gesture ends when the fingers stop**: a pinch is a stream
    // with no end in it, so what ends it is the gap after the last event.
    // Until then every page is stretched rather than redrawn — see
    // [`crate::page::Chosen::holding`] — and this puts them back sharp.
    {
        let notifying = notifying.clone();
        let last = use_hook(|| Rc::new(Cell::new(0u64)));
        use_effect(move || {
            let Some(token) = viewer.read().zoom_gesture() else {
                return;
            };
            if token == last.get() {
                return;
            }
            last.set(token);
            crate::emit::after(
                ZOOM_SETTLES,
                notifying.clone(),
                crate::emit::News {
                    event: "zoom-settled".into(),
                    target: None,
                    payload: Payload::Token(token),
                },
            );
        });
    }

    let held = viewer.read();
    let scroll_top = held.scroll_top;
    // How far across the document sits, which is nothing unless it is wider
    // than the window. See [`Viewer::across`].
    let scroll_left = held.scroll_left();
    let wearing = held.palette();
    let theme_name = held.theme_name();
    // The colours the pages wear, in every page's key — the colours and not
    // the theme's name, because the theme editor's preview, "Recolour
    // pictures too" and a theme file edited on disk all change the one
    // without the other. See `page.rs`.
    let worn = chosen.get().key();
    // Which document is being drawn — in every page's key, so that another
    // document replaces the nodes and the textures with them. A recompile
    // does not: a new draft is drawn in place. See `Viewer::opened`.
    let opened = held.opened;
    let mounted = held.layout.mounted(held.scroll_top);
    let content_width = held.layout.content_width();
    let content_height = held.layout.content_height();
    // What follows "of" in the toolbar, which is not always the number of
    // pages. See [`Viewer::pages_text`].
    let pages_text = held.pages_text();
    let notice = held.notice.clone();
    // Read once for the whole render rather than per page: six strings out of
    // the settings table, and the great majority of renders draw no popover
    // at all.
    let markup_colours = held.markup_colors();
    // Where the stationary scroll is anchored, and the strip above the
    // document that its point has to be measured against. `None` almost
    // always: see the block on it in `Viewer`.
    let still_anchor = held.still_anchor();
    // Built only when there is one to draw: this component renders on every
    // frame of a scroll, and the marker is up for a few seconds a session.
    let mark = still_anchor
        .map(|_| {
            still_mark(
                &crate::palette::hex(wearing.surface()),
                &crate::palette::hex(wearing.line()),
                &crate::palette::hex(wearing.muted()),
            )
        })
        .unwrap_or_default();
    let chrome = held.chrome();
    let sidebar_open = held.sidebar_open;
    let find_open = held.find_open;
    let presenting = held.presenting;
    // The bar on screen, which the page field can borrow with the setting
    // still off — see [`Viewer::toolbar_up`]. `toolbar_set` is the switch in
    // the Settings menu, which has to keep saying what the reader chose.
    let toolbar_on = held.toolbar_up() && !held.presenting;
    let toolbar_set = held.toolbar;
    // The pill, and what it says. See [`Viewer::flash_pill`].
    // The note the reader has opened, and what its page is called — the label
    // rather than the position, which is what `showNote` says too.
    let worn_built_in = held.store.theme().built_in;
    let note_open = held.note_open.clone();
    let colours_open = held.colours_open;
    let locked = held.locked.clone();
    // The bullets the password field shows, counted here because a format
    // hole in `rsx!` cannot hold a string literal of its own. See the field.
    let locked_shown = locked
        .as_ref()
        .map(|locked| "\u{2022}".repeat(locked.typed.chars().count()))
        .unwrap_or_default();
    let details_open = held.details_open;
    let details_rows = if details_open {
        held.details()
    } else {
        Vec::new()
    };
    // The Sign window, with the three lists it shows — read when it opened,
    // not here, because one of them opens the file. See `Signing::kept`.
    let signing = held.signing.clone();
    let (kept, signed_here, seals) = signing
        .as_ref()
        .map(|signing| {
            (
                signing.kept.clone(),
                signing.signed_here.clone(),
                signing.seals.clone(),
            )
        })
        .unwrap_or_default();
    // Whether a signature is looking for somewhere to go, which changes what
    // a click on a page means and what the pointer looks like over one.
    let placing = held.placing.is_some();
    // Asked before the list is consumed by the rows below it, which is the
    // only reason it is a variable.
    let nothing_kept = kept.is_empty();
    let note_page = note_open
        .as_ref()
        .map(|(page, _)| held.label(*page))
        .unwrap_or_default();
    let peeking = held.peeking();
    let pill_up = held.pill_shown();
    let pill_text = held.pill_text();
    // The scrollbar's thumb, and where the page count goes beside it. `None`
    // is a document that fits, which has neither. See [`Viewer::bar_thumb`].
    let thumb = held.bar_thumb();
    let on_bar = held.dragging_bar();
    // …and whether there is one on screen right now. See
    // [`Viewer::flash_bar`]: the bar comes up with the document's movement
    // and goes a few seconds after it stops.
    let bar_up = held.bar_shown();
    // In the root's space rather than the viewer's, because that is what the
    // pill is positioned against: the chrome above it, then the middle of the
    // thumb, less half a pill.
    let pill_y = thumb.map(|(top, height)| held.chrome() + top + height / 2.0 - 15.0);
    // Whether this window has a document in it, which decides two things: what
    // the toolbar carries, and whether the body is the document or the start
    // screen. See [`Viewer::empty`].
    let empty = held.empty();
    // A document over the window, if there is one. See [`Viewer::dragging`].
    let dragging = held.dragging;
    // What the button the Document menu hangs off is called: the document's
    // name, or the app's own "Open…" when there is none.
    // The shelf, read once for this render. It is a file on the disk, so it is
    // read when the Open menu is open and not otherwise.
    let recents = if held.menu == Some(Menu::Open) {
        held.recents()
    } else {
        Vec::new()
    };
    // The title button exists only where there is a document, so this is the
    // document's name and nothing else. Open… is a button of its own, there
    // whether or not anything is open, which is what the app does.
    let shelf_name = held.store.title().to_string();
    // **Whether the name has run out of box**, which decides the fade over its
    // last twenty-four pixels. Blitz has no `text-overflow: ellipsis`, so the
    // fade stands in for one, and drawn unconditionally it faded every name
    // that fits. Thirty-four characters is what the box holds — the app's own
    // `34ch`, counted rather than measured. Erring long is the safe direction:
    // a name just past the cap is cut without a fade, which everyone has seen
    // from a narrow column.
    let name_clipped = shelf_name.chars().count() > 34;
    let find_query = held.find_query.clone();
    let find_asked = held.find_asked;
    let find_count = held.find_count();
    let find_options = held.search.options();
    let highlight_all = held.highlight_all;
    let marked = held.store.is_marked(held.page());
    // What the menus need, read once with the rest of it. The theme list is
    // names and nothing else — a swatch would have to go through `parseColor`
    // to be honest about what the renderer can read, which is the app's own
    // rule and a thing to build when the theme editor is.
    let menu = held.menu;
    // Each theme's name, the two colours its swatch is drawn in, and whether
    // it is one the reader wrote. Read through `palette` rather than handed
    // raw to CSS: a swatch that shows a colour the renderer cannot read is
    // the one place in the app meant to show what you are about to get
    // lying about it — the app's own finding, in `ui.swatch`.
    let theme_rows: Vec<(String, String, String, bool)> = held
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
        .collect();
    let dark_now = held.store.dark_now();
    let following = held.store.flag("follow_system_theme");
    let key_dark = held.chord_for(Action::Dark);
    let theme_index = held.store.theme_index();
    let fit = held.layout.fit;
    let spread = held.layout.spread;
    let key_open = held.chord_for(Action::Open);
    let key_new_window = held.chord_for(Action::NewWindow);
    let key_mark = held.chord_for(Action::Mark);
    let key_print = held.chord_for(Action::Print);
    let key_fit_width = held.chord_for(Action::FitWidth);
    let key_fit_page = held.chord_for(Action::FitPage);
    let key_actual = held.chord_for(Action::ActualSize);
    // The settings menu names three more keys, for the same reason every
    // other menu item here names one: read off the keymap, so a rebound key
    // is the key the menu shows.
    let key_toolbar = held.chord_for(Action::Toolbar);
    let key_fullscreen = held.chord_for(Action::Fullscreen);
    let key_settings = held.chord_for(Action::Settings);
    let key_rotate_left = held.chord_for(Action::RotateLeft);
    let key_rotate_right = held.chord_for(Action::RotateRight);
    let full_screen = held.full_screen;
    let scroll_mode = held.layout.mode;
    let recolor_images = held.recolor_images();
    let page_pill = held.page_pill();
    let numbering_printed = held.numbering_printed();
    let own_numbering = held.has_own_numbering();
    let (numbering_as_printed, numbering_by_position) = held.numbering_choices();
    let page_field = held.page_field();
    // How wide the page box is: padding, border and the number in it, with a
    // floor so page 1 of a pamphlet is not a slot. Blitz cannot centre an
    // input's text, so the box is made to fit rather than the text made to sit
    // in the middle of it.
    //
    // **The floor is the app's own 44px**, not the smallest box a digit fits
    // in: a narrower floor makes page 1 a slot half the size of the count
    // beside it. Growing past 44 only happens at four digits.
    let page_box = (14.0 + 9.1 * page_field.chars().count() as f64).max(44.0);
    // **And where the number sits inside that box, which the box's width alone
    // does not settle.** `.page-field` says `text-align: center` and Blitz
    // ignores it on an input — `create_text_editor` calls
    // `editor.set_width(None)`, so there is no box to align within and the run
    // starts at the leading edge. That is invisible while the box is the width
    // of its contents and plain the moment it is not: the floor holds page 1 in
    // a 44px box, so pressing the go-to-page key moved the number it had just
    // selected to the left wall. The slack the floor leaves is split in half
    // and paid as left padding, which is the one thing Blitz does honour — so a
    // one-digit field is centred and a four-digit one, which is already fitted,
    // is unchanged.
    let page_pad =
        6.0 + ((page_box - 14.0 - 9.1 * page_field.chars().count() as f64) / 2.0).max(0.0);
    // What an icon is drawn in, and why it is a string rather than a class.
    // An inline `<svg>` here is handed to usvg with no cascade behind it — see
    // [`Icon`] — so the shade a chip's label resolves to has to be passed down
    // beside it. Two of them, because a chip whose thing is in force is the
    // accent and every other chip is the quiet shade.
    let ink = crate::palette::hex(wearing.muted());
    let ink_on = crate::palette::hex(wearing.accent);
    // The third shade: what a tick that is *not* ticked is drawn in, and the
    // magnifier at the head of the find bar. `.find-option svg` is
    // `opacity: 0.28` in the app, which is the same thing said in the one way
    // an icon with no cascade behind it can be told.
    let faint = crate::palette::hex(wearing.faint());
    // And the fourth: the cross on Close, while the pointer is over it. An
    // icon's stroke is an attribute, never the cascade (see [`Icon`]), so
    // both crosses are drawn and `:hover` picks one — `.icon.hot` in
    // `styles.rs`. A flag set on `mouseenter` misses the button that appears
    // under a pointer that has not moved, which is where Close window lands.
    let danger = crate::palette::hex(wearing.negative());
    // And a signature, drawn in the Sign window as it will look on the page:
    // `sign::INK`, the navy that goes into the file, where the theme leaves
    // pages alone, and the theme's own ink where it recolours them — on a
    // dark theme's pad the navy could hardly be seen.
    let pen = if wearing.recolor {
        crate::palette::hex(wearing.text)
    } else {
        crate::sign::INK.to_string()
    };
    let arming = held.arming.clone();
    let typing_page = held.typing_page;
    // Whether the field is still showing all of its contents as selected. See
    // `.page-field.fresh` in `styles.rs`, which is what makes that visible.
    let page_fresh = held.page_fresh;
    // How wide the page box is: the padding, the border, and the number in it,
    // with a floor so that page 1 of a pamphlet is not a slot. See the comment on
    // `.pill` below — Blitz cannot centre an input's text, so the box is made
    // to fit rather than the text made to sit in the middle of it.
    let zoom = match held.layout.fit {
        Fit::Width => "Fit width".to_string(),
        Fit::Page => "Fit page".to_string(),
        Fit::Actual => format!("{:.0}%", held.layout.zoom * 100.0),
    };
    // What the zoom menu needs, read while the reader is held: the remembered
    // zoom, which is what the presets tick against, and what is actually on
    // screen, which is where the stepper starts. In a fit mode those are
    // different numbers — see [`Viewer::zoom_percent`].
    let zoom_now = held.layout.zoom * 100.0;
    // Whole percent, as the zoom notice says it: a pinch leaves fractions
    // nobody asked to read.
    let shown_percent = held.zoom_percent().round();
    // "Actual size" is a fit mode *and* a zoom of 1, so it is ticked only when
    // both are true — `showZoomMenu` asks the same two questions.
    let actual_100 = held.layout.fit == Fit::Actual && (zoom_now - 100.0).abs() < 0.5;
    // What a page is *drawn* at, against what it is *shown* at. The two are
    // the same number except under a zoom gesture, where the drawn size is
    // frozen at whatever it was when the fingers went down — see
    // [`Viewer::zoom_held_at`] and [`crate::page::Chosen::holding`].
    let held_at = held.zoom_held_at();
    let boxes: Vec<Placed> = mounted
        .iter()
        .filter_map(|&index| {
            held.layout.box_of(index).map(|page| Placed {
                index,
                top: page.top,
                left: page.left,
                width: page.width,
                height: page.height,
                drawn: (
                    (page.width * held_at).round(),
                    (page.height * held_at).round(),
                ),
                hits: held.highlights(index + 1),
                links: held.link_areas(index + 1),
                notes: held.note_areas(index + 1),
                selected: held.selected_areas(index + 1),
                swatches: held
                    .markup_at
                    .filter(|(page, _)| *page == index + 1)
                    .map(|(_, area)| area),
                mark: held
                    .mark_open
                    .as_ref()
                    .filter(|(page, ..)| *page == index + 1)
                    .map(|(_, area, key, colour)| (*area, key.clone(), colour.clone())),
            })
        })
        .collect();
    // **What each page paints into itself rather than draws over.** A link's
    // colour and a selected word's are the *page's*, baked into the bitmap as
    // `tintLinks` and `paintSelection` bake them in the app — see
    // [`crate::page::Ramped`]. The rectangles are the box's own here and
    // fractions of it by the time a widget reads them, because a texture is
    // drawn at the box's size times the density, held under a ceiling and
    // frozen mid-pinch, and a fraction survives all three.
    //
    // The whole map at once, so a page scrolled out of the mounting window
    // takes its entry with it.
    chosen.place(
        boxes
            .iter()
            .map(|placed| {
                let (across, down) = (placed.width.max(1.0), placed.height.max(1.0));
                let fractions = |rect: &Rect| {
                    [
                        (rect.left / across) as f32,
                        (rect.top / down) as f32,
                        ((rect.left + rect.width) / across) as f32,
                        ((rect.top + rect.height) / down) as f32,
                    ]
                };
                (
                    placed.index,
                    crate::page::Ramped {
                        links: placed
                            .links
                            .iter()
                            .map(|(rect, _)| fractions(rect))
                            .collect(),
                        selection: placed.selected.iter().map(fractions).collect(),
                    },
                )
            })
            .collect(),
    );
    // How every page is drawn, and the string that says so in a key. A turn
    // of 180° leaves a page exactly the shape it was, so the box's size
    // cannot stand in for this: the pixels differ and nothing else would say
    // so. See `page.rs` on why the key is what invalidates a texture.
    let view = held.layout.view();
    let view_key = match view.crop {
        Some(crop) => format!(
            "{}|{:.3},{:.3},{:.3},{:.3}",
            view.rotation, crop.x, crop.y, crop.width, crop.height
        ),
        None => format!("{}|whole", view.rotation),
    };
    let chosen = chosen.clone();
    drop(held);

    let variables = crate::styles::variables(&wearing);

    // **The card hangs off the Search chip**, the way every other panel in
    // this bar hangs off the button that opens it. Written once and placed
    // twice: inside the chip's own `.anchor` while the toolbar is up, and at
    // the window's edge when the toolbar is away and there is no chip left to
    // hang under.
    //
    // And at the window's edge, too, once the chips have lost their words —
    // the first `@media` step above `.chip` in `styles.rs`, whose 1110 this
    // is. Hung off a chip that has moved left of where its words kept it,
    // the card ran out of the window.
    let bar_tight = viewer.read().window_width <= 1110.0;
    let find_card = {
        // Cloned in rather than moved: the same three colours and the query
        // are read by the bar this closure builds and by the toolbar around
        // it, and a `move` closure would take them.
        let (ink, ink_on, faint) = (ink.clone(), ink_on.clone(), faint.clone());
        let (find_query, find_count) = (find_query.clone(), find_count.clone());
        let tick = |on: bool| if on { "boxChecked" } else { "box" };
        move |place: &str| {
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
                        "data-keyboard": if typing_page { None } else { Some("find") },
                        onmounted: move |event| {
                            let node = event.data();
                            let task = node.set_focus(true);
                            spawn(async move { let _ = task.await; });
                        },
                        oninput: move |event| {
                            let typed = event.value();
                            let token = viewer.write().find(&typed);
                            scan(token);
                        },
                        // Every key typed here also bubbles to the root, which
                        // turns keys into actions — so without this, typing
                        // "just" into the field scrolls the document four times.
                        // What the field lets past is a chord with a modifier on
                        // it, so ⌘+ still zooms while somebody is searching.
                        onkeydown: move |event| {
                            let key = event.key();
                            let modifiers = event.modifiers();
                            let plain = crate::keymap::plain(modifiers);
                            match key {
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
                                _ if plain => event.stop_propagation(),
                                // A chord with a modifier is not typing — and
                                // Blitz applies the keystroke to a focused field
                                // whatever is held down, so ⌘G stepped to the
                                // next match *and* put a "g" in the query. What
                                // the field keeps is what a text field owns.
                                // And stays there: ⌘A selected the field *and*
                                // the page, because the root heard it too.
                                Key::Character(ref typed)
                                    if matches!(typed.as_str(), "a" | "c" | "v" | "x" | "z") =>
                                {
                                    event.stop_propagation()
                                }
                                _ => event.prevent_default(),
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
                            scan(token);
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
                            scan(token);
                        },
                        Icon { name: tick(find_options.whole_words), stroke: if find_options.whole_words { ink_on.clone() } else { faint.clone() } }
                        "Whole words"
                    }
                }
                }
            }
        }
    };

    rsx! {
        // Never rewritten. See `styles.rs`: the theme is on the root as
        // variables, because a stylesheet that changes while something is
        // hovered takes Stylo down.
        style { {crate::styles::SHEET} }
        div {
            // Presenting is a class rather than a pile of conditions: the
            // chrome is gone from the DOM either way, and what is left for
            // CSS is the ground the document sits on.
            class: match (presenting, placing) {
                // A signature waiting for somewhere to go changes what a click
                // on the page means, so it changes what the pointer looks like
                // over one. Nothing else in this reader arms a click, and a
                // mode with nothing on screen saying so is a mode a reader
                // discovers by signing something they did not mean to.
                (true, _) => "root presenting",
                (false, true) => "root placing",
                (false, false) => "root",
            },
            style: "{variables}",
            tabindex: 0,
            onkeydown: on_key,
            // A key with nothing focused goes to the *root element*, which is
            // `<html>` — above anything a component can put a handler on. So
            // the reader's own root takes the focus as soon as it exists, and
            // every key arrives here.
            onmounted: move |event| {
                let node = event.data();
                // And kept, for everything that has to give the keyboard back
                // to it later. See [`RootFocus`].
                root_focus.set(Some(node.clone()));
                let task = node.set_focus(true);
                spawn(async move {
                    let _ = task.await;
                });
            },
            // And says so, for whoever owns the window: see [`KEYBOARD`] and
            // `give_keyboard_back`. A click takes the focus away from here
            // and a component cannot take it back.
            "data-keyboard": "reader",
            // The sidebar's resize handle starts a drag but cannot track it:
            // dragging widens the panel, which moves the pointer out from
            // under the element it started on. These sit on the root instead,
            // which is the one ancestor spanning the whole window.
            // `drag_sidebar` is a no-op with nothing to drag, so an ordinary
            // move is one `read` — `write` marks the signal dirty whether or
            // not anything changed, and every move reaches here.
            // A press anywhere a menu is not puts the menu away. The menu and
            // the buttons it hangs off stop propagation rather than being
            // tested for here: a press that closed the menu on its way down
            // would leave the button's own click to open it straight back up.
            // And a press anywhere the page field is not puts *that* away,
            // abandoning what was typed. The field stops propagation itself,
            // for the reason the menu buttons do.
            // And a press past the find bar puts *that* away: anything below
            // the toolbar is somewhere else, and going there is done with the
            // search — the results panel it borrowed goes with it, or every
            // search would end with a panel to be closed by hand.
            //
            // The app spells its exceptions as a selector (`FIND_KEEPS_OPEN`)
            // and a handler here has no `closest` to ask with. So the top
            // strip is asked for by *height*, which is what that half of the
            // selector means, and the rest stop the press themselves. The
            // results panel is the one worth naming: it is this search seen
            // larger, so picking a line out of it must not close what found
            // it — `sidebar.rs` stops the press over it and its tab.
            onmousedown: move |event| {
                // **Any press ends the stationary scroll**, and ends it
                // instead of doing whatever else it would have done: the
                // press that puts the anchor away is spent on putting it
                // away. The page's own handler has already declined to begin
                // a sweep for the same reason.
                if viewer.read().scrolling_still() {
                    viewer.write().stop_still();
                    return;
                }
                let (menu, typing, find, strip) = {
                    let held = viewer.read();
                    (
                        held.menu.is_some(),
                        held.typing_page,
                        held.find_open,
                        held.chrome(),
                    )
                };
                if menu {
                    viewer.write().close_menu();
                }
                if typing {
                    viewer.write().cancel_page();
                }
                // `chrome()` is nought with the bar away or while presenting,
                // which is the right answer both times: the find card comes up
                // to meet the window's edge and there is no strip to be inside
                // of, so every press that reaches the root is past it. That is
                // `#shell[data-toolbar="hidden"]` in the app saying the same.
                if find && event.client_coordinates().y > strip {
                    let mut held = viewer.write();
                    held.close_find();
                    // The page's own handler has already begun a sweep at
                    // this press, measured against a layout the panel's
                    // closing has just changed: carried on, it selected a
                    // stray patch near the pointer. The press was spent on
                    // the bar.
                    held.clear_selection();
                }
            },
            onmousemove: move |event| {
                // Before anything else, because it is about the pointer
                // rather than about what the pointer is doing: every move
                // puts it back on the screen and starts its rest over.
                stir_pointer();
                // The stationary scroll, which is steered by where the
                // pointer *is* rather than by anything it does — and only
                // the root hears a pointer that has left the page it
                // anchored on. A no-op outside the gesture.
                if viewer.read().scrolling_still() {
                    let at = event.client_coordinates();
                    viewer.write().steer_still((at.x, at.y));
                    return;
                }
                let (resizing, sweeping, drawing, on_bar) = {
                    let held = viewer.read();
                    (
                        held.resize_from.is_some(),
                        held.sweeping(),
                        held.signing.as_ref().is_some_and(|pad| pad.drawing),
                        held.dragging_bar(),
                    )
                };
                // **A drag with no button down is a release nobody heard.**
                // Let go past the edge of the window, the button comes up
                // over nothing, and Blitz gives that to `<html>`, above this
                // handler — so the thumb, the sweep or the pen followed the
                // pointer for ever after.
                if (resizing || sweeping || drawing || on_bar)
                    && event.held_buttons().is_empty()
                {
                    viewer.write().let_go();
                    return;
                }
                // The scrollbar, for `drag_sidebar`'s reason: a drag that
                // began on the thumb has to go on being a drag when the
                // pointer leaves it, and only the root hears about that.
                if on_bar {
                    viewer.write().drag_bar(event.client_coordinates().y);
                    return;
                }
                // **A hand signing a name leaves the pad**, which is why this
                // is here and not on the pad. The point is taken in the pad's
                // own space, and a handler cannot ask an element where it is,
                // so it is remembered at the press. See [`Viewer::draw_from`].
                if drawing {
                    let at = event.client_coordinates();
                    viewer.write().draw_on_pad((at.x, at.y));
                    return;
                }
                if resizing {
                    let x = event.client_coordinates().x;
                    viewer.write().drag_sidebar(x);
                } else if sweeping {
                    // The pointer has left the page it went down on most of
                    // the time — a sweep of two lines crosses the gap between
                    // pages, and one down the margin is never over a page at
                    // all. So this works in the *content's* coordinates, which
                    // is what the origin recorded at the press is for.
                    let at = event.client_coordinates();
                    viewer.write().sweep_to((at.x, at.y));
                } else {
                    // The top edge of the window, reached for. See
                    // [`Viewer::reach_for_toolbar`] — it does nothing at all
                    // while the toolbar is up, which is almost always.
                    let y = event.client_coordinates().y;
                    if viewer.read().peek_changes(y) {
                        viewer.write().reach_for_toolbar(y);
                    }
                }
            },
            onmouseup: move |_| viewer.write().let_go(),
            if toolbar_on {
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
                        // Contents is `opens(…)` in `main.ts` for the same
                        // reason the five menus are — see `show_menu`. The
                        // *keyboard* action is not, there or here: a shortcut
                        // asked for the panel and said nothing about the
                        // search.
                        onclick: move |_| {
                            viewer.write().close_find();
                            viewer.write().toggle_sidebar();
                        },
                        Icon { name: "contents", stroke: if sidebar_open { ink_on.clone() } else { ink.clone() } }
                        span { class: "chip-label", "Contents" }
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
                                // The two-documents-at-once route in one step:
                                // pick the second and it arrives beside the
                                // first rather than on top of it. A menu item
                                // and not a key, because it is not a key in
                                // `keys.ts` either.
                                button {
                                    class: "menu-item",
                                    onclick: {
                                        let pick = pick.clone();
                                        move |_| {
                                            viewer.write().close_menu();
                                            pick.ask(Opening::Beside);
                                        }
                                    },
                                    Icon { name: "window", stroke: ink.clone() }
                                    span { class: "menu-label", "Open document in new window…" }
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
                                    Icon { name: "window", stroke: ink.clone() }
                                    span { class: "menu-label", "New window" }
                                    span { class: "menu-key", "{key_new_window}" }
                                }
                                // **And the other half of what ⌘N used to do
                                // by accident.** macOS turns a new window
                                // into a tab of its own accord while the app
                                // is full screen, which is a good thing to be
                                // able to ask for and a bad thing to be given
                                // — so it is switched off (see `tabs.rs`) and
                                // said here instead. macOS alone has tabs, so
                                // the item is there alone.
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
                                        Icon { name: "window", stroke: ink.clone() }
                                        span { class: "menu-label", "New tab" }
                                    }
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
                                            Icon { name: "document", stroke: crate::palette::hex(wearing.faint()) }
                                            span { class: "menu-label", "{entry.title}" }
                                            span { class: "menu-key", "p. {entry.page}" }
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
                        onclick: {
                            let frame = frame.clone();
                            move |_| frame.ask(Ask::NewWindow)
                        },
                        Icon { name: "window", stroke: ink.clone() }
                        span { class: "chip-label", "New window" }
                    }
                    button {
                        class: "chip close-window",
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
                            class: match (menu == Some(Menu::Document), name_clipped) {
                                (true, true) => "chip title on clipped",
                                (true, false) => "chip title on",
                                (false, true) => "chip title clipped",
                                (false, false) => "chip title",
                            },
                            onmousedown: move |event| event.stop_propagation(),
                            onclick: move |_| viewer.write().show_menu(Menu::Document),
                            // **No icon.** This is the one thing in the bar
                            // that is not a verb, and an icon in front of a
                            // file name says nothing the name does not — while
                            // costing it twenty-three pixels in a bar with none
                            // to spare.
                            //
                            // The colour is named here for the reason the zoom
                            // readout's is: with no icon nothing about this
                            // button changes when the theme does, and Blitz
                            // settles a text run's colour when it builds the
                            // run.
                            style: if menu == Some(Menu::Document) { "color: {ink_on}" } else { "color: {crate::palette::hex(wearing.faint())}" },
                            "{shelf_name}"
                        }
                        if menu == Some(Menu::Document) {
                            div { class: "menu document", role: "menu", "aria-label": "Document",
                                onmousedown: move |event| event.stop_propagation(),
                                // Where the document lives, which is the app's
                                // own first item — and the one thing in this
                                // menu that is about the file rather than
                                // about what is in it.
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
                                // The page marked. Not a chip in the bar: a
                                // mark is set once and read from the Contents
                                // panel, so a permanent button for it is one
                                // nobody presses twice in an hour. It ticks,
                                // which is what the chip's "on" state said.
                                button {
                                    class: "menu-item",
                                    "data-item": "mark",
                                    onclick: move |_| {
                                        viewer.write().close_menu();
                                        let page = viewer.read().page();
                                        viewer.write().mark_page(page);
                                    },
                                    Icon { name: "mark", stroke: ink.clone() }
                                    span { class: "menu-label", "Bookmark this page" }
                                    span { class: "menu-tick", {if marked { "✓" } else { "" }} }
                                    span { class: "menu-key", "{key_mark}" }
                                }
                                // **The one item in this menu the app has no
                                // counterpart for** — see [`crate::sign`]. It
                                // sits beside Mark this page because both are
                                // done *to* the document, as against the three
                                // below, which take it somewhere else.
                                button {
                                    class: "menu-item",
                                    "data-item": "sign",
                                    onclick: move |_| { viewer.write().open_signing(); },
                                    Icon { name: "sign", stroke: ink.clone() }
                                    span { class: "menu-label", "Sign…" }
                                }
                                // Printing prints nothing: the document goes
                                // to a program that does. See [`Printer`].
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
                                // Two ways of taking the document with you,
                                // which is the app's own pair. The name is
                                // what the toolbar shows; the path is what
                                // another program will want.
                                button {
                                    class: "menu-item",
                                    "data-item": "copy-name",
                                    onclick: {
                                        let clip = clip.clone();
                                        move |_| {
                                            viewer.write().close_menu();
                                            let name = viewer.read().store.title().to_string();
                                            clip.put(&name);
                                            viewer.write().notice = "Name copied.".into();
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
                                            clip.put(&path);
                                            viewer.write().notice = "Path copied.".into();
                                        }
                                    },
                                    Icon { name: "copy", stroke: ink.clone() }
                                    span { class: "menu-label", "Copy path" }
                                }
                                div { class: "menu-rule" }
                                // What the document says about itself. Last,
                                // and behind a rule, because it is the one
                                // item here that opens something rather than
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
                            class: if page_fresh { "page-field fresh" } else { "page-field" },
                            style: "width: {page_box}px; padding-left: {page_pad}px;",
                            r#type: "text",
                            value: "{page_field}",
                            // Not the root's business: see its `onmousedown`.
                            // A press inside the field is putting the caret
                            // somewhere, which also ends the "everything is
                            // selected" state — and `oninput` above depends on
                            // that, being able to take the label off the front
                            // only while the caret is still at the front.
                            onmousedown: move |event| {
                                event.stop_propagation();
                                if viewer.read().page_fresh {
                                    viewer.write().page_fresh = false;
                                }
                            },
                            "aria-label": "Go to page",
                            "data-keyboard": "goto",
                            // Arrived at by a button or a key, never by a
                            // press inside it: the caret goes after the number.
                            "data-caret": "end",
                            onmounted: move |event| {
                                let node = event.data();
                                let task = node.set_focus(true);
                                spawn(async move { let _ = task.await; });
                            },
                            // **The other half of the emulated select-all, and
                            // it is here rather than in the keydown because of
                            // where the caret ends up.** `set_text` replaces the
                            // editor's string and does not touch the *selection*,
                            // so the editor inserts at the caret — and a digit
                            // taken at face value there makes "50" out of "12"
                            // and a typed "50".
                            //
                            // Letting the editor insert moves the caret for us:
                            // fresh means it was at the end (`data-caret`), so
                            // what arrived is at the end and taking the old
                            // label off the front leaves exactly what was typed.
                            oninput: move |event| {
                                let typed = event.value();
                                let held = viewer.read();
                                let (fresh, was) = (held.page_fresh, held.page_typed.clone());
                                drop(held);
                                if fresh {
                                    let first = typed.strip_prefix(&was).unwrap_or(&typed);
                                    let first = first.to_string();
                                    viewer.write().type_page(&first);
                                } else {
                                    viewer.write().type_page(&typed);
                                }
                            },
                            // The same two rules the find field has, and for the
                            // same two reasons: a plain key typed here would
                            // otherwise bubble to the root and scroll the
                            // document, and Blitz applies a keystroke to a focused
                            // field whatever modifier is held down.
                            onkeydown: move |event| {
                                let key = event.key();
                                let modifiers = event.modifiers();
                                let plain = crate::keymap::plain(modifiers);
                                match key {
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
                                    // Backspace on a field whose contents are
                                    // all "selected" empties it. The editor's own
                                    // deletes one character before the caret.
                                    Key::Backspace | Key::Delete
                                        if plain && viewer.read().page_fresh =>
                                    {
                                        event.stop_propagation();
                                        event.prevent_default();
                                        viewer.write().type_page("");
                                    }
                                    _ if plain => event.stop_propagation(),
                                    Key::Character(ref typed)
                                        if matches!(typed.as_str(), "a" | "c" | "v" | "x" | "z") =>
                                    {
                                        event.stop_propagation()
                                    }
                                    _ => event.prevent_default(),
                                }
                            },
                        }
                        } else {
                        button {
                            class: "page-now",
                            // The colour is written out beside the width for
                            // the zoom readout's reason: Blitz left this label
                            // in the previous theme's ink, which on a dark theme
                            // after a light one is a page number nobody can see.
                            style: "width: {page_box}px; color: {crate::palette::hex(wearing.text)};",
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
                                        div { class: "menu-section", "Page count" }
                                        button {
                                            class: if numbering_printed { "menu-item on" } else { "menu-item" },
                                            onclick: move |_| { viewer.write().set_page_numbering(true); viewer.write().close_menu(); },
                                            span { class: "menu-tick", {if numbering_printed { "✓" } else { "" }} }
                                            span { class: "menu-label", "{numbering_as_printed}" }
                                            span { class: "menu-key", "As printed" }
                                        }
                                        button {
                                            class: if !numbering_printed { "menu-item on" } else { "menu-item" },
                                            onclick: move |_| { viewer.write().set_page_numbering(false); viewer.write().close_menu(); },
                                            span { class: "menu-tick", {if !numbering_printed { "✓" } else { "" }} }
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
                    // The card hangs off this chip, so the chip is in an
                    // `.anchor` like every other button in the bar that opens
                    // something: the card came down at the window's right edge
                    // and twelve pixels below the bar, which reads as a panel
                    // belonging to nothing.
                    div { class: "anchor",
                        button {
                            class: if find_open { "chip find on" } else { "chip find" },
                            onclick: move |_| {
                                let token = viewer.write().open_find();
                                rescan(viewer, token);
                            },
                            Icon { name: "search", stroke: if find_open { ink_on.clone() } else { ink.clone() } }
                            span { class: "chip-label", "Search" }
                        }
                        if find_open && !bar_tight {
                            {find_card("top: calc(100% + 8px); right: 0;")}
                        }
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
                                // **The one label in the bar with no icon
                                // beside it, and the only one that kept the last
                                // theme's colour.** Blitz settles a text run's
                                // colour when it builds the run, and rebuilds
                                // only when the element or its children are
                                // mutated — a change to a custom property on the
                                // root is neither. Every other chip has an `Icon`
                                // whose `stroke` is the theme's, so every other
                                // chip is mutated and comes out right. Naming the
                                // colour here is the same answer the icons carry.
                                style: if menu == Some(Menu::View) { "color: {ink_on}" } else { "color: {ink}" },
                                onmousedown: move |event| event.stop_propagation(),
                                onclick: move |_| viewer.write().show_menu(Menu::View),
                                "{zoom}"
                            }
                            if menu == Some(Menu::View) {
                                div { class: "menu view", role: "menu", "aria-label": "View",
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
                                        span { class: "menu-tick", {if fit == Fit::Width { "✓" } else { "" }} }
                                        span { class: "menu-label", "Fit width" }
                                        span { class: "menu-key", "{key_fit_width}" }
                                    }
                                    button {
                                        class: if fit == Fit::Page { "menu-item on" } else { "menu-item" },
                                        onclick: move |_| viewer.write().set_fit(Fit::Page),
                                        span { class: "menu-tick", {if fit == Fit::Page { "✓" } else { "" }} }
                                        span { class: "menu-label", "Fit page" }
                                        span { class: "menu-key", "{key_fit_page}" }
                                    }
                                    button {
                                        class: if actual_100 { "menu-item on" } else { "menu-item" },
                                        onclick: move |_| viewer.write().actual_size(),
                                        span { class: "menu-tick", {if actual_100 { "✓" } else { "" }} }
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
                                                    span { class: "menu-tick", {if on { "✓" } else { "" }} }
                                                    span { class: "menu-label", "{percent:.0}%" }
                                                }
                                            }
                                        }
                                    }
                                    // **The rotations, here rather than in the
                                    // bar**: rare, and their room in the bar
                                    // is better spent keeping every other
                                    // control's words. This menu stays up, so
                                    // turning a scan twice is two presses here
                                    // as it was there. `menu-gap` rather than
                                    // `menu-rule`: see `OURS` in
                                    // `tests/parity.rs`.
                                    div { class: "menu-gap" }
                                    button {
                                        class: "menu-item",
                                        onclick: move |_| viewer.write().rotate(-1),
                                        span { class: "menu-tick" }
                                        span { class: "menu-label", "Rotate left" }
                                        span { class: "menu-key", "{key_rotate_left}" }
                                    }
                                    button {
                                        class: "menu-item",
                                        onclick: move |_| viewer.write().rotate(1),
                                        span { class: "menu-tick" }
                                        span { class: "menu-label", "Rotate right" }
                                        span { class: "menu-key", "{key_rotate_right}" }
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
                                                span { class: "menu-tick", {if index == theme_index { "✓" } else { "" }} }
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
                                        span { class: "menu-label", "Delete this theme…" }
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
                            onmousedown: move |event| event.stop_propagation(),
                            onclick: move |_| viewer.write().show_menu(Menu::Settings),
                            Icon { name: "settings", stroke: if menu == Some(Menu::Settings) { ink_on.clone() } else { ink.clone() } }
                            span { class: "chip-label", "Settings" }
                        }
                        if menu == Some(Menu::Settings) {
                            div { class: "menu settings", role: "menu", "aria-label": "Settings",
                                onmousedown: move |event| event.stop_propagation(),
                                div { class: "menu-section", "Window" }
                                div { class: "menu-row",
                                    label { class: "menu-row-text",
                                        onclick: move |_| {
                                            viewer.write().toggle_toolbar();
                                            viewer.write().close_menu();
                                        },
                                        span { class: "menu-row-label", "Show toolbar" }
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
                                div { class: "menu-rule" }
                                div { class: "menu-section", "Reading" }
                                button {
                                    class: if scroll_mode == crate::layout::Mode::Continuous { "menu-item on" } else { "menu-item" },
                                    onclick: move |_| {
                                        viewer.write().set_scroll_mode(crate::layout::Mode::Continuous);
                                        viewer.write().close_menu();
                                    },
                                    span { class: "menu-tick", {if scroll_mode == crate::layout::Mode::Continuous { "✓" } else { "" }} }
                                    span { class: "menu-label", "Continuous scrolling" }
                                    span { class: "menu-key", "Default" }
                                }
                                button {
                                    class: if scroll_mode == crate::layout::Mode::Paged { "menu-item on" } else { "menu-item" },
                                    onclick: move |_| {
                                        viewer.write().set_scroll_mode(crate::layout::Mode::Paged);
                                        viewer.write().close_menu();
                                    },
                                    span { class: "menu-tick", {if scroll_mode == crate::layout::Mode::Paged { "✓" } else { "" }} }
                                    span { class: "menu-label", "One page at a time" }
                                }
                                div { class: "menu-rule" }
                                div { class: "menu-section", "Pages side by side" }
                                button {
                                    class: if spread == Spread::Single { "menu-item on" } else { "menu-item" },
                                    onclick: move |_| { viewer.write().set_spread(Spread::Single); viewer.write().close_menu(); },
                                    span { class: "menu-tick", {if spread == Spread::Single { "✓" } else { "" }} }
                                    span { class: "menu-label", "One page across" }
                                    span { class: "menu-key", "Default" }
                                }
                                button {
                                    class: if spread == Spread::Two { "menu-item on" } else { "menu-item" },
                                    onclick: move |_| { viewer.write().set_spread(Spread::Two); viewer.write().close_menu(); },
                                    span { class: "menu-tick", {if spread == Spread::Two { "✓" } else { "" }} }
                                    span { class: "menu-label", "Two side by side" }
                                }
                                button {
                                    class: if spread == Spread::Cover { "menu-item on" } else { "menu-item" },
                                    onclick: move |_| { viewer.write().set_spread(Spread::Cover); viewer.write().close_menu(); },
                                    span { class: "menu-tick", {if spread == Spread::Cover { "✓" } else { "" }} }
                                    span { class: "menu-label", "Two, cover alone" }
                                }
                                div { class: "menu-rule" }
                                div { class: "menu-row",
                                    label { class: "menu-row-text",
                                        onclick: move |_| viewer.write().set_recolor_images(!recolor_images),
                                        span { class: "menu-row-label", "Recolour pictures too" }
                                        span { class: "menu-row-note", "Off leaves them as printed." }
                                    }
                                    crate::prefs::Toggle {
                                        on: recolor_images,
                                        onchange: move |on: bool| viewer.write().set_recolor_images(on),
                                    }
                                }
                                div { class: "menu-row",
                                    label { class: "menu-row-text",
                                        onclick: move |_| viewer.write().set_page_pill(!page_pill),
                                        span { class: "menu-row-label", "Show page count while scrolling" }
                                        span { class: "menu-row-note", "Only when the toolbar is hidden." }
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
                                    span { class: "menu-tick", {if numbering_printed { "✓" } else { "" }} }
                                    span { class: "menu-label", "As printed on the page" }
                                    span { class: "menu-key", "Default" }
                                }
                                button {
                                    class: if !numbering_printed { "menu-item on" } else { "menu-item" },
                                    onclick: move |_| { viewer.write().set_page_numbering(false); viewer.write().close_menu(); },
                                    span { class: "menu-tick", {if !numbering_printed { "✓" } else { "" }} }
                                    span { class: "menu-label", "Count from 1" }
                                }
                                div { class: "menu-rule" }
                                button {
                                    class: "menu-item",
                                    onclick: move |_| {
                                        viewer.write().close_menu();
                                        viewer.write().open_settings();
                                    },
                                    span { class: "menu-tick", "" }
                                    Icon { name: "settings", stroke: ink.clone() }
                                    span { class: "menu-label", "All settings…" }
                                    span { class: "menu-key", "{key_settings}" }
                                }
                            }
                        }
                    }
                }
            }
            }
            // With the toolbar away there is no Search chip to hang under, so
            // the card comes up to meet the window's edge at the right — which
            // is `#shell[data-toolbar="hidden"] .find-bar` in the app.
            if find_open && !toolbar_on {
                {find_card("top: 8px; right: 10px;")}
            } else if find_open && bar_tight {
                {find_card("top: 54px; right: 10px;")}
            }
            div { class: "body",
            if empty {
                Start { viewer, pick: pick.clone(), frame: frame.clone() }
            }
            if sidebar_open && !presenting && !empty {
                Sidebar {
                    viewer,
                    chosen: chosen.clone(),
                }
            }
            if !empty {
            div {
                class: "viewer",
                onmounted: move |_| resize_from_window(viewer),
                // **The middle button drops the anchor**, and the press is
                // kept here rather than left to bubble: the root ends the
                // gesture on any press, and this is the one press that
                // begins one. See the stationary scroll in `Viewer`.
                onmousedown: move |event| {
                    if event.trigger_button() != Some(dioxus::html::input_data::MouseButton::Auxiliary) {
                        return;
                    }
                    event.stop_propagation();
                    let at = event.client_coordinates();
                    start_still(viewer.write().hold_still((at.x, at.y)));
                },
                onwheel: move |event| {
                    // A wheel is the reader scrolling for themselves, which
                    // is the gesture the anchor was standing in for.
                    if viewer.read().scrolling_still() {
                        viewer.write().stop_still();
                    }
                    // **⌃-wheel and ⌘-wheel are zoom**, which is how a mouse
                    // with no pinch says it and how winit reports a pinch on
                    // the platforms that do not send a gesture. The factor is
                    // exponential in the distance, so a fast flick and a slow
                    // drag arrive at the same place per pixel; 320 is the app's
                    // constant.
                    if crate::keymap::command(event.modifiers())
                        || event.modifiers().ctrl()
                    {
                        let down = match event.delta() {
                            WheelDelta::Pixels(delta) => delta.y,
                            WheelDelta::Lines(delta) => delta.y * LINE,
                            WheelDelta::Pages(delta) => {
                                delta.y * viewer.read().layout.viewport.height
                            }
                        };
                        let capped = down.clamp(-60.0, 60.0);
                        viewer.write().zoom_by((capped / 320.0).exp());
                        return;
                    }
                    // A trackpad sends pixels and a mouse sends lines; both
                    // arrive here. The sign is the platform's, not the DOM's:
                    // winit hands over a negative y for reading forwards and
                    // Blitz's own scroller negates it, so this does too.
                    let (across, down) = match event.delta() {
                        WheelDelta::Pixels(delta) => (delta.x, delta.y),
                        WheelDelta::Lines(delta) => (delta.x * LINE, delta.y * LINE),
                        WheelDelta::Pages(delta) => {
                            let height = viewer.read().layout.viewport.height;
                            (delta.x * height, delta.y * height)
                        }
                    };
                    // **A wheel that moves nothing is not a scroll.** macOS
                    // sends one when two fingers land on the trackpad, which
                    // is how every pinch begins, and it flashed the pill.
                    if across == 0.0 && down == 0.0 {
                        return;
                    }
                    // The other axis, which only ever has anything in it when
                    // the reader has zoomed past the width of the window.
                    // macOS turns ⇧-wheel into one of these before winit sees
                    // it, so this is both gestures.
                    if across != 0.0 {
                        viewer.write().pan(-across);
                    }
                    viewer.write().wheel_nudge(-down);
                },
                div {
                    class: "pages",
                    style: "width: {content_width}px; height: {content_height}px;",
                    // The one thing the reader can see and the DOM cannot
                    // say. Everything else the harness asserts on is text in
                    // the toolbar, which is better because it is what somebody
                    // reading the screen would check; a scroll offset is a
                    // number with no pixels of its own.
                    "data-scroll": "{scroll_top}",
                    for placed in boxes {
                        Page {
                            // The page, the theme it is wearing, its view and
                            // which document it is of. Not its size: a page
                            // redraws itself at a new size, showing the old
                            // texture until the new one lands — see `page.rs`.
                            key: "{placed.index}:{worn}:{view_key}:{opened}",
                            chosen: chosen.clone(),
                            index: placed.index,
                            top: placed.top - scroll_top,
                            left: placed.left - scroll_left,
                            width: placed.width,
                            height: placed.height,
                            hits: placed.hits,
                            links: placed.links,
                            notes: placed.notes,
                            selected: placed.selected,
                            swatches: placed.swatches,
                            mark: placed.mark,
                            drawn: placed.drawn,
                            colours: markup_colours.clone(),
                            view,
                            viewer,
                            away: away.clone(),
                        }
                    }
                }
                // **The scrollbar, drawn over the document and hard against
                // the window's edge.** It is the last child of `.viewer` and
                // carries a `z-index` for the hit-testing reason every other
                // layer in this window does — without it the press goes to
                // the page underneath and begins a sweep.
                //
                // No margin on its right, and that is the requirement rather
                // than a detail: a pointer thrown at the edge of the screen
                // stops at the edge, and a bar an inch short of it is a bar
                // that has to be aimed at.
                // **Where the stationary scroll is anchored.** The one
                // thing on screen saying the gesture is running, and the
                // point the pointer's distance is measured from — so it is
                // drawn at the press, not at the pointer. `chrome()` is what
                // takes it from the window's coordinates into `.body`'s,
                // exactly as the scrollbar's own press does.
                if let Some((anchor_x, anchor_y)) = still_anchor {
                    div {
                        class: "still-anchor",
                        style: "left: {anchor_x - STILL_MARK / 2.0}px; top: {anchor_y - chrome - STILL_MARK / 2.0}px;",
                        svg {
                            view_box: "0 0 24 24",
                            width: "{STILL_MARK}",
                            height: "{STILL_MARK}",
                            "aria-hidden": "true",
                            dangerous_inner_html: "{mark}",
                        }
                    }
                }
                if let Some((thumb_top, thumb_height)) = thumb.filter(|_| bar_up) {
                    div {
                        class: "scrollbar",
                        // Left to bubble on purpose: a press here is still a
                        // press somewhere a menu is not, and the root is what
                        // puts one away.
                        onmousedown: move |event| {
                            viewer.write().press_bar(event.client_coordinates().y);
                        },
                        div {
                            class: if on_bar { "bar-thumb held" } else { "bar-thumb" },
                            "data-thumb": "{thumb_top as i64}:{thumb_height as i64}",
                            style: "top: {thumb_top}px; height: {thumb_height}px;",
                        }
                    }
                }
            }
            }
            }
            // **The way back to a toolbar that is not there.** `#toolbar-peek`
            // in the app: nothing is on screen until somebody reaches for the
            // top edge, and then a handle drops in and puts the bar back. The
            // notice that names ⌘T is four seconds long and this is not, which
            // is the difference between a way back and having been told one.
            if !toolbar_on && !presenting && peeking {
                div { class: "peek-line",
                    button {
                        class: "toolbar-peek",
                        onclick: move |_| viewer.write().toggle_toolbar(),
                        Icon { name: "down", stroke: ink.clone() }
                        // Two lines, like the notice above it: "Show toolbar"
                        // on one line beside an icon is a wide, low box that
                        // reads as a strip of chrome rather than as a button.
                        "Show\ntoolbar"
                    }
                }
            }
            // Where the reader is, while they scroll with the toolbar away.
            // `#page-pill` in the app, and the same two conditions on it —
            // see [`Viewer::flash_pill`].
            // **Beside the scrollbar, not over the middle of the page.** It
            // was centred along the lower edge, which is the one place it is
            // both hard to read and in the way of what it is describing. The
            // bar is where the reader's eye already is while they are moving,
            // and the pill rides its thumb; with no bar — a document that
            // fits — there is nothing to ride, so it keeps to the corner.
            if pill_up && !presenting {
                div {
                    class: "pill-line",
                    style: match pill_y {
                        Some(y) => format!("top: {y}px; bottom: auto;"),
                        None => String::new(),
                    },
                    div { class: "page-pill", "{pill_text}" }
                }
            }
            // The line the toolbar's own way back is written on, which is
            // why it outlives the toolbar — and presenting's, which is the
            // one mode with no other way to say how to leave it.
            if !notice.is_empty() {
                // Over the document, in the top right corner — under the
                // toolbar when there is one and up in its place when there is
                // not, which is the whole of what `tucked` changes. Two
                // elements rather than one because the placing is the outer
                // row's: a flex row does it with no transform, and a transform
                // is not something to lean on in Blitz. See `.notice-line`.
                div { class: if toolbar_on { "notice-line" } else { "notice-line tucked" },
                    div { class: "notice", "{notice}" }
                }
            }
            // What a document being dragged over the window looks like. Over
            // everything but Settings, and outside `.body` so that it covers
            // the panel and the toolbar too: the promise is "anywhere in this
            // window", which is the app's own wording and has to be true of
            // the whole window.
            if let Some(takeable) = dragging {
                div { class: if takeable { "drop-hint" } else { "drop-hint refused" },
                    span { class: "drop-hint-word",
                        {if takeable { "Drop to open" } else { "That is not a PDF" }}
                    }
                }
            }
            // A note, opened. A window rather than a tooltip because a note can
            // be a paragraph, and a tooltip's whole vocabulary is one line that
            // goes away when the pointer does. The sentence at its foot is the
            // honest half: this reader shows the notes a document carries and
            // has no way to write one.
            if let Some((_page, note)) = note_open {
                div {
                    class: "window-scrim",
                    onmousedown: move |event| {
                        event.stop_propagation();
                        viewer.write().close_note();
                    },
                    div {
                        class: "window note-window",
                        role: "dialog",
                        "aria-modal": "true",
                        "aria-label": "Note",
                        onmousedown: move |event| event.stop_propagation(),
                        div { class: "window-bar",
                            span { class: "window-title",
                                {if note.by.is_empty() { "Note".to_string() } else { note.by.clone() }}
                            }
                            button {
                                class: "chip window-close",
                                "aria-label": "Close",
                                onclick: move |_| { viewer.write().close_note(); },
                                Icon { name: "close", stroke: ink.clone() }
                            }
                        }
                        div { class: "note-body",
                            p { class: "note-where", "On page {note_page}." }
                            p { class: "note-text", "{note.text}" }
                            p { class: "note-said",
                                "Moonowl shows the notes a document already carries. It does not write them."
                            }
                        }
                    }
                }
            }
            // The six highlight colours, edited. A window for the theme
            // editor's reason: a full picker under a swatch on a page would
            // hang off the passage and off the edge of the window with it.
            if colours_open {
                crate::prefs::MarkupColours { viewer }
            }
            // What the document says about itself. `showDocumentDetails` in
            // `main.ts`, field for field and in its order — and a window
            // rather than a panel for the reason the note beside it is one:
            // it is read once and dismissed.
            if details_open {
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
                        onmousedown: move |event| event.stop_propagation(),
                        div { class: "window-bar",
                            span { class: "window-title", "Information" }
                            button {
                                class: "chip window-close",
                                "aria-label": "Close",
                                onclick: move |_| { viewer.write().close_details(); },
                                Icon { name: "close", stroke: ink.clone() }
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
            // **The Sign window**, which has no counterpart in the app: see
            // [`crate::sign`]. A window rather than a panel for the reason the
            // two above are windows — it is opened on purpose, answered, and
            // dismissed — and it is the only one in this reader with a
            // *drawing surface* in it.
            if let Some(pad) = signing {
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
                        onmousedown: move |event| event.stop_propagation(),
                        div { class: "window-bar",
                            span { class: "window-title", "Sign this document" }
                            button {
                                class: "chip window-close",
                                "aria-label": "Close",
                                onclick: move |_| { viewer.write().close_signing(); },
                                Icon { name: "close", stroke: ink.clone() }
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
                                                span { class: "sign-where", "page {placed.page}" }
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
            // A document that will not open without a password.
            // `ui.askForPassword` in the app, and it is a window there for the
            // reason the two above are: it is the only thing on screen worth
            // attending to, and the answer decides whether there is a document
            // at all.
            if let Some(asking) = locked {
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
                        onmousedown: move |event| event.stop_propagation(),
                        div { class: "window-bar",
                            span { class: "window-title", "This document is locked" }
                            button {
                                class: "chip window-close",
                                "aria-label": "Close",
                                onclick: move |_| { viewer.write().stop_unlocking(); },
                                Icon { name: "close", stroke: ink.clone() }
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
                                    // The same two rules every field in this
                                    // file has — a plain key would otherwise
                                    // scroll the document behind the window —
                                    // plus the two this one is for.
                                    onkeydown: {
                                        let frame = frame.clone();
                                        move |event: KeyboardEvent| {
                                            let plain =
                                                crate::keymap::plain(event.modifiers());
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
                                                _ if plain => event.stop_propagation(),
                                                Key::Character(ref typed)
                                                    if matches!(
                                                        typed.as_str(),
                                                        "a" | "c" | "v" | "x" | "z"
                                                    ) =>
                                                {
                                                    event.stop_propagation()
                                                }
                                                _ => event.prevent_default(),
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
            // And Settings, over all of it. Last in the root so that it is
            // last in paint order too — Blitz paints by the rules, and a
            // scrim that comes before the document is a scrim behind it.
            crate::prefs::Settings { viewer, frame: frame.clone() }
            // Over Settings, because the editor's Delete opens it from there.
            crate::prefs::ConfirmDeleteTheme { viewer }
        }
    }
}

/// The window with nothing in it: what a reader sees before there is a
/// document, and after they have put one down.
///
/// The app's own screen item for item — the name, one line under it, the
/// button that opens a document, the last six read with the page each was left
/// on, and the sentence saying a document can be dropped on the window. The
/// one difference is that it *replaces* the document region rather than being
/// laid over it, which is the same picture and one fewer thing on screen.
///
/// Its absence had reached into three other places: ⌘N opened a second window
/// on the document already in front of somebody, `Handover::Fill` was
/// unreachable because no window was ever idle, and there was no way to close
/// a document without closing its window.
#[component]
fn Start(viewer: Signal<Viewer>, pick: Pick, frame: Frame) -> Element {
    // Read once for the render, like everything else the toolbar reads: this
    // is a file on the disk, and the alternative is reading it per row.
    let recents = viewer.read().recents();
    let ink = crate::palette::hex(viewer.read().palette().muted());
    let open = {
        let pick = pick.clone();
        move |_| pick.ask(Opening::Here)
    };
    rsx! {
        div { class: "start",
            div { class: "start-inner",
                h1 { class: "start-name", "Moonowl" }
                p { class: "start-sub", "A calm place to read." }
                button {
                    class: "start-open",
                    onclick: open,
                    // The label's own colour, which is `--accent-contrast`
                    // and not the theme's paper: the two are close on a dark
                    // theme and are not the same colour, and `.btn-primary`
                    // names the first.
                    Icon { name: "folder", stroke: crate::palette::hex(viewer.read().palette().accent_contrast()) }
                    "Open a document"
                }
                if !recents.is_empty() {
                    div { class: "recents",
                        div { class: "recents-title", "Recently read" }
                        for entry in recents {
                            div {
                                key: "{entry.path}",
                                class: "recent",
                                button {
                                    class: "recent-open",
                                    // The whole row opens it. The × beside it
                                    // is a button of its own rather than a
                                    // click this one has to tell apart by
                                    // where it landed.
                                    onclick: {
                                        let path = entry.path.clone();
                                        let frame = frame.clone();
                                        move |_| {
                                            let path = path.clone();
                                            if viewer.write().open_here(&path) {
                                                let title = viewer.read().store.title().to_string();
                                                frame.ask(Ask::Showing { path, title });
                                            }
                                        }
                                    },
                                    Icon { name: "document", stroke: ink.clone() }
                                    span { class: "recent-name", "{entry.title}" }
                                    // Where they stopped, in a column of its
                                    // own — on every row rather than only the
                                    // ones past page one, so the list has a
                                    // straight edge. The app's own reasoning,
                                    // and its own words.
                                    span { class: "recent-page", "p. {entry.page}" }
                                }
                                button {
                                    class: "recent-forget",
                                    "aria-label": "Remove from this list",
                                    onclick: {
                                        let path = entry.path.clone();
                                        move |_| viewer.write().forget(&path)
                                    },
                                    Icon { name: "close", stroke: ink.clone() }
                                }
                            }
                        }
                    }
                }
                p { class: "start-hint", "Or drop a PDF anywhere in this window" }
            }
        }
    }
}

/// A signature, drawn: an SVG `polyline` per stroke, with round caps and joins
/// so a name looks written rather than plotted.
///
/// `literal` is the difference between the pad and the list. In the list a
/// signature is stretched to its box, which is what it will look like on the
/// page. The pad must not do that: stretching what is being drawn as it is
/// drawn moves the ink out from under the hand, and the second stroke lands
/// where the first has just been dragged away from.
#[component]
pub(crate) fn Scrawl(
    signature: crate::sign::Signature,
    width: f64,
    height: f64,
    /// What the strokes are drawn in — a string, as an icon's is: an inline
    /// `<svg>` has no cascade behind it. See [`Icon`].
    ink: String,
    #[props(default)] literal: bool,
) -> Element {
    // Fitted into the box, or taken as written. **One scale either way**, which
    // is the same rule `sign::place` follows and for the same reason: the
    // strokes are height-normalised, so x and y are in one unit and scaling
    // them differently is what turns a name into a scribble.
    let aspect = signature.aspect();
    let (scale, drawn_width, drawn_height) = if literal {
        // The pad's own space, where a point is already a pixel.
        (1.0, width, height)
    } else {
        // Whichever of the two constraints binds first, so a wide signature in
        // a short box is fitted across rather than clipped.
        let fitted = height.min(width / aspect.max(f64::EPSILON));
        (fitted, fitted * aspect, fitted)
    };
    let across = (width - drawn_width) / 2.0;
    let down = (height - drawn_height) / 2.0;
    let lines: Vec<String> = signature
        .strokes
        .iter()
        .filter(|stroke| !stroke.is_empty())
        .map(|stroke| {
            stroke
                .iter()
                .map(|point| {
                    format!(
                        "{:.2},{:.2}",
                        across + point[0] * scale,
                        down + point[1] * scale
                    )
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    rsx! {
        svg {
            class: "scrawl",
            width: "{width}",
            height: "{height}",
            view_box: "0 0 {width} {height}",
            for (at, points) in lines.iter().enumerate() {
                polyline {
                    key: "{at}",
                    points: "{points}",
                    fill: "none",
                    stroke: "{ink}",
                    "stroke-width": "2",
                    "stroke-linecap": "round",
                    "stroke-linejoin": "round",
                }
            }
        }
    }
}

/// One icon, at the size the chrome wants it.
///
/// **The colour is an attribute rather than `currentColor`, and that is
/// Blitz's shape rather than a choice.** An inline `<svg>` is not laid out as
/// elements: `construct.rs` hands the subtree's `outer_html` to usvg, which has
/// no CSS cascade and no idea what `color` its button resolved to. So the
/// theme's shade goes in as `stroke`, and what a browser gives for free — an
/// icon following its label through hover — has to be passed down.
#[component]
pub(crate) fn Icon(
    name: &'static str,
    #[props(default)] stroke: Option<String>,
    /// Beside `icon`: `hot` or `rest` for the pair a hover swaps.
    #[props(default)]
    class: &'static str,
) -> Element {
    // A name nothing draws is nothing drawn, rather than a panic.
    let Some(body) = crate::icons::path(name) else {
        return rsx! {};
    };
    let stroke = stroke.unwrap_or_else(|| "currentColor".to_string());
    rsx! {
        svg {
            class: "icon {class}",
            view_box: "0 0 24 24",
            width: "16",
            height: "16",
            fill: "none",
            stroke: "{stroke}",
            // And `color`, because two of these icons fill part of themselves
            // with `currentColor` — the theme circle's dark half, and the
            // cog's centre. usvg resolves that against the `color` property on
            // the element or an ancestor and falls back to black, which on a
            // dark theme is a hole in the middle of the icon.
            color: "{stroke}",
            stroke_width: "1.7",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            dangerous_inner_html: "{body}",
        }
    }
}

/// One page, in its place.
#[component]
fn Page(
    chosen: Chosen,
    index: usize,
    top: f64,
    left: f64,
    width: f64,
    height: f64,
    /// Where the matches on this page are, in CSS pixels from its top left,
    /// and which of them the reader is on.
    ///
    /// **These are nodes, not pixels, and that is the whole of the port.** The
    /// app copies the page canvas through a luminance ramp and lays it back
    /// down, because a `::selection` colour puts pdf.js's text layer on screen
    /// and a page's bold type comes back regular. There is no text layer here:
    /// a match is a rectangle in PDF points, so it is a `div` over the page and
    /// the glyphs underneath are the ones pdfium drew.
    hits: Vec<(Rect, bool)>,
    /// The notes on this page, in the same space as the links.
    notes: Vec<(Rect, crate::render::Note)>,
    /// The document's own links on this page, in the same space as `hits`.
    ///
    /// A node each, for the reason the highlights are nodes: there is no text
    /// layer here to hang an anchor off, and a rectangle is what a link
    /// actually is in the file.
    links: Vec<(Rect, Target)>,
    /// What the reader has swept over, on this page.
    ///
    /// Rectangles like the matches. On the GPU the words under them are
    /// ramped to the theme's selection ink by [`crate::gpu::Recolorer::select`],
    /// which is the app's `paintSelection`; these are what is pressed and what
    /// the software path paints.
    selected: Vec<Rect>,
    /// Where the colour popover goes on this page, when it is on this page.
    ///
    /// It hangs off the page rather than the window because that is the space
    /// it is positioned in: under the last line of the selection, which is a
    /// rectangle in the page's own box.
    swatches: Option<Rect>,
    /// The mark the reader clicked on, when it is on this page: the line they
    /// hit, how to take it out, and the colour it is drawn in. See
    /// [`Viewer::mark_open`].
    mark: Option<(Rect, MarkKey, String)>,
    /// The size this page's texture is drawn at, which is its box except
    /// under a zoom gesture. See [`Viewer::zoom_held_at`].
    drawn: (f64, f64),
    /// The colours it offers. See [`Viewer::markup_colors`].
    colours: Vec<String>,
    /// How this page is turned and how much of it is drawn. In the key as
    /// well, which is what gives the old texture back — see [`PageWidget`].
    view: crate::layout::View,
    /// Following a link is a jump, and a jump is the viewer's.
    viewer: Signal<Viewer>,
    /// …unless it leads out of the document, which is the window's. See
    /// [`Away`].
    away: Away,
) -> Element {
    // The widget is handed over the first time the attribute is set, so
    // `use_hook` is what keeps a re-render from building a second one — and
    // what makes a page that merely moved keep the texture it has.
    let worn = chosen.get();
    let on_page = move |colour: &String| {
        crate::palette::read_colour(colour)
            .map(|rgb| crate::palette::hex(worn.on_page(rgb)))
            .unwrap_or_else(|| colour.clone())
    };
    let widget = use_hook(|| {
        let shell = dioxus_core::try_consume_context::<
            std::sync::Arc<dyn blitz_traits::shell::ShellProvider>,
        >();
        CustomWidgetAttr::new(PageWidget::new(index, view, chosen.clone(), shell))
    });

    rsx! {
        div {
            class: "page",
            // Which page this is. The mounting window is the single most
            // load-bearing thing in `layout.rs` and it is invisible from
            // outside without this.
            "data-page": "{index + 1}",
            // The size the texture under this page is drawn at, which is the
            // box's own except while a zoom gesture is running — see
            // [`Viewer::zoom_held_at`]. On the page because nothing else in the
            // DOM says it, and a test without it would have to photograph the
            // difference between sharp and stretched.
            "data-drawn": "{drawn.0}x{drawn.1}",
            style: "position: absolute; top: {top}px; left: {left}px; width: {width}px; height: {height}px;",
            // Where a sweep begins, and the whole of what a page hears from
            // the pointer.
            //
            // **`onclick` and `ondoubleclick` would never fire here.** A page is
            // a custom widget, and `handle_dom_event` forwards an event whose
            // target is a widget straight to the widget and *returns* — so the
            // default action never runs, and `click` and `dblclick` are both
            // default actions of `pointerup`. Handlers still run, the handler
            // phase being before the default action, which is why `onmousedown`
            // works at all. So the second click is counted here; see
            // [`Viewer::begin_sweep`].
            //
            // The rest of the sweep is on the root, because a pointer dragged
            // down a document leaves the page it started on within a line or
            // two.
            onmousedown: move |event| {
                // **Only the left button does anything to a page.** The
                // middle one drops the stationary scroll's anchor and the
                // right one is the system's; both used to begin a sweep,
                // which put a stray selection down under the very gesture
                // that was about to do something else.
                if event.trigger_button() != Some(dioxus::html::input_data::MouseButton::Primary) {
                    return;
                }
                // The press that ends a stationary scroll is spent on ending
                // it; the root does that when the press gets there.
                if viewer.read().scrolling_still() {
                    return;
                }
                let on = event.element_coordinates();
                let client = event.client_coordinates();
                // **A signature waiting for somewhere to go takes this press
                // instead of the sweep**, and is the first thing a press is
                // asked about: a sweep begun here would put the selection down
                // over the very page the reader is aiming at.
                if viewer.read().placing.is_some() {
                    viewer.write().sign_at(index + 1, (on.x, on.y));
                    return;
                }
                viewer.write().begin_sweep(
                    index + 1,
                    (on.x, on.y),
                    (client.x, client.y),
                );
            },
            object {
                "data": widget,
                // A widget laid out at 0×0 is a blank window with nothing to
                // say why, which is what `display: block` costs to avoid.
                style: "display: block; width: {width}px; height: {height}px;",
            }
            for (at, area) in selected.iter().enumerate() {
                div {
                    key: "s{at}",
                    class: "selected",
                    style: "position: absolute; top: {area.top}px; left: {area.left}px; width: {area.width}px; height: {area.height}px;",
                }
            }
            for (at, (quad, current)) in hits.iter().enumerate() {
                div {
                    key: "{at}",
                    class: if *current { "hit current" } else { "hit" },
                    style: "position: absolute; top: {quad.top}px; left: {quad.left}px; width: {quad.width}px; height: {quad.height}px;",
                }
            }
            if let Some(area) = swatches {
                div {
                    class: "markup-popover",
                    // **A press on the swatches must not reach the page**, or it
                    // begins a sweep of its own and puts down the very selection
                    // it is there to mark. The app answers this the other way
                    // round, capturing the selection when the popover opens,
                    // because a webview collapses it before any handler runs.
                    // Here the selection is the reader's own, so stopping the
                    // press is enough.
                    onmousedown: move |event| event.stop_propagation(),
                    // Under the line it is about. The rectangle is the line's
                    // own, so the offset is simply its height.
                    style: "position: absolute; top: {area.top + area.height + 8.0}px; left: {area.left}px;",
                    // Each swatch shows the colour as the page will show it
                    // — see `Palette::on_page` — and carries the colour as
                    // written, which is what is marked in.
                    for colour in colours.iter() {
                        button {
                            key: "{colour}",
                            class: "markup-swatch",
                            "data-colour": "{colour}",
                            "aria-label": "Highlight in {colour}",
                            style: "background: {on_page(colour)};",
                            onclick: {
                                let colour = colour.clone();
                                move |_| {
                                    viewer.write().mark_selection(&colour);
                                }
                            },
                        }
                    }
                    // The six are a shortcut, and this is the long way round:
                    // the window with the full picker, which is also where
                    // the six are changed.
                    button {
                        class: "markup-more",
                        "aria-label": "More colours",
                        title: "More colours…",
                        onclick: move |_| viewer.write().open_markup_colours(),
                        "…"
                    }
                    // And a way to put the swatches away that is not Escape
                    // and not a press somewhere else, both of which take the
                    // selection down with them.
                    button {
                        class: "markup-close",
                        "aria-label": "Close",
                        onclick: move |_| { viewer.write().close_markup(); },
                        "×"
                    }
                }
            }
            // **A mark clicked on says how to take it off.** Removal has worked
            // since markup landed and was reachable only from a × the width of a
            // full stop, on a row in a panel that does not open on that tab. See
            // [`Viewer::mark_open`], and `end_sweep`, which decides that a press
            // was a click rather than a sweep.
            if let Some((area, key, colour)) = mark {
                div {
                    class: "mark-popover",
                    // The same rule the swatches have, and for the same
                    // reason: a press in here must not reach the page and
                    // begin a sweep of its own — which would take this very
                    // popover down again on the way.
                    onmousedown: move |event| event.stop_propagation(),
                    style: "position: absolute; top: {area.top + area.height + 8.0}px; left: {area.left}px;",
                    span { class: "mark-dot", style: "background: {on_page(&colour)};" }
                    button {
                        class: "mark-remove",
                        onclick: move |_| {
                            if !viewer.write().remove_markup(&key) {
                                viewer.write().close_mark();
                            }
                        },
                        "Remove highlight"
                    }
                }
            }
            // **The notes a document already carries, made readable.** pdfium
            // paints an annotation's appearance into the page, so a sticky note
            // arrives as the icon it was drawn as — and the words behind it live
            // in the annotation, which nothing was reading. The icon sat there
            // looking like a button and was not one.
            for (at, (area, note)) in notes.iter().enumerate() {
                {
                    let opening = note.clone();
                    // A marker is pressable all over; a comment over a
                    // highlighted sentence answers on a strip at its right
                    // edge, because covering the sentence would put the words
                    // out of reach of a pointer that wants to select them.
                    let (left, width) = if note.icon {
                        (area.left, area.width)
                    } else {
                        (area.left + area.width, NOTE_EDGE)
                    };
                    let said = if note.by.is_empty() {
                        format!("Note. {}", note.text)
                    } else {
                        format!("Note. {}: {}", note.by, note.text)
                    };
                    rsx! {
                        div {
                            key: "n{at}",
                            class: if note.icon { "note-spot" } else { "note-edge" },
                            role: "button",
                            "aria-label": "{said}",
                            title: "{said}",
                            style: "position: absolute; top: {area.top}px; left: {left}px; width: {width}px; height: {area.height}px;",
                            onclick: move |_| viewer.write().open_note(index + 1, opening.clone()),
                        }
                    }
                }
            }
            for (at, (area, target)) in links.iter().enumerate() {
                div {
                    key: "l{at}",
                    class: "link",
                    // Deliberately not an `<a href>`, the app's decision made
                    // again for a different reason: an `href` would go through
                    // `nav.rs`, the chrome's door, which knows only http, https
                    // and mailto — so an internal link would find no scheme it
                    // allows and do nothing at all.
                    role: "link",
                    // A name, because the element has no text of its own: it is
                    // a bare rectangle over printed words with no text layer to
                    // reach them through, and a page of cross-references would
                    // otherwise read as "link, link, link".
                    "aria-label": match target {
                        Target::Away(url) => url.clone(),
                        Target::Place { page, .. } => format!("Page {page} of this document"),
                    },
                    style: "position: absolute; top: {area.top}px; left: {area.left}px; width: {area.width}px; height: {area.height}px;",
                    onclick: {
                        let target = target.clone();
                        let away = away.clone();
                        move |_| {
                            if let Some(url) = viewer.write().follow(&target) {
                                away.open(&url);
                            }
                        }
                    },
                }
            }
        }
    }
}

/// The same text, in the form a passage is looked up by. See
/// [`crate::search::fold`].
/// Where a remembered passage is now, starting from the page it used to be
/// on and working outwards — see [`Viewer::restore_markup`].
///
/// Through [`crate::search::fold`], which is what makes it work at all: a
/// passage that moved has very often been re-typeset on the way, so its
/// ligatures and soft hyphens are not the ones it had.
fn find_quote(document: &dyn PageSource, was_on: usize, quote: &str) -> Option<(usize, Vec<Rect>)> {
    let wanted = folded(quote);
    if wanted.is_empty() {
        return None;
    }
    let mut order: Vec<usize> = (1..=document.pages()).collect();
    order.sort_by_key(|page| page.abs_diff(was_on));
    for page in order {
        let text = document.text_of(page - 1);
        if text.chars.is_empty() {
            continue;
        }
        let folded = crate::search::fold(&text.chars, false);
        // Whitespace is flattened on both sides, because a passage that
        // moved has often been re-broken as well as re-set. Each character
        // of the flattened text remembers where in the folded text it came
        // from, so the answer is still a range of the page's own
        // characters — the trick `fold` plays one level down.
        let mut flat = String::with_capacity(folded.text.len());
        let mut back = Vec::with_capacity(folded.text.len());
        for (at, &character) in folded.text.iter().enumerate() {
            if character.is_whitespace() {
                if flat.ends_with(' ') || flat.is_empty() {
                    continue;
                }
                flat.push(' ');
            } else {
                flat.push(character);
            }
            back.push(at);
        }
        let Some(at) = flat.find(&wanted) else {
            continue;
        };
        // Byte offset into character offset, which is what `back` — and
        // through it `origin` — is indexed by.
        let from = flat[..at].chars().count();
        let to = from + wanted.chars().count();
        let (start, end) = (
            *folded.origin.get(*back.get(from)?)?,
            back.get(to)
                .and_then(|at| folded.origin.get(*at))
                .copied()
                .unwrap_or(text.chars.len()),
        );
        let quads = text.quads(start, end);
        if !quads.is_empty() {
            return Some((page, quads));
        }
    }
    None
}

fn folded(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    crate::search::fold(&chars, false)
        .text
        .into_iter()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// "One passage" and "three passages", which is a sentence rather than a
/// count followed by a noun.
fn said_of(many: usize, one: &str, more: &str) -> String {
    if many == 1 {
        format!("1 {one}")
    } else {
        format!("{many} {more}")
    }
}

/// Drive a scan that something restarted, from anywhere in the component.
///
/// The mailbox has a closure of this shape and cannot lend it out, being
/// spawned inside the task that owns the news — so this is it said once more,
/// for the gestures that reload the document without any news arriving.
pub(crate) fn rescan(mut viewer: Signal<Viewer>, token: Option<u64>) {
    let Some(token) = token else { return };
    spawn(async move {
        while viewer.write().scan_slice(token) {
            Breathe::once().await;
        }
    });
}

/// What a key still does while a window is up over the reader: leave it,
/// move between windows, and the few switches that are about the whole app.
fn answers_over_a_window(action: Action) -> bool {
    matches!(
        action,
        Action::Dismiss
            | Action::Settings
            | Action::Help
            | Action::CloseWindow
            | Action::Quit
            | Action::NewWindow
            | Action::NewTab
            | Action::Dark
            | Action::Fullscreen
    )
}

/// One handler per action, and a dispatch of about thirty lines: the table
/// decides *which* action, so nothing here knows anything about keys.
///
/// **There are no arms missing.** All forty-three of the app's actions answer
/// here, and the catch-all that used to say "not built yet" is gone — so an
/// action added to [`crate::keymap`] and not handled here is a compile error
/// rather than a sentence in the notice line.
fn perform(
    mut viewer: Signal<Viewer>,
    action: Action,
    screen: f64,
    frame: &Frame,
    clip: &Clip,
    pick: &Pick,
    printer: &Printer,
) {
    // Every movement goes through the viewer rather than through an offset,
    // because in paged mode an offset is not where a reader ends up: the page
    // has to be turned first. See [`Viewer::nudge`] and [`Viewer::go_to`].
    fn by(mut viewer: Signal<Viewer>, delta: f64) {
        viewer.write().nudge(delta);
    }

    match action {
        // Handled where the keystroke is, because which tab was asked for is
        // the digit that was pressed. See the key handler in `Reader`.
        Action::GoToTab => {}
        Action::ScrollDown => by(viewer, LINE),
        Action::ScrollUp => by(viewer, -LINE),
        Action::HalfScreenDown => by(viewer, (screen - OVERLAP) / 2.0),
        Action::HalfScreenUp => by(viewer, -(screen - OVERLAP) / 2.0),
        Action::ScreenDown => by(viewer, screen - OVERLAP),
        Action::ScreenUp => by(viewer, -(screen - OVERLAP)),
        Action::FirstPage => viewer.write().to_start(),
        Action::LastPage => viewer.write().to_end(),
        Action::NextPage => viewer.write().next_page(),
        Action::PreviousPage => viewer.write().previous_page(),
        Action::ZoomIn => viewer.write().zoom(true),
        Action::ZoomOut => viewer.write().zoom(false),
        Action::FitWidth => viewer.write().set_fit(Fit::Width),
        Action::FitPage => viewer.write().set_fit(Fit::Page),
        Action::ActualSize => viewer.write().actual_size(),
        Action::RotateRight => viewer.write().rotate(1),
        Action::RotateLeft => viewer.write().rotate(-1),
        Action::NextTheme => viewer.write().next_theme(),
        Action::Dark => viewer.write().toggle_dark(),
        // F1 and ⌘/, and the app's own answer to both: the Keyboard page,
        // which is the list of everything this reader answers to and is drawn
        // from the keymap rather than from a list of its own. "Help" behind a
        // cog is a strange place to keep the answer to "what can this thing
        // do", which is why it is a key at all.
        Action::Help => viewer.write().show_pane(Pane::Keyboard),
        // ⌘P, and it prints nothing: the document is handed to a program that
        // does. See [`Printer`].
        Action::Print => {
            let path = viewer.read().document.path().to_string();
            if path.is_empty() {
                return;
            }
            if let Err(said) = printer.print(&path) {
                viewer.write().notice = said;
            }
        }
        Action::Sidebar => viewer.write().toggle_sidebar(),
        Action::Mark => {
            let page = viewer.read().page();
            viewer.write().mark_page(page);
        }
        Action::GoToPage => viewer.write().open_page_field(),
        // Silence would be the wrong answer at the end of the history: the
        // reader pressed a key and has no way to tell a shortcut that did
        // nothing from one that is not bound. The app says the same two
        // sentences.
        Action::Back => {
            if !viewer.write().go_back() {
                viewer.write().notice = "Nowhere further back".to_string();
            }
        }
        Action::Forward => {
            if !viewer.write().go_forward() {
                viewer.write().notice = "Nowhere further forward".to_string();
            }
        }
        Action::Find => {
            let token = viewer.write().open_find();
            rescan(viewer, token);
        }
        // After Escape the matches are gone and the query is not: ⌘G brings
        // the search back, as ⌘F does, rather than saying "No matches" for a
        // word that was found a moment ago.
        Action::FindNext | Action::FindPrevious
            if !viewer.read().find_open && !viewer.read().find_query.is_empty() =>
        {
            let token = viewer.write().open_find();
            rescan(viewer, token);
        }
        Action::FindNext => viewer.write().step_match(true),
        Action::FindPrevious => viewer.write().step_match(false),
        // Escape, which is the way out of four things and says nothing when
        // there is nothing to leave — a key that answers with a complaint
        // about what it did not do is worse than one that does nothing.
        Action::Dismiss => {
            // Outward, in the order the reader arrived at them. A menu is
            // outermost of all — over everything and the thing the pointer is
            // inside — so it goes first, ahead of a field opened underneath it.
            // Then the page field: Escape while typing a number means "not that
            // after all", not "close the find bar I opened a minute ago".
            // Escape typed *into* the field never reaches here, so this is the
            // case where the pointer took the focus elsewhere.
            if viewer.write().close_menu() {
                return;
            }
            // A removal waiting for its second press: Escape is "not that".
            if viewer.write().arming.take().is_some() {
                return;
            }
            // "Delete this theme?", which is over everything, Settings included.
            if viewer.write().close_delete_theme() {
                return;
            }
            // The highlight colours window, before the swatches it was opened
            // from: it is over them, and Escape means the thing on top.
            if viewer.write().close_markup_colours() {
                return;
            }
            // The colour popover, which is a menu in everything but name and
            // sits in the same place in this list.
            if viewer.write().close_markup() {
                return;
            }
            // …and the one a mark clicked on puts up, which is the same kind
            // of thing in the same place. See [`Viewer::mark_open`].
            if viewer.write().close_mark() {
                return;
            }
            // The theme editor's colour picker, which is the same kind of
            // thing one line further in: a popover inside the Settings window,
            // so Escape means it before it means the window around it. The
            // fields below it answer Escape themselves — see
            // `prefs::typing_is_not_a_shortcut` — and never reach here.
            if viewer.write().close_picker() {
                return;
            }
            // Settings next, and above everything below it: it is a window
            // over the reader, and Escape inside a window means that window.
            // Below the menus because a menu opened *from* Settings is inside
            // it, which is the same "outward, in the order the reader arrived"
            // this whole list is.
            if viewer.write().close_settings() {
                return;
            }
            // And a note, which is a window of the same kind one line down:
            // it is over the reader, and Escape inside a window means that
            // window.
            if viewer.write().close_note() {
                return;
            }
            // The Information window, which is the note's neighbour in every
            // other respect and was missing from this list: Escape closed the
            // note beside it and left this one up. `showDocumentDetails` puts
            // up the same kind of window and the app's modal closes on Escape
            // whatever is in it.
            if viewer.write().close_details() {
                return;
            }
            // The Sign window, then a signature looking for somewhere to go.
            // Two states and the reader is in one of them: the window is over
            // the page, so Escape means the window; and a signature waiting to
            // be placed has no window, so Escape is the only way to put it down
            // without signing something.
            if viewer.write().close_signing() {
                return;
            }
            if viewer.write().put_down() {
                return;
            }
            // And the password window. Below the rest because a reader inside
            // it is inside a field, so this is only reached when the pointer has
            // taken the keyboard elsewhere. Withdrawing the question is not
            // answering it with an empty password.
            if viewer.write().stop_unlocking() {
                return;
            }
            let (typing, finding, selected, presenting, full) = {
                let held = viewer.read();
                (
                    held.typing_page,
                    held.find_open,
                    held.has_selection(),
                    held.presenting,
                    held.full_screen,
                )
            };
            if typing {
                viewer.write().cancel_page();
            } else if finding {
                viewer.write().close_find();
            } else if selected {
                // Between the find bar and presenting: a selection is a thing
                // on the page, where full screen and presenting are things the
                // window is doing. A reader presenting who has swept a sentence
                // to point at means to put the sentence down first.
                viewer.write().clear_selection();
            } else if presenting {
                let full = viewer.write().present(false);
                frame.ask(Ask::FullScreen(full));
            } else if full {
                viewer.write().set_full_screen(false);
                frame.ask(Ask::FullScreen(false));
            }
        }
        // The three a selection is for. `Copy` is this experiment's own —
        // see `keymap::EXTRA`: in the app ⌘C is the webview's, because the
        // browser owns the selection and therefore owns copying it.
        Action::SelectPage => {
            viewer.write().select_page();
        }
        Action::Copy => {
            let copied = viewer.read().selected_text();
            if copied.is_empty() {
                viewer.write().notice = "Select something first, and this copies it.".into();
            } else {
                clip.put(&copied);
                viewer.write().notice = "Copied.".into();
            }
        }
        Action::CopyQuote => {
            let quoted = viewer.read().quoted();
            let said = match quoted {
                Some((quote, where_from)) => {
                    clip.put(&quote);
                    format!("Copied, with {where_from}.")
                }
                None => "Select something first, and this copies it with its page number.".into(),
            };
            viewer.write().notice = said;
        }
        // The window's own three, which the page can only ask for. See
        // [`Frame`]: what answers is the shell in the app and a list in the
        // harness, and the reader's side is the same either way.
        // The picker, through `Pick` rather than the shell directly, because a
        // modal window belonging to the operating system is the one door a test
        // must not be able to open. A window of its own is a menu item and not
        // a key, there being no `open-new-window` in `keys.ts` either.
        Action::Open => pick.ask(Opening::Here),
        Action::NewWindow => frame.ask(Ask::NewWindow),
        Action::NewTab => frame.ask(Ask::NewTab),
        Action::CloseWindow => frame.ask(Ask::Close),
        Action::Quit => frame.ask(Ask::Quit),
        Action::Toolbar => viewer.write().toggle_toolbar(),
        Action::Fullscreen => {
            // While presenting the window is in full screen whatever the
            // reader's own switch says, so the key leaves it — and presenting,
            // which has nowhere else to be.
            if viewer.read().presenting {
                viewer.write().present(false);
                viewer.write().set_full_screen(false);
                frame.ask(Ask::FullScreen(false));
                return;
            }
            let on = !viewer.read().full_screen;
            viewer.write().set_full_screen(on);
            frame.ask(Ask::FullScreen(on));
        }
        Action::Present => {
            let on = !viewer.read().presenting;
            let full = viewer.write().present(on);
            frame.ask(Ask::FullScreen(full));
        }
        Action::Settings => viewer.write().open_settings(),
        // ⌘⇧H. The key opens the popover rather than marking in the last
        // colour used, which is the app's arrangement: a colour is a decision
        // and a keystroke that made one silently would be a keystroke nobody
        // could undo.
        Action::Markup => {
            if !viewer.write().open_markup() {
                let held = viewer.read();
                let nothing = held.text_on(held.page()).is_empty();
                drop(held);
                viewer.write().notice = if nothing {
                    "There is no text on this page to highlight.".into()
                } else {
                    "Select something first, and this highlights it.".into()
                };
            }
        }
        // One page across and back to the pair the reader chose last, which
        // the key used to answer with Cover whatever that was: somebody on
        // "Two side by side" pressed `s` twice and was on Cover.
        Action::Spread => {
            let next = if viewer.read().layout.spread == Spread::Single {
                viewer.read().last_pair
            } else {
                Spread::Single
            };
            viewer.write().set_spread(next);
        }
    }
}

/// One turn of the event loop, awaited: wake the task immediately and return
/// `Pending`, so the scheduler requeues it and whoever is driving the document
/// — the shell, or `pump()` in the harness — gets a turn first. Returning
/// `Ready` would keep the window out of the loop, which is `breathe()` in
/// `search.ts` avoiding the same thing with `setTimeout(resolve, 0)`.
struct Breathe(bool);

impl Breathe {
    fn once() -> Breathe {
        Breathe(false)
    }
}

impl std::future::Future for Breathe {
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        if self.0 {
            return std::task::Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        std::task::Poll::Pending
    }
}

/// The number the last page is printed with, when a document's labels are one
/// ascending run of numbers — and nothing, when they are anything else.
///
/// See [`Viewer::pages_text`], which is the only caller and says why.
fn straight_run(first: &str, last: &str, pages: usize) -> Option<String> {
    let first: usize = first.parse().ok()?;
    let last: usize = last.parse().ok()?;
    (last == first + pages.checked_sub(1)?).then(|| last.to_string())
}

#[cfg(test)]
mod counting {
    use super::straight_run;

    #[test]
    fn the_count_follows_the_numbers_printed_on_the_pages() {
        // An offprint: eighteen pages, printed 2669 to 2686.
        assert_eq!(
            straight_run("2669", "2686", 18).as_deref(),
            Some("2686"),
            "the pages are numbered straight through, from somewhere else"
        );
        // The ordinary document, which this changes nothing about.
        assert_eq!(straight_run("1", "400", 400).as_deref(), Some("400"));
        // Front matter in roman, then a body that starts again at 1: the
        // sixth page is called 3, and there is no last number to count to.
        assert_eq!(straight_run("i", "3", 6), None);
        // And a document whose labels are numbers but skip: 12 pages that end
        // at 30 are not a run.
        assert_eq!(straight_run("1", "30", 12), None);
    }
}

#[cfg(test)]
mod links {
    use super::openable;

    #[test]
    fn only_web_and_mail_addresses_are_opened() {
        assert_eq!(openable("HTTPS://a.org").as_deref(), Some("HTTPS://a.org"));
        assert_eq!(openable("www.a.org").as_deref(), Some("https://www.a.org"));
        assert_eq!(
            openable("doi:10.1/x").as_deref(),
            Some("https://doi.org/10.1/x")
        );
        assert_eq!(openable("file:///etc/passwd"), None);
        assert_eq!(openable("javascript:alert(1)"), None);
    }
}
