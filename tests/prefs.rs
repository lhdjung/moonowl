//! The Settings window: five pages, and the switches in them doing what they
//! say.
//!
//! It is a window in the flow rather than one of the system's — see
//! `src/prefs.rs` — which is why it can be tested here at all: a second winit
//! window would be a second `Viewer` over a second `Store`, and the harness
//! has no windows.

use blitz_dom::Document as _;
use moonowl::harness::{Options, Reader};
use moonowl::keymap::Action;
use moonowl::theme;

fn book() -> Reader {
    Reader::open(&Reader::book())
}

/// Whether the window is up.
fn open(reader: &Reader) -> bool {
    reader.harness.query(".window").is_some()
}

/// Which page the nav column has in force.
fn page(reader: &Reader) -> String {
    reader.harness.text_content(".nav-item.on")
}

#[test]
fn the_settings_window_opens_on_its_key_and_leaves_on_escape() {
    let mut reader = book();
    assert!(!open(&reader));
    reader.press_chord("mod+,");
    assert!(open(&reader), "⌘, is the app's own key for it");
    assert_eq!(page(&reader), "Reading");

    reader.press("Escape");
    assert!(!open(&reader));

    // And it comes back to the page it was left on, which is what a window
    // with a nav column is expected to do — `currentPage` in `settings.ts`.
    reader.press_chord("mod+,");
    reader.click_nth(".nav-item", 3);
    assert_eq!(page(&reader), "Keyboard");
    reader.press("Escape");
    reader.press_chord("mod+,");
    assert_eq!(page(&reader), "Keyboard");
}

#[test]
fn the_cog_opens_a_menu_and_the_menu_opens_the_window() {
    // ⌘, is not discoverable, and the app has a Settings button at the end of
    // its bar. **What the button opens is a menu**, not the window —
    // `showSettingsMenu` in `main.ts`: the switches somebody reaches for while
    // reading are one press, and "All settings…" at the foot of it is the way
    // to the rest. Going straight to the window put a window over the document
    // for a switch.
    let mut reader = book();
    reader.click(".chip.settings");
    assert!(!open(&reader), "the window is not up");
    assert_eq!(reader.state().menu.as_deref(), Some("settings"));

    // The last item of that menu, which is the door to the window.
    let items = reader.harness.query_all(".menu.settings .menu-item").len();
    reader.click_nth(".menu.settings .menu-item", items - 1);
    assert!(open(&reader), "and now it is");
}

#[test]
fn a_press_beside_the_window_closes_it_and_a_press_inside_does_not() {
    let mut reader = book();
    reader.press_chord("mod+,");

    // Inside: the frame stops the press before the scrim sees it.
    let (x, y) = reader.harness.center_of(".window-pane");
    reader.click_at(x, y);
    assert!(open(&reader), "a press inside stays inside");

    // Beside it: the scrim's own press, which is `showWindow`'s scrim in the
    // app doing the same thing.
    reader.click_at(20.0, 500.0);
    assert!(!open(&reader));
}

#[test]
fn a_switch_changes_the_reader_and_is_written_down() {
    let mut reader = book();
    reader.press_chord("mod+,");
    let before = reader.harness.layout_rect(".page");

    // "Trim the margins" is the fourth field on the Reading page and the
    // first switch on it. **What is asserted is the page**, because that is
    // where a reader would see it — the toolbar used to carry a Trim chip and
    // does not any more, the app having never had one either: trimming is a
    // setting somebody turns on for a scanned book and leaves on, not a thing
    // pressed twice in an hour.
    reader.click(".switch");
    assert_eq!(
        reader.harness.attr(".switch", "aria-checked").as_deref(),
        Some("true"),
        "the switch says so, which is what a screen reader is told",
    );
    let after = reader.harness.layout_rect(".page");
    assert!(
        (after.height - before.height).abs() > 1.0,
        "a page with its margins taken off is a different shape: \
         {before:?} against {after:?}",
    );

    // Written down rather than only done: a second reader on the same config
    // directory opens trimming.
    let config = reader.config.clone();
    let mut beside = Reader::open_with(
        &Reader::book(),
        Options {
            config,
            ..Options::default()
        },
    );
    beside.press_chord("mod+,");
    assert_eq!(
        beside.harness.attr(".switch", "aria-checked").as_deref(),
        Some("true"),
    );
}

