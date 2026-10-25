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
    let (pages, lost) = arrange::combine(&[one.clone(), other], &to).expect("combined");
    assert_eq!(pages, 9);
    assert!(!lost, "neither file links between its own pages");
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
    assert_eq!(reader.state().page, 3, "and so is the reader");

    reader.press_chord("mod+shift+z");
    reader.settle();
    assert_eq!(reader.state().pages, 5);
    assert_eq!(
        marked_pages(&reader),
        [2],
        "redone, the bookmark moves again"
    );
    assert_eq!(reader.state().page, 2, "and the reader with it");
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
    assert_eq!(headings.iter().filter(|(_, _, at)| *at == page).count(), 1);
    let mut reader = with_thumbnails(&path, Options::default());
    reader.click(".tab[data-tab='pages']");
    from_thumb(&mut reader, page, "remove-pages");
    in_place(&mut reader);
    let notice = reader.state().notice;
    let said = "The heading that pointed at it is taken out of the contents.";
    assert!(notice.contains(said), "{notice}");
    assert!(!contents(&path)
        .iter()
        .any(|(title, ..)| *title == headings[1].0));
}

/// The titles, depths and pages of a file's contents.
fn contents(path: &str) -> Vec<(String, usize, Option<usize>)> {
    (render::open(path).unwrap().outline().into_iter())
        .map(|heading| (heading.title, heading.depth, heading.page))
        .collect()
}

/// Every heading of the contents fixture, as [`contents`] reads it, with
/// the pages `moved` says they went to. A heading whose page went is left
/// out.
fn expected(moved: impl Fn(usize) -> Option<usize>) -> Vec<(String, usize, Option<usize>)> {
    (fixture::expected_headings().into_iter())
        .filter_map(|(title, depth, page)| Some((title, depth, Some(moved(page)?))))
        .collect()
}

#[test]
fn a_heading_whose_page_is_removed_leaves_its_own_headings_in_its_place() {
    let dir = folder("headings-lifted");
    let path = path_of(&dir, "contents.pdf");
    std::fs::copy(fixture::contents_pdf(), &path).unwrap();
    // "A section" is on page 4, and "Under a section", on page 5, was under it.
    arrange::Edit::Remove(vec![4]).write(&path, &path).unwrap();
    let moved = |page: usize| match page {
        4 => None,
        page if page > 4 => Some(page - 1),
        page => Some(page),
    };
    let mut want = expected(moved);
    let under = want
        .iter_mut()
        .find(|(title, ..)| title == "Under a section")
        .unwrap();
    under.1 = 1;
    assert_eq!(contents(&path), want);
    // A heading that only groups others goes when they all have: "Front
    // matter" is on page 1 and "Preface" on page 2.
    arrange::Edit::Remove(vec![1, 2])
        .write(&path, &path)
        .unwrap();
    assert_eq!(contents(&path)[0].0, "Chapter One");
}

#[test]
fn links_to_removed_pages_are_taken_off_and_keep_nothing_in_the_file() {
    let dir = folder("links-gone");
    let path = path_of(&dir, "links.pdf");
    std::fs::copy(fixture::links_pdf(), &path).unwrap();
    // Page 1 links to page 5, and page 2 to page 6.
    arrange::Edit::Remove(vec![5, 6])
        .write(&path, &path)
        .unwrap();
    let document = lopdf::Document::load(&path).unwrap();
    let annotations = |page: u32| {
        let page = *document.get_pages().get(&page).unwrap();
        document.get_page_annotations(page).unwrap().len()
    };
    assert_eq!((annotations(1), annotations(2)), (1, 0));
    let pages = (document.objects.values())
        .filter(|object| object.type_name().is_ok_and(|name| name == b"Page"))
        .count();
    assert_eq!(pages, 4, "the removed pages are not in the file");
}

