//! **A large comparison is bounded in time and in memory.** Its own file
//! because the memory measured is the process's, and the tests in
//! `compare.rs` run in parallel with it. The memory is the peak, so what
//! the comparison used and gave back counts too. Off macOS and Linux it is
//! not measured.

use moonowl::fixture;

/// **A large comparison is bounded in time and in memory**: four hundred
/// pages of prose against the same pages, and against a short document
/// with other words, where nearly every word of the book is a change. The
/// diff gives up being exact after `compare::PATIENCE`, which is what keeps
/// a comparison of two long, different documents within the time.
#[test]
fn a_large_comparison_is_quick_and_small() {
    let book = moonowl::render::open(&fixture::long_prose_pdf()).unwrap();
    let short = moonowl::render::open(&fixture::contents_pdf()).unwrap();
    let (held, before) = moonowl::stats::footprint_mb();
    let began = std::time::Instant::now();
    let same = moonowl::compare::compare(&*book, &*book).unwrap();
    let other = moonowl::compare::compare(&*book, &*short).unwrap();
    let took = began.elapsed();
    let (holds, peak) = moonowl::stats::footprint_mb();
    eprintln!("compare: {took:?}, the peak {before:.0}MB before and {peak:.0}MB after");
    assert!(same.is_empty());
    assert!(!other.is_empty());
    assert!(took.as_secs() < 20, "took {took:?}");
    assert!(peak - before < 50.0, "{before:.0}MB, then {peak:.0}MB");
    // Where writing the book raised the peak past what the comparison
    // reaches, the peak says nothing, and what the answers hold still does.
    assert!(holds - held < 50.0, "{held:.0}MB, then {holds:.0}MB");
    drop((same, other));
}

/// **Many small changes are compared in time too**: three lines of every
/// page of four hundred written again, each a passage only one file has,
/// which the comparison looks for elsewhere in the other.
#[test]
fn many_small_changes_are_compared_quickly() {
    let book = moonowl::render::open(&fixture::long_prose_pdf()).unwrap();
    let edited = moonowl::render::open(&fixture::long_prose_edited_pdf()).unwrap();
    let began = std::time::Instant::now();
    let changes = moonowl::compare::compare(&*edited, &*book).unwrap();
    let took = began.elapsed();
    eprintln!("compare: {took:?}, {} changes", changes.len());
    // The diff may put one change as a line taken out and a line put in.
    assert!((1200..1210).contains(&changes.len()), "{}", changes.len());
    assert!(changes.iter().all(|change| change.moved.is_none()));
    assert!(took.as_secs() < 20, "took {took:?}");
}
