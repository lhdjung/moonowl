//! Documents written by hand, for the tests that need a shape the app's own
//! fixtures do not have.
//!
//! Everything here is written in Rust for one reason, and it is the reason
//! `PROGRESS.md` gives for wanting this suite on three platforms: `cargo test`
//! should need cargo and nothing else. A fixture that needs Node to exist
//! first is a test that does not run on a machine which has not built the app
//! — which is exactly how it failed. `tests/fixtures/make-pdf.mjs` wrote the
//! four hundred page book, a step in two workflows called it, the script went
//! with the port, and every bundle job then died on a missing module.
//! [`book_pdf`] is that document, written here.
//!
//! The writer is deliberately the dullest possible one — uncompressed streams,
//! one object per line of the body, a real `xref` table computed from the byte
//! offsets — because what is being tested is the reader, and a fixture that
//! needs debugging is a fixture that has stopped being evidence.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A body of PDF objects, numbered from 1, and the file they make.
///
/// The objects are bytes rather than `String`s, and only one fixture needs
/// them to be: an encrypted document's streams are RC4 over arbitrary bytes,
/// and there is no encoding under which those are text. Everything else here
/// still writes a `String` and is converted on the way in.
struct Pdf {
    objects: Vec<Vec<u8>>,
    /// The `/Info` dictionary, if this document has one. Named in the trailer
    /// rather than reachable from the catalogue, which is where a PDF keeps
    /// the title it calls itself by and is why it is a field here.
    info: Option<usize>,
    /// The `/Encrypt` dictionary, if this document has one, and the file
    /// identifier the key was derived from. Both live in the trailer, and
    /// neither is reachable from the catalogue — which is the whole reason a
    /// reader can tell a locked document from a corrupt one without a
    /// password: the *structure* is in the clear and only the contents are
    /// not. See [`locked_pdf`].
    encrypt: Option<usize>,
    id: Option<[u8; 16]>,
}

impl Pdf {
    fn new() -> Pdf {
        Pdf {
            objects: Vec::new(),
            info: None,
            encrypt: None,
            id: None,
        }
    }

    /// Reserve an object number without saying yet what is in it, which is
    /// what a tree of cross-references needs: an outline item names its
    /// parent, its siblings and its page, and half of them are written after
    /// it.
    fn reserve(&mut self) -> usize {
        self.objects.push(Vec::new());
        self.objects.len()
    }

    fn put(&mut self, id: usize, body: impl Into<Vec<u8>>) {
        self.objects[id - 1] = body.into();
    }

    fn add(&mut self, body: impl Into<Vec<u8>>) -> usize {
        let id = self.reserve();
        self.put(id, body);
        id
    }

