//! Filling in a form: text typed into a field, and boxes and radio buttons
//! clicked, written into the document. See `forms.rs`.

use moonowl::fixture;
use moonowl::forms::{self, Fill};
use moonowl::render::{self, FieldKind};

/// A directory of this test's own, because every test here writes files.
fn form_in(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("moonowl-forms-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory to write in");
    let path = dir.join("form.pdf");
    std::fs::copy(fixture::form_pdf(), &path).expect("a copy of the fixture");
    path.to_string_lossy().into_owned()
}

/// Each field on page 1 as (kind, value), in the page's order.
fn fields(path: &str) -> Vec<(FieldKind, String)> {
    let document = render::open(path).expect("the document opens");
    (document.fields_of(0).into_iter())
        .map(|field| (field.kind, field.value))
        .collect()
}

/// The annotation index of the field at `at` among the fields read.
fn index(path: &str, at: usize) -> usize {
    render::open(path).unwrap().fields_of(0)[at].index
}

#[test]
fn the_fields_that_can_be_filled_in_are_read() {
    let path = form_in("read");
    assert_eq!(
        fields(&path),
        [
            (FieldKind::Text { multiline: false }, "Ada".to_string()),
            (FieldKind::Text { multiline: true }, String::new()),
            (FieldKind::Check(false), String::new()),
            (FieldKind::Radio(false), String::new()),
            (FieldKind::Radio(false), String::new()),
        ],
        "the read-only field is left out"
    );
}

#[test]
fn text_typed_into_a_field_is_written() {
    let path = form_in("text");
    forms::fill(&path, 1, index(&path, 0), Fill::Text("Grace Hopper".into())).expect("filled");
    assert_eq!(fields(&path)[0].1, "Grace Hopper");
}

/// How many dark pixels the name field shows, drawn at 72 dpi.
fn ink_in_name(path: &str) -> usize {
    let drawn = render::open(path)
        .unwrap()
        .render_owned(0, 612, 792, moonowl::layout::View::WHOLE)
        .unwrap();
    let mut dark = 0;
    for y in 118..142 {
        for x in 72..300 {
            if drawn.bgra[(y * 612 + x) * 4 + 1] < 128 {
                dark += 1;
            }
        }
    }
    dark
}

#[test]
fn a_field_filled_in_is_drawn_with_its_new_words() {
    let path = form_in("drawn");
    let before = ink_in_name(&path);
    forms::fill(
        &path,
        1,
        index(&path, 0),
        Fill::Text("Grace Brewster Hopper".into()),
    )
    .unwrap();
    let after = ink_in_name(&path);
    assert!(
        after > before * 3 && before > 0,
        "{before} dark pixels, then {after}"
    );
}

#[test]
fn a_box_clicked_is_ticked_and_clicked_again_is_not() {
    let path = form_in("check");
    forms::fill(&path, 1, index(&path, 2), Fill::Click).expect("ticked");
    assert_eq!(fields(&path)[2].0, FieldKind::Check(true));
    forms::fill(&path, 1, index(&path, 2), Fill::Click).expect("cleared");
    assert_eq!(fields(&path)[2].0, FieldKind::Check(false));
}

#[test]
fn a_radio_button_clicked_turns_the_other_off() {
    let path = form_in("radio");
    forms::fill(&path, 1, index(&path, 3), Fill::Click).expect("small");
    let read = fields(&path);
    assert_eq!(
        (read[3].0, read[4].0),
        (FieldKind::Radio(true), FieldKind::Radio(false))
    );
    forms::fill(&path, 1, index(&path, 4), Fill::Click).expect("large");
    let read = fields(&path);
    assert_eq!(
        (read[3].0, read[4].0),
        (FieldKind::Radio(false), FieldKind::Radio(true))
    );
}

use moonowl::harness::Reader;

/// The middle of the name field, the check box and the two radio buttons,
/// as fractions of page 1.
const NAME: (f32, f32) = (186.0 / 612.0, 130.0 / 792.0);
const AGREE: (f32, f32) = (81.0 / 612.0, 183.0 / 792.0);
const SMALL: (f32, f32) = (81.0 / 612.0, 233.0 / 792.0);
const LARGE: (f32, f32) = (181.0 / 612.0, 233.0 / 792.0);

#[test]
fn a_click_ticks_a_box_and_undo_takes_it_back() {
    let path = form_in("click");
    let mut reader = Reader::open(&path);
    reader.click_on_page(1, AGREE);
    assert_eq!(fields(&path)[2].0, FieldKind::Check(true));
    reader.click_on_page(1, SMALL);
    reader.click_on_page(1, LARGE);
    let read = fields(&path);
    assert_eq!(
        (read[3].0, read[4].0),
        (FieldKind::Radio(false), FieldKind::Radio(true))
    );
    reader.press_chord("mod+z");
    let read = fields(&path);
    assert_eq!(
        (read[3].0, read[4].0),
        (FieldKind::Radio(true), FieldKind::Radio(false))
    );
}

#[test]
fn words_typed_into_a_field_are_written_on_enter() {
    let path = form_in("typed");
    let mut reader = Reader::open(&path);
    reader.click_on_page(1, NAME);
    assert_eq!(reader.field(".form-field"), "Ada");
    reader.type_text(" Lovelace");
    assert_eq!(fields(&path)[0].1, "Ada", "nothing is written while typing");
    reader.press("Enter");
    assert!(reader.harness.query(".form-field").is_none());
    assert_eq!(fields(&path)[0].1, "Ada Lovelace");
}

#[test]
fn a_click_elsewhere_keeps_the_words_typed() {
    let path = form_in("elsewhere");
    let mut reader = Reader::open(&path);
    reader.click_on_page(1, NAME);
    reader.type_text("!");
    reader.click_on_page(1, (0.7, 0.7));
    assert_eq!(fields(&path)[0].1, "Ada!");
}

/// A copy of a certified form, allowing `allowed`.
fn certified_in(name: &str, allowed: i64) -> String {
    let dir = std::env::temp_dir().join(format!("moonowl-forms-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("certified.pdf");
    std::fs::copy(fixture::certified_form_pdf(allowed), &path).unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn a_certified_form_is_filled_in_after_its_signed_bytes() {
    let path = certified_in("certified", 2);
    let signed = std::fs::read(&path).unwrap();
    let at = render::open(&path).unwrap().fields_of(0)[0].index;
    forms::fill(&path, 1, at, Fill::Text("Ada".into())).expect("form filling is allowed");
    let written = std::fs::read(&path).unwrap();
    assert!(
        written.starts_with(&signed),
        "the signed bytes are as they were"
    );
    assert_eq!(render::open(&path).unwrap().fields_of(0)[0].value, "Ada");
}

#[test]
fn a_certification_that_allows_no_change_refuses_a_form() {
    let path = certified_in("refused", 1);
    let before = std::fs::read(&path).unwrap();
    let at = render::open(&path).unwrap().fields_of(0)[0].index;
    assert!(forms::fill(&path, 1, at, Fill::Text("Ada".into())).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn the_reader_may_fill_in_a_form_its_certification_allows() {
    let path = certified_in("standing", 2);
    let mut reader = Reader::open(&path);
    reader.click_on_page(1, (0.2, 180.0 / 792.0));
    reader.type_text("Ada");
    reader.press("Enter");
    assert_eq!(render::open(&path).unwrap().fields_of(0)[0].value, "Ada");
}
