//! Page edits: saving pages as a new file, combining files, and removing,
//! turning, moving and inserting pages in the open document.

use dioxus::html::Modifiers;
use moonowl::app::{Ask, Choosing};
use moonowl::arrange;
use moonowl::fixture;
use moonowl::harness::{Options, Reader};
use moonowl::render;

/// A directory of this test's own, because every test here writes files.
fn folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("moonowl-pages-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory to write in");
    dir
}

/// A copy of the prose fixture, six pages with one line of words on each,
/// in `dir`.
fn prose_in(dir: &std::path::Path) -> String {
    let path = dir.join("prose.pdf");
    std::fs::copy(fixture::prose_pdf(), &path).expect("a copy of the fixture");
    path.to_string_lossy().into_owned()
}

/// The words on each page of the document at `path`.
fn words(path: &str) -> Vec<String> {
    let document = render::open(path).expect("the document opens");
    (0..document.pages())
        .map(|index| document.text_of(index).chars.iter().collect::<String>())
        .map(|text| text.trim().to_string())
        .collect()
}

fn path_of(dir: &std::path::Path, name: &str) -> String {
    dir.join(name).to_string_lossy().into_owned()
}

#[test]
fn saving_pages_writes_exactly_those_pages_and_leaves_the_document_alone() {
    let dir = folder("save");
    let path = prose_in(&dir);
    let before = std::fs::read(&path).unwrap();
    let to = path_of(&dir, "some.pdf");
    arrange::save_pages(&path, None, &[3, 4, 1], &to).expect("the pages are saved");
    let all = words(&path);
    assert_eq!(
        words(&to),
        vec![all[2].clone(), all[3].clone(), all[0].clone()]
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "the document is unchanged"
    );
}

#[test]
fn combining_counts_the_pages_of_every_source() {
    let dir = folder("combine");
    let one = prose_in(&dir);
    let other = path_of(&dir, "draft.pdf");
    fixture::draft(std::path::Path::new(&other), 3);
    let to = path_of(&dir, "both.pdf");
    let pages = arrange::combine(&[one.clone(), other], &to).expect("combined");
    assert_eq!(pages, 9);
    assert_eq!(render::open(&to).unwrap().pages(), 9);
    assert_eq!(words(&to)[..6], words(&one)[..]);
}

#[test]
fn an_encrypted_file_is_not_combined() {
    let dir = folder("locked");
    let to = path_of(&dir, "both.pdf");
    let refused = arrange::combine(&[fixture::locked_pdf()], &to);
    assert!(refused.is_err_and(|said| said.contains("encrypted")));
    assert!(!std::path::Path::new(&to).exists());
}

#[test]
fn the_document_menu_saves_the_pages_typed() {
    let dir = folder("menu");
    let path = prose_in(&dir);
    let to = path_of(&dir, "chapter.pdf");
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![to.clone()]],
            ..Options::default()
        },
    );
    reader.click(".chip.title");
    assert_eq!(
        reader.field(".menu-pages-field"),
        "1",
        "the page being read"
    );
    reader.click(".menu-pages-field");
    reader.press_chord("mod+a");
    reader.type_text("2-3");
    reader.click("[data-item=save-pages]");
    reader.settle();
    let (asked, suggested) = reader.dialogs().pop().expect("a save dialog");
    assert_eq!(asked, Choosing::SavePages(vec![2, 3]));
    assert_eq!(
        suggested.as_deref(),
        Some(path_of(&dir, "prose (pages 2 and 3).pdf").as_str())
    );
    assert_eq!(words(&to), words(&path)[1..3].to_vec());
    // The notice, and the Open button beside it.
    assert_eq!(
        reader.state().notice,
        "Saved pages 2 and 3 as chapter.pdf.Open"
    );
    reader.click(".notice-open");
    assert!(
        reader.asks().contains(&Ask::NewWindowOn(to.clone())),
        "Open opens the new file beside this one: {:?}",
        reader.asks()
    );
}

