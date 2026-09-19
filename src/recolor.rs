//! The recolouring ramp on the CPU, which is the reference the shader is held
//! against.
//!
//! Recolouring maps *lightness* onto the theme — a pixel's luma says where on
//! the ramp between the theme's ink and its paper it belongs, and a pixel that
//! has a colour of its own is put there with that colour intact. The retired
//! frontend did it two ways that had to agree: a chain of canvas blend modes,
//! and a walk over the pixels. On the GPU there is one way and it is neither:
//! a shader over the page texture at composite time.
//!
//! So this is the walk over the pixels, ported from `recolorByPixel` line for
//! line, rounding included, and `tests/recolor.rs` holds the shader to within
//! one level out of 255 of it.
//!
//! Two details of the port are load-bearing, because they are where a faithful
//! translation and an obvious one differ.
//!
//! *The luma is rounded, not truncated.* `(r*77 + g*151 + b*28 + 128) >> 8` is
//! Rec. 601 in integers with the half added before the shift. The white point
//! multiplies any disagreement by 255/235, and half a level of truncation
//! came out the other end as two.
//!
//! *Every write in the original lands in a `Uint8ClampedArray`*, which rounds
//! half to even rather than half away from zero. It is one level, on exactly
//! the values that fall on a half, and it is free to be right about it.

/// Where paper begins. Above this level everything is the theme's background,
/// whatever colour it was printed in — which is what keeps a scan's warm cast
/// from surviving as a tint, and a hairline printed at 90% white from arriving
/// as a bright cage around a hyperref box.
pub const WHITE_POINT: u32 = 235;

/// How much chroma a pixel needs before it keeps any of its colour, and how
/// much before it keeps all of it.
pub const COLOUR_FLOOR: u32 = 12;
pub const COLOUR_FULL: u32 = 32;

/// An 8-bit colour, as the theme gives it.
pub type Rgb = [u8; 3];

/// The JavaScript conversion, which is what the original writes through:
/// clamp to 0..255 and round halves to even.
fn clamped(value: f32) -> u8 {
    if value.is_nan() {
        return 0;
    }
    if value <= 0.0 {
        return 0;
    }
    if value >= 255.0 {
        return 255;
    }
    let floor = value.floor();
    let fraction = value - floor;
    // Up when it is past the half, and on the half only when going up lands on
    // an even number. That last clause is the whole of "round half to even".
    let up = fraction > 0.5 || (fraction == 0.5 && !(floor as u32).is_multiple_of(2));
    (if up { floor + 1.0 } else { floor }) as u8
}

/// How much chroma an HSL lightness has room for: all of it in the middle,
/// none of it at either end, because black is black and white is white.
fn room_at(lightness: u32) -> u32 {
    255 - (2 * lightness as i32 - 255).unsigned_abs()
}

/// The four tables the pixel walk reads, built once per theme.
pub struct Tables {
    /// The theme's ink at black, its paper at white, 256 steps of three.
    pub ramp: [[u8; 3]; 256],
    /// The HSL lightness of `ramp[level]`.
    pub mapped: [u8; 256],
    /// The chroma available at that lightness.
    pub room: [u32; 256],
    /// The chroma available at the level a pixel arrived with, reciprocated.
    pub inverse_room: [f32; 256],
    /// How much of its own colour a pixel of a given chroma keeps.
    pub share: [f32; 256],
}

impl Tables {
    pub fn new(text: Rgb, bg: Rgb, keep_colour: bool) -> Self {
        let mut ramp = [[0u8; 3]; 256];
        for (level, entry) in ramp.iter_mut().enumerate() {
            // The white point, arrived at the way the canvas dodge arrives at
            // it: an 8-bit canvas rounds after every composite, so rounding
            // here too is what keeps the two paths on the same level.
            let t = (((level as f32 * 255.0 / WHITE_POINT as f32).round()).min(255.0)) / 255.0;
            for channel in 0..3 {
                entry[channel] =
                    clamped(text[channel] as f32 + (bg[channel] as f32 - text[channel] as f32) * t);
            }
        }

        let mut mapped = [0u8; 256];
        let mut room = [0u32; 256];
        let mut inverse_room = [0f32; 256];
        let mut share = [0f32; 256];
        if keep_colour {
            for level in 0..256usize {
                let entry = ramp[level];
                let high = entry[0].max(entry[1]).max(entry[2]) as u32;
                let low = entry[0].min(entry[1]).min(entry[2]) as u32;
                mapped[level] = ((high + low) >> 1) as u8;
                room[level] = room_at(mapped[level] as u32);
                let here = room_at(level as u32);
                inverse_room[level] = if here != 0 { 1.0 / here as f32 } else { 0.0 };
                share[level] = if level as u32 <= COLOUR_FLOOR {
                    0.0
                } else if level as u32 >= COLOUR_FULL {
                    1.0
                } else {
                    (level as u32 - COLOUR_FLOOR) as f32 / (COLOUR_FULL - COLOUR_FLOOR) as f32
                };
            }
        }

        Tables {
            ramp,
            mapped,
            room,
            inverse_room,
            share,
        }
    }
}

