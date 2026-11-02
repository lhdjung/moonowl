//! Two versions of a document compared word by word: what the document has
//! that the other file does not, and what the other file had that the
//! document no longer has. Nothing is written to either file.
//!
//! Words are compared as the search compares them, after
//! [`crate::search::fold`], so a ligature, an accent or a hyphen at the end
//! of a line is not a change. A word broken across two lines of a page with
//! a hyphen is one word, since the same word in the other version may sit in
//! the middle of a line. A word broken across two pages stays two. The
//! hyphen pdfium reports at the end of a line, and the soft hyphen, are
//! left out of the words shown.

use crate::render::{PageSource, PageText};

/// How long the comparison may take before it settles for a coarser answer,
/// in which a long stretch of changes is one change.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(5);

/// How many characters of a change's words are kept, about four pages.
/// After the comparison gives up being exact, one change can be a whole
/// chapter, which is better read on the pages of the file that has it.
const SHOWN: usize = 10_000;

/// One word of a document: what is compared, what is shown, and where it
/// is. The first two are numbers in [`Vocabulary`].
struct Word {
    key: u32,
    shown: u32,
    /// One-based.
    page: usize,
    from: usize,
    to: usize,
}

/// Every different word of both files, kept once. A word of a document is
/// its number here, so a long book costs a few bytes a word rather than two
/// strings a word.
#[derive(Default)]
struct Vocabulary {
    numbers: std::collections::HashMap<Box<str>, u32>,
    words: Vec<Box<str>>,
}

impl Vocabulary {
    fn number(&mut self, word: &str) -> u32 {
        if let Some(&number) = self.numbers.get(word) {
            return number;
        }
        let number = self.words.len() as u32;
        self.words.push(word.into());
        self.numbers.insert(word.into(), number);
        number
    }
}

/// One change. Neither file is taken to be the earlier one, so each side
/// is said in the same way.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    /// Where its words are in the document, a range of characters on each
    /// page they are on. Where the document has no words of the change,
    /// there is one place, with `from` and `to` the same, where the other
    /// file's words would be.
    pub places: Vec<(usize, usize, usize)>,
    /// The same in the other file.
    pub places_there: Vec<(usize, usize, usize)>,
    /// The words the document has here that the other file does not, cut
    /// short after `SHOWN` characters.
    pub here: String,
    /// The words the other file has here that the document does not, cut
    /// short in the same way.
    pub there: String,
    /// How many words the document has here that the other file does not.
    pub count: usize,
    /// How many words the other file has here that the document does not.
    pub count_there: usize,
}

impl Change {
    /// The page it begins on, one-based.
    pub fn page(&self) -> usize {
        self.places.first().map_or(1, |&(page, _, _)| page)
    }

    /// The page it begins on in the other file, one-based.
    pub fn page_there(&self) -> usize {
        self.places_there.first().map_or(1, |&(page, _, _)| page)
    }
}

/// Every change from `other` to `document`, in the document's order.
// ponytail: every word of both files is held at once, about 32 bytes a
// word, or 45MB for two versions of a 1,700-page book. Comparing page by
// page would need the pages to line up.
pub fn compare(document: &dyn PageSource, other: &dyn PageSource) -> Result<Vec<Change>, String> {
    let mut vocabulary = Vocabulary::default();
    let ours = words_of(document, &mut vocabulary)?;
    let theirs = words_of(other, &mut vocabulary)?;
    let ops = similar::capture_diff_slices_deadline(
        similar::Algorithm::Patience,
        &keys(&theirs),
        &keys(&ours),
        Some(std::time::Instant::now() + PATIENCE),
    );
    let said = |words: &[Word]| {
        let mut said = String::new();
        for word in words {
            if !said.is_empty() {
                said.push(' ');
            }
            said.push_str(&vocabulary.words[word.shown as usize]);
            if let Some((at, _)) = said.char_indices().nth(SHOWN) {
                said.truncate(at);
                said.push('\u{2026}');
                break;
            }
        }
        said
    };
    let mut changes = Vec::new();
    for op in ops {
        let (old, new) = (op.old_range(), op.new_range());
        if matches!(op, similar::DiffOp::Equal { .. }) {
            continue;
        }
        changes.push(Change {
            count: new.len(),
            count_there: old.len(),
            places: places(&ours, new.clone()),
            places_there: places(&theirs, old.clone()),
            here: said(&ours[new]),
            there: said(&theirs[old]),
        });
    }
    Ok(changes)
}

/// What is compared of each word.
fn keys(words: &[Word]) -> Vec<u32> {
    words.iter().map(|word| word.key).collect()
}

