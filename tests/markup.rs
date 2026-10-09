//! Marking a passage, what lands in the file, and taking it out again.

use moonowl::markup;
use moonowl::render::{self, PageSource, Rect};

/// A copy of the plain fixture, in a directory of this test's own: everything
/// here writes to the document, and the fixtures are shared.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("moonowl-markup-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory to write in");
    let path = dir.join("marked.pdf");
    moonowl::fixture::draft(&path, 3);
    path
}

/// The first line of a page, as rectangles.
fn first_line(document: &std::sync::Arc<dyn PageSource>, page: usize) -> (Vec<Rect>, String) {
    let text = document.text_of(page - 1);
    let end = text.chars.len().min(12);
    (text.quads(0, end), text.chars[..end].iter().collect())
}

#[test]
fn a_marked_passage_is_a_highlight_in_the_file() {
    let path = scratch("written");
    let document = render::open(path.to_str().unwrap()).expect("the fixture opens");
    let (quads, words) = first_line(&document, 1);
    assert!(!quads.is_empty(), "the fixture has text on its first page");
    assert!(document.markup().is_empty(), "and no markup in it yet");
    drop(document);

    markup::add(
        path.to_str().unwrap(),
        &[(1, quads.clone())],
        "#ffd60a",
        "Moonowl",
    )
    .expect("the highlight is written");

    // Read back through a second open, which is the only thing that proves it
    // is in the *file* rather than in a list this process is holding.
    let again = render::open(path.to_str().unwrap()).expect("the document reopens");
    let marks = again.markup();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].page, 1);
    assert_eq!(marks[0].color, "#ffd60a");
    assert_eq!(marks[0].quads.len(), quads.len(), "one run per line");
    // The words under it, read off the page rather than out of the file — see
    // `markup::quote_under`, and the reason no quote is written.
    let quoted = markup::quote_under(&again.text_of(0), &marks[0].quads);
    assert_eq!(
        quoted,
        words.trim(),
        "the mark covers the words it was given"
    );
}

#[test]
fn the_mark_is_where_the_words_are() {
    // Not merely present: in the right place, which is the half a round trip
    // through the file cannot show on its own. pdfium counts from the bottom
    // of a page and everything in this crate counts from the top, so a flip
    // done once too often is a mark that lands mirrored down the page — and
    // it would still read back as one highlight with the right colour.
    let path = scratch("placed");
    let document = render::open(path.to_str().unwrap()).expect("the fixture opens");
    let (quads, _) = first_line(&document, 1);
    let wanted = quads[0];
    drop(document);

    markup::add(
        path.to_str().unwrap(),
        &[(1, quads.clone())],
        "#7bed9f",
        "Moonowl",
    )
    .expect("written");
    let again = render::open(path.to_str().unwrap()).expect("reopened");
    let landed = again.markup()[0].quads[0];
    assert!(
        (landed.left - wanted.left).abs() < 0.5
            && (landed.top - wanted.top).abs() < 0.5
            && (landed.width - wanted.width).abs() < 0.5
            && (landed.height - wanted.height).abs() < 0.5,
        "the run came back as {landed:?}, and it was drawn at {wanted:?}",
    );
}

#[test]
fn a_highlight_already_in_the_file_can_be_taken_out() {
    // **The thing the app cannot do.** `saveDocument()` in pdf.js writes an
    // incremental update and no markup subtype overrides `Annotation.save()`,
    // so an annotation already in the document cannot be edited or deleted
    // through it — the app answers with a pristine backup, a detached load, a
    // replay of everything else and a refusal path for markup it cannot
    // account for. Here it is one call, and this is the test that says so.
    let path = scratch("removed");
    let name = path.to_str().unwrap().to_string();
    let document = render::open(&name).expect("the fixture opens");
    let (first, _) = first_line(&document, 1);
    let (second, _) = first_line(&document, 2);
    drop(document);

    markup::add(&name, &[(1, first.clone())], "#ffd60a", "Moonowl").expect("the first is written");
    markup::add(&name, &[(2, second.clone())], "#74c0fc", "Moonowl")
        .expect("the second is written");
    let marks = render::open(&name).expect("reopened").markup();
    assert_eq!(marks.len(), 2);

    markup::remove(&name, 1, marks[0].index).expect("the first comes out");
    let left = render::open(&name).expect("reopened").markup();
    assert_eq!(left.len(), 1, "one of them went and the other stayed");
    assert_eq!(left[0].page, 2);
    assert_eq!(left[0].color, "#74c0fc");

    markup::remove(&name, 2, left[0].index).expect("and so does the other");
    assert!(render::open(&name).expect("reopened").markup().is_empty());
}

#[test]
fn nothing_is_left_beside_the_document() {
    // Writing a mark used to leave `.moonowl-original` in the reader's folder.
    // Their folder is theirs: after two writes it holds the document alone.
    let path = scratch("beside");
    let name = path.to_str().unwrap().to_string();
    let document = render::open(&name).expect("opens");
    let (quads, _) = first_line(&document, 1);
    drop(document);

    markup::add(&name, &[(1, quads.clone())], "#ffd60a", "Moonowl").expect("written");
    markup::add(&name, &[(2, quads)], "#ffd60a", "Moonowl").expect("written again");
    let left: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .expect("the folder")
        .map(|entry| entry.expect("an entry").file_name())
        .collect();
    assert_eq!(left, vec![std::ffi::OsString::from("marked.pdf")]);
}

