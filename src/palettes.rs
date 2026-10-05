//! Highlight palettes: the six colours a selection offers, kept as files the
//! way themes are — see [`crate::shelf`]. A palette chosen is what new marks
//! are made in; a mark already in a document keeps the colour it was written
//! in.
//!
//! Not `Palette`, which is a theme's colours resolved: see `palette.rs`.

use serde::{Deserialize, Serialize};

pub use crate::shelf::Kept;

// The shipped set is the contents of `palettes/`, checked and tabled by
// `build.rs` as `themes/` is.
include!(concat!(env!("OUT_DIR"), "/built_in_palettes.rs"));

/// The palette a fresh install offers.
pub const DEFAULT: &str = "soft";

/// How many colours a palette has: the swatches under a selection.
pub const SIZE: usize = 6;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HighlightPalette {
    /// The stem of its file, verbatim. See [`crate::shelf::path_for`].
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub colors: Vec<String>,
    /// Set by the loader, not by the file.
    #[serde(default)]
    pub built_in: bool,
}

/// What gets written when a reader saves one.
#[derive(Serialize)]
struct PaletteFile<'a> {
    name: &'a str,
    colors: &'a [String],
}

impl Kept for HighlightPalette {
    const NOUN: &'static str = "palette";
    const SHIPPED: &'static [(&'static str, &'static str)] = BUILT_IN;
    /// Two lines, not a wall: what the file is, and that a shipped one is
    /// the app's. See `theme::BANNER` for why a shipped file says so.
    const BANNER: &'static str = "\
# A Moonowl highlight palette. This shipped one is rewritten at every launch:
# copy it under another name to create a new palette.

";
    const NOT_ONE: &'static str = "That is not a Moonowl palette — it needs a name and six colors.";

    fn name(&self) -> &str {
        &self.name
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn place(&mut self, id: String, built_in: bool) {
        self.id = id;
        self.built_in = built_in;
    }

    fn rename(&mut self, name: String) {
        self.name = name;
    }

    /// Six, and every one a colour the renderer can read.
    fn check(&self) -> Result<(), String> {
        if self.colors.len() != SIZE {
            return Err(format!(
                "A palette has six colours, and {} has {}.",
                self.name.trim(),
                self.colors.len()
            ));
        }
        let bad: Vec<&str> = self
            .colors
            .iter()
            .filter(|colour| crate::palette::read_colour(colour).is_none())
            .map(String::as_str)
            .collect();
        if bad.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "That palette names colours Moonowl cannot read: {}.",
                bad.join(", ")
            ))
        }
    }

    fn to_toml(&self) -> Result<String, String> {
        toml::to_string_pretty(&PaletteFile {
            name: self.name.trim(),
            colors: &self.colors,
        })
        .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("moonowl-palettes-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    /// The shelf's rules hold for a palette as they do for a theme: the
    /// shipped set listed first and in order, a built-in edited saved as a
    /// copy, and one of six colours or none at all.
    #[test]
    fn a_palette_is_kept_as_a_theme_is() {
        let dir = scratch("kept");
        HighlightPalette::install_built_ins(&dir);
        let all = HighlightPalette::load_all(&dir);
        assert_eq!(all[0].id, DEFAULT);
        assert!(all
            .iter()
            .all(|one| one.built_in && one.colors.len() == SIZE));

        let mut soft = all[0].clone();
        soft.colors[0] = "#fff3b0".into();
        let saved = HighlightPalette::save(&dir, &soft).expect("saved");
        assert_ne!(saved.id, DEFAULT, "a copy, not the shipped file");
        assert!(std::fs::read_to_string(dir.join("soft.toml"))
            .unwrap()
            .contains("#f7df8b"));

        let short = HighlightPalette {
            colors: vec!["#ffffff".into()],
            id: String::new(),
            ..saved.clone()
        };
        assert!(HighlightPalette::save(&dir, &short).is_err());
        assert!(HighlightPalette::import(
            &dir,
            "name = \"X\"\ncolors = [\"teal\", \"#fff\", \"#fff\", \"#fff\", \"#fff\", \"#fff\"]"
        )
        .is_err());
    }
}
