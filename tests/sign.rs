//! Signing a document with ink: the store, what lands in the file, and where.
//!
//! The other half of the word is not here and cannot be — see `sign.rs`'s own
//! note and `signing-assessment.md`. What is tested is the half this reader
//! does: a signature drawn once, kept, and dropped onto a page as the
//! specification's own `/Ink` annotation.

use moonowl::render::{self, Rect};
use moonowl::sign::{self, Signature};

/// A copy of the plain fixture, in a directory of this test's own — everything
/// here writes to the document, and the fixtures are shared.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("moonowl-sign-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory to write in");
    let path = dir.join("signed.pdf");
    moonowl::fixture::draft(&path, 3);
    path
}

/// A config directory of this test's own, handed to the store rather than set
/// in the environment — see `sign::dir`, and the reason it takes a path.
fn own_config(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("moonowl-signs-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a config directory");
    dir
}

/// Something that looks like a name written by hand: three strokes, none of
/// them straight, filling most of the unit box.
fn scrawl() -> Signature {
    let curve = |from: f64, to: f64, height: f64| -> Vec<[f64; 2]> {
        (0..24)
            .map(|step| {
                let along = step as f64 / 23.0;
                let x = from + (to - from) * along;
                [x, height + 0.18 * (along * std::f64::consts::TAU).sin()]
            })
            .collect()
    };
    Signature {
        name: "A Reader".to_string(),
        id: String::new(),
        strokes: vec![
            curve(0.02, 0.44, 0.5),
            curve(0.46, 0.98, 0.42),
            // The crossbar, which is a straight line and is the stroke that
            // would be lost by anything that assumed a signature curves.
            vec![[0.10, 0.86], [0.90, 0.86]],
        ],
    }
}

/* --------------------------------------------------------------- the store */

#[test]
fn a_signature_is_a_file_that_comes_back() {
    let config = own_config("roundtrip");
    assert!(sign::load_all(&config).is_empty(), "nothing kept yet");

    let stored = sign::save(&config, &scrawl()).expect("it is kept");
    assert_eq!(stored.id, "a-reader", "the id is made from the name");

    let back = sign::load_all(&config);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].name, "A Reader");
    assert_eq!(back[0].id, "a-reader");
    assert_eq!(back[0].strokes.len(), 3, "all three strokes");

    sign::forget(&config, "a-reader").expect("it goes");
    assert!(sign::load_all(&config).is_empty(), "and it is gone");
}

#[test]
fn two_signatures_of_the_same_name_do_not_replace_each_other() {
    let config = own_config("twice");
    let first = sign::save(&config, &scrawl()).expect("kept");
    let second = sign::save(&config, &scrawl()).expect("kept again");
    assert_eq!(first.id, "a-reader");
    assert_eq!(second.id, "a-reader-2");
    assert_eq!(sign::load_all(&config).len(), 2, "both are there");
}

#[test]
fn a_pad_nobody_drew_on_is_refused() {
    let config = own_config("empty");
    let nothing = Signature {
        name: "Nobody".to_string(),
        ..Default::default()
    };
    assert!(
        sign::save(&config, &nothing).is_err(),
        "there is nothing to keep"
    );
    assert!(sign::load_all(&config).is_empty());
}

/// **Trimming happens on the way in, so what is on disk is what will be
/// drawn.** A signature drawn in the middle of a pad is not a smaller
/// signature; it is the same one, and dropping it at 40 points should give 40
/// points of ink rather than 40 points of mostly-empty box.
///
/// The fixture is square, deliberately, so that "0 to 1 on both axes" is the
/// right answer here and stays the right answer under a trim that keeps the
/// shape — which is what `a_wide_name_keeps_its_shape` below is about.
#[test]
fn a_signature_drawn_in_a_corner_is_kept_at_its_own_size() {
    let config = own_config("trim");
    let cramped = Signature {
        name: "Small".to_string(),
        id: String::new(),
        strokes: vec![vec![[0.4, 0.4], [0.6, 0.5], [0.4, 0.6]]],
    };
    let stored = sign::save(&config, &cramped).expect("kept");
    let xs: Vec<f64> = stored.strokes[0].iter().map(|p| p[0]).collect();
    let ys: Vec<f64> = stored.strokes[0].iter().map(|p| p[1]).collect();
    let span = |v: &[f64]| {
        let lo = v.iter().cloned().fold(f64::MAX, f64::min);
        let hi = v.iter().cloned().fold(f64::MIN, f64::max);
        (lo, hi)
    };
    assert_eq!(span(&xs), (0.0, 1.0), "stretched across");
    assert_eq!(span(&ys), (0.0, 1.0), "and down");
}

