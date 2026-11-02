//! A comparison written out as a PDF of its own: a first part with what
//! each file says about itself, side by side, and then every difference,
//! with the words only one file has and the page they are on in each file.
//! The words are compared in order (see `compare.rs`), so no page of one
//! file has to match a page of the other: a page added or taken out is a
//! difference like any other.

use pdfium_render::prelude::*;

use crate::compare::Change;

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
    pub fn of(document: &dyn crate::render::PageSource) -> File {
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

const PAPER: (f32, f32) = (595.0, 842.0);
const MARGIN: f32 = 56.0;
/// The distance from one line to the next, as a multiple of the type's size.
const LEADING: f32 = 1.4;

type Colour = (u8, u8, u8);
const INK: Colour = (0x1f, 0x23, 0x28);
const FAINT: Colour = (0x6b, 0x72, 0x80);
const RED: Colour = (0xb4, 0x23, 0x18);
const GREEN: Colour = (0x1a, 0x7f, 0x37);

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
}

impl Row {
    fn text(size: f32, before: f32, colour: Colour, text: impl Into<String>) -> Row {
        Row {
            size,
            before,
            keep: 0,
            cells: vec![Cell {
                x: 0.0,
                width: PAPER.0 - 2.0 * MARGIN,
                text: text.into(),
                colour,
            }],
        }
    }

    fn keeping(self, keep: usize) -> Row {
        Row { keep, ..self }
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
}

/// The report, as rows to set.
fn rows(there: &File, here: &File, changes: &[Change]) -> Vec<Row> {
    let (a, b) = (there.name(), here.name());
    let version = env!("CARGO_PKG_VERSION");
    let mut rows = vec![
        Row::text(18.0, 0.0, INK, format!("Comparison of {a} and {b}")),
        Row::text(
            10.0,
            4.0,
            FAINT,
            format!("Made by Moonowl {version} on {}.", crate::sign::today()),
        ),
    ];
    let only_there: usize = changes.iter().map(|change| change.count_there).sum();
    let only_here: usize = changes.iter().map(|change| change.count).sum();
    let words = |n: usize| match n {
        1 => "1 word is".to_string(),
        n => format!("{n} words are"),
    };
    let summary = match changes.len() {
        0 => "The text of the two files is the same.".to_string(),
        n => format!(
            "{n} {}. {} only in {a}, and {} only in {b}.",
            if n == 1 { "difference" } else { "differences" },
            words(only_there),
            words(only_here),
        ),
    };
    rows.push(Row::text(10.5, 18.0, INK, summary));
    rows.push(Row::text(
        10.0,
        6.0,
        FAINT,
        format!(
            "The words of the two files are compared in order, so a passage that moved is a \
             difference in both places. Words only in {a} are red and marked \u{2212}, and \
             words only in {b} are green and marked +. Pictures, layout and annotations are \
             not compared. A difference longer than about four pages is cut short here, and \
             can be read on its pages in the file."
        ),
    ));

    // What each file says about itself, side by side.
    rows.push(Row::text(13.0, 22.0, INK, "The files").keeping(2));
    let (label, column) = (90.0, (PAPER.0 - 2.0 * MARGIN - 90.0 - 12.0) / 2.0);
    let side_by_side = |size, before, colour, cells: [String; 3]| {
        let [first, second, third] = cells;
        Row {
            size,
            before,
            keep: 0,
            cells: vec![
                Cell {
                    x: 0.0,
                    width: label - 6.0,
                    text: first,
                    colour: FAINT,
                },
                Cell {
                    x: label,
                    width: column,
                    text: second,
                    colour,
                },
                Cell {
                    x: label + column + 12.0,
                    width: column,
                    text: third,
                    colour,
                },
            ],
        }
    };
    rows.push(side_by_side(9.5, 8.0, FAINT, [String::new(), a.clone(), b.clone()]).keeping(1));
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
        rows.push(side_by_side(9.5, 3.0, INK, [named.to_string(), one, two]));
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

    if changes.is_empty() {
        return rows;
    }
    rows.push(Row::text(13.0, 22.0, INK, "Differences").keeping(2));
    for (number, change) in changes.iter().enumerate() {
        let (page_there, page_here) = (change.page_there(), change.page());
        rows.push(
            Row::text(
                9.5,
                10.0,
                FAINT,
                format!(
                    "{}. Page {} of {a}, page {} of {b}",
                    number + 1,
                    there.label(page_there),
                    here.label(page_here),
                ),
            )
            .keeping(1),
        );
        if !change.there.is_empty() {
            rows.push(Row::text(
                10.0,
                2.0,
                RED,
                format!("\u{2212} {}", change.there),
            ));
        }
        if !change.here.is_empty() {
            rows.push(Row::text(10.0, 2.0, GREEN, format!("+ {}", change.here)));
        }
    }
    rows
}

/// "a, b and c".
fn in_a_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
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
                    });
                }
            }
            y += step;
        }
    }
    pages
}

