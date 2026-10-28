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
            "x_y [a] <b> c|d ~e",
            "# Not a heading\n- not a list\n2. nor this",
            None,
            MarkKey::InFile(1, 0),
        ),
        "1".to_string(),
    )];
    assert_eq!(
        highlights_as(HighlightFormat::Markdown, "*Paper*", "paper.pdf", &rows),
        "# \\*Paper\\*\n\n> x\\_y \\[a\\] \\<b> c\\|d \\~e\n>\n> — p. 1\n\n\
         \\# Not a heading\\\n\\- not a list\\\n2\\. nor this\n"
    );
    assert!(
        highlights_as(HighlightFormat::Text, "*Paper*", "paper.pdf", &rows)
            .contains("“x_y [a] <b> c|d ~e” — p. 1\n# Not a heading\n"),
        "plain text is left as it is"
    );
    // A line that ends in a carriage return alone is a line all the same.
    let rows = vec![(
        row(1, "", "ok\r# x\r\n- y", None, MarkKey::InFile(1, 0)),
        "1".to_string(),
    )];
    let markdown = highlights_as(HighlightFormat::Markdown, "Paper", "paper.pdf", &rows);
    assert!(
        markdown.ends_with("\nok\\\n\\# x\\\n\\- y\n"),
        "{markdown:?}"
    );
    // Indented, a line would be code, where the escapes show.
    let rows = vec![(
        row(1, "", "first\n\n    x_y", None, MarkKey::InFile(1, 0)),
        "1".to_string(),
    )];
    let markdown = highlights_as(HighlightFormat::Markdown, "Paper", "paper.pdf", &rows);
    assert!(markdown.ends_with("\nfirst\n\nx\\_y\n"), "{markdown:?}");
    let text = highlights_as(HighlightFormat::Text, "Paper", "paper.pdf", &rows);
    assert!(!text.contains('\r'), "{text:?}");
}

#[test]
fn the_document_menu_exports_the_highlights_in_the_format_typed() {
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
    reader.click("[data-item=export-highlights]");
    reader.settle();
    let (asked, suggested) = reader.dialogs().pop().expect("a save dialog");
    assert_eq!(asked, Choosing::ExportHighlights);
    assert_eq!(
        suggested.as_deref(),
        Some(dir.join("paper (highlights).md").to_str().unwrap())
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
    assert!(reader
        .harness
        .query_all("[data-item=export-highlights]")
        .is_empty());
}

/// A page saved as a picture is the page at 300 dpi, in its own colours.
#[test]
fn a_page_is_saved_as_a_picture_from_the_document_menu() {
    let (dir, path) = draft_in("picture");
    let to = dir.join("page.png").to_string_lossy().into_owned();
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![to.clone()]],
            ..Options::default()
        },
    );
    reader.click(".chip.title");
    reader.click("[data-item=save-picture]");
    reader.settle();
    let (asked, suggested) = reader.dialogs().pop().expect("a save dialog");
    assert_eq!(asked, Choosing::SavePicture(1));
    assert_eq!(
        suggested.as_deref(),
        Some(dir.join("paper (page 1).png").to_str().unwrap())
    );
    assert_eq!(reader.state().notice, "Saved page 1 as page.png.");

    let size = render::open(&path).unwrap().size_of(0);
    let picture = image::open(&to).expect("a PNG").to_rgb8();
    assert_eq!(
        (picture.width(), picture.height()),
        (
            (size.width * 300.0 / 72.0).round() as u32,
            (size.height * 300.0 / 72.0).round() as u32
        )
    );
    assert_eq!(picture.get_pixel(0, 0).0, [255, 255, 255], "white paper");
    assert!(
        picture.pixels().any(|pixel| pixel.0[0] < 100),
        "and dark type on it"
    );
    // The strips line up: the picture is the page drawn whole.
    let whole = render::open(&path)
        .unwrap()
        .render_owned(
            0,
            picture.width(),
            picture.height(),
            moonowl::layout::View::WHOLE,
        )
        .unwrap();
    let differ = (whole.bgra.as_chunks::<4>().0.iter().zip(picture.pixels()))
        .filter(|(drawn, saved)| {
            (0..3).any(|channel| drawn[2 - channel].abs_diff(saved.0[channel]) > 8)
        })
        .count();
    assert!(
        differ * 1000 < whole.bgra.len() / 4,
        "{differ} pixels differ"
    );
}

