//! A document behind a password: asking for one, refusing a wrong one, and
//! what happens to the reader who declines.
//!
//! `ui.askForPassword` in the app, which was the largest thing left on
//! `PROGRESS.md`'s list of what this port did not have — an encrypted document
//! opened as an error, in a reader whose whole premise is that it opens what
//! you give it.
//!
//! The fixture is written here rather than found: [`fixture::locked_pdf`] is
//! the PDF standard security handler at revision 2, RC4 at 40 bits, which is
//! forty lines of MD5 and RC4 and no dependency. See its own comment for why
//! the weakest variant in the spec is the right one to test against.

use moonowl::app::Ask;
use moonowl::fixture::{self, LOCKED_PASSWORD};
use moonowl::harness::Reader;

/// MD5 against the RFC's own vectors, because everything below stands on it:
/// a key derivation that is quietly wrong makes a fixture no reader can open,
/// and the failure would look exactly like the feature not working.
#[test]
fn the_digest_the_key_is_derived_with_is_md5() {
    for (input, expected) in [
        ("", "d41d8cd98f00b204e9800998ecf8427e"),
        ("a", "0cc175b9c0f1b6a831c399e269772661"),
        ("abc", "900150983cd24fb0d6963f7d28e17f72"),
        ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
        (
            "abcdefghijklmnopqrstuvwxyz",
            "c3fcd3d76192e4007dfb496cca67e13b",
        ),
        (
            "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
            "57edf4a22be3c955ac49da2e2107b67a",
        ),
    ] {
        assert_eq!(
            fixture::digest_for_test(input.as_bytes()),
            expected,
            "md5 of {input:?}",
        );
    }
}

/// The renderer's own answer, under the interface: locked is a *kind* of
/// refusal and not a sentence, which is what lets everything above it tell a
/// question from a failure.
#[test]
fn pdfium_says_locked_rather_than_broken() {
    let path = fixture::locked_pdf();
    assert_eq!(
        moonowl::render::open(&path).err(),
        Some(moonowl::render::Refusal::Locked),
        "a locked document with no password",
    );
    let wrong = moonowl::render::open_with(&path, Some("not the password"));
    assert_eq!(
        wrong.err(),
        Some(moonowl::render::Refusal::Locked),
        "and a wrong one, which pdfium reports identically",
    );
    let opened = moonowl::render::open_with(&path, Some(LOCKED_PASSWORD))
        .unwrap_or_else(|err| panic!("the password did not open it: {err}"));
    assert_eq!(opened.pages(), 3);
    assert!(opened.encrypted(), "and it knows it came in through a lock");
}

/// A window made on a locked document comes up empty with the question over
/// it. `Session::window_on` is what makes one: every other refusal is a line
/// on the terminal and no window at all, because there is nothing a reader
/// could do about a file that is missing.
#[test]
fn a_locked_document_is_asked_about_rather_than_refused() {
    let reader = Reader::locked(&fixture::locked_pdf());
    let window = reader.harness.text_content(".ask-window");
    assert!(
        window.contains("This document is locked"),
        "the window's own title: {window:?}",
    );
    assert!(
        window.contains("It needs a password before it can be opened."),
        "and the app's own first sentence: {window:?}",
    );
    assert!(
        reader.state().empty,
        "and nothing is open behind it until it is answered",
    );
}

/// The answer opens it, and the reader lands in the document.
#[test]
fn the_password_opens_it() {
    let mut reader = Reader::locked(&fixture::locked_pdf());
    reader.type_text(LOCKED_PASSWORD);
    reader.press("Enter");
    assert!(
        reader.harness.query(".ask-window").is_none(),
        "the question is answered and gone",
    );
    let state = reader.state();
    assert!(!state.empty, "and there is a document: {state:?}");
    assert_eq!(state.pages, 3);
}

