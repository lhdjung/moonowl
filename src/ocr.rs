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
use crate::layout::Crop;
use crate::render::PageSource;
use crate::render::Rect;
use pdfium_render::prelude::{
    PdfPageContentRegenerationStrategy, PdfPageObjectCommon, PdfPageObjectsCommon,
    PdfPageTextRenderMode, PdfPoints,
};
use std::sync::atomic::{AtomicBool, Ordering};

/// What recognising answers when the app is ending.
const GIVEN_UP: &str = "The app was ending, so no text was recognised.";

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

/// What [`recognise`] found: the words on each page it read, by page index
/// (zero-based), and how many pages the engine failed on, which are left
/// out.
#[derive(Debug, Default)]
pub struct Found {
    pub pages: Vec<(usize, Vec<Word>)>,
    pub failed: usize,
}

/// Said when the reader stops recognising text.
pub const STOPPED: &str = "Recognising text was stopped, so no copy was saved.";

/// The words on the pages of `document` that are not text already. A page is recognised if it has no text, or if
/// pictures cover some of it: a scan with a line stamped on it, a scan
/// recognised only in part, or a page of type with a scanned table in it.
/// Only the words where the page has no text of its own are kept. A page
/// of text is read only where its pictures are, and a page without text
/// whole. A page that was read and where nothing new was found is listed
/// with no words. A page the engine fails on, for example by taking too
/// long, or that cannot be drawn, is counted and left out, so that one page
/// does not cost the rest.
/// The pages are drawn one at a time, between the pages drawn for the
/// window. Recognising ends early when `stop` is set.
pub fn recognise(document: &dyn PageSource, stop: &AtomicBool) -> Result<Found, String> {
    recognise_with(document, stop, words_in)
}

/// [`recognise`], with the engine named, for the tests.
pub fn recognise_with(
    document: &dyn PageSource,
    stop: &AtomicBool,
    engine: impl Fn(&[u8], u32, u32, f64, &AtomicBool) -> Result<Vec<Word>, String>,
) -> Result<Found, String> {
    // Whether the reader stopped, or the app is ending. This is asked
    // before every part of a page and once more at the end, so that a stop
    // during the last page saves no copy either.
    let halted = || -> Result<(), String> {
        if GIVE_UP.load(Ordering::Relaxed) {
            return Err(GIVEN_UP.into());
        }
        if stop.load(Ordering::Relaxed) {
            return Err(STOPPED.into());
        }
        Ok(())
    };
    let mut found = Found::default();
    let mut first_failure = None;
    'pages: for index in 0..document.pages() {
        halted()?;
        let text =
            (document.text_aside(index)).ok_or(crate::export::CLOSED_WHILE_READ.to_string())?;
        let typed = text.chars.iter().any(|c| !c.is_whitespace());
        let size = document.size_of(index);
        let parts = match typed {
            false => vec![Crop {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            }],
            true => {
                let pictures = document.pictures_aside(index);
                let covered: f64 = pictures.iter().map(|(_, area)| area).sum();
                if covered < PICTURED * size.width * size.height {
                    continue;
                }
                parts_of(pictures.into_iter().map(|(rect, _)| rect).collect(), size)
            }
        };
        let mut words = Vec::new();
        for part in parts {
            halted()?;
            let (across, down) = (part.width * size.width, part.height * size.height);
            let scale = (DPI / 72.0).min((MOST_PIXELS / (across * down)).sqrt());
            let pixels = |points: f64| (points * scale).round().max(1.0) as u32;
            let (width, height) = (pixels(across), pixels(down));
            // A page that cannot be drawn is counted as one the engine
            // failed on, unless the document was put down meanwhile.
            let grey = match document.grey_aside(index, width, height, Some(part)) {
                Ok(grey) => grey,
                Err(_) if document.released() => {
                    return Err(crate::export::CLOSED_WHILE_READ.into())
                }
                Err(_) => {
                    found.failed += 1;
                    first_failure.get_or_insert(format!("Page {} could not be drawn.", index + 1));
                    continue 'pages;
                }
            };
            let read = match engine(&grey, width, height, scale * 72.0, stop) {
                Ok(read) => read,
                // Without an engine, no page can be read.
                Err(said) if engine_missing(&said) => return Err(said),
                Err(said) => {
                    found.failed += 1;
                    first_failure.get_or_insert(said);
                    continue 'pages;
                }
            };
            words.extend(read.into_iter().map(|word| Word {
                at: Rect {
                    left: part.x + word.at.left * part.width,
                    top: part.y + word.at.top * part.height,
                    width: word.at.width * part.width,
                    height: word.at.height * part.height,
                },
                ..word
            }));
        }
        words.retain(|word| !over_text(word, &text, size));
        found.pages.push((index, words));
    }
    halted()?;
    // Every page that was tried failed, so the reason is worth more than a
    // count.
    match first_failure {
        Some(said) if found.pages.is_empty() => Err(said),
        _ => Ok(found),
    }
}