#[test]
fn a_row_of_choices_changes_what_is_in_force() {
    let mut reader = book();
    reader.press_chord("mod+,");
    // Page progression is the first segmented control, and paged is its
    // second option. Nothing else in this app can reach it: there is
    // deliberately no shortcut for it, which is the brief's own rule.
    reader.click_nth(".segmented .segment", 1);
    // Paged mode lays out one row and nothing else — see `Layout::relayout`,
    // where that is the whole of the difference between the two modes — so
    // the pages in the DOM are what says it happened.
    assert_eq!(reader.state().mounted, vec![1]);
    assert_eq!(
        reader
            .harness
            .attr(".segmented .segment", "aria-pressed")
            .as_deref(),
        Some("false"),
        "and the one that was in force stands down",
    );
}

#[test]
fn a_number_can_be_stepped_and_typed() {
    let mut reader = book();
    // Fit page first, so that there is more than one page in the window to
    // measure between: at fit width a page of this fixture is taller than the
    // window and only one is mounted.
    reader.press_action(Action::FitPage);
    reader.press_chord("mod+,");
    // Measured off the pages rather than off the setting: the gap is a
    // distance on the screen, and the screen is where it has to appear.
    let gap = |reader: &Reader| {
        let pages = reader.harness.query_all(".page");
        let first = reader.harness.layout_rect_of(pages[0]);
        let second = reader.harness.layout_rect_of(pages[1]);
        (second.y - (first.y + first.height)).round()
    };
    assert_eq!(gap(&reader), 16.0, "the default");

    // The first stepper on the Reading page is the space between pages.
    reader.click(".stepper .step-up");
    assert_eq!(gap(&reader), 20.0, "one press is one step");

    // And a typed value is clamped to the range but never snapped to the
    // step: the step is how far one press moves, not a list of the answers
    // allowed. `ui.stepper` in the app says the same.
    //
    // The caret goes where the press put it: just inside the right edge is
    // after the number, so Backspace takes its last digit.
    let field = reader.harness.layout_rect(".step-field");
    reader.click_at(field.x + field.width - 3.0, field.y + field.height / 2.0);
    reader.press("Backspace");
    reader.type_text("8");
    assert_eq!(gap(&reader), 28.0);
}

/// **Every field on the page can be typed into, and a press elsewhere leaves
/// it.** Each stepper used to ask for the keyboard, and the innermost asking
/// wins every event — so the last one on the page ("Wait before hiding it")
/// had the focus the moment the window opened and took it back from any other
/// field clicked into.
#[test]
fn any_stepper_takes_the_keyboard_and_a_press_elsewhere_gives_it_back() {
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            settings: vec![("hide_cursor".into(), serde_json::json!(true))],
            ..Options::default()
        },
    );
    reader.press_chord("mod+,");
    let focused = |reader: &Reader| reader.harness.doc.inner().get_focussed_node_id();
    let fields = reader.harness.query_all(".step-field");
    assert!(fields.len() > 1, "more than one stepper on the page");
    assert!(!fields.iter().any(|&id| focused(&reader) == Some(id)), "none has it on opening");

    reader.click(".step-field");
    assert_eq!(focused(&reader), Some(fields[0]), "the one clicked has it");

    reader.click(".pane-title");
    assert!(!fields.iter().any(|&id| focused(&reader) == Some(id)), "and gave it up");
}

/// Tab is the other way into a field, and it puts the caret after the number.
#[test]
fn a_field_reached_by_tab_has_its_caret_at_the_end() {
    let mut reader = book();
    reader.press_action(Action::FitPage);
    reader.press_chord("mod+,");
    // Tab walks every button before it, the toolbar's included.
    let field = reader.harness.query(".step-field");
    for _ in 0..40 {
        if reader.harness.doc.inner().get_focussed_node_id() == field {
            break;
        }
        reader.press("Tab");
    }
    assert_eq!(reader.harness.doc.inner().get_focussed_node_id(), field, "Tab reached it");
    reader.press("Backspace");
    reader.type_text("8");
    let pages = reader.harness.query_all(".page");
    let first = reader.harness.layout_rect_of(pages[0]);
    let second = reader.harness.layout_rect_of(pages[1]);
    assert_eq!((second.y - (first.y + first.height)).round(), 18.0, "16 became 1, then 18");
}