#[test]
fn the_mark_is_drawn_on_the_page() {
    // pdfium generates the appearance stream for a markup annotation that has
    // none — `GenerateHighlightAP` in `cpdf_generateap.cpp` — so a highlight
    // created through `FPDFPage_CreateAnnot` and saved is one every other
    // reader draws too. Nothing in this crate asks it to: annotations are on
    // by default in `PdfRenderConfig`, which is the whole of why a mark is
    // pixels on the page here rather than a rectangle this reader lays over
    // it. That is the one place markup parts company with the search hits and
    // the selection beside it, and it is the right way round: a mark is in
    // the document, and the document is what pdfium draws.
    let path = scratch("drawn");
    let name = path.to_str().unwrap().to_string();
    let document = render::open(&name).expect("opens");
    let (quads, _) = first_line(&document, 1);
    let size = document.size_of(0);
    let (width, height) = (size.width.round() as u32, size.height.round() as u32);
    let view = moonowl::layout::View::WHOLE;
    let sample = |document: &std::sync::Arc<dyn PageSource>| {
        let mut pixel = [0u8; 3];
        let at = (
            (quads[0].left + quads[0].width / 2.0).round() as u32,
            (quads[0].top + quads[0].height / 2.0).round() as u32,
        );
        document
            .render(0, width, height, view, &mut |bitmap| {
                let start = ((at.1 * bitmap.width + at.0) * 4) as usize;
                pixel.copy_from_slice(&bitmap.bgra[start..start + 3]);
            })
            .expect("the page draws");
        pixel
    };
    let before = sample(&document);
    drop(document);

    markup::add(&name, &[(1, quads.clone())], "#ff0000", "Moonowl").expect("written");
    let after = sample(&render::open(&name).expect("reopened"));
    assert_ne!(before, after, "the page under the mark changed");
    // BGRA, as `Bitmap` says and as `render` now actually asks for: the mark
    // was `#ff0000`, so what comes back is the *last* of the three channels.
    // Finding out that it was the first is what turned up the byte order the
    // whole reader had been drawing with — see `pdfium.rs`.
    assert!(
        after[2] > 200 && after[1] < 60 && after[0] < 60,
        "the pixel came back as {after:?}, which is not the colour it was marked in",
    );

    // And another colour, which is drawn too: a `/C` changed under an
    // appearance stream is a file that says green and a page that shows red.
    let was = render::open(&name).expect("reopened").markup()[0].clone();
    // Green, because a blue black type reads on is no longer pure blue.
    markup::recolour(&name, 1, was.index, "#00ff00").expect("recoloured");
    let again = render::open(&name).expect("reopened");
    let marks = again.markup();
    assert_eq!(
        marks.len(),
        1,
        "the old mark came out as the new one went in"
    );
    assert_eq!(marks[0].color, "#00ff00");
    assert_eq!(marks[0].quads, was.quads, "over the same words");
    let green = sample(&again);
    assert!(
        green[1] > 200 && green[0] < 60 && green[2] < 60,
        "the pixel came back as {green:?} after the mark was made green",
    );
}

#[test]
fn a_dark_mark_goes_in_light_enough_to_read_through() {
    // Every reader multiplies a highlight into the page, so a black one is
    // black words on black in Preview too. What goes in is lifted off the
    // ink instead, and a colour that already reads is left as it was.
    let path = scratch("dark");
    let name = path.to_str().unwrap().to_string();
    let (quads, _) = first_line(&render::open(&name).expect("opens"), 1);
    markup::add(&name, &[(1, quads)], "#000000", "Moonowl").expect("written");
    let written = render::open(&name).expect("reopened").markup()[0]
        .color
        .clone();
    let rgb = moonowl::palette::read_colour(&written).expect("a colour");
    assert!(
        moonowl::palette::contrast_ratio(rgb, [0; 3]) >= 4.5,
        "a black mark went in as {written}, which black type does not read on",
    );
    assert_eq!(moonowl::palette::offered("#ff6b6b"), "#ff6b6b");
}

/* ------------------------------------------------------------ the gesture */

use moonowl::harness::{Options, Reader};

/// A copy of the prose fixture — one line of type near the top of each of six
/// pages — in a directory of this test's own, because every test below writes
/// to the document it opens.
fn readable(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("moonowl-marked-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory to write in");
    let path = dir.join("prose.pdf");
    std::fs::copy(moonowl::fixture::prose_pdf(), &path).expect("a copy of the fixture");
    path.to_string_lossy().into_owned()
}

/// Where the one line of a `prose_pdf` page sits inside its box. `select.rs`
/// works the same number out the same way.
const LINE: f32 = 0.108;

fn open(path: &str) -> Reader {
    Reader::open_with(path, Options::default())
}

#[test]
fn the_swatches_have_a_way_out_that_keeps_the_selection() {
    let mut reader = open(&readable("closed"));
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    assert!(reader.harness.query(".markup-popover").is_some());
    reader.click(".markup-close");
    assert!(
        reader.harness.query(".markup-popover").is_none(),
        "the × puts the swatches away"
    );
    assert!(
        !reader.harness.query_all(".selected").is_empty(),
        "and leaves the passage selected"
    );
}

#[test]
fn the_swatches_offer_to_copy_the_passage() {
    let mut reader = open(&readable("copied"));
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-copy");
    assert_eq!(reader.copied(), vec!["A needle in the first page."]);
    assert_eq!(reader.state().notice, "Copied.");
}

#[test]
fn the_swatches_wait_to_be_asked_for_when_the_setting_says_so() {
    let options = Options {
        settings: vec![("offer_highlight_on_select".into(), serde_json::json!(false))],
        ..Options::default()
    };
    let mut reader = Reader::open_with(&readable("unoffered"), options);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    assert!(reader.harness.query(".markup-popover").is_none());
    assert!(!reader.harness.query_all(".selected").is_empty());
}

/// **The picker's knob goes where the pointer goes**, short of the dark that
/// black type cannot be read on. Across the foot of the square the colour
/// keeps the saturation the pointer asks for: a colour too dark is made
/// brighter, straight up the square, not paler. Paler, the foot of the
/// square was a few per cent saturated all the way across, and the knob
/// stayed at the left edge wherever the pointer went.
#[test]
fn the_picker_follows_the_pointer_into_the_dark() {
    fn saturation(hex: &str) -> f64 {
        let rgb = moonowl::palette::read_colour(hex).expect("a colour");
        let (top, bottom) = (*rgb.iter().max().unwrap(), *rgb.iter().min().unwrap());
        f64::from(top - bottom) / f64::from(top.max(1))
    }
    fn press(reader: &mut Reader, selector: &str, at: (f32, f32)) {
        let (x, y, width, height) = reader.box_of(selector).expect("on screen");
        reader.click_at(x + width * at.0, y + height * at.1);
    }
    let mut reader = open(&readable("picker"));
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-more");
    reader.click_nth(".colours-window .color-swatch", 0);
    // A red, whose full strength is far enough off black to leave room.
    press(&mut reader, ".colours-window .color-strip", (0.01, 0.5));
    for across in [0.2, 0.4, 0.6, 0.8] {
        press(&mut reader, ".colours-window .color-square", (across, 0.95));
        let hex = reader.field(".colours-window .color-hex");
        assert!(
            (saturation(&hex) - f64::from(across)).abs() < 0.06,
            "at {across} across the foot, {hex} is not where the pointer is"
        );
    }
}

