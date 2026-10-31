//! Text recognised on a scanned page and written into it. See `ocr.rs`.

use moonowl::fixture;
use moonowl::harness::Reader;
use moonowl::layout::View;
use moonowl::ocr;
use moonowl::render::{self, PageSource};

const PLAIN: View = View {
    rotation: 0,
    crop: None,
};

/// A copy of the scanned fixture in a directory of this test's own.
fn scan_in(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("moonowl-ocr-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory to write in");
    let path = dir.join("scan.pdf");
    std::fs::copy(fixture::scanned_pdf(), &path).expect("a copy of the fixture");
    path.to_string_lossy().into_owned()
}

fn text(document: &dyn PageSource, index: usize) -> String {
    document.text_of(index).chars.into_iter().collect()
}

/// Off macOS the words come from Tesseract, which a machine may not have.
fn can_recognise() -> bool {
    cfg!(target_os = "macos")
        || std::process::Command::new("tesseract")
            .arg("--version")
            .output()
            .is_ok()
}

#[test]
fn only_the_page_without_text_is_recognised() {
    if !can_recognise() {
        return;
    }
    let document = render::open(&fixture::scanned_pdf()).unwrap();
    let found = ocr::recognise(&*document).expect("recognised");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, 0);
    let words: Vec<&str> = found[0].1.iter().map(|word| word.text.as_str()).collect();
    assert_eq!(words.join(" "), fixture::SCANNED.join(" "));
    // "Moonowl" is set at 28pt from 72pt in and 112pt down a 612 by 792 page.
    let first = &found[0].1[0].at;
    assert!((first.left * 612.0 - 72.0).abs() < 6.0, "{first:?}");
    assert!(
        (first.top + first.height) * 792.0 > 112.0 - 4.0,
        "{first:?}"
    );
    assert!(first.top * 792.0 < 112.0 - 14.0, "{first:?}");
}

#[test]
fn tesseract_finds_the_same_words() {
    if std::process::Command::new("tesseract")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let document = render::open(&fixture::scanned_pdf()).unwrap();
    let drawn = document.render_owned(0, 1224, 1584, PLAIN).unwrap();
    let grey: Vec<u8> = drawn
        .bgra
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| pixel[1])
        .collect();
    let words = ocr::tesseract(&grey, 1224, 1584, 144.0).expect("read");
    let words: Vec<&str> = words.iter().map(|word| word.text.as_str()).collect();
    assert_eq!(words.join(" "), fixture::SCANNED.join(" "));
}

/// The words go into the page as text that is not drawn: the page looks as
/// it did, and its text is the words.
#[test]
fn recognised_words_are_written_into_the_page_unseen() {
    if !can_recognise() {
        return;
    }
    let path = scan_in("written");
    let before = render::open(&path).unwrap();
    let found = ocr::recognise(&*before).unwrap();
    let pixels =
        |document: &dyn PageSource| document.render_owned(0, 306, 396, PLAIN).unwrap().bgra;
    let looked = pixels(&*before);
    before.release();
    ocr::write(&path, &found).expect("written");
    let after = render::open(&path).unwrap();
    let read = text(&*after, 0);
    for line in fixture::SCANNED {
        assert!(read.contains(line), "{read:?}");
    }
    assert_eq!(text(&*after, 1), "This page has text.");
    // "M" covers the M in the picture, which is 28pt type 72pt in, with
    // its top at 92pt down and its foot on the baseline at 112pt, and
    // pdfium's box for a character reaches below the baseline.
    let m = after.text_of(0).boxes[0];
    assert!((m.left - 72.0).abs() < 4.0, "{m:?}");
    assert!(m.top < 94.0 && m.top > 82.0, "{m:?}");
    assert!(
        m.top + m.height > 112.0 && m.top + m.height < 122.0,
        "{m:?}"
    );
    assert!(pixels(&*after) == looked, "the page looks as it did");
}

#[test]
fn recognise_text_from_the_document_menu_and_undo_it() {
    if !can_recognise() {
        return;
    }
    let path = scan_in("menu");
    let mut reader = Reader::open(&path);
    reader.click(".chip.title");
    reader.click("[data-item=recognise]");
    reader.settle();
    assert_eq!(reader.state().notice, "Recognised the text of 1 page.");
    assert!(text(&*render::open(&path).unwrap(), 0).contains("scanned pages"));
    reader.press_chord("mod+f");
    reader.type_text("scanned pages");
    reader.scan_out();
    assert_eq!(reader.state().find.as_deref(), Some("1 of 1"));
    reader.press("Escape");
    reader.press_chord("mod+z");
    reader.settle();
    assert_eq!(text(&*render::open(&path).unwrap(), 0), "");
}
