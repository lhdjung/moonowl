//! pdfium behind `PageSource`.
//!
//! Two things about pdfium are restated from `render.rs` on the
//! `pdfium-prototype` branch, because they are properties of the library and
//! not of the app: it has one global initialiser and no thread safety, so
//! there is one instance for the process and a lock around it; and a page
//! costs nothing once dropped, so there is no page cache to keep here — what
//! is cached is the texture, one layer up, where the memory actually is.

use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use pdfium_render::prelude::*;

use crate::layout::{Size, View};
use crate::render::{Bitmap, Heading, Link, PageSource, PageText, Rect, Target};

/// The lock every call into pdfium is taken behind.
///
/// **`pdfium-render`'s `thread_safe` feature does not make pdfium thread
/// safe.** It is two `unsafe impl`s and a bound on the bindings accessor;
/// nothing in the crate serialises a call. pdfium has process-wide state and no
/// locking, and two threads inside it abort the process — `SIGABRT`, no panic,
/// no message, no stack.
///
/// Invisible with one document on one thread; it arrived with the harness,
/// because `cargo test` runs its tests in parallel. The lock is the
/// **library's**, not the document's — a per-document lock is exactly what was
/// there and exactly what does not help.
static LIBRARY: Mutex<()> = Mutex::new(());

pub(crate) fn library() -> std::sync::MutexGuard<'static, ()> {
    LIBRARY.lock().unwrap_or_else(|e| e.into_inner())
}

/// The one pdfium instance, created on first use and kept for the life of the
/// process. Leaked deliberately: every document and page borrows from it.
pub(crate) fn pdfium() -> Result<&'static Pdfium, String> {
    static INSTANCE: Mutex<Option<&'static Pdfium>> = Mutex::new(None);
    let mut held = INSTANCE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(instance) = *held {
        return Ok(instance);
    }
    let bindings =
        Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&library_dir()))
            .map_err(|e| format!("pdfium could not be loaded: {e}"))?;
    let instance: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bindings)));
    *held = Some(instance);
    Ok(instance)
}

/// Where `libpdfium` is: `MOONOWL_PDFIUM` if it is set, then wherever the bundle
/// this binary was installed from put it, then the copy vendored with the
/// spike. Nothing is fetched at runtime, which is the promise the pdf.js assets
/// make today.
///
/// Three places rather than one because the four bundle formats disagree: a
/// `.app` keeps a signed dylib in `Contents/Frameworks`, an `.msi` keeps the
/// DLL beside the `.exe`, and a `.deb` splits them — `/usr/bin/Moonowl` and
/// `/usr/lib/Moonowl/`. They are stat'd in order rather than picked by `cfg`,
/// because the ones that are not there cost nothing.
fn library_dir() -> String {
    if let Ok(dir) = std::env::var("MOONOWL_PDFIUM") {
        return dir;
    }
    let name = Pdfium::pdfium_platform_library_name();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        let beside = [dir.join("../Frameworks"), dir.join("../lib/Moonowl"), dir];
        if let Some(found) = beside.iter().find(|dir| dir.join(&name).exists()) {
            return found.to_string_lossy().into_owned();
        }
    }
    format!(
        "{}/experiments/dioxus-spike/vendor/lib",
        env!("CARGO_MANIFEST_DIR")
    )
}

pub struct Document {
    inner: Mutex<Open>,
    path: String,
    sizes: Vec<Size>,
    /// Each page's space, read beside its size: what a link's destination
    /// is measured in, and the one thing about a page a link on another
    /// page needs. See [`crate::markup::Space`].
    spaces: Vec<crate::markup::Space>,
    /// The document's own table of contents, read once when it is opened.
    ///
    /// Read at open rather than on demand, unlike everything else here, and
    /// for the reason the app reads it at open too: it is the one question
    /// whose answer decides what the sidebar *is* — a column of chapters or a
    /// sentence saying there are none — and a document of four hundred pages
    /// has an outline of tens of lines. What it costs is a walk of the
    /// bookmark tree behind the same lock every other call takes; what it
    /// saves is the sidebar having to be an async component to ask.
    outline: Vec<Heading>,
    /// What the document calls its own pages, read once beside their sizes,
    /// and empty when it calls them 1 to n. See [`PageSource::labels`].
    labels: Vec<String>,
    /// What the document calls *itself*, as written, or empty. Read at open
    /// like the outline and for the same reason: it decides what the toolbar
    /// and the window say, and a name that arrives late is a name that was
    /// wrong. See [`PageSource::title`].
    title: String,
    /// What the document says about itself, read at open with everything else
    /// that costs a page load. See [`PageSource::details`].
    details: Vec<(String, String)>,
    /// Whether the file is encrypted, password or no password. See
    /// [`PageSource::encrypted`], which is the one thing that asks.
    encrypted: bool,
    /// The password itself, for opening the same file again after a reload.
    password: Option<String>,
    /// Whether the document carries a signature with bytes in it. Read at
    /// open, with everything else that costs a file load; see
    /// [`PageSource::sealed`].
    sealed: bool,
    opened_in: f64,
}

