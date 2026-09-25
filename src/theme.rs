//! Themes are plain TOML files, one per theme, so that a theme can be written
//! by hand (or by an LLM) without touching the app.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::atomic_write;

// The shipped set is the contents of `themes/`, turned into a table by
// `build.rs` — which also refuses to build a theme that will not parse or that
// names a colour the renderer cannot read. Adding a theme is adding a file
// with an `order` in it; there is no list here to keep in step with the
// directory, and none in `api.ts` either.
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
    recolor: bool,
}

pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    let slug = out.trim_matches('-').to_string();
    if slug.is_empty() {
        "theme".into()
    } else {
        slug
    }
}

fn parse(id: &str, source: &str, built_in: bool) -> Option<Theme> {
    let mut theme: Theme = toml::from_str(source).ok()?;
    theme.id = id.to_string();
    theme.built_in = built_in;
    Some(theme)
}

/// Whatever its case: `Nord.toml` is `nord.toml` to APFS, and a user theme
/// named that would be overwritten by the built-in on the next run.
fn is_built_in(id: &str) -> bool {
    BUILT_IN
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case(id))
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
# This file ships with Moonowl and is rewritten every time the app starts.
# Edit it and your changes will be gone at the next launch.
#
# To make it yours: copy it to a new name in this folder — any name but the
# ones the shipped themes use — change the `name` inside, and it will appear in
# the theme list alongside these. The app does the same thing when you press
# \"Copy this theme\".
#
# The `order` below says where this one sits among the shipped themes. It means
# nothing in a theme of your own: those are listed after these, by name.

";

fn shipped(source: &str) -> String {
    format!("{BANNER}{source}")
}

/// Write the shipped themes out on every run, so that a built-in whose colours
/// change in the app changes on disk too, rather than the first install of it
/// sitting there forever. Editing a built-in through the app already saves a
/// copy under an id of its own, so nothing a reader made is at stake; a
/// built-in file hand-edited in place is overwritten, deliberately — and the
/// banner on top of it says so, so nobody finds that out the hard way.
pub fn install_built_ins(dir: &Path) {
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    for (id, source) in BUILT_IN {
        let path = dir.join(format!("{id}.toml"));
        let wanted = shipped(source);
        // Only when it differs: no reason to touch a file that already says
        // exactly this.
        let on_disk = fs::read_to_string(&path).unwrap_or_default();
        if on_disk != wanted {
            // Through `atomic_write` like every other write in this crate. A
            // plain `fs::write` truncates and then fills, so there is a moment
            // when the file on disk is a shipped theme with no colours in it —
            // and this directory is watched, and read by anything the reader
            // has open beside the app. Rewriting fifteen files at every
            // launch is fifteen chances at that moment.
            let _ = atomic_write(&path, wanted.as_bytes());
        }
    }
}

/// The theme files in a directory that [`load_all`] could not read, each with
/// why, as of the last time it looked. A file with a typo in it is otherwise
/// simply missing from the list, which is no answer to somebody who wrote one.
static PROBLEMS: std::sync::Mutex<std::collections::BTreeMap<PathBuf, Vec<String>>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

pub fn problems(dir: &Path) -> Vec<String> {
    PROBLEMS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(dir)
        .cloned()
        .unwrap_or_default()
}

/// All themes, built-ins first and in the order they are declared above, then
/// the user's own in alphabetical order.
pub fn load_all(dir: &Path) -> Vec<Theme> {
    let mut themes: Vec<Theme> = Vec::new();

    for (id, embedded) in BUILT_IN {
        let from_disk = fs::read_to_string(dir.join(format!("{id}.toml")))
            .ok()
            .and_then(|source| parse(id, &source, true));
        if let Some(theme) = from_disk.or_else(|| parse(id, embedded, true)) {
            themes.push(theme);
        }
    }

    let mut custom: Vec<Theme> = Vec::new();
    let mut refused: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if is_built_in(id) {
                continue;
            }
            let read = fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|source| {
                    toml::from_str::<Theme>(&source).map_err(|e| e.message().to_string())
                });
            match read {
                Ok(mut theme) => {
                    theme.id = id.to_string();
                    custom.push(theme);
                }
                Err(why) => refused.push(format!(
                    "{id}.toml is not listed, because it could not be read: {why}."
                )),
            }
        }
    }
    refused.sort();
    PROBLEMS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(dir.to_path_buf(), refused);
    custom.sort_by_key(|theme| theme.name.to_lowercase());
    themes.append(&mut custom);
    themes
}

