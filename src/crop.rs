//! Where the ink is: the margins measured off a sample, and taken away.
//!
//! `measureCrop` and `inkBox` from `viewer.ts`, and the arguments they carry
//! come with them. A scanned book and a LaTeX paper both spend a quarter of
//! the window on white paper, and fit width fits the paper rather than the
//! words. Every reader built for reading rather than for printing has this —
//! Sumatra calls it fit content, Zathura and Sioyek call it cropping — and it
//! is worth more than any zoom preset on exactly the documents this app is
//! for.
//!
//! **The measurement is synchronous here, and the whole of the app's
//! machinery for it is gone.** In the app this is an `async` method with a
//! `cropping` generation counter, a check after every await that the document
//! has not been closed and the run has not been superseded, and a `void`
//! call at three call sites — because eight page renders in a browser are
//! eight trips through pdf.js's worker and cannot be waited for. Here eight
//! pages at a hundred and sixty pixels wide is under five milliseconds in the
//! same call, so a toggle measures and lays out before it returns. There is
//! no run to supersede and no state to be stale.

use std::sync::Arc;

use crate::layout::{Crop, View};
use crate::render::PageSource;

/// How many pages are looked at to decide where the ink on this document
/// begins.
///
/// A sample rather than the whole document: measuring a page means drawing
/// it, and drawing nine hundred of them to decide on a margin is not a trade
/// anybody would make. First page, last page and evenly spaced between them,
/// because the shapes that vary are the front matter, the plates and the
/// index, and those are where they live.
pub const SAMPLE: usize = 8;
/// Blank left around the ink, as a fraction of the page.
pub const PAD: f64 = 0.012;
/// The most that may be taken off any one side. A page whose margins are
/// wider than this is more likely to be a page this has misread, and the cost
/// of being wrong is a reader who cannot see the top line.
pub const MAX: f64 = 0.3;
/// Below this there is nothing worth trimming, and the answer is to leave the
/// page as it is rather than to move it by a hair.
pub const MIN: f64 = 0.03;
/// How wide a page is drawn when it is being measured rather than read. A
/// millisecond or two of work, and enough to find a margin to within a
/// character.
pub const PROBE_WIDTH: u32 = 160;
/// Where paper stops and ink begins, on the 0-255 luma scale `WHITE_POINT`
/// uses for the same question — so a hairline printed at 90% white, or a
/// scan's warm paper, counts as paper here exactly as it does when a page is
/// recoloured.
pub const INK: u8 = 235;

/// The pages to look at, in order: the first, the last, and evenly spaced
/// between them.
pub fn sample(pages: usize) -> Vec<usize> {
    if pages == 0 {
        return Vec::new();
    }
    // Placed by proportion rather than by a whole step, which rounded down:
    // fourteen pages were sampled as the first eight and the last.
    let mut chosen: Vec<usize> = (0..SAMPLE)
        .map(|n| n * (pages - 1) / (SAMPLE - 1))
        .collect();
    chosen.dedup();
    chosen
}

/// Where the ink is on one drawn page, as fractions of it.
///
/// BGRA, top row first, exactly as [`crate::render::Bitmap`] carries it, and
/// judged by the luma the recolouring judges paper by.
pub fn ink_box(bgra: &[u8], width: u32, height: u32) -> Option<Crop> {
    let (width, height) = (width as usize, height as usize);
    if width == 0 || height == 0 || bgra.len() < width * height * 4 {
        return None;
    }
    let mut left = width;
    let mut top = height;
    let mut right = 0usize;
    let mut bottom = 0usize;
    let mut found = false;
    for y in 0..height {
        let row = &bgra[y * width * 4..(y + 1) * width * 4];
        for (x, pixel) in row.as_chunks::<4>().0.iter().enumerate() {
            // Luma, as recolouring reads it, and not every channel: a scan's
            // warm paper — (245, 238, 215) — is paper to the recolouring and
            // was ink here, so a scanned book never had margins to trim.
            let (b, g, r) = (pixel[0] as u32, pixel[1] as u32, pixel[2] as u32);
            if (r * 77 + g * 151 + b * 28 + 128) >> 8 >= INK as u32 {
                continue;
            }
            found = true;
            if x < left {
                left = x;
            }
            if x > right {
                right = x;
            }
            if y < top {
                top = y;
            }
            if y > bottom {
                bottom = y;
            }
        }
    }
    // A blank page says nothing, which is different from a page whose ink
    // reaches every edge.
    if !found {
        return None;
    }
    Some(Crop {
        x: left as f64 / width as f64,
        y: top as f64 / height as f64,
        width: (right + 1 - left) as f64 / width as f64,
        height: (bottom + 1 - top) as f64 / height as f64,
    })
}