struct Open {
    /// `None` between [`Document::release`] and the reopen that follows it.
    ///
    /// **The file is held open for as long as the document is**, which is the
    /// whole of why this is an `Option`. pdfium reads a page's objects when
    /// the page is asked for, not when the file is loaded, so
    /// `FPDF_LoadDocument` keeps the file open for the life of the document —
    /// which is the same lazy read the app gets from `read_range`, arrived at
    /// from the other side. It costs nothing until something wants to *write*
    /// that file, and then it costs everything: see [`Document::release`].
    document: Option<PdfDocument<'static>>,
    /// The one buffer every page is drawn into.
    ///
    /// pdfium will make its own if asked, and `as_raw_bytes()` then copies it
    /// into a `Vec` — two 24MB allocations a page, freed immediately and *not*
    /// handed back by macOS's allocator. `PdfBitmap::from_bytes` renders into a
    /// buffer we own, so a document scrolled end to end allocates once.
    ///
    /// Behind the same lock as the document, because every render is already
    /// serialised through it: "one buffer" and "one page at a time" are the
    /// same statement.
    scratch: Vec<u8>,
}

// pdfium is not thread safe and everything here is behind the lock; the
// `PdfDocument` borrows from the leaked instance, which lives forever.
unsafe impl Send for Open {}

/// **Closing a document is a call into pdfium like any other, and it was the
/// one call in this file not taken behind the lock.**
///
/// Nothing here calls `FPDF_CloseDocument`: `PdfDocument`'s own `Drop` does,
/// whenever the last `Arc<dyn PageSource>` goes, on whatever thread that
/// happens to be. What it corrupts is not this document — pdfium keeps a
/// **process-wide** map of stock fonts keyed by `CPDF_Document*`, and
/// `~CPDF_Document` erases its own entry from it. Erase a node from a red-black
/// tree while another thread inserts one and any thread that later walks it
/// dies, which is why the crash landed in a test that was *opening* a document.
///
/// So the close is brought inside the lock, and the `Open` that drops
/// afterwards has nothing left to close. The rule to watch: a `Document` must
/// never be dropped by a thread already holding [`library()`], because `Mutex`
/// is not reentrant.
///
/// The general form is worth carrying to anything wrapping a C library behind a
/// lock: **a `Drop` is a call site, and it is the one call site that does not
/// appear at the place it happens.**
impl Drop for Document {
    fn drop(&mut self) {
        let _library = library();
        let mut held = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        held.document = None;
    }
}

impl Document {
    /// A document with no password, which is nearly all of them.
    pub fn open(path: &str) -> Result<Self, String> {
        Self::open_with(path, None).map_err(|refused| refused.to_string())
    }

    /// A document, with the password if it needs one.
    ///
    /// **The error type is the whole reason this is not one function with an
    /// `Option` on it.** Everything else that can go wrong here is a sentence
    /// to show the reader and nothing more; a locked document is a *question*,
    /// and the caller has to be able to tell the two apart without reading
    /// English out of a string. See [`crate::render::Refusal`].
    pub fn open_with(path: &str, password: Option<&str>) -> Result<Self, crate::render::Refusal> {
        let began = Instant::now();
        // Asked before pdfium is, because pdfium answers it badly: a missing
        // file comes back as `IoError(Os { code: 2, kind: NotFound, … })`,
        // which is a Rust type name and a struct in front of the one fact
        // worth saying. It is also much the commonest way to fail here.
        if !std::path::Path::new(path).is_file() {
            return Err(crate::render::Refusal::Said(format!(
                "{path}: there is no such file."
            )));
        }
        let _library = library();
        let pdfium = pdfium().map_err(crate::render::Refusal::Said)?;
        // **The one error worth telling apart from the rest**, and pdfium is
        // the reason it can be: `FPDF_ERR_PASSWORD` is a different answer from
        // `FPDF_ERR_FORMAT`, so a locked document and a corrupt one do not
        // arrive here looking the same. Which of the two questions it is —
        // "this needs a password" or "that password was not right" — is the
        // caller's to know, because the caller is the one who did or did not
        // supply one.
        let document = pdfium.load_pdf_from_file(path, password).map_err(|e| {
            if matches!(
                e,
                PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError)
            ) {
                crate::render::Refusal::Locked
            } else {
                crate::render::Refusal::Said(format!("{path}: {e}"))
            }
        })?;
        // One pass, because loading a page is what both of these cost and
        // `pages().iter()` loads each one. Four hundred pages is a few
        // milliseconds; asking twice would be twice that for no reason.
        let mut sizes = Vec::with_capacity(document.pages().len() as usize);
        let mut labels = Vec::with_capacity(sizes.capacity());
        let mut spaces = Vec::with_capacity(sizes.capacity());
        for page in document.pages().iter() {
            sizes.push(Size {
                width: page.width().value as f64,
                height: page.height().value as f64,
            });
            spaces.push(crate::markup::Space::of(&page));
            labels.push(page.label().unwrap_or_default().to_string());
        }
        let outline = read_outline(&document);
        // `/Info /Title`, trimmed. Whether it is worth showing is not asked
        // here: that needs the file name to weigh it against, and the one
        // place that has both is `store::worth_calling`.
        let title = document
            .metadata()
            .get(PdfDocumentMetadataTagType::Title)
            .map(|tag| tag.value().trim().to_string())
            .unwrap_or_default();
        let details = read_details(&document, sizes.first(), path);
        // **A signature field is not a signature.** `FPDF_GetSignatureCount`
        // counts every `/FT /Sig` field in the `/AcroForm`, signed or not, and
        // a great many documents ship with a blank one — the line at the foot
        // of a contract nobody has signed. Warning that ink would break a
        // signature that does not exist is a warning a reader learns to
        // ignore. `/Contents` is what tells the two apart, and `bytes()` is
        // that entry.
        let sealed = document
            .signatures()
            .iter()
            .any(|signature| !signature.bytes().is_empty());
        // **Asked of pdfium rather than inferred from the password**: a great
        // many documents are encrypted under an owner password alone — no
        // printing, no copying — and open with none, and rewriting one of
        // those is rewriting an encrypted file. Anything but a plain
        // "unprotected" counts, an AES-256 revision pdfium-render has no name
        // for included.
        let encrypted = !matches!(
            document.permissions().security_handler_revision(),
            Ok(PdfSecurityHandlerRevision::Unprotected)
        );
        let labels = own_numbering(labels);
        let labels = if labels.is_empty() {
            printed_numbering(&document, sizes.len())
        } else {
            labels
        };
        Ok(Document {
            path: path.to_string(),
            labels,
            sizes,
            spaces,
            outline,
            title,
            details,
            encrypted,
            password: password.map(str::to_string),
            sealed,
            opened_in: began.elapsed().as_secs_f64() * 1000.0,
            inner: Mutex::new(Open {
                document: Some(document),
                scratch: Vec::new(),
            }),
        })
    }
}