pub fn save(dir: &Path, theme: &Theme) -> Result<Theme, String> {
    // Refused, never guessed: the editor's Save and Import both come here,
    // and a theme written with `steelblue` in it is worn in the fallback
    // colour with a complaint nobody asked for.
    let bad = crate::palette::unreadable(theme);
    if !bad.is_empty() {
        return Err(format!(
            "That theme names colours Moonowl cannot read: {}.",
            bad.join(", ")
        ));
    }
    if theme.name.trim().is_empty() {
        return Err("A theme needs a name.".into());
    }
    // Two of one name in the menu are two rows nobody can tell apart.
    if load_all(dir).iter().any(|other| {
        other.id != theme.id && other.name.trim().eq_ignore_ascii_case(theme.name.trim())
    }) {
        return Err(format!(
            "There is already a theme called {}.",
            theme.name.trim()
        ));
    }
    let mut id = if theme.id.trim().is_empty() {
        // A new theme never lands on top of one that is already there. This is
        // the one place a slug is made rather than kept: a *name* is prose and
        // has to become a file name somehow.
        unique_id(dir, &slugify(&theme.name))
    } else {
        // An id that already exists is the stem of the file the theme lives in
        // — see `path_for`. Slugifying it here wrote `my-theme.toml` beside the
        // `My Theme.toml` it was meant to be editing, so a hand-written theme
        // came back doubled instead of changed.
        theme.id.trim().to_string()
    };
    // **A renamed theme is renamed on the disk too, where the app is the one
    // that named the file.** Brownie renamed to Bay Brown left `brownie.toml`
    // saying `name = "Bay Brown"`, which is a directory nobody can read: the
    // file name is what a person sorts by, copies and hands to somebody else.
    //
    // *Where the app named it*, and that is the whole of the caution. A file
    // called `My Theme.toml` was named by the person who wrote it, and this app
    // does not rename somebody else's file — the same rule that stops it
    // reverting a built-in edited in place. So the old file is only left behind
    // when its stem is the slug of the name it had, which is what `save` writes
    // and nothing else does.
    let renamed = old_file_name(dir, &id)
        .filter(|old| slugify(old) == id && slugify(&theme.name) != id)
        .map(|_| unique_id(dir, &slugify(&theme.name)));
    if is_built_in(&id) {
        // Editing a built-in makes a copy rather than shadowing the original.
        id = unique_id(dir, &format!("{}-custom", slugify(&id)));
    }
    let path = path_for(dir, &id)
        .ok_or("A theme cannot be saved under that name — it is not a file name.")?;

    let body = to_toml(theme)?;

    // The rename is a write of the new file and then a delete of the old, in
    // that order: a machine that stops between them has both copies, which is
    // a theme listed twice, and the other order has none at all.
    if let Some(fresh) = &renamed {
        let moved = path_for(dir, fresh)
            .ok_or("A theme cannot be saved under that name — it is not a file name.")?;
        atomic_write(&moved, body.as_bytes())?;
        let _ = fs::remove_file(&path);
        let mut saved = theme.clone();
        saved.id = fresh.clone();
        saved.built_in = false;
        return Ok(saved);
    }

    atomic_write(&path, body.as_bytes())?;

    let mut saved = theme.clone();
    saved.id = id;
    saved.built_in = false;
    Ok(saved)
}

/// A theme as its file says it: what [`save`] writes, and what Export hands
/// over — no banner and no `order`, which mean nothing outside this folder.
pub fn to_toml(theme: &Theme) -> Result<String, String> {
    let stored = ThemeFile {
        name: theme.name.trim(),
        text: &theme.text,
        background: &theme.background,
        accent: &theme.accent,
        link: &theme.link,
        selection_area: &theme.selection_area,
        selection_text: &theme.selection_text,
        recolor: theme.recolor,
    };
    toml::to_string_pretty(&stored).map_err(|e| e.to_string())
}