#[test]
fn pages_cannot_be_saved_over_their_own_document() {
    let dir = folder("over");
    let path = prose_in(&dir);
    let before = std::fs::read(&path).unwrap();
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![path.clone()]],
            ..Options::default()
        },
    );
    reader.click(".chip.title");
    reader.click("[data-item=save-pages]");
    reader.settle();
    assert!(reader.state().notice.starts_with("Choose a new name."));
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn the_start_screen_combines_files_and_opens_the_result() {
    let dir = folder("start");
    let one = prose_in(&dir);
    let other = path_of(&dir, "draft.pdf");
    fixture::draft(std::path::Path::new(&other), 2);
    let to = path_of(&dir, "together.pdf");
    let mut reader = Reader::empty(Options {
        chooses: vec![vec![one.clone(), other.clone()], vec![to.clone()]],
        ..Options::default()
    });
    reader.click(".start-combine");
    reader.settle();
    let dialogs = reader.dialogs();
    assert_eq!(dialogs[0], (Choosing::Combine, None));
    assert_eq!(
        dialogs[1],
        (
            Choosing::CombineInto(vec![one, other]),
            Some(path_of(&dir, "prose (combined).pdf"))
        )
    );
    assert_eq!(reader.state().pages, 8, "the combined file is open");
}

#[test]
fn a_cancelled_dialog_saves_nothing() {
    let dir = folder("cancel");
    let path = prose_in(&dir);
    let mut reader = Reader::open(&path);
    reader.click(".chip.title");
    reader.click("[data-item=save-pages]");
    reader.settle();
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    assert_eq!(reader.state().notice, "");
}

/// A reader on `path` with the thumbnails showing.
fn with_thumbnails(path: &str, options: Options) -> Reader {
    let mut reader = Reader::open_with(path, options);
    reader.press_chord("mod+b");
    reader.click(".tab[data-tab='pages']");
    reader
}

/// The middle of a thumbnail's picture, in the window. The pointer moves
/// first, because layout is brought up to date by the next event, and a
/// turned page has changed the rows.
fn thumb(reader: &mut Reader, page: usize) -> (f32, f32) {
    reader.point_to(1.0, 1.0);
    let picture = reader
        .harness
        .layout_rect(&format!(".thumb[data-thumb='{page}'] .thumb-picture"));
    (
        picture.x + picture.width / 2.0,
        picture.y + picture.height / 2.0,
    )
}

/// Right-click a thumbnail and choose a row of its menu.
fn from_thumb(reader: &mut Reader, page: usize, item: &str) {
    let (x, y) = thumb(reader, page);
    reader.right_click_at(x, y);
    reader.click(&format!(".pages-menu [data-item={item}]"));
    reader.settle();
}

/// The first edit's question, answered "Change this file".
fn in_place(reader: &mut Reader) {
    reader.click(".ask-also");
    reader.settle();
}

fn marked_pages(reader: &Reader) -> Vec<u32> {
    reader.flush();
    moonowl::store::flush();
    moonowl::library::load(&reader.config)
        .files
        .iter()
        .flat_map(|entry| entry.marks.iter().map(|mark| mark.page))
        .collect()
}

#[test]
fn removing_a_page_moves_the_rest_up_and_undo_puts_it_back() {
    let dir = folder("remove");
    let path = prose_in(&dir);
    let all = words(&path);
    let mut reader = with_thumbnails(&path, Options::default());
    let (x, y) = thumb(&mut reader, 3);
    reader.click_at(x, y);
    reader.press_chord("mod+shift+b");
    assert_eq!(marked_pages(&reader), [3]);
    // A bookmark shows the Contents; back to the pages, and up to the top.
    reader.click(".tab[data-tab='pages']");
    reader.wheel_over(".panel.thumb-column", -5000.0);

    from_thumb(&mut reader, 2, "remove-pages");
    in_place(&mut reader);
    assert_eq!(reader.state().pages, 5);
    let now = words(&path);
    assert_eq!(now[1], all[2], "the old page 3 is page 2");
    assert_eq!(marked_pages(&reader), [2], "and its bookmark went with it");
    assert_eq!(reader.state().page, 2, "the reader is still on that page");
    assert!(
        reader.state().notice.starts_with("Removed page 2. "),
        "{}",
        reader.state().notice
    );

    reader.press_chord("mod+z");
    reader.settle();
    assert_eq!(reader.state().pages, 6);
    assert_eq!(words(&path), all);
    assert_eq!(
        marked_pages(&reader),
        [3],
        "the bookmark is back where it was"
    );
}

