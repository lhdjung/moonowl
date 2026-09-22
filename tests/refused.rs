//! A renderer that will not draw, and a document let go of mid-write.
//!
//! Both are about what the page widget *remembers*. A page pdfium refuses is
//! remembered as refused, or it is asked again on every frame, under the one
//! lock every other page is waiting for. A page refused because the document
//! is let go of for a write is not refused at all — it is the same page, a
//! moment later — and remembering it as refused leaves it blank until it is
//! unmounted.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use moonowl::harness::{Options, Reader};
use moonowl::layout::{Size, View};
use moonowl::render::{Bitmap, PageSource};

/// A document of three grey pages that can be made to refuse, and that counts
/// what was asked of it.
struct Refusing {
    asked: AtomicUsize,
    closed: AtomicBool,
    /// Whether refusing means "let go of for a write" or "will not draw".
    released: AtomicBool,
}

impl Refusing {
    fn new() -> Arc<Self> {
        Arc::new(Refusing {
            asked: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
            released: AtomicBool::new(false),
        })
    }
}

impl PageSource for Refusing {
    fn pages(&self) -> usize {
        3
    }

    fn size_of(&self, _index: usize) -> Size {
        Size {
            width: 612.0,
            height: 792.0,
        }
    }

    fn render(
        &self,
        _index: usize,
        width: u32,
        height: u32,
        _view: View,
        take: &mut dyn FnMut(Bitmap),
    ) -> Result<(), String> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        if self.closed.load(Ordering::SeqCst) {
            return Err("That document has been closed.".into());
        }
        // Mid grey, so a page that is drawn cannot be mistaken for paper.
        let mut bgra = vec![255u8; (width as usize) * (height as usize) * 4];
        for pixel in bgra.as_chunks_mut::<4>().0 {
            *pixel = [64, 64, 64, 255];
        }
        take(Bitmap {
            width,
            height,
            bgra: &bgra,
            drew_in: 0.0,
        });
        Ok(())
    }

    fn released(&self) -> bool {
        self.released.load(Ordering::SeqCst)
    }

    fn opened_in(&self) -> f64 {
        0.0
    }
}

/// The patch a page covers, well inside the first one on screen.
fn page_rect(reader: &Reader) -> (u32, u32, u32, u32) {
    let rect = reader.harness.layout_rect(".page");
    (
        rect.x as u32 + 8,
        rect.y as u32 + 8,
        (rect.x + rect.width) as u32 - 8,
        // Clamped to the window: the page is taller than it.
        ((rect.y + rect.height) as u32 - 8).min(reader.window().1 as u32 - 8),
    )
}

/// **A page that will not draw is asked once.** The answer was remembered on
/// the GPU path and not on the CPU one, so every frame went back to pdfium —
/// under its one lock — for a page that was never going to arrive.
#[test]
fn a_page_that_will_not_draw_is_not_asked_again() {
    let document = Refusing::new();
    document.closed.store(true, Ordering::SeqCst);
    let mut reader = Reader::over(document.clone(), Options::default());
    reader.screenshot();
    let asked = document.asked.load(Ordering::SeqCst);
    assert!(asked > 0, "the pages were never asked for");

    for _ in 0..5 {
        reader.screenshot();
    }
    assert_eq!(
        document.asked.load(Ordering::SeqCst),
        asked,
        "the refusal was not remembered"
    );
}

/// **A document let go of for a write is not a page that will not draw.** The
/// pages keep what they have until the write lands, rather than going blank
/// for the length of it.
#[test]
fn a_page_keeps_its_pixels_while_the_document_is_let_go_of() {
    let document = Refusing::new();
    let mut reader = Reader::over(document.clone(), Options::default());
    let page = page_rect(&reader);
    // Drawn on a thread: the first frame asks, the next one has it.
    reader.screenshot();
    reader.settle();
    let drawn = reader.screenshot().mean(page);
    assert!(drawn[0] < 200.0, "the page was not drawn at all: {drawn:?}");

    // What `Viewer::write` does: the file is let go of, and a zoom in the
    // middle of it asks every page for a size it has not got.
    document.released.store(true, Ordering::SeqCst);
    document.closed.store(true, Ordering::SeqCst);
    reader.press_action(moonowl::keymap::Action::ZoomIn);
    let during = reader.screenshot().mean(page);
    assert!(
        (during[0] - drawn[0]).abs() < 24.0,
        "the page went blank while the document was let go of: {during:?}"
    );

    // And it draws again when the document comes back, rather than staying
    // as it was.
    document.released.store(false, Ordering::SeqCst);
    document.closed.store(false, Ordering::SeqCst);
    reader.press_action(moonowl::keymap::Action::ZoomIn);
    reader.settle();
    let after = reader.screenshot().mean(page_rect(&reader));
    assert!(after[0] < 200.0, "the page never came back: {after:?}");
}
