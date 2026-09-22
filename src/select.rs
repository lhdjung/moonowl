//! Selecting words on a page: where a caret lands, what is between two of
//! them, and what that reads as.
//!
//! **This is the file the app does not have**, because a webview comes with
//! one. There, pdf.js's text layer sits over every page and `paintSelection`
//! spends a hundred lines undoing the damage it does — spans with no weight,
//! no style and a generic family, stretched to the printer's width, so bold
//! type comes back regular and mathematics comes back as boxes.
//!
//! There is no text layer here, so there is nothing to hide. pdfium answers
//! per character — [`crate::render::PageText`] is characters and their boxes,
//! indexed together — so a selection is two indices, what it covers is a range
//! of characters, and what it looks like is
//! [`crate::render::PageText::quads`]. The glyphs stay the ones pdfium drew,
//! painted through the theme's own selection colours by the region shader —
//! see `gpu.rs`, and `Page::selected` in `app.rs`.
//!
//! What that costs is what a text layer buys: no keyboard selection, no idea
//! what a word is until [`words_around`] guesses, and nothing about
//! right-to-left or vertical text — a selection is a range of *indices in the
//! document's own order*, which is what pdfium reports and usually but not
//! always the order a reader would sweep. This is the first place in the port
//! where the webview was doing something worth having.

use crate::render::{PageText, Rect};

/// One end of a selection: a page, and a caret in its text.
///
/// The caret is *between* characters, so it runs 0..=len rather than 0..len —
/// which is the difference between "the pointer is on the l of 'olive'" and
/// "the pointer is on the near side of it", and the whole of what makes a
/// sweep left-to-right and one right-to-left cover the same words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Spot {
    /// One-based, as every page number in this crate is.
    pub page: usize,
    /// Into the page's own characters — which is also into its boxes.
    pub index: usize,
}

/// Where a sweep began and where it has got to.
///
/// Anchor and head rather than start and end, because which is which is what
/// the reader is doing and not what they have done: dragging back past the
/// anchor is an ordinary thing to do and it must not turn the selection inside
/// out. Everything that *reads* a selection asks for [`Selection::span`],
/// which is the ordered pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Spot,
    pub head: Spot,
}

impl Selection {
    /// A sweep that has begun and covers nothing yet.
    pub fn at(spot: Spot) -> Selection {
        Selection {
            anchor: spot,
            head: spot,
        }
    }

    /// The two ends in reading order.
    pub fn span(&self) -> (Spot, Spot) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    /// Nothing is covered, which is what a click is: pressing the pointer down
    /// makes a selection and letting it go without moving leaves this. The
    /// caller drops it rather than painting a selection of no width.
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// The pages this selection touches, in order.
    pub fn pages(&self) -> std::ops::RangeInclusive<usize> {
        let (from, to) = self.span();
        from.page..=to.page
    }

    /// What is covered on one page, as a range of characters — `None` for a
    /// page the selection does not reach.
    ///
    /// `len` is that page's own character count, because the middle pages of a
    /// multi-page sweep are covered *entirely* and this is the only thing that
    /// knows how much that is.
    pub fn range_on(&self, page: usize, len: usize) -> Option<(usize, usize)> {
        let (from, to) = self.span();
        if page < from.page || page > to.page {
            return None;
        }
        let start = if page == from.page { from.index } else { 0 };
        let end = if page == to.page { to.index } else { len };
        let (start, end) = (start.min(len), end.min(len));
        if start >= end {
            return None;
        }
        Some((start, end))
    }
}