/// Recolour a buffer of RGBA in place — the reference, and the thing the
/// shader is measured against.
pub fn recolor_cpu(pixels: &mut [u8], text: Rgb, bg: Rgb, keep_colour: bool) {
    let tables = Tables::new(text, bg, keep_colour);
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let (r, g, b) = (pixel[0] as u32, pixel[1] as u32, pixel[2] as u32);
        let level = ((r * 77 + g * 151 + b * 28 + 128) >> 8) as usize;
        let ramp = tables.ramp[level];
        let high = r.max(g).max(b);
        let low = r.min(g).min(b);
        // Anything this light is paper, whatever colour it is; below the floor
        // there is no colour to keep, which is every pixel of a page of type.
        let keep = if level as u32 >= WHITE_POINT {
            0.0
        } else {
            tables.share[(high - low) as usize]
        };
        if keep == 0.0 {
            pixel[0] = ramp[0];
            pixel[1] = ramp[1];
            pixel[2] = ramp[2];
            continue;
        }
        // Hue and saturation as they were, at the lightness the ramp asked
        // for: the channels keep their distances from the lowest of them,
        // scaled by the room at the new lightness against the room at the old.
        let scale = tables.room[level] as f32 * tables.inverse_room[((high + low) >> 1) as usize];
        let foot = tables.mapped[level] as f32 - (high - low) as f32 * scale / 2.0;
        for (channel, value) in [r, g, b].into_iter().enumerate() {
            let base = ramp[channel] as f32;
            pixel[channel] = clamped(base + (foot + (value - low) as f32 * scale - base) * keep);
        }
    }
}

/// The same thing, as a compute shader.
pub const SHADER: &str = include_str!("recolor.wgsl");

/// And the second pass: the parts of a page painted through a ramp of their own
/// — a link, a mark, a selected word. See `regions.wgsl`.
pub const REGIONS: &str = include_str!("regions.wgsl");

/* ------------------------------------------------------------------ regions */

/// A rectangle of a page painted through a ramp of its own.
///
/// **This is `duotone` in `themes.ts`, which is the ramp before it learned to
/// keep a colour.** Three things in the app want it and all three are saying
/// *this part of the page is different* in a colour the theme chose, so what
/// the page was printed in is exactly what must not survive: a link
/// (`tintLinks`), a passage the reader has marked (`tintMarkup`) and a word
/// they have swept over (`paintSelection`). Hue and saturation are the page's
/// business everywhere else and are none of it here.
///
/// The area is in pixels of the drawn page rather than in fractions, because
/// this is the last step before the pixels and a fraction would be one more
/// multiplication at every one of them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    /// Left, top, right, bottom.
    pub area: [f32; 4],
    /// What the ink inside becomes, and what the paper under it becomes.
    pub ink: Rgb,
    pub paper: Rgb,
}

/// The regions of a buffer of RGBA, painted through their own ramps in place.
///
/// The reference for the shader, and the whole of the software path's answer.
///
/// **Every region reads the page as it arrived, not as the region before it
/// left it**, and overlapping regions are therefore decided rather than
/// compounded: the last one to name a pixel is the one that gets it. That is
/// what the app does and it is not obvious from the shape of the code there —
/// `tintLinks` and `tintMarkup` each *restore the pristine copy inside their
/// own clip* before ramping, so a link over a highlighted passage is the link
/// applied to the printed page and not to the highlight. Ramping the result of
/// a ramp would darken the overlap towards nothing in a few steps.
///
/// Which is why the untouched pixels are kept: `source` is the page as it
/// arrived, and it is a copy of the whole buffer only on this path, where the
/// page is already a buffer this side owns. The shader keeps the same rule a
/// different way — two invocations that write one texel have no order between
/// them, so `gpu::disjoint` cuts every run back to the part no later run covers
/// before the dispatch, and the question never arises on the GPU at all.
pub fn duotone_cpu(pixels: &mut [u8], width: u32, height: u32, regions: &[Region]) {
    if regions.is_empty() {
        return;
    }
    let source = pixels.to_vec();
    duotone_from(pixels, &source, width, height, regions);
}

/// The same, reading from `source` rather than from what is in `pixels` —
/// for the links, which are ramped from the page as pdfium drew it, as the
/// GPU's pass ramps them. Ramped from the recoloured page instead, a dark
/// theme's paper read as ink and every link came out a solid block of the
/// link colour with the words cut out of it in the paper's.
pub fn duotone_from(pixels: &mut [u8], source: &[u8], width: u32, height: u32, regions: &[Region]) {
    for region in regions {
        let ramp = Tables::new(region.ink, region.paper, false).ramp;
        let left = region.area[0].max(0.0).floor() as u32;
        let top = region.area[1].max(0.0).floor() as u32;
        let right = region.area[2].ceil().clamp(0.0, width as f32) as u32;
        let bottom = region.area[3].ceil().clamp(0.0, height as f32) as u32;
        for y in top..bottom.min(height) {
            let row = (y as usize) * (width as usize) * 4;
            for x in left..right.min(width) {
                let at = row + (x as usize) * 4;
                let (r, g, b) = (
                    source[at] as u32,
                    source[at + 1] as u32,
                    source[at + 2] as u32,
                );
                let level = ((r * 77 + g * 151 + b * 28 + 128) >> 8) as usize;
                let entry = ramp[level];
                pixels[at] = entry[0];
                pixels[at + 1] = entry[1];
                pixels[at + 2] = entry[2];
            }
        }
    }
}