/// The words `range` of `all` as ranges of characters, one range per page
/// they are on. No words are a place between two: before the word that
/// follows, after the last word of the document, or at the top of the first
/// page if the document has no words at all.
fn places(all: &[Word], range: std::ops::Range<usize>) -> Vec<(usize, usize, usize)> {
    if range.is_empty() {
        return match (all.get(range.start), all.last()) {
            (Some(next), _) => vec![(next.page, next.from, next.from)],
            (None, Some(last)) => vec![(last.page, last.to, last.to)],
            (None, None) => vec![(1, 0, 0)],
        };
    }
    let mut out: Vec<(usize, usize, usize)> = Vec::new();
    for word in &all[range] {
        match out.last_mut() {
            Some((page, _, to)) if *page == word.page => *to = word.to,
            _ => out.push((word.page, word.from, word.to)),
        }
    }
    out
}

/// Every word of a document, in the order its pages have them.
fn words_of(document: &dyn PageSource, vocabulary: &mut Vocabulary) -> Result<Vec<Word>, String> {
    let mut words = Vec::new();
    for index in 0..document.pages() {
        if crate::export::GIVE_UP.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(String::new());
        }
        let text = document
            .text_aside(index)
            .ok_or(crate::export::CLOSED_WHILE_READ)?;
        words_on(&text, index + 1, vocabulary, &mut words);
    }
    Ok(words)
}

/// The words on one page. A word that ends in a hyphen at the end of a line
/// is joined to the first word of the next one.
fn words_on(text: &PageText, page: usize, vocabulary: &mut Vocabulary, out: &mut Vec<Word>) {
    let chars = &text.chars;
    let mut at = 0;
    let mut joining = false;
    while at < chars.len() {
        if chars[at].is_whitespace() {
            at += 1;
            continue;
        }
        let from = at;
        while at < chars.len() && !chars[at].is_whitespace() {
            at += 1;
        }
        let word: String = (chars[from..at].iter())
            .filter(|&&c| !crate::search::ignored(c))
            .collect();
        if word.is_empty() {
            continue;
        }
        let key: String = crate::search::fold(&chars[from..at], true)
            .text
            .into_iter()
            .collect();
        let last = chars[at - 1];
        let broken = (crate::search::hyphen(last)
            || matches!(last, '-' | '\u{2010}') && at - from > 1)
            && chars.get(at).is_some_and(|&c| c == '\r' || c == '\n');
        match out.last_mut() {
            Some(last) if joining => {
                let before = |number: u32| {
                    (vocabulary.words[number as usize].trim_end_matches(['-', '\u{2010}']))
                        .to_string()
                };
                let (key_before, shown_before) = (before(last.key), before(last.shown));
                last.key = vocabulary.number(&format!("{key_before}{key}"));
                last.shown = vocabulary.number(&format!("{shown_before}{word}"));
                last.to = at;
            }
            _ => out.push(Word {
                key: vocabulary.number(&key),
                shown: vocabulary.number(&word),
                page,
                from,
                to: at,
            }),
        }
        joining = broken;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Cell;

    fn page(text: &str) -> PageText {
        let chars: Vec<char> = text.chars().collect();
        let boxes = vec![Cell::default(); chars.len()];
        PageText { chars, boxes }
    }

    fn keys(text: &str) -> Vec<String> {
        let (mut vocabulary, mut out) = (Vocabulary::default(), Vec::new());
        words_on(&page(text), 1, &mut vocabulary, &mut out);
        (out.iter())
            .map(|word| vocabulary.words[word.key as usize].to_string())
            .collect()
    }

    /// A word is a handful of numbers, which is what makes a long book
    /// affordable. See the note on [`compare`].
    #[test]
    fn a_word_costs_32_bytes() {
        assert_eq!(std::mem::size_of::<Word>(), 32);
    }

    #[test]
    fn a_word_broken_at_a_line_is_one_word() {
        assert_eq!(
            keys("the typo-\r\ngraphy of it"),
            ["the", "typography", "of", "it"]
        );
        assert_eq!(keys("a well-known word"), ["a", "well-known", "word"]);
        assert_eq!(keys("ends with -\r\nnext"), ["ends", "with", "-", "next"]);
        assert_eq!(keys("re\u{2}\r\ntrieved"), ["retrieved"]);
        let (mut vocabulary, mut out) = (Vocabulary::default(), Vec::new());
        words_on(&page("Re\u{2}trieved"), 1, &mut vocabulary, &mut out);
        assert_eq!(&*vocabulary.words[out[0].shown as usize], "Retrieved");
    }
}