#[test]
fn a_highlight_on_a_removed_page_leaves_and_comes_back_with_undo() {
    let dir = folder("highlight");
    let path = prose_in(&dir);
    let document = render::open(&path).unwrap();
    let text = document.text_of(1);
    let quads = text.quads(0, 7);
    drop(document);
    moonowl::markup::add(&path, &[(2, quads)], "#ffd60a", "Moonowl").expect("marked");
    let mut reader = with_thumbnails(&path, Options::default());
    let journal = |reader: &Reader| {
        reader.flush();
        moonowl::store::flush();
        (moonowl::library::load(&reader.config).files.iter())
            .flat_map(|entry| entry.highlights.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(journal(&reader).len(), 1);

    from_thumb(&mut reader, 2, "remove-pages");
    in_place(&mut reader);
    assert!(render::open(&path).unwrap().markup().is_empty());
    assert!(journal(&reader).is_empty(), "the journal let it go");

    reader.press_chord("mod+z");
    reader.settle();
    let marks = render::open(&path).unwrap().markup();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].page, 2);
    assert_eq!(journal(&reader).len(), 1, "and the journal has it again");
}

#[test]
fn removing_the_page_a_heading_points_at_says_so() {
    let dir = folder("headings");
    let path = path_of(&dir, "contents.pdf");
    std::fs::copy(fixture::contents_pdf(), &path).unwrap();
    let headings = fixture::expected_headings();
    let page = headings[1].2;
    let on_it = headings.iter().filter(|(_, _, at)| *at == page).count();
    let mut reader = with_thumbnails(&path, Options::default());
    reader.click(".tab[data-tab='pages']");
    from_thumb(&mut reader, page, "remove-pages");
    in_place(&mut reader);
    let notice = reader.state().notice;
    let said = if on_it == 1 {
        "A heading pointed at it and now leads nowhere.".to_string()
    } else {
        format!("{on_it} headings pointed at it and now lead nowhere.")
    };
    assert!(notice.contains(&said), "{notice}");
}

#[test]
fn the_last_page_cannot_be_removed() {
    let dir = folder("last");
    let path = path_of(&dir, "one.pdf");
    fixture::draft(std::path::Path::new(&path), 1);
    let mut reader = with_thumbnails(&path, Options::default());
    from_thumb(&mut reader, 1, "remove-pages");
    assert_eq!(reader.state().pages, 1);
    assert_eq!(reader.state().notice, "A document needs at least one page.");
}

fn entry(reader: &Reader, path: &str) -> Option<moonowl::library::Entry> {
    reader.flush();
    moonowl::store::flush();
    (moonowl::library::load(&reader.config).files.into_iter()).find(|entry| entry.path == path)
}

#[test]
fn the_first_edit_goes_into_a_copy_and_the_window_reads_on_in_it() {
    let dir = folder("copy");
    let path = prose_in(&dir);
    let before = std::fs::read(&path).unwrap();
    let all = words(&path);
    let copy = path_of(&dir, "prose (edited).pdf");
    let mut reader = with_thumbnails(
        &path,
        Options {
            chooses: vec![vec![copy.clone()]],
            ..Options::default()
        },
    );
    let (x, y) = thumb(&mut reader, 4);
    reader.click_at(x, y);
    reader.press_chord("mod+shift+b");
    reader.click(".tab[data-tab='pages']");
    reader.wheel_over(".panel.thumb-column", -5000.0);

    from_thumb(&mut reader, 2, "remove-pages");
    assert_eq!(
        reader.harness.text_content(".ask-window .pane-lede"),
        "Page edits go into a copy, so prose.pdf stays as it is."
    );
    // Edit a copy… is the default, and Enter presses it.
    reader.press("Enter");
    reader.settle();
    assert_eq!(
        reader.dialogs().last(),
        Some(&(
            Choosing::Copy(arrange::Edit::Remove(vec![2])),
            Some(copy.clone())
        ))
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "the original is untouched"
    );
    let mut wanted = all.clone();
    wanted.remove(1);
    assert_eq!(words(&copy), wanted, "the copy has the edit");
    assert_eq!(reader.state().pages, 5);
    assert!(
        reader.asks().iter().any(|ask| matches!(
            ask,
            Ask::Showing { path, .. } if *path == copy
        )),
        "the window shows the copy: {:?}",
        reader.asks()
    );
    assert_eq!(
        reader.state().notice,
        "Saved the edit as prose (edited).pdf. From now on, page edits go into this copy."
    );
    let marks = |entry: Option<moonowl::library::Entry>| {
        entry.map(|entry| entry.marks.iter().map(|mark| mark.page).collect::<Vec<_>>())
    };
    assert_eq!(
        marks(entry(&reader, &copy)),
        Some(vec![3]),
        "the bookmark moved"
    );
    assert_eq!(
        marks(entry(&reader, &path)),
        Some(vec![4]),
        "the original's is kept"
    );

    // The next edit goes into the copy, and no other file is made.
    reader.wheel_over(".panel.thumb-column", -5000.0);
    let (x, y) = thumb(&mut reader, 1);
    reader.right_click_at(x, y);
    reader.click(".pages-menu [data-item=remove-pages]");
    reader.settle();
    assert!(
        reader.harness.query(".ask-window").is_none(),
        "nothing is asked"
    );
    assert_eq!(words(&copy).len(), 4);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);

    // Undo twice: the copy is the original again, and stays on the disk.
    reader.press_chord("mod+z");
    reader.settle();
    reader.press_chord("mod+z");
    reader.settle();
    assert_eq!(words(&copy), all);
}

