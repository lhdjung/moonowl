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

/// **Running heads and page numbers are not compared**, so a page added
/// at the front is one change and not one on every page after it.
#[test]
fn running_heads_are_left_out() {
    let (earlier, later) = fixture::moved_pdfs();
    let changes = moonowl::compare::compare(
        &*moonowl::render::open(&later).unwrap(),
        &*moonowl::render::open(&earlier).unwrap(),
    )
    .unwrap();
    let said: Vec<(&str, &str)> = changes
        .iter()
        .map(|change| (change.there.as_str(), change.here.as_str()))
        .collect();
    assert_eq!(
        said,
        [
            ("", "A new opening page."),
            ("Moved words here.", ""),
            ("", "Moved words here."),
        ]
    );
}

/// The later version, compared with the earlier one as the menu and the
/// file dialog would.
fn comparing() -> Reader {
    let (earlier, later) = fixture::compared_pdfs();
    let mut reader = Reader::open(&later);
    reader.click(".chip.title");
    reader.click("[data-item='compare']");
    assert!(reader
        .dialogs()
        .iter()
        .any(|(choosing, _)| *choosing == Choosing::Compare));
    reader.deliver(News {
        event: Event::Chosen(Choosing::Compare, vec![earlier]),
        target: None,
    });
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
    reader.deliver(News {
        event: Event::Chosen(Choosing::Compare, vec![earlier]),
        target: None,
    });
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

#[test]
fn a_document_is_not_compared_with_itself() {
    let (_, later) = fixture::compared_pdfs();
    let mut reader = Reader::open(&later);
    reader.deliver(News {
        event: Event::Chosen(Choosing::Compare, vec![later]),
        target: None,
    });
    assert_eq!(
        reader.state().notice,
        "That is this document. Choose another version of it."
    );
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
        reader.deliver(News {
            event: Event::Chosen(Choosing::Compare, vec![earlier]),
            target: None,
        });
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
        reader.deliver(News {
            event: Event::Chosen(Choosing::Compare, vec![earlier]),
            target: None,
        });
        assert_eq!(
            reader.state().notice,
            "The document is being reloaded. Try again in a moment."
        );
    });
    assert!(reader.harness.query("[data-tab='changes']").is_none());
}

/// **"Save as a report…" writes the comparison as a PDF of its own**: what
/// each file says about itself, and every difference with its pages.
#[test]
fn the_comparison_is_saved_as_a_report() {
    let mut reader = comparing();
    reader.click(".changes-report");
    let (asked, _) = reader.dialogs().pop().expect("a save dialog");
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
    let version = format!("Moonowl {}", env!("CARGO_PKG_VERSION"));
    assert!(report
        .details()
        .contains(&("Made with".to_string(), version.clone())));
    assert!(reader
        .asks()
        .contains(&moonowl::app::Ask::SendOn(to.clone())));
    // The first page, the summary, and a page side by side for each pair of
    // pages where the two differ.
    assert_eq!(report.pages(), 4);
    let page = |index: usize| {
        let text: String = report.text_of(index).chars.into_iter().collect();
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    for (index, said) in [
        (0, "Comparison report".to_string()),
        (
            0,
            "A moonowl-fixture-earlier.pdf B moonowl-fixture-later.pdf".into(),
        ),
        (0, format!("Made by {version} on")),
        (0, "https://github.com/lhdjung/moonowl".into()),
        (
            0,
            "3 differences. 2 words are only in A, and 5 words are only in B.".into(),
        ),
        (0, "on page 2 lists each difference".into()),
        (0, "From page 3, each pair of pages".into()),
        (0, "Pages 2 2".into()),
        (1, "Summary of differences".into()),
        (
            1,
            "1. Page 1 of A, page 1 of B. Shown on page 3. A: brown B: red".into(),
        ),
        (
            1,
            "3. Page 2 of A, page 2 of B. Shown on page 4. B: And one more line.".into(),
        ),
        (2, "A, page 1".into()),
        (2, "B, page 1".into()),
        (2, "The quick brown fox".into()),
        (3, "A, page 2".into()),
    ] {
        let text = page(index);
        assert!(
            text.contains(&said),
            "{said:?} is not on page {index}: {text:?}"
        );
    }
    // The address is a link, and each difference leads to its page.
    use moonowl::render::Target;
    let targets = |index: usize| -> Vec<Target> {
        (report.links_of(index).into_iter())
            .map(|link| link.target)
            .collect()
    };
    assert_eq!(
        targets(0),
        [Target::Away("https://github.com/lhdjung/moonowl".into())]
    );
    assert_eq!(
        targets(1),
        [3, 3, 4].map(|page| Target::Place { page, spot: None })
    );
    report.release();
}