    /// The whole file: header, body, `xref` and trailer.
    fn bytes(&self) -> Vec<u8> {
        let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::with_capacity(self.objects.len());
        for (index, body) in self.objects.iter().enumerate() {
            offsets.push(out.len());
            let _ = writeln!(out, "{} 0 obj", index + 1);
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        let _ = write!(
            out,
            "xref\n0 {}\n0000000000 65535 f \n",
            self.objects.len() + 1
        );
        for offset in &offsets {
            let _ = writeln!(out, "{offset:010} 00000 n ");
        }
        let info = match self.info {
            Some(id) => format!(" /Info {id} 0 R"),
            None => String::new(),
        };
        // `/Encrypt` and `/ID` travel together: the identifier is one of the
        // four things the encryption key is derived from, so a file that
        // names one and not the other cannot be opened by anybody, with the
        // password or without it.
        let locked = match (self.encrypt, self.id) {
            (Some(id), Some(bytes)) => format!(" /Encrypt {id} 0 R /ID [<{0}> <{0}>]", hex(&bytes)),
            _ => String::new(),
        };
        let _ = write!(
            out,
            "trailer\n<< /Size {} /Root 1 0 R{info}{locked} >>\nstartxref\n{}\n%%EOF\n",
            self.objects.len() + 1,
            xref,
        );
        out
    }
}

/// A fixture on disk: rewritten only when it changed, and written so that two tests
/// asking for it at the same moment both get the whole of it.
///
/// **Both halves of that are load-bearing, and the second one was wrong.** The
/// rename is what `atomic_write` does and for its reason — a reader must never
/// see half a file — but the temporary was named for the *process*, and
/// `cargo test` runs its tests as threads of one process. Two tests wanting a
/// fixture neither had yet wrote the same temporary and both renamed it: the
/// first succeeded, the second failed with `NotFound` on a file it had just
/// written. A counter is what makes the name a thread's rather than a
/// process's; the rename itself is already atomic, so whichever lands last
/// wins and both are the same bytes.
fn written(name: &str, build: impl FnOnce() -> Vec<u8>) -> String {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path: PathBuf = std::env::temp_dir().join(name);
    // Built every time and compared, not trusted by name: a fixture edited in
    // this file was otherwise the old one until somebody emptied the temp
    // directory by hand, and the test written against the edit passed on it.
    let bytes = build();
    if std::fs::read(&path).ok().as_deref() != Some(&bytes[..]) {
        // A name may carry a directory — `book.pdf` is written under one so
        // that it keeps the name the parity fixture measured, which is the
        // name the toolbar shows.
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let temp = path.with_extension(format!(
            "{}.{}.part",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&temp, &bytes).expect("write the fixture");
        std::fs::rename(&temp, &path).expect("put the fixture in place");
    }
    // As the door hands a path over: on Windows `join` leaves the `/` in the
    // name as it is, `config::absolute` turns it round, and a document opened
    // twice under two spellings was two rows in the library.
    crate::config::absolute(&path.to_string_lossy())
}

/// One line of the table of contents a fixture is asked for: a title, the
/// page it goes to, and the entries under it.
pub struct Section {
    pub title: &'static str,
    pub page: usize,
    pub under: &'static [Section],
}

const fn section(title: &'static str, page: usize, under: &'static [Section]) -> Section {
    Section { title, page, under }
}

/// The contents the fixture carries, and therefore what a test asserts on:
/// seven rows over three levels, in this order.
pub const CONTENTS: &[Section] = &[
    section("Front matter", 1, &[section("Preface", 2, &[])]),
    section(
        "Chapter One",
        3,
        &[
            section("A section", 4, &[section("Under a section", 5, &[])]),
            section("Another section", 6, &[]),
        ],
    ),
    section("Chapter Two", 8, &[]),
    section("Index", 11, &[]),
];

/// The same list flattened the way [`crate::render::Heading`] flattens it:
/// title, depth, page.
pub fn expected_headings() -> Vec<(String, usize, usize)> {
    fn walk(into: &mut Vec<(String, usize, usize)>, sections: &[Section], depth: usize) {
        for section in sections {
            into.push((section.title.to_string(), depth, section.page));
            walk(into, section.under, depth + 1);
        }
    }
    let mut flat = Vec::new();
    walk(&mut flat, CONTENTS, 0);
    flat
}

/// A twelve-page document that carries [`CONTENTS`] as its own outline.
///
/// Written once per process into the temp directory and reused, because
/// writing it is a millisecond and every test that wants it wants the same
/// bytes.
pub fn contents_pdf() -> String {
    written("moonowl-fixture-contents.pdf", || build(12))
}

/// A document of `pages` pages, written where you say, right now.
///
/// **The opposite of everything else in this file**, which is cached and shared
/// because two tests wanting the same fixture want the same bytes. This is what
/// a recompile looks like — a named file *rewritten* while a reader holds it
/// open — so it takes a path, writes every time, and each draft is a different
/// length so a test can tell them apart.
///
/// Through the app's own [`crate::atomic_write`], which is what `watch.rs`
/// expects to see: a compiler replaces a document by renaming another over it,
/// which is why the watch is on the directory.
pub fn draft(path: &std::path::Path, pages: usize) {
    crate::atomic_write(path, &build(pages)).expect("write the draft");
}

/// Three plain pages under a title the document gives itself.
///
/// The one shape none of the fixtures above has: an `/Info` dictionary with a
/// `/Title` in it, which is what `2310.06825v3.pdf` usually carries instead of
/// a name worth reading. Parameterised rather than fixed because the
/// interesting half of the feature is the *judgement* — a great many documents
/// name themselves "Microsoft Word - report.doc" or the file name over again,
/// and each of those is worse than the file name because it looks deliberate.
/// See [`crate::store::worth_calling`].
pub fn titled_pdf(title: &str) -> String {
    // The name on disk has to follow the title, or the second call would be
    // handed the first one's file. It is a digest rather than the title itself
    // because a title is somebody's prose and a file name is not.
    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in title.as_bytes() {
        digest ^= *byte as u64;
        digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let named = title.to_string();
    written(
        &format!("moonowl-fixture-titled-{digest:016x}.pdf"),
        move || build_titled(&named),
    )
}

/// One page the file itself turns, out of a box that does not begin at 0,0.
///
/// The two things every other fixture here leaves alone, and the two that
/// decide whether what pdfium says about a page — in the file's own space —
/// lands on the page as it is drawn. A `pdflscape` table is the first and a
/// journal's cropped offprint the second. See [`crate::markup::Space`].
pub fn turned_pdf(rotate: u32) -> String {
    written(&format!("moonowl-fixture-turned-{rotate}.pdf"), move || {
        let mut pdf = Pdf::new();
        let catalog = pdf.reserve();
        let tree = pdf.reserve();
        let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
        let stream = "BT /F1 36 Tf 100 600 Td (Turned) Tj ET";
        let content = pdf.add(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            stream
        ));
        let page = pdf.add(format!(
            "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
             /CropBox [36 48 576 756] /Rotate {rotate} \
             /Resources << /Font << /F1 {font} 0 R >> >> /Contents {content} 0 R >>"
        ));
        pdf.put(
            tree,
            format!("<< /Type /Pages /Count 1 /Kids [{page} 0 R] >>"),
        );
        pdf.put(catalog, format!("<< /Type /Catalog /Pages {tree} 0 R >>"));
        pdf.bytes()
    })
}

fn build_titled(title: &str) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let tree = pdf.reserve();
    let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    let page_ids: Vec<usize> = (0..3).map(|_| pdf.reserve()).collect();
    for (index, &id) in page_ids.iter().enumerate() {
        let stream = format!("BT /F1 18 Tf 72 700 Td (Page {}.) Tj ET", index + 1);
        let content = pdf.add(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            stream
        ));
        pdf.put(
            id,
            format!(
                "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
                 /Resources << /Font << /F1 {font} 0 R >> >> /Contents {content} 0 R >>"
            ),
        );
    }
    pdf.put(
        tree,
        format!(
            "<< /Type /Pages /Count 3 /Kids [{}] >>",
            page_ids
                .iter()
                .map(|id| format!("{id} 0 R"))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    );
    pdf.put(catalog, format!("<< /Type /Catalog /Pages {tree} 0 R >>"));
    // A literal string, so the three characters that end one early are the
    // three to escape.
    let escaped: String = title
        .chars()
        .flat_map(|c| match c {
            '(' | ')' | '\\' => vec!['\\', c],
            other => vec![other],
        })
        .collect();
    pdf.info = Some(pdf.add(format!("<< /Title ({escaped}) >>")));
    pdf.bytes()
}

/// Six pages of prose, written to exercise the *fold* through the renderer
/// rather than in isolation.
///
/// `search.rs` tests `fold` directly, which proves nothing about what pdfium
/// actually reports — and two of the three answers are not what the app sees:
///
/// * **A ligature comes back already split.** pdfium hands over "f" and "i",
///   with a box each, whatever the font says. See the `/Differences` font
///   below.
/// * **An accent comes back precomposed**, U+00E9 rather than "e" and a
///   combining mark, so the fold's decompose-and-drop is what makes "resume"
///   find "résumé". As it is in the app.
/// * **A soft hyphen comes back as a soft hyphen** — but only if the document
///   says so in a `/ToUnicode` map. Written as the byte 0255 in
///   WinAnsiEncoding it is an ordinary hyphen, because that is what the
///   encoding says code 0255 is, which took a probe to notice and is the
///   reason there is a third font here.
///
/// Two things about the file. Everything above ASCII is a PDF octal escape —
/// `\351` for é — because the bytes in a PDF string are the font's codes rather
/// than UTF-8. And the ligature has a font of its own: `/fi` is not in
/// WinAnsiEncoding, so it takes an `/Encoding` with `/Differences` and a
/// `/ToUnicode` map, which is the pair a real typesetter emits.
pub fn prose_pdf() -> String {
    written("moonowl-fixture-prose.pdf", build_prose)
}

/// What each page of [`prose_pdf`] says: a list of runs, each naming the font
/// it is set in.
///
/// `F1` is Helvetica in WinAnsiEncoding and is everything ordinary. `F2` and
/// `F3` exist for one character each, and which of them a character needs is
/// the interesting part — see [`prose_pdf`].
pub const PROSE: &[&[(&str, &str)]] = &[
    &[("F1", "A needle in the first page.")],
    &[("F1", "Nothing to look for on this one.")],
    &[("F1", "The needle again, and a needle beside it.")],
    &[("F1", "Type "), ("F2", r"\001nd"), ("F1", " to find it.")],
    &[("F1", r"Her r\351sum\351 is filed on this page.")],
    &[
        ("F1", "The word typo"),
        ("F3", r"\002"),
        ("F1", "graphy broke across a line."),
    ],
];

fn build_prose() -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let tree = pdf.reserve();
    let plain = pdf
        .add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>");
    // The ligature: one glyph, named in a `/Differences` array, drawn by one
    // byte in the content stream.
    //
    // **And pdfium hands it back as two characters, "f" and "i", with a box
    // each** — with a `/ToUnicode` saying U+FB01 and without one alike. So the
    // ligature half of `fold` has nothing to do on this renderer, where on
    // pdf.js it decides whether "find" is found in a typeset book. The fold
    // keeps it: hayro will not do pdfium's normalising, and a document can
    // carry U+FB01 by other routes.
    let ligature = pdf.add(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
         /Encoding << /Type /Encoding /BaseEncoding /WinAnsiEncoding \
         /Differences [1 /fi] >> >>",
    );
    // The soft hyphen, which needs a map for the opposite reason: in
    // WinAnsiEncoding the code 0255 *is* `hyphen`, so writing the byte gets an
    // ordinary hyphen back and the fold has nothing to do. A `/ToUnicode`
    // saying U+00AD is how a document actually carries one, and is what a
    // typesetter breaking a word across a line emits.
    let map = to_unicode(&mut pdf, "1 beginbfchar <02> <00ad> endbfchar");
    let soft = pdf.add(format!(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
         /Encoding << /Type /Encoding /BaseEncoding /WinAnsiEncoding \
         /Differences [2 /hyphen] >> /ToUnicode {map} 0 R >>"
    ));

