//! Themes are plain TOML files, one per theme, so that a theme can be written
//! by hand (or by an LLM) without touching the app.

use serde::{Deserialize, Serialize};

pub use crate::shelf::Kept;

// The shipped set is the contents of `themes/`, turned into a table by
// `build.rs` — which also refuses to build a theme that will not parse or that
// names a colour the renderer cannot read. Adding a theme is adding a file
// and its id to `themes/order`, which says where the menu lists it.
//
// The themes are still embedded: the generated table is `include_str!` per
// file, so the binary carries its own copies and `install_built_ins` can write
// them out on a machine that has never seen them.
include!(concat!(env!("OUT_DIR"), "/built_in.rs"));

pub const DEFAULT_LIGHT: &str = "moonowl-light";
pub const DEFAULT_DARK: &str = "moonowl-dark";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    /// The stem of the file this theme lives in, verbatim. Not stored in the
    /// file itself, and not a slug: a theme the app made has a slug for a name
    /// because `slugify` chose one, and a theme somebody wrote by hand is
    /// called whatever they called the file. Anything looking for the file has
    /// to use this exactly — see `path_for`.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub text: String,
    pub background: String,
    #[serde(default)]
    pub accent: Option<String>,
    /// The colour links are tinted with while the document is being recoloured.
    /// Absent means "use the accent".
    #[serde(default)]
    pub link: Option<String>,
    /// The colour behind selected text. Absent means "derive it from the
    /// accent", which is what every theme did before this was settable and is
    /// still the right answer for most of them.
    ///
    /// The alias is what it used to be called. `selection` read as the whole
    /// of what selecting does, which is two colours and not one, and a theme
    /// naming it alongside `selection_text` was naming the pair and then one
    /// half of the pair again. Renaming a key is not free: a theme somebody
    /// wrote is a file on their disk that this app does not own, and dropping
    /// a field it no longer recognises would take their colour away silently
    /// and give them the derived one back. So the old spelling is still read.
    /// Only the new one is written.
    #[serde(default, alias = "selection")]
    pub selection_area: Option<String>,
    /// The colour selected text itself is drawn in. Absent means "derive it
    /// from the colour behind it", which is what most themes want: the two
    /// only ever appear together, so one of them can always answer for both.
    #[serde(default)]
    pub selection_text: Option<String>,
    /// The colour around the page. Absent means "the background, a little
    /// darker".
    #[serde(default)]
    pub surround: Option<String>,
    /// When false the document keeps its own colors and only the app chrome is
    /// themed. Used by Moonowl Light.
    #[serde(default = "yes")]
    pub recolor: bool,
    /// Set by the loader, not by the file.
    #[serde(default)]
    pub built_in: bool,
}

fn yes() -> bool {
    true
}

/// What actually gets written to disk when a user saves a theme.
#[derive(Debug, Serialize)]
struct ThemeFile<'a> {
    name: &'a str,
    text: &'a str,
    background: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    accent: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    link: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selection_area: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selection_text: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    surround: &'a Option<String>,
    recolor: bool,
}

/// The banner every shipped theme file carries.
///
/// These files are rewritten on every run, so an edit made in place disappears
/// at the next launch. That is deliberate — the shipped set is the app's to
/// define, and a built-in that could drift would make "Moonowl Dark" mean
/// something different on every machine. But a file that silently undoes your
/// work and says nothing about it is a trap, and the whole point of keeping
/// themes as plain text is that someone can open one and get somewhere. So the
/// file says what it is and where to put a copy.
const BANNER: &str = "\
# A Moonowl theme. The app rewrites this file at every launch, so edits made
# here are lost. To make your own, copy it under a new file name and change its
# `name`, or use \"Copy this theme…\" in the app.

";

impl Kept for Theme {
    const NOUN: &'static str = "theme";
    const SHIPPED: &'static [(&'static str, &'static str)] = BUILT_IN;
    const BANNER: &'static str = BANNER;
    const NOT_ONE: &'static str =
        "That is not a Moonowl theme — it needs a name, a text colour and a background.";

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