/// The crop for a whole document, or `None` when there is nothing worth
/// taking off.
///
/// The union of what the sample finds, padded, and refused outright if what
/// is left is nearly the whole page (there was nothing to trim) or a sliver
/// of it (something was misread — a blank page, a page that failed to draw).
///
/// Measured against the document as it is printed, never against the page as
/// the reader has turned it: this is a question about the document, and the
/// answer must not move because somebody pressed ⌘R. The caller turns the
/// answer instead — see [`Crop::turned`].
pub fn measure(document: &Arc<dyn PageSource>) -> Option<Crop> {
    let mut left = 1.0f64;
    let mut top = 1.0f64;
    let mut right = 0.0f64;
    let mut bottom = 0.0f64;
    let mut found = false;

    for index in sample(document.pages()) {
        let size = document.size_of(index);
        if size.width <= 0.0 || size.height <= 0.0 {
            continue;
        }
        // A hundred and sixty across, and no more than eight times that
        // down: a banner of a page 3pt wide and 14,400 tall asked for half a
        // gigabyte, which the render scratch then kept. The box is a fraction
        // of the page, so a narrower probe measures the same thing.
        let tall = size.height / size.width * PROBE_WIDTH as f64;
        let most = (PROBE_WIDTH * 8) as f64;
        let (width, height) = if tall > most {
            (
                ((PROBE_WIDTH as f64) * most / tall).round().max(1.0) as u32,
                most as u32,
            )
        } else {
            (PROBE_WIDTH, tall.round().max(1.0) as u32)
        };
        let mut ink = None;
        // The page is drawn whole and unturned. `View::WHOLE` is the point of
        // the constant: a probe that inherited the reader's own view would be
        // measuring a page that has already been trimmed, and the second
        // press of the switch would trim the trim.
        let outcome = document.render(index, width, height, View::WHOLE, &mut |bitmap| {
            ink = ink_box(bitmap.bgra, bitmap.width, bitmap.height);
        });
        if outcome.is_err() {
            continue;
        }
        let Some(ink) = ink else { continue };
        found = true;
        left = left.min(ink.x);
        top = top.min(ink.y);
        right = right.max(ink.x + ink.width);
        bottom = bottom.max(ink.y + ink.height);
    }
    if !found {
        return None;
    }
    refine(left, top, right, bottom)
}

