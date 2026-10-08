//! The renderer, behind the door it will always be behind.
//!
//! The assessment's rule for the new tree: one trait — render a page, ask for
//! its text, its outline, its links — so that pdfium is a decision that can be
//! remade rather than a dependency spread through the viewer. That is the same
//! rule `viewer.ts` obeys today by being the only file that imports pdf.js.
//!
//! What is here is the smallest part of it the page spike needs: how big a
//! page is, and its pixels at a given size.
//!
//! Two things about pdfium are restated from `render.rs` on the
//! `pdfium-prototype` branch, because they are properties of the library and
//! not of the app: it has one global initialiser and no thread safety, so
//! there is one instance for the process and a lock around it; and a page
//! costs nothing once dropped, so there is no page cache to keep.

use std::sync::Mutex;
use std::time::Instant;

use pdfium_render::prelude::*;

/// A page's size in PDF points.
#[derive(Clone, Copy, Debug)]
pub struct PageSize {
    pub width: f32,
    pub height: f32,
}

/// One page's pixels, as pdfium hands them over: BGRA, top row first.
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    /// What pdfium spent drawing it, which is the number the assessment's
    /// table is made of.
    pub drew_in: f64,
}

/// The one pdfium instance, created on first use and kept for the life of the
/// process. Leaked deliberately: every document and page borrows from it.
fn pdfium() -> Result<&'static Pdfium, String> {
    static INSTANCE: Mutex<Option<&'static Pdfium>> = Mutex::new(None);
    let mut held = INSTANCE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(instance) = *held {
        return Ok(instance);
    }
    let bindings = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(
        &library_dir(),
    ))
    .map_err(|e| format!("pdfium could not be loaded: {e}"))?;
    let instance: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bindings)));
    *held = Some(instance);
    Ok(instance)
}

/// Where `libpdfium` is. In the spike it is vendored beside the crate; in a
/// bundled app it would sit beside the executable. Nothing is fetched at
/// runtime, which is the promise the pdf.js assets make today.
fn library_dir() -> String {
    if let Ok(dir) = std::env::var("SPIKE_PDFIUM") {
        return dir;
    }
    format!("{}/vendor/lib", env!("CARGO_MANIFEST_DIR"))
}

/// A document, open, with one lock around everything pdfium touches.
pub struct Document {
    inner: Mutex<Open>,
    pub sizes: Vec<PageSize>,
    /// What opening it cost — pdf.js spends most of a document open starting
    /// its worker, so this is the other half of that comparison.
    pub opened_in: f64,
}

struct Open {
    document: PdfDocument<'static>,
}

// pdfium is not thread safe and everything here is behind the lock; the
// `PdfDocument` borrows from the leaked instance, which lives forever.
unsafe impl Send for Open {}

/// Two handles to the same open document are the same document, and no two
/// documents are ever equal. Dioxus wants this to decide whether a component's
/// props have changed; there is nothing in an open file to compare.
impl PartialEq for Document {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Document {
    pub fn open(path: &str) -> Result<Self, String> {
        let began = Instant::now();
        let pdfium = pdfium()?;
        let document = pdfium
            .load_pdf_from_file(path, None)
            .map_err(|e| format!("{path}: {e}"))?;
        let sizes = document
            .pages()
            .iter()
            .map(|page| PageSize {
                width: page.width().value,
                height: page.height().value,
            })
            .collect();
        Ok(Document {
            sizes,
            opened_in: began.elapsed().as_secs_f64() * 1000.0,
            inner: Mutex::new(Open { document }),
        })
    }

    pub fn pages(&self) -> usize {
        self.sizes.len()
    }

    /// Draw one page at exactly this many pixels.
    pub fn render(&self, index: usize, width: u32, height: u32) -> Result<Bitmap, String> {
        let held = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let page = held
            .document
            .pages()
            .get(index as i32)
            .map_err(|e| format!("page {index}: {e}"))?;
        let config = PdfRenderConfig::new().set_target_size(width as i32, height as i32);
        let began = Instant::now();
        let bitmap = page
            .render_with_config(&config)
            .map_err(|e| format!("page {index}: {e}"))?;
        let drew_in = began.elapsed().as_secs_f64() * 1000.0;
        Ok(Bitmap {
            width: bitmap.width() as u32,
            height: bitmap.height() as u32,
            bgra: bitmap.as_raw_bytes().to_vec(),
            drew_in,
        })
    }
}
