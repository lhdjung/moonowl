//! Page edits: saving pages as a new file, combining files, and removing,
//! turning, moving and inserting pages in the open document.

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