/// **A palette is chosen, changed and kept as a theme is.** The shipped ones
/// are listed, choosing one offers its colours at once, and a shipped one
/// changed is saved as a copy that new marks are made in from then on —
/// leaving the shipped file as it was. The copy, being the reader's own, can
/// be deleted.
#[test]
fn a_palette_is_chosen_changed_and_kept() {
    let config = std::env::temp_dir().join(format!("moonowl-palettes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&config);
    let options = Options {
        config: config.clone(),
        ..Options::default()
    };
    let mut reader = Reader::open_with(&readable("palettes"), options);
    let first = |reader: &mut Reader| reader.harness.attr(".markup-swatch", "data-colour");
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-more");
    assert_eq!(
        reader.attribute_all(".palette-choice", "data-palette"),
        ["soft", "vivid", "muted"]
    );
    assert_eq!(
        reader
            .harness
            .attr(".palette-choice.on", "data-palette")
            .as_deref(),
        Some("soft")
    );

    reader.click_nth(".palette-choice", 1);
    assert_eq!(first(&mut reader).as_deref(), Some("#ffd60a"));

    // The first colour, retyped: the swatch under the passage follows.
    reader.click_nth(".colours-window .color-hex", 0);
    reader.press("End");
    for _ in 0..7 {
        reader.press("Backspace");
    }
    reader.type_text("#abcdef");
    assert_eq!(
        first(&mut reader).as_deref(),
        Some("#abcdef"),
        "the popover under the window shows the change at once"
    );

    reader.click(".colours-window .chip.action.primary");
    assert_eq!(reader.state().notice, "Saved Vivid copy.");
    assert_eq!(
        reader
            .harness
            .attr(".palette-choice.on", "data-palette")
            .as_deref(),
        Some("vivid-copy")
    );
    let palettes = config.join("palettes");
    let read = |name: &str| std::fs::read_to_string(palettes.join(name)).unwrap_or_default();
    assert!(read("vivid-copy.toml").contains("#abcdef"));
    assert!(
        read("vivid.toml").contains("#ffd60a"),
        "the shipped file is as it was"
    );

    // The copy is the reader's own, so it can go: Delete is the second of
    // New palette and Delete palette.
    reader.click_nth(".colours-window .pane-actions .chip.action", 1);
    reader.click(".ask-go");
    assert!(!palettes.join("vivid-copy.toml").exists());
    assert_eq!(
        reader
            .harness
            .attr(".palette-choice.on", "data-palette")
            .as_deref(),
        Some("soft"),
        "and the shipped default is in use again"
    );

    // A palette of the reader's own, begun from the button: untouched, it is
    // kept only when saved.
    reader.click(".colours-window .pane-actions .chip.action");
    assert_eq!(
        reader.text_all(".colours-window .chip.action.primary"),
        ["Save palette"]
    );
    reader.click(".colours-window .chip.action.primary");
    assert_eq!(reader.state().notice, "Saved New palette.");
    assert!(palettes.join("new-palette.toml").exists());
    assert_eq!(
        reader
            .harness
            .attr(".palette-choice.on", "data-palette")
            .as_deref(),
        Some("new-palette")
    );

    reader.press("Escape");
    assert!(
        reader.harness.query(".colours-window").is_none(),
        "Escape closes the window"
    );
    assert!(
        reader.harness.query(".markup-popover").is_some(),
        "and the swatches are still there to mark with"
    );
    let _ = std::fs::remove_dir_all(&config);
}

#[test]
fn a_sweep_offers_the_colours_and_a_swatch_marks_the_passage() {
    let path = readable("swept");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    // **The swatches come up by themselves**, which is the app's own
    // hard-won answer: there, the popover was reachable only by ⌘⇧H for a
    // while and nobody could find the feature at all.
    let swatches = reader.harness.query_all(".markup-swatch").len();
    assert_eq!(swatches, 6, "six colours, which is what the settings hold");

    let chosen = reader
        .harness
        .attr(".markup-swatch", "data-colour")
        .unwrap_or_default();
    reader.click(".markup-swatch");
    assert_eq!(reader.state().notice, "", "a mark says nothing");
    assert!(
        reader.harness.query(".markup-popover").is_none(),
        "and the swatches go once one of them has been chosen",
    );

    // In the file, which is the whole point of the feature.
    let marks = render::open(&path).expect("reopens").markup();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].page, 1);
    assert_eq!(marks[0].color, chosen, "in the colour that was pressed");
}

#[test]
fn the_panel_lists_the_passage_and_the_words_in_it() {
    let path = readable("listed");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    // The prose fixture has no table of contents, so the panel opens on its
    // pages — see `Viewer::restore`. The markup lives beside the contents.
    reader.press_chord("mod+b");
    reader.click("[data-tab=\"contents\"]");

    let rows = reader.harness.query_all(".markup-row");
    assert_eq!(
        rows.len(),
        1,
        "one row, in the panel that already lists marks"
    );
    let said = reader.harness.text_content(".markup-row .mark-go");
    assert!(
        said.starts_with("A needle"),
        "the row says what was marked, and it said {said:?}",
    );
}

#[test]
fn a_mark_is_still_there_the_next_time_the_document_is_opened() {
    // A second reader over the same file, which is what "in the document"
    // means and the only way to ask it.
    let path = readable("kept");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    drop(reader);

    let mut again = open(&path);
    again.press_chord("mod+b");
    again.click("[data-tab=\"contents\"]");
    assert_eq!(again.harness.query_all(".markup-row").len(), 1);
}

/// **One press is not a removal.** The × turns into the word for what it
/// does, and Escape puts it back.
#[test]
fn the_first_press_on_a_rows_cross_asks() {
    let path = readable("asked");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    reader.press_chord("mod+b");
    reader.click("[data-tab=\"contents\"]");

    reader.click(".markup-row .mark-drop");
    assert_eq!(
        reader.harness.text_content(".markup-row .mark-drop"),
        "Remove"
    );
    assert_eq!(render::open(&path).expect("reopens").markup().len(), 1);
    reader.press("Escape");
    assert_eq!(
        reader.harness.text_content(".markup-row .mark-drop"),
        "",
        "back to the cross, which is a drawing",
    );
    assert_eq!(render::open(&path).expect("reopens").markup().len(), 1);
}

#[test]
fn a_mark_can_be_taken_off_from_the_panel() {
    let path = readable("dropped");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    // The prose fixture has no table of contents, so the panel opens on its
    // pages — see `Viewer::restore`. The markup lives beside the contents.
    reader.press_chord("mod+b");
    reader.click("[data-tab=\"contents\"]");
    assert_eq!(reader.harness.query_all(".markup-row").len(), 1);

    reader.click(".markup-row .mark-drop");
    reader.click(".markup-row .mark-drop");
    assert_eq!(reader.state().notice, "", "nor does taking one out");
    assert_eq!(reader.harness.query_all(".markup-row").len(), 0);
    // And out of the file, not merely off the screen — which is the sentence
    // the app cannot say. See the head of `src/markup.rs`.
    assert!(render::open(&path).expect("reopens").markup().is_empty());
}

