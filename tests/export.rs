//! Taking things out of a document: its highlights as a file of their own,
//! a page as a picture, and its text.

use moonowl::app::{highlights_as, Choosing, HighlightFormat, MarkKey, MarkRow};
use moonowl::harness::{Options, Reader};
use moonowl::markup;
use moonowl::render;

/// A directory of this test's own, with a three-page draft in it.
fn draft_in(name: &str) -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("moonowl-export-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory to write in");
    let path = dir.join("paper.pdf");
    moonowl::fixture::draft(&path, 3);
    (dir, path.to_string_lossy().into_owned())
}

/// The first words of page 2, highlighted in the file.
fn highlight(path: &str) {
    let document = render::open(path).expect("the fixture opens");
    let text = document.text_of(1);
    let quads = text.quads(0, text.chars.len().min(12));
    drop(document);
    markup::add(path, &[(2, quads)], "#ffd60a", "Moonowl").expect("the highlight is written");
}

fn row(page: usize, quote: &str, note: &str, made: Option<&str>, key: MarkKey) -> MarkRow {
    MarkRow {
        page,
        color: "#ffd60a".into(),
        quote: quote.into(),
        note: note.into(),
        made: made.map(|made| chrono::DateTime::parse_from_rfc3339(made).unwrap()),
        key,
    }
}

#[test]
fn highlights_are_written_as_markdown_text_or_json() {
    let made = "2026-10-27T12:00:00+01:00";
    let rows = vec![
        (
            row(
                3,
                "A needle",
                "Look this up.",
                Some(made),
                MarkKey::InFile(3, 0),
            ),
            "iii".to_string(),
        ),
        (
            row(5, "in a haystack", "", None, MarkKey::Beside("x".into())),
            "5".to_string(),
        ),
    ];
    // The date in the reader's own time zone, which the test cannot fix.
    let day = chrono::DateTime::parse_from_rfc3339(made)
        .unwrap()
        .with_timezone(&chrono::Local)
        .format("%-d %b %Y")
        .to_string();
    assert_eq!(
        highlights_as(HighlightFormat::Markdown, "Paper", "paper.pdf", &rows),
        format!(
            "# Paper\n\n> A needle\n>\n> — p. iii · {day}\n\nLook this up.\n\n\
             > in a haystack\n>\n> — p. 5 · only in Moonowl\n"
        )
    );
    assert_eq!(
        highlights_as(HighlightFormat::Text, "Paper", "paper.pdf", &rows),
        format!(
            "Paper\n\n“A needle” — p. iii · {day}\nLook this up.\n\n\
             “in a haystack” — p. 5 · only in Moonowl\n"
        )
    );
    let json: serde_json::Value = serde_json::from_str(&highlights_as(
        HighlightFormat::Json,
        "Paper",
        "paper.pdf",
        &rows,
    ))
    .unwrap();
    assert_eq!(json["title"], "Paper");
    assert_eq!(json["file"], "paper.pdf");
    assert_eq!(
        json["highlights"][0],
        serde_json::json!({
            "page": 3, "label": "iii", "quote": "A needle", "comment": "Look this up.",
            "created": made, "color": "#ffd60a", "in_file": true,
        })
    );
    assert_eq!(json["highlights"][1]["created"], serde_json::Value::Null);
    assert_eq!(json["highlights"][1]["in_file"], false);
}

#[test]
fn markdown_shows_the_readers_words_as_they_are() {
    let rows = vec![(
        row(
            1,
            "x_y [a] <b>",
            "# Not a heading\n- not a list\n2. nor this",
            None,
            MarkKey::InFile(1, 0),
        ),
        "1".to_string(),
    )];
    assert_eq!(
        highlights_as(HighlightFormat::Markdown, "*Paper*", "paper.pdf", &rows),
        "# \\*Paper\\*\n\n> x\\_y \\[a\\] \\<b>\n>\n> — p. 1\n\n\
         \\# Not a heading\n\\- not a list\n2\\. nor this\n"
    );
    assert!(
        highlights_as(HighlightFormat::Text, "*Paper*", "paper.pdf", &rows)
            .contains("“x_y [a] <b>” — p. 1\n# Not a heading\n"),
        "plain text is left as it is"
    );
}

#[test]
fn the_document_menu_exports_the_highlights_in_the_format_pressed() {
    let (dir, path) = draft_in("menu");
    highlight(&path);
    let to = dir.join("notes.json").to_string_lossy().into_owned();
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![to.clone()]],
            ..Options::default()
        },
    );
    reader.click(".chip.title");
    reader.click("[data-item=export-json]");
    reader.settle();
    let (asked, suggested) = reader.dialogs().pop().expect("a save dialog");
    assert_eq!(asked, Choosing::ExportHighlights(HighlightFormat::Json));
    assert_eq!(
        suggested.as_deref(),
        Some(dir.join("paper (highlights).json").to_str().unwrap())
    );
    assert_eq!(reader.state().notice, "Exported 1 highlight to notes.json.");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&to).unwrap()).unwrap();
    let highlight = &json["highlights"][0];
    assert_eq!(highlight["page"], 2);
    assert_eq!(highlight["in_file"], true);
    assert!(!highlight["quote"].as_str().unwrap().is_empty());
    // The date the file gives the highlight, which was just now.
    let created = chrono::DateTime::parse_from_rfc3339(highlight["created"].as_str().unwrap())
        .expect("a date");
    let ago = chrono::Utc::now().signed_duration_since(created);
    assert!(ago.num_minutes().abs() < 10, "made {ago} ago");
}

#[test]
fn a_document_without_highlights_offers_no_export() {
    let (_dir, path) = draft_in("none");
    let mut reader = Reader::open(&path);
    reader.click(".chip.title");
    assert!(reader.harness.query_all(".menu-export").is_empty());
}
