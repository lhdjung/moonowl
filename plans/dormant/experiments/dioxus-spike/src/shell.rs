//! A window shell for Dioxus Native, because `launch()` only makes one window.
//!
//! `dioxus_native::launch` builds a `DioxusNativeApplication` around a single
//! `WindowConfig` and hands it to winit. `DioxusNativeApplication::add_window`
//! exists and is public, but it pushes onto `BlitzApplication::pending_windows`,
//! which is drained in `can_create_surfaces()` and nowhere else — and the
//! Dioxus half of the setup (the renderer and window contexts, and
//! `initial_build()`) happens only for the *one* window `launch` created. A
//! second window added that way comes up empty and stays empty.
//!
//! So the shell is ours. It owns `BlitzApplication` directly — its fields are
//! public — and does the per-window Dioxus setup itself. Everything a window
//! needs that `dioxus-native` keeps private (the net provider for `dioxus://`
//! assets, the navigation provider that opens a link in a browser) is small
//! enough to restate; see `nav.rs`.
//!
//! A window can only be created from inside a winit callback, because
//! `event_loop.create_window` wants the `&dyn ActiveEventLoop` that only a
//! callback has. So asking for a window is two things: a `WindowSpec` pushed
//! onto a queue, and a wake-up sent through the shell proxy. The spec carries
//! a `VirtualDom`, which is `!Send`, and the event carries nothing.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::Receiver;

use blitz_shell::{BlitzApplication, BlitzShellEvent, BlitzShellProxy, View, WindowConfig};
use dioxus_core::{provide_context, ScopeId, VirtualDom};
use dioxus_native::{DioxusDocument, DioxusNativeWindowRenderer, DocumentConfig};
use winit::application::ApplicationHandler;
use winit::dpi::Position;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::{WindowAttributes, WindowId};

/// What a window is made of, before it has one.
pub struct WindowSpec {
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
    pub vdom: VirtualDom,
}

impl WindowSpec {
    pub fn new(vdom: VirtualDom, attributes: WindowAttributes) -> Self {
        Self {
            attributes,
            position: None,
            vdom,
        }
    }

    pub fn at(mut self, position: impl Into<Position>) -> Self {
        self.position = Some(position.into());
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

/// "Make whatever window comes next" — the shape `hand_over` has in the real
/// app, where a double-clicked document arrives from somewhere with no window
/// of its own to describe. Carries nothing, so it can be sent from any thread.
struct Ask;

/// Ask every window to close and the app to end.
struct Quit;

impl Windows {
    pub fn open(&self, spec: WindowSpec) {
        self.queue.borrow_mut().push(spec);
        self.proxy.send_event(BlitzShellEvent::embedder_event(Spawn));
    }

    /// Ask the shell's own factory for the next window. Unlike `open`, this
    /// needs nothing that is `!Send`, so it can be sent from another thread —
    /// which is what the Dock menu item does today.
    pub fn request(&self) {
        self.proxy.send_event(BlitzShellEvent::embedder_event(Ask));
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
    pub fn request(&self) {
        self.proxy.send_event(BlitzShellEvent::embedder_event(Ask));
    }

    pub fn quit(&self) {
        self.proxy.send_event(BlitzShellEvent::embedder_event(Quit));
    }
}

pub struct Shell {
    inner: BlitzApplication<DioxusNativeWindowRenderer>,
    proxy: BlitzShellProxy,
    windows: Windows,
    /// Where a window comes from when nobody handed one over: one place that
    /// makes them, which is what `spawn_window` is today.
    factory: Option<Box<dyn FnMut() -> WindowSpec>>,
    /// Whether winit has told us surfaces can be created yet. A window made
    /// before that is left for `BlitzApplication::can_create_surfaces` to
    /// bring up; one made after has to be brought up here, because nothing
    /// else will.
    ///
    /// Resuming a window *twice* is not harmless and cost an afternoon:
    /// `View::resume` builds a fresh renderer, and every resource a widget
    /// registered with the old one is orphaned — the next frame that hands
    /// back a cached texture dies with "Tried to draw an invalid empty image
    /// ... maybe it was registered to a different renderer". Blitz's own
    /// `Widget` trait now has the honest answer to this on the widget's side:
    /// `destroy_surfaces` is where a cached texture is dropped, and
    /// `can_create_surfaces` is where it comes back.
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
            started: false,
            trace: true,
        }
    }

    /// Say where a window comes from when one is asked for by name only.
    pub fn on_request(&mut self, factory: impl FnMut() -> WindowSpec + 'static) {
        self.factory = Some(Box::new(factory));
    }

    /// The handle to hand out before the loop starts.
    pub fn windows(&self) -> Windows {
        self.windows.clone()
    }

    fn drain(&mut self, event_loop: &dyn ActiveEventLoop) {
        let specs: Vec<WindowSpec> = self.windows.queue.borrow_mut().drain(..).collect();
        for spec in specs {
            self.open(event_loop, spec);
        }
    }

    fn open(&mut self, event_loop: &dyn ActiveEventLoop, spec: WindowSpec) {
        // One renderer per window: `DioxusNativeWindowRenderer` is an
        // `Rc<RefCell<VelloWindowRenderer>>` over one surface, and a surface
        // belongs to one window.
        let renderer = DioxusNativeWindowRenderer::new();

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

        // The Dioxus half, which `BlitzApplication` knows nothing about.
        let windows = self.windows.clone();
        let winit_window = std::sync::Arc::clone(&view.window);
        let shell_provider = view.doc.inner().shell_provider.clone();
        let doc = view.downcast_doc_mut::<DioxusDocument>();
        doc.vdom.in_scope(ScopeId::ROOT, move || {
            provide_context(renderer);
            provide_context(windows);
            provide_context(winit_window);
            provide_context(shell_provider);
        });
        doc.initial_build();

        if self.started {
            // A window made after the first `can_create_surfaces` has to be
            // resumed here; the renderer answers with `ResumeReady`, which
            // `BlitzApplication` turns into `complete_resume` once the view is
            // in its map — which is why the insert below happens either way.
            view.resume();
        }

        if let Some(position) = spec.position {
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

        view.request_redraw();
        self.inner.windows.insert(view.window_id(), view);
    }
}

impl ApplicationHandler for Shell {
    /// Where a window is actually born. Winit calls this once the platform can
    /// make surfaces, and again after a `destroy_surfaces`; everything queued
    /// so far is brought up by `BlitzApplication`, which resumes every window
    /// it holds.
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.drain(event_loop);
        self.inner.can_create_surfaces(event_loop);
        self.started = true;
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
        if self.trace && matches!(event, WindowEvent::CloseRequested) {
            eprintln!(
                "shell: closing {:?}, {} open",
                window_id,
                self.inner.windows.len()
            );
        }
        self.inner.window_event(event_loop, window_id, event);
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
                if payload.downcast_ref::<Ask>().is_some() {
                    if let Some(mut factory) = self.factory.take() {
                        let spec = factory();
                        self.factory = Some(factory);
                        self.open(event_loop, spec);
                    } else {
                        eprintln!("shell: asked for a window with no factory set");
                    }
                    continue;
                }
                if payload.downcast_ref::<Quit>().is_some() {
                    self.inner.windows.clear();
                    event_loop.exit();
                    continue;
                }
            }
            self.inner.handle_blitz_shell_event(event_loop, event);
        }
    }
}
