//! "Compare with…": another version of the document read word by word,
//! the differences painted on the pages and listed in the Differences tab. See
//! `compare.rs` and `app/compare.rs`.

use moonowl::app::Choosing;
use moonowl::emit::{Event, News};
use moonowl::fixture;
use moonowl::harness::Reader;

#[test]
fn the_changes_are_the_words_added_and_removed() {
    let (earlier, later) = fixture::compared_pdfs();
    let changes = moonowl::compare::compare(
        &*moonowl::render::open(&later).unwrap(),
        &*moonowl::render::open(&earlier).unwrap(),
    )
    .unwrap();
    let said: Vec<(usize, &str, &str)> = changes
        .iter()
        .map(|change| (change.page(), change.there.as_str(), change.here.as_str()))
        .collect();
    assert_eq!(
        said,
        [
            (1, "brown", "red"),
            (1, "lazy", ""),
            (2, "", "And one more line."),
        ]
    );
}

/// A space left out after punctuation is a change, and so is punctuation
/// taken out between two words. A change that runs into the next is one
/// change.
#[test]
fn punctuation_and_spaces_between_the_same_words_are_compared() {
    let (earlier, later) = fixture::line_pdfs(
        "spaced",
        "See p. 73 and Smith & Jones & Lee, the end. Next one.",
        "See p.73 and Smith Jones Lee, the end.Next one.",
    );
    let changes = moonowl::compare::compare(
        &*moonowl::render::open(&later).unwrap(),
        &*moonowl::render::open(&earlier).unwrap(),
    )
    .unwrap();
    let said: Vec<(&str, &str, usize)> = changes
        .iter()
        .map(|change| (change.there.as_str(), change.here.as_str(), change.count))
        .collect();
    assert_eq!(
        said,
        [
            ("p. 73", "p.73", 0),
            ("Smith & Jones & Lee,", "Smith Jones Lee,", 0),
            ("end. Next", "end.Next", 0),
        ]
    );
}

/// Brackets taken out around a word, beside a word taken out, are one
/// change, not that change and the brackets again.
#[test]
fn brackets_beside_a_change_are_said_once() {
    let (earlier, later) = fixture::line_pdfs("bracketed", "the Foo (IO) set", "the IO set");
    let changes = moonowl::compare::compare(
        &*moonowl::render::open(&later).unwrap(),
        &*moonowl::render::open(&earlier).unwrap(),
    )
    .unwrap();
    let said: Vec<(&str, &str)> = changes
        .iter()
        .map(|change| (change.there.as_str(), change.here.as_str()))
        .collect();
    assert_eq!(said, [("Foo (IO)", "IO")]);
}

/// **Running heads and page numbers are not compared**, so a page added
/// at the front is one change and not one on every page after it. A
/// passage that moved is two changes that name each other.
#[test]
fn running_heads_are_left_out_and_a_moved_passage_is_paired() {
    let (earlier, later) = fixture::moved_pdfs();
    let changes = moonowl::compare::compare(
        &*moonowl::render::open(&later).unwrap(),
        &*moonowl::render::open(&earlier).unwrap(),
    )
    .unwrap();
    let said: Vec<(&str, &str, Option<usize>)> = changes
        .iter()
        .map(|change| (change.there.as_str(), change.here.as_str(), change.moved))
        .collect();
    assert_eq!(
        said,
        [
            ("", "A new opening page.", None),
            ("Moved words here.", "", Some(2)),
            ("", "Moved words here.", Some(1)),
        ]
    );
}

/// **A passage the file keeps in another order is not a change** where it
/// is in the same place on the page, and only its changed word is.
#[test]
fn a_passage_kept_in_another_order_is_compared_in_place() {
    let (earlier, later) = fixture::reordered_pdfs();
    let changes = moonowl::compare::compare(
        &*moonowl::render::open(&later).unwrap(),
        &*moonowl::render::open(&earlier).unwrap(),
    )
    .unwrap();
    let said: Vec<(&str, &str, Option<usize>)> = changes
        .iter()
        .map(|change| (change.there.as_str(), change.here.as_str(), change.moved))
        .collect();
    assert_eq!(said, [("here.", "there.", None)]);
}

/// **A passage that moved on its page is a passage that moved**, even
/// where both places are on the same page.
#[test]
fn a_passage_moved_on_its_page_is_shown_as_moved() {
    let (earlier, later) = fixture::swapped_pdfs();
    let changes = moonowl::compare::compare(
        &*moonowl::render::open(&later).unwrap(),
        &*moonowl::render::open(&earlier).unwrap(),
    )
    .unwrap();
    assert_eq!(changes.len(), 2, "{changes:#?}");
    assert!(changes.iter().all(|change| change.moved.is_some()));
}

/// The later version, compared with the earlier one as the menu and the
/// file dialog would.
fn comparing() -> Reader {
    let (earlier, later) = fixture::compared_pdfs();
    comparing_with(&later, earlier)
}

/// The document compared with the file at `path` as the reader does it:
/// Compare with… in the Document menu, the file chosen as A in the window
/// that opens, and Show differences.
fn compare_with(reader: &mut Reader, path: String) {
    reader.click(".chip.title");
    reader.click("[data-item='compare']");
    if reader.harness.query(".report-window").is_none() {
        return;
    }
    reader.click("[data-side='A']");
    assert_eq!(
        reader.dialogs().pop(),
        Some((Choosing::ReportFile(0), None))
    );
    reader.deliver(News {
        event: Event::Chosen(Choosing::ReportFile(0), vec![path]),
        target: None,
    });
    reader.click(".report-show");
}

/// `document`, compared with `earlier` as the menu and the file dialog
/// would.
fn comparing_with(document: &str, earlier: String) -> Reader {
    comparing_in(document, earlier, moonowl::harness::Options::default())
}

/// [`comparing_with`], with `options`.
fn comparing_in(document: &str, earlier: String, options: moonowl::harness::Options) -> Reader {
    let mut reader = Reader::open_with(document, options);
    compare_with(&mut reader, earlier);
    assert!(
        reader.wait_until(5.0, |reader| reader
            .harness
            .query("[data-tab='changes']")
            .is_some()),
        "told {:?}",
        reader.state().notice
    );
    reader
}