/// **Nothing is written into a draft the reader is not looking at.** A mark
/// is a place in *this* file, and Zotero or a compiler may have written
/// another since: taken out by index there, it takes out something else.
#[test]
fn a_mark_is_not_taken_out_of_a_file_that_changed_under_it() {
    let path = readable("changed-under");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    reader.press_chord("mod+b");
    reader.click("[data-tab=\"contents\"]");

    // Somebody else's highlight, written before the watch has said so.
    let (line, _) = first_line(&render::open(&path).expect("opens"), 2);
    markup::add(&path, &[(2, line)], "#74c0fc", "Zotero").expect("theirs is written");

    reader.click(".markup-row .mark-drop");
    reader.click(".markup-row .mark-drop");
    assert!(
        reader.state().notice.contains("changed on disk"),
        "{}",
        reader.state().notice
    );
    assert_eq!(
        render::open(&path).expect("reopens").markup().len(),
        2,
        "both are still there"
    );
}

#[test]
fn a_mark_clicked_on_offers_to_come_off_and_does() {
    // **The whole of what "I cannot remove a highlight" was.** Removal has
    // worked since the day markup landed and the only way to reach it was a
    // × the width of a full stop, on a row in a panel that does not open on
    // the tab the row is on. A mark is a thing on a page; the way to take it
    // off is on the page.
    let path = readable("clicked-off");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    assert_eq!(render::open(&path).expect("reopens").markup().len(), 1);
    assert!(
        reader.harness.query(".mark-popover").is_none(),
        "nothing is offered until the mark is clicked",
    );

    // A click, not a sweep: the press and the release in the same place, over
    // the words that were marked.
    reader.click_on_page(1, (0.30, LINE));
    assert!(
        reader.harness.query(".mark-popover").is_some(),
        "clicking the mark asks about the mark",
    );

    reader.click(".mark-remove");
    assert_eq!(reader.state().notice, "", "nor does taking one out");
    assert!(reader.harness.query(".mark-popover").is_none());
    // Out of the file, which is the sentence the app cannot say.
    assert!(render::open(&path).expect("reopens").markup().is_empty());
}

#[test]
fn a_mark_clicked_on_a_clickpad_still_asks() {
    // Pressing a clickpad down slides the pointer a few pixels, and past
    // two that was a sweep: a letter selected and the swatches offered.
    let path = readable("clickpad");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    // From several places along the line, so that the slide crosses from
    // one letter into the next somewhere.
    for at in [0.20, 0.23, 0.26, 0.29, 0.32, 0.35, 0.38, 0.41] {
        reader.click_unsteadily_on_page(1, (at, LINE), 3.9);
        assert!(reader.harness.query(".mark-popover").is_some(), "at {at}");
    }
}

#[test]
fn a_sweep_across_a_mark_selects_it_rather_than_asking_about_it() {
    // The reason the question is asked on the release and not on the press:
    // a passage that is already marked is exactly the passage somebody wants
    // to select and copy, and a popover that opened when the sweep began
    // would take itself down again as the sweep went on.
    let path = readable("swept-over");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");

    reader.sweep_page(1, (0.12, LINE), (0.40, LINE));
    assert!(
        reader.harness.query(".selected").is_some(),
        "the marked words can still be selected",
    );
    assert!(
        reader.harness.query(".mark-popover").is_none(),
        "and a sweep is not a question about the mark",
    );
}

#[test]
fn a_click_off_the_mark_asks_nothing() {
    let path = readable("clicked-past");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    // Well below the one line this fixture's pages carry.
    reader.click_on_page(1, (0.30, 0.70));
    assert!(reader.harness.query(".mark-popover").is_none());
}

#[test]
fn the_key_says_which_of_the_two_things_is_wrong() {
    // "Select something first" and "there is no text in this document" are
    // different sentences, and the second is the one worth saying: no amount
    // of selecting will help on a scan. The app's own step 7.
    let mut reader = open(&readable("unselected"));
    reader.press_chord("mod+shift+h");
    assert_eq!(
        reader.state().notice,
        "Select something first, and this highlights it."
    );
    assert!(reader.harness.query(".markup-popover").is_none());
}

#[test]
fn the_swatches_come_up_under_the_line_they_are_about() {
    // The app had this wrong for a day: its anchor element had no height, so
    // `getBoundingClientRect().bottom` was the *top* of the selection and the
    // swatches came up over the words they were about. Here the rectangle is
    // the line's own, so there is a number to check.
    let mut reader = open(&readable("placed-popover"));
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    assert!(
        reader.harness.query(".selected").is_some(),
        "a line is selected"
    );
    assert!(
        reader.harness.query(".markup-popover").is_some(),
        "the swatches are up"
    );
    let line = reader.harness.layout_rect(".selected");
    let popover = reader.harness.layout_rect(".markup-popover");
    assert!(
        popover.y >= line.y + line.height,
        "the swatches are at {} and the line ends at {}",
        popover.y,
        line.y + line.height,
    );
}

#[cfg(unix)]
#[test]
fn a_document_that_cannot_be_written_keeps_its_marks_beside_it() {
    // Step 7's first edge, and the one that decides whether this feature can
    // be trusted at all: a passage the reader marked is not lost because the
    // disk said no.
    use std::os::unix::fs::PermissionsExt;
    let path = readable("read-only");
    let before = std::fs::read(&path).expect("the fixture is on disk");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444))
        .expect("make it read-only");

    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    assert!(
        reader.harness.query_all(".markup-comment").is_empty(),
        "a comment is not offered where it cannot be written",
    );
    reader.click(".markup-swatch");
    assert_eq!(
        reader.state().notice,
        "Highlighted. This document is read-only, so the highlight is kept beside it rather than in it.",
    );
    // The prose fixture has no table of contents, so the panel opens on its
    // pages — see `Viewer::restore`. The markup lives beside the contents.
    reader.press_chord("mod+b");
    reader.click("[data-tab=\"contents\"]");
    assert_eq!(reader.harness.query_all(".markup-row").len(), 1);
    assert_eq!(
        reader.harness.text_content(".markup-beside"),
        "beside the document",
        "and the row says which kind of mark it is",
    );
    assert_eq!(
        std::fs::read(&path).expect("still there"),
        before,
        "the document itself was not touched",
    );

    // Taken off again, which for this kind is a line out of `library.toml`.
    reader.click(".markup-row .mark-drop");
    reader.click(".markup-row .mark-drop");
    assert_eq!(reader.harness.query_all(".markup-row").len(), 0);

    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644));
}

