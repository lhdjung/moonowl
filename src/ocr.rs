//! Text recognised on scanned pages, and written into each page as text that
//! is not drawn, over the picture, as ocrmypdf does. The page then looks as
//! it did, and its words can be searched, selected, highlighted and copied
//! like any other page's.
//!
//! The words are recognised by the engine the system already has: Vision on
//! macOS, and Tesseract elsewhere when it is installed. Only pages with no
//! text of their own are recognised, so a page that has text is never given
//! a second copy of it.

use crate::export::GIVE_UP;
use crate::layout::View;
use crate::render::PageSource;
use crate::render::Rect;
use pdfium_render::prelude::{
    PdfPageContentRegenerationStrategy, PdfPageObjectCommon, PdfPageObjectsCommon,
    PdfPageTextRenderMode, PdfPoints,
};
use std::sync::atomic::Ordering;

/// One word found on a page. Its box is in fractions of the page as it is
/// drawn, from the top left. It runs across the word, and down from the
/// top of the line's capitals to the foot of its descenders, as Vision
/// gives it, so that a short word is set at the size of its line.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub at: Rect,
}

/// The resolution a page is recognised at. Both engines read type well at
/// 300 dpi.
const DPI: f64 = 300.0;

/// The most pixels a page is recognised at, so that a poster is not
/// billions of them.
const MOST_PIXELS: f64 = 12_000_000.0;

/// Said where neither engine can be used.
pub const NO_ENGINE: &str =
    "Recognising text needs Tesseract, which is not installed on this computer.";

/// The words on every page of `document` that has no text of its own, by
/// page index (zero-based). A page where nothing was found is listed with no
/// words. The pages are drawn one at a time, between the pages drawn for
/// the window.
pub fn recognise(document: &dyn PageSource) -> Result<Vec<(usize, Vec<Word>)>, String> {
    let mut found = Vec::new();
    for index in 0..document.pages() {
        if GIVE_UP.load(Ordering::Relaxed) {
            return Err("The app was ending, so no text was recognised.".into());
        }
        let text =
            (document.text_aside(index)).ok_or(crate::export::CLOSED_WHILE_READ.to_string())?;
        if text.chars.iter().any(|c| !c.is_whitespace()) {
            continue;
        }
        let size = document.size_of(index);
        let scale = (DPI / 72.0).min((MOST_PIXELS / (size.width * size.height)).sqrt());
        let pixels = |points: f64| (points * scale).round().max(1.0) as u32;
        let view = View {
            rotation: 0,
            crop: None,
        };
        let drawn = document.render_aside(index, pixels(size.width), pixels(size.height), view)?;
        let grey: Vec<u8> = (drawn.bgra.as_chunks::<4>().0.iter())
            .map(|[b, g, r, _]| {
                ((*r as u32 * 299 + *g as u32 * 587 + *b as u32 * 114) / 1000) as u8
            })
            .collect();
        drop(drawn.bgra);
        found.push((
            index,
            words_in(&grey, drawn.width, drawn.height, scale * 72.0)?,
        ));
    }
    Ok(found)
}

/// The words in a picture of a page, one byte of grey for each pixel,
/// drawn at `dpi` dots an inch.
pub fn words_in(grey: &[u8], width: u32, height: u32, dpi: f64) -> Result<Vec<Word>, String> {
    #[cfg(target_os = "macos")]
    return vision(grey, width, height, dpi);
    #[cfg(not(target_os = "macos"))]
    return tesseract(grey, width, height, dpi);
}

/// The picture as a PNG that says its resolution. Tesseract takes a picture
/// that does not say to be 70 dpi, and sizes what it looks for by that.
fn png(grey: &[u8], width: u32, height: u32, dpi: f64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let per_metre = (dpi / 0.0254).round() as u32;
    encoder.set_pixel_dims(Some(png::PixelDimensions {
        xppu: per_metre,
        yppu: per_metre,
        unit: png::Unit::Meter,
    }));
    (encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(grey)))
    .map_err(|e| format!("The page could not be prepared for recognising: {e}"))?;
    Ok(out)
}