    let page_ids: Vec<usize> = PROSE.iter().map(|_| pdf.reserve()).collect();
    for (index, &id) in page_ids.iter().enumerate() {
        let mut stream = String::from("BT 18 Tf 72 700 Td");
        for (font, text) in PROSE[index] {
            stream.push_str(&format!(" /{font} 18 Tf ({text}) Tj"));
        }
        stream.push_str(" ET");
        let content = pdf.add(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            stream
        ));
        pdf.put(
            id,
            format!(
                "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
                 /Resources << /Font << /F1 {plain} 0 R /F2 {ligature} 0 R \
                 /F3 {soft} 0 R >> >> /Contents {content} 0 R >>"
            ),
        );
    }
    pdf.put(
        tree,
        format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            page_ids.len(),
            page_ids
                .iter()
                .map(|id| format!("{id} 0 R"))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    );
    pdf.put(catalog, format!("<< /Type /Catalog /Pages {tree} 0 R >>"));
    pdf.bytes()
}

/// A `/ToUnicode` CMap around one `beginbfchar` block.
fn to_unicode(pdf: &mut Pdf, chars: &str) -> usize {
    let cmap = format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
         /CMapName /Custom def /CMapType 2 def\n\
         1 begincodespacerange <00> <ff> endcodespacerange\n\
         {chars}\n\
         endcmap CMapName currentdict /CMap defineresource pop end end"
    );
    pdf.add(format!(
        "<< /Length {} >>\nstream\n{}\nendstream",
        cmap.len(),
        cmap
    ))
}

