//! Marking a passage in a colour, and taking the mark off again.
//!
//! The reader sweeps a sentence, chooses a colour, and a `/Subtype /Highlight`
//! with `/QuadPoints` and `/C` goes into the PDF itself — the specification's
//! own annotation, the one Preview, Acrobat and Zotero all read.
//! `markup-assessment.md` is the long form of what a highlight is and why it
//! is the only markup worth writing.
//!
//! **This is where the port stops being a port.** An annotation already in the
//! file can be *deleted* here: [`remove`] is eleven lines over
//! `FPDFPage_RemoveAnnot`. pdf.js cannot — `Annotation.save()` is not
//! overridden by any markup subtype — so the app works around the missing call
//! with several hundred lines that replay every highlight but one into a
//! pristine backup.
//!
//! What pdfium charges for it, and it is a real charge:
//!
//! *The save is a full rewrite.* `save_to_bytes` is `FPDF_SaveAsCopy` with
//! `flags = 0`, and `pdfium-render` does not expose the flags. So where the
//! app appends objects and leaves every original byte untouched, this
//! re-serialises the document. Nothing for an ordinary paper; the end of the
//! signature for a signed one. That is why [`standing`] asks its questions.
//! Nothing is left beside the document: a reader's folder is theirs, and a
//! stray `.moonowl-original` in it reads as junk.
//!
//! *And the file has to be let go of before it can be written.* See
//! [`crate::render::PageSource::release`].

use pdfium_render::prelude::{
    PdfColor, PdfDocument, PdfPageAnnotationCommon, PdfQuadPoints, PdfRect,
};

use crate::render::{PageText, Rect};

use chrono::{DateTime, FixedOffset, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};

/// One passage marked in a colour, as this reader deals with it.
///
/// **No quote and no id of its own.** The quote is not written into the file: a
/// `/Contents` on a highlight is a *note*, which a reader asks for rather than
/// has invented on their behalf, and Preview would show every mark as a
/// comment. So the words are read back off the page — [`quote_under`] — which
/// has the property that matters: it is right for markup this reader did not
/// make.
///
/// The identity is `page` and `index`, good for as long as the document is the
/// one it was read from. Every write is followed by a reopen and a re-read, so
/// an index never crosses one. The app needs a durable id because its journal is
/// written before the file is; this list is read *from* the file.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    /// One-based, as every page number in this crate is.
    pub page: usize,
    /// Where it sits among the annotations of that page.
    pub index: usize,
    /// One rectangle per line the mark covers, in the page's own points
    /// counting from the top left — the space everything else in this crate
    /// works in, and the flip from pdfium's is done where the page height is
    /// in hand.
    pub quads: Vec<Rect>,
    /// `#rrggbb`, as a theme's colours are, and read the same careful way.
    pub color: String,
    /// The comment on it, `/Contents`, which is where Preview and Acrobat
    /// write theirs. Empty for a plain mark.
    pub note: String,
}

impl Mark {
    /// Where on the page it begins, which is what the sidebar sorts by: the
    /// top of its first line, and its left edge to break a tie between two
    /// marks on the same line.
    pub fn begins(&self) -> (f64, f64) {
        self.quads.iter().fold((f64::MAX, f64::MAX), |best, quad| {
            if (quad.top, quad.left) < best {
                (quad.top, quad.left)
            } else {
                best
            }
        })
    }
}

/// Where markup on this document can go, asked once when it opens rather than
/// found out halfway through the reader's gesture.
///
/// The app's `MarkupStanding`. *Encrypted* is here, and it arrived with the password prompt: before there
/// was one, a locked document never got this far. It refuses for a reason of
/// its own rather than the app's — see [`standing`]. So is *too large*, and
/// for a reason of its own too: see [`IN_FILE_LIMIT`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Standing {
    /// Whether a mark can be written into the document at all. When this is
    /// false the mark is kept beside the document instead, and the reader is
    /// told once, in one line.
    pub into_file: bool,
    /// Why not, in a sentence, when it cannot.
    pub refused: String,
    /// Whether the document carries a signature. Asked, not refused: it is
    /// their document, and a rewrite is exactly the thing a signature is
    /// there to detect. Said once.
    pub signed: bool,
}

