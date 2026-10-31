//! Documents written by hand, for the tests that need a shape the files in
//! `tests/fixtures/` do not have.
//!
//! Everything here is written in Rust so that `cargo test` needs cargo and
//! nothing else, on all three platforms. A fixture that needs another tool to
//! exist first is a test that does not run on a machine without that tool.
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

/// Take away what earlier runs left in the temp directory: every test makes
/// folders and files named `moonowl-…` there, and nothing else does. Once per
/// process, before the harness or a fixture hands out a path, so nothing this
/// run is using can go.
// ponytail: an hour old is "not this run's"; a test that runs longer, or two
// suites at once an hour apart, would want a lock file per run instead.
pub fn sweep() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let hour = std::time::Duration::from_secs(3600);
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        for entry in entries.flatten() {
            let old = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .is_ok_and(|at| at.elapsed().is_ok_and(|age| age > hour));
            if old && entry.file_name().to_string_lossy().starts_with("moonowl-") {
                let path = entry.path();
                let _ = std::fs::remove_dir_all(&path).or_else(|_| std::fs::remove_file(&path));
            }
        }
    });
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
    sweep();
    let path: PathBuf = std::env::temp_dir().join(name);
    // Built every time and compared, not trusted by name: otherwise a fixture
    // edited in this file stays the old one until somebody empties the temp
    // directory by hand, and a test written against the edit passes on it.
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
/// page it goes to, how far up that page in points if not its top, and the
/// entries under it.
pub struct Section {
    pub title: &'static str,
    pub page: usize,
    pub top: Option<u32>,
    pub under: &'static [Section],
}

const fn section(title: &'static str, page: usize, under: &'static [Section]) -> Section {
    Section {
        title,
        page,
        top: None,
        under,
    }
}