/// What [`links_pdf`] carries, so that a test can name a link rather than an
/// index: the page it is on, the area it covers in **top-left points**, and
/// where it goes.
pub const LINKS: &[(usize, [f64; 4], &str)] = &[
    // Page one: one address, one place in this document. Both are written the
    // ordinary way, with a `/Dest` on the annotation.
    (1, [72.0, 72.0, 128.0, 20.0], "https://example.com/paper"),
    (1, [72.0, 122.0, 128.0, 20.0], "page 5"),
    // Page two: the same thing said the other way, as a `/GoTo` action — which
    // is the route `link.action()` answers and `link.destination()` does not.
    (2, [72.0, 72.0, 128.0, 20.0], "page 6"),
    // And a link that points nowhere at all, which is dropped rather than kept
    // as a rectangle that does nothing when it is clicked.
    (3, [72.0, 72.0, 128.0, 20.0], ""),
];

/// Where the internal link on page one lands, as a fraction of the page.
///
/// The destination is `/XYZ null 400 null` on a page whose box is 792 points
/// tall but cropped to start 100 up — an offprint — and pdfium counts from
/// the bottom of the *media* box: 392 points down of the 692 drawn. Measured
/// against the crop box's own height it landed 0.42 of the way down instead.
pub const LINK_OFFSET: f64 = (792.0 - 400.0) / (792.0 - 100.0);

/// What the labelled pages of [`links_pdf`] are called, in order.
///
/// Three pages of front matter numbered in lower-case roman, then a body that
/// starts again at 1 — which is the shape the whole of `label` and
/// `page_for_label` exists for, and the shape no fixture in the app has.
pub const LABELS: &[&str] = &["i", "ii", "iii", "1", "2", "3"];

/// Where the ink sits on every page of [`margins_pdf`], as fractions of the
/// page: left, top, right, bottom.
///
/// Deliberately not the same on both axes. A crop that keeps the page's shape
/// is a crop a layout test cannot see, and the whole of what trimming does to
/// a reader is change the shape of what is in front of them.
pub const INK: (f64, f64, f64, f64) = (0.2, 0.1, 0.8, 0.9);

/// Three pages with a black rectangle on each and nothing else.
///
/// The one fixture here that is about *pixels* rather than about text, and it
/// exists because the margins have to be arithmetic: every other document in
/// this file carries a single line of type, whose ink box is a band a few
/// percent tall, and a crop measured off one is decided entirely by the
/// clamp in [`crate::crop`] rather than by the page. Here the answer is
/// [`INK`] padded, and a test can say so in numbers.
pub fn margins_pdf() -> String {
    written("moonowl-fixture-margins.pdf", build_margins)
}

