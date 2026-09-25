//! Sweeping the pointer over words, and what a reader then does with them.
//!
//! Phase 3 item 10, the half that had to exist before markup could. The app
//! has no test file for any of this and could not usefully have one: there,
//! selecting text is the webview's, and what `reader.test.mjs` can assert is
//! that a text layer exists. Here the selection is the reader's own, so what
//! it covers, what it reads as and what reaches the clipboard are all things
//! `cargo test` can ask.
//!
//! Everything is asked through the interface: a sweep is three pointer events,
//! the selection is the rectangles on the page, and a copy is what the
//! harness's clipboard wrote down — see [`moonowl::app::Clip`], which
//! exists so that a test run does not empty anybody's real one.

use moonowl::fixture;
use moonowl::harness::{Options, Reader};
use moonowl::keymap::Action;
use moonowl::stats;

/// Six pages, one line of type near the top of each. `PROSE` says what they
/// are, which is what makes an assertion about copied text an assertion about
/// the document rather than about whatever came back.
fn prose() -> Reader {
    Reader::open(&fixture::prose_pdf())
}

/// Where the one line of a `prose_pdf` page sits inside its box, as fractions:
/// the type is set at 18 points on a 792-point page with its baseline at 700,
/// so it is about a tenth of the way down and starts an eighth of the way
/// across.
const LINE: f32 = 0.108;

/// How many selection rectangles are on screen. One per line of type, which on
/// this fixture means one per page swept.
fn painted(reader: &Reader) -> usize {
    reader.harness.query_all(".selected").len()
}

/// What is selected, by copying it — which is the only way a reader can find
/// out too, and therefore the right way for a test to ask.
///
/// Empty when nothing was selected: ⌘C on an empty selection copies nothing
/// and says so on the notice line, so the list the harness keeps does not
/// grow. See [`moonowl::app::Clip`].
fn selected(reader: &mut Reader) -> String {
    let before = reader.copied().len();
    reader.press_chord("mod+c");
    let now = reader.copied();
    if now.len() == before {
        String::new()
    } else {
        now.last().cloned().unwrap_or_default()
    }
}

/// Go to a page through the field in the toolbar, which is what a reader does.
fn go_to(reader: &mut Reader, page: usize) {
    reader.press("p");
    reader.type_text(&page.to_string());
    reader.press("Enter");
}

#[test]
fn a_sweep_across_a_line_selects_the_words_under_it() {
    let mut reader = prose();
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    assert!(painted(&reader) > 0, "nothing was painted over the words");
    assert_eq!(
        selected(&mut reader),
        "A needle in the first page.",
        "the sweep covered the line, so the line is what is selected"
    );
}

#[test]
fn a_sweep_that_stops_short_selects_what_it_covered() {
    let mut reader = prose();
    reader.sweep_page(1, (0.10, LINE), (0.20, LINE));
    let some = selected(&mut reader);
    assert!(
        some.starts_with('A') && some.len() < "A needle in the first page.".len(),
        "expected the first few characters, got {some:?}"
    );
}

#[test]
fn a_sweep_backwards_covers_the_same_words() {
    let mut forwards = prose();
    forwards.sweep_page(1, (0.10, LINE), (0.55, LINE));
    let mut backwards = prose();
    backwards.sweep_page(1, (0.55, LINE), (0.10, LINE));
    assert_eq!(selected(&mut forwards), selected(&mut backwards));
}

#[test]
fn a_click_is_not_a_selection() {
    let mut reader = prose();
    reader.sweep_page(1, (0.30, LINE), (0.30, LINE));
    assert_eq!(painted(&reader), 0);
    assert_eq!(selected(&mut reader), "");
}

#[test]
fn a_sweep_below_the_line_reaches_the_end_of_it() {
    // A reader dragging down the margin means "carry on to the end", which is
    // `page_at_point` clamping into the page and `caret_at` choosing the line
    // it is nearest to. Neither is visible from the DOM; the text is.
    let mut reader = prose();
    reader.sweep_page(1, (0.10, LINE), (0.90, 0.60));
    assert_eq!(selected(&mut reader), "A needle in the first page.");
}

