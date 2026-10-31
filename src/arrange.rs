//! Page edits: removing pages, turning them in the file, moving them, putting
//! another file's pages in, and writing pages of several files into a new
//! file. `plans/page-edits.md` is the plan these follow.
//!
//! An edit of the open document goes through `markup::rewrite`, as a
//! highlight does, and is written into the document itself. Every edit says
//! where each old page went ([`Moved`]), because the reader's place, their
//! bookmarks, their way back and the pages' spaces all have to follow the
//! pages.
//!
//! pdfium cannot write the outline or `/PageLabels`, so `outline.rs` and
//! `labels.rs` write them through lopdf, after pdfium has saved.

use crate::objects::Update;
use crate::render::{Cell, PageText, Pin};
use pdfium_render::prelude::{
    PdfDocument, PdfPageAnnotation, PdfPageRenderRotation, PdfiumError, PdfiumInternalError,
    PdfiumLibraryBindingsAccessor, FPDF_FILEWRITE, FS_RECTF,
};

/// More pages than this removed at once from the document are asked about
/// first.
pub const ASK_BEYOND: usize = 5;

/// A page edit on the open document. Every page here is one-based.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Edit {
    /// The pages from this page on numbered as this first label begins, up
    /// to the next page the document starts a new numbering on. The number
    /// is what the first page is shown as when the document gives no labels
    /// and Moonowl reads the numbers printed on its pages, so that the pages
    /// before keep them.
    Number(usize, String, Option<usize>),
    /// The document made into these pages, in this order: what the page
    /// organiser saves. A page of the document that is not here is removed.
    Arrange(Vec<Sheet>),
}

/// A page of an arrangement.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Sheet {
    /// The file the page comes from, or `None` for the document itself.
    pub from: Option<String>,
    /// Which page of that file, one-based.
    pub page: usize,
    /// How many clockwise quarter turns are added to the turn it has.
    pub turn: i32,
}

/// What a page edit did beyond moving pages, for the notice to say.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wrote {
    /// Pages put in from another file had links between them, which could
    /// not be written again, because lopdf does not read the document.
    pub links_lost: bool,
    /// A link on a page put in from another file led to a page of that
    /// file that did not come along, and leads nowhere now.
    pub links_left_behind: bool,
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
/// as `typed` begins, and write it back. pdfium has no part in it.
fn number(path: &str, page: usize, typed: &str, printed: Option<usize>) -> Result<(), String> {
    let work = crate::objects::Work {
        number: Some((page, typed.to_string(), printed)),
        ..Default::default()
    };
    crate::markup::rewrite(path, Update::Other, |bytes| {
        match crate::objects::rework(bytes, &work) {
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
        }
    })
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
    /// before it. `after` is the number of pages after the edit. A page
    /// that came in from another file was nowhere.
    pub fn inverse(&self, after: usize) -> Moved {
        let mut back = vec![None; after];
        for (from, to) in self.0.iter().enumerate() {
            if let Some(to) = to.filter(|&to| to < after) {
                back[to] = Some(from);
            }
        }
        Moved(back)
    }
}

impl Edit {
    /// Where every page went, for a document of `before` pages.
    pub fn moved(&self, before: usize) -> Moved {
        Moved(match self {
            Edit::Number(..) => (0..before).map(Some).collect(),
            Edit::Arrange(sheets) => {
                let mut map = vec![None; before];
                for (at, sheet) in sheets.iter().enumerate() {
                    if sheet.from.is_none() {
                        if let Some(slot) = map.get_mut(sheet.page.wrapping_sub(1)) {
                            slot.get_or_insert(at);
                        }
                    }
                }
                map
            }
        })
    }

    /// Whether the edit would leave every page where it is, in a document of
    /// `count` pages: the same pages in the same order, none turned.
    pub fn moves_nothing(&self, count: usize) -> bool {
        let Edit::Arrange(sheets) = self else {
            return false;
        };
        sheets.len() == count
            && (sheets.iter().enumerate()).all(|(at, sheet)| {
                sheet.from.is_none() && sheet.page == at + 1 && sheet.turn % 4 == 0
            })
    }

    /// The pages' spaces as they are after the edit, from what was known of
    /// them before. A page that came in from another file is not known yet.
    pub fn spaces(
        &self,
        old: &[Option<crate::markup::Space>],
        after: usize,
    ) -> Vec<Option<crate::markup::Space>> {
        let moved = self.moved(old.len());
        let mut new = vec![None; after];
        for (index, space) in old.iter().enumerate() {
            let Some(to) = moved.0[index].filter(|&to| to < after) else {
                continue;
            };
            new[to] = match (self, space) {
                (Edit::Arrange(sheets), Some(space)) => {
                    Some(space.turned_by(sheets.get(to).map_or(0, |sheet| sheet.turn)))
                }
                _ => *space,
            };
        }
        new
    }

    /// Write the edit: read the document at `path`, change it, and write it
    /// back. After pdfium has saved, every page keeps its label
    /// (`labels::follow`), and a removal takes out what led to the removed
    /// pages (`outline::forget_gone`).
    pub fn write(&self, path: &str) -> Result<Wrote, String> {
        let wrote = std::cell::Cell::new(Wrote::default());
        // The number of pages before the edit, and the links between pages
        // put in from another file.
        let had = std::cell::Cell::new(0);
        let links = std::cell::RefCell::new(Vec::new());
        let behind = std::cell::Cell::new(false);
        // `forget` is whether to look for what led to removed pages.
        // The pages are put in order by lopdf, or by pdfium where lopdf
        // cannot read the file.
        let rework = |bytes, forget: bool, order: Vec<usize>| {
            let moves = order.iter().enumerate().any(|(at, &page)| at != page);
            let work = crate::objects::Work {
                order: if moves { order.clone() } else { Vec::new() },
                // Only the pages that came in: the document's own pages are
                // as they always were, and a wrong decision about one would
                // be written into the reader's file.
                slim: match self {
                    Edit::Arrange(sheets) => (sheets.iter().enumerate())
                        .filter(|(_, sheet)| sheet.from.is_some())
                        .map(|(at, _)| at)
                        .collect(),
                    Edit::Number(..) => Vec::new(),
                },
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
                links_left_behind: behind.get(),
            });
            // A document whose pages lopdf could not put in order comes
            // back as it was (`objects::rework`), and pdfium puts them in
            // order instead.
            match reworked {
                Ok((bytes, _)) => Ok(bytes),
                Err(refused) => reorder(refused.bytes, &order),
            }
        };
        match self {
            Edit::Number(page, typed, printed) => number(path, *page, typed, *printed)?,
            // The other files are read before pdfium's lock is taken, so that
            // pages go on being drawn while they are read. pdfium-render
            // keeps a document's handle to itself, so the pages are put in
            // order by a second pass over the bytes.
            Edit::Arrange(sheets) => {
                let others = read_others(sheets)?;
                crate::markup::rewrite(path, Update::Other, |bytes| {
                    let order = std::cell::RefCell::new(Vec::new());
                    let mut saved = crate::markup::saved_by_pdfium(path, bytes, |document| {
                        had.set(document.pages().len() as usize);
                        let (placed, relinks, left) = arrange_in(document, sheets, others)?;
                        *order.borrow_mut() = placed;
                        *links.borrow_mut() = relinks;
                        behind.set(left);
                        Ok(())
                    })?;
                    let removed = (1..=had.get()).any(|page| {
                        !(sheets.iter()).any(|sheet| sheet.from.is_none() && sheet.page == page)
                    });
                    let bytes = std::mem::take(&mut saved.bytes);
                    Ok(crate::markup::Saved {
                        bytes: rework(bytes, removed, order.take())?,
                        ..saved
                    })
                })?
            }
        }
        Ok(wrote.get())
    }
}