fn build_margins() -> Vec<u8> {
    const PAGES: usize = 3;
    const WIDTH: f64 = 612.0;
    const HEIGHT: f64 = 792.0;
    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let tree = pdf.reserve();
    let page_ids: Vec<usize> = (0..PAGES).map(|_| pdf.reserve()).collect();

    // PDF space counts from the bottom, so the fractions above are turned
    // over on the way in: what `INK` calls the top is the far edge from the
    // origin.
    let left = INK.0 * WIDTH;
    let right = INK.2 * WIDTH;
    let bottom = (1.0 - INK.3) * HEIGHT;
    let top = (1.0 - INK.1) * HEIGHT;
    let stream = format!(
        "0 0 0 rg {left:.2} {bottom:.2} {:.2} {:.2} re f",
        right - left,
        top - bottom
    );
    let content = pdf.add(format!(
        "<< /Length {} >>\nstream\n{}\nendstream",
        stream.len(),
        stream
    ));

    for &id in &page_ids {
        pdf.put(
            id,
            format!(
                "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 {WIDTH} {HEIGHT}] \
                 /Resources << >> /Contents {content} 0 R >>"
            ),
        );
    }
    pdf.put(
        tree,
        format!(
            "<< /Type /Pages /Count {PAGES} /Kids [{}] >>",
            page_ids
                .iter()
                .map(|id| format!("{id} 0 R"))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    );
    pdf.put(catalog, format!("<< /Type /Catalog /Pages {tree} 0 R >>"));
    pdf.bytes()
}

/// Six pages that carry links and number their own pages.
///
/// Two documents in one, because they are the same item of the plan and a
/// second fixture is a second thing to keep true. The links are written three
/// ways on purpose — see [`LINKS`] — and the labels are the `/PageLabels`
/// number tree, which is the only way a PDF says what is printed on a page.
pub fn links_pdf() -> String {
    written("moonowl-fixture-links.pdf", build_links)
}

fn build_links() -> Vec<u8> {
    const PAGES: usize = 6;
    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let tree = pdf.reserve();
    let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    let page_ids: Vec<usize> = (0..PAGES).map(|_| pdf.reserve()).collect();

    // The annotations, written after the pages are numbered because two of
    // them name a page. `/Rect` is the PDF's own space, counting from the
    // bottom, which is where [`LINKS`] stops agreeing with the file.
    let external = pdf.add(
        "<< /Type /Annot /Subtype /Link /Rect [72 700 200 720] /Border [0 0 0] \
         /A << /S /URI /URI (https://example.com/paper) >> >>",
    );
    let internal = pdf.add(format!(
        "<< /Type /Annot /Subtype /Link /Rect [72 650 200 670] /Border [0 0 0] \
         /Dest [{} 0 R /XYZ null 400 null] >>",
        page_ids[4],
    ));
    let by_action = pdf.add(format!(
        "<< /Type /Annot /Subtype /Link /Rect [72 700 200 720] /Border [0 0 0] \
         /A << /S /GoTo /D [{} 0 R /Fit] >> >>",
        page_ids[5],
    ));
    // Neither an action nor a destination: a rectangle that means nothing.
    let empty = pdf.add("<< /Type /Annot /Subtype /Link /Rect [72 700 200 720] /Border [0 0 0] >>");

    for (index, &id) in page_ids.iter().enumerate() {
        let text = format!("Page {} of the fixture.", index + 1);
        // A line above the links as well, so that a press on a link read as a
        // press at the page's top left selects the wrong line.
        let stream = format!(
            "BT /F1 18 Tf 72 740 Td (Above the links.) Tj ET BT /F1 18 Tf 72 700 Td ({text}) Tj ET"
        );
        let content = pdf.add(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            stream
        ));
        let annots = match index {
            0 => format!(" /Annots [{external} 0 R {internal} 0 R]"),
            1 => format!(" /Annots [{by_action} 0 R]"),
            2 => format!(" /Annots [{empty} 0 R]"),
            _ => String::new(),
        };
        // Page five, the `/XYZ` target, is cropped: see [`LINK_OFFSET`].
        let cropbox = if index == 4 {
            " /CropBox [0 100 612 792]"
        } else {
            ""
        };
        pdf.put(
            id,
            format!(
                "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792]{cropbox} \
                 /Resources << /Font << /F1 {font} 0 R >> >> /Contents {content} 0 R{annots} >>"
            ),
        );
    }
    pdf.put(
        tree,
        format!(
            "<< /Type /Pages /Count {PAGES} /Kids [{}] >>",
            page_ids
                .iter()
                .map(|id| format!("{id} 0 R"))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    );
    // The number tree: pages 0..2 in lower-case roman, then a fresh decimal
    // run starting at 1. `/St` is where the run starts and is the half a
    // document usually gets wrong.
    pdf.put(
        catalog,
        format!(
            "<< /Type /Catalog /Pages {tree} 0 R \
             /PageLabels << /Nums [0 << /S /r >> 3 << /S /D /St 1 >>] >> >>"
        ),
    );
    pdf.bytes()
}

/// A document with the notes somebody else left in it: a sticky note, a
/// comment over a passage, an annotation with nothing to read, and a link —
/// the last two being the cases that must *not* show up as notes.
pub fn notes_pdf() -> String {
    written("moonowl-fixture-notes.pdf", build_notes)
}

fn build_notes() -> Vec<u8> {
    const PAGES: usize = 3;
    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let tree = pdf.reserve();
    let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    let page_ids: Vec<usize> = (0..PAGES).map(|_| pdf.reserve()).collect();

    // A sticky note: small in both directions, so it is a marker and is
    // pressable all over. `/T` is who left it and `/Contents` is what it says.
    let sticky = pdf.add(
        "<< /Type /Annot /Subtype /Text /Rect [500 700 520 720] /T (A Reader) \
         /Contents (Check this against the second edition.) >>",
    );
    // A comment over a highlighted sentence: a passage rather than a marker,
    // so only the strip at its right edge answers a press.
    let comment = pdf.add(
        "<< /Type /Annot /Subtype /Highlight /Rect [72 690 400 715] \
         /QuadPoints [72 715 400 715 72 690 400 690] \
         /Contents (This is the sentence the whole argument turns on.) >>",
    );
    // Nothing to read: an annotation with no `/Contents` is not a note.
    let silent = pdf.add("<< /Type /Annot /Subtype /Square /Rect [72 600 200 640] >>");
    // And a link, whose text is where it goes and which is never a note.
    let link = pdf.add(
        "<< /Type /Annot /Subtype /Link /Rect [72 500 200 520] /Border [0 0 0] \
         /Contents (not a note) /A << /S /URI /URI (https://example.com) >> >>",
    );

    for (index, &id) in page_ids.iter().enumerate() {
        let text = format!("Page {} of the fixture.", index + 1);
        let stream = format!("BT /F1 18 Tf 72 700 Td ({text}) Tj ET");
        let content = pdf.add(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            stream
        ));
        let annots = match index {
            0 => format!(" /Annots [{sticky} 0 R {comment} 0 R {silent} 0 R {link} 0 R]"),
            _ => String::new(),
        };
        pdf.put(
            id,
            format!(
                "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
                 /Resources << /Font << /F1 {font} 0 R >> >> /Contents {content} 0 R{annots} >>"
            ),
        );
    }
    pdf.put(
        tree,
        format!(
            "<< /Type /Pages /Count {PAGES} /Kids [{}] >>",
            page_ids
                .iter()
                .map(|id| format!("{id} 0 R"))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    );
    pdf.put(catalog, format!("<< /Type /Catalog /Pages {tree} 0 R >>"));
    pdf.bytes()
}

/// **The four hundred page book**, which is the document every memory and
/// timing number in `experiments/PROGRESS.md` was taken on, and the one most
/// of this suite reads.
///
/// It used to be written by `tests/fixtures/make-pdf.mjs` and generated by a
/// step in two workflows, which is why it is here now: the script went when
/// the port did, and the workflows went on calling it. Written in Rust it
/// needs cargo and nothing else, which is what the rest of this file is for.
///
/// Four hundred pages of the same sentence, set as one long line so a page has
/// a block of ink on it and a wide margin around it — the shape margin
/// trimming, the layout and the range reads all want.
pub fn book_pdf() -> String {
    written("moonowl-fixtures/book.pdf", || build_book(400))
}

/// A journal offprint: nineteen pages printed 407 to 425 at the foot, a
/// running head with the year on every one, and no `/PageLabels` — the file
/// says nothing about its numbers and the paper says everything.
pub fn offprint_pdf() -> String {
    written("moonowl-fixture-offprint.pdf", build_offprint)
}

fn build_offprint() -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let tree = pdf.reserve();
    let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    let page_ids: Vec<usize> = (0..19).map(|_| pdf.reserve()).collect();
    for (index, &id) in page_ids.iter().enumerate() {
        let stream = format!(
            "BT /F1 9 Tf 72 760 Td (JOURNAL OF THINGS 2011 Vol. 100) Tj ET \
             BT /F1 11 Tf 72 600 Td (Body text on page {}.) Tj ET \
             BT /F1 10 Tf 300 40 Td ({}) Tj ET",
            index + 1,
            407 + index
        );
        let content = pdf.add(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            stream
        ));
        pdf.put(
            id,
            format!(
                "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
                 /Resources << /Font << /F1 {font} 0 R >> >> /Contents {content} 0 R >>"
            ),
        );
    }
    pdf.put(
        tree,
        format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            page_ids.len(),
            page_ids
                .iter()
                .map(|id| format!("{id} 0 R"))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    );
    pdf.put(catalog, format!("<< /Type /Catalog /Pages {tree} 0 R >>"));
    pdf.bytes()
}