#[test]
fn cancelling_the_copy_changes_nothing() {
    let dir = folder("no-copy");
    let path = prose_in(&dir);
    let before = std::fs::read(&path).unwrap();
    let mut reader = with_thumbnails(&path, Options::default());
    from_thumb(&mut reader, 2, "remove-pages");
    reader.click(".ask-go");
    reader.settle();
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    assert_eq!(reader.state().pages, 6);
    assert!(entry(&reader, &path).is_some_and(|entry| entry.marks.is_empty()));
    // And the next edit asks again.
    from_thumb(&mut reader, 2, "remove-pages");
    assert!(reader.harness.query(".ask-window").is_some());
}

#[cfg(unix)]
#[test]
fn a_read_only_document_is_edited_into_a_copy_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = folder("read-only");
    let path = prose_in(&dir);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    let copy = path_of(&dir, "copy.pdf");
    let mut reader = with_thumbnails(
        &path,
        Options {
            chooses: vec![vec![copy.clone()]],
            ..Options::default()
        },
    );
    from_thumb(&mut reader, 2, "remove-pages");
    in_place(&mut reader);
    assert_eq!(
        reader.state().notice,
        "This document is read-only, so page edits can only go into a copy."
    );
    assert_eq!(reader.state().pages, 6);
    from_thumb(&mut reader, 2, "remove-pages");
    reader.click(".ask-go");
    reader.settle();
    assert_eq!(words(&copy).len(), 5);
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644));
}

fn pages(path: &str) -> usize {
    render::open(path).unwrap().pages()
}

fn command() -> Modifiers {
    if cfg!(target_os = "macos") {
        Modifiers::META
    } else {
        Modifiers::CONTROL
    }
}

fn picked(reader: &Reader) -> Vec<String> {
    reader.attribute_all(".thumb.picked", "data-thumb")
}

#[test]
fn thumbnails_are_selected_as_files_are() {
    let dir = folder("select");
    let path = prose_in(&dir);
    let mut reader = with_thumbnails(&path, Options::default());
    let (x, y) = thumb(&mut reader, 2);
    reader.click_at(x, y);
    assert_eq!(picked(&reader), ["2"], "a click selects the page alone");
    assert_eq!(reader.state().page, 2, "and goes to it");
    assert!(
        reader.harness.query(".thumb-foot").is_none(),
        "one page has no bar"
    );
    assert_eq!(reader.state().pages, 6, "and nothing moved");

    let (x, y) = thumb(&mut reader, 4);
    reader.click_at_with(x, y, command());
    assert_eq!(picked(&reader), ["2", "4"]);
    assert_eq!(reader.state().page, 2, "⌘-click does not go to the page");
    assert_eq!(
        reader.harness.text_content(".thumb-foot-count"),
        "2 pages selected"
    );
    let (x, y) = thumb(&mut reader, 1);
    reader.click_at_with(x, y, Modifiers::SHIFT);
    assert_eq!(
        picked(&reader),
        ["1", "2", "3", "4"],
        "⇧-click runs from the last"
    );
}