/// The words Tesseract finds, read from its table of words. It reads
/// English.
///
/// ponytail: English only; pass `-l` with the reader's languages when a
/// scan in another language comes out wrong.
pub fn tesseract(grey: &[u8], width: u32, height: u32, dpi: f64) -> Result<Vec<Word>, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let png = png(grey, width, height, dpi)?;
    let mut child = Command::new("tesseract")
        .args(["stdin", "stdout", "tsv"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| NO_ENGINE.to_string())?;
    // Tesseract reads the whole picture before it writes anything, so the
    // picture can be written before the answer is read.
    if let Some(mut input) = child.stdin.take() {
        input
            .write_all(&png)
            .map_err(|e| format!("Tesseract could not be given the page: {e}"))?;
    }
    let output =
        (child.wait_with_output()).map_err(|e| format!("Tesseract did not answer: {e}"))?;
    if !output.status.success() {
        return Err("Tesseract could not read the page.".into());
    }
    let (width, height) = (f64::from(width), f64::from(height));
    // Columns: level, page, block, paragraph, line, word, left, top, width,
    // height, confidence and the word. Level 4 is a line and level 5 a
    // word, and a word takes its line's top and height.
    let mut line_at = (0.0, 0.0);
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .filter_map(|line| {
            let columns: Vec<&str> = line.split('\t').collect();
            let [level, _, _, _, _, _, left, top, across, down, _, text] = columns[..] else {
                return None;
            };
            let number = |column: &str| column.parse::<f64>().ok();
            if level == "4" {
                line_at = (number(top)? / height, number(down)? / height);
            }
            (level == "5" && !text.trim().is_empty()).then_some(())?;
            Some(Word {
                text: text.trim().to_string(),
                at: Rect {
                    left: number(left)? / width,
                    top: line_at.0,
                    width: number(across)? / width,
                    height: line_at.1,
                },
            })
        })
        .collect())
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::encode::{Encode, Encoding};

    #[link(name = "Vision", kind = "framework")]
    unsafe extern "C" {}

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct CGRect {
        pub x: f64,
        pub y: f64,
        pub width: f64,
        pub height: f64,
    }

    unsafe impl Encode for CGRect {
        const ENCODING: Encoding = Encoding::Struct(
            "CGRect",
            &[
                Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]),
                Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]),
            ],
        );
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct NSRange {
        pub location: usize,
        pub length: usize,
    }

    unsafe impl Encode for NSRange {
        const ENCODING: Encoding =
            Encoding::Struct("_NSRange", &[usize::ENCODING, usize::ENCODING]);
    }
}

/// The words Vision finds. Each line Vision reads is cut into words at its
/// spaces, and each word is given the box Vision gives that part of the
/// line.
#[cfg(target_os = "macos")]
fn vision(grey: &[u8], width: u32, height: u32, dpi: f64) -> Result<Vec<Word>, String> {
    use mac::{CGRect, NSRange};
    use objc2::rc::autoreleasepool;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::{msg_send, sel};
    use std::ffi::{c_void, CStr};
    use std::ptr::null_mut;

    let png = png(grey, width, height, dpi)?;
    let class = |name: &CStr| AnyClass::get(name).ok_or(NO_ENGINE.to_string());
    let (handler_class, request_class) = (
        class(c"VNImageRequestHandler")?,
        class(c"VNRecognizeTextRequest")?,
    );
    autoreleasepool(|_| unsafe {
        let data: *mut AnyObject = msg_send![class(c"NSData")?, dataWithBytes: png.as_ptr().cast::<c_void>(), length: png.len()];
        let options: *mut AnyObject = msg_send![class(c"NSDictionary")?, dictionary];
        let handler: *mut AnyObject = msg_send![handler_class, alloc];
        let handler: *mut AnyObject = msg_send![handler, initWithData: data, options: options];
        let request: *mut AnyObject = msg_send![request_class, alloc];
        let request: *mut AnyObject = msg_send![request, init];
        // VNRequestTextRecognitionLevelAccurate.
        let _: () = msg_send![request, setRecognitionLevel: 0isize];
        let _: () = msg_send![request, setUsesLanguageCorrection: Bool::YES];
        let detects = sel!(setAutomaticallyDetectsLanguage:);
        let can: Bool = msg_send![request, respondsToSelector: detects];
        if can.as_bool() {
            let _: () = msg_send![request, setAutomaticallyDetectsLanguage: Bool::YES];
        }
        let requests: *mut AnyObject = msg_send![class(c"NSArray")?, arrayWithObject: request];
        let mut error: *mut AnyObject = null_mut();
        let done: Bool = msg_send![handler, performRequests: requests, error: &mut error];
        let mut words = Vec::new();
        if done.as_bool() {
            let results: *mut AnyObject = msg_send![request, results];
            let count: usize = if results.is_null() {
                0
            } else {
                msg_send![results, count]
            };
            for at in 0..count {
                let observation: *mut AnyObject = msg_send![results, objectAtIndex: at];
                let candidates: *mut AnyObject = msg_send![observation, topCandidates: 1usize];
                let best: *mut AnyObject = msg_send![candidates, firstObject];
                if best.is_null() {
                    continue;
                }
                let string: *mut AnyObject = msg_send![best, string];
                let utf8: *const std::ffi::c_char = msg_send![string, UTF8String];
                let line = CStr::from_ptr(utf8).to_string_lossy().into_owned();
                let whole: CGRect = msg_send![observation, boundingBox];
                // Vision counts a range in UTF-16 units, as NSString does.
                let mut unit = 0;
                for piece in line.split(' ') {
                    let length = piece.encode_utf16().count();
                    if !piece.trim().is_empty() {
                        let range = NSRange {
                            location: unit,
                            length,
                        };
                        let mut error: *mut AnyObject = null_mut();
                        let part: *mut AnyObject =
                            msg_send![best, boundingBoxForRange: range, error: &mut error];
                        let rect: CGRect = if part.is_null() {
                            whole
                        } else {
                            msg_send![part, boundingBox]
                        };
                        // Vision's boxes are fractions from the bottom left.
                        words.push(Word {
                            text: piece.trim().to_string(),
                            at: Rect {
                                left: rect.x,
                                top: 1.0 - rect.y - rect.height,
                                width: rect.width,
                                height: rect.height,
                            },
                        });
                    }
                    unit += length + 1;
                }
            }
        }
        let _: () = msg_send![request, release];
        let _: () = msg_send![handler, release];
        match done.as_bool() {
            true => Ok(words),
            false => Err("The text on the page could not be recognised.".into()),
        }
    })
}

