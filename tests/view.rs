//! Trimming the margins and turning the page — Phase 3 item 6, the two halves
//! of it that are about the *page* rather than about the window.
//!
//! Both are asked of the interface. What a reader would check is the shape of
//! what is in front of them and where the ink sits in it, so that is what is
//! asserted: the page's box, the ink in a screenshot, and a link's rectangle
//! following the page it is drawn on.
//!
//! `fixture::margins_pdf` is the document, because the arithmetic has to be
//! checkable — a black rectangle at [`fixture::INK`] on three otherwise empty
//! pages, its ink box the same on every one of them.

use moonowl::fixture;
use moonowl::harness::{Options, Reader};

/// The page's box on screen, as (width, height) in CSS pixels.
fn page_shape(reader: &Reader) -> (f32, f32) {
    let rect = reader.harness.layout_rect(".page");
    (rect.width, rect.height)
}

/// How tall the page is against how wide, which is the one number a trim and
/// a turn both move and the window size does not.
fn page_ratio(reader: &Reader) -> f64 {
    let (width, height) = page_shape(reader);
    height as f64 / width as f64
}

/// A strip across the middle of the page, in device pixels, pulled in from
/// the edges.
///
/// The middle rather than the whole page for one reason: `.page` carries a
/// drop shadow, so the outermost pixels of its box are not the paper and a
/// band that includes them finds "ink" in its first column. The ink on this
/// fixture runs from a tenth down to nine tenths, so the middle of the page
/// is inside it whatever the crop.
fn middle_strip(reader: &Reader) -> (u32, u32, u32, u32) {
    let rect = reader.harness.layout_rect(".page");
    let window = reader.window();
    // The middle of what is *on screen*, not the middle of the page: a page
    // fitted to the width is usually taller than the window, and a trimmed
    // one is taller still.
    let top = rect.y.max(0.0);
    let bottom = (rect.y + rect.height).min(window.1 as f32);
    let middle = (top + bottom) / 2.0;
    (
        rect.x as u32 + 3,
        (middle - (bottom - top) * 0.04) as u32,
        (rect.x + rect.width) as u32 - 3,
        (middle + (bottom - top) * 0.04) as u32,
    )
}

fn margined() -> Reader {
    Reader::open(&fixture::margins_pdf())
}

/// What the ink box becomes once [`moonowl::crop`] has padded it: the
/// same arithmetic the module does, restated here so that a change to `PAD`
/// shows up as a failure rather than as a test that agrees with whatever the
/// code now says.
fn expected_crop() -> (f64, f64, f64, f64) {
    let (left, top, right, bottom) = fixture::INK;
    let pad = moonowl::crop::PAD;
    (
        left - pad,
        top - pad,
        right - left + pad * 2.0,
        bottom - top + pad * 2.0,
    )
}

/// Throw the "Trim the margins" switch, which is the whole of the interface
/// for it.
///
/// **The toolbar used to carry a Trim chip and does not any more.** The app
/// has never had one, and the reason is what the chip was: trimming is
/// something somebody turns on for a scanned book and leaves on, not a thing
/// pressed twice in an hour — and a permanent button for it took a place in
/// the bar that the page controls wanted. It is the fourth field on the
/// Reading page and the first switch on it.
fn trim(reader: &mut Reader) {
    reader.press_chord("mod+,");
    reader.click(".switch");
    reader.press("Escape");
    reader.settle();
}

/// Whether the switch is on, asked of the window it lives in.
fn trimming(reader: &mut Reader) -> bool {
    reader.press_chord("mod+,");
    let on = reader.harness.attr(".switch", "aria-checked").as_deref() == Some("true");
    reader.press("Escape");
    reader.settle();
    on
}

#[test]
fn trimming_changes_the_shape_of_the_page_and_putting_them_back_restores_it() {
    let mut reader = margined();
    let whole = page_ratio(&reader);
    // 792 by 612, which is the page as the document has it.
    assert!((whole - 792.0 / 612.0).abs() < 0.02, "{whole}");

    trim(&mut reader);

    let (_, _, width, height) = expected_crop();
    let trimmed = page_ratio(&reader);
    let want = (792.0 * height) / (612.0 * width);
    // A percent of slack, because the ink box is measured off a page drawn a
    // hundred and sixty pixels wide and an edge lands between two of them.
    assert!(
        (trimmed / want - 1.0).abs() < 0.02,
        "trimmed to {trimmed}, expected about {want}"
    );
    assert!(
        trimmed > whole,
        "this document's margins are wider than tall"
    );
    assert!(trimming(&mut reader), "the switch is on");
    assert!(
        reader.state().notice.contains("trimmed"),
        "{:?}",
        reader.state().notice
    );

    trim(&mut reader);
    assert!((page_ratio(&reader) - whole).abs() < 0.001);
    assert!(!trimming(&mut reader), "and off again");
}

