//! Page labels, the names a document gives its pages ("iv", "12", "A-3"),
//! written through lopdf: pdfium reads them but cannot write them.
//!
//! A document gives its labels in ranges: from this page on, number in
//! this style, after this prefix, starting at this number. Two things are
//! done with them here. After pages are removed, moved or put in, every
//! page keeps the label it had, and the ranges are worked out again from
//! the labels. And the reader can number the pages from one page on by
//! typing the first label.

use crate::arrange::Moved;
use lopdf::{Dictionary, Document, Object};

/// How a range numbers its pages.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Style {
    /// 1, 2, 3
    Decimal,
    /// i, ii, iii
    RomanLower,
    /// I, II, III
    RomanUpper,
    /// a, b, … z, aa, bb
    LettersLower,
    /// A, B, … Z, AA, BB
    LettersUpper,
}

impl Style {
    /// The name the file gives this style in `/S`.
    fn name(self) -> &'static [u8] {
        match self {
            Style::Decimal => b"D",
            Style::RomanLower => b"r",
            Style::RomanUpper => b"R",
            Style::LettersLower => b"a",
            Style::LettersUpper => b"A",
        }
    }

    fn named(name: &[u8]) -> Option<Style> {
        [
            Style::Decimal,
            Style::RomanLower,
            Style::RomanUpper,
            Style::LettersLower,
            Style::LettersUpper,
        ]
        .into_iter()
        .find(|style| style.name() == name)
    }

    /// `number` written in this style.
    fn write(self, number: u32) -> String {
        match self {
            Style::Decimal => number.to_string(),
            Style::RomanLower => roman(number),
            Style::RomanUpper => roman(number).to_uppercase(),
            Style::LettersLower => letters(number),
            Style::LettersUpper => letters(number).to_uppercase(),
        }
    }

    /// The number `text` is in this style, if it is written as this style
    /// writes it.
    fn read(self, text: &str) -> Option<u32> {
        let number = match self {
            Style::Decimal if text.bytes().all(|b| b.is_ascii_digit()) => text.parse().ok()?,
            Style::RomanLower | Style::RomanUpper => (1..4000)
                .find(|&n| self.write(n) == text)
                .filter(|_| text.len() <= 15)?,
            Style::LettersLower | Style::LettersUpper => {
                let first = text.chars().next()?;
                let lower = first.to_ascii_lowercase();
                if !lower.is_ascii_lowercase() || text.chars().any(|c| c != first) {
                    return None;
                }
                26 * (text.len() as u32 - 1) + (lower as u32 - 'a' as u32 + 1)
            }
            _ => return None,
        };
        (number >= 1 && self.write(number) == text).then_some(number)
    }
}

/// `number` in lower-case Roman numerals.
fn roman(mut number: u32) -> String {
    const NUMERALS: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut written = String::new();
    for (value, numeral) in NUMERALS {
        while number >= value {
            written += numeral;
            number -= value;
        }
    }
    written
}

/// `number` as the letters style writes it: a to z, then aa to zz.
fn letters(number: u32) -> String {
    let number = number.max(1) - 1;
    let letter = (b'a' + (number % 26) as u8) as char;
    std::iter::repeat_n(letter, (number / 26 + 1) as usize).collect()
}

/// One range of labels: from page `start` (zero-based) on, `prefix` and
/// then the page's number in `style`, counting from `first`. A range with
/// no style labels every page with the prefix alone.
#[derive(Clone, Debug, PartialEq)]
pub struct Range {
    pub start: usize,
    pub style: Option<Style>,
    pub prefix: String,
    pub first: u32,
}

impl Range {
    /// The label of the page `after` pages into the range.
    fn label(&self, after: usize) -> String {
        match self.style {
            Some(style) => format!("{}{}", self.prefix, style.write(self.first + after as u32)),
            None => self.prefix.clone(),
        }
    }
}

/// The ways `label` could begin a range, the likeliest first: as a number
/// in each style after a prefix, and as a prefix alone.
fn readings(label: &str) -> Vec<Range> {
    let mut readings = Vec::new();
    let styles = [
        Style::Decimal,
        Style::RomanLower,
        Style::RomanUpper,
        Style::LettersLower,
        Style::LettersUpper,
    ];
    for style in styles {
        // The longest ending of the label this style can read.
        let ends = (label.char_indices().map(|(at, _)| at)).filter_map(|at| {
            let number = style.read(&label[at..])?;
            Some(Range {
                start: 0,
                style: Some(style),
                prefix: label[..at].to_string(),
                first: number,
            })
        });
        readings.extend(ends.take(1));
    }
    readings.push(Range {
        start: 0,
        style: None,
        prefix: label.to_string(),
        first: 1,
    });
    readings
}

