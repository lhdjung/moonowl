//! Where a window comes from, and what one gives back when it goes.
//!
//! This is `spawn_window` and the acting half of `hand_over` out of the app's
//! `lib.rs`. The *deciding* half is [`crate::windows`], which has no window in
//! it and is tested; this is the part that opens a file, builds a virtual DOM
//! and hands it to the shell, and it is the part a test cannot reach because
//! it makes a window.
//!
//! Three things about it are worth knowing before changing it.
//!
//! **A window is made for a document, never before one.** The app builds an
//! empty window and fills it, because it has a start screen to show in the
//! meantime; this reader has none — see item 7, "there is nowhere to show a
//! recently-read list in a reader that always has a document open" — so a
//! document that will not open produces no window at all rather than an empty
//! one, and ⌘N opens a second window on what the front one is reading.
//!
//! **Everything a window is told about is addressed to its label.** The label
//! goes into the [`Desk`], into [`crate::emit::Exchange`], and into
//! `watch.rs`'s per-window document watch, and those three are the whole of
//! what a window is to the rest of the process. Giving them back is
//! [`Session::tidy`], which is `tidy_after` under another name.
//!
//! **The reader's own `Store` is per window and is made inside it.** So two
//! windows have two copies of the settings table, and a setting changed in one
//! is not seen by the other until it is opened again — which is exactly what
//! `AGENTS.md` says about the app, and for the same reason. Themes are the
//! exception, because the watcher broadcasts them.

use std::path::PathBuf;
use std::sync::Arc;

use dioxus::prelude::VirtualDom;
use dioxus_core::{provide_context, ScopeId};
use dioxus_native::{LogicalSize, WindowAttributes};

use crate::app::{Config, Handle, Reader, ReaderProps};
use crate::emit::{Exchange, Post};
use crate::page::Chosen;
use crate::shell::{Remote, WindowSpec};
use crate::windows::{Desk, Handover};
use crate::{palette, render, store};

/// Everything a window needs that is the *process's* rather than its own.
pub struct Session {
    pub desk: Desk,
    pub exchange: Exchange,
    /// One watcher, shared. See the hook in `app.rs`: a watcher per window
    /// would be that many watches on one themes directory and that many
    /// copies of every theme reload.
    pub watching: Arc<crate::watch::Watching>,
    pub dir: PathBuf,
    /// `--theme N`, which every window this run makes wears.
    pub theme: Option<usize>,
    pub size: (f64, f64),
    /// Whether a window comes up filling the screen, which is the app's own
    /// default (`window_maximized`, true in `settings.rs`). It matters more
    /// than it sounds: the toolbar holds fourteen controls and a document
    /// title, and in a 1100-pixel window the title is squeezed to nothing
    /// while every group crowds its neighbour. The app opens at 1280×860
    /// *maximized*; a reader who has only seen this one in a small window has
    /// been reading a cramped copy of the same bar.
    pub maximized: bool,
    /// How a window in front is brought forward, which is the shell's to do
    /// and is asked for from here.
    pub remote: Remote,
}

impl Session {
    /// A window on this document, or nothing if it will not open.
    ///
    /// The claim on the document happens here, before the window exists, for
    /// the app's own reason: two files arriving in the same instant must not
    /// both be handed to the same window.
    pub fn window(&self, path: &str) -> Option<WindowSpec> {
        self.window_over(path, render::open(path))
    }

    /// The same, over what `render::open` already answered — for `main`,
    /// which opens the launch document before the shell exists so that it can
    /// say what it found, and must not pay to open it twice.
    pub fn window_over(
        &self,
        path: &str,
        opened: Result<Arc<dyn render::PageSource>, render::Refusal>,
    ) -> Option<WindowSpec> {
        self.window_on(Some(path), opened)
    }

    /// A window with nothing in it.
    ///
    /// **This is what ⌘N means now, and it is the app's own answer.** It used
    /// to be a second window on whatever the front one was reading, because
    /// there was nowhere to show a window with nothing in it — the note that
    /// stood here said so in as many words, and called it the cheaper of two
    /// answers. It was cheaper because the expensive one had not been built:
    /// there is a start screen now (see [`crate::app::Start`]), so an empty
    /// window is a window with the shelf in it, which is what somebody
    /// pressing ⌘N is usually after. Two places in one book at once is still
    /// one gesture — "Open in a new window…" under the document's own name,
    /// with the document already there.
    pub fn empty_window(&self) -> Option<WindowSpec> {
        self.window_on(None, Ok(render::nothing()))
    }