impl PageSource for Document {
    fn pages(&self) -> usize {
        self.sizes.len()
    }

    fn size_of(&self, index: usize) -> Size {
        self.sizes[index]
    }

    fn path(&self) -> &str {
        &self.path
    }

    fn outline(&self) -> Vec<Heading> {
        self.outline.clone()
    }

    fn labels(&self) -> Vec<String> {
        self.labels.clone()
    }

    fn title(&self) -> String {
        self.title.clone()
    }

    fn details(&self) -> Vec<(String, String)> {
        self.details.clone()
    }

    /// The links on one page. Three things about pdfium's answer:
    ///
    /// *A link's area comes from the annotation, not the action.*
    /// `FPDFLink_GetAnnotRect` is the `/Rect` of the `/Link`, counting from the
    /// bottom — the same flip [`PageSource::text_of`] does.
    ///
    /// *A destination arrives two ways and a document uses either*: a `/Dest`
    /// on the annotation, or one under `/A` in a `/GoTo` action. The bookmark
    /// walk below has the same shape for the same reason.
    ///
    /// *And a link with neither is dropped* rather than kept as a dead
    /// rectangle: a `/Launch`, a `/JavaScript`, a `/Dest` resolving to no page
    /// is a hit area over printed words that does nothing, which reads as the
    /// app being broken.
    fn links_of(&self, index: usize) -> Vec<Link> {
        let _library = library();
        let held = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(document) = held.document.as_ref() else {
            return Vec::new();
        };
        let Ok(page) = document.pages().get(index as i32) else {
            return Vec::new();
        };
        let space = crate::markup::Space::of(&page);
        let mut links = Vec::new();
        for link in page.links().iter() {
            let Ok(rect) = link.rect() else { continue };
            let area = space.down(&rect);
            // A link with no area is not a link anybody can click, whatever
            // it points at.
            if area.width <= 0.0 || area.height <= 0.0 {
                continue;
            }
            let action = link.action();
            let away = action
                .as_ref()
                .and_then(|action| action.as_uri_action())
                .and_then(|uri| uri.uri().ok())
                .filter(|uri| !uri.is_empty());
            let target = match away {
                Some(uri) => Target::Away(uri),
                None => {
                    let place = link.destination().or_else(|| {
                        action
                            .as_ref()?
                            .as_local_destination_action()?
                            .destination()
                            .ok()
                    });
                    let Some(place) = place else { continue };
                    let Ok(page) = place.page_index() else {
                        continue;
                    };
                    // In the space of the page it points *at*: that is what
                    // `/XYZ top` is measured in, and the page the offset is
                    // multiplied back out by.
                    let target = self.spaces.get(page as usize).copied().unwrap_or(space);
                    Target::Place {
                        page: page as usize + 1,
                        offset: offset_within(&place, &target),
                    }
                }
            };
            links.push(Link { rect: area, target });
        }
        links
    }

    /// The notes on one page: any annotation with words in it.
    ///
    /// **By whether there is anything to read rather than by subtype**, which
    /// is `notesIn` in `viewer.ts` and its reasoning: a sticky note and a
    /// comment on a highlighted sentence are the same thing to a reader.
    /// Links are the exception — their text is where they go, and that is
    /// already on the link — and so is a Popup, which is the box another
    /// annotation's words are shown *in* rather than an annotation with words
    /// of its own.
    fn notes_of(&self, index: usize) -> Vec<crate::render::Note> {
        let _library = library();
        let held = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(document) = held.document.as_ref() else {
            return Vec::new();
        };
        let Ok(page) = document.pages().get(index as i32) else {
            return Vec::new();
        };
        let (width, height) = (page.width().value as f64, page.height().value as f64);
        let space = crate::markup::Space::of(&page);
        let mut notes = Vec::new();
        for annotation in page.annotations().iter() {
            if matches!(
                annotation,
                PdfPageAnnotation::Link(_) | PdfPageAnnotation::Popup(_)
            ) {
                continue;
            }
            let text = annotation.contents().unwrap_or_default().trim().to_string();
            if text.is_empty() {
                continue;
            }
            let Ok(bounds) = annotation.bounds() else {
                continue;
            };
            let rect = space.down(&bounds);
            if rect.width <= 0.0 || rect.height <= 0.0 {
                continue;
            }
            notes.push(crate::render::Note {
                // Small in both directions is a marker; anything bigger is a
                // comment sitting over words somebody may want to select.
                icon: rect.width < width * 0.06 && rect.height < height * 0.06,
                rect,
                by: annotation.creator().unwrap_or_default().trim().to_string(),
                text,
            });
        }
        notes
    }