#[test]
fn a_sweep_across_two_pages_selects_on_both() {
    let mut reader = prose();
    // Small enough that the *first line of each* page is on screen at once,
    // which fit page alone no longer manages: a page fitted to this window is
    // 813 points tall and the window is 900, so page two's first line lands
    // below the bottom of it and the move that would reach it is never
    // delivered. Three steps out is 50%, which is comfortable. (The document
    // area grew when the notice line stopped being a row of the window — see
    // `.notice-line` in `styles.rs` — and fit page grew with it.)
    reader.press_action(Action::FitPage);
    for _ in 0..3 {
        reader.press_chord("mod+-");
    }
    // From the line on page one to the line on page two, which is the second
    // page's own box further down the same scroll.
    let start = reader.point_on(1, (0.10, LINE));
    let end = reader.point_on(2, (0.55, LINE));
    reader.sweep(start, end);
    let text = selected(&mut reader);
    assert!(
        text.starts_with("A needle in the first page.")
            && text.ends_with("Nothing to look for on this one."),
        "expected both lines, got {text:?}"
    );
    // And both pages are painted, which is the thing a reader would see.
    assert!(painted(&reader) >= 2, "only one page was painted");
}

#[test]
fn copying_puts_the_words_on_the_clipboard() {
    let mut reader = prose();
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.press_chord("mod+c");
    assert_eq!(reader.copied(), vec!["A needle in the first page."]);
    assert_eq!(reader.state().notice, "Copied.");
}

#[test]
fn copying_nothing_says_so_and_copies_nothing() {
    let mut reader = prose();
    reader.press_chord("mod+c");
    assert!(reader.copied().is_empty());
    assert!(
        reader.state().notice.starts_with("Select something first"),
        "{}",
        reader.state().notice
    );
}

#[test]
fn a_quote_carries_the_page_it_came_from() {
    let mut reader = prose();
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.press_chord("mod+shift+c");
    let copied = reader.copied();
    let quote = copied.first().expect("something was copied");
    assert!(
        quote.contains("“A needle in the first page.”") && quote.ends_with("p. 1"),
        "{quote}"
    );
    assert!(
        reader.state().notice.contains("p. 1"),
        "{}",
        reader.state().notice
    );
}

#[test]
fn the_whole_page_can_be_selected() {
    let mut reader = prose();
    reader.press_chord("mod+a");
    assert_eq!(selected(&mut reader), "A needle in the first page.");
    // …and it is the page the reader is on, not the first one.
    let mut later = prose();
    go_to(&mut later, 3);
    later.press_chord("mod+a");
    assert_eq!(
        selected(&mut later),
        "The needle again, and a needle beside it."
    );
}

#[test]
fn a_second_click_takes_the_word_under_it() {
    // The one gesture in this reader that is a browser's convention rather
    // than the app's, and the app gets it from the webview for nothing.
    let mut reader = prose();
    // A fifth of the way across the line is inside "needle".
    reader.double_click_on(1, (0.20, LINE));
    assert_eq!(selected(&mut reader), "needle");
}

/// **A word taken by a double-click is not offered a colour**: it is most
/// often a word to copy or look up, and the popover sat over it.
#[test]
fn a_double_click_offers_no_colours() {
    let mut reader = prose();
    reader.double_click_on(1, (0.20, LINE));
    assert_eq!(selected(&mut reader), "needle");
    assert!(reader.harness.query(".markup-swatch").is_none());
}

#[test]
fn a_double_click_survives_the_twitch_before_the_release() {
    // A real mouse moves a pixel between the second press and its release,
    // and that move used to cut the word back to the letters before the
    // pointer.
    let mut reader = prose();
    reader.double_click_unsteadily_on(1, (0.20, LINE));
    assert_eq!(selected(&mut reader), "needle");
}

