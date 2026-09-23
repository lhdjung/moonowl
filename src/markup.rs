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
    if runs.iter().all(|(_, quads)| quads.is_empty()) {
        return Err("There is nothing there to highlight.".into());
    }
    let [red, green, blue] = crate::palette::read_colour(color).ok_or("That is not a colour.")?;
    edit(path, |document| {
        for (page, quads) in runs {
            if quads.is_empty() {
                continue;
            }
            mark_one(document, *page, quads, (red, green, blue), author)?;
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
        // Who made it, which is what every other reader shows in the margin.
        let _ = annotation.set_creator(author);
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
/// document to fail the same way a second time is how a paper is lost.
fn write_over(target: &std::path::Path, body: &[u8]) -> Result<(), String> {
    // A rename replaces a link itself, never what it points at — so a paper
    // reached through one is written where it lives, which is also what the
    // watch follows. See `watch::follow`.
    let target = &std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    let written = crate::config::atomic_write_keeping(target, body);
    #[cfg(windows)]
    let written = written.or_else(|_| std::fs::write(target, body).map_err(|e| e.to_string()));
    written.map_err(|e| format!("{}: {e}", target.display()))
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
    let Some(cell) = text.boxes.get(at) else {
        return false;
    };
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