#[test]
fn a_trimmed_page_puts_its_ink_where_its_margins_were() {
    // The pixels, not the layout: trimming is a promise about what the reader
    // sees, and a page laid out at the right shape with the wrong part of the
    // document drawn on it is exactly the failure a crop can have.
    let mut reader = margined();
    let rect = reader.harness.layout_rect(".page");
    let band = middle_strip(&reader);
    let shot = reader.screenshot();
    let before = shot.leftmost_ink(band).expect("there is ink on the page");
    let inset_before = (before - band.0) as f64 / rect.width as f64;
    assert!(
        (inset_before - fixture::INK.0).abs() < 0.02,
        "the ink starts a fifth of the way in: {inset_before}"
    );

    trim(&mut reader);
    let rect = reader.harness.layout_rect(".page");
    let band = middle_strip(&reader);
    let shot = reader.screenshot();
    let after = shot.leftmost_ink(band).expect("there is still ink on it");
    let inset_after = (after - band.0) as f64 / rect.width as f64;
    // Everything but the pad has come off, and the pad is what is left.
    let pad = moonowl::crop::PAD / expected_crop().2;
    assert!(
        (inset_after - pad).abs() < 0.03,
        "the ink now starts at the padding: {inset_after}, expected about {pad}"
    );
}

#[test]
fn the_trim_switch_is_remembered_and_the_crop_is_not() {
    let mut reader = margined();
    trim(&mut reader);
    let trimmed = page_ratio(&reader);

    // The same config directory, which is what makes this a second run of the
    // same reader rather than a second reader.
    let mut again = Reader::open_with(
        &fixture::margins_pdf(),
        Options {
            config: reader.config.clone(),
            ..Options::default()
        },
    );
    again.settle();
    assert!(trimming(&mut again), "the switch came back on");
    // Measured again on this document rather than restored from the last one:
    // the answer is the same because the document is, which is the whole
    // reason the crop is not written down.
    assert!((page_ratio(&again) - trimmed).abs() < 0.001);
}

#[test]
fn turning_the_page_turns_the_document_and_four_quarters_is_where_it_started() {
    let mut reader = margined();
    let upright = page_ratio(&reader);

    reader.press_chord("mod+r");
    reader.settle();
    let sideways = page_ratio(&reader);
    assert!(
        (sideways * upright - 1.0).abs() < 0.02,
        "a page on its side is as much wider as it was taller: {upright} then {sideways}"
    );
    assert_eq!(reader.state().notice, "Turned 90°");

    for _ in 0..3 {
        reader.press_chord("mod+r");
    }
    reader.settle();
    assert!((page_ratio(&reader) - upright).abs() < 0.001);
    assert_eq!(reader.state().notice, "Upright");

    // And the other way round is the same journey backwards.
    reader.press_chord("mod+l");
    reader.settle();
    assert_eq!(reader.state().notice, "Turned 270°");
}

/// **A turn stays with the window, not with the document.** It is the
/// reader's way of looking, like the fit and the zoom beside it, so a document
/// opened into a turned window is turned — and none of it is written down, so
/// the next run is upright. See `Layout::rotation`.
#[test]
fn a_document_opened_into_a_turned_window_is_turned() {
    let mut reader = margined();
    let upright = page_ratio(&reader);
    reader.press_chord("mod+r");
    reader.settle();

    reader.hand_over(&fixture::prose_pdf());
    reader.settle();
    let sideways = page_ratio(&reader);
    assert!(
        (sideways * upright - 1.0).abs() < 0.02,
        "the new document came up upright: {upright} then {sideways}"
    );
}

#[test]
fn a_turned_page_is_drawn_turned_and_not_merely_laid_out_that_way() {
    let mut reader = margined();
    reader.press_chord("mod+r");
    reader.settle();

    let rect = reader.harness.layout_rect(".page");
    let band = middle_strip(&reader);
    let shot = reader.screenshot();
    // The ink was a fifth in from the left and a tenth down. Turned a quarter
    // clockwise, the tenth is what is now on the left — so the ink starts
    // nearer the left edge than it did, and a page drawn upright into a box
    // laid out sideways would put it at a fifth still.
    let ink = shot.leftmost_ink(band).expect("there is ink on the page");
    let inset = (ink - band.0) as f64 / rect.width as f64;
    assert!(
        (inset - fixture::INK.1).abs() < 0.03,
        "what was the top margin is now the left one: {inset}"
    );
}

#[test]
fn a_link_follows_the_page_it_is_drawn_on() {
    // The one thing a rotation could plausibly get right in the picture and
    // wrong everywhere else: a link is a rectangle nobody re-measures, and it
    // has to turn with the page it is over.
    let mut reader = Reader::open(&fixture::links_pdf());
    let before = reader.harness.layout_rect(".link");
    assert!(
        before.width > before.height,
        "the link is a wide, short box"
    );

    reader.press_chord("mod+r");
    reader.settle();
    let after = reader.harness.layout_rect(".link");
    assert!(
        after.height > after.width,
        "and it lies the other way round once the page has turned: {after:?}"
    );
    // The same rectangle, transposed: what was 128 by 20 points is now 20 by
    // 128, at the same scale.
    assert!(
        ((before.width / before.height) - (after.height / after.width)).abs() < 0.05,
        "{before:?} then {after:?}"
    );
}

#[test]
fn a_document_can_be_turned_and_trimmed_at_once() {
    let mut reader = margined();
    trim(&mut reader);
    let trimmed = page_ratio(&reader);

    reader.press_chord("mod+r");
    reader.settle();
    let both = page_ratio(&reader);
    assert!(
        (both * trimmed - 1.0).abs() < 0.02,
        "the trimmed page, on its side: {trimmed} then {both}"
    );

    // And trimming after turning finds the same crop, because the measurement
    // is made on the document rather than on the page as the reader has it.
    let mut other = margined();
    other.press_chord("mod+r");
    trim(&mut other);
    assert!(
        (page_ratio(&other) - both).abs() < 0.01,
        "turn then trim is trim then turn: {} against {both}",
        page_ratio(&other)
    );
}