#[test]
fn a_third_click_takes_the_line() {
    let mut reader = prose();
    reader.triple_click_on(1, (0.20, LINE));
    assert_eq!(selected(&mut reader), "A needle in the first page.");
}

#[test]
fn escape_puts_the_selection_down() {
    let mut reader = prose();
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    assert!(painted(&reader) > 0);
    // Letting go of a sweep offers to mark it — see `tests/markup.rs` — and
    // the swatches are the outermost thing on screen, so the first Escape is
    // theirs. That is the same "outward, in the order the reader arrived"
    // every other Escape in this app follows.
    assert!(reader.harness.query(".markup-popover").is_some());
    reader.press("Escape");
    assert!(reader.harness.query(".markup-popover").is_none());
    assert!(
        painted(&reader) > 0,
        "and the selection is still there under it"
    );
    reader.press("Escape");
    assert_eq!(painted(&reader), 0);
    assert_eq!(selected(&mut reader), "");
}

#[test]
fn escape_takes_the_find_bar_before_the_selection() {
    // The order is outward, in the order the reader arrived at things: the bar
    // they opened last goes first, and the selection is still there under it.
    let mut reader = prose();
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    reader.press_chord("mod+f");
    reader.press("Escape");
    assert!(reader.state().find.is_none(), "the bar is still up");
    assert_eq!(selected(&mut reader), "A needle in the first page.");
    reader.press("Escape");
    assert_eq!(selected(&mut reader), "");
}

#[test]
fn the_selection_survives_a_zoom_and_a_turn() {
    // It is held as characters rather than as rectangles, which is the whole
    // reason: a match, a link and a selection are all in the page's own
    // unturned points and `place_on` is what meets the rotation. Nothing has
    // to be recomputed and nothing can drift.
    let mut reader = prose();
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    let before = selected(&mut reader);
    reader.press_chord("mod++");
    reader.press_chord("mod+r");
    assert_eq!(selected(&mut reader), before);
    assert!(painted(&reader) > 0, "the words are still marked");
}

#[test]
fn a_sweep_on_a_turned_page_selects_what_is_under_the_pointer() {
    // The pointer is the one thing that arrives in the wrong space, so a turn
    // is the case that says whether `unplace_on` really is `place_on`
    // backwards. Turned a quarter to the right, the line that ran across the
    // top of the page runs down its right-hand side.
    let mut reader = prose();
    reader.press_chord("mod+r");
    reader.sweep_page(1, (1.0 - LINE, 0.10), (1.0 - LINE, 0.55));
    assert_eq!(selected(&mut reader), "A needle in the first page.");
}

#[test]
fn a_document_with_no_text_says_so_rather_than_nothing() {
    let mut reader = Reader::open(&fixture::margins_pdf());
    reader.press_chord("mod+a");
    // Asserted before anything else is pressed: the notice line carries one
    // sentence and the next key overwrites it.
    assert!(
        reader.state().notice.contains("no text"),
        "{}",
        reader.state().notice
    );
    assert_eq!(selected(&mut reader), "");
}

#[test]
fn a_recompile_puts_the_selection_down() {
    // A selection is indices into a document, and a paper recompiled by LaTeX
    // is a different document — so a selection kept across one would be a
    // highlight over words nobody chose. (Markup is the case where a passage
    // *does* survive a rebuild, and it survives as a quote to be looked up
    // again rather than as a range. That is item 11.)
    let path = fixture::prose_pdf();
    let mut reader = Reader::open(&path);
    reader.sweep_page(1, (0.10, LINE), (0.55, LINE));
    assert!(painted(&reader) > 0);
    reader.document_changed(&path);
    assert_eq!(painted(&reader), 0);
    assert_eq!(selected(&mut reader), "");
}

