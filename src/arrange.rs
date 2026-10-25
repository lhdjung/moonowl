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
//! pdfium cannot write the outline or `/PageLabels`, so `outline.rs` and
//! `labels.rs` write them through lopdf, after pdfium has saved.

use crate::objects::Update;
use crate::render::{Cell, PageText, Pin};
use pdfium_render::prelude::{
    PdfDocument, PdfPageAnnotation, PdfPageRenderRotation, PdfiumError, PdfiumInternalError,
    PdfiumLibraryBindingsAccessor, FPDF_FILEWRITE,
};

/// More pages than this removed at once from the document itself are asked
/// about first. In a copy, undo is enough.
pub const ASK_BEYOND: usize = 5;

/// A page edit on the open document. Every page here is one-based.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
    /// The pages from this page on numbered as this first label begins, up
    /// to the next page the document starts a new numbering on.
    Number(usize, String),
}

/// What a page edit did beyond moving pages, for the notice to say.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wrote {
    /// Pages put in from another file had links between them, which could
    /// not be written again, because lopdf does not read the document.
    pub links_lost: bool,
    /// How many headings that led to removed pages were taken out of the
    /// document, with the links and named destinations that led there. None
    /// are for a document lopdf does not read (`objects::rework`); those
    /// headings lead nowhere instead.
    pub forgot: usize,
    /// Every page kept its label. Not so for a document lopdf does not
    /// read, whose labels stay with the places.
    pub labels_kept: bool,
}

/// Number the pages of the document at `path` from `page` (one-based) on
/// as `typed` begins, and write it to `to`. pdfium has no part in it.
fn number(path: &str, to: &str, page: usize, typed: &str) -> Result<(), String> {
    let work = crate::objects::Work {
        number: Some((page, typed.to_string())),
        ..Default::default()
    };
    crate::markup::rewrite(
        path,
        to,
        Update::Other,
        |bytes| match crate::objects::rework(bytes, &work) {
            Ok((
                _,
                crate::objects::Done {
                    refused: Some(refused),
                    ..
                },
            )) => Err(refused),
            Ok((bytes, done)) => Ok(crate::markup::Saved {
                bytes,
                signed: done.signed,
            }),
            Err(refused) => Err(match refused.why {
                crate::objects::Why::TooLarge => format!(
                    "The pages of this document cannot be numbered: {}",
                    crate::objects::TOO_LARGE
                ),
                crate::objects::Why::Stopped => format!(
                    "The pages of this document could not be numbered: {}",
                    crate::objects::STOPPED
                ),
                crate::objects::Why::Unreadable => NOT_NUMBERED.into(),
            }),
        },
    )
}

/// Said when lopdf does not read a document whose pages are to be numbered.
const NOT_NUMBERED: &str = "The pages of this document cannot be numbered here: \
     its file is written in a way that cannot be changed safely.";

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

    /// The way back: where each page of the document after the edit was
    /// before it, for a document of `before` pages before it. A page that
    /// came in from another file was nowhere.
    pub fn inverse(&self, before: usize) -> Moved {
        let mut back = vec![None; before];
        for (from, to) in self.0.iter().enumerate() {
            if let Some(to) = to.filter(|&to| to < before) {
                back[to] = Some(from);
            }
        }
        Moved(back)
    }
}

impl Edit {
    /// The pages the edit is about, one-based and in order.
    pub fn pages(&self) -> Vec<usize> {
        match self {
            Edit::Remove(pages) | Edit::Turn(pages, _) | Edit::Move(pages, _) => pages.clone(),
            Edit::Number(page, _) => vec![*page],
            Edit::Insert(..) => Vec::new(),
        }
    }

