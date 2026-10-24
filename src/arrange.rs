//! Page edits: removing pages, turning them in the file, moving them, putting
//! another file's pages in, and writing some pages, or several files, into a
//! new file. `plans/page-edits.md` is the plan these follow.
//!
//! An edit of the open document goes through `markup::edit_into`, as a
//! highlight does, and is written either into the document itself or into a
//! copy of it. Every edit says where each old page went ([`Moved`]), because
//! the reader's place, their bookmarks, their way back and the pages' spaces
//! all have to follow the pages.
//!
//! What pdfium cannot do limits these edits. It cannot write the outline or
//! `/PageLabels`, so a heading on a removed page leads nowhere afterwards,
//! and labels stay with the positions rather than the pages. The reader is
//! told both in the notice when they apply.

use pdfium_render::prelude::{
    PdfDocument, PdfPageRenderRotation, PdfiumError, PdfiumInternalError,
    PdfiumLibraryBindingsAccessor, FPDF_FILEWRITE,
};

/// More pages than this removed at once from the document itself are asked
/// about first. In a copy, undo is enough.
pub const ASK_BEYOND: usize = 5;

/// A page edit on the open document. Every page here is one-based.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// These pages taken out.
    Remove(Vec<usize>),
    /// These pages turned in the file by this many clockwise quarter turns.
    Turn(Vec<usize>, i32),
    /// These pages moved to just before this page. One past the last page is
    /// the end of the document.
    Move(Vec<usize>, usize),
    /// Every page of the file at this path put in before this page.
    Insert(String, usize),
}

/// Where each page of the document before an edit is after it, by zero-based
/// index: `None` for a page that was removed.
#[derive(Clone, Debug, PartialEq)]
pub struct Moved(pub Vec<Option<usize>>);

impl Moved {
    /// Where one-based `page` went, one-based, or `None` if it was removed.
    pub fn page(&self, page: usize) -> Option<usize> {
        let to = *self.0.get(page.checked_sub(1)?)?;
        to.map(|index| index + 1)
    }

    /// Where a reader on one-based `page` lands: the page itself, or for a
    /// removed page the first page after it that is still there, or the last
    /// page that is.
    pub fn landing(&self, page: usize) -> usize {
        let from = page.saturating_sub(1).min(self.0.len().saturating_sub(1));
        let after = self.0.iter().skip(from).flatten().next();
        let before = self.0.iter().take(from).flatten().next_back();
        after.or(before).map_or(1, |index| index + 1)
    }
}

impl Edit {
    /// The pages the edit is about, one-based and in order.
    pub fn pages(&self) -> Vec<usize> {
        match self {
            Edit::Remove(pages) | Edit::Turn(pages, _) | Edit::Move(pages, _) => pages.clone(),
            Edit::Insert(..) => Vec::new(),
        }
    }

    /// Where every page went, for a document of `before` pages that had
    /// `after` once the edit was written.
    pub fn moved(&self, before: usize, after: usize) -> Moved {
        let picked = |page: usize, pages: &[usize]| pages.contains(&(page + 1));
        Moved(match self {
            Edit::Turn(..) => (0..before).map(Some).collect(),
            Edit::Remove(pages) => {
                let mut kept = 0;
                (0..before)
                    .map(|index| {
                        (!picked(index, pages)).then(|| {
                            kept += 1;
                            kept - 1
                        })
                    })
                    .collect()
            }
            Edit::Move(pages, to) => {
                let order = moved_order(before, pages, *to);
                let mut map = vec![None; before];
                for (new, &old) in order.iter().enumerate() {
                    map[old] = Some(new);
                }
                map
            }
            Edit::Insert(_, to) => {
                let added = after.saturating_sub(before);
                (0..before)
                    .map(|index| {
                        Some(if index + 1 < *to {
                            index
                        } else {
                            index + added
                        })
                    })
                    .collect()
            }
        })
    }

    /// Whether the edit would leave every page where it is, in a document of
    /// `count` pages: pages moved to where they already are.
    pub fn moves_nothing(&self, count: usize) -> bool {
        matches!(self, Edit::Move(..))
            && (self.moved(count, count).0.iter().enumerate()).all(|(at, to)| *to == Some(at))
    }