    /// Every highlight in the document, in reading order. Three things pdfium
    /// decides here:
    ///
    /// *A highlight is `/Subtype /Highlight` and nothing else.* Underline,
    /// strike-out and squiggly are not read because they are not written, and a
    /// list showing a mark this reader cannot unmake would have a dead row in
    /// it.
    ///
    /// *The colour is `/C`, which pdfium calls the stroke colour.* See
    /// [`crate::markup::add`], where taking that name at face value costs an
    /// hour.
    ///
    /// *And a highlight with no `/QuadPoints` is dropped*: a row that scrolls
    /// to a page and points at nothing is worse than no row.
    fn markup(&self) -> Vec<crate::markup::Mark> {
        let _library = library();
        let held = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(document) = held.document.as_ref() else {
            return Vec::new();
        };
        let mut marks = Vec::new();
        for (number, page) in document.pages().iter().enumerate() {
            let space = crate::markup::Space::of(&page);
            for (index, annotation) in page.annotations().iter().enumerate() {
                let PdfPageAnnotation::Highlight(highlight) = &annotation else {
                    continue;
                };
                let quads: Vec<Rect> = highlight
                    .attachment_points()
                    .iter()
                    .map(|quad| space.down(&quad.to_rect()))
                    .filter(|quad| quad.width > 0.0 && quad.height > 0.0)
                    .collect();
                if quads.is_empty() {
                    continue;
                }
                let colour = highlight
                    .stroke_color()
                    .map(|colour| {
                        crate::palette::hex([colour.red(), colour.green(), colour.blue()])
                    })
                    .unwrap_or_else(|_| "#ffd60a".to_string());
                marks.push(crate::markup::Mark {
                    page: number + 1,
                    index,
                    quads,
                    color: colour,
                });
            }
        }
        marks
    }

    /// Every signature in the document, read the way the highlights are.
    ///
    /// **An `/Ink` annotation is not necessarily a signature**, and this does
    /// not pretend otherwise: what it reads is every ink annotation, whoever
    /// put it there and whatever they meant by it. That is the honest answer
    /// and it is also the useful one — a reader who wants their signature off
    /// a page can take it off, and so can they with a scribble somebody else
    /// left, which is a thing they would also like to be able to do.
    fn signatures(&self) -> Vec<crate::sign::Placed> {
        let _library = library();
        let held = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(document) = held.document.as_ref() else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for (number, page) in document.pages().iter().enumerate() {
            let space = crate::markup::Space::of(&page);
            for (index, annotation) in page.annotations().iter().enumerate() {
                // Ink is a hand and a stamp is a line of type — the two things
                // this reader writes, listed together because they come off the
                // page the same way and a reader taking one off does not think
                // of them as two features.
                let (kind, bounds, by) = match &annotation {
                    PdfPageAnnotation::Ink(ink) => (
                        crate::sign::Written::Hand,
                        ink.bounds(),
                        ink.creator().unwrap_or_default(),
                    ),
                    PdfPageAnnotation::Stamp(stamp) => (
                        crate::sign::Written::Line,
                        stamp.bounds(),
                        stamp.contents().unwrap_or_default(),
                    ),
                    _ => continue,
                };
                let at = space.down(&bounds.unwrap_or(PdfRect::ZERO));
                if at.width <= 0.0 || at.height <= 0.0 {
                    continue;
                }
                found.push(crate::sign::Placed {
                    kind,
                    page: number + 1,
                    index,
                    at,
                    by,
                });
            }
        }
        found
    }

    /// Close the file, keeping everything else. See
    /// [`PageSource::release`] for why this exists at all.
    ///
    /// What is left behind is a `Document` that knows its own path, its
    /// pages' sizes, its outline, its labels and its title — everything read
    /// at open — and cannot draw or read a page. That is deliberate: the
    /// layout is built from those sizes, and a release that took them with it
    /// would take the reader's place in the document with them.
    fn release(&self) {
        let _library = library();
        let mut held = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        held.document = None;
    }

    /// Open the file again after [`Document::release`], if it is still
    /// released. For a write that failed and left nothing to reopen, so the
    /// reader is not left with a document that draws nothing.
    fn retake(&self) {
        let _library = library();
        let mut held = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if held.document.is_some() {
            return;
        }
        if let Ok(pdfium) = pdfium() {
            held.document = pdfium
                .load_pdf_from_file(&self.path, self.password.as_deref())
                .ok();
        }
    }

    fn encrypted(&self) -> bool {
        self.encrypted
    }

    fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    fn sealed(&self) -> bool {
        self.sealed
    }

    fn opened_in(&self) -> f64 {
        self.opened_in
    }

    /// One page's characters and where each of them sits. Three things about
    /// pdfium's answer:
    ///
    /// *`loose_bounds`, not `tight_bounds`.* The tight box is the glyph's own
    /// outline, so a highlight drawn from it clips ascenders and descenders and
    /// a lower-case run comes out half the height of its line. The loose box is
    /// the character's cell, which is what a reader means by "highlight this".
    ///
    /// *A character can have no box at all.* pdfium generates spaces and line
    /// breaks the printer never drew, and asking one for its bounds fails. They
    /// are kept — they are what makes two words two words — with a box of no
    /// size, and [`PageText::quads`] skips them.
    ///
    /// *And this is one FFI call per character*, which at a couple of thousand
    /// a page is the cost of the whole feature.
    fn text_of(&self, index: usize) -> PageText {
        let _library = library();
        let held = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(document) = held.document.as_ref() else {
            return PageText::default();
        };
        let Ok(page) = document.pages().get(index as i32) else {
            return PageText::default();
        };
        read_text(&page)
    }