#[test]
fn saved_pages_bring_their_headings() {
    let dir = folder("headings-saved");
    let to = path_of(&dir, "chapter.pdf");
    // Chapter One is pages 3 to 7.
    arrange::save_pages(&fixture::contents_pdf(), None, &[3, 4, 5, 6, 7], &to).unwrap();
    let moved = |page: usize| (3..=7).contains(&page).then(|| page - 2);
    assert_eq!(contents(&to), expected(moved));
    // Where on its page a heading leads comes with it.
    let document = render::open(&to).unwrap();
    let under = (document.outline().into_iter())
        .find(|heading| heading.title == "Under a section")
        .unwrap();
    assert!(
        under.spot.is_some_and(|spot| spot.top == 400.0),
        "{:?}",
        under.spot
    );
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
    reader.click(".chip.title");
    reader.click("[data-item=split]");
    reader.settle();
    // The top-level headings are on pages 1, 3, 8 and 11 of 12.
    let names = [
        "1 Front matter",
        "2 Chapter One",
        "3 Chapter Two",
        "4 Index",
    ];
    let pages = [2, 5, 3, 2];
    for (name, count) in names.iter().zip(pages) {
        let file = path_of(std::path::Path::new(&into), &format!("{name}.pdf"));
        assert_eq!(render::open(&file).expect(name).pages(), count, "{name}");
    }
    let chapter = path_of(std::path::Path::new(&into), "2 Chapter One.pdf");
    assert_eq!(
        contents(&chapter)[0],
        ("Chapter One".to_string(), 0, Some(1))
    );
    assert_eq!(
        reader.state().notice,
        "Saved 4 chapters as files in chapters."
    );
    // Again into the same folder writes over nothing.
    reader.click(".chip.title");
    reader.click("[data-item=split]");
    reader.settle();
    assert_eq!(
        reader.state().notice,
        "1 Front matter.pdf is already in that folder. Choose another folder."
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
    reader.click(".chip.title");
    reader.click("[data-item=split]");
    reader.settle();
    // Opening is pages 1 to 3, Middle 3 and 4, End 5 and 6.
    for (name, count) in [("1 Opening", 3), ("2 Middle", 2), ("3 End", 2)] {
        let file = path_of(std::path::Path::new(&into), &format!("{name}.pdf"));
        assert_eq!(render::open(&file).expect(name).pages(), count, "{name}");
    }
}

#[test]
fn a_document_without_chapters_offers_no_split() {
    let dir = folder("no-split");
    let path = prose_in(&dir);
    let mut reader = Reader::open_with(&path, Options::default());
    reader.click(".chip.title");
    assert!(reader.harness.query_all("[data-item=split]").is_empty());
}

/// The labels a file gives its pages, as the reader reads them.
fn labels_of(path: &str) -> Vec<String> {
    render::open(path).unwrap().labels()
}

#[test]
fn page_labels_move_with_their_pages() {
    let dir = folder("labels-follow");
    let path = path_of(&dir, "links.pdf");
    std::fs::copy(fixture::links_pdf(), &path).unwrap();
    assert_eq!(labels_of(&path), ["i", "ii", "iii", "1", "2", "3"]);
    arrange::Edit::Move(vec![2], 7).write(&path, &path).unwrap();
    assert_eq!(labels_of(&path), ["i", "iii", "1", "2", "3", "ii"]);
    let wrote = arrange::Edit::Remove(vec![1]).write(&path, &path).unwrap();
    assert!(wrote.labels_kept);
    assert_eq!(labels_of(&path), ["iii", "1", "2", "3", "ii"]);
    // Pages put in from a file with no labels are labelled with their
    // places, which other readers show too.
    let other = path_of(&dir, "draft.pdf");
    fixture::draft(std::path::Path::new(&other), 2);
    arrange::Edit::Insert(other, 2).write(&path, &path).unwrap();
    assert_eq!(labels_of(&path), ["iii", "2", "3", "1", "2", "3", "ii"]);
}

#[test]
fn pages_are_numbered_from_the_label_typed() {
    let dir = folder("labels-typed");
    let path = prose_in(&dir);
    let mut reader = with_thumbnails(&path, Options::default());
    from_thumb(&mut reader, 3, "number-from");
    assert_eq!(
        reader.field(".number-field"),
        "3",
        "what the page is called now"
    );
    reader.press_chord("mod+a");
    reader.type_text("A-1");
    reader.press("Enter");
    reader.settle();
    in_place(&mut reader);
    assert_eq!(labels_of(&path), ["1", "2", "A-1", "A-2", "A-3", "A-4"]);
    assert_eq!(
        reader.state().notice,
        "Page 3 is now called A-1, and the pages after it are numbered on from there."
    );
    // The front pages in Roman numerals, up to where A-1 begins.
    from_thumb(&mut reader, 1, "number-from");
    reader.press_chord("mod+a");
    reader.type_text("i");
    reader.press("Enter");
    reader.settle();
    assert_eq!(labels_of(&path), ["i", "ii", "A-1", "A-2", "A-3", "A-4"]);
    // And back, with undo.
    reader.press_chord("mod+z");
    reader.settle();
    assert_eq!(labels_of(&path), ["1", "2", "A-1", "A-2", "A-3", "A-4"]);
}

#[test]
fn numbering_starts_at_the_page_clicked_and_offers_its_label() {
    let dir = folder("labels-clicked");
    let path = prose_in(&dir);
    let mut reader = with_thumbnails(&path, Options::default());
    from_thumb(&mut reader, 3, "number-from");
    reader.press_chord("mod+a");
    reader.type_text("A-1");
    reader.press("Enter");
    reader.settle();
    in_place(&mut reader);
    // With positions shown, the field still offers the page's label.
    reader.click(".of.choice");
    reader.click_nth(".menu.numbering .menu-item", 1);
    let (x, y) = thumb(&mut reader, 2);
    reader.click_at(x, y);
    let (x, y) = thumb(&mut reader, 4);
    reader.click_at_with(x, y, command());
    from_thumb(&mut reader, 4, "number-from");
    assert_eq!(reader.field(".number-field"), "A-2");
    // Left as it is, nothing is written.
    let before = std::fs::read(&path).unwrap();
    reader.press("Enter");
    reader.settle();
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // Numbered from the page clicked, not the first selected.
    from_thumb(&mut reader, 4, "number-from");
    reader.press_chord("mod+a");
    reader.type_text("B-1");
    reader.press("Enter");
    reader.settle();
    assert_eq!(labels_of(&path), ["1", "2", "A-1", "B-1", "B-2", "B-3"]);
}

/// A document with no labels of its own keeps the numbers printed on the
/// pages before the numbering, which Moonowl showed for them.
#[test]
fn numbering_keeps_the_printed_numbers_before_it() {
    let dir = folder("labels-printed");
    let path = path_of(&dir, "offprint.pdf");
    std::fs::copy(fixture::offprint_pdf(), &path).unwrap();
    let mut reader = with_thumbnails(&path, Options::default());
    from_thumb(&mut reader, 3, "number-from");
    assert_eq!(reader.field(".number-field"), "409");
    reader.press_chord("mod+a");
    reader.type_text("A-1");
    reader.press("Enter");
    reader.settle();
    in_place(&mut reader);
    assert_eq!(labels_of(&path)[..4], ["407", "408", "A-1", "A-2"]);
}

#[test]
fn combined_files_bring_the_headings_of_each() {
    let dir = folder("headings-combined");
    let to = path_of(&dir, "both.pdf");
    let contents_pdf = fixture::contents_pdf();
    arrange::combine(&[fixture::prose_pdf(), contents_pdf.clone()], &to).unwrap();
    let prose = render::open(&fixture::prose_pdf()).unwrap().pages();
    assert_eq!(contents(&to), expected(|page| Some(page + prose)));
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
    assert_eq!(
        reader.harness.text_content(".thumb-foot-count"),
        "1 page selected",
        "one page has the bar too"
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
fn pages_are_deselected_from_the_bar_the_menu_or_escape() {
    let dir = folder("deselect");
    let path = prose_in(&dir);
    let mut reader = with_thumbnails(&path, Options::default());
    let (x, y) = thumb(&mut reader, 2);
    reader.click_at(x, y);
    reader.click(".thumb-foot [data-item=deselect-pages]");
    assert!(picked(&reader).is_empty(), "the bar's Deselect");
    assert!(
        reader.harness.query(".thumb-foot").is_none(),
        "and the bar goes"
    );

    from_thumb(&mut reader, 3, "deselect-pages");
    assert!(picked(&reader).is_empty(), "the menu's Deselect");

    let (x, y) = thumb(&mut reader, 4);
    reader.click_at(x, y);
    reader.press("Escape");
    assert!(
        picked(&reader).is_empty(),
        "Escape after a click in the column"
    );
    reader.press("Delete");
    assert!(
        reader.harness.query(".ask-window").is_none(),
        "and Delete then removes nothing"
    );
    assert_eq!(reader.state().pages, 6);
}

#[test]
fn a_certified_document_refuses_page_edits_without_asking() {
    let dir = folder("certified");
    let path = path_of(&dir, "certified.pdf");
    std::fs::copy(fixture::certified_pdf(3), &path).unwrap();
    assert!(
        render::open(&path).unwrap().certified() == Some(3),
        "pdfium sees it"
    );
    let before = std::fs::read(&path).unwrap();
    let mut reader = with_thumbnails(&path, Options::default());
    from_thumb(&mut reader, 1, "turn-right");
    assert!(
        reader.harness.query(".ask-window").is_none(),
        "nothing is asked"
    );
    assert!(
        reader.state().notice.contains("certified"),
        "{}",
        reader.state().notice
    );
    assert_eq!(std::fs::read(&path).unwrap(), before, "nothing is written");
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

    // A column the reader has put away takes no keys.
    reader.click(".tab[data-tab='pages']");
    let (x, y) = thumb(&mut reader, 2);
    reader.click_at(x, y);
    reader.press_chord("mod+b");
    reader.press("Delete");
    assert!(reader.harness.query(".ask-window").is_none());
    assert_eq!(reader.state().pages, 6);
    reader.press_chord("mod+b");

    reader.click(".tab[data-tab='pages']");
    let (x, y) = thumb(&mut reader, 2);
    reader.click_at(x, y);
    reader.press("Delete");
    in_place(&mut reader);
    assert_eq!(reader.state().pages, 5);

    // A Mac laptop has no Delete key, so ⌘⌫ removes there too, as in the
    // Finder.
    if cfg!(target_os = "macos") {
        let (x, y) = thumb(&mut reader, 2);
        reader.click_at(x, y);
        reader.press_chord("mod+backspace");
        reader.settle();
        assert_eq!(reader.state().pages, 4);
    }
}

#[test]
fn the_document_menu_offers_the_pages_selected_in_the_thumbnails() {
    let dir = folder("offer");
    let path = prose_in(&dir);
    let mut reader = with_thumbnails(&path, Options::default());
    let (x, y) = thumb(&mut reader, 2);
    reader.click_at(x, y);
    reader.click(".chip.title");
    assert_eq!(
        reader.field(".menu-pages-field"),
        "2",
        "one page clicked is the page being read"
    );
    reader.press("Escape");
    let (x, y) = thumb(&mut reader, 4);
    reader.click_at_with(x, y, command());
    reader.click(".chip.title");
    assert_eq!(reader.field(".menu-pages-field"), "2, 4");
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
    let file = lopdf::Document::load(&path).unwrap();
    let first = *file.get_pages().get(&1).unwrap();
    let rotate = file.get_dictionary(first).unwrap().get(b"Rotate").unwrap();
    assert_eq!(rotate.as_i64().unwrap(), 90, "and says so in the file");
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
    assert!(
        reader.harness.query(".thumb-lift object").is_some(),
        "a picture of page 1"
    );
    reader.press("Escape");
    reader.harness.mouse_up_at(three.x + 20.0, three.y + 6.0);
    reader.settle();
}

#[test]
fn inserting_a_file_counts_the_pages_of_both() {
    let dir = folder("insert");
    let path = prose_in(&dir);
    let all = words(&path);
    let other = path_of(&dir, "draft.pdf");
    fixture::draft(std::path::Path::new(&other), 3);
    let theirs = words(&other);
    let mut reader = with_thumbnails(
        &path,
        Options {
            chooses: vec![vec![other.clone()]],
            ..Options::default()
        },
    );
    let (x, y) = thumb(&mut reader, 4);
    reader.click_at(x, y);
    reader.wheel_over(".panel.thumb-column", -5000.0);
    from_thumb(&mut reader, 2, "insert-after");
    in_place(&mut reader);
    assert_eq!(reader.state().page, 7, "the reader's page moved along");
    let now = words(&path);
    assert_eq!(now.len(), 9);
    assert_eq!(now[..2], all[..2]);
    assert_eq!(now[2..5], theirs[..]);
    assert_eq!(now[5..], all[2..]);
    assert!(
        reader
            .state()
            .notice
            .starts_with("Put in the pages of draft.pdf before page 3."),
        "{}",
        reader.state().notice
    );
    assert_eq!(words(&other), theirs, "the other file is not changed");

    reader.press_chord("mod+z");
    reader.settle();
    assert_eq!(words(&path), all);
    assert_eq!(reader.state().page, 4, "undone, the reader is back on it");
    reader.press_chord("mod+shift+z");
    reader.settle();
    assert_eq!(reader.state().page, 7, "and redone, moved along again");
}

#[test]
fn the_document_menu_inserts_at_the_end() {
    let dir = folder("insert-end");
    let path = prose_in(&dir);
    let other = path_of(&dir, "draft.pdf");
    fixture::draft(std::path::Path::new(&other), 2);
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![other.clone()]],
            ..Options::default()
        },
    );
    reader.click(".chip.title");
    reader.click("[data-item=insert]");
    reader.settle();
    assert_eq!(
        reader.dialogs().last(),
        Some(&(Choosing::InsertFrom(7), None))
    );
    in_place(&mut reader);
    assert_eq!(words(&path)[6..], words(&other)[..]);
}

