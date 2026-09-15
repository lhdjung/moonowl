//! A window shell for Dioxus Native, because `launch()` only makes one window.
//!
//! `DioxusNativeApplication::add_window` is public and does not do what its
//! name says: it pushes onto `BlitzApplication::pending_windows`, drained in
//! `can_create_surfaces()` and nowhere else, and the Dioxus half of the setup
//! (the contexts, `initial_build()`) happens only for the one window `launch`
//! created. A second window added that way comes up empty and stays empty.
//!
//! So the shell is ours. It owns `BlitzApplication` directly — its fields are
//! public — and does the per-window Dioxus setup itself. What `dioxus-native`
//! keeps private (the net provider for `dioxus://` assets, the navigation
//! provider) is small enough to restate; see `nav.rs`.
//!
//! A window can only be created from inside a winit callback, because
//! `event_loop.create_window` wants the `&dyn ActiveEventLoop` only a callback
//! has. So asking for a window is a `WindowSpec` on a queue plus a wake-up
//! through the shell proxy.
//!
//! **Everything a window is asked to do arrives as one of those events, even
//! what could be done on the spot.** Closing a window and putting it in full
//! screen are reached from a Dioxus event handler, which runs inside a borrow
//! of the document *and* the shell's borrow of the window map — taking the
//! window out of that map from in there cannot be written. So the ask is
//! posted and answered on the next turn. It costs a frame nobody can see and
//! makes every window verb one shape.
//!
//! What this file deliberately does not know is what a window is *for*: no
//! document, no library, no settings. A window is a virtual DOM, a label and a
//! place, and what happens when one goes is a closure somebody else set — see
//! [`Shell::on_close`]. The bookkeeping is `windows.rs`, the wiring `main.rs`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::Receiver;

use blitz_shell::{BlitzApplication, BlitzShellEvent, BlitzShellProxy, View, WindowConfig};
use dioxus_core::{provide_context, ScopeId, VirtualDom};
use dioxus_native::{DioxusDocument, DocumentConfig};

