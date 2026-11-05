//! A comparison written out as a PDF of its own. The first page names the
//! two files, A and B, lists the report's parts, says what its colours
//! mean, and lists what each file says about itself side by side. Then
//! each pair of pages where the two differ is shown side by side, with what
//! only A has marked on A's page and what only B has marked on B's. Last,
//! the list of differences says each in words, two columns to a page. The
//! parts are the file's outline as well.
//!
//! The words are compared in order (see `compare.rs`), so no page of one
//! file has to match a page of the other: the pages shown together are the
//! pages a difference is on in each file.

use std::collections::HashMap;

use pdfium_render::prelude::*;
use unicode_normalization::UnicodeNormalization;

use crate::compare::Change;
use crate::render::{PageSource, Rect};

/// One of the two files compared, as the report describes it.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct File {
    pub path: String,
    pub title: String,
    /// What the Information window lists (`PageSource::details`).
    pub details: Vec<(String, String)>,
    /// What each page is called (`PageSource::labels`).
    pub labels: Vec<String>,
    /// The file's length and time when it was read, so that a report is not
    /// written from a later draft.
    pub stamp: Option<crate::render::Stamp>,
    /// The password it was opened with, if it wants one.
    pub password: Option<String>,
}

impl File {
    /// The file as `document` describes it.
    pub fn of(document: &dyn PageSource) -> File {
        File {
            path: document.path().to_string(),
            title: document.title(),
            details: document.details(),
            labels: document.labels(),
            stamp: document.stamp(),
            password: document.password().map(str::to_string),
        }
    }

    pub fn name(&self) -> String {
        crate::store::file_name(&self.path)
    }

    /// Whether the file on disk is still the one that was compared, as it
    /// is asked before the file is read and again after.
    pub fn unchanged(&self, stamp: Option<crate::render::Stamp>) -> Result<(), String> {
        match stamp == self.stamp {
            true => Ok(()),
            false => Err(format!(
                "{} changed on disk after it was compared. Compare again, then save the report.",
                self.name()
            )),
        }
    }

    pub(crate) fn label(&self, page: usize) -> String {
        match self.labels.get(page.wrapping_sub(1)) {
            Some(label) if !label.is_empty() => label.clone(),
            _ => page.to_string(),
        }
    }

    /// The rows of its column in the table of details: the title, what the
    /// document says about itself, and the file's name. A report is sent to
    /// other people, so the folder the file is in is left out.
    fn rows(&self) -> Vec<(String, String)> {
        let mut rows = vec![("Title".to_string(), self.title.trim().to_string())];
        rows.extend(self.details.iter().cloned());
        rows.push(("File".into(), self.name()));
        if let Ok(about) = std::fs::metadata(&self.path) {
            rows.push(("Size".into(), size_in_words(about.len())));
        }
        rows
    }
}

/// A number of bytes as people say it: "312 KB", "4.2 MB".
fn size_in_words(bytes: u64) -> String {
    match bytes {
        0..1_000 => format!("{bytes} bytes"),
        1_000..1_000_000 => format!("{} KB", bytes / 1_000),
        _ => format!("{:.1} MB", bytes as f64 / 1e6),
    }
}

/// The size of a page of text, A4.
const PAPER: (f32, f32) = (595.0, 842.0);
const MARGIN: f32 = 56.0;
/// The distance from one line to the next, as a multiple of the type's size.
const LEADING: f32 = 1.4;
/// Around and between the two pages of a sheet, and above them for their
/// names, in points.
const SHEET_MARGIN: f32 = 28.0;
const SHEET_GAP: f32 = 24.0;
const SHEET_HEAD: f32 = 22.0;
/// The size of a difference's number in the margin of a page side by side.
const TAG: f32 = 7.0;
/// The list's two columns: the space between them, and the width of each.
const COLUMN_GAP: f32 = 24.0;
const COLUMN: f32 = (PAPER.0 - 2.0 * MARGIN - COLUMN_GAP) / 2.0;
/// How many words of each side of a difference the list shows.
const SUMMED: usize = 50;

pub type Colour = (u8, u8, u8);
const INK: Colour = (0x1f, 0x23, 0x28);
/// The grey of "No title", which stands where a file gives no title.
const PLACEHOLDER: Colour = (0x6b, 0x72, 0x80);

/// The colours a report marks its differences in, chosen by the reader.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Colours {
    /// Words only in A.
    pub a: Colour,
    /// Words only in B.
    pub b: Colour,
    /// Words that are in both files, in different places.
    pub moved: Colour,
}

/// The colours a report uses unless the reader chooses others. Words only
/// in A are red and words only in B are green, as Git shows a difference,
/// and words that moved are blue.
pub const COLOURS: Colours = Colours {
    a: (0xb4, 0x23, 0x18),
    b: (0x1a, 0x7f, 0x37),
    moved: (0x4f, 0x46, 0xc8),
};

/// A mark over a page's words in `colour`: a pale shade of it, multiplied
/// with the page, so that the words stay as dark as they were.
fn mark((red, green, blue): Colour) -> Colour {
    let pale = |c: u8| (255.0 - (255.0 - c as f32) * 0.3).round() as u8;
    (pale(red), pale(green), pale(blue))
}
/// The edge drawn around each page of a sheet.
const EDGE: Colour = (0xc8, 0xcd, 0xd5);
const LINK: Colour = (0x1d, 0x5f, 0xc4);
/// `FPDF_ANNOT_LINK`, which pdfium-render does not export.
const LINK_SUBTYPE: FPDF_ANNOTATION_SUBTYPE = 2;
/// Where Moonowl is, named on the first page.
const HOME: &str = "https://github.com/lhdjung/moonowl";

/// Where a line of the report leads when it is clicked.
#[derive(Clone, Debug, PartialEq)]
enum Link {
    Away(&'static str),
    /// A page of the report, one-based.
    Page(usize),
    /// Where difference `change`, counting from nought, is marked on the
    /// pages side by side: on B's page if `b`, and on A's otherwise.
    Mark {
        change: usize,
        b: bool,
    },
}

/// Text to be wrapped inside `width` points, `x` points from the margin.
struct Cell {
    x: f32,
    width: f32,
    text: String,
    colour: Colour,
    /// Where it leads, if not where its row leads.
    link: Option<Link>,
}

/// One or more cells set side by side, line for line.
struct Row {
    size: f32,
    /// Space above the row, in points.
    before: f32,
    /// How many lines after its first must fit on the page with it, so that
    /// a heading is not left alone at the bottom of a page.
    keep: usize,
    cells: Vec<Cell>,
    /// Where each of its lines leads.
    link: Option<Link>,
}

impl Row {
    fn text(size: f32, before: f32, colour: Colour, text: impl Into<String>) -> Row {
        Row::at(0.0, size, before, colour, text)
    }

    fn at(x: f32, size: f32, before: f32, colour: Colour, text: impl Into<String>) -> Row {
        Row {
            size,
            before,
            keep: 0,
            cells: vec![Cell {
                x,
                width: PAPER.0 - 2.0 * MARGIN - x,
                text: text.into(),
                colour,
                link: None,
            }],
            link: None,
        }
    }

    fn keeping(self, keep: usize) -> Row {
        Row { keep, ..self }
    }

