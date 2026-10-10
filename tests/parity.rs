//! **The interface's measurements, frozen.**
//!
//! `tests/parity/app-inventory.json` was measured off the retired Tauri app,
//! in WebKit, and served as the spec for the port. That app is gone, so the
//! numbers are now simply what this interface *is*: control widths, row
//! heights, the start screen's layout, and every colour the chrome is built
//! from. Changing one is a decision, taken here.
//!
//! Only measurements are compared, never words: a label lives in the code
//! that draws it and nowhere else. The fixture still carries the old app's
//! labels; nothing reads them. A label that changes a control's width does
//! change a number here, because the width is the type.
//!
//! What this file cannot cover is anything about the *window* — dragging it,
//! full screen, the traffic lights.

use moonowl::harness::{Options, Reader};
use serde_json::Value;

/// Two pixels on macOS, where the fixture was taken, and a typeface's worth of
/// room anywhere else.
///
/// The numbers in `app-inventory.json` came out of WebKit on the machine this
/// was written on, where `ui-sans-serif` is SF Pro. A Windows runner resolves
/// that stack to Segoe UI and a Linux one to DejaVu Sans; the first sets the
/// same words a couple of per cent narrower and the second several per cent
/// wider, and no CSS makes a different typeface measure the same. So off macOS
/// the allowance is proportional — the error is in the *type*, so it grows
/// with how much of it is in the box: six pixels on a chip, ten on the start
/// screen's Open button, which carries the longest label in the interface.
/// What is still being asserted is that the arrangement is the app's.
fn slack(want: f64) -> f64 {
    if cfg!(target_os = "macos") {
        2.0
    } else {
        (want.abs() * 0.06).max(6.0)
    }
}

fn app() -> Value {
    let raw = include_str!("parity/app-inventory.json");
    serde_json::from_str(raw).expect("the app's inventory")
}

fn reader() -> Reader {
    // Wider than the fixture was taken at (1280): under 1400px the left
    // group's chips fold to their symbols, and what is measured here is the
    // words. A chip's width does not depend on the window's otherwise.
    Reader::open_with(
        &Reader::book(),
        Options {
            width: 1440,
            height: 860,
            system_font: cfg!(target_os = "macos"),
            ..Default::default()
        },
    )
}

/// **The toolbar is the size of the snapshot, control for control.**
///
/// This is the one measurement in the file, and it is here because it is the
/// only question that can see the *type*. A chip is its padding plus its icon
/// plus its word; the padding and the icon are numbers both readers already
/// agree on, so what is left over is the word, and the word is the font. The
/// port's every chip came out about five per cent narrow — `Contents` 96
/// against 101.3, `of 400` 38 against 41.5 — because parley does not read SF's
/// `trak` table and WebKit does, so the system font arrived with its
/// small-size tracking missing. Nothing that compares *labels* could have said
/// so, and what a reader saw was a bar that was tighter and darker than the
/// app's without being able to name what had changed. See `body` in
/// `styles.rs`.
///
/// Two pixels of tolerance, and it has to be that loose: the app's numbers
/// are fractional and Blitz rounds a box to whole pixels, so 101.3 can only
/// ever come back as 101.
#[test]
fn the_toolbar_keeps_its_widths() {
    let reader = reader();
    let app = app();
    // The middle group, whole. The two sides hold the same controls at the
    // same widths — which is what is asserted below — but where they *end* is
    // the flex arrangement, and that is deliberately not the app's: see
    // `.bar-left` in `styles.rs` on why the bases are nought here and `auto`
    // there.
    let want = app["toolbar"]["bar-center-box"]["width"]
        .as_f64()
        .expect("the middle group's width");
    let got = reader.width_of(".bar-center").expect("the middle group");
    assert!(
        (got - want).abs() <= slack(want),
        "the middle group is {got} wide and the snapshot's is {want}",
    );

    // Every control that carries a word, by the class this reader gives it.
    // The icon-only ones are left out on purpose: a square is a square in
    // both readers and says nothing about the type.
    // Not the sidebar's chip, which says "Sidebar" where the app's said
    // "Contents": a different word, so a different width.
    for (id, selector) in [
        ("close-doc", ".chip.close-doc"),
        ("doc-title", ".chip.title"),
        ("page-count", ".of"),
        ("find", ".chip.find"),
        ("rotate-left", ".chip.rotate-left"),
        ("rotate-right", ".chip.rotate-right"),
        ("zoom-level", ".chip.fit"),
    ] {
        let want = app["toolbar"]
            .as_object()
            .expect("the toolbar")
            .values()
            .filter_map(Value::as_array)
            .flatten()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(id))
            .and_then(|row| row.get("width"))
            .and_then(Value::as_f64)
            .unwrap_or_else(|| panic!("no {id} in the inventory"));
        // The name ends in sixteen pixels of padding where the app's had
        // eight: the room its fade is drawn in. See `.title-name`.
        let want = if id == "doc-title" { want + 8.0 } else { want };
        let got = reader
            .width_of(selector)
            .unwrap_or_else(|| panic!("no {selector}"));
        assert!(
            (got - want).abs() <= slack(want),
            "{id} is {got} wide and the snapshot's is {want}"
        );
    }
}