use crate::steady::Steady;
use winit::application::ApplicationHandler;
use winit::dpi::{Position, Size};
use winit::event::{ElementState, StartCause, TouchPhase, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::{WindowAttributes, WindowId};

/// What a window is made of, before it has one.
pub struct WindowSpec {
    /// What this window is called: `main`, then `reader-1`. The name is the
    /// shell's only interest in it — it is handed back to [`Shell::on_close`]
    /// and given to the window's own [`crate::app::Frame`] — and everything
    /// that name means is `windows.rs`'s.
    pub label: String,
    pub attributes: WindowAttributes,
    /// Where the window should be, applied *after* it is created as well as
    /// through the attributes.
    ///
    /// The app this is a spike for has a comment about exactly this: on macOS
    /// a window given a position by the builder is moved onto the launch
    /// window's frame when it is shown, so every window lands on top of every
    /// other. `Placements` in `lib.rs` puts it back. Whether winit has the
    /// same fault is one of the questions this spike answers, so the position
    /// is set twice and the second time is reported.
    pub position: Option<Position>,
    /// Whether this window is to join the front one's tab group rather than
    /// stand beside it. macOS only, and asked for rather than inferred — see
    /// `tabs.rs`.
    pub tab: bool,
    pub vdom: VirtualDom,
}

impl WindowSpec {
    pub fn new(label: impl Into<String>, vdom: VirtualDom, attributes: WindowAttributes) -> Self {
        Self {
            label: label.into(),
            attributes,
            position: None,
            tab: false,
            vdom,
        }
    }

    pub fn at(mut self, position: impl Into<Position>) -> Self {
        self.position = Some(position.into());
        self
    }

    pub fn tabbed(mut self, tab: bool) -> Self {
        self.tab = tab;
        self
    }
}

/// The door a component opens a window through. Available as a context.
#[derive(Clone)]
pub struct Windows {
    queue: Rc<RefCell<Vec<WindowSpec>>>,
    proxy: BlitzShellProxy,
}

/// The wake-up. It carries nothing: the spec is in the queue, because a
/// `VirtualDom` cannot cross a channel that wants `Send + Sync`.
struct Spawn;

/// "Make a window, on this document" — the shape `hand_over` has in the real
/// app, where a document arrives from somewhere with no window of its own to
/// describe. A path and nothing else, so it can be sent from any thread: the
/// Dock menu's item and the single-instance listener are both on one.
///
/// `None` means "a window, and you choose the document", which is what ⌘N is
/// in a reader with no start screen — see [`crate::windows::Desk::hand_over`].
///
/// The flag is whether it is to be a *tab* of the window in front rather than
/// a window of its own. It is asked for by name and never guessed: macOS will
/// tab a new window on its own — see `tabs.rs` — and this reader turns that
/// off, because ⌘N is a window.
struct Wanted(Option<String>, bool);

/// This window, closed, from inside one of its own event handlers.
struct CloseOne(WindowId);

/// This window is showing a different document now — the path and the name.
///
/// Deferred through the proxy like every other ask, and for the ordinary
/// reason: it arrives from a Dioxus handler, inside a borrow of the very
/// window it is about. What answers it is [`Shell::on_swap`], because the
/// shell does not know what a document is.
struct Swapped(WindowId, String, String);

/// Bring the window with this name forward — what a document that is already
/// open answers with, rather than opening a second copy of itself. See
/// [`crate::windows::Handover::Front`].
struct Show(String);

/// The nth tab of this window's group, brought to the front. One-based, as
/// ⌘1 is. macOS alone has tabs, so everywhere else this is a no-op.
struct SelectTab(WindowId, usize);

/// This window, in or out of full screen. Deferred for the reason above: the
/// ask comes from a Dioxus handler, and on macOS the answer is an animation
/// the window is in the middle of being borrowed for.
struct FullScreen(WindowId, bool);

/// Print this window's document — the path — through the system: a sheet
/// on the window on macOS, the print dialog and a GDI job on Windows. See
/// `print.rs`.
#[cfg(any(target_os = "macos", target_os = "windows"))]
struct Print(WindowId, String);

/// Ask every window to close and the app to end.
struct Quit;

impl Windows {
    pub fn open(&self, spec: WindowSpec) {
        self.queue.borrow_mut().push(spec);
        self.proxy
            .send_event(BlitzShellEvent::embedder_event(Spawn));
    }

    /// Ask the shell's own factory for the next window, on this document.
    /// Unlike `open`, this needs nothing that is `!Send`, so it can be sent
    /// from another thread — which is what the Dock menu item does, and what
    /// a second launch of the app does through the single-instance socket.
    pub fn request(&self, path: Option<String>) {
        self.proxy
            .send_event(BlitzShellEvent::embedder_event(Wanted(path, false)));
    }

    /// A handle that can cross threads, carrying only the proxy.
    pub fn remote(&self) -> Remote {
        Remote {
            proxy: self.proxy.clone(),
        }
    }

    pub fn quit(&self) {
        self.proxy.send_event(BlitzShellEvent::embedder_event(Quit));
    }
}

/// The `Send` half of [`Windows`]: it can ask for a window and it can end the
/// app, and it cannot carry a `VirtualDom`.
#[derive(Clone)]
pub struct Remote {
    proxy: BlitzShellProxy,
}

impl Remote {
    pub fn request(&self, path: Option<String>) {
        self.proxy
            .send_event(BlitzShellEvent::embedder_event(Wanted(path, false)));
    }

    /// The same, as a tab of the window in front.
    pub fn request_tab(&self, path: Option<String>) {
        self.proxy
            .send_event(BlitzShellEvent::embedder_event(Wanted(path, true)));
    }

    /// Bring a window forward by name.
    pub fn show(&self, label: &str) {
        self.proxy
            .send_event(BlitzShellEvent::embedder_event(Show(label.to_string())));
    }

    pub fn quit(&self) {
        self.proxy.send_event(BlitzShellEvent::embedder_event(Quit));
    }
}

/// Where a window comes from, asked by path. `None` back means no window
/// after all — a document that will not open.
type Factory = Box<dyn FnMut(Option<String>) -> Option<WindowSpec>>;

/// What a window gives back when it goes, by name. See [`Shell::on_close`].
type Tidy = Box<dyn FnMut(&str)>;

/// A window's name and the document it has swapped to. See [`Shell::on_swap`].
type Swap = Box<dyn FnMut(&str, &str)>;

/// A window's name, said again every time the window changes size. See
/// [`Shell::on_resized`].
type Resized = Box<dyn FnMut(&str, f64, f64, bool)>;
/// That two fingers moved apart or together on a window, or lifted (`None`).
/// See [`Shell::on_pinch`].
type Pinched = Box<dyn FnMut(&str, Option<f64>)>;

/// A window's name, said again every time the machine goes light or dark. See
/// [`Shell::on_theme`].
type Themed = Box<dyn FnMut(&str)>;

/// A file dragged over a window, or let go over one. See [`Shell::on_drop`].
type Dropped = Box<dyn FnMut(&str, Drag)>;

/// What is happening with a file over a window.
///
/// The paths winit hands over are filtered to documents this reader can open
/// before they get here, so `Over(true)` is "this will be caught" and
/// `Over(false)` is "this will not" — which is the difference between a hint
/// that means something and a hint that appears for every drag across the
/// screen. `Drop` carries the one path that is going to be opened, because a
/// reader dropping four files on one window means one document and three they
/// will have to drop again; the app takes the first the same way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drag {
    /// Over the window, and whether there is a document in it.
    Over(bool),
    /// Gone, without anything being let go.
    Left,
    /// Let go, on this document.
    Drop(String),
    /// Let go, on something this reader will not open. A case of its own
    /// rather than [`Drag::Left`] with the hint taken down, because the reader
    /// did something and deserves to be told why nothing happened — "That is
    /// not a PDF" is the app's own sentence for it.
    Refused,
}

/// Whether a dragged path is something this reader would open.
///
/// By extension and nothing else. The alternative is opening the file to find
/// out, and this runs on the thread drawing the window every time a pointer
/// carrying a file crosses it.
pub fn is_document(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}