/// Page `page` (one-based) of `document` turned by this many clockwise
/// quarter turns, added to the turn it has.
fn turn(document: &PdfDocument<'static>, page: usize, quarters: i32) -> Result<(), String> {
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
    Ok(())
}

/// Every other file the sheets take pages from, read whole, in the order
/// they are first used.
fn read_others(sheets: &[Sheet]) -> Result<Vec<(String, Source)>, String> {
    let mut paths: Vec<&String> = Vec::new();
    for path in sheets.iter().filter_map(|sheet| sheet.from.as_ref()) {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    // One wait for all of them, not one per file.
    let whole = crate::watch::all_whole(
        &paths
            .iter()
            .map(|path| std::path::Path::new(path.as_str()))
            .collect::<Vec<_>>(),
    );
    (paths.into_iter().zip(whole))
        .map(|(path, whole)| Ok((path.clone(), read_whole_as(path, whole)?)))
        .collect()
}

/// The pages of every file in `others` that `sheets` use, copied in at the
/// end of `document`, one import call per file; every sheet turned as it
/// says; and every page of the document no sheet uses taken out. Answers
/// where each sheet's page is now, for [`reorder`], the links between
/// copied pages, to be written again once the pages are in order, and
/// whether a copied link led to a page that did not come along.
fn arrange_in(
    document: &mut PdfDocument<'static>,
    sheets: &[Sheet],
    others: Vec<(String, Source)>,
) -> Result<(Vec<usize>, Vec<crate::outline::Relink>, bool), String> {
    if sheets.is_empty() {
        return Err("A document needs at least one page.".into());
    }
    let count = document.pages().len() as usize;
    let mut used = std::collections::BTreeSet::new();
    let mut at = vec![0; sheets.len()];
    for (index, sheet) in sheets.iter().enumerate() {
        if sheet.from.is_none() {
            if sheet.page == 0 || sheet.page > count {
                return Err(format!("There is no page {} in this document.", sheet.page));
            }
            if !used.insert(sheet.page - 1) {
                return Err("A page of the document can be in it only once.".into());
            }
            at[index] = sheet.page - 1;
        }
    }
    let pdfium = crate::pdfium::pdfium()?;
    let (mut links, mut behind) = (Vec::new(), false);
    for (path, source) in others {
        let source = load_source(pdfium, &path, source, None)?;
        let mine: Vec<usize> = (0..sheets.len())
            .filter(|&index| sheets[index].from.as_deref() == Some(path.as_str()))
            .collect();
        let start = document.pages().len() as usize;
        let listed: Vec<String> = mine.iter().map(|&i| sheets[i].page.to_string()).collect();
        document
            .pages_mut()
            .copy_pages_from_document(&source, &listed.join(","), start as i32)
            .map_err(|e| {
                format!(
                    "the pages of {} could not be put in: {e}",
                    crate::store::file_name(&path)
                )
            })?;
        for (copied, &index) in mine.iter().enumerate() {
            at[index] = start + copied;
            // pdfium copies a page's turn and CropBox only where the other
            // file gives one, and a page without them takes this document's
            // from the branch of the page tree it is put under.
            let turn = page_of(&source, sheets[index].page)?
                .rotation()
                .unwrap_or(PdfPageRenderRotation::None);
            let mut page = page_of(document, start + copied + 1)?;
            page.set_rotation(turn);
            if page.boundaries().crop().is_err() {
                if let Ok(media) = page.boundaries().media() {
                    let _ = page.boundaries_mut().set_crop(media.bounds);
                }
            }
        }
        let copies = |index: usize| {
            let page = index + 1;
            (mine.iter().copied())
                .filter(|&i| sheets[i].page == page)
                .collect()
        };
        let (kept, unkept) = links_kept(&source, copies);
        links.extend(kept);
        behind |= unkept;
    }
    for (index, sheet) in sheets.iter().enumerate() {
        if sheet.turn.rem_euclid(4) != 0 {
            turn(document, at[index] + 1, sheet.turn)?;
        }
    }
    // From the end, so that taking one out moves none still to come.
    let gone: Vec<usize> = (0..count).filter(|index| !used.contains(index)).collect();
    for &index in gone.iter().rev() {
        page_of(document, index + 1)?
            .delete()
            .map_err(|e| format!("page {} could not be removed: {e}", index + 1))?;
    }
    let order = (at.iter())
        .map(|&index| index - gone.iter().filter(|&&g| g < index).count())
        .collect();
    Ok((order, links, behind))
}

/// The document in `bytes` with its pages put in `order`: the page now at
/// `order[0]` first, and so on, for a file lopdf cannot put in order (see
/// [`put_in_order`]). `FPDF_MovePages` keeps each page the same object, so
/// headings and links that point at a page still find it, but a page moved
/// under another node of the page tree takes what that node gives.
fn reorder(bytes: Vec<u8>, order: &[usize]) -> Result<Vec<u8>, String> {
    if order.iter().enumerate().all(|(at, &page)| at == page) {
        return Ok(bytes);
    }
    let _library = crate::pdfium::library();
    let bindings = crate::pdfium::pdfium()?.bindings();
    let document = unsafe { bindings.FPDF_LoadMemDocument64(&bytes, None) };
    if document.is_null() {
        return Err("The document could not be read.".into());
    }
    // pdfium moves a page without what it takes from its branch of the page
    // tree, so each page is first given its own turn and boxes, as
    // `put_in_order` does. pdfium answers a page's turn and its bounding box
    // with what it inherits, but its MediaBox and CropBox only as the page
    // itself says them.
    // ponytail: inherited resources cannot be given to a page through
    // pdfium, so a page whose fonts are named by its branch loses them here.
    for index in 0..order.len() {
        let page = unsafe { bindings.FPDF_LoadPage(document, index as i32) };
        if page.is_null() {
            continue;
        }
        unsafe {
            let turn = bindings.FPDFPage_GetRotation(page);
            if turn >= 0 {
                bindings.FPDFPage_SetRotation(page, turn);
            }
            let mut shown = FS_RECTF {
                left: 0.0,
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
            };
            if bindings.is_true(bindings.FPDF_GetPageBoundingBox(page, &mut shown)) {
                let (mut left, mut bottom, mut right, mut top) = (0.0, 0.0, 0.0, 0.0);
                let (l, b, r, t) = (shown.left, shown.bottom, shown.right, shown.top);
                if !bindings.is_true(bindings.FPDFPage_GetMediaBox(
                    page,
                    &mut left,
                    &mut bottom,
                    &mut right,
                    &mut top,
                )) {
                    bindings.FPDFPage_SetMediaBox(page, l, b, r, t);
                }
                if !bindings.is_true(bindings.FPDFPage_GetCropBox(
                    page,
                    &mut left,
                    &mut bottom,
                    &mut right,
                    &mut top,
                )) {
                    bindings.FPDFPage_SetCropBox(page, l, b, r, t);
                }
            }
            bindings.FPDF_ClosePage(page);
        }
    }
    // Where each page is as they are moved, by where it was before.
    let mut now: Vec<usize> = (0..order.len()).collect();
    let mut moved = true;
    for (to, &page) in order.iter().enumerate() {
        let Some(from) = now.iter().position(|&was| was == page) else {
            moved = false;
            break;
        };
        if from == to {
            continue;
        }
        let index = from as i32;
        if !bindings.is_true(unsafe { bindings.FPDF_MovePages(document, &index, 1, to as i32) }) {
            moved = false;
            break;
        }
        now.remove(from);
        now.insert(to, page);
    }
    let saved = match moved {
        true => save_raw(bindings, document),
        false => Err("The pages could not be put in order.".into()),
    };
    unsafe { bindings.FPDF_CloseDocument(document) };
    saved
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

/// Another file whose pages are copied: a PDF, or a JPEG that becomes one
/// page. A PNG arrives as a PDF, since it is made into one in another
/// process.
enum Source {
    Pdf(Vec<u8>),
    /// A JPEG as it is in its file, its width and height in pixels as
    /// stored, and how it is turned to be shown upright.
    Jpeg(Vec<u8>, u32, u32, image::metadata::Orientation),
}

/// Another file, read whole, for its pages. A PNG is made into a PDF here,
/// and not under pdfium's lock, because a large picture takes a while.
fn read_whole(path: &str) -> Result<Source, String> {
    let whole = crate::watch::whole(std::path::Path::new(path)).is_some();
    read_whole_as(path, whole)
}

/// [`read_whole`], told whether the file is a whole PDF.
fn read_whole_as(path: &str, whole: bool) -> Result<Source, String> {
    let name = crate::store::file_name(path);
    if whole {
        return std::fs::read(path)
            .map(Source::Pdf)
            .map_err(|e| format!("{name}: {e}"));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{name}: {e}"))?;
    let unread = |e: image::ImageError| format!("{name} could not be read as a picture: {e}");
    match image::guess_format(&bytes) {
        Ok(image::ImageFormat::Jpeg) => {
            use image::ImageDecoder;
            // This reads only the JPEG's headers, not its pixels.
            let mut decoder = image::codecs::jpeg::JpegDecoder::new(std::io::Cursor::new(&bytes))
                .map_err(unread)?;
            let orientation = decoder
                .orientation()
                .unwrap_or(image::metadata::Orientation::NoTransforms);
            let (width, height) = decoder.dimensions();
            drop(decoder);
            Ok(Source::Jpeg(bytes, width, height, orientation))
        }
        Ok(image::ImageFormat::Png) => {
            let (width, height) = image::ImageReader::with_format(
                std::io::Cursor::new(&bytes),
                image::ImageFormat::Png,
            )
            .into_dimensions()
            .map_err(unread)?;
            crate::objects::picture(&bytes, u64::from(width) * u64::from(height))
                .map(Source::Pdf)
                .map_err(|why| match why {
                    crate::objects::Why::TooLarge => {
                        format!("{name} is too large a picture to be made into a page here.")
                    }
                    crate::objects::Why::Stopped => format!(
                        "{name} could not be made into a page: it took too long, or the picture is too large."
                    ),
                    crate::objects::Why::Unreadable => {
                        format!("{name} could not be made into a page.")
                    }
                })
        }
        _ => Err(format!("{name} is not a whole PDF or a picture.")),
    }
}

/// A picture as a PDF of one page, for the organiser to draw and copy. A
/// PDF is answered as it is.
pub fn as_pdf(path: &str) -> Result<Vec<u8>, String> {
    match read_whole(path)? {
        Source::Pdf(bytes) => Ok(bytes),
        jpeg => {
            let _library = crate::pdfium::library();
            let document = load_source(crate::pdfium::pdfium()?, path, jpeg, None)?;
            document
                .save_to_bytes()
                .map_err(|e| format!("the picture could not be made into a page: {e}"))
        }
    }
}

/// A PDF of one empty page, `width` by `height` points, for the
/// organiser's "Add a blank page after".
pub fn blank_pdf(width: f64, height: f64) -> Result<Vec<u8>, String> {
    use lopdf::{dictionary, Document, Object};
    let mut document = Document::with_version("1.7");
    let pages = document.new_object_id();
    let page = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), (width as f32).into(), (height as f32).into()],
        // The specification asks every page for resources, even none.
        "Resources" => dictionary! {},
    });
    document.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page.into()],
            "Count" => 1,
        }),
    );
    let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    document.trailer.set("Root", catalog);
    let mut out = Vec::new();
    (document.save_to(&mut out)).map_err(|e| format!("a blank page could not be made: {e}"))?;
    Ok(out)
}