    /// The pages' spaces as they are after the edit, from what was known of
    /// them before. A page that came in from another file is not known yet.
    pub fn spaces(
        &self,
        old: &[Option<crate::markup::Space>],
        after: usize,
    ) -> Vec<Option<crate::markup::Space>> {
        let moved = self.moved(old.len(), after);
        let mut new = vec![None; after];
        for (index, space) in old.iter().enumerate() {
            let Some(to) = moved.0[index].filter(|&to| to < after) else {
                continue;
            };
            new[to] = match (self, space) {
                (Edit::Turn(pages, quarters), Some(space)) if pages.contains(&(index + 1)) => {
                    Some(space.turned_by(*quarters))
                }
                _ => *space,
            };
        }
        new
    }

    /// Write the edit: read `path`, change it, and write the result to `to`,
    /// which is `path` itself or the copy the reader named. Answers whether
    /// pages put in from another file had links between them, which do not
    /// come with them.
    pub fn write(&self, path: &str, to: &str) -> Result<bool, String> {
        let mut lost = false;
        match self {
            Edit::Move(pages, before) => {
                let (pages, before) = (pages.clone(), *before);
                crate::markup::rewrite(path, to, move |bytes| move_pages(bytes, &pages, before))?
            }
            _ => crate::markup::edit_into(path, to, |document| {
                lost = self.apply(document)?;
                Ok(())
            })?,
        }
        Ok(lost)
    }

    fn apply(&self, document: &mut PdfDocument<'static>) -> Result<bool, String> {
        let count = document.pages().len() as usize;
        match self {
            Edit::Remove(pages) => {
                if pages.len() >= count {
                    return Err("A document needs at least one page.".into());
                }
                // From the end, so that taking one out moves none still to come.
                let mut pages = pages.clone();
                pages.sort_unstable();
                for &page in pages.iter().rev() {
                    page_of(document, page)?
                        .delete()
                        .map_err(|e| format!("page {page} could not be removed: {e}"))?;
                }
            }
            Edit::Turn(pages, quarters) => {
                for &page in pages {
                    let mut turned = page_of(document, page)?;
                    let now = match turned.rotation() {
                        Ok(PdfPageRenderRotation::Degrees90) => 1,
                        Ok(PdfPageRenderRotation::Degrees180) => 2,
                        Ok(PdfPageRenderRotation::Degrees270) => 3,
                        _ => 0,
                    };
                    turned.set_rotation(match (now + quarters).rem_euclid(4) {
                        1 => PdfPageRenderRotation::Degrees90,
                        2 => PdfPageRenderRotation::Degrees180,
                        3 => PdfPageRenderRotation::Degrees270,
                        _ => PdfPageRenderRotation::None,
                    });
                }
            }
            Edit::Insert(other, before) => {
                let pdfium = crate::pdfium::pdfium()?;
                let source = load_source(pdfium, other)?;
                let last = source.pages().len() - 1;
                let at = (*before).clamp(1, count + 1) - 1;
                let lost = links_between(&source, None);
                document
                    .pages_mut()
                    .copy_page_range_from_document(&source, 0..=last, at as i32)
                    .map_err(|e| format!("the pages could not be put in: {e}"))?;
                return Ok(lost);
            }
            Edit::Move(..) => unreachable!("moving is written by `move_pages`"),
        }
        Ok(false)
    }
}

/// The old zero-based indices in their new order, for `pages` (one-based)
/// moved to just before page `to`.
fn moved_order(count: usize, pages: &[usize], to: usize) -> Vec<usize> {
    let picked = |index: &usize| pages.contains(&(index + 1));
    let (moving, staying): (Vec<usize>, Vec<usize>) = (0..count).partition(picked);
    let at = staying.iter().filter(|&&index| index + 1 < to).count();
    let mut order = staying[..at].to_vec();
    order.extend(moving);
    order.extend(&staying[at..]);
    order
}

fn page_of(
    document: &PdfDocument<'static>,
    page: usize,
) -> Result<pdfium_render::prelude::PdfPage<'static>, String> {
    document
        .pages()
        .get(page.saturating_sub(1) as i32)
        .map_err(|e| format!("page {page}: {e}"))
}