/// The fewest ranges that give `labels` to the pages, one label per page.
pub fn ranges(labels: &[String]) -> Vec<Range> {
    let mut ranges: Vec<Range> = Vec::new();
    for (at, label) in labels.iter().enumerate() {
        if ranges
            .last()
            .is_some_and(|range| range.label(at - range.start) == *label)
        {
            continue;
        }
        // The reading the next page's label goes on from, if one does: "c"
        // followed by "d" is a letter, followed by "ci" a numeral. If none
        // does, a label that is a number is read as one, and any other is
        // a prefix alone: "Cover" is not "Cove" and the letter r.
        let readings = readings(label);
        let next = labels.get(at + 1);
        let pick = (readings.iter())
            .find(|reading| next.is_some_and(|next| reading.label(1) == *next))
            .or_else(|| readings.iter().find(|reading| reading.prefix.is_empty()))
            .unwrap_or(&readings[readings.len() - 1]);
        ranges.push(Range {
            start: at,
            ..pick.clone()
        });
    }
    ranges
}

/// Every page's label, for a document of `count` pages. A page before the
/// first range has none.
pub fn labels(ranges: &[Range], count: usize) -> Vec<String> {
    (0..count)
        .map(|at| {
            (ranges.iter())
                .rfind(|range| range.start <= at)
                .map_or_else(String::new, |range| range.label(at - range.start))
        })
        .collect()
}

/// The document's ranges, or `None` when it gives its pages no labels.
pub fn read(document: &Document) -> Option<Vec<Range>> {
    let catalog = document.catalog().ok()?;
    let tree = document
        .dereference(catalog.get(b"PageLabels").ok()?)
        .ok()?
        .1
        .as_dict()
        .ok()?;
    let mut ranges = Vec::new();
    let mut waiting = vec![(tree, 0)];
    while let Some((node, depth)) = waiting.pop() {
        let array = |key: &[u8]| {
            (node.get(key).ok())
                .and_then(|value| document.dereference(value).ok())
                .and_then(|(_, value)| value.as_array().ok())
        };
        for pair in array(b"Nums")
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .chunks(2)
        {
            let [start, range] = pair else { continue };
            let (Ok(start), Some(range)) = (
                start.as_i64(),
                (document.dereference(range).ok()).and_then(|(_, range)| range.as_dict().ok()),
            ) else {
                continue;
            };
            let prefix = (range.get(b"P").ok())
                .and_then(|prefix| lopdf::decode_text_string(prefix).ok())
                .unwrap_or_default();
            ranges.push(Range {
                start: start.max(0) as usize,
                style: (range.get(b"S").and_then(Object::as_name).ok()).and_then(Style::named),
                prefix,
                first: range
                    .get(b"St")
                    .and_then(Object::as_i64)
                    .map_or(1, |first| first.max(1) as u32),
            });
        }
        for kid in array(b"Kids").into_iter().flatten() {
            let kid = (document.dereference(kid).ok()).and_then(|(_, kid)| kid.as_dict().ok());
            if let (Some(kid), true) = (kid, depth < 32) {
                waiting.push((kid, depth + 1));
            }
        }
    }
    ranges.sort_by_key(|range| range.start);
    Some(ranges)
}

/// Write `ranges` as the document's labels, in place of any it gives.
fn write(document: &mut Document, ranges: &[Range]) {
    let mut numbers = Vec::new();
    for range in ranges {
        let mut entry = Dictionary::new();
        if let Some(style) = range.style {
            entry.set("S", Object::Name(style.name().to_vec()));
            if range.first != 1 {
                entry.set("St", range.first as i64);
            }
        }
        if !range.prefix.is_empty() {
            entry.set("P", lopdf::text_string(&range.prefix));
        }
        numbers.push(Object::Integer(range.start as i64));
        numbers.push(Object::Dictionary(entry));
    }
    let tree = document.add_object(lopdf::dictionary! { "Nums" => numbers });
    if let Ok(catalog) = document.catalog_mut() {
        catalog.set("PageLabels", tree);
    }
}

/// Give every page the label it had before an edit that `moved` its pages.
/// A page put in from another file has none, and is called by its place.
/// Answers whether the document gives its pages labels at all.
pub fn follow(document: &mut Document, moved: &Moved) -> bool {
    let Some(ranges) = read(document) else {
        return false;
    };
    let had = labels(&ranges, moved.0.len());
    let mut now = vec![String::new(); document.get_pages().len()];
    for (old, label) in had.into_iter().enumerate() {
        if let Some(new) = moved.0[old].filter(|&new| new < now.len()) {
            now[new] = label;
        }
    }
    write(document, &self::ranges(&now));
    true
}

