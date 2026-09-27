//! What a reading session costs, asserted rather than observed.
//!
//! Taking the floor apart ended with this on the list — see `PROGRESS.md`:
//! the harness needs a memory assertion, and `footprint_mb()` is what it
//! should assert on, because "three
//! copies of every page" is exactly the shape a test catches and a reading
//! session does not. It cost 96MB and went unnoticed through the whole of
//! Phase 1.
//!
//! Two things about how it is asserted. It is a *growth* bound rather than a
//! ceiling: what the process costs to start depends on the machine, the
//! allocator and how many system fonts are installed, and none of that is what
//! a leak looks like. And it is asserted on the same counter the app reports,
//! so a number here can be compared with a number from `--measure`.
//!
//! This is the CPU path, which is not the one the app ships — a page here is
//! an `ImageData` on the heap rather than a texture on the GPU. That makes it
//! the *weaker* test of the two for absolute cost and the stronger one for
//! leaks, because everything it holds is charged to the process and nothing is
//! hidden behind a driver.

use moonowl::harness::{Options, Reader};
use moonowl::stats;

/// A window small enough that the rasteriser is not the slow part.
fn options() -> Options {
    Options {
        width: 700,
        height: 560,
        ..Default::default()
    }
}

#[test]
fn reading_a_book_does_not_grow_without_bound() {
    let (settled, _) = stats::footprint_mb();
    if settled == 0.0 {
        // macOS answers out of `vmmap` and Linux out of `/proc/self/status`;
        // Windows has neither and would want `GetProcessMemoryInfo` and a
        // dependency for it. Where nothing answers, the counters below still
        // stand — which is most of what a leak of the shape this test exists
        // for actually moves.
        eprintln!("cost: no footprint on this platform; the counters still stand");
    }

    let mut reader = Reader::open_with(&Reader::book(), options());
    // Ten screenfuls to reach a steady state — the first few pages drawn are
    // where the one-off costs land — and then forty more to see whether it
    // keeps climbing. Each one draws a page and rasterises a window, both on
    // the CPU, which is a fifth of a second with the dependencies optimised
    // and four seconds without: see `[profile.dev.package."*"]`.
    for _ in 0..10 {
        reader.wheel_screen();
        reader.screenshot();
    }
    let (warm, _) = stats::footprint_mb();
    let drawn_warm = stats::get(&stats::DRAWN);

    for _ in 0..40 {
        reader.wheel_screen();
        reader.screenshot();
    }
    let (after, peak) = stats::footprint_mb();
    let drawn = stats::get(&stats::DRAWN);
    eprintln!(
        "cost: {drawn} pages drawn ({} in the last forty screenfuls), \
         {warm:.0}MB warm, {after:.0}MB after, {peak:.0}MB peak, \
         {} mounted holding {:.0}MB",
        drawn - drawn_warm,
        stats::get(&stats::MOUNTED),
        stats::get(&stats::RESIDENT) as f64 / 1e6,
    );

    assert!(
        drawn > drawn_warm,
        "the forty screenfuls drew something: {drawn} against {drawn_warm}"
    );

    // What is held is the mounting window and nothing else. A page at this
    // size is 700 × 900-ish × 4 bytes; three of them is the most the layout
    // ever mounts at once.
    let mounted = stats::get(&stats::MOUNTED);
    assert!(
        mounted <= 4,
        "the mounting window is a handful of pages: {mounted}"
    );
    let resident = stats::get(&stats::RESIDENT) as f64 / 1e6;
    assert!(
        resident < 40.0,
        "and they are all it holds: {resident:.0}MB"
    );

    if settled > 0.0 {
        let grew = after - warm;
        assert!(
            grew < 60.0,
            "forty screenfuls after the first ten cost {grew:.0}MB \
             ({warm:.0} → {after:.0}); a page that is not given back looks \
             exactly like this"
        );
    }

    // ---------------------------------------------------------- the column
    //
    // The same question of the thumbnail column, and it is the same test
    // rather than a second one because these counters are the process's: two
    // test functions running at once would each be reading the other's
    // pages.
    //
    // `AGENTS.md` says the app's memory table was measured with the sidebar
    // shut, and warns in as many words that "if you are measuring, open the
    // Pages tab and scroll it, because that is where a fourth leak would hide
    // next" — the column there drew a thumbnail for every page it passed and
    // gave none of them back. Here a thumbnail belongs to its row and a row
    // is unmounted the moment it leaves the band, so scrolling four hundred
    // pages of column should cost what a screenful of it costs.
    reader.press_chord("mod+b");
    // book.pdf has no table of contents, so the panel opens on the pages.
    assert_eq!(reader.state().sidebar.as_deref(), Some("pages"));
    for _ in 0..10 {
        reader.wheel_over(".panel.thumb-column", 2_000.0);
        reader.screenshot();
    }
    let (column_warm, _) = stats::footprint_mb();
    for _ in 0..40 {
        reader.wheel_over(".panel.thumb-column", 2_000.0);
        reader.screenshot();
    }
    let (column_after, _) = stats::footprint_mb();
    let thumbs = reader.state().thumbs.len();
    let held = stats::get(&stats::RESIDENT) as f64 / 1e6;
    eprintln!(
        "cost: {thumbs} thumbnails mounted, {} pages and thumbnails in all, \
         holding {held:.0}MB, {column_warm:.0} → {column_after:.0}MB",
        stats::get(&stats::MOUNTED),
    );

    assert!(
        (1..=24).contains(&thumbs),
        "the column is a window, not a list: {thumbs} thumbnails"
    );
    assert!(
        held < 40.0,
        "and what it holds does not grow with the book: {held:.0}MB"
    );
    if settled > 0.0 {
        let grew = column_after - column_warm;
        assert!(
            grew < 60.0,
            "forty screenfuls of column after the first ten cost {grew:.0}MB \
             ({column_warm:.0} → {column_after:.0})"
        );
    }

    // ---------------------------------------------------------- the index
    //
    // The search keeps every page it read for as long as the bar is up, and
    // nothing once it is down.
    reader.press_chord("mod+f");
    reader.type_text("the");
    reader.scan_out();
    let index = stats::get(&stats::INDEX_BYTES);
    eprintln!("cost: the search index holds {:.1}MB", index as f64 / 1e6);
    assert!(index > 0, "a scanned book is an index");
    reader.press("Escape");
    assert_eq!(
        stats::get(&stats::INDEX_BYTES),
        0,
        "and closing the bar gives it back"
    );
}