#[test]
fn the_changes_tab_lists_them_and_the_pages_show_them() {
    let mut reader = comparing();
    assert_eq!(
        reader.state().notice,
        "3 differences from moonowl-fixture-earlier.pdf."
    );
    assert_eq!(
        reader.text_all(".change-row .result-line"),
        ["brown red", "lazy", "And one more line."]
    );
    // "red" is added on page 1, and a bar stands where "lazy" was.
    assert!(!reader.harness.query_all(".change.added").is_empty());
    assert_eq!(reader.harness.query_all(".change.removed").len(), 1);

    reader.click("[data-change='2']");
    assert_eq!(reader.state().page, 2);
    assert_eq!(reader.harness.query_all(".change-row.current").len(), 1);

    reader.click(".changes-stop");
    assert!(reader.harness.query("[data-tab='changes']").is_none());
    assert!(reader.harness.query_all(".change").is_empty());
}

/// A scan with no words, compared with a version that has some, is one
/// change at the top of its first page, and going to it goes there.
#[test]
fn a_document_with_no_words_is_compared_too() {
    let (earlier, _) = fixture::compared_pdfs();
    let mut reader = Reader::open(&fixture::wordless_pdf());
    compare_with(&mut reader, earlier);
    assert!(
        reader.wait_until(5.0, |reader| reader
            .harness
            .query("[data-tab='changes']")
            .is_some()),
        "told {:?}",
        reader.state().notice
    );
    reader.click("[data-change='0']");
    assert_eq!(reader.state().page, 1);
}

/// Only the first ten thousand characters of a long change are kept, about
/// four pages. A whole book is read on its pages.
#[test]
fn a_long_change_is_cut_short() {
    let changes = moonowl::compare::compare(
        &*moonowl::render::open(&fixture::wordless_pdf()).unwrap(),
        &*moonowl::render::open(&fixture::long_prose_pdf()).unwrap(),
    )
    .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].there.chars().count(), 10_001);
    assert!(changes[0].there.ends_with('\u{2026}'));
}

/// **The change gone to opens under its row**, with each file's words in
/// full and where they are, and a change one file has no words of says so.
#[test]
fn the_change_gone_to_shows_both_sides_in_full() {
    let mut reader = comparing();
    reader.click("[data-change='0']");
    assert_eq!(
        reader.text_all(".change-open .change-side"),
        [
            "moonowl-fixture-earlier.pdf, page 1",
            "This document, page 1"
        ]
    );
    assert_eq!(
        reader.text_all(".change-open .change-text"),
        ["brown", "red"]
    );

    reader.click("[data-change='1']");
    assert_eq!(reader.text_all(".change-open .change-text"), ["lazy"]);
    assert_eq!(
        reader.text_all(".change-open .change-none"),
        ["No words here."]
    );
    assert_eq!(reader.harness.query_all(".change-open").len(), 1);
}

/// **"Show in …" swaps the two files**: the window shows the other one,
/// compared with the document it showed, at the same change, so that the
/// words only the other file has are on the page.
#[test]
fn the_other_file_is_shown_at_the_same_change() {
    let mut reader = comparing();
    reader.click("[data-change='1']");
    reader.click(".change-swap");
    assert!(
        reader.wait_until(5.0, |reader| reader.state().notice
            == "3 differences from moonowl-fixture-later.pdf."),
        "told {:?}",
        reader.state().notice
    );
    // "lazy", which only the earlier file has, is now on the page.
    assert_eq!(reader.text_all(".change-open .change-text"), ["lazy"]);
    assert_eq!(
        reader.text_all(".change-open .change-side"),
        ["moonowl-fixture-later.pdf, page 1", "This document, page 1"]
    );
    assert_eq!(reader.harness.query_all(".change.added.current").len(), 1);
}

/// "Show in …" on a document opened with its password compares the other
/// file with it, opening it again with the same password.
#[test]
fn the_other_file_is_shown_beside_a_locked_document() {
    let (earlier, _) = fixture::moved_pdfs();
    let mut reader = Reader::locked(&fixture::locked_pdf());
    reader.type_text(moonowl::fixture::LOCKED_PASSWORD);
    reader.press("Enter");
    compare_with(&mut reader, earlier);
    assert!(
        reader.wait_until(5.0, |reader| reader
            .harness
            .query("[data-change='0']")
            .is_some()),
        "told {:?}",
        reader.state().notice
    );
    reader.click("[data-change='0']");
    reader.click(".change-swap");
    assert!(
        reader.wait_until(5.0, |reader| reader
            .state()
            .notice
            .ends_with("from moonowl-locked.pdf.")),
        "told {:?}",
        reader.state().notice
    );
}