/// Another file, read whole, for its pages. An encrypted one is refused,
/// because its password is not known here.
fn load_source(
    pdfium: &'static pdfium_render::prelude::Pdfium,
    path: &str,
) -> Result<PdfDocument<'static>, String> {
    let name = crate::store::file_name(path);
    if crate::watch::whole(std::path::Path::new(path)).is_none() {
        return Err(format!("{name} is not a whole PDF."));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{name}: {e}"))?;
    pdfium
        .load_pdf_from_byte_vec(bytes, None)
        .map_err(|e| match e {
            PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
                format!("{name} is encrypted, so its pages cannot be copied.")
            }
            e => format!("{name} could not be read: {e}"),
        })
}

/// Pages moved with `FPDF_MovePages`, which keeps each page the same object,
/// so headings and links that point at a page still find it. pdfium-render
/// has no wrapper for it and keeps a document's handle to itself, so the
/// document is loaded and saved through the raw calls.
fn move_pages(bytes: Vec<u8>, pages: &[usize], before: usize) -> Result<Vec<u8>, String> {
    let _library = crate::pdfium::library();
    let bindings = crate::pdfium::pdfium()?.bindings();
    let document = unsafe { bindings.FPDF_LoadMemDocument64(&bytes, None) };
    if document.is_null() {
        return Err("The document could not be read.".into());
    }
    let count = unsafe { bindings.FPDF_GetPageCount(document) } as usize;
    let mut indices: Vec<i32> = pages.iter().map(|&page| page as i32 - 1).collect();
    indices.sort_unstable();
    // `FPDF_MovePages` places the first moved page at this index of the
    // document as it is once the moved pages are taken out.
    let to = (0..count)
        .filter(|index| index + 1 < before && !pages.contains(&(index + 1)))
        .count();
    let moved = unsafe {
        bindings.FPDF_MovePages(
            document,
            indices.as_ptr(),
            indices.len() as std::os::raw::c_ulong,
            to as i32,
        )
    };
    let saved = if bindings.is_true(moved) {
        save_raw(bindings, document)
    } else {
        Err("The pages could not be moved.".into())
    };
    unsafe { bindings.FPDF_CloseDocument(document) };
    saved
}

/// `FPDF_SaveAsCopy` into memory, with no flags: what `save_to_bytes` does.
fn save_raw(
    bindings: &dyn pdfium_render::prelude::PdfiumLibraryBindings,
    document: pdfium_render::prelude::FPDF_DOCUMENT,
) -> Result<Vec<u8>, String> {
    #[repr(C)]
    struct Sink {
        write: FPDF_FILEWRITE,
        bytes: Vec<u8>,
    }
    unsafe extern "C" fn block(
        this: *mut FPDF_FILEWRITE,
        data: *const std::os::raw::c_void,
        size: std::os::raw::c_ulong,
    ) -> std::os::raw::c_int {
        // `write` is the first field of a `repr(C)` `Sink`, so the pointer
        // pdfium hands back is the sink's.
        let sink = unsafe { &mut *(this as *mut Sink) };
        let data = unsafe { std::slice::from_raw_parts(data as *const u8, size as usize) };
        sink.bytes.extend_from_slice(data);
        1
    }
    let mut sink = Sink {
        write: FPDF_FILEWRITE {
            version: 1,
            WriteBlock: Some(block),
        },
        bytes: Vec::new(),
    };
    let saved = unsafe { bindings.FPDF_SaveAsCopy(document, &mut sink.write, 0) };
    if bindings.is_true(saved) {
        Ok(sink.bytes)
    } else {
        Err("the document could not be saved".into())
    }
}

/// Whether any of `pages` (one-based, or every page for `None`) has a link
/// to a page of the same document. pdfium copies a page without the pages
/// such a link points at, so in another document the link leads nowhere.
fn links_between(document: &PdfDocument, pages: Option<&[usize]>) -> bool {
    let wanted = |index: usize| pages.is_none_or(|pages| pages.contains(&(index + 1)));
    (document.pages().iter().enumerate())
        .filter(|(index, _)| wanted(*index))
        .any(|(_, page)| {
            page.links().iter().any(|link| {
                let action = link.action();
                let place = link.destination().or_else(|| {
                    action
                        .as_ref()?
                        .as_local_destination_action()?
                        .destination()
                        .ok()
                });
                place.is_some_and(|place| place.page_index().is_ok())
            })
        })
}