/// The largest document a mark is written into; past it the mark goes beside
/// the document, like any other that cannot go in.
///
/// A mark is the whole file read, rewritten by `FPDF_SaveAsCopy` and written
/// back: the file in memory three times over — as read, as pdfium holds it,
/// and as saved — and pdfium's one lock held for the length of it. The write
/// is off the thread that draws the window (`Viewer::write`), so this is no
/// longer what keeps the window moving; it is what keeps a highlight from
/// costing a gigabyte. The app's `MARKUP_IN_FILE_LIMIT`, at the same number.
pub const IN_FILE_LIMIT: u64 = 100 * 1024 * 1024;

/// Ask the disk, rather than finding out from a write that failed.
///
/// **Opening the file for writing and closing it again is the only question
/// whose answer is actually true.** Permission bits, a read-only volume, a
/// file owned by somebody else and a sandbox all come back the same way, and
/// none of them can be worked out by looking at the metadata. It is the app's
/// `document_writability`, which reached the same conclusion from the same
/// starting point.
pub fn standing(path: &str, encrypted: bool, sealed: bool) -> Standing {
    // **Asked before the disk is**, because it is the one refusal that has
    // nothing to do with the file's permissions: an encrypted document may sit
    // in a folder anybody can write to and still be a document this reader
    // will not rewrite. See [`crate::render::PageSource::encrypted`] for why.
    if encrypted {
        return Standing {
            into_file: false,
            refused: "this document is encrypted".to_string(),
            signed: false,
        };
    }
    if std::fs::metadata(path).is_ok_and(|file| file.len() > IN_FILE_LIMIT) {
        return Standing {
            into_file: false,
            refused: "this document is very large".to_string(),
            signed: false,
        };
    }
    let refused = |why: std::io::Error, what: &str| Standing {
        into_file: false,
        refused: match why.kind() {
            std::io::ErrorKind::PermissionDenied => format!("{what} is read-only"),
            _ => format!("{what} cannot be written ({why})"),
        },
        signed: false,
    };
    if let Err(why) = std::fs::OpenOptions::new().write(true).open(path) {
        return refused(why, "this document");
    }
    match folder_takes_a_file(path) {
        Ok(_) => Standing {
            into_file: true,
            refused: String::new(),
            signed: sealed,
        },
        Err(why) => refused(why, "the folder this document is in"),
    }
}

/// **The folder has to take a file too**: a write is a new file beside the
/// document renamed over it — see `config::atomic_write_keeping` — so a
/// writable document in a folder that is not passed here and failed at the
/// write. Asked the same way, by doing it. Windows writes in place when the
/// rename is refused, so there the file's answer is the whole answer.
fn folder_takes_a_file(path: &str) -> std::io::Result<()> {
    if cfg!(windows) {
        return Ok(());
    }
    let real = std::fs::canonicalize(path)?;
    let Some(folder) = real.parent() else {
        return Ok(());
    };
    let probe = folder.join(format!(".moonowl-probe-{}", std::process::id()));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)?;
    std::fs::remove_file(&probe)
}

/// Put a highlight into the document.
///
/// The quads are in the page's own points from the top left, which is the
/// space [`crate::render::PageText::quads`] answers in and the space the
/// selection is already carrying about.
pub fn add(
    path: &str,
    runs: &[(usize, Vec<Rect>)],
    color: &str,
    author: &str,
) -> Result<(), String> {
    add_noted(path, runs, color, author, "")
}

/// [`add`], with a comment on the mark — on each page's mark, where the
/// passage runs over more than one, as a reader writing in the margin
/// would see it beside every part.
pub fn add_noted(
    path: &str,
    runs: &[(usize, Vec<Rect>)],
    color: &str,
    author: &str,
    note: &str,
) -> Result<(), String> {
    if runs.iter().all(|(_, quads)| quads.is_empty()) {
        return Err("There is nothing there to highlight.".into());
    }
    let [red, green, blue] = crate::palette::read_colour(color).ok_or("That is not a colour.")?;
    edit(path, |document| {
        for (page, quads) in runs {
            if quads.is_empty() {
                continue;
            }
            mark_one(document, *page, quads, (red, green, blue), author, note)?;
        }
        Ok(())
    })
}