pub struct Shell {
    inner: BlitzApplication<Steady>,
    proxy: BlitzShellProxy,
    windows: Windows,
    /// Where a window comes from when nobody handed one over: one place that
    /// makes them, which is what `spawn_window` is today. It answers `None`
    /// when there is to be no window after all — a document that will not
    /// open, or a picker the reader closed.
    factory: Option<Factory>,
    /// The launch window, decided at the first `can_create_surfaces` rather
    /// than before the loop — the moment a Finder launch's documents are known.
    launch: Option<Box<dyn FnOnce() -> Option<WindowSpec>>>,
    /// What each window is called, so that a `WindowId` arriving from winit
    /// can be handed to [`Shell::on_close`] as a name.
    labels: std::collections::HashMap<WindowId, String>,
    /// What to do when a window goes: give back its place in the library, its
    /// mailbox and its document watch. The shell knows none of that and does
    /// not want to — see the module comment.
    tidy: Option<Tidy>,
    /// What a window has swapped its document for, by name. See
    /// [`Shell::on_swap`].
    swap: Option<Swap>,
    /// Which window has the keyboard, reported as winit says so.
    focus: Option<Box<dyn FnMut(Option<String>)>>,
    /// Which windows have drawn at least once.
    ///
    /// **So that a window which comes up with a field already in it gets the
    /// keyboard**, which is the password window and nothing else so far: every
    /// other field in this reader is opened by a key or a click, and both of
    /// those already hand the focus back afterwards. A window made *asking*
    /// for a password has had no event at all, so the field it exists for sat
    /// there unfocused until the reader touched something. Once per window
    /// rather than once per frame, because the query below walks the document
    /// and a scroll is the one path in this app that must not grow work.
    painted: std::collections::HashSet<WindowId>,
    /// That a window changed size. See [`Shell::on_resized`].
    resized: Option<Resized>,
    /// That two fingers moved apart or together on it.
    pinched: Option<Pinched>,
    /// That the machine went light or dark. See [`Shell::on_theme`].
    themed: Option<Themed>,
    /// That a document is being dragged over a window, or has been let go
    /// over one. See [`Shell::on_drop`].
    dropped: Option<Dropped>,
    /// Raised before the first window of a quit goes.
    leaving: Option<Box<dyn FnMut()>>,
    /// Whether winit has told us surfaces can be created yet. A window made
    /// before that is left for `BlitzApplication::can_create_surfaces` to
    /// bring up; one made after has to be brought up here, because nothing
    /// else will.
    ///
    /// Resuming a window *twice* is not harmless: `View::resume` builds a fresh
    /// renderer and orphans every resource a widget registered with the old
    /// one, so the next frame handing back a cached texture dies with "Tried to
    /// draw an invalid empty image … maybe it was registered to a different
    /// renderer". The widget's side of that is `destroy_surfaces` and
    /// `can_create_surfaces`.
    started: bool,
    /// Reported once per window, so that "where did it actually land" is
    /// answerable from the terminal rather than from a ruler on the screen.
    pub trace: bool,
}

impl Shell {
    pub fn new(proxy: BlitzShellProxy, event_queue: Receiver<BlitzShellEvent>) -> Self {
        Self {
            inner: BlitzApplication::new(proxy.clone(), event_queue),
            windows: Windows {
                queue: Rc::new(RefCell::new(Vec::new())),
                proxy: proxy.clone(),
            },
            proxy,
            factory: None,
            launch: None,
            labels: std::collections::HashMap::new(),
            tidy: None,
            swap: None,
            focus: None,
            painted: std::collections::HashSet::new(),
            resized: None,
            pinched: None,
            themed: None,
            dropped: None,
            leaving: None,
            started: false,
            trace: true,
        }
    }

    /// Say where a window comes from when one is asked for by path only.
    pub fn on_request(
        &mut self,
        factory: impl FnMut(Option<String>) -> Option<WindowSpec> + 'static,
    ) {
        self.factory = Some(Box::new(factory));
    }

    /// Say what the launch window is, asked once surfaces can be made.
    pub fn on_launch(&mut self, launch: impl FnOnce() -> Option<WindowSpec> + 'static) {
        self.launch = Some(Box::new(launch));
    }

    /// Say what a window has to give back when it goes.
    ///
    /// Called with the window's label, before the window is taken down —
    /// which is the app's `tidy_after` at one remove, and the remove is the
    /// point: there it is a Tauri window event handler and everything it
    /// touches is `State<'_, …>` off an `AppHandle`; here it is a closure and
    /// what it touches is `windows.rs`.
    pub fn on_close(&mut self, tidy: impl FnMut(&str) + 'static) {
        self.tidy = Some(Box::new(tidy));
    }

    /// Say what has to happen before the app goes: raising the flag that
    /// tells a window closing because of a quit from a window closed by the
    /// reader. See [`crate::windows::Desk::closing`].
    pub fn on_quit(&mut self, leaving: impl FnMut() + 'static) {
        self.leaving = Some(Box::new(leaving));
    }

    /// Say what happens when a window changes size.
    ///
    /// **Blitz answers a resize itself and tells nobody**, which a reader sees
    /// as two faults: `SurfaceResized` sets the document's viewport and asks
    /// for a redraw, so the *chrome* follows the window and the *document* does
    /// not. A window opened at 1100 and dragged wider leaves the pages laid out
    /// for 1100, so the page sits left of centre and Fit width fits a width the
    /// window no longer has.
    ///
    /// There is no `ResizeObserver` and `get_client_rect` cannot be called from
    /// inside an event, so the news comes through the window's mailbox like
    /// everything else. `main.rs` turns this into an emit.
    pub fn on_resized(&mut self, resized: impl FnMut(&str, f64, f64, bool) + 'static) {
        self.resized = Some(Box::new(resized));
    }

    /// Say what happens when two fingers pinch on a window.
    pub fn on_pinch(&mut self, pinched: impl FnMut(&str, Option<f64>) + 'static) {
        self.pinched = Some(Box::new(pinched));
    }

