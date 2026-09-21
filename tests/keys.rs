//! The keyboard: the table, the file it can be rewritten from, and a key
//! actually pressed in a running reader.
//!
//! This is `tests/keys.test.mjs` from the app, carried across with the port it
//! tests, and it is worth saying what happened to it on the way. In the app
//! most of this file runs `keys.ts` twice through a helper that compiles a
//! module in memory and substitutes `const isMac = true` — because `isMac` is
//! a module-level constant imported from `api.ts` and there is no other way to
//! ask the same function what it would say on a different machine. Here `mac`
//! is a parameter, so both platforms are two arguments rather than two
//! compilations, and `MOONOWL_PLATFORM=other` — a whole environment variable
//! the app's harness carries to lie to `navigator.platform` — has nothing left
//! to do.
//!
//! The last two tests are the ones that cannot be written against the table:
//! the shipped `keys.toml` agreeing with the defaults it claims to be showing,
//! and a rebound key reaching the document.

use std::collections::BTreeMap;

use dioxus::html::{Code, Key, Modifiers};
use moonowl::harness::{Options, Reader};
use moonowl::keymap::{
    chords_of, default_keys, describe_binding, describe_chord, every, needs_document,
    parse_binding, parse_chord, Action, Keymap, Press, ACTIONS, EXTRA, GROUPS,
};

const MAC: bool = true;
const PC: bool = false;

/// A keydown, as the reader would see it. The code is what the physical key
/// says, which is the half a modifier can take away.
fn press_on(mac: bool, key: &str, code: &str, modifiers: Modifiers) -> Vec<String> {
    let pressed = if key.chars().count() == 1 {
        Key::Character(key.to_string())
    } else {
        key.parse().expect("a key")
    };
    let hit: Code = code.parse().unwrap_or(Code::Unidentified);
    chords_of(&pressed, hit, modifiers, mac)
}

fn press(key: &str, code: &str, modifiers: Modifiers) -> Vec<String> {
    press_on(MAC, key, code, modifiers)
}

/* ---------------------------------------------------------------- chords */

#[test]
fn a_chord_is_spelled_one_way_however_it_was_written() {
    assert_eq!(
        parse_chord("Shift+Mod+F", MAC).as_deref(),
        Some("mod+shift+f")
    );
    assert_eq!(parse_chord("cmd+alt+g", MAC).as_deref(), Some("mod+alt+g"));
    assert_eq!(
        parse_chord("option+ArrowLeft", MAC).as_deref(),
        Some("alt+left")
    );
    assert_eq!(parse_chord("ESC", MAC).as_deref(), Some("escape"));
    assert_eq!(parse_chord("G", MAC).as_deref(), Some("shift+g"));
    assert_eq!(parse_chord("Plus", MAC).as_deref(), Some("+"));
    // `+` is a key as well as a separator, which is why a chord is peeled from
    // the front rather than split.
    assert_eq!(parse_chord("mod++", MAC).as_deref(), Some("mod++"));
    assert_eq!(parse_chord("mod+-", MAC).as_deref(), Some("mod+-"));
}

#[test]
fn control_is_its_own_key_on_a_mac_and_is_the_modifier_everywhere_else() {
    assert_eq!(parse_chord("ctrl+d", MAC).as_deref(), Some("ctrl+d"));
    assert_ne!(parse_chord("ctrl+d", MAC), parse_chord("mod+d", MAC));
    // Not a second thing it could mean: pretending otherwise would be a file
    // that quietly does nothing on Windows.
    assert_eq!(parse_chord("ctrl+d", PC).as_deref(), Some("mod+d"));
}

#[test]
fn a_key_moonowl_cannot_read_says_so_rather_than_guessing() {
    assert_eq!(parse_chord("mod+wibble", MAC), None);
    assert_eq!(parse_chord("", MAC), None);
    assert_eq!(parse_chord("mod+", MAC), None);
    assert_eq!(parse_chord("f13", MAC), None);
    // Half a sequence is not a shorter sequence.
    assert_eq!(parse_binding("g wibble", MAC), None);
    assert_eq!(parse_binding("g g", MAC).as_deref(), Some("g g"));
}

/* ---------------------------------------------------------------- events */