/// One page's worth of it, inside the one open the whole gesture costs.
///
/// **A mark belongs to a page**, so a sweep that runs across a page boundary
/// is two annotations rather than one — that is the PDF's own arrangement and
/// not a limitation of this. What is deliberately *not* two is the write: a
/// selection over three pages is one open, three annotations and one save,
/// because the save is the expensive half and a reader who marked one passage
/// did one thing.
fn mark_one(
    document: &mut PdfDocument<'static>,
    page: usize,
    quads: &[Rect],
    (red, green, blue): (u8, u8, u8),
    author: &str,
    note: &str,
) -> Result<(), String> {
    {
        let mut page = document
            .pages()
            .get(page.saturating_sub(1) as i32)
            .map_err(|e| format!("page {page}: {e}"))?;
        let space = Space::of(&page);
        let mut annotation = page
            .annotations_mut()
            .create_highlight_annotation()
            .map_err(|e| format!("the highlight could not be made: {e}"))?;
        // **`set_stroke_color` is the one that writes `/C`.** `set_fill_color`
        // writes `/IC`, the interior colour, which a highlight does not use
        // and which pdfium's own appearance stream ignores — so a mark made
        // with it is written, saved, read back with the colour it was given,
        // and drawn in black. The names come from the two entries meaning
        // outline and fill on a square or a circle; on a markup annotation
        // `/C` is simply the colour of the markup.
        annotation
            .set_stroke_color(PdfColor::new(red, green, blue, 255))
            .map_err(|e| format!("the colour was refused: {e}"))?;
        // Who made it and when, which is what every other reader shows in
        // the margin.
        let _ = annotation.set_creator(author);
        let _ = annotation.set_creation_date(Utc::now());
        let _ = annotation.set_modification_date(Utc::now());
        if !note.is_empty() {
            annotation
                .set_contents(note)
                .map_err(|e| format!("the comment was refused: {e}"))?;
        }
        // The box around the lot, because an annotation that is not
        // positioned is not drawn — `create_highlight_annotation_over_object`
        // in `pdfium-render` says so in as many words — and then the runs
        // themselves, one per line.
        annotation
            .set_bounds(space.up(&surrounding(quads)))
            .map_err(|e| format!("the highlight could not be placed: {e}"))?;
        for quad in quads {
            annotation
                .attachment_points_mut()
                .create_attachment_point_at_end(corners(quad, &space))
                .map_err(|e| format!("a run of the highlight was refused: {e}"))?;
        }
    }
    Ok(())
}

/// A PDF date, `D:20260928210958+02'00'`, as the reader's own clock says it:
/// `28 Sep 2026, 21:09`. A date with no time is only a date, and one with no
/// zone is shown as written. Anything else is handed back unchanged — see
/// [`crate::sign::in_words`] for why.
pub fn when(raw: &str) -> String {
    let digits = digits_of(raw);
    let format = if digits.len() >= 12 {
        "%-d %b %Y, %H:%M"
    } else {
        "%-d %b %Y"
    };
    match (read_date(raw), naive(&digits)) {
        (Some(at), _) => at.with_timezone(&Local).format(format).to_string(),
        (None, Some(at)) => at.format(format).to_string(),
        _ => raw.trim().to_string(),
    }
}

fn digits_of(raw: &str) -> String {
    let raw = raw.trim();
    raw.strip_prefix("D:")
        .unwrap_or(raw)
        .chars()
        .take_while(char::is_ascii_digit)
        .collect()
}

/// `YYYY[MM[DD[HH[mm[SS]]]]]`, the fields the specification lets a writer
/// leave off defaulting as it says.
fn naive(digits: &str) -> Option<NaiveDateTime> {
    let field = |from: usize, or: u32| {
        digits
            .get(from..from + 2)
            .map_or(Some(or), |d| d.parse().ok())
    };
    NaiveDate::from_ymd_opt(digits.get(0..4)?.parse().ok()?, field(4, 1)?, field(6, 1)?)?
        .and_hms_opt(field(8, 0)?, field(10, 0)?, field(12, 0)?)
}

/// A PDF date that says its zone, as an instant.
fn read_date(raw: &str) -> Option<DateTime<FixedOffset>> {
    let digits = digits_of(raw);
    let at = naive(&digits)?;
    let raw = raw.trim();
    let zone = &raw[raw.find(&digits)? + digits.len()..];
    let offset = match zone.chars().next()? {
        'Z' => FixedOffset::east_opt(0)?,
        sign @ ('+' | '-') => {
            let parts: String = zone[1..].chars().filter(char::is_ascii_digit).collect();
            let hours: i32 = parts.get(0..2)?.parse().ok()?;
            let minutes: i32 = parts.get(2..4).and_then(|m| m.parse().ok()).unwrap_or(0);
            let seconds = (hours * 60 + minutes) * 60;
            FixedOffset::east_opt(if sign == '-' { -seconds } else { seconds })?
        }
        _ => return None,
    };
    offset.from_local_datetime(&at).single()
}