    /// **Backspace is a question the window has to have asked to be told the
    /// answer to**, and nothing in this app had asked it.
    ///
    /// See [`ApplicationHandlerExtMacOS`] below for the first half. What
    /// forwarding the callback did not fix is that **winit only reads a
    /// keystroke against the standard key bindings when IME is enabled on the
    /// window**: `key_down` calls `interpretKeyEvents` inside `if
    /// ime_capabilities.is_some()`, and that is the only thing that ever calls
    /// `doCommandBySelector:`.
    ///
    /// `blitz-dom` means to enable it — `Node::focus` asks the shell for IME
    /// when the focused node is a text input — but **it asks one moment too
    /// early**: a text input's editor is built by `create_text_editor` during
    /// *layout construction*, and this app focuses its fields from `onmounted`,
    /// which runs before the first layout. So `text_input_data()` is still
    /// `None`, the request is never made, and the focus never moves away and
    /// back to make it again.
    ///
    /// So the window is asked here instead, whenever the focus has landed on
    /// something being typed into. Nothing turns it off again: winit's
    /// `set_ime_allowed` returns early when IME is on, so `ImeRequest::Disable`
    /// does nothing and "enabled once" is the only reachable state.
    fn keep_ime_in_step(&mut self, window_id: WindowId) {
        use winit::window::{ImeCapabilities, ImeEnableRequest, ImeRequest, ImeRequestData};
        let Some(view) = self.inner.windows.get(&window_id) else {
            return;
        };
        let editing = {
            let doc = view.doc.inner();
            doc.get_focussed_node_id()
                .and_then(|id| doc.get_node(id))
                .and_then(|node| node.element_data())
                .is_some_and(|data| data.text_input_data().is_some())
        };
        if !editing {
            return;
        }
        if let Some(ask) = ImeEnableRequest::new(ImeCapabilities::new(), ImeRequestData::default())
        {
            // `Err(AlreadyEnabled)` is the ordinary answer from the second
            // keystroke onwards, and is nothing to report.
            let _ = view.window.request_ime_update(ImeRequest::Enable(ask));
        }
    }

    /// Say what happens when the machine goes light or dark.
    ///
    /// winit's `WindowEvent::ThemeChanged`, which arrives per window rather
    /// than per process — so every window's reader follows, which is the right
    /// shape: a setting changed in one window is not seen by another until it
    /// opens again.
    ///
    /// Like a resize, the event carries no answer worth carrying: it says there
    /// is a new one and the reader asks the window through
    /// [`crate::app::Appearance`], so one place answers the question and a
    /// harness answers it from one cell.
    pub fn on_theme(&mut self, themed: impl FnMut(&str) + 'static) {
        self.themed = Some(Box::new(themed));
    }

    /// Say what happens when a document is dragged onto a window.
    ///
    /// "Or drop a PDF anywhere in this window" is the start screen's last line
    /// and it is a promise. There is no webview and no DOM event here; winit
    /// reports it on the window, which is the right place, because what is
    /// dropped is a *file*.
    ///
    /// Three states rather than one, because the hint is half the gesture:
    /// [`Drag::Over`] carries what would be opened, so a folder or a `.txt` is
    /// turned away before it is let go rather than after.
    pub fn on_drop(&mut self, dropped: impl FnMut(&str, Drag) + 'static) {
        self.dropped = Some(Box::new(dropped));
    }

    /// Say what happens when a window opens a different document in itself.
    ///
    /// Two things outside the window have to hear about it and neither is the
    /// window's: the desk, which is where the restore list is read from, and
    /// the watch, which is following the file that was open a moment ago.
    /// Both belong to the process — see `session.rs` — which is why this is a
    /// closure here rather than something the reader does for itself.
    pub fn on_swap(&mut self, swapped: impl FnMut(&str, &str) + 'static) {
        self.swap = Some(Box::new(swapped));
    }

    /// Say where "which window is in front" should be written down.
    pub fn on_focus(&mut self, tell: impl FnMut(Option<String>) + 'static) {
        self.focus = Some(Box::new(tell));
    }

    /// Where every window is, in logical pixels — what a new one cascades
    /// past. See [`crate::windows::cascade`].
    pub fn corners(&self) -> Vec<(f64, f64)> {
        self.inner
            .windows
            .values()
            .filter_map(|view| {
                let scale = view.window.scale_factor();
                let at = view.window.outer_position().ok()?.to_logical::<f64>(scale);
                Some((at.x, at.y))
            })
            .collect()
    }

    /// The handle to hand out before the loop starts.
    pub fn windows(&self) -> Windows {
        self.windows.clone()
    }

    /// The window with the keyboard, else any window at all. What "in front"
    /// means to a tab looking for the group it is joining — which is macOS
    /// alone, hence the gate; the cascade below asks the same question of the
    /// corners rather than of the window.
    #[cfg(target_os = "macos")]
    fn front(&self) -> Option<std::sync::Arc<dyn winit::window::Window>> {
        self.inner
            .windows
            .values()
            .find(|view| view.window.has_focus())
            .or_else(|| self.inner.windows.values().next())
            .map(|view| std::sync::Arc::clone(&view.window))
    }

    /// One step down from the window in front, keeping its edges, and on
    /// again while the spot is taken. `None` when there is no window to step
    /// off, which is the first one.
    fn next_spot(&self) -> Option<(Position, Size)> {
        let frame = |view: &View<Steady>| {
            let scale = view.window.scale_factor();
            let at = view.window.outer_position().ok()?.to_logical::<f64>(scale);
            // The *surface* size, because that is what a window can be asked
            // to be — and the outer height less the step gave a surface that,
            // with its title bar back on top, was exactly the height it was.
            let size = view.window.surface_size().to_logical::<f64>(scale);
            Some((at.x, at.y, size.width, size.height))
        };
        let front = self
            .inner
            .windows
            .values()
            .find(|view| view.window.has_focus())
            .or_else(|| self.inner.windows.values().next())
            .and_then(frame);
        let (x, y, width, height) = crate::windows::cascade(front, &self.corners())?;
        Some((
            winit::dpi::LogicalPosition::new(x, y).into(),
            winit::dpi::LogicalSize::new(width, height).into(),
        ))
    }