#[test]
fn the_keyboard_page_is_drawn_from_the_keymap() {
    // A hand-written table of shortcuts drifts the moment a key moves — the
    // app's did, naming ⌘T twice — so every row is an action out of the
    // keymap with whatever `keys.toml` gave it.
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            keys: [("next-page".to_string(), vec!["n".to_string()])]
                .into_iter()
                .collect(),
            ..Options::default()
        },
    );
    reader.press_chord("mod+,");
    reader.click_nth(".nav-item", 3);

    let listed: String = reader
        .harness
        .query_all(".keys")
        .iter()
        .map(|&node| reader.harness.layout_rect_of(node))
        .map(|_| String::new())
        .collect::<String>()
        + &reader.harness.text_content(".window-pane");
    assert!(
        listed.contains("Next pageN"),
        "a rebound key is the key it was rebound to: {listed}",
    );
    assert!(
        listed.contains("Search this document"),
        "and the rest of the keymap is still listed",
    );
}

#[test]
fn a_theme_is_chosen_from_its_own_swatch() {
    let mut reader = book();
    reader.press_chord("mod+,");
    reader.click_nth(".nav-item", 1);

    let cards = reader.harness.query_all(".theme-card").len();
    assert_eq!(
        cards,
        theme::BUILT_IN.len(),
        "every shipped theme is listed"
    );

    // The second card, which is the dark one the Moonowl family opens with.
    reader.click_nth(".theme-card", 1);
    assert_eq!(reader.state().theme, "Moonowl Dark");
}

/* --------------------------------------------------- dark mode, and the machine */

/// Which of the Appearance page's switches is on, by its label.
fn switched(reader: &Reader, label: &str) -> bool {
    let labels = reader.text_all(".field-label");
    let index = labels
        .iter()
        .position(|found| found == label)
        .unwrap_or_else(|| panic!("no field called {label}: {labels:?}"));
    reader.attribute_all("[role='switch']", "aria-checked")[index] == "true"
}

/// The Appearance page, with the switches on it.
fn appearance(reader: &mut Reader) {
    reader.press_chord("mod+,");
    reader.click_nth(".nav-item", 1);
    assert_eq!(page(reader), "Appearance");
}

/// A theme that leaves the page alone has no use for a link colour, so the
/// editor offers one only while the document is recoloured.
#[test]
fn the_link_colour_is_offered_only_while_recolouring() {
    let mut reader = book();
    appearance(&mut reader);
    reader.wheel_over(".window-pane", 600.0);
    // "New theme…", from Moonowl Light, which does not recolour.
    reader.click(".pane-actions button");
    let has_links = |reader: &Reader| reader.text_all(".field-label").iter().any(|l| l == "Links");
    assert!(!has_links(&reader), "{:?}", reader.text_all(".field-label"));
    // The editor's own switch is the last on the page.
    let last = reader
        .attribute_all("[role='switch']", "aria-checked")
        .len()
        - 1;
    reader.click_nth("[role='switch']", last);
    assert!(has_links(&reader), "{:?}", reader.text_all(".field-label"));
}

/// Five buttons under the theme grid for a theme of your own, and the last of
/// them — Delete — was cut off at the right. The row wraps instead.
#[test]
fn every_theme_button_fits_inside_the_page() {
    let temp = std::fs::canonicalize(std::env::temp_dir()).expect("a temp directory");
    let dir = temp.join(format!("moonowl-prefs-{}-buttons", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("themes")).expect("a themes directory");
    std::fs::write(
        dir.join("themes/fake-horse.toml"),
        "name = \"Fake Horse\"\ntext = \"#222222\"\nbackground = \"#eeeeee\"\n",
    )
    .expect("a theme");
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            config: dir,
            settings: vec![("theme".into(), serde_json::json!("fake-horse"))],
            ..Options::default()
        },
    );
    appearance(&mut reader);
    let (row_x, _, row_width, _) = reader.box_of(".pane-actions").expect("the row");
    let (x, _, width, _) = reader.box_of(".pane-actions .danger").expect("Delete");
    assert!(
        x + width <= row_x + row_width + 0.5,
        "Delete ends at {} and the row at {}",
        x + width,
        row_x + row_width
    );
}