/// The report comparing `there` with `here`, as the bytes of a PDF.
pub fn write(there: &File, here: &File, changes: &[Change]) -> Result<Vec<u8>, String> {
    let mut rows = rows(there, here, changes);
    // The type is a system font, cut down to the letters the report uses. A
    // letter no font on this computer has is written as a question mark.
    // ponytail: one font for the whole report; a font per script if a
    // report in, say, Arabic and Chinese at once is ever needed.
    let mut all = String::new();
    for cell in rows.iter().flat_map(|row| &row.cells) {
        all.push_str(&cell.text);
    }
    let mut fonts = crate::sign::SystemFonts::default();
    let found = (fonts.for_line(&all))
        .or_else(|| fonts.for_line(""))
        .ok_or("No font was found on this computer to write the report in.")?;
    let font = fonts.bytes(found).to_vec();
    let face = ttf_parser::Face::parse(&font, 0).map_err(|e| format!("the font: {e}"))?;
    let shown = |character: char| match character {
        c if c.is_whitespace() => ' ',
        c if face.glyph_index(c).is_some() => c,
        _ => '?',
    };
    for cell in rows.iter_mut().flat_map(|row| &mut row.cells) {
        if cell.text.chars().any(|c| shown(c) != c) {
            cell.text = cell.text.chars().map(shown).collect();
        }
    }
    let per_em = face.units_per_em() as f32;
    let advance = |character: char| {
        (face.glyph_index(character))
            .and_then(|glyph| face.glyph_hor_advance(glyph))
            .unwrap_or(0) as f32
            / per_em
    };
    let measure = |text: &str, size: f32| text.chars().map(advance).sum::<f32>() * size;
    let mut pages = set(&rows, &measure);
    let count = pages.len();
    for (at, page) in pages.iter_mut().enumerate() {
        page.push(Placed {
            x: MARGIN,
            y: PAPER.1 - MARGIN / 2.0,
            size: 8.5,
            colour: FAINT,
            text: format!("Page {} of {count}", at + 1),
        });
    }

    let mut letters: Vec<char> = (pages.iter().flatten())
        .flat_map(|placed| placed.text.chars())
        .collect();
    letters.sort_unstable();
    letters.dedup();
    let letters: String = letters.into_iter().collect();
    let font = crate::sign::subset(&font, &letters).unwrap_or(font);

    let pdfium = crate::pdfium::pdfium()?;
    let held = crate::pdfium::library();
    let made = pdfium.create_new_pdf();
    drop(held);
    let mut document = made.map_err(|e| format!("a new document could not be made: {e}"))?;
    let failed = |e: PdfiumError| format!("the report could not be written: {e}");
    // pdfium's lock is taken for one page at a time, so the pages being
    // drawn for the window go between them.
    let written = (|| {
        let font = {
            let _held = crate::pdfium::library_when_free();
            (document.fonts_mut().load_true_type_from_bytes(&font, true)).map_err(failed)?
        };
        for lines in &pages {
            let _held = crate::pdfium::library_when_free();
            let size = PdfPagePaperSize::Custom(PdfPoints::new(PAPER.0), PdfPoints::new(PAPER.1));
            let mut objects = Vec::with_capacity(lines.len());
            for line in lines {
                // pdfium puts no space between two text objects when it
                // reads a page's text, so each line carries its own.
                let mut object = PdfPageTextObject::new(
                    &document,
                    format!("{} ", line.text),
                    font,
                    PdfPoints::new(line.size),
                )
                .map_err(failed)?;
                let (red, green, blue) = line.colour;
                (object.set_fill_color(PdfColor::new(red, green, blue, 255))).map_err(failed)?;
                (object.translate(PdfPoints::new(line.x), PdfPoints::new(PAPER.1 - line.y)))
                    .map_err(failed)?;
                objects.push(object);
            }
            let mut page = (document.pages_mut().create_page_at_end(size)).map_err(failed)?;
            page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
            for object in objects {
                page.objects_mut().add_text_object(object).map_err(failed)?;
            }
            page.regenerate_content().map_err(failed)?;
        }
        let _held = crate::pdfium::library_when_free();
        document.save_to_bytes().map_err(failed)
    })();
    {
        let _held = crate::pdfium::library();
        drop(document);
    }
    let bytes = written?;
    // The title and the app that made it go into the file's `/Info` through
    // lopdf, which writes the file smaller too. Where lopdf cannot, the
    // file is kept as pdfium wrote it.
    let work = crate::objects::Work {
        details: Some(crate::objects::Details {
            title: format!("Comparison of {} and {}", there.name(), here.name()),
            made_with: Some(format!("Moonowl {}", env!("CARGO_PKG_VERSION"))),
            ..Default::default()
        }),
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
}