/// A straight line has no height, and dividing by it would put every point at
/// infinity — which reads back out of the file as an annotation nothing draws.
#[test]
fn a_signature_that_is_one_straight_line_survives_being_trimmed() {
    let flat = Signature {
        name: "Line".to_string(),
        id: String::new(),
        strokes: vec![vec![[0.1, 0.5], [0.9, 0.5]]],
    };
    let trimmed = flat.trimmed();
    for point in trimmed.strokes.iter().flatten() {
        assert!(point[0].is_finite() && point[1].is_finite(), "{point:?}");
        assert!((0.0..=1.0).contains(&point[1]), "{point:?}");
    }
    assert_eq!(flat.aspect(), 1.0, "and it has a width to be drawn at");
}

/// **One scale for both axes, and it is the whole of what `trimmed` gets
/// right.** Stretching each axis to 0-1 separately throws the shape away, so
/// a name written across a pad comes back square — which is invisible to
/// anything that asks whether a signature is there, and obvious the moment one
/// is looked at.
#[test]
fn a_wide_name_keeps_its_shape() {
    let wide = Signature {
        name: "Wide".to_string(),
        id: String::new(),
        // Four across, one down.
        strokes: vec![vec![[10.0, 20.0], [50.0, 30.0], [90.0, 20.0]]],
    };
    let trimmed = wide.trimmed();
    let xs: Vec<f64> = trimmed.strokes[0].iter().map(|point| point[0]).collect();
    let ys: Vec<f64> = trimmed.strokes[0].iter().map(|point| point[1]).collect();
    let hi = |v: &[f64]| v.iter().cloned().fold(f64::MIN, f64::max);
    assert_eq!(hi(&ys), 1.0, "one unit tall, which is the unit");
    assert_eq!(
        hi(&xs),
        8.0,
        "and eight units wide, because that is its shape"
    );
    assert_eq!(trimmed.aspect(), 8.0);
    // And the shape is the same before and after, which is the property that
    // matters — the numbers above are one instance of it.
    assert!((wide.aspect() - trimmed.aspect()).abs() < 1e-9);
}

/* ------------------------------------------------------------- the document */

#[test]
fn a_signature_dropped_on_a_page_is_ink_in_the_file() {
    let path = scratch("written");
    let file = path.to_str().unwrap();
    let document = render::open(file).expect("the fixture opens");
    assert!(document.signatures().is_empty(), "nothing signed yet");
    drop(document);

    sign::place(
        file,
        2,
        Rect {
            left: 90.0,
            top: 640.0,
            width: 0.0,
            height: 48.0,
        },
        &scrawl().trimmed(),
        sign::INK,
    )
    .expect("the signature is written");

    // Read back through a second open, which is the only thing that proves it
    // is in the *file* rather than in a list this process is holding.
    let again = render::open(file).expect("the document reopens");
    let placed = again.signatures();
    assert_eq!(placed.len(), 1, "one signature");
    assert_eq!(placed[0].page, 2, "on the page it was dropped on");
    assert_eq!(placed[0].by, "A Reader", "and it says whose it is");
}