#[test]
fn option_is_not_a_letter_on_a_mac_and_the_physical_key_still_is() {
    // ⌥G arrives as ©: the G is not there to compare any more, which used to
    // need `event.code` by hand in the one branch that knew about it.
    let chords = press("©", "KeyG", Modifiers::META | Modifiers::ALT);
    assert!(chords.contains(&"mod+alt+g".to_string()), "{chords:?}");
}

#[test]
fn shift_is_kept_for_a_letter_and_offered_both_ways_for_anything_else() {
    assert_eq!(press("G", "KeyG", Modifiers::SHIFT), vec!["shift+g"]);
    // ⇧Space must be able to mean something of its own, and must still fall
    // through to Space when it does not.
    assert_eq!(
        press(" ", "Space", Modifiers::SHIFT),
        vec!["shift+space", "space"]
    );
}

#[test]
fn a_chord_is_offered_by_what_it_says_and_by_what_key_was_hit() {
    assert_eq!(
        press("+", "Equal", Modifiers::META | Modifiers::SHIFT),
        vec!["mod+shift++", "mod+shift+=", "mod++", "mod+="]
    );
}

#[test]
fn the_modifiers_a_platform_does_not_use_match_nothing() {
    let shift: Vec<String> = press("Shift", "ShiftLeft", Modifiers::SHIFT);
    assert!(shift.is_empty(), "{shift:?}");
    // The Windows key is not bound to anything, and reading it as no modifier
    // at all would turn ⊞J into a scroll.
    assert!(press_on(PC, "j", "KeyJ", Modifiers::META).is_empty());
    assert_eq!(press_on(PC, "j", "KeyJ", Modifiers::CONTROL), vec!["mod+j"]);
}

/* --------------------------------------------------------------- keymaps */

#[test]
fn what_moonowl_ships_with_is_readable_and_no_key_does_two_things() {
    for mac in [MAC, PC] {
        let map = Keymap::shipped(mac);
        assert_eq!(map.problems, Vec::<String>::new(), "on mac={mac}");
        for spec in every() {
            for binding in default_keys(spec, mac) {
                assert_eq!(
                    parse_binding(binding, mac).as_deref(),
                    Some(binding),
                    "{}: {binding}",
                    spec.id.as_str()
                );
            }
        }
    }
}

#[test]
fn every_action_is_in_a_group_and_named_once() {
    let mut names: Vec<&str> = Vec::new();
    for spec in every() {
        assert!(GROUPS.contains(&spec.group), "{}", spec.id.as_str());
        assert!(!spec.label.is_empty(), "{}", spec.id.as_str());
        names.push(spec.id.as_str());
    }
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "an action is named twice");
}

#[test]
fn naming_an_action_replaces_its_keys_and_naming_it_with_nothing_unbinds_it() {
    let map = Keymap::build(MAC, &table(&[("next-page", &["n"]), ("dark", &[])]));
    assert_eq!(map.problems, Vec::<String>::new());
    assert_eq!(map.action_for("n"), Some(Action::NextPage));
    assert_eq!(map.action_for("right"), None, "replaced, not added to");
    assert_eq!(map.action_for("mod+d"), None);
    // Everything not mentioned keeps what it shipped with, so a file that
    // changes one key stays one line long.
    assert_eq!(map.action_for("mod+f"), Some(Action::Find));
}

#[test]
fn a_line_that_cannot_be_used_is_named_and_the_rest_of_the_file_still_lands() {
    let map = Keymap::build(
        MAC,
        &table(&[("next-page", &["n", "mod+wibble"]), ("eat-lunch", &["x"])]),
    );
    assert_eq!(map.action_for("n"), Some(Action::NextPage));
    assert_eq!(map.problems.len(), 2, "{:?}", map.problems);
    assert!(map.problems.iter().any(|p| p.contains("mod+wibble")));
    assert!(map.problems.iter().any(|p| p.contains("eat-lunch")));
}

#[test]
fn a_key_given_to_two_things_goes_to_the_one_the_reader_wrote() {
    let map = Keymap::build(MAC, &table(&[("next-page", &["mod+f"])]));
    assert_eq!(map.action_for("mod+f"), Some(Action::NextPage));
    assert_eq!(map.by_action[&Action::Find], Vec::<String>::new());
    assert_eq!(map.problems.len(), 1, "{:?}", map.problems);
    assert!(map.problems[0].contains("find"), "{}", map.problems[0]);
}

