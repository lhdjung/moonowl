//! What the window looks like when it is not being asked anything: where the
//! page sits in it, what colour it is before it has been drawn, and what
//! colour the toolbar's own labels are.
//!
//! Grievances from reading with it, and every one is the sort of thing a test
//! suite that asks "does it work" will never raise: a correct answer badly
//! placed, badly coloured, unreachable, or computed against a window that had
//! stopped being the window.

use moonowl::fixture;
use moonowl::harness::{Options, Reader};
use moonowl::keymap::Action;
use moonowl::theme;

fn book() -> Reader {
    Reader::open(&Reader::book())
}

/// A shipped theme, by the id it is named by rather than by where it happens
/// to sit in the list.
fn shipped(id: &str) -> usize {
    theme::BUILT_IN
        .iter()
        .position(|(name, _)| *name == id)
        .unwrap_or_else(|| panic!("{id} ships"))
}

/// A reader wearing one, with the fixture open.
fn wearing(id: &str) -> Reader {
    Reader::open_with(
        &Reader::book(),
        Options {
            theme: Some(shipped(id)),
            ..Options::default()
        },
    )
}

/// How much ground there is either side of the page — the two numbers that
/// have to agree for a page to be centred.
fn margins(reader: &Reader) -> (f32, f32) {
    let page = reader.harness.layout_rect(".page");
    let viewer = reader.harness.layout_rect(".viewer");
    (
        page.x - viewer.x,
        (viewer.x + viewer.width) - (page.x + page.width),
    )
}

#[test]
fn a_page_narrower_than_the_window_stands_in_the_middle_of_it() {
    let mut reader = book();
    reader.press_action(Action::FitPage);
    let (left, right) = margins(&reader);
    assert!(left > 10.0, "there is ground either side of it: {left}");
    assert!((left - right).abs() <= 1.0, "{left} against {right}");
}

#[test]
fn a_page_wider_than_the_window_is_centred_and_can_be_reached() {
    let mut reader = book();
    reader.press_action(Action::ActualSize);
    // Five steps of the app's own ladder — 110, 125, 150, 175, 200 — which is
    // one more than it was here until `ZOOMS` got the three rungs it had been
    // missing. See `ZOOM_LADDER` in `main.ts`.
    for _ in 0..5 {
        reader.press_chord("mod+=");
    }
    assert_eq!(reader.state().zoom, "200%");

    // Wider than the window, and hanging out of it by the same amount at
    // both ends. **It used to hang out of the right alone**, pinned twenty
    // pixels from the left with the rest of it off the screen and no way to
    // scroll there: `#viewer` in the app is `overflow: auto` and `#pages` is
    // `margin: 0 auto`, and Blitz has neither.
    let (left, right) = margins(&reader);
    assert!(left < -40.0, "the page is wider than the window: {left}");
    assert!((left - right).abs() <= 1.0, "{left} against {right}");

    // And the far edge can be brought into view.
    reader.wheel_across(200.0);
    let (panned_left, panned_right) = margins(&reader);
    assert!(panned_left < left - 100.0, "{left} -> {panned_left}");
    assert!(panned_right > right + 100.0, "{right} -> {panned_right}");

    // Zooming back out to something that fits puts it back in the middle
    // rather than leaving it where the pan left it.
    reader.press_action(Action::FitPage);
    let (left, right) = margins(&reader);
    assert!((left - right).abs() <= 1.0, "{left} against {right}");
}

#[test]
fn a_window_that_changes_size_lays_the_document_out_again() {
    // **The one fault behind two complaints**: the page not centred, and Fit
    // width fitting a width the window no longer had. Blitz answers
    // `SurfaceResized` by moving its own viewport and asking for a redraw, and
    // tells nobody — so the chrome followed the window and `Viewer::layout`
    // kept the viewport it was handed when the window was mounted. A window
    // opened at 1100 and dragged to 1600 laid its pages out for 1100 inside a
    // `.viewer` that was now 1600: the page centred in a `.pages` box narrower
    // than the window, which is a page against the left of the screen.
    //
    // See `Shell::on_resized` and the `window-resized` arm in `app.rs`, which
    // are the two halves of the wire this drives.
    let mut reader = book();
    reader.press_chord("mod+0");
    let (left, right) = margins(&reader);
    assert!((left - right).abs() <= 1.0, "{left} against {right}");

    reader.resize(1600, 1000);
    let viewer = reader.harness.layout_rect(".viewer");
    assert!(
        viewer.width > 1500.0,
        "the window is wider: {}",
        viewer.width
    );
    let page = reader.harness.layout_rect(".page");
    assert!(
        (page.width - viewer.width).abs() <= 1.0,
        "fit width fits the width it has now: page {} in {}",
        page.width,
        viewer.width,
    );

    // And a mode with something to centre is centred in the window it has.
    reader.press_action(Action::FitPage);
    let (left, right) = margins(&reader);
    assert!(left > 10.0, "there is ground either side of it: {left}");
    assert!((left - right).abs() <= 1.0, "{left} against {right}");
}

#[test]
fn the_document_is_centred_beside_an_open_panel() {
    // One pixel, and the same fault as the resize above in miniature: the
    // panel's hairline is a border, a content box put it outside the width the
    // panel was given, and the document was laid out for a viewport a pixel
    // wider than the box it was drawn into. Every page came out flush against
    // the panel with its far edge a pixel over the window.
    let mut reader = book();
    reader.press_chord("mod+b");
    reader.press_action(Action::FitPage);
    let (left, right) = margins(&reader);
    assert!((left - right).abs() <= 1.0, "{left} against {right}");

    let panel = reader.harness.layout_rect(".sidebar");
    let viewer = reader.harness.layout_rect(".viewer");
    assert!(
        (panel.x + panel.width - viewer.x).abs() <= 0.5,
        "the panel ends where the document starts: {} against {}",
        panel.x + panel.width,
        viewer.x,
    );
}