/// Where a caret goes for a point on a page, in the page's own points.
///
/// The rule is a browser's and is the one nobody notices when it is right:
/// find the line the point is nearest to, then the character on that line it
/// is nearest to, then put the caret on whichever side of that character the
/// point actually fell. A click past the end of a line lands after its last
/// character and not at the start of the next one, and a click below the last
/// line of the page lands at the end of the page.
///
/// Characters with no box are skipped as *targets* and still counted in the
/// index, because they are the spaces and line breaks pdfium generated rather
/// than the printer drew — see [`crate::render::PageSource::text_of`]. A caret
/// that could land on one would be a caret in a place the reader cannot see.
pub fn caret_at(text: &PageText, x: f64, y: f64) -> usize {
    let mut nearest: Option<(f64, usize, Rect)> = None;
    for (index, glyph) in text.boxes.iter().enumerate() {
        if glyph.width <= 0.0 || glyph.height <= 0.0 {
            continue;
        }
        // Vertical first and by a long way: a point level with a line belongs
        // to that line however far along it is, which is what makes a sweep
        // that leaves the right edge of the page carry on to the end of the
        // line rather than jumping to whatever is directly below.
        let dy = gap(y, glyph.top, glyph.height);
        let dx = gap(x, glyph.left, glyph.width);
        let distance = dy * 1000.0 + dx;
        if nearest.is_none_or(|(best, _, _)| distance < best) {
            nearest = Some((distance, index, *glyph));
        }
    }
    let Some((_, index, glyph)) = nearest else {
        return 0;
    };
    if x > glyph.left + glyph.width / 2.0 {
        index + 1
    } else {
        index
    }
}

/// How far a value is outside a span, and zero when it is inside it.
fn gap(value: f64, start: f64, length: f64) -> f64 {
    if value < start {
        start - value
    } else if value > start + length {
        value - start - length
    } else {
        0.0
    }
}

/// The word the caret is in, as a range — or the caret twice over, when it is
/// not in one.
///
/// This is what a double click means, and the definition of "word" is a
/// browser's near enough: a run of letters and digits, with an apostrophe
/// allowed inside it so that "don't" is one word. Punctuation is not part of
/// the word beside it — the comma, the full stop, the em dash — and a double
/// click on a punctuation mark takes that mark alone. Languages that do not
/// put spaces between words are still one run, because matching the Unicode
/// word-break tables would be a lot to carry for a gesture that is a
/// convenience.
pub fn words_around(text: &PageText, caret: usize) -> (usize, usize) {
    let len = text.chars.len();
    if len == 0 {
        return (0, 0);
    }
    let chars = &text.chars;
    let wordy = |at: usize| {
        chars[at].is_alphanumeric()
            || (matches!(chars[at], '\'' | '\u{2019}')
                && at > 0
                && at + 1 < len
                && chars[at - 1].is_alphanumeric()
                && chars[at + 1].is_alphanumeric())
    };
    // The caret sits between characters, so the one it is "in" is the one
    // before it when the one after is not a word — a click at the end of a
    // word means that word rather than the comma or the gap after it.
    let at = caret.min(len - 1);
    let at = if at > 0
        && !wordy(at)
        && (wordy(at - 1) || chars[at].is_whitespace() && !chars[at - 1].is_whitespace())
    {
        at - 1
    } else {
        at
    };
    if chars[at].is_whitespace() {
        return (caret, caret);
    }
    if !wordy(at) {
        return (at, at + 1);
    }
    let mut from = at;
    while from > 0 && wordy(from - 1) {
        from -= 1;
    }
    let mut to = at + 1;
    while to < len && wordy(to) {
        to += 1;
    }
    (from, to)
}

/// What a sweep takes hold of at a time: one character, one word, or one
/// line — which is a first, a second and a third click on the same spot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Char,
    Word,
    Line,
}

/// The unit the caret is in, as a range. See [`words_around`] and
/// [`line_around`]; a character is the caret twice over, so a plain sweep
/// goes through the same door as the others.
pub fn unit_around(text: &PageText, caret: usize, unit: Unit) -> (usize, usize) {
    match unit {
        Unit::Char => (caret, caret),
        Unit::Word => words_around(text, caret),
        Unit::Line => line_around(text, caret),
    }
}