/// A PNG as a PDF of one page, the size [`picture_page`] gives it. The
/// picture is drawn onto white, since white paper is what a page has under
/// it, and upright, as its EXIF says, if it has any. This decodes every
/// pixel, so [`crate::objects::picture`] runs it in another process.
pub fn picture_pdf(png: &[u8]) -> Option<Vec<u8>> {
    use image::ImageDecoder;
    use lopdf::{dictionary, Document, Object, Stream};
    let reader =
        image::ImageReader::with_format(std::io::Cursor::new(png), image::ImageFormat::Png);
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture = image::DynamicImage::from_decoder(decoder).ok()?;
    picture.apply_orientation(orientation);
    let picture = picture.into_rgba8();
    let (width, height) = picture.dimensions();
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    for pixel in picture.pixels() {
        let [red, green, blue, alpha] = pixel.0;
        let over = |channel: u8| {
            let (channel, alpha) = (u32::from(channel), u32::from(alpha));
            ((channel * alpha + 255 * (255 - alpha)) / 255) as u8
        };
        rgb.extend([over(red), over(green), over(blue)]);
    }
    drop(picture);
    let (across, down) = picture_page(width, height);
    let mut document = Document::with_version("1.7");
    let image = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => i64::from(width),
            "Height" => i64::from(height),
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
        },
        rgb,
    ));
    let content = format!("q {across} 0 0 {down} 0 0 cm /Picture Do Q");
    let content = document.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let pages = document.new_object_id();
    let page = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), across.into(), down.into()],
        "Contents" => content,
        "Resources" => dictionary! { "XObject" => dictionary! { "Picture" => image } },
    });
    document.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page.into()],
            "Count" => 1,
        }),
    );
    let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    document.trailer.set("Root", catalog);
    document.compress();
    let mut out = Vec::new();
    document.save_to(&mut out).ok()?;
    Some(out)
}