/// "Show in …" waits while a report is being written, which still reads
/// the document the window would put down.
#[test]
fn the_other_file_is_not_shown_while_a_report_is_written() {
    let mut reader = comparing();
    let dir = std::env::temp_dir().join(format!("moonowl-show-in-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join("report.pdf").to_string_lossy().into_owned();
    reader.click("[data-change='1']");
    reader.while_answering(|reader| {
        reader.deliver(News {
            event: Event::Chosen(Choosing::SaveReport, vec![to.clone()]),
            target: None,
        });
        reader.click(".change-swap");
        assert_eq!(
            reader.state().notice,
            "Still saving the comparison. Try again when it is done, or stop it in the \
             Document menu."
        );
    });
    assert!(
        reader.wait_until(10.0, |reader| reader
            .state()
            .notice
            .starts_with("Saved the comparison as report.pdf.")),
        "told {:?}",
        reader.state().notice
    );
}

#[test]
fn a_document_is_not_compared_with_itself() {
    let (_, later) = fixture::compared_pdfs();
    let mut reader = Reader::open(&later);
    compare_with(&mut reader, later);
    assert_eq!(
        reader.state().notice,
        "That file is already B. Choose a different one."
    );
    assert!(reader.harness.query("[data-tab='changes']").is_none());
}

#[test]
fn a_new_draft_on_disk_puts_the_comparison_away() {
    let mut reader = comparing();
    let (_, later) = fixture::compared_pdfs();
    reader.document_changed(&later);
    assert!(
        reader.wait_until(5.0, |reader| reader
            .harness
            .query("[data-tab='changes']")
            .is_none()),
        "told {:?}",
        reader.state().notice
    );
    assert!(reader.harness.query_all(".change").is_empty());
}

/// A comparison that is ready only after a new draft has landed was made
/// with the draft that has gone, and is not shown on this one.
#[test]
fn a_comparison_finished_after_a_new_draft_is_not_shown() {
    let (earlier, later) = fixture::compared_pdfs();
    let mut reader = Reader::open(&later);
    reader.while_answering(|reader| {
        compare_with(reader, earlier);
        reader.settle();
        reader.document_changed(&later);
        reader.settle();
    });
    assert!(reader.harness.query("[data-tab='changes']").is_none());
    assert!(reader.harness.query_all(".change").is_empty());
    assert_eq!(
        reader.state().notice,
        "The document changed while it was being read, so nothing was done. Try again."
    );
}

/// Nothing is compared while the document reloads, since the answer would
/// be about the draft being put down.
#[test]
fn nothing_is_compared_while_the_document_reloads() {
    let (earlier, later) = fixture::compared_pdfs();
    let mut reader = Reader::open(&later);
    reader.while_writing(|reader| {
        reader.document_changed(&later);
        compare_with(reader, earlier);
        assert_eq!(
            reader.state().notice,
            "The document is being reloaded. Try again in a moment."
        );
    });
    assert!(reader.harness.query("[data-tab='changes']").is_none());
}

/// **Two documents can be compared from the start screen**: Compare
/// documents… opens the comparison window with nothing chosen, and once
/// both files are, Show differences opens B in the window and compares it
/// with A.
#[test]
fn two_documents_are_compared_from_the_start_screen() {
    let (earlier, later) = fixture::compared_pdfs();
    let mut reader = Reader::empty(moonowl::harness::Options::default());
    reader.click(".start-compare");
    assert_eq!(
        reader.text_all(".report-file"),
        ["Choose a file\u{2026}", "Choose a file\u{2026}"]
    );
    choose_sides(&mut reader, earlier, later);
    assert_eq!(
        reader.text_all(".report-file"),
        ["moonowl-fixture-earlier.pdf", "moonowl-fixture-later.pdf"]
    );
    reader.click(".report-show");
    assert!(
        reader.wait_until(5.0, |reader| reader
            .harness
            .query("[data-tab='changes']")
            .is_some()),
        "told {:?}",
        reader.state().notice
    );
    assert!(reader.harness.query(".report-window").is_none());
    assert_eq!(reader.state().pages, 2);
}

/// `a` and `b` chosen for A and B in the comparison window.
fn choose_sides(reader: &mut Reader, a: String, b: String) {
    for (side, path) in [(0, a), (1, b)] {
        reader.deliver(News {
            event: Event::Chosen(Choosing::ReportFile(side), vec![path]),
            target: None,
        });
    }
}

/// Two documents to compare whose second does not open are not compared
/// when that file is opened later.
#[test]
fn a_comparison_whose_document_did_not_open_is_not_begun_later() {
    let (earlier, later) = fixture::compared_pdfs();
    let dir = std::env::temp_dir().join(format!("moonowl-unopened-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let broken = dir.join("broken.pdf");
    std::fs::write(&broken, "not a document").unwrap();
    let broken = broken.to_string_lossy().into_owned();
    let mut reader = Reader::empty(moonowl::harness::Options::default());
    reader.click(".start-compare");
    choose_sides(&mut reader, earlier, broken.clone());
    reader.click(".report-show");
    assert!(reader.wait_until(5.0, |reader| !reader.state().notice.is_empty()));
    std::fs::copy(&later, &broken).unwrap();
    reader.deliver(News {
        event: Event::OpenDocument(broken),
        target: None,
    });
    assert!(reader.wait_until(5.0, |reader| reader.state().pages == 2));
    reader.later(1.0);
    assert!(reader.harness.query(".report-window").is_none());
    assert!(reader.harness.query("[data-tab='changes']").is_none());
}

/// **A report can be saved from the start screen** with no comparison
/// shown: the two files are compared when it is saved, and the report is
/// suggested beside B.
#[test]
fn a_report_is_saved_from_the_start_screen() {
    let (earlier, later) = fixture::compared_pdfs();
    let mut reader = Reader::empty(moonowl::harness::Options::default());
    reader.click(".start-compare");
    choose_sides(&mut reader, earlier, later.clone());
    reader.click(".report-save");
    let (choosing, suggested) = reader.dialogs().pop().expect("a save dialog");
    assert_eq!(choosing, Choosing::SaveReport);
    let folder = std::path::Path::new(&later).parent().unwrap();
    assert!(suggested.is_some_and(|path| std::path::Path::new(&path).parent() == Some(folder)));
    let dir = std::env::temp_dir().join(format!("moonowl-start-report-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join("report.pdf").to_string_lossy().into_owned();
    reader.deliver(News {
        event: Event::Chosen(Choosing::SaveReport, vec![to.clone()]),
        target: None,
    });
    assert!(
        reader.wait_until(10.0, |reader| reader
            .state()
            .notice
            .starts_with("Saved the comparison as report.pdf.")),
        "told {:?}",
        reader.state().notice
    );
    let report = moonowl::render::open(&to).unwrap();
    assert_eq!(report.outline().len(), 3);
    report.release();
    drop(reader);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **The report window's two files are as wide as each other** however
/// long their names are: a long name is cut short.
#[test]
fn the_report_windows_files_are_as_wide_as_each_other() {
    let (earlier, later) = fixture::compared_pdfs();
    let dir = std::env::temp_dir().join(format!("moonowl-long-name-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let long = dir.join("A much longer name for the earlier version of this document.pdf");
    std::fs::copy(&earlier, &long).unwrap();
    let mut reader = comparing_with(&later, long.to_string_lossy().into_owned());
    reader.click(".changes-report");
    let widths: Vec<f32> = (reader.harness.query_all(".report-side").into_iter())
        .map(|node| reader.harness.layout_rect_of(node).width)
        .collect();
    assert_eq!(widths.len(), 2);
    // Within the pixel that rounding the layout can give one of them.
    assert!((widths[0] - widths[1]).abs() <= 1.0, "{widths:?}");
    drop(reader);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **A click on a file in the report window chooses another in its
/// place**, and the report then compares the file chosen, which nothing
/// had compared before. The file that is already the other side is
/// refused.
#[test]
fn the_report_window_chooses_another_file() {
    let (third, _) = fixture::line_pdfs(
        "report-side",
        "The quick green fox jumps over the lazy dog.",
        "Unused.",
    );
    let (_, later) = fixture::compared_pdfs();
    let mut reader = comparing();
    reader.click(".changes-report");
    reader.click("[data-side='A']");
    assert_eq!(
        reader.dialogs().pop(),
        Some((Choosing::ReportFile(0), None))
    );
    reader.deliver(News {
        event: Event::Chosen(Choosing::ReportFile(0), vec![later]),
        target: None,
    });
    assert_eq!(
        reader.state().notice,
        "That file is already B. Choose a different one."
    );
    reader.deliver(News {
        event: Event::Chosen(Choosing::ReportFile(0), vec![third]),
        target: None,
    });
    assert_eq!(
        reader.text_all(".report-file"),
        [
            "moonowl-fixture-report-side-earlier.pdf",
            "moonowl-fixture-later.pdf"
        ]
    );
    let dir = std::env::temp_dir().join(format!("moonowl-report-side-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join("report.pdf").to_string_lossy().into_owned();
    reader.click(".report-save");
    reader.deliver(News {
        event: Event::Chosen(Choosing::SaveReport, vec![to.clone()]),
        target: None,
    });
    assert!(
        reader.wait_until(10.0, |reader| reader
            .state()
            .notice
            .starts_with("Saved the comparison as report.pdf.")),
        "told {:?}",
        reader.state().notice
    );
    let report = moonowl::render::open(&to).unwrap();
    assert_eq!(
        report.title(),
        "Comparison of moonowl-fixture-report-side-earlier.pdf and moonowl-fixture-later.pdf"
    );
    let text: String = (0..report.pages())
        .flat_map(|page| report.text_of(page).chars)
        .collect();
    assert!(text.contains("green"), "{text:?}");
    report.release();
    drop(reader);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A report is not written from a draft of the other file that came after
/// the comparison, since its changes would be marked on the wrong words.
#[test]
fn a_report_is_not_saved_once_the_other_file_changed() {
    let (earlier, later) = fixture::compared_pdfs();
    let dir = std::env::temp_dir().join(format!("moonowl-report-stale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let other = dir.join("earlier.pdf");
    std::fs::copy(&earlier, &other).unwrap();
    let mut reader = comparing_with(&later, other.to_string_lossy().into_owned());
    fixture::draft(&other, 2);
    reader.click(".changes-report");
    reader.click(".report-save");
    let to = dir.join("report.pdf").to_string_lossy().into_owned();
    reader.deliver(News {
        event: Event::Chosen(Choosing::SaveReport, vec![to.clone()]),
        target: None,
    });
    assert!(
        reader.wait_until(10.0, |reader| !reader.state().notice.is_empty()
            && !reader.state().notice.contains("differences")),
        "told {:?}",
        reader.state().notice
    );
    assert_eq!(
        reader.state().notice,
        "earlier.pdf changed on disk after it was compared. Compare again, then save the report."
    );
    assert!(!std::path::Path::new(&to).exists());
    drop(reader);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A report is not saved over the file the document is compared with. That
/// file is open in no window, so only the comparison knows about it.
#[test]
fn a_report_is_not_saved_over_the_other_file() {
    let (earlier, later) = fixture::compared_pdfs();
    let dir = std::env::temp_dir().join(format!("moonowl-report-over-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let other = dir.join("earlier.pdf");
    std::fs::copy(&earlier, &other).unwrap();
    let before = std::fs::read(&other).unwrap();
    let mut reader = comparing_with(&later, other.to_string_lossy().into_owned());
    reader.click(".changes-report");
    reader.click(".report-save");
    reader.deliver(News {
        event: Event::Chosen(
            Choosing::SaveReport,
            vec![other.to_string_lossy().into_owned()],
        ),
        target: None,
    });
    reader.settle();
    assert_eq!(
        reader.state().notice,
        "Choose a new name. The document is being compared with that file, so it cannot be \
         saved over."
    );
    assert_eq!(std::fs::read(&other).unwrap(), before);
    drop(reader);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A report is not written once the document in the window changed on
/// disk, even before the window has reloaded it.
#[test]
fn a_report_is_not_saved_once_the_document_changed() {
    let (earlier, later) = fixture::compared_pdfs();
    let dir = std::env::temp_dir().join(format!("moonowl-report-stale-own-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let document = dir.join("later.pdf");
    std::fs::copy(&later, &document).unwrap();
    let config = dir.join("config");
    let options = moonowl::harness::Options {
        config: config.clone(),
        ..Default::default()
    };
    let mut reader = comparing_in(&document.to_string_lossy(), earlier.clone(), options);
    fixture::draft(&document, 2);
    reader.click(".changes-report");
    // A colour chosen for a report that is refused is not kept.
    reader.click_nth(".colours-window .color-hex", 0);
    reader.press("End");
    for _ in 0..7 {
        reader.press("Backspace");
    }
    reader.type_text("#abcdef");
    reader.press("Enter");
    reader.click(".report-save");
    let to = dir.join("report.pdf").to_string_lossy().into_owned();
    reader.deliver(News {
        event: Event::Chosen(Choosing::SaveReport, vec![to.clone()]),
        target: None,
    });
    assert!(
        reader.wait_until(10.0, |reader| !reader.state().notice.is_empty()
            && !reader.state().notice.contains("differences")),
        "told {:?}",
        reader.state().notice
    );
    assert_eq!(
        reader.state().notice,
        "later.pdf changed on disk after it was compared. Compare again, then save the report."
    );
    assert!(!std::path::Path::new(&to).exists());
    drop(reader);
    moonowl::store::flush();
    let settings = std::fs::read_to_string(config.join("settings.toml")).unwrap_or_default();
    assert!(!settings.contains("#abcdef"), "{settings}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// **"Save as a report…" writes the comparison as a PDF of its own**: what
/// each file says about itself, and every difference with its pages.
#[test]
fn the_comparison_is_saved_as_a_report() {
    let mut reader = comparing();
    reader.click(".changes-report");
    // The report window names the two files, the other one first.
    let names = reader.text_all(".report-file");
    assert_eq!(
        names,
        ["moonowl-fixture-earlier.pdf", "moonowl-fixture-later.pdf"]
    );
    reader.click(".report-save");
    let (asked, suggested) = reader.dialogs().pop().expect("a save dialog");
    assert!(suggested
        .unwrap_or_default()
        .ends_with("moonowl-fixture-later-compared-with-moonowl-fixture-earlier.pdf"));
    assert_eq!(asked, Choosing::SaveReport);
    let dir = std::env::temp_dir().join(format!("moonowl-report-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join("report.pdf").to_string_lossy().into_owned();
    reader.deliver(News {
        event: Event::Chosen(Choosing::SaveReport, vec![to.clone()]),
        target: None,
    });
    assert!(
        reader.wait_until(10.0, |reader| reader
            .state()
            .notice
            .starts_with("Saved the comparison as report.pdf.")),
        "told {:?}",
        reader.state().notice
    );
    let report = moonowl::render::open(&to).unwrap();
    assert_eq!(
        report.title(),
        "Comparison of moonowl-fixture-earlier.pdf and moonowl-fixture-later.pdf"
    );
    // The parts its first page lists are its outline, for the sidebar.
    let parts: Vec<(String, Option<usize>)> = (report.outline().into_iter())
        .map(|heading| (heading.title, heading.page))
        .collect();
    assert_eq!(
        parts,
        [
            ("Summary".to_string(), Some(1)),
            ("Pages side by side".to_string(), Some(2)),
            ("List of differences".to_string(), Some(4)),
        ]
    );
    let version = format!("Moonowl {}", env!("CARGO_PKG_VERSION"));
    assert!(report
        .details()
        .contains(&("Made with".to_string(), version.clone())));
    assert!(reader
        .asks()
        .contains(&moonowl::app::Ask::SendOn(to.clone())));
    // The first page, a page side by side for each pair of pages where the
    // two differ, and the list of differences.
    assert_eq!(report.pages(), 4);
    let page = |index: usize| {
        let text: String = report.text_of(index).chars.into_iter().collect();
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    for (index, said) in [
        (0, "Comparison report".to_string()),
        (
            0,
            "A No title moonowl-fixture-earlier.pdf B No title moonowl-fixture-later.pdf".into(),
        ),
        (0, format!("Made by {version} on")),
        (0, "https://github.com/lhdjung/moonowl".into()),
        (
            0,
            "3 differences in total \u{2022} 3 in the wording, from a single letter to whole \
             sentences"
                .into(),
        ),
        (
            0,
            "Contents Summary p. 1 Pages side by side p. 2 List of differences p. 4".into(),
        ),
        (0, "Only in A What only A has".into()),
        (0, "Not compared Running heads".into()),
        (0, "Shorthand A B".into()),
        (0, "Pages 2 2".into()),
        (
            3,
            "List of differences Each difference begins with the page".into(),
        ),
        (3, "1. A 1, B 1 | p. 2 A: brown B: red".into()),
        (3, "3. A 2, B 2 | p. 3 B: And one more line.".into()),
        (1, "A, page 1".into()),
        (1, "B, page 1".into()),
        (1, "The quick brown fox".into()),
        (2, "A, page 2".into()),
    ] {
        let text = page(index);
        assert!(
            text.contains(&said),
            "{said:?} is not on page {index}: {text:?}"
        );
    }
    // Nothing moved, so the key does not say what a moved passage is.
    assert!(!page(0).contains("Moved"));
    // The address is a link, and each difference leads to its page.
    use moonowl::render::Target;
    let targets = |index: usize| -> Vec<Target> {
        (report.links_of(index).into_iter())
            .map(|link| link.target)
            .collect()
    };
    // The contents lead to each part, from its name and from its page
    // number.
    let mut first = vec![Target::Away("https://github.com/lhdjung/moonowl".into())];
    first.extend([1, 1, 2, 2, 4, 4].map(|page| Target::Place { page, spot: None }));
    assert_eq!(targets(0), first);
    // In the list, A and B lead to the difference's mark on that file's
    // page, and the page number to the page that shows both.
    let led: Vec<(usize, bool)> = (targets(3).into_iter())
        .map(|target| match target {
            Target::Place { page, spot } => (page, spot.is_some()),
            Target::Away(_) => (0, false),
        })
        .collect();
    assert_eq!(
        led,
        [
            (2, true),
            (2, true),
            (2, false),
            (2, true),
            (2, true),
            (2, false),
            (3, true),
            (3, true),
            (3, false),
        ]
    );
    // Each difference's number stands beside it on the pages side by side:
    // in the margin left of A's page, and right of B's.
    let in_margin = |index: usize, number: char, right: bool| {
        let text = report.text_of(index);
        let width = report.size_of(index).width;
        (0..text.chars.len()).any(|at| {
            let glyph = text.glyph(at);
            text.chars[at] == number
                && match right {
                    false => glyph.left + glyph.width < 28.0,
                    true => glyph.left > width - 28.0,
                }
        })
    };
    assert!(in_margin(1, '1', false) && in_margin(1, '2', true));
    assert!(in_margin(2, '3', false) && in_margin(2, '3', true));
    report.release();
    // The pages shown share their fonts, and the report has each one once.
    let file = lopdf::Document::load(&to).expect("lopdf reads the report");
    let mut fonts: Vec<(Vec<u8>, Vec<u8>)> = (file.objects.values())
        .filter_map(|object| object.as_dict().ok())
        .filter(|dictionary| dictionary.has_type(b"Font"))
        .map(|dictionary| {
            let name = |key: &[u8]| {
                (dictionary.get(key).and_then(|name| name.as_name()))
                    .map_or(Vec::new(), <[u8]>::to_vec)
            };
            (name(b"Subtype"), name(b"BaseFont"))
        })
        .collect();
    let all = fonts.len();
    fonts.sort();
    fonts.dedup();
    assert!(all > 1, "the report has fonts: {fonts:?}");
    assert_eq!(fonts.len(), all, "each font once");
}

/// A passage that moved says so in the Differences tab, and in the report,
/// where each half names the other.
#[test]
fn a_moved_passage_is_shown_as_moved() {
    let (earlier, later) = fixture::moved_pdfs();
    let mut reader = Reader::open(&later);
    compare_with(&mut reader, earlier.clone());
    assert!(
        reader.wait_until(5.0, |reader| reader
            .harness
            .query("[data-tab='changes']")
            .is_some()),
        "told {:?}",
        reader.state().notice
    );
    assert_eq!(
        reader.text_all(".change-row .result-line"),
        [
            "A new opening page.",
            "Moved: Moved words here.",
            "Moved: Moved words here."
        ]
    );
    // Both halves are on the last page here: the words, and a bar where
    // they were.
    reader.click("[data-change='2']");
    assert_eq!(reader.harness.query_all(".change.moved").len(), 2);
    assert_eq!(reader.harness.query_all(".change.moved.bar").len(), 1);

    let (a, b) = (
        moonowl::render::open(&earlier).unwrap(),
        moonowl::render::open(&later).unwrap(),
    );
    let changes = moonowl::compare::compare(&*b, &*a).unwrap();
    let file = moonowl::report::File::of;
    let colours = moonowl::report::COLOURS;
    let (bytes, unfinished) =
        moonowl::report::write(&file(&*a), &file(&*b), &changes, &colours).unwrap();
    assert_eq!(unfinished, moonowl::report::Lacking::default());
    let dir = std::env::temp_dir().join(format!("moonowl-moved-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join("report.pdf");
    std::fs::write(&to, bytes).unwrap();
    let report = moonowl::render::open(&to.to_string_lossy()).unwrap();
    let text: String = (0..report.pages())
        .flat_map(|page| report.text_of(page).chars)
        .collect();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    for said in [
        "3 differences in total \u{2022} 1 in the wording, from a single letter to whole \
         sentences \u{2022} 2 differences as a result of moving passages (1 passage moved)",
        "Moved A passage of three words or more",
        "A: Moved words here. These words of A are in B at difference 3.",
        "B: Moved words here. These words of B are in A at difference 2.",
    ] {
        assert!(text.contains(said), "{said:?} is not in {text:?}");
    }
    report.release();
}

/// **A report is not written from a file that changed after it was
/// compared**, which is asked again once its pages have been read.
#[test]
fn a_report_reads_no_file_that_changed_since_it_was_compared() {
    let (earlier, later) = fixture::moved_pdfs();
    let (a, b) = (
        moonowl::render::open(&earlier).unwrap(),
        moonowl::render::open(&later).unwrap(),
    );
    let changes = moonowl::compare::compare(&*b, &*a).unwrap();
    let file = moonowl::report::File::of;
    let mut changed = file(&*b);
    changed.stamp = None;
    let colours = moonowl::report::COLOURS;
    let said = moonowl::report::write(&file(&*a), &changed, &changes, &colours)
        .expect_err("a report from a file that changed");
    assert!(said.contains("changed on disk"), "{said}");
}

/// **A document opened with its password is put into a report**, read
/// again with the same password.
#[test]
fn a_report_reads_a_locked_document_with_its_password() {
    let (earlier, _) = fixture::moved_pdfs();
    let (a, b) = (
        moonowl::render::open(&earlier).unwrap(),
        moonowl::render::open_with(&fixture::locked_pdf(), Some(fixture::LOCKED_PASSWORD)).unwrap(),
    );
    let changes = moonowl::compare::compare(&*b, &*a).unwrap();
    assert!(!changes.is_empty());
    let file = moonowl::report::File::of;
    let colours = moonowl::report::COLOURS;
    moonowl::report::write(&file(&*a), &file(&*b), &changes, &colours)
        .expect("a report with the locked document's pages");
}

/// A report of a document opened with its password is saved without one,
/// and the reader is told so.
#[test]
fn a_report_of_a_locked_document_says_it_has_no_password() {
    let (earlier, _) = fixture::moved_pdfs();
    let locked = fixture::locked_pdf();
    let mut reader = Reader::locked(&locked);
    reader.type_text(moonowl::fixture::LOCKED_PASSWORD);
    reader.press("Enter");
    compare_with(&mut reader, earlier);
    assert!(
        reader.wait_until(5.0, |reader| reader
            .harness
            .query("[data-tab='changes']")
            .is_some()),
        "told {:?}",
        reader.state().notice
    );
    let dir = std::env::temp_dir().join(format!("moonowl-locked-report-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join("report.pdf").to_string_lossy().into_owned();
    reader.click(".changes-report");
    reader.click(".report-save");
    reader.deliver(News {
        event: Event::Chosen(Choosing::SaveReport, vec![to.clone()]),
        target: None,
    });
    let name = moonowl::store::file_name(&locked);
    let told = format!(
        "Saved the comparison as report.pdf. It has no password, though \u{201c}{name}\u{201d} has one."
    );
    assert!(
        reader.wait_until(10.0, |reader| reader.state().notice == told),
        "told {:?}",
        reader.state().notice
    );
    assert!(moonowl::render::open(&to).is_ok());
}

/// **A report that cannot be saved yet keeps the report window** and what
/// was chosen in it: here while the document's text is being saved.
#[test]
fn a_report_refused_while_busy_keeps_its_window() {
    let mut reader = comparing();
    let dir = std::env::temp_dir().join(format!("moonowl-refused-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join("report.pdf").to_string_lossy().into_owned();
    let text = dir.join("text.txt").to_string_lossy().into_owned();
    reader.click(".changes-report");
    reader.click(".report-swap");
    let names = reader.text_all(".report-file");
    reader.click(".report-save");
    reader.while_answering(|reader| {
        reader.deliver(News {
            event: Event::Chosen(Choosing::SaveText, vec![text]),
            target: None,
        });
        reader.deliver(News {
            event: Event::Chosen(Choosing::SaveReport, vec![to.clone()]),
            target: None,
        });
        assert_eq!(
            reader.state().notice,
            "Moonowl is still busy with the last file. Try again in a moment."
        );
        assert_eq!(reader.text_all(".report-file"), names);
    });
    assert!(!std::path::Path::new(&to).exists());
}

/// **Every difference's number is beside its page in the report**, even
/// where there are more than fit a line apart.
#[test]
fn a_reports_numbers_stay_beside_the_page() {
    let (earlier, later) = fixture::dense_pdfs();
    let (a, b) = (
        moonowl::render::open(&earlier).unwrap(),
        moonowl::render::open(&later).unwrap(),
    );
    let changes = moonowl::compare::compare(&*b, &*a).unwrap();
    assert_eq!(changes.len(), 180);
    let file = moonowl::report::File::of;
    let colours = moonowl::report::COLOURS;
    let (bytes, unfinished) =
        moonowl::report::write(&file(&*a), &file(&*b), &changes, &colours).unwrap();
    assert_eq!(unfinished, moonowl::report::Lacking::default());
    let dir = std::env::temp_dir().join(format!("moonowl-dense-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join("report.pdf");
    std::fs::write(&to, bytes).unwrap();
    let report = moonowl::render::open(&to.to_string_lossy()).unwrap();
    let mut numbers = 0;
    for index in 0..report.pages() {
        let height = report.size_of(index).height as f32;
        let text = report.text_of(index);
        for (c, cell) in text.chars.iter().zip(&text.boxes) {
            assert!(
                cell.top >= 0.0 && cell.top + cell.height <= height,
                "{c:?} at {cell:?}"
            );
            numbers += usize::from(c.is_ascii_digit());
        }
    }
    assert!(numbers > 0);
    report.release();
}

/// "Swap A and B" in the report window makes the document A and the other
/// file B, and the report names them and their words that way round.
#[test]
fn the_report_can_name_the_files_the_other_way_round() {
    let mut reader = comparing();
    reader.click(".changes-report");
    reader.click(".report-swap");
    let names = reader.text_all(".report-file");
    assert_eq!(
        names,
        ["moonowl-fixture-later.pdf", "moonowl-fixture-earlier.pdf"]
    );
    reader.click(".report-save");
    let (_, suggested) = reader.dialogs().pop().expect("a save dialog");
    assert!(suggested
        .unwrap_or_default()
        .ends_with("moonowl-fixture-earlier-compared-with-moonowl-fixture-later.pdf"));
    let dir = std::env::temp_dir().join(format!("moonowl-swapped-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join("report.pdf").to_string_lossy().into_owned();
    reader.deliver(News {
        event: Event::Chosen(Choosing::SaveReport, vec![to.clone()]),
        target: None,
    });
    assert!(
        reader.wait_until(10.0, |reader| reader
            .state()
            .notice
            .starts_with("Saved the comparison as report.pdf.")),
        "told {:?}",
        reader.state().notice
    );
    assert!(reader.harness.query(".report-window").is_none());
    let report = moonowl::render::open(&to).unwrap();
    assert_eq!(
        report.title(),
        "Comparison of moonowl-fixture-later.pdf and moonowl-fixture-earlier.pdf"
    );
    let text: String = (0..report.pages())
        .flat_map(|page| report.text_of(page).chars)
        .collect();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    for said in [
        "A No title moonowl-fixture-later.pdf B No title moonowl-fixture-earlier.pdf",
        "3 differences in total",
        "1. A 1, B 1 | p. 2 A: red B: brown",
    ] {
        assert!(text.contains(said), "{said:?} is not in {text:?}");
    }
    report.release();
}

/// **A passage that moved with a word changed inside it is one passage**
/// in the report's summary, though it moves as two runs.
#[test]
fn a_moved_passage_with_a_word_changed_is_one_passage() {
    const BEFORE: &str = "This passage has enough words in it to be one that moved, and it";
    const AFTER: &str = "moves to the end of the other version with one word put in its place.";
    const STAYS: [&str; 4] = [
        "The first paragraph stays where it is, and it has a good many words in it,",
        "more than the passage that moves, so that the passage is what moved and",
        "the rest is what stayed. The second page has text that stays put as well,",
        "and it goes on for a line or two more, so that there is plenty of it here.",
    ];
    let changed = AFTER.replace("with one word", "with: a word");
    let (earlier, later) = fixture::texts_pdfs(
        "moved-changed",
        &[
            (1, 700, BEFORE),
            (1, 680, AFTER),
            (1, 600, STAYS[0]),
            (1, 580, STAYS[1]),
            (2, 600, STAYS[2]),
            (2, 580, STAYS[3]),
        ],
        &[
            (1, 600, STAYS[0]),
            (1, 580, STAYS[1]),
            (2, 600, STAYS[2]),
            (2, 580, STAYS[3]),
            (2, 500, BEFORE),
            (2, 480, &changed),
        ],
    );
    let (a, b) = (
        moonowl::render::open(&earlier).unwrap(),
        moonowl::render::open(&later).unwrap(),
    );
    let changes = moonowl::compare::compare(&*b, &*a).unwrap();
    let passages: std::collections::HashSet<_> =
        changes.iter().filter_map(|change| change.passage).collect();
    assert_eq!(passages.len(), 1, "{changes:#?}");
    assert!(
        changes
            .iter()
            .filter(|change| change.moved.is_some())
            .count()
            > 2,
        "it moves as more than one run: {changes:#?}"
    );
    // No word is marked both as moved and as changed.
    let marked = |moved: bool| -> Vec<(usize, usize, usize)> {
        (changes.iter())
            .filter(|change| change.moved.is_some() == moved)
            .flat_map(|change| change.places.iter().chain(&change.places_there).copied())
            .filter(|(_, from, to)| from < to)
            .collect()
    };
    for (page, from, to) in marked(false) {
        assert!(
            marked(true)
                .iter()
                .all(|&(on, a, b)| on != page || b <= from || to <= a),
            "{changes:#?}"
        );
    }
}

/// **A short phrase moved only when that is the one answer.** "for the
/// most part" taken out in two places and put in in a third has not moved,
/// and neither has one taken out once and put in in two places.
#[test]
fn a_common_phrase_moved_more_than_one_way_has_not_moved() {
    let compared = |name: &str, earlier: &[(usize, u32, &str)], later: &[(usize, u32, &str)]| {
        let (earlier, later) = fixture::texts_pdfs(name, earlier, later);
        let (a, b) = (
            moonowl::render::open(&earlier).unwrap(),
            moonowl::render::open(&later).unwrap(),
        );
        moonowl::compare::compare(&*b, &*a).unwrap()
    };
    let changes = compared(
        "phrase-twice",
        &[
            (1, 600, "It was for the most part fine."),
            (1, 560, "We were for the most part happy."),
            (1, 520, "The last line has nothing like that in it."),
        ],
        &[
            (1, 600, "It was fine."),
            (1, 560, "We were happy."),
            (
                1,
                520,
                "The last line has nothing for the most part like that in it.",
            ),
        ],
    );
    assert_eq!(changes.len(), 3, "{changes:#?}");
    assert!(
        changes.iter().all(|change| change.moved.is_none()),
        "{changes:#?}"
    );
    let changes = compared(
        "phrase-once",
        &[
            (1, 600, "It was for the most part fine."),
            (1, 560, "We were happy."),
            (1, 520, "The last line has nothing like that in it."),
        ],
        &[
            (1, 600, "It was fine."),
            (1, 560, "We were for the most part happy."),
            (
                1,
                520,
                "The last line has nothing for the most part like that in it.",
            ),
        ],
    );
    assert_eq!(changes.len(), 3, "{changes:#?}");
    assert!(
        changes.iter().all(|change| change.moved.is_none()),
        "{changes:#?}"
    );
}

/// **A comparison says that it is under way, and the Document menu stops
/// it.** Nothing is shown once it has been stopped.
#[test]
fn a_comparison_says_so_and_can_be_stopped() {
    let (earlier, later) = fixture::compared_pdfs();
    let mut reader = Reader::open(&later);
    reader.while_answering(|reader| {
        compare_with(reader, earlier);
        assert_eq!(
            reader.state().notice,
            "Comparing with moonowl-fixture-earlier.pdf\u{2026}"
        );
        reader.click(".chip.title");
        assert_eq!(
            reader.text_all("[data-item='compare'] .menu-label"),
            ["Stop comparing"]
        );
        reader.click("[data-item='compare']");
        assert_eq!(reader.state().notice, "Stopping\u{2026}");
    });
    assert_eq!(
        reader.state().notice,
        "Comparing was stopped, so no differences are shown."
    );
    assert!(reader.harness.query("[data-tab='changes']").is_none());
}

/// **A report stopped while it is saved is not kept**, though the save
/// itself has no way to stop.
#[test]
fn a_report_stopped_while_saving_is_not_kept() {
    let mut reader = comparing();
    reader.click(".changes-report");
    reader.click(".report-save");
    reader.dialogs().pop().expect("a save dialog");
    let dir = std::env::temp_dir().join(format!("moonowl-report-stop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join("report.pdf");
    reader.while_answering(|reader| {
        reader.deliver(News {
            event: Event::Chosen(
                Choosing::SaveReport,
                vec![to.to_string_lossy().into_owned()],
            ),
            target: None,
        });
        reader.click(".chip.title");
        reader.click("[data-item='compare']");
    });
    assert_eq!(
        reader.state().notice,
        "Saving the comparison was stopped, so no report was saved."
    );
    assert!(reader
        .asks()
        .iter()
        .all(|ask| !matches!(ask, moonowl::app::Ask::SendOn(_))));
    assert!(reader.wait_until(5.0, |_| !to.exists()));
}

/// Comparing itself stops when it is asked to, rather than only having
/// its answer thrown away.
#[test]
fn comparing_stops_when_asked() {
    let (earlier, later) = fixture::compared_pdfs();
    let here = moonowl::render::open(&later).expect("opens");
    let there = moonowl::render::open(&earlier).expect("opens");
    let stop = std::sync::atomic::AtomicBool::new(true);
    assert!(moonowl::compare::compare_until(&*here, &*there, &stop).is_err());
}

/// **A highlight does not wait for a comparison**, which reads the document
/// from the disk, and the comparison is shown after it.
#[test]
fn a_highlight_does_not_wait_for_a_comparison() {
    let (earlier, later) = fixture::compared_pdfs();
    let dir = std::env::temp_dir().join(format!("moonowl-compare-marked-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory to write in");
    let path = dir.join("later.pdf");
    std::fs::copy(&later, &path).expect("a copy of the fixture");
    let path = path.to_string_lossy().into_owned();
    let mut reader = Reader::open(&path);
    reader.while_answering(|reader| {
        compare_with(reader, earlier);
        reader.sweep_page(1, (0.15, 0.237), (0.5, 0.237));
        reader.click(".markup-swatch");
        // Asked before the comparison is shown, which says something else.
        let marks = moonowl::render::open(&path).expect("reopens").markup();
        assert_eq!(marks.len(), 1, "told {:?}", reader.state().notice);
    });
    assert!(reader.harness.query("[data-tab='changes']").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}