    fn render(
        &self,
        index: usize,
        width: u32,
        height: u32,
        view: View,
        take: &mut dyn FnMut(Bitmap),
    ) -> Result<(), String> {
        // The library first, then the document — always in that order, which
        // is what keeps two documents from deadlocking each other.
        let _library = library();
        let mut held = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let held = &mut *held;
        let page = held
            .document
            .as_ref()
            .ok_or("That document has been closed.")?
            .pages()
            .get(index as i32)
            .map_err(|e| format!("page {index}: {e}"))?;

        // A row can be wider than the pixels in it; ask rather than assume.
        // (For BGRA it works out at exactly four bytes a pixel, because the
        // stride is rounded up to a multiple of four and it is already one.)
        let wanted = PdfBitmap::bytes_required_for_size_and_format(
            width as i32,
            height as i32,
            PdfBitmapFormat::BGRA,
        );
        if held.scratch.len() != wanted {
            // Only when the size actually changes — which is a zoom or a
            // window resize, not a page turn.
            held.scratch.clear();
            held.scratch.resize(wanted, 0);
        }
        let mut bitmap = PdfBitmap::from_bytes(
            width as i32,
            height as i32,
            PdfBitmapFormat::BGRA,
            &mut held.scratch,
        )
        .map_err(|e| format!("page {index}: {e}"))?;

        // **The crop is a window onto a page drawn whole, not a page drawn
        // small.** `start_x`/`start_y` are where the page's top-left corner goes
        // in the bitmap and everything outside it is not drawn — so a page is
        // asked for at the size it *would* be uncropped and slid up and left.
        // That is why a trimmed document costs *less* to draw than an untrimmed
        // one: nothing is rendered that will be clipped.
        //
        // The rotation is a quarter turn on top of whatever `/Rotate` the file
        // asks for.
        // **`set_reverse_byte_order(false)`, and it is not a nicety.**
        // `PdfRenderConfig::new()` turns `FPDF_REVERSE_BYTE_ORDER` *on* for
        // `image`'s `DynamicImage`, so a bitmap asked for as BGRA is not BGRA.
        // Both paths above take pdfium at its word — the GPU uploads as
        // `Bgra8Unorm` and lets the sampler swizzle, and `ensure_software` swaps
        // by hand — so with the flag on, both were swapping an order that had
        // already been swapped.
        //
        // Invisible on almost everything: a page of black type is the same
        // picture either way, and so is every scan. What shows it is a *known*
        // colour, and the first thing to put one on a page is markup — a
        // passage marked `#ff0000` came back `#0000ff`.
        let mut config = PdfRenderConfig::new()
            .set_reverse_byte_order(false)
            .set_target_size(width as i32, height as i32);
        if let Some(crop) = view.crop {
            // What the whole page would be, at the scale that makes the crop
            // exactly the pixels asked for. Rounded once, here, so that the
            // origin below is an offset into the same grid.
            let whole_width = (width as f64 / crop.width.max(0.001)).round().max(1.0);
            let whole_height = (height as f64 / crop.height.max(0.001)).round().max(1.0);
            config = config
                .set_target_size(whole_width as i32, whole_height as i32)
                .set_origin(
                    -(crop.x * whole_width).round() as i32,
                    -(crop.y * whole_height).round() as i32,
                );
        }
        config = config.rotate(
            match view.rotation {
                90 => PdfPageRenderRotation::Degrees90,
                180 => PdfPageRenderRotation::Degrees180,
                270 => PdfPageRenderRotation::Degrees270,
                _ => PdfPageRenderRotation::None,
            },
            false,
        );
        let began = Instant::now();
        page.render_into_bitmap_with_config(&mut bitmap, &config)
            .map_err(|e| format!("page {index}: {e}"))?;
        let drew_in = began.elapsed().as_secs_f64() * 1000.0;
        drop(bitmap);

        take(Bitmap {
            width,
            height,
            bgra: &held.scratch,
            drew_in,
        });
        Ok(())
    }
}

/// How far down its page a destination sits, as a fraction of the page's
/// height.
///
/// `/XYZ` and the `Fit*` forms that name a top edge say where on the page they
/// mean; everything else means the top. pdfium answers all of them through one
/// call, so the six view settings collapse to "is there a y, and is a y what
/// this form means".
///
/// Clamped at 0.95: a destination at the very bottom of a page scrolls that
/// page out of the window, and the reader lands looking at the next one.
fn offset_within(destination: &PdfDestination, space: &crate::markup::Space) -> f64 {
    use PdfDestinationViewSettings as View;
    let (left, top) = match destination.view_settings() {
        Ok(View::SpecificCoordinatesAndZoom(x, Some(y), _)) => (x, y),
        Ok(View::FitPageHorizontallyToWindow(Some(y))) => (None, y),
        Ok(View::FitBoundsHorizontallyToWindow(Some(y))) => (None, y),
        _ => return 0.0,
    };
    // pdfium counts from the bottom of the page's box, before `/Rotate`, as
    // it does everywhere else here; `Space` is the one conversion.
    space
        .fraction_down(left.map(|x| x.value as f64), top.value as f64)
        .clamp(0.0, 0.95)
}