/// The size of a picture's page, in points: the picture's own shape, as
/// large as fits on an A4 page, which is turned for a picture wider than it
/// is tall.
pub fn picture_page(width: u32, height: u32) -> (f32, f32) {
    let (width, height) = (width.max(1) as f32, height.max(1) as f32);
    let (across, down) = if width > height {
        (841.89, 595.28)
    } else {
        (595.28, 841.89)
    };
    let scale = (across / width).min(down / height);
    (width * scale, height * scale)
}

/// Where the corners of a picture's unit square go so that a picture stored
/// turned is shown upright, as `[a, b, c, d, e, f]` of a PDF matrix. The
/// square is then stretched to the page.
fn upright(orientation: image::metadata::Orientation) -> [f32; 6] {
    use image::metadata::Orientation::*;
    match orientation {
        NoTransforms => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        Rotate90 => [0.0, -1.0, 1.0, 0.0, 0.0, 1.0],
        Rotate180 => [-1.0, 0.0, 0.0, -1.0, 1.0, 1.0],
        Rotate270 => [0.0, 1.0, -1.0, 0.0, 1.0, 0.0],
        FlipHorizontal => [-1.0, 0.0, 0.0, 1.0, 1.0, 0.0],
        FlipVertical => [1.0, 0.0, 0.0, -1.0, 0.0, 1.0],
        Rotate90FlipH => [0.0, -1.0, -1.0, 0.0, 1.0, 1.0],
        Rotate270FlipH => [0.0, 1.0, 1.0, 0.0, 0.0, 0.0],
    }
}

/// That file opened in pdfium, or a JPEG made into a document of one page.
/// The JPEG goes into the file as it is, and a JPEG whose EXIF says it is
/// turned is turned by the way it is drawn, since pdfium does not read
/// EXIF. An encrypted PDF is refused, because its password is not known
/// here.
fn load_source(
    pdfium: &'static pdfium_render::prelude::Pdfium,
    path: &str,
    source: Source,
    password: Option<&str>,
) -> Result<PdfDocument<'static>, String> {
    let name = crate::store::file_name(path);
    let (bytes, width, height, orientation) = match source {
        Source::Pdf(bytes) => {
            return pdfium
                .load_pdf_from_byte_vec(bytes, password)
                .map_err(|e| match e {
                    PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
                        format!("{name} is encrypted, so its pages cannot be copied.")
                    }
                    e => format!("{name} could not be read: {e}"),
                });
        }
        Source::Jpeg(bytes, width, height, orientation) => (bytes, width, height, orientation),
    };
    let made = || -> Result<PdfDocument<'static>, PdfiumError> {
        use pdfium_render::prelude::*;
        let mut document = pdfium.create_new_pdf()?;
        use image::metadata::Orientation::{Rotate270, Rotate270FlipH, Rotate90, Rotate90FlipH};
        let (shown_width, shown_height) = match orientation {
            Rotate90 | Rotate270 | Rotate90FlipH | Rotate270FlipH => (height, width),
            _ => (width, height),
        };
        let (across, down) = picture_page(shown_width, shown_height);
        let mut object =
            PdfPageImageObject::new_from_jpeg_reader(&document, std::io::Cursor::new(bytes))?;
        let [a, b, c, d, e, f] = upright(orientation);
        object.transform(
            a * across,
            b * down,
            c * across,
            d * down,
            e * across,
            f * down,
        )?;
        let size = PdfPagePaperSize::Custom(PdfPoints::new(across), PdfPoints::new(down));
        let mut page = document.pages_mut().create_page_at_end(size)?;
        page.objects_mut().add_image_object(object)?;
        drop(page);
        Ok(document)
    };
    made().map_err(|e| format!("{name} could not be made into a page: {e}"))
}