#[test]
fn an_encrypted_file_is_not_inserted() {
    let dir = folder("insert-locked");
    let path = prose_in(&dir);
    let refused = arrange::Edit::Insert(fixture::locked_pdf(), 1).write(&path, &path);
    assert!(refused.is_err_and(|said| said.contains("encrypted")));
    assert_eq!(words(&path).len(), 6);
}

/// Links come with pages put in from another file: a link out of the
/// document as pdfium copies it, and a link from one of those pages to
/// another, which pdfium copies without its destination, written again.
#[test]
fn inserted_pages_keep_their_links() {
    let dir = folder("links");
    let path = prose_in(&dir);
    arrange::Edit::Insert(fixture::links_pdf(), 7)
        .write(&path, &path)
        .unwrap();
    let document = render::open(&path).unwrap();
    let carried: Vec<render::Target> = (6..document.pages())
        .flat_map(|index| document.links_of(index))
        .map(|link| link.target)
        .collect();
    // Page 1 of the fixture links to page 5, at 400 points up, and page 2
    // to page 6; they are pages 11 and 12 now.
    let pin = render::Pin {
        left: None,
        top: 400.0,
    };
    assert_eq!(
        carried,
        [
            render::Target::Away("https://example.com/paper".into()),
            render::Target::Place {
                page: 11,
                spot: Some(pin)
            },
            render::Target::Place {
                page: 12,
                spot: None
            },
        ]
    );
}