/// A page's characters and their boxes. See [`PageSource::text_of`].
fn read_text(page: &PdfPage) -> PageText {
    // pdfium counts from the bottom of the page — before `/Rotate`, from
    // wherever its box begins — and the layout counts from the top of the
    // page as drawn. See [`crate::markup::Space`].
    let space = crate::markup::Space::of(page);
    let Ok(text) = page.text() else {
        return PageText::default();
    };
    let chars = text.chars();
    let mut out = PageText {
        chars: Vec::with_capacity(chars.len()),
        boxes: Vec::with_capacity(chars.len()),
    };
    for character in chars.iter() {
        let Some(value) = character.unicode_char() else {
            continue;
        };
        let glyph = character
            .loose_bounds()
            .map(|rect| space.down(&rect))
            .unwrap_or(Rect {
                left: 0.0,
                top: 0.0,
                width: 0.0,
                height: 0.0,
            });
        out.chars.push(value);
        out.boxes.push(glyph);
    }
    out
}

/// The numbers printed on the pages, when the file does not say what they are.
///
/// A journal offprint is numbered 407 to 425 on the paper and carries no
/// `/PageLabels` at all, so every viewer calls its first page 1 — and a reader
/// with a citation in hand types 412 and lands nowhere. The number is on the
/// page, so it is read off the page: a sample of them (the crop's sample, for
/// the crop's reason), a whole number standing alone in the top or bottom
/// band of each, and the offset from the position that most of them agree on.
///
/// Cautious, because a wrong answer here renames every page: at least three
/// pages and more than half the sample have to agree, the first page has to
/// come out at 1 or more, and an offset that says "1 to n" is the file's own
/// silence again. A book whose body starts again at 1 after its front matter
/// fails the second test, and rightly — its front matter has no number here
/// to give it.
fn printed_numbering(document: &PdfDocument, pages: usize) -> Vec<String> {
    let samples: Vec<(usize, Vec<usize>)> = crate::crop::sample(pages)
        .into_iter()
        .filter_map(|index| {
            let page = document.pages().get(index as i32).ok()?;
            let height = page.height().value as f64;
            Some((index, numbers_in_margins(&read_text(&page), height)))
        })
        .collect();
    match agreed_first_number(&samples, pages) {
        Some(first) => (0..pages)
            .map(|index| (first + index).to_string())
            .collect(),
        None => Vec::new(),
    }
}

/// How much of a page, top and bottom, a running number lives in.
const MARGIN_BAND: f64 = 0.12;

/// Every whole number standing alone — whitespace either side — in the top
/// or bottom band of a page.
fn numbers_in_margins(text: &PageText, height: f64) -> Vec<usize> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < text.chars.len() {
        if text.chars[at].is_whitespace() {
            at += 1;
            continue;
        }
        let from = at;
        while at < text.chars.len() && !text.chars[at].is_whitespace() {
            at += 1;
        }
        let word: String = text.chars[from..at].iter().collect();
        let Ok(number) = word.parse::<usize>() else {
            continue;
        };
        let glyph = text.boxes[from];
        if glyph.height <= 0.0 {
            continue;
        }
        let middle = glyph.top + glyph.height / 2.0;
        if middle < height * MARGIN_BAND || middle > height * (1.0 - MARGIN_BAND) {
            found.push(number);
        }
    }
    found
}

/// The number the first page is printed with, if enough of the sample says
/// the same thing — see [`printed_numbering`] for the rules.
fn agreed_first_number(samples: &[(usize, Vec<usize>)], pages: usize) -> Option<usize> {
    let mut votes: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (index, numbers) in samples {
        // One vote per page, whatever else is printed in its margins.
        let mut said: Vec<usize> = numbers
            .iter()
            .filter_map(|number| number.checked_sub(*index))
            .filter(|first| *first >= 1)
            .collect();
        said.sort_unstable();
        said.dedup();
        for first in said {
            *votes.entry(first).or_default() += 1;
        }
    }
    let (first, count) = votes
        .into_iter()
        .max_by_key(|(first, count)| (*count, std::cmp::Reverse(*first)))?;
    (count >= 3 && count * 2 > samples.len() && first > 1 && first + pages < 100_000)
        .then_some(first)
}

/// The labels, unless they say nothing.
///
/// See [`PageSource::labels`]: a document that labels its pages 1 to n has
/// said exactly what the position already said, and a list of those is a list
/// every lookup above would run for no reason. A document with no labels at
/// all comes back from pdfium as a list of empty strings and is the same case.
fn own_numbering(labels: Vec<String>) -> Vec<String> {
    let says_nothing = labels
        .iter()
        .enumerate()
        .all(|(index, label)| label.is_empty() || label == &(index + 1).to_string());
    if says_nothing {
        Vec::new()
    } else {
        labels
    }
}

