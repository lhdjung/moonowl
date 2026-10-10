//! pdfium, on its own, before any of it is on a screen.
//!
//! The prototype branch measured drawing at the 10.5 megapixels the app
//! actually renders at — fit width on a retina display — and the whole
//! argument for looking at this again is that those milliseconds are not
//! followed by 43 of bridge. This binary is the first half of that: what a
//! page costs to draw, on this machine, through this build.

use std::env;

use dioxus_spike::pdf::Document;

fn main() {
    let mut args = env::args().skip(1);
    let path = args.next().unwrap_or_else(|| {
        format!(
            "{}/../../tests/fixtures/book.pdf",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    let pages: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(10);
    // Fit width on a 1400pt-wide window at 2x, which is what the app draws at.
    let target_width: u32 = args.next().and_then(|n| n.parse().ok()).unwrap_or(2800);

    let doc = match Document::open(&path) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    println!(
        "{} pages, opened in {:.1}ms",
        doc.pages(),
        doc.opened_in
    );

    let mut total = 0.0;
    let mut megapixels = 0.0;
    let count = pages.min(doc.pages());
    for index in 0..count {
        let size = doc.sizes[index];
        let height = (target_width as f32 * size.height / size.width).round() as u32;
        match doc.render(index, target_width, height) {
            Ok(bitmap) => {
                total += bitmap.drew_in;
                megapixels += (bitmap.width as f64 * bitmap.height as f64) / 1e6;
                if index < 3 {
                    println!(
                        "page {index}: {}x{} ({:.1}MP) in {:.1}ms",
                        bitmap.width,
                        bitmap.height,
                        (bitmap.width as f64 * bitmap.height as f64) / 1e6,
                        bitmap.drew_in
                    );
                }
            }
            Err(err) => eprintln!("{err}"),
        }
    }
    println!(
        "{count} pages: {:.1}ms each on average, {:.1}MP each",
        total / count as f64,
        megapixels / count as f64
    );
}