/// Import and export in the theme editor are greyed out exactly while the
/// draft is unsaved — and only then, which `disabled="false"` got wrong.
#[test]
fn the_editor_greys_out_import_and_export_only_while_unsaved() {
    let temp = std::fs::canonicalize(std::env::temp_dir()).expect("a temp directory");
    let dir = temp.join(format!("moonowl-prefs-{}-greyed", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("themes")).expect("a themes directory");
    std::fs::write(
        dir.join("themes/fake-horse.toml"),
        "name = \"Fake Horse\"\ntext = \"#222222\"\nbackground = \"#eeeeee\"\n",
    )
    .expect("a theme");
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            config: dir,
            settings: vec![("theme".into(), serde_json::json!("fake-horse"))],
            ..Options::default()
        },
    );
    appearance(&mut reader);
    let greyed = |reader: &Reader| -> Vec<String> {
        let names = reader.text_all(".pane-actions button");
        let off = reader.attribute_all(".pane-actions button", "disabled");
        names
            .into_iter()
            .zip(off)
            .filter(|(name, off)| {
                name.ends_with("theme…") && !name.starts_with("New") && !off.is_empty()
            })
            .map(|(name, _)| name)
            .collect()
    };
    assert!(greyed(&reader).is_empty(), "the Themes view greys nothing");

    // "Edit Fake Horse…", a theme on disk with nothing changed yet.
    reader.wheel_over(".window-pane", 600.0);
    reader.click_nth(".pane-actions button", 1);
    assert_eq!(
        reader.text_all(".pane-group").last().map(String::as_str),
        Some("Edit theme")
    );
    assert!(greyed(&reader).is_empty(), "saved: {:?}", greyed(&reader));

    // The editor's own switch, "Recolour the document", is the last one.
    let last = reader
        .attribute_all("[role='switch']", "aria-checked")
        .len()
        - 1;
    reader.click_nth("[role='switch']", last);
    assert_eq!(greyed(&reader), ["Import theme…", "Export theme…"]);
}

#[test]
fn dark_mode_is_a_key_and_a_switch_and_they_are_the_same_thing() {
    let mut reader = Reader::open(&Reader::book());
    assert_eq!(reader.state().theme, "Moonowl Light");

    // ⌘D, which until now answered "Dark mode is not built yet".
    reader.press_chord("mod+d");
    assert_eq!(reader.state().theme, "Moonowl Dark");
    reader.press_chord("mod+d");
    assert_eq!(reader.state().theme, "Moonowl Light");

    // And the switch on the Appearance page, which is the same call.
    appearance(&mut reader);
    assert!(!switched(&reader, "Dark mode"));
    // Second on the page: the app puts "Follow the system" above it.
    reader.click_nth("[role='switch']", 1);
    assert!(switched(&reader, "Dark mode"));
    reader.press("Escape");
    assert_eq!(reader.state().theme, "Moonowl Dark");
}

#[test]
fn dark_mode_returns_to_the_pair_the_reader_chose() {
    // Sepia by day and Tokyo Night by night, which is the whole reason there
    // are two remembered slots rather than one remembered theme.
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            settings: vec![
                ("theme".into(), "sepia".into()),
                ("light_theme".into(), "sepia".into()),
                ("dark_theme".into(), "tokyo-night".into()),
            ],
            ..Options::default()
        },
    );
    assert_eq!(reader.state().theme, "Sepia");
    reader.press_chord("mod+d");
    assert_eq!(reader.state().theme, "Tokyo Night");
    reader.press_chord("mod+d");
    assert_eq!(reader.state().theme, "Sepia", "not Moonowl Light");
}