    fn leading_to(self, link: Link) -> Row {
        Row {
            link: Some(link),
            ..self
        }
    }
}

/// A line of text where it goes on its page: `y` is its baseline, from the
/// top of the page.
#[derive(Debug, PartialEq)]
struct Placed {
    x: f32,
    y: f32,
    size: f32,
    colour: Colour,
    text: String,
    link: Option<Link>,
}

/// One half of a page of the report: a page of A or of B, one-based, and
/// where that page is already shown on another of the pages side by side,
/// which one, counting from the first. The half is then left blank.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Half {
    page: usize,
    elsewhere: Option<usize>,
}

/// The pages shown side by side, a page of A and a page of B, and for each
/// change the pages its first page in A and its first page in B are shown
/// on. These are the same page unless one of the two was shown earlier.
/// Each change is shown beside the pages it is on in the other file, and
/// no page is shown twice:
/// - A change over more pages in one file than in the other is shown with
///   the last of its pages in the other file beside the first of the rest,
///   and a blank half beside the others.
/// - A pair of pages whose two pages are both shown on the pairs either
///   side of it is left out: a change on A's page 2 and B's page 1, between
///   one on the first pages and one on the second pages, is shown on those.
fn pairs(changes: &[Change]) -> (Vec<[Half; 2]>, Vec<[usize; 2]>) {
    let pages = |places: &[(usize, usize, usize)]| {
        let mut pages: Vec<usize> = places.iter().map(|&(page, _, _)| page).collect();
        pages.dedup();
        pages
    };
    let (mut pairs, mut first) = (Vec::new(), Vec::with_capacity(changes.len()));
    for change in changes {
        let (a, b) = (pages(&change.places_there), pages(&change.places));
        let mut shown = None;
        for at in 0..a.len().max(b.len()) {
            let pair = (a[at.min(a.len() - 1)], b[at.min(b.len() - 1)]);
            if pairs.last() != Some(&pair) {
                pairs.push(pair);
            }
            shown.get_or_insert(pairs.len() - 1);
        }
        first.push(shown.unwrap_or(0));
    }
    // Never two left out in a row, so that the one after a pair left out is
    // always kept.
    let mut kept = vec![true; pairs.len()];
    for at in 1..pairs.len().saturating_sub(1) {
        let (before, (a, b), after) = (pairs[at - 1], pairs[at], pairs[at + 1]);
        if kept[at - 1] && (a == before.0 || a == after.0) && (b == before.1 || b == after.1) {
            kept[at] = false;
        }
    }
    let mut sheets: Vec<[Half; 2]> = Vec::new();
    let (mut shown_a, mut shown_b) = (HashMap::new(), HashMap::new());
    for (at, &(a, b)) in pairs.iter().enumerate() {
        if !kept[at] {
            continue;
        }
        let half = |page: usize, shown: &HashMap<usize, usize>| Half {
            page,
            elsewhere: shown.get(&page).copied(),
        };
        let (on_a, on_b) = (half(a, &shown_a), half(b, &shown_b));
        if on_a.elsewhere.is_some() && on_b.elsewhere.is_some() {
            continue;
        }
        shown_a.entry(a).or_insert(sheets.len());
        shown_b.entry(b).or_insert(sheets.len());
        sheets.push([on_a, on_b]);
    }
    // Every page of a pair is shown somewhere, on its own sheet or on a
    // sheet either side of a pair left out, so a change's pages are
    // found where they are shown.
    let sheet = |page: usize, shown: &HashMap<usize, usize>| shown.get(&page).copied().unwrap_or(0);
    let first = (first.into_iter())
        .map(|at| match pairs.get(at) {
            Some(&(a, b)) => [sheet(a, &shown_a), sheet(b, &shown_b)],
            None => [0; 2],
        })
        .collect();
    (sheets, first)
}

/// How far in from the margin a line of a list begins, after its bullet.
const BULLET: f32 = 16.0;

/// What the report's first part is called, in its contents and its outline.
const SUMMARY: &str = "Summary";