/// The bookmark tree, flattened into the rows the sidebar draws.
///
/// Walked by hand rather than through `iter_all_descendants`, because a row
/// needs its depth and an iterator that flattens the tree has thrown it away.
///
/// Three refusals: a bookmark with no title is skipped, being a row nobody can
/// aim at; a destination that does not resolve leaves `page` at `None` rather
/// than dropping the row, the heading still being the document's own account of
/// itself; and the walk stops at `LIMIT` entries and `DEPTH` levels, because a
/// malformed document can point a bookmark at its own ancestor.
/// What the Information window lists, in the app's own order and under the
/// app's own labels — `showDocumentDetails` in `main.ts`. A field the document
/// does not name is left out rather than shown empty.
///
/// Dates are PDF date strings (`D:20240131120000+01'00'`), which nobody reads
/// as written; `readableDate` in the app turns them into a date, and so does
/// this.
fn read_details(
    document: &PdfDocument<'_>,
    first: Option<&Size>,
    path: &str,
) -> Vec<(String, String)> {
    let metadata = document.metadata();
    let tag = |which: PdfDocumentMetadataTagType| {
        metadata
            .get(which)
            .map(|tag| tag.value().trim().to_string())
            .filter(|value| !value.is_empty())
    };
    let mut out = Vec::new();
    let mut put = |label: &str, value: Option<String>| {
        if let Some(value) = value {
            out.push((label.to_string(), value));
        }
    };
    put("Author", tag(PdfDocumentMetadataTagType::Author));
    put("Subject", tag(PdfDocumentMetadataTagType::Subject));
    put("Keywords", tag(PdfDocumentMetadataTagType::Keywords));
    put("Pages", Some(document.pages().len().to_string()));
    put("Page size", first.map(page_size));
    put("Made with", tag(PdfDocumentMetadataTagType::Creator));
    put("Written by", tag(PdfDocumentMetadataTagType::Producer));
    // `Pdf1_7` is the variant's name and not a version anybody writes down.
    put(
        "PDF version",
        format!("{:?}", document.version())
            .strip_prefix("Pdf")
            .map(|number| number.replace('_', ".")),
    );
    put(
        "Created",
        tag(PdfDocumentMetadataTagType::CreationDate).map(|raw| readable_date(&raw)),
    );
    put(
        "Changed",
        tag(PdfDocumentMetadataTagType::ModificationDate)
            .or_else(|| mod_date(path))
            .map(|raw| readable_date(&raw)),
    );
    out
}

/// `/ModDate`, read out of the file, because pdfium will never be asked for it.
///
/// **`pdfium-render` asks pdfium for the wrong key.** `PdfMetadata::get` spells
/// it `"ModificationDate"` (0.9.3, `src/pdf/document/metadata.rs`) where the
/// specification's `/Info` key is `ModDate`, so the call returns a length of
/// zero and the tag reads as absent. Every other tag is spelled correctly,
/// which is what makes it quiet. `PdfDocument::handle()` is `pub(crate)`, so
/// the raw call cannot be made from out here either.
///
/// So the bytes are read. **Bounded, and from the end first**: an `/Info`
/// dictionary sits near the trailer in most documents, and necessarily in the
/// last update of one this reader has appended to. The front is tried after,
/// because a linearised document puts its first-page objects at the head. One
/// that keeps it in the middle gets no row, which is what it gets today.
fn mod_date(path: &str) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};

    /// As much of either end as is worth reading. An `/Info` dictionary is a
    /// few hundred bytes and sits beside the trailer.
    const WINDOW: u64 = 128 * 1024;

    let mut file = std::fs::File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    let mut read_at = |from: u64| -> Option<Vec<u8>> {
        file.seek(SeekFrom::Start(from)).ok()?;
        let mut bytes = vec![0u8; WINDOW.min(size - from) as usize];
        file.read_exact(&mut bytes).ok()?;
        Some(bytes)
    };

    let tail = read_at(size.saturating_sub(WINDOW))?;
    if let Some(found) = last_mod_date(&tail) {
        return Some(found);
    }
    if size <= WINDOW {
        return None;
    }
    last_mod_date(&read_at(0)?)
}

/// The last `/ModDate (…)` in a run of bytes, as it was written.
///
/// The *last*, because a document that has been appended to carries every
/// version of its `/Info` and only the newest one counts. A date written as a
/// hex string — `/ModDate <44 3a ...>`, which the format allows and no producer
/// uses — is left alone rather than guessed at; [`readable_date`] hands
/// anything it cannot read straight back, and this does the same by finding
/// nothing.
fn last_mod_date(bytes: &[u8]) -> Option<String> {
    const KEY: &[u8] = b"/ModDate";
    let mut found = None;
    let mut at = 0usize;
    while let Some(offset) = bytes[at..]
        .windows(KEY.len())
        .position(|window| window == KEY)
    {
        let after = at + offset + KEY.len();
        at = after;
        // Whitespace, then a literal string. Anything else is a key that merely
        // begins the same way, or a shape this does not read — and it is
        // skipped rather than given up on, because giving up would throw away a
        // date already found earlier in the window.
        let Some(open) = bytes[after..]
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .map(|at| at + after)
        else {
            break;
        };
        if bytes.get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = bytes[open + 1..].iter().position(|byte| *byte == b')') else {
            break;
        };
        let value = &bytes[open + 1..open + 1 + close];
        if let Ok(text) = std::str::from_utf8(value) {
            found = Some(text.to_string());
        }
    }
    found
}

/// A page's size, which is what the app shows: PDF points are 1/72 inch, and
/// nobody thinks in points.
///
/// **In both, because a reader knows their paper in one or the other** — the
/// sentence `Viewer.details` in `viewer.ts` is written under, and the half this
/// was missing. A4 is 210 × 297 mm to one reader and Letter is 8.5 × 11 in to
/// another, and a window that answers in millimetres alone makes the second of
/// them do arithmetic.
fn page_size(size: &Size) -> String {
    let mm = |points: f64| points * 25.4 / 72.0;
    let inches = |points: f64| points / 72.0;
    format!(
        "{:.0} × {:.0} mm ({:.2} × {:.2} in)",
        mm(size.width),
        mm(size.height),
        inches(size.width),
        inches(size.height)
    )
}

