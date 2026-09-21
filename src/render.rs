//! The renderer, behind the one door it will always be behind.
//!
//! One trait — draw a page, ask how big it is, and later its text, its
//! outline, its links, its labels and its title — so that pdfium is a decision
//! that can be remade rather than a dependency spread through the viewer. It
//! is the rule `viewer.ts` obeys by being the only file that imports pdf.js,
//! and it is what would make `hayro` a swap rather than a rewrite.
//!
//! Each question was added when something asked, because a trait method with
//! no caller is a guess about what the caller will want, and every one but the
//! first two has a default answering "nothing" — a renderer that cannot say is
//! not a renderer this reader refuses to run over.

use std::sync::Arc;

use crate::layout::{Size, View};

/// One page's pixels, as pdfium hands them over: BGRA, top row first.
///
/// BGRA rather than RGBA on purpose: the swizzle is free on the GPU, the
/// texture being uploaded as `Bgra8Unorm` and read by the recolouring shader
/// anyway. On the CPU it was 1.6ms a page at 3.3 megapixels and 5.1ms at 10.1.
///
/// **Borrowed, not owned, and that is the whole point.** A page is 24MB, and
/// returning a `Vec` meant three copies alive at once — pdfium's own buffer,
/// the `Vec` `as_raw_bytes()` copies it into, and the `to_vec()` on top —
/// allocated and freed per page. macOS's allocator does not hand large freed
/// blocks back, so they showed as 120MB of `MALLOC_LARGE (empty)` on a reader
/// holding 46MB of actual page.
pub struct Bitmap<'a> {
    pub width: u32,
    pub height: u32,
    pub bgra: &'a [u8],
    /// What the renderer spent drawing it, in milliseconds.
    pub drew_in: f64,
}

/// One line of a document's own table of contents.
///
/// Flat, with a depth, rather than a tree of children — which is what
/// `buildOutline` in `sidebar.ts` walks a tree to produce, and it produces
/// exactly this: a row, indented by its depth, that goes to a page. Nothing
/// above this ever asks an entry for its children, so the tree is flattened
/// where it is read rather than carried up to be flattened again.
///
/// `page` is one-based, and `None` for an entry whose destination the
/// document does not actually resolve — a broken outline is common enough
/// that it is a row you cannot click rather than a row that is not there.
#[derive(Clone, Debug, PartialEq)]
pub struct Heading {
    pub title: String,
    pub depth: usize,
    pub page: Option<usize>,
}

/// A rectangle on a page, in the space everything above the renderer works in.
///
/// **One rectangle type, three things that are rectangles**: a character's
/// cell, a match's quad, and the area a link is clicked in.
///
/// In **PDF points with the origin at the top left**, which is
/// [`crate::layout`]'s space rather than the PDF's own — multiply by a
/// [`crate::layout::PageBox`]'s `scale` and it is where the thing goes on
/// screen. pdfium counts from the bottom, and the conversion belongs on the
/// side that knows the page height rather than in every caller.
///
/// **The per-character half is what pdf.js could not give, and why the search
/// here is half the size of the app's.** pdf.js hands over *runs*, so
/// `search.ts` folds them into one string, keeps a `starts[]`, binary-searches
/// it back to a run and an offset, and hands the pair to the DOM to be measured
/// against a text layer. pdfium answers per character
/// (`FPDFText_GetLooseCharBox`), so a match is a range of characters and that is
/// already a list of rectangles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// Where one of the document's own links goes.
///
/// Two cases and no third, which is the same division `onExternalLink` in
/// `main.ts` makes and for the same reason: opening a link is either a thing
/// this window does or a thing the system does, and nothing else in a PDF is
/// worth following. A `/Launch` action naming a program to run, which the
/// format also allows, is neither — it is not carried here at all, so it
/// cannot be followed by accident.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Out of the document: an address for the system to open.
    Away(String),
    /// Somewhere in this document: a page, one-based, and how far down it as
    /// a fraction of its height. Zero is the top of the page, which is what a
    /// destination naming no position means.
    Place { page: usize, offset: f64 },
}