/// The recognised words written into the document at `path`, each as a
/// text object that is not drawn, stretched over the word in the picture.
pub fn write(path: &str, found: &[(usize, Vec<Word>)]) -> Result<(), String> {
    crate::markup::rewrite(path, crate::objects::Update::Other, |bytes| {
        crate::markup::saved_by_pdfium(path, bytes, |document| {
            for (index, words) in found.iter().filter(|(_, words)| !words.is_empty()) {
                write_page(document, *index, words)?;
            }
            Ok(())
        })
    })
}

fn write_page(
    document: &mut pdfium_render::prelude::PdfDocument<'static>,
    index: usize,
    words: &[Word],
) -> Result<(), String> {
    let failed = |e: pdfium_render::prelude::PdfiumError| {
        format!(
            "The recognised text could not be written into page {}: {e}",
            index + 1
        )
    };
    // Helvetica is not put into the file, but it has only the Western
    // European letters; words with others are set in a system font that
    // has them, and the glyphs they use go into the file.
    let western = |word: &&Word| word.text.chars().all(crate::sign::in_win_ansi);
    let all: String = words.iter().map(|word| word.text.as_str()).collect();
    let mut words: Vec<&Word> = words.iter().collect();
    let font = if words.iter().all(western) {
        document.fonts_mut().helvetica()
    } else if let Some(bytes) = crate::sign::font_for(&all) {
        let bytes = crate::sign::subset(&bytes, &all).unwrap_or(bytes);
        (document.fonts_mut().load_true_type_from_bytes(&bytes, true)).map_err(failed)?
    } else {
        // No font has every letter, so the words Helvetica can set go in and
        // the others are left out. They would not have been seen, but a
        // search for them finds nothing.
        // ponytail: one font for the whole page. A font for each script
        // would keep every word.
        words.retain(western);
        document.fonts_mut().helvetica()
    };
    let (space, across, down) = {
        let page = document.pages().get(index as i32).map_err(failed)?;
        let space = crate::markup::Space::of(&page);
        (space, page.width().value as f64, page.height().value as f64)
    };
    let mut objects = Vec::with_capacity(words.len());
    for word in words {
        let (left, top) = (word.at.left * across, word.at.top * down);
        let (width, height) = (word.at.width * across, word.at.height * down);
        if width <= 0.0 || height <= 0.0 {
            continue;
        }
        // The box runs from the top of the capitals to the foot of the
        // descenders, which in Helvetica is 0.718 and 0.207 of the type's
        // size, with the baseline between them.
        let size = height / (0.718 + 0.207);
        let mut object = pdfium_render::prelude::PdfPageTextObject::new(
            document,
            &word.text,
            font,
            PdfPoints::new(size as f32),
        )
        .map_err(failed)?;
        object
            .set_render_mode(PdfPageTextRenderMode::Invisible)
            .map_err(failed)?;
        let set = object
            .width()
            .map(|points| points.value as f64)
            .unwrap_or(0.0);
        if set > 0.0 {
            object.scale((width / set) as f32, 1.0).map_err(failed)?;
        }
        // pdfium puts no space between two text objects when it reads the
        // page's text, so each word carries its own, past the word's box.
        object.set_text(format!("{} ", word.text)).map_err(failed)?;
        if space.turned() != 0.0 {
            (object.rotate_counter_clockwise_degrees(space.turned())).map_err(failed)?;
        }
        let (x, baseline) = space.point_up(left, top + size * 0.718);
        (object.translate(PdfPoints::new(x as f32), PdfPoints::new(baseline as f32)))
            .map_err(failed)?;
        objects.push(object);
    }
    let mut page = document.pages().get(index as i32).map_err(failed)?;
    page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
    for object in objects {
        page.objects_mut().add_text_object(object).map_err(failed)?;
    }
    page.regenerate_content().map_err(failed)
}
