//! Full-document search: the fold, the index, and stepping through what it
//! found.
//!
//! This is much shorter than `search.ts` for one reason that has nothing to do
//! with Rust: **pdfium answers per character.** pdf.js hands over *runs* — a
//! string and a transform — so the app has to join them, keep a `starts[]`,
//! binary-search it to turn a match back into a run and an offset, and measure
//! the result against a text layer. Here a match is a range of characters and
//! a character already knows its box. See [`crate::render::Rect`].
//!
//! What is ported exactly, because it is right and hard-won:
//!
//! * **`fold`.** Ligatures, accents and soft hyphens stand between a typed
//!   word and the same word in a PDF, and all three are invisible to the
//!   person typing. A line-for-line translation — minus the app's UTF-16
//!   care, because a `Vec<char>` cannot be half a character and the bug
//!   cannot be written.
//! * **Whole words tested against the *folded* text**, so a word hyphenated
//!   across a line is one whole word by the time the test sees it.
//! * **Starting at the page being read and going outwards.**
//! * **A cap on matches rather than on pages.** `MATCH_LIMIT` is the app's
//!   number: a page cap meant a common word in a long book reached it in the
//!   first chapter and the rest of the document was not capped but
//!   *unsearched*.
//!
//! **The scan is still sliced**, though pdfium makes a whole 376-page book of
//! typeset mathematics 498ms rather than the seconds pdf.js spends. Half a
//! second is still a window that stops answering while somebody is typing.
//!
//! **The index costs about thirty-six bytes a character**, three quarters of
//! it boxes — 20MB for that book — and is given back by [`Search::forget`]
//! when the bar closes. If it ever has to be smaller, the boxes of a page with
//! no match are the three quarters to drop; the characters have to stay,
//! because they are what makes changing "Match case" a refold rather than a
//! rescan.

use std::collections::BTreeMap;
use std::collections::HashMap;

use crate::render::{PageText, Rect};

/// Where a search gives up.
///
/// The app's own number and its own reasoning, restated in `search.ts`: this
/// was two thousand, which is a number a real query reaches, and reaching it
/// left the rest of the document unsearched rather than capped. A hundred
/// thousand is beyond any query anybody means.
pub const MATCH_LIMIT: usize = 100_000;

/// How long a slice of the scan runs before the window gets a turn.
pub const SLICE_MS: f64 = 8.0;

/// How many results a list shows. The app's `results(limit)` is handed one by
/// its caller; here the caller is the sidebar and there is one of it.
pub const RESULT_LIMIT: usize = 300;

/// How a query is matched.
///
/// "Highlight all" is not here, exactly as it is not in the app's
/// `SearchOptions`: it changes nothing about what is found, only how much of
/// it is painted, and that belongs to the viewer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub match_case: bool,
    pub whole_words: bool,
}

/// One match: a page, and the characters of it that matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// One-based, as every page number in this crate is.
    pub page: usize,
    /// Into the page's own characters — which is also into its boxes.
    pub from: usize,
    pub to: usize,
}

/// What the find bar says about itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub query: String,
    pub total: usize,
    /// Which match the reader is on, zero-based; `None` when there is none.
    pub at: Option<usize>,
    pub scanning: bool,
    /// True when the scan stopped at [`MATCH_LIMIT`], so the total is a floor.
    pub capped: bool,
    /// True when the scan read the whole document and found no text in it at
    /// all — a scan nobody put through OCR, or a renderer with no text
    /// extraction (see [`crate::render::PageSource::text_of`]). Nothing is
    /// wrong; there is simply nothing to search, and "None" says the other
    /// thing.
    pub textless: bool,
}

/// One line of the results list: enough of the document either side of a match
/// to read it as a sentence.
#[derive(Clone, Debug, PartialEq)]
pub struct Result {
    pub at: usize,
    pub page: usize,
    pub before: String,
    pub hit: String,
    pub after: String,
}