/// The first page: the two files, what the report holds, how to read its
/// pages, and what each file says about itself. `sheets_from` and
/// `list_from` are the report's pages the pages side by side and the list
/// of differences begin on, and `replaced` how many characters no font had.
fn cover(
    there: &File,
    here: &File,
    changes: &[Change],
    colours: &Colours,
    (sheets_from, list_from): (usize, usize),
    replaced: (usize, &str),
) -> Vec<Row> {
    let mut rows = vec![Row::text(22.0, 0.0, INK, "Comparison report")];
    for (letter, colour, file, before) in
        [("A", colours.a, there, 14.0), ("B", colours.b, here, 10.0)]
    {
        // The title the file gives itself, and its name on the disk below.
        let (shown, shade) = match file.title.trim() {
            "" => ("No title", PLACEHOLDER),
            title => (title, INK),
        };
        rows.push(Row {
            size: 15.0,
            before,
            keep: 1,
            cells: vec![
                Cell {
                    x: 0.0,
                    width: 18.0,
                    text: letter.into(),
                    colour,
                    link: None,
                },
                Cell {
                    x: 22.0,
                    width: PAPER.0 - 2.0 * MARGIN - 22.0,
                    text: shown.to_string(),
                    colour: shade,
                    link: None,
                },
            ],
            link: None,
        });
        rows.push(Row::at(22.0, 9.5, 2.0, INK, file.name()));
    }
    rows.push(Row::text(
        10.0,
        14.0,
        INK,
        format!(
            "Made by Moonowl {} on {}.",
            env!("CARGO_PKG_VERSION"),
            crate::sign::today()
        ),
    ));
    rows.push(Row::text(10.0, 2.0, LINK, HOME).leading_to(Link::Away(HOME)));

    // A difference in the wording, from one letter to whole sentences, or
    // one only in punctuation, hyphens or spaces.
    let stayed = || changes.iter().filter(|change| change.moved.is_none());
    let in_wording = stayed()
        .filter(|change| change.count + change.count_there > 0)
        .count();
    let in_marks = stayed().count() - in_wording;
    // A passage that moved is two differences, where it was and where it
    // is, and the runs of one passage with words changed inside it are one
    // passage.
    let halves = changes.len() - stayed().count();
    let moved = (changes.iter().filter_map(|change| change.passage))
        .collect::<std::collections::HashSet<_>>()
        .len();
    let differences = |n: usize| if n == 1 { "difference" } else { "differences" };
    let passages = if moved == 1 { "passage" } else { "passages" };
    // How many differences there are, and under it a line for each kind.
    let kinds: Vec<String> = [
        (
            in_wording,
            format!("{in_wording} in the wording, from a single letter to whole sentences"),
        ),
        (
            in_marks,
            format!("{in_marks} only in punctuation, hyphens or spaces"),
        ),
        (
            halves,
            format!(
                "{halves} {} as a result of moving passages ({moved} {passages} moved)",
                differences(halves)
            ),
        ),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(_, said)| said)
    .collect();
    let n = changes.len();
    rows.push(Row::text(
        10.5,
        18.0,
        INK,
        match n {
            0 => "The text of the two files is the same.".to_string(),
            n => format!("{n} {} in total", differences(n)),
        },
    ));
    for kind in kinds {
        let mut row = Row::at(BULLET, 10.5, 3.0, INK, kind);
        row.cells.insert(
            0,
            Cell {
                x: 4.0,
                width: BULLET,
                text: "\u{2022}".into(),
                colour: INK,
                link: None,
            },
        );
        rows.push(row);
    }
    if !changes.is_empty() {
        // Each part of the report, this one too, with the page it begins on.
        rows.push(Row::text(13.0, 22.0, INK, "Contents").keeping(2));
        for (part, at) in [
            (SUMMARY, 1),
            ("Pages side by side", sheets_from),
            ("List of differences", list_from),
        ] {
            let pages = format!("p. {at}");
            rows.push(Row {
                size: 10.5,
                before: 4.0,
                keep: 0,
                cells: vec![
                    Cell {
                        x: 0.0,
                        width: 240.0,
                        text: part.into(),
                        colour: LINK,
                        link: None,
                    },
                    Cell {
                        x: 250.0,
                        width: 80.0,
                        text: pages,
                        colour: LINK,
                        link: None,
                    },
                ],
                link: Some(Link::Page(at)),
            });
        }

        // What the colours on the pages mean, each said in its colour.
        rows.push(Row::text(13.0, 22.0, INK, "How to read the pages").keeping(2));
        let mut key = vec![
            (
                "Only in A",
                colours.a,
                "What only A has, marked on A\u{2019}s pages: words, punctuation, hyphens \
                 or spaces. On B\u{2019}s pages, a thin bar in this colour shows where it \
                 would be.",
            ),
            (
                "Only in B",
                colours.b,
                "What only B has, marked on B\u{2019}s pages: words, punctuation, hyphens \
                 or spaces. On A\u{2019}s pages, a thin bar in this colour shows where it \
                 would be.",
            ),
        ];
        if moved > 0 {
            key.push((
                "Moved",
                colours.moved,
                "A passage of three words or more that both files have, in different \
                 places. It is marked in both. Words changed within it are marked as \
                 other changes are.",
            ));
        }
        key.push((
            "Numbers",
            INK,
            "Beside each difference on the pages side by side, its number in the list \
             of differences. In the list, a click on A or on B goes to it on that \
             file\u{2019}s page.",
        ));
        key.push((
            "Small changes",
            INK,
            "Where a word, a punctuation mark, a hyphen or a space takes the place of \
             another, only the characters that differ are marked. The list shows the \
             words around them whole.",
        ));
        key.push((
            "Not compared",
            INK,
            "Running heads, page numbers and line numbers, capital letters, the raised \
             marks of footnotes and affiliations, hyphens where a word is broken across \
             two lines, spaces before punctuation, the shapes of quotation marks, \
             apostrophes and dashes, pictures, layout and annotations.",
        ));
        key.push((
            "Cut short",
            INK,
            "A difference longer than about fifty words, in the list. The pages show \
             all of it.",
        ));
        for (named, colour, said) in key {
            rows.push(Row {
                size: 10.0,
                before: 6.0,
                keep: 0,
                cells: vec![
                    Cell {
                        x: 0.0,
                        width: 84.0,
                        text: named.into(),
                        colour,
                        link: None,
                    },
                    Cell {
                        x: 90.0,
                        width: PAPER.0 - 2.0 * MARGIN - 90.0,
                        text: said.into(),
                        colour: INK,
                        link: None,
                    },
                ],
                link: None,
            });
        }
    }
    match replaced {
        (0, _) => {}
        (1, shown) => rows.push(Row::text(
            10.0,
            6.0,
            INK,
            format!("1 character that no font on this computer has is shown as {shown}."),
        )),
        (n, shown) => rows.push(Row::text(
            10.0,
            6.0,
            INK,
            format!("{n} characters that no font on this computer has are shown as {shown}."),
        )),
    }

    // What each file says about itself, side by side.
    rows.push(Row::text(13.0, 22.0, INK, "The files").keeping(2));
    let (label, column) = (90.0, (PAPER.0 - 2.0 * MARGIN - 90.0 - 12.0) / 2.0);
    let side_by_side = |size, before, colours: [Colour; 3], cells: [String; 3]| {
        let xs = [0.0, label, label + column + 12.0];
        let widths = [label - 6.0, column, column];
        Row {
            size,
            before,
            keep: 0,
            cells: (cells.into_iter().zip(colours).enumerate())
                .map(|(at, (text, colour))| Cell {
                    x: xs[at],
                    width: widths[at],
                    text,
                    colour,
                    link: None,
                })
                .collect(),
            link: None,
        }
    };
    rows.push(
        side_by_side(
            9.5,
            8.0,
            [INK, colours.a, colours.b],
            ["Shorthand".into(), "A".into(), "B".into()],
        )
        .keeping(1),
    );
    let (rows_there, rows_here) = (there.rows(), here.rows());
    let mut labels: Vec<&str> = Vec::new();
    for (named, _) in rows_there.iter().chain(&rows_here) {
        if !labels.contains(&named.as_str()) {
            labels.push(named);
        }
    }
    let value = |rows: &[(String, String)], named: &str| {
        (rows.iter())
            .find(|(label, _)| label == named)
            .map_or_else(String::new, |(_, value)| value.trim().to_string())
    };
    let mut differ = Vec::new();
    for named in labels {
        let (one, two) = (value(&rows_there, named), value(&rows_here, named));
        if one.is_empty() && two.is_empty() {
            continue;
        }
        if one != two {
            differ.push(named.to_string());
        }
        rows.push(side_by_side(
            9.5,
            3.0,
            [INK, INK, INK],
            [named.to_string(), one, two],
        ));
    }
    rows.push(Row::text(
        10.0,
        10.0,
        INK,
        match differ.len() {
            0 => "The files say the same about themselves.".to_string(),
            _ => format!("These details differ: {}.", in_a_list(&differ)),
        },
    ));
    rows
}

/// Every difference in words, each leading to the report's page it is
/// shown on, counting from `sheets_from`. Set in two columns, so each row is
/// a column wide, and a long difference is cut short after [`SUMMED`] words.
fn list(
    there: &File,
    here: &File,
    changes: &[Change],
    colours: &Colours,
    first: &[[usize; 2]],
    sheets_from: usize,
    measure: &dyn Fn(&str, f32) -> f32,
) -> Vec<Row> {
    let line = |size: f32, before: f32, colour: Colour, text: String| {
        let mut row = Row::text(size, before, colour, text);
        row.cells[0].width = COLUMN;
        row
    };
    let summed = |text: &str| {
        let mut words = text.split_whitespace();
        let mut said = words.by_ref().take(SUMMED).collect::<Vec<_>>().join(" ");
        if words.next().is_some() {
            said.push('\u{2026}');
        }
        said
    };
    let mut rows = vec![
        line(13.0, 0.0, INK, "List of differences".into()).keeping(2),
        line(
            9.5,
            4.0,
            INK,
            "Each difference begins with the page it is on in A and in B, and after \
             the bar, the page of this report that shows them side by side: \
             \u{201c}A 10, B 8 | p. 12\u{201d}. Where no one page shows both, the \
             bar is followed by the page that shows A and the page that shows B. \
             A click on A or on B goes to the \
             difference on that page, where its number stands beside it."
                .into(),
        ),
    ];
    for (number, change) in changes.iter().enumerate() {
        let [on_a, on_b] = first[number].map(|sheet| sheets_from + sheet);
        // "12. A 10, B 8 | p. 12", each part leading somewhere of its own:
        // the difference on A's page, on B's page, and the page that shows
        // them side by side, or the two pages that show them.
        let mut parts = vec![
            (
                format!("{}. A {},", number + 1, there.label(change.page_there())),
                Link::Mark {
                    change: number,
                    b: false,
                },
            ),
            (
                format!("B {}", here.label(change.page())),
                Link::Mark {
                    change: number,
                    b: true,
                },
            ),
        ];
        if on_a == on_b {
            parts.push((format!("| p. {on_a}"), Link::Page(on_a)));
        } else {
            parts.push((format!("| p. {on_a},"), Link::Page(on_a)));
            parts.push((format!("p. {on_b}"), Link::Page(on_b)));
        }
        let mut x = 0.0;
        let mut cells = Vec::with_capacity(parts.len());
        for (text, link) in parts {
            // Each part is written with a space after it.
            let width = measure(&format!("{text} "), 9.5);
            cells.push(Cell {
                x,
                width: width + 1.0,
                text,
                colour: INK,
                link: Some(link),
            });
            x += width;
        }
        rows.push(Row {
            size: 9.5,
            before: 10.0,
            keep: 1,
            cells,
            link: None,
        });
        let (red, green) = match change.moved {
            Some(_) => (colours.moved, colours.moved),
            None => (colours.a, colours.b),
        };
        if !change.there.is_empty() {
            rows.push(line(
                10.0,
                2.0,
                red,
                format!("A: {}", summed(&change.there)),
            ));
        }
        if !change.here.is_empty() {
            rows.push(line(
                10.0,
                2.0,
                green,
                format!("B: {}", summed(&change.here)),
            ));
        }
        if let Some(other) = change.moved {
            let (from, to) = match change.here.is_empty() {
                true => ("A", "B"),
                false => ("B", "A"),
            };
            rows.push(line(
                9.5,
                2.0,
                INK,
                format!(
                    "These words of {from} are in {to} at difference {}.",
                    other + 1
                ),
            ));
        }
    }
    rows
}

