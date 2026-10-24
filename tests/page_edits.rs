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

/// A reader on `path` with the thumbnails showing.
fn with_thumbnails(path: &str, options: Options) -> Reader {
    let mut reader = Reader::open_with(path, options);
    reader.press_chord("mod+b");
    reader.click(".tab[data-tab='pages']");
    reader
}

/// The middle of a thumbnail's picture, in the window.
fn thumb(reader: &Reader, page: usize) -> (f32, f32) {
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
    let (x, y) = thumb(&reader, 3);
    reader.click_at(x, y);
    reader.press_chord("mod+shift+b");
    assert_eq!(marked_pages(&reader), [3]);
    // A bookmark shows the Contents; back to the pages, and up to the top.
    reader.click(".tab[data-tab='pages']");
    reader.wheel_over(".panel.thumb-column", -5000.0);

    from_thumb(&mut reader, 2, "remove-pages");
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