/// A page, as the index holds it.
struct Indexed {
    text: PageText,
    /// The page's characters folded, and where each folded character came
    /// from. Rebuilt when "Match case" moves, which is the cheap half — the
    /// trip into the renderer is the expensive one.
    fold: Fold,
    /// Whether [`Indexed::fold`] was made with the case left alone.
    cased: bool,
}

/// The index, the matches, and where the reader is in them.
///
/// Deliberately knows nothing about the viewer, which the app's `Search` is
/// constructed with: this one is fed pages and asked questions, so the whole
/// of it can be tested without a document, a window or a screen. What drives
/// it is `app.rs`.
#[derive(Default)]
pub struct Search {
    pages: HashMap<usize, Indexed>,
    /// Matches by page, so the ordered list can be rebuilt as pages come in
    /// without sorting the whole of it again.
    found: BTreeMap<usize, Vec<Hit>>,
    matches: Vec<Hit>,
    at: Option<usize>,
    /// The first match the scan found, which is the nearest one at or after
    /// the page the reader was on rather than the first in the document.
    ///
    /// The list is in page order and the scan is not, so without this a
    /// search from page three of five settles on page one — technically the
    /// first result and never the one anybody meant. `preferred` in
    /// `search.ts` is the same variable doing the same job.
    preferred: Option<Hit>,
    query: String,
    needle: Vec<char>,
    options: Options,
    capped: bool,
    scanning: bool,
    textless: bool,
    /// The pages still to be read, in the order to read them.
    queue: Vec<usize>,
}

impl Search {
    pub fn new() -> Search {
        Search::default()
    }

    pub fn options(&self) -> Options {
        self.options
    }