#[test]
fn a_dark_machine_is_read_in_the_dark_from_the_first_frame() {
    // Not "the theme changes shortly after launch": the reader asks the
    // window before it lays anything out, so a dark machine never sees a
    // white page on the way in. There is no frame here in which it is light.
    let reader = Reader::open_with(
        &Reader::book(),
        Options {
            appearance: Some(true),
            ..Options::default()
        },
    );
    assert_eq!(reader.state().theme, "Moonowl Dark");
}

#[test]
fn the_machine_changing_its_mind_is_followed_and_then_is_not() {
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            appearance: Some(false),
            ..Options::default()
        },
    );
    assert_eq!(reader.state().theme, "Moonowl Light");

    // Evening.
    reader.set_appearance(Some(true));
    assert_eq!(reader.state().theme, "Moonowl Dark");
    reader.set_appearance(Some(false));
    assert_eq!(reader.state().theme, "Moonowl Light");

    // Now the reader overrules it, by pressing ⌘D at noon. Following stops,
    // and the reader is told where the switch is — otherwise the machine's
    // next word would take the choice straight back off them.
    reader.press_chord("mod+d");
    assert_eq!(reader.state().theme, "Moonowl Dark");
    assert!(
        reader.state().notice.contains("No longer following"),
        "{}",
        reader.state().notice
    );
    appearance(&mut reader);
    assert!(!switched(&reader, "Follow the system"));
    reader.press("Escape");

    // …and the machine going light again leaves them where they are.
    reader.set_appearance(Some(false));
    assert_eq!(reader.state().theme, "Moonowl Dark");
}

#[test]
fn following_can_be_switched_back_on_and_takes_effect_at_once() {
    // A switch that says "follow the system" and leaves a light theme up on a
    // dark machine has not been believed by anybody.
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            appearance: Some(true),
            settings: vec![("follow_system_theme".into(), false.into())],
            ..Options::default()
        },
    );
    assert_eq!(
        reader.state().theme,
        "Moonowl Light",
        "not following, so not moved"
    );

    appearance(&mut reader);
    assert!(!switched(&reader, "Follow the system"));
    reader.click_nth("[role='switch']", 0);
    assert!(switched(&reader, "Follow the system"));
    reader.press("Escape");
    assert_eq!(reader.state().theme, "Moonowl Dark");
}

#[test]
fn a_machine_that_will_not_say_leaves_the_reader_alone() {
    // winit answers `Option<Theme>` and the `None` is real. Read as "light"
    // it would move every reader on such a platform to the light theme at
    // every launch, and turn following off the first time they chose a dark
    // one — which is why `Store::outside` is an `Option` all the way down.
    let mut reader = Reader::open_with(
        &Reader::book(),
        Options {
            settings: vec![
                ("theme".into(), theme::DEFAULT_DARK.into()),
                ("dark_theme".into(), theme::DEFAULT_DARK.into()),
            ],
            ..Options::default()
        },
    );
    assert_eq!(reader.state().theme, "Moonowl Dark");
    reader.set_appearance(None);
    assert_eq!(reader.state().theme, "Moonowl Dark");

    appearance(&mut reader);
    // The switch still reads the setting rather than what the setting can do
    // today — a control that reads back other than what is in the file is the
    // picker lying about the page. The sentence under it is where the
    // machine's silence is said.
    assert!(switched(&reader, "Follow the system"));
    let notes = reader.text_all(".field-note");
    assert!(
        notes
            .iter()
            .any(|note| note.contains("does not report an appearance")),
        "{notes:?}"
    );
}