#[test]
fn a_passage_survives_the_document_being_rebuilt() {
    // **The case the whole journal exists for.** A paper recompiled by LaTeX
    // is a new file: every annotation in the old one went with it, and the
    // words are usually still there. So the passage is looked up again and
    // written back — offered, never done on its own, because re-anchoring is
    // a guess however good a one.
    let path = readable("rebuilt");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    let marks = render::open(&path).expect("reopens").markup();
    assert_eq!(marks.len(), 1);
    // A comment written on it, which the file alone carries.
    markup::set_note(&path, 1, marks[0].index, "see chapter 2", "").expect("noted");
    reader.document_changed(&path);

    // The compiler's output: the same six pages, written over the top the way
    // `atomic_write` and every LaTeX run does it.
    std::fs::copy(moonowl::fixture::prose_pdf(), &path).expect("recompiled");
    reader.document_changed(&path);
    assert!(
        render::open(&path).expect("reopens").markup().is_empty(),
        "the rebuild took the annotation with it, which is the premise",
    );
    assert_eq!(
        reader.state().notice,
        "This version of the document lost a highlight. The sidebar can put it back.",
    );
    assert!(
        reader.harness.query(".kept").is_none(),
        "and it is not drawn where the old version had it",
    );

    reader.press_chord("mod+b");
    reader.click("[data-tab=\"contents\"]");
    assert_eq!(
        reader.harness.text_content(".markup-restore"),
        "Put 1 passage back",
        "and the panel offers to look it up again",
    );
    reader.click(".markup-restore");
    assert_eq!(reader.state().notice, "1 passage put back.");
    let marks = render::open(&path).expect("reopens").markup();
    assert_eq!(marks.len(), 1, "and it is in the file again");
    assert_eq!(marks[0].page, 1);
    assert_eq!(marks[0].note, "see chapter 2", "with its comment");
    assert!(
        reader.harness.query(".markup-restore").is_none(),
        "with nothing left to offer",
    );
    // And ⌘Z takes back what was put back, as it does a highlight.
    reader.press_chord("mod+z");
    assert!(
        render::open(&path).expect("reopens").markup().is_empty(),
        "{}",
        reader.state().notice,
    );
}

#[test]
fn a_mark_the_reader_took_off_is_not_offered_back() {
    // The other half of the same machinery, and the bug the app had to fix in
    // it: a mark missing from the file after a removal looks exactly like a
    // mark a rebuild lost. The journal is told first, which is what tells
    // them apart.
    let path = readable("not-offered");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    reader.press_chord("mod+b");
    reader.click("[data-tab=\"contents\"]");
    reader.click(".markup-row .mark-drop");
    reader.click(".markup-row .mark-drop");
    assert_eq!(reader.harness.query_all(".markup-row").len(), 0);
    assert!(
        reader.harness.query(".markup-restore").is_none(),
        "nothing was lost, so there is nothing to put back",
    );
}

#[test]
fn the_mark_is_on_the_screen_in_the_colour_it_was_given() {
    // **The test that found both faults in this item**, and the reason it
    // renders rather than re-reading: a highlight written with the corners in
    // the wrong order reads back perfectly and draws as nothing, and a page
    // drawn with pdfium's byte order reversed reads back perfectly and draws
    // red as blue. Neither is visible from anywhere but a pixel.
    let path = readable("on-screen");
    let options = Options {
        settings: vec![("highlight_palette".into(), serde_json::json!("vivid"))],
        ..Options::default()
    };
    let mut reader = Reader::open_with(&path, options);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    // The third swatch of Vivid — `#ff6b6b`, the one colour of the six whose
    // channels are far enough apart to say which is which.
    let colour = reader
        .attribute_all(".markup-swatch", "data-colour")
        .get(2)
        .cloned()
        .expect("six swatches");
    assert_eq!(colour, "#ff6b6b");
    reader.click_nth(".markup-swatch", 2);
    assert_eq!(reader.state().notice, "", "a mark says nothing");

    let shot = reader.screenshot();
    let wanted: [i32; 3] = [0xff, 0x6b, 0x6b];
    let mut close = 0;
    for y in 0..shot.height {
        for x in 0..shot.width {
            let pixel = shot.at(x, y);
            if (0..3).all(|c| (pixel[c] as i32 - wanted[c]).abs() <= 24) {
                close += 1;
            }
        }
    }
    assert!(
        close > 500,
        "only {close} pixels of the window are the colour the passage was marked in",
    );
}

/// Past the limit a mark goes beside the document, because writing it in is
/// the whole file rewritten on the thread that draws the window. The file is
/// sparse: a length, and no bytes.
#[test]
fn a_very_large_document_is_not_written_into() {
    let path = std::env::temp_dir().join(format!("moonowl-huge-{}.pdf", std::process::id()));
    let file = std::fs::File::create(&path).expect("a file");
    file.set_len(moonowl::markup::IN_FILE_LIMIT + 1)
        .expect("a length");
    let standing = moonowl::markup::standing(path.to_str().unwrap(), false, false);
    let _ = std::fs::remove_file(&path);
    assert!(!standing.into_file);
    assert_eq!(standing.refused, "This document is very large");
}

#[cfg(unix)]
#[test]
fn a_mark_on_a_linked_document_lands_where_the_link_points() {
    // A rename replaces a link itself, never its target: the first highlight
    // turned the link into a plain copy and left the real paper unmarked.
    let path = scratch("linked");
    let link = path.with_file_name("link.pdf");
    std::os::unix::fs::symlink(&path, &link).expect("a link to the fixture");
    let document = render::open(link.to_str().unwrap()).expect("opens through the link");
    let (quads, _) = first_line(&document, 1);
    drop(document);

    markup::add(link.to_str().unwrap(), &[(1, quads)], "#ffd60a", "Moonowl").expect("written");

    assert!(
        link.symlink_metadata().unwrap().file_type().is_symlink(),
        "the link is still a link"
    );
    let real = render::open(path.to_str().unwrap()).expect("the target reopens");
    assert_eq!(real.markup().len(), 1, "and the mark is in the target");
}

