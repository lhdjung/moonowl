//! Page edits: saving pages as a new file, combining files, and removing,
//! turning, moving and inserting pages in the open document.

use dioxus::html::Modifiers;
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
    let lost = combine(&[&one, &other], &to).expect("combined");
    assert!(!lost, "neither file links between its own pages");
    assert_eq!(render::open(&to).unwrap().pages(), 9);
    assert_eq!(words(&to)[..6], words(&one)[..]);
}

#[test]
fn an_encrypted_file_is_not_combined() {
    let dir = folder("locked");
    let to = path_of(&dir, "both.pdf");
    let refused = arrange::assemble(None, &[sheet(Some(&fixture::locked_pdf()), 1, 0)], &to);
    assert!(refused.is_err_and(|said| said.contains("encrypted")));
    assert!(!std::path::Path::new(&to).exists());
}

/// Every page of every file in `paths`, in that order, in a new file at
/// `to`. A picture is one page.
fn combine(paths: &[&str], to: &str) -> Result<bool, String> {
    let sheets: Vec<arrange::Sheet> = (paths.iter())
        .flat_map(|path| {
            let count = render::open(path).map_or(1, |document| document.pages());
            (1..=count).map(move |page| sheet(Some(path), page, 0))
        })
        .collect();
    arrange::assemble(None, &sheets, to)
}