#[test]
fn a_sequence_waits_and_never_waits_behind_a_key_that_already_does_something() {
    let shipped = Keymap::shipped(MAC);
    assert!(shipped.prefixes.contains("g"));
    assert_eq!(shipped.action_for("g g"), Some(Action::FirstPage));
    // Which is why the page field is on p: g has to be free to wait.
    assert_eq!(shipped.action_for("p"), Some(Action::GoToPage));

    // Give g to something and g g becomes unreachable — a key cannot both act
    // at once and wait to see what follows it. The shorter one keeps it.
    let clash = Keymap::build(MAC, &table(&[("go-to-page", &["g"])]));
    assert_eq!(clash.action_for("g g"), None);
    assert_eq!(clash.action_for("g"), Some(Action::GoToPage));
    assert_eq!(clash.problems.len(), 1, "{:?}", clash.problems);
    assert!(
        clash.problems[0].contains("can never be pressed"),
        "{}",
        clash.problems[0]
    );
}

/// The dispatch, which in the app is inside `wireKeyboard` and reads four
/// fields of the `App` object on its way through. Lifted out, it is a
/// function of a keystroke and what was pressed before it, and every branch of
/// it can be asked directly.
#[test]
fn a_pending_prefix_is_continued_dropped_or_used_on_its_own() {
    let map = Keymap::shipped(MAC);
    let chord = |text: &str| vec![text.to_string()];

    assert_eq!(map.resolve(&chord("g"), ""), Press::Wait("g".into()));
    assert_eq!(
        map.resolve(&chord("g"), "g"),
        Press::Act(Action::FirstPage),
        "g then g is the top of the document"
    );
    // A chord that continues nothing is not a mistake: it is a reader
    // changing their mind, so it is tried on its own.
    assert_eq!(
        map.resolve(&chord("j"), "g"),
        Press::Act(Action::ScrollDown)
    );
    assert_eq!(map.resolve(&chord("z"), "g"), Press::Nothing);
    // A modifier on its own must not clear what is waiting.
    assert_eq!(
        map.press(&Key::Shift, Code::ShiftLeft, Modifiers::SHIFT, "g"),
        Press::Wait("g".into())
    );
}

#[test]
fn what_needs_a_document_open_is_only_what_moves_around_inside_one() {
    assert!(needs_document(Action::ScrollDown));
    assert!(needs_document(Action::SelectPage));
    assert!(!needs_document(Action::Open));
    assert!(!needs_document(Action::Dismiss));
}

// The app's own table was the other half of this, and it is gone: `keys.ts`
// listed the same actions with their labels, groups and `needsDocument`, and a
// test here read that file so the two could not drift. There is one table now,
// `keymap.rs`, so the drift it guarded against cannot happen.

/* -------------------------------------------------------------- in words */

#[test]
fn a_chord_reads_the_way_the_platform_writes_it() {
    assert_eq!(describe_chord("mod+shift+f", MAC), "⇧⌘F");
    assert_eq!(describe_chord("mod+shift+f", PC), "Ctrl+Shift+F");
    assert_eq!(describe_chord("alt+left", MAC), "⌥←");
    assert_eq!(describe_chord("mod+ctrl+f", MAC), "⌃⌘F");
    assert_eq!(describe_chord("f11", MAC), "F11");
    // A bare shift and a letter is the letter: nobody reads G as ⇧G.
    assert_eq!(describe_chord("shift+g", MAC), "G");
    assert_eq!(describe_chord("j", MAC), "j");
    assert_eq!(describe_binding("g g", MAC), "g g");
}