/// **A page of settings starts at the top of its box.**
///
/// The pane scrolls — Reading is taller than the window — and the offset
/// belonged to the node rather than to what was in it, so leaving a scrolled
/// Reading for About kept the scroll and About, which is four short paragraphs,
/// sat entirely above the top of the box: the reader saw a blank page and
/// reported the About page as empty.
#[test]
fn a_page_opened_after_a_scrolled_one_starts_at_the_top() {
    let mut reader = book();
    reader.press_chord("mod+,");
    let fresh = reader.harness.layout_rect(".pane-title").y;

    reader.wheel_over(".window-pane", 400.0);
    assert!(
        reader.harness.layout_rect(".pane-title").y < fresh,
        "Reading scrolls",
    );

    // About is the last of the five and the shortest.
    reader.click_nth(".nav-item", 4);
    assert_eq!(page(&reader), "About");
    assert_eq!(
        reader.harness.layout_rect(".pane-title").y,
        fresh,
        "and About is where a page starts",
    );
    assert!(
        reader
            .harness
            .text_content(".window-pane")
            .contains("A calm place to read"),
        "with what is on it on screen",
    );
}

/// **A theme card shows a page, not a palette.**
///
/// Two bars of colour said what a theme was made of; the app's card says what
/// reading under it looks like — a word of body text and a link, each in the
/// ink it would really be drawn in. The link is the half that was missing:
/// nothing in this window showed a theme's link colour, which is a colour a
/// reader picks a theme for and cannot otherwise see until they meet a link.
#[test]
fn every_theme_card_shows_its_own_link_colour() {
    let mut reader = book();
    reader.press_chord("mod+,");
    reader.click_nth(".nav-item", 1);

    let cards = reader.harness.query_all(".theme-card").len();
    assert!(cards > 1, "the shipped themes are listed: {cards}");
    assert_eq!(
        reader.harness.query_all(".swatch-link").len(),
        cards,
        "one link on every card",
    );
    assert!(
        reader.harness.text_content(".theme-card").contains("Link"),
        "and it says so",
    );
}

/// **Naming a theme is not making one per letter.** The draft is shown by
/// standing it in the theme list, and a theme with no file has no id to be
/// found by — so every keystroke appended another copy, and a reader typing
/// "Brownie" watched B, Br, Bro and four more pile up in the Theme menu.
#[test]
fn naming_a_new_theme_leaves_one_theme_in_the_list() {
    let mut reader = book();
    reader.press_chord("mod+,");
    reader.click_nth(".nav-item", 1);
    reader.wheel_over(".window-pane", 600.0);
    // "New theme…", the first of the buttons under the cards.
    reader.click(".pane-actions button");
    reader.click(".text-field");
    reader.type_text("Brownie");
    // Out of the window, which leaves the draft being worn: the Theme menu in
    // the bar is where the pile showed. Twice, because Escape leaves the field
    // before it leaves the window — see
    // `escape_leaves_the_field_then_the_picker_then_the_window`.
    reader.press("Escape");
    reader.press("Escape");
    reader.click(".chip.theme");
    assert_eq!(
        reader.harness.query_all(".menu.theme .swatch").len(),
        theme::BUILT_IN.len() + 1,
        "fourteen themes and the one being written, whatever it is called",
    );
}

/// Into the theme editor, with a new theme in it.
fn editing(reader: &mut Reader) {
    reader.press_chord("mod+,");
    reader.click_nth(".nav-item", 1);
    // Far enough down to reach "New theme…", and no further: the editor
    // keeps this scroll, and the tests below click at what it leaves on
    // screen.
    reader.wheel_over(".window-pane", 346.0);
    reader.click(".pane-actions button");
}

/// **A hex field holds what is typed, and complains rather than correcting.**
/// It used to show the theme's colour and pass on only what parsed, so every
/// keystroke that was not yet a colour was rewritten under the caret: a
/// Backspace took a character out and the field put it straight back, which
/// made editing six digits a fight.
#[test]
fn a_colour_can_be_typed_wrong_on_the_way_to_being_right() {
    let mut reader = book();
    editing(&mut reader);
    let was = reader.attribute_all(".color-swatch", "style");
    let full = reader.field(".color-hex");
    assert_eq!(full.len(), 7, "six digits and a hash: {full}");

    // A digit taken out is a colour no longer, and the box says so and keeps
    // what is in it.
    reader.click_nth(".color-hex", 0);
    reader.press("End");
    reader.press("Backspace");
    let short = reader.field(".color-hex");
    assert_eq!(short, full[..full.len() - 1], "the digit went: {short}");
    assert!(
        reader.harness.query(".color-hex.unreadable").is_some(),
        "and five digits is not a colour",
    );
    assert_eq!(
        reader.attribute_all(".color-swatch", "style"),
        was,
        "so nothing has changed colour",
    );

    // Typed back to six, and the theme wears it as it is typed.
    reader.type_text("0");
    assert_eq!(reader.field(".color-hex").len(), 7);
    assert_ne!(
        reader.attribute_all(".color-swatch", "style"),
        was,
        "a colour that reads is worn as soon as it does",
    );
}