/// Not merely present: **the right way up**. pdfium counts from the bottom of
/// a page and everything in this crate counts from the top, and a flip done
/// once too often is a signature upside down at the other end of the page —
/// which would still read back as one ink annotation with the right name.
#[test]
fn the_signature_lands_where_it_was_dropped() {
    let path = scratch("placed");
    let file = path.to_str().unwrap();
    let wanted = Rect {
        left: 90.0,
        top: 640.0,
        width: 0.0,
        height: 48.0,
    };
    sign::place(file, 1, wanted, &scrawl().trimmed(), sign::INK).expect("written");

    let again = render::open(file).expect("reopened");
    let landed = again.signatures()[0].at;
    // Within a nib, which is what `box_of` pads by so that a stroke drawn
    // centred on its path is not shaved by a viewer clipping to `/Rect`.
    let nib = 48.0 / 28.0 + 0.5;
    assert!(
        (landed.left - wanted.left).abs() < nib,
        "left: {landed:?} against {wanted:?}",
    );
    assert!(
        (landed.top - wanted.top).abs() < nib,
        "top: {landed:?} against {wanted:?}",
    );
    assert!(
        (landed.height - wanted.height).abs() < 2.0 * nib,
        "height: {landed:?} against {wanted:?}",
    );
    // The width is the signature's own shape rather than anything passed in —
    // a signature is drawn to a height and is whatever width its hand makes
    // it, which is the whole reason `place` ignores the width it is given.
    let expected = wanted.height * scrawl().trimmed().aspect();
    assert!(
        (landed.width - expected).abs() < 2.0 * nib,
        "width: {landed:?}, wanted about {expected}",
    );
}

/// A signature comes off again, which is the thing the app cannot offer for
/// markup at all — `Annotation.save()` is not overridden by any subtype there,
/// so nothing already in a file can be removed through `saveDocument()`. Here
/// it is one call, and it is the same one a highlight comes out through.
#[test]
fn a_signature_can_be_taken_off_again() {
    let path = scratch("removed");
    let file = path.to_str().unwrap();
    sign::place(
        file,
        1,
        Rect {
            left: 80.0,
            top: 600.0,
            width: 0.0,
            height: 40.0,
        },
        &scrawl().trimmed(),
        sign::INK,
    )
    .expect("written");
    let placed = render::open(file).expect("reopened").signatures();
    assert_eq!(placed.len(), 1);

    moonowl::markup::remove(file, placed[0].page, placed[0].index).expect("taken off");
    assert!(
        render::open(file)
            .expect("reopened again")
            .signatures()
            .is_empty(),
        "and it is gone from the file",
    );
}

/// **Two signatures on one page are two annotations**, and the second does not
/// disturb the first — which is the case that would break if the write were
/// building the page's annotations rather than adding to them.
#[test]
fn a_second_signature_joins_the_first() {
    let path = scratch("two");
    let file = path.to_str().unwrap();
    let mark = scrawl().trimmed();
    for top in [560.0, 640.0] {
        sign::place(
            file,
            1,
            Rect {
                left: 80.0,
                top,
                width: 0.0,
                height: 40.0,
            },
            &mark,
            sign::INK,
        )
        .expect("written");
    }
    let placed = render::open(file).expect("reopened").signatures();
    assert_eq!(placed.len(), 2, "both are there");
    assert_ne!(placed[0].at.top, placed[1].at.top, "in two places");
}

/// **What signing this document would mean, asked before it is offered.** The
/// fixture is an ordinary file in a writable directory, so all three answers
/// are the easy ones; the point of the test is that the question is asked at
/// all and that `rewrites` is false for a document carrying no signature.
#[test]
fn an_ordinary_document_can_be_signed_and_says_so() {
    let path = scratch("standing");
    let standing = sign::standing(path.to_str().unwrap(), false, false);
    assert!(standing.into_file, "{}", standing.refused);
    assert!(standing.refused.is_empty());
    assert!(!standing.rewrites, "it carries no signature to break");
}

/// An encrypted document is refused before the disk is asked, for the reason
/// markup refuses one: the save is `FPDF_SaveAsCopy`, and what that produces
/// for a document opened with a password is not a question worth guessing at
/// over somebody's file.
#[test]
fn an_encrypted_document_is_not_signed() {
    let path = scratch("locked");
    let standing = sign::standing(path.to_str().unwrap(), true, false);
    assert!(!standing.into_file);
    assert_eq!(standing.refused, "this document is encrypted");
}

/* -------------------------------------------------------- and in the app */

mod through_the_reader {
    use moonowl::harness::{Options, Reader};
    use moonowl::render;