/// Whether the engine's answer means it cannot be used at all.
fn engine_missing(said: &str) -> bool {
    #[cfg(target_os = "macos")]
    if said == NO_VISION {
        return true;
    }
    said == NO_ENGINE
}

/// The parts of a page that are pictures, as fractions of the page: the
/// pictures' boxes, with boxes that overlap joined into one, so that no
/// word is read twice. A box is kept within the page.
fn parts_of(mut boxes: Vec<Rect>, size: crate::layout::Size) -> Vec<Crop> {
    // Boxes that only touch are joined too, within a point: a scanner can
    // cut a page into strips, and a line across two strips is one line.
    const TOUCH: f64 = 1.0;
    let overlap = |a: &Rect, b: &Rect| {
        a.left < b.left + b.width + TOUCH
            && b.left < a.left + a.width + TOUCH
            && a.top < b.top + b.height + TOUCH
            && b.top < a.top + a.height + TOUCH
    };
    // Each box takes in every box after it that it overlaps, and is checked
    // against them again whenever it grows. A box that grew can overlap one
    // before it, so this is repeated until nothing is joined: a page cut
    // into a thousand tiles is one pass of about a million comparisons.
    let mut joined = true;
    while joined && !GIVE_UP.load(Ordering::Relaxed) {
        joined = false;
        let mut i = 0;
        while i < boxes.len() {
            let mut j = i + 1;
            while j < boxes.len() {
                if !overlap(&boxes[i], &boxes[j]) {
                    j += 1;
                    continue;
                }
                let (a, b) = (boxes[i], boxes.swap_remove(j));
                let (left, top) = (a.left.min(b.left), a.top.min(b.top));
                boxes[i] = Rect {
                    left,
                    top,
                    width: (a.left + a.width).max(b.left + b.width) - left,
                    height: (a.top + a.height).max(b.top + b.height) - top,
                };
                joined = true;
                j = i + 1;
            }
            i += 1;
        }
    }
    (boxes.into_iter())
        .filter_map(|rect| {
            let x = (rect.left / size.width).clamp(0.0, 1.0);
            let y = (rect.top / size.height).clamp(0.0, 1.0);
            let width = ((rect.left + rect.width) / size.width).clamp(0.0, 1.0) - x;
            let height = ((rect.top + rect.height) / size.height).clamp(0.0, 1.0) - y;
            (width > 0.0 && height > 0.0).then_some(Crop {
                x,
                y,
                width,
                height,
            })
        })
        .collect()
}

/// Whether a recognised word lies over text the page has already: the
/// page's letters cover at least a quarter of the word's box. A recognised
/// box is seldom exactly where the letters are, and one shifted by half a
/// letter misses every letter's middle, but it still covers much of them.
/// Both are measured on the page as it is drawn, from its top left.
fn over_text(word: &Word, text: &crate::render::PageText, size: crate::layout::Size) -> bool {
    let left = word.at.left * size.width;
    let top = word.at.top * size.height;
    let (right, bottom) = (
        left + word.at.width * size.width,
        top + word.at.height * size.height,
    );
    let covered: f64 = (text.boxes.iter())
        .map(|cell| {
            let (x0, y0) = (f64::from(cell.left), f64::from(cell.top));
            let (x1, y1) = (x0 + f64::from(cell.width), y0 + f64::from(cell.height));
            let across = (x1.min(right) - x0.max(left)).max(0.0);
            let down = (y1.min(bottom) - y0.max(top)).max(0.0);
            across * down
        })
        .sum();
    covered > 0.0 && covered >= (right - left) * (bottom - top) / 4.0
}