    /// Where every page went, for a document of `before` pages that had
    /// `after` once the edit was written.
    pub fn moved(&self, before: usize, after: usize) -> Moved {
        let picked = |page: usize, pages: &[usize]| pages.contains(&(page + 1));
        Moved(match self {
            Edit::Turn(..) | Edit::Number(..) => (0..before).map(Some).collect(),
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
    /// which is `path` itself or the copy the reader named. After pdfium has
    /// saved, every page keeps its label (`labels::follow`), and a removal
    /// takes out what led to the removed pages (`outline::forget_gone`).
    pub fn write(&self, path: &str, to: &str) -> Result<Wrote, String> {
        let wrote = std::cell::Cell::new(Wrote::default());
        // The number of pages before the edit, for the edit that does not
        // say, and the links between pages put in from another file.
        let had = std::cell::Cell::new(0);
        let links = std::cell::RefCell::new(Vec::new());
        // `forget` is whether to look for what led to removed pages.
        let rework = |bytes, forget: bool| {
            let work = crate::objects::Work {
                slim: matches!(self, Edit::Insert(..)),
                follow: Some((self.clone(), had.get())),
                forget,
                relink: links.borrow().clone(),
                ..Default::default()
            };
            let reworked = crate::objects::rework(bytes, &work);
            wrote.set(Wrote {
                forgot: reworked.as_ref().map_or(0, |(_, done)| done.forgot),
                labels_kept: reworked.is_ok(),
                links_lost: match &reworked {
                    Ok((_, done)) => done.links_lost,
                    Err(_) => !links.borrow().is_empty(),
                },
            });
            reworked.map_or_else(|refused| refused.bytes, |(bytes, _)| bytes)
        };
        match self {
            Edit::Move(pages, before) => {
                let (pages, before) = (pages.clone(), *before);
                crate::markup::rewrite(path, to, Update::Other, |bytes| {
                    let saved = move_pages(bytes, &pages, before)?;
                    Ok(crate::markup::Saved {
                        bytes: rework(saved.bytes, false),
                        ..saved
                    })
                })?
            }
            // The other file is read before pdfium's lock is taken, so that
            // pages go on being drawn while it is read. Its pages are
            // slimmed as a new file's are.
            Edit::Insert(other, before) => {
                let bytes = read_whole(other)?;
                let change = |document: &mut PdfDocument<'static>| {
                    had.set(document.pages().len() as usize);
                    *links.borrow_mut() = insert(document, other, bytes, *before)?;
                    Ok(())
                };
                crate::markup::edit_then(path, to, Update::Other, change, |bytes| {
                    rework(bytes, false)
                })?
            }
            Edit::Remove(_) => crate::markup::edit_then(
                path,
                to,
                Update::Other,
                |document| self.apply(document),
                |bytes| rework(bytes, true),
            )?,
            Edit::Turn(..) => crate::markup::edit_then(
                path,
                to,
                Update::Other,
                |document| self.apply(document),
                crate::objects::compact,
            )?,
            Edit::Number(page, typed) => number(path, to, *page, typed)?,
        }
        Ok(wrote.get())
    }

    fn apply(&self, document: &mut PdfDocument<'static>) -> Result<(), String> {
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
            Edit::Move(..) | Edit::Insert(..) | Edit::Number(..) => {
                unreachable!("written by `Edit::write`")
            }
        }
        Ok(())
    }
}

/// Every page of the file at `path`, read as `bytes`, put in before page
/// `before` of `document`. Answers the links between those pages, to be
/// written again once pdfium has saved.
fn insert(
    document: &mut PdfDocument<'static>,
    path: &str,
    bytes: Vec<u8>,
    before: usize,
) -> Result<Vec<crate::outline::Relink>, String> {
    let count = document.pages().len() as usize;
    let source = load_source(crate::pdfium::pdfium()?, path, bytes)?;
    let last = source.pages().len() - 1;
    let at = before.clamp(1, count + 1) - 1;
    let (links, _) = links_kept(&source, |page| Some(at + page));
    document
        .pages_mut()
        .copy_page_range_from_document(&source, 0..=last, at as i32)
        .map_err(|e| format!("the pages could not be put in: {e}"))?;
    Ok(links)
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

/// Another file, read whole, for its pages.
fn read_whole(path: &str) -> Result<Vec<u8>, String> {
    let name = crate::store::file_name(path);
    if crate::watch::whole(std::path::Path::new(path)).is_none() {
        return Err(format!("{name} is not a whole PDF."));
    }
    std::fs::read(path).map_err(|e| format!("{name}: {e}"))
}

/// That file opened in pdfium. An encrypted one is refused, because its
/// password is not known here.
fn load_source(
    pdfium: &'static pdfium_render::prelude::Pdfium,
    path: &str,
    bytes: Vec<u8>,
) -> Result<PdfDocument<'static>, String> {
    let name = crate::store::file_name(path);
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
fn move_pages(
    bytes: Vec<u8>,
    pages: &[usize],
    before: usize,
) -> Result<crate::markup::Saved, String> {
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
    // A signature with nothing in its `/Contents` is a blank field.
    let signed = (0..unsafe { bindings.FPDF_GetSignatureCount(document) }).any(|index| {
        let signature = unsafe { bindings.FPDF_GetSignatureObject(document, index) };
        !signature.is_null()
            && unsafe { bindings.FPDFSignatureObj_GetContents(signature, std::ptr::null_mut(), 0) }
                > 0
    });
    let saved = if bindings.is_true(moved) {
        save_raw(bindings, document).map(|bytes| crate::markup::Saved { bytes, signed })
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

/// Every link on a page of `document` that `place` keeps to another page
/// of `document`. `place` says where a page (zero-based) goes in the file
/// being written, or `None` for a page that does not go. A link whose page
/// goes is answered, to be written again there (pdfium copies a link
/// without its destination); `true` beside them says some link leads to a
/// page that does not go, and so leads nowhere.
fn links_kept(
    document: &PdfDocument,
    place: impl Fn(usize) -> Option<usize>,
) -> (Vec<crate::outline::Relink>, bool) {
    let mut kept = Vec::new();
    let mut lost = false;
    // By index, since loading a page parses it, and a chapter of a long
    // book needs only a few of its pages.
    let pages = document.pages();
    for index in 0..pages.len() as usize {
        let Some(from) = place(index) else {
            continue;
        };
        let Ok(page) = pages.get(index as i32) else {
            continue;
        };
        // By index too, since the iterator stops at the first annotation
        // that does not load, and the links after it would go unsaid.
        let annotations = page.annotations();
        for annotation in 0..annotations.len() {
            let Ok(PdfPageAnnotation::Link(link)) = &annotations.get(annotation) else {
                continue;
            };
            let Ok(link) = link.link() else {
                continue;
            };
            let action = link.action();
            let destination = link.destination().or_else(|| {
                action
                    .as_ref()?
                    .as_local_destination_action()?
                    .destination()
                    .ok()
            });
            let Some(destination) = destination else {
                continue;
            };
            let Ok(target) = destination.page_index() else {
                continue;
            };
            match place(target as usize) {
                Some(to) => kept.push(crate::outline::Relink {
                    page: from,
                    annotation,
                    to,
                    spot: crate::pdfium::spot_of(&destination),
                }),
                None => lost = true,
            }
        }
    }
    (kept, lost)
}

/// Write `pages` (one-based, in this order) of the document at `path` to a
/// new file at `to`. The document itself is not changed. Answers whether
/// the pages had links to pages not saved with them, which do not work in
/// the new file.
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
        let place = |page: usize| pages.iter().position(|&saved| saved == page);
        let (links, lost) = links_kept(&source, |index| place(index + 1));
        let (headings, _) = crate::pdfium::read_outline(&source);
        let entries = crate::outline::entries(&headings, place);
        let bytes = made
            .save_to_bytes()
            .map_err(|e| format!("the new document could not be saved: {e}"))?;
        (bytes, lost, entries, links)
    };
    let (written, lost, entries, links) = written;
    let (written, relinked) = slim_with(written, &entries, &links);
    crate::atomic_write(std::path::Path::new(to), &written)?;
    Ok(lost || !relinked)
}

/// Write every page of every file in `paths`, in that order, to a new file at
/// `to`. None of the files is changed. Answers how many pages it has, and
/// whether any file had links between its own pages that could not be
/// written again, which do not work in the new file.
pub fn combine(paths: &[String], to: &str) -> Result<(usize, bool), String> {
    let mut entries = Vec::new();
    let mut links = Vec::new();
    let (written, pages) = {
        let _library = crate::pdfium::library();
        let pdfium = crate::pdfium::pdfium()?;
        let mut made = pdfium
            .create_new_pdf()
            .map_err(|e| format!("a new document could not be made: {e}"))?;
        for path in paths {
            let source = load_source(pdfium, path, read_whole(path)?)?;
            let before = made.pages().len() as usize;
            links.extend(links_kept(&source, |page| Some(before + page)).0);
            let (headings, _) = crate::pdfium::read_outline(&source);
            entries.extend(crate::outline::entries(&headings, |page| {
                Some(before + page - 1)
            }));
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
    let (written, relinked) = slim_with(written, &entries, &links);
    crate::atomic_write(std::path::Path::new(to), &written)?;
    Ok((pages, !relinked))
}

/// A new file slimmed, with `entries` written as its outline and `links`
/// pointed at their pages again. Answers whether there were no links, or
/// they were written.
fn slim_with(
    bytes: Vec<u8>,
    entries: &[crate::outline::Entry],
    links: &[crate::outline::Relink],
) -> (Vec<u8>, bool) {
    let work = crate::objects::Work {
        slim: true,
        outline: entries.to_vec(),
        relink: links.to_vec(),
        ..Default::default()
    };
    match crate::objects::rework(bytes, &work) {
        Ok((bytes, done)) => (bytes, !done.links_lost),
        Err(refused) => (refused.bytes, links.is_empty()),
    }
}

/// Each page of a new file made to keep only the resources its content
/// uses. The file is then compacted (`objects::rework`).
///
/// **Why it is needed.** Many producers give every page of a document one
/// shared resource dictionary that names every font and picture in it, and
/// pdfium copies a page with everything its dictionary names. Four pages of
/// a 68-page paper with fifteen figures came out at nearly the size of the
/// whole paper. pdfium cannot change a resource dictionary; lopdf can.
///
/// **Anything unsure is left as pdfium wrote it.** A page whose content
/// cannot be read keeps its dictionary whole. So does a page that draws a
/// form, a Type3 font or an annotation's appearance without resources of
/// its own, since each of those takes its names from the page. A file lopdf
/// cannot read, or that comes out no smaller, is written as it was.
///
/// The open document's own pages are slimmed only when pages come in from
/// another file: reading every page's content of a long book is slow, and
/// a document's own pages are only as wasteful as they always were.
pub(crate) fn slim_pages(document: &mut lopdf::Document) {
    let pages: Vec<lopdf::ObjectId> = document.get_pages().into_values().collect();
    let mut every_page = true;
    for &page in &pages {
        match slimmed(document, page) {
            Some(resources) => {
                if let Ok(dictionary) = document.get_object_mut(page).and_then(|o| o.as_dict_mut())
                {
                    dictionary.set("Resources", lopdf::Object::Dictionary(resources));
                }
            }
            None => every_page = false,
        }
    }
    // Every page has its own now, so what the page tree hands down is used
    // by nobody, and would keep everything it names in the file.
    if every_page {
        let nodes: Vec<lopdf::ObjectId> = (document.objects.iter())
            .filter(|(_, object)| {
                object.as_dict().is_ok_and(|d| {
                    d.get(b"Type")
                        .and_then(|t| t.as_name())
                        .is_ok_and(|t| t == b"Pages")
                })
            })
            .map(|(id, _)| *id)
            .collect();
        for node in nodes {
            if let Ok(dictionary) = document.get_object_mut(node).and_then(|o| o.as_dict_mut()) {
                dictionary.remove(b"Resources");
            }
        }
    }
}

/// The resource categories a page's content names its resources from, and
/// the operators that name them. Everything else in a resource dictionary
/// (`/ProcSet`) is kept as it is.
const NAMED: [&[u8]; 7] = [
    b"XObject",
    b"Font",
    b"ExtGState",
    b"Shading",
    b"ColorSpace",
    b"Pattern",
    b"Properties",
];

/// The resource dictionary `page` needs: its own, or the one its page tree
/// hands down, with only the names its content uses. `None` when that cannot
/// be said for certain.
fn slimmed(document: &lopdf::Document, page: lopdf::ObjectId) -> Option<lopdf::Dictionary> {
    use lopdf::Object;
    // The page's own dictionary, or the nearest one up the page tree.
    let mut node = document.get_dictionary(page).ok()?;
    let resources = loop {
        if let Ok(found) = node.get(b"Resources") {
            break document.dereference(found).ok()?.1.as_dict().ok()?;
        }
        let parent = node.get(b"Parent").and_then(Object::as_reference).ok()?;
        node = document.get_dictionary(parent).ok()?;
    };
    // Every content stream has to decode: a filter lopdf does not know hands
    // back the raw bytes, which would read as content that names nothing.
    for stream in document.get_page_contents(page) {
        let stream = document
            .get_object(stream)
            .and_then(Object::as_stream)
            .ok()?;
        if stream.dict.has(b"Filter") && stream.decompressed_content().is_err() {
            return None;
        }
    }
    // lopdf stops reading at the first token it cannot parse and returns what
    // came before it, where pdfium reads on. A marker added at the end shows
    // whether lopdf read the whole stream; if it did not, the names after the
    // stop are unknown and the page keeps its dictionary.
    let mut bytes = document.get_page_content(page);
    bytes.extend_from_slice(b"\n/MoonowlEnd MP\n");
    let mut content = lopdf::content::Content::decode(&bytes).ok()?;
    let end = content.operations.pop()?;
    if end.operator != "MP"
        || end.operands.first().and_then(|o| o.as_name().ok()) != Some(b"MoonowlEnd")
    {
        return None;
    }
    let mut used: std::collections::BTreeMap<&[u8], std::collections::BTreeSet<Vec<u8>>> =
        std::collections::BTreeMap::new();
    let mut name = |category: &'static [u8], operand: Option<&Object>| {
        if let Some(Ok(name)) = operand.map(Object::as_name) {
            used.entry(category).or_default().insert(name.to_vec());
        }
    };
    for operation in &content.operations {
        let operands = &operation.operands;
        match operation.operator.as_str() {
            "Do" => name(b"XObject", operands.first()),
            "Tf" => name(b"Font", operands.first()),
            "gs" => name(b"ExtGState", operands.first()),
            "sh" => name(b"Shading", operands.first()),
            "cs" | "CS" => name(b"ColorSpace", operands.first()),
            "scn" | "SCN" => name(b"Pattern", operands.last()),
            "BDC" | "DP" => name(b"Properties", operands.get(1)),
            // An inline image names its colour space in its own dictionary,
            // and one lopdf could not read is a reason to keep everything.
            "BI" => {
                let image = operands.first().and_then(|o| o.as_stream().ok())?;
                let space = image
                    .dict
                    .get(b"CS")
                    .or_else(|_| image.dict.get(b"ColorSpace"));
                name(b"ColorSpace", space.ok());
            }
            _ => {}
        }
    }
    let mut slim = lopdf::Dictionary::new();
    for (key, value) in resources.iter() {
        if !NAMED.contains(&key.as_slice()) {
            slim.set(key.clone(), value.clone());
            continue;
        }
        let entries = document.dereference(value).ok()?.1.as_dict().ok()?;
        let mut kept = lopdf::Dictionary::new();
        for (entry, value) in entries.iter() {
            if used
                .get(key.as_slice())
                .is_some_and(|names| names.contains(entry))
            {
                kept.set(entry.clone(), value.clone());
            }
        }
        if !kept.is_empty() {
            slim.set(key.clone(), Object::Dictionary(kept));
        }
    }
    // A form drawn on the page that has no resources of its own takes the
    // page's, so the page's must stay whole.
    if let Ok(forms) = slim.get(b"XObject").and_then(Object::as_dict) {
        for (_, form) in forms.iter() {
            let form = document.dereference(form).ok()?.1.as_stream().ok()?;
            let is_form = form
                .dict
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_ok_and(|s| s == b"Form");
            if is_form && !form.dict.has(b"Resources") {
                return None;
            }
        }
    }
    // So does a Type3 font with no resources of its own, for its glyphs.
    if let Ok(fonts) = slim.get(b"Font").and_then(Object::as_dict) {
        for (_, font) in fonts.iter() {
            let font = document.dereference(font).ok()?.1.as_dict().ok()?;
            let type3 =
                (font.get(b"Subtype").and_then(Object::as_name)).is_ok_and(|s| s == b"Type3");
            if type3 && !font.has(b"Resources") {
                return None;
            }
        }
    }
    // pdfium also draws an annotation's appearance that has no resources of
    // its own with the page's.
    let annotations = document.get_dictionary(page).ok()?.get(b"Annots");
    if let Ok(annotations) = annotations {
        for annotation in document.dereference(annotations).ok()?.1.as_array().ok()? {
            let appearances = (document.dereference(annotation))
                .and_then(|(_, annotation)| annotation.as_dict())
                .and_then(|annotation| annotation.get(b"AP"));
            let Ok(appearances) = appearances else {
                continue;
            };
            for (_, appearance) in document
                .dereference(appearances)
                .ok()?
                .1
                .as_dict()
                .ok()?
                .iter()
            {
                // One appearance, or one for each state of the annotation.
                let streams: Vec<&Object> = match document.dereference(appearance).ok()?.1 {
                    Object::Dictionary(states) => states.iter().map(|(_, stream)| stream).collect(),
                    stream => vec![stream],
                };
                for stream in streams {
                    let stream = document.dereference(stream).ok()?.1.as_stream().ok()?;
                    if !stream.dict.has(b"Resources") {
                        return None;
                    }
                }
            }
        }
    }
    Some(slim)
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

/// One chapter of a document, to be saved as a file of its own.
#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    pub title: String,
    /// Its pages, one-based, up to the page before the next chapter's.
    pub pages: Vec<usize>,
    /// The page where the next chapter begins, and where on it, when its
    /// heading points to a place on the page. The page holds the end of
    /// this chapter too if [`ends_there`] says so.
    pub next: Option<(usize, Pin)>,
}

/// Whether `page` (one-based) holds the end of a chapter as well as the
/// start of the next, whose heading points to `spot` on it. It does when
/// there is text above the heading. Lines that the pages around it repeat
/// at the same height, such as running heads, do not count, and neither
/// do lines with only a page number on them.
///
/// A page whose place for the heading is not known yet counts as holding
/// both, since a page saved twice loses nothing and a page left out does.
/// A page with no text, such as a scan, holds both when its heading is
/// further down than [`CHAPTER_AT_TOP`].
pub fn ends_there(document: &dyn crate::render::PageSource, page: usize, spot: Pin) -> bool {
    let text = document.text_of(page - 1);
    let Some(fraction) = document.place(page, spot) else {
        return true;
    };
    if lines(&text).is_empty() {
        return fraction > CHAPTER_AT_TOP;
    }
    let down = fraction * document.size_of(page - 1).height;
    let around: Vec<PageText> = (page.saturating_sub(2)..=page + 2)
        .filter(|&near| near != page && near >= 1 && near <= document.pages())
        .map(|near| document.text_of(near - 1))
        .collect();
    text_above(&text, &around, down)
}

/// How far down its page, as a fraction of the page's height, a chapter's
/// heading may point on a page with no text and still begin the page.
const CHAPTER_AT_TOP: f64 = 0.15;

/// Whether `text` has a line above `down` points from the top of the page
/// that is not repeated at the same height on one of the pages `around` it
/// and that has more than a page number on it.
fn text_above(text: &PageText, around: &[PageText], down: f64) -> bool {
    let near: Vec<Line> = around.iter().flat_map(lines).collect();
    // A line repeats on another page when the words are the same and the
    // two lines are within this many points of the same height.
    const SAME_HEIGHT: f64 = 2.0;
    // The heading's own line ends below the place it points to.
    (lines(text).into_iter())
        .filter(|line| line.bottom <= down + 1.0 && !line.words.is_empty())
        .any(|line| {
            !near.iter().any(|other| {
                other.words == line.words && (other.middle - line.middle).abs() <= SAME_HEIGHT
            })
        })
}

/// One line of a page's text.
struct Line {
    /// The height of its middle, in points from the top of the page.
    middle: f64,
    /// The height of its lowest character's bottom.
    bottom: f64,
    /// Its characters from left to right, without spaces and digits, so
    /// that a running head with a page number in it reads the same on
    /// every page.
    words: String,
}

/// The lines of a page's text, from the top of the page down.
fn lines(text: &PageText) -> Vec<Line> {
    let middle = |cell: &Cell| f64::from(cell.top + cell.height / 2.0);
    let mut glyphs: Vec<(char, Cell)> = (text.chars.iter().copied())
        .zip(text.boxes.iter().copied())
        .filter(|(_, cell)| cell.height > 0.0)
        .collect();
    glyphs.sort_by(|(_, one), (_, other)| middle(one).total_cmp(&middle(other)));
    // A character joins the line above when its middle is within half the
    // height of that line's first character.
    let mut grouped: Vec<Vec<(char, Cell)>> = Vec::new();
    for glyph in glyphs {
        match grouped.last_mut() {
            Some(line)
                if middle(&glyph.1) - middle(&line[0].1) <= f64::from(line[0].1.height) / 2.0 =>
            {
                line.push(glyph)
            }
            _ => grouped.push(vec![glyph]),
        }
    }
    (grouped.into_iter())
        .map(|mut line| {
            line.sort_by(|(_, one), (_, other)| one.left.total_cmp(&other.left));
            Line {
                middle: middle(&line[0].1),
                bottom: (line.iter())
                    .map(|(_, cell)| f64::from(cell.top + cell.height))
                    .fold(f64::MIN, f64::max),
                words: (line.iter())
                    .map(|(character, _)| *character)
                    .filter(|character| !character.is_whitespace() && !character.is_ascii_digit())
                    .collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page's text, from lines of words at a height each.
    fn page_of(lines: &[(&str, f32)]) -> PageText {
        let mut text = PageText::default();
        for (words, top) in lines {
            for (at, character) in words.chars().enumerate() {
                text.chars.push(character);
                text.boxes.push(Cell {
                    left: 72.0 + 6.0 * at as f32,
                    top: *top,
                    width: 6.0,
                    height: 10.0,
                });
            }
        }
        text
    }

    #[test]
    fn only_text_the_pages_around_do_not_repeat_ends_a_chapter_above_a_heading() {
        let head = |number: &str| format!("{number}   A Paper by Some Authors");
        let around = [
            page_of(&[(&head("11"), 40.0), ("Body text of page 11.", 100.0)]),
            page_of(&[(&head("13"), 40.0), ("Body text of page 13.", 100.0)]),
        ];
        // Under a running head and a page number, a heading far down the
        // page begins it.
        let opening = page_of(&[(&head("12"), 40.0), ("12", 20.0), ("Chapter", 300.0)]);
        assert!(!text_above(&opening, &around, 295.0));
        // The end of the chapter before, above the heading, is shared.
        let shared = page_of(&[(&head("12"), 40.0), ("The end of it.", 200.0)]);
        assert!(text_above(&shared, &around, 295.0));
        // Text below the heading does not count.
        assert!(!text_above(&shared, &around, 150.0));
    }

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
        // Undone, the pages put in go nowhere, and the way back again is
        // the edit itself.
        let back = moved.inverse(5);
        assert_eq!(back.0, vec![Some(0), None, None, Some(1), Some(2)]);
        assert_eq!(back.inverse(3), moved);
    }

    #[test]
    fn a_removal_undone_puts_each_page_back() {
        let moved = Edit::Remove(vec![2, 4]).moved(5, 3);
        assert_eq!(moved.inverse(3).0, vec![Some(0), Some(2), Some(4)]);
        assert_eq!(moved.inverse(3).inverse(5), moved);
    }

    /// A page that draws one font, `font`.
    fn page_with(font: lopdf::Dictionary) -> (lopdf::Document, lopdf::ObjectId) {
        use lopdf::{dictionary, Object, Stream};
        let mut document = lopdf::Document::with_version("1.7");
        let font = document.add_object(font);
        let content = b"BT /F1 12 Tf (a) Tj ET".to_vec();
        let content = document.add_object(Stream::new(dictionary! {}, content));
        let tree = document.new_object_id();
        let page = dictionary! {
            "Type" => "Page",
            "Parent" => tree,
            "Contents" => content,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        };
        let page = document.add_object(page);
        let kids = vec![Object::from(page)];
        let tree_dictionary = dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 1 };
        document
            .objects
            .insert(tree, Object::Dictionary(tree_dictionary));
        (document, page)
    }

    #[test]
    fn what_takes_its_names_from_the_page_keeps_the_page_whole() {
        use lopdf::{dictionary, Stream};
        let plain =
            || dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" };
        let (document, page) = page_with(plain());
        assert!(slimmed(&document, page).is_some());

        let type3 = dictionary! { "Type" => "Font", "Subtype" => "Type3" };
        let (document, page) = page_with(type3.clone());
        assert!(
            slimmed(&document, page).is_none(),
            "a Type3 font with no resources"
        );
        let mut own = type3;
        own.set("Resources", dictionary! {});
        let (document, page) = page_with(own);
        assert!(
            slimmed(&document, page).is_some(),
            "a Type3 font with its own"
        );

        let (mut document, page) = page_with(plain());
        let bare = document.add_object(Stream::new(dictionary! {}, b"0 0 m".to_vec()));
        let annotation =
            dictionary! { "Subtype" => "Highlight", "AP" => dictionary! { "N" => bare } };
        let annotation = document.add_object(annotation);
        let dictionary = document
            .get_object_mut(page)
            .and_then(|o| o.as_dict_mut())
            .unwrap();
        dictionary.set("Annots", vec![lopdf::Object::from(annotation)]);
        assert!(
            slimmed(&document, page).is_none(),
            "an appearance with no resources"
        );
    }

    #[test]
    fn content_lopdf_cannot_read_to_the_end_keeps_the_page_whole() {
        use lopdf::{dictionary, Object};
        let plain =
            || dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" };
        let (mut document, page) = page_with(plain());
        let contents = document.get_page_contents(page)[0];
        let stream = document
            .get_object_mut(contents)
            .and_then(Object::as_stream_mut)
            .unwrap();
        stream.set_content(b"BT /F1 12 Tf (a) Tj ET ] /F2 12 Tf".to_vec());
        assert!(slimmed(&document, page).is_none());
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