#[test]
fn a_menu_comes_down_under_the_button_that_opened_it() {
    // They were one layer pinned to the ends of the bar, so the View menu —
    // whose button sits between Trim and the theme — came down under the page
    // field, three chips to the right of what had been clicked. Each is in an
    // anchor of its own now; see `.anchor` in `styles.rs`.
    let mut reader = book();
    reader.click(".chip.fit");
    let menu = reader.harness.layout_rect(".menu.view");
    let chip = reader.harness.layout_rect(".chip.fit");
    assert!(
        (menu.x - chip.x).abs() <= 1.0,
        "the View menu is under its own button: {} against {}",
        menu.x,
        chip.x,
    );
    assert!(menu.y > chip.y, "and below it");

    // 1280 rather than the harness's 1100, for the reason `tests/menus.rs`
    // gives: this menu hangs off the document's name, and at 1100 there is no
    // bar left to give the name — it is squeezed to its floor and, where
    // `ui-sans-serif` is wider than SF Pro, out of its own group, which is
    // where Blitz stops hit-testing it.
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            width: 1280,
            ..Options::default()
        },
    );
    reader.click(".chip.title");
    // Said this way round rather than letting `layout_rect` panic on a missing
    // selector: a menu that did not open is a click that landed somewhere else,
    // and where everything was is the whole of what tells you why — a runner
    // whose `ui-sans-serif` is wider lays the bar out differently.
    assert!(
        reader.box_of(".menu.document").is_some(),
        "the document menu did not open: title {:?}, middle {:?}, left {:?}, right {:?}",
        reader.box_of(".chip.title"),
        reader.box_of(".bar-center"),
        reader.box_of(".bar-left"),
        reader.box_of(".bar-right"),
    );
    let menu = reader.harness.layout_rect(".menu.document");
    let chip = reader.harness.layout_rect(".chip.title");
    assert!(
        (menu.x - chip.x).abs() <= 1.0,
        "{} against {}",
        menu.x,
        chip.x
    );

    // The theme menu is the one aligned by its right edge, because it is wider
    // than its button and near the end of the bar.
    let mut reader = book();
    reader.click(".chip.theme");
    let menu = reader.harness.layout_rect(".menu.theme");
    let chip = reader.harness.layout_rect(".chip.theme");
    let (menu_end, chip_end) = (menu.x + menu.width, chip.x + chip.width);
    assert!(
        (menu_end - chip_end).abs() <= 1.0,
        "the Theme menu ends where its button does: {menu_end} against {chip_end}",
    );
}

#[test]
fn an_undrawn_page_is_the_theme_s_paper_and_not_white() {
    // Moonowl Dark: a recolouring theme, so a page under it is drawn on the
    // theme's own paper and a page that has not been drawn yet must be too.
    // A white rectangle on a dark theme is the flash a reader sees on every
    // zoom step and every jump — a re-keyed page is a new node with no
    // texture, and until pdfium answers, this is what is on screen.
    let reader = wearing("moonowl-dark");
    let style = reader.harness.attr(".root", "style").unwrap_or_default();
    let paper = paper_of(&style);
    let page = value_of(&style, "--page");
    assert_eq!(page, paper, "the page is the theme's paper: {style}");
    assert_ne!(page, "#ffffff");
}

#[test]
fn a_page_no_theme_is_recolouring_is_white() {
    // Moonowl Light does not recolour, so the paper on screen is the paper the
    // printer used, whatever the chrome around it is.
    let reader = wearing(theme::DEFAULT_LIGHT);
    let style = reader.harness.attr(".root", "style").unwrap_or_default();
    assert_eq!(value_of(&style, "--page"), "#ffffff");
}

#[test]
fn the_toolbar_wears_the_theme_rather_than_a_grey() {
    // Mark, Trim, the zoom and the two steppers are all `--muted`, and it was
    // mixed halfway between the paper and the ink — which is a mid-grey
    // whatever the two ends are, so all fourteen themes put very nearly the
    // same colour in the bar. What is asserted is the distance from the
    // theme's own ink: near it, and much nearer than the halfway shade was.
    for id in [
        "moonowl-light",
        "moonowl-dark",
        "bay-brown",
        "sepia",
        "nord",
    ] {
        let reader = wearing(id);
        let style = reader.harness.attr(".root", "style").unwrap_or_default();
        let ink = rgb(&value_of(&style, "--text"));
        let paper = rgb(&value_of(&style, "--paper"));
        let muted = rgb(&value_of(&style, "--muted"));
        let span = distance(ink, paper);
        let off = distance(ink, muted);
        assert!(
            off < span * 0.35,
            "{id}: the bar is the theme's ink and not a grey — {off} of {span}",
        );
    }
}

#[test]
fn the_toolbar_carries_the_app_s_icons_in_the_theme_s_shades() {
    // Every button in the app's `index.html` has a `data-icon` and none here
    // did, which is the other half of a bar that read as grey words. The
    // colour is asserted because it is the part that has no cascade behind it:
    // an inline `<svg>` reaches usvg as its own document — see `Icon` — so a
    // `currentColor` icon comes out black on every theme, which on Moonowl Dark
    // is invisible.
    let mut reader = wearing("moonowl-dark");
    let style = reader.harness.attr(".root", "style").unwrap_or_default();
    let muted = value_of(&style, "--muted");
    let accent = value_of(&style, "--accent");

    assert_eq!(
        reader
            .harness
            .attr(".chip.contents .icon", "stroke")
            .as_deref(),
        Some(muted.as_str()),
        "an idle chip's icon is the quiet shade",
    );

    // And a chip whose thing is in force takes the accent, icon and all. This
    // was the Mark chip until the bar stopped having one — a mark is set once
    // and read from the Contents panel, so a permanent button for it was a
    // permanent button for something nobody presses twice in an hour, and the
    // app has never had one. Contents is the same shape: a chip with an icon,
    // a word, and an on state.
    reader.press_chord("mod+b");
    assert_eq!(
        reader
            .harness
            .attr(".chip.contents .icon", "stroke")
            .as_deref(),
        Some(accent.as_str()),
    );

    // The panel's tabs too, which are the other place a label stands alone.
    assert!(
        reader.harness.query(".tab .icon").is_some(),
        "the tabs carry them"
    );
}

/* ----------------------------------------------------------- reading it */

fn value_of(style: &str, name: &str) -> String {
    style
        .split(';')
        .find_map(|piece| {
            let (key, value) = piece.split_once(':')?;
            (key.trim() == name).then(|| value.trim().to_string())
        })
        .unwrap_or_else(|| panic!("no {name} in {style}"))
}

fn paper_of(style: &str) -> String {
    value_of(style, "--paper")
}