/// The words in a picture of a page, one byte of grey for each pixel,
/// drawn at `dpi` dots an inch. Tesseract is stopped when `stop` is set;
/// Vision cannot be, and finishes its page.
pub fn words_in(
    grey: &[u8],
    width: u32,
    height: u32,
    dpi: f64,
    stop: &AtomicBool,
) -> Result<Vec<Word>, String> {
    #[cfg(target_os = "macos")]
    {
        let _ = stop;
        vision(grey, width, height, dpi)
    }
    #[cfg(not(target_os = "macos"))]
    return tesseract(grey, width, height, dpi, stop);
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
/// ponytail: Tesseract is asked to read English only. If scans in other
/// languages come out wrong, it should be given the reader's languages with
/// `-l`.
pub fn tesseract(
    grey: &[u8],
    width: u32,
    height: u32,
    dpi: f64,
    stop: &AtomicBool,
) -> Result<Vec<Word>, String> {
    tesseract_at(
        &tesseract_program(),
        TESSERACT_LIMIT,
        grey,
        width,
        height,
        dpi,
        stop,
    )
}

/// The Tesseract to run. On Windows, the usual installer puts it in its own
/// folder under Program Files, or under the user's own programs when it is
/// installed for one user, and does not add that folder to the path, so
/// those folders are tried first.
fn tesseract_program() -> String {
    #[cfg(windows)]
    for (variable, under) in [
        ("ProgramFiles", r"Tesseract-OCR\tesseract.exe"),
        ("LOCALAPPDATA", r"Programs\Tesseract-OCR\tesseract.exe"),
    ] {
        if let Some(installed) = std::env::var_os(variable)
            .map(|folder| std::path::Path::new(&folder).join(under))
            .filter(|installed| installed.is_file())
        {
            return installed.to_string_lossy().into_owned();
        }
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
    stop: &AtomicBool,
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
    let mut child = command.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => NO_ENGINE.to_string(),
        _ => format!("Tesseract could not be started: {e}"),
    })?;
    // The answer is read on a thread of its own, so that a long table does
    // not fill the pipe and stop Tesseract before it has finished.
    let answer = child.stdout.take().map(|mut out| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            out.read_to_end(&mut bytes).map(|_| bytes)
        })
    });
    // The picture is given on a thread too, so that a Tesseract that never
    // reads it is given up on in time like one that never answers. Killing
    // it ends the write.
    let deadline = std::time::Instant::now() + limit;
    let giving =
        (child.stdin.take()).map(|mut input| std::thread::spawn(move || input.write_all(&png)));
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            // Quitting waits for this thread, so it does not wait for
            // Tesseract, and neither does the reader stopping it.
            Ok(None)
                if std::time::Instant::now() < deadline
                    && !GIVE_UP.load(Ordering::Relaxed)
                    && !stop.load(Ordering::Relaxed) =>
            {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(if GIVE_UP.load(Ordering::Relaxed) {
                    GIVEN_UP.into()
                } else if stop.load(Ordering::Relaxed) {
                    STOPPED.into()
                } else {
                    "Tesseract did not answer in time.".into()
                });
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Tesseract did not answer: {e}"));
            }
        }
    };
    if let Some(Ok(Err(e))) = giving.map(std::thread::JoinHandle::join) {
        return Err(format!("Tesseract could not be given the page: {e}"));
    }
    if !status.success() {
        return Err("Tesseract could not read the page.".into());
    }
    let stdout = answer
        .and_then(|reading| reading.join().ok())
        .and_then(Result::ok)
        .unwrap_or_default();
    Ok(read_table(
        &String::from_utf8_lossy(&stdout),
        f64::from(width),
        f64::from(height),
    ))
}

/// How sure Tesseract must be of a word, out of 100, for it to be kept. A
/// photograph or a plot read as text gives words it is much less sure of.
// ponytail: one number for every scan. If faint scans lose real words,
// lower it, or let the reader choose.
const TESSERACT_SURE: f64 = 30.0;