#[test]
fn combined_and_saved_pages_keep_the_links_between_them() {
    let dir = folder("links-new");
    let both = path_of(&dir, "both.pdf");
    arrange::combine(&[fixture::prose_pdf(), fixture::links_pdf()], &both).unwrap();
    let document = render::open(&both).unwrap();
    let targets: Vec<render::Target> = (0..document.pages())
        .flat_map(|index| document.links_of(index))
        .map(|link| link.target)
        .filter(|target| matches!(target, render::Target::Place { .. }))
        .collect();
    assert_eq!(targets.len(), 2);
    assert!(matches!(targets[1], render::Target::Place { page: 12, .. }));
    // Pages 2 and 6 saved, and the link between them with them; page 1's
    // link to page 5 is lost, and said to be.
    let some = path_of(&dir, "some.pdf");
    assert!(!arrange::save_pages(&fixture::links_pdf(), None, &[2, 6], &some).unwrap());
    let document = render::open(&some).unwrap();
    let target = document.links_of(0).into_iter().next().unwrap().target;
    assert_eq!(
        target,
        render::Target::Place {
            page: 2,
            spot: None
        }
    );
    assert!(arrange::save_pages(&fixture::links_pdf(), None, &[1], &some).unwrap());
}

/// How many font dictionaries a file carries.
fn fonts_in(path: &str) -> usize {
    let document = lopdf::Document::load(path).unwrap();
    (document.objects.values())
        .filter(|object| object.type_name().is_ok_and(|name| name == b"Font"))
        .count()
}