    /// Change how a query is matched. The extracted text stays: only the fold
    /// and the boundary test depend on these, and both are cheap.
    pub fn set_options(&mut self, options: Options) {
        self.options = options;
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn scanning(&self) -> bool {
        self.scanning
    }

    pub fn matches(&self) -> &[Hit] {
        &self.matches
    }

    /// Put the index down.
    ///
    /// Every page ever scanned is kept, which is what makes stepping through
    /// matches instant and what makes a long book cost tens of megabytes for
    /// as long as it is open. A fair trade while the find bar is up and no
    /// trade at all once it is closed — so the index goes when the bar does,
    /// and reopening it rescans, which is under half a second.
    pub fn forget(&mut self) {
        self.pages.clear();
        self.pages.shrink_to_fit();
        self.clear();
    }

    /// Stop looking, and keep what has been read.
    pub fn clear(&mut self) {
        self.query.clear();
        self.needle.clear();
        self.found.clear();
        self.matches.clear();
        self.matches.shrink_to_fit();
        self.queue.clear();
        self.at = None;
        self.preferred = None;
        self.capped = false;
        self.scanning = false;
        self.textless = false;
    }

    /// Start looking for `query`, from the page being read outwards.
    ///
    /// Returns whether there is a scan to run: a query that is nothing but
    /// whitespace, or one that folds away to nothing at all — a string of soft
    /// hyphens — finds nothing rather than matching at every position.
    pub fn find(&mut self, query: &str, from_page: usize, pages: usize) -> bool {
        self.clear();
        self.query = query.to_string();
        if query.trim().is_empty() {
            return false;
        }
        let typed: Vec<char> = query.chars().collect();
        self.needle = fold(&typed, self.options.match_case).text;
        if self.needle.is_empty() {
            return false;
        }
        self.textless = true;
        self.scanning = true;
        // Start at the page being read, then outwards, so the first result is
        // usually the one just below the reader's eyes. Reversed because the
        // queue is drained from the back.
        self.queue = pages_from_here(from_page, pages);
        self.queue.reverse();
        true
    }

    /// The next page the scan wants, or `None` when it is done.
    pub fn wants(&self) -> Option<usize> {
        self.queue.last().copied()
    }

    /// Hand over the page [`Search::wants`] asked for.
    ///
    /// `text` is only read when the page has not been seen before, so a
    /// caller that already knows the page is indexed may hand over an empty
    /// one — which is what a rescan after a change of options does, and is why
    /// changing "Match case" does not go back to the renderer.
    pub fn feed(&mut self, page: usize, text: impl FnOnce() -> PageText) {
        if self.queue.last() != Some(&page) {
            return;
        }
        self.queue.pop();
        let case = self.options.match_case;
        let indexed = self.pages.entry(page).or_insert_with(|| {
            let text = text();
            let fold = fold(&text.chars, case);
            Indexed {
                text,
                fold,
                cased: case,
            }
        });
        // Brought up to date if the case setting has moved since it was read.
        if indexed.cased != case {
            indexed.fold = fold(&indexed.text.chars, case);
            indexed.cased = case;
        }
        // A document with nothing to search is a different answer from a
        // document that does not contain what was asked for, and "None" says
        // the second when it means the first. One page with a word on it is
        // enough to settle it.
        if indexed.text.chars.iter().any(|c| !c.is_whitespace()) {
            self.textless = false;
        }
        if self.capped {
            return;
        }
        let mut hits = locate(&indexed.fold, &self.needle, page, self.options.whole_words);
        if hits.is_empty() {
            return;
        }
        // Strictly greater: a document with exactly `MATCH_LIMIT` matches has
        // had none of them dropped, and a "+" on an exact count is a lie.
        let total: usize = self.found.values().map(Vec::len).sum();
        if total + hits.len() > MATCH_LIMIT {
            hits.truncate(MATCH_LIMIT - total);
            self.capped = true;
            self.queue.clear();
        }
        if !hits.is_empty() {
            self.preferred = self.preferred.or(Some(hits[0]));
            self.found.insert(page, hits);
        }
    }

    /// Rebuild the ordered list from what has been found so far, keeping the
    /// reader on the match they were on.
    ///
    /// `publish` in `search.ts`, less the two calls into the viewer: nothing
    /// here reaches into anything.
    pub fn publish(&mut self) {
        let standing = self
            .at
            .and_then(|at| self.matches.get(at).copied())
            .or(self.preferred);
        self.matches = self.found.values().flatten().copied().collect();
        self.at = standing
            .and_then(|hit| self.matches.iter().position(|&other| other == hit))
            .or(if self.matches.is_empty() {
                None
            } else {
                Some(0)
            });
        if self.queue.is_empty() {
            self.scanning = false;
        }
    }

    /// Move to the next match, or the one before. Wraps, which is what every
    /// find bar does and what makes ⌘G a way of walking a document.
    pub fn step(&mut self, forwards: bool) {
        if self.matches.is_empty() {
            return;
        }
        let count = self.matches.len();
        self.at = Some(match self.at {
            Some(at) if forwards => (at + 1) % count,
            Some(at) => (at + count - 1) % count,
            None => 0,
        });
    }

    /// Go to one result by its place in the list — what a row of the results
    /// tab does when it is clicked.
    pub fn go_to(&mut self, at: usize) {
        if at < self.matches.len() {
            self.at = Some(at);
        }
    }

    pub fn current(&self) -> Option<Hit> {
        self.at.and_then(|at| self.matches.get(at).copied())
    }

    /// Every match on one page, and which of them is the current one, as
    /// rectangles in PDF points from the top left of the page.
    ///
    /// This is the whole of what the viewer needs to paint highlights, and it
    /// is the payoff for [`crate::render::Rect`]: the app measures a
    /// `Range` against a text layer to get here.
    pub fn quads_on(&self, page: usize) -> Vec<(Rect, bool)> {
        let Some(indexed) = self.pages.get(&page) else {
            return Vec::new();
        };
        let current = self.current();
        let mut out = Vec::new();
        for hit in self.found.get(&page).map(Vec::as_slice).unwrap_or(&[]) {
            let now = current == Some(*hit);
            out.extend(
                indexed
                    .text
                    .quads(hit.from, hit.to)
                    .into_iter()
                    .map(|quad| (quad, now)),
            );
        }
        out
    }

    /// The matches, in page order, with a line of the document either side of
    /// each — for a list somebody reads rather than steps through.
    ///
    /// Cut on demand from the text already indexed rather than kept beside
    /// every match, and bounded, because a list of ten thousand results is not
    /// a list.
    pub fn results(&self, limit: usize) -> Vec<Result> {
        let mut out = Vec::new();
        for (at, hit) in self.matches.iter().enumerate() {
            if out.len() >= limit {
                break;
            }
            let Some(indexed) = self.pages.get(&hit.page) else {
                continue;
            };
            let chars = &indexed.text.chars;
            // Short before, long after. The line is one row of a narrow panel
            // and is cut off at the end, so a match with as much in front of
            // it as behind is a match nobody can see: what is wanted is enough
            // to place it and then the sentence it is in.
            let before = hit.from.saturating_sub(18);
            let after = (hit.to + 70).min(chars.len());
            out.push(Result {
                at,
                page: hit.page,
                before: tidy(&chars[before..hit.from]),
                hit: tidy(&chars[hit.from..hit.to.min(chars.len())]),
                after: tidy(&chars[hit.to.min(chars.len())..after]),
            });
        }
        out
    }

    pub fn state(&self) -> State {
        State {
            query: self.query.clone(),
            total: self.matches.len(),
            at: self.at,
            scanning: self.scanning,
            capped: self.capped,
            textless: self.textless && !self.scanning,
        }
    }
}

/// Whitespace collapsed, because a PDF's own line breaks fall wherever the
/// printer put them and a result should read as a sentence.
fn tidy(chars: &[char]) -> String {
    let mut out = String::with_capacity(chars.len());
    let mut space = false;
    for &character in chars {
        if character.is_whitespace() {
            space = true;
            continue;
        }
        if space {
            out.push(' ');
        }
        space = false;
        out.push(character);
    }
    // A run of whitespace at either end collapses to one space rather than to
    // nothing: the line either side of a match is a cut out of a sentence, and
    // a cut that closes up reads as a word running into the match.
    if space {
        out.push(' ');
    }
    out
}

/// The pages of a document, starting at the one being read.
pub fn pages_from_here(current: usize, count: usize) -> Vec<usize> {
    let current = current.clamp(1, count.max(1));
    let mut order: Vec<usize> = (current..=count).collect();
    order.extend(1..current);
    order
}

/// A page's characters folded, and where each folded character came from.
#[derive(Debug, Default)]
pub struct Fold {
    pub text: Vec<char>,
    /// For each character of `text`, which character of the input it came
    /// from — and one more at the end, so a match running to the last
    /// character has somewhere to point.
    pub origin: Vec<usize>,
}

/// Fold text into the form a search is actually done against, and record where
/// every character of the result came from.
///
/// Three things stand between a typed word and the same word in a PDF, and all
/// three are invisible to the person typing:
///
/// * **Ligatures.** A professionally typeset document does not contain "fi" —
///   it contains "ﬁ", one character. Searching for "find" in a book set in
///   anything but Courier found nothing at all, which reads as the search
///   being broken rather than as a fact about typography.
/// * **Accents.** Someone typing "resume" means to find "résumé". Decomposing
///   and dropping the combining marks makes both sides the same word.
/// * **Soft hyphens.** A word broken across a line keeps a U+00AD in the
///   extracted text, so "typography" split at the margin is two words to an
///   exact match and one word to a reader.
///
/// `origin` is what keeps the answer usable: folding changes lengths, so a hit
/// at index *i* in the folded text has to be translated back before it can be
/// turned into a range of the page's own characters — and so into boxes.
///
/// Case is the one part of this the reader can turn off. The other three are
/// not offered as choices because nobody types a soft hyphen on purpose.
pub fn fold(input: &[char], case_sensitive: bool) -> Fold {
    let mut text = Vec::with_capacity(input.len());
    let mut origin = Vec::with_capacity(input.len());
    // A hyphen that was only a line break has just been dropped, so the line
    // break after it goes too: "typo-" and "graphy" are one word.
    let mut joining = false;
    for (source, &character) in input.iter().enumerate() {
        // NFKD splits the ligatures into their letters and the accented
        // letters into a letter plus its marks; the marks are then dropped.
        // Done a character at a time so that every piece of the result knows
        // which character of the original it came from.
        unicode_normalization::char::decompose_compatible(character, |piece| {
            if combining(piece) || ignored(piece) {
                joining |= hyphen(piece);
                return;
            }
            // Any run of white space is one space: pdfium ends a line with
            // "\r\n", and a phrase typed with a space has to find itself
            // where the typesetter broke it.
            if piece.is_whitespace() {
                if !joining && text.last() != Some(&' ') {
                    text.push(' ');
                    origin.push(source);
                }
                return;
            }
            joining = false;
            if case_sensitive {
                text.push(piece);
                origin.push(source);
            } else {
                for lowered in piece.to_lowercase() {
                    // İ lowers to i plus a combining dot; the dot is a mark
                    // like any other, and "İstanbul" has to find "istanbul".
                    if combining(lowered) {
                        continue;
                    }
                    text.push(lowered);
                    origin.push(source);
                }
            }
        });
    }
    // One past the end, so a match that runs to the last character has
    // somewhere to point its end at.
    origin.push(input.len());
    Fold { text, origin }
}

/// Combining marks, which are what is left of an accent after NFKD.
fn combining(character: char) -> bool {
    matches!(character as u32,
        0x0300..=0x036f
        | 0x1ab0..=0x1aff
        | 0x1dc0..=0x1dff
        | 0x20d0..=0x20f0
        | 0xfe20..=0xfe2f)
}

/// Characters that are in the text but not in the word: the soft hyphen, and
/// the zero-width joiners that some producers scatter through it.
///
/// U+0002 and U+FFFE are pdfium's: what it reports in place of a hyphen that
/// ends a line.
fn ignored(character: char) -> bool {
    hyphen(character) || matches!(character as u32, 0x200b..=0x200d | 0xfeff)
}

fn hyphen(character: char) -> bool {
    matches!(character as u32, 0x0002 | 0x00ad | 0xfffe)
}

/// Letters, digits and the underscore: what "whole words" counts as being part
/// of a word, in every alphabet rather than only the Latin one.
fn word(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// Whether a match has something other than a word character on each side.
///
/// The test is done against the folded text, which is the point of doing it
/// here rather than against the page: a word hyphenated across a line break
/// has already had the soft hyphen taken out of it, so "typo-graphy" at the
/// margin is one whole word by the time this sees it, as it is to a reader.
fn stands_alone(text: &[char], start: usize, end: usize) -> bool {
    if start > 0 && word(text[start - 1]) {
        return false;
    }
    if end < text.len() && word(text[end]) {
        return false;
    }
    true
}

/// Every place `needle` occurs in a folded page, as ranges of the page's own
/// characters.
pub fn locate(page: &Fold, needle: &[char], number: usize, whole_words: bool) -> Vec<Hit> {
    let mut found = Vec::new();
    if needle.is_empty() || page.text.len() < needle.len() {
        return found;
    }
    let mut at = 0;
    while at + needle.len() <= page.text.len() {
        if &page.text[at..at + needle.len()] != needle {
            at += 1;
            continue;
        }
        if whole_words && !stands_alone(&page.text, at, at + needle.len()) {
            // A rejected hit only moves the search on by one: "and" inside
            // "understand" is not a word, but the "and" that ends it is, and
            // it starts one character later.
            at += 1;
            continue;
        }
        // Back from the folded text to the page's own characters. The end has
        // to clear the last character it matched: one source character can
        // fold to several — "ﬁ" is two — so a match ending inside a ligature
        // would otherwise start and end on the same character and highlight
        // nothing.
        let last = at + needle.len() - 1;
        let from = page.origin[at];
        let to = page.origin[at + needle.len()].max(page.origin[last] + 1);
        found.push(Hit {
            page: number,
            from,
            to,
        });
        at += needle.len();
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(text: &str) -> Vec<char> {
        text.chars().collect()
    }

    fn folded(text: &str) -> String {
        fold(&chars(text), false).text.into_iter().collect()
    }

    /// A phrase finds itself over a line break, and a word over a hyphenated
    /// one — pdfium's own marker for it included.
    #[test]
    fn a_line_break_is_a_space_and_a_line_end_hyphen_is_nothing() {
        assert_eq!(folded("needle\r\nagain  and"), "needle again and");
        assert_eq!(folded("typo\u{2}\r\ngraphy"), "typography");
        assert_eq!(folded("typo\u{ad}graphy"), "typography");
    }

    /// The three things that stand between a typed word and the same word in
    /// a PDF. `search.test.mjs` asserts exactly these, and this is the same
    /// list.
    #[test]
    fn ligatures_accents_and_soft_hyphens_all_fold_away() {
        assert_eq!(folded("ﬁnd"), "find");
        assert_eq!(folded("ﬄuent"), "ffluent");
        assert_eq!(folded("résumé"), "resume");
        assert_eq!(folded("typo\u{00ad}graphy"), "typography");
        assert_eq!(folded("a\u{200b}b"), "ab");
        assert_eq!(folded("MiXeD"), "mixed");
        // İ lowers to i plus a combining dot, which is a mark like the rest.
        assert_eq!(folded("İstanbul"), "istanbul");
        assert_eq!(
            fold(&chars("MiXeD"), true).text.iter().collect::<String>(),
            "MiXeD"
        );
    }

    /// Everything above the basic plane goes through whole — which in
    /// `search.ts` needs a comment and a deliberate iteration by code point,
    /// and here cannot be got wrong, because a `char` is not half of
    /// anything.
    #[test]
    fn mathematical_bold_folds_to_the_letters_on_the_keyboard() {
        assert_eq!(folded("\u{1d400}\u{1d401}"), "ab");
    }

    /// `origin` is what makes a hit usable, and the property worth asserting
    /// is that it never lies about lengths: every folded character points at
    /// a real character of the input, and the extra entry at the end points
    /// one past it.
    #[test]
    fn every_folded_character_knows_where_it_came_from() {
        let input = chars("ﬁt\u{00ad}résumé");
        let folded = fold(&input, false);
        assert_eq!(folded.origin.len(), folded.text.len() + 1);
        assert_eq!(*folded.origin.last().unwrap(), input.len());
        assert!(folded.origin.iter().all(|&at| at <= input.len()));
        // Monotonic: folding reorders nothing.
        assert!(folded.origin.windows(2).all(|pair| pair[0] <= pair[1]));
        // "ﬁ" is one character and two folded ones, and both point at it.
        assert_eq!(folded.origin[0], 0);
        assert_eq!(folded.origin[1], 0);
    }

    /// A match that ends inside a ligature still covers the ligature — the
    /// one place the translation back is not simply `origin[end]`.
    #[test]
    fn a_match_ending_inside_a_ligature_covers_it() {
        let page = fold(&chars("aﬁb"), false);
        let hits = locate(&page, &chars("af"), 1, false);
        assert_eq!(
            hits,
            vec![Hit {
                page: 1,
                from: 0,
                to: 2
            }]
        );
    }

    #[test]
    fn whole_words_is_tested_against_the_folded_text() {
        let page = fold(&chars("understand and stand"), false);
        assert_eq!(locate(&page, &chars("and"), 1, false).len(), 3);
        // And a hit rejected for sitting inside a word does not end the scan:
        // the standalone "and" is found after two rejections.
        let whole = locate(&page, &chars("and"), 1, true);
        assert_eq!(whole.len(), 1);
        assert_eq!(whole[0].from, 11);
        // And a word broken across a line is whole again by the time the test
        // sees it, which is the reason the test is done here.
        let broken = fold(&chars("typo\u{00ad}graphy is"), false);
        assert_eq!(locate(&broken, &chars("typography"), 1, true).len(), 1);
    }

    /// Overlapping matches are counted once, which is what advancing by the
    /// length of the needle means and is what the app does: "banana" holds
    /// two "ana"s that share a letter and a reader stepping through them
    /// would visit the same word twice.
    #[test]
    fn overlapping_matches_are_counted_once() {
        let page = fold(&chars("bandana banana"), false);
        let hits = locate(&page, &chars("ana"), 1, false);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].from, 4);
        assert_eq!(hits[1].from, 9);
    }

    #[test]
    fn the_scan_starts_where_the_reader_is_and_comes_round() {
        assert_eq!(pages_from_here(3, 5), vec![3, 4, 5, 1, 2]);
        assert_eq!(pages_from_here(1, 3), vec![1, 2, 3]);
        // A page number out of range is clamped rather than producing
        // nothing: the reader is somewhere.
        assert_eq!(pages_from_here(9, 3), vec![3, 1, 2]);
        assert_eq!(pages_from_here(0, 2), vec![1, 2]);
    }

    /// A page of text, laid out as one line of equal boxes, which is enough
    /// for everything but the joining test below.
    fn page_of(text: &str) -> PageText {
        let chars: Vec<char> = text.chars().collect();
        let boxes = chars
            .iter()
            .enumerate()
            .map(|(at, _)| Rect {
                left: at as f64 * 10.0,
                top: 100.0,
                width: 10.0,
                height: 12.0,
            })
            .collect();
        PageText { chars, boxes }
    }

    fn scan(search: &mut Search, pages: &[&str]) {
        while let Some(page) = search.wants() {
            let text = page_of(pages[page - 1]);
            search.feed(page, || text);
        }
        search.publish();
    }

    #[test]
    fn a_scan_finds_every_match_and_settles_on_the_first_one_below_the_reader() {
        let pages = ["a needle here", "nothing", "needle and needle"];
        let mut search = Search::new();
        assert!(search.find("needle", 3, 3));
        scan(&mut search, &pages);
        let state = search.state();
        assert_eq!(state.total, 3);
        assert!(!state.scanning);
        assert!(!state.textless);
        // The list is in page order…
        assert_eq!(
            search
                .matches()
                .iter()
                .map(|hit| hit.page)
                .collect::<Vec<_>>(),
            vec![1, 3, 3]
        );
        // …and the reader is on the first match at or after the page they
        // were reading, which is the point of scanning from there.
        assert_eq!(search.current().map(|hit| hit.page), Some(3));
    }

    #[test]
    fn stepping_wraps_in_both_directions() {
        let mut search = Search::new();
        assert!(search.find("a", 1, 2));
        scan(&mut search, &["a a", "a"]);
        assert_eq!(search.state().total, 3);
        assert_eq!(search.state().at, Some(0));
        search.step(true);
        search.step(true);
        assert_eq!(search.state().at, Some(2));
        search.step(true);
        assert_eq!(search.state().at, Some(0));
        search.step(false);
        assert_eq!(search.state().at, Some(2));
    }

    /// A document with nothing in it to search says so, and it is a different
    /// sentence from a document that does not contain the word.
    #[test]
    fn a_document_with_no_text_says_so_and_one_without_the_word_does_not() {
        let mut search = Search::new();
        assert!(search.find("needle", 1, 2));
        scan(&mut search, &["", "   "]);
        assert!(search.state().textless);

        let mut search = Search::new();
        assert!(search.find("needle", 1, 2));
        scan(&mut search, &["haystack", "hay"]);
        assert_eq!(search.state().total, 0);
        assert!(!search.state().textless);
    }

    /// A query of nothing, or of nothing that survives folding, finds nothing
    /// rather than matching at every position.
    #[test]
    fn an_empty_needle_is_refused_rather_than_matching_everywhere() {
        let mut search = Search::new();
        assert!(!search.find("   ", 1, 1));
        assert!(!search.find("\u{00ad}\u{200b}", 1, 1));
        assert_eq!(search.state().total, 0);
    }

    /// Changing "Match case" refolds rather than re-extracting — the trip into
    /// the renderer is the expensive half, so a second scan must not ask for
    /// a page it already has.
    #[test]
    fn changing_the_case_setting_does_not_go_back_to_the_renderer() {
        let mut search = Search::new();
        assert!(search.find("Needle", 1, 1));
        scan(&mut search, &["a Needle and a needle"]);
        assert_eq!(search.state().total, 2);

        search.set_options(Options {
            match_case: true,
            whole_words: false,
        });
        assert!(search.find("Needle", 1, 1));
        while let Some(page) = search.wants() {
            // The page is already indexed, so nothing may be asked of the
            // renderer: a closure that panics is how that is said.
            search.feed(page, || panic!("the renderer was asked again"));
        }
        search.publish();
        assert_eq!(search.state().total, 1);
    }

    /// The reader stays on the match they were on while the scan is still
    /// bringing more in, which is what `publish` is for.
    #[test]
    fn a_reader_stepping_while_the_scan_runs_keeps_their_place() {
        let mut search = Search::new();
        assert!(search.find("x", 1, 3));
        // One page at a time, publishing as the app does.
        let pages = ["x x", "x", "x x"];
        search.feed(1, || page_of(pages[0]));
        search.publish();
        assert!(search.state().scanning);
        search.step(true);
        let standing = search.current().unwrap();
        search.feed(2, || page_of(pages[1]));
        search.publish();
        assert_eq!(search.current(), Some(standing));
        search.feed(3, || page_of(pages[2]));
        search.publish();
        assert_eq!(search.current(), Some(standing));
        assert!(!search.state().scanning);
        assert_eq!(search.state().total, 5);
    }

    /// A match is a few rectangles, one per line, rather than one per
    /// character.
    #[test]
    fn a_match_on_one_line_is_one_rectangle() {
        let mut search = Search::new();
        assert!(search.find("needle", 1, 1));
        scan(&mut search, &["a needle"]);
        let quads = search.quads_on(1);
        assert_eq!(quads.len(), 1);
        let (quad, current) = quads[0];
        assert!(current, "the only match is the current one");
        assert_eq!(quad.left, 20.0);
        assert_eq!(quad.width, 60.0);
        assert_eq!(quad.top, 100.0);
    }

    /// And a match broken across two lines is two, because a single box round
    /// the pair would cover the whole width of the paragraph.
    #[test]
    fn a_match_across_a_line_break_is_two_rectangles() {
        let text = PageText {
            chars: chars("ab"),
            boxes: vec![
                Rect {
                    left: 500.0,
                    top: 100.0,
                    width: 10.0,
                    height: 12.0,
                },
                Rect {
                    left: 20.0,
                    top: 130.0,
                    width: 10.0,
                    height: 12.0,
                },
            ],
        };
        assert_eq!(text.quads(0, 2).len(), 2);
    }

    /// A result reads as a sentence: whitespace collapsed, enough in front to
    /// place it and the line it is in behind.
    #[test]
    fn a_result_is_a_line_of_the_document() {
        let mut search = Search::new();
        assert!(search.find("needle", 1, 1));
        scan(&mut search, &["there is a\n  needle in this   haystack"]);
        let results = search.results(RESULT_LIMIT);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].hit, "needle");
        assert_eq!(results[0].before, "there is a ");
        assert_eq!(results[0].after, " in this haystack");
        assert_eq!(results[0].page, 1);
    }

    /// Forgetting the index leaves nothing behind, which is what closing the
    /// find bar does and is the whole of the memory policy.
    #[test]
    fn forgetting_puts_the_index_down() {
        let mut search = Search::new();
        assert!(search.find("a", 1, 1));
        scan(&mut search, &["a a a"]);
        assert_eq!(search.state().total, 3);
        search.forget();
        assert_eq!(search.state().total, 0);
        assert!(search.quads_on(1).is_empty());
        assert!(search.results(RESULT_LIMIT).is_empty());
    }
}