/// Write `pages` (one-based, in this order) of the document at `path` to a
/// new file at `to`. The document itself is not changed. Answers whether
/// the pages had links into the document, which do not work in the new file.
pub fn save_pages(
    path: &str,
    password: Option<&str>,
    pages: &[usize],
    to: &str,
) -> Result<bool, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let written = {
        let _library = crate::pdfium::library();
        let pdfium = crate::pdfium::pdfium()?;
        let source = pdfium
            .load_pdf_from_byte_vec(bytes, password)
            .map_err(|e| format!("{path}: {e}"))?;
        let mut made = pdfium
            .create_new_pdf()
            .map_err(|e| format!("a new document could not be made: {e}"))?;
        // **Every page in one call.** pdfium copies what the pages share —
        // fonts, pictures, often one resource dictionary for the whole
        // document — once per call, so a call per run of pages put a copy of
        // all of it into the new file for every run.
        let listed: Vec<String> = pages.iter().map(usize::to_string).collect();
        made.pages_mut()
            .copy_pages_from_document(&source, &listed.join(","), 0)
            .map_err(|e| format!("the pages could not be copied: {e}"))?;
        let lost = links_between(&source, Some(pages));
        let bytes = made
            .save_to_bytes()
            .map_err(|e| format!("the new document could not be saved: {e}"))?;
        (bytes, lost)
    };
    let (written, lost) = written;
    crate::atomic_write(std::path::Path::new(to), &written)?;
    Ok(lost)
}

/// Write every page of every file in `paths`, in that order, to a new file at
/// `to`. None of the files is changed. Answers how many pages it has, and
/// whether any file had links between its own pages, which do not work in
/// the new file.
pub fn combine(paths: &[String], to: &str) -> Result<(usize, bool), String> {
    let mut lost = false;
    let (written, pages) = {
        let _library = crate::pdfium::library();
        let pdfium = crate::pdfium::pdfium()?;
        let mut made = pdfium
            .create_new_pdf()
            .map_err(|e| format!("a new document could not be made: {e}"))?;
        for path in paths {
            let source = load_source(pdfium, path)?;
            lost |= links_between(&source, None);
            made.pages_mut().append(&source).map_err(|e| {
                format!(
                    "the pages of {} could not be copied: {e}",
                    crate::store::file_name(path)
                )
            })?;
        }
        let pages = made.pages().len() as usize;
        let bytes = made
            .save_to_bytes()
            .map_err(|e| format!("the new document could not be saved: {e}"))?;
        (bytes, pages)
    };
    crate::atomic_write(std::path::Path::new(to), &written)?;
    Ok((pages, lost))
}

/// One-based pages in runs of consecutive pages, each as `first..last + 1`.
fn runs(pages: &[usize]) -> Vec<std::ops::Range<usize>> {
    let mut runs: Vec<std::ops::Range<usize>> = Vec::new();
    for &page in pages {
        match runs.last_mut() {
            Some(run) if run.end == page => run.end += 1,
            _ => runs.push(page..page + 1),
        }
    }
    runs
}

/// Pages typed as "3–7, 12" (a hyphen or a dash between two numbers), for a
/// document of `count` pages. A page typed twice is kept the first time.
pub fn parse_pages(typed: &str, count: usize) -> Result<Vec<usize>, String> {
    const SAY: &str = "Type pages as numbers, like 3–7, 12.";
    let mut pages = Vec::new();
    for part in typed.split([',', ';']).map(str::trim) {
        if part.is_empty() {
            continue;
        }
        let number = |text: &str| text.trim().parse::<usize>().map_err(|_| SAY.to_string());
        let (from, to) = match part.split_once(['-', '–', '—']) {
            Some((from, to)) => (number(from)?, number(to)?),
            None => (number(part)?, number(part)?),
        };
        for page in [from, to] {
            if page == 0 || page > count {
                return Err(format!("This document has pages 1 to {count}."));
            }
        }
        let run: Vec<usize> = if from <= to {
            (from..=to).collect()
        } else {
            (to..=from).rev().collect()
        };
        for page in run {
            if !pages.contains(&page) {
                pages.push(page);
            }
        }
    }
    if pages.is_empty() {
        return Err(SAY.into());
    }
    Ok(pages)
}