#[test]
fn delete_removes_the_selection_only_after_a_click_in_the_column() {
    let dir = folder("delete");
    let path = prose_in(&dir);
    let mut reader = with_thumbnails(&path, Options::default());
    let (x, y) = thumb(&mut reader, 2);
    reader.click_at(x, y);
    reader.click_on_page(2, (0.5, 0.5));
    reader.press("Delete");
    assert!(
        reader.harness.query(".ask-window").is_none(),
        "Delete did nothing"
    );
    assert_eq!(reader.state().pages, 6);

    let before = reader.state().page;
    reader.press("Backspace");
    assert!(
        reader.state().page <= before,
        "Backspace is still Back, and removes nothing"
    );
    assert_eq!(reader.state().pages, 6);

    reader.click(".tab[data-tab='pages']");
    let (x, y) = thumb(&mut reader, 2);
    reader.click_at(x, y);
    reader.press("Delete");
    in_place(&mut reader);
    assert_eq!(reader.state().pages, 5);
}

#[test]
fn command_a_selects_every_page_after_a_click_in_the_column() {
    let dir = folder("all");
    let path = prose_in(&dir);
    let mut reader = with_thumbnails(&path, Options::default());
    let (x, y) = thumb(&mut reader, 1);
    reader.click_at(x, y);
    reader.press_chord("mod+a");
    assert_eq!(
        reader.harness.text_content(".thumb-foot-count"),
        "6 pages selected"
    );
    // After a click on the page, ⌘A is the page's text again.
    reader.click_on_page(1, (0.5, 0.5));
    reader.press_chord("mod+a");
    assert!(
        reader.harness.query(".selected").is_some(),
        "the page's text is selected"
    );
}

#[test]
fn removing_many_pages_from_the_file_itself_asks_first() {
    let dir = folder("many");
    let path = path_of(&dir, "long.pdf");
    fixture::draft(std::path::Path::new(&path), 9);
    let mut reader = with_thumbnails(&path, Options::default());
    let (x, y) = thumb(&mut reader, 1);
    reader.click_at(x, y);
    reader.press_chord("mod+a");
    // Eight of nine: everything but the first page.
    reader.click_at_with(x, y, command());
    assert_eq!(
        reader.harness.text_content(".thumb-foot-count"),
        "8 pages selected"
    );
    reader.click(".thumb-foot-remove");
    in_place(&mut reader);
    assert_eq!(pages(&path), 9, "not yet");
    assert_eq!(
        reader.harness.text_content(".ask-window .window-title"),
        "Remove 8 pages?"
    );
    reader.click(".ask-go");
    reader.settle();
    assert_eq!(pages(&path), 1);
}

#[test]
fn removing_five_pages_from_the_file_does_not_ask_again() {
    let dir = folder("five");
    let path = path_of(&dir, "long.pdf");
    fixture::draft(std::path::Path::new(&path), 9);
    let mut reader = with_thumbnails(&path, Options::default());
    let (x, y) = thumb(&mut reader, 1);
    reader.click_at(x, y);
    let (x, y) = thumb(&mut reader, 2);
    reader.click_at_with(x, y, Modifiers::SHIFT);
    reader.click(".thumb-foot-remove");
    in_place(&mut reader);

    assert_eq!(pages(&path), 7);
    // Five more, and still no question: only more than five are asked about.
    // All seven but the first two.
    let (x, y) = thumb(&mut reader, 1);
    reader.click_at(x, y);
    reader.press_chord("mod+a");
    reader.click_at_with(x, y, command());
    let (x, y) = thumb(&mut reader, 2);
    reader.click_at_with(x, y, command());
    assert_eq!(
        reader.harness.text_content(".thumb-foot-count"),
        "5 pages selected"
    );
    reader.click(".thumb-foot-remove");
    assert!(reader.harness.query(".ask-window").is_none());
    assert_eq!(pages(&path), 2);
}

#[test]
fn removing_many_pages_from_a_copy_does_not_ask() {
    let dir = folder("many-copy");
    let path = path_of(&dir, "long.pdf");
    fixture::draft(std::path::Path::new(&path), 9);
    let copy = path_of(&dir, "copy.pdf");
    let mut reader = with_thumbnails(
        &path,
        Options {
            chooses: vec![vec![copy.clone()]],
            ..Options::default()
        },
    );
    let (x, y) = thumb(&mut reader, 1);
    reader.click_at(x, y);
    reader.press_chord("mod+a");
    reader.click_at_with(x, y, command());
    reader.click(".thumb-foot-remove");
    reader.click(".ask-go");
    reader.settle();
    assert_eq!(pages(&copy), 1);
    assert_eq!(pages(&path), 9, "the document itself is as it was");
}

