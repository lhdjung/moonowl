//! A comparison written out as a PDF of its own. The first page names the
//! two files, A and B, lists the report's parts, says what its colours
//! mean, and lists what each file says about itself side by side. The
//! summary lists every difference in words. Then each pair of pages where
//! the two differ is shown side by side, with the words only A has marked
//! on A's page and the words only B has marked on B's.
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
#[derive(Clone, Debug, Default)]
pub struct File {
    pub path: String,
    pub title: String,
    /// What the Information window lists (`PageSource::details`).
    pub details: Vec<(String, String)>,
    /// What each page is called (`PageSource::labels`).
    pub labels: Vec<String>,
}

impl File {
    /// The file as `document` describes it.
    pub fn of(document: &dyn PageSource) -> File {
        File {
            path: document.path().to_string(),
            title: document.title(),
            details: document.details(),
            labels: document.labels(),
        }
    }

    fn name(&self) -> String {
        crate::store::file_name(&self.path)
    }

    fn label(&self, page: usize) -> String {
        match self.labels.get(page.wrapping_sub(1)) {
            Some(label) if !label.is_empty() => label.clone(),
            _ => page.to_string(),
        }
    }

    /// The rows of its column in the table of details: the title, what the
    /// document says about itself, and the file on the disk.
    fn rows(&self) -> Vec<(String, String)> {
        let mut rows = vec![("Title".to_string(), self.title.trim().to_string())];
        rows.extend(self.details.iter().cloned());
        rows.push(("File".into(), self.path.clone()));
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

pub type Colour = (u8, u8, u8);
const INK: Colour = (0x1f, 0x23, 0x28);
const FAINT: Colour = (0x6b, 0x72, 0x80);

/// The colours a report marks its differences in, chosen by the reader.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colours {
    /// Words only in A.
    pub a: Colour,
    /// Words only in B.
    pub b: Colour,
    /// Words that are in both files, in different places.
    pub moved: Colour,
}

/// Red and green, as Git shows a difference, and blue for what moved.
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
}