/// The contents the fixture carries, and therefore what a test asserts on:
/// seven rows over three levels, in this order.
pub const CONTENTS: &[Section] = &[
    section("Front matter", 1, &[section("Preface", 2, &[])]),
    section(
        "Chapter One",
        3,
        &[
            section(
                "A section",
                4,
                // Down its page, 392 points of 792.
                &[Section {
                    top: Some(400),
                    ..section("Under a section", 5, &[])
                }],
            ),
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
    written("moonowl-fixture-contents.pdf", || build(12, CONTENTS))
}

/// Three chapters in six pages. "Middle" begins halfway down page 3, under
/// a line that ends "Opening". "End" begins a third of the way down page
/// 5, under nothing but the line every page has, as a chapter does whose
/// heading is set low under a running head.
const MIDPAGE: &[Section] = &[
    section("Opening", 1, &[]),
    Section {
        top: Some(400),
        ..section("Middle", 3, &[])
    },
    Section {
        top: Some(500),
        ..section("End", 5, &[])
    },
];

/// A six-page document with [`MIDPAGE`] as its contents.
pub fn midpage_chapters_pdf() -> String {
    written("moonowl-fixture-midpage.pdf", || {
        build_with(6, MIDPAGE, &[(3, 500, "The opening chapter ends here.")])
    })
}

/// A table of contents with entries that lead nowhere on both sides of one
/// that leads somewhere: page 0 below is written as a named destination the
/// document does not have, as an extract keeps the contents of the whole it
/// was cut from. "Part" has no place of its own but has an entry under it
/// that does.
const STRAY: &[Section] = &[
    section("Before it", 0, &[section("Under that", 0, &[])]),
    section("Part", 0, &[section("Here", 2, &[])]),
    section("After it", 0, &[]),
];

/// Six pages of a paper whose references have no links behind them: "[3]",
/// "Figure 2" and "Table 7" on the first page, a sentence and then the
/// caption beginning with "Figure 2" on the third, and the bibliography on
/// the fifth, where "[30]" comes before "[3]". No caption begins with
/// "Table 7".
pub fn references_pdf() -> String {
    written("moonowl-fixture-references.pdf", || {
        build_with(
            6,
            &[],
            &[
                (1, 600, "[3] found it first, as the next line says."),
                (1, 560, "Figure 2 has the picture."),
                (1, 520, "See Table 7 for more."),
                (3, 500, "Figure 2 shows nothing yet."),
                (3, 300, "Figure 2: The caption of the figure."),
                (5, 650, "References"),
                (5, 620, "[30] Not this one."),
                (5, 600, "[3] The paper that found it."),
            ],
        )
    })
}

/// The LaTeX source [`synctex_pdf`] stands for, line by line.
pub const SYNCTEX_SOURCE: &str = "\\documentclass{article}
\\begin{document}
\\section{Introduction}
The first line of the introduction.

A second paragraph, on line six.
\\newpage
Text on page two, line eight.
\\end{document}
";

/// What pdflatex 2020 wrote for [`SYNCTEX_SOURCE`] with `-synctex=1`, less
/// the inputs the class reads.
const SYNCTEX: &str = "SyncTeX Version:1
Input:1:./paper.tex
Output:pdf
Magnification:1000
Unit:1
X Offset:0
Y Offset:0
Content:
!443
{1
[1,7:4736286,46220574:26673152,41484288,0
[1,7:8799518,46220574:22609920,40435712,0
[1,7:8799518,44254494:22609920,36044800,0
(1,3:8799518,8865054:22609920,655359,0
(1,3:8799518,8865054:1592523,608173,0
g1,3:8799518,8865054
x1,3:9330359,8865054
g1,3:10392041,8865054
)
x1,3:11377265,8865054
k1,3:31409438,8865054:15174871
g1,3:31409438,8865054
)
(1,5:8799518,10300473:22609920,455111,0
x1,5:9928195,10300473
g1,4:10146648,10300473
x1,4:11280787,10300473
g1,4:11499240,10300473
x1,4:12518691,10300473
k1,5:31409438,10300473:13081696
g1,5:31409438,10300473
)
(1,7:8799518,11086905:22609920,455111,127431
h1,6:8799518,11086905:983040,0,0
x1,6:10274079,11086905
g1,6:10492532,11086905
x1,6:12389438,11086905
k1,7:31409438,11086905:12368082
g1,7:31409438,11086905
)
]
]
]
!1391
}1
!9
{2
[1,9:4736286,46220574:26673152,41484288,0
[1,9:8799518,44254494:22609920,36044800,0
(1,9:8799518,8865054:22609920,455111,127431
h1,8:8799518,8865054:983040,0,0
x1,8:10255874,8865054
x1,8:11093280,8865054
g1,8:11311733,8865054
k1,9:31409438,8865054:13416663
g1,9:31409438,8865054
)
]
]
!803
}2
Postamble:
Count:72
Post scriptum:
";

/// A two-page document compiled from [`SYNCTEX_SOURCE`], with its source
/// and its `paper.synctex.gz` beside it in a folder of their own. Each
/// line of text is at the height pdflatex put it.
pub fn synctex_pdf() -> String {
    written("moonowl-synctex/paper.tex", || SYNCTEX_SOURCE.into());
    written("moonowl-synctex/paper.synctex.gz", || {
        let mut packed = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        packed.write_all(SYNCTEX.as_bytes()).expect("packed");
        packed.finish().expect("packed")
    });
    written("moonowl-synctex/paper.pdf", || {
        build_with(
            2,
            &[],
            &[
                (1, 657, "1 Introduction"),
                (1, 635, "The first line of the introduction."),
                (1, 623, "A second paragraph, on line six."),
                (2, 657, "Text on page two, line eight."),
            ],
        )
    })
}

/// [`synctex_pdf`] as a book in two parts compiled into a folder of its
/// own, as latexmk's `-outdir=out` does: `ch1/paper.tex` made the first
/// page and `ch2/paper.tex` the second, and both are named from the folder
/// LaTeX was run in, which holds `main.tex`. Answers the PDF, in `out`.
pub fn synctex_parts_pdf() -> String {
    let root = std::env::temp_dir().join("moonowl-synctex-parts");
    written("moonowl-synctex-parts/main.tex", || {
        b"\\input{ch1/paper}\n\\input{ch2/paper}\n".to_vec()
    });
    written("moonowl-synctex-parts/ch1/paper.tex", || {
        SYNCTEX_SOURCE.into()
    });
    written("moonowl-synctex-parts/ch2/paper.tex", || {
        SYNCTEX_SOURCE.into()
    });
    written("moonowl-synctex-parts/out/paper.synctex.gz", || {
        let inputs = format!(
            "Input:1:./ch1/paper.tex\nInput:2:./ch2/paper.tex\nInput:3:{}/main.tex",
            root.display()
        );
        let (first, second) = SYNCTEX.split_once("{2\n").expect("a second page");
        // Every record of the second page is tagged with the second input.
        let second: String = (second.lines())
            .map(|line| match line.split_at_checked(2) {
                Some((kind, rest)) if kind.ends_with('1') && rest.starts_with(',') => {
                    format!("{}2{rest}\n", &kind[..1])
                }
                _ => format!("{line}\n"),
            })
            .collect();
        let file = format!("{first}{{2\n{second}").replace("Input:1:./paper.tex", &inputs);
        let mut packed = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        packed.write_all(file.as_bytes()).expect("packed");
        packed.finish().expect("packed")
    });
    written("moonowl-synctex-parts/out/paper.pdf", || {
        std::fs::read(synctex_pdf()).expect("the SyncTeX fixture")
    })
}

/// Two versions of a two-page document, the earlier one first. The later
/// one says "red" where the earlier said "brown", has lost "lazy", and has
/// a line more on its second page.
pub fn compared_pdfs() -> (String, String) {
    let earlier = written("moonowl-fixture-earlier.pdf", || {
        build_with(
            2,
            &[],
            &[
                (1, 600, "The quick brown fox jumps over the lazy dog."),
                (2, 600, "A second page that stays the same."),
            ],
        )
    });
    let later = written("moonowl-fixture-later.pdf", || {
        build_with(
            2,
            &[],
            &[
                (1, 600, "The quick red fox jumps over the dog."),
                (2, 600, "A second page that stays the same."),
                (2, 560, "And one more line."),
            ],
        )
    });
    (earlier, later)
}

/// One empty page with no words on it, as a scan with no text has none.
pub fn wordless_pdf() -> String {
    written("moonowl-fixture-wordless.pdf", || {
        crate::arrange::blank_pdf(612.0, 792.0).expect("a blank page")
    })
}

/// A three-page document whose contents name a place it does not have.
pub fn stray_contents_pdf() -> String {
    written("moonowl-fixture-stray-contents.pdf", || build(3, STRAY))
}

/// One page, and an outline that loops: three untitled entries, each of
/// whose children are the same three entries again.
pub fn cyclic_outline_pdf() -> String {
    written("moonowl-fixture-cyclic-outline.pdf", || {
        let mut pdf = Pdf::new();
        let catalog = pdf.reserve();
        let tree = pdf.reserve();
        let page = pdf.reserve();
        pdf.put(
            page,
            format!("<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] >>"),
        );
        pdf.put(
            tree,
            format!("<< /Type /Pages /Count 1 /Kids [{page} 0 R] >>"),
        );
        let outline = pdf.reserve();
        let ids: Vec<usize> = (0..3).map(|_| pdf.reserve()).collect();
        for (at, &id) in ids.iter().enumerate() {
            let mut body = format!(
                "<< /Title () /Parent {outline} 0 R /First {} 0 R /Last {} 0 R /Count 3",
                ids[0], ids[2]
            );
            if let Some(next) = ids.get(at + 1) {
                body.push_str(&format!(" /Next {next} 0 R"));
            }
            body.push_str(" >>");
            pdf.put(id, body);
        }
        pdf.put(
            outline,
            format!(
                "<< /Type /Outlines /Count 3 /First {} 0 R /Last {} 0 R >>",
                ids[0], ids[2]
            ),
        );
        pdf.put(
            catalog,
            format!("<< /Type /Catalog /Pages {tree} 0 R /Outlines {outline} 0 R >>"),
        );
        pdf.bytes()
    })
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
    crate::atomic_write(path, &build(pages, CONTENTS)).expect("write the draft");
}

/// Three pages that share one resource dictionary, as many producers write
/// them: it names the font every page uses and a picture only the second
/// page draws. Saving the first page alone should not bring the picture.
pub fn shared_pdf() -> String {
    written("moonowl-fixture-shared.pdf", || {
        let mut pdf = Pdf::new();
        let catalog = pdf.reserve();
        let tree = pdf.reserve();
        let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
        // 128 by 128 in RGB, of noise that does not compress, so that it is
        // plainly the biggest thing in the file whatever pdfium does to it.
        let side = 128;
        let mut seed: u32 = 0x9e37_79b9;
        let pixels: Vec<u8> = (0..side * side * 3)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed >> 24) as u8
            })
            .collect();
        let mut picture = format!(
            "<< /Type /XObject /Subtype /Image /Width {side} /Height {side} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {} >>\nstream\n",
            pixels.len()
        )
        .into_bytes();
        picture.extend_from_slice(&pixels);
        picture.extend_from_slice(b"\nendstream");
        let picture = pdf.add(picture);
        let resources = pdf.add(format!(
            "<< /Font << /F1 {font} 0 R >> /XObject << /Im1 {picture} 0 R >> >>"
        ));
        let pages: Vec<usize> = (0..3).map(|_| pdf.reserve()).collect();
        for (index, &id) in pages.iter().enumerate() {
            let mut stream = format!("BT /F1 18 Tf 72 700 Td (Page {}.) Tj ET", index + 1);
            if index == 1 {
                stream += " q 200 0 0 200 72 400 cm /Im1 Do Q";
            }
            let content = pdf.add(format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                stream.len(),
                stream
            ));
            pdf.put(
                id,
                format!(
                    "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
                     /Resources {resources} 0 R /Contents {content} 0 R >>"
                ),
            );
        }
        let kids: Vec<String> = pages.iter().map(|id| format!("{id} 0 R")).collect();
        pdf.put(
            tree,
            format!("<< /Type /Pages /Count 3 /Kids [{}] >>", kids.join(" ")),
        );
        pdf.put(catalog, format!("<< /Type /Catalog /Pages {tree} 0 R >>"));
        pdf.bytes()
    })
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