/// One of the document's own links: an area on the page, and where it goes.
///
/// The area is in the page's own points ([`Rect`]) rather than in fractions
/// of it, which is where `viewer.ts` keeps them. The app's reason for
/// fractions is that its link layer is a DOM overlay sized in percentages, so
/// it survives a zoom without being rebuilt; here the overlay is rebuilt from
/// the layout on every frame anyway — the page's box *is* the render — so
/// points are one multiplication away from pixels and a fraction would be two.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub rect: Rect,
    pub target: Target,
}

/// A note somebody left in the document: an area on the page, and what it
/// says.
///
/// Any annotation with words in it, whatever its subtype, because a comment on
/// a highlight and a sticky note are the same thing to a reader. Links are the
/// exception, their text being where they go, and a Popup is the box another
/// annotation's words are shown in.
///
/// The area is in the page's own points, like a [`Link`]'s and for its
/// reason. `icon` is the app's own judgement: a note that is small in both
/// directions is a marker and can be pressed anywhere on it, and one that is
/// a passage of text is a comment on a highlighted sentence — pressing that
/// would put the sentence underneath out of reach of a pointer that wants to
/// select it, so only a strip at its right edge answers.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub rect: Rect,
    pub icon: bool,
    /// Who left it, or empty where the document does not say.
    pub by: String,
    pub text: String,
}

/// A page's text, and where every character of it is.
///
/// `chars` and `boxes` are the same length and are indexed together — which is
/// the whole of the data structure, and is why `search.rs` can be about
/// searching.
#[derive(Clone, Debug, Default)]
pub struct PageText {
    pub chars: Vec<char>,
    pub boxes: Vec<Rect>,
}

impl PageText {
    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// The characters `from..to` as rectangles, one per line rather than one
    /// per character.
    ///
    /// A match is drawn as a few boxes and not as ninety, and the join is done
    /// here because it is the same question `joinRuns` in `viewer.ts` answers
    /// for the same reason: pdf.js's spans do not abut, so the gaps between
    /// them show as white rules through a highlighted sentence. Characters
    /// abut rather better than spans do, and the rule still holds — a run is
    /// extended while the next character sits on the same line, and a new one
    /// begins when it does not.
    pub fn quads(&self, from: usize, to: usize) -> Vec<Rect> {
        let mut quads: Vec<Rect> = Vec::new();
        for index in from..to.min(self.boxes.len()) {
            let glyph = self.boxes[index];
            // A character with no size is a space pdfium generated rather than
            // one the printer drew, and it would otherwise stretch a run to
            // the far edge of the page.
            if glyph.width <= 0.0 || glyph.height <= 0.0 {
                continue;
            }
            match quads.last_mut() {
                // The same line, if the two overlap vertically by most of
                // their height — which is a looser test than "the same top",
                // because a line of type is full of characters that sit a
                // fraction high or low.
                Some(run)
                    if overlap(run.top, run.height, glyph.top, glyph.height) > 0.5
                        && glyph.left + glyph.width > run.left
                        && glyph.left < run.left + run.width + glyph.height =>
                {
                    let right = (run.left + run.width).max(glyph.left + glyph.width);
                    let bottom = (run.top + run.height).max(glyph.top + glyph.height);
                    run.left = run.left.min(glyph.left);
                    run.top = run.top.min(glyph.top);
                    run.width = right - run.left;
                    run.height = bottom - run.top;
                }
                _ => quads.push(glyph),
            }
        }
        quads
    }
}

/// How much of the shorter of two vertical spans the two share, as a fraction.
fn overlap(top: f64, height: f64, other_top: f64, other_height: f64) -> f64 {
    let shared = (top + height).min(other_top + other_height) - top.max(other_top);
    let shortest = height.min(other_height);
    if shortest <= 0.0 {
        0.0
    } else {
        (shared / shortest).max(0.0)
    }
}

