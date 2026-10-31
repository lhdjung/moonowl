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
        .map(|change| {
            (
                change.page(),
                change.removed.as_str(),
                change.added.as_str(),
            )
        })
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

/// Only the beginning of a long change is kept, since only that is shown.
#[test]
fn a_long_change_is_cut_short() {
    let changes = moonowl::compare::compare(
        &*moonowl::render::open(&fixture::wordless_pdf()).unwrap(),
        &*moonowl::render::open(&fixture::offprint_pdf()).unwrap(),
    )
    .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0].removed.chars().count(),
        201,
        "{:?}",
        changes[0].removed
    );
    assert!(changes[0].removed.ends_with('\u{2026}'));
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

/// **A large comparison is bounded in time and in memory**: the four
/// hundred page book against the same book, and against a short document
/// with other words, which is the worst case for the diff.
#[test]
fn a_large_comparison_is_quick_and_small() {
    let book = moonowl::render::open(&Reader::book()).unwrap();
    let short = moonowl::render::open(&fixture::contents_pdf()).unwrap();
    let (before, _) = moonowl::stats::footprint_mb();
    let began = std::time::Instant::now();
    let same = moonowl::compare::compare(&*book, &*book).unwrap();
    let other = moonowl::compare::compare(&*book, &*short).unwrap();
    let took = began.elapsed();
    let (after, peak) = moonowl::stats::footprint_mb();
    eprintln!("compare: {took:?}, {before:.0}MB before, {after:.0}MB after, {peak:.0}MB peak");
    assert!(same.is_empty());
    assert!(!other.is_empty());
    assert!(took.as_secs() < 20, "took {took:?}");
    assert!(after - before < 50.0, "{before:.0}MB, then {after:.0}MB");
}