/// Four pages in two branches of the page tree, each page taking its
/// resources, its box and its turn from its branch: "One" and "Two" are
/// upright letters in Helvetica, and "Three" and "Four" are small pages in
/// Courier turned a quarter. Moving a page between branches must keep what
/// it inherits.
pub fn inherited_pdf() -> String {
    written("moonowl-fixture-inherited.pdf", || {
        let mut pdf = Pdf::new();
        let catalog = pdf.reserve();
        let root = pdf.reserve();
        let branches = [pdf.reserve(), pdf.reserve()];
        let fonts = [
            pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
            pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>"),
        ];
        let mut kids = [Vec::new(), Vec::new()];
        for (index, name) in ["One", "Two", "Three", "Four"].into_iter().enumerate() {
            // Each branch names the font differently, so a page under the
            // other branch would find no font by its name.
            let stream = format!("BT /F{} 24 Tf 40 100 Td ({name}) Tj ET", index / 2 + 1);
            let content = pdf.add(format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                stream.len(),
                stream
            ));
            let branch = index / 2;
            let page = pdf.add(format!(
                "<< /Type /Page /Parent {} 0 R /Contents {content} 0 R >>",
                branches[branch]
            ));
            kids[branch].push(format!("{page} 0 R"));
        }
        let inherits = [
            "/MediaBox [0 0 612 792]".to_string(),
            "/MediaBox [0 0 400 300] /Rotate 90".to_string(),
        ];
        for branch in 0..2 {
            pdf.put(
                branches[branch],
                format!(
                    "<< /Type /Pages /Parent {root} 0 R /Count 2 /Kids [{}] {} \
                     /Resources << /Font << /F{} {} 0 R >> >> >>",
                    kids[branch].join(" "),
                    inherits[branch],
                    branch + 1,
                    fonts[branch]
                ),
            );
        }
        pdf.put(
            root,
            format!(
                "<< /Type /Pages /Count 4 /Kids [{} 0 R {} 0 R] >>",
                branches[0], branches[1]
            ),
        );
        pdf.put(catalog, format!("<< /Type /Catalog /Pages {root} 0 R >>"));
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
/// actually reports — and two of the three answers are not what one would
/// guess:
///
/// * **A ligature comes back already split.** pdfium hands over "f" and "i",
///   with a box each, whatever the font says. See the `/Differences` font
///   below.
/// * **An accent comes back precomposed**, U+00E9 rather than "e" and a
///   combining mark, so the fold's decompose-and-drop is what makes "resume"
///   find "résumé".
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
    // ligature half of `fold` has nothing to do on this renderer. The fold
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
/// against the crop box's own height it would land 0.42 of the way down.
pub const LINK_OFFSET: f64 = (792.0 - 400.0) / (792.0 - 100.0);

/// What the labelled pages of [`links_pdf`] are called, in order.
///
/// Three pages of front matter numbered in lower-case roman, then a body that
/// starts again at 1 — which is the shape the whole of `label` and
/// `page_for_label` exists for.
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

/// A paper set in two columns under a title the width of the page, with a
/// comment on a passage in each column.
pub fn columns_pdf() -> String {
    written("moonowl-fixture-columns.pdf", build_columns)
}

fn build_columns() -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let tree = pdf.reserve();
    let page = pdf.reserve();
    let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    let mut stream =
        String::from("BT /F1 16 Tf 72 740 Td (A title across the whole width of the page) Tj ET\n");
    for line in 0..40 {
        let y = 700 - line * 14;
        for x in [72, 316] {
            stream.push_str(&format!(
                "BT /F1 10 Tf {x} {y} Td (Lorem ipsum dolor sit amet, consectetur elit) Tj ET\n"
            ));
        }
    }
    let content = pdf.add(format!(
        "<< /Length {} >>\nstream\n{}\nendstream",
        stream.len(),
        stream
    ));
    let left = pdf.add(
        "<< /Type /Annot /Subtype /Highlight /Rect [72 640 280 654] \
         /QuadPoints [72 654 280 654 72 640 280 640] /C [1 0.84 0.04] \
         /Contents (On the left column.) >>",
    );
    let right = pdf.add(
        "<< /Type /Annot /Subtype /Highlight /Rect [316 640 520 654] \
         /QuadPoints [316 654 520 654 316 640 520 640] /C [0.45 0.75 0.99] \
         /Contents (On the right column.) >>",
    );
    pdf.put(
        page,
        format!(
            "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
             /Resources << /Font << /F1 {font} 0 R >> >> /Contents {content} 0 R \
             /Annots [{left} 0 R {right} 0 R] >>"
        ),
    );
    pdf.put(
        tree,
        format!("<< /Type /Pages /Count 1 /Kids [{page} 0 R] >>"),
    );
    pdf.put(catalog, format!("<< /Type /Catalog /Pages {tree} 0 R >>"));
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
    // so it is a card beside the page, or underlined where there is no room.
    // With the `/Popup` Preview and Acrobat give a comment, which is in the
    // page's list too.
    let comment = pdf.reserve();
    let popup = pdf.add(format!(
        "<< /Type /Annot /Subtype /Popup /Rect [420 640 600 720] /Parent {comment} 0 R >>"
    ));
    pdf.put(
        comment,
        format!(
            "<< /Type /Annot /Subtype /Highlight /Rect [72 690 400 715] \
             /QuadPoints [72 715 400 715 72 690 400 690] /Popup {popup} 0 R \
             /M (D:202609282109) /Contents (This is the sentence the whole argument turns on.) >>"
        ),
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
            0 => {
                format!(" /Annots [{sticky} 0 R {comment} 0 R {popup} 0 R {silent} 0 R {link} 0 R]")
            }
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

/// A one-page document with two files in it, as a paper carries its data
/// and its code: `data.csv` and `notes.txt`.
pub fn attached_pdf() -> String {
    written("moonowl-fixture-attached.pdf", build_attached)
}

/// What `data.csv` in [`attached_pdf`] holds.
pub const ATTACHED_CSV: &str = "x,y\n1,2\n3,4\n";

fn build_attached() -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let tree = pdf.reserve();
    let page = pdf.reserve();
    let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    let stream = "BT /F1 18 Tf 72 700 Td (A paper with its data.) Tj ET";
    let content = pdf.add(format!(
        "<< /Length {} >>\nstream\n{stream}\nendstream",
        stream.len()
    ));
    let file = |pdf: &mut Pdf, name: &str, body: &str| {
        let stream = pdf.add(format!(
            "<< /Type /EmbeddedFile /Length {} >>\nstream\n{body}\nendstream",
            body.len()
        ));
        pdf.add(format!(
            "<< /Type /Filespec /F ({name}) /UF ({name}) /EF << /F {stream} 0 R >> >>"
        ))
    };
    let csv = file(&mut pdf, "data.csv", ATTACHED_CSV);
    let txt = file(&mut pdf, "notes.txt", "Read me first.");
    pdf.put(
        page,
        format!(
            "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
             /Resources << /Font << /F1 {font} 0 R >> >> /Contents {content} 0 R >>"
        ),
    );
    pdf.put(
        tree,
        format!("<< /Type /Pages /Count 1 /Kids [{page} 0 R] >>"),
    );
    pdf.put(
        catalog,
        format!(
            "<< /Type /Catalog /Pages {tree} 0 R \
             /Names << /EmbeddedFiles << /Names [(data.csv) {csv} 0 R (notes.txt) {txt} 0 R] >> >> >>"
        ),
    );
    pdf.bytes()
}

/// **The four hundred page book**, which is the document every memory and
/// timing number in `plans/dormant/experiments/PROGRESS.md` was taken on, and the one most
/// of this suite reads.
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

fn build(pages: usize, contents: &[Section]) -> Vec<u8> {
    build_with(pages, contents, &[])
}

/// [`build`], with each line of `more` written on its page (one-based) at
/// its height from the bottom of the page.
fn build_with(pages: usize, contents: &[Section], more: &[(usize, u32, &str)]) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let tree = pdf.reserve();
    let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");

    let page_ids: Vec<usize> = (0..pages).map(|_| pdf.reserve()).collect();
    for (index, &id) in page_ids.iter().enumerate() {
        let text = format!("Page {} of the fixture.", index + 1);
        let mut stream = format!("BT /F1 18 Tf 72 700 Td ({text}) Tj ET");
        for (_, height, line) in more.iter().filter(|(page, _, _)| *page == index + 1) {
            stream += &format!(" BT /F1 12 Tf 72 {height} Td ({line}) Tj ET");
        }
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
    let top = write_sections(&mut pdf, contents, outline, &page_ids);
    pdf.put(
        outline,
        format!(
            "<< /Type /Outlines /Count {} {} >>",
            contents.len(),
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
        let page = page_ids[section.page.saturating_sub(1).min(page_ids.len() - 1)];
        let top = section
            .top
            .map_or("null".to_string(), |top| top.to_string());
        let dest = if section.page == 0 {
            "(elsewhere)".to_string()
        } else {
            format!("[{page} 0 R /XYZ null {top} null]")
        };
        let mut body = format!(
            "<< /Title ({}) /Parent {parent} 0 R /Dest {dest}",
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
/// **`tests/fixtures/signed.pdf` is not this.** That one is a
/// `/FT /Sig` widget with `/SigFlags 3` and **no `/V`** — a signature *field*,
/// which is a blank line at the foot of a contract. This has a `/V` holding a
/// real `/Sig` dictionary with the four entries pdfium can read.
///
/// The blob is not a real signature over these bytes and could not be, nothing
/// here being able to make one. It is a well-formed DER prologue, which is
/// enough to be *present* — and being present is the fact this reader
/// reports.
pub fn signed_pdf() -> String {
    written("moonowl-fixture-signed.pdf", || build_signed(true, None, 2))
}

/// [`signed_pdf`] with nine pages, enough to take out more than a reader
/// is asked about.
pub fn signed_long_pdf() -> String {
    written("moonowl-fixture-signed-long.pdf", || {
        build_signed(true, None, 9)
    })
}

/// [`signed_pdf`] saved with object streams and a cross-reference stream,
/// as most files written since PDF 1.5 are.
pub fn signed_modern_pdf() -> String {
    written("moonowl-fixture-signed-modern.pdf", || {
        let mut document = lopdf::Document::load_mem(&build_signed(true, None, 2)).unwrap();
        let mut bytes = Vec::new();
        document.save_modern(&mut bytes).unwrap();
        bytes
    })
}

/// [`signed_pdf`], certified by its author: the signature carries a
/// `/DocMDP` reference whose `/P` is `allowed` (1 nothing may change, 2
/// forms and signatures, 3 annotations too).
pub fn certified_pdf(allowed: i64) -> String {
    written(&format!("moonowl-fixture-certified-{allowed}.pdf"), || {
        build_signed(true, Some(allowed), 2)
    })
}

/// One page with a form on it: a text field "Name" holding "Ada", a
/// field "Notes" of several lines, a check box "Agree" not ticked, a radio
/// group "Size" of "Small" and "Large" with nothing chosen, a read-only
/// field "Code", and a password field "PIN". The buttons carry their own
/// appearances, and
/// the text fields none, so pdfium draws theirs.
pub fn form_pdf() -> String {
    written("moonowl-fixture-form.pdf", || {
        let mut pdf = Pdf::new();
        let catalog = pdf.reserve();
        let tree = pdf.reserve();
        let page = pdf.reserve();
        let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
        let mut look = |stream: &str| {
            pdf.add(format!(
                "<< /Type /XObject /Subtype /Form /BBox [0 0 18 18] /Length {} >>\nstream\n{}\nendstream",
                stream.len(),
                stream
            ))
        };
        let on = look("0 g 4 4 10 10 re f 0 G 0.5 0.5 17 17 re S");
        let off = look("0 G 0.5 0.5 17 17 re S");
        let text = |name: &str, rect: &str, more: &str| {
            format!(
                "<< /Type /Annot /Subtype /Widget /FT /Tx /T ({name}) /Rect [{rect}] /F 4 \
                 /DA (/Helv 12 Tf 0 g) /P {page} 0 R{more} >>"
            )
        };
        let name = pdf.add(text("Name", "72 650 300 674", " /V (Ada)"));
        let notes = pdf.add(text("Notes", "72 420 400 520", " /Ff 4096"));
        let code = pdf.add(text("Code", "72 380 200 404", " /Ff 1 /V (A-1)"));
        let pin = pdf.add(text("PIN", "72 340 200 364", " /Ff 8192"));
        let agree = pdf.add(format!(
            "<< /Type /Annot /Subtype /Widget /FT /Btn /T (Agree) /Rect [72 600 90 618] /F 4 \
             /V /Off /AS /Off /AP << /N << /Yes {on} 0 R /Off {off} 0 R >> >> /P {page} 0 R >>"
        ));
        let size = pdf.reserve();
        let radio = |choice: &str, rect: &str| {
            format!(
                "<< /Type /Annot /Subtype /Widget /Parent {size} 0 R /Rect [{rect}] /F 4 /AS /Off \
                 /AP << /N << /{choice} {on} 0 R /Off {off} 0 R >> >> /P {page} 0 R >>"
            )
        };
        let small = pdf.add(radio("Small", "72 550 90 568"));
        let large = pdf.add(radio("Large", "172 550 190 568"));
        pdf.put(
            size,
            format!("<< /FT /Btn /T (Size) /Ff 49152 /V /Off /Kids [{small} 0 R {large} 0 R] >>"),
        );
        let stream = "BT /F1 12 Tf 72 680 Td (Name) Tj 0 -54 Td (  ) Tj 26 -26 Td (I agree) Tj \
                      0 -50 Td (Small) Tj 100 0 Td (Large) Tj ET";
        let content = pdf.add(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            stream
        ));
        pdf.put(
            page,
            format!(
                "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
                 /Resources << /Font << /F1 {font} 0 R >> >> /Contents {content} 0 R \
                 /Annots [{name} 0 R {notes} 0 R {code} 0 R {agree} 0 R {small} 0 R {large} 0 R {pin} 0 R] >>"
            ),
        );
        pdf.put(
            tree,
            format!("<< /Type /Pages /Count 1 /Kids [{page} 0 R] >>"),
        );
        pdf.put(
            catalog,
            format!(
                "<< /Type /Catalog /Pages {tree} 0 R /AcroForm << \
                 /Fields [{name} 0 R {notes} 0 R {code} 0 R {agree} 0 R {size} 0 R {pin} 0 R] \
                 /DR << /Font << /Helv {font} 0 R >> >> /DA (/Helv 0 Tf 0 g) >> >>"
            ),
        );
        pdf.bytes()
    })
}

/// The same document with the signature field left blank — the shape of
/// `tests/fixtures/signed.pdf`, rebuilt here so that the pair can be compared in one test.
pub fn unsigned_field_pdf() -> String {
    written("moonowl-fixture-blank-field.pdf", || {
        build_signed(false, None, 2)
    })
}

/// A signed and certified document with a text field "Name" on its first
/// page beside the signature, and `allowed` as its certification's `/P`.
pub fn certified_form_pdf(allowed: i64) -> String {
    written(
        &format!("moonowl-fixture-certified-form-{allowed}.pdf"),
        || build_signed_with(true, Some(allowed), 2, true),
    )
}

fn build_signed(filled: bool, certified: Option<i64>, pages: usize) -> Vec<u8> {
    build_signed_with(filled, certified, pages, false)
}

fn build_signed_with(filled: bool, certified: Option<i64>, pages: usize, form: bool) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let tree = pdf.reserve();
    let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    // The signature itself. `/ByteRange` and `/SubFilter` are written because
    // a `/Sig` without them is malformed, and not because anything reads them:
    // pdfium has getters for both and `pdfium-render` exposes neither, and
    // hides the handle they would be called on. See [`crate::sign::Seal`].
    let value = if filled {
        Some(pdf.add(format!(
            "<< /Type /Sig /Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached \
              /Name (A. Reader) /Reason (I agree to the terms) \
              /M (D:20240314093000+01'00') \
              /ByteRange [0 840 1560 620]{reference} \
              /Contents <308006092a864886f70d010702a0803080020101310f300d060960864801650304020105000000> >>",
            reference = certified.map_or_else(String::new, |allowed| format!(
                " /Reference [<< /Type /SigRef /TransformMethod /DocMDP \
                   /TransformParams << /Type /TransformParams /P {allowed} /V /1.2 >> >>]"
            )),
        )))
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
    let name = form.then(|| {
        pdf.add(
            "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Name) /Rect [54 600 300 624] /F 4 \
              /DA (/Helv 12 Tf 0 g) >>",
        )
    });
    let fields = match name {
        Some(name) => format!("{field} 0 R {name} 0 R"),
        None => format!("{field} 0 R"),
    };
    let page_ids: Vec<usize> = (0..pages).map(|_| pdf.reserve()).collect();
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
            format!(" /Annots [{fields}]")
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
            "<< /Type /Pages /Count {pages} /Kids [{}] >>",
            page_ids
                .iter()
                .map(|id| format!("{id} 0 R"))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    );
    let perms = match (certified, value) {
        (Some(_), Some(value)) => format!(" /Perms << /DocMDP {value} 0 R >>"),
        _ => String::new(),
    };
    pdf.put(
        catalog,
        format!(
            "<< /Type /Catalog /Pages {tree} 0 R \
             /AcroForm << /Fields [{fields}] /SigFlags 3 \
             /DR << /Font << /Helv {font} 0 R >> >> >>{perms} >>"
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

/// What the scanned page of [`scanned_pdf`] says, line by line.
pub const SCANNED: &[&str] = &["Moonowl reads scanned pages", "with nothing typed in them"];

/// The line stamped on the third page of [`scanned_pdf`].
pub const STAMP: &str = "Downloaded from an archive on 1 November 2026";

/// Three pages: the first a picture of [`SCANNED`] with no text in it, as a
/// scanner makes, the second a page with text of its own, and the third the
/// picture again with a line stamped across its foot, as an archive stamps
/// what it hands out, the fourth the third again with the picture drawn
/// through a form, as Word and Acrobat's "optimise" write it, and the fifth
/// a page of text with the picture at half its size as a figure in its
/// lower half. The picture is a page of type drawn by pdfium, in grey at
/// 144 dpi.
pub fn scanned_pdf() -> String {
    written("moonowl-fixture-scanned.pdf", build_scanned)
}

fn build_scanned() -> Vec<u8> {
    let typed = written("moonowl-fixture-scanned-type.pdf", || {
        let mut pdf = Pdf::new();
        let catalog = pdf.reserve();
        let tree = pdf.reserve();
        let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
        let stream: String = (SCANNED.iter().enumerate())
            .map(|(at, line)| format!("BT /F1 28 Tf 72 {} Td ({line}) Tj ET\n", 680 - at * 44))
            .collect();
        let content = pdf.add(format!(
            "<< /Length {} >>\nstream\n{stream}\nendstream",
            stream.len()
        ));
        let page = pdf.add(format!(
            "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
             /Resources << /Font << /F1 {font} 0 R >> >> /Contents {content} 0 R >>"
        ));
        pdf.put(
            tree,
            format!("<< /Type /Pages /Count 1 /Kids [{page} 0 R] >>"),
        );
        pdf.put(catalog, format!("<< /Type /Catalog /Pages {tree} 0 R >>"));
        pdf.bytes()
    });
    let (width, height) = (1224, 1584);
    let view = crate::layout::View {
        rotation: 0,
        crop: None,
    };
    let drawn = (crate::render::open(&typed).ok())
        .and_then(|document| document.render_owned(0, width, height, view).ok())
        .expect("the page of type is drawn");
    let grey: Vec<u8> = (drawn.bgra.as_chunks::<4>().0.iter())
        .map(|pixel| pixel[1])
        .collect();
    let mut packed = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    packed.write_all(&grey).expect("packed");
    let packed = packed.finish().expect("packed");

    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let tree = pdf.reserve();
    let mut picture = format!(
        "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
         /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
        packed.len()
    )
    .into_bytes();
    picture.extend_from_slice(&packed);
    picture.extend_from_slice(b"\nendstream");
    let picture = pdf.add(picture);
    let scan = "q 612 0 0 792 0 0 cm /Im1 Do Q";
    let scan = pdf.add(format!(
        "<< /Length {} >>\nstream\n{scan}\nendstream",
        scan.len()
    ));
    let font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    let text = "BT /F1 18 Tf 72 700 Td (This page has text.) Tj ET";
    let text = pdf.add(format!(
        "<< /Length {} >>\nstream\n{text}\nendstream",
        text.len()
    ));
    let first = pdf.add(format!(
        "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
         /Resources << /XObject << /Im1 {picture} 0 R >> >> /Contents {scan} 0 R >>"
    ));
    let second = pdf.add(format!(
        "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
         /Resources << /Font << /F1 {font} 0 R >> >> /Contents {text} 0 R >>"
    ));
    let stamp = format!("q 612 0 0 792 0 0 cm /Im1 Do Q BT /F1 8 Tf 72 30 Td ({STAMP}) Tj ET");
    let stamp = pdf.add(format!(
        "<< /Length {} >>\nstream\n{stamp}\nendstream",
        stamp.len()
    ));
    let third = pdf.add(format!(
        "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
         /Resources << /XObject << /Im1 {picture} 0 R >> /Font << /F1 {font} 0 R >> >> \
         /Contents {stamp} 0 R >>"
    ));
    let form = "q 612 0 0 792 0 0 cm /Im1 Do Q";
    let form = pdf.add(format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 612 792] \
         /Resources << /XObject << /Im1 {picture} 0 R >> >> /Length {} >>\nstream\n{form}\nendstream",
        form.len()
    ));
    let formed = format!("/Fm1 Do BT /F1 8 Tf 72 30 Td ({STAMP}) Tj ET");
    let formed = pdf.add(format!(
        "<< /Length {} >>\nstream\n{formed}\nendstream",
        formed.len()
    ));
    let fourth = pdf.add(format!(
        "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
         /Resources << /XObject << /Fm1 {form} 0 R >> /Font << /F1 {font} 0 R >> >> \
         /Contents {formed} 0 R >>"
    ));
    let figured =
        "BT /F1 18 Tf 72 740 Td (This page has text.) Tj ET q 306 0 0 396 153 60 cm /Im1 Do Q";
    let figured = pdf.add(format!(
        "<< /Length {} >>\nstream\n{figured}\nendstream",
        figured.len()
    ));
    let fifth = pdf.add(format!(
        "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 612 792] \
         /Resources << /XObject << /Im1 {picture} 0 R >> /Font << /F1 {font} 0 R >> >> \
         /Contents {figured} 0 R >>"
    ));
    pdf.put(
        tree,
        format!(
            "<< /Type /Pages /Count 5 /Kids [{first} 0 R {second} 0 R {third} 0 R {fourth} 0 R \
             {fifth} 0 R] >>"
        ),
    );
    pdf.put(catalog, format!("<< /Type /Catalog /Pages {tree} 0 R >>"));
    pdf.bytes()
}