fn rgb(hex: &str) -> [f64; 3] {
    let hex = hex.trim_start_matches('#');
    [0, 2, 4].map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap() as f64)
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/* ------------------------------------------------------- the page field */

/// Whether the field is up rather than the readout it replaces.
fn typing(reader: &Reader) -> bool {
    reader.harness.query(".page-field").is_some()
}

#[test]
fn the_page_field_opens_holding_the_page_it_is_on() {
    let mut reader = book();
    reader.press("p");
    reader.type_text("37");
    reader.press("Enter");
    assert_eq!(reader.state().page, 37);

    // **And it opens holding it**, rather than empty. The app selects the
    // field's contents (`el.pageNumber.select()`); parley will do that only
    // when a keystroke asks it to and there is no imperative door onto it, so
    // the selection is emulated — the number is there, and the first thing
    // typed replaces all of it.
    reader.press("p");
    assert!(typing(&reader));
    assert_eq!(reader.state().label, "37");

    reader.press("9");
    assert_eq!(reader.state().label, "9");
    // And the second digit lands *after* the first. It did not: writing the
    // character in through the value attribute replaced the editor's string
    // and left the caret at the front, so "50" was typed as "05" — which
    // parses to page 5 and passes every test written in one digit.
    reader.press("2");
    assert_eq!(reader.state().label, "92");
    reader.press("Enter");
    assert_eq!(reader.state().page, 92);
}

/// **A caret moved is a caret placed**: the emulated "all selected" goes
/// with an arrow key, and what is typed next goes where the caret is. End
/// then 5 on a fresh "37" replaced the lot with "5".
#[test]
fn an_arrow_in_the_page_field_ends_the_select_all() {
    let mut reader = book();
    reader.press("p");
    reader.type_text("37");
    reader.press("Enter");
    reader.press("p");
    assert_eq!(reader.state().label, "37");
    reader.press("End");
    assert!(reader.harness.query(".page-field.fresh").is_none());
    reader.press("5");
    assert_eq!(reader.state().label, "375");
}

#[test]
fn the_page_field_shows_that_all_of_it_is_selected() {
    // The emulated select-all was invisible: the field opened looking like a
    // field somebody had clicked into, and the first digit replacing the whole
    // number came as a surprise. `.page-field.fresh` is the theme's own
    // selection colours — the pair a swept passage on the page is drawn in.
    let mut reader = book();
    reader.press("p");
    let class = reader
        .harness
        .attr(".page-field", "class")
        .unwrap_or_default();
    assert!(class.contains("fresh"), "opened selected: {class}");

    // And typing ends it, because from then on there is a caret and a number
    // being built rather than a value standing in for a selection.
    reader.press("9");
    let class = reader
        .harness
        .attr(".page-field", "class")
        .unwrap_or_default();
    assert!(!class.contains("fresh"), "typed into: {class}");
}

#[test]
fn the_page_box_is_the_app_s_width_until_the_number_outgrows_it() {
    // Blitz gives parley no alignment for a text input's own text and calls
    // `set_width(None)`, so `text-align: center` on one does nothing at all —
    // which left the page number pinned against the left wall of a box wide
    // enough for four digits. A box that fits is most of the answer, and the
    // padding under it is the rest — see the test below. See the comment on
    // `.pill` in `app.rs`.
    //
    // **The floor is the app's 44px**, not the smallest box a digit will sit
    // in. `.page-jump input` is `width: 44px` whatever is in it — four digits
    // fit and one digit is centred in the same box — and a floor of
    // twenty-eight made page 1 of any document a slot half the size of the
    // count beside it, which is half of what "cramped" meant. So one, two and
    // three digits are all the app's width, and only the fourth grows.
    let mut reader = book();
    let one = reader.harness.layout_rect(".page-now").width;
    assert!((one - 44.0).abs() <= 1.0, "page 1 is the app's box: {one}");
    reader.press("p");
    reader.type_text("250");
    let three = reader.harness.layout_rect(".page-field").width;
    assert!(
        (three - one).abs() <= 1.0,
        "and so is page 250: {three} against {one}"
    );
    reader.press("Escape");

    // Four does not fit in it, and grows rather than being cut off — which is
    // the one place this parts company with the app, and only because Blitz
    // cannot centre what is in the field.
    reader.press("p");
    reader.type_text("1250");
    let four = reader.harness.layout_rect(".page-field").width;
    assert!(
        four > one + 4.0,
        "four digits is wider: {four} against {one}"
    );
    reader.press("Escape");
    reader.press("p");
    reader.type_text("250");
    let three = reader.harness.layout_rect(".page-field").width;

    // And the readout the field replaces is the same width, so opening it
    // moves nothing else in the bar.
    reader.press("Enter");
    let now = reader.harness.layout_rect(".page-now").width;
    assert!((now - three).abs() <= 1.0, "{now} against {three}");
}

#[test]
fn a_chip_in_force_stands_on_the_accent_rather_than_wearing_it() {
    // Every theme in this app names a near-monochrome text colour, so a bar
    // written in a shade of it is grey whatever theme is on — and the only
    // colour that ever appeared was the accent, arriving as one bright word
    // among the grey with nothing under it. The tint is what carries the
    // theme. `--accent-soft` is a fifth of the way from the paper to the
    // accent: plainly the accent, and still somewhere a word can be read.
    for id in ["moonowl-light", "bay-brown", "dracula"] {
        let reader = wearing(id);
        let style = reader.harness.attr(".root", "style").unwrap_or_default();
        // Measured against the *surface*, which is what the app mixes it from
        // — `mix(accent, surface, 0.86)` in `applyTheme`. It was measured
        // against the paper here, which is a different colour on a light
        // theme (the surface is pulled more than halfway to white) and a very
        // different one on a dark.
        let surface = rgb(&value_of(&style, "--surface"));
        let accent = rgb(&value_of(&style, "--accent"));
        let soft = rgb(&value_of(&style, "--accent-soft"));
        let span = distance(surface, accent);
        assert!(
            distance(surface, soft) > span * 0.05 && distance(accent, soft) > span * 0.5,
            "{id}: a tint of the accent, not the accent — {soft:?}",
        );
    }
}

#[test]
fn backspace_on_a_field_nobody_has_typed_into_empties_it() {
    let mut reader = book();
    reader.press("p");
    assert_eq!(reader.state().label, "1");
    reader.press("Backspace");
    assert_eq!(reader.state().label, "");
}

