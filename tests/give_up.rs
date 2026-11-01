//! Quitting while text is recognised: nothing more is read, and no copy is
//! written. Its own file because the flag it raises is the process's.

use moonowl::export::GIVE_UP;
use moonowl::{fixture, ocr, render};
use std::sync::atomic::Ordering;

#[test]
fn quitting_recognises_nothing_more_and_writes_no_copy() {
    let dir = std::env::temp_dir().join(format!("moonowl-give-up-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let from = dir.join("scan.pdf").to_string_lossy().into_owned();
    std::fs::copy(fixture::scanned_pdf(), &from).unwrap();
    let to = dir
        .join("scan (searchable).pdf")
        .to_string_lossy()
        .into_owned();
    let document = render::open(&from).unwrap();
    let read = render::stamp_of(&from);
    let found = ocr::recognise_with(&*document, &Default::default(), |_, _, _, _, _| {
        Ok(vec![ocr::Word {
            text: "found".into(),
            at: render::Rect {
                left: 0.1,
                top: 0.1,
                width: 0.2,
                height: 0.05,
            },
        }])
    })
    .unwrap();
    GIVE_UP.store(true, Ordering::Relaxed);
    let again = ocr::recognise_with(&*document, &Default::default(), |_, _, _, _, _| {
        panic!("no page is read once the app is ending")
    });
    let written = ocr::write(&from, read, &found.pages, &to);
    GIVE_UP.store(false, Ordering::Relaxed);
    let given_up = Err("The app was ending, so no text was recognised.".to_string());
    assert_eq!(again.map(|found| found.pages.len()), given_up);
    assert_eq!(written, given_up.map(|_| ()));
    assert!(!std::path::Path::new(&to).exists());
}