/// A file the app wrote carries nothing that nothing refers to: pdfium
/// leaves a removed page's objects in the file, and the file is compacted.
#[test]
fn removed_pages_leave_nothing_behind() {
    let dir = folder("compact");
    let path = prose_in(&dir);
    let before = std::fs::metadata(&path).unwrap().len();
    arrange::Edit::Remove(vec![2, 3, 4, 5, 6])
        .write(&path, &path)
        .unwrap();
    let mut document = lopdf::Document::load(&path).unwrap();
    assert_eq!(document.get_pages().len(), 1);
    // The streams the objects are packed in are what the file is laid out
    // with, and nothing refers to them by number.
    let layout: Vec<lopdf::ObjectId> = (document.objects.iter())
        .filter(|(_, object)| {
            object
                .type_name()
                .is_ok_and(|n| n == b"ObjStm" || n == b"XRef")
        })
        .map(|(id, _)| *id)
        .collect();
    assert!(!layout.is_empty(), "the objects are packed");
    assert_eq!(
        document.prune_objects(),
        layout,
        "nothing else is left over"
    );
    assert!(std::fs::metadata(&path).unwrap().len() < before);
    assert_eq!(words(&path).len(), 1);
}

#[test]
fn pages_apart_from_each_other_share_what_they_share_once() {
    let dir = folder("shared");
    let path = prose_in(&dir);
    let (one, three) = (path_of(&dir, "one.pdf"), path_of(&dir, "three.pdf"));
    arrange::save_pages(&path, None, &[1], &one).unwrap();
    arrange::save_pages(&path, None, &[1, 3, 5], &three).unwrap();
    assert!(fonts_in(&one) > 0);
    assert_eq!(
        fonts_in(&three),
        fonts_in(&one),
        "three pages that are not side by side carry one copy of the fonts"
    );
}