    /// A reader over a document of its own, with a config directory of its
    /// own — signing writes to both.
    fn reader(name: &str) -> (Reader, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("moonowl-signui-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        let pdf = dir.join("doc.pdf");
        moonowl::fixture::draft(&pdf, 3);
        let reader = Reader::open_with(
            pdf.to_str().expect("a path"),
            Options {
                width: 1100,
                height: 800,
                config: dir.clone(),
                ..Default::default()
            },
        );
        (reader, pdf)
    }

    /// A name across the pad: wide, curved, and nothing like a square — which
    /// is what makes it able to say whether the shape survived.
    fn wave() -> Vec<(f32, f32)> {
        (0..30)
            .map(|step| {
                let along = step as f32 / 29.0;
                (
                    0.08 + 0.84 * along,
                    0.5 + 0.3 * (along * std::f32::consts::TAU).sin(),
                )
            })
            .collect()
    }

    fn open_the_window(reader: &mut Reader) {
        // **Widened for the click and given straight back.** The window opens
        // off the Document menu, and that menu hangs off the document's name,
        // which has no room in a 1100px bar — see `tests/menus.rs`. Everything
        // below is measured at 1100, where the pad and the page are the sizes
        // these tests were written against, so the bar is borrowed rather than
        // kept.
        reader.resize(1280, 800);
        reader.click(".chip.title");
        let items = reader.text_all(".menu.document .menu-item");
        let at = items
            .iter()
            .position(|label| label.starts_with("Sign"))
            .expect("the Document menu offers signing");
        reader.click_nth(".menu.document .menu-item", at);
        reader.resize(1100, 800);
    }

    /// **A document that is already signed says so, in the window that is
    /// about to break it.**
    ///
    /// The Sign window's first sentence says this is not a digital signature.
    /// The place to say what the document's *own* digital signatures are is
    /// directly under that sentence, and a reader who came here wanting the
    /// green tick should meet them before they meet the pad.
    #[test]
    fn the_window_says_what_the_document_is_already_signed_with() {
        let dir = std::env::temp_dir().join(format!("moonowl-signui-{}-seal", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        let pdf = dir.join("doc.pdf");
        std::fs::copy(moonowl::fixture::signed_pdf(), &pdf).expect("a signed copy");
        let mut reader = Reader::open_with(
            pdf.to_str().expect("a path"),
            Options {
                width: 1100,
                height: 800,
                config: dir.clone(),
                ..Default::default()
            },
        );

        open_the_window(&mut reader);
        assert_eq!(
            reader.text_all(".sign-seal .sign-name"),
            vec!["Signed".to_string()]
        );
        assert_eq!(
            reader.text_all(".sign-seal .sign-where"),
            vec!["signed 14 March 2024 · I agree to the terms".to_string()],
        );
    }

    /// **A date goes on the page the same way a name does.**
    ///
    /// One armed slot and one click, whichever of the two it is — see
    /// `app::Placing`. The button that fills the field does not place
    /// anything, so a reader who wants a different date can edit it.
    #[test]
    fn a_date_typed_in_the_window_lands_on_the_page() {
        let (mut reader, pdf) = reader("dated");
        open_the_window(&mut reader);

        reader.click(".sign-today");
        reader.click(".sign-place-text");
        // Mid-page, because 0.6 of a page is below the foot of an 800px
        // window at fit width and a pointer cannot reach what is not there.
        reader.click_on_page(1, (0.3, 0.5));

        let placed = render::open(pdf.to_str().expect("a path"))
            .expect("reopened")
            .signatures();
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].kind, moonowl::sign::Written::Line);
        assert_eq!(placed[0].by, moonowl::sign::today());
        assert_eq!(placed[0].page, 1);
    }

    /// An empty field is not a thing to place, and pressing the button says so
    /// rather than arming a click that would write nothing.
    #[test]
    fn an_empty_line_is_not_placed() {
        let (mut reader, pdf) = reader("empty-line");
        open_the_window(&mut reader);
        reader.click(".sign-place-text");
        assert_eq!(
            reader.text_all(".notice"),
            vec!["There is nothing typed to put on the page.".to_string()],
        );
        reader.click_on_page(1, (0.3, 0.6));
        assert!(render::open(pdf.to_str().expect("a path"))
            .expect("reopened")
            .signatures()
            .is_empty());
    }

    /// **The whole gesture, end to end**: draw a name, keep it, take it up,
    /// click on a page, and find ink in the file.
    #[test]
    fn a_name_drawn_once_is_ink_on_the_page() {
        let (mut reader, pdf) = reader("gesture");
        open_the_window(&mut reader);
        assert_eq!(
            reader.text_all(".sign-window .window-title"),
            vec!["Sign this document".to_string()],
        );

        reader.scrawl(&wave());
        reader.click(".sign-body .text-field");
        reader.type_text("A Reader");
        reader.click(".sign-window .pane-actions button.primary");
        assert_eq!(reader.text_all(".sign-name"), vec!["A Reader".to_string()]);

        reader.click(".sign-use");
        assert_eq!(
            reader.text_all(".notice"),
            vec!["Click on the page where the signature should go.".to_string()],
            "and the window closes on a question",
        );

        reader.click_on_page(1, (0.3, 0.5));
        assert_eq!(
            reader.text_all(".notice"),
            vec!["Signed on page 1.".to_string()],
        );

        let placed = render::open(pdf.to_str().expect("a path"))
            .expect("the document reopens")
            .signatures();
        assert_eq!(placed.len(), 1, "and it is in the file");
        assert_eq!(placed[0].by, "A Reader");
    }

    /// **The shape survives the trip.** Every scale between the pad and the
    /// page has to be the same in both axes, and there are three of them —
    /// `keep_signature`, `trimmed` and `place`. Two of the three divided x and
    /// y by different numbers, and the fault they produce is not visible in
    /// any assertion about *whether* a signature is there: a name written
    /// across the pad arrived on the page as a square.
    #[test]
    fn a_wide_name_lands_wide() {
        let (mut reader, pdf) = reader("shape");
        open_the_window(&mut reader);
        reader.scrawl(&wave());
        reader.click(".sign-body .text-field");
        reader.type_text("Wide");
        reader.click(".sign-window .pane-actions button.primary");
        reader.click(".sign-use");
        reader.click_on_page(1, (0.3, 0.5));

        let placed = render::open(pdf.to_str().expect("a path"))
            .expect("reopened")
            .signatures();
        let at = placed.first().expect("one signature").at;
        // The wave is drawn across 84% of a 440pt pad and down 60% of a 150pt
        // one, so it is about four times as wide as it is tall. Two, because
        // what is being caught is a signature that came back square.
        assert!(at.width / at.height > 2.0, "the shape was lost: {at:?}",);
    }

    /// Escape puts a signature down rather than signing something with it.
    /// A mode a reader cannot leave without using it is a mode that signs the
    /// wrong page.
    #[test]
    fn escape_puts_the_signature_down() {
        let (mut reader, pdf) = reader("escape");
        open_the_window(&mut reader);
        reader.scrawl(&wave());
        reader.click(".sign-window .pane-actions button.primary");
        reader.click(".sign-use");
        reader.press("Escape");
        assert_eq!(
            reader.text_all(".notice"),
            vec!["Signing cancelled.".to_string()],
        );
        reader.click_on_page(1, (0.3, 0.5));
        assert!(
            render::open(pdf.to_str().expect("a path"))
                .expect("reopened")
                .signatures()
                .is_empty(),
            "and the click that followed signed nothing",
        );
    }

    /// And Escape with the window up closes the window, which is the arm above
    /// it: two states, and the reader is in one of them.
    #[test]
    fn escape_closes_the_window() {
        let (mut reader, _) = reader("close");
        open_the_window(&mut reader);
        assert!(reader.box_of(".sign-window").is_some());
        reader.press("Escape");
        assert!(reader.box_of(".sign-window").is_none());
    }

    /// A pad nobody drew on keeps nothing, and says so rather than writing an
    /// empty file into the list.
    #[test]
    fn an_empty_pad_is_not_a_signature() {
        let (mut reader, _) = reader("empty");
        open_the_window(&mut reader);
        reader.click(".sign-window .pane-actions button.primary");
        assert_eq!(
            reader.text_all(".notice"),
            vec!["Draw a signature first, and this keeps it.".to_string()],
        );
        assert!(reader.text_all(".sign-name").is_empty());
    }

    /// One kept can be taken off the list again.
    #[test]
    fn a_signature_can_be_forgotten_from_the_window() {
        let (mut reader, _) = reader("forget");
        open_the_window(&mut reader);
        reader.scrawl(&wave());
        reader.click(".sign-body .text-field");
        reader.type_text("Gone");
        reader.click(".sign-window .pane-actions button.primary");
        assert_eq!(reader.text_all(".sign-name"), vec!["Gone".to_string()]);
        reader.click(".sign-forget");
        assert!(reader.text_all(".sign-name").is_empty());
    }

    /// **A signature comes off again, from the window that put it on.**
    ///
    /// The assessment that led to this feature named one caveat worth settling
    /// before shipping — *it cannot be removed afterwards* — and that is true
    /// of the app and not of this renderer. So it is settled the other way.
    #[test]
    fn a_signature_on_the_document_can_be_taken_off() {
        let (mut reader, pdf) = reader("unsign");
        open_the_window(&mut reader);
        reader.scrawl(&wave());
        reader.click(".sign-window .pane-actions button.primary");
        reader.click(".sign-use");
        reader.click_on_page(1, (0.3, 0.5));
        assert_eq!(
            render::open(pdf.to_str().expect("a path"))
                .expect("reopened")
                .signatures()
                .len(),
            1,
        );

        open_the_window(&mut reader);
        assert_eq!(
            reader.text_all(".sign-where"),
            vec!["page 1".to_string()],
            "the window lists what is already on the document",
        );
        reader.click(".sign-forget");
        assert_eq!(
            reader.text_all(".notice"),
            vec!["Signature taken off page 1.".to_string()],
        );
        assert!(
            render::open(pdf.to_str().expect("a path"))
                .expect("reopened")
                .signatures()
                .is_empty(),
            "and it is out of the file",
        );
    }

    /// **A plain key typed into a field is not a shortcut.** The Sign window's
    /// Name field is the second text field in this reader, and the first one —
    /// the theme editor's — had this fault the whole time: `space` is a screen
    /// down and `d` is half of one, so typing a name scrolled the document
    /// behind the window. See `prefs::typing_is_not_a_shortcut`.
    #[test]
    fn typing_a_name_does_not_move_the_document() {
        let (mut reader, _) = reader("keys");
        open_the_window(&mut reader);
        let before = reader.state().scroll;
        reader.click(".sign-body .text-field");
        reader.type_text("A Reader");
        assert_eq!(reader.state().scroll, before, "the document stayed put");
    }
}

/* -------------------------------------------- a date, and a line of text */

/// **A line of text goes onto the page and is drawn there.**
///
/// The assessment asks for this beside the drawing — *the form under a
/// signature usually wants both* — and the interesting half is that it is
/// drawn at all: a `/FreeText` annotation with no appearance stream is text
/// nobody renders, pdfium included, which is why this is a `/Stamp` with a
/// real text object in it. Counting dark pixels is the only way to tell those
/// two apart, because both of them read back out of the file.
#[test]
fn a_line_of_text_is_drawn_on_the_page() {
    let path = scratch("typed");
    let file = path.to_str().expect("a path");
    let at = Rect {
        left: 100.0,
        top: 300.0,
        width: 0.0,
        height: sign::LINE_HEIGHT,
    };

    let dark = |file: &str| {
        let document = render::open(file).expect("opens");
        let size = document.size_of(0);
        let (width, height) = (size.width.round() as u32, size.height.round() as u32);
        let mut counted = 0u32;
        document
            .render(
                0,
                width,
                height,
                moonowl::layout::View::WHOLE,
                &mut |bitmap| {
                    for y in 290..312u32 {
                        for x in 95..260u32 {
                            let at = ((y * bitmap.width + x) * 4) as usize;
                            if bitmap.bgra[at + 2] < 200 {
                                counted += 1;
                            }
                        }
                    }
                },
            )
            .expect("the page draws");
        counted
    };

    assert_eq!(dark(file), 0, "nothing is written there yet");
    sign::place_text(file, 1, at, "14 March 2024", sign::INK).expect("written");
    assert!(dark(file) > 50, "and now there is type on the page");
}

/// It reads back beside the ink, because a reader taking something off a page
/// does not think of the two as separate features — and it says what it says.
#[test]
fn a_line_of_text_is_listed_and_comes_off_again() {
    let path = scratch("typed-off");
    let file = path.to_str().expect("a path");
    sign::place_text(
        file,
        2,
        Rect {
            left: 72.0,
            top: 400.0,
            width: 0.0,
            height: sign::LINE_HEIGHT,
        },
        "Reading, 14 March 2024",
        sign::INK,
    )
    .expect("written");

    let placed = render::open(file).expect("reopened").signatures();
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].kind, sign::Written::Line);
    assert_eq!(placed[0].page, 2);
    assert_eq!(placed[0].by, "Reading, 14 March 2024");

    moonowl::markup::remove(file, placed[0].page, placed[0].index).expect("taken off");
    assert!(render::open(file)
        .expect("reopened again")
        .signatures()
        .is_empty());
}