/// What a document is, to everything above it.
pub trait PageSource: Send + Sync {
    fn pages(&self) -> usize;
    /// A page's size in PDF points, which is what the layout works in.
    fn size_of(&self, index: usize) -> Size;
    /// Draw one page at exactly this many pixels, and lend the pixels to
    /// `take` for as long as it wants them.
    ///
    /// A callback rather than a return value because the buffer is the
    /// renderer's and is reused — see [`Bitmap`].
    ///
    /// `view` says how the page is to be turned and how much of it to draw,
    /// and the pixels handed over are `width`×`height` of *that*. A renderer
    /// that cannot turn or crop is not a renderer this reader can use, so it
    /// is an argument rather than something with a default: a page drawn
    /// whole into a box shaped for a cropped one is a picture that is subtly
    /// and permanently wrong, which is the failure a default would buy.
    fn render(
        &self,
        index: usize,
        width: u32,
        height: u32,
        view: View,
        take: &mut dyn FnMut(Bitmap),
    ) -> Result<(), String>;
    /// Where the document was opened from.
    ///
    /// Not a rendering question, and it is here because it is the only thing
    /// that identifies one document from another for anything that has to
    /// write it down — the library keys its entries by path, and the reader
    /// asks the document rather than being told twice. Empty for a document
    /// that came from nowhere in particular, which nothing here produces yet
    /// and a document handed over as bytes one day would.
    fn path(&self) -> &str {
        ""
    }

    /// The document's own table of contents, in reading order, flattened.
    ///
    /// Empty when there is none, which is most documents — and the sidebar
    /// says so in as many words rather than showing an empty column.
    fn outline(&self) -> Vec<Heading> {
        Vec::new()
    }

    /// A page's text, and where every character of it sits.
    ///
    /// Empty for a page with nothing on it, and empty for a *renderer* that
    /// cannot answer — which is not hypothetical: hayro, the pure-Rust
    /// renderer the assessment names as the one to watch, has no text
    /// extraction at all. So this has a default, and a reader over a renderer
    /// with no text in it finds nothing rather than failing to build. What
    /// says so on the screen is [`crate::search::State::textless`], which the
    /// app already needed for a scan nobody put through OCR.
    fn text_of(&self, _index: usize) -> PageText {
        PageText::default()
    }

    /// The links on one page, in the order the document lists them.
    ///
    /// Asked per page and asked late, like the text: a document of typeset
    /// mathematics has hundreds of cross-references on a page and there is no
    /// reason to resolve any of them until the page is on screen.
    ///
    /// Empty for a renderer that cannot answer, for the reason [`Self::text_of`]
    /// has a default: a reader over one still reads.
    fn links_of(&self, _index: usize) -> Vec<Link> {
        Vec::new()
    }

    /// The notes on one page, in the order the document lists them.
    ///
    /// Asked per page and asked late, exactly as the links are — and empty
    /// for a renderer that cannot answer, for the reason [`Self::text_of`]
    /// has a default.
    fn notes_of(&self, _index: usize) -> Vec<Note> {
        Vec::new()
    }

    /// What the document calls its own pages, one per page, or empty.
    ///
    /// A book's front matter is numbered i, ii, iii and its body starts again
    /// at 1, so the twelfth page of the file is page xii and page 314 of the
    /// index is not the 314th thing in the file. A reader typing a number off
    /// a citation means the printed one.
    ///
    /// **Empty means "this document numbers its pages 1 to n"**, which is what
    /// the great majority do and is the same thing `readLabels` in `viewer.ts`
    /// decides by dropping a list that merely restates the position. A list
    /// that says nothing is worse than no list: everything above would carry
    /// it, look every page up in it, and get back the number it started with.
    fn labels(&self) -> Vec<String> {
        Vec::new()
    }