/// The items in quotes, as “a” and “b”, or “a”, “b”, and “c”.
pub(crate) fn in_a_list(items: &[String]) -> String {
    let quoted: Vec<String> = (items.iter())
        .map(|item| format!("\u{201c}{item}\u{201d}"))
        .collect();
    match quoted.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [one, two] => format!("{one} and {two}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

/// `text` broken into lines no wider than `width`, where `measure` says how
/// wide a piece of text is. A word wider than a line is broken where it
/// reaches the edge.
fn wrap(text: &str, width: f32, measure: &dyn Fn(&str) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let joined = match line.is_empty() {
            true => word.to_string(),
            false => format!("{line} {word}"),
        };
        if measure(&joined) <= width {
            line = joined;
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        for character in word.chars() {
            line.push(character);
            if measure(&line) > width && line.chars().count() > 1 {
                line.pop();
                lines.push(std::mem::replace(&mut line, character.to_string()));
            }
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// The rows set on pages: each page's lines. The rows run down each of a
/// page's `columns` in turn, each column [`COLUMN_GAP`] from the next.
// ponytail: each line is measured afresh as words are added to it, which is
// quadratic in the words of a line, and a line has a dozen.
fn set(rows: &[Row], measure: &dyn Fn(&str, f32) -> f32, columns: usize) -> Vec<Vec<Placed>> {
    let bottom = PAPER.1 - MARGIN;
    let across = (PAPER.0 - 2.0 * MARGIN + COLUMN_GAP) / columns as f32;
    let mut pages: Vec<Vec<Placed>> = vec![Vec::new()];
    let (mut y, mut column) = (MARGIN, 0);
    let mut next_column = |pages: &mut Vec<Vec<Placed>>, y: &mut f32| {
        (*y, column) = (MARGIN, column + 1);
        if column == columns {
            column = 0;
            pages.push(Vec::new());
        }
        column
    };
    let mut x = MARGIN;
    for row in rows {
        let step = row.size * LEADING;
        let cells: Vec<Vec<String>> = (row.cells.iter())
            .map(|cell| wrap(&cell.text, cell.width, &|text| measure(text, row.size)))
            .collect();
        let lines = cells.iter().map(Vec::len).max().unwrap_or(0);
        let first = y == MARGIN;
        let before = if first { 0.0 } else { row.before };
        if !first && y + before + step * (1 + row.keep) as f32 > bottom {
            x = MARGIN + next_column(&mut pages, &mut y) as f32 * across;
        } else {
            y += before;
        }
        for line in 0..lines {
            if y + step > bottom {
                x = MARGIN + next_column(&mut pages, &mut y) as f32 * across;
            }
            let page = pages.last_mut().expect("there is always a page");
            for (cell, wrapped) in row.cells.iter().zip(&cells) {
                if let Some(text) = wrapped.get(line).filter(|text| !text.is_empty()) {
                    page.push(Placed {
                        x: x + cell.x,
                        y: y + row.size,
                        size: row.size,
                        colour: cell.colour,
                        text: text.clone(),
                        link: cell.link.clone().or_else(|| row.link.clone()),
                    });
                }
            }
            y += step;
        }
    }
    pages
}

/// The font the report is set in, and what it can show.
struct Type<'a> {
    face: ttf_parser::Face<'a>,
    /// What a character the font has no glyph for is shown as.
    missing: &'static str,
}

impl Type<'_> {
    fn has(&self, character: char) -> bool {
        self.face.glyph_index(character).is_some()
    }

    /// The characters the report shows, which the font is cut down to: the
    /// `letters` of its text, the plain forms [`Type::shown`] puts in place
    /// of some of them, and those of the page numbers.
    fn used(&self, letters: &str) -> String {
        let mut used: Vec<char> = letters.chars().collect();
        used.extend(letters.chars().flat_map(|c| std::iter::once(c).nfkc()));
        used.extend(self.missing.chars());
        used.extend("Page of 0123456789".chars());
        used.sort_unstable();
        used.dedup();
        used.into_iter().filter(|&c| self.has(c)).collect()
    }

    /// `text` as the font can show it. A character it has no glyph for is
    /// shown in its plain form where it has one, such as a mathematical
    /// italic x as x, and otherwise as [`Type::missing`], counted in
    /// `replaced`.
    fn shown(&self, text: &str, replaced: &mut usize) -> String {
        if text.chars().all(|c| c == ' ' || self.has(c)) {
            return text.to_string();
        }
        let mut shown = String::with_capacity(text.len());
        for character in text.chars() {
            if character.is_whitespace() {
                shown.push(' ');
            } else if self.has(character) {
                shown.push(character);
            } else {
                let plain: String = std::iter::once(character).nfkc().collect();
                if plain.chars().all(|c| c != character && self.has(c)) {
                    shown.push_str(&plain);
                } else {
                    shown.push_str(self.missing);
                    *replaced += 1;
                }
            }
        }
        shown
    }

    /// How wide `text` is at `size` points.
    fn width(&self, text: &str, size: f32) -> f32 {
        let per_em = self.face.units_per_em() as f32;
        (text.chars())
            .map(|c| {
                (self.face.glyph_index(c))
                    .and_then(|glyph| self.face.glyph_hor_advance(glyph))
                    .unwrap_or(0) as f32
            })
            .sum::<f32>()
            * size
            / per_em
    }
}

/// The report comparing `there`, which is A, with `here`, which is B, as
/// the bytes of a PDF, marked in `colours`. Each file is read from the disk
/// once, and refused if it is not as it was when it was compared, so its
/// pages and the text that places the marks on them are those compared.
/// Beside the bytes is what the report came out without.
pub fn write(
    there: &File,
    here: &File,
    changes: &[Change],
    colours: &Colours,
) -> Result<(Vec<u8>, Lacking), String> {
    write_with(there, here, changes, colours, |bytes, work| {
        crate::objects::rework(bytes, work).map_err(|refused| refused.bytes)
    })
}

/// A report to be saved: what [`write`] is given, and where it goes.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Asked {
    pub there: File,
    pub here: File,
    pub changes: Vec<Change>,
    pub colours: Colours,
    pub to: String,
}

/// The report written to `asked.to`, and what it came out without. It is
/// made in the process that does lopdf's work, which has a pdfium of its
/// own and writes the file itself: a report holds every page it shows and
/// is saved in one go, which would keep pdfium's lock from every window
/// for as long, and keep the memory it took. Where that process cannot be
/// started, it is made here.
pub fn save(asked: Asked) -> Result<Lacking, String> {
    crate::objects::report(asked).unwrap_or_else(|asked| {
        save_with(&asked, |bytes, work| {
            crate::objects::rework(bytes, work).map_err(|refused| refused.bytes)
        })
    })
}

/// [`save`], with `rework` for lopdf's part.
pub(crate) fn save_with(
    asked: &Asked,
    rework: impl FnOnce(
        Vec<u8>,
        &crate::objects::Work,
    ) -> Result<(Vec<u8>, crate::objects::Done), Vec<u8>>,
) -> Result<Lacking, String> {
    let Asked {
        there,
        here,
        changes,
        colours,
        to,
    } = asked;
    let (bytes, lacking) = write_with(there, here, changes, colours, rework)?;
    crate::atomic_write(std::path::Path::new(to), &bytes)?;
    Ok(lacking)
}

/// [`write`], with `rework` for lopdf's part.
fn write_with(
    there: &File,
    here: &File,
    changes: &[Change],
    colours: &Colours,
    rework: impl FnOnce(
        Vec<u8>,
        &crate::objects::Work,
    ) -> Result<(Vec<u8>, crate::objects::Done), Vec<u8>>,
) -> Result<(Vec<u8>, Lacking), String> {
    let (pairs, first) = pairs(changes);
    // What is written above each half of the pages side by side, which
    // begin at the report's page `sheets_from`.
    let heads = |sheets_from: usize| -> Vec<[String; 2]> {
        let head = |letter: &str, file: &File, half: &Half| match half.elsewhere {
            None => format!("{letter}, page {}", file.label(half.page)),
            Some(sheet) => format!(
                "{letter}, page {} (see p. {})",
                file.label(half.page),
                sheets_from + sheet
            ),
        };
        (pairs.iter())
            .map(|[a, b]| [head("A", there, a), head("B", here, b)])
            .collect()
    };

    // The type is the system font that has the most of the letters the
    // report uses, cut down to those letters.
    let mut letters: Vec<char> = Vec::new();
    {
        let mut seen = std::collections::HashSet::new();
        let rows = (cover(there, here, changes, colours, (2, 3), (0, "")).into_iter()).chain(list(
            there,
            here,
            changes,
            colours,
            &first,
            3,
            &|text: &str, size: f32| text.chars().count() as f32 * size,
        ));
        for row in rows {
            for cell in row.cells {
                letters.extend(cell.text.chars().filter(|&c| seen.insert(c)));
            }
        }
        for head in heads(3).iter().flatten() {
            letters.extend(head.chars().filter(|&c| seen.insert(c)));
        }
    }
    let letters: String = letters.into_iter().collect();
    let mut fonts = crate::sign::SystemFonts::default();
    let found = (fonts.most_of(&letters))
        .ok_or("No font was found on this computer to write the report in.")?;
    let font = fonts.take(found);
    drop(fonts);
    let face = ttf_parser::Face::parse(&font, 0).map_err(|e| format!("the font: {e}"))?;
    let missing = match face.glyph_index('\u{fffd}') {
        Some(_) => "\u{fffd}",
        None => "[?]",
    };
    let face = Type { face, missing };
    let measure = |text: &str, size: f32| face.width(text, size);

    // The first page and the list say which pages follow them, and the
    // first page how many characters no font had. Each can change how many
    // pages they take, so they are set again until they say what they hold.
    let (mut covers, mut lists, mut replaced) = (1, usize::from(!changes.is_empty()), 0);
    let (cover_pages, list_pages, heads) = 'set: {
        for _ in 0..5 {
            let mut counted = 0;
            let mut shown = |rows: &mut Vec<Row>| {
                for cell in rows.iter_mut().flat_map(|row| &mut row.cells) {
                    cell.text = face.shown(&cell.text, &mut counted);
                }
            };
            let (sheets_from, list_from) = (covers + 1, covers + pairs.len() + 1);
            let mut cover_rows = cover(
                there,
                here,
                changes,
                colours,
                (sheets_from, list_from),
                (replaced, missing),
            );
            let mut list_rows = list(there, here, changes, colours, &first, sheets_from, &measure);
            shown(&mut cover_rows);
            shown(&mut list_rows);
            let heads: Vec<[String; 2]> = (heads(sheets_from).into_iter())
                .map(|head| head.map(|text| face.shown(&text, &mut counted)))
                .collect();
            let cover_pages = set(&cover_rows, &measure, 1);
            let list_pages = match changes.is_empty() {
                true => Vec::new(),
                false => set(&list_rows, &measure, 2),
            };
            if (counted, cover_pages.len(), list_pages.len()) == (replaced, covers, lists) {
                break 'set (cover_pages, list_pages, heads);
            }
            (covers, lists, replaced) = (cover_pages.len(), list_pages.len(), counted);
        }
        return Err("The report's first pages could not be set.".into());
    };
    let total = cover_pages.len() + pairs.len() + list_pages.len();
    let footer = |number: usize| Placed {
        x: MARGIN,
        y: PAPER.1 - MARGIN / 2.0,
        size: 8.5,
        colour: INK,
        text: format!("Page {number} of {total}"),
        link: None,
    };

    let used = face.used(&letters);
    let cut = crate::sign::subset(&font, &used).unwrap_or_else(|| font.clone());

    // Where each file's changes are on its pages.
    let on_pages = |side: fn(&Change) -> &[(usize, usize, usize)]| {
        let mut on: HashMap<usize, Vec<(usize, usize, bool, usize)>> = HashMap::new();
        for (number, change) in changes.iter().enumerate() {
            for &(page, from, to) in side(change) {
                (on.entry(page).or_default()).push((from, to, change.moved.is_some(), number));
            }
        }
        on
    };
    let marks_there = on_pages(|change| &change.places_there);
    let marks_here = on_pages(|change| &change.places);

    let pdfium = crate::pdfium::pdfium()?;
    let failed = |e: PdfiumError| format!("the report could not be written: {e}");
    let source = |file: &File, letter: &str| -> Result<Option<PdfDocument<'static>>, String> {
        if pairs.is_empty() {
            return Ok(None);
        }
        let bytes = std::fs::read(&file.path)
            .map_err(|e| format!("{} could not be read: {e}", file.name()))?;
        // The pages are read from the disk, and the marks are put on them
        // where the words were when the file was compared.
        file.unchanged(crate::render::stamp_of(&file.path))?;
        let _held = crate::pdfium::library();
        (pdfium
            .load_pdf_from_byte_vec(bytes, file.password.as_deref())
            .map(Some))
        .map_err(|e| match e {
            PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
                format!("{letter} is encrypted, so its pages cannot be put into a report.")
            }
            e => format!("{} could not be read: {e}", file.name()),
        })
    };
    // A document is closed under pdfium's lock, on the way out too.
    let source_there = source(there, "A")?;
    let source_here = match source(here, "B") {
        Ok(source) => source,
        Err(e) => {
            let _held = crate::pdfium::library();
            drop(source_there);
            return Err(e);
        }
    };
    let mut document = {
        let _held = crate::pdfium::library();
        match pdfium.create_new_pdf() {
            Ok(document) => document,
            Err(e) => {
                drop(source_there);
                drop(source_here);
                return Err(format!("a new document could not be made: {e}"));
            }
        }
    };
    let bindings = pdfium.bindings();

    // pdfium's lock is taken for one page at a time, so the pages being
    // drawn for the window go between them. A page's text is read first,
    // since reading it takes the lock too.
    let written = (|| -> Result<(Vec<u8>, Vec<crate::outline::Relink>), String> {
        let font = {
            let _held = crate::pdfium::library_when_free();
            (document.fonts_mut().load_true_type_from_bytes(&cut, true)).map_err(failed)?
        };
        let text = |document: &PdfDocument<'static>, line: &Placed, height: f32| {
            // pdfium puts no space between two text objects when it reads a
            // page's text, so each line carries its own.
            let mut object = PdfPageTextObject::new(
                document,
                format!("{} ", line.text),
                font,
                PdfPoints::new(line.size),
            )?;
            let (red, green, blue) = line.colour;
            object.set_fill_color(PdfColor::new(red, green, blue, 255))?;
            object.translate(PdfPoints::new(line.x), PdfPoints::new(height - line.y))?;
            Ok::<_, PdfiumError>(object)
        };
        // The links to the report's own pages are written by lopdf, after
        // pdfium has saved: pdfium makes a link to an address only.
        let mut relinks = Vec::new();
        // Where each difference is marked on the pages side by side, for
        // each file: the report's page, one-based, and the top of its first
        // mark there, in the page's own space.
        let mut marked: HashMap<(usize, bool), (usize, f32)> = HashMap::new();
        let mut text_page = |document: &mut PdfDocument<'static>,
                             lines: &[Placed],
                             number: usize,
                             marked: &HashMap<(usize, bool), (usize, f32)>|
         -> Result<(), String> {
            let _held = crate::pdfium::library_when_free();
            let mut objects = Vec::with_capacity(lines.len() + 1);
            for line in lines.iter().chain([&footer(number)]) {
                objects.push(text(document, line, PAPER.1).map_err(failed)?);
            }
            let size = PdfPagePaperSize::Custom(PdfPoints::new(PAPER.0), PdfPoints::new(PAPER.1));
            let mut page = (document.pages_mut().create_page_at_end(size)).map_err(failed)?;
            page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
            for object in objects {
                page.objects_mut().add_text_object(object).map_err(failed)?;
            }
            page.regenerate_content().map_err(failed)?;
            let handle = bindings.get_handle_from_page(&page);
            for line in lines {
                let Some(link) = &line.link else {
                    continue;
                };
                let baseline = PAPER.1 - line.y;
                let area = FS_RECTF {
                    left: line.x,
                    right: line.x + face.width(&line.text, line.size),
                    bottom: baseline - 0.3 * line.size,
                    top: baseline + line.size,
                };
                let index = unsafe {
                    let made = bindings.FPDFPage_CreateAnnot(handle, LINK_SUBTYPE);
                    if made.is_null() {
                        continue;
                    }
                    bindings.FPDFAnnot_SetRect(made, &area);
                    bindings.FPDFAnnot_SetBorder(made, 0.0, 0.0, 0.0);
                    if let Link::Away(address) = link {
                        bindings.FPDFAnnot_SetURI(made, address);
                    }
                    bindings.FPDFPage_CloseAnnot(made);
                    bindings.FPDFPage_GetAnnotCount(handle) - 1
                };
                // A difference is gone to a little above its mark, so that
                // the line before it shows too. One that was not marked goes
                // to the page that shows it.
                let (to, spot) = match *link {
                    Link::Away(_) => continue,
                    Link::Page(to) => (to, None),
                    Link::Mark { change, b } => match marked.get(&(change, b)) {
                        Some(&(to, top)) => (
                            to,
                            Some(crate::render::Pin {
                                left: None,
                                top: f64::from(top + 3.0 * TAG),
                            }),
                        ),
                        None => (cover_pages.len() + 1 + first[change][usize::from(b)], None),
                    },
                };
                relinks.push(crate::outline::Relink {
                    page: number - 1,
                    annotation: index as usize,
                    to: to - 1,
                    spot,
                });
            }
            Ok(())
        };
        let mut number = 0;
        for lines in &cover_pages {
            number += 1;
            text_page(&mut document, lines, number, &marked)?;
        }

        let mut flattened = std::collections::HashSet::new();
        for ([a, b], head) in pairs.iter().zip(&heads) {
            number += 1;
            let sides = [
                (a, source_there.as_ref(), &marks_there, colours.a, colours.b),
                (b, source_here.as_ref(), &marks_here, colours.b, colours.a),
            ];
            let mut drawn = Vec::with_capacity(2);
            for (letter, (half, source, marks, colour, other)) in sides.into_iter().enumerate() {
                // A page shown on another page of the report leaves its half
                // blank, at the size of the page.
                let (page, shown) = (half.page, half.elsewhere.is_none());
                let source = source.ok_or("a document to draw was not read")?;
                let _held = crate::pdfium::library_when_free();
                // The text is read before the page is flattened, as it was
                // read when the file was compared.
                let text = match shown && marks.contains_key(&page) {
                    true => Some(crate::pdfium::read_text(
                        &(source.pages().get(page as i32 - 1)).map_err(failed)?,
                    )),
                    false => None,
                };
                if shown && flattened.insert((letter, page)) {
                    crate::export::flatten(source, page - 1)?;
                }
                let read = (source.pages().get(page as i32 - 1)).map_err(failed)?;
                let space = crate::markup::Space::of(&read);
                let size = (read.width().value, read.height().value);
                drawn.push((page, shown, text, source, marks, colour, other, space, size));
            }
            // Each side's page size, which is the last of what was kept.
            let sizes: Vec<(f32, f32)> = drawn.iter().map(|(.., size)| *size).collect();
            let height = sizes.iter().map(|&(_, h)| h).fold(0.0, f32::max);
            let width = sizes.iter().map(|&(w, _)| w).sum::<f32>() + SHEET_GAP + 2.0 * SHEET_MARGIN;
            let tall = height + SHEET_HEAD + 2.0 * SHEET_MARGIN;

            let _held = crate::pdfium::library_when_free();
            let size = PdfPagePaperSize::Custom(PdfPoints::new(width), PdfPoints::new(tall));
            let mut sheet = (document.pages_mut().create_page_at_end(size)).map_err(failed)?;
            sheet.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
            let handle = bindings.get_handle_from_page(&sheet);
            let into = bindings.get_handle_from_document(&document);
            let mut paths = Vec::new();
            let mut lines = Vec::new();
            let mut left = SHEET_MARGIN;
            for (
                letter,
                ((page, shown, text, source, marks, colour, other, space, (w, h)), head),
            ) in drawn.into_iter().zip(head).enumerate()
            {
                lines.push(Placed {
                    x: left,
                    y: SHEET_MARGIN + 12.0,
                    size: 11.0,
                    colour: if shown { colour } else { INK },
                    text: head.clone(),
                    link: None,
                });
                if !shown {
                    left += w + SHEET_GAP;
                    continue;
                }
                // The page as drawn, its top left corner where it goes on
                // the sheet: from the file's space to the page as drawn,
                // and then up from the bottom of the sheet.
                let top = SHEET_MARGIN + SHEET_HEAD;
                let on_sheet = |x: f64, y: f64| {
                    let (x, y) = space.point_down(x, y);
                    (left as f64 + x, (tall - top) as f64 - y)
                };
                let (origin, across, up) =
                    (on_sheet(0.0, 0.0), on_sheet(1.0, 0.0), on_sheet(0.0, 1.0));
                let from = bindings.get_handle_from_document(source);
                let placed = unsafe {
                    let xobject = bindings.FPDF_NewXObjectFromPage(into, from, page as i32 - 1);
                    let object = bindings.FPDF_NewFormObjectFromXObject(xobject);
                    bindings.FPDF_CloseXObject(xobject);
                    if !object.is_null() {
                        bindings.FPDFPageObj_Transform(
                            object,
                            across.0 - origin.0,
                            across.1 - origin.1,
                            up.0 - origin.0,
                            up.1 - origin.1,
                            origin.0,
                            origin.1,
                        );
                        bindings.FPDFPage_InsertObject(handle, object);
                    }
                    !object.is_null()
                };
                if !placed {
                    return Err(format!("page {number} of the report could not be drawn"));
                }
                // A rectangle of the page as drawn, on the sheet.
                let rect = |r: &Rect| {
                    let bottom = tall - top - (r.top + r.height) as f32;
                    PdfRect::new_from_values(
                        bottom,
                        left + r.left as f32,
                        bottom + r.height as f32,
                        left + (r.left + r.width) as f32,
                    )
                };
                paths.push((
                    rect(&Rect {
                        left: 0.0,
                        top: 0.0,
                        width: w as f64,
                        height: h as f64,
                    }),
                    None,
                    Some(EDGE),
                    false,
                ));
                // This file's words are marked in its colour, and a bar in
                // the other file's colour is where that file has words this
                // one does not. Each difference's number stands in the
                // margin beside its first mark on the page, in the colour of
                // the file it is in: A's to the left, B's to the right.
                let mut tags: Vec<(f32, String, Colour)> = Vec::new();
                if let (Some(text), Some(marks)) = (&text, marks.get(&page)) {
                    for &(from, to, moved, change) in marks {
                        let (ours, theirs) = match moved {
                            true => (colours.moved, colours.moved),
                            false => (colour, other),
                        };
                        let areas: Vec<(Rect, Colour, bool)> = match from < to {
                            true => (text.quads(from, to).into_iter())
                                .map(|quad| (quad, ours, true))
                                .collect(),
                            false => (crate::compare::bar(text, from).into_iter())
                                .map(|bar| (bar, theirs, false))
                                .collect(),
                        };
                        if let Some((first_area, tagged, _)) = areas.first() {
                            let on_sheet = rect(first_area);
                            marked
                                .entry((change, letter == 1))
                                .or_insert((number, on_sheet.top().value));
                            let shade = if moved { colours.moved } else { *tagged };
                            let y = top + first_area.top as f32 + TAG;
                            tags.push((y, (change + 1).to_string(), shade));
                        }
                        for (area, shade, words) in areas {
                            let fill = if words { mark(shade) } else { shade };
                            paths.push((rect(&area), Some(fill), None, words));
                        }
                    }
                }
                // From the top down, each tag below the one before it, and
                // from the foot of the page up where that takes the last
                // past it. Where they cannot all be a line apart beside the
                // page, they are spread evenly down it.
                tags.sort_by(|one, other| one.0.total_cmp(&other.0));
                let (step, highest, lowest) = (TAG * LEADING, top + TAG, top + h);
                let mut ys: Vec<f32> = tags.iter().map(|(y, ..)| *y).collect();
                for at in 1..ys.len() {
                    ys[at] = ys[at].max(ys[at - 1] + step);
                }
                if ys.last().is_some_and(|&y| y > lowest) {
                    let mut next = lowest + step;
                    for y in ys.iter_mut().rev() {
                        *y = y.min(next - step);
                        next = *y;
                    }
                }
                if ys.first().is_some_and(|&y| y < highest) {
                    let gap = (lowest - highest) / (ys.len() - 1).max(1) as f32;
                    for (at, y) in ys.iter_mut().enumerate() {
                        *y = highest + gap * at as f32;
                    }
                }
                for ((_, tag, shade), y) in tags.into_iter().zip(ys) {
                    let x = match letter {
                        0 => left - 3.0 - face.width(&tag, TAG),
                        _ => left + w + 3.0,
                    };
                    lines.push(Placed {
                        x,
                        y,
                        size: TAG,
                        colour: shade,
                        text: tag,
                        link: None,
                    });
                }
                left += w + SHEET_GAP;
            }
            lines.push(Placed {
                y: tall - SHEET_MARGIN / 2.0,
                x: SHEET_MARGIN,
                ..footer(number)
            });
            for (area, fill, edge, multiplied) in paths {
                let mut path = PdfPagePathObject::new_rect(
                    &document,
                    area,
                    edge.map(|(r, g, b)| PdfColor::new(r, g, b, 255)),
                    edge.map(|_| PdfPoints::new(0.75)),
                    fill.map(|(r, g, b)| PdfColor::new(r, g, b, 255)),
                )
                .map_err(failed)?;
                if multiplied {
                    path.set_blend_mode(PdfPageObjectBlendMode::Multiply)
                        .map_err(failed)?;
                }
                sheet.objects_mut().add_path_object(path).map_err(failed)?;
            }
            for line in &lines {
                let object = text(&document, line, tall).map_err(failed)?;
                sheet
                    .objects_mut()
                    .add_text_object(object)
                    .map_err(failed)?;
            }
            sheet.regenerate_content().map_err(failed)?;
        }

        for lines in &list_pages {
            number += 1;
            text_page(&mut document, lines, number, &marked)?;
        }

        let _held = crate::pdfium::library_when_free();
        let bytes = document.save_to_bytes().map_err(failed)?;
        Ok((bytes, relinks))
    })();
    {
        let _held = crate::pdfium::library();
        drop(document);
        drop(source_there);
        drop(source_here);
    }
    let (bytes, relink) = written?;
    // The parts the first page lists are the file's outline too, so that a
    // reader can go to them from the contents in the sidebar.
    let part = |title: &str, page: usize| crate::outline::Entry {
        title: title.into(),
        depth: 0,
        page: Some(page),
        spot: None,
    };
    let mut outline = vec![part(SUMMARY, 0)];
    if !changes.is_empty() {
        outline.push(part("Pages side by side", cover_pages.len()));
        outline.push(part("List of differences", cover_pages.len() + pairs.len()));
    }
    // The title and the app that made it go into the file's `/Info` through
    // lopdf, with the outline and the list's links to the pages, and
    // lopdf writes the file smaller too. Where lopdf cannot, the file is
    // kept as pdfium wrote it, with no outline, and the list's links lead
    // nowhere.
    let work = crate::objects::Work {
        outline,
        details: Some(crate::objects::Details {
            title: format!("Comparison of {} and {}", there.name(), here.name()),
            made_with: Some(format!("Moonowl {}", env!("CARGO_PKG_VERSION"))),
            ..Default::default()
        }),
        relink,
        // pdfium copies each page shown with everything it uses, so pages
        // that share a font would otherwise each carry a copy of it.
        merge: true,
        ..Default::default()
    };
    let links = !work.relink.is_empty();
    Ok(match rework(bytes, &work) {
        Ok((bytes, done)) => (
            bytes,
            Lacking {
                contents: false,
                links: done.links_lost,
            },
        ),
        Err(bytes) => (
            bytes,
            Lacking {
                contents: true,
                links,
            },
        ),
    })
}