#[test]
fn enter_on_a_field_nobody_has_typed_into_is_never_mind() {
    let mut reader = book();
    reader.press("p");
    reader.type_text("40");
    reader.press("Enter");
    assert_eq!(reader.state().page, 40);

    // Opening the field and pressing Enter goes nowhere — and, in
    // particular, does not put an entry in the history for a jump that never
    // happened, which stepping back proves.
    reader.press("p");
    reader.press("Enter");
    assert!(!typing(&reader));
    assert_eq!(reader.state().page, 40);
    reader.press_chord("mod+[");
    assert_eq!(reader.state().page, 1);
}

#[test]
fn a_press_anywhere_else_puts_the_page_field_away() {
    let mut reader = book();
    reader.press("p");
    reader.type_text("250");
    assert!(typing(&reader));

    // Clicking the document abandons it and puts the current page back,
    // which is the field's own `blur` handler in `main.ts`. Nothing else
    // took the field down: it held the keyboard until Escape or Enter, and a
    // reader who had clicked away from it was typing into a field they were
    // no longer looking at.
    let (x, y) = reader.point_on(1, (0.5, 0.5));
    reader.click_at(x, y);
    assert!(!typing(&reader));
    assert_eq!(reader.state().label, "1");
    assert_eq!(reader.state().page, 1);
}

/// And the page field, for the same reason: Backspace is a command rather
/// than a key on a Mac. See `a_query_can_be_corrected_as_well_as_typed` in
/// `tests/search.rs`, which is the same fault in the other field.
#[test]
fn a_typed_page_can_be_corrected() {
    let mut reader = Reader::open(&Reader::book());
    reader.press_chord("mod+alt+g");
    reader.type_text("123");
    assert_eq!(
        reader.harness.attr(".page-field", "value").as_deref(),
        Some("123"),
    );
    reader.press("Backspace");
    assert_eq!(
        reader.harness.attr(".page-field", "value").as_deref(),
        Some("12"),
        "a digit typed by mistake can be taken back",
    );
}

/* ------------------------------------------------- the way back to the bar */

/// **With the toolbar away, the top edge stands in for it.** `#toolbar-peek`
/// in the app: reaching for the edge drops a handle in, and pressing it puts
/// the bar back. Until this the only way back was the key the notice names,
/// which is a sentence that has to be read in four seconds and remembered.
#[test]
fn reaching_for_the_top_edge_gives_the_toolbar_back() {
    let mut reader = book();
    reader.press_chord("mod+t");
    assert!(!reader.state().toolbar, "the toolbar is away");
    assert!(
        reader.harness.query(".toolbar-peek").is_none(),
        "and nothing is on screen until somebody reaches for it",
    );

    // Half way down the window is not reaching for anything.
    reader.point_to(400.0, 300.0);
    assert!(reader.harness.query(".toolbar-peek").is_none());

    reader.point_to(400.0, 3.0);
    assert!(
        reader.harness.query(".toolbar-peek").is_some(),
        "the handle is down",
    );

    reader.click(".toolbar-peek");
    assert!(reader.state().toolbar, "and the bar is back");
}

/// **The zoom is said once.** With the bar up its zoom chip shows the new
/// size, so the notice would only repeat it; with the bar away the notice is
/// all there is, in the top right corner where the chip would be.
#[test]
fn the_zoom_notice_speaks_only_with_the_bar_away() {
    let mut reader = book();
    let height = reader.window().1 as f32;

    reader.press_action(Action::ZoomIn);
    assert!(
        reader.box_of(".notice").is_none(),
        "the chip already says it"
    );

    reader.press_chord("mod+t");
    reader.press_action(Action::ZoomIn);
    let (_, top, _, _) = reader.box_of(".notice").expect("the zoom said so");
    assert!(top < height / 2.0, "in the upper half of the window: {top}");
}

/// And a setting to be left alone by it, bar or no bar.
#[test]
fn the_zoom_notice_can_be_turned_off() {
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            settings: vec![("show_zoom_notice".into(), serde_json::json!(false))],
            ..Default::default()
        },
    );
    reader.press_chord("mod+t");
    reader.press_action(Action::ZoomIn);
    let notice = reader.state().notice;
    assert!(!notice.ends_with('%'), "asked to stay quiet: {notice:?}");
}

/// **And the handle's own place is inside the reach**, which it was not: the
/// band was the top eight pixels and the handle sits below the strip macOS
/// slides its title bar over, so the pointer had to be put where the handle is
/// not in order to see it — and moving to it was moving out of the band that
/// offered it.
#[test]
fn the_handle_appears_where_the_handle_is() {
    let mut reader = book();
    reader.press_chord("mod+t");
    reader.point_to(400.0, 3.0);
    let (_, top, _, height) = reader
        .box_of(".toolbar-peek")
        .expect("the handle is on screen");

    // Away, and then straight to where it was: the middle of the handle's own
    // box is where a reader who has seen it once puts the pointer.
    reader.point_to(400.0, 400.0);
    assert!(reader.harness.query(".toolbar-peek").is_none(), "and away");
    reader.point_to(400.0, top + height / 2.0);
    assert!(
        reader.harness.query(".toolbar-peek").is_some(),
        "pointing at the handle brings the handle down",
    );
}

/// It stays while it is being reached for — the hand has to travel to it —
/// and goes when the pointer is plainly somewhere else.
#[test]
fn the_handle_stays_until_the_pointer_is_well_away() {
    let mut reader = book();
    reader.press_chord("mod+t");
    reader.point_to(400.0, 3.0);
    reader.point_to(400.0, 60.0);
    assert!(
        reader.harness.query(".toolbar-peek").is_some(),
        "still there while the pointer is on its way to it",
    );
    reader.point_to(400.0, 300.0);
    assert!(reader.harness.query(".toolbar-peek").is_none());
}

