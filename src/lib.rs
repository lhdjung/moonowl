//! Moonowl: a reader that reads, drawn by Blitz rather than by a webview.
//!
//! **The headless harness is [`harness`]** — the reader driven with no window
//! and no screen, behind the `harness` feature, which `cargo test` turns on
//! and `cargo build` does not.
//!
//! The documents, settings and themes are the `moonowl-core` crate in
//! `core/`. Its modules are re-exported here under their own names, so
//! `crate::layout` is the same module in both crates.

pub mod app;
/// The Dock's own menu, which is AppKit and exists nowhere else.
#[cfg(target_os = "macos")]
pub mod dock;
pub mod gpu;
/// The reader driven with no window. Not in the binary: see the `harness`
/// feature.
#[cfg(feature = "harness")]
pub mod harness;
pub mod icons;
pub mod keymap;
pub mod launch;
pub mod nav;
/// Documents double-clicked in the Finder, which arrive as an Apple Event.
#[cfg(target_os = "macos")]
pub mod openfiles;
/// The page organiser: the draft of pages being arranged, its grid and its view.
pub mod organiser;
pub mod page;
pub mod prefs;
/// Printing: PDFKit's panel on macOS, pdfium into a GDI printer on Windows.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod print;
pub mod session;
pub mod shell;
pub mod sidebar;
pub mod single;
pub mod steady;
pub mod styles;
/// Window tabs, which are macOS's alone. See the module's own comment.
#[cfg(target_os = "macos")]
pub mod tabs;
pub mod windows;

#[cfg(feature = "harness")]
pub use moonowl_core::fixture;
pub use moonowl_core::{
    arrange, atomic_write, compare, config, crop, emit, export, forms, keys, labels, layout,
    library, markup, objects, ocr, outline, palette, palettes, pdfium, recolor, refs, render,
    report, search, select, settings, shelf, sign, stats, store, synctex, theme, watch,
};