#[test]
fn pages_can_be_inserted_before_the_selection_too() {
    let dir = folder("insert-before");
    let path = prose_in(&dir);
    let other = path_of(&dir, "draft.pdf");
    fixture::draft(std::path::Path::new(&other), 2);
    let mut reader = with_thumbnails(
        &path,
        Options {
            chooses: vec![vec![other.clone()]],
            ..Options::default()
        },
    );
    from_thumb(&mut reader, 2, "insert-before");
    assert_eq!(
        reader.dialogs().last(),
        Some(&(Choosing::InsertFrom(2), None))
    );
    in_place(&mut reader);
    assert_eq!(words(&path)[1..3], words(&other)[..]);
}

#[test]
fn the_bar_turns_pages_left_as_well_as_right() {
    let dir = folder("bar-left");
    let path = prose_in(&dir);
    let size = render::open(&path).unwrap().size_of(0);
    let mut reader = with_thumbnails(&path, Options::default());
    let (x, y) = thumb(&mut reader, 1);
    reader.click_at(x, y);
    let (x, y) = thumb(&mut reader, 2);
    reader.click_at_with(x, y, Modifiers::SHIFT);
    reader.click(".thumb-foot [data-item=turn-left]");
    in_place(&mut reader);
    let document = render::open(&path).unwrap();
    let turned = document.size_of(1);
    assert_eq!((turned.width, turned.height), (size.height, size.width));
    assert_eq!(
        reader.state().notice,
        "Pages 1 and 2 are turned left in the file."
    );
}