#[test]
fn the_selection_follows_the_pages() {
    let dir = folder("follow");
    let path = prose_in(&dir);
    let mut reader = with_thumbnails(&path, Options::default());
    let (x, y) = thumb(&mut reader, 4);
    reader.click_at(x, y);
    let (x, y) = thumb(&mut reader, 5);
    reader.click_at_with(x, y, command());
    reader.wheel_over(".panel.thumb-column", -5000.0);
    from_thumb(&mut reader, 1, "remove-pages");
    in_place(&mut reader);
    assert_eq!(
        picked(&reader),
        Vec::<String>::new(),
        "page 1 alone was selected"
    );
    let (x, y) = thumb(&mut reader, 3);
    reader.click_at(x, y);
    let (x, y) = thumb(&mut reader, 4);
    reader.click_at_with(x, y, command());
    reader.wheel_over(".panel.thumb-column", -5000.0);
    let (x, y) = thumb(&mut reader, 1);
    reader.click_at_with(x, y, command());
    reader.press("Delete");
    reader.settle();
    assert_eq!(reader.state().pages, 2);
}

#[test]
fn turning_a_page_writes_its_rotation_and_keeps_its_highlight_on_its_words() {
    let dir = folder("turn");
    let path = prose_in(&dir);
    let document = render::open(&path).unwrap();
    let (quads, size) = (document.text_of(0).quads(0, 8), document.size_of(0));
    drop(document);
    moonowl::markup::add(&path, &[(1, quads)], "#ffd60a", "Moonowl").expect("marked");
    let quoted = |path: &str| {
        let document = render::open(path).unwrap();
        let mark = document.markup()[0].clone();
        moonowl::markup::quote_under(&document.text_of(0), &mark.quads)
    };
    let before = quoted(&path);

    let mut reader = with_thumbnails(&path, Options::default());
    from_thumb(&mut reader, 1, "turn-right");
    in_place(&mut reader);
    assert_eq!(reader.state().notice, "Page 1 is turned right in the file.");
    let document = render::open(&path).unwrap();
    let turned = document.size_of(0);
    assert_eq!(
        (turned.width, turned.height),
        (size.height, size.width),
        "the page is on its side"
    );
    assert!(
        String::from_utf8_lossy(&std::fs::read(&path).unwrap()).contains("/Rotate 90"),
        "and says so in the file"
    );
    assert_eq!(document.pages(), 6, "and nothing else changed");
    drop(document);
    assert_eq!(quoted(&path), before, "the highlight is still on its words");

    // And back the other way, from the bar under a selection.
    let (x, y) = thumb(&mut reader, 1);
    reader.click_at(x, y);
    let (x, y) = thumb(&mut reader, 2);
    reader.click_at_with(x, y, Modifiers::SHIFT);
    reader.click(".thumb-foot [data-item=turn-right]");
    reader.settle();
    let document = render::open(&path).unwrap();
    let (one, two) = (document.size_of(0), document.size_of(1));
    assert_eq!((one.width, one.height), (size.width, size.height));
    assert_eq!((two.width, two.height), (size.height, size.width));
}

#[test]
fn moving_pages_to_the_end_keeps_their_highlights_and_the_reader_with_them() {
    let dir = folder("move");
    let path = prose_in(&dir);
    let all = words(&path);
    let document = render::open(&path).unwrap();
    let quads = document.text_of(0).quads(0, 8);
    drop(document);
    moonowl::markup::add(&path, &[(1, quads)], "#ffd60a", "Moonowl").expect("marked");
    let mut reader = with_thumbnails(&path, Options::default());
    let (x, y) = thumb(&mut reader, 1);
    reader.click_at(x, y);
    let (x, y) = thumb(&mut reader, 2);
    reader.click_at_with(x, y, Modifiers::SHIFT);
    let (x, y) = thumb(&mut reader, 1);
    reader.right_click_at(x, y);
    reader.click(".pages-menu [data-item=move-to]");
    reader.type_text("7");
    reader.press("Enter");
    reader.settle();
    in_place(&mut reader);
    let now = words(&path);
    assert_eq!(now[..4], all[2..]);
    assert_eq!(now[4..], all[..2]);
    let marks = render::open(&path).unwrap().markup();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].page, 5, "the highlight went with its page");
    assert_eq!(reader.state().page, 5, "and so did the reader");
    assert_eq!(reader.state().notice, "Moved 2 pages to the end.");
    assert_eq!(
        reader.harness.text_content(".thumb-foot-count"),
        "2 pages selected",
        "the pages moved are still selected"
    );
}

