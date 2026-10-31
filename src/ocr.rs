//! Text recognised on scanned pages, and written into each page as text that
//! is not drawn, over the picture, as ocrmypdf does. The page then looks as
//! it did, and its words can be searched, selected, highlighted and copied
//! like any other page's.
//!
//! The words are recognised by the engine the system already has: Vision on
//! macOS, and Tesseract elsewhere when it is installed. Only the words where
//! a page has no text of its own are kept, so a page is never given a second
//! copy of its text.

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

/// Said on a Mac whose system has no Vision to recognise text with.
#[cfg(target_os = "macos")]
const NO_VISION: &str = "This version of macOS cannot recognise text.";

/// How much of a page its pictures must cover for it to be recognised, if
/// it has text of its own. A logo or an icon is less.
const PICTURED: f64 = 0.1;

/// The words on the pages of `document` that are not text already, by page
/// index (zero-based). A page is recognised if it has no text, or if
/// pictures cover some of it: a scan with a line stamped on it, a scan
/// recognised only in part, or a page of type with a scanned table in it.
/// Only the words where the page has no text of its own are kept. A page
/// where nothing new was found is listed with no words. The pages are drawn
/// one at a time, between the pages drawn for the window.
pub fn recognise(document: &dyn PageSource) -> Result<Vec<(usize, Vec<Word>)>, String> {
    let mut found = Vec::new();
    for index in 0..document.pages() {
        if GIVE_UP.load(Ordering::Relaxed) {
            return Err("The app was ending, so no text was recognised.".into());
        }
        let text =
            (document.text_aside(index)).ok_or(crate::export::CLOSED_WHILE_READ.to_string())?;
        let typed = text.chars.iter().any(|c| !c.is_whitespace());
        if typed && document.pictured_aside(index) < PICTURED {
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
        let mut words = words_in(&grey, drawn.width, drawn.height, scale * 72.0)?;
        words.retain(|word| !over_text(word, &text, size));
        found.push((index, words));
    }
    Ok(found)
}

/// Whether a recognised word lies over text the page has already, which a
/// letter of it in the middle of the word's box says. Both are measured on
/// the page as it is drawn, from its top left.
fn over_text(word: &Word, text: &crate::render::PageText, size: crate::layout::Size) -> bool {
    let left = word.at.left * size.width;
    let top = word.at.top * size.height;
    let (right, bottom) = (
        left + word.at.width * size.width,
        top + word.at.height * size.height,
    );
    text.boxes.iter().any(|cell| {
        let x = f64::from(cell.left + cell.width / 2.0);
        let y = f64::from(cell.top + cell.height / 2.0);
        cell.width > 0.0 && (left..right).contains(&x) && (top..bottom).contains(&y)
    })
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
    tesseract_at(
        &tesseract_program(),
        TESSERACT_LIMIT,
        grey,
        width,
        height,
        dpi,
    )
}

/// The Tesseract to run. On Windows, the usual installer puts it in its own
/// folder under Program Files and does not add that folder to the path, so
/// that folder is tried first.
fn tesseract_program() -> String {
    #[cfg(windows)]
    if let Some(installed) = std::env::var_os("ProgramFiles")
        .map(|folder| std::path::Path::new(&folder).join(r"Tesseract-OCR\tesseract.exe"))
        .filter(|installed| installed.is_file())
    {
        return installed.to_string_lossy().into_owned();
    }
    "tesseract".into()
}

/// How long one page may take Tesseract. A page at 300 dpi takes a few
/// seconds; a Tesseract that has stopped answering would otherwise hold the
/// recognising thread, and with it every write, until the app is quit.
pub const TESSERACT_LIMIT: std::time::Duration = std::time::Duration::from_secs(120);

/// [`tesseract`], with the program and the time it may take named, for the
/// tests.
pub fn tesseract_at(
    program: &str,
    limit: std::time::Duration,
    grey: &[u8],
    width: u32,
    height: u32,
    dpi: f64,
) -> Result<Vec<Word>, String> {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    let png = png(grey, width, height, dpi)?;
    let mut command = Command::new(program);
    command
        .args(["stdin", "stdout", "tsv"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Otherwise a console window opens for every page.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|_| NO_ENGINE.to_string())?;
    // The answer is read on a thread of its own, so that a long table does
    // not fill the pipe and stop Tesseract before it has finished.
    let answer = child.stdout.take().map(|mut out| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            out.read_to_end(&mut bytes).map(|_| bytes)
        })
    });
    // Tesseract reads the whole picture before it writes anything, so the
    // picture can be written before the answer is read.
    if let Some(mut input) = child.stdin.take() {
        if let Err(e) = input.write_all(&png) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("Tesseract could not be given the page: {e}"));
        }
    }
    let deadline = std::time::Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Tesseract did not answer in time.".into());
            }
            Err(e) => return Err(format!("Tesseract did not answer: {e}")),
        }
    };
    if !status.success() {
        return Err("Tesseract could not read the page.".into());
    }
    let stdout = answer
        .and_then(|reading| reading.join().ok())
        .and_then(Result::ok)
        .unwrap_or_default();
    let (width, height) = (f64::from(width), f64::from(height));
    // Columns: level, page, block, paragraph, line, word, left, top, width,
    // height, confidence and the word. Level 4 is a line and level 5 a
    // word, and a word takes its line's top and height.
    let mut line_at = (0.0, 0.0);
    Ok(String::from_utf8_lossy(&stdout)
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
    let class = |name: &CStr| AnyClass::get(name).ok_or(NO_VISION.to_string());
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
                if utf8.is_null() {
                    continue;
                }
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

/// The document at `from` written to `to` with the recognised words in it,
/// each as a text object that is not drawn, stretched over the word in the
/// picture.
pub fn write(from: &str, found: &[(usize, Vec<Word>)], to: &str) -> Result<(), String> {
    let bytes = std::fs::read(from).map_err(|e| format!("{from}: {e}"))?;
    let saved = crate::markup::saved_by_pdfium(from, bytes, |document| {
        for (index, words) in found.iter().filter(|(_, words)| !words.is_empty()) {
            write_page(document, *index, words)?;
        }
        Ok(())
    })?;
    crate::atomic_write(std::path::Path::new(to), &saved.bytes)
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