/// A theme file from elsewhere, added to the reader's own under an id of its
/// own — never over a theme already there. Refused, rather than imported to
/// render black on white, when it is not a theme or names a colour the
/// renderer cannot read.
pub fn import(dir: &Path, source: &str) -> Result<Theme, String> {
    let theme: Theme = toml::from_str(source).map_err(|_| {
        "That is not a Moonowl theme — it needs a name, a text colour and a background.".to_string()
    })?;
    // Beside a theme of the same name, as "Nord 2": importing is asking for
    // it to be added, and a name is not a reason to refuse.
    let taken: Vec<String> = load_all(dir)
        .iter()
        .map(|theme| theme.name.trim().to_lowercase())
        .collect();
    let base = theme.name.trim().to_string();
    let name = (1..)
        .map(|n| {
            if n == 1 {
                base.clone()
            } else {
                format!("{base} {n}")
            }
        })
        .find(|name| !taken.contains(&name.to_lowercase()))
        .unwrap_or(base);
    save(
        dir,
        &Theme {
            id: String::new(),
            built_in: false,
            name,
            ..theme
        },
    )
}

/// What the theme in `id`'s file is *currently* called, or nothing when there
/// is no such file. Read rather than remembered: the name in the draft is the
/// new one by the time [`save`] is asked.
fn old_file_name(dir: &Path, id: &str) -> Option<String> {
    let path = path_for(dir, id)?;
    let source = fs::read_to_string(path).ok()?;
    parse(id, &source, false).map(|theme| theme.name)
}

pub fn delete(dir: &Path, id: &str) -> Result<(), String> {
    if is_built_in(id) {
        return Err("Built-in themes cannot be deleted.".into());
    }
    let path =
        path_for(dir, id).ok_or("That is not a theme name, so there is nothing to delete.")?;
    if !path.exists() {
        // Not silence. This used to return `Ok(())` for a theme it had failed
        // to find — because it was looking under a slugified name and the file
        // is named whatever its author named it — and the app then said
        // "Deleted X" about a theme still sitting in the list.
        return Err(format!("{id} is not there to delete."));
    }
    fs::remove_file(path).map_err(|e| e.to_string())
}

/// The file a theme lives in, or nothing if that id could not name one.
///
/// An id **is** a file stem: `load_all` reads it off the directory verbatim,
/// so a theme somebody wrote by hand is called `My Theme` and not `my-theme`,
/// and everything that goes looking for it has to use the name it actually
/// has. Slugifying on the way back out is what made `delete` miss the file and
/// report success, and `save` write a second one beside the first.
///
/// Verbatim is not unchecked. An id crosses the bridge from the frontend, so
/// it has to name a file in this directory and nothing else: no separators, no
/// walking upwards, nothing empty. Anything else is refused rather than
/// repaired, because a repaired path is a path pointing somewhere nobody asked
/// for.
fn path_for(dir: &Path, id: &str) -> Option<PathBuf> {
    let name = format!("{id}.toml");
    if id.trim().is_empty() || Path::new(&name).file_name() != Some(name.as_ref()) {
        return None;
    }
    Some(dir.join(name))
}

