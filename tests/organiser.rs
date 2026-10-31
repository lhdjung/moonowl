//! The page organiser: pages arranged in a grid and saved as one file or
//! several, or into the document itself.

use dioxus::html::Modifiers;
use moonowl::app::Choosing;
use moonowl::fixture;
use moonowl::harness::{Options, Reader};
use moonowl::render;

/// A directory of this test's own, because every test here writes files.
fn folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("moonowl-organiser-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory to write in");
    dir
}

/// A copy of the prose fixture, six pages with one line of words on each.
fn prose_in(dir: &std::path::Path) -> String {
    let path = dir.join("prose.pdf");
    std::fs::copy(fixture::prose_pdf(), &path).expect("a copy of the fixture");
    path.to_string_lossy().into_owned()
}

fn path_of(dir: &std::path::Path, name: &str) -> String {
    dir.join(name).to_string_lossy().into_owned()
}

/// The words on each page of the document at `path`.
fn words(path: &str) -> Vec<String> {
    let document = render::open(path).expect("the document opens");
    (0..document.pages())
        .map(|index| document.text_of(index).chars.iter().collect::<String>())
        .map(|text| text.trim().to_string())
        .collect()
}

/// "Arrange pages…" from the Document menu.
fn arrange(reader: &mut Reader) {
    reader.click(".chip.title");
    reader.click("[data-item=arrange]");
    reader.settle();
}

/// The middle of a page in the grid, in the window.
fn sheet(reader: &mut Reader, at: usize) -> (f32, f32) {
    reader.point_to(1.0, 1.0);
    let picture = reader
        .harness
        .layout_rect(&format!(".sheet[data-sheet='{at}'] .sheet-picture"));
    (
        picture.x + picture.width / 2.0,
        picture.y + picture.height / 2.0,
    )
}

fn click_sheet(reader: &mut Reader, at: usize, modifiers: Modifiers) {
    let (x, y) = sheet(reader, at);
    reader.click_at_with(x, y, modifiers);
    reader.settle();
}

fn picked(reader: &Reader) -> Vec<String> {
    reader.attribute_all(".sheet.picked", "data-sheet")
}

#[test]
fn it_opens_on_the_page_being_read_and_saves_into_the_document_with_undo() {
    let dir = folder("into");
    let path = prose_in(&dir);
    let all = words(&path);
    let mut reader = Reader::open(&path);
    reader.press_chord("mod+shift+b");
    arrange(&mut reader);
    assert!(reader.harness.query(".organiser").is_some());
    assert_eq!(picked(&reader), ["0"], "the page being read");
    click_sheet(&mut reader, 1, Modifiers::empty());
    reader.click("[data-item=remove-pages]");
    reader.settle();
    assert_eq!(reader.text_all(".organiser-count"), ["5 pages"]);
    reader.click("[data-item=save-into]");
    reader.settle();
    assert!(
        reader.harness.query(".organiser").is_none(),
        "it closes once written"
    );
    let mut wanted = all.clone();
    wanted.remove(1);
    assert_eq!(words(&path), wanted);
    let notice = reader.state().notice;
    assert!(
        notice.starts_with("Arranged prose.pdf: removed 1 page. ")
            && notice.ends_with(" puts it back."),
        "{notice}"
    );
    reader.press_chord("mod+z");
    reader.settle();
    assert_eq!(words(&path), all, "undo puts the whole file back");
}

#[test]
fn a_save_into_a_document_changed_on_disk_puts_the_draft_away() {
    let dir = folder("changed");
    let path = prose_in(&dir);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    click_sheet(&mut reader, 1, Modifiers::empty());
    reader.click("[data-item=remove-pages]");
    reader.settle();
    fixture::draft(std::path::Path::new(&path), 2);
    reader.click("[data-item=save-into]");
    reader.settle();
    assert!(reader.harness.query(".organiser").is_none());
    assert_eq!(words(&path).len(), 2, "nothing was written over it");
    assert_eq!(reader.state().pages, 2, "the new draft is open");
}

#[cfg(unix)]
#[test]
fn a_save_into_that_fails_leaves_the_draft_arranging_the_document() {
    use std::os::unix::fs::PermissionsExt;
    let dir = folder("refused");
    let path = prose_in(&dir);
    let all = words(&path);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    click_sheet(&mut reader, 1, Modifiers::empty());
    reader.click("[data-item=remove-pages]");
    reader.settle();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    reader.click("[data-item=save-into]");
    reader.settle();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(words(&path), all, "nothing was written");
    assert!(
        reader.harness.query(".organiser").is_some(),
        "{}",
        reader.state().notice
    );
    assert_eq!(reader.text_all(".organiser-count"), ["5 pages"]);
}

#[test]
fn pages_are_selected_as_files_are_and_the_keys_act_on_them() {
    let dir = folder("keys");
    let path = prose_in(&dir);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    click_sheet(&mut reader, 1, Modifiers::empty());
    click_sheet(&mut reader, 3, Modifiers::SHIFT);
    assert_eq!(picked(&reader), ["1", "2", "3"]);
    click_sheet(&mut reader, 2, command());
    assert_eq!(picked(&reader), ["1", "3"]);
    reader.press_chord("mod+a");
    assert_eq!(picked(&reader).len(), 6, "⌘A selects every page");
    reader.press("Delete");
    reader.settle();
    assert_eq!(reader.state().notice, "A document needs at least one page.");
    assert_eq!(reader.text_all(".organiser-count"), ["6 of 6 selected"]);
    reader.press("Escape");
    assert!(picked(&reader).is_empty(), "Escape clears the selection");
    reader.press("Escape");
    assert!(
        reader.harness.query(".organiser").is_none(),
        "and then closes, with no changes"
    );
}

/// The turn buttons leave a narrow bar, and ⌘R and ⌘L, which turn the view
/// behind the organiser, turn the selected pages instead.
#[test]
fn the_keys_that_turn_the_view_turn_the_selected_pages() {
    let dir = folder("turn-keys");
    let mut reader = Reader::open(&prose_in(&dir));
    arrange(&mut reader);
    let shape = |reader: &mut Reader| {
        let picture = (reader.harness).layout_rect(".sheet[data-sheet='0'] .sheet-picture");
        picture.width > picture.height
    };
    assert!(!shape(&mut reader));
    reader.press_chord("mod+r");
    reader.settle();
    assert!(shape(&mut reader));
    reader.press_chord("mod+l");
    reader.settle();
    assert!(!shape(&mut reader));
}