/// **And where you are, while you scroll without a bar to say so.**
/// `#page-pill` in the app, under the same two conditions: only with the
/// toolbar away, because with it up the same number is already on screen, and
/// only if the reader wants it — which is now something they have to say, the
/// setting having been turned off. See the two tests below this one.
#[test]
fn the_page_pill_says_where_you_are_when_the_toolbar_is_away() {
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            settings: vec![("show_page_pill".into(), serde_json::json!(true))],
            ..Options::default()
        },
    );
    reader.wheel(1_200.0);
    assert!(
        reader.harness.query(".page-pill").is_none(),
        "the toolbar is up and already says it",
    );

    reader.press_chord("mod+t");
    reader.wheel(1_200.0);
    // Read off the pill rather than off `state().page`, which is the number in
    // the toolbar — and the toolbar is the thing that is not there.
    let said = reader.harness.text_content(".page-pill");
    let (page, rest) = said.split_once(" of ").unwrap_or_default();
    assert_eq!(rest, "400", "how many there are: {said:?}");
    assert!(
        page.parse::<usize>().is_ok_and(|page| page > 1),
        "and which one we have scrolled to: {said:?}",
    );
}

/// **A zoom moves the scroll and is not a scroll**: the pill answers the
/// reader scrolling, not the document being laid out again under them.
#[test]
fn the_pill_stays_down_through_a_zoom() {
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            settings: vec![("show_page_pill".into(), serde_json::json!(true))],
            ..Options::default()
        },
    );
    reader.press_chord("mod+t");
    reader.wheel(5_000.0);
    // Past the pill's second, so what is on screen is the zoom's doing.
    reader.wait_until(3.0, |reader| reader.harness.query(".page-pill").is_none());
    reader.press_action(Action::ZoomIn);
    assert!(
        reader.harness.query(".page-pill").is_none(),
        "a zoom, not a scroll"
    );
    // …and a pinch, which begins with two fingers landing: a wheel of nothing.
    reader.wheel(0.0);
    reader.pinch(0.2);
    reader.pinch_ended();
    assert!(
        reader.harness.query(".page-pill").is_none(),
        "a pinch, not a scroll"
    );
    reader.wheel(600.0);
    assert!(
        reader.harness.query(".page-pill").is_some(),
        "and a scroll after it still says so"
    );
}

/// …and not at all otherwise, which is the default now: a count that appears
/// of its own accord over the middle of the page covers the thing it is
/// describing, and the scrollbar says the same without saying anything.
#[test]
fn the_pill_is_quiet_until_it_is_asked_for() {
    let mut reader = book();
    reader.press_chord("mod+t");
    reader.wheel(1_200.0);
    assert!(reader.harness.query(".page-pill").is_none());
}

/// **The scrollbar, which this reader draws because it does not inherit one.**
/// A thumb as tall as the window's share of the document, with a floor under
/// it — the honest height for one page of four hundred is two pixels — and
/// hard against the right-hand edge, because a pointer thrown at the side of
/// the screen stops at the edge.
#[test]
fn the_scrollbar_says_how_far_into_the_book_you_are() {
    let mut reader = book();
    let bar = reader.harness.layout_rect(".scrollbar");
    let viewer = reader.harness.layout_rect(".viewer");
    assert!(
        ((bar.x + bar.width) - (viewer.x + viewer.width)).abs() < 0.5,
        "flush with the edge: bar ends at {}, window at {}",
        bar.x + bar.width,
        viewer.x + viewer.width,
    );
    assert!(
        (10.0..=16.0).contains(&bar.width),
        "neither a hairline nor a column: {}",
        bar.width,
    );

    let top = reader.harness.layout_rect(".bar-thumb");
    assert!(
        top.height >= 30.0,
        "catchable in a long book: {}",
        top.height
    );
    assert!(
        top.y - bar.y < 1.0,
        "at the top of an unread book: {}",
        top.y
    );

    reader.wheel(4_000.0);
    let moved = reader.harness.layout_rect(".bar-thumb");
    assert!(
        moved.y > top.y,
        "and it follows the reader down: {} against {}",
        moved.y,
        top.y,
    );
}

/// A press on the track is a jump, and the count comes up beside it for as
/// long as the hand is on it — whatever the setting above says, because this
/// is the one gesture with nothing else on screen saying where it arrived.
#[test]
fn the_bar_can_be_dragged_and_says_where_it_has_got_to() {
    let mut reader = book();
    let before = reader.state().scroll;
    let bar = reader.harness.layout_rect(".scrollbar");
    let x = bar.x + bar.width / 2.0;
    let low = bar.y + bar.height * 0.75;

    reader.harness.mouse_down_at(x, low);
    reader.settle();
    assert!(
        reader.state().scroll > before,
        "the press alone carried the document: {} against {before}",
        reader.state().scroll,
    );
    let jumped = reader.state().scroll;
    assert!(
        reader.harness.query(".page-pill").is_some(),
        "and the count is up while the hand is on the bar",
    );
    // **And it rides the thumb.** Over the foot of the page it was covering
    // the thing it described; beside the bar it is where the eye already is.
    let pill = reader.harness.layout_rect(".page-pill");
    let thumb = reader.harness.layout_rect(".bar-thumb");
    assert!(
        pill.x + pill.width <= bar.x,
        "to the left of the bar: pill ends at {}, bar starts at {}",
        pill.x + pill.width,
        bar.x,
    );
    assert!(
        ((pill.y + pill.height / 2.0) - (thumb.y + thumb.height / 2.0)).abs() < 6.0,
        "level with the thumb: pill at {}, thumb at {}",
        pill.y + pill.height / 2.0,
        thumb.y + thumb.height / 2.0,
    );

    reader.carry(x, bar.y + 4.0);
    reader.settle();
    assert!(
        reader.state().scroll < jumped,
        "dragging back up goes back up: {} against {jumped}",
        reader.state().scroll,
    );

    reader.harness.mouse_up_at(x, bar.y + 4.0);
    reader.settle();
    assert!(
        reader.harness.query(".page-pill").is_none(),
        "and it goes when the hand does",
    );
}

/// **And it is not there for long.** The bar comes up with the document's
/// movement and goes a few seconds after it stops — so a page being read has
/// nothing down its edge. It is drawn away rather than drawn transparent
/// because the track is twelve live pixels hard against the edge of the
/// window: an invisible thing that jumps the document when it is pressed is
/// worse than no thing at all.
#[test]
fn the_scrollbar_goes_away_once_the_reader_has_stopped() {
    let mut reader = book();
    reader.wheel(1_200.0);
    assert!(
        reader.harness.query(".scrollbar").is_some(),
        "up while the document is moving",
    );
    assert!(
        reader.wait_until(6.0, |reader| reader.harness.query(".scrollbar").is_none()),
        "and away again once it has stopped",
    );
    // …and back on the next wheel, which is the half that makes it a bar
    // rather than a thing that was there once.
    reader.wheel(600.0);
    assert!(reader.harness.query(".scrollbar").is_some());
}