/// Pages as they are written back to the reader: "3–7, 12", in the order
/// given, with runs of consecutive pages joined.
pub fn typed(pages: &[usize]) -> String {
    runs(pages)
        .iter()
        .map(|run| match run.len() {
            1 => run.start.to_string(),
            _ => format!("{}–{}", run.start, run.end - 1),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Pages named in a sentence: "page 4", "pages 3 and 4", "pages 3–7 and 12".
/// `label` is what each page is called on screen.
pub fn named(pages: &[usize], label: impl Fn(usize) -> String) -> String {
    let parts: Vec<String> = runs(pages)
        .iter()
        .flat_map(|run| match run.len() {
            1 => vec![label(run.start)],
            2 => vec![label(run.start), label(run.start + 1)],
            _ => vec![format!("{}–{}", label(run.start), label(run.end - 1))],
        })
        .collect();
    let many = pages.len() > 1;
    let list = match parts.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    format!("{} {list}", if many { "pages" } else { "page" })
}

/// The name a new file is offered under: `paper (edited).pdf` beside
/// `paper.pdf`, or whatever `how` says instead of "edited".
pub fn beside(path: &str, how: &str) -> String {
    let path = std::path::Path::new(path);
    let stem = path
        .file_stem()
        .map_or_else(|| "document".into(), |stem| stem.to_string_lossy());
    let name = format!("{stem} ({how}).pdf");
    match path.parent() {
        Some(folder) => folder.join(name).to_string_lossy().into_owned(),
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_removed_page_lands_on_the_one_after_it() {
        let moved = Edit::Remove(vec![2, 3]).moved(4, 2);
        assert_eq!(moved.0, vec![Some(0), None, None, Some(1)]);
        assert_eq!(moved.page(4), Some(2));
        assert_eq!(moved.landing(2), 2);
        // The last page gone, a reader on it lands on the last left.
        let moved = Edit::Remove(vec![3]).moved(3, 2);
        assert_eq!(moved.landing(3), 2);
    }

    #[test]
    fn moving_pages_keeps_the_rest_in_order() {
        // Pages 1 and 2 to the end: 3, 1, 2.
        assert_eq!(moved_order(3, &[1, 2], 4), vec![2, 0, 1]);
        let moved = Edit::Move(vec![1, 2], 4).moved(3, 3);
        assert_eq!(moved.0, vec![Some(1), Some(2), Some(0)]);
        // Page 4 to before page 2: 1, 4, 2, 3.
        assert_eq!(moved_order(4, &[4], 2), vec![0, 3, 1, 2]);
        // Before one of the moving pages: they stay where the first was.
        assert_eq!(moved_order(4, &[2, 3], 3), vec![0, 1, 2, 3]);
        assert!(Edit::Move(vec![2, 3], 3).moves_nothing(4));
        assert!(!Edit::Move(vec![2, 3], 1).moves_nothing(4));
    }

    #[test]
    fn inserted_pages_push_the_rest_along() {
        let moved = Edit::Insert("other.pdf".into(), 2).moved(3, 5);
        assert_eq!(moved.0, vec![Some(0), Some(3), Some(4)]);
    }

    #[test]
    fn pages_are_read_as_typed_and_written_back_the_same_way() {
        assert_eq!(parse_pages("3–5, 1", 9), Ok(vec![3, 4, 5, 1]));
        assert_eq!(parse_pages("2-2;2", 9), Ok(vec![2]));
        assert_eq!(parse_pages("5-3", 9), Ok(vec![5, 4, 3]));
        assert!(parse_pages("10", 9).is_err());
        assert!(parse_pages("a", 9).is_err());
        assert!(parse_pages(" , ", 9).is_err());
        assert_eq!(typed(&[3, 4, 5, 12]), "3–5, 12");
        let label = |page: usize| page.to_string();
        assert_eq!(named(&[4], label), "page 4");
        assert_eq!(named(&[3, 4], label), "pages 3 and 4");
        assert_eq!(named(&[3, 4, 5, 12], label), "pages 3–5 and 12");
    }

    #[test]
    fn a_new_file_is_offered_beside_the_old() {
        assert_eq!(
            beside("/papers/paper.pdf", "edited"),
            "/papers/paper (edited).pdf"
        );
    }
}