#[test]
fn links_that_no_longer_work_are_said() {
    let dir = folder("links-said");
    let path = prose_in(&dir);
    let to = path_of(&dir, "first.pdf");
    let mut reader = with_thumbnails(
        &path,
        Options {
            chooses: vec![vec![fixture::links_pdf()]],
            ..Options::default()
        },
    );
    from_thumb(&mut reader, 1, "insert-before");
    in_place(&mut reader);
    assert!(
        !reader.state().notice.contains("do not work"),
        "the links between the pages put in are written again: {}",
        reader.state().notice
    );

    // Saving the page with the link into the document.
    let links = fixture::links_pdf();
    let mut reader = Reader::open_with(
        &links,
        Options {
            chooses: vec![vec![to.clone()]],
            ..Options::default()
        },
    );
    reader.click(".chip.title");
    reader.click("[data-item=save-pages]");
    reader.settle();
    assert!(
        reader
            .state()
            .notice
            .contains("Links in them to other pages of this document do not work in the new file."),
        "{}",
        reader.state().notice
    );
    // And not for pages with no such link: page 7 is the first of prose's.
    let other = path_of(&dir, "plain.pdf");
    assert!(!arrange::save_pages(&path, None, &[7], &other).unwrap());
}

#[test]
fn select_pages_in_the_document_menu_opens_the_thumbnails_on_the_page() {
    let dir = folder("select-pages");
    let path = prose_in(&dir);
    let mut reader = Reader::open_with(&path, Options::default());
    reader.click(".chip.title");
    reader.click("[data-item=select-pages]");
    assert!(reader.harness.query(".tab.on[data-tab='pages']").is_some());
    assert_eq!(picked(&reader), ["1"], "the page being read is selected");
    assert_eq!(
        reader.harness.text_content(".thumb-foot-count"),
        "1 page selected"
    );
}

#[test]
fn the_last_thumbnail_scrolls_clear_of_the_bar() {
    let dir = folder("clear");
    let path = path_of(&dir, "long.pdf");
    fixture::draft(std::path::Path::new(&path), 9);
    let mut reader = with_thumbnails(&path, Options::default());
    let (x, y) = thumb(&mut reader, 1);
    reader.click_at(x, y);
    let (x, y) = thumb(&mut reader, 2);
    reader.click_at_with(x, y, Modifiers::SHIFT);
    reader.wheel_over(".panel.thumb-column", 50000.0);
    reader.point_to(1.0, 1.0);
    let last = reader
        .box_of(".thumb[data-thumb='9']")
        .expect("the last row");
    let foot = reader.box_of(".thumb-foot").expect("the bar");
    assert!(
        last.1 + last.3 <= foot.1 + 0.5,
        "the last row ends at {} and the bar starts at {}",
        last.1 + last.3,
        foot.1
    );
}