#[test]
fn closing_the_window_or_quitting_with_changes_asks_first() {
    let dir = folder("close-window");
    let path = prose_in(&dir);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    reader.click("[data-item=turn-right]");
    reader.settle();
    let mut closes = vec![("mod+w", moonowl::app::Ask::Close)];
    // On a Mac, ⌘Q is the application menu's, which the keymap never hears.
    if !cfg!(target_os = "macos") {
        closes.push(("mod+q", moonowl::app::Ask::Quit));
    }
    for (chord, ask) in closes {
        reader.press_chord(chord);
        reader.settle();
        assert!(reader.harness.query(".ask-window").is_some(), "{chord}");
        assert!(!reader.asks().contains(&ask), "{chord}");
        reader.press("Escape");
        reader.settle();
    }
    // The close button and the application menu's Quit, which the keymap
    // never hears, ask the same question.
    for (quit, ask) in [
        (false, moonowl::app::Ask::Close),
        (true, moonowl::app::Ask::QuitAnswered),
    ] {
        reader.close_asked(quit);
        reader.settle();
        assert!(reader.harness.query(".ask-window").is_some(), "{quit}");
        assert!(!reader.asks().contains(&ask), "{quit}");
        reader.press("Escape");
        reader.settle();
    }
    // Enter keeps the draft, since that is the answer that loses nothing.
    reader.press_chord("mod+w");
    reader.settle();
    reader.press("Enter");
    reader.settle();
    assert!(reader.harness.query(".ask-window").is_none());
    assert!(reader.harness.query(".organiser").is_some());
    assert!(!reader.asks().contains(&moonowl::app::Ask::Close));
    reader.press_chord("mod+w");
    reader.settle();
    reader.click(".ask-go");
    reader.settle();
    assert!(reader.asks().contains(&moonowl::app::Ask::Close));
    // With nothing to ask about, the window goes at once.
    let mut reader = Reader::open(&path);
    reader.close_asked(false);
    reader.settle();
    assert_eq!(reader.asks(), [moonowl::app::Ask::Close]);
    reader.close_asked(true);
    reader.settle();
    assert_eq!(
        reader.asks(),
        [moonowl::app::Ask::Close, moonowl::app::Ask::QuitAnswered]
    );
}

#[test]
fn closing_with_changes_asks_first() {
    let dir = folder("close");
    let path = prose_in(&dir);
    let before = std::fs::read(&path).unwrap();
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    reader.click("[data-item=turn-right]");
    reader.settle();
    reader.click("[data-item=cancel]");
    reader.settle();
    assert!(reader.harness.query(".ask-window").is_some());
    reader.press("Escape");
    assert!(
        reader.harness.query(".organiser").is_some(),
        "Keep arranging"
    );
    reader.press_chord("mod+z");
    reader.click("[data-item=cancel]");
    reader.settle();
    assert!(
        reader.harness.query(".organiser").is_none(),
        "undone, there is nothing to lose"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn a_dragged_page_lands_where_the_line_is() {
    let dir = folder("drag");
    let path = prose_in(&dir);
    let all = words(&path);
    let to = path_of(&dir, "moved.pdf");
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![to.clone()]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    let from = sheet(&mut reader, 0);
    let three = reader
        .harness
        .layout_rect(".sheet[data-sheet='3'] .sheet-picture");
    // Just inside the left of the fourth page: before it.
    let target = (three.x + 4.0, three.y + 10.0);
    reader.harness.mouse_down_at(from.0, from.1);
    for step in 1..=4 {
        let t = step as f32 / 4.0;
        reader.carry(
            from.0 + (target.0 - from.0) * t,
            from.1 + (target.1 - from.1) * t,
        );
    }
    reader.settle();
    assert!(
        reader.harness.query(".organiser-drop").is_some(),
        "the line"
    );
    reader.harness.mouse_up_at(target.0, target.1);
    reader.settle();
    assert!(reader.harness.query(".organiser-drop").is_none());
    assert_eq!(picked(&reader), ["2"], "the page moved, still selected");
    reader.click("[data-item=save-as]");
    reader.settle();
    let mut wanted = all.clone();
    let first = wanted.remove(0);
    wanted.insert(2, first);
    assert_eq!(words(&to), wanted);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        std::fs::read(fixture::prose_pdf()).unwrap()
    );
}

#[test]
fn files_are_combined_from_the_start_screen() {
    let dir = folder("combine");
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
    assert_eq!(reader.dialogs()[0], (Choosing::AddFiles, None));
    assert_eq!(reader.text_all(".organiser-count"), ["8 of 8 selected"]);
    assert_eq!(
        reader.text_all(".sheet[data-sheet='6'] .sheet-label"),
        ["draft.pdf · 1"]
    );
    reader.click("[data-item=save-as]");
    reader.settle();
    assert_eq!(
        reader.dialogs()[1],
        (
            Choosing::SaveArranged,
            Some(path_of(&dir, "prose (combined).pdf"))
        )
    );
    assert_eq!(reader.state().pages, 8, "the combined file is open");
}

#[test]
fn every_few_pages_is_a_file_of_its_own() {
    let dir = folder("every");
    let path = prose_in(&dir);
    let into = dir.join("parts");
    std::fs::create_dir_all(&into).unwrap();
    let into = into.to_string_lossy().into_owned();
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![into.clone()]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    reader.click("[data-item=split]");
    reader.click(".organiser-every");
    reader.type_text("4");
    reader.press("Enter");
    reader.settle();
    assert_eq!(reader.harness.query_all(".organiser-head").len(), 2);
    assert!(reader.harness.query("[data-item=save-into]").is_none());
    assert_eq!(reader.text_all("[data-item=save-as]"), ["Save as 2 files…"]);
    reader.click("[data-item=save-as]");
    reader.settle();
    assert_eq!(reader.dialogs()[0], (Choosing::SaveParts, None));
    let parts = std::path::Path::new(&into);
    let all = words(&path);
    assert_eq!(
        words(&path_of(parts, "prose (part 1).pdf")),
        all[..4].to_vec()
    );
    assert_eq!(
        words(&path_of(parts, "prose (part 2).pdf")),
        all[4..].to_vec()
    );
    assert_eq!(reader.state().notice, "Saved 2 files in parts.");
    // With the first page selected, the new file begins after it, since
    // a file always begins at the first page.
    arrange(&mut reader);
    assert_eq!(picked(&reader), ["0"]);
    reader.click("[data-item=cut]");
    reader.settle();
    assert_eq!(reader.harness.query_all(".organiser-head").len(), 2);
    reader.click("[data-item=join]");
    reader.settle();
    assert_eq!(reader.harness.query_all(".organiser-head").len(), 1);
    // With every page selected there is nowhere for a new file to begin.
    reader.click("[data-item=select]");
    reader.click("[data-item=select-all]");
    reader.settle();
    assert!(reader.harness.query("[data-item=cut][disabled]").is_some());
    click_sheet(&mut reader, 2, Modifiers::empty());
    reader.click("[data-item=cut]");
    reader.settle();
    assert_eq!(reader.harness.query_all(".organiser-head").len(), 2);
    reader.click("[data-item=join]");
    reader.settle();
    assert_eq!(reader.harness.query_all(".organiser-head").len(), 1);
}

#[test]
fn each_chapter_is_saved_as_a_file_named_after_it() {
    let dir = folder("split");
    let path = path_of(&dir, "book.pdf");
    std::fs::copy(fixture::contents_pdf(), &path).unwrap();
    let into = dir.join("chapters");
    std::fs::create_dir_all(&into).unwrap();
    let into = into.to_string_lossy().into_owned();
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![into.clone()], vec![into.clone()]],
            ..Options::default()
        },
    );
    reader.settle();
    let split = |reader: &mut Reader| {
        arrange(reader);
        reader.click("[data-item=split]");
        reader.click("[data-item=split-chapters]");
        reader.settle();
        reader.click("[data-item=save-as]");
        reader.settle();
    };
    split(&mut reader);
    // The top-level headings are on pages 1, 3, 8 and 11 of 12.
    let names = [
        "1 Front matter",
        "2 Chapter One",
        "3 Chapter Two",
        "4 Index",
    ];
    for (name, count) in names.iter().zip([2, 5, 3, 2]) {
        let file = path_of(std::path::Path::new(&into), &format!("{name}.pdf"));
        assert_eq!(render::open(&file).expect(name).pages(), count, "{name}");
    }
    assert_eq!(reader.state().notice, "Saved 4 files in chapters.");
    // Again into the same folder writes over nothing.
    split(&mut reader);
    assert_eq!(
        reader.state().notice,
        "1 Front matter.pdf is already in that folder. Choose another folder."
    );
    assert!(
        reader.harness.query(".organiser").is_some(),
        "and the draft stays"
    );
}