/// `FPDF_SaveAsCopy` into memory, with no flags: what `save_to_bytes` does.
pub(crate) fn save_raw(
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

/// Every link on a page of `document` that `places` keeps to another page
/// of `document`. `places` says where each copy of a page (zero-based)
/// goes in the file being written, which is nowhere for a page that does
/// not go. A link is answered for every copy of its page, to be written
/// again there (pdfium copies a link without its destination), and leads
/// to the first copy of its target; `true` beside them says some link
/// leads to a page that does not go, and so leads nowhere.
fn links_kept(
    document: &PdfDocument,
    places: impl Fn(usize) -> Vec<usize>,
) -> (Vec<crate::outline::Relink>, bool) {
    let mut kept = Vec::new();
    let mut lost = false;
    // By index, since loading a page parses it, and a chapter of a long
    // book needs only a few of its pages.
    let pages = document.pages();
    for index in 0..pages.len() as usize {
        let copies = places(index);
        if copies.is_empty() {
            continue;
        }
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
            let Some(&to) = places(target as usize).first() else {
                lost = true;
                continue;
            };
            let spot = crate::pdfium::spot_of(&destination);
            kept.extend(copies.iter().map(|&page| crate::outline::Relink {
                page,
                annotation,
                to,
                spot,
            }));
        }
    }
    (kept, lost)
}

/// Write the pages `sheets` name, in that order and each turned as it
/// says, to a new file at `to`. A sheet from no other file is a page of
/// `document`, the path and password of the document open. No file is
/// changed. Answers whether some page had a link to a page that did not
/// come along, which does not work in the new file. With a [`Layout`] of
/// two pages to a sheet, the sheets are written instead of the pages.
// ponytail: no size ceiling. The files are held in memory several times
// over while this runs (as read, as pdfium holds them, as saved, loaded
// again by `reorder`, and copied to the helper), so a 400MB scan wants
// about 2GB. "Save" refuses past `markup::IN_FILE_LIMIT`; refuse here
// too if that is ever too much.
pub fn assemble(
    document: Option<(&str, Option<&str>)>,
    sheets: &[Sheet],
    layout: Layout,
    to: &str,
) -> Result<bool, String> {
    assemble_with(document, sheets, layout, to, crate::atomic_write)
}

/// [`assemble`], but never over a file, even one that appears at `to`
/// while the new one is being made.
pub fn assemble_new(
    document: Option<(&str, Option<&str>)>,
    sheets: &[Sheet],
    layout: Layout,
    to: &str,
) -> Result<bool, String> {
    assemble_with(
        document,
        sheets,
        layout,
        to,
        crate::config::atomic_write_new,
    )
}

fn assemble_with(
    document: Option<(&str, Option<&str>)>,
    sheets: &[Sheet],
    layout: Layout,
    to: &str,
    write: fn(&std::path::Path, &[u8]) -> Result<(), String>,
) -> Result<bool, String> {
    if sheets.is_empty() {
        return Err("A document needs at least one page.".into());
    }
    let mut files: Vec<Option<&str>> = Vec::new();
    for sheet in sheets {
        if !files.contains(&sheet.from.as_deref()) {
            files.push(sheet.from.as_deref());
        }
    }
    // The files are read before pdfium's lock is taken, so that no window
    // waits on the disk to draw a page, and with one wait for all of them.
    let whole = crate::watch::all_whole(
        &(files.iter().flatten())
            .map(std::path::Path::new)
            .collect::<Vec<_>>(),
    );
    let mut whole = whole.into_iter();
    let read: Vec<(&str, Option<&str>, Source)> = (files.iter())
        .map(|file| match file {
            Some(path) => {
                let whole = whole.next().unwrap_or(false);
                Ok((*path, None, read_whole_as(path, whole)?))
            }
            None => {
                let (path, password) =
                    document.ok_or("There is no document to take pages from.")?;
                let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
                Ok((path, password, Source::Pdf(bytes)))
            }
        })
        .collect::<Result<_, String>>()?;
    let mut entries = Vec::new();
    let mut links = Vec::new();
    let mut lost = false;
    let mut at = vec![0; sheets.len()];
    let written = {
        let _library = crate::pdfium::library();
        let pdfium = crate::pdfium::pdfium()?;
        let mut made = pdfium
            .create_new_pdf()
            .map_err(|e| format!("a new document could not be made: {e}"))?;
        for ((path, password, bytes), file) in read.into_iter().zip(&files) {
            let source = load_source(pdfium, path, bytes, password)?;
            let mine: Vec<usize> = (0..sheets.len())
                .filter(|&index| sheets[index].from.as_deref() == *file)
                .collect();
            let start = made.pages().len() as usize;
            // **Every page of a file in one call.** pdfium copies what the
            // pages share — fonts, pictures, often one resource dictionary
            // for the whole document — once per call, so a call per run of
            // pages put a copy of all of it into the new file for every run.
            let listed: Vec<String> = mine.iter().map(|&i| sheets[i].page.to_string()).collect();
            made.pages_mut()
                .copy_pages_from_document(&source, &listed.join(","), start as i32)
                .map_err(|e| {
                    format!(
                        "the pages of {} could not be copied: {e}",
                        crate::store::file_name(path)
                    )
                })?;
            for (copied, &index) in mine.iter().enumerate() {
                at[index] = start + copied;
            }
            // Where a page of this file (one-based) is in the new file.
            let place = |page: usize| mine.iter().copied().find(|&i| sheets[i].page == page);
            let copies = |index: usize| {
                let page = index + 1;
                (mine.iter().copied())
                    .filter(|&i| sheets[i].page == page)
                    .collect()
            };
            let (kept, unkept) = links_kept(&source, copies);
            links.extend(kept);
            lost |= unkept;
            let (headings, _) = crate::pdfium::read_outline(&source);
            entries.extend(crate::outline::entries(&headings, place));
        }
        for (index, sheet) in sheets.iter().enumerate() {
            if sheet.turn.rem_euclid(4) != 0 {
                turn(&made, at[index] + 1, sheet.turn)?;
            }
        }
        made.save_to_bytes()
            .map_err(|e| format!("the new document could not be saved: {e}"))?
    };
    let written = reorder(written, &at)?;
    if layout == Layout::One {
        let (written, relinked) = slim_with(written, sheets.len(), &entries, &links);
        write(std::path::Path::new(to), &written)?;
        return Ok(lost || !relinked);
    }
    // The sheets carry no links or headings, so none is written before the
    // pages are laid out, and none is said to be lost. The pages are
    // slimmed first, since a page drawn onto a sheet takes its resources
    // with it.
    let (written, _) = slim_with(written, sheets.len(), &[], &[]);
    let work = crate::objects::Work {
        merge: true,
        ..Default::default()
    };
    let laid = lay_out(written, layout)?;
    let laid =
        crate::objects::rework(laid, &work).map_or_else(|refused| refused.bytes, |(laid, _)| laid);
    write(std::path::Path::new(to), &laid)?;
    Ok(false)
}