fn build_book(pages: usize) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let tree = pdf.reserve();
    let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    let page_ids: Vec<usize> = (0..pages).map(|_| pdf.reserve()).collect();
    for (index, &id) in page_ids.iter().enumerate() {
        let filler = "The quick brown fox jumps over the lazy dog. ".repeat(12);
        let stream = format!(
            "BT /F1 11 Tf 54 720 Td 14 TL (Page {}. {filler}) Tj ET",
            index + 1
        );
        let content = pdf.add(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            stream
        ));
        pdf.put(
            id,
            format!(
                "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
                 /Resources << /Font << /F1 {font} 0 R >> >> /Contents {content} 0 R >>"
            ),
        );
    }
    pdf.put(
        tree,
        format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            pages,
            page_ids
                .iter()
                .map(|id| format!("{id} 0 R"))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    );
    pdf.put(catalog, format!("<< /Type /Catalog /Pages {tree} 0 R >>"));
    pdf.bytes()
}

fn build(pages: usize) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let tree = pdf.reserve();
    let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");

    let page_ids: Vec<usize> = (0..pages).map(|_| pdf.reserve()).collect();
    for (index, &id) in page_ids.iter().enumerate() {
        let text = format!("Page {} of the fixture.", index + 1);
        let stream = format!("BT /F1 18 Tf 72 700 Td ({text}) Tj ET");
        let content = pdf.add(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            stream
        ));
        pdf.put(
            id,
            format!(
                "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
                 /Resources << /Font << /F1 {font} 0 R >> >> /Contents {content} 0 R >>"
            ),
        );
    }
    pdf.put(
        tree,
        format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            pages,
            page_ids
                .iter()
                .map(|id| format!("{id} 0 R"))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    );

    let outline = pdf.reserve();
    let top = write_sections(&mut pdf, CONTENTS, outline, &page_ids);
    pdf.put(
        outline,
        format!(
            "<< /Type /Outlines /Count {} {} >>",
            CONTENTS.len(),
            first_and_last(&top),
        ),
    );
    pdf.put(
        catalog,
        format!(
            "<< /Type /Catalog /Pages {tree} 0 R /Outlines {outline} 0 R /PageMode /UseOutlines >>"
        ),
    );
    pdf.bytes()
}

/// Write one level of the outline and return the object number of each entry
/// in it, in order — so the level above can chain them.
fn write_sections(
    pdf: &mut Pdf,
    sections: &[Section],
    parent: usize,
    page_ids: &[usize],
) -> Vec<usize> {
    let ids: Vec<usize> = sections.iter().map(|_| pdf.reserve()).collect();
    for (at, section) in sections.iter().enumerate() {
        let children = write_sections(pdf, section.under, ids[at], page_ids);
        let page = page_ids[(section.page - 1).min(page_ids.len() - 1)];
        let mut body = format!(
            "<< /Title ({}) /Parent {parent} 0 R /Dest [{page} 0 R /XYZ null null null]",
            section.title,
        );
        if at > 0 {
            body.push_str(&format!(" /Prev {} 0 R", ids[at - 1]));
        }
        if at + 1 < ids.len() {
            body.push_str(&format!(" /Next {} 0 R", ids[at + 1]));
        }
        if !children.is_empty() {
            // A positive count is an outline the viewer opens; the sign is
            // the whole of what "expanded" means in the format.
            body.push_str(&format!(
                " /Count {} {}",
                children.len(),
                first_and_last(&children)
            ));
        }
        body.push_str(" >>");
        pdf.put(ids[at], body);
    }
    ids
}

fn first_and_last(ids: &[usize]) -> String {
    match (ids.first(), ids.last()) {
        (Some(first), Some(last)) => format!("/First {first} 0 R /Last {last} 0 R"),
        _ => String::new(),
    }
}

/// The password [`locked_pdf`] is written under.
///
/// A constant rather than a parameter because a fixture is cached by its file
/// name and two passwords would want two files, and because what the tests
/// need is one document that is locked, one password that opens it and any
/// other string to be wrong.
pub const LOCKED_PASSWORD: &str = "moonowl";