/// A hand and a line on one page are two annotations and two rows, and each
/// says which it is.
#[test]
fn a_signature_and_a_date_sit_side_by_side() {
    let path = scratch("both");
    let file = path.to_str().expect("a path");
    sign::place(
        file,
        1,
        Rect {
            left: 80.0,
            top: 600.0,
            width: 0.0,
            height: 40.0,
        },
        &scrawl().trimmed(),
        sign::INK,
    )
    .expect("signed");
    sign::place_text(
        file,
        1,
        Rect {
            left: 300.0,
            top: 610.0,
            width: 0.0,
            height: sign::LINE_HEIGHT,
        },
        &sign::today(),
        sign::INK,
    )
    .expect("dated");

    let placed = render::open(file).expect("reopened").signatures();
    assert_eq!(placed.len(), 2);
    let kinds: Vec<sign::Written> = placed.iter().map(|one| one.kind).collect();
    assert!(kinds.contains(&sign::Written::Hand));
    assert!(kinds.contains(&sign::Written::Line));
}

/// Nothing typed is not a thing to put on a page, and it is refused with a
/// sentence rather than written as an empty annotation.
#[test]
fn nothing_typed_is_not_placed() {
    let path = scratch("blank-line");
    let file = path.to_str().expect("a path");
    let at = Rect {
        left: 100.0,
        top: 300.0,
        width: 0.0,
        height: sign::LINE_HEIGHT,
    };
    assert!(sign::place_text(file, 1, at, "   ", sign::INK).is_err());
    assert!(render::open(file).expect("opens").signatures().is_empty());
}