/// How the pages of a new file are put on its sheets of paper.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Layout {
    /// Each page on a sheet of its own.
    #[default]
    One,
    /// Two pages side by side on each sheet, in order.
    Two,
    /// Two pages side by side on each sheet, in the order that makes a
    /// booklet when the sheets are printed on both sides and folded in the
    /// middle.
    Booklet,
}

impl Layout {
    /// Which of `count` pages (zero-based) go on each sheet, left and
    /// right. `None` is a half left blank.
    pub fn sheets(self, count: usize) -> Vec<[Option<usize>; 2]> {
        let page = |index: usize| (index < count).then_some(index);
        match self {
            Layout::One => (0..count).map(|index| [Some(index), None]).collect(),
            Layout::Two => (0..count.div_ceil(2))
                .map(|sheet| [page(2 * sheet), page(2 * sheet + 1)])
                .collect(),
            // Each sheet of paper is two sides: the outside holds the last
            // page and the first, and the inside the second and the one
            // before the last. A booklet's pages are a multiple of four, so
            // the pages missing at the end are left blank.
            Layout::Booklet => {
                let all = count.div_ceil(4) * 4;
                (0..all / 4)
                    .flat_map(|paper| {
                        let (front, back) = (2 * paper, all - 1 - 2 * paper);
                        [[page(back), page(front)], [page(front + 1), page(back - 1)]]
                    })
                    .collect()
            }
        }
    }
}

/// The size most of `sizes` are, to the nearest point, and the first of
/// them where two sizes are as common.
fn most_common(sizes: &[(f64, f64)]) -> (f64, f64) {
    let point = |&(w, h): &(f64, f64)| (w.round() as i64, h.round() as i64);
    let count = |size: &(f64, f64)| {
        sizes
            .iter()
            .filter(|other| point(other) == point(size))
            .count()
    };
    // ponytail: quadratic in the pages, a map of sizes if a book of
    // thousands of pages is ever laid out.
    (sizes.iter())
        .rev()
        .max_by_key(|size| count(size))
        .copied()
        .unwrap_or((612.0, 792.0))
}

/// The pages of `bytes` put two to a sheet as `layout` orders them. Each
/// page is drawn onto its half as a form XObject, scaled to fit the half and
/// centred in it. A half is the size most of the pages are, as they are
/// shown, so that one fold-out page does not make every sheet larger.
/// A page's annotations are drawn into it first, since a form XObject
/// carries only the page's content. pdfium's lock is taken for a page or a
/// sheet at a time, so the pages being drawn for the window go between them.
fn lay_out(bytes: Vec<u8>, layout: Layout) -> Result<Vec<u8>, String> {
    let pdfium = crate::pdfium::pdfium()?;
    let held = crate::pdfium::library();
    let source = (pdfium.load_pdf_from_byte_vec(bytes, None))
        .map_err(|e| format!("the new document could not be read: {e}"))?;
    let made =
        (pdfium.create_new_pdf()).map_err(|e| format!("a new document could not be made: {e}"))?;
    let count = source.pages().len() as usize;
    drop(held);
    let bindings = pdfium.bindings();
    let lay = || -> Result<Vec<u8>, String> {
        let mut sizes = Vec::with_capacity(count);
        for index in 0..count {
            let _held = crate::pdfium::library();
            let page = (source.pages().get(index as i32))
                .map_err(|e| format!("page {} could not be read: {e}", index + 1))?;
            sizes.push((page.width().value as f64, page.height().value as f64));
            drop(page);
            crate::export::flatten(&source, index)?;
        }
        let (width, height) = most_common(&sizes);
        let into = bindings.get_handle_from_document(&made);
        let from = bindings.get_handle_from_document(&source);
        for (at, halves) in layout.sheets(sizes.len()).into_iter().enumerate() {
            let _held = crate::pdfium::library();
            let sheet = unsafe { bindings.FPDFPage_New(into, at as i32, 2.0 * width, height) };
            if sheet.is_null() {
                return Err("a sheet could not be made".into());
            }
            let mut drawn = true;
            for (half, index) in halves.into_iter().enumerate() {
                let Some(index) = index else {
                    continue;
                };
                let (w, h) = sizes[index];
                let scale = (width / w).min(height / h);
                let x = half as f64 * width + (width - w * scale) / 2.0;
                let y = (height - h * scale) / 2.0;
                unsafe {
                    let xobject = bindings.FPDF_NewXObjectFromPage(into, from, index as i32);
                    let object = bindings.FPDF_NewFormObjectFromXObject(xobject);
                    bindings.FPDF_CloseXObject(xobject);
                    if object.is_null() {
                        drawn = false;
                        break;
                    }
                    bindings.FPDFPageObj_Transform(object, scale, 0.0, 0.0, scale, x, y);
                    bindings.FPDFPage_InsertObject(sheet, object);
                }
            }
            drawn &= bindings.is_true(unsafe { bindings.FPDFPage_GenerateContent(sheet) });
            unsafe { bindings.FPDF_ClosePage(sheet) };
            if !drawn {
                return Err(format!("sheet {} could not be drawn", at + 1));
            }
        }
        let _held = crate::pdfium::library();
        (made.save_to_bytes()).map_err(|e| format!("the new document could not be saved: {e}"))
    };
    let laid = lay();
    // Closing the documents is a call into pdfium too.
    let held = crate::pdfium::library();
    drop(made);
    drop(source);
    drop(held);
    laid
}

/// A new file of `count` pages, every page slimmed, with `entries` written
/// as its outline and `links` pointed at their pages again. Answers whether
/// there were no links, or they were written.
fn slim_with(
    bytes: Vec<u8>,
    count: usize,
    entries: &[crate::outline::Entry],
    links: &[crate::outline::Relink],
) -> (Vec<u8>, bool) {
    let work = crate::objects::Work {
        slim: (0..count).collect(),
        outline: entries.to_vec(),
        relink: links.to_vec(),
        ..Default::default()
    };
    match crate::objects::rework(bytes, &work) {
        Ok((bytes, done)) => (bytes, !done.links_lost),
        Err(refused) => (refused.bytes, links.is_empty()),
    }
}