/// Two pages, and a signature field that has actually been signed.
///
/// **The app's own `tests/fixtures/signed.pdf` is not this.** That one is a
/// `/FT /Sig` widget with `/SigFlags 3` and **no `/V`** — a signature *field*,
/// which is a blank line at the foot of a contract. This has a `/V` holding a
/// real `/Sig` dictionary with the four entries pdfium can read.
///
/// The blob is not a real signature over these bytes and could not be, nothing
/// here being able to make one. It is a well-formed DER prologue, which is
/// enough to be *present* — and being present is the fact this reader
/// reports.
pub fn signed_pdf() -> String {
    written("moonowl-fixture-signed.pdf", || build_signed(true))
}

/// The same document with the signature field left blank — the app's fixture,
/// rebuilt here so that the pair can be compared in one test.
pub fn unsigned_field_pdf() -> String {
    written("moonowl-fixture-blank-field.pdf", || build_signed(false))
}

fn build_signed(filled: bool) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let tree = pdf.reserve();
    let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    // The signature itself. `/ByteRange` and `/SubFilter` are written because
    // a `/Sig` without them is malformed, and not because anything reads them:
    // pdfium has getters for both and `pdfium-render` exposes neither, and
    // hides the handle they would be called on. See [`crate::sign::Seal`].
    let value = if filled {
        Some(pdf.add(
            "<< /Type /Sig /Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached \
              /Name (A. Reader) /Reason (I agree to the terms) \
              /M (D:20240314093000+01'00') \
              /ByteRange [0 840 1560 620] \
              /Contents <308006092a864886f70d010702a0803080020101310f300d060960864801650304020105000000> >>",
        ))
    } else {
        None
    };
    let signature = match value {
        Some(id) => format!(" /V {id} 0 R"),
        None => String::new(),
    };
    let field = pdf.add(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Sig /T (Signature1) /Ff 0 \
          /Rect [400 60 560 110] /F 4{signature} >>"
    ));
    let page_ids: Vec<usize> = (0..2).map(|_| pdf.reserve()).collect();
    for (index, &id) in page_ids.iter().enumerate() {
        let stream = format!(
            "BT /F1 12 Tf 54 720 Td (Page {}. Sign here.) Tj ET",
            index + 1
        );
        let content = pdf.add(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            stream
        ));
        let annots = if index == 0 {
            format!(" /Annots [{field} 0 R]")
        } else {
            String::new()
        };
        pdf.put(
            id,
            format!(
                "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
                 /Resources << /Font << /F1 {font} 0 R >> >> /Contents {content} 0 R{annots} >>"
            ),
        );
    }
    pdf.put(
        tree,
        format!(
            "<< /Type /Pages /Count 2 /Kids [{}] >>",
            page_ids
                .iter()
                .map(|id| format!("{id} 0 R"))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    );
    pdf.put(
        catalog,
        format!(
            "<< /Type /Catalog /Pages {tree} 0 R \
             /AcroForm << /Fields [{field} 0 R] /SigFlags 3 >> >>"
        ),
    );
    pdf.bytes()
}

/// Three pages behind a password: the PDF standard security handler, revision
/// 2, RC4 at 40 bits.
///
/// **The oldest and weakest thing the spec describes, chosen for that reason:**
/// what is tested is that the reader notices a locked document, asks, and opens
/// it with the answer. Revision 2's key derivation is a single MD5 with no
/// iteration, so the handler below is forty lines rather than a dependency.
///
/// What makes the prompt possible: **the structure is in the clear and only the
/// contents are not.** The catalogue, page tree and `xref` are readable without
/// the password; the streams and strings are not. So a locked document can be
/// told from a corrupt one before there is anything to unlock it with, which is
/// what `FPDF_ERR_PASSWORD` says and `FPDF_ERR_FORMAT` does not.
pub fn locked_pdf() -> String {
    written("moonowl-locked.pdf", || build_locked(LOCKED_PASSWORD))
}

/// The same three pages under an *owner* password alone: encrypted, and opened
/// by anybody with no password at all — which is how a document that only
/// forbids printing or copying is made, and the one a reader must not mistake
/// for an unencrypted file when it comes to writing into it.
pub fn restricted_pdf() -> String {
    written("moonowl-restricted.pdf", || build_locked(""))
}