/// And what is left unreadable goes back to the colour the theme has, on the
/// way out of the field.
#[test]
fn an_unreadable_colour_reverts_when_the_field_is_left() {
    let mut reader = book();
    editing(&mut reader);
    let good = reader.field(".color-hex");

    reader.click_nth(".color-hex", 0);
    reader.type_text("nonsense");
    assert!(
        reader.harness.query(".color-hex.unreadable").is_some(),
        "the field says it cannot read that",
    );
    // Away to the next field, which is what leaving one means.
    reader.click_nth(".color-hex", 1);
    assert_eq!(reader.field(".color-hex"), good, "the colour comes back");
    assert!(reader.harness.query(".color-hex.unreadable").is_none());
}

/// **A grid of colours to point at**, because six hexadecimal digits is a
/// question most readers cannot answer — and the app had the system's own
/// picker through `<input type="color">`, which Blitz has not.
#[test]
fn a_colour_can_be_chosen_from_the_swatches() {
    let mut reader = book();
    editing(&mut reader);
    assert!(
        reader.harness.query(".color-picker").is_none(),
        "shut to start with"
    );

    reader.click_nth(".color-swatch", 0);
    assert!(
        reader.harness.query(".color-picker").is_some(),
        "the grid is down"
    );
    let choices = reader.harness.query_all(".color-choice").len();
    assert!(choices > 20, "a grid worth pointing at: {choices}");

    reader.click_nth(".color-choice", 2);
    assert_eq!(
        reader.field(".color-hex"),
        "#d9dce1",
        "the third swatch, which is what the field now says",
    );
    assert!(
        reader.harness.query(".color-picker").is_none(),
        "and the grid is away"
    );
}

/// **Enter finishes the theme editor**: the theme is saved and the window goes,
/// which is what Enter means in every window with a form in it. Before it, the
/// only way out of the editor was the pointer.
#[test]
fn enter_saves_the_theme_and_closes_the_window() {
    let mut reader = book();
    editing(&mut reader);
    reader.click(".text-field");
    reader.type_text("!");

    reader.press("Enter");
    assert!(!open(&reader), "the window has gone");
    assert!(
        reader.state().notice.starts_with("Saved"),
        "and it says what it did: {}",
        reader.state().notice,
    );
    // Which means a file: the theme is in the list the next time the menu is
    // opened, and it is the one being worn.
    reader.click(".chip.theme");
    let named = reader.text_all(".menu.theme .menu-label");
    assert!(
        named.iter().any(|name| name.ends_with('!')),
        "the theme that was written is in the list: {named:?}",
    );
}

/// A press at a fraction of the way across a box, which is how the square and
/// the strip are asked for a colour.
fn click_in(reader: &mut Reader, selector: &str, at: (f32, f32)) {
    let (x, y, width, height) = reader.box_of(selector).expect("it is on screen");
    reader.click_at(x + width * at.0, y + height * at.1);
}