/// Every page of a document of `count` pages but `gone`, in order.
fn keep(count: usize, gone: &[usize]) -> Vec<arrange::Sheet> {
    (1..=count)
        .filter(|page| !gone.contains(page))
        .map(|page| sheet(None, page, 0))
        .collect()
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

fn marked_pages(reader: &Reader) -> Vec<u32> {
    reader.flush();
    moonowl::store::flush();
    moonowl::library::load(&reader.config)
        .files
        .iter()
        .flat_map(|entry| entry.marks.iter().map(|mark| mark.page))
        .collect()
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
    arrange::Edit::Arrange(keep(12, &[4])).write(&path).unwrap();
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
    // "Front matter" on page 1 goes with its page, and "Preface", on page
    // 2 under it, with its own.
    arrange::Edit::Arrange(keep(11, &[1, 2]))
        .write(&path)
        .unwrap();
    assert_eq!(contents(&path)[0].0, "Chapter One");
}

#[test]
fn links_to_removed_pages_are_taken_off_and_keep_nothing_in_the_file() {
    let dir = folder("links-gone");
    let path = path_of(&dir, "links.pdf");
    std::fs::copy(fixture::links_pdf(), &path).unwrap();
    // Page 1 links to page 5, and page 2 to page 6.
    arrange::Edit::Arrange(keep(6, &[5, 6]))
        .write(&path)
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
    let order = [1, 3, 4, 5, 6, 2].map(|page| sheet(None, page, 0));
    arrange::Edit::Arrange(order.to_vec()).write(&path).unwrap();
    assert_eq!(labels_of(&path), ["i", "iii", "1", "2", "3", "ii"]);
    let wrote = arrange::Edit::Arrange(keep(6, &[1])).write(&path).unwrap();
    assert!(wrote.labels_kept);
    assert_eq!(labels_of(&path), ["iii", "1", "2", "3", "ii"]);
    // Pages put in from a file with no labels are labelled with their
    // places, which other readers show too.
    let other = path_of(&dir, "draft.pdf");
    fixture::draft(std::path::Path::new(&other), 2);
    let mut sheets = keep(5, &[]);
    sheets.splice(1..1, [sheet(Some(&other), 1, 0), sheet(Some(&other), 2, 0)]);
    arrange::Edit::Arrange(sheets).write(&path).unwrap();
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
    // With positions shown, the field still offers the page's label.
    reader.click(".of.choice");
    reader.click_nth(".menu.numbering .menu-item", 1);
    from_thumb(&mut reader, 4, "number-from");
    assert_eq!(reader.field(".number-field"), "A-2");
    // Left as it is, nothing is written.
    let before = std::fs::read(&path).unwrap();
    reader.press("Enter");
    reader.settle();
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // Numbered from the page clicked.
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
    assert_eq!(labels_of(&path)[..4], ["407", "408", "A-1", "A-2"]);
}

#[test]
fn combined_files_bring_the_headings_of_each() {
    let dir = folder("headings-combined");
    let to = path_of(&dir, "both.pdf");
    let contents_pdf = fixture::contents_pdf();
    combine(&[&fixture::prose_pdf(), &contents_pdf], &to).unwrap();
    let prose = render::open(&fixture::prose_pdf()).unwrap().pages();
    assert_eq!(contents(&to), expected(|page| Some(page + prose)));
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

#[test]
fn an_encrypted_file_is_not_inserted() {
    let dir = folder("insert-locked");
    let path = prose_in(&dir);
    let mut sheets = keep(6, &[]);
    sheets.insert(0, sheet(Some(&fixture::locked_pdf()), 1, 0));
    let refused = arrange::Edit::Arrange(sheets).write(&path);
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
    let links = fixture::links_pdf();
    let mut sheets = keep(6, &[]);
    sheets.extend((1..=6).map(|page| sheet(Some(&links), page, 0)));
    arrange::Edit::Arrange(sheets).write(&path).unwrap();
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
    combine(&[&fixture::prose_pdf(), &fixture::links_pdf()], &both).unwrap();
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
    arrange::Edit::Arrange(keep(6, &[2, 3, 4, 5, 6]))
        .write(&path)
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
    combine(&[&path, &fixture::prose_pdf()], &to).unwrap();
    let (theirs, ours) = (drawn(&path), drawn(&to));
    assert_eq!(ours.len(), 9);
    assert_eq!(ours[..3], theirs[..]);
    assert_eq!(ours[3..], drawn(&fixture::prose_pdf())[..]);
}

/// A JPEG whose EXIF says it was taken turned: shown upright, it is
/// `height` wide and `width` tall. As stored, its left half is red and its
/// right half blue, so shown upright its top half is red.
fn turned_jpeg(width: u32, height: u32) -> Vec<u8> {
    let mut plain = Vec::new();
    let picture = image::RgbImage::from_fn(width, height, |x, _| match x < width / 2 {
        true => image::Rgb([255, 0, 0]),
        false => image::Rgb([0, 0, 255]),
    });
    image::codecs::jpeg::JpegEncoder::new(&mut plain)
        .encode_image(&picture)
        .unwrap();
    // An APP1 segment holding one EXIF tag, Orientation (0x0112) = 6,
    // which means the picture is shown turned a quarter clockwise.
    let mut exif = vec![0xFF, 0xE1, 0x00, 0x22];
    exif.extend(b"Exif\0\0II\x2A\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0");
    let mut turned = plain[..2].to_vec();
    turned.extend(exif);
    turned.extend(&plain[2..]);
    turned
}

#[test]
fn pictures_are_combined_as_pages_of_their_own_shape() {
    let dir = folder("pictures");
    let png = path_of(&dir, "red.png");
    image::RgbaImage::from_pixel(200, 100, image::Rgba([255, 0, 0, 255]))
        .save(&png)
        .unwrap();
    let jpeg = path_of(&dir, "turned.jpg");
    std::fs::write(&jpeg, turned_jpeg(40, 20)).unwrap();
    let pdf = prose_in(&dir);
    let to = path_of(&dir, "together.pdf");
    combine(&[&png, &jpeg, &pdf], &to).expect("the files are combined");
    let document = render::open(&to).expect("the new file opens");
    assert_eq!(
        document.pages(),
        8,
        "a page for each picture, and the six of the PDF"
    );
    let size = |index| {
        let size = document.size_of(index);
        (size.width.round(), size.height.round())
    };
    assert_eq!(size(0), (842.0, 421.0), "a wide picture on A4, turned");
    assert_eq!(
        size(1),
        (421.0, 842.0),
        "the photo upright, as its EXIF says"
    );
    let drawn = document
        .render_owned(0, 100, 50, moonowl::layout::View::WHOLE)
        .unwrap();
    let middle = &drawn.bgra[(25 * 100 + 50) * 4..][..4];
    assert!(
        middle[2] > 200 && middle[1] < 50 && middle[0] < 50,
        "the page shows the picture: {middle:?}"
    );
    let drawn = document
        .render_owned(1, 50, 100, moonowl::layout::View::WHOLE)
        .unwrap();
    let at = |y: usize| &drawn.bgra[(y * 50 + 12) * 4..][..3];
    assert!(at(20)[2] > 200 && at(20)[0] < 80, "red above: {:?}", at(20));
    assert!(
        at(80)[0] > 200 && at(80)[2] < 80,
        "blue below: {:?}",
        at(80)
    );
}

#[test]
fn a_png_whose_exif_says_it_is_turned_is_made_upright() {
    // One EXIF tag, Orientation (0x0112) = 6: shown turned a quarter
    // clockwise, so a picture stored 100 wide and 140 tall is wide.
    let exif = b"II\x2A\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0".to_vec();
    let mut info = png::Info::with_size(100, 140);
    info.color_type = png::ColorType::Rgb;
    info.bit_depth = png::BitDepth::Eight;
    info.exif_metadata = Some(exif.into());
    let mut bytes = Vec::new();
    let mut writer = png::Encoder::with_info(&mut bytes, info)
        .unwrap()
        .write_header()
        .unwrap();
    writer.write_image_data(&[200; 100 * 140 * 3]).unwrap();
    writer.finish().unwrap();
    let made = arrange::picture_pdf(&bytes).expect("a page");
    let dir = folder("png-exif");
    let path = path_of(&dir, "page.pdf");
    std::fs::write(&path, made).unwrap();
    let size = render::open(&path).unwrap().size_of(0);
    assert!(size.width > size.height, "{size:?}");
}

#[test]
fn a_picture_is_inserted_as_a_page() {
    let dir = folder("insert-picture");
    let path = prose_in(&dir);
    let png = path_of(&dir, "scan.png");
    image::RgbaImage::from_pixel(100, 140, image::Rgba([0, 0, 0, 0]))
        .save(&png)
        .unwrap();
    let mut sheets = keep(6, &[]);
    sheets.insert(1, sheet(Some(&png), 1, 0));
    arrange::Edit::Arrange(sheets)
        .write(&path)
        .expect("the picture goes in");
    let document = render::open(&path).expect("the document opens");
    assert_eq!(document.pages(), 7);
    let drawn = document
        .render_owned(1, 50, 70, moonowl::layout::View::WHOLE)
        .unwrap();
    assert!(
        drawn.bgra[(35 * 50 + 25) * 4..][..3]
            .iter()
            .all(|&c| c > 250),
        "a clear picture is drawn onto white"
    );
}

fn sheet(from: Option<&str>, page: usize, turn: i32) -> arrange::Sheet {
    arrange::Sheet {
        from: from.map(str::to_string),
        page,
        turn,
    }
}

#[test]
fn an_arrangement_removes_turns_moves_and_adds_pages_in_one_write() {
    let dir = folder("arrange");
    let path = prose_in(&dir);
    let all = words(&path);
    let other = path_of(&dir, "draft.pdf");
    fixture::draft(std::path::Path::new(&other), 3);
    let theirs = words(&other);
    let size = render::open(&path).unwrap().size_of(0);
    let edit = arrange::Edit::Arrange(vec![
        sheet(None, 3, 0),
        sheet(Some(&other), 2, 0),
        sheet(None, 1, 1),
        sheet(None, 6, 0),
    ]);
    edit.write(&path).expect("arranged");
    assert_eq!(
        words(&path),
        vec![
            all[2].clone(),
            theirs[1].clone(),
            all[0].clone(),
            all[5].clone()
        ]
    );
    let turned = render::open(&path).unwrap().size_of(2);
    assert_eq!((turned.width, turned.height), (size.height, size.width));
    let moved = edit.moved(6);
    assert_eq!(moved.0, vec![Some(2), None, Some(0), None, None, Some(3)]);
    assert!(
        arrange::Edit::Arrange((1..=4).map(|page| sheet(None, page, 0)).collect()).moves_nothing(4)
    );
}

#[test]
fn an_arrangement_keeps_the_links_between_added_pages() {
    let dir = folder("arrange-links");
    let path = prose_in(&dir);
    let links = fixture::links_pdf();
    // Pages 6, then 2 of the fixture, which links to page 6: now page 1.
    arrange::Edit::Arrange(vec![
        sheet(Some(&links), 6, 0),
        sheet(None, 1, 0),
        sheet(Some(&links), 2, 0),
    ])
    .write(&path)
    .unwrap();
    let document = render::open(&path).unwrap();
    let target = document.links_of(2).into_iter().next().unwrap().target;
    assert_eq!(
        target,
        render::Target::Place {
            page: 1,
            spot: None
        }
    );
}

#[test]
fn assembled_pages_come_from_several_files_in_order_and_turned() {
    let dir = folder("assemble");
    let path = prose_in(&dir);
    let all = words(&path);
    let other = path_of(&dir, "draft.pdf");
    fixture::draft(std::path::Path::new(&other), 3);
    let theirs = words(&other);
    let to = path_of(&dir, "new.pdf");
    let size = render::open(&path).unwrap().size_of(0);
    let lost = arrange::assemble(
        Some((&path, None)),
        &[
            sheet(Some(&other), 3, 0),
            sheet(None, 2, -1),
            sheet(Some(&other), 1, 0),
            sheet(None, 5, 0),
        ],
        &to,
    )
    .expect("assembled");
    assert!(!lost);
    assert_eq!(
        words(&to),
        vec![
            theirs[2].clone(),
            all[1].clone(),
            theirs[0].clone(),
            all[4].clone()
        ]
    );
    let turned = render::open(&to).unwrap().size_of(1);
    assert_eq!((turned.width, turned.height), (size.height, size.width));
}

/* ------------------------------------------- through the page organiser */

/// "Arrange pages…" from the Document menu.
fn organise(reader: &mut Reader) {
    reader.click(".chip.title");
    reader.click("[data-item=arrange]");
    reader.settle();
}

/// Click a page in the organiser's grid, by its place in the draft.
fn pick(reader: &mut Reader, at: usize, modifiers: Modifiers) {
    let (x, y) = in_grid(reader, at);
    reader.click_at_with(x, y, modifiers);
    reader.settle();
}

/// The middle of a page in the organiser's grid, in the window.
fn in_grid(reader: &mut Reader, at: usize) -> (f32, f32) {
    reader.point_to(1.0, 1.0);
    let picture = reader
        .harness
        .layout_rect(&format!(".sheet[data-sheet='{at}'] .sheet-picture"));
    (
        picture.x + picture.width / 2.0,
        picture.y + picture.height / 2.0,
    )
}

fn save_into(reader: &mut Reader) {
    reader.click("[data-item=save-into]");
    reader.settle();
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

    organise(&mut reader);
    pick(&mut reader, 1, Modifiers::empty());
    reader.click("[data-item=remove-pages]");
    save_into(&mut reader);
    assert_eq!(reader.state().pages, 5);
    let now = words(&path);
    assert_eq!(now[1], all[2], "the old page 3 is page 2");
    assert_eq!(marked_pages(&reader), [2], "and its bookmark went with it");
    assert_eq!(reader.state().page, 2, "the reader is still on that page");

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
    let quads = document.text_of(1).quads(0, 7);
    drop(document);
    moonowl::markup::add(&path, &[(2, quads)], "#ffd60a", "Moonowl").expect("marked");
    let mut reader = Reader::open(&path);
    let journal = |reader: &Reader| {
        reader.flush();
        moonowl::store::flush();
        (moonowl::library::load(&reader.config).files.iter())
            .flat_map(|entry| entry.highlights.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(journal(&reader).len(), 1);

    organise(&mut reader);
    pick(&mut reader, 1, Modifiers::empty());
    reader.press("Delete");
    save_into(&mut reader);
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
    let mut reader = Reader::open(&path);
    organise(&mut reader);
    pick(&mut reader, page - 1, Modifiers::empty());
    reader.press("Delete");
    save_into(&mut reader);
    let notice = reader.state().notice;
    let said = "The heading that pointed at removed pages is taken out of the contents.";
    assert!(notice.contains(said), "{notice}");
    assert!(!contents(&path)
        .iter()
        .any(|(title, ..)| *title == headings[1].0));
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
    let mut reader = Reader::open(&path);
    organise(&mut reader);
    reader.click("[data-item=turn-right]");
    save_into(&mut reader);
    assert!(
        reader.harness.query(".ask-window").is_none(),
        "nothing is asked"
    );
    assert!(
        reader.state().notice.contains("certified"),
        "{}",
        reader.state().notice
    );
    assert!(
        reader.harness.query(".organiser").is_some(),
        "the draft stays"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before, "nothing is written");
}

#[test]
fn a_read_only_document_is_not_saved_into() {
    use std::os::unix::fs::PermissionsExt;
    let dir = folder("read-only");
    let path = prose_in(&dir);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    let mut reader = Reader::open(&path);
    organise(&mut reader);
    reader.press("Delete");
    save_into(&mut reader);
    assert_eq!(
        reader.state().notice,
        "This document is read-only, so its pages cannot be changed."
    );
    assert_eq!(pages(&path), 6);
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644));
}

#[test]
fn removing_many_pages_from_the_file_asks_first_and_five_do_not() {
    let dir = folder("many");
    let path = path_of(&dir, "long.pdf");
    fixture::draft(std::path::Path::new(&path), 13);
    let mut reader = Reader::open(&path);
    organise(&mut reader);
    // Five: the second to the sixth.
    pick(&mut reader, 1, Modifiers::empty());
    pick(&mut reader, 5, Modifiers::SHIFT);
    reader.press("Delete");
    save_into(&mut reader);
    assert!(reader.harness.query(".ask-window").is_none());
    assert_eq!(pages(&path), 8);
    // Seven of the eight left: everything but the first page.
    organise(&mut reader);
    reader.press_chord("mod+a");
    pick(&mut reader, 0, command());
    reader.press("Delete");
    save_into(&mut reader);
    assert_eq!(pages(&path), 8, "not yet");
    assert_eq!(
        reader.harness.text_content(".ask-window .window-title"),
        "Remove 7 pages?"
    );
    reader.click(".ask-go");
    reader.settle();
    assert_eq!(pages(&path), 1);
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

    let mut reader = Reader::open(&path);
    organise(&mut reader);
    reader.click("[data-item=turn-right]");
    save_into(&mut reader);
    let notice = reader.state().notice;
    assert!(
        notice.starts_with("Arranged prose.pdf: turned 1 page. "),
        "{notice}"
    );
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
    let mut reader = Reader::open(&path);
    organise(&mut reader);
    pick(&mut reader, 1, Modifiers::SHIFT);
    // Past the last page of the last row: the end.
    let from = in_grid(&mut reader, 0);
    let last = reader
        .harness
        .layout_rect(".sheet[data-sheet='5'] .sheet-picture");
    let to = (last.x + last.width + 60.0, last.y + 10.0);
    reader.harness.mouse_down_at(from.0, from.1);
    for step in 1..=4 {
        let t = step as f32 / 4.0;
        reader.carry(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
    }
    reader.harness.mouse_up_at(to.0, to.1);
    reader.settle();
    save_into(&mut reader);
    let now = words(&path);
    assert_eq!(now[..4], all[2..]);
    assert_eq!(now[4..], all[..2]);
    let marks = render::open(&path).unwrap().markup();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].page, 5, "the highlight went with its page");
    assert_eq!(reader.state().page, 5, "and so did the reader");
    // Undone, the pages and the reader go back, and redone, along again.
    reader.press_chord("mod+z");
    reader.settle();
    assert_eq!(words(&path), all);
    assert_eq!(reader.state().page, 1);
    reader.press_chord("mod+shift+z");
    reader.settle();
    assert_eq!(words(&path), now);
    assert_eq!(reader.state().page, 5);
}

#[test]
fn saving_an_unchanged_arrangement_writes_nothing() {
    let dir = folder("stay");
    let path = prose_in(&dir);
    let before = std::fs::read(&path).unwrap();
    let mut reader = Reader::open(&path);
    organise(&mut reader);
    save_into(&mut reader);
    assert!(reader.harness.query(".organiser").is_none());
    assert_eq!(std::fs::read(&path).unwrap(), before);
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
    organise(&mut reader);
    pick(&mut reader, 1, Modifiers::empty());
    reader.click("[data-item=add-files]");
    reader.settle();
    save_into(&mut reader);
    assert_eq!(reader.state().page, 7, "the reader's page moved along");
    let now = words(&path);
    assert_eq!(now.len(), 9);
    assert_eq!(now[..2], all[..2]);
    assert_eq!(now[2..5], theirs[..]);
    assert_eq!(now[5..], all[2..]);
    let notice = reader.state().notice;
    assert!(
        notice.starts_with("Arranged prose.pdf: added 3 pages from other files. "),
        "{notice}"
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

/// The column is a map of the document: a click goes to a page and selects
/// nothing, and Delete and ⌘A do what they do anywhere else.
#[test]
fn the_thumbnails_go_to_pages_and_select_nothing() {
    let dir = folder("map");
    let path = prose_in(&dir);
    let before = std::fs::read(&path).unwrap();
    let mut reader = with_thumbnails(&path, Options::default());
    let (x, y) = thumb(&mut reader, 3);
    reader.click_at(x, y);
    assert_eq!(reader.state().page, 3);
    assert!(reader.harness.query(".thumb-foot").is_none());
    reader.press("Delete");
    reader.settle();
    assert_eq!(std::fs::read(&path).unwrap(), before);
    reader.press_chord("mod+a");
    assert!(
        reader.harness.query(".selected").is_some(),
        "the page's text is selected"
    );
    // A right-click offers the organiser on that page.
    from_thumb(&mut reader, 2, "arrange");
    assert_eq!(reader.attribute_all(".sheet.picked", "data-sheet"), ["1"]);
}
