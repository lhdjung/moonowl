//! Moonowl: a reader that reads, drawn by Blitz rather than by a webview.
//!
//! This crate was `experiments/dioxus-reader` until it took the app over.
//! `experiments/PROGRESS.md` is what building it found — the numbers, the
//! rules the port turned up, and the upstream faults it is written around;
//! `experiments/dioxus-assessment.md` was the plan.
//!
//! Two things about this crate are worth knowing before changing it.
//!
//! **Five modules came over from the Tauri app** — [`theme`], [`settings`],
//! [`keys`], [`library`] and [`watch`] were mounted by `#[path]` out of
//! `src-tauri/src/` for the whole of the port, so that a copy could not go
//! stale while the two applications ran side by side. There is one application
//! now and they are plain modules; their forty-four tests came with them.
//! `watch.rs` is the one that has been edited since: it posted through a shim
//! of Tauri's `AppHandle` and `Emitter`, and it posts to [`emit::Exchange`]
//! directly.
//!
//! **The headless harness is [`harness`]** — the reader driven with no window
//! and no screen, behind the `harness` feature, which `cargo test` turns on
//! and `cargo build` does not.

pub mod app;
pub mod config;
pub mod crop;
/// The Dock's own menu, which is AppKit and exists nowhere else.
#[cfg(target_os = "macos")]
pub mod dock;
/// News, and the mailbox each window reads it out of.
pub mod emit;
/// Documents written by hand for the tests. Not in the binary either.
#[cfg(feature = "harness")]
pub mod fixture;
pub mod gpu;
/// Phase 2, and it is not in the binary: see the `harness` feature.
#[cfg(feature = "harness")]
pub mod harness;
pub mod icons;
pub mod keymap;
pub mod layout;
pub mod markup;
pub mod nav;
/// Documents double-clicked in the Finder, which arrive as an Apple Event.
#[cfg(target_os = "macos")]
pub mod openfiles;
pub mod page;
pub mod palette;
pub mod pdfium;
pub mod prefs;
/// Printing: PDFKit's panel on macOS, pdfium into a GDI printer on Windows.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod print;
pub mod recolor;
pub mod render;
pub mod search;
pub mod select;
pub mod session;
pub mod shell;
pub mod sidebar;
pub mod sign;
pub mod single;
pub mod stats;
pub mod steady;
pub mod store;
pub mod styles;
/// Window tabs, which are macOS's alone. See the module's own comment.
#[cfg(target_os = "macos")]
pub mod tabs;
pub mod windows;

// The app's own. See the module comment above.
pub mod keys;
pub mod library;
pub mod settings;
pub mod theme;
pub mod watch;

// `theme.rs` and `settings.rs` write through this, and reach it at the crate
// root under the name they use in the app.
pub use config::atomic_write;