/// A slug nothing in the directory is using yet. `base` comes from `slugify`,
/// so it is always a name a file can have.
fn unique_id(dir: &Path, base: &str) -> String {
    let free = |id: &str| !is_built_in(id) && path_for(dir, id).is_some_and(|path| !path.exists());
    if free(base) {
        return base.to_string();
    }
    for n in 2.. {
        let candidate = format!("{base}-{n}");
        if free(&candidate) {
            return candidate;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `selection` was renamed to `selection_area`, and a theme somebody wrote
    /// is a file on their disk that this app does not own. Dropping a key it
    /// no longer recognises would take their colour away and hand back the
    /// derived one, with nothing anywhere saying why — which is the same
    /// silent-revert this module refuses to do to a built-in edited in place.
    #[test]
    fn the_old_spelling_of_selection_area_is_still_read() {
        let old = parse(
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

        let listed = load_all(&dir);
        let mine = listed
            .iter()
            .find(|theme| theme.name == "My Theme")
            .expect("listed");
        assert_eq!(mine.id, "My Theme", "the id is the file's own stem");

        delete(&dir, &mine.id).expect("delete");
        assert!(!dir.join("My Theme.toml").exists());
        assert!(!load_all(&dir).iter().any(|theme| theme.name == "My Theme"));
    }

    /// And editing one changes it rather than producing a second copy beside
    /// it under a slugified name.
    #[test]
    fn editing_a_hand_written_theme_changes_that_file() {
        let dir = scratch("edit");
        hand_written(&dir, "My Theme.toml", "My Theme");
        let mut mine = load_all(&dir)
            .into_iter()
            .find(|theme| theme.name == "My Theme")
            .expect("listed");

        mine.background = "#222222".into();
        let saved = save(&dir, &mine).expect("save");
        assert_eq!(saved.id, "My Theme");

        let files: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| !is_built_in(name.trim_end_matches(".toml")))
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
        let problem = delete(&dir, "never-existed").expect_err("should report it");
        assert!(problem.contains("never-existed"), "{problem}");
    }

    /// An id crosses the bridge from the frontend, and a file name is the only
    /// thing it may be.
    #[test]
    fn an_id_that_is_not_a_file_name_is_refused() {
        let dir = scratch("escape");
        for id in ["../outside", "sub/theme", "", "   "] {
            assert!(path_for(&dir, id).is_none(), "{id:?} was allowed through");
            assert!(
                delete(&dir, id).is_err(),
                "{id:?} was allowed through delete"
            );
        }
        // And a name with nothing wrong with it still works.
        assert_eq!(path_for(&dir, "My Theme"), Some(dir.join("My Theme.toml")),);
    }

    /// A built-in is never overwritten, however it is asked for.
    #[test]
    fn editing_a_built_in_saves_a_copy() {
        let dir = scratch("built-in");
        install_built_ins(&dir);
        let original = fs::read_to_string(dir.join(format!("{DEFAULT_DARK}.toml"))).unwrap();

        let mut theme = load_all(&dir)
            .into_iter()
            .find(|theme| theme.id == DEFAULT_DARK)
            .expect("shipped");
        theme.background = "#010203".into();
        let saved = save(&dir, &theme).expect("save");

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
        install_built_ins(&dir);
        let mut theme = load_all(&dir)
            .into_iter()
            .find(|theme| theme.id == DEFAULT_DARK)
            .expect("shipped");
        theme.id = String::new();
        theme.name = " nord ".into();
        assert_eq!(
            save(&dir, &theme).unwrap_err(),
            "There is already a theme called nord."
        );
    }

    /// **A renamed theme renames its file**, where the app is the one that
    /// named it: `brownie.toml` saying `name = "Walnut"` is a directory
    /// nobody can read.
    #[test]
    fn renaming_a_theme_renames_the_file_the_app_named() {
        let dir = scratch("rename");
        let made = save(
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
                recolor: true,
                built_in: false,
            },
        )
        .expect("saved");
        assert_eq!(made.id, "brownie");

        let renamed = save(
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
        let listed = load_all(&dir);
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
        let mine = load_all(&dir)
            .into_iter()
            .find(|theme| theme.id == "My Theme")
            .expect("listed");

        let saved = save(
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
        let original = parse("x", source, true).expect("a shipped theme");
        let imported = import(&dir, &to_toml(&original).unwrap()).expect("imports");
        assert_eq!(imported.name, format!("{} 2", original.name));
        assert!(!imported.built_in);
        let again = import(&dir, &shipped(source)).expect("the banner and order are ignored");
        assert_ne!(again.id, imported.id);

        assert!(import(&dir, "title = \"nope\"").is_err());
        assert!(import(
            &dir,
            "name = \"X\"\ntext = \"steelblue\"\nbackground = \"#000\""
        )
        .is_err());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
    }
}