#[test]
fn a_second_window_has_a_key_and_closing_one_is_not_quitting() {
    let shipped = Keymap::shipped(MAC);
    assert_eq!(shipped.problems, Vec::<String>::new());
    assert_eq!(shipped.action_for("mod+n"), Some(Action::NewWindow));

    let spec_of = |id: Action| {
        ACTIONS
            .iter()
            .find(|spec| spec.id == id)
            .expect("in the table")
    };
    // **⌘W is the app's own on every platform.** It was left to the menu bar
    // on a Mac, and winit installs an application menu with no Window menu in
    // it, so the key reached nothing — which is also the only way to close a
    // tab. ⌘Q is still the menu's: that one it does have.
    assert_eq!(shipped.action_for("mod+w"), Some(Action::CloseWindow));
    assert!(default_keys(spec_of(Action::Quit), MAC).is_empty());

    let elsewhere = Keymap::shipped(PC);
    assert_eq!(elsewhere.action_for("mod+w"), Some(Action::CloseWindow));
    assert_eq!(elsewhere.action_for("mod+q"), Some(Action::Quit));
    assert_eq!(elsewhere.action_for("mod+n"), Some(Action::NewWindow));
}

/* ------------------------------------------------------------- the file */

/// The template is the first thing a reader sees when they open the file, and
/// a template quoting a key the app no longer uses is worse than one quoting
/// none. In the app this test guards a copy in TypeScript against a copy in
/// TOML; here it guards a copy in Rust against the same TOML — which is the
/// same drift, one language shorter.
///
/// It also says which side of the line the two extra actions are on: they are
/// not in the app's file, and they had better not be.
#[test]
fn the_shipped_keys_toml_shows_the_keys_the_app_actually_ships_with() {
    let body = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/keys.toml"))
        .expect("the app's keys.toml");

    // Every commented-out binding, uncommented and read as the TOML it is.
    let mut uncommented = String::new();
    for line in body.lines() {
        let Some(rest) = line.strip_prefix("# ") else {
            continue;
        };
        // A binding and not prose about one: the file explains itself at
        // length, and one of its sentences ends in a binding.
        let named = rest
            .split_once(" = [")
            .map(|(name, _)| {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '-')
            })
            .unwrap_or(false);
        if named && rest.ends_with(']') {
            uncommented.push_str(rest);
            uncommented.push('\n');
        }
    }
    let shown: BTreeMap<String, Vec<String>> =
        toml::from_str(&uncommented).expect("the commented bindings are readable TOML");
    assert!(shown.len() > 30, "only found {} of them", shown.len());

    for spec in ACTIONS {
        let keys = shown
            .get(spec.id.as_str())
            .unwrap_or_else(|| panic!("{} is not in keys.toml", spec.id.as_str()));
        assert_eq!(
            keys.iter().map(String::as_str).collect::<Vec<_>>(),
            spec.keys,
            "keys.toml disagrees about {}",
            spec.id.as_str()
        );
    }
    for name in shown.keys() {
        assert!(
            ACTIONS.iter().any(|spec| spec.id.as_str() == name),
            "keys.toml offers {name}, which Moonowl cannot do"
        );
    }
    for spec in EXTRA {
        assert!(
            !shown.contains_key(spec.id.as_str()),
            "{} is this experiment's and does not belong in the app's file",
            spec.id.as_str()
        );
    }
}

/* ----------------------------------------------------- and in a document */

fn table(pairs: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
    pairs
        .iter()
        .map(|(action, keys)| {
            (
                action.to_string(),
                keys.iter().map(|key| key.to_string()).collect(),
            )
        })
        .collect()
}

fn reader_with(keys: BTreeMap<String, Vec<String>>) -> Reader {
    Reader::open_with(
        &Reader::book(),
        Options {
            keys,
            ..Default::default()
        },
    )
}

#[test]
fn a_rebound_key_reaches_the_document() {
    let mut reader = reader_with(table(&[("next-page", &["n"]), ("scroll-down", &[])]));

    // The key the reader gave it turns the page.
    reader.press("n");
    assert_eq!(reader.state().page, 2);

    // The key it replaced does not.
    reader.press("ArrowRight");
    assert_eq!(reader.state().page, 2);

    // An action unbound does nothing at all.
    let still = reader.state().scroll;
    reader.press("j");
    assert_eq!(reader.state().scroll, still);

    // Everything the file did not mention still works.
    reader.press("End");
    assert_eq!(reader.state().page, 400);
}