/// Take one highlight out of the document, by where it sits.
///
/// **The whole of what the app needed a backup file, a detached document, a
/// replay and a refusal path for.** See this module's own note.
pub fn remove(path: &str, page: usize, index: usize) -> Result<(), String> {
    edit(path, |document| {
        let mut page = document
            .pages()
            .get(page.saturating_sub(1) as i32)
            .map_err(|e| format!("page {page}: {e}"))?;
        let annotations = page.annotations_mut();
        let annotation = annotations
            .get(index)
            .map_err(|_| "That highlight is no longer there.".to_string())?;
        // An index is a place in a list, and a list can have moved under
        // whoever is holding one. Only what this reader writes comes out by
        // it: never somebody's link or comment that slid into the place.
        if !matches!(
            annotation,
            pdfium_render::prelude::PdfPageAnnotation::Highlight(_)
                | pdfium_render::prelude::PdfPageAnnotation::Ink(_)
                | pdfium_render::prelude::PdfPageAnnotation::Stamp(_)
        ) {
            return Err("That highlight is no longer there.".to_string());
        }
        annotations
            .delete_annotation(annotation)
            .map_err(|e| format!("the highlight could not be taken out: {e}"))
    })
}

/// Write a comment on one highlight, by where it sits; an empty one takes
/// the comment off.
pub fn set_note(path: &str, page: usize, index: usize, note: &str) -> Result<(), String> {
    edit(path, |document| {
        let mut page = document
            .pages()
            .get(page.saturating_sub(1) as i32)
            .map_err(|e| format!("page {page}: {e}"))?;
        let gone = || "That highlight is no longer there.".to_string();
        let mut mark = page.annotations_mut().get(index).map_err(|_| gone())?;
        // By index, so only a highlight: see [`remove`].
        if !matches!(
            mark,
            pdfium_render::prelude::PdfPageAnnotation::Highlight(_)
        ) {
            return Err(gone());
        }
        mark.set_contents(note)
            .map_err(|e| format!("the comment was refused: {e}"))?;
        let _ = mark.set_modification_date(Utc::now());
        Ok(())
    })
}

/// Give one highlight another colour, by where it sits.
///
/// **Made again rather than painted over.** `FPDFAnnot_SetColor` refuses an
/// annotation that carries an appearance stream — every highlight another
/// reader wrote does — and pdfium-render's fallback then hands the
/// annotation to a call meant for page objects. So a new mark goes over the
/// same runs, with the old one's author and note, and the old one comes out:
/// one write, as a removal is.
///
/// ponytail: replies (`/IRT`) and a popup pointing at the old mark are not
/// carried over. Recolouring in place wants `FPDFAnnot_SetAP` on the
/// annotation's handle, which pdfium-render keeps to itself.
pub fn recolour(path: &str, page: usize, index: usize, color: &str) -> Result<(), String> {
    let [red, green, blue] = crate::palette::read_colour(color).ok_or("That is not a colour.")?;
    edit(path, |document| {
        let mut page = document
            .pages()
            .get(page.saturating_sub(1) as i32)
            .map_err(|e| format!("page {page}: {e}"))?;
        let annotations = page.annotations_mut();
        let gone = || "That highlight is no longer there.".to_string();
        let old = annotations.get(index).map_err(|_| gone())?;
        // By index, so only a highlight: see [`remove`].
        if !matches!(old, pdfium_render::prelude::PdfPageAnnotation::Highlight(_)) {
            return Err(gone());
        }
        let bounds = old
            .bounds()
            .map_err(|e| format!("the highlight could not be read: {e}"))?;
        let quads: Vec<PdfQuadPoints> = old.attachment_points().iter().collect();
        let (creator, note) = (old.creator(), old.contents());
        let (made, changed) = (old.creation_date(), old.modification_date());
        let mut new = annotations
            .create_highlight_annotation()
            .map_err(|e| format!("the highlight could not be made: {e}"))?;
        // `/C`, as in [`mark_one`].
        new.set_stroke_color(PdfColor::new(red, green, blue, 255))
            .map_err(|e| format!("the colour was refused: {e}"))?;
        if let Some(creator) = creator {
            let _ = new.set_creator(&creator);
        }
        if let Some(note) = note.filter(|note| !note.is_empty()) {
            let _ = new.set_contents(&note);
        }
        if let Some(at) = made.as_deref().and_then(read_date) {
            let _ = new.set_creation_date(at.with_timezone(&Utc));
        }
        if let Some(at) = changed.as_deref().and_then(read_date) {
            let _ = new.set_modification_date(at.with_timezone(&Utc));
        }
        new.set_bounds(bounds)
            .map_err(|e| format!("the highlight could not be placed: {e}"))?;
        for quad in quads {
            new.attachment_points_mut()
                .create_attachment_point_at_end(quad)
                .map_err(|e| format!("a run of the highlight was refused: {e}"))?;
        }
        annotations
            .delete_annotation(old)
            .map_err(|e| format!("the old highlight could not be taken out: {e}"))
    })
}