#[test]
fn a_drag_held_at_the_bottom_of_the_column_scrolls_it() {
    let dir = folder("roll");
    let path = path_of(&dir, "long.pdf");
    fixture::draft(std::path::Path::new(&path), 9);
    let mut reader = with_thumbnails(&path, Options::default());
    let from = thumb(&mut reader, 1);
    reader.harness.mouse_down_at(from.0, from.1);
    reader.carry(from.0, from.1 + 30.0);
    reader.carry(from.0, 895.0);
    reader.settle();
    let scrolled = |reader: &mut Reader| {
        reader.attribute_all("[data-thumb-scroll]", "data-thumb-scroll")[0]
            .parse::<f64>()
            .unwrap()
            > 100.0
    };
    assert!(
        reader.wait_until(5.0, scrolled),
        "the column scrolled under a pointer that stopped"
    );
    reader.press("Escape");
    reader.harness.mouse_up_at(from.0, 895.0);
    reader.settle();
}

#[test]
fn what_is_dragged_is_shown_as_a_picture() {
    let dir = folder("picture");
    let path = prose_in(&dir);
    let mut reader = with_thumbnails(&path, Options::default());
    let (x, y) = thumb(&mut reader, 1);
    reader.click_at(x, y);
    let (x, y) = thumb(&mut reader, 2);
    reader.click_at_with(x, y, Modifiers::SHIFT);
    let from = thumb(&mut reader, 1);
    reader.harness.mouse_down_at(from.0, from.1);
    reader.carry(from.0 + 20.0, from.1 + 40.0);
    reader.settle();
    assert!(reader.harness.query(".thumb-lift.stacked object").is_some());
    assert_eq!(reader.harness.text_content(".thumb-lift-count"), "2");
    reader.press("Escape");
    reader.harness.mouse_up_at(from.0 + 20.0, from.1 + 40.0);
    reader.settle();
}

/// Each page as pdfium draws it.
fn drawn(path: &str) -> Vec<Vec<u8>> {
    let document = render::open(path).unwrap();
    (0..document.pages())
        .map(|index| {
            (document.render_owned(index, 306, 396, moonowl::layout::View::WHOLE))
                .unwrap()
                .bgra
        })
        .collect()
}

#[test]
fn a_saved_page_brings_only_the_pictures_it_draws() {
    let dir = folder("slim");
    let path = fixture::shared_pdf();
    let (first, second) = (path_of(&dir, "first.pdf"), path_of(&dir, "second.pdf"));
    arrange::save_pages(&path, None, &[1], &first).unwrap();
    arrange::save_pages(&path, None, &[2], &second).unwrap();
    let size = |path: &str| std::fs::metadata(path).unwrap().len();
    let picture = 128 * 128 * 3;
    assert!(size(&first) < 10_000, "no picture: {} bytes", size(&first));
    assert!(
        size(&second) > picture,
        "the picture it draws: {} bytes",
        size(&second)
    );
    // And each page is drawn exactly as it was.
    let all = drawn(&path);
    assert_eq!(drawn(&first)[0], all[0]);
    assert_eq!(drawn(&second)[0], all[1]);
}

#[test]
fn combined_files_are_slimmed_too_and_drawn_the_same() {
    let dir = folder("slim-combined");
    let to = path_of(&dir, "both.pdf");
    let path = fixture::shared_pdf();
    arrange::combine(&[path.clone(), fixture::prose_pdf()], &to).unwrap();
    let (theirs, ours) = (drawn(&path), drawn(&to));
    assert_eq!(ours.len(), 9);
    assert_eq!(ours[..3], theirs[..]);
    assert_eq!(ours[3..], drawn(&fixture::prose_pdf())[..]);
}
