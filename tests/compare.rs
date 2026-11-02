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
        &*moonowl::render::open(&fixture::book_pdf()).unwrap(),
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