/// **And the surfaces that float or list are the size the app's are.**
///
/// The widths above catch the type in the toolbar; this catches it everywhere
/// else, and by height rather than by width for the same reason: a row is its
/// padding plus its line, both readers agree about the padding, so the height
/// is the type. Every one of these was a row shorter — a menu item 30 against
/// 35, a tab 26 against 28, a switch 23 against 20 — because the port wrote
/// its menus, its sidebar and its Settings window in the toolbar's 13.5 where
/// `.popover`, `#sidebar` and `.window` each say 14.5. Which is not a thing
/// any comparison of *labels* can see, and the reader who reported it could
/// only say that the port looked smaller.
#[test]
fn the_surfaces_keep_their_heights() {
    let app = app();
    let want = |name: &str| app["rows"][name].as_f64();

    // A menu, open. Theme is the one every reader sees.
    let mut menu = reader();
    menu.click(".chip.theme");
    for (name, selector) in [
        ("menu-item", ".menu-item"),
        ("menu-heading", ".menu-section"),
    ] {
        let Some(tall) = want(name) else { continue };
        assert!(menu.harness.query(selector).is_some(), "no {selector}");
        let got = menu.harness.layout_rect(selector).height as f64;
        assert!(
            (got - tall).abs() <= slack(tall),
            "{name} is {got} tall and the snapshot's is {tall}",
        );
    }

    // The sidebar's tab strip.
    let mut panel = reader();
    panel.press_chord("mod+b");
    let tab = panel.harness.layout_rect(".tab").height as f64;
    let wanted = want("tab").expect("the app's tab");
    assert!(
        (tab - wanted).abs() <= slack(wanted),
        "a tab is {tab} tall and the snapshot's is {wanted}"
    );

    // And the Settings window.
    let mut window = reader();
    window.press_chord("mod+,");
    for (name, selector) in [
        ("window-bar", ".window-bar"),
        ("nav-item", ".nav-item"),
        ("switch", ".switch"),
    ] {
        let Some(tall) = want(name) else { continue };
        let got = window.harness.layout_rect(selector).height as f64;
        assert!(
            (got - tall).abs() <= slack(tall),
            "{name} is {got} tall and the snapshot's is {tall}",
        );
    }
}