    fn drain(&mut self, event_loop: &dyn ActiveEventLoop) {
        let specs: Vec<WindowSpec> = self.windows.queue.borrow_mut().drain(..).collect();
        for spec in specs {
            self.open(event_loop, spec);
        }
    }

    fn open(&mut self, event_loop: &dyn ActiveEventLoop, spec: WindowSpec) {
        // Which window a tab is joining, asked before this one is made.
        #[cfg(target_os = "macos")]
        let joining = if spec.tab { self.front() } else { None };
        // One renderer per window: `DioxusNativeWindowRenderer` is an
        // `Rc<RefCell<VelloWindowRenderer>>` over one surface, and a surface
        // belongs to one window. [`Steady`] is that renderer with the scene
        // reset at the top of every frame — see `steady.rs`, which is the
        // whole account of why.
        let renderer = Steady::new();

        let doc = DioxusDocument::new(
            spec.vdom,
            DocumentConfig {
                navigation_provider: Some(crate::nav::provider()),
                // Without this, `dangerous_inner_html` silently does nothing —
                // which is how every icon in the chrome spike came out blank
                // the first time. `dioxus_native::launch` passes it behind its
                // `html` feature; a shell that makes its own windows has to
                // pass it too.
                html_parser_provider: Some(std::sync::Arc::new(blitz_html::HtmlProvider)),
                ..Default::default()
            },
        );

        let config =
            WindowConfig::with_attributes(Box::new(doc) as _, renderer.clone(), spec.attributes);
        let mut view = View::init(config, event_loop, &self.proxy);
        self.labels.insert(view.window_id(), spec.label.clone());

        // The Dioxus half, which `BlitzApplication` knows nothing about.
        let windows = self.windows.clone();
        let winit_window = std::sync::Arc::clone(&view.window);
        let shell_provider = view.doc.inner().shell_provider.clone();
        // What `use_window()` was reached for and the only thing it was
        // reached for. See `Screen` in `app.rs`: a component that asks winit
        // how big it is cannot be built without winit, and the harness has no
        // window at all.
        let screen = {
            let window = std::sync::Arc::clone(&view.window);
            crate::app::Screen::new(move || {
                let size = window.surface_size();
                let scale = window.scale_factor();
                (size.width as f64 / scale, size.height as f64 / scale, scale)
            })
        };
        // What this window can be asked to do. Every one of them goes back
        // out through the proxy rather than being done here — see the module
        // comment: the ask arrives from inside a borrow of this very window.
        // What the machine says about light and dark. `None` where the
        // platform will not say, which winit allows and this reader answers
        // by leaving the theme alone. See [`crate::app::Appearance`].
        let appearance = {
            let window = std::sync::Arc::clone(&view.window);
            crate::app::Appearance::new(move || {
                window
                    .theme()
                    .map(|theme| theme == winit::window::Theme::Dark)
            })
        };
        let frame = {
            let proxy = self.proxy.clone();
            let id = view.window_id();
            crate::app::Frame::new(move |ask| {
                let event = match ask {
                    crate::app::Ask::NewWindow => {
                        BlitzShellEvent::embedder_event(Wanted(None, false))
                    }
                    crate::app::Ask::NewTab => BlitzShellEvent::embedder_event(Wanted(None, true)),
                    crate::app::Ask::SelectTab(at) => {
                        BlitzShellEvent::embedder_event(SelectTab(id, at))
                    }
                    crate::app::Ask::Close => BlitzShellEvent::embedder_event(CloseOne(id)),
                    crate::app::Ask::Quit => BlitzShellEvent::embedder_event(Quit),
                    crate::app::Ask::FullScreen(on) => {
                        BlitzShellEvent::embedder_event(FullScreen(id, on))
                    }
                    // The picker's two answers. A window of its own goes
                    // through the same door the Dock menu and a second launch
                    // use, so a document already open is brought forward
                    // rather than opened twice.
                    crate::app::Ask::NewWindowOn(path) => {
                        BlitzShellEvent::embedder_event(Wanted(Some(path), false))
                    }
                    crate::app::Ask::Showing { path, title } => {
                        BlitzShellEvent::embedder_event(Swapped(id, path, title))
                    }
                };
                proxy.send_event(event);
            })
        };
        // Printing, which wants the window — a sheet on it, or a dialog
        // owned by it — and so has to be answered by the shell. The reader's
        // own default hands the file to another program, which is what Linux
        // still does and what these fall back to.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let printer = {
            let proxy = self.proxy.clone();
            let id = view.window_id();
            crate::app::Printer::new(move |path| {
                let path = crate::app::Printer::present(path)?;
                proxy.send_event(BlitzShellEvent::embedder_event(Print(id, path)));
                Ok(())
            })
        };
        let doc = view.downcast_doc_mut::<DioxusDocument>();
        doc.vdom.in_scope(ScopeId::ROOT, move || {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            provide_context(printer);
            provide_context(renderer);
            provide_context(windows);
            provide_context(winit_window);
            provide_context(shell_provider);
            provide_context(screen);
            provide_context(appearance);
            provide_context(frame);
        });
        doc.initial_build();

        // Where it goes: what the spec asked for, else one step down from
        // the window in front of it, the same width and that much shorter
        // so its bottom edge stays on the screen. The cascade is
        // `windows::cascade` and the argument for it is there; what is here
        // is that the shell is the only thing that knows where the windows
        // actually are, and it knows *now* rather than when the spec was
        // made. In the app this is a `Placements` map applied after the
        // window is shown, because showing it on macOS moves it onto the
        // launch window's frame — here the window is made, placed and drawn
        // in one turn and nothing is seen in between.
        // …unless it is a tab, which has no frame of its own to place: the
        // window it joins owns the frame and the cascade would be a window
        // stepping off itself.
        let position = if spec.tab {
            None
        } else if let Some(position) = spec.position {
            Some(position)
        } else if let Some((position, size)) = self.next_spot() {
            let _ = view.window.request_surface_size(size);
            Some(position)
        } else {
            None
        };
        if let Some(position) = position {
            let before = view.window.outer_position().ok();
            view.window.set_outer_position(position);
            if self.trace {
                let after = view.window.outer_position().ok();
                eprintln!(
                    "shell: window {:?} asked for {:?}; before {:?}, after {:?}",
                    view.window_id(),
                    position,
                    before,
                    after
                );
            }
        }