/// Number the pages from `page` (zero-based) on as `typed` begins: "iv"
/// numbers them iv, v, vi, and "A-1" numbers them A-1, A-2. The numbering
/// runs until the next page the document starts a new range on. Pages
/// before it keep their labels, or are numbered by their place where the
/// document gives none.
pub fn number_from(document: &mut Document, page: usize, typed: &str) -> Result<(), String> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Err("Type what this page is called, like 1, iv or A-1.".into());
    }
    let mut readings = readings(typed);
    // A single c, d, l or m is more likely a letter than a numeral.
    if typed.len() == 1 && "cdlmCDLM".contains(typed) {
        readings.retain(|reading| {
            !matches!(reading.style, Some(Style::RomanLower | Style::RomanUpper))
        });
    }
    let mut ranges = read(document).unwrap_or_default();
    if !ranges.iter().any(|range| range.start == 0) {
        ranges.insert(
            0,
            Range {
                start: 0,
                style: Some(Style::Decimal),
                prefix: String::new(),
                first: 1,
            },
        );
    }
    ranges.retain(|range| range.start != page);
    ranges.push(Range {
        start: page,
        ..readings[0].clone()
    });
    ranges.sort_by_key(|range| range.start);
    write(document, &ranges);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(labels: &[&str]) -> Vec<String> {
        labels.iter().map(|label| label.to_string()).collect()
    }

    #[test]
    fn labels_come_back_from_the_ranges_made_of_them() {
        let books = [
            owned(&["i", "ii", "iii", "1", "2", "3", "A-1", "A-2", "Cover"]),
            owned(&["c", "d", "e"]),
            owned(&["xcix", "c", "ci"]),
            owned(&["", "", "1", "2"]),
            owned(&["7", "9", "10", "007"]),
            owned(&["z", "aa", "bb"]),
        ];
        for labels in books {
            let ranges = ranges(&labels);
            assert_eq!(self::labels(&ranges, labels.len()), labels);
        }
        let ranges = ranges(&owned(&["i", "ii", "iii", "1", "2", "3"]));
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[1].style, Some(Style::Decimal));
        let cover = &self::ranges(&owned(&["Cover"]))[0];
        assert_eq!((cover.style, cover.prefix.as_str()), (None, "Cover"));
    }

    #[test]
    fn numbers_are_written_in_every_style() {
        assert_eq!(roman(1994), "mcmxciv");
        assert_eq!(Style::RomanUpper.write(4), "IV");
        assert_eq!(letters(1), "a");
        assert_eq!(letters(28), "bb");
        assert_eq!(Style::LettersLower.read("bb"), Some(28));
        assert_eq!(Style::RomanLower.read("iiii"), None);
        assert_eq!(Style::Decimal.read("07"), None);
    }

    /// A document of `count` pages and no labels.
    fn pages(count: usize) -> Document {
        let mut document = Document::with_version("1.7");
        let tree = document.new_object_id();
        let kids: Vec<Object> = (0..count)
            .map(|_| {
                document
                    .add_object(lopdf::dictionary! { "Type" => "Page", "Parent" => tree })
                    .into()
            })
            .collect();
        let pages =
            lopdf::dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count as i64 };
        document.objects.insert(tree, Object::Dictionary(pages));
        let catalog =
            document.add_object(lopdf::dictionary! { "Type" => "Catalog", "Pages" => tree });
        document.trailer.set("Root", catalog);
        document
    }

    #[test]
    fn numbering_from_a_page_runs_to_the_next_range() {
        let mut document = pages(8);
        number_from(&mut document, 0, "i").unwrap();
        number_from(&mut document, 3, "1").unwrap();
        number_from(&mut document, 6, "A-1").unwrap();
        let read = read(&document).unwrap();
        assert_eq!(
            labels(&read, 8),
            owned(&["i", "ii", "iii", "1", "2", "3", "A-1", "A-2"])
        );
        // Numbered again from the second page, up to the next range.
        number_from(&mut document, 1, "c").unwrap();
        let read = super::read(&document).unwrap();
        assert_eq!(
            labels(&read, 8),
            owned(&["i", "c", "d", "1", "2", "3", "A-1", "A-2"])
        );
        assert!(number_from(&mut document, 1, " ").is_err());
    }

    #[test]
    fn labels_follow_their_pages() {
        let mut document = pages(4);
        number_from(&mut document, 0, "i").unwrap();
        number_from(&mut document, 2, "1").unwrap();
        // Pages 1 and 2 (i and ii) moved to the end.
        let moved = Moved(vec![Some(2), Some(3), Some(0), Some(1)]);
        assert!(follow(&mut document, &moved));
        let read = read(&document).unwrap();
        assert_eq!(labels(&read, 4), owned(&["1", "2", "i", "ii"]));
        // A document that gives no labels is left without.
        assert!(!follow(&mut pages(2), &Moved(vec![Some(1), Some(0)])));
    }
}
