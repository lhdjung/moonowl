//! What actually lands on the screen.
//!
//! This is the half the app's own harness never had. `recolor.test.mjs` tests
//! the ramp against a reference and `reader.test.mjs` tests that the interface
//! moves; nothing on either side could say that a page was *drawn*, because a
//! canvas in a headless WebKit is a canvas nobody looks at. Here the whole
//! window is rasterised on the CPU and the pixels are read back.
//!
//! The assertions are about measurable properties rather than about a
//! reference PNG, and that is a decision rather than a shortcut — see
//! `PROGRESS.md`. A reference image is only as portable as the fonts that went
//! into it, and the toolbar is drawn in whatever `ui-sans-serif` resolves to
//! on the machine.

use moonowl::harness::{Options, Reader};
use moonowl::palette;
use moonowl::recolor;
use moonowl::theme;

/// Moonowl Dark, as the app's own theme file defines it. The list is fifteen
/// long now and read off the app's `themes/` directory, so a test that wants
/// *the dark one* asks for it by id rather than by a place in an array.
fn moonowl_dark() -> (usize, palette::Palette) {
    let index = theme::BUILT_IN
        .iter()
        .position(|(id, _)| *id == theme::DEFAULT_DARK)
        .expect("Moonowl Dark ships");
    let parsed: theme::Theme =
        toml::from_str(theme::BUILT_IN[index].1).expect("Moonowl Dark parses");
    (index, palette::resolve(&parsed, true))
}

/// The rectangle a page occupies, in device pixels, pulled in a little so
/// that a shadow or a rounding does not land in the sample.
fn page_rect(reader: &Reader) -> (u32, u32, u32, u32) {
    let rect = reader.harness.layout_rect(".page");
    (
        rect.x as u32 + 8,
        rect.y as u32 + 8,
        (rect.x + rect.width) as u32 - 8,
        (rect.y + rect.height) as u32 - 8,
    )
}

#[test]
fn a_page_is_drawn_where_the_layout_puts_it() {
    let mut reader = Reader::open(&Reader::book());
    let page = page_rect(&reader);
    let shot = reader.screenshot();

    // The paper is the page's own white, and it is not the ground the page
    // stands on. Two samples: one inside the page, one in the margin above it
    // — which is where the ground shows at fit width, the page having reached
    // both sides.
    let inside = shot.at(page.0 + 40, page.1 + 40);
    let above = shot.at(page.0 + 40, page.1 - 14);
    assert!(
        inside[0] > 240 && inside[1] > 240 && inside[2] > 240,
        "the page is paper: {inside:?}"
    );
    assert!(
        above[0] < 240,
        "and the ground it stands on is not: {above:?}"
    );
}

#[test]
fn there_is_ink_on_the_page() {
    let mut reader = Reader::open(&Reader::book());
    let page = page_rect(&reader);
    let shot = reader.screenshot();
    // The fixture is one line of text near the top of each page, so the band
    // that holds it is where to look — and the rest of the page is paper,
    // which is why this is a band and not the whole page.
    let band = (page.0, page.1 + 100, page.2, page.1 + 200);
    let ink = shot.unlike([255, 255, 255], band);
    assert!(
        ink > 0.01,
        "a page with text on it has ink: {:.4} of the band",
        ink
    );
}

#[test]
fn a_recolouring_theme_reaches_the_page() {
    let light = {
        let mut reader = Reader::open(&Reader::book());
        let page = page_rect(&reader);
        reader.screenshot().mean(page)
    };
    let dark = {
        let mut reader = Reader::open_with(
            &Reader::book(),
            Options {
                theme: Some(moonowl_dark().0),
                ..Default::default()
            },
        );
        let page = page_rect(&reader);
        reader.screenshot().mean(page)
    };
    assert!(light[0] > 200.0, "a light page is paper: {light:?}");
    assert!(
        dark[0] < 80.0,
        "and a dark one is not — the recolouring is on the pixels, not on the CSS: {dark:?}"
    );
    assert!(
        moonowl_dark().1.recolor,
        "…which is what Moonowl Dark asks for"
    );
}