/// `D:20240131120000+01'00'` → `31 January 2024`. Anything that is not a PDF
/// date string is passed through as written, which is what the app does: a
/// value nobody can parse is still a value somebody put there.
fn readable_date(raw: &str) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let digits = raw.strip_prefix("D:").unwrap_or(raw);
    // `get`, not a slice: a non-ASCII character inside the first eight bytes
    // makes `[..8]` a boundary panic while opening the document.
    let Some(date) = digits
        .get(..8)
        .filter(|date| date.bytes().all(|b| b.is_ascii_digit()))
    else {
        return raw.to_string();
    };
    let year = &date[0..4];
    let month: usize = date[4..6].parse().unwrap_or(0);
    let day: usize = date[6..8].parse().unwrap_or(0);
    let Some(name) = MONTHS.get(month.wrapping_sub(1)) else {
        return raw.to_string();
    };
    if !(1..=31).contains(&day) {
        return raw.to_string();
    }
    // **And the time, where the document wrote one.** `readableDate` in
    // `main.ts` asks for `dateStyle: "long"` with `timeStyle: "short"` beside
    // it whenever the hour is there, and the difference is not cosmetic: two
    // drafts of a paper compiled the same afternoon are one row and the same
    // row without it. What is deliberately *not* copied is the locale — that is
    // `Intl` doing the work, and there is no `Intl` here — so the month is
    // named in English, as every other date in this crate already is, and the
    // clock is 24-hour.
    let clock = digits
        .get(8..12)
        .filter(|time| time.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|time| {
            let hour: usize = time[..2].parse().unwrap_or(24);
            let minute: usize = time[2..].parse().unwrap_or(60);
            (hour < 24 && minute < 60).then(|| format!(", {hour:02}:{minute:02}"))
        });
    format!("{day} {name} {year}{}", clock.unwrap_or_default())
}

fn read_outline(document: &PdfDocument<'static>) -> Vec<Heading> {
    /// As many rows as anybody will ever scroll through, and few enough that
    /// a cycle cannot cost anything.
    const LIMIT: usize = 20_000;
    /// The PDF specification sets no limit on nesting and no real document
    /// goes past a handful.
    const DEPTH: usize = 16;

    let bookmarks = document.bookmarks();
    // `root()` is the *first top-level bookmark*, not a node above them all,
    // so the top level of the outline is that bookmark and its siblings.
    let mut top = Vec::new();
    let mut next = bookmarks.root();
    while let Some(bookmark) = next {
        next = bookmark.next_sibling();
        top.push(bookmark);
        if top.len() >= LIMIT {
            break;
        }
    }

    let mut headings = Vec::new();
    // Depth-first. Children are pushed in reverse so that popping them comes
    // back to reading order, and the level above is pushed in reverse for the
    // same reason.
    let mut stack: Vec<_> = top.into_iter().rev().map(|node| (node, 0usize)).collect();
    while let Some((bookmark, depth)) = stack.pop() {
        if headings.len() >= LIMIT {
            break;
        }
        let title = bookmark.title().unwrap_or_default().trim().to_string();
        // A destination of its own first, then the one its action carries:
        // most bookmarks have the first, and a bookmark written as a GoTo
        // action has only the second.
        let action = bookmark.action();
        let page = bookmark
            .destination()
            .and_then(|destination| destination.page_index().ok())
            .or_else(|| {
                action
                    .as_ref()?
                    .as_local_destination_action()?
                    .destination()
                    .ok()?
                    .page_index()
                    .ok()
            })
            .map(|index| index as usize + 1);
        if !title.is_empty() {
            headings.push(Heading { title, depth, page });
        }
        // Only children, never siblings: `iter_direct_children` already walks
        // the sibling chain under a node, so following a sibling here as well
        // is how every entry but the first of its level gets listed twice —
        // which is exactly what the first version of this did.
        if depth + 1 < DEPTH {
            let children: Vec<_> = bookmark.iter_direct_children().collect();
            for child in children.into_iter().rev() {
                stack.push((child, depth + 1));
            }
        }
    }
    headings
}

#[cfg(test)]
mod tests {
    use super::{agreed_first_number, readable_date};

    #[test]
    fn a_number_the_sample_agrees_on_names_the_first_page() {
        // An offprint: pages 0, 5, 10 and 18 printed 407, 412, 417 and 425,
        // with the year in the running head of each.
        let offprint = vec![
            (0, vec![2011, 407]),
            (5, vec![2011, 412]),
            (10, vec![2011, 417]),
            (18, vec![2011, 425]),
        ];
        assert_eq!(agreed_first_number(&offprint, 19), Some(407));
        // The year alone agrees on nothing, because the offset moves.
        let heads = vec![(0, vec![2011]), (5, vec![2011]), (10, vec![2011])];
        assert_eq!(agreed_first_number(&heads, 19), None);
        // Numbered 1 to n on the paper: the file's own silence again.
        let plain = vec![(0, vec![1]), (5, vec![6]), (10, vec![11])];
        assert_eq!(agreed_first_number(&plain, 19), None);
        // Front matter, then a body that starts at 1: the first page would
        // come out below 1, so nothing is said.
        let book = vec![(0, vec![]), (5, vec![2]), (10, vec![7]), (15, vec![12])];
        assert_eq!(agreed_first_number(&book, 19), None);
        // Two pages agreeing is a coincidence, not a numbering.
        let thin = vec![(0, vec![407]), (5, vec![412]), (10, vec![]), (18, vec![])];
        assert_eq!(agreed_first_number(&thin, 19), None);
    }

    #[test]
    fn a_date_that_is_not_ascii_is_passed_through_rather_than_panicking() {
        assert_eq!(readable_date("D:2024０131120000"), "D:2024０131120000");
        assert_eq!(readable_date("D:20240131é0"), "31 January 2024");
        assert_eq!(
            readable_date("D:20240131120000+01'00'"),
            "31 January 2024, 12:00"
        );
    }
}