    /// Refused, never guessed: a theme written with `steelblue` in it is worn
    /// in the fallback colour with a complaint nobody asked for.
    fn check(&self) -> Result<(), String> {
        let bad = crate::palette::unreadable(self);
        if bad.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "That theme names colours Moonowl cannot read: {}.",
                bad.join(", ")
            ))
        }
    }

    fn to_toml(&self) -> Result<String, String> {
        let stored = ThemeFile {
            name: self.name.trim(),
            text: &self.text,
            background: &self.background,
            accent: &self.accent,
            link: &self.link,
            selection_area: &self.selection_area,
            selection_text: &self.selection_text,
            surround: &self.surround,
            recolor: self.recolor,
        };
        toml::to_string_pretty(&stored).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn a_taken_name_is_numbered() {
        let named = |name: &str| {
            let source = format!("name = \"{name}\"\ntext = \"#fff\"\nbackground = \"#000\"\n");
            crate::shelf::parse::<Theme>("x", &source, false).expect("a theme")
        };
        let themes = [named("New theme"), named("new theme 2")];
        assert_eq!(Theme::free_name(&themes, "New theme"), "New theme 3");
        assert_eq!(Theme::free_name(&themes, "Nord copy"), "Nord copy");
    }

    /// `selection` was renamed to `selection_area`, and a theme somebody wrote
    /// is a file on their disk that this app does not own. Dropping a key it
    /// no longer recognises would take their colour away and hand back the
    /// derived one, with nothing anywhere saying why — which is the same
    /// silent-revert this module refuses to do to a built-in edited in place.
    #[test]
    fn the_old_spelling_of_selection_area_is_still_read() {
        let old = crate::shelf::parse::<Theme>(
            "x",
            "name = \"X\"\ntext = \"#fff\"\nbackground = \"#000\"\nselection = \"#123456\"\n",
            false,
        )
        .expect("a theme using the old key still parses");
        assert_eq!(old.selection_area.as_deref(), Some("#123456"));
    }

    /// And only the new one is written, so a theme saved through the editor
    /// comes back with one spelling rather than two.
    #[test]
    fn only_the_new_spelling_is_written() {
        let stored = ThemeFile {
            name: "X",
            text: "#fff",
            background: "#000",
            accent: &None,
            link: &None,
            selection_area: &Some("#123456".into()),
            selection_text: &None,
            surround: &None,
            recolor: true,
        };
        let body = toml::to_string_pretty(&stored).unwrap();
        assert!(body.contains("selection_area = \"#123456\""), "{body}");
        assert!(!body.contains("\nselection = "), "{body}");
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("moonowl-theme-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    fn hand_written(dir: &Path, file: &str, name: &str) {
        fs::write(
            dir.join(file),
            format!("name = {name:?}\ntext = \"#111111\"\nbackground = \"#eeeeee\"\n"),
        )
        .expect("theme file");
    }

    /// The whole argument for keeping themes as plain TOML is that somebody
    /// can write one by hand — and a file written by hand is exactly the one
    /// whose name is not already a slug. An id is the file's stem, so a round
    /// trip through the app has to find the file the author named.
    #[test]
    fn a_hand_written_theme_can_be_deleted() {
        let dir = scratch("delete");
        hand_written(&dir, "My Theme.toml", "My Theme");

        let listed = Theme::load_all(&dir);
        let mine = listed
            .iter()
            .find(|theme| theme.name == "My Theme")
            .expect("listed");
        assert_eq!(mine.id, "My Theme", "the id is the file's own stem");

        Theme::delete(&dir, &mine.id).expect("delete");
        assert!(!dir.join("My Theme.toml").exists());
        assert!(!Theme::load_all(&dir)
            .iter()
            .any(|theme| theme.name == "My Theme"));
    }

    /// And editing one changes it rather than producing a second copy beside
    /// it under a slugified name.
    #[test]
    fn editing_a_hand_written_theme_changes_that_file() {
        let dir = scratch("edit");
        hand_written(&dir, "My Theme.toml", "My Theme");
        let mut mine = Theme::load_all(&dir)
            .into_iter()
            .find(|theme| theme.name == "My Theme")
            .expect("listed");

        mine.background = "#222222".into();
        let saved = Theme::save(&dir, &mine).expect("save");
        assert_eq!(saved.id, "My Theme");

        let files: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| !crate::shelf::is_built_in::<Theme>(name.trim_end_matches(".toml")))
            .collect();
        assert_eq!(files, vec!["My Theme.toml".to_string()], "a copy was made");
        assert!(fs::read_to_string(dir.join("My Theme.toml"))
            .unwrap()
            .contains("#222222"));
    }

    /// A theme that is not there is said to be not there. Reporting success
    /// is how "Deleted X" came to be said about a theme still in the list.
    #[test]
    fn deleting_something_that_is_not_there_says_so() {
        let dir = scratch("missing");
        let problem = Theme::delete(&dir, "never-existed").expect_err("should report it");
        assert!(problem.contains("never-existed"), "{problem}");
    }

    /// An id crosses the bridge from the frontend, and a file name is the only
    /// thing it may be.
    #[test]
    fn an_id_that_is_not_a_file_name_is_refused() {
        let dir = scratch("escape");
        for id in ["../outside", "sub/theme", "", "   "] {
            assert!(
                crate::shelf::path_for(&dir, id).is_none(),
                "{id:?} was allowed through"
            );
            assert!(
                Theme::delete(&dir, id).is_err(),
                "{id:?} was allowed through delete"
            );
        }
        // And a name with nothing wrong with it still works.
        assert_eq!(
            crate::shelf::path_for(&dir, "My Theme"),
            Some(dir.join("My Theme.toml")),
        );
    }

    /// A built-in is never overwritten, however it is asked for.
    #[test]
    fn editing_a_built_in_saves_a_copy() {
        let dir = scratch("built-in");
        Theme::install_built_ins(&dir);
        let original = fs::read_to_string(dir.join(format!("{DEFAULT_DARK}.toml"))).unwrap();

        let mut theme = Theme::load_all(&dir)
            .into_iter()
            .find(|theme| theme.id == DEFAULT_DARK)
            .expect("shipped");
        theme.background = "#010203".into();
        let saved = Theme::save(&dir, &theme).expect("save");

        assert_ne!(saved.id, DEFAULT_DARK);
        assert!(!saved.built_in);
        assert_eq!(
            fs::read_to_string(dir.join(format!("{DEFAULT_DARK}.toml"))).unwrap(),
            original,
            "the shipped file was written over"
        );
    }

    /// A second theme of a name already in the menu is refused.
    #[test]
    fn a_name_already_taken_is_refused() {
        let dir = scratch("taken");
        Theme::install_built_ins(&dir);
        let mut theme = Theme::load_all(&dir)
            .into_iter()
            .find(|theme| theme.id == DEFAULT_DARK)
            .expect("shipped");
        theme.id = String::new();
        theme.name = " nord ".into();
        assert_eq!(
            Theme::save(&dir, &theme).unwrap_err(),
            "There is already a theme called nord."
        );
    }

    /// **A renamed theme renames its file**, where the app is the one that
    /// named it: `brownie.toml` saying `name = "Walnut"` is a directory
    /// nobody can read.
    #[test]
    fn renaming_a_theme_renames_the_file_the_app_named() {
        let dir = scratch("rename");
        let made = Theme::save(
            &dir,
            &Theme {
                id: String::new(),
                name: "Brownie".into(),
                text: "#111111".into(),
                background: "#eeeeee".into(),
                accent: None,
                link: None,
                selection_area: None,
                selection_text: None,
                surround: None,
                recolor: true,
                built_in: false,
            },
        )
        .expect("saved");
        assert_eq!(made.id, "brownie");

        let renamed = Theme::save(
            &dir,
            &Theme {
                name: "Walnut".into(),
                ..made
            },
        )
        .expect("saved again");
        assert_eq!(renamed.id, "walnut", "the id follows the name");
        assert!(dir.join("walnut.toml").exists());
        assert!(
            !dir.join("brownie.toml").exists(),
            "and the old file is gone"
        );
        let listed = Theme::load_all(&dir);
        assert_eq!(
            listed.iter().filter(|theme| !theme.built_in).count(),
            1,
            "one theme, not two: {listed:?}",
        );
    }

    /// And a file somebody named themselves keeps its name, whatever the theme
    /// inside it comes to be called. The same rule that stops this app
    /// reverting a built-in edited in place: it does not own that file.
    #[test]
    fn renaming_leaves_a_hand_written_file_where_it_is() {
        let dir = scratch("rename-hand");
        hand_written(&dir, "My Theme.toml", "My Theme");
        let mine = Theme::load_all(&dir)
            .into_iter()
            .find(|theme| theme.id == "My Theme")
            .expect("listed");

        let saved = Theme::save(
            &dir,
            &Theme {
                name: "Something Else".into(),
                ..mine
            },
        )
        .expect("saved");
        assert_eq!(
            saved.id, "My Theme",
            "the file keeps the name its author gave it"
        );
        assert!(dir.join("My Theme.toml").exists());
        assert!(!dir.join("something-else.toml").exists());
    }

    /// An exported theme imports back as a theme of the reader's own, beside
    /// the one it came from rather than over it; a file that is not a theme,
    /// or names a colour nobody can read, is refused and writes nothing.
    #[test]
    fn an_exported_theme_imports_beside_the_original() {
        let dir = scratch("import");
        let (_, source) = BUILT_IN[0];
        let original = crate::shelf::parse::<Theme>("x", source, true).expect("a shipped theme");
        let imported = Theme::import(&dir, &original.to_toml().unwrap()).expect("imports");
        assert_eq!(imported.name, format!("{} 2", original.name));
        assert!(!imported.built_in);
        let again = Theme::import(&dir, &Theme::shipped(source)).expect("the banner is ignored");
        assert_ne!(again.id, imported.id);

        assert!(Theme::import(&dir, "title = \"nope\"").is_err());
        assert!(Theme::import(
            &dir,
            "name = \"X\"\ntext = \"steelblue\"\nbackground = \"#000\""
        )
        .is_err());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
    }
}