/// A document short enough to fit has no bar, which is the point of asking
/// `bar_thumb` rather than always drawing one: a track with a thumb the whole
/// length of it says nothing and is one more thing on the page.
#[test]
fn a_document_that_fits_has_no_scrollbar() {
    let path = std::env::temp_dir().join("moonowl-chrome-fits.pdf");
    fixture::draft(&path, 1);
    let mut reader = Reader::open(path.to_str().expect("a path"));
    reader.press_action(Action::FitPage);
    assert!(reader.harness.query(".scrollbar").is_none());
}

/// **The name of what is open is readable, and it was twenty pixels wide.**
///
/// `.chip.title` had `flex: 1 1 0` — a basis of nothing, asking for whatever
/// the bar has left over, which in a bar carrying fourteen controls is nothing
/// at all. So the document's name came out as three letters at every window
/// size, and the wider the window the more absurd it looked. The app's own
/// `.doc-title` is `flex: 0 1 auto`: it asks for the name and gives way under
/// pressure, which is what `min-width: 0` and the fade are for.
#[test]
fn the_name_of_the_document_is_wide_enough_to_read() {
    // **In a bar with room in it**, which the harness's default 1100 is not:
    // fourteen controls at the app's own sizes come to more than that, and
    // the app collapses its own `.doc-title` to sixteen pixels — the two
    // paddings, no name — at 1100 and at 1180, measured. What this is about
    // is the basis, not the width of the window: `flex: 1 1 0` asked for
    // nothing and was given nothing at *every* size, so the name was three
    // letters on a display of any width.
    let reader = Reader::open_with(
        &Reader::book(),
        Options {
            width: 1280,
            ..Default::default()
        },
    );
    let title = reader.harness.layout_rect(".chip.title");
    assert!(
        title.width > 50.0,
        "there is room for a file name in it: {title:?}",
    );
    // And it still gives way rather than pushing the bar over: `max-width` is
    // the app's 34ch, and the fixture's name is far shorter than that.
    assert!(title.width < 200.0, "{title:?}");
    assert_eq!(reader.state().title, "book.pdf");

    // …and it is the side that gives way, which is the other half of the
    // app's rule: squeeze the bar and the name goes rather than the bar
    // overflowing or the page controls being pushed off the middle.
    //
    // Just above the last of the bar's `@media` steps, which is the only
    // place left where it has to: since issue 3 the chips lose their words at
    // 1200px, and the room that makes is the name's at every width down to
    // here. At 1100, where this used to look, it is no longer squeezed at all.
    let narrow = Reader::open_with(
        &Reader::book(),
        Options {
            width: 602,
            ..Default::default()
        },
    );
    let squeezed = narrow.harness.layout_rect(".chip.title");
    assert!(
        squeezed.width < title.width,
        "{squeezed:?} against {title:?}"
    );
    let bar = narrow.harness.layout_rect(".toolbar");
    let last = narrow.harness.layout_rect(".chip.settings");
    assert!(
        last.x + last.width <= bar.x + bar.width + 1.0,
        "the bar overflowed instead: {last:?} against {bar:?}",
    );
}

/// **And it is only faded when there is something to fade.**
///
/// Blitz has no `text-overflow: ellipsis`, so a gradient mask over the last
/// twenty-four pixels stands in for one — and it was on the button
/// unconditionally, so every name in every document went pale at its right
/// edge whether or not it had run out of room. On `book.pdf`, a button
/// sixty-four pixels wide, that is more than a third of it, and it reads as
/// exactly what the reader called it: a button too small for its name. The
/// app shows nothing at all until there is something to cut.
#[test]
fn a_name_that_fits_is_not_faded_and_one_that_does_not_is() {
    let short = book();
    assert!(
        !short
            .attribute_all(".chip.title", "class")
            .iter()
            .any(|class| class.contains("clipped")),
        "a name that fits was faded anyway",
    );

    // A name past the cap — `max-width: 276px`, which is the app's 34ch — is
    // cut, and the fade is what says so.
    let long = Reader::open_with(
        &fixture::titled_pdf("A rather long document title that will not fit in the bar"),
        Options::default(),
    );
    assert!(
        long.attribute_all(".chip.title", "class")
            .iter()
            .any(|class| class.contains("clipped")),
        "a name that does not fit was not faded",
    );
}

/// The page count reads the way the app's does — `of 400`, not `/ 400`. It is
/// `#page-count` in `index.html` and it is one string, which is exactly the
/// kind of thing that drifts when an interface is written from memory.
#[test]
fn the_page_count_is_said_the_way_the_app_says_it() {
    let reader = book();
    assert_eq!(reader.harness.text_content(".of").trim(), "of 400");
}

/// **The zoom readout kept the last theme's colour.** Blitz settles the colour
/// of a run of text when it builds the run, and it rebuilds a run when
/// something about the element or its children is mutated — a change to a
/// custom property on the root is neither. Every other chip in the bar has an
/// icon whose `stroke` is the theme's, so every other chip is mutated and
/// comes out right; this one and the document's name have no icon, and both
/// name their colour for themselves now. The tell is that the colour only
/// arrived at the next zoom step, when the text changed.
#[test]
fn the_chips_with_no_icon_change_colour_with_the_theme() {
    let mut reader = Reader::open_with(&Reader::book(), Options::with_theme_key());
    let before = reader.attribute_all(".chip.fit", "style");
    reader.press("t");
    let after = reader.attribute_all(".chip.fit", "style");
    assert_ne!(before, after, "the readout wears the theme it is under");
    assert!(after[0].starts_with("color: #"), "{after:?}");
    let name = reader.attribute_all(".chip.title", "style");
    assert!(name[0].starts_with("color: #"), "{name:?}");
}