/// A wrong one says so and asks again, which is the app's second sentence and
/// the reason [`Locked::wrong`] exists at all: pdfium reports "needs a
/// password" and "that was not it" as the same error, and the difference is
/// whether one was supplied.
#[test]
fn a_wrong_password_says_so_and_asks_again() {
    let mut reader = Reader::locked(&fixture::locked_pdf());
    reader.type_text("moonowl but wrong");
    reader.press("Enter");
    let window = reader.harness.text_content(".ask-window");
    assert!(
        window.contains("That password was not right. Try again."),
        "the second sentence: {window:?}",
    );
    assert!(reader.state().empty, "and still nothing behind it");

    // And the field is empty again, so the next answer is typed rather than
    // edited into the last one.
    reader.type_text(LOCKED_PASSWORD);
    reader.press("Enter");
    assert!(
        reader.harness.query(".ask-window").is_none(),
        "the right one still works after a wrong one",
    );
    assert_eq!(reader.state().pages, 3);
}

/// What is drawn is bullets. Blitz builds a text editor for
/// `type="password"` and gives it the right accessibility role, and it does
/// not mask it — so the reader does, by taking the ink out of the field and
/// drawing one bullet a character over the top. The password is in the field's
/// value while the question is up, which is where a browser keeps it too.
#[test]
fn the_field_shows_bullets_and_not_the_password() {
    let mut reader = Reader::locked(&fixture::locked_pdf());
    reader.type_text("secret");
    assert_eq!(
        reader.text_all(".ask-bullets"),
        vec!["••••••".to_string()],
        "one bullet a character",
    );
    // And what is tried is the password rather than the bullets, which is the
    // half that would break silently: six bullets are a wrong password too.
    reader.press("Enter");
    let window = reader.harness.text_content(".ask-window");
    assert!(
        window.contains("That password was not right"),
        "it tried what was typed: {window:?}",
    );
}

/// "Not now" withdraws the question and leaves the reader with what they had.
///
/// **Declining is not answering with an empty password**, which is the app's
/// own hard-won note about pdf.js: a reader on their way out of the question
/// must not be asked it again.
#[test]
fn not_now_leaves_the_reader_where_they_were() {
    let mut reader = Reader::locked(&fixture::locked_pdf());
    reader.click("[data-item='not-now']");
    assert!(
        reader.harness.query(".ask-window").is_none(),
        "the question is withdrawn",
    );
    assert!(
        reader.state().empty,
        "and the window is the empty one again"
    );
}

/// Escape does the same, from inside the field — which is where the reader
/// actually is when they press it.
#[test]
fn escape_withdraws_it_too() {
    let mut reader = Reader::locked(&fixture::locked_pdf());
    reader.press("Escape");
    assert!(
        reader.harness.query(".ask-window").is_none(),
        "Escape closes it, like every other window over the reader",
    );
}

/// A locked document handed to a reader that is already reading one: the
/// question goes up over the document they have, and declining leaves it
/// exactly as it was. Nothing is ever displaced by a document that has not
/// been opened.
#[test]
fn a_reader_who_declines_keeps_the_document_they_had() {
    let mut reader = Reader::open(&Reader::book());
    let was = reader.state().pages;
    assert!(was > 3, "the book, which is not the locked fixture");

    reader.hand_over(&fixture::locked_pdf());
    let window = reader.harness.text_content(".ask-window");
    assert!(
        window.contains("It needs a password"),
        "the question, over the document already open: {window:?}",
    );
    assert_eq!(
        reader.state().pages,
        was,
        "and the book is still what is behind it",
    );

    reader.click("[data-item='not-now']");
    assert_eq!(reader.state().pages, was, "and still is, after declining",);
}

