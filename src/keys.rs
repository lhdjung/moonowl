//! The keyboard, as a file: `keys.toml`, a table of action names against the
//! keys that ask for them.
//!
//! What this side owns is the file — reading it, and saying which of its lines
//! are not a table entry of the right shape. What it deliberately does *not*
//! own is the meaning of a line: the list of actions and the grammar of a
//! chord both live in `keymap.rs`, which is what turns a keystroke into a
//! chord and would need the whole grammar anyway. Splitting it here would
//! mean writing the same parser twice and finding out about the disagreement
//! from a bug report — which is the drift `build.rs` exists to prevent
//! elsewhere.
//!
//! So an action Moonowl has never heard of, or a key it cannot read, is
//! carried across as written and reported by the keymap, on the Keyboard
//! page. Everything this module rejects is a shape TOML itself can describe
//! but the keymap cannot use: `find = 3`, `find = { key = "f" }`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::atomic_write;

/// The commented file a new install gets. Every action, with the keys it
/// ships with, all of it switched off — so the file explains itself and
/// changing one key is uncommenting one line.
const TEMPLATE: &str = include_str!("../keys.toml");

pub const FILE: &str = "keys.toml";

/// Action name → the keys the reader has given it. A `BTreeMap` because the
/// order has to be the same on every run: two actions asking for one key is
/// something the frontend reports, and a report that changed its mind between
/// launches would be worse than no report.
pub type Bindings = BTreeMap<String, Vec<String>>;

#[derive(Debug, Default, Serialize)]
pub struct Keys {
    pub bindings: Bindings,
    /// Lines that were not usable, in words meant for the reader. The
    /// frontend adds its own — an action it does not have, a chord it cannot
    /// read — and shows them together.
    pub problems: Vec<String>,
}

/// Write the template out on a machine that has never seen it, and never
/// touch it again.
///
/// Unlike a shipped theme, this file is the reader's from the moment it
/// exists: a theme we ship is ours and is rewritten every run so a fix
/// reaches a machine that already has the old one, but there is only one
/// `keys.toml` and it is the one somebody has been editing. Nothing is lost
/// by leaving it alone, because every line in the template is a comment and
/// the Keyboard page in Settings lists what is actually in force.
pub fn install(dir: &Path) {
    let path = dir.join(FILE);
    if path.exists() {
        return;
    }
    let _ = atomic_write(&path, TEMPLATE.as_bytes());
}

pub fn load(dir: &Path) -> Keys {
    let path = dir.join(FILE);
    let body = match std::fs::read_to_string(&path) {
        Ok(body) => body,
        // Not there is the ordinary case on the first run, and on any run
        // where the reader has deleted it: no keys, no complaint, defaults.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Keys::default(),
        Err(e) => {
            return Keys {
                bindings: Bindings::new(),
                problems: vec![format!("{FILE} could not be read: {e}")],
            }
        }
    };

    let table: toml::Table = match toml::from_str(&body) {
        Ok(table) => table,
        Err(e) => {
            // `toml` puts the line and column in its message, which is the
            // only part of this the reader can act on.
            return Keys {
                bindings: Bindings::new(),
                problems: vec![format!("{FILE} is not readable TOML: {e}")],
            };
        }
    };

    let mut keys = Keys::default();
    for (action, value) in table {
        match value {
            // One key, written without the brackets. A convenience worth
            // having: most rebinds are one key, and `find = "mod+e"` is what
            // somebody will write first.
            toml::Value::String(one) => {
                keys.bindings.insert(action, vec![one]);
            }
            toml::Value::Array(items) => {
                let mut chords = Vec::with_capacity(items.len());
                let mut bad = false;
                for item in items {
                    match item {
                        toml::Value::String(chord) => chords.push(chord),
                        other => {
                            bad = true;
                            keys.problems.push(format!(
                                "{action}: {} is not a key. Keys are written in quotes.",
                                kind_of(&other)
                            ));
                        }
                    }
                }
                // The readable half is still kept: a list with one bad entry
                // in it is a typo, not a reason to give the reader back keys
                // they have spent an evening replacing.
                if !bad || !chords.is_empty() {
                    keys.bindings.insert(action, chords);
                }
            }
            other => keys.problems.push(format!(
                "{action}: expected a key or a list of keys, found {}.",
                kind_of(&other)
            )),
        }
    }
    keys
}

fn kind_of(value: &toml::Value) -> &'static str {
    match value {
        toml::Value::String(_) => "text",
        toml::Value::Integer(_) => "a number",
        toml::Value::Float(_) => "a number",
        toml::Value::Boolean(_) => "true or false",
        toml::Value::Datetime(_) => "a date",
        toml::Value::Array(_) => "a list",
        toml::Value::Table(_) => "a table",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("moonowl-keys-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn nothing_on_disk_is_not_a_complaint() {
        let keys = load(&dir("missing"));
        assert!(keys.bindings.is_empty());
        assert!(keys.problems.is_empty());
    }

    #[test]
    fn the_template_installs_once_and_is_then_the_readers() {
        let dir = dir("install");
        install(&dir);
        let path = dir.join(FILE);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), TEMPLATE);
        // Every line of it is a comment, so a fresh install binds nothing.
        assert!(load(&dir).bindings.is_empty());

        std::fs::write(&path, "find = [\"mod+e\"]\n").unwrap();
        install(&dir);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "find = [\"mod+e\"]\n"
        );
    }

    #[test]
    fn one_key_or_a_list_of_them() {
        let dir = dir("shapes");
        std::fs::write(
            dir.join(FILE),
            "find = \"mod+e\"\nquit = []\nback = [\"mod+[\", \"alt+left\"]\n",
        )
        .unwrap();
        let keys = load(&dir);
        assert_eq!(keys.bindings["find"], vec!["mod+e"]);
        assert_eq!(keys.bindings["quit"], Vec::<String>::new());
        assert_eq!(keys.bindings["back"], vec!["mod+[", "alt+left"]);
        assert!(keys.problems.is_empty());
    }

    #[test]
    fn a_shape_the_frontend_could_not_use_is_named_rather_than_dropped() {
        let dir = dir("wrong");
        std::fs::write(dir.join(FILE), "find = 3\nback = [\"mod+[\", 7]\n").unwrap();
        let keys = load(&dir);
        assert!(!keys.bindings.contains_key("find"));
        // The good half of a list survives; the bad half is reported.
        assert_eq!(keys.bindings["back"], vec!["mod+["]);
        assert_eq!(keys.problems.len(), 2);
        assert!(keys.problems.iter().any(|p| p.starts_with("find:")));
        assert!(keys.problems.iter().any(|p| p.starts_with("back:")));
    }

    #[test]
    fn a_file_that_will_not_parse_says_so_and_binds_nothing() {
        let dir = dir("broken");
        std::fs::write(dir.join(FILE), "find = [\"mod+e\"\n").unwrap();
        let keys = load(&dir);
        assert!(keys.bindings.is_empty());
        assert_eq!(keys.problems.len(), 1);
        assert!(keys.problems[0].contains("keys.toml"));
    }
}
