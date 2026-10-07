//! A heading or a link into a page nothing has loaded yet lands where it
//! points, without loading the page on the window's thread.
//!
//! Placing a destination needs the page's space, which only a loaded page
//! can say. The reader jumps to the page's top at once and lands on the spot
//! as soon as the page has been read — which bringing it into view does
//! (`Viewer::land_on`). The document here says its pages' spaces only after
//! its links have been asked for, which is what the real one does.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use moonowl::harness::{Options, Reader};
use moonowl::layout::{Size, View};
use moonowl::render::{Bitmap, Heading, Link, PageSource, Pin};

/// Thirty blank pages, a heading halfway down the twentieth, and a twentieth
/// page whose space is known only once something has read it — too far from
/// the first for the pages mounted around it to have been.
#[derive(Default)]
struct Unread {
    read: AtomicBool,
    /// Whether the spot was asked for while the page was still unread — the
    /// ask that cannot be answered, and so has to be landed on later.
    asked_unread: AtomicBool,
}

const PIN: Pin = Pin {
    left: None,
    top: 396.0,
};

impl PageSource for Unread {
    fn pages(&self) -> usize {
        30
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
        let bgra = vec![255u8; (width as usize) * (height as usize) * 4];
        take(Bitmap {
            width,
            height,
            bgra: &bgra,
            drew_in: 0.0,
        });
        Ok(())
    }

    fn outline(&self) -> Vec<Heading> {
        vec![Heading {
            title: "Halfway down page twenty".into(),
            depth: 0,
            page: Some(20),
            offset: 0.0,
            spot: Some(PIN),
        }]
    }

    fn links_of(&self, index: usize) -> Vec<Link> {
        if index == 19 {
            self.read.store(true, Ordering::SeqCst);
        }
        Vec::new()
    }

    fn place(&self, page: usize, pin: Pin) -> Option<f64> {
        if page != 20 {
            return None;
        }
        if !self.read.load(Ordering::SeqCst) {
            self.asked_unread.store(true, Ordering::SeqCst);
            return None;
        }
        Some(pin.top / 792.0)
    }

    fn opened_in(&self) -> f64 {
        0.0
    }
}

/// How far down page twenty the top of the document area is, as a fraction
/// of the page.
fn down_the_page(reader: &mut Reader) -> Option<f64> {
    let at = reader.state().mounted.iter().position(|&page| page == 20)?;
    let page = reader
        .harness
        .layout_rect_of(reader.harness.query_all(".page")[at]);
    let viewer = reader
        .harness
        .layout_rect_of(reader.harness.query(".viewer").expect("the document area"));
    Some(((viewer.y - page.y) / page.height) as f64)
}

#[test]
fn a_heading_on_a_page_not_yet_read_lands_once_it_is() {
    let document = Arc::new(Unread::default());
    let mut reader = Reader::over(document.clone(), Options::default());
    reader.press_chord("mod+b");
    assert_eq!(reader.state().page, 1);
    let row = reader.harness.query(".outline-item").expect("the heading");
    let (x, y) = reader.harness.layout_rect_of(row).center();
    reader.click_at(x, y);
    assert_eq!(reader.state().page, 20, "on the page at once");
    // A page a jump lands on was not mounted before it, so its space was
    // not known when the heading was clicked: the jump went to its top, and
    // the spot was landed on once bringing the page into view had read it.
    assert!(
        document.asked_unread.load(Ordering::SeqCst),
        "the heading's page was unread when it was clicked"
    );
    assert!(
        reader.wait_until(5.0, |reader| {
            down_the_page(reader).is_some_and(|down| (down - 0.5).abs() < 0.02)
        }),
        "landed halfway down once the page was read: {:?}",
        down_the_page(&mut reader)
    );
}