        let id = view.window_id();
        self.inner.windows.insert(id, view);
        // The tab is joined once the window exists and before anything else
        // happens to it. `joining` was read before it did, because "the
        // window in front" stops meaning what it meant the moment there is a
        // new one. See `tabs.rs`.
        #[cfg(target_os = "macos")]
        if let (Some(front), Some(view)) = (joining, self.inner.windows.get(&id)) {
            crate::tabs::tab_onto(front.as_ref(), view.window.as_ref());
        }
        if self.started {
            // A window made after the first `can_create_surfaces` has to be
            // resumed here; the renderer answers with `ResumeReady`, which
            // `BlitzApplication` turns into `complete_resume` once the view is
            // in its map — which is why the insert above happens either way.
            if let Some(view) = self.inner.windows.get_mut(&id) {
                view.resume();
                view.request_redraw();
            }
        }
    }
}

/// **Backspace is not a key on a Mac.**
///
/// AppKit does not deliver the editing keys as keystrokes. It reads them
/// against the standard key bindings and calls `doCommandBySelector:` with a
/// name — `deleteBackward:`, `moveToBeginningOfLine:` — which winit surfaces as
/// [`ApplicationHandlerExtMacOS::standard_key_binding`], a *separate* callback
/// from `window_event`. `blitz-dom` knows this: its `Key::Backspace` arm is
/// `#[cfg(not(target_os = "macos"))]`.
///
/// `BlitzApplication` implements the callback and returns itself from
/// `macos_handler`; a shell that wraps it and implements `ApplicationHandler`
/// itself answers `None` by default and drops every one of those commands. What
/// that looked like: every text field in the app was write-only.
#[cfg(target_os = "macos")]
impl winit::platform::macos::ApplicationHandlerExtMacOS for Shell {
    fn standard_key_binding(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        action: &str,
    ) {
        self.inner
            .standard_key_binding(event_loop, window_id, action);
    }
}

impl ApplicationHandler for Shell {
    /// See [`ApplicationHandlerExtMacOS`] above: without this, winit answers
    /// `None` and every editing key on a Mac is lost.
    #[cfg(target_os = "macos")]
    fn macos_handler(
        &mut self,
    ) -> Option<&mut dyn winit::platform::macos::ApplicationHandlerExtMacOS> {
        Some(self)
    }

