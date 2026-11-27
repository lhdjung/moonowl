//! Moonowl's documents, settings and themes, with no interface: what the
//! app reads, draws, writes and keeps. The app in `src/` is the interface,
//! and re-exports every module here under its own name.

/// Page edits: removing, turning, moving, combining and splitting pages.
pub mod arrange;
pub mod compare;
pub mod config;
pub mod crop;
/// News, and the mailbox each window reads it out of.
pub mod emit;
/// The document in other forms: a page as a picture, and its text.
pub mod export;
/// Documents written by hand for the tests. Not in the binary.
#[cfg(feature = "harness")]
pub mod fixture;
pub mod forms;
/// Page labels, written through lopdf when pages change or are numbered.
pub mod labels;
pub mod layout;
pub mod markup;
/// The document's own objects through lopdf: incremental updates and smaller files.
pub mod objects;
pub mod ocr;
/// The outline, kept true when pages change, through lopdf.
pub mod outline;
pub mod palette;
pub mod palettes;
pub mod pdfium;
pub mod recolor;
pub mod refs;
pub mod render;
pub mod report;
pub mod search;
pub mod select;
pub mod sign;
pub mod stats;
pub mod store;
pub mod synctex;

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