    /// What the document calls itself, or empty when it says nothing.
    ///
    /// `2310.06825v3.pdf` is not a name and a shelf of them is unreadable, but
    /// whatever produced the file usually wrote a title into it. This is that
    /// string exactly as the document gives it — whether it is *worth* calling
    /// the document is a judgement rather than a rendering question, and it is
    /// made once in [`crate::store::worth_calling`] where the file name it has
    /// to be weighed against is also known.
    ///
    /// Empty for a renderer that cannot answer, for the reason
    /// [`Self::labels`] has a default: the file name is what the library
    /// falls back to and is what it held before this question existed.
    fn title(&self) -> String {
        String::new()
    }

    /// What the document says about itself: the fields its metadata carries,
    /// in the order the app's Information window lists them, and the size of
    /// its first page. Anything the document does not name is left out rather
    /// than shown empty, which is what `showDocumentDetails` does.
    ///
    /// Empty for a renderer that cannot answer — the window then has only the
    /// name, the page count and the path, which are the reader's own.
    fn details(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Every highlight in the document, in reading order.
    ///
    /// The whole document rather than a page at a time, unlike the links and
    /// the text beside it, and for the reason the outline is read at open:
    /// what this answers is a *list* — the panel shows every mark in the book
    /// — and a list assembled page by page is a list that cannot be shown
    /// until the last page has been asked. It is a page load each, which is
    /// what the sizes and the labels already cost at open.
    ///
    /// Empty for a renderer that cannot answer, which is the default here for
    /// the reason [`Self::text_of`] has one — and a reader over one still
    /// reads, it simply cannot mark.
    fn markup(&self) -> Vec<crate::markup::Mark> {
        Vec::new()
    }

    /// Every signature in the document, in reading order.
    ///
    /// The neighbour of [`Self::markup`] and read the same way, for the same
    /// reason: a list has to be assembled before it can be shown. What it
    /// answers is where each one sits and how big it is, which is all the
    /// interface needs — the strokes themselves are the document's business
    /// once they are in it, and this reader draws none of them.
    ///
    /// Empty for a renderer that cannot answer, and a reader over one still
    /// reads.
    fn signatures(&self) -> Vec<crate::sign::Placed> {
        Vec::new()
    }

    /// Let go of the file, because something is about to write to it.
    ///
    /// **This exists because pdfium reads a page when the page is asked
    /// for**, not when the document is loaded, so the file stays open for as
    /// long as the document does. That is the right arrangement for reading —
    /// it is the same lazy read the app gets out of `read_range`, arrived at
    /// from the other end — and it is the one thing standing between this
    /// reader and writing a highlight into the document it is showing.
    /// Renaming a file over one that is held open is refused on Windows, and
    /// truncating it is refused there too, because the handle pdfium opens
    /// does not grant either.
    ///
    /// So the write path lets go first and reopens immediately afterwards:
    /// [`crate::app::Viewer::mark_selection`] releases, writes, and reloads
    /// the document through the path a recompile already uses. A released
    /// document renders nothing and answers nothing, which is safe rather
    /// than merely tolerable — every caller of every method here already
    /// handles a page it cannot have.
    ///
    /// The app needs none of this: the file it holds open is held open by
    /// Rust's own `File`, which shares deletion and writing with anybody
    /// else, and the bytes it writes came from the worker rather than from
    /// the handle.
    fn release(&self) {}

    /// Whether this document is encrypted — behind a password, or under an
    /// owner password alone, which opens without one.
    ///
    /// Asked for one reason: **markup must not go into an encrypted file**.
    /// pdfium's only way to write a document back is `FPDF_SaveAsCopy`, which
    /// is a full rewrite, and what comes out of it for an encrypted document
    /// is not a question worth guessing at over somebody's file.
    /// The app refuses the same case for its own reason, and the mark goes
    /// beside the document instead — which is what the journal is for. See
    /// [`crate::markup::Standing`].
    fn encrypted(&self) -> bool {
        false
    }

    /// The password it was opened with, so that a reload can open it again.
    fn password(&self) -> Option<&str> {
        None
    }

    /// Whether the document is digitally signed — a signature with bytes in
    /// it, which is what a rewrite breaks. See `markup::standing`.
    fn sealed(&self) -> bool {
        false
    }

    /// Take the file up again after [`PageSource::release`], for a write
    /// that failed and left nothing new to reopen.
    fn retake(&self) {}

    /// What opening the document cost, in milliseconds — the other half of the
    /// comparison with pdf.js, which spends most of a document open starting
    /// its worker.
    fn opened_in(&self) -> f64;
}

/// Why a document would not open.
///
/// **Two arms, and the whole point is the first one.** Everything a reader can
/// do about a document that will not open is nothing — the file is missing,
/// the bytes are not a PDF, the library did not load — and for all of those a
/// sentence in the notice line is the whole of the answer. A document that is
/// *locked* is the one case where there is something for them to do, so it is
/// the one case the type distinguishes. Matching on English in an error
/// message would work today and stop working the day pdfium rewords one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// It wants a password — or the one it was given was not right, which
    /// pdfium reports identically and the caller tells apart by knowing
    /// whether it supplied one.
    Locked,
    /// Anything else, already said in a sentence.
    Said(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Said in full here because this is what the terminal prints at
            // launch, where there is no window to ask in.
            Refusal::Locked => write!(f, "That document is locked. It needs a password."),
            Refusal::Said(said) => write!(f, "{said}"),
        }
    }
}