/// What a report was saved without, because lopdf could not finish it.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Lacking {
    /// Its outline, its title and the fonts merged into one copy: lopdf
    /// could not read the file at all.
    pub contents: bool,
    /// Some of the list's links to the pages lead nowhere, or all of them
    /// when `contents` is missing too.
    pub links: bool,
}

impl Lacking {
    /// What the notice says after "Saved the comparison as …".
    pub fn said(&self) -> &'static str {
        match (self.contents, self.links) {
            (false, false) => ".",
            (false, true) => ", but some of its links to its pages lead nowhere.",
            (true, false) => ", but it could not be finished, so it has no contents.",
            (true, true) => {
                ", but it could not be finished, so it has no contents, and its links to its \
                 pages lead nowhere."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A character the font has no glyph for is shown in its plain form,
    /// and the font is not cut down without that form.
    #[test]
    fn the_font_keeps_the_plain_forms_it_shows() {
        let font = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fonts/DejaVuSans.ttf"
        ))
        .unwrap();
        let face = Type {
            face: ttf_parser::Face::parse(&font, 0).unwrap(),
            missing: "?",
        };
        let letters = "\u{1d6fc}\u{338f}\u{2460}";
        let shown = face.shown(letters, &mut 0);
        assert_ne!(shown, letters, "some are shown in their plain forms");
        let used = face.used(letters);
        for character in shown.chars().filter(|&c| face.has(c)) {
            assert!(used.contains(character), "{character:?} is in {used:?}");
        }
    }