#[test]
fn a_page_where_a_chapter_begins_partway_down_goes_into_both_chapters() {
    let dir = folder("split-midpage");
    let path = path_of(&dir, "book.pdf");
    std::fs::copy(fixture::midpage_chapters_pdf(), &path).unwrap();
    let into = dir.join("chapters");
    std::fs::create_dir_all(&into).unwrap();
    let into = into.to_string_lossy().into_owned();
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![into.clone()]],
            ..Options::default()
        },
    );
    reader.settle();
    arrange(&mut reader);
    // Twice: a second split finds the page the first put in twice, and
    // must not add a third copy.
    for _ in 0..2 {
        reader.click("[data-item=split]");
        reader.click("[data-item=split-chapters]");
        reader.settle();
    }
    assert!(
        reader.harness.query("[data-item=save-into]").is_none(),
        "a page in it twice cannot be written into the document"
    );
    reader.click("[data-item=save-as]");
    reader.settle();
    // Opening is pages 1 to 3, Middle 3 and 4, End 5 and 6.
    for (name, count) in [("1 Opening", 3), ("2 Middle", 2), ("3 End", 2)] {
        let file = path_of(std::path::Path::new(&into), &format!("{name}.pdf"));
        assert_eq!(render::open(&file).expect(name).pages(), count, "{name}");
    }
}

#[test]
fn every_few_pages_after_a_chapter_split_keeps_the_selection_on_its_page() {
    let dir = folder("every-after-chapters");
    let path = path_of(&dir, "book.pdf");
    std::fs::copy(fixture::midpage_chapters_pdf(), &path).unwrap();
    let mut reader = Reader::open(&path);
    reader.settle();
    arrange(&mut reader);
    reader.click("[data-item=split]");
    reader.click("[data-item=split-chapters]");
    reader.settle();
    assert_eq!(
        reader.harness.query_all(".sheet").len(),
        7,
        "page 3 is in twice"
    );
    click_sheet(&mut reader, 6, Modifiers::empty());
    assert_eq!(picked(&reader), ["6"]);
    // Every two pages takes the second copy of page 3 out, and the
    // selection moves up a place with the last page.
    reader.click("[data-item=split]");
    reader.click(".organiser-every");
    reader.type_text("2");
    reader.press("Enter");
    reader.settle();
    assert_eq!(reader.harness.query_all(".sheet").len(), 6);
    assert_eq!(picked(&reader), ["5"]);
    let wide = |reader: &mut Reader, at: usize| {
        let picture =
            (reader.harness).layout_rect(&format!(".sheet[data-sheet='{at}'] .sheet-picture"));
        picture.width > picture.height
    };
    reader.press_chord("mod+r");
    reader.settle();
    assert!(wide(&mut reader, 5), "the last page is turned");
    assert!(!wide(&mut reader, 4), "and the page before it is not");
}

#[test]
fn a_document_without_chapters_offers_no_split_at_chapters() {
    let dir = folder("no-split");
    let path = prose_in(&dir);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    reader.click("[data-item=split]");
    assert!(reader.harness.query("[data-item=split-every]").is_some());
    assert!(reader.harness.query("[data-item=split-chapters]").is_none());
}

/// A file is "combined" when its pages come from several files, not when
/// another file was added and then taken out again.
#[test]
fn a_file_added_and_removed_again_is_not_combined() {
    let dir = folder("uncombined");
    let path = prose_in(&dir);
    let other = path_of(&dir, "draft.pdf");
    fixture::draft(std::path::Path::new(&other), 2);
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![other.clone()], vec![]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    reader.click("[data-item=add-files]");
    reader.settle();
    reader.click("[data-item=remove-pages]");
    reader.click("[data-item=save-as]");
    reader.settle();
    assert_eq!(
        reader.dialogs()[1],
        (
            Choosing::SaveArranged,
            Some(path_of(&dir, "prose (arranged).pdf"))
        )
    );
}