#[test]
fn the_pages_kept_for_a_selection_are_capped() {
    // The one cache in `app.rs` that is bounded, because a page of text is a
    // hundred kilobytes and a book is four hundred pages. Sweeping through the
    // document must not accumulate them.
    let mut reader = Reader::open_with(&Reader::book(), Options::default());
    let cap = moonowl::app::TEXT_CACHE as u64;
    for page in 1..=cap as usize + 12 {
        go_to(&mut reader, page);
        reader.press_chord("mod+a");
    }
    let kept = stats::get(&stats::TEXT_PAGES);
    assert!(kept <= cap, "kept {kept} pages of text");
    assert!(kept > 0, "nothing was kept at all, so nothing was tested");
}

/// **The boxes of the type are where the type is drawn**, on a page the file
/// turns and crops. pdfium draws such a page right on its own; what it says
/// about the characters is in the file's space, and used to be flipped as
/// though the page were neither.
#[test]
fn the_text_of_a_turned_and_cropped_page_is_where_its_ink_is() {
    for rotate in [0, 90, 180, 270] {
        let document =
            moonowl::render::open(&moonowl::fixture::turned_pdf(rotate)).expect("it opens");
        let size = document.size_of(0);
        let (width, height) = (size.width.round() as u32, size.height.round() as u32);
        let text = document.text_of(0);
        assert!(!text.boxes.is_empty(), "{rotate}: no text");
        // One pixel a point, so a box is its own pixels. Every dark pixel of
        // the page has to be inside the boxes' union, and there have to be some.
        let (mut left, mut top, mut right, mut bottom) = (f64::MAX, f64::MAX, 0.0f64, 0.0f64);
        for glyph in &text.boxes {
            left = left.min(glyph.left);
            top = top.min(glyph.top);
            right = right.max(glyph.left + glyph.width);
            bottom = bottom.max(glyph.top + glyph.height);
        }
        let (mut inked, mut astray) = (0, 0);
        document
            .render(
                0,
                width,
                height,
                moonowl::layout::View::WHOLE,
                &mut |bitmap| {
                    for y in 0..bitmap.height {
                        for x in 0..bitmap.width {
                            if bitmap.bgra[((y * bitmap.width + x) * 4 + 1) as usize] > 100 {
                                continue;
                            }
                            let (x, y) = (x as f64, y as f64);
                            if x >= left - 2.0
                                && x <= right + 2.0
                                && y >= top - 2.0
                                && y <= bottom + 2.0
                            {
                                inked += 1;
                            } else {
                                astray += 1;
                            }
                        }
                    }
                },
            )
            .expect("the page draws");
        assert!(
            inked > 100,
            "{rotate}: only {inked} dark pixels under the text"
        );
        assert_eq!(
            astray, 0,
            "{rotate}: ink outside {left},{top}–{right},{bottom}"
        );
    }
}

/// **A sweep held against the bottom of the window scrolls on**, so a
/// passage longer than the screen can be selected in one gesture — the
/// document runs under the pointer and the selection runs with it.
#[test]
fn a_sweep_held_at_the_bottom_edge_scrolls_on() {
    let mut reader = prose();
    let (_, height) = reader.window();
    let (x, y, width, page) = reader.box_of(".page").expect("a page");
    let start = (x + width * 0.1, y + page * 0.108);
    reader.harness.mouse_down_at(start.0, start.1);
    // Into the last pixels of the window.
    let below = height as f32 - 2.0;
    reader.carry(x + width * 0.5, below);
    assert!(
        reader.wait_until(4.0, |reader| reader.state().page > 1),
        "the document ran under the pointer"
    );
    reader.harness.mouse_up_at(x + width * 0.5, below);
    reader.settle();
    let stopped = reader.state().scroll;
    assert!(
        !reader.wait_until(0.3, |reader| reader.state().scroll > stopped + 1.0),
        "and stopped when the button came up"
    );
    reader.press_action(Action::Copy);
    let copied = reader.copied().last().cloned().unwrap_or_default();
    assert!(copied.lines().count() > 1, "{copied:?}");
}