    /// Every letter is half the size wide.
    fn measure(text: &str, size: f32) -> f32 {
        text.chars().count() as f32 * size * 0.5
    }

    #[test]
    fn a_line_is_broken_between_words_and_inside_one_too_long() {
        let measure = |text: &str| measure(text, 2.0);
        assert_eq!(
            wrap("one two three", 5.0, &measure),
            ["one", "two", "three"]
        );
        assert_eq!(wrap("one two", 7.0, &measure), ["one two"]);
        assert_eq!(wrap("abcdefghij", 4.0, &measure), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("", 4.0, &measure), [""]);
    }

    /// A row longer than what is left of a page goes on over the next, and
    /// a heading with no room for the line after it begins the next page.
    #[test]
    fn rows_go_on_over_the_next_page() {
        let long = "word ".repeat(2_000);
        let rows = [
            Row::text(10.0, 0.0, INK, long),
            Row::text(13.0, 22.0, INK, "Heading").keeping(1),
            Row::text(10.0, 0.0, INK, "after"),
        ];
        let pages = set(&rows, &measure, 1);
        assert!(pages.len() >= 2);
        let last = pages.last().unwrap();
        let heading = last.iter().position(|line| line.text == "Heading").unwrap();
        assert_eq!(last[heading + 1].text, "after");
        for line in pages.iter().flatten() {
            assert!(line.y <= PAPER.1 - MARGIN);
        }
    }