/// Only the pages of another file kept, the new file is named after that
/// file, not after the document.
#[test]
fn pages_kept_from_another_file_are_named_after_it() {
    let dir = folder("kept-other");
    let path = prose_in(&dir);
    let other = path_of(&dir, "draft.pdf");
    fixture::draft(std::path::Path::new(&other), 2);
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![other.clone()], vec![]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    reader.click("[data-item=add-files]");
    reader.settle();
    let first: usize = picked(&reader)[0].parse().unwrap();
    let (x, y) = sheet(&mut reader, first);
    reader.right_click_at(x, y);
    reader.click("[data-item=keep-only]");
    reader.settle();
    reader.click("[data-item=save-as]");
    reader.settle();
    assert_eq!(
        reader.dialogs()[1],
        (
            Choosing::SaveArranged,
            Some(path_of(&dir, "draft (arranged).pdf"))
        )
    );
}

#[test]
fn added_files_come_after_the_selection_and_turned_pages_are_written_turned() {
    let dir = folder("add");
    let path = prose_in(&dir);
    let all = words(&path);
    let other = path_of(&dir, "draft.pdf");
    fixture::draft(std::path::Path::new(&other), 2);
    let theirs = words(&other);
    let size = render::open(&path).unwrap().size_of(0);
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![other.clone()]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    reader.click("[data-item=add-files]");
    reader.settle();
    assert_eq!(picked(&reader), ["1", "2"], "after page 1, and selected");
    reader.click("[data-item=turn-left]");
    reader.click("[data-item=save-into]");
    reader.settle();
    let mut wanted = all.clone();
    wanted.splice(1..1, theirs.clone());
    assert_eq!(words(&path), wanted);
    let turned = render::open(&path).unwrap().size_of(1);
    assert_eq!((turned.width, turned.height), (size.height, size.width));
    let notice = reader.state().notice;
    assert!(
        notice
            .starts_with("Arranged prose.pdf: turned 2 pages and added 2 pages from other files. "),
        "{notice}"
    );
}

#[test]
fn files_added_together_are_one_step_of_undo() {
    let dir = folder("add-two");
    let path = prose_in(&dir);
    let one = path_of(&dir, "one.pdf");
    let two = path_of(&dir, "two.pdf");
    fixture::draft(std::path::Path::new(&one), 2);
    fixture::draft(std::path::Path::new(&two), 3);
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![one, two]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    reader.click("[data-item=add-files]");
    reader.settle();
    assert_eq!(reader.text_all(".organiser-count"), ["5 of 11 selected"]);
    reader.press_chord("mod+z");
    reader.settle();
    assert_eq!(
        reader.text_all(".organiser-count"),
        ["1 of 6 selected"],
        "undone, the page being read is selected again"
    );
}

#[test]
fn an_encrypted_file_is_not_added() {
    let dir = folder("locked");
    let path = prose_in(&dir);
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![fixture::locked_pdf()]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    reader.click("[data-item=add-files]");
    reader.settle();
    assert!(
        reader.state().notice.contains("encrypted"),
        "{}",
        reader.state().notice
    );
    assert_eq!(reader.text_all(".organiser-count"), ["1 of 6 selected"]);
}

fn command() -> Modifiers {
    if cfg!(target_os = "macos") {
        Modifiers::META
    } else {
        Modifiers::CONTROL
    }
}