/// `user` is the password that opens it; the owner's is always
/// [`LOCKED_PASSWORD`].
fn build_locked(user: &str) -> Vec<u8> {
    // Fixed rather than random, because a fixture that is different every run
    // is a fixture that cannot be cached and cannot be compared.
    let id: [u8; 16] = *b"Moonowl fixture ";
    // The spec's Algorithm 3: the user password, encrypted under the owner's.
    // pdfium takes one string and tries it as both.
    let owner = rc4(&md5(&padded(LOCKED_PASSWORD))[..5], &padded(user));
    let key = encryption_key(user, &owner, &id);
    let user = rc4(&key, &PAD);

    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let tree = pdf.reserve();
    let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    let page_ids: Vec<usize> = (0..3).map(|_| pdf.reserve()).collect();
    for (index, &id_of_page) in page_ids.iter().enumerate() {
        // Reserved before it is written, because a stream is encrypted under a
        // key derived from its own object number — see [`object_key`] — so
        // the number has to be known before the bytes can be made.
        let content = pdf.reserve();
        let stream = format!("BT /F1 18 Tf 72 700 Td (Locked page {}.) Tj ET", index + 1);
        let sealed = rc4(&object_key(&key, content), stream.as_bytes());
        // `/Length` is the length of what is actually in the file, which is
        // the ciphertext. RC4 is a stream cipher, so it is also the length of
        // the plaintext; that they agree is a property of this cipher rather
        // than a rule.
        let mut body = format!("<< /Length {} >>\nstream\n", sealed.len()).into_bytes();
        body.extend_from_slice(&sealed);
        body.extend_from_slice(b"\nendstream");
        pdf.put(content, body);
        pdf.put(
            id_of_page,
            format!(
                "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
                 /Resources << /Font << /F1 {font} 0 R >> >> /Contents {content} 0 R >>"
            ),
        );
    }
    pdf.put(
        tree,
        format!(
            "<< /Type /Pages /Count 3 /Kids [{}] >>",
            page_ids
                .iter()
                .map(|id| format!("{id} 0 R"))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    );
    pdf.put(catalog, format!("<< /Type /Catalog /Pages {tree} 0 R >>"));
    // The one dictionary in the file whose own strings are *not* encrypted,
    // for the obvious reason: they are what the key is checked against.
    pdf.encrypt = Some(pdf.add(format!(
        "<< /Filter /Standard /V 1 /R 2 /O <{}> /U <{}> /P -1 >>",
        hex(&owner),
        hex(&user),
    )));
    pdf.id = Some(id);
    pdf.bytes()
}

/// The 32 bytes every password in the standard security handler is padded
/// with, from the spec's own table.
const PAD: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

/// A password as the handler wants it: its first 32 bytes, made up to 32 with
/// the padding above. A password longer than 32 bytes has the rest ignored,
/// which is the spec's rule and not an oversight here.
fn padded(password: &str) -> [u8; 32] {
    let mut out = PAD;
    let given = password.as_bytes();
    let taken = given.len().min(32);
    out[..taken].copy_from_slice(&given[..taken]);
    out[taken..].copy_from_slice(&PAD[..32 - taken]);
    out
}

/// The file encryption key: the spec's Algorithm 2 at revision 2, which is one
/// MD5 over the padded password, the `/O` entry, the permissions and the file
/// identifier, cut to five bytes.
fn encryption_key(password: &str, owner: &[u8], id: &[u8; 16]) -> Vec<u8> {
    let mut fed = Vec::with_capacity(32 + 32 + 4 + 16);
    fed.extend_from_slice(&padded(password));
    fed.extend_from_slice(owner);
    // `/P -1`, as four bytes, low one first. Every permission granted, which
    // is what a document locked only to be read says.
    fed.extend_from_slice(&(-1i32).to_le_bytes());
    fed.extend_from_slice(id);
    md5(&fed)[..5].to_vec()
}

/// The key one object's bytes are encrypted under: the spec's Algorithm 1,
/// which mixes the file key with the object's own number so that two identical
/// streams do not encrypt to the same bytes. Generation is always 0 here.
fn object_key(key: &[u8], object: usize) -> Vec<u8> {
    let mut fed = key.to_vec();
    fed.extend_from_slice(&(object as u32).to_le_bytes()[..3]);
    fed.extend_from_slice(&[0, 0]);
    let taken = (key.len() + 5).min(16);
    md5(&fed)[..taken].to_vec()
}

/// RC4, which is the whole of what revision 2 encrypts with. Twenty lines, and
/// they are the same twenty lines in every description of it.
fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut state: [u8; 256] = std::array::from_fn(|i| i as u8);
    let mut j = 0usize;
    for i in 0..256 {
        j = (j + state[i] as usize + key[i % key.len()] as usize) % 256;
        state.swap(i, j);
    }
    let (mut i, mut j) = (0usize, 0usize);
    data.iter()
        .map(|&byte| {
            i = (i + 1) % 256;
            j = (j + state[i] as usize) % 256;
            state.swap(i, j);
            byte ^ state[(state[i] as usize + state[j] as usize) % 256]
        })
        .collect()
}

/// MD5, for the key derivation above and for nothing else in this tree.
///
/// Written here rather than taken as a dependency because it is used by one
/// fixture, in test builds only, to make a file that is deliberately weak —
/// and because a hash in a dependency is a hash somebody may reach for later
/// believing it is fit for something. This one is not.
fn md5(input: &[u8]) -> [u8; 16] {
    const SHIFTS: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    // The constants are the first 32 bits of the fractional part of the sine
    // of the round number, which is how the algorithm defines them.
    let table: [u32; 64] =
        std::array::from_fn(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32);

    let mut message = input.to_vec();
    let bits = (input.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_le_bytes());

    let mut hash: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    for chunk in message.chunks(64) {
        let words: [u32; 16] = std::array::from_fn(|i| {
            u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ])
        });
        let [mut a, mut b, mut c, mut d] = hash;
        for round in 0..64 {
            let (mixed, index) = match round / 16 {
                0 => ((b & c) | (!b & d), round),
                1 => ((d & b) | (!d & c), (5 * round + 1) % 16),
                2 => (b ^ c ^ d, (3 * round + 5) % 16),
                _ => (c ^ (b | !d), (7 * round) % 16),
            };
            let mixed = mixed
                .wrapping_add(a)
                .wrapping_add(table[round])
                .wrapping_add(words[index]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(mixed.rotate_left(SHIFTS[round]));
        }
        for (slot, add) in hash.iter_mut().zip([a, b, c, d]) {
            *slot = slot.wrapping_add(add);
        }
    }
    let mut out = [0u8; 16];
    for (index, word) in hash.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// Bytes as the digits a PDF hexadecimal string is written in.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// MD5 of some bytes, as hexadecimal — the one door onto [`md5`], so that a
/// test can check it against the RFC's vectors without the function itself
/// becoming part of this module's surface.
pub fn digest_for_test(bytes: &[u8]) -> String {
    hex(&md5(bytes))
}