/// **Every colour the chrome is built from, against the app's own.**
///
/// `applyTheme` writes twenty-two custom properties onto the root and this
/// reader writes the same values under names of its own; the pairs are below.
/// They were all near-misses of the app's arithmetic — a surface pulled 6%
/// towards the ink where the app pulls it 55% towards white — which is the
/// kind of difference nobody can name from a screenshot and every one of
/// these catches.
///
/// Worn on the colours Moonowl Light had when the snapshot was taken, written
/// as a theme of the reader's own: the arithmetic is what is compared, and
/// Moonowl Light has its own creams since.
#[test]
fn the_theme_keeps_its_colours() {
    let dir = std::env::temp_dir().join(format!("moonowl-parity-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("themes")).expect("a themes directory");
    std::fs::write(
        dir.join("themes").join("snapshot.toml"),
        "name = \"Snapshot\"\ntext = \"#2f3237\"\nbackground = \"#f2f1ed\"\n\
         accent = \"#3f7d94\"\nselection_area = \"#eac8c2\"\n\
         selection_text = \"#2f3237\"\nrecolor = false\n",
    )
    .expect("the snapshot's theme");
    let reader = Reader::open_with(
        &Reader::book(),
        Options {
            width: 1440,
            height: 860,
            config: dir,
            // After the ones that ship.
            theme: Some(moonowl::theme::BUILT_IN.len()),
            ..Default::default()
        },
    );
    let app = app();
    let style = reader.harness.attr(".root", "style").unwrap_or_default();
    let of = |name: &str| {
        style
            .split(';')
            .filter_map(|entry| entry.split_once(':'))
            .find(|(key, _)| key.trim() == name)
            .map(|(_, value)| value.trim().to_string())
            .unwrap_or_else(|| panic!("no {name} on the root"))
    };
    // Not the bar's four: the app mixed them from the white paper a theme
    // that does not recolour prints on, and this reader from the theme's own
    // background, which is what the bar now stands on.
    for (theirs, ours) in [
        ("--bg", "--ground"),
        ("--surface", "--surface"),
        ("--surface-hover", "--hover"),
        ("--surface-sunk", "--sunk"),
        ("--line", "--line"),
        ("--text", "--text"),
        // Not "--text-faint", "--text-soft" or "--text-note": the app's were
        // under 3:1 and 4.5:1 on its own grounds, for words meant to be read,
        // and the port's come back towards the ink until they are not. See
        // `Palette::readable`.
        ("--accent", "--accent"),
        ("--accent-soft", "--accent-soft"),
        ("--accent-contrast", "--accent-contrast"),
        ("--positive", "--positive"),
        ("--negative", "--negative"),
        ("--negative-contrast", "--negative-contrast"),
        ("--page-paper", "--page"),
        ("--selection-area", "--found"),
        ("--selection-text", "--found-ink"),
    ] {
        let want = app["theme"][theirs].as_str().expect(theirs);
        assert_eq!(of(ours), want, "{ours} against the snapshot's {theirs}");
    }
}

/// **The start screen is laid out and coloured the way the snapshot is.**
///
/// The start screen's words all matched while it was wrong in three ways
/// at once, which is the argument for this test: a reader who looked at the
/// two side by side could say the button was too wide, the ground was the
/// wrong colour and the names in the shelf were spaced oddly, and nothing
/// that compares *words* could see any of it.
///
/// What each number here catches:
///
/// - *The ground.* `#welcome` is `--bg`, the shade a page floats on, because
///   the start screen stands where the document will. This was `--paper` —
///   the toolbar's own colour — so the window changed shade the moment
///   anything was opened, and the screen read as one flat panel with the bar.
/// - *The type.* `#welcome` sets 14.5px, the way `.popover`, `#sidebar` and
///   `.window` do; this was the body's 13.5, which is the same fault the
///   three of them had and this the fourth surface with it. The button's
///   width is what says so — padding plus icon plus gap plus the word — and
///   the shelf's row height is the other half, being padding plus a line.
/// - *The button.* 100% of the column against the app's 176: a band across
///   the screen where the app has a button the width of its own words.
///
/// The ground is read off the *pixels* rather than off a stylesheet, because
/// a colour named correctly and painted by something else on top is still the
/// wrong colour on screen. A point in the left margin, well clear of the
/// 460px column in the middle of it.
#[test]
fn the_start_screen_keeps_its_layout() {
    let mut reader = Reader::empty(Options {
        width: 1280,
        height: 860,
        system_font: cfg!(target_os = "macos"),
        ..Default::default()
    });
    let app = app();
    let start = &app["start"];

    // The fixture's own two answers agree, which is what makes the assertion
    // below a comparison and not a number written down twice.
    assert_eq!(
        start["background"], app["theme"]["--bg"],
        "the app's start screen is its `--bg`",
    );

    // The ground, though not the app's: Moonowl Light names its own now.
    let style = reader.harness.attr(".root", "style").unwrap_or_default();
    let want = style
        .split(';')
        .filter_map(|entry| entry.split_once(':'))
        .find(|(key, _)| key.trim() == "--ground")
        .map(|(_, value)| value.trim().to_string())
        .expect("the theme's ground");
    let shot = reader.screenshot();
    let pixel = shot.at(shot.width / 20, shot.height * 3 / 4);
    let got = format!("#{:02x}{:02x}{:02x}", pixel[0], pixel[1], pixel[2]);
    assert_eq!(got, want, "the ground the start screen stands on");

    for (name, selector) in [("open", ".start-open"), ("inner", ".start-inner")] {
        let box_of = |key: &str| start["boxes"][name][key].as_f64();
        let (_, _, width, height) = reader
            .box_of(selector)
            .unwrap_or_else(|| panic!("no {selector}"));
        // Width alone for the column, which is `min(460px, 82vw)` in both and
        // whose *height* is the sum of everything in it — a number that says
        // nothing on its own and would fail for whichever part of it moved.
        let wanted = box_of("width").expect("a width");
        // The same two pixels every other measured control gets. It was three
        // for a while, to carry the 2.9px the flat `letter-spacing` left on
        // this button — the longest label in the interface, so the one that
        // showed it worst. Setting the font's optical size instead took that
        // to 0.1px and the allowance went back with it.
        assert!(
            (width as f64 - wanted).abs() <= slack(wanted),
            "{name} is {width} wide and the snapshot's is {wanted}",
        );
        if let Some(wanted) = box_of("height").filter(|_| name == "open") {
            assert!(
                (height as f64 - wanted).abs() <= slack(wanted),
                "{name} is {height} tall and the snapshot's is {wanted}",
            );
        }
    }
}

/// **A row of the recents shelf is the height of the app's.**
///
/// Separate from the test above because it needs a shelf, and a shelf needs a
/// library: `Reader::empty` opens on a fresh config directory with nothing
/// read yet. The app's own number is measured rather than stated — see the
/// probe in `take-inventory.mjs` — because the browser fallback's `bootstrap`
/// hands the harness no library at all and the row therefore has to be built
/// out of the app's own cascade.
///
/// It was 34 against 37, which is 8px of padding either side of a line of
/// 1.45 × 13.5 where the app has 1.45 × 14.5. Six rows of it is a shelf a
/// fifth of a row short, which is exactly the sort of difference a reader
/// notices and cannot name.
#[test]
fn a_recents_row_keeps_its_height() {
    let app = app();
    let want = app["start"]["boxes"]["recent"]["height"]
        .as_f64()
        .expect("the app's row");
    // A config directory of its own, a document read in it, and the document
    // put down again — which is the only way to a shelf with something on it,
    // there being no seam for seeding one.
    let dir = std::env::temp_dir().join(format!("moonowl-parity-shelf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            width: 1280,
            height: 860,
            config: dir,
            system_font: cfg!(target_os = "macos"),
            ..Default::default()
        },
    );
    reader.click("[data-item=\"close-document\"]");
    let (_, _, _, height) = reader.box_of(".recent").expect("no shelf");
    assert!(
        (height as f64 - want).abs() <= slack(want),
        "a row is {height} tall and the snapshot's is {want}",
    );
}

/// **The two large areas of the interface, by the colour actually on them.**
///
/// The twenty-two above are compared name for name and every one of them
/// matched while the start screen stood on the wrong one: `--paper`, the
/// toolbar's own colour, where the app has `--bg`. A whole window of the
/// wrong shade, and nothing that reads a variable could see it — the values
/// were right and what was painted with them was not.
///
/// So these two are read off the pixels. The bar with a document behind it,
/// and the ground the start screen stands on, which is asserted where the
/// start screen is. Between them they are nearly all of the app that is not
/// a page.
///
/// The bar is not the app's white: it stands on the theme's background, the
/// paper a theme that does not recolour leaves to the pages.
#[test]
fn the_chrome_keeps_its_colours() {
    let mut reader = reader();
    let style = reader.harness.attr(".root", "style").unwrap_or_default();
    let want = style
        .split(';')
        .filter_map(|entry| entry.split_once(':'))
        .find(|(key, _)| key.trim() == "--paper")
        .map(|(_, value)| value.trim().to_string())
        .expect("the theme's background");
    let (_, y, _, height) = reader.box_of(".toolbar").expect("no bar");
    let shot = reader.screenshot();
    // Two pixels in from the left edge: the bar's own padding is ten, so this
    // is bar and nothing else whatever is on it.
    let scale = shot.height as f32 / 860.0;
    let pixel = shot.at(2, ((y + height / 2.0) * scale) as u32);
    let got = format!("#{:02x}{:02x}{:02x}", pixel[0], pixel[1], pixel[2]);
    assert_eq!(got, want, "the toolbar");
}

/// **The recolouring, against the app's own — pixel for pixel.**
///
/// `recolor.rs` calls itself a faithful port of `recolorByPixel` and
/// `tests/recolor.rs` holds the shader to it, but the thing it is faithful
/// *to* was never in either comparison: both sides could have been wrong
/// together and the only place it would show is a document. This closes that,
/// which makes it the one test in the file that is about a page rather than
/// about the chrome.
///
/// The fixture is written by `take-recolor.mjs`, which runs the app's own
/// function in WebKit over 525 pixels picked to reach every branch of it — the
/// whole grey ramp, saturated colour, the near-neutrals either side of
/// `COLOUR_FLOOR`, the washes above `WHITE_POINT`, and a few colours a
/// plotting library actually emits.
///
/// Two ramps, named by what wants them. `duotone` is the one a **link** takes,
/// in Moonowl Light's real copper on the white a page that is not recoloured is
/// printed on — so this is also what says the port paints a cross-reference
/// the colour the app paints it. `recolor` is a page put onto a dark theme
/// with the colours on it kept.
///
/// One level out of 255, which is the tolerance the app already holds its own
/// two paths to and the port already holds its two to.
#[test]
fn the_recolouring_is_the_app_s() {
    let raw = include_str!("parity/recolor-fixture.json");
    let fixture: Value = serde_json::from_str(raw).expect("the app's recolouring");
    let bytes = |hex: &str| -> Vec<u8> {
        (0..hex.len() / 2)
            .map(|at| u8::from_str_radix(&hex[at * 2..at * 2 + 2], 16).expect("a byte"))
            .collect()
    };
    let colour = |hex: &str| -> [u8; 3] {
        let bytes = bytes(hex.trim_start_matches('#'));
        [bytes[0], bytes[1], bytes[2]]
    };
    let pixels = bytes(fixture["pixels"].as_str().expect("the page"));

    for ramp in fixture["ramps"].as_array().expect("the ramps") {
        let name = ramp["name"].as_str().unwrap_or_default();
        let want = bytes(ramp["out"].as_str().expect("what the app made of it"));
        let mut got = pixels.clone();
        moonowl::recolor::recolor_cpu(
            &mut got,
            colour(ramp["text"].as_str().expect("ink")),
            colour(ramp["bg"].as_str().expect("paper")),
            ramp["keepColour"]
                .as_bool()
                .expect("whether colour is kept"),
        );

        let (worst, at) = want.iter().zip(&got).enumerate().fold(
            (0i32, 0usize),
            |(worst, at), (index, (a, b))| {
                let off = (*a as i32 - *b as i32).abs();
                if off > worst {
                    (off, index)
                } else {
                    (worst, at)
                }
            },
        );
        let pixel = at / 4 * 4;
        assert!(
            worst <= 1,
            "{name}: off by {worst} levels at pixel {} — in {:?}, the app {:?}, this reader {:?}",
            at / 4,
            &pixels[pixel..pixel + 4],
            &want[pixel..pixel + 4],
            &got[pixel..pixel + 4],
        );
    }
}