#[test]
fn the_text_of_every_page_is_saved_with_a_blank_line_between_pages() {
    let (dir, path) = draft_in("text");
    let to = dir.join("paper.txt").to_string_lossy().into_owned();
    let mut reader = Reader::open_with(
        &path,
        Options {
            chooses: vec![vec![to.clone()]],
            ..Options::default()
        },
    );
    reader.click(".chip.title");
    reader.click("[data-item=save-text]");
    reader.settle();
    let (asked, _) = reader.dialogs().pop().expect("a save dialog");
    assert_eq!(asked, Choosing::SaveText);
    assert_eq!(reader.state().notice, "Saved the text as paper.txt.");
    let saved = std::fs::read_to_string(&to).unwrap();
    assert_eq!(
        saved,
        "Page 1 of the fixture.\n\nPage 2 of the fixture.\n\nPage 3 of the fixture.\n"
    );
}

#[test]
fn a_document_with_no_text_says_so_instead_of_saving_an_empty_file() {
    let (dir, _) = draft_in("scan");
    let picture = dir.join("scan.png").to_string_lossy().into_owned();
    image::RgbImage::from_pixel(60, 80, image::Rgb([200, 200, 200]))
        .save(&picture)
        .unwrap();
    let scan = dir.join("scan.pdf").to_string_lossy().into_owned();
    let picture = moonowl::arrange::Sheet {
        from: Some(picture),
        page: 1,
        turn: 0,
    };
    moonowl::arrange::assemble(None, &[picture], &scan).expect("a page with no text");
    let to = dir.join("scan.txt").to_string_lossy().into_owned();
    let mut reader = Reader::open_with(
        &scan,
        Options {
            chooses: vec![vec![to.clone()]],
            ..Options::default()
        },
    );
    reader.click(".chip.title");
    reader.click("[data-item=save-text]");
    reader.settle();
    assert!(reader
        .state()
        .notice
        .starts_with("This document has no text to save."));
    assert!(!std::path::Path::new(&to).exists());
}

/// A document put down while its text is read would answer every page
/// with nothing, so the text is refused rather than cut short.
#[test]
fn the_text_of_a_released_document_is_refused() {
    let (_dir, path) = draft_in("released");
    let document = render::open(&path).unwrap();
    assert!(moonowl::export::text(&*document).unwrap().contains('\n'));
    document.release();
    assert_eq!(
        moonowl::export::text(&*document),
        Err(moonowl::export::CLOSED_WHILE_READ.to_string())
    );
    // Let go of while its second page is read, and taken up again, as a
    // write that fails does.
    let document = render::open(&path).unwrap();
    assert!(document.pages() > 1);
    assert_eq!(
        moonowl::export::text(&LetGoOf(document)),
        Err(moonowl::export::CLOSED_WHILE_READ.to_string())
    );
}

/// A document let go of for the reading of its second page.
struct LetGoOf(std::sync::Arc<dyn render::PageSource>);

impl render::PageSource for LetGoOf {
    fn pages(&self) -> usize {
        self.0.pages()
    }

    fn size_of(&self, index: usize) -> moonowl::layout::Size {
        self.0.size_of(index)
    }

    fn render(
        &self,
        index: usize,
        width: u32,
        height: u32,
        view: moonowl::layout::View,
        take: &mut dyn FnMut(render::Bitmap),
    ) -> Result<(), String> {
        self.0.render(index, width, height, view, take)
    }

    fn opened_in(&self) -> f64 {
        0.0
    }

    fn text_aside(&self, index: usize) -> Option<render::PageText> {
        if index == 1 {
            self.0.release();
        }
        let text = self.0.text_aside(index);
        self.0.retake();
        text
    }
}

#[test]
fn the_name_typed_decides_the_format_of_the_highlights() {
    assert_eq!(HighlightFormat::of("notes.json"), HighlightFormat::Json);
    assert_eq!(HighlightFormat::of("notes.TXT"), HighlightFormat::Text);
    assert_eq!(HighlightFormat::of("notes.md"), HighlightFormat::Markdown);
    assert_eq!(HighlightFormat::of("notes"), HighlightFormat::Markdown);
}