/// Today is a date somebody would write on a form, and it round-trips through
/// the reader that shows a document's own dates.
#[test]
fn today_is_a_date_a_person_would_write() {
    let today = sign::today();
    let parts: Vec<&str> = today.split(' ').collect();
    assert_eq!(parts.len(), 3, "day, month and year: {today}");
    assert!(parts[0]
        .parse::<u32>()
        .is_ok_and(|day| (1..=31).contains(&day)));
    assert!(parts[1].chars().all(|c| c.is_alphabetic()));
    assert!(parts[2].parse::<i64>().is_ok_and(|year| year >= 2024));
}

/// The day arithmetic, against dates whose answers are known: the epoch, a
/// leap day, the day before one, and a century that is not a leap year.
#[test]
fn the_day_count_becomes_the_right_date() {
    // `civil` is not public — it is reached through the one caller that is,
    // which is the whole of what it exists for.
    assert_eq!(sign::in_words("D:19700101000000"), "1 January 1970");
    assert_eq!(sign::in_words("D:20000229000000"), "29 February 2000");
    assert_eq!(sign::in_words("D:19000228000000"), "28 February 1900");
}

/* ------------------------------------ what the document is already signed with */

/// **A document that already carries a cryptographic signature is signed with
/// ink anyway, and the reader is told what that costs.**
///
/// The save is `FPDF_SaveAsCopy`, a full rewrite, and a cryptographic
/// signature covers a byte range of a specific file — so ink into a signed
/// document is the end of the signature that was there. Asked rather than
/// refused, which is the app's own decision about markup made again: it is
/// their document.
#[test]
fn a_signed_document_says_what_signing_it_costs() {
    let dir = std::env::temp_dir().join(format!("moonowl-seal-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory");
    let path = dir.join("carries-a-signature.pdf");
    std::fs::copy(moonowl::fixture::signed_pdf(), &path).expect("a copy to write to");

    let opened = moonowl::render::open(path.to_str().expect("a path")).expect("it opens");
    let standing = sign::standing(opened.path(), false, opened.sealed());
    assert!(standing.into_file, "it can still be signed with ink");
    assert!(
        standing.rewrites,
        "and doing so costs it the signature it carries"
    );
    assert!(
        sign::BREAKS_A_SIGNATURE.contains("stop verifying"),
        "and the sentence says so in words, not in mechanism",
    );
}

/// **A signature field is not a signature, and this is the test that says so.**
///
/// `FPDF_GetSignatureCount` counts every `/FT /Sig` field in the `/AcroForm`
/// whether or not anybody has signed it, and a blank one is the ordinary
/// furniture of a contract: it is the line at the foot of the page. Warning a
/// reader that ink will break a signature that has never been made is worse
/// than saying nothing, because a warning nobody can act on is one they learn
/// to ignore.
///
/// The app's own `tests/fixtures/signed.pdf` is exactly this shape, which is
/// how the fault was found: the reader was warned about every document that
/// merely had somewhere to sign.
#[test]
fn a_blank_signature_field_is_not_a_signature() {
    let blank = moonowl::fixture::unsigned_field_pdf();
    let seals = moonowl::render::open(&blank).expect("it opens").seals();
    assert_eq!(seals.len(), 1, "pdfium counts the field either way");
    assert!(!seals[0].filled, "and this one has nothing in it");
    assert_eq!(seals[0].says(), "waiting to be signed");
    assert!(
        !sign::standing(
            &blank,
            false,
            moonowl::render::open(&blank).expect("it opens").sealed()
        )
        .rewrites,
        "so there is no signature here for ink to break",
    );
}

/// **What a signature says about itself is read out of it and shown.**
///
/// Four facts, and every one of them certain: that something was actually
/// signed, when it says it was, why, and whether it forbids changes. See
/// [`sign::Seal`] for the three things the assessment expected here that are
/// not obtainable at all — the signer's name among them.
#[test]
fn a_signature_says_when_and_why() {
    let seals = moonowl::render::open(&moonowl::fixture::signed_pdf())
        .expect("it opens")
        .seals();
    assert_eq!(seals.len(), 1);
    assert!(seals[0].filled);
    assert_eq!(seals[0].when, "14 March 2024");
    assert_eq!(seals[0].reason, "I agree to the terms");
    assert_eq!(
        seals[0].says(),
        "signed 14 March 2024 · I agree to the terms"
    );
}

/// A PDF date is written for a machine and read by a person, and a date that
/// will not parse is handed back as it was written rather than dropped or
/// guessed at.
#[test]
fn a_date_is_shown_in_words_or_as_it_was_written() {
    assert_eq!(sign::in_words("D:20240314093000+01'00'"), "14 March 2024");
    assert_eq!(sign::in_words("D:19991231235959Z"), "31 December 1999");
    // No `D:`, which a good many writers leave off.
    assert_eq!(sign::in_words("20240101000000"), "1 January 2024");
    // Month 13 is not a month, and the string is the only honest answer left.
    assert_eq!(sign::in_words("D:20241301000000"), "D:20241301000000");
    assert_eq!(sign::in_words("last Tuesday"), "last Tuesday");
    assert_eq!(sign::in_words(""), "");
}

/// A document nobody has been near carries none of this, and asking is not an
/// error.
#[test]
fn an_ordinary_document_carries_no_signatures() {
    assert!(moonowl::render::open(&moonowl::fixture::prose_pdf())
        .expect("it opens")
        .seals()
        .is_empty());
}
