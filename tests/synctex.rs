//! SyncTeX: ⇧⌘-click on a page opens the LaTeX source at the line that
//! made it, and an editor's request shows the place a line made. See
//! `synctex.rs` and `app/synctex.rs`.

use dioxus::html::Modifiers;
use moonowl::fixture;
use moonowl::harness::Reader;
use moonowl::synctex::{self, Forward};

fn source() -> std::path::PathBuf {
    let pdf = fixture::synctex_pdf();
    std::fs::canonicalize(std::path::Path::new(&pdf).with_extension("tex")).unwrap()
}

#[test]
fn a_place_on_the_page_is_the_line_of_the_word_there() {
    let pdf = fixture::synctex_pdf();
    // "first", in the paragraph whose line box is tagged with line 5,
    // where it ends.
    assert_eq!(synctex::line_at(&pdf, 1, 172.0, 155.0), Ok((source(), 4)));
    assert_eq!(synctex::line_at(&pdf, 1, 140.0, 132.0), Ok((source(), 3)));
    assert_eq!(synctex::line_at(&pdf, 2, 150.0, 133.0), Ok((source(), 8)));
}

#[test]
fn a_line_is_the_place_it_made_or_the_next_line_that_made_one() {
    let pdf = fixture::synctex_pdf();
    let tex = source().to_string_lossy().into_owned();
    let (page, x, y) = synctex::place_of(&pdf, &tex, 6).unwrap().unwrap();
    assert_eq!(page, 1);
    assert!(
        (x - 133.77).abs() < 0.1 && (y - 168.54).abs() < 0.1,
        "{x}, {y}"
    );
    assert_eq!(synctex::place_of(&pdf, &tex, 8).unwrap().unwrap().0, 2);
    // `\begin{document}` made nothing; the section heading after it did.
    let (_, _, y) = synctex::place_of(&pdf, &tex, 2).unwrap().unwrap();
    assert!((y - 134.77).abs() < 0.1, "{y}");
    assert_eq!(synctex::place_of(&pdf, "/elsewhere/other.tex", 4), Ok(None));
}

/// A document compiled into a folder of its own finds its sources from the
/// folder LaTeX was run in, and a line asked for is looked for in the file
/// named by its path before a file of the same name in another folder.
#[test]
fn sources_are_found_beside_a_document_compiled_elsewhere() {
    let pdf = fixture::synctex_parts_pdf();
    let root = std::fs::canonicalize(std::env::temp_dir().join("moonowl-synctex-parts")).unwrap();
    let (one, two) = (root.join("ch1/paper.tex"), root.join("ch2/paper.tex"));
    assert_eq!(synctex::line_at(&pdf, 1, 172.0, 155.0), Ok((one, 4)));
    assert_eq!(
        synctex::line_at(&pdf, 2, 150.0, 133.0),
        Ok((two.clone(), 8))
    );
    let two = two.to_string_lossy().into_owned();
    assert_eq!(synctex::place_of(&pdf, &two, 3).unwrap().unwrap().0, 2);
}

#[test]
fn a_file_named_with_line_in_it_keeps_its_name() {
    let command = synctex::command(
        "code --goto %file:%line",
        std::path::Path::new("/a/%line.tex"),
        7,
    );
    assert_eq!(command, ["code", "--goto", "/a/%line.tex:7"]);
}

fn command_click(reader: &mut Reader, page: usize, x: f32, y: f32) {
    let (x, y) = reader.point_on(page, (x / 612.0, y / 792.0));
    let held = if cfg!(target_os = "macos") {
        Modifiers::META
    } else {
        Modifiers::CONTROL
    };
    reader.click_at_with(x, y, held | Modifiers::SHIFT);
}

#[test]
fn a_click_with_shift_and_command_opens_the_editor_at_the_line() {
    let mut reader = Reader::open(&fixture::synctex_pdf());
    command_click(&mut reader, 1, 172.0, 155.0);
    assert!(reader.opened().is_empty());
    assert_eq!(
        reader.state().notice,
        "Choose the editor SyncTeX opens in Settings, under Reading."
    );

    // Chosen in Settings, by name.
    reader.press_chord("mod+,");
    let names = reader.text_all(".segmented .segment");
    let zed = names
        .iter()
        .position(|name| name == "Zed")
        .expect("Zed is offered");
    reader.click_nth(".segmented .segment", zed);
    reader.press("Escape");

    command_click(&mut reader, 1, 172.0, 155.0);
    let wanted = format!("zed {}:4", source().display());
    assert!(
        reader.wait_until(5.0, |reader| reader.opened() == [wanted.clone()]),
        "started {:?}, told {:?}",
        reader.opened(),
        reader.state().notice
    );
}

#[test]
fn an_editors_request_shows_the_line_selected() {
    let pdf = fixture::synctex_pdf();
    let mut reader = Reader::open(&pdf);
    let tex = source().to_string_lossy().into_owned();
    synctex::wait(Forward::parse(&format!("8:0:{tex}"), &pdf).unwrap());
    reader.deliver(moonowl::emit::News {
        event: moonowl::emit::Event::SyncTexForward,
        target: None,
    });
    assert!(
        reader.wait_until(5.0, |reader| reader.state().page == 2),
        "on page {}, told {:?}",
        reader.state().page,
        reader.state().notice
    );
    reader.press_chord("mod+c");
    assert_eq!(reader.copied(), ["Text on page two, line eight."]);
}