/// **A signed document is asked about before it is rewritten**, not told
/// afterwards: the first colour chosen writes nothing and asks, the same
/// click again only asks again, and "Highlight anyway" goes ahead.
#[test]
fn a_signed_document_asks_before_it_is_marked() {
    let dir = std::env::temp_dir().join(format!("moonowl-marked-{}-signed", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory to write in");
    let path = dir.join("signed.pdf");
    std::fs::copy(moonowl::fixture::signed_pdf(), &path).expect("a copy of the fixture");
    let path = path.to_string_lossy().into_owned();
    let before = std::fs::read(&path).expect("read");

    let mut reader = open(&path);
    reader.press_chord("mod+a");
    reader.press_chord("mod+shift+h");
    reader.click(".markup-swatch");
    assert!(!reader.harness.query_all(".ask-go").is_empty(), "it asks");
    reader.press("Escape");
    reader.click(".markup-swatch");
    assert!(
        !reader.harness.query_all(".ask-go").is_empty(),
        "and asks again after a Cancel"
    );
    assert_eq!(
        std::fs::read(&path).expect("read"),
        before,
        "nothing written yet"
    );

    reader.click(".ask-go");
    assert_eq!(render::open(&path).expect("reopens").markup().len(), 1);
}

/// …and before a mark is taken out of it, which rewrites it just the same.
#[test]
fn a_signed_document_asks_before_a_mark_comes_out() {
    let dir = std::env::temp_dir().join(format!("moonowl-unmarked-{}-signed", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory to write in");
    let path = dir.join("signed.pdf");
    std::fs::copy(moonowl::fixture::signed_pdf(), &path).expect("a copy of the fixture");
    let path = path.to_string_lossy().into_owned();
    let (quads, _) = first_line(&render::open(&path).expect("opens"), 1);
    markup::add(&path, &[(1, quads)], "#ffd60a", "Acrobat").expect("theirs is written");
    let before = std::fs::read(&path).expect("read");

    let mut reader = open(&path);
    reader.press_chord("mod+b");
    reader.click("[data-tab=\"contents\"]");
    reader.click(".markup-row .mark-drop");
    reader.click(".markup-row .mark-drop");
    assert!(!reader.harness.query_all(".ask-go").is_empty(), "it asks");
    assert_eq!(
        std::fs::read(&path).expect("read"),
        before,
        "nothing written yet"
    );

    reader.click(".ask-go");
    assert!(render::open(&path).expect("reopens").markup().is_empty());
}

/// **A Remove waiting for its second press does not outlive a reload.** The
/// row it was armed on is keyed by an index into the file's annotations, and
/// after a rewrite by something else that index is somebody else's: one
/// click took out a highlight nobody had asked twice about.
#[test]
fn a_reload_disarms_a_removal() {
    let path = readable("rearmed");
    let (quads, _) = first_line(&render::open(&path).expect("opens"), 1);
    markup::add(&path, &[(1, quads.clone())], "#ffd60a", "Acrobat").expect("written");

    let mut reader = open(&path);
    reader.press_chord("mod+b");
    reader.click("[data-tab=\"contents\"]");
    reader.click(".markup-row .mark-drop");

    markup::add(&path, &[(1, quads)], "#a0d8ff", "Acrobat").expect("written again");
    reader.document_changed(&path);
    reader.settle();
    reader.click(".markup-row .mark-drop");
    assert_eq!(
        render::open(&path).expect("reopens").markup().len(),
        2,
        "one click after a reload only arms"
    );
}

/// **Marking a passage keeps the way back.** Every write reopens the
/// document, and the reopen cleared the history as a rebuild must.
#[test]
fn a_mark_keeps_the_way_back() {
    let path = readable("way-back");
    let mut reader = open(&path);
    reader.press("p");
    reader.type_text("2");
    reader.press("Enter");
    assert_eq!(reader.state().page, 2);

    reader.sweep_page(2, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    assert_eq!(render::open(&path).expect("reopens").markup().len(), 1);

    reader.press_chord("mod+[");
    assert_eq!(reader.state().page, 1);
}

/// **A writable document in a folder that is not is refused up front**, as a
/// read-only one is: the write is a new file in the folder renamed over the
/// document, and it failed at the write with a highlight half made.
#[cfg(unix)]
#[test]
fn a_document_in_a_read_only_folder_is_marked_beside_it() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("moonowl-folder-{}-shut", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory");
    let path = dir.join("paper.pdf");
    std::fs::copy(readable("folder-source"), &path).expect("a copy");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).expect("shut it");

    let standing = markup::standing(&path.to_string_lossy(), false, false);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).expect("open it");
    assert!(!standing.into_file);
    assert!(standing.refused.contains("folder"), "{}", standing.refused);
}

/// **Half a draft is not written over.** A compiler that starts writing just
/// after the reader's stamp check handed pdfium a document without its end,
/// which it repairs — and the repair was renamed over the draft in progress.
#[test]
fn a_document_caught_half_written_is_left_alone() {
    let path = readable("half");
    let (quads, _) = first_line(&render::open(&path).expect("opens"), 1);
    let mut bytes = std::fs::read(&path).expect("read");
    bytes.truncate(bytes.len() / 2);
    std::fs::write(&path, &bytes).expect("half of it");

    let refused = markup::add(&path, &[(1, quads)], "#ffd60a", "Moonowl").expect_err("refused");
    assert!(refused.contains("being written"), "{refused}");
    assert_eq!(std::fs::read(&path).expect("read"), bytes, "and untouched");
}

/// **A document moved away while open keeps reading.** The write cannot land,
/// and letting go of the file for it lost the only handle still reading the
/// moved file: every page went blank.
#[cfg(unix)]
#[test]
fn a_highlight_on_a_document_moved_away_leaves_it_readable() {
    let path = readable("moved");
    let mut reader = open(&path);
    std::fs::rename(&path, format!("{path}.moved")).expect("moved");
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    let notice = reader.state().notice;
    assert!(notice.contains("no longer where"), "{notice}");
    // Kept beside the document, and drawn where it was made: pdfium has
    // nothing of it to draw.
    let (_, top, width, _) = reader
        .box_of(".kept")
        .expect("the highlight is on the page");
    let (_, page_top, page_width, page_height) = reader.box_of(".page").expect("a page");
    assert!(
        ((top - page_top) / page_height - LINE).abs() < 0.05,
        "at {top}"
    );
    assert!(width > page_width * 0.3, "{width} of {page_width}");
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.press_chord("mod+c");
    assert!(
        reader.copied().last().is_some_and(|text| !text.is_empty()),
        "the page still has its words"
    );
}

/// **A mark taken off in another app stays off.** It came back as a row
/// "not in the document", with an offer to put it back, as though a rebuild
/// had lost it.
#[test]
fn a_mark_taken_off_elsewhere_is_not_a_ghost() {
    let path = readable("taken-elsewhere");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    reader.press("ArrowRight");
    reader.settle();
    reader.sweep_page(2, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    assert_eq!(render::open(&path).expect("reopens").markup().len(), 2);

    // Preview, say, taking off the second.
    let second = render::open(&path).expect("reopens").markup()[1].clone();
    moonowl::markup::remove(&path, second.page, second.index).expect("taken off");
    reader.document_changed(&path);

    reader.press_chord("mod+b");
    reader.click("[data-tab=\"contents\"]");
    assert_eq!(reader.harness.query_all(".markup-row").len(), 1);
    assert!(reader.harness.query(".markup-restore").is_none());
}

/// **The swatches answer a pointer already over them, on the frame they
/// appear.** A sweep let go of just below its last line leaves the pointer
/// where the popover then comes up. With the scrollbar faded and no link on
/// the page, the swatch under it was not hovered and moving over the
/// swatches changed nothing — see `.hit-layer`. One frame is all this lets
/// happen after the release, because that is all the window is sure to draw.
#[test]
fn a_swatch_under_the_pointer_is_hovered_the_frame_it_appears() {
    use blitz_traits::events::{BlitzPointerId, MouseEventButton, MouseEventButtons, UiEvent};
    let mut reader = open(&readable("hovered"));
    // Where the swatches come up for this sweep.
    reader.sweep_page(1, (0.10, LINE), (0.25, LINE));
    let swatch = reader
        .harness
        .layout_rect_of(reader.harness.query_all(".markup-swatch")[3]);
    let (x, y) = (
        swatch.x + swatch.width / 2.0,
        swatch.y + swatch.height / 2.0,
    );
    reader.press("Escape");
    reader.press("Escape");
    // The bar fades a few seconds after the last scroll.
    for token in 0..20 {
        reader.deliver(moonowl::emit::News {
            event: "bar-timeout".into(),
            target: None,
            payload: moonowl::emit::Payload::Token(token),
        });
    }
    reader.settle();
    assert!(reader.harness.query(".scrollbar").is_none());

    let from = reader.point_on(1, (0.10, LINE));
    reader.harness.mouse_down_at(from.0, from.1);
    reader.carry(x, y);
    reader.harness.pump();
    reader.harness.mouse_up_at(x, y);
    let under = reader.harness.query_all(".markup-swatch")[3];
    assert_eq!(
        reader.harness.hovered(),
        Some(under),
        "hovered as it appears"
    );
    reader
        .harness
        .dispatch(UiEvent::PointerMove(blitz_test_harness::pointer_event(
            BlitzPointerId::Mouse,
            x + 2.0,
            y + 1.0,
            MouseEventButton::Main,
            MouseEventButtons::empty(),
            Default::default(),
        )));
    assert_eq!(reader.harness.hovered(), Some(under), "and after a move");
}

/// "Remove all highlights" asks, and a yes takes every one out: the file's
/// in one write, and the ones beside it with them.
#[test]
fn every_highlight_comes_out_at_once_after_asking() {
    let path = readable("all");
    let (quads, _) = first_line(&render::open(&path).expect("opens"), 1);
    let (more, _) = first_line(&render::open(&path).expect("opens"), 2);
    markup::add(&path, &[(1, quads), (2, more)], "#ffd60a", "Acrobat").expect("written");
    let mut reader = open(&path);

    reader.click(".chip.title");
    reader.click("[data-item='unmark-all']");
    assert_eq!(
        reader.harness.query_all(".ask-window").len(),
        1,
        "it asks first"
    );
    reader.press("Escape");
    assert_eq!(
        render::open(&path).expect("reopens").markup().len(),
        2,
        "and Escape is no"
    );

    reader.click(".chip.title");
    reader.click("[data-item='unmark-all']");
    reader.click(".ask-go");
    assert!(render::open(&path).expect("reopens").markup().is_empty());
    reader.press_chord("mod+b");
    reader.click("[data-tab=\"contents\"]");
    assert_eq!(reader.harness.query_all(".markup-row").len(), 0);

    reader.click(".chip.title");
    reader.click("[data-item='unmark-all']");
    assert_eq!(
        reader.state().notice,
        "There are no highlights in this document."
    );
}

#[test]
fn a_highlight_change_is_undone_and_redone() {
    let path = readable("undo");
    let marks = || render::open(&path).expect("reopens").markup();
    let mut reader = open(&path);
    reader.press_chord("mod+z");
    assert_eq!(reader.state().notice, "Nothing to undo.");

    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    assert_eq!(marks().len(), 1);
    reader.press_chord("mod+z");
    assert!(marks().is_empty(), "a mark made is taken back");
    reader.press_chord("mod+shift+z");
    assert_eq!(marks().len(), 1, "and made again");

    // Every mark at once, which is the one that asks.
    reader.click(".chip.title");
    reader.click("[data-item='unmark-all']");
    reader.click(".ask-go");
    assert!(marks().is_empty());
    reader.press_chord("mod+z");
    assert_eq!(marks().len(), 1, "every mark comes back");
    reader.press_chord("mod+z");
    assert!(marks().is_empty(), "and the one before that");
    reader.press_chord("mod+shift+z");
    reader.press_chord("mod+shift+z");
    assert!(marks().is_empty(), "redone in order");
    reader.press_chord("mod+shift+z");
    assert_eq!(reader.state().notice, "Nothing to redo.");

    // A change on disk that is not ours forgets them: undo puts the whole
    // file back, and would take that change with it.
    reader.press_chord("mod+z");
    assert_eq!(marks().len(), 1);
    let (line, _) = first_line(&render::open(&path).expect("opens"), 2);
    markup::add(&path, &[(2, line)], "#74c0fc", "Zotero").expect("theirs is written");
    reader.document_changed(&path);
    reader.press_chord("mod+z");
    assert_eq!(reader.state().notice, "Nothing to undo.");
    assert_eq!(marks().len(), 2, "theirs is kept");
}

#[test]
fn a_passage_takes_a_comment_that_other_readers_can_read() {
    let path = readable("comment");
    let marks = || render::open(&path).expect("reopens").markup();
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-comment");
    assert!(
        reader.harness.query(".markup-swatch").is_none(),
        "the field takes the row"
    );
    reader.type_text("Worth a second look");
    reader.press_chord("mod+enter");
    // A highlight with `/Contents`, which is what Preview and Acrobat write.
    let written = marks();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].note, "Worth a second look");

    // Clicked on, the mark says it, and it can be changed.
    reader.click_on_page(1, (0.30, LINE));
    assert_eq!(
        reader.harness.text_content(".mark-note .note-card-text"),
        "Worth a second look"
    );
    // Edited in its card, where it is read, and the rest of the menu goes.
    reader.click(".mark-note .note-card-edit");
    assert!(reader.harness.query(".mark-popover").is_none());
    assert_eq!(reader.field(".note-card-field"), "Worth a second look");
    reader.type_text(", twice");
    reader.click(".note-card-done");
    assert_eq!(marks()[0].note, "Worth a second look, twice");

    reader.press_chord("mod+b");
    reader.click("[data-tab=\"contents\"]");
    assert_eq!(
        reader.harness.text_content(".markup-note"),
        "Worth a second look, twice"
    );

    reader.press_chord("mod+b");
    reader.press_chord("mod+z");
    assert_eq!(marks()[0].note, "Worth a second look", "and undone");

    // Its words taken off, and the mark left. Clicked somewhere else on it:
    // a fast run is within a double click of the last click there.
    reader.click_on_page(1, (0.45, LINE));
    reader.click(".mark-uncomment");
    assert_eq!(marks().len(), 1);
    assert_eq!(marks()[0].note, "");
    reader.press_chord("mod+z");
    assert_eq!(marks()[0].note, "Worth a second look");

    // Done while the document is being written keeps the field and its
    // words, and they outlast the write landing; Done again writes them.
    reader.while_writing(|reader| {
        reader.sweep_page(1, (0.40, LINE), (0.55, LINE));
        reader.click(".markup-swatch");
        reader.sweep_page(1, (0.12, LINE), (0.30, LINE));
        reader.click(".markup-comment");
        reader.type_text("not yet");
        reader.press_chord("mod+enter");
        assert_eq!(
            reader.state().notice,
            "Still writing the last change into the document."
        );
        assert_eq!(reader.field(".note-card-field"), "not yet");
    });
    assert_eq!(marks().len(), 2, "the first write landed");
    assert_eq!(reader.field(".note-card-field"), "not yet");
    reader.press_chord("mod+enter");
    let written = marks();
    assert_eq!(written.len(), 3);
    assert!(written.iter().any(|mark| mark.note == "not yet"));
    reader.press_chord("mod+z");
    reader.press_chord("mod+z");
    assert_eq!(marks().len(), 1);

    // Escape keeps the words, as a press elsewhere does, and ⌘Z takes them
    // back.
    reader.sweep_page(1, (0.12, LINE), (0.40, LINE));
    reader.click(".markup-comment");
    reader.type_text("never mind");
    reader.press("Escape");
    assert_eq!(marks().len(), 2);
    reader.press_chord("mod+z");
    assert_eq!(marks().len(), 1);
    assert_eq!(marks()[0].note, "Worth a second look");
}

#[test]
fn a_mark_taken_off_with_its_comment_says_so_and_how_to_get_it_back() {
    let path = readable("comment-off");
    let marks = || render::open(&path).expect("reopens").markup();
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-comment");
    reader.type_text("Worth keeping");
    reader.press_chord("mod+enter");
    reader.click_on_page(1, (0.30, LINE));
    reader.click(".mark-remove");
    assert!(marks().is_empty());
    let undo = if cfg!(target_os = "macos") {
        "⌘Z"
    } else {
        "Ctrl+Z"
    };
    assert_eq!(
        reader.state().notice,
        format!("Comment also removed.\nPress {undo} to undo.")
    );
    reader.press_chord("mod+z");
    assert_eq!(marks()[0].note, "Worth keeping");
}

#[test]
fn a_marks_menu_has_its_comment_then_its_colours_then_its_rows() {
    let path = readable("one-row");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-comment");
    reader.type_text("A comment long enough to wrap onto a second line of its own, if it has to");
    reader.press_chord("mod+enter");
    reader.click_on_page(1, (0.30, LINE));
    let (_, swatch, _, _) = reader.box_of(".mark-swatch").expect("swatches");
    let (_, note, _, height) = reader.box_of(".mark-note").expect("the comment");
    assert!(note + height <= swatch, "the comment above the colours");
    // Edited in its card, so there is no row for that.
    assert_eq!(
        reader.attribute_all(".mark-popover .menu-item", "data-item"),
        vec!["copy", "recolour", "uncomment", "remove"],
    );
    let rows: Vec<f32> = [
        ".mark-popover [data-item='copy']",
        ".mark-popover [data-item='recolour']",
        ".mark-uncomment",
        ".mark-remove",
    ]
    .iter()
    .map(|row| reader.box_of(row).expect(row).1)
    .collect();
    assert!(rows[0] > swatch, "the rows under the colours");
    assert!(
        rows.windows(2).all(|pair| pair[1] > pair[0] + 20.0),
        "one under another: {rows:?}"
    );
}

#[test]
fn undo_in_a_field_undoes_the_typing_and_not_a_highlight() {
    let path = readable("field-undo");
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-swatch");
    reader.press_chord("mod+f");
    reader.type_text("needle first");
    reader.press("Backspace");
    reader.press_chord("mod+z");
    assert_eq!(reader.field(".find-field"), "needle first");
    reader.press_chord("mod+z");
    assert_eq!(
        reader.field(".find-field"),
        "",
        "a run of typing is one step"
    );
    reader.press_chord("mod+shift+z");
    assert_eq!(reader.field(".find-field"), "needle first");
    assert_eq!(
        render::open(&path).expect("reopens").markup().len(),
        1,
        "the highlight is still there"
    );
}

/// **A comment is signed with the reader's name**, as Preview signs one, and
/// a mark that names nobody takes the name of whoever writes on it.
#[test]
fn a_comment_is_signed_with_the_readers_name() {
    let path = readable("signed-comment");
    let options = Options {
        settings: vec![("author".into(), serde_json::json!("A Reader"))],
        ..Options::default()
    };
    let mut reader = Reader::open_with(&path, options);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.click(".markup-comment");
    reader.type_text("Worth a second look");
    reader.press_chord("mod+enter");
    let by = |path: &str| {
        render::open(path)
            .expect("reopens")
            .notes_of(0)
            .into_iter()
            .map(|note| note.by)
            .collect::<Vec<_>>()
    };
    assert_eq!(by(&path), ["A Reader"], "the name is the comment's author");

    let unsigned = readable("unsigned-comment");
    let (line, _) = first_line(&render::open(&unsigned).expect("opens"), 1);
    markup::add(&unsigned, &[(1, line)], "#74c0fc", "").expect("a mark of nobody's");
    let index = render::open(&unsigned).expect("reopens").markup()[0].index;
    markup::set_note(&unsigned, 1, index, "mine now", "A Reader").expect("noted");
    assert_eq!(
        by(&unsigned),
        ["A Reader"],
        "and signs a mark that named nobody"
    );
}

/// A draft that changed after the write began is not written over: the check
/// is made right before the rename, after the read and the save.
#[test]
fn a_mark_is_not_written_into_a_draft_that_changed() {
    let path = scratch("changed");
    let file = path.to_str().unwrap();
    let document = render::open(file).expect("the fixture opens");
    let (quads, _) = first_line(&document, 1);
    drop(document);
    let before = std::fs::read(&path).expect("the fixture");
    let stamp = render::stamp_of(file).expect("a stamp");
    let elsewhere = Some((stamp.0 + 1, stamp.1));
    let refused = markup::into_draft(elsewhere, || {
        markup::add(file, &[(1, quads.clone())], "#ffd60a", "Moonowl")
    });
    assert!(refused.is_err(), "refused");
    assert_eq!(
        std::fs::read(&path).expect("still there"),
        before,
        "and untouched"
    );
    markup::into_draft(Some(stamp), || {
        markup::add(file, &[(1, quads)], "#ffd60a", "Moonowl")
    })
    .expect("the draft it was meant for takes it");
}
