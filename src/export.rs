//! The document in other forms: one page as a picture, the text of every
//! page, and a copy with its annotations drawn into the pages.

use crate::layout::{Crop, View};
use crate::render::PageSource;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

/// Raised when the app ends. A picture or a text being made is then given
/// up between one strip or page and the next, and nothing is written, so
/// that quitting does not wait for a whole book to be read.
pub static GIVE_UP: AtomicBool = AtomicBool::new(false);

/// Said when a file was given up because the app was ending.
const GIVEN_UP: &str = "The app was ending, so the file was not made.";

/// The resolution of a page saved as a picture, which is enough to print it.
pub const PICTURE_DPI: f64 = 300.0;

/// The most pixels a page saved as a picture has. A page larger than A0 is
/// drawn at a lower resolution, since a poster or a plan at 300 dpi can be
/// billions of pixels.
pub const PICTURE_PIXELS: f64 = 100_000_000.0;

/// The most bytes of pixels drawn at a time. A whole A4 page at 300 dpi is
/// 35MB, and macOS keeps a freed block that large for the life of the
/// process, so the page is drawn in strips of rows, and each strip is
/// written to the file before the next one is drawn. pdfium goes through
/// the whole page's content for every strip, so a smaller strip is a
/// slower picture: an A4 page is about nine strips at this size.
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
            if GIVE_UP.load(Ordering::Relaxed) {
                return Err(std::io::Error::other(GIVEN_UP));
            }
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
            // Drawn into a strip of its own, which goes before the next is
            // drawn, and only while no page for the window is waiting.
            let drawn =
                (document.render_aside(index, width, rows, view)).map_err(std::io::Error::other)?;
            rgb.clear();
            for [blue, green, red, _] in drawn.bgra.as_chunks::<4>().0 {
                rgb.extend([*red, *green, *blue]);
            }
            drop(drawn);
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
/// A document released while it is read is refused, even if it is taken up
/// again before the end, since the file would be cut short where it was
/// put down.
pub fn text(document: &dyn PageSource) -> Result<String, String> {
    let mut out = String::new();
    for index in 0..document.pages() {
        if GIVE_UP.load(Ordering::Relaxed) {
            return Err(GIVEN_UP.to_string());
        }
        let Some(text) = document.text_aside(index) else {
            return Err(CLOSED_WHILE_READ.to_string());
        };
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
    if !out.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

/// Said instead of making a flattened copy of an encrypted document, which
/// pdfium cannot write again with its password.
pub const ENCRYPTED_FLAT: &str =
    "This document is encrypted, so a flattened copy of it cannot be made.";

/// A copy of the document with its highlights, signatures, notes and form
/// fields drawn into the pages, so that no program can move or remove them.
/// Every annotation the reader can see is drawn in, whether or not it is
/// marked for printing, since many programs' highlights are not.
///
/// The copy is read from the file, not from the open document, and pdfium's
/// lock is taken for one page at a time, so the pages being drawn for the
/// window go between them. The copy keeps its links, its headings and its
/// page labels. Its form is taken out, since its fields are now drawn into
/// the pages. The result is compacted like any new file.
pub fn flattened(document: &dyn PageSource) -> Result<Flat, String> {
    if document.encrypted() {
        return Err(ENCRYPTED_FLAT.into());
    }
    let name = crate::store::file_name(document.path());
    let bytes = std::fs::read(document.path()).map_err(|e| format!("{name}: {e}"))?;
    let pdfium = crate::pdfium::pdfium()?;
    let held = crate::pdfium::library();
    let copy = pdfium
        .load_pdf_from_byte_vec(bytes, None)
        .map_err(|e| format!("{name} could not be read: {e}"))?;
    let pages = copy.pages().len();
    let has_form = copy.form().is_some();
    drop(held);
    let mut relink = Vec::new();
    let mut flatten = || -> Result<Vec<u8>, String> {
        for index in 0..pages as usize {
            if GIVE_UP.load(Ordering::Relaxed) {
                return Err(GIVEN_UP.to_string());
            }
            let _held = crate::pdfium::library();
            let links = links_on(&copy, index);
            flatten(&copy, index)?;
            relink.extend(put_back(&copy, index, links));
        }
        let _held = crate::pdfium::library();
        copy.save_to_bytes()
            .map_err(|e| format!("The flattened copy could not be made: {e}"))
    };
    let made = flatten();
    // Closing the document is a call into pdfium too.
    let held = crate::pdfium::library();
    drop(copy);
    drop(held);
    let work = crate::objects::Work {
        relink,
        unform: true,
        ..Default::default()
    };
    made.map(|bytes| match crate::objects::rework(bytes, &work) {
        Ok((bytes, done)) => Flat {
            bytes,
            form_kept: false,
            links_lost: done.links_lost,
        },
        // Links to pages are pointed at their pages by lopdf, so without it
        // they lead nowhere.
        Err(refused) => Flat {
            bytes: refused.bytes,
            form_kept: has_form,
            links_lost: !work.relink.is_empty(),
        },
    })
}

/// A flattened copy, from [`flattened`].
pub struct Flat {
    pub bytes: Vec<u8>,
    /// Whether the copy still has its form, which happens when lopdf
    /// cannot read it.
    pub form_kept: bool,
    /// Whether some links to pages in the copy lead nowhere, which happens
    /// for the same reason.
    pub links_lost: bool,
}

/// Where a link leads: to an address, or to a page (zero-based) and a spot
/// on it.
enum Leads {
    Away(String),
    To(usize, Option<crate::render::Pin>),
}

/// The links on page `index` of `document`, with their rectangles in the
/// page's own space. The caller holds pdfium's lock.
fn links_on(
    document: &pdfium_render::prelude::PdfDocument,
    index: usize,
) -> Vec<(pdfium_render::prelude::PdfRect, Leads)> {
    let Ok(page) = document.pages().get(index as i32) else {
        return Vec::new();
    };
    let mut links = Vec::new();
    for link in page.links().iter() {
        let Ok(rect) = link.rect() else { continue };
        let action = link.action();
        let away = (action.as_ref())
            .and_then(|action| action.as_uri_action())
            .and_then(|uri| uri.uri().ok())
            .filter(|uri| !uri.is_empty());
        let leads = match away {
            Some(uri) => Leads::Away(uri),
            None => {
                let place = link.destination().or_else(|| {
                    action
                        .as_ref()?
                        .as_local_destination_action()?
                        .destination()
                        .ok()
                });
                let Some(place) = place else { continue };
                let Ok(to) = place.page_index() else { continue };
                Leads::To(to as usize, crate::pdfium::spot_of(&place))
            }
        };
        links.push((rect, leads));
    }
    links
}

/// Put `links` on page `index` again, after flattening took every
/// annotation off it. An address is written here. A link to a page is
/// answered instead, to be pointed at its page by lopdf, since pdfium has no
/// call that writes a destination. The caller holds pdfium's lock.
fn put_back(
    document: &pdfium_render::prelude::PdfDocument,
    index: usize,
    links: Vec<(pdfium_render::prelude::PdfRect, Leads)>,
) -> Vec<crate::outline::Relink> {
    use pdfium_render::prelude::{PdfiumLibraryBindingsAccessor, FS_RECTF};
    // FPDF_ANNOT_LINK.
    const LINK: i32 = 2;
    let Ok(page) = document.pages().get(index as i32) else {
        return Vec::new();
    };
    let bindings = page.bindings();
    let handle = bindings.get_handle_from_page(&page);
    let mut relink = Vec::new();
    for (rect, leads) in links {
        unsafe {
            let annotation = bindings.FPDFPage_CreateAnnot(handle, LINK);
            if annotation.is_null() {
                continue;
            }
            let rect = FS_RECTF {
                left: rect.left().value,
                top: rect.top().value,
                right: rect.right().value,
                bottom: rect.bottom().value,
            };
            bindings.FPDFAnnot_SetRect(annotation, &rect);
            // With no border written, other programs draw a box around it.
            bindings.FPDFAnnot_SetBorder(annotation, 0.0, 0.0, 0.0);
            match leads {
                Leads::Away(uri) => {
                    bindings.FPDFAnnot_SetURI(annotation, &uri);
                }
                Leads::To(to, spot) => relink.push(crate::outline::Relink {
                    page: index,
                    annotation: (bindings.FPDFPage_GetAnnotCount(handle) - 1) as usize,
                    to,
                    spot,
                }),
            }
            bindings.FPDFPage_CloseAnnot(annotation);
        }
    }
    relink
}

/// Page `index` (zero-based) of `document` with its annotations drawn into
/// its content, as it is shown. The caller holds pdfium's lock.
pub(crate) fn flatten(
    document: &pdfium_render::prelude::PdfDocument,
    index: usize,
) -> Result<(), String> {
    let page = (document.pages().get(index as i32))
        .map_err(|e| format!("Page {} could not be read: {e}", index + 1))?;
    // Some annotations have no appearance stream of their own, such as
    // highlights made by other programs, or by this one before it wrote
    // them. pdfium makes one when it draws the page, and flattening draws in
    // only the appearance that is in the file, so the page is drawn at one
    // pixel first.
    page.render(1, 1, None)
        .map_err(|e| format!("Page {} could not be drawn: {e}", index + 1))?;
    // pdfium's own call, as the page is shown rather than as it is printed.
    // pdfium-render's `flatten` asks for what is printed, and then writes the
    // page's content again from what it read before the flattening, which
    // takes the flattened marks out.
    use pdfium_render::prelude::PdfiumLibraryBindingsAccessor;
    let bindings = page.bindings();
    let handle = bindings.get_handle_from_page(&page);
    // The flag 0 is FLAT_NORMALDISPLAY, and pdfium answers 0
    // (FLATTEN_FAIL) when the page could not be flattened.
    if unsafe { bindings.FPDFPage_Flatten(handle, 0) } == 0 {
        return Err(format!("Page {} could not be flattened.", index + 1));
    }
    Ok(())
}