/// Every page with annotations of its own. pdfium gives the copies of a
/// page that is in a new file twice the same annotation objects, so a
/// comment changed on one copy would change on the other, and each
/// annotation's `/P` would name only one of the pages. Each annotation a
/// page shares with a page before it is copied, with its popup, except a
/// form field's widget, which the field names among its kids. Answers
/// whether any was copied.
pub(crate) fn own_annotations(document: &mut lopdf::Document) -> bool {
    use lopdf::{Object, ObjectId};
    let mut seen = std::collections::HashSet::new();
    let mut copied = false;
    for page in document
        .get_pages()
        .into_values()
        .collect::<Vec<ObjectId>>()
    {
        let Ok(annots) = (document.get_dictionary(page)).and_then(|d| d.get(b"Annots")) else {
            continue;
        };
        let list = match annots {
            Object::Reference(id) => document.get_object(*id).and_then(Object::as_array),
            other => other.as_array(),
        };
        let Ok(list) = list.cloned() else {
            continue;
        };
        let ids: Vec<ObjectId> = list.iter().filter_map(|o| o.as_reference().ok()).collect();
        let widget = |id: &ObjectId| {
            (document.get_dictionary(*id))
                .and_then(|d| d.get(b"Subtype"))
                .and_then(Object::as_name)
                .is_ok_and(|subtype| subtype == b"Widget")
        };
        let shared: Vec<ObjectId> = ids
            .iter()
            .filter(|id| seen.contains(*id) && !widget(id))
            .copied()
            .collect();
        seen.extend(ids);
        if shared.is_empty() {
            continue;
        }
        let mut copies = std::collections::HashMap::new();
        for &id in &shared {
            if let Ok(dictionary) = document.get_dictionary(id).cloned() {
                copies.insert(id, document.add_object(dictionary));
            }
        }
        // The copies lead to each other, and to their page.
        let point = |object: &mut Object| {
            if let Object::Reference(id) = object {
                if let Some(&copy) = copies.get(id) {
                    *id = copy;
                }
            }
        };
        for &copy in copies.values() {
            if let Ok(dictionary) = document.get_object_mut(copy).and_then(Object::as_dict_mut) {
                for key in [&b"Popup"[..], b"Parent", b"IRT"] {
                    if let Ok(object) = dictionary.get_mut(key) {
                        point(object);
                    }
                }
                dictionary.set("P", page);
            }
        }
        let mut list = list;
        list.iter_mut().for_each(point);
        if let Ok(dictionary) = document.get_object_mut(page).and_then(Object::as_dict_mut) {
            dictionary.set("Annots", list);
        }
        copied = true;
    }
    copied
}

/// The pages put in `order`, where `order[at]` is the page (zero-based)
/// that goes at `at`. Every page is put straight under the root of the
/// page tree, with the resources, boxes and turn it took from the nodes
/// above it, because a page that pdfium moves under another node takes
/// that node's instead. Answers whether the pages were put in order.
pub(crate) fn put_in_order(document: &mut lopdf::Document, order: &[usize]) -> bool {
    use lopdf::{Object, ObjectId};
    const INHERITED: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];
    let pages: Vec<ObjectId> = document.get_pages().into_values().collect();
    let mut sorted = order.to_vec();
    sorted.sort_unstable();
    if sorted != (0..pages.len()).collect::<Vec<_>>() {
        return false;
    }
    let root = (document.catalog())
        .and_then(|catalog| catalog.get(b"Pages"))
        .and_then(Object::as_reference);
    let Ok(root) = root else {
        return false;
    };
    for &page in &pages {
        // The nearest node that says a thing is the one the page takes it
        // from. The walk is bounded, since a broken tree can lead round.
        let mut inherited: Vec<(&[u8], Object)> = Vec::new();
        let mut node = page;
        for _ in 0..64 {
            let parent = (document.get_dictionary(node))
                .and_then(|dictionary| dictionary.get(b"Parent"))
                .and_then(Object::as_reference);
            let Ok(parent) = parent else {
                break;
            };
            let Ok(dictionary) = document.get_dictionary(parent) else {
                break;
            };
            for key in INHERITED {
                if !inherited.iter().any(|(had, _)| *had == key) {
                    if let Ok(value) = dictionary.get(key) {
                        inherited.push((key, value.clone()));
                    }
                }
            }
            node = parent;
        }
        let Ok(dictionary) = document.get_dictionary_mut(page) else {
            return false;
        };
        for (key, value) in inherited {
            if !dictionary.has(key) {
                dictionary.set(key, value);
            }
        }
        dictionary.set("Parent", root);
    }
    let kids = order.iter().map(|&page| Object::Reference(pages[page]));
    let Ok(tree) = document.get_dictionary_mut(root) else {
        return false;
    };
    tree.set("Kids", Object::Array(kids.collect()));
    tree.set("Count", pages.len() as i64);
    true
}

