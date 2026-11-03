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
    let (earlier, later) = fixture::respaced_pdfs();
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

/// The later version, compared with the earlier one as the menu and the
/// file dialog would.
fn comparing() -> Reader {
    let (earlier, later) = fixture::compared_pdfs();
    comparing_with(&later, earlier)
}

/// `document`, compared with `earlier` as the menu and the file dialog
/// would.
fn comparing_with(document: &str, earlier: String) -> Reader {
    let mut reader = Reader::open(document);
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
/// **Two documents can be compared from the start screen**: the second
/// opens, is compared with the first, and the report window opens with the
/// first as A.
#[test]
fn two_documents_are_compared_from_the_start_screen() {
    let (earlier, later) = fixture::compared_pdfs();
    let mut reader = Reader::empty(moonowl::harness::Options {
        chooses: vec![vec![earlier, later.clone()]],
        ..Default::default()
    });
    reader.click(".start-compare");
    assert_eq!(reader.dialogs()[0], (Choosing::CompareTwo, None));
    assert!(
        reader.wait_until(5.0, |reader| reader.harness.query(".report-file").is_some()),
        "told {:?}",
        reader.state().notice
    );
    assert_eq!(
        reader.text_all(".report-file"),
        ["moonowl-fixture-earlier.pdf", "moonowl-fixture-later.pdf"]
    );
}

/// **The report window's rows line up** however long the files' names
/// are: a long name takes only the room left after the colours.
#[test]
fn the_report_windows_rows_line_up() {
    let (earlier, later) = fixture::compared_pdfs();
    let dir = std::env::temp_dir().join(format!("moonowl-long-name-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let long = dir.join("A much longer name for the earlier version of this document.pdf");
    std::fs::copy(&earlier, &long).unwrap();
    let mut reader = comparing_with(&later, long.to_string_lossy().into_owned());
    reader.click(".changes-report");
    let lefts: Vec<f32> = (reader.harness.query_all(".report-file").into_iter())
        .map(|node| reader.harness.layout_rect_of(node).x)
        .collect();
    assert_eq!(lefts.len(), 2);
    assert_eq!(lefts[0], lefts[1]);
}

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
            ("Comparison report".to_string(), Some(1)),
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
        (0, "3 differences in words.".into()),
        (
            0,
            "Contents Pages side by side pp. 2\u{2013}3 List of differences p. 4".into(),
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
    // The contents lead to the pages side by side and to the list, from
    // the name of each part and from its page number.
    let mut first = vec![Target::Away("https://github.com/lhdjung/moonowl".into())];
    first.extend([2, 2, 4, 4].map(|page| Target::Place { page, spot: None }));
    assert_eq!(targets(0), first);
    assert_eq!(
        targets(3),
        [2, 2, 3].map(|page| Target::Place { page, spot: None })
    );
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
    reader.deliver(News {
        event: Event::Chosen(Choosing::Compare, vec![earlier.clone()]),
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
    let bytes =
        moonowl::report::write(&file(&*a), &file(&*b), &changes, &colours, &*a, &*b).unwrap();
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
        "3 differences in words. 1 passage moved.",
        "Moved A passage of three words or more",
        "A: Moved words here. These words of A are in B at difference 3.",
        "B: Moved words here. These words of B are in A at difference 2.",
    ] {
        assert!(text.contains(said), "{said:?} is not in {text:?}");
    }
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
        "3 differences in words.",
        "1. A 1, B 1 | p. 2 A: red B: brown",
    ] {
        assert!(text.contains(said), "{said:?} is not in {text:?}");
    }
    report.release();
}