/// Take every highlight on `pages` out of the document, in one write. Links,
/// comments and signatures stay.
pub fn remove_all(path: &str, pages: &[usize]) -> Result<(), String> {
    edit(path, |document| {
        for &number in pages {
            let mut page = document
                .pages()
                .get(number.saturating_sub(1) as i32)
                .map_err(|e| format!("page {number}: {e}"))?;
            let annotations = page.annotations_mut();
            // From the end, so that taking one out moves none still to come.
            for index in (0..annotations.len()).rev() {
                if let Ok(annotation @ pdfium_render::prelude::PdfPageAnnotation::Highlight(_)) =
                    annotations.get(index)
                {
                    annotations
                        .delete_annotation(annotation)
                        .map_err(|e| format!("a highlight could not be taken out: {e}"))?;
                }
            }
        }
        Ok(())
    })
}

/// Open the document, change it, and write it back where it came from.
///
/// The document is loaded **from bytes** rather than from the path, which is
/// deliberate: the file is about to be replaced, and a pdfium document loaded
/// from a path reads from that path for as long as it lives. Loading a copy
/// into memory for the length of one edit is the version of this that cannot
/// read half of one file and half of another.
pub(crate) fn edit(
    path: &str,
    change: impl FnOnce(&mut PdfDocument<'static>) -> Result<(), String>,
) -> Result<(), String> {
    // **A whole document or none.** The check on the stamp that `write`
    // makes comes a moment before this read, and a compiler starting in that
    // moment handed pdfium half a draft — which it repairs, and which was
    // then renamed over the draft still being written.
    if crate::watch::whole(std::path::Path::new(path)).is_none() {
        return Err(
            "The document is being written by something else. Try again in a moment.".into(),
        );
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let written = {
        let _library = crate::pdfium::library();
        let pdfium = crate::pdfium::pdfium()?;
        let mut document = pdfium
            .load_pdf_from_byte_vec(bytes, None)
            .map_err(|e| format!("{path}: {e}"))?;
        change(&mut document)?;
        document
            .save_to_bytes()
            .map_err(|e| format!("the document could not be saved: {e}"))?
    };
    write_over(std::path::Path::new(path), &written)
}

/// **The document as it was before a change, for undo.** A copy in the
/// config directory rather than bytes in memory, because a paper is a few
/// megabytes a step and a scanned volume a hundred; gone when dropped.
/// Taken on the write's own thread, right before the change goes in.
pub struct Before(std::path::PathBuf);

impl Before {
    /// A place for one; nothing is copied until [`Before::take_to`].
    pub fn make() -> Before {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        static SWEPT: std::sync::Once = std::sync::Once::new();
        let folder = crate::config::config_dir().join("undo");
        // What a crash left behind, a day on: another process on Windows
        // may still be holding a younger one.
        SWEPT.call_once(|| {
            let day = std::time::Duration::from_secs(24 * 60 * 60);
            for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
                let old = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|at| at.elapsed().ok())
                    .is_some_and(|age| age > day);
                if old {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        });
        let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Before(folder.join(format!("{}-{next}.pdf", std::process::id())))
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }

    /// Copy the document to `at`, which is a [`Before::path`]. Apart from
    /// the value so that the copy can happen on the write's thread.
    pub fn take_to(at: &std::path::Path, path: &str) -> Result<(), String> {
        let folder = at.parent().unwrap_or(std::path::Path::new("."));
        std::fs::create_dir_all(folder)
            .and_then(|_| std::fs::copy(path, at))
            .map(|_| ())
            .map_err(|e| format!("The document could not be kept for undo: {e}"))
    }

    pub fn put_back(&self, path: &str) -> Result<(), String> {
        let body = std::fs::read(&self.0).map_err(|e| format!("Nothing to undo to: {e}"))?;
        write_over(std::path::Path::new(path), &body)
    }
}

impl Drop for Before {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Replace the document, atomically where the platform allows it.
///
/// `atomic_write` is the app's own and is what everything else in this crate
/// writes through: write beside the target, rename over the top, so there is
/// never a half-written file on disk. It is tried first here for that reason,
/// and it can fail for one that has nothing to do with this write — a
/// document held open by another program on Windows cannot be renamed over,
/// because the handle it is held by does not grant `FILE_SHARE_DELETE`. So
/// the fallback is the ordinary truncate-and-fill, which is not atomic and
/// says so: it is the difference between "this write may leave a broken file
/// if the machine stops in the middle of it" and "this write cannot happen at
/// all".
///
/// **On Windows only.** Anywhere else the atomic write fails for reasons the
/// fallback shares — a full disk above all — and truncating the reader's
/// document to fail the same way a second time is how a paper is lost. On
/// Windows that is still true, which is what [`fill_in_place`] is careful of.
fn write_over(target: &std::path::Path, body: &[u8]) -> Result<(), String> {
    // A rename replaces a link itself, never what it points at — so a paper
    // reached through one is written where it lives, which is also what the
    // watch follows. See `watch::follow`.
    let target = &std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    let written = crate::config::atomic_write_keeping(target, body);
    #[cfg(windows)]
    let written = written.or_else(|_| fill_in_place(target, body));
    written.map_err(|e| format!("{}: {e}", target.display()))
}

/// Truncate-and-fill, with the document copied aside first: a disk too full
/// for the copy is too full for the write, and the original is not touched;
/// a fill that stops half-way is put back from the copy.
#[cfg(windows)]
fn fill_in_place(target: &std::path::Path, body: &[u8]) -> Result<(), String> {
    let aside = target.with_extension("moonowl-aside");
    std::fs::copy(target, &aside).map_err(|e| e.to_string())?;
    let filled = std::fs::write(target, body).map_err(|e| e.to_string());
    if filled.is_err() {
        let _ = std::fs::copy(&aside, target);
    }
    let _ = std::fs::remove_file(&aside);
    filled
}

/// The words under a mark, read off the page rather than out of the file.
///
/// A character belongs to a run when its middle is inside it, which is the
/// test the search already uses to decide what a match covers and the one
/// that survives a box overlapping its neighbour by a hair.
pub fn quote_under(text: &PageText, quads: &[Rect]) -> String {
    let mut out = String::new();
    for (at, cell) in text.boxes.iter().enumerate() {
        if cell.width <= 0.0 && cell.height <= 0.0 {
            // A character pdfium generated rather than one the printer drew —
            // a space, a line break. It has no box, so it cannot be inside
            // one; it is kept for the reason `text_of` keeps it, which is
            // that it is what makes two words two words.
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            continue;
        }
        if inside_any(text, quads, at) {
            out.push(text.chars[at]);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn inside_any(text: &PageText, quads: &[Rect], at: usize) -> bool {
    let Some(&cell) = text.boxes.get(at) else {
        return false;
    };
    let cell = Rect::from(cell);
    let (x, y) = (cell.left + cell.width / 2.0, cell.top + cell.height / 2.0);
    quads.iter().any(|quad| {
        x >= quad.left
            && x <= quad.left + quad.width
            && y >= quad.top
            && y <= quad.top + quad.height
    })
}

/// The box around every run of a mark.
pub fn surrounding(quads: &[Rect]) -> Rect {
    let left = quads.iter().map(|quad| quad.left).fold(f64::MAX, f64::min);
    let top = quads.iter().map(|quad| quad.top).fold(f64::MAX, f64::min);
    let right = quads
        .iter()
        .map(|quad| quad.left + quad.width)
        .fold(f64::MIN, f64::max);
    let bottom = quads
        .iter()
        .map(|quad| quad.top + quad.height)
        .fold(f64::MIN, f64::max);
    Rect {
        left,
        top,
        width: (right - left).max(0.0),
        height: (bottom - top).max(0.0),
    }
}

/// One run of a mark, as the four points `/QuadPoints` wants — and **in the
/// order it wants them**, which is the one thing about this feature that a
/// round trip cannot catch.
///
/// The specification (12.5.6.10) numbers the corners upper-left, upper-right,
/// lower-left, lower-right, and `RectFromQuadPointsArray` reads them back that
/// way. `PdfQuadPoints::from_rect` instead *walks* the rectangle —
/// bottom-left, bottom-right, top-right, top-left — so a mark written with it
/// gets an appearance stream whose `/BBox` has no width and **nothing draws
/// it**: in the file, the right colour, invisible.
///
/// And it cannot be seen from inside this crate, because `to_rect` takes the
/// min and max of the four points and undoes `from_rect` exactly. Written
/// wrong, read back right, and only something else opening the document ever
/// finds out — which is why the test beside it renders the page rather than
/// re-reading the annotation.
fn corners(quad: &Rect, space: &Space) -> PdfQuadPoints {
    let rect = space.up(quad);
    PdfQuadPoints::new(
        rect.left(),
        rect.top(),
        rect.right(),
        rect.top(),
        rect.left(),
        rect.bottom(),
        rect.right(),
        rect.bottom(),
    )
}

/// **Between the file's space and the page as it is looked at.**
///
/// Everything pdfium says about what is *on* a page — a character's box, a
/// link, an annotation — is in the page's own user space: origin wherever the
/// file put it, y upwards, and before `/Rotate`. Everything this crate works
/// in is the page as drawn: origin at its top-left corner, y downwards, turned
/// the way the file asks. `height - y` is that conversion only for a page whose
/// box starts at 0,0 and is not turned, which is most pages and not a
/// `pdflscape` table or a journal's cropped offprint — where the page drew
/// right and every search hit, link and mark sat somewhere else.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Space {
    /// The page's box in user space, not turned.
    left: f64,
    bottom: f64,
    width: f64,
    height: f64,
    /// `/Rotate`, in clockwise quarter turns.
    turns: u8,
}

impl Space {
    pub(crate) fn of(page: &pdfium_render::prelude::PdfPage) -> Space {
        let turns = match page.rotation() {
            Ok(pdfium_render::prelude::PdfPageRenderRotation::Degrees90) => 1,
            Ok(pdfium_render::prelude::PdfPageRenderRotation::Degrees180) => 2,
            Ok(pdfium_render::prelude::PdfPageRenderRotation::Degrees270) => 3,
            _ => 0,
        };
        // As drawn, so turned: put back for a quarter turn.
        let (across, down) = (page.width().value as f64, page.height().value as f64);
        let (width, height) = if turns % 2 == 1 {
            (down, across)
        } else {
            (across, down)
        };
        // `FPDF_GetPageBoundingBox`, which despite pdfium-render's name for
        // it is the box the page is drawn from: the crop box within the media
        // box. Only its corner is wanted; the size is the page's own.
        let (left, bottom) = page
            .boundaries()
            .bounding()
            .map(|found| {
                (
                    found.bounds.left().value as f64,
                    found.bounds.bottom().value as f64,
                )
            })
            .unwrap_or((0.0, 0.0));
        Space {
            left,
            bottom,
            width,
            height,
            turns,
        }
    }

    /// A point of the file's, on the page as drawn.
    fn point_down(&self, x: f64, y: f64) -> (f64, f64) {
        let (x, y) = (x - self.left, y - self.bottom);
        match self.turns {
            1 => (y, x),
            2 => (self.width - x, y),
            3 => (self.height - y, self.width - x),
            _ => (x, self.height - y),
        }
    }

    /// A point of the drawn page, in the file's space.
    pub(crate) fn point_up(&self, x: f64, y: f64) -> (f64, f64) {
        let (x, y) = match self.turns {
            1 => (y, x),
            2 => (self.width - x, y),
            3 => (self.width - y, self.height - x),
            _ => (x, self.height - y),
        };
        (x + self.left, y + self.bottom)
    }

    /// How far down the page as drawn a point of the file's sits, as a
    /// fraction of its height — a link's `/XYZ top`, which on a turned page
    /// is an `x`, and on a cropped one counts from the box. Without an `x`
    /// a turned page's destination is its top.
    pub(crate) fn fraction_down(&self, x: Option<f64>, y: f64) -> f64 {
        // **The height of the page as drawn**, which on a quarter-turned one
        // is the width of its box: `point_down` answers in drawn space, and
        // dividing by the box's height put a link halfway down a landscape
        // page somewhere else entirely.
        let tall = if self.turns % 2 == 1 {
            self.width
        } else {
            self.height
        };
        if tall <= 0.0 {
            return 0.0;
        }
        // Without an x, the top of the page *as drawn* — which on a turned
        // page is an edge of the file's x axis, and which edge depends on the
        // turn. `self.left` is the drawn bottom of a page turned 270°.
        let x = x.unwrap_or(match self.turns {
            3 => self.left + self.width,
            _ => self.left,
        });
        let (_, down) = self.point_down(x, y);
        down / tall
    }

    /// How far the page as drawn is turned, clockwise, in degrees.
    pub(crate) fn turned(&self) -> f32 {
        self.turns as f32 * 90.0
    }

    /// A rectangle counted from the bottom of the page, said in this crate's.
    pub(crate) fn down(&self, rect: &PdfRect) -> Rect {
        let one = self.point_down(rect.left().value as f64, rect.bottom().value as f64);
        let other = self.point_down(rect.right().value as f64, rect.top().value as f64);
        Rect {
            left: one.0.min(other.0),
            top: one.1.min(other.1),
            width: (one.0 - other.0).abs(),
            height: (one.1 - other.1).abs(),
        }
    }

    /// A rectangle counted from the top of the page, said in pdfium's terms.
    pub(crate) fn up(&self, quad: &Rect) -> PdfRect {
        let one = self.point_up(quad.left, quad.top);
        let other = self.point_up(quad.left + quad.width, quad.top + quad.height);
        PdfRect::new_from_values(
            one.1.min(other.1) as f32,
            one.0.min(other.0) as f32,
            one.1.max(other.1) as f32,
            one.0.max(other.0) as f32,
        )
    }
}

/// The eight numbers a run is written as in the journal, in the page's own
/// PDF space counting from the bottom and in the specification's order.
///
/// This is the app's format rather than a shape of this crate's own: the
/// journal is `library.toml`, `library.rs` is the app's file mounted here,
/// and a journal one of them writes is one the other reads.
pub fn flat(quads: &[Rect], height: f64) -> Vec<f64> {
    let mut out = Vec::with_capacity(quads.len() * 8);
    for quad in quads {
        let (left, right) = (quad.left, quad.left + quad.width);
        let (top, bottom) = (height - quad.top, height - quad.top - quad.height);
        out.extend_from_slice(&[left, top, right, top, left, bottom, right, bottom]);
    }
    out
}

#[cfg(test)]
mod space {
    use super::*;

    /// A destination is a fraction of the page *as drawn*, and one with no x
    /// of its own is the top of it.
    #[test]
    fn a_destination_is_a_fraction_of_the_page_as_drawn() {
        for turns in 0..4 {
            let space = Space {
                left: 36.0,
                bottom: 48.0,
                width: 540.0,
                height: 708.0,
                turns,
            };
            // On a turned page the file's y says nothing about how far down
            // the drawn page a point is, so a destination with no x is the
            // top of it.
            if turns % 2 == 1 {
                let top = space.fraction_down(None, 756.0);
                assert!(top.abs() < 1e-9, "{turns}: not the top of the page: {top}");
            }
            // The middle of the page as drawn, whichever axis that is.
            let middle = if turns % 2 == 1 {
                space.fraction_down(Some(36.0 + 270.0), 400.0)
            } else {
                space.fraction_down(Some(100.0), 48.0 + 354.0)
            };
            assert!(
                (middle - 0.5).abs() < 0.01,
                "{turns}: the middle of the page is at {middle}"
            );
        }
    }

    /// Up undoes down, whichever way the page is turned — and a point in the
    /// page's box lands inside the page as drawn.
    #[test]
    fn a_point_comes_back_from_every_turn() {
        for turns in 0..4 {
            let space = Space {
                left: 36.0,
                bottom: 48.0,
                width: 540.0,
                height: 708.0,
                turns,
            };
            let (across, down) = if turns % 2 == 1 {
                (708.0, 540.0)
            } else {
                (540.0, 708.0)
            };
            let (x, y) = space.point_down(100.0, 600.0);
            assert!(
                (0.0..=across).contains(&x) && (0.0..=down).contains(&y),
                "{turns}: {x},{y}"
            );
            let (back_x, back_y) = space.point_up(x, y);
            assert!(
                (back_x - 100.0).abs() < 1e-9 && (back_y - 600.0).abs() < 1e-9,
                "{turns}"
            );
        }
    }
}

#[cfg(test)]
mod dates {
    use super::*;

    /// A zone is read as the file writes it, a missing time or zone is not
    /// invented, and what is not a date comes back as written.
    #[test]
    fn a_pdf_date_is_read_as_written() {
        let at = read_date("D:20260928210958+02'00'").unwrap();
        assert_eq!(
            at.with_timezone(&Utc).to_rfc3339(),
            "2026-09-28T19:09:58+00:00"
        );
        let west = read_date("D:20260928210958-05'30'").unwrap();
        assert_eq!(west.offset().local_minus_utc(), -(5 * 3600 + 30 * 60));
        assert!(read_date("D:20260928210958Z00'00'").is_some());
        assert_eq!(when("D:202609282109"), "28 Sep 2026, 21:09");
        assert_eq!(when("D:20260928"), "28 Sep 2026");
        assert_eq!(when("last Tuesday"), "last Tuesday");
    }
}
