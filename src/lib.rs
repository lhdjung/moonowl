//! Moonowl: a reader that reads, drawn by Blitz rather than by a webview.
//!
//! `plans/dormant/experiments/PROGRESS.md` records the numbers behind its
//! rules and the upstream faults it is written around.
//!
//! **The headless harness is [`harness`]** — the reader driven with no window
//! and no screen, behind the `harness` feature, which `cargo test` turns on
//! and `cargo build` does not.

pub mod app;
/// Page edits: removing, turning, moving, combining and splitting pages.
pub mod arrange;
pub mod compare;
pub mod config;
pub mod crop;
/// The Dock's own menu, which is AppKit and exists nowhere else.
#[cfg(target_os = "macos")]
pub mod dock;
/// News, and the mailbox each window reads it out of.
pub mod emit;
/// The document in other forms: a page as a picture, and its text.
pub mod export;
/// Documents written by hand for the tests. Not in the binary either.
#[cfg(feature = "harness")]
pub mod fixture;
pub mod forms;
pub mod gpu;
/// The reader driven with no window. Not in the binary: see the `harness`
/// feature.
#[cfg(feature = "harness")]
pub mod harness;
pub mod icons;
pub mod keymap;
/// Page labels, written through lopdf when pages change or are numbered.
pub mod labels;
pub mod layout;
pub mod markup;
pub mod nav;
/// The document's own objects through lopdf: incremental updates and smaller files.
pub mod objects;
pub mod ocr;
/// Documents double-clicked in the Finder, which arrive as an Apple Event.
#[cfg(target_os = "macos")]
pub mod openfiles;
/// The outline, kept true when pages change, through lopdf.
pub mod organiser;
pub mod outline;
pub mod page;
pub mod palette;
pub mod palettes;
pub mod pdfium;
pub mod prefs;
/// Printing: PDFKit's panel on macOS, pdfium into a GDI printer on Windows.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod print;
pub mod recolor;
pub mod refs;
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
pub mod synctex;
/// Window tabs, which are macOS's alone. See the module's own comment.
#[cfg(target_os = "macos")]
pub mod tabs;
pub mod windows;

// The settings, the library, the keys and the themes on disk.
pub mod keys;
pub mod library;
pub mod settings;
pub mod shelf;
pub mod theme;
pub mod watch;

// `theme.rs` and `settings.rs` write through this, and reach it at the crate
// root.
pub use config::atomic_write;
