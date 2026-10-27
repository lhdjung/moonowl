//! The document in other forms: one page as a picture, and the text of
//! every page.

use crate::layout::{Crop, View};
use crate::render::PageSource;
use std::io::Write;

/// The resolution of a page saved as a picture, which is enough to print it.
pub const PICTURE_DPI: f64 = 300.0;

/// The most pixels a page saved as a picture has. A page larger than A0 is
/// drawn at a lower resolution, since a poster or a plan at 300 dpi can be
/// billions of pixels.
pub const PICTURE_PIXELS: f64 = 100_000_000.0;

/// The most bytes of pixels drawn at a time. A whole A4 page at 300 dpi is
/// 35MB, and macOS keeps a freed block that large for the life of the
/// process, so the page is drawn in strips of rows, and each strip is
/// written to the file before the next one is drawn.
const STRIP_BYTES: usize = 4 << 20;

/// Page `index` written to `to` as a PNG at [`PICTURE_DPI`], or at most
/// [`PICTURE_PIXELS`], as printed: in the file's own colours, not the
/// theme's.
pub fn save_picture(document: &dyn PageSource, index: usize, to: &str) -> Result<(), String> {
    let size = document.size_of(index);
    let scale = (PICTURE_DPI / 72.0).min((PICTURE_PIXELS / (size.width * size.height)).sqrt());
    let pixels = |points: f64| (points * scale).round().max(1.0) as u32;
    let (width, height) = (pixels(size.width), pixels(size.height));
    let strip = (STRIP_BYTES / (width as usize * 4)).clamp(1, height as usize) as u32;
    let mut rgb = Vec::with_capacity(width as usize * strip as usize * 3);
    let failed = |e: png::EncodingError| {
        std::io::Error::other(format!("The picture could not be made: {e}"))
    };
    crate::config::atomic_write_with(std::path::Path::new(to), &mut |file| {
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(failed)?;
        let mut stream = writer.stream_writer().map_err(failed)?;
        for top in (0..height).step_by(strip as usize) {
            let rows = strip.min(height - top);
            let view = View {
                rotation: 0,
                crop: Some(Crop {
                    x: 0.0,
                    y: f64::from(top) / f64::from(height),
                    width: 1.0,
                    height: f64::from(rows) / f64::from(height),
                }),
            };
            // The pixels are lent for the length of the call, so they are
            // turned into RGB there, and written once pdfium's lock is
            // given back.
            rgb.clear();
            document
                .render(index, width, rows, view, &mut |drawn| {
                    for [blue, green, red, _] in drawn.bgra.as_chunks::<4>().0 {
                        rgb.extend([*red, *green, *blue]);
                    }
                })
                .map_err(std::io::Error::other)?;
            stream.write_all(&rgb)?;
        }
        stream.finish().map_err(failed)?;
        writer.finish().map_err(failed)
    })
}

/// Said when the document was closed or written while its text was being
/// read.
pub const CLOSED_WHILE_READ: &str =
    "The document was closed while its text was being read. Try again.";

/// The text of every page, with a blank line between one page and the
/// next. A page with no text adds nothing, so a scan gives an empty string.
/// A document released while it is read is refused, since a released
/// document answers every page with no text, and the file would be cut
/// short where it was put down.
pub fn text(document: &dyn PageSource) -> Result<String, String> {
    let mut out = String::new();
    for index in 0..document.pages() {
        let text = document.text_of(index);
        let page = crate::select::quote(&text, 0, text.chars.len());
        let page = page.trim();
        if page.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(page);
    }
    if document.released() {
        return Err(CLOSED_WHILE_READ.to_string());
    }
    if !out.is_empty() {
        out.push('\n');
    }
    Ok(out)
}