/// Text to be wrapped inside `width` points, `x` points from the margin.
struct Cell {
    x: f32,
    width: f32,
    text: String,
    colour: Colour,
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

/// The pairs of pages shown side by side, a page of A and a page of B (both
/// one-based), and for each change the pair it is first shown on. A change
/// over more pages in one file than in the other is shown with the last of
/// its pages in the other file beside each of the rest.
// ponytail: a long passage only one file has repeats the other file's page
// beside each of its pages; a blank half could stand in for the repeats.
fn pairs(changes: &[Change]) -> (Vec<(usize, usize)>, Vec<usize>) {
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
    (pairs, first)
}

/// The first page: the two files, what the report holds, how to read its
/// pages, and what each file says about itself. `summary_from` and
/// `sheets_from` are the report's pages the summary and the pages side by
/// side begin on, `sheets` how many pages side by side there are, and
/// `replaced` how many characters no font had.
fn cover(
    there: &File,
    here: &File,
    changes: &[Change],
    colours: &Colours,
    (summary_from, sheets_from, sheets): (usize, usize, usize),
    replaced: (usize, &str),
) -> Vec<Row> {
    let mut rows = vec![Row::text(22.0, 0.0, INK, "Comparison report")];
    for (letter, colour, file, before) in
        [("A", colours.a, there, 14.0), ("B", colours.b, here, 10.0)]
    {
        let (title, name) = (file.title.trim(), file.name());
        let shown = if title.is_empty() { &name } else { title };
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
                },
                Cell {
                    x: 22.0,
                    width: PAPER.0 - 2.0 * MARGIN - 22.0,
                    text: shown.to_string(),
                    colour: INK,
                },
            ],
            link: None,
        });
        if shown != name {
            rows.push(Row::at(22.0, 9.5, 2.0, FAINT, name));
        }
    }
    rows.push(Row::text(
        10.0,
        14.0,
        FAINT,
        format!(
            "Made by Moonowl {} on {}.",
            env!("CARGO_PKG_VERSION"),
            crate::sign::today()
        ),
    ));
    rows.push(Row::text(10.0, 2.0, LINK, HOME).leading_to(Link::Away(HOME)));

    let stayed = || changes.iter().filter(|change| change.moved.is_none());
    let only_there: usize = stayed().map(|change| change.count_there).sum();
    let only_here: usize = stayed().map(|change| change.count).sum();
    let moved = changes.len() - stayed().count();
    let words = |n: usize| match n {
        1 => "1 word is".to_string(),
        n => format!("{n} words are"),
    };
    rows.push(Row::text(
        10.5,
        18.0,
        INK,
        match changes.len() {
            0 => "The text of the two files is the same.".to_string(),
            n => format!(
                "{n} {}. {} only in A, and {} only in B.{}",
                if n == 1 { "difference" } else { "differences" },
                words(only_there),
                words(only_here),
                match moved / 2 {
                    0 => String::new(),
                    1 => " 1 passage moved.".into(),
                    n => format!(" {n} passages moved."),
                },
            ),
        },
    ));
    if !changes.is_empty() {
        // Each part of the report after this page, with the page it begins
        // on, or the pages it takes.
        let last = sheets_from + sheets.max(1) - 1;
        let sheet_pages = match sheets_from == last {
            true => sheets_from.to_string(),
            false => format!("{sheets_from}\u{2013}{last}"),
        };
        rows.push(Row::text(13.0, 22.0, INK, "Contents").keeping(2));
        for (part, pages, at) in [
            (
                "Summary of differences",
                summary_from.to_string(),
                summary_from,
            ),
            ("Pages side by side", sheet_pages, sheets_from),
        ] {
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
                    },
                    Cell {
                        x: 250.0,
                        width: 80.0,
                        text: pages,
                        colour: LINK,
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
                "Words that only A has, marked on A\u{2019}s pages. On B\u{2019}s pages, a \
                 thin bar in this colour shows where they would be.",
            ),
            (
                "Only in B",
                colours.b,
                "Words that only B has, marked on B\u{2019}s pages. On A\u{2019}s pages, a \
                 thin bar in this colour shows where they would be.",
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
            "Within a word",
            INK,
            "Where one word takes the place of another, or its punctuation changed, \
             only the characters that differ are marked. The summary shows the words \
             whole.",
        ));
        key.push((
            "Not compared",
            INK,
            "Running heads, page numbers and line numbers, capital letters, hyphens \
             where a word is broken across two lines, how punctuation is spaced and \
             shaped, pictures, layout and annotations.",
        ));
        key.push((
            "Cut short",
            INK,
            "A difference longer than about four pages, in the summary. The pages show \
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
                    },
                    Cell {
                        x: 90.0,
                        width: PAPER.0 - 2.0 * MARGIN - 90.0,
                        text: said.into(),
                        colour: FAINT,
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
            FAINT,
            format!("1 character that no font on this computer has is shown as {shown}."),
        )),
        (n, shown) => rows.push(Row::text(
            10.0,
            6.0,
            FAINT,
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
                })
                .collect(),
            link: None,
        }
    };
    rows.push(
        side_by_side(
            11.0,
            8.0,
            [FAINT, colours.a, colours.b],
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
            [FAINT, INK, INK],
            [named.to_string(), one, two],
        ));
    }
    rows.push(Row::text(
        10.0,
        10.0,
        FAINT,
        match differ.len() {
            0 => "The files say the same about themselves.".to_string(),
            _ => format!("These details differ: {}.", in_a_list(&differ)),
        },
    ));
    rows
}