    fn change(places_there: &[(usize, usize, usize)], places: &[(usize, usize, usize)]) -> Change {
        Change {
            places: places.to_vec(),
            places_there: places_there.to_vec(),
            here: String::new(),
            there: String::new(),
            count: 0,
            count_there: 0,
            moved: None,
            passage: None,
        }
    }

    /// Each change is shown on the pages it is on in each file, a pair
    /// once, and a change over more pages of one file has the other's last
    /// page beside the first of the rest, and a blank half beside the
    /// others.
    #[test]
    fn pages_are_paired_by_their_changes() {
        let changes = [
            change(&[(1, 0, 5)], &[(1, 0, 4)]),
            change(&[(1, 9, 12)], &[(1, 8, 8)]),
            change(&[(2, 0, 9), (3, 0, 9), (4, 0, 4)], &[(2, 0, 0)]),
            change(&[(6, 3, 3)], &[(4, 0, 2), (5, 0, 7)]),
        ];
        let (sheets, first) = pairs(&changes);
        let half = |page, elsewhere| Half { page, elsewhere };
        assert_eq!(
            sheets,
            [
                [half(1, None), half(1, None)],
                [half(2, None), half(2, None)],
                [half(3, None), half(2, Some(1))],
                [half(4, None), half(2, Some(1))],
                [half(6, None), half(4, None)],
                [half(6, Some(4)), half(5, None)],
            ]
        );
        assert_eq!(first, [[0, 0], [0, 0], [1, 1], [4, 4]]);
    }

