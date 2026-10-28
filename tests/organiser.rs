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
    // Joined again, there is one file.
    arrange(&mut reader);
    reader.click("[data-item=cut]");
    reader.settle();
    assert_eq!(
        reader.harness.query_all(".organiser-head").len(),
        1,
        "page 1 begins no file"
    );
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
    reader.click("[data-item=split]");
    reader.click("[data-item=split-chapters]");
    reader.settle();
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
fn a_document_without_chapters_offers_no_split_at_chapters() {
    let dir = folder("no-split");
    let path = prose_in(&dir);
    let mut reader = Reader::open(&path);
    arrange(&mut reader);
    reader.click("[data-item=split]");
    assert!(reader.harness.query("[data-item=split-every]").is_some());
    assert!(reader.harness.query("[data-item=split-chapters]").is_none());
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