/// The words in Tesseract's table of words (its `tsv` output) for a picture
/// `width` by `height` pixels, placed as fractions of the picture. A word
/// Tesseract is not sure of is left out.
fn read_table(table: &str, width: f64, height: f64) -> Vec<Word> {
    // Columns: level, page, block, paragraph, line, word, left, top, width,
    // height, confidence and the word. Level 4 is a line and level 5 a
    // word, and a word takes its line's top and height.
    let mut line_at = (0.0, 0.0);
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let columns: Vec<&str> = line.split('\t').collect();
            let [level, _, _, _, _, _, left, top, across, down, sure, text] = columns[..] else {
                return None;
            };
            let number = |column: &str| column.parse::<f64>().ok();
            if level == "4" {
                line_at = (number(top)? / height, number(down)? / height);
            }
            (level == "5" && !text.trim().is_empty()).then_some(())?;
            (number(sure)? >= TESSERACT_SURE).then_some(())?;
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
        .collect()
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
/// picture. `read` is the stamp of the draft the words were found on.
pub fn write(
    from: &str,
    read: Option<crate::render::Stamp>,
    found: &[(usize, Vec<Word>)],
    to: &str,
) -> Result<(), String> {
    let bytes = std::fs::read(from).map_err(|e| format!("{from}: {e}"))?;
    // The words are placed on the pages they were found on, and a compiler
    // or a sync that has written the file since, perhaps only halfway, may
    // have made other pages of it.
    if crate::render::stamp_of(from) != read {
        return Err(
            "The document changed on disk while its text was being recognised, \
                    so no copy was saved. Try again."
                .into(),
        );
    }
    let (chosen, fonts) = fonts_for(found);
    let name = crate::store::file_name(from);
    let pdfium = crate::pdfium::pdfium()?;
    let held = crate::pdfium::library();
    let mut copy = pdfium
        .load_pdf_from_byte_vec(bytes, None)
        .map_err(|e| format!("{name} could not be read: {e}"))?;
    drop(held);
    // pdfium's lock is taken for one page at a time, as for a flattened
    // copy, so that the pages being drawn for the window go between them.
    let mut write = || -> Result<Vec<u8>, String> {
        let failed = |e: pdfium_render::prelude::PdfiumError| {
            format!("The fonts for the recognised text could not be loaded: {e}")
        };
        let held = crate::pdfium::library();
        let helvetica = copy.fonts_mut().helvetica();
        let mut loaded = std::collections::HashMap::new();
        for (at, bytes) in &fonts {
            let font = copy.fonts_mut().load_true_type_from_bytes(bytes, true);
            loaded.insert(*at, font.map_err(failed)?);
        }
        drop(held);
        for ((index, words), chosen) in found.iter().zip(&chosen) {
            if words.is_empty() {
                continue;
            }
            if GIVE_UP.load(Ordering::Relaxed) {
                return Err(GIVEN_UP.into());
            }
            let words: Vec<_> = (words.iter().zip(chosen))
                .filter_map(|(word, chosen)| match (*chosen)? {
                    None => Some((word, helvetica)),
                    Some(at) => Some((word, loaded[&at])),
                })
                .collect();
            let _held = crate::pdfium::library_when_free();
            write_page(&mut copy, *index, words)?;
        }
        // Quitting waits for this thread, and saving and compacting a large
        // scan takes a while.
        if GIVE_UP.load(Ordering::Relaxed) {
            return Err(GIVEN_UP.into());
        }
        let _held = crate::pdfium::library();
        copy.save_to_bytes()
            .map_err(|e| format!("the document could not be saved: {e}"))
    };
    let made = write();
    // Closing the document is a call into pdfium too.
    let held = crate::pdfium::library();
    drop(copy);
    drop(held);
    let made = made?;
    if GIVE_UP.load(Ordering::Relaxed) {
        return Err(GIVEN_UP.into());
    }
    let bytes = crate::objects::compact(made);
    // Compacting is given up on when the app is ending, and no copy is
    // written then.
    if GIVE_UP.load(Ordering::Relaxed) {
        return Err(GIVEN_UP.into());
    }
    crate::atomic_write(std::path::Path::new(to), &bytes)
}

/// The font of each recognised word, chosen before pdfium's lock is taken,
/// since that reads the system's font files. Helvetica is not put into the
/// file, but it has only the Western European letters. A word with other
/// letters is set in the first system font that has them all, and the
/// glyphs the document uses from that font go into the file once. A word
/// that no font can set is left out. It would not have been seen, but a
/// search for it finds nothing.
///
/// Answers, for each word of each page, `Some(None)` for Helvetica,
/// `Some(Some(at))` for system font `at`, or `None`; and each system font
/// used, cut down to the letters of its words.
#[allow(clippy::type_complexity)]
fn fonts_for(
    found: &[(usize, Vec<Word>)],
) -> (Vec<Vec<Option<Option<usize>>>>, Vec<(usize, Vec<u8>)>) {
    let mut system = crate::sign::SystemFonts::default();
    let chosen: Vec<Vec<Option<Option<usize>>>> = (found.iter())
        .map(|(_, words)| {
            (words.iter())
                .map(
                    |word| match word.text.chars().all(crate::sign::in_win_ansi) {
                        true => Some(None),
                        false => system.for_line(&word.text).map(Some),
                    },
                )
                .collect()
        })
        .collect();
    let mut used: std::collections::BTreeMap<usize, String> = Default::default();
    let pairs = found.iter().zip(&chosen);
    for ((_, words), chosen) in pairs {
        for (word, chosen) in words.iter().zip(chosen) {
            // Each word is written with a space after it.
            if let Some(Some(at)) = chosen {
                let used = used.entry(*at).or_default();
                used.push_str(&word.text);
                used.push(' ');
            }
        }
    }
    let fonts = (used.into_iter())
        .map(|(at, used)| {
            let whole = system.bytes(at);
            (
                at,
                crate::sign::subset(whole, &used).unwrap_or_else(|| whole.to_vec()),
            )
        })
        .collect();
    (chosen, fonts)
}

fn write_page(
    document: &mut pdfium_render::prelude::PdfDocument<'static>,
    index: usize,
    words: Vec<(&Word, pdfium_render::prelude::PdfFontToken)>,
) -> Result<(), String> {
    let failed = |e: pdfium_render::prelude::PdfiumError| {
        format!(
            "The recognised text could not be written into page {}: {e}",
            index + 1
        )
    };
    let (space, across, down) = {
        let page = document.pages().get(index as i32).map_err(failed)?;
        let space = crate::markup::Space::of(&page);
        (space, page.width().value as f64, page.height().value as f64)
    };
    let mut objects = Vec::with_capacity(words.len());
    for (word, font) in words {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Picture strips that only touch are read as one part, so that a line
    /// across two of them is read whole. Pictures apart stay apart.
    #[test]
    fn strips_that_touch_are_one_part() {
        let size = crate::layout::Size {
            width: 100.0,
            height: 100.0,
        };
        let strip = |top: f64| Rect {
            left: 0.0,
            top,
            width: 100.0,
            height: 25.0,
        };
        assert_eq!(
            parts_of(vec![strip(0.0), strip(25.0), strip(50.0)], size).len(),
            1
        );
        assert_eq!(parts_of(vec![strip(0.0), strip(60.0)], size).len(), 2);
        // A page of tiles, given out of order, is one part.
        let tile = |at: usize| Rect {
            left: (at % 40) as f64 * 2.5,
            top: (at / 40) as f64 * 2.5,
            width: 2.5,
            height: 2.5,
        };
        let tiles = (0..1600).map(|at| tile((at * 7) % 1600)).collect();
        assert_eq!(parts_of(tiles, size).len(), 1);
        // Two boxes joined can reach one that neither touched.
        let at = |left: f64, top: f64, width: f64, height: f64| Rect {
            left,
            top,
            width,
            height,
        };
        let parts = parts_of(
            vec![
                at(40.0, 40.0, 10.0, 10.0),
                at(0.0, 0.0, 10.0, 10.0),
                at(80.0, 80.0, 10.0, 10.0),
                at(5.0, 5.0, 80.0, 1.0),
                at(85.0, 5.0, 1.0, 80.0),
            ],
            size,
        );
        assert_eq!(parts.len(), 1, "{parts:?}");
    }

    /// **Tesseract's table is read on every platform**, though only a Mac
    /// in CI has an engine at all: a table Tesseract 5 wrote, with Windows
    /// line ends on one line and an empty word, which is left out.
    #[test]
    fn a_table_of_words_from_tesseract_is_read() {
        let table = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
            1\t1\t0\t0\t0\t0\t0\t0\t1000\t500\t-1\t\n\
            2\t1\t1\t0\t0\t0\t100\t50\t700\t60\t-1\t\n\
            3\t1\t1\t1\t0\t0\t100\t50\t700\t60\t-1\t\n\
            4\t1\t1\t1\t1\t0\t100\t50\t700\t60\t-1\t\r\n\
            5\t1\t1\t1\t1\t1\t100\t55\t300\t50\t96.5\tMoonowl\r\n\
            5\t1\t1\t1\t1\t2\t450\t52\t350\t55\t91.0\treads\n\
            5\t1\t1\t1\t1\t3\t820\t52\t10\t55\t0.0\t \n\
            5\t1\t1\t1\t1\t4\t840\t52\t100\t55\t12.0\t~/|;\n";
        let words = read_table(table, 1000.0, 500.0);
        let said: Vec<&str> = words.iter().map(|word| word.text.as_str()).collect();
        // The last word, which Tesseract was not sure of, is left out.
        assert_eq!(said, ["Moonowl", "reads"]);
        let at = words[1].at;
        assert!((at.left - 0.45).abs() < 1e-9 && (at.width - 0.35).abs() < 1e-9);
        // The line's top and height, not the word's own.
        assert!((at.top - 0.1).abs() < 1e-9 && (at.height - 0.12).abs() < 1e-9);
    }

    /// A recognised word shifted down by half a line still lies over the
    /// letters it was read from, though it misses every letter's middle.
    #[test]
    fn a_word_read_a_little_off_its_letters_is_over_them() {
        let cell = |left: f32| crate::render::Cell {
            left,
            top: 100.0,
            width: 10.0,
            height: 20.0,
        };
        let text = crate::render::PageText {
            chars: vec!['O', 'w', 'l'],
            boxes: vec![cell(100.0), cell(110.0), cell(120.0)],
        };
        let size = crate::layout::Size {
            width: 1000.0,
            height: 1000.0,
        };
        let word = |left: f64, top: f64| Word {
            text: "Owl".into(),
            at: Rect {
                left,
                top,
                width: 0.03,
                height: 0.02,
            },
        };
        assert!(over_text(&word(0.1, 0.111), &text, size), "shifted");
        assert!(!over_text(&word(0.1, 0.13), &text, size), "the line below");
        assert!(!over_text(&word(0.2, 0.1), &text, size), "beside it");
    }

    /// **A system font goes into the document once**, cut down to the
    /// letters of every page's words, however many pages use it.
    #[test]
    fn a_font_two_pages_use_is_chosen_once() {
        if crate::sign::font_for("Ωμέγα αβ").is_none() {
            eprintln!("no font here sets Greek");
            return;
        }
        let word = |text: &str| Word {
            text: text.into(),
            at: crate::render::Rect {
                left: 0.1,
                top: 0.1,
                width: 0.2,
                height: 0.03,
            },
        };
        let found = [
            (0, vec![word("Moonowl"), word("Ωμέγα")]),
            (1, vec![word("αβ")]),
        ];
        let (chosen, fonts) = fonts_for(&found);
        assert_eq!(fonts.len(), 1);
        let at = fonts[0].0;
        assert_eq!(
            chosen,
            [vec![Some(None), Some(Some(at))], vec![Some(Some(at))]]
        );
        let face = ttf_parser::Face::parse(&fonts[0].1, 0).unwrap();
        assert!("Ωμέγααβ ".chars().all(|c| face.glyph_index(c).is_some()));
    }

    /// **Two words that no one font sets are both kept**, each in a font of
    /// its own. Which letters the system fonts have differs from computer
    /// to computer, so the words are chosen from those this one has.
    #[test]
    fn words_no_one_font_sets_are_each_set_in_their_own() {
        use crate::sign::font_for;
        let letters = ["Iași", "日本", "₹", "ᚠ", "ئ", "Ωμέγα", "ꙮ"];
        let pair = (letters.iter())
            .flat_map(|a| letters.iter().map(move |b| (*a, *b)))
            .find(|(a, b)| {
                font_for(a).is_some()
                    && font_for(b).is_some()
                    && font_for(&format!("{a}{b}")).is_none()
            });
        let Some((a, b)) = pair else {
            eprintln!("every font here sets all of these letters, or none does");
            return;
        };
        let dir = std::env::temp_dir().join(format!("moonowl-ocr-fonts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scan.pdf").to_string_lossy().into_owned();
        std::fs::copy(crate::fixture::scanned_pdf(), &path).unwrap();
        let word = |text: &str, left: f64| Word {
            text: text.into(),
            at: crate::render::Rect {
                left,
                top: 0.1,
                width: 0.2,
                height: 0.03,
            },
        };
        let found = vec![(0, vec![word("Moonowl", 0.1), word(a, 0.4), word(b, 0.7)])];
        write(&path, crate::render::stamp_of(&path), &found, &path).expect("written");
        let document = crate::render::open(&path).unwrap();
        let text = document.text_of(0);
        let text: String = text.chars.iter().collect();
        for word in ["Moonowl", a, b] {
            assert!(text.contains(word), "{word} is in {text:?}");
        }
    }
}
