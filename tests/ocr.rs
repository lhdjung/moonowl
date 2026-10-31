//! Text recognised on a scanned page and written into it. See `ocr.rs`.

use moonowl::app::Choosing;
use moonowl::emit::{Event, News};
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

/// The scans are recognised, the one with a line stamped on it too, and the
/// page of text is not.
#[test]
fn only_the_scanned_pages_are_recognised() {
    if !can_recognise() {
        return;
    }
    let document = render::open(&fixture::scanned_pdf()).unwrap();
    let found = ocr::recognise(&*document).expect("recognised");
    let pages: Vec<usize> = found.iter().map(|(page, _)| *page).collect();
    assert_eq!(pages, [0, 2]);
    let stamped: Vec<&str> = found[1].1.iter().map(|word| word.text.as_str()).collect();
    assert!(
        stamped.join(" ").contains(fixture::SCANNED[0]),
        "{stamped:?}"
    );
    // The stamp is text already, and is not recognised a second time.
    assert!(!stamped.contains(&"archive"), "{stamped:?}");
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

/// A Tesseract that never answers is given up on, so the thread recognising
/// the pages, and every write waiting on it, is not held for ever.
#[cfg(unix)]
#[test]
fn a_tesseract_that_does_not_answer_is_given_up_on() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("moonowl-tesseract-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let program = dir.join("tesseract");
    std::fs::write(&program, "#!/bin/sh\ncat > /dev/null\nsleep 30\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let began = std::time::Instant::now();
    let answer = ocr::tesseract_at(
        program.to_str().unwrap(),
        std::time::Duration::from_millis(300),
        &[255; 16],
        4,
        4,
        72.0,
    );
    assert_eq!(answer, Err("Tesseract did not answer in time.".into()));
    assert!(
        began.elapsed().as_secs() < 5,
        "gave up after {:?}",
        began.elapsed()
    );
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
    let copy = path.replace("scan.pdf", "scan (searchable).pdf");
    ocr::write(&path, &found, &copy).expect("written");
    let after = render::open(&copy).unwrap();
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

/// "Recognise text…" asks where to save the copy, as the file dialog
/// would be answered.
fn recognise_into(reader: &mut Reader, copy: &str) {
    reader.click(".chip.title");
    reader.click("[data-item=recognise]");
    assert!(reader
        .dialogs()
        .iter()
        .any(|(choosing, _)| *choosing == Choosing::SaveRecognised));
    reader.deliver(News {
        event: Event::Chosen(Choosing::SaveRecognised, vec![copy.to_string()]),
        target: None,
    });
}

/// The text is recognised into a copy, which the notice offers to open.
/// The document itself is left as it was.
#[test]
fn recognise_text_from_the_document_menu_into_a_copy() {
    if !can_recognise() {
        return;
    }
    let path = scan_in("menu");
    let copy = path.replace("scan.pdf", "scan (searchable).pdf");
    let mut reader = Reader::open(&path);
    recognise_into(&mut reader, &copy);
    reader.settle();
    // The notice ends in its Open button.
    assert_eq!(
        reader.state().notice,
        "Saved scan (searchable).pdf, with the text of 2 pages recognised.Open"
    );
    assert!(text(&*render::open(&copy).unwrap(), 0).contains("scanned pages"));
    assert_eq!(text(&*render::open(&path).unwrap(), 0), "");
}

/// A document whose pages are all text, with no pictures, has nothing to
/// recognise, and no copy is saved.
#[test]
fn a_document_of_text_has_nothing_to_recognise() {
    if !can_recognise() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("moonowl-ocr-{}-typed", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let copy = dir.join("copy.pdf").to_string_lossy().into_owned();
    let mut reader = Reader::open(&fixture::references_pdf());
    recognise_into(&mut reader, &copy);
    reader.settle();
    assert_eq!(reader.state().notice, "No text to recognise.");
    assert!(!std::path::Path::new(&copy).exists());
}

/// A certified document is signed, and a copy with text written into its
/// pages would break the signature, so none is offered.
#[test]
fn a_certified_document_is_not_recognised() {
    let mut reader = Reader::open(&fixture::certified_pdf(1));
    reader.click(".chip.title");
    reader.click("[data-item=recognise]");
    assert!(
        reader.state().notice.starts_with("This document is signed"),
        "{}",
        reader.state().notice
    );
    assert!(reader.dialogs().is_empty());
}

/// A mark made while the pages are being recognised is refused. Writing it
/// would let go of the document under the thread reading it, and every page
/// recognised so far would be lost.
#[test]
fn a_mark_waits_while_text_is_recognised() {
    if !can_recognise() {
        return;
    }
    let path = scan_in("marked");
    let copy = path.replace("scan.pdf", "scan (searchable).pdf");
    let mut reader = Reader::open(&path);
    reader.while_writing(|reader| {
        recognise_into(reader, &copy);
        reader.box_page(1, (0.2, 0.4), (0.6, 0.7));
        reader.click(".markup-swatch");
        assert_eq!(
            reader.state().notice,
            "Moonowl is still busy with the last file. Try again in a moment."
        );
    });
    assert!(text(&*render::open(&copy).unwrap(), 0).contains("scanned pages"));
    assert!(render::open(&path).unwrap().markup().is_empty());
}

/// Words found on a document that changes on disk before the copy is
/// written are not written, since its pages may now be other pages.
#[test]
fn a_new_draft_while_text_is_recognised_saves_no_copy() {
    if !can_recognise() {
        return;
    }
    let path = scan_in("redrafted");
    let copy = path.replace("scan.pdf", "scan (searchable).pdf");
    let mut reader = Reader::open(&path);
    reader.while_writing(|reader| {
        recognise_into(reader, &copy);
        // The words are found, and wait to be written.
        reader.settle();
        fixture::draft(std::path::Path::new(&path), 2);
        reader.document_changed(&path);
    });
    assert!(!std::path::Path::new(&copy).exists());
}

/// A recognised letter that no font on the computer has costs only its own
/// word. The words are never drawn, so leaving one out changes nothing that
/// is seen.
#[test]
fn a_letter_no_font_has_leaves_out_only_its_word() {
    let path = scan_in("unsettable");
    let word = |text: &str, left: f64| ocr::Word {
        text: text.into(),
        at: render::Rect {
            left,
            top: 0.1,
            width: 0.2,
            height: 0.03,
        },
    };
    // U+E000 is in the private use area, which no shipped font draws.
    let found = vec![(0, vec![word("Moonowl", 0.1), word("\u{e000}", 0.4)])];
    ocr::write(&path, &found, &path).expect("written");
    assert_eq!(text(&*render::open(&path).unwrap(), 0).trim(), "Moonowl");
}