#[test]
fn moving_pages_to_where_they_are_writes_nothing() {
    let dir = folder("stay");
    let path = prose_in(&dir);
    let before = std::fs::read(&path).unwrap();
    let mut reader = with_thumbnails(&path, Options::default());
    let (x, y) = thumb(&mut reader, 2);
    reader.right_click_at(x, y);
    reader.click(".pages-menu [data-item=move-to]");
    reader.type_text("3");
    reader.press("Enter");
    reader.settle();
    assert!(reader.harness.query(".ask-window").is_none());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

/// Press a thumbnail, carry it to `to` in steps, and let go there.
fn drag_thumb(reader: &mut Reader, page: usize, to: (f32, f32), escape: bool) {
    let from = thumb(reader, page);
    reader.harness.mouse_down_at(from.0, from.1);
    for step in 1..=4 {
        let t = step as f32 / 4.0;
        reader.carry(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
    }
    reader.settle();
    if escape {
        reader.press("Escape");
    }
    reader.harness.mouse_up_at(to.0, to.1);
    reader.settle();
}

#[test]
fn a_thumbnail_dragged_between_two_others_moves_its_page_there() {
    let dir = folder("drag");
    let path = prose_in(&dir);
    let all = words(&path);
    let mut reader = with_thumbnails(&path, Options::default());
    reader.point_to(1.0, 1.0);
    let three = reader
        .harness
        .layout_rect(".thumb[data-thumb='3'] .thumb-picture");
    // Just inside the top of page 3: before it.
    drag_thumb(&mut reader, 1, (three.x + 20.0, three.y + 6.0), false);
    in_place(&mut reader);
    let mut wanted = all.clone();
    wanted.swap(0, 1);
    assert_eq!(words(&path), wanted);
    assert_eq!(reader.state().notice, "Moved page 1 to before page 3.");
    assert!(
        reader.harness.query(".thumb-drop").is_none(),
        "the line has gone"
    );
}

#[test]
fn a_press_that_hardly_moves_is_a_click_and_moves_nothing() {
    let dir = folder("nudge");
    let path = prose_in(&dir);
    let before = std::fs::read(&path).unwrap();
    let mut reader = with_thumbnails(&path, Options::default());
    let (x, y) = thumb(&mut reader, 3);
    reader.harness.mouse_down_at(x, y);
    reader.carry(x + 2.0, y + 2.0);
    reader.harness.mouse_up_at(x + 2.0, y + 2.0);
    reader.settle();
    assert_eq!(reader.state().page, 3, "it went to the page");
    assert!(reader.harness.query(".ask-window").is_none());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn escape_puts_dragged_thumbnails_back() {
    let dir = folder("escape");
    let path = prose_in(&dir);
    let before = std::fs::read(&path).unwrap();
    let mut reader = with_thumbnails(&path, Options::default());
    reader.point_to(1.0, 1.0);
    let three = reader
        .harness
        .layout_rect(".thumb[data-thumb='3'] .thumb-picture");
    drag_thumb(&mut reader, 1, (three.x + 20.0, three.y + 6.0), true);
    assert!(reader.harness.query(".ask-window").is_none());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // And let go outside the column, too.
    drag_thumb(&mut reader, 1, (900.0, 400.0), false);
    assert!(reader.harness.query(".ask-window").is_none());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn a_dragged_thumbnail_shows_where_it_will_land() {
    let dir = folder("line");
    let path = prose_in(&dir);
    let mut reader = with_thumbnails(&path, Options::default());
    let from = thumb(&mut reader, 1);
    reader.harness.mouse_down_at(from.0, from.1);
    let three = reader
        .harness
        .layout_rect(".thumb[data-thumb='3'] .thumb-picture");
    reader.carry(from.0, from.1 + 30.0);
    reader.carry(three.x + 20.0, three.y + 6.0);
    reader.settle();
    let line = reader.box_of(".thumb-drop").expect("the line");
    assert!(
        line.1 < three.y && line.1 > three.y - 20.0,
        "between pages 2 and 3: {line:?} against {three:?}"
    );
    assert_eq!(reader.harness.text_content(".thumb-lift"), "Page 1");
    reader.press("Escape");
    reader.harness.mouse_up_at(three.x + 20.0, three.y + 6.0);
    reader.settle();
}