#[test]
fn the_ink_survives_the_theme() {
    // The whole argument of the ramp is that a dark theme is not a blackout:
    // paper becomes ink and ink becomes paper, so the *contrast* in the band
    // that holds the text is preserved.
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            theme: Some(moonowl_dark().0),
            ..Default::default()
        },
    );
    let page = page_rect(&reader);
    let shot = reader.screenshot();
    let band = (page.0, page.1 + 100, page.2, page.1 + 200);
    let paper = shot.mean((page.0, page.3 - 100, page.2, page.3));
    let ink = shot.unlike([paper[0] as u8, paper[1] as u8, paper[2] as u8], band);
    assert!(
        ink > 0.01,
        "the letters are still there, in the other colour: {ink:.4}"
    );
}

#[test]
fn the_theme_reaches_the_chrome_too() {
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            theme: Some(moonowl_dark().0),
            ..Default::default()
        },
    );
    let shot = reader.screenshot();
    let bar = shot.mean((0, 0, 1100, 40));
    let paper = moonowl_dark().1.background;
    for channel in 0..3 {
        assert!(
            (bar[channel] - paper[channel] as f64).abs() < 24.0,
            "the toolbar wears the theme's paper: {bar:?} against {paper:?}"
        );
    }
}

#[test]
fn scrolling_changes_what_is_drawn() {
    let mut reader = Reader::open(&Reader::book());
    let first = reader.screenshot();
    for _ in 0..4 {
        reader.wheel_screen();
    }
    let later = reader.screenshot();
    let mut different = 0u64;
    for at in (0..first.rgba.len()).step_by(4 * 37) {
        if first.rgba[at] != later.rgba[at] {
            different += 1;
        }
    }
    assert!(
        different > 100,
        "four screenfuls later the window is not the same picture: {different}"
    );
}

/* ---------------------------------------------------- a zoom, under a pinch */

/// The size a page's texture is drawn at, off the page's own attribute.
fn drawn_of(reader: &Reader) -> String {
    reader
        .harness
        .attr(".page", "data-drawn")
        .expect("a mounted page says what it is drawn at")
}

/// **A pinch is one gesture, not a hundred zoom steps.**
///
/// A trackpad sends a magnification event a frame. Each one changes the size
/// every page is laid out at, and a page whose size changed is a page whose
/// texture is registered afresh — which cannot be *drawn* until the frame
/// after it is registered, see `fresh` in `page.rs`. So the document went
/// blank for the whole of a pinch and came back when the fingers stopped,
/// which is what the reader saw and reported.
///
/// What holds it together is that the drawn size is frozen for the length of
/// the gesture and the page is stretched to whatever the layout asks for.
/// That is the pair this checks: the box grows, the drawn size does not.
#[test]
fn a_pinch_stretches_the_page_it_has_rather_than_drawing_a_new_one() {
    let mut reader = Reader::open_with(&Reader::book(), Options::default());
    let before_box = reader.harness.layout_rect(".page").width;
    let before_drawn = drawn_of(&reader);

    for _ in 0..6 {
        reader.pinch(0.05);
    }
    let after_box = reader.harness.layout_rect(".page").width;
    let after_drawn = drawn_of(&reader);

    assert!(
        after_box > before_box + 1.0,
        "the page grew under the fingers: {before_box} → {after_box}",
    );
    assert_eq!(
        after_drawn, before_drawn,
        "and it is the same texture, stretched, rather than six new ones",
    );
}

/// …and when the fingers lift, it is drawn again at the size it now is.
///
/// **The fingers lifting, not a gap in the stream.** The settle timer used to
/// end a pinch, and two fingers resting on the trackpad send nothing — so a
/// pause mid-gesture redrew every page at the size the pause was at, and
/// again when the fingers lifted. macOS does say when a pinch ends, as the
/// phase on the event, and that is what ends it now; the timer is for the
/// ⌃-wheel, which has no phase. See `Viewer::end_pinch`.
#[test]
fn and_when_the_fingers_stop_the_page_is_drawn_at_the_size_it_reached() {
    let mut reader = Reader::open_with(&Reader::book(), Options::default());
    for _ in 0..6 {
        reader.pinch(0.05);
    }
    let held = drawn_of(&reader);
    let grown = reader.harness.layout_rect(".page").width;

    let by_timer = reader.wait_until(1.0, |reader| drawn_of(reader) != held);
    assert!(!by_timer, "a pause is not the end of a pinch");
    reader.pinch_ended();
    let settled = reader.wait_until(3.0, |reader| drawn_of(reader) != held);
    assert!(settled, "the fingers lifted and the page was drawn again");
    let (width, _) = drawn_of(&reader)
        .split_once('x')
        .map(|(w, h)| (w.to_string(), h.to_string()))
        .expect("the attribute is <width>x<height>");
    let width: f64 = width.parse().expect("a number");
    assert!(
        (width - grown as f64).abs() <= 1.0,
        "drawn at {width} for a box of {grown}",
    );
}