/// While the question is up, the desk is told the window is showing the
/// document it asks about — and, declined, what it showed before. Down as
/// empty, an asking window was where the desk sent the next document handed
/// over, and the window sent it back: a loop that spun the event loop.
#[test]
fn asking_is_showing_as_far_as_the_desk_knows() {
    let book = Reader::book();
    let locked = fixture::locked_pdf();
    let mut reader = Reader::open(&book);
    reader.hand_over(&locked);
    let showing = |reader: &Reader| -> Vec<String> {
        reader
            .asks()
            .into_iter()
            .filter_map(|ask| match ask {
                Ask::Showing { path, .. } => Some(path),
                _ => None,
            })
            .collect()
    };
    assert_eq!(showing(&reader), vec![locked.clone()], "asked for is shown");
    reader.click("[data-item='not-now']");
    assert_eq!(
        showing(&reader),
        vec![locked.clone(), book.clone()],
        "declined, the book is shown again"
    );

    let mut reader = Reader::locked(&locked);
    reader.click("[data-item='not-now']");
    assert_eq!(
        showing(&reader),
        vec![String::new()],
        "an empty window is empty again"
    );
}

/// And answering it swaps the document, which is ⌘O's own path — the same
/// `Ask::Showing` afterwards, so the desk, the restore list, the watch and the
/// window's title all move with it.
#[test]
fn answering_it_opens_the_document_in_place() {
    let mut reader = Reader::open(&Reader::book());
    reader.hand_over(&fixture::locked_pdf());
    reader.type_text(LOCKED_PASSWORD);
    reader.click("[data-item='unlock']");
    assert_eq!(reader.state().pages, 3, "the locked document, opened");
    assert!(
        reader
            .asks()
            .iter()
            .any(|ask| matches!(ask, moonowl::app::Ask::Showing { path, .. }
                if path.ends_with("moonowl-locked.pdf"))),
        "and the process was told, exactly as ⌘O tells it: {:?}",
        reader.asks(),
    );
}

/// **Markup does not go into an encrypted document.** pdfium's only way to
/// write one back is `FPDF_SaveAsCopy`, which is a full rewrite, and what that
/// makes of a document opened with a password is not a thing to find out over
/// somebody's file. So the mark is kept beside the document instead, which is
/// what the journal has always been for, and the reader is told in one line.
#[test]
fn a_mark_on_an_encrypted_document_stays_beside_it() {
    let opened = moonowl::render::open_with(&fixture::locked_pdf(), Some(LOCKED_PASSWORD))
        .expect("the password opens it");
    let standing = moonowl::markup::standing(opened.path(), opened.encrypted(), opened.sealed());
    assert!(!standing.into_file, "nothing is written into it");
    assert_eq!(standing.refused, "this document is encrypted");
}

/// Encrypted is asked of pdfium, not inferred from whether a password was
/// needed: an owner-password-only document opens with none, and is still a
/// file this reader must not rewrite. See `markup::standing`.
#[test]
fn a_document_under_an_owner_password_alone_is_still_encrypted() {
    let restricted =
        moonowl::render::open(&fixture::restricted_pdf()).expect("it opens with no password");
    assert!(restricted.encrypted());
    let plain = moonowl::render::open(&fixture::prose_pdf()).expect("a plain document");
    assert!(!plain.encrypted());
}

/// **Nothing behind the question answers the keyboard.** The password field
/// lets a chord through, and ⌘F put a find bar over the prompt that then
/// took the typing.
#[test]
fn a_chord_does_not_reach_the_document_behind_the_question() {
    let mut reader = Reader::open(&Reader::book());
    reader.hand_over(&fixture::locked_pdf());
    assert!(reader.harness.query(".ask-window").is_some());

    reader.press_chord("mod+f");
    assert_eq!(reader.state().find, None, "a find bar came up behind it");
}

/// **A file that is not a PDF is said in a sentence**, not in pdfium's
/// `PdfiumLibraryInternalError(FormatError)`.
#[test]
fn a_file_that_is_not_a_pdf_is_said_plainly() {
    let path = std::env::temp_dir().join(format!("moonowl-{}-notes.pdf", std::process::id()));
    std::fs::write(&path, "just some text").expect("write");
    let said = match moonowl::render::open(&path.to_string_lossy()) {
        Err(refused) => refused.to_string(),
        Ok(_) => panic!("opened a text file"),
    };
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    assert_eq!(said, format!("{name} is not a PDF, or it is damaged."));
    let _ = std::fs::remove_file(&path);
}