/// The union of what the sample found, padded, clamped, and then refused if
/// it is not worth doing.
///
/// Its own function because it is the half of the measurement with no pixels
/// in it: everything that decides whether a reader's page moves is four
/// numbers and three constants, and a test that has to draw a document to
/// reach it is a test of pdfium.
pub fn refine(left: f64, top: f64, right: f64, bottom: f64) -> Option<Crop> {
    let mut crop = Crop {
        x: (left - PAD).max(0.0),
        y: (top - PAD).max(0.0),
        width: 0.0,
        height: 0.0,
    };
    crop.width = (right + PAD).min(1.0) - crop.x;
    crop.height = (bottom + PAD).min(1.0) - crop.y;

    // Never take more than a share of any side: a page whose margins measure
    // wider than that is more likely to be a page this has misread, and the
    // cost of being wrong is a reader who cannot see the top line.
    crop.x = crop.x.min(MAX);
    crop.y = crop.y.min(MAX);
    crop.width = crop.width.clamp(1.0 - MAX - crop.x, 1.0 - crop.x);
    crop.height = crop.height.clamp(1.0 - MAX - crop.y, 1.0 - crop.y);

    // Nothing worth doing, either because the page has no margins or because
    // what came back is too small to be a page of anything.
    let trimmed = 1.0 - crop.width * crop.height;
    if trimmed < MIN || crop.width < 0.3 || crop.height < 0.3 {
        return None;
    }
    Some(crop)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page of `width`×`height` white pixels with a black rectangle on it.
    fn page(width: u32, height: u32, ink: (u32, u32, u32, u32)) -> Vec<u8> {
        let mut bytes = vec![255u8; (width * height * 4) as usize];
        for y in ink.1..ink.3 {
            for x in ink.0..ink.2 {
                let at = ((y * width + x) * 4) as usize;
                bytes[at..at + 3].copy_from_slice(&[0, 0, 0]);
            }
        }
        bytes
    }

    #[test]
    fn the_ink_box_is_the_ink_and_not_the_paper() {
        let bytes = page(100, 200, (10, 20, 90, 120));
        let ink = ink_box(&bytes, 100, 200).expect("there is ink on it");
        assert!((ink.x - 0.10).abs() < 0.001, "{ink:?}");
        assert!((ink.y - 0.10).abs() < 0.001, "{ink:?}");
        assert!((ink.width - 0.80).abs() < 0.001, "{ink:?}");
        assert!((ink.height - 0.50).abs() < 0.001, "{ink:?}");
    }

    #[test]
    fn a_blank_page_says_nothing() {
        assert_eq!(ink_box(&vec![255u8; 40 * 40 * 4], 40, 40), None);
        // And "nothing" is not "all of it": a page covered in ink answers
        // with the whole page, which is a page with no margins to trim.
        let all = ink_box(&vec![0u8; 40 * 40 * 4], 40, 40).expect("ink everywhere");
        assert_eq!(all.width, 1.0);
        assert_eq!(all.height, 1.0);
    }

    #[test]
    fn a_hairline_at_ninety_percent_white_is_paper() {
        // `INK` is the same threshold `WHITE_POINT` uses when a page is
        // recoloured, and the two have to agree: a line this app calls paper
        // when it themes a page is a line it must not crop to.
        let mut bytes = vec![255u8; 40 * 40 * 4];
        for x in 0..40u32 {
            let at = ((5 * 40 + x) * 4) as usize;
            bytes[at..at + 3].copy_from_slice(&[240, 240, 240]);
        }
        assert_eq!(ink_box(&bytes, 40, 40), None);
    }

    #[test]
    fn a_scans_warm_paper_is_paper() {
        // (245, 238, 215) — luma 238, so the recolouring calls it paper; its
        // blue channel alone made it ink here.
        let mut bytes = vec![0u8; 40 * 40 * 4];
        for pixel in bytes.chunks_mut(4) {
            pixel.copy_from_slice(&[215, 238, 245, 255]);
        }
        assert_eq!(ink_box(&bytes, 40, 40), None);
    }

    #[test]
    fn a_page_with_no_margins_is_left_alone() {
        // Ink to all four edges: there is nothing to take off, and the answer
        // is to leave the page as it is rather than to move it by a hair.
        assert_eq!(refine(0.0, 0.0, 1.0, 1.0), None);
        // And a margin too thin to be worth the move — under `MIN` of the
        // page's area between them — is the same answer.
        assert_eq!(refine(0.005, 0.005, 0.995, 0.995), None);
    }

    #[test]
    fn no_more_than_a_share_of_any_side_comes_off() {
        // A page whose ink is one word in the middle: what this measures is a
        // page it has probably misread — a plate, a blank, a page that failed
        // to draw — and the cost of believing it is a reader who cannot see
        // the top line.
        let crop = refine(0.45, 0.45, 0.55, 0.55).expect("something comes off");
        assert!(crop.x <= MAX + 0.001, "{crop:?}");
        assert!(crop.y <= MAX + 0.001, "{crop:?}");
        assert!(crop.x + crop.width >= 1.0 - MAX - 0.001, "{crop:?}");
        assert!(crop.y + crop.height >= 1.0 - MAX - 0.001, "{crop:?}");
    }

    #[test]
    fn a_margin_is_padded_rather_than_cut_to_the_ink() {
        let crop = refine(0.2, 0.1, 0.8, 0.9).expect("there are margins");
        assert!((crop.x - (0.2 - PAD)).abs() < 0.001, "{crop:?}");
        assert!((crop.y - (0.1 - PAD)).abs() < 0.001, "{crop:?}");
        assert!((crop.width - (0.6 + PAD * 2.0)).abs() < 0.001, "{crop:?}");
        assert!((crop.height - (0.8 + PAD * 2.0)).abs() < 0.001, "{crop:?}");
    }

    #[test]
    fn a_crop_turned_four_times_is_the_crop_it_was() {
        let crop = Crop {
            x: 0.11,
            y: 0.07,
            width: 0.62,
            height: 0.83,
        };
        let mut turned = crop;
        for _ in 0..4 {
            turned = turned.turned();
        }
        assert!((turned.x - crop.x).abs() < 1e-9, "{turned:?}");
        assert!((turned.y - crop.y).abs() < 1e-9, "{turned:?}");
        assert!((turned.width - crop.width).abs() < 1e-9, "{turned:?}");
        assert!((turned.height - crop.height).abs() < 1e-9, "{turned:?}");
        // And one turn puts what was at the bottom on the left, which is what
        // turning a page clockwise does to a rectangle on it.
        let once = crop.turned();
        assert!(
            (once.x - (1.0 - crop.y - crop.height)).abs() < 1e-9,
            "{once:?}"
        );
        assert_eq!(once.width, crop.height);
    }

    #[test]
    fn the_sample_takes_both_ends_and_no_more_than_it_said() {
        for pages in [1usize, 2, 7, 8, 9, 400, 901] {
            let chosen = sample(pages);
            assert_eq!(chosen[0], 0, "{pages} pages");
            assert_eq!(*chosen.last().unwrap(), pages - 1, "{pages} pages");
            assert!(chosen.len() <= SAMPLE + 1, "{pages} pages: {chosen:?}");
            // In order, and each page asked about once.
            let mut sorted = chosen.clone();
            sorted.dedup();
            assert_eq!(sorted, chosen, "{pages} pages");
            assert!(
                chosen.windows(2).all(|pair| pair[0] < pair[1]),
                "{chosen:?}"
            );
        }
    }
}