    /// A change on A's second page and B's first, between changes on the
    /// first pages and on the second pages, is shown on those, so that no
    /// page is shown twice.
    #[test]
    fn a_page_is_shown_once() {
        let changes = [
            change(&[(1, 0, 5)], &[(1, 0, 4)]),
            change(&[(2, 0, 3)], &[(1, 9, 12)]),
            change(&[(2, 5, 9)], &[(2, 0, 2)]),
        ];
        let (sheets, first) = pairs(&changes);
        let pages: Vec<(usize, usize)> = (sheets.iter()).map(|[a, b]| (a.page, b.page)).collect();
        assert_eq!(pages, [(1, 1), (2, 2)]);
        assert_eq!(first, [[0, 0], [1, 0], [1, 1]]);
    }

    /// A change on pages that are both shown already, but on different
    /// pages of the report, leads to each of those pages.
    #[test]
    fn a_change_on_pages_shown_apart_leads_to_both() {
        let changes = [
            change(&[(1, 0, 5)], &[(1, 0, 4)]),
            change(&[(2, 0, 3)], &[(2, 0, 2)]),
            change(&[(1, 9, 12)], &[(2, 5, 9)]),
        ];
        let (sheets, first) = pairs(&changes);
        assert_eq!(sheets.len(), 2);
        assert_eq!(first, [[0, 0], [1, 1], [0, 1]]);
    }

    /// A character the font lacks is shown in its plain form where there
    /// is one, and is otherwise counted and shown as clearly missing.
    #[test]
    fn a_character_no_font_has_is_counted() {
        let Some(bytes) = crate::sign::font_for("x") else {
            return;
        };
        let face = Type {
            face: ttf_parser::Face::parse(&bytes, 0).unwrap(),
            missing: "[?]",
        };
        let mut replaced = 0;
        // A mathematical italic x, and a character no font has.
        let shown = face.shown("\u{1d465} and \u{10ffff}", &mut replaced);
        assert_eq!(shown, "x and [?]");
        assert_eq!(replaced, 1);
    }

    #[test]
    fn a_list_is_quoted_with_a_comma_before_its_last_item() {
        let list =
            |items: &[&str]| in_a_list(&items.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(list(&["File"]), "\u{201c}File\u{201d}");
        assert_eq!(list(&["A", "B"]), "\u{201c}A\u{201d} and \u{201c}B\u{201d}");
        assert_eq!(
            list(&["A", "B", "C"]),
            "\u{201c}A\u{201d}, \u{201c}B\u{201d}, and \u{201c}C\u{201d}"
        );
    }
}
