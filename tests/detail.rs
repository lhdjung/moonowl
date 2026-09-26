//! A page too big to draw whole is sharp where the reader is looking.
//!
//! A page is drawn whole under `MAX_PIXELS` and stretched past it; over it, a
//! detail widget draws what is on screen at the box's own pixels. The document
//! here is a checkerboard of single pixels *of the page drawn whole at the
//! size asked*, so a part drawn at full size and put exactly where it belongs
//! shows every neighbour black against white, and the page drawn under the
//! ceiling and stretched shows greys.

use std::sync::Arc;

use moonowl::harness::{Options, Reader};
use moonowl::layout::{Size, View};
use moonowl::render::{Bitmap, PageSource};

struct Checker;

impl PageSource for Checker {
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
        view: View,
        take: &mut dyn FnMut(Bitmap),
    ) -> Result<(), String> {
        // Where this bitmap sits on the page drawn whole, as `pdfium.rs`
        // works it out.
        let (x, y) = match view.crop {
            Some(crop) => (
                (crop.x * (width as f64 / crop.width).round()).round() as u32,
                (crop.y * (height as f64 / crop.height).round()).round() as u32,
            ),
            None => (0, 0),
        };
        let mut bgra = vec![255u8; (width as usize) * (height as usize) * 4];
        for (at, pixel) in bgra.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (across, down) = (at as u32 % width + x, at as u32 / width + y);
            if (across + down) % 2 == 0 {
                *pixel = [0, 0, 0, 255];
            }
        }
        take(Bitmap {
            width,
            height,
            bgra: &bgra,
            drew_in: 0.0,
        });
        Ok(())
    }

    fn opened_in(&self) -> f64 {
        0.0
    }
}

/// How many neighbours across a patch in the middle of the window are black
/// against white, out of how many.
fn crisp(reader: &mut Reader) -> (usize, usize) {
    let shot = reader.screenshot();
    let (width, height) = reader.window();
    let (middle_x, middle_y) = (width as u32 / 2, height as u32 / 2);
    let mut sharp = 0;
    let mut pairs = 0;
    for y in middle_y - 50..middle_y + 50 {
        for x in middle_x - 50..middle_x + 50 {
            let (a, b) = (shot.at(x, y)[1], shot.at(x + 1, y)[1]);
            pairs += 1;
            if a.abs_diff(b) > 200 {
                sharp += 1;
            }
        }
    }
    (sharp, pairs)
}

#[test]
fn a_page_past_the_ceiling_is_sharp_where_it_is_read() {
    let mut reader = Reader::over(Arc::new(Checker), Options::default());
    let (sharp, pairs) = crisp(&mut reader);
    assert!(
        sharp * 100 >= pairs * 95,
        "fit width is under the ceiling and drawn 1:1: {sharp} of {pairs}"
    );

    // 600%: 4896 × 6336, which is 31 million pixels against twelve.
    for _ in 0..8 {
        reader.press_chord("mod+=");
    }
    reader.screenshot();
    let (sharp, pairs) = crisp(&mut reader);
    assert!(
        sharp * 100 >= pairs * 95,
        "at 600% what is on screen is drawn at full size, in place: {sharp} of {pairs}"
    );
}