/// **A selected passage is the theme's own two colours, not a wash over the
/// printed ones.**
///
/// This is `paintSelection` in `viewer.ts` and the reason it exists: the
/// obvious way to colour selected text is a translucent rectangle over the
/// words, and it leaves the letters the colour they were printed in — so on a
/// dark theme a selected sentence came out as the page's near-white type
/// showing pink through a maroon wash. The pixels under each line are run
/// through the luminance ramp instead, ink to `selection_text` and paper to
/// `selection_area`, so the band is the theme's ground and the words on it are
/// the theme's ink.
///
/// Moonowl Dark on purpose: a recoloured dark page is already light ink on dark
/// paper, so the darkest pixel in a run is its *paper* and the ramp has to go
/// the other way round. That is the branch `selection_ramp` exists for, and the
/// one a light theme would not exercise.
#[test]
fn a_selected_line_is_painted_in_the_theme_s_selection_colours() {
    let (index, theme) = moonowl_dark();
    let mut reader = Reader::open_with(
        &moonowl::fixture::prose_pdf(),
        Options {
            theme: Some(index),
            ..Options::default()
        },
    );
    // The one line of a `prose_pdf` page: 18pt type with its baseline at 700 on
    // a 792-point page, so about a tenth of the way down.
    reader.sweep_page(1, (0.10, 0.108), (0.55, 0.108));
    let band = reader.harness.layout_rect(".selected");
    assert!(band.width > 10.0 && band.height > 4.0, "{band:?}");

    // **What the ground under the words should be, worked out rather than
    // guessed.** The ramp's ends are luma 0 and the white point, and the paper
    // of a page Moonowl Dark has already recoloured is neither: it is the theme's
    // background, luma about 40, which lands a sixth of the way along rather
    // than at the end. So the expected colour is the ramp entry for that level
    // — the same table `duotone_cpu` builds, asked for one row.
    let luma = |colour: palette::Rgb| {
        ((colour[0] as u32 * 77 + colour[1] as u32 * 151 + colour[2] as u32 * 28 + 128) >> 8)
            as usize
    };
    // Moonowl Dark recolours and its ink is lighter than its paper, so the ramp
    // runs the other way round: the darkest pixel in the run is the page's
    // *paper*. See `PageWidget::selection_ramp`.
    let ramp = recolor::Tables::new(theme.selection_area, theme.selection_text, false).ramp;
    let ground = ramp[luma(theme.background)];
    let printed = ramp[luma(theme.text)];
    assert_ne!(ground, printed, "the two ends of the band are not the same");

    let shot = reader.screenshot();
    let near = |a: [u8; 4], b: [u8; 3]| {
        (0..3).all(|channel| (a[channel] as i32 - b[channel] as i32).abs() <= 3)
    };
    // The ground between the letters is most of the band. Sampled across it
    // rather than at one point, because one point can land on a stem.
    let mut on_ground = 0;
    let mut sampled = 0;
    for step in 1..20 {
        let x = (band.x + band.width * step as f32 / 20.0) as u32;
        let y = (band.y + band.height * 0.25) as u32;
        sampled += 1;
        if near(shot.at(x, y), ground) {
            on_ground += 1;
        }
    }
    assert!(
        on_ground * 2 > sampled,
        "only {on_ground} of {sampled} samples across the band were the \
         theme's selection ground {ground:?} — the words are still wearing a \
         wash rather than the ramp",
    );
}

/// **A theme change redraws the pages in place.** With the colours in the
/// page's key, ⌘D made every page a new node with nothing to show until
/// pdfium had drawn it again — the whole window blank for a moment.
#[test]
fn a_theme_change_keeps_the_pages_it_redraws() {
    let mut reader = Reader::open(&Reader::book());
    let before = reader.harness.query(".page");
    reader.press_chord("mod+d");
    assert_eq!(reader.harness.query(".page"), before);
}
