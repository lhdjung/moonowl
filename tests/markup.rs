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

#[test]
fn the_six_colours_can_be_changed_and_put_back() {
    let mut reader = open(&readable("recoloured"));
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    let first = reader
        .harness
        .attr(".markup-swatch", "data-colour")
        .unwrap_or_default();
    reader.click(".markup-more");
    assert!(
        reader.harness.query(".colours-window").is_some(),
        "the … opens the window with the full picker"
    );
    assert_eq!(
        reader.harness.query_all(".colours-window .color-hex").len(),
        6
    );

    // The first colour, retyped: the swatch under the passage follows.
    reader.click_nth(".colours-window .color-hex", 0);
    reader.press("End");
    for _ in 0..7 {
        reader.press("Backspace");
    }
    reader.type_text("#123456");
    assert_eq!(
        reader
            .harness
            .attr(".markup-swatch", "data-colour")
            .as_deref(),
        Some("#123456"),
        "the popover under the window shows the change at once"
    );

    // Resetting asks first, and keeping them changes nothing.
    reader.click(".colours-window .chip.action");
    assert!(
        reader.harness.query(".colours-ask").is_some(),
        "a question, not a reset"
    );
    reader.click_nth(".colours-window .pane-actions .chip.action", 1);
    assert_eq!(
        reader
            .harness
            .attr(".markup-swatch", "data-colour")
            .as_deref(),
        Some("#123456"),
    );
    reader.click(".colours-window .chip.action");
    reader.click(".colours-window .chip.action.danger");
    assert_eq!(
        reader
            .harness
            .attr(".markup-swatch", "data-colour")
            .as_deref(),
        Some(first.as_str()),
        "reset puts the default back"
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
    assert_eq!(reader.state().notice, "", "nor does taking one out");
    assert_eq!(reader.harness.query_all(".markup-row").len(), 0);
    // And out of the file, not merely off the screen — which is the sentence
    // the app cannot say. See the head of `src/markup.rs`.
    assert!(render::open(&path).expect("reopens").markup().is_empty());
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
        "Select something first, and this marks it."
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
    reader.click(".markup-swatch");
    assert_eq!(
        reader.state().notice,
        "Marked — but this document is read-only, so it is kept beside the document rather than in it.",
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
    assert_eq!(render::open(&path).expect("reopens").markup().len(), 1);

    // The compiler's output: the same six pages, written over the top the way
    // `atomic_write` and every LaTeX run does it.
    std::fs::copy(moonowl::fixture::prose_pdf(), &path).expect("recompiled");
    reader.document_changed(&path);
    assert!(
        render::open(&path).expect("reopens").markup().is_empty(),
        "the rebuild took the annotation with it, which is the premise",
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
    assert!(
        reader.harness.query(".markup-restore").is_none(),
        "with nothing left to offer",
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
    let mut reader = open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    // The third swatch, which is `markup_color_3` — `#ff6b6b`, the one colour
    // of the six whose channels are far enough apart to say which is which.
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
    assert_eq!(standing.refused, "this document is very large");
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
