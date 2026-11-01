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

/// Which annotation the field at `at` among the fields read is.
fn index(path: &str, at: usize) -> render::Spot {
    render::open(path).unwrap().fields_of(0)[at].spot
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
        "the read-only and password fields are left out, since a password \
         typed over the page would be shown"
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

/// The picture that text is recognised in leaves out what the form's fields
/// show, since those words are not the page's own.
#[test]
fn text_is_recognised_without_the_words_in_the_fields() {
    let path = form_in("recognised");
    forms::fill(
        &path,
        1,
        index(&path, 0),
        Fill::Text("Grace Brewster Hopper".into()),
    )
    .unwrap();
    let grey = render::open(&path)
        .unwrap()
        .grey_aside(0, 612, 792, None)
        .unwrap();
    let mut dark = 0;
    for y in 118..142 {
        for x in 72..300 {
            if grey[y * 612 + x] < 128 {
                dark += 1;
            }
        }
    }
    let shown = ink_in_name(&path);
    assert!(
        dark * 3 < shown,
        "{dark} dark pixels to recognise, where {shown} are shown"
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

/// A field is found by its rectangle when an annotation before it has been
/// taken out since it was read, as a highlight removed while a click on it
/// waited does.
#[test]
fn a_field_moved_up_the_page_list_is_still_the_one_filled() {
    let path = form_in("moved-up");
    let mut spot = index(&path, 2);
    spot.index -= 1;
    forms::fill(&path, 1, spot, Fill::Click).expect("ticked");
    let read = fields(&path);
    assert_eq!(read[2].0, FieldKind::Check(true));
    assert_eq!(read[0].1, "Ada", "the field before it is as it was");
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
const NOTES: (f32, f32) = (236.0 / 612.0, 322.0 / 792.0);
const SMALL: (f32, f32) = (81.0 / 612.0, 233.0 / 792.0);
const LARGE: (f32, f32) = (181.0 / 612.0, 233.0 / 792.0);

/// On a view turned a quarter, the field typed into is upright, at the
/// field's own shape and the view's scale.
#[test]
fn a_field_on_a_turned_view_is_typed_into_upright() {
    let path = form_in("turned");
    let mut reader = Reader::open(&path);
    reader.click_on_page(1, NAME);
    let upright = reader.harness.layout_rect(".form-field");
    reader.press("Escape");
    reader.click(".chip.rotate-right");
    reader.settle();
    let name = format!("[data-field=\"{}\"]", index(&path, 0).index);
    reader.click(&name);
    let turned = reader.harness.layout_rect(".form-field");
    assert!(turned.width > turned.height, "{turned:?}");
    let (ratio, was) = (turned.width / turned.height, upright.width / upright.height);
    assert!((ratio - was).abs() < 0.1, "{turned:?} {upright:?}");
}

/// A box clicked again while its first click is still being written, and
/// so still drawn as it was, is ticked once.
#[test]
fn a_box_clicked_twice_while_it_is_written_is_ticked() {
    let path = form_in("clicked-twice");
    let mut reader = Reader::open(&path);
    reader.while_writing(|reader| {
        reader.click_on_page(1, AGREE);
        reader.click_on_page(1, AGREE);
        assert_eq!(
            reader.state().notice,
            "Still writing the last click into the document."
        );
    });
    reader.settle();
    assert_eq!(fields(&path)[2].0, FieldKind::Check(true));
}

#[test]
fn a_click_ticks_a_box_and_undo_takes_it_back() {
    let path = form_in("click");
    let mut reader = Reader::open(&path);
    let agree = format!("[data-field=\"{}\"]", index(&path, 2).index);
    assert_eq!(
        reader.harness.attr(&agree, "role").as_deref(),
        Some("checkbox")
    );
    assert_eq!(
        reader.harness.attr(&agree, "aria-checked").as_deref(),
        Some("false")
    );
    reader.click_on_page(1, AGREE);
    assert_eq!(fields(&path)[2].0, FieldKind::Check(true));
    assert_eq!(
        reader.harness.attr(&agree, "aria-checked").as_deref(),
        Some("true")
    );
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

/// Tab, which takes the keyboard elsewhere, writes the words and closes the
/// field, so the keys typed after it are not lost to the page.
#[test]
fn tab_writes_the_words_and_closes_the_field() {
    let path = form_in("tabbed");
    let mut reader = Reader::open(&path);
    reader.click_on_page(1, NAME);
    reader.type_text("!");
    reader.press("Tab");
    reader.settle();
    assert!(reader.harness.query(".form-field").is_none());
    assert_eq!(fields(&path)[0].1, "Ada!");
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

/// A box clicked just after typing is ticked once the words are written.
/// The press saves the words, so the click comes while that write is under
/// way.
#[test]
fn a_box_clicked_while_the_words_are_written_is_ticked_after() {
    let path = form_in("ticked-after");
    let mut reader = Reader::open(&path);
    reader.click_on_page(1, NAME);
    reader.type_text("!");
    reader.while_writing(|reader| reader.click_on_page(1, AGREE));
    let read = fields(&path);
    assert_eq!(read[0].1, "Ada!");
    assert_eq!(read[2].0, FieldKind::Check(true));
}

/// Quitting while a box clicked waits for a write keeps the window until
/// the box is written.
#[test]
fn quitting_waits_for_a_box_clicked_during_a_write() {
    let path = form_in("quit-after-click");
    let mut reader = Reader::open(&path);
    reader.click_on_page(1, NAME);
    reader.type_text("!");
    reader.while_writing(|reader| {
        reader.press("Enter");
        reader.click_on_page(1, AGREE);
        reader.close_asked(true);
        assert!(reader.asks().is_empty(), "{:?}", reader.asks());
    });
    assert_eq!(fields(&path)[2].0, FieldKind::Check(true));
}

/// A field closed while another write is under way closes at once, and
/// its words are written when that write lands.
#[test]
fn a_field_closed_while_a_write_is_under_way_is_written_after() {
    let path = form_in("closed-while-busy");
    let mut reader = Reader::open(&path);
    reader.click_on_page(1, NAME);
    reader.type_text("!");
    reader.while_writing(|reader| {
        reader.press("Enter");
        reader.click_on_page(1, NOTES);
        reader.type_text("Later");
        reader.press("Escape");
        assert!(reader.harness.query(".form-field").is_none());
    });
    reader.settle();
    let read = fields(&path);
    assert_eq!(read[0].1, "Ada!");
    assert_eq!(read[1].1, "Later");
}

/// Closing the window or quitting writes the words being typed first, and
/// the window goes only once they are written.
#[test]
fn closing_writes_the_words_being_typed_first() {
    use moonowl::app::Ask;
    let path = form_in("closing");
    let mut reader = Reader::open(&path);
    for (at, words, quit) in [(NAME, "!", false), (NOTES, "Later", true)] {
        reader.click_on_page(1, at);
        reader.type_text(words);
        match quit {
            false => reader.press_chord("mod+w"),
            true => reader.close_asked(true),
        }
        assert!(reader.asks().is_empty(), "{quit}: {:?}", reader.asks());
        reader.settle();
    }
    let read = fields(&path);
    assert_eq!(read[0].1, "Ada!");
    assert_eq!(read[1].1, "Later");
    reader.press_chord("mod+w");
    reader.settle();
    assert_eq!(reader.asks(), [Ask::Close]);
}

/// A field opened and left as it was does not keep the window while the
/// document reloads.
#[test]
fn an_unchanged_field_does_not_keep_the_window() {
    use moonowl::app::Ask;
    let path = form_in("unchanged-close");
    let mut reader = Reader::open(&path);
    reader.click_on_page(1, NAME);
    reader.while_writing(|reader| {
        reader.document_changed(&path);
        reader.press_chord("mod+w");
        assert_eq!(reader.asks(), [Ask::Close], "{}", reader.state().notice);
    });
}

/// A new draft from outside closes the field being typed in: its words were
/// meant for a field of the draft that has gone, and the same place in the
/// new one may be another field, or none.
#[test]
fn a_new_draft_closes_the_field_being_typed_in() {
    let path = form_in("redrafted");
    let mut reader = Reader::open(&path);
    reader.click_on_page(1, NAME);
    reader.type_text(" Lovelace");
    fixture::draft(std::path::Path::new(&path), 2);
    reader.document_changed(&path);
    assert!(reader.harness.query(".form-field").is_none());
    assert_eq!(
        reader.state().notice,
        "The document changed while a field was being filled in, so the words were not saved."
    );
    reader.press("Enter");
    reader.settle();
    let bytes = std::fs::read(&path).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("Lovelace"));
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
    let at = render::open(&path).unwrap().fields_of(0)[0].spot;
    forms::fill(&path, 1, at, Fill::Text("Ada".into())).expect("form filling is allowed");
    let written = std::fs::read(&path).unwrap();
    assert!(
        written.starts_with(&signed),
        "the signed bytes are as they were"
    );
    assert_eq!(render::open(&path).unwrap().fields_of(0)[0].value, "Ada");
}

/// A field the signature locks (its `/Lock`) is not filled in, and one it
/// does not lock is.
#[test]
fn a_field_a_signature_locks_is_not_filled_in() {
    use lopdf::dictionary;
    for (locks, refused) in [("Name", true), ("Other", false)] {
        let path = certified_in(&format!("locked-{locks}"), 2);
        let mut document = lopdf::Document::load(&path).unwrap();
        for object in document.objects.values_mut() {
            let Ok(field) = object.as_dict_mut() else {
                continue;
            };
            if field.get(b"FT").and_then(|t| t.as_name()).ok() == Some(b"Sig") {
                field.set(
                    "Lock",
                    dictionary! {
                        "Type" => "SigFieldLock", "Action" => "Include",
                        "Fields" => vec![lopdf::Object::string_literal(locks)],
                    },
                );
            }
        }
        document.save(&path).unwrap();
        let at = render::open(&path).unwrap().fields_of(0)[0].spot;
        let filled = forms::fill(&path, 1, at, Fill::Text("Ada".into()));
        assert_eq!(filled.is_err(), refused, "{locks}: {filled:?}");
        let value = &render::open(&path).unwrap().fields_of(0)[0].value;
        assert_eq!(value.is_empty(), refused, "{locks}");
    }
}

#[test]
fn a_certification_that_allows_no_change_refuses_a_form() {
    let path = certified_in("refused", 1);
    let before = std::fs::read(&path).unwrap();
    let at = render::open(&path).unwrap().fields_of(0)[0].spot;
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

/// "Add text here…" opens an empty field where the right-click was, and
/// what is typed there is written on the page as a line of text, with undo.
#[test]
fn text_is_added_where_the_right_click_was() {
    let path = form_in("added");
    let mut reader = Reader::open(&path);
    reader.right_click_on_page(1, (0.2, 0.8));
    reader.click(".menu.context [data-item='add-text']");
    assert_eq!(reader.field(".form-field"), "");
    reader.type_text("Berlin");
    reader.press("Enter");
    assert!(reader.harness.query(".form-field").is_none());
    let lines = || render::open(&path).unwrap().signatures();
    let added = lines();
    assert_eq!(added.len(), 1, "{}", reader.state().notice);
    assert_eq!(added[0].by, "Berlin");
    let size = render::open(&path).unwrap().size_of(0);
    let middle = added[0].at.top + added[0].at.height / 2.0;
    assert!((middle / size.height - 0.8).abs() < 0.02, "{middle}");
    assert!((added[0].at.left / size.width - 0.2).abs() < 0.02);
    reader.press_chord("mod+z");
    assert!(lines().is_empty());
}

/// On a view turned a quarter, the field opens across the page as it is
/// seen, from the right-click, and the line is written there, reading the
/// same way.
#[test]
fn text_added_on_a_turned_view_reads_across_it() {
    let path = form_in("added-turned");
    let (_, before) = dark_split(&path, |_, _| false);
    let mut reader = Reader::open(&path);
    reader.click(".chip.rotate-right");
    reader.settle();
    reader.right_click_on_page(1, (0.2, 0.8));
    reader.click(".menu.context [data-item='add-text']");
    let field = reader.harness.layout_rect(".form-field");
    assert!(field.width > 4.0 * field.height, "{field:?}");
    reader.type_text("Berlin");
    reader.press("Enter");
    let added = render::open(&path).unwrap().signatures();
    assert_eq!(added.len(), 1, "{}", reader.state().notice);
    // Turned right, the click is 80% down the page and 80% across it, and
    // the line runs up the page from there.
    let size = render::open(&path).unwrap().size_of(0);
    let at = added[0].at;
    assert!(at.height > at.width, "{at:?}");
    let middle = at.left + at.width / 2.0;
    assert!((middle / size.width - 0.8).abs() < 0.02, "{at:?}");
    assert!(
        ((at.top + at.height) / size.height - 0.8).abs() < 0.02,
        "{at:?}"
    );
    // The words are drawn inside that box, so they run up the page too.
    let inside = |x: usize, y: usize| {
        let (x, y) = (x as f64 + 0.5, y as f64 + 0.5);
        x >= at.left && x <= at.left + at.width && y >= at.top && y <= at.top + at.height
    };
    let (within, outside) = dark_split(&path, inside);
    assert!(within > 20, "{within}");
    assert_eq!(outside, before);
}

/// How many dark pixels page 1 shows at 72 dpi where `inside` says, and
/// how many elsewhere.
fn dark_split(path: &str, inside: impl Fn(usize, usize) -> bool) -> (usize, usize) {
    let drawn = render::open(path)
        .unwrap()
        .render_owned(0, 612, 792, moonowl::layout::View::WHOLE)
        .unwrap();
    let mut counts = (0, 0);
    for y in 0..792 {
        for x in 0..612 {
            if drawn.bgra[(y * 612 + x) * 4 + 1] < 128 {
                match inside(x, y) {
                    true => counts.0 += 1,
                    false => counts.1 += 1,
                }
            }
        }
    }
    counts
}

/// A field closed with nothing typed in it writes nothing.
#[test]
fn an_empty_added_field_writes_nothing() {
    let path = form_in("added-empty");
    let before = std::fs::read(&path).unwrap();
    let mut reader = Reader::open(&path);
    reader.right_click_on_page(1, (0.2, 0.8));
    reader.click(".menu.context [data-item='add-text']");
    reader.press("Escape");
    assert!(reader.harness.query(".form-field").is_none());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

/// "Add text here…" on a document that cannot take an annotation says why,
/// and opens no field.
#[test]
fn no_text_is_added_to_an_encrypted_or_certified_document() {
    for (fixture, name) in [
        (fixture::restricted_pdf(), "encrypted"),
        (fixture::certified_form_pdf(2), "certified"),
    ] {
        let dir = std::env::temp_dir().join(format!(
            "moonowl-forms-{}-no-text-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("refused.pdf");
        std::fs::copy(&fixture, &path).unwrap();
        let path = path.to_string_lossy().into_owned();
        let before = std::fs::read(&path).unwrap();
        let mut reader = Reader::open(&path);
        reader.right_click_on_page(1, (0.2, 0.8));
        reader.click(".menu.context [data-item='add-text']");
        assert!(reader.harness.query(".form-field").is_none(), "{name}");
        assert!(
            reader
                .state()
                .notice
                .ends_with("so no text can be added to it."),
            "{name}: {}",
            reader.state().notice
        );
        assert_eq!(std::fs::read(&path).unwrap(), before, "{name}");
    }
}

/// A field opened again while its words wait to be written opens with
/// those words, not the ones in the file, and a field whose words are being
/// written does not open until they are.
#[test]
fn a_field_opened_again_before_its_words_are_written_shows_them() {
    let path = form_in("opened-again");
    let mut reader = Reader::open(&path);
    reader.click_on_page(1, NAME);
    reader.type_text("!");
    reader.while_writing(|reader| {
        reader.press("Enter");
        reader.click_on_page(1, NAME);
        assert!(reader.harness.query(".form-field").is_none());
        assert!(reader.state().notice.starts_with("Still writing the words"));
        reader.click_on_page(1, NOTES);
        reader.type_text("Later");
        reader.press("Escape");
        reader.click_on_page(1, NOTES);
        assert_eq!(reader.field(".form-field"), "Later");
        reader.type_text(" on");
        reader.press("Escape");
    });
    reader.settle();
    let read = fields(&path);
    assert_eq!(read[0].1, "Ada!");
    assert_eq!(read[1].1, "Later on");
}

/// A radio button that is on is not written again when it is clicked.
#[test]
fn a_radio_button_that_is_on_writes_nothing_when_clicked() {
    let path = form_in("radio-on");
    let mut reader = Reader::open(&path);
    reader.click_on_page(1, SMALL);
    reader.settle();
    assert_eq!(fields(&path)[3].0, FieldKind::Radio(true));
    let before = std::fs::read(&path).unwrap();
    reader.click_on_page(1, SMALL);
    reader.settle();
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

/// A document that takes no filled-in form says so when a field is
/// clicked, before anything is typed into it.
#[test]
fn a_field_that_cannot_be_filled_in_does_not_open() {
    let path = certified_in("no-field", 1);
    let mut reader = Reader::open(&path);
    reader.settle();
    reader.click_on_page(1, (0.2, 180.0 / 792.0));
    assert!(reader.harness.query(".form-field").is_none());
    assert!(
        (reader.state().notice).ends_with("so the form cannot be filled in here."),
        "{}",
        reader.state().notice
    );
}