/// A document, opened by whichever renderer this build carries.
pub fn open(path: &str) -> Result<Arc<dyn PageSource>, Refusal> {
    open_with(path, None)
}

/// The same, with the password for a document that wants one.
pub fn open_with(path: &str, password: Option<&str>) -> Result<Arc<dyn PageSource>, Refusal> {
    Ok(Arc::new(crate::pdfium::Document::open_with(
        path, password,
    )?))
}

/// A window with nothing in it.
///
/// **The start screen is the reason this exists**, and it is the cheaper of
/// the two ways to have one. The other is `Option<Arc<dyn PageSource>>`
/// threaded through the viewer, the layout, the search, the sidebar and every
/// component under them — several hundred `if let` arms whose every branch is
/// "there is no document, do nothing", which is what a document of no pages
/// already says. Here `pages()` is 0, the layout has no boxes, the mounting
/// window holds nothing, `page()` is 0, the search finds nothing and the
/// sidebar has nothing to draw. Everything above carries on being written for
/// a document, and one predicate — [`crate::app::Viewer::empty`] — decides
/// what is on the screen.
///
/// It is also the honest shape. A window that is showing nothing is not a
/// window whose renderer is absent; the renderer is right there and the answer
/// to every question is that there is nothing to say.
pub struct Nothing;

impl PageSource for Nothing {
    fn pages(&self) -> usize {
        0
    }

    /// Never asked, there being no page to ask about — and US Letter rather
    /// than zero because a size of nothing is the shape that divides by zero
    /// somewhere downstream if the first half of that sentence ever stops
    /// being true.
    fn size_of(&self, _index: usize) -> Size {
        Size {
            width: 612.0,
            height: 792.0,
        }
    }

    fn render(
        &self,
        _index: usize,
        _width: u32,
        _height: u32,
        _view: View,
        _take: &mut dyn FnMut(Bitmap),
    ) -> Result<(), String> {
        Err("there is no document open".to_string())
    }

    fn opened_in(&self) -> f64 {
        0.0
    }
}

/// The document a window shows when it is showing none.
pub fn nothing() -> Arc<dyn PageSource> {
    Arc::new(Nothing)
}