/// The line the caret is on, as a range — what a triple click means.
///
/// A line is what pdfium says it is: the run between the `\r\n` it puts at
/// the end of each one. The break itself is left out, so a copied line does
/// not end in a newline.
pub fn line_around(text: &PageText, caret: usize) -> (usize, usize) {
    let len = text.chars.len();
    if len == 0 {
        return (0, 0);
    }
    let is_break = |c: char| c == '\r' || c == '\n';
    let mut at = caret.min(len - 1);
    // A caret at the end of a line sits on its break; it means that line.
    while at > 0 && is_break(text.chars[at]) {
        at -= 1;
    }
    if is_break(text.chars[at]) {
        return (caret, caret);
    }
    let mut from = at;
    while from > 0 && !is_break(text.chars[from - 1]) {
        from -= 1;
    }
    let mut to = at + 1;
    while to < len && !is_break(text.chars[to]) {
        to += 1;
    }
    (from, to)
}

/// A range of a page's characters, as the reader would paste it.
///
/// Two things are done to it and no more. The line endings pdfium reports are
/// `\r\n`, which is what a PDF's own text operators leave behind rather than
/// anything about the machine reading it, so they become `\n`. And the result
/// is trimmed, because a sweep that overshoots the end of a paragraph picks up
/// the break after it and nobody means to paste that.
///
/// What is deliberately *not* done is joining hyphenated words across a line.
/// [`crate::search::fold`] drops a soft hyphen because a reader typing a word
/// did not type the printer's line break; a reader *copying* a passage is
/// taking the document's own words, and silently editing them is a different
/// thing from finding them. The app does not do it either — it copies what the
/// DOM selection says, which is the printed text.
pub fn quote(text: &PageText, from: usize, to: usize) -> String {
    let to = to.min(text.chars.len());
    if from >= to {
        return String::new();
    }
    let mut out = String::with_capacity(to - from);
    let mut skip = false;
    for (at, character) in text.chars[from..to].iter().enumerate() {
        if skip {
            skip = false;
            continue;
        }
        if *character == '\r' {
            out.push('\n');
            // …and eat the `\n` that follows it, rather than leaving a blank
            // line between every two lines of the paragraph.
            skip = text.chars.get(from + at + 1) == Some(&'\n');
            continue;
        }
        // pdfium's stand-in for a hyphen that ends a line; a control
        // character is nothing to hand the clipboard.
        out.push(match *character {
            '\u{2}' | '\u{fffe}' => '-',
            other => other,
        });
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page of one line: five characters, each ten wide, on a line twenty
    /// tall starting at y=100.
    fn line() -> PageText {
        let chars: Vec<char> = "olive".chars().collect();
        let boxes = (0..chars.len())
            .map(|at| Rect {
                left: 10.0 * at as f64,
                top: 100.0,
                width: 10.0,
                height: 20.0,
            })
            .collect();
        PageText { chars, boxes }
    }

    #[test]
    fn a_caret_lands_on_the_side_the_pointer_fell() {
        let text = line();
        assert_eq!(caret_at(&text, 1.0, 110.0), 0);
        assert_eq!(caret_at(&text, 9.0, 110.0), 1);
        assert_eq!(caret_at(&text, 21.0, 110.0), 2);
    }

    #[test]
    fn past_the_end_of_a_line_is_the_end_of_it() {
        let text = line();
        assert_eq!(caret_at(&text, 900.0, 110.0), 5);
        assert_eq!(caret_at(&text, -900.0, 110.0), 0);
    }

    #[test]
    fn below_everything_is_the_last_line() {
        let text = line();
        assert_eq!(caret_at(&text, 900.0, 9000.0), 5);
    }

    #[test]
    fn a_line_is_chosen_before_a_column() {
        // Two lines, the second below the first and further left. A point far
        // to the right of the first line belongs to the first line, not to
        // the second one whose characters are horizontally nearer.
        let mut text = line();
        for at in 0..3 {
            text.chars.push('x');
            text.boxes.push(Rect {
                left: 10.0 * at as f64,
                top: 140.0,
                width: 10.0,
                height: 20.0,
            });
        }
        assert_eq!(caret_at(&text, 400.0, 110.0), 5);
    }

    #[test]
    fn a_character_pdfium_generated_is_never_a_target() {
        let mut text = line();
        // The line break after "olive", which the printer never drew.
        text.chars.push('\r');
        text.boxes.push(Rect {
            left: 0.0,
            top: 0.0,
            width: 0.0,
            height: 0.0,
        });
        // A point at the very top left, which is exactly where the empty box
        // is. It goes to the line that is actually on the page.
        assert_eq!(caret_at(&text, 0.0, 0.0), 0);
    }

    #[test]
    fn a_sweep_backwards_covers_the_same_words() {
        let forwards = Selection {
            anchor: Spot { page: 1, index: 2 },
            head: Spot { page: 1, index: 7 },
        };
        let backwards = Selection {
            anchor: Spot { page: 1, index: 7 },
            head: Spot { page: 1, index: 2 },
        };
        assert_eq!(forwards.span(), backwards.span());
        assert_eq!(forwards.range_on(1, 20), Some((2, 7)));
        assert_eq!(backwards.range_on(1, 20), Some((2, 7)));
    }

    #[test]
    fn a_middle_page_is_covered_entirely() {
        let sweep = Selection {
            anchor: Spot { page: 2, index: 40 },
            head: Spot { page: 5, index: 3 },
        };
        assert_eq!(sweep.range_on(1, 100), None);
        assert_eq!(sweep.range_on(2, 100), Some((40, 100)));
        assert_eq!(sweep.range_on(3, 100), Some((0, 100)));
        assert_eq!(sweep.range_on(5, 100), Some((0, 3)));
        assert_eq!(sweep.range_on(6, 100), None);
        assert_eq!(sweep.pages().collect::<Vec<_>>(), vec![2, 3, 4, 5]);
    }

    #[test]
    fn a_page_the_sweep_only_grazes_covers_nothing() {
        // The head landed at the very start of page 5, so page 5 has nothing
        // on it — and a range of no width is `None` rather than an empty
        // rectangle to paint.
        let sweep = Selection {
            anchor: Spot { page: 4, index: 0 },
            head: Spot { page: 5, index: 0 },
        };
        assert_eq!(sweep.range_on(5, 100), None);
        assert_eq!(sweep.range_on(4, 100), Some((0, 100)));
    }

    #[test]
    fn a_word_stops_at_punctuation() {
        let chars: Vec<char> = "end, don\u{2019}t stop\u{2014}now.".chars().collect();
        let boxes = vec![
            Rect {
                left: 0.0,
                top: 0.0,
                width: 1.0,
                height: 1.0
            };
            chars.len()
        ];
        let text = PageText { chars, boxes };
        // "end" with the caret before the comma, and on the comma's near side.
        assert_eq!(words_around(&text, 3), (0, 3));
        assert_eq!(words_around(&text, 1), (0, 3));
        // The comma itself, from its far side.
        assert_eq!(words_around(&text, 4), (3, 4));
        // The apostrophe stays inside the word.
        assert_eq!(words_around(&text, 7), (5, 10));
        // The em dash divides "stop" from "now", and the full stop is left out.
        assert_eq!(words_around(&text, 13), (11, 15));
        assert_eq!(words_around(&text, 18), (16, 19));
    }

    #[test]
    fn a_word_is_what_is_around_the_caret() {
        let chars: Vec<char> = "one two three".chars().collect();
        let boxes = vec![
            Rect {
                left: 0.0,
                top: 0.0,
                width: 1.0,
                height: 1.0
            };
            chars.len()
        ];
        let text = PageText { chars, boxes };
        assert_eq!(words_around(&text, 5), (4, 7));
        // The caret at the end of a word means that word, not the space.
        assert_eq!(words_around(&text, 7), (4, 7));
        // …and in the middle of a run of spaces, nothing.
        assert_eq!(words_around(&text, 3), (0, 3));
    }

    #[test]
    fn a_quote_is_the_printed_words_with_the_line_endings_mended() {
        let chars: Vec<char> = "one\r\ntwo\r\n".chars().collect();
        let boxes = vec![
            Rect {
                left: 0.0,
                top: 0.0,
                width: 1.0,
                height: 1.0
            };
            chars.len()
        ];
        let text = PageText { chars, boxes };
        assert_eq!(quote(&text, 0, 10), "one\ntwo");
        assert_eq!(quote(&text, 0, 0), "");
        // Past the end is the end, rather than a panic.
        assert_eq!(quote(&text, 0, 900), "one\ntwo");
    }
}
