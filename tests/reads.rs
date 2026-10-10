//! **A read of the document is not a write.** Its own file, as `frames.rs`
//! is, because the counters it reads are the process's.

use moonowl::harness::{Options, Reader};
use moonowl::stats;

fn open_sign(reader: &mut Reader) {
    reader.click(".chip.title");
    let items = reader.text_all(".menu.document .menu-item");
    let at = items
        .iter()
        .position(|label| label.starts_with("Sign"))
        .expect("the Document menu offers signing");
    reader.click_nth(".menu.document .menu-item", at);
}

/// The Sign window reads every page on a thread. Counted as a write, quit
/// waited seconds for it on a book; and opened again on the same draft, it
/// read every page again.
#[test]
fn the_sign_window_reads_the_document_once_and_not_as_a_write() {
    let dir = std::env::temp_dir().join(format!("moonowl-reads-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let pdf = dir.join("doc.pdf");
    moonowl::fixture::draft(&pdf, 3);
    let mut reader = Reader::open_with(
        pdf.to_str().expect("a path"),
        Options {
            width: 1280,
            height: 800,
            config: dir.clone(),
            ..Default::default()
        },
    );
    reader.settle();
    let written = stats::get(&stats::WRITTEN);
    let read = stats::get(&stats::READ);

    open_sign(&mut reader);
    reader.settle();
    assert_eq!(
        stats::get(&stats::READ),
        read + 1,
        "the document was not read on a thread"
    );
    assert_eq!(
        stats::get(&stats::WRITTEN),
        written,
        "the read was counted as a write, which quit waits for"
    );

    reader.press("Escape");
    open_sign(&mut reader);
    reader.settle();
    assert_eq!(
        stats::get(&stats::READ),
        read + 1,
        "the same draft was read again"
    );
}