/// **The square is how every other colour is reached.** The forty swatches are
/// a shortcut; without a spectrum behind them, any colour outside the grid was
/// a question of six hexadecimal digits — and Blitz has no
/// `<input type="color">` to ask it with.
///
/// The corners are what this checks, because they are the only points whose
/// answer does not depend on where the strip is: saturation runs out down the
/// left-hand edge and value runs out along the foot, so the near corner is
/// black and the one above it is white whatever hue is in use. The far corner
/// is the hue at full strength, which is where the strip comes in.
#[test]
fn the_square_reaches_the_colours_the_swatches_do_not() {
    fn channels(hex: &str) -> (u8, u8, u8) {
        let at = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).expect("six digits: {hex}");
        (at(1), at(3), at(5))
    }
    let mut reader = book();
    editing(&mut reader);
    reader.click_nth(".color-swatch", 0);
    assert!(
        reader.harness.query(".color-square").is_some(),
        "the square is down with the picker",
    );

    click_in(&mut reader, ".color-square", (0.01, 0.99));
    let (r, g, b) = channels(&reader.field(".color-hex"));
    assert!(r < 10 && g < 10 && b < 10, "the foot is black: {r} {g} {b}");

    click_in(&mut reader, ".color-square", (0.01, 0.01));
    let (r, g, b) = channels(&reader.field(".color-hex"));
    assert!(
        r > 245 && g > 245 && b > 245,
        "and straight up from it is white: {r} {g} {b}",
    );

    // Half way along the strip is cyan, and the far corner of the square is
    // whatever the strip says at full strength.
    click_in(&mut reader, ".color-strip", (0.5, 0.5));
    click_in(&mut reader, ".color-square", (0.99, 0.01));
    let (r, g, b) = channels(&reader.field(".color-hex"));
    assert!(
        r < 10 && g > 240 && b > 240,
        "the hue the strip was pointed at, at full strength: {r} {g} {b}",
    );
}

/// **Escape is a ladder, and the window is its last rung.** A field is nearer
/// than the picker under it and the picker is nearer than the window around
/// them, so thinking better of six hexadecimal digits took the whole theme
/// editor with it — the key went straight past both to `Action::Dismiss`.
#[test]
fn escape_leaves_the_field_then_the_picker_then_the_window() {
    let mut reader = book();
    editing(&mut reader);

    // From inside a field: the caret is the innermost thing there is.
    reader.click_nth(".color-hex", 0);
    reader.press("Escape");
    assert!(
        reader.harness.query(".window-pane").is_some(),
        "the editor is still up",
    );

    // Then the picker, which Escape reaches once no field holds the keyboard.
    reader.click_nth(".color-swatch", 0);
    assert!(
        reader.harness.query(".color-picker").is_some(),
        "the picker is down"
    );
    reader.press("Escape");
    assert!(
        reader.harness.query(".color-picker").is_none(),
        "and Escape puts it away",
    );
    assert!(
        reader.harness.query(".window-pane").is_some(),
        "without taking the window with it",
    );

    // And then the window.
    reader.press("Escape");
    assert!(
        reader.harness.query(".window-pane").is_none(),
        "which is what Escape means with nothing nearer to leave",
    );
}

/// **Each colour field's picker is pointed at its own colour.** The square and
/// the strip remember a hue, because the round trip through HSV cannot — a
/// grey has no hue to read back — and a picker that carried the last field's
/// hue into the next one would put the marker somewhere the colour is not.
/// Which bites exactly where a picker is nicest to have: nudging a colour a
/// little is impossible when the square starts in the wrong place.
///
/// Here the fields are separate components, and a hue that is remembered is
/// dropped the moment it stops producing the colour the field holds. The
/// far corner of the square is the hue at full strength, so it is the one
/// point that says which hue the picker is on.
#[test]
fn a_second_field_does_not_inherit_the_first_ones_hue() {
    let mut reader = book();
    editing(&mut reader);

    // The first field is dragged to cyan and put away.
    reader.click_nth(".color-swatch", 0);
    click_in(&mut reader, ".color-strip", (0.5, 0.5));
    reader.click_nth(".color-swatch", 0);

    // The second is given a blue off the grid — index 30, `#2f7fc4` — which
    // closes the picker, and then asked for that hue at full strength.
    reader.click_nth(".color-swatch", 1);
    reader.click_nth(".color-choice", 30);
    reader.click_nth(".color-swatch", 1);
    click_in(&mut reader, ".color-square", (0.99, 0.01));

    let hex = reader.attribute_all(".color-hex", "value")[1].clone();
    let at = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).expect("six digits");
    let (r, g, b) = (at(1), at(3), at(5));
    assert!(
        r < 10 && (110..170).contains(&g) && b > 245,
        "the blue this field holds, not the cyan the last one was left on: {hex}",
    );
}