    /// Where a window is actually born. Winit calls this once the platform can
    /// make surfaces, and again after a `destroy_surfaces`; everything queued
    /// so far is brought up by `BlitzApplication`, which resumes every window
    /// it holds.
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Winit calls this more than once — on macOS it comes again after the
        // window is on screen — and `BlitzApplication::can_create_surfaces`
        // resumes *every* window it holds each time. A second resume builds a
        // second renderer, and the first one's registered textures are still
        // in the window renderer's map: the next frame draws an image whose
        // override belongs to a renderer that no longer exists, which Vello
        // reports as "tried to draw an invalid empty image ... maybe it was
        // registered to a different renderer". It is a panic, it lands three
        // frames into every run, and the trail back to here is not short. So
        // the first call is the one that resumes; after it, a new window
        // resumes itself in `open`.
        if let Some(spec) = self.launch.take().and_then(|launch| launch()) {
            self.windows.queue.borrow_mut().push(spec);
        }
        self.drain(event_loop);
        if !self.started {
            // **⌘N is a window, and macOS had been making it a tab.**
            // `allowsAutomaticWindowTabbing` is on by default, and Apple's own
            // default for *Prefer tabs when opening documents* is "In Full
            // Screen" — so a reader in full screen who asked for a second
            // window got a second tab, with no way to ask for the other
            // thing. Off, a tab is something asked for by name and nothing
            // else; explicit tabbing goes on working, which is what `tabs.rs`
            // uses. The call has to be made from inside the loop because that
            // is where an `ActiveEventLoop` exists.
            #[cfg(target_os = "macos")]
            {
                use winit::platform::macos::ActiveEventLoopExtMacOS;
                event_loop.set_allows_automatic_window_tabbing(false);
            }
            self.inner.can_create_surfaces(event_loop);
            self.started = true;
        }
    }

    fn destroy_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.started = false;
        self.inner.destroy_surfaces(event_loop);
    }

    fn resumed(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.resumed(event_loop);
    }

    fn suspended(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.suspended(event_loop);
    }

    fn new_events(&mut self, event_loop: &dyn ActiveEventLoop, cause: StartCause) {
        self.inner.new_events(event_loop, cause);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::CloseRequested) {
            if self.trace {
                eprintln!(
                    "shell: closing {:?}, {} open",
                    window_id,
                    self.inner.windows.len()
                );
            }
            // Before `BlitzApplication` drops the window, because everything
            // this gives back is asked *of* the window: what it was showing,
            // and the document it was having watched. Afterwards there is a
            // `WindowId` and nothing to look it up in.
            if let Some(label) = self.labels.remove(&window_id) {
                if let Some(tidy) = self.tidy.as_mut() {
                    tidy(&label);
                }
            }
        }
        // Which window has the keyboard, which is what a new window cascades
        // off and what a handed-over document prefers. winit is the only
        // thing that knows, and it says so exactly once per change.
        if let WindowEvent::Focused(gained) = event {
            if let Some(tell) = self.focus.as_mut() {
                let label = gained
                    .then(|| self.labels.get(&window_id).cloned())
                    .flatten();
                tell(label);
            }
        }
        // A click clears the focus off the page, after which every shortcut
        // goes to `<html>`. Giving it back belongs to whoever owns the window,
        // because the one call that asks for the focus panics from inside an
        // event handler — see `app::KEYBOARD`.
        //
        // A key as well as a click, because a key can take the focused node away
        // with it: Escape closes the find bar, the field stops existing, and the
        // focus goes with it.
        let moved_focus = matches!(
            event,
            WindowEvent::PointerButton {
                state: ElementState::Released,
                ..
            } | WindowEvent::KeyboardInput { .. }
        );
        // A resize, which Blitz answers for the chrome and nobody answers for
        // the document. See [`Shell::on_resized`]. The scale factor counts as
        // one: a window dragged to a screen of a different density is the same
        // number of CSS pixels changing under the same layout.
        let resized = matches!(
            event,
            WindowEvent::SurfaceResized(_) | WindowEvent::ScaleFactorChanged { .. }
        );
        // …and a two-finger pinch, which is how a trackpad asks to zoom and is
        // not a wheel: macOS reports it as a gesture of its own, so an
        // application listening only for ⌃-wheel hears the opening of the
        // gesture at best and usually nothing at all. Blitz has no DOM event
        // for it, so it goes down the mailbox like the resize.
        // The end of the gesture is news of its own: two fingers resting on
        // the trackpad send nothing, and only the phase tells a pause from
        // the fingers lifting. See `Viewer::end_pinch`.
        let pinched = match event {
            WindowEvent::PinchGesture {
                phase: TouchPhase::Ended | TouchPhase::Cancelled,
                ..
            } => Some(None),
            WindowEvent::PinchGesture { delta, .. } if delta.is_finite() && delta != 0.0 => {
                Some(Some(delta))
            }
            _ => None,
        };
        // …and the machine going light or dark, which nothing else in this
        // process hears. See [`Shell::on_theme`].
        let themed = matches!(event, WindowEvent::ThemeChanged(_));
        // …and a document being dragged onto the window. Read *before* the
        // event is handed on, like the two above it, because `window_event`
        // takes the event by value. `DragMoved` is deliberately not answered:
        // it fires for every pixel the pointer travels and says nothing that
        // `DragEntered` has not already said, so answering it would be one
        // emit per frame for the whole of a drag.
        let dragging = match event {
            WindowEvent::DragEntered { ref paths, .. } => {
                Some(Drag::Over(paths.iter().any(|path| is_document(path))))
            }
            WindowEvent::DragLeft { .. } => Some(Drag::Left),
            WindowEvent::DragDropped { ref paths, .. } => paths
                .iter()
                .find(|path| is_document(path))
                .map(|path| Drag::Drop(path.to_string_lossy().into_owned()))
                .or(Some(Drag::Refused)),
            _ => None,
        };
        // …and the first frame a window draws, which is the one moment a field
        // can already be on screen without anything having happened. See
        // [`Shell::painted`]. Asked before the event is handed on, because
        // that consumes it.
        let first_paint =
            matches!(event, WindowEvent::RedrawRequested) && self.painted.insert(window_id);
        self.inner.window_event(event_loop, window_id, event);
        if resized {
            // The size goes with the news, because the one thing that wants
            // it outside this file is the setting that remembers it — and
            // `main.rs`, which is where that is written, has no window to
            // ask. Logical, not physical: a setting written in device pixels
            // comes back at half the size on the next screen.
            let geometry = self.inner.windows.get(&window_id).map(|view| {
                let scale = view.window.scale_factor();
                let size = view.window.surface_size();
                (
                    size.width as f64 / scale,
                    size.height as f64 / scale,
                    view.window.is_maximized(),
                )
            });
            if let (Some(label), Some((width, height, maximized)), Some(tell)) = (
                self.labels.get(&window_id).cloned(),
                geometry,
                self.resized.as_mut(),
            ) {
                tell(&label, width, height, maximized);
            }
        }
        if let Some(delta) = pinched {
            if let (Some(label), Some(tell)) =
                (self.labels.get(&window_id).cloned(), self.pinched.as_mut())
            {
                tell(&label, delta);
            }
        }
        if themed {
            if let (Some(label), Some(tell)) =
                (self.labels.get(&window_id).cloned(), self.themed.as_mut())
            {
                tell(&label);
            }
        }
        if let Some(drag) = dragging {
            if let (Some(label), Some(tell)) =
                (self.labels.get(&window_id).cloned(), self.dropped.as_mut())
            {
                tell(&label, drag);
            }
        }
        if moved_focus || first_paint {
            if let Some(view) = self.inner.windows.get_mut(&window_id) {
                crate::app::give_keyboard_back(&mut view.doc.inner_mut());
                view.request_redraw();
            }
        }
        // Asked on every event rather than only where the focus moved: a
        // field's editor is not built until the layout after it is mounted,
        // and a layout happens on a redraw rather than on anything this sees
        // go past. See [`Shell::keep_ime_in_step`], which does nothing at all
        // unless something is being typed into.
        self.keep_ime_in_step(window_id);
    }

    /// Blitz's events arrive on a channel now and the proxy only says "there
    /// is something on it", so the shell drains the same queue the
    /// application would and takes its own three events out of it.
    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        while let Ok(event) = self.inner.event_queue.try_recv() {
            if let BlitzShellEvent::Embedder(ref payload) = event {
                if payload.downcast_ref::<Spawn>().is_some() {
                    self.drain(event_loop);
                    continue;
                }
                if let Some(wanted) = payload.downcast_ref::<Wanted>() {
                    // Taken out and put back rather than borrowed, because the
                    // factory is `FnMut` and what it does is make a window
                    // through this same shell.
                    if let Some(mut factory) = self.factory.take() {
                        let spec = factory(wanted.0.clone());
                        self.factory = Some(factory);
                        if let Some(spec) = spec {
                            self.open(event_loop, spec.tabbed(wanted.1));
                        }
                    } else {
                        eprintln!("shell: asked for a window with no factory set");
                    }
                    continue;
                }
                if let Some(Show(label)) = payload.downcast_ref::<Show>() {
                    let id = self
                        .labels
                        .iter()
                        .find(|(_, known)| *known == label)
                        .map(|(id, _)| *id);
                    if let Some(view) = id.and_then(|id| self.inner.windows.get(&id)) {
                        view.window.set_minimized(false);
                        view.window.focus_window();
                    }
                    continue;
                }
                if let Some(Swapped(id, path, title)) = payload.downcast_ref::<Swapped>() {
                    // The window wears the document's name, spelled the way
                    // `Session::window` spells it for a window that was born
                    // on one.
                    if let Some(view) = self.inner.windows.get(id) {
                        // …and wears the application's own name when it is
                        // showing nothing, rather than " — Moonowl" with a
                        // hole where a document used to be.
                        view.window.set_title(&if title.is_empty() {
                            "Moonowl".to_string()
                        } else {
                            format!("{title} — Moonowl")
                        });
                    }
                    if let (Some(label), Some(swap)) =
                        (self.labels.get(id).cloned(), self.swap.as_mut())
                    {
                        swap(&label, path);
                    }
                    continue;
                }
                if let Some(CloseOne(id)) = payload.downcast_ref::<CloseOne>() {
                    // The same path a click on the close button takes, rather
                    // than a second way of closing a window: everything that
                    // has to be given back is hung off `CloseRequested`.
                    self.window_event(event_loop, *id, WindowEvent::CloseRequested);
                    continue;
                }
                if let Some(SelectTab(id, at)) = payload.downcast_ref::<SelectTab>() {
                    // One-based coming in, because ⌘1 is the first tab, and
                    // winit's own call is zero-based. Out of range is a no-op
                    // there, which is the right answer for ⌘7 with three tabs.
                    #[cfg(target_os = "macos")]
                    if let Some(view) = self.inner.windows.get(id) {
                        use winit::platform::macos::WindowExtMacOS;
                        view.window.select_tab_at_index(at.saturating_sub(1));
                    }
                    #[cfg(not(target_os = "macos"))]
                    let _ = (id, at);
                    continue;
                }
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                if let Some(Print(id, path)) = payload.downcast_ref::<Print>() {
                    if let Some(view) = self.inner.windows.get(id) {
                        // The hand-off is the fallback, not the answer: a
                        // document the system will not print is still one
                        // somebody wants on paper.
                        #[cfg(target_os = "macos")]
                        if let Err(said) = crate::print::sheet(view.window.as_ref(), path) {
                            eprintln!("print: {said}");
                            let _ = crate::app::Printer::to_the_system().print(path);
                        }
                        // Modal dialog, then the job: both block, so neither
                        // runs on the event loop.
                        #[cfg(target_os = "windows")]
                        {
                            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                            let hwnd = match view.window.window_handle().map(|h| h.as_raw()) {
                                Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
                                _ => 0,
                            };
                            let path = path.clone();
                            let name = std::path::Path::new(&path)
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| path.clone());
                            std::thread::spawn(move || {
                                if let Err(said) = crate::print::dialog(hwnd, &path, &name) {
                                    eprintln!("print: {said}");
                                    let _ = crate::app::Printer::to_the_system().print(&path);
                                }
                            });
                        }
                    }
                    continue;
                }
                if let Some(FullScreen(id, on)) = payload.downcast_ref::<FullScreen>() {
                    if let Some(view) = self.inner.windows.get(id) {
                        view.window.set_fullscreen(
                            on.then_some(winit::monitor::Fullscreen::Borderless(None)),
                        );
                    }
                    continue;
                }
                if payload.downcast_ref::<Quit>().is_some() {
                    if let Some(leaving) = self.leaving.as_mut() {
                        leaving();
                    }
                    // Every window closed through the door a window closes
                    // through, so that each of them gives back what it holds
                    // — and whoever raised the flag that says this is a quit
                    // has already done so, which is what stops the session
                    // being forgotten on the way out. See
                    // [`crate::windows::Desk::closing`].
                    let ids: Vec<WindowId> = self.inner.windows.keys().copied().collect();
                    for id in ids {
                        self.window_event(event_loop, id, WindowEvent::CloseRequested);
                    }
                    self.inner.windows.clear();
                    event_loop.exit();
                    continue;
                }
            }
            self.inner.handle_blitz_shell_event(event_loop, event);
        }
        // A field can arrive without a window event to announce it: ⌘F is a
        // keystroke, the bar it opens is rendered on the poll that follows,
        // and the field's editor is not built until the layout after that. So
        // the question is asked again once the queue is drained, and every
        // window is asked because a poll names none. See
        // [`Shell::keep_ime_in_step`].
        let ids: Vec<WindowId> = self.inner.windows.keys().copied().collect();
        for id in ids {
            self.keep_ime_in_step(id);
        }
    }
}