/// Nothing in the path throws, and the reader is told rather than left to
/// find out by pressing a key that does nothing. Both halves of the report
/// are here: `keys.rs` rejects the shapes TOML can describe and this side
/// cannot use, `keymap` rejects the chords and the action names.
#[test]
fn a_file_that_cannot_be_read_is_reported_on_the_notice_line() {
    let mut reader = reader_with(table(&[("next-page", &["n", "mod+wibble"])]));
    let notice = reader.state().notice;
    assert!(notice.starts_with("keys.toml:"), "{notice}");
    assert!(notice.contains("mod+wibble"), "{notice}");
    // …and the readable half of the same line still landed.
    reader.press("n");
    assert_eq!(reader.state().page, 2);
}

#[test]
fn the_keys_a_vim_shaped_reader_reaches_for() {
    let mut reader = Reader::open(&Reader::book());

    // h and l turn pages, the same as the arrows beside them.
    reader.press("l");
    assert_eq!(reader.state().page, 2);
    reader.press("h");
    assert_eq!(reader.state().page, 1);

    // gg and G are the two ends. Two presses, one binding: the first g waits
    // to see what follows it.
    reader.press_chord("shift+g");
    assert_eq!(reader.state().page, 400);
    reader.press("g");
    reader.press("g");
    assert_eq!(reader.state().page, 1);

    // A g that leads nowhere is dropped rather than left waiting.
    reader.press("g");
    reader.press("l");
    assert_eq!(reader.state().page, 2);

    // d and u are half a screen, which is half of what Space moves.
    reader.press("g");
    reader.press("g");
    let start = reader.state().scroll;
    reader.press("d");
    let half = reader.state().scroll - start;
    reader.press("u");
    assert_eq!(reader.state().scroll, start, "u did not put it back");
    reader.press(" ");
    let whole = reader.state().scroll - start;
    assert!(half > 0.0, "d did not move");
    assert!((whole - half * 2.0).abs() < 3.0, "{half} then {whole}");
}

/// **Every action in the table answers**, which used to be the interesting
/// thing this file said the other way round: for most of Phase 3 an action
/// this reader could not do fell through to a catch-all saying so, and the
/// keyboard was a live list of what was left. Nothing is left, so the
/// catch-all is gone and this presses the last three to go.
#[test]
fn every_action_in_the_table_answers() {
    let mut reader = Reader::open(&Reader::book());

    // ⌘P hands the document to a program that prints. Nothing here opens one
    // — see `Printer` — and what is checked is that the path went somewhere.
    reader.press_chord("mod+p");
    assert_eq!(reader.printed(), vec![Reader::book()]);
    assert_eq!(reader.state().notice, "", "and it says nothing about it");

    // F1 is the Keyboard page, which is the list of everything above.
    reader.press("F1");
    assert_eq!(reader.harness.text_content(".nav-item.on"), "Keyboard");
    reader.press("Escape");

    // ⌘D is the other half of the theme the reader chose. `tests/prefs.rs`
    // is where the pair, the machine and the switch are.
    reader.press_chord("mod+d");
    assert_eq!(reader.state().theme, "Moonowl Dark");
}

/// **Command arrives in either bit, and reading only one of them was the
/// whole of why no ⌘ chord worked in the real app.**
///
/// `keyboard_types::Modifiers` carries both `META` and `SUPER`; its own
/// `meta()` asks for `META`, and `winit_modifiers_to_kbt_modifiers` in
/// `blitz-shell` answers winit's `meta_key()` — Command, on a Mac — with
/// `SUPER`. So ⌘T arrived as a bare `t` and cycled the theme, ⌘F never opened
/// the find bar, and every test passed, because the harness was spelling the
/// chord `META` by hand. Which bit a keystroke carries is the window system's
/// business; both are Command here.
#[test]
fn command_is_read_whichever_bit_it_arrives_in() {
    for held in [Modifiers::META, Modifiers::SUPER] {
        assert_eq!(press("t", "KeyT", held), vec!["mod+t"], "{held:?}");
        assert_eq!(press("f", "KeyF", held), vec!["mod+f"], "{held:?}");
        let keymap = Keymap::shipped(MAC);
        assert_eq!(keymap.action_for("mod+t"), Some(Action::Toolbar));
    }
    // And on a PC neither of them is `mod`: that is the Windows key, which is
    // bound to nothing at all.
    assert!(press_on(PC, "j", "KeyJ", Modifiers::SUPER).is_empty());
    assert!(press_on(PC, "j", "KeyJ", Modifiers::META).is_empty());
}