#[test]
fn pages_are_not_saved_over_a_file_they_come_from_or_nowhere() {
    let dir = folder("over");
    let path = prose_in(&dir);
    let before = std::fs::read(&path).unwrap();
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![], vec![path.clone()]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    reader.click("[data-item=remove-pages]");
    // A dialog cancelled writes nothing and keeps the draft.
    reader.click("[data-item=save-as]");
    reader.settle();
    assert!(reader.harness.query(".organiser").is_some());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    reader.click("[data-item=save-as]");
    reader.settle();
    assert!(reader.state().notice.starts_with("Choose a new name."));
    assert!(reader.harness.query(".organiser").is_some());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

/// A document open in another window is being read there; saving over it
/// would reload it under its reader, past undo.
#[test]
fn pages_are_not_saved_over_a_document_open_in_another_window() {
    let dir = folder("over-window");
    let path = prose_in(&dir);
    let two = path_of(&dir, "two.pdf");
    fixture::draft(std::path::Path::new(&two), 2);
    let before = std::fs::read(&two).unwrap();
    let desk = moonowl::windows::Desk::new();
    desk.set("reader-9", Some(&two));
    let mut reader = Reader::open_with(
        &path,
        Options {
            desk: Some(desk),
            chooses: vec![vec![two.clone()]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    reader.click("[data-item=remove-pages]");
    reader.click("[data-item=save-as]");
    reader.settle();
    assert!(
        reader
            .state()
            .notice
            .starts_with("Choose a new name. That document is open in a window"),
        "{}",
        reader.state().notice
    );
    assert!(reader.harness.query(".organiser").is_some());
    assert_eq!(std::fs::read(&two).unwrap(), before);
}

#[test]
fn a_page_saved_without_the_page_its_link_leads_to_says_so() {
    let dir = folder("links");
    let links = path_of(&dir, "links.pdf");
    std::fs::copy(fixture::links_pdf(), &links).unwrap();
    let to = path_of(&dir, "first.pdf");
    let mut reader = Reader::open_with(
        &links,
        Options {
            chooses: vec![vec![to.clone()]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    // Page 1 links to page 5, which stays behind.
    reader.press_chord("mod+a");
    click_sheet(&mut reader, 0, command());
    reader.press("Delete");
    reader.click("[data-item=save-as]");
    reader.settle();
    let notice = reader.state().notice;
    assert!(
        notice.starts_with(
            "Saved first.pdf, 1 page. Links in them to pages that were left out do not work."
        ),
        "{notice}"
    );
}

#[test]
fn pages_put_into_the_document_without_the_page_their_link_leads_to_say_so() {
    let dir = folder("links-into");
    let path = prose_in(&dir);
    let links = path_of(&dir, "links.pdf");
    std::fs::copy(fixture::links_pdf(), &links).unwrap();
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![links.clone()]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    reader.click("[data-item=add-files]");
    reader.settle();
    // Only the first page of links.pdf stays, and it links to its fifth.
    click_sheet(&mut reader, 1, command());
    reader.press("Delete");
    reader.click("[data-item=save-into]");
    reader.settle();
    let notice = reader.state().notice;
    assert!(
        notice.contains("Links on the pages put in that lead to pages left out do not work."),
        "{notice}"
    );
}

#[test]
fn the_arrows_move_the_selection_and_shift_grows_it() {
    let dir = folder("arrows");
    let path = prose_in(&dir);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    assert_eq!(picked(&reader), ["0"]);
    reader.press("ArrowRight");
    reader.settle();
    assert_eq!(picked(&reader), ["1"]);
    reader.press_with("ArrowRight", Modifiers::SHIFT);
    reader.settle();
    assert_eq!(picked(&reader), ["1", "2"], "Shift grows the selection");
    reader.press("ArrowLeft");
    reader.press("ArrowLeft");
    reader.settle();
    assert_eq!(picked(&reader), ["0"], "and stops at the first page");
    let first = reader.harness.layout_rect(".sheet[data-sheet='0']");
    reader.press("ArrowDown");
    reader.settle();
    let below = picked(&reader);
    assert_eq!(below.len(), 1);
    let under = reader
        .harness
        .layout_rect(&format!(".sheet[data-sheet='{}']", below[0]));
    assert!(
        under.y > first.y && (under.x - first.x).abs() < 1.0,
        "the row below"
    );
    reader.press("ArrowUp");
    reader.settle();
    assert_eq!(picked(&reader), ["0"]);
}

#[test]
fn enter_is_save_as() {
    let dir = folder("enter");
    let path = prose_in(&dir);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    reader.press("Enter");
    reader.settle();
    assert_eq!(reader.dialogs().len(), 1);
    assert_eq!(reader.dialogs()[0].0, Choosing::SaveArranged);
}

/// A page's menu opened low in the window is placed above the pointer, so
/// that all of it is on screen.
#[test]
fn the_page_menu_stays_inside_the_window() {
    let dir = folder("menu-low");
    let path = prose_in(&dir);
    let mut reader = Reader::open(&path);
    reader.resize(900, 600);
    arrange(&mut reader);
    reader.point_to(1.0, 1.0);
    let picture = reader
        .harness
        .layout_rect(".sheet[data-sheet='0'] .sheet-picture");
    // Low enough that the menu would run off the bottom if it opened
    // under the pointer.
    let (x, y) = (
        picture.x + picture.width / 2.0,
        picture.y + picture.height - 40.0,
    );
    reader.right_click_at(x, y);
    let menu = reader.harness.layout_rect(".sheet-menu");
    assert!(
        menu.y + menu.height <= 600.0 && menu.y >= 52.0,
        "the menu runs from {} to {}",
        menu.y,
        menu.y + menu.height
    );
    assert!(menu.y < y, "it opened above the pointer at {y}");
}

#[test]
fn the_page_menu_duplicates_keeps_and_reverses_and_undo_reselects() {
    let dir = folder("menu");
    let path = prose_in(&dir);
    let all = words(&path);
    let to = path_of(&dir, "kept.pdf");
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![to.clone()]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    let (x, y) = sheet(&mut reader, 1);
    reader.right_click_at(x, y);
    assert!(reader.harness.query(".sheet-menu").is_some());
    assert_eq!(picked(&reader), ["1"], "the page under the pointer");
    reader.click("[data-item=duplicate]");
    reader.settle();
    assert!(reader.harness.query(".sheet-menu").is_none());
    assert_eq!(reader.text_all(".organiser-count"), ["1 of 7 selected"]);
    assert_eq!(picked(&reader), ["2"], "the copy");
    assert!(
        reader.harness.query("[data-item=save-into]").is_none(),
        "a page in it twice cannot be written into the document"
    );
    reader.press_chord("mod+z");
    reader.settle();
    assert_eq!(
        picked(&reader),
        ["1"],
        "undone, the page copied is selected"
    );
    // Removed and put back, the pages come back selected.
    click_sheet(&mut reader, 3, Modifiers::SHIFT);
    reader.press("Delete");
    reader.settle();
    assert_eq!(reader.text_all(".organiser-count"), ["3 pages"]);
    reader.press_chord("mod+z");
    reader.settle();
    assert_eq!(picked(&reader), ["1", "2", "3"]);
    reader.press_chord("mod+shift+z");
    reader.settle();
    assert_eq!(reader.text_all(".organiser-count"), ["3 pages"]);
    reader.press_chord("mod+z");
    reader.settle();
    // Reverse the selection, then keep only it.
    let (x, y) = sheet(&mut reader, 2);
    reader.right_click_at(x, y);
    assert_eq!(
        picked(&reader),
        ["1", "2", "3"],
        "a right-click inside the selection keeps it"
    );
    assert_eq!(
        reader.text_all("[data-item=reverse] .menu-label"),
        ["Reverse these pages"]
    );
    reader.click("[data-item=reverse]");
    reader.settle();
    reader.right_click_at(x, y);
    reader.click("[data-item=keep-only]");
    reader.settle();
    assert_eq!(reader.text_all(".organiser-count"), ["3 pages"]);
    reader.click("[data-item=save-as]");
    reader.settle();
    assert_eq!(
        words(&to),
        vec![all[3].clone(), all[2].clone(), all[1].clone()]
    );
}

#[test]
fn a_page_moves_to_the_page_typed_in_its_menu() {
    let dir = folder("move-typed");
    let path = prose_in(&dir);
    let all = words(&path);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    let (x, y) = sheet(&mut reader, 0);
    reader.right_click_at(x, y);
    reader.click(".organiser-move");
    reader.type_text("9");
    reader.press("Enter");
    reader.settle();
    assert_eq!(
        reader.state().notice,
        "Type the page to move them to, from 1 to 6."
    );
    reader.right_click_at(x, y);
    reader.click(".organiser-move");
    reader.type_text("6");
    reader.press("Enter");
    reader.settle();
    assert!(
        reader.harness.query(".sheet-menu").is_none(),
        "the menu closes"
    );
    assert_eq!(picked(&reader), ["5"], "the page moved is still selected");
    reader.click("[data-item=save-into]");
    reader.settle();
    let mut wanted = all.clone();
    let first = wanted.remove(0);
    wanted.push(first);
    assert_eq!(words(&path), wanted);
}

/// The short way to a few pages of a document: type them, keep them, save.
#[test]
fn keep_only_beside_the_pages_typed_keeps_them() {
    let dir = folder("keep-typed");
    let path = prose_in(&dir);
    let all = words(&path);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    reader.click("[data-item=select]");
    reader.click(".organiser-typed");
    reader.type_text("2-3");
    reader.click("[data-item=keep-typed]");
    reader.settle();
    assert!(reader.harness.query(".organiser-split-menu").is_none());
    assert_eq!(reader.text_all(".organiser-count"), ["2 pages"]);
    reader.click("[data-item=save-into]");
    reader.settle();
    assert_eq!(words(&path), all[1..3]);
}

#[test]
fn the_select_menu_picks_odd_even_and_typed_pages() {
    let dir = folder("select");
    let path = prose_in(&dir);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    reader.click("[data-item=select]");
    reader.click("[data-item=select-odd]");
    reader.settle();
    assert_eq!(picked(&reader), ["0", "2", "4"]);
    reader.click("[data-item=select]");
    reader.click("[data-item=select-even]");
    reader.settle();
    assert_eq!(picked(&reader), ["1", "3", "5"]);
    reader.click("[data-item=select]");
    reader.click(".organiser-typed");
    reader.type_text("2-3, 6");
    reader.press("Enter");
    reader.settle();
    assert_eq!(picked(&reader), ["1", "2", "5"]);
    assert!(
        reader.harness.query(".organiser-typed").is_none(),
        "the menu closes"
    );
    reader.click("[data-item=select]");
    reader.click(".organiser-typed");
    reader.type_text("x");
    reader.press("Enter");
    reader.settle();
    assert_eq!(
        reader.state().notice,
        "Type which pages to select, like 3, 5-8 or 10-."
    );
    reader.press("Escape");
    assert!(
        reader.harness.query(".organiser-typed").is_none(),
        "Escape closes the menu"
    );
    reader.click("[data-item=select]");
    reader.click("[data-item=select-all]");
    reader.settle();
    assert_eq!(picked(&reader).len(), 6);
}

#[test]
fn a_blank_page_goes_after_the_page_and_has_its_size() {
    let dir = folder("blank");
    let path = prose_in(&dir);
    let all = words(&path);
    let to = path_of(&dir, "padded.pdf");
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![to.clone()]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    reader.click("[data-item=turn-right]");
    reader.settle();
    let (x, y) = sheet(&mut reader, 2);
    reader.right_click_at(x, y);
    reader.click("[data-item=blank]");
    reader.settle();
    assert_eq!(reader.text_all(".organiser-count"), ["1 of 7 selected"]);
    assert_eq!(picked(&reader), ["3"]);
    assert_eq!(
        reader.text_all(".sheet[data-sheet='3'] .sheet-label"),
        ["Blank page"]
    );
    assert!(
        !reader.text_all(".sheet[data-sheet='0'] .sheet-label")[0].contains("prose.pdf"),
        "a page of the one file is not named after it"
    );
    reader.click("[data-item=save-as]");
    reader.settle();
    let mut wanted = all.clone();
    wanted.insert(3, String::new());
    assert_eq!(words(&to), wanted);
    let saved = render::open(&to).unwrap();
    let (page, blank) = (saved.size_of(2), saved.size_of(3));
    assert!((page.width - blank.width).abs() < 0.5 && (page.height - blank.height).abs() < 0.5);
    // The turned first page is wider than it is tall, and a blank after it
    // takes that shape.
    let turned = saved.size_of(0);
    assert!(turned.width > turned.height);
}

/// A blank page goes in after the selection, and takes the size of the page
/// it follows rather than of the page whose menu it came from.
#[test]
fn a_blank_page_after_several_has_the_size_of_the_last() {
    let dir = folder("blank-after");
    let path = prose_in(&dir);
    let to = path_of(&dir, "padded.pdf");
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![to.clone()]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    reader.click("[data-item=turn-right]");
    click_sheet(&mut reader, 2, Modifiers::SHIFT);
    let (x, y) = sheet(&mut reader, 0);
    reader.right_click_at(x, y);
    reader.click("[data-item=blank]");
    reader.settle();
    assert_eq!(picked(&reader), ["3"]);
    reader.click("[data-item=save-as]");
    reader.settle();
    let saved = render::open(&to).unwrap();
    let (page, blank) = (saved.size_of(2), saved.size_of(3));
    assert!(blank.width < blank.height);
    assert!((page.width - blank.width).abs() < 0.5 && (page.height - blank.height).abs() < 0.5);
}

#[test]
fn a_double_click_reads_the_page() {
    let dir = folder("dblclick");
    let path = prose_in(&dir);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    let (x, y) = sheet(&mut reader, 3);
    reader.double_click_at(x, y);
    assert!(reader.harness.query(".organiser").is_none());
    assert_eq!(reader.state().page, 4);
    // With changes, it asks first.
    arrange(&mut reader);
    reader.click("[data-item=turn-right]");
    reader.settle();
    let (x, y) = sheet(&mut reader, 1);
    reader.double_click_at(x, y);
    assert!(reader.harness.query(".ask-window").is_some());
    reader.press("Escape");
    assert!(reader.harness.query(".organiser").is_some());
}

#[test]
fn a_file_dropped_on_the_organiser_is_added_to_it() {
    let dir = folder("dropped");
    let path = prose_in(&dir);
    let two = path_of(&dir, "two.pdf");
    fixture::draft(std::path::Path::new(&two), 2);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    reader.drag_over(true);
    assert_eq!(reader.state().dragging.as_deref(), Some("Drop to add"));
    reader.drop_document(&two);
    reader.settle();
    assert_eq!(reader.text_all(".organiser-count"), ["2 of 8 selected"]);
    assert_eq!(picked(&reader), ["1", "2"], "after the page being read");
    assert_eq!(reader.state().pages, 6, "the document behind is as it was");
}

#[test]
fn files_dropped_together_are_all_added() {
    let dir = folder("dropped-together");
    let path = prose_in(&dir);
    let two = path_of(&dir, "two.pdf");
    fixture::draft(std::path::Path::new(&two), 2);
    let png = path_of(&dir, "scan.png");
    image::RgbaImage::from_pixel(100, 140, image::Rgba([255, 0, 0, 255]))
        .save(&png)
        .unwrap();
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    reader.drop_documents(&[&two, &png]);
    reader.settle();
    assert_eq!(reader.text_all(".organiser-count"), ["3 of 9 selected"]);
}

#[test]
fn a_document_handed_over_goes_beside_a_draft() {
    let dir = folder("handed-over");
    let path = prose_in(&dir);
    let other = path_of(&dir, "draft.pdf");
    fixture::draft(std::path::Path::new(&other), 2);
    let mut reader = Reader::empty(Options {
        chooses: vec![vec![other.clone()]],
        ..Options::default()
    });
    reader.click(".start-combine");
    reader.settle();
    assert!(reader.harness.query(".organiser").is_some());
    reader.handed_over(&path);
    reader.settle();
    assert!(
        reader
            .asks()
            .contains(&moonowl::app::Ask::SendOn(path.clone())),
        "{:?}",
        reader.asks()
    );
    assert_eq!(reader.state().pages, 0, "nothing opened under the draft");
}

#[test]
fn a_file_is_named_by_its_heading() {
    let dir = folder("named");
    let path = prose_in(&dir);
    let into = dir.join("parts");
    std::fs::create_dir_all(&into).unwrap();
    let into = into.to_string_lossy().into_owned();
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![into.clone()]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    assert!(
        reader.harness.query("[data-item=name]").is_none(),
        "one file has no name to type"
    );
    reader.click("[data-item=split]");
    reader.click(".organiser-every");
    reader.type_text("3");
    reader.press("Enter");
    reader.settle();
    reader.click_nth("[data-item=name]", 1);
    reader.settle();
    reader.type_text("Appendix");
    reader.press("Enter");
    reader.settle();
    assert_eq!(
        reader.text_all(".organiser-head-name"),
        ["File 1 · 3 pages", "Appendix · 3 pages"]
    );
    reader.click("[data-item=save-as]");
    reader.settle();
    let parts = std::path::Path::new(&into);
    assert!(parts.join("1 part 1.pdf").exists());
    assert_eq!(words(&path_of(parts, "2 Appendix.pdf")).len(), 3);
}

#[test]
fn zoom_keys_size_the_pages() {
    let dir = folder("size");
    let path = prose_in(&dir);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    let zoom = reader.state().zoom;
    let before = reader
        .harness
        .layout_rect(".sheet[data-sheet='0'] .sheet-picture");
    reader.press_chord("mod+=");
    reader.settle();
    let after = reader
        .harness
        .layout_rect(".sheet[data-sheet='0'] .sheet-picture");
    assert!(after.height > before.height);
    assert_eq!(
        reader.state().zoom,
        zoom,
        "the document behind did not zoom"
    );
}

#[test]
fn interleave_slots_the_selected_pages_in_behind_the_others() {
    let dir = folder("interleave");
    let path = prose_in(&dir);
    let all = words(&path);
    let to = path_of(&dir, "sided.pdf");
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![to.clone()]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    // With pages 5 and 6 selected, they go in behind pages 1 and 2.
    reader.click("[data-item=select]");
    reader.click(".organiser-typed");
    reader.type_text("5-6");
    reader.press("Enter");
    reader.settle();
    let (x, y) = sheet(&mut reader, 4);
    reader.right_click_at(x, y);
    assert_eq!(
        reader.text_all("[data-item=interleave] .menu-label"),
        ["Interleave with the other pages"]
    );
    reader.click("[data-item=interleave]");
    reader.settle();
    assert_eq!(picked(&reader), ["1", "3"], "the pages slotted in");
    reader.press_chord("mod+z");
    reader.settle();
    assert_eq!(
        picked(&reader),
        ["4", "5"],
        "undone, the pages slotted in are selected"
    );
    // With nothing selected, the second half goes in behind the first.
    reader.press("Escape");
    reader.settle();
    assert_eq!(picked(&reader), Vec::<String>::new());
    reader.press_chord("mod+a");
    let (x, y) = sheet(&mut reader, 0);
    reader.right_click_at(x, y);
    assert_eq!(
        reader.text_all("[data-item=interleave] .menu-label"),
        ["Interleave the two halves"]
    );
    reader.click("[data-item=interleave]");
    reader.settle();
    assert_eq!(picked(&reader), ["1", "3", "5"]);
    reader.click("[data-item=save-as]");
    reader.settle();
    assert_eq!(words(&to), [0, 3, 1, 4, 2, 5].map(|page| all[page].clone()));
}

#[test]
fn a_picture_dropped_on_the_reader_opens_the_organiser_with_it() {
    let dir = folder("dropped-picture");
    let path = prose_in(&dir);
    let png = path_of(&dir, "scan.png");
    image::RgbaImage::from_pixel(100, 140, image::Rgba([255, 0, 0, 255]))
        .save(&png)
        .unwrap();
    let mut reader = Reader::open(&path);
    reader.drop_document(&png);
    reader.settle();
    assert!(
        reader.harness.query(".organiser").is_some(),
        "the organiser opens"
    );
    assert_eq!(reader.text_all(".organiser-count"), ["1 of 7 selected"]);
    assert_eq!(picked(&reader), ["1"], "after the page being read");
    assert_eq!(reader.state().pages, 6, "the document behind is as it was");
    // Dropped on an empty window, the picture is the whole draft.
    let mut reader = Reader::empty(Options::default());
    reader.drop_document(&png);
    reader.settle();
    assert_eq!(reader.text_all(".organiser-count"), ["1 of 1 selected"]);
}

/// A bar narrower than what it holds ran the count under "Add files…" and
/// "Cancel" over "Select". At every width a window can have, nothing in the
/// bar stands on anything else and nothing hangs off the end of it. The
/// harness sets every word in DejaVu, and SF Pro sets the same words about
/// 10% wider, so the bar has to keep a tenth of what it holds free.
#[test]
fn the_bar_never_overlaps_itself_however_narrow_the_window() {
    let dir = folder("narrow");
    let mut reader = Reader::open(&prose_in(&dir));
    arrange(&mut reader);
    let items = [
        ".organiser-title",
        ".organiser-count",
        "[data-item=add-files]",
        "[data-item=turn-left]",
        "[data-item=turn-right]",
        "[data-item=remove-pages]",
        "[data-item=cut]",
        "[data-item=split]",
        "[data-item=select]",
        "[data-item=layout]",
        ".organiser-size",
        "[data-item=cancel]",
        "[data-item=save-into]",
        "[data-item=save-as]",
    ];
    for width in (480u32..=1600).step_by(20) {
        reader.resize(width, 800);
        reader.settle();
        let mut end = 0.0f32;
        let mut before = "the window's edge";
        let mut tools = (f32::MAX, 0.0f32);
        for (at, item) in items.into_iter().enumerate() {
            let Some((x, _, w, _)) = reader.box_of(item).filter(|b| b.2 > 0.0) else {
                continue;
            };
            assert!(x + 0.5 >= end, "{item} stands on {before} at {width}px");
            end = x + w;
            before = item;
            if (2..=10).contains(&at) {
                tools = (tools.0.min(x), tools.1.max(x + w));
            }
        }
        assert!(
            end <= width as f32 + 0.5,
            "{before} runs off a {width}px window"
        );
        // What is set in type: the title, the count, the words of the tools,
        // and the three buttons at the end, which are words alone.
        let held: f32 = [".organiser-title", ".organiser-count"]
            .into_iter()
            .map(str::to_string)
            .chain(
                items[2..10]
                    .iter()
                    .map(|item| format!("{item} .chip-label")),
            )
            .chain(items[11..].iter().map(|item| item.to_string()))
            .filter_map(|text| reader.box_of(&text))
            .filter(|b| b.2 > 0.0 && b.3 > 0.0)
            .map(|b| b.2)
            .sum();
        // The tools stand in the middle of a box that takes up what room
        // there is, so what it has beyond them is the room left.
        let room = reader.box_of(".organiser-tools").unwrap().2 - (tools.1 - tools.0);
        assert!(
            room + 0.5 >= held / 10.0,
            "the bar has {room}px to spare in a {width}px window, too little for a wider font"
        );
    }
    assert!(reader
        .box_of("[data-item=save-as]")
        .is_some_and(|b| b.2 > 0.0));
}

#[test]
fn a_booklet_is_chosen_before_saving() {
    let dir = folder("booklet");
    let path = prose_in(&dir);
    let to = path_of(&dir, "prose (booklet).pdf");
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![to.clone()]],
            ..Options::default()
        },
    );
    arrange(&mut reader);
    reader.click("[data-item=layout]");
    reader.click("[data-item=layout-booklet]");
    reader.settle();
    reader.click("[data-item=layout]");
    assert_eq!(
        reader.text_all(".menu-item.on .menu-label"),
        ["A booklet, to fold in the middle"]
    );
    reader.click("[data-item=save-as]");
    reader.settle();
    assert_eq!(
        reader.dialogs()[0],
        (Choosing::SaveArranged, Some(to.clone()))
    );
    // Six pages make a booklet of eight: two sheets of paper, two sides each.
    assert_eq!(render::open(&to).unwrap().pages(), 4);
    assert!(
        reader
            .state()
            .notice
            .starts_with("Saved prose (booklet).pdf, 4 pages."),
        "{}",
        reader.state().notice
    );
}

/// Letting go of a page dragged past the edge of the window puts it back.
/// The window does not hear that button come up, and learns of it when the
/// pointer comes back with no button held.
#[test]
fn a_page_let_go_outside_the_window_stays_where_it_was() {
    let dir = folder("drag-aside");
    let path = prose_in(&dir);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    let from = sheet(&mut reader, 0);
    let three = reader
        .harness
        .layout_rect(".sheet[data-sheet='3'] .sheet-picture");
    reader.harness.mouse_down_at(from.0, from.1);
    reader.carry(three.x + 4.0, three.y + 10.0);
    reader.settle();
    assert!(reader.harness.query(".organiser-drop").is_some());
    reader.point_to(three.x + 4.0, three.y + 10.0);
    assert!(reader.harness.query(".organiser-drop").is_none());
    assert_eq!(picked(&reader), ["0"], "the page did not move");
}

/// A blank page saved into the document is said to be one, not a page from
/// another file.
#[test]
fn a_blank_page_saved_into_the_document_is_said_to_be_blank() {
    let dir = folder("blank-into");
    let path = prose_in(&dir);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    let (x, y) = sheet(&mut reader, 0);
    reader.right_click_at(x, y);
    reader.click("[data-item=blank]");
    reader.settle();
    reader.click("[data-item=save-into]");
    reader.settle();
    assert!(
        reader
            .state()
            .notice
            .starts_with("Arranged prose.pdf: added 1 blank page. "),
        "{}",
        reader.state().notice
    );
}

/// "Save" writes pages one to a sheet, so it is not offered once the pages
/// are laid out two to a sheet.
#[test]
fn save_is_not_offered_for_pages_two_to_a_sheet() {
    let dir = folder("two-to-a-sheet");
    let path = prose_in(&dir);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    assert!(reader.harness.query("[data-item=save-into]").is_some());
    reader.click("[data-item=layout]");
    reader.click("[data-item=layout-two]");
    reader.settle();
    assert!(reader.harness.query("[data-item=save-into]").is_none());
}

/// A draft of blank pages only is offered a name beside the window's
/// document, not "blank (arranged).pdf" in no folder at all.
#[test]
fn a_draft_of_blank_pages_is_named_after_the_document() {
    let dir = folder("only-blank");
    let path = prose_in(&dir);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    let (x, y) = sheet(&mut reader, 0);
    reader.right_click_at(x, y);
    reader.click("[data-item=blank]");
    reader.settle();
    assert_eq!(picked(&reader), ["1"]);
    reader.press_chord("mod+a");
    click_sheet(&mut reader, 1, command());
    reader.press("Delete");
    reader.settle();
    assert_eq!(reader.text_all(".sheet .sheet-label"), ["Blank page"]);
    reader.click("[data-item=save-as]");
    reader.settle();
    assert_eq!(
        reader.dialogs().last(),
        Some(&(
            Choosing::SaveArranged,
            Some(path_of(&dir, "prose (arranged).pdf"))
        ))
    );
}