    /// One window, on a document or on nothing.
    ///
    /// The two used to be one function that could not answer the second
    /// question, and the difference between them turns out to be four lines:
    /// what to open, what to record on the desk, what to call the window and
    /// what to hand the component. Everything else — the label, the mailbox,
    /// the config, the contexts — is the same because it is about a window
    /// rather than about a document.
    fn window_on(
        &self,
        path: Option<&str>,
        opened: Result<Arc<dyn render::PageSource>, render::Refusal>,
    ) -> Option<WindowSpec> {
        // **A locked document makes a window rather than refusing one**, and
        // that is the whole of what the password prompt costs out here: the
        // window comes up empty with the question over it, because there is
        // nowhere else to ask. Every other refusal is still a line on the
        // terminal and no window at all — there is nothing a reader could do
        // about a file that is missing or is not a PDF.
        let (document, asking) = match (path, opened) {
            (_, Ok(document)) => (document, None),
            (Some(path), Err(render::Refusal::Locked)) => {
                (render::nothing(), Some(path.to_string()))
            }
            (_, Err(refused)) => {
                eprintln!("{refused}");
                return None;
            }
        };
        // …and until it is answered this window is showing *nothing*, which is
        // what it is: the desk, the restore list and the window's own title
        // are all about a document that has been opened. The answer comes back
        // through `Ask::Showing`, which is the one path that sets all three —
        // the same path ⌘O uses.
        let path = if asking.is_some() { None } else { path };
        let label = self.desk.name();
        self.desk.set(&label, path);
        // What the next launch comes back to, written as each window opens.
        let _ = crate::library::set_open(&self.dir, &self.desk.open());

        let post = Post::new();
        self.exchange.join(&label, post.clone());

        // The window wears the document's name, decided the way the toolbar
        // decides it — see `store::worth_calling`. It is settled here because
        // a window's title is an attribute given to the builder, and pdfium
        // answers at open, so there is nothing to gain by waiting.
        let called = match path {
            Some(path) => format!("{} — Moonowl", store::called(path, &document.title())),
            None => "Moonowl".to_string(),
        };
        let attributes = WindowAttributes::default()
            .with_title(called)
            .with_surface_size(LogicalSize::new(self.size.0, self.size.1))
            .with_maximized(self.maximized);

        let config = Config {
            dir: self.dir.clone(),
            theme: self.theme,
            watch: true,
            window: label.clone(),
        };
        let vdom = VirtualDom::new_with_props(
            Reader,
            ReaderProps {
                document: Handle(document),
                // Black on white until this window reads the theme, which
                // happens during its first render and before anything is
                // painted. See `main.rs`.
                chosen: Chosen::new(palette::FALLBACK),
                config,
                asking,
            },
        );
        let watching = self.watching.clone();
        let exchange = self.exchange.clone();
        vdom.in_scope(ScopeId::ROOT, move || {
            provide_context(post);
            provide_context(exchange);
            provide_context(watching);
        });
        Some(WindowSpec::new(label, vdom, attributes))
    }

    /// A window is showing a different document now, because the reader
    /// pressed ⌘O in it.
    ///
    /// The three things that belong to the process rather than the window, in
    /// the order `Session::window` does them for a new one: who is showing what,
    /// what the next launch comes back to, and which file this window's watch is
    /// following. The title is the window's own and is set elsewhere.
    ///
    /// The document's name is not asked for again — it was read when the reader
    /// opened it, and asking pdfium again means opening the file again.
    /// An empty path is a window showing *nothing*, the reader having put a
    /// document down. One of the three below depends on it: the restore list is
    /// read off the desk, so a document closed by hand is one the next launch
    /// does not put back — the distinction between a window closed and an app
    /// quit. The watch is dropped with it.
    pub fn showing(&self, label: &str, path: &str) {
        let showing = (!path.is_empty()).then_some(path);
        self.desk.set(label, showing);
        let _ = crate::library::set_open(&self.dir, &self.desk.open());
        self.watching.document(label, showing);
    }

    /// A document handed to us by the system — a second launch, "Open with",
    /// the command line — put where [`Desk::hand_over`] says it goes.
    ///
    /// `None` means no window is to be made, which covers both the document
    /// that is already open somewhere (that window comes forward instead) and
    /// the file that will not open.
    pub fn hand_over(&self, path: &str) -> Option<WindowSpec> {
        match self.desk.hand_over(path) {
            Handover::Front(label) => {
                self.remote.show(&label);
                None
            }
            // **The arm that was unreachable until there was a start screen.**
            // A window on the shelf is exactly the window a double-clicked
            // document should land in: nothing is displaced by filling a window
            // showing nothing.
            //
            // The document goes down the mailbox rather than into the window,
            // because a window is a component and news is the only way to reach
            // one. The window then does its own `Ask::Showing`, so everything is
            // set by the one path that sets it for ⌘O.
            Handover::Fill(label) => {
                self.exchange.post(crate::emit::News {
                    event: "open-document".into(),
                    target: Some(label.clone()),
                    payload: crate::emit::Payload::Text(path.to_string()),
                });
                self.remote.show(&label);
                None
            }
            // A tab of the window in front unless the reader asked for
            // windows. Read off the disk, because each window holds its own
            // copy of the settings and this is none of them.
            Handover::Spawn => self.window(path).map(|spec| {
                let tabs = crate::settings::load(&self.dir)
                    .get("open_in_tabs")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true);
                spec.tabbed(cfg!(target_os = "macos") && tabs)
            }),
        }
    }

    /// ⌘N, the Dock's "New Window", and a second launch with no document
    /// named: a window on whatever the front one is reading.
    ///
    /// **This used to be where the port stopped being a port**, and the
    /// reason was the start screen: in the app a new window is an empty one
    /// because there is something to show in an empty window, and here there
    /// was not, so ⌘N gave a second window on the document already in front of
    /// somebody. That is no longer a difference — see [`Self::empty_window`].
    pub fn another(&self) -> Option<WindowSpec> {
        self.empty_window()
    }

    /// What a window gives back when it goes: its entry in the library, its
    /// mailbox, and its place in the document watch.
    ///
    /// The write is the interesting one and the rule is [`Desk::closing`]'s:
    /// a window closed by the reader is a document they have finished with, a
    /// window closed because the app is quitting is not, and the last window
    /// cannot tell the two apart so it writes nothing.
    pub fn tidy(&self, label: &str) {
        self.watching.document(label, None);
        self.exchange.leave(label);
        if let Some(remaining) = self.desk.closing(label) {
            let _ = crate::library::set_open(&self.dir, &remaining);
        }
    }
}
