//! **Scrolling by itself stops at the end of the document, and so does the
//! clock behind it.** Its own file because `stats::RENDERS` is the
//! process's, and the tests in `scroll_by_itself.rs` scroll in parallel.

use moonowl::harness::{Options, Reader};
use moonowl::stats;

#[test]
fn scrolling_by_itself_ends_with_the_document_and_rendering_stops() {
    let (_, short) = moonowl::fixture::compared_pdfs();
    let mut reader = Reader::open_with(
        &short,
        Options {
            settings: vec![("scroll_by_itself_speed".into(), 400.0.into())],
            ..Default::default()
        },
    );
    reader.click(".chip.fit");
    reader.click(".scroll-by-itself");
    reader.settle();
    // Ended once a twentieth of a second goes by without it moving.
    let last = std::cell::Cell::new(f64::NAN);
    let ended = reader.later_until(30.0, |reader| {
        let now = reader.state().scroll;
        now == last.replace(now)
    });
    assert!(ended, "it reached the end: {}", reader.state().scroll);
    let end = reader.state().scroll;
    assert!(end > 0.0);
    reader.settle();
    let before = stats::get(&stats::RENDERS);
    reader.later(1.0);
    for _ in 0..20 {
        reader.settle();
    }
    assert_eq!(reader.state().scroll, end);
    assert_eq!(
        stats::get(&stats::RENDERS),
        before,
        "nothing renders once it has stopped"
    );
}