/// **The name of the document overhung the two buttons to its left, and took
/// their presses.** With `flex: 1 1 0` the chip was twenty pixels wide and its
/// label was laid out from a negative offset — which is why it read "ool"
/// rather than "book" — so the text node's box covered Close and Open. The
/// anchor around it is positioned, and a positioned element is hit-tested
/// ahead of its in-flow siblings, so hovering Open highlighted the document's
/// name and pressing Close opened its menu.
#[test]
fn each_button_in_the_bar_answers_for_itself() {
    let reader = book();
    let chip = reader.harness.layout_rect(".chip.title");
    for label in [".chip.contents", ".chip.open", ".chip.close-doc"] {
        let rect = reader.harness.layout_rect(label);
        assert!(
            rect.x + rect.width <= chip.x + 0.5,
            "{label} is clear of the name: {rect:?} against {chip:?}",
        );
        let hit = reader
            .harness
            .hit(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
            .map(|hit| hit.node_id);
        // Up from whatever was hit — usually the label's own text node — to
        // see whether the button is above it.
        let chip_node = reader.harness.query(label);
        let mut walk = hit;
        let mut landed = false;
        while let Some(node) = walk {
            if Some(node) == chip_node {
                landed = true;
                break;
            }
            walk = reader.harness.base().get_node(node).and_then(|n| n.parent);
        }
        assert!(landed, "a press in the middle of {label} lands on {label}");
    }
}

/// A press that slides a little is still a press. Blitz turns a two-pixel
/// movement with the button down into a text selection and then declines to
/// dispatch the click — so every button in this window answered about one
/// press in three. See `blitz-button-select.md`.
#[test]
fn a_press_that_slides_a_little_is_still_a_press() {
    let mut reader = book();
    reader.press_and_drag(".chip.close-doc", 6.0);
    assert!(reader.state().empty, "the document was closed");
}

/// **The cross on Close reddens under the pointer, and nothing else does.**
///
/// `#close-doc:hover svg` in the app's `styles.css` gives the cross
/// `--negative` and leaves the label the bar's own hover colour, so the warning
/// sits on the one glyph that means *close* rather than on the whole button.
/// An icon's `stroke` is an attribute and never the cascade, so both crosses
/// are drawn and `:hover` shows one — read here off which of them has a width.
#[test]
fn the_cross_on_close_reddens_under_the_pointer() {
    let mut reader = book();
    let shown = |reader: &Reader, button: &str| {
        let width = |which: &str| reader.width_of(&format!("{button} .icon.{which}"));
        match (width("rest"), width("hot")) {
            (Some(rest), Some(hot)) if rest > 0.0 && hot == 0.0 => "rest",
            (Some(rest), Some(hot)) if hot > 0.0 && rest == 0.0 => "hot",
            other => panic!("{button}: not one cross of the two, {other:?}"),
        }
    };
    assert_eq!(shown(&reader, ".close-doc"), "rest");

    let (x, y) = reader.harness.center_of(".close-doc");
    reader.point_to(x, y);
    assert_eq!(
        shown(&reader, ".close-doc"),
        "hot",
        "the cross did not change under the pointer"
    );

    // The theme's own negative, resolved the way `paint.rs` resolves a shipped
    // theme — `themes.ts`'s `RED_DARK`, since the reader opens on Moonowl Light
    // unless it is told otherwise.
    let parsed: theme::Theme = toml::from_str(theme::BUILT_IN[shipped(theme::DEFAULT_LIGHT)].1)
        .expect("Moonowl Light parses");
    let red = moonowl::palette::resolve(&parsed, false).negative();
    assert_eq!(
        reader.attribute_all(".close-doc .icon.hot", "stroke"),
        vec![moonowl::palette::hex(red)],
        "the cross is the theme's own negative",
    );

    // And the label beside it is not: only the glyph is unhappy.
    let label = reader.text_all(".close-doc");
    assert_eq!(label, vec!["Close".to_string()]);

    // Away again, and it goes back. A hover that only ever turns on is a button
    // that stays red for the rest of the session.
    reader.point_to(x, y + 300.0);
    assert_eq!(
        shown(&reader, ".close-doc"),
        "rest",
        "the cross stayed red after the pointer left"
    );

    // Close window takes Close's place in the bar. Under a pointer that has
    // not moved since, it is hovered, and its cross is red without a
    // `mouseenter` to say so.
    let (x, y) = reader.harness.center_of(".close-doc");
    reader.click(".close-doc");
    let (left, top, width, height) = reader
        .box_of(".close-window")
        .expect("Close window in the empty bar");
    assert!(
        (left..left + width).contains(&x) && (top..top + height).contains(&y),
        "Close window is not where Close was, so this proves nothing",
    );
    assert_eq!(shown(&reader, ".close-window"), "hot");
}

/// **The box fitting its contents settles three digits and not one.**
///
/// The floor holds page 1 in a box wide enough for three, so there are
/// twenty-odd pixels of slack in it and Blitz lays the run out from the leading
/// edge — the number the go-to-page key had just selected jumped to the left
/// wall of its own field the moment the readout became one. The slack is split
/// and paid as left padding, which is the one half of centring Blitz does
/// honour.
#[test]
fn the_number_stays_in_the_middle_of_a_box_wider_than_it() {
    fn padding(reader: &Reader) -> f64 {
        let style = reader
            .harness
            .attr(".page-field", "style")
            .unwrap_or_default();
        style
            .split("padding-left:")
            .nth(1)
            .and_then(|rest| {
                rest.trim()
                    .trim_end_matches(&[';', 'x', 'p'][..])
                    .trim()
                    .parse()
                    .ok()
            })
            .unwrap_or_else(|| panic!("no padding in {style:?}"))
    }

    let mut reader = book();
    reader.press("p");
    // One digit in a 44px box: the sheet's own 6px, and half the slack again.
    let one = padding(&reader);
    assert!(one > 12.0, "page 1 is pushed off the wall: {one}");

    // Four digits fill the box they grew, so there is no slack to split and the
    // padding is the sheet's — which is what keeps the two cases one rule
    // rather than a special case for short numbers.
    reader.type_text("1250");
    let four = padding(&reader);
    assert!(
        (four - 6.0).abs() < 0.5,
        "a fitted number is not moved: {four}"
    );
}

/// **The go-to-page key borrows a toolbar that is not there.**
///
/// There is nowhere to put the cursor with the bar away, so the shortcut brings
/// it in itself rather than making the reader do that first — and gives it back
/// when the jump is made or abandoned, because it borrowed the bar without
/// changing the setting. `focusPageNumber` in `main.ts` and the `blur` handler
/// that undoes it.
#[test]
fn the_go_to_page_key_brings_a_hidden_toolbar_in_and_puts_it_back() {
    let mut reader = book();
    reader.press_chord("mod+t");
    assert!(!reader.state().toolbar, "the toolbar is away");

    reader.press("p");
    assert!(
        reader.harness.query(".toolbar").is_some(),
        "and the key that needs it brings it in",
    );
    assert!(typing(&reader), "with the field open and holding the page");

    reader.type_text("37");
    reader.press("Enter");
    assert!(
        reader.harness.query(".toolbar").is_none(),
        "the loan ends with the jump",
    );
    reader.press_chord("mod+t");
    assert_eq!(reader.state().page, 37, "which is still made");

    // Abandoning it gives the bar back too, and neither is the setting: the
    // switch in the Settings menu still says what the reader chose.
    reader.press_chord("mod+t");
    reader.press("p");
    assert!(reader.harness.query(".toolbar").is_some());
    reader.press("Escape");
    assert!(
        reader.harness.query(".toolbar").is_none(),
        "and so does abandoning it",
    );
}

/// **The pointer goes away where the reader asked for it, and nowhere else.**
///
/// The wait is a real clock, because the rest is measured against one: see
/// `CURSOR_RESTS` and the "cursor-timeout" arm in `app.rs`.
#[test]
fn the_pointer_goes_away_when_it_is_left_alone() {
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            settings: vec![
                ("hide_cursor".into(), serde_json::json!(true)),
                // A second rather than the three it ships with: this is a
                // real clock, and the suite pays for every one of them.
                ("hide_cursor_after".into(), serde_json::json!(1.0)),
            ],
            ..Options::default()
        },
    );
    let (width, height) = reader.window();
    let (x, y) = (width as f32 / 2.0, height as f32 / 2.0);

    reader.point_to(x, y);
    assert!(
        reader.cursor_shown(),
        "a pointer that has just moved is there"
    );
    assert!(
        reader.wait_until(4.0, |reader| !reader.cursor_shown()),
        "and one left alone is not"
    );

    // Moving it brings it straight back, without waiting for anything.
    reader.point_to(x + 40.0, y + 40.0);
    assert!(reader.cursor_shown(), "it comes back the moment it moves");
}