/// Every difference in words, each leading to the report's page it is
/// shown on, counting from `sheets_from`.
fn list(
    there: &File,
    here: &File,
    changes: &[Change],
    colours: &Colours,
    first: &[usize],
    sheets_from: usize,
) -> Vec<Row> {
    let mut rows = vec![Row::text(13.0, 0.0, INK, "Summary of differences").keeping(2)];
    for (number, change) in changes.iter().enumerate() {
        rows.push(
            Row::text(
                9.5,
                10.0,
                FAINT,
                format!(
                    "{}. Page {} of A, page {} of B. Shown on page {}.",
                    number + 1,
                    there.label(change.page_there()),
                    here.label(change.page()),
                    sheets_from + first[number],
                ),
            )
            .keeping(1)
            .leading_to(Link::Page(sheets_from + first[number])),
        );
        let (red, green) = match change.moved {
            Some(_) => (colours.moved, colours.moved),
            None => (colours.a, colours.b),
        };
        if !change.there.is_empty() {
            rows.push(Row::text(10.0, 2.0, red, format!("A: {}", change.there)));
        }
        if !change.here.is_empty() {
            rows.push(Row::text(10.0, 2.0, green, format!("B: {}", change.here)));
        }
        if let Some(other) = change.moved {
            let (from, to) = match change.here.is_empty() {
                true => ("A", "B"),
                false => ("B", "A"),
            };
            rows.push(Row::text(
                9.5,
                2.0,
                FAINT,
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
fn in_a_list(items: &[String]) -> String {
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

/// The rows set on pages: each page's lines.
// ponytail: each line is measured afresh as words are added to it, which is
// quadratic in the words of a line, and a line has a dozen.
fn set(rows: &[Row], measure: &dyn Fn(&str, f32) -> f32) -> Vec<Vec<Placed>> {
    let bottom = PAPER.1 - MARGIN;
    let mut pages: Vec<Vec<Placed>> = vec![Vec::new()];
    let mut y = MARGIN;
    for row in rows {
        let step = row.size * LEADING;
        let cells: Vec<Vec<String>> = (row.cells.iter())
            .map(|cell| wrap(&cell.text, cell.width, &|text| measure(text, row.size)))
            .collect();
        let lines = cells.iter().map(Vec::len).max().unwrap_or(0);
        let first = pages.last().is_some_and(Vec::is_empty);
        let before = if first { 0.0 } else { row.before };
        if !first && y + before + step * (1 + row.keep) as f32 > bottom {
            pages.push(Vec::new());
            y = MARGIN;
        } else {
            y += before;
        }
        for line in 0..lines {
            if y + step > bottom {
                pages.push(Vec::new());
                y = MARGIN;
            }
            let page = pages.last_mut().expect("there is always a page");
            for (cell, wrapped) in row.cells.iter().zip(&cells) {
                if let Some(text) = wrapped.get(line).filter(|text| !text.is_empty()) {
                    page.push(Placed {
                        x: MARGIN + cell.x,
                        y: y + row.size,
                        size: row.size,
                        colour: cell.colour,
                        text: text.clone(),
                        link: row.link.clone(),
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
/// the bytes of a PDF, marked in `colours`. `text_there` and `text_here`
/// are the two documents as they were compared, whose text says where on
/// its page each word is.
pub fn write(
    there: &File,
    here: &File,
    changes: &[Change],
    colours: &Colours,
    text_there: &dyn PageSource,
    text_here: &dyn PageSource,
) -> Result<Vec<u8>, String> {
    let (pairs, first) = pairs(changes);
    let heads: Vec<[String; 2]> = (pairs.iter())
        .map(|&(a, b)| {
            [
                format!("A, page {}", there.label(a)),
                format!("B, page {}", here.label(b)),
            ]
        })
        .collect();

    // The type is the system font that has the most of the letters the
    // report uses, cut down to those letters.
    let mut letters: Vec<char> = Vec::new();
    {
        let mut seen = std::collections::HashSet::new();
        let rows = (cover(there, here, changes, colours, (2, 3, pairs.len()), (0, "")).into_iter())
            .chain(list(there, here, changes, colours, &first, 3));
        for row in rows {
            for cell in row.cells {
                letters.extend(cell.text.chars().filter(|&c| seen.insert(c)));
            }
        }
        for head in heads.iter().flatten() {
            letters.extend(head.chars().filter(|&c| seen.insert(c)));
        }
    }
    let letters: String = letters.into_iter().collect();
    let mut fonts = crate::sign::SystemFonts::default();
    let found = (fonts.most_of(&letters))
        .ok_or("No font was found on this computer to write the report in.")?;
    let font = fonts.bytes(found).to_vec();
    let face = ttf_parser::Face::parse(&font, 0).map_err(|e| format!("the font: {e}"))?;
    let missing = match face.glyph_index('\u{fffd}') {
        Some(_) => "\u{fffd}",
        None => "[?]",
    };
    let face = Type { face, missing };
    let measure = |text: &str, size: f32| face.width(text, size);

    // The first page and the summary say which pages follow them, and the
    // first page how many characters no font had. Each can change how many
    // pages they take, so they are set again until they say what they hold.
    let (mut covers, mut summaries, mut replaced) = (1, usize::from(!changes.is_empty()), 0);
    let (cover_pages, list_pages, heads) = 'set: {
        for _ in 0..5 {
            let mut counted = 0;
            let mut shown = |rows: &mut Vec<Row>| {
                for cell in rows.iter_mut().flat_map(|row| &mut row.cells) {
                    cell.text = face.shown(&cell.text, &mut counted);
                }
            };
            let (summary_from, sheets_from) = (covers + 1, covers + summaries + 1);
            let mut cover_rows = cover(
                there,
                here,
                changes,
                colours,
                (summary_from, sheets_from, pairs.len()),
                (replaced, missing),
            );
            let mut list_rows = list(there, here, changes, colours, &first, sheets_from);
            shown(&mut cover_rows);
            shown(&mut list_rows);
            let heads: Vec<[String; 2]> = (heads.iter())
                .map(|head| head.clone().map(|text| face.shown(&text, &mut counted)))
                .collect();
            let cover_pages = set(&cover_rows, &measure);
            let list_pages = match changes.is_empty() {
                true => Vec::new(),
                false => set(&list_rows, &measure),
            };
            if (counted, cover_pages.len(), list_pages.len()) == (replaced, covers, summaries) {
                break 'set (cover_pages, list_pages, heads);
            }
            (covers, summaries, replaced) = (cover_pages.len(), list_pages.len(), counted);
        }
        return Err("The report's first pages could not be set.".into());
    };
    let total = cover_pages.len() + pairs.len() + list_pages.len();
    let footer = |number: usize| Placed {
        x: MARGIN,
        y: PAPER.1 - MARGIN / 2.0,
        size: 8.5,
        colour: FAINT,
        text: format!("Page {number} of {total}"),
        link: None,
    };

    let mut used: Vec<char> = letters.chars().collect();
    used.extend(face.missing.chars());
    used.extend("Page of 0123456789".chars());
    used.sort_unstable();
    used.dedup();
    let used: String = used.into_iter().filter(|&c| face.has(c)).collect();
    let cut = crate::sign::subset(&font, &used).unwrap_or_else(|| font.clone());

    // Where each file's changes are on its pages.
    let on_pages = |side: fn(&Change) -> &[(usize, usize, usize)]| {
        let mut on: HashMap<usize, Vec<(usize, usize, bool)>> = HashMap::new();
        for change in changes {
            for &(page, from, to) in side(change) {
                (on.entry(page).or_default()).push((from, to, change.moved.is_some()));
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
        let _held = crate::pdfium::library();
        (pdfium.load_pdf_from_byte_vec(bytes, None).map(Some)).map_err(|e| match e {
            PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
                format!("{letter} is encrypted, so its pages cannot be put into a report.")
            }
            e => format!("{} could not be read: {e}", file.name()),
        })
    };
    let source_there = source(there, "A")?;
    let source_here = source(here, "B")?;
    let made = {
        let _held = crate::pdfium::library();
        pdfium.create_new_pdf()
    };
    let mut document = made.map_err(|e| format!("a new document could not be made: {e}"))?;
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
        let mut text_page = |document: &mut PdfDocument<'static>,
                             lines: &[Placed],
                             number: usize|
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
                if let Link::Page(to) = link {
                    relinks.push(crate::outline::Relink {
                        page: number - 1,
                        annotation: index as usize,
                        to: to - 1,
                        spot: None,
                    });
                }
            }
            Ok(())
        };
        let mut number = 0;
        for lines in cover_pages.iter().chain(&list_pages) {
            number += 1;
            text_page(&mut document, lines, number)?;
        }

        let mut flattened = std::collections::HashSet::new();
        for (&(a, b), head) in pairs.iter().zip(&heads) {
            number += 1;
            let sides = [
                (
                    a,
                    text_there,
                    source_there.as_ref(),
                    &marks_there,
                    colours.a,
                    colours.b,
                ),
                (
                    b,
                    text_here,
                    source_here.as_ref(),
                    &marks_here,
                    colours.b,
                    colours.a,
                ),
            ];
            let mut drawn = Vec::with_capacity(2);
            for (letter, (page, text, source, marks, colour, other)) in
                sides.into_iter().enumerate()
            {
                let source = source.ok_or("a document to draw was not read")?;
                let text = match marks.contains_key(&page) {
                    true => {
                        Some((text.text_aside(page - 1)).ok_or(crate::export::CLOSED_WHILE_READ)?)
                    }
                    false => None,
                };
                let _held = crate::pdfium::library_when_free();
                if flattened.insert((letter, page)) {
                    crate::export::flatten(source, page - 1)?;
                }
                let read = (source.pages().get(page as i32 - 1)).map_err(failed)?;
                let space = crate::markup::Space::of(&read);
                let size = (read.width().value, read.height().value);
                drawn.push((page, text, source, marks, colour, other, space, size));
            }
            let height = drawn.iter().map(|side| side.7 .1).fold(0.0, f32::max);
            let width = drawn[0].7 .0 + drawn[1].7 .0 + SHEET_GAP + 2.0 * SHEET_MARGIN;
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
            for ((page, text, source, marks, colour, other, space, (w, h)), head) in
                drawn.into_iter().zip(head)
            {
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
                // one does not.
                if let (Some(text), Some(marks)) = (&text, marks.get(&page)) {
                    for &(from, to, moved) in marks {
                        let (ours, theirs) = match moved {
                            true => (colours.moved, colours.moved),
                            false => (colour, other),
                        };
                        if from < to {
                            for quad in text.quads(from, to) {
                                paths.push((rect(&quad), Some(mark(ours)), None, true));
                            }
                        } else if let Some(bar) = crate::compare::bar(text, from) {
                            paths.push((rect(&bar), Some(theirs), None, false));
                        }
                    }
                }
                lines.push(Placed {
                    x: left,
                    y: SHEET_MARGIN + 12.0,
                    size: 11.0,
                    colour,
                    text: head.clone(),
                    link: None,
                });
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
    // The title and the app that made it go into the file's `/Info` through
    // lopdf, with the summary's links to the pages, and lopdf writes the
    // file smaller too. Where lopdf cannot, the file is kept as pdfium wrote
    // it, and the summary's links lead nowhere.
    let work = crate::objects::Work {
        details: Some(crate::objects::Details {
            title: format!("Comparison of {} and {}", there.name(), here.name()),
            made_with: Some(format!("Moonowl {}", env!("CARGO_PKG_VERSION"))),
            ..Default::default()
        }),
        relink,
        ..Default::default()
    };
    Ok(match crate::objects::rework(bytes, &work) {
        Ok((bytes, _)) => bytes,
        Err(refused) => refused.bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let pages = set(&rows, &measure);
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
        }
    }

    /// Each change is shown on the pages it is on in each file, a pair
    /// once, and a change over more pages of one file has the other's last
    /// page beside each of the rest.
    #[test]
    fn pages_are_paired_by_their_changes() {
        let changes = [
            change(&[(1, 0, 5)], &[(1, 0, 4)]),
            change(&[(1, 9, 12)], &[(1, 8, 8)]),
            change(&[(2, 0, 9), (3, 0, 9), (4, 0, 4)], &[(2, 0, 0)]),
            change(&[(6, 3, 3)], &[(4, 0, 2), (5, 0, 7)]),
        ];
        let (pairs, first) = pairs(&changes);
        assert_eq!(pairs, [(1, 1), (2, 2), (3, 2), (4, 2), (6, 4), (6, 5)]);
        assert_eq!(first, [0, 0, 1, 4]);
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