/// The pages at `indices` (zero-based) made to keep only the resources
/// their content uses. The file is then compacted (`objects::rework`).
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
/// A new file has every page slimmed. The open document has only the pages
/// put in from another file slimmed: its own pages are only as wasteful as
/// they always were, reading every page's content of a long book is slow,
/// and a wrong decision about one would be written into the reader's file.
pub(crate) fn slim_pages(document: &mut lopdf::Document, indices: &[usize]) {
    let pages: Vec<lopdf::ObjectId> = document.get_pages().into_values().collect();
    let mut every_page = indices.len() >= pages.len();
    let chosen = (pages.iter().enumerate()).filter(|(at, _)| indices.contains(at));
    for (_, &page) in chosen {
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
            // `/DefaultRGB` and its kind are used without being named: they
            // stand in for the device colour spaces wherever those are.
            let default = key == b"ColorSpace" && entry.starts_with(b"Default");
            if default
                || used
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
    // So does a soft mask's group with no resources of its own.
    if let Ok(states) = slim.get(b"ExtGState").and_then(Object::as_dict) {
        for (_, state) in states.iter() {
            let state = document.dereference(state).ok()?.1.as_dict().ok()?;
            let Ok(mask) = state.get(b"SMask") else {
                continue;
            };
            let Ok(mask) = document
                .dereference(mask)
                .and_then(|(_, mask)| mask.as_dict())
            else {
                continue;
            };
            let Ok(group) = mask.get(b"G") else {
                continue;
            };
            let group = document.dereference(group).ok()?.1.as_stream().ok()?;
            if !group.dict.has(b"Resources") {
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
    // A line counts when its middle is above the place the heading points
    // to, which the heading's own line is not. A line that ends just below
    // that place still counts, since a page saved twice loses nothing.
    (lines(text).into_iter())
        .filter(|line| line.middle <= down && !line.page_number)
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
    /// Whether it holds only a page number: "12", "xii", "– 12 –" or
    /// "Page 12".
    page_number: bool,
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
            let kept: String = (line.iter())
                .map(|(character, _)| character.to_ascii_lowercase())
                .filter(|character| character.is_alphanumeric())
                .collect();
            let number = (kept.strip_prefix("page"))
                .or_else(|| kept.strip_prefix('p'))
                .filter(|rest| !rest.is_empty())
                .unwrap_or(&kept);
            Line {
                middle: middle(&line[0].1),
                page_number: number.chars().all(|c| c.is_ascii_digit())
                    || crate::labels::Style::RomanLower.read(number).is_some(),
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

    /// Pages put in order by pdfium keep the box and the turn they took
    /// from their branch of the page tree.
    #[test]
    fn pages_pdfium_puts_in_order_keep_their_boxes_and_turns() {
        let shown = |bytes: &[u8]| {
            let _library = crate::pdfium::library();
            let pdfium = crate::pdfium::pdfium().unwrap();
            let document = pdfium.load_pdf_from_byte_slice(bytes, None).unwrap();
            (document.pages().iter())
                .map(|page| {
                    let bounds = page.boundaries().bounding().unwrap().bounds;
                    (format!("{bounds:?}"), page.rotation().unwrap() as i32)
                })
                .collect::<Vec<_>>()
        };
        let bytes = std::fs::read(crate::fixture::inherited_pdf()).unwrap();
        let before = shown(&bytes);
        let order = [3, 0, 1, 2];
        let after = shown(&reorder(bytes, &order).unwrap());
        for (at, &page) in order.iter().enumerate() {
            assert_eq!(after[at], before[page], "page {}", page + 1);
        }
    }

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
        // Nor does a page number, however it is written.
        for number in ["xii", "– 12 –", "Page 12", "p. 12"] {
            let opening = page_of(&[(number, 20.0), ("Chapter", 300.0)]);
            assert!(!text_above(&opening, &around, 295.0), "{number}");
        }
        // A last line that reaches a little below the place the heading
        // points to still counts.
        let low = page_of(&[("The end of it.", 290.0), ("Chapter", 310.0)]);
        assert!(text_above(&low, &around, 297.0));
    }

    /// The document's own pages, one-based, in this order.
    fn arranged(pages: &[usize]) -> Edit {
        let sheet = |page| Sheet {
            from: None,
            page,
            turn: 0,
        };
        Edit::Arrange(pages.iter().map(|&page| sheet(page)).collect())
    }

    #[test]
    fn a_removed_page_lands_on_the_one_after_it() {
        let moved = arranged(&[1, 4]).moved(4);
        assert_eq!(moved.0, vec![Some(0), None, None, Some(1)]);
        assert_eq!(moved.page(4), Some(2));
        assert_eq!(moved.landing(2), 2);
        // The last page gone, a reader on it lands on the last left.
        let moved = arranged(&[1, 2]).moved(3);
        assert_eq!(moved.landing(3), 2);
    }

    #[test]
    fn moved_pages_are_followed_and_the_same_order_moves_nothing() {
        let moved = arranged(&[3, 1, 2]).moved(3);
        assert_eq!(moved.0, vec![Some(1), Some(2), Some(0)]);
        assert!(arranged(&[1, 2, 3, 4]).moves_nothing(4));
        assert!(!arranged(&[1, 3, 2, 4]).moves_nothing(4));
        assert!(!arranged(&[1, 2, 3]).moves_nothing(4));
    }

    #[test]
    fn added_pages_push_the_rest_along_and_undo_takes_them_out() {
        let mut edit = arranged(&[1, 2, 3]);
        if let Edit::Arrange(sheets) = &mut edit {
            let other = |page| Sheet {
                from: Some("other.pdf".into()),
                page,
                turn: 0,
            };
            sheets.splice(1..1, [other(1), other(2)]);
        }
        let moved = edit.moved(3);
        assert_eq!(moved.0, vec![Some(0), Some(3), Some(4)]);
        // Undone, the pages put in go nowhere, and the way back again is
        // the edit itself.
        let back = moved.inverse(5);
        assert_eq!(back.0, vec![Some(0), None, None, Some(1), Some(2)]);
        assert_eq!(back.inverse(3), moved);
    }

    #[test]
    fn a_removal_undone_puts_each_page_back() {
        let moved = arranged(&[1, 3, 5]).moved(5);
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
    fn a_default_colour_space_and_a_soft_mask_are_kept() {
        use lopdf::{dictionary, Object, Stream};
        let plain =
            || dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" };
        let (mut document, page) = page_with(plain());
        let group = document.add_object(Stream::new(dictionary! { "Subtype" => "Form" }, vec![]));
        let mask = dictionary! { "S" => "Luminosity", "G" => group };
        let resources = document
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .unwrap()
            .get_mut(b"Resources")
            .and_then(Object::as_dict_mut)
            .unwrap();
        resources.set("ColorSpace", dictionary! { "DefaultRGB" => "DeviceRGB" });
        let slim = slimmed(&document, page).unwrap();
        assert!(slim
            .get(b"ColorSpace")
            .unwrap()
            .as_dict()
            .unwrap()
            .has(b"DefaultRGB"));
        let resources = document
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .unwrap()
            .get_mut(b"Resources")
            .and_then(Object::as_dict_mut)
            .unwrap();
        resources.set(
            "ExtGState",
            dictionary! { "G1" => dictionary! { "SMask" => mask } },
        );
        let contents = document.get_page_contents(page)[0];
        let stream = document
            .get_object_mut(contents)
            .and_then(Object::as_stream_mut)
            .unwrap();
        stream.set_content(b"/G1 gs BT /F1 12 Tf (a) Tj ET".to_vec());
        assert!(
            slimmed(&document, page).is_none(),
            "a mask with no resources"
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
    fn a_new_file_is_offered_beside_the_old() {
        assert_eq!(
            beside("/papers/paper.pdf", "edited"),
            "/papers/paper (edited).pdf"
        );
    }
}