/// And with the setting off — which is how it ships — nothing ever takes it
/// away.
#[test]
fn the_pointer_stays_unless_it_was_asked_to_go() {
    let mut reader = book();
    let (width, height) = reader.window();
    reader.point_to(width as f32 / 2.0, height as f32 / 2.0);
    assert!(
        !reader.wait_until(2.5, |reader| !reader.cursor_shown()),
        "off is off"
    );
}

/// And the wait is the reader's: a longer one is still waiting when a short
/// one would have finished.
#[test]
fn the_pointer_waits_as_long_as_it_was_told_to() {
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            settings: vec![
                ("hide_cursor".into(), serde_json::json!(true)),
                ("hide_cursor_after".into(), serde_json::json!(5.0)),
            ],
            ..Options::default()
        },
    );
    let (width, height) = reader.window();
    reader.point_to(width as f32 / 2.0, height as f32 / 2.0);
    assert!(
        !reader.wait_until(2.0, |reader| !reader.cursor_shown()),
        "five seconds is not two"
    );
}

/// **The toolbar keeps its words as long as they fit**: a half-screen window
/// on a laptop had a bar of bare symbols at 1200px, and the brief rules that
/// out. The rotations went to the View menu to make the room.
#[test]
fn the_toolbar_keeps_its_words_down_to_a_half_screen_window() {
    let mut reader = book();
    reader.resize(1120, 800);
    reader.settle();
    assert!(reader
        .box_of(".chip.find .chip-label")
        .is_some_and(|b| b.2 > 0.0));
    assert!(reader
        .box_of(".chip.theme .chip-label")
        .is_some_and(|b| b.2 > 0.0));
    assert!(
        reader.box_of(".chip.rotate-left").is_none(),
        "in the View menu"
    );
    reader.click(".chip.fit");
    assert!(reader
        .text_all(".menu.view .menu-label")
        .contains(&"Rotate left".to_string()));
}

/// Issue 3: a window made narrower ran the left of the bar on under the page
/// controls, and "Open…" could be pressed through the down arrow. At every
/// width a window can have, nothing in the bar stands on anything else and
/// nothing hangs off the end of it.
#[test]
fn the_toolbar_never_overlaps_itself_however_narrow_the_window() {
    let mut reader = book();
    for width in [
        1400u32, 1210, 1190, 1120, 1100, 1000, 730, 710, 610, 590, 480,
    ] {
        reader.resize(width, 800);
        reader.settle();
        let centre = reader.box_of(".bar-center").expect("page controls");
        let right = reader.box_of(".bar-right").expect("the right of the bar");
        for chip in [
            ".chip.contents",
            ".chip.open",
            ".chip.close-doc",
            ".chip.title",
        ] {
            // Hidden is `None` or a box of nothing, depending on the step.
            let Some((x, _, w, _)) = reader.box_of(chip).filter(|b| b.2 > 0.0) else {
                continue;
            };
            assert!(
                x + w <= centre.0 + 0.5,
                "{chip} runs under the page controls at {width}px"
            );
        }
        assert!(
            centre.0 + centre.2 <= right.0 + 0.5,
            "the middle meets the right at {width}px"
        );
        assert!(
            right.0 + right.2 <= width as f32 + 0.5,
            "the bar runs off a {width}px window"
        );
    }
    // The way to open a document is never one of the things that goes.
    assert!(reader.box_of(".chip.open").is_some_and(|b| b.2 > 0.0));
}

/// **The bar coming and going does not move the words.** The page field
/// borrows the toolbar when it is away, and the page jumped 47px down under
/// the reader's eyes while they typed a number.
#[test]
fn the_page_stays_put_when_the_bar_is_borrowed() {
    let mut reader = book();
    reader.press_chord("mod+t");
    reader.press("j");
    reader.press("j");
    reader.settle();
    let before = reader.box_of(".page").expect("a page").1;
    reader.press("p");
    reader.settle();
    let after = reader.box_of(".page").expect("a page").1;
    assert!((before - after).abs() <= 1.5, "{before} then {after}");
}
