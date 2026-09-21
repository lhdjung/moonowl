//! What changes on the disk while the reader is running.
//!
//! Phase 3 item 8. The watching itself is `src-tauri/src/watch.rs`, mounted
//! into this crate unchanged and carrying its own thirteen tests — a burst
//! settling, a document that is only half written, a rewrite of the same
//! length inside one tick of a coarse clock, a watch on a folder two windows
//! share. This file is about the half that is the *reader's*: what it does
//! when it is told.
//!
//! **Most of these post the news rather than causing it**, which is the
//! deterministic thing to do and is also how the app's own suite tests this
//! side of the bridge. The news is the news `watch.rs` emits, in the shape it
//! emits it, through the mailbox the reader really listens on — see
//! `crate::emit`. The last test is the one that goes the whole way: a real
//! watcher, a real file deleted, and a wait on a real file system.

use std::path::{Path, PathBuf};

use moonowl::harness::{Options, Reader};
use moonowl::{fixture, theme};

/// A settings directory nothing else is using.
fn scratch(name: &str) -> PathBuf {
    // **Canonical, and that is not tidiness.** On macOS the temp directory is
    // reached through a symlink — `/var` is `/private/var` — and the watcher
    // decides an event is about the themes directory by comparing the event's
    // parent with the directory it was given. The file system reports the
    // real path. `watch.rs` compares against both names now, so this is no
    // longer what makes the tests pass — it keeps the paths they print the
    // ones the file system would.
    let temp = std::fs::canonicalize(std::env::temp_dir()).expect("a real temp directory");
    let dir = temp.join(format!("moonowl-watch-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// A document of its own, at a path a test may rewrite.
///
/// A directory each, because a document is watched through its *folder* and
/// one shared folder would mean every test's drafts arriving as events in
/// every other test's watcher.
fn document(name: &str, pages: usize) -> PathBuf {
    let temp = std::fs::canonicalize(std::env::temp_dir()).expect("a real temp directory");
    let dir = temp.join(format!("moonowl-drafts-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a place for drafts");
    let path = dir.join(format!("{name}.pdf"));
    fixture::draft(&path, pages);
    path
}

fn reader_at(path: &Path, dir: &Path) -> Reader {
    Reader::open_with(
        &path.to_string_lossy(),
        Options {
            config: dir.to_path_buf(),
            ..Options::default()
        },
    )
}

/// A reader already wearing a named theme, which is what a reader who chose
/// one in an earlier run is.
fn reader_wearing(path: &Path, dir: &Path, id: &str) -> Reader {
    Reader::open_with(
        &path.to_string_lossy(),
        Options {
            config: dir.to_path_buf(),
            settings: vec![("theme".into(), serde_json::json!(id))],
            ..Options::default()
        },
    )
}

/// A theme somebody wrote, in the reader's themes directory. Its id is its
/// file name, which is what `load_all` reads it as — so the name inside the
/// file can change without the theme becoming a different theme, which is
/// what [`the_watcher_is_wired_to_the_reader`] leans on.
fn save_theme(dir: &Path, id: &str, name: &str, text: &str, background: &str) {
    let themes = dir.join("themes");
    std::fs::create_dir_all(&themes).expect("a themes directory");
    std::fs::write(
        themes.join(format!("{id}.toml")),
        format!("name = \"{name}\"\ntext = \"{text}\"\nbackground = \"{background}\"\n"),
    )
    .expect("write a theme");
}

fn write_theme(dir: &Path, id: &str, text: &str, background: &str) {
    save_theme(dir, id, id, text, background);
}

/// A patch of the first page, well inside it — what the paper's colour is
/// read off. In corners, because `mean` takes them rather than a size.
const PAGE: (u32, u32, u32, u32) = (350, 250, 550, 450);

/* --------------------------------------------------------------- themes */

/// A theme file rewritten beside the reader shows up in the reader, and that
/// is the whole reason themes are files.
///
/// The theme is one somebody wrote, for two reasons: it is the case the
/// feature exists for, and Moonowl Light does not recolour anything — editing
/// its paper is a change to the chrome and to nothing on the page, which is
/// what that theme is *for* and a poor thing to photograph.
#[test]
fn a_theme_edited_on_disk_is_worn_at_once() {
    let dir = scratch("edited");
    write_theme(&dir, "Mine", "#101010", "#f0f0f0");
    let mut reader = reader_wearing(&document("edited", 3), &dir, "Mine");
    let before = reader.screenshot().mean(PAGE);

    // The set as it is, with the theme in use given a different paper. This
    // is what the watcher hands over when an editor saves the file.
    let mut themes = theme::load_all(&dir.join("themes"));
    themes
        .iter_mut()
        .find(|theme| theme.name == "Mine")
        .expect("the theme in use is in the set")
        .background = "#204020".to_string();
    reader.themes_changed(&themes);

    let after = reader.screenshot().mean(PAGE);
    assert_ne!(
        before.map(|c| c.round()),
        after.map(|c| c.round()),
        "the page did not take the edited theme"
    );
    // Green, because that is what was asked for, and the page is mostly paper.
    assert!(
        after[1] > after[0] + 8.0 && after[1] > after[2] + 8.0,
        "{after:?}"
    );
    // And the reader is still wearing the same theme by name: an edit is not
    // a change of theme.
    assert_eq!(reader.state().theme, "Mine");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A theme whose file has gone takes the reader somewhere else, rather than
/// leaving the colours of something that no longer exists on screen.
///
/// **It has to be a theme somebody wrote**, and that is `theme.rs` being
/// right rather than this being awkward: a shipped theme is embedded in the
/// binary and `load_all` falls back to the embedded copy, so deleting
/// `moonowl-light.toml` deletes nothing a reader can see. The theme that can
/// actually vanish is the one that only ever existed as a file.
#[test]
fn a_theme_that_is_deleted_hands_the_reader_to_another() {
    let dir = scratch("deleted");
    write_theme(&dir, "Mine", "#e8e8e8", "#101018");
    let mut reader = reader_wearing(&document("deleted", 3), &dir, "Mine");
    assert_eq!(reader.state().theme, "Mine");

    std::fs::remove_file(dir.join("themes/Mine.toml")).expect("delete the theme");
    reader.themes_changed(&theme::load_all(&dir.join("themes")));

    let state = reader.state();
    assert_ne!(state.theme, "Mine", "still wearing a theme that is gone");
    assert!(
        state.notice.contains("Mine") && state.notice.contains(&state.theme),
        "{}",
        state.notice
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// And it is remembered, unlike an edit: a choice made on the reader's behalf
/// is still a choice, and the next run has to know what it was.
///
/// The theme deleted here is dark, so what it is replaced by is the dark
/// theme this reader remembers rather than whatever is first in the list —
/// which is `replacementFor` in `main.ts` and the reason it has an order.
#[test]
fn the_replacement_survives_the_run_that_chose_it() {
    let dir = scratch("replacement");
    let path = document("replacement", 3);
    write_theme(&dir, "Mine", "#e8e8e8", "#101018");
    let chosen = {
        let mut reader = reader_wearing(&path, &dir, "Mine");
        std::fs::remove_file(dir.join("themes/Mine.toml")).expect("delete the theme");
        reader.themes_changed(&theme::load_all(&dir.join("themes")));
        reader.state().theme
    };
    assert_eq!(
        chosen, "Moonowl Dark",
        "a dark theme was replaced by a light one"
    );

    let again = reader_at(&path, &dir);
    assert_eq!(again.state().theme, chosen);
    let _ = std::fs::remove_dir_all(&dir);
}

/* ------------------------------------------------------------- documents */

/// A paper recompiled underneath the reader: the reader stays where they were
/// and the new draft is what they are looking at.
#[test]
fn a_recompiled_document_is_reopened_where_the_reader_was() {
    let dir = scratch("recompiled");
    let path = document("recompiled", 12);
    let mut reader = reader_at(&path, &dir);
    reader.press("p");
    reader.type_text("6");
    reader.press("Enter");
    assert_eq!(reader.state().page, 6);

    fixture::draft(&path, 20);
    reader.document_changed(&path.to_string_lossy());

    let state = reader.state();
    assert_eq!(state.pages, 20, "the new draft was not read");
    assert_eq!(state.page, 6, "the reader lost their place");
    // **And nothing is said about it.** The reload notice was taken out — a
    // reader watching a paper rebuild sees the page redraw and, when the
    // `\title{}` moved, the toolbar change, which is the whole of the news;
    // the sentence only told somebody who does not know what a reload is that
    // something they did not do had happened to their file. This assertion
    // used to be its opposite, and was left behind when the notice went.
    assert!(
        !state.notice.contains("changed on disk"),
        "a reload is not news: {}",
        state.notice,
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A draft that lost its last chapter lands on the end of what is left.
/// `go_to` clamps, and paged mode would otherwise lay out a page that is not
/// there.
#[test]
fn a_document_that_got_shorter_lands_on_what_is_left() {
    let dir = scratch("shorter");
    let path = document("shorter", 12);
    let mut reader = reader_at(&path, &dir);
    reader.press("p");
    reader.type_text("11");
    reader.press("Enter");
    assert_eq!(reader.state().page, 11);

    fixture::draft(&path, 4);
    reader.document_changed(&path.to_string_lossy());

    let state = reader.state();
    assert_eq!(state.pages, 4);
    assert!(state.page <= 4 && state.page >= 1, "{}", state.page);
    let _ = std::fs::remove_dir_all(&dir);
}

/// News about somebody else's document is not this reader's business. It is
/// `emit_to` that keeps that true in the app, and this is the belt to its
/// braces: the path is checked against the document actually open.
#[test]
fn news_about_another_document_is_ignored() {
    let dir = scratch("elsewhere");
    let path = document("elsewhere", 12);
    let mut reader = reader_at(&path, &dir);
    let before = reader.state();

    let other = document("elsewhere-other", 3);
    reader.document_changed(&other.to_string_lossy());

    assert_eq!(
        reader.state(),
        before,
        "another document's news was acted on"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A document that calls itself something else after a rebuild is called that.
/// A paper whose `\title{}` changed between two runs of LaTeX is the ordinary
/// case, and the toolbar is where it shows.
#[test]
fn a_document_renamed_by_its_rebuild_is_renamed_in_the_toolbar() {
    let dir = scratch("renamed");
    let drafts = std::env::temp_dir().join(format!("moonowl-drafts-{}", std::process::id()));
    std::fs::create_dir_all(&drafts).expect("a place for drafts");
    let path = drafts.join("renamed.pdf");
    std::fs::copy(fixture::titled_pdf("An Early Draft of Something"), &path).expect("first draft");

    let mut reader = reader_at(&path, &dir);
    assert_eq!(reader.state().title, "An Early Draft of Something");

    std::fs::copy(fixture::titled_pdf("What It Was Actually About"), &path).expect("second draft");
    reader.document_changed(&path.to_string_lossy());

    assert_eq!(reader.state().title, "What It Was Actually About");
    let _ = std::fs::remove_dir_all(&dir);
}

/* ------------------------------------------------------- and really wired */

/// The one test with the file system in it.
///
/// Everything above posts the news; this causes it. A theme file is deleted
/// beside a reader that has the real watcher running, and the reader is
/// expected to notice on its own — which exercises `notify`, the settle
/// window, the load-and-compare that decides a theme reload is news, the
/// mailbox, the waker, and the task, in that order.
///
/// It waits on a clock because there is nothing else to wait on: the file
/// system reports when the platform says so. The deadline is generous and the
/// assertion is on the state, so a slow machine reports what it was stuck on.
#[test]
fn the_watcher_is_wired_to_the_reader() {
    let dir = scratch("wired");
    write_theme(&dir, "Mine", "#101010", "#f0f0f0");
    let mut reader = Reader::open_with(
        &document("wired", 3).to_string_lossy(),
        Options {
            config: dir.clone(),
            settings: vec![("theme".into(), serde_json::json!("Mine"))],
            watch: true,
            ..Options::default()
        },
    );
    assert_eq!(reader.state().theme, "Mine");

    // **Saved more than once, and the first one waited for.** The watch is
    // set up on a thread of its own and nothing anywhere says when it is up,
    // so a save made before that produces no event at all and no amount of
    // waiting afterwards conjures one. The pause is what gives it a chance
    // and the repeat is what makes a slow machine a slow pass rather than a
    // failure — and saving the same file again is what an editor does anyway.
    // Each round is longer than the watcher's own settle window, or the burst
    // never ends and nothing is ever reported.
    //
    // **And each round writes a different name**, which is the half that was
    // missing and made this test fail for good rather than slowly. The
    // watcher decides a theme reload by comparing what it has just loaded
    // against the last set it handed over — `known` in `watch.rs`, and that
    // is the right rule: the app writes into this directory itself on every
    // launch, and a write that changes nothing is not news. So a round that
    // saved the same bytes as the round before produced no event at all, and
    // once the first round's news had been missed no later round could ever
    // arrive. Renaming per round is what an editor saving twice actually
    // does, and it is what the retry needs to mean anything.
    let mut noticed = false;
    for round in 1..=6 {
        let renamed = format!("Renamed {round}");
        std::thread::sleep(std::time::Duration::from_millis(600));
        save_theme(&dir, "Mine", &renamed, "#101010", "#f0f0f0");
        if reader.wait_until(4.0, |reader| reader.state().theme == renamed) {
            noticed = true;
            break;
        }
    }

    let state = reader.state();
    assert!(noticed, "the reader never heard about it: {state:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
