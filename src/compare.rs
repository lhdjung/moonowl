//! Two versions of a document compared word by word: what the document has
//! that the other file does not, and what the other file had that the
//! document no longer has. Nothing is written to either file.
//!
//! The comparison is about the text, not about how it is set, so what a
//! typesetter changes is not a difference:
//! - Words are compared as the search compares them, after
//!   [`crate::search::fold`], so a ligature or an accent is not a change,
//!   and neither is a capital letter, which a journal's headings change.
//! - A word broken across two lines with a hyphen is one word, since the
//!   same word in the other version may sit in the middle of a line. Where
//!   the hyphen could be the word's own, as in "cost-efficient" broken
//!   after "cost-", the word is the same with or without it. Elsewhere a
//!   hyphen added or taken out is a change.
//! - Punctuation is compared apart from the words, as what comes between
//!   two of them, so that a comma added is marked by itself and "p.73" is
//!   the same as "p. 73". The shapes of quotation marks, apostrophes and
//!   dashes are not compared.
//! - Running heads and page numbers are not compared: a page added before
//!   them would otherwise make every one after it a change. They are found
//!   as the split at each chapter finds them (`arrange::furniture`). Line
//!   numbers in the margin of a manuscript are left out too.
//! - A passage that one file has in one place and the other in another is
//!   found even where words in it changed. Where it is on pages that the
//!   rest of the comparison puts side by side, it is in the same place on
//!   the page, and only the order the file keeps its text in differs, as it
//!   does for an abstract in a journal's layout. Then only its changed words
//!   are changes. Elsewhere it is a passage that moved: what is the same in
//!   it is two changes that name each other, and what differs is a change
//!   like any other.
//!
//! The hyphen pdfium reports at the end of a line, and the soft hyphen, are
//! left out of the words shown. Where one word stands in place of another,
//! only the letters that differ are marked on the page.

use std::ops::Range;

use crate::render::{PageSource, PageText, Rect};

/// How long the comparison may take before it settles for a coarser answer,
/// in which a long stretch of changes is one change. Finding the passages
/// that moved has as long again.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(5);

/// How many characters of a change's words are kept, about four pages.
/// After the comparison gives up being exact, one change can be a whole
/// chapter, which is better read on the pages of the file that has it.
const SHOWN: usize = 10_000;

/// How many words a passage needs to be taken for one that moved, so that
/// a "the" taken out here and put in there is not.
const MOVED_AT_LEAST: usize = 3;

/// How many words a passage needs to be looked for elsewhere when the other
/// file has other words in its place. A shorter one is a word or two put in
/// place of others.
const PASSAGE: usize = 12;

/// The most words two long changes may have between them and still be one
/// change. A diff matches a word as common as "in" or "the" in two passages
/// that have nothing else in common, and that would cut the change into
/// pieces.
const BETWEEN: usize = 4;

/// The hyphens, which a word keeps inside it, and the apostrophes.
const HYPHENS: [char; 3] = ['-', '\u{2010}', '\u{2011}'];
const APOSTROPHES: [char; 4] = ['\'', '\u{2019}', '\u{2018}', '\u{02bc}'];

/// One word of a document, with the punctuation after it: what is
/// compared, what is shown, and where it is. The first three are numbers
/// in [`Vocabulary`].
struct Word {
    /// Its letters.
    key: u32,
    /// The word as it is written, with its punctuation.
    shown: u32,
    /// The punctuation after it, and before it if it begins its page.
    marks: u32,
    /// One-based.
    page: u32,
    from: u32,
    to: u32,
    /// Whether white space comes before it, or it begins its page, so that
    /// the words can be said as they were written.
    spaced: bool,
    /// Whether it was broken at the end of a line with a hyphen, which may
    /// or may not have been the word's own.
    joined: bool,
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
    /// page they are on. Where one word stands in place of another, the
    /// range holds only the letters that differ. Where the document has no
    /// words or letters of the change, there is one place, with `from` and
    /// `to` the same, where the other file's would be.
    pub places: Vec<(usize, usize, usize)>,
    /// The same in the other file.
    pub places_there: Vec<(usize, usize, usize)>,
    /// The words the document has here that the other file does not, cut
    /// short after `SHOWN` characters. Where only the punctuation differs,
    /// the word it is written against.
    pub here: String,
    /// The words the other file has here that the document does not, said
    /// in the same way.
    pub there: String,
    /// How many words the document has here that the other file does not.
    /// A word whose punctuation differs is not counted.
    pub count: usize,
    /// How many words the other file has here that the document does not.
    pub count_there: usize,
    /// The change that holds the same words in the other file, where these
    /// words moved.
    pub moved: Option<usize>,
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

    /// The same change, seen from the other file.
    pub fn swapped(self) -> Change {
        Change {
            places: self.places_there,
            places_there: self.places,
            here: self.there,
            there: self.here,
            count: self.count_there,
            count_there: self.count,
            moved: self.moved,
        }
    }
}

/// Every change from `other` to `document`, in the document's order.
// ponytail: every word of both files is held at once, about 28 bytes a
// word, or 40MB for two versions of a 1,700-page book. Comparing page by
// page would need the pages to line up.
pub fn compare(document: &dyn PageSource, other: &dyn PageSource) -> Result<Vec<Change>, String> {
    use similar::DiffOp;
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;
    let mut vocabulary = Vocabulary::default();
    let ours = words_of(document, &mut vocabulary)?;
    let theirs = words_of(other, &mut vocabulary)?;
    let (our_keys, their_keys) = (keys(&ours), keys(&theirs));
    let diff = |old: &[u32], new: &[u32], deadline: Instant| {
        similar::capture_diff_slices_deadline(
            similar::Algorithm::Patience,
            old,
            new,
            Some(deadline),
        )
    };
    let ops = diff(&their_keys, &our_keys, Instant::now() + PATIENCE);
    let stretches = stretches_of(&ops);

    // The pages that the words both files have put side by side.
    let mut beside = HashSet::new();
    for op in &ops {
        if let DiffOp::Equal {
            old_index,
            new_index,
            len,
        } = *op
        {
            for at in 0..len {
                beside.insert((theirs[old_index + at].page, ours[new_index + at].page));
            }
        }
    }

    // Each passage only the other file has is paired with the passage only
    // the document has that shares the most words with it, where they share
    // at least half the words of the longer of the two. What each has before
    // the first word they share and after the last stays where it was.
    let free = |side: &Range<usize>, other: &Range<usize>| {
        side.len() >= MOVED_AT_LEAST && (other.is_empty() || side.len() >= PASSAGE)
    };
    let targets: Vec<usize> = (0..stretches.len())
        .filter(|&at| free(&stretches[at].1, &stretches[at].0))
        .collect();
    // What each stretch gave to a pair, of the other file's words and of
    // the document's.
    let mut cut_old: Vec<Option<Range<usize>>> = vec![None; stretches.len()];
    let mut cut_new: Vec<Option<Range<usize>>> = vec![None; stretches.len()];
    let mut paired = Vec::new();
    let deadline = Instant::now() + PATIENCE;
    let (mut had, mut used) = (HashMap::new(), HashMap::new());
    for (source, (old, new)) in stretches.iter().enumerate() {
        if !free(old, new) || Instant::now() > deadline {
            continue;
        }
        let words = &their_keys[old.clone()];
        had.clear();
        for &key in words {
            *had.entry(key).or_insert(0usize) += 1;
        }
        let mut best: Option<(usize, usize, Vec<DiffOp>)> = None;
        for &target in &targets {
            let others = &our_keys[stretches[target].1.clone()];
            let need = (words.len().max(others.len()))
                .div_ceil(2)
                .max(MOVED_AT_LEAST);
            if target == source || cut_new[target].is_some() || words.len().min(others.len()) < need
            {
                continue;
            }
            // The words they have in common in any order, which is as many
            // as they can have in common in the same order, or more.
            used.clear();
            let mut shared = 0;
            for &key in others {
                let count = used.entry(key).or_insert(0usize);
                if *count < had.get(&key).copied().unwrap_or(0) {
                    *count += 1;
                    shared += 1;
                }
            }
            if shared < need || best.as_ref().is_some_and(|(most, ..)| shared <= *most) {
                continue;
            }
            let ops = diff(words, others, deadline);
            let shared = equal(&ops);
            if shared >= need && best.as_ref().is_none_or(|(most, ..)| shared > *most) {
                best = Some((shared, target, ops));
            }
        }
        let Some((_, target, ops)) = best else {
            continue;
        };
        let is_equal = |op: &DiffOp| matches!(op, DiffOp::Equal { .. });
        let (first, last) = (
            ops.iter().position(is_equal).expect("shared words"),
            ops.iter().rposition(is_equal).expect("shared words"),
        );
        let ops = ops[first..=last].to_vec();
        let (begin, end) = (ops[0].old_range().start, ops[ops.len() - 1].old_range().end);
        cut_old[source] = Some(old.start + begin..old.start + end);
        let new = &stretches[target].1;
        let (begin, end) = (ops[0].new_range().start, ops[ops.len() - 1].new_range().end);
        cut_new[target] = Some(new.start + begin..new.start + end);
        paired.push((source, target, ops));
    }

    let sides = Sides {
        ours: &ours,
        theirs: &theirs,
        vocabulary: &vocabulary,
    };
    let mut changes = Vec::new();
    for (at, (old, new)) in stretches.iter().enumerate() {
        // What is left of each side once a pair has taken its part: before
        // that part, and after it. Each piece left of one side goes with
        // the piece left of the other in the same place.
        let left = |range: &Range<usize>, cut: &Option<Range<usize>>| match cut {
            None => [range.clone(), range.end..range.end],
            Some(cut) => [range.start..cut.start, cut.end..range.end],
        };
        let (olds, news) = (left(old, &cut_old[at]), left(new, &cut_new[at]));
        for (old, new) in olds.into_iter().zip(news) {
            if !old.is_empty() || !new.is_empty() {
                changes.push(sides.change(old, new));
            }
        }
    }
    let marked = |old: usize, new: usize, len: usize, changes: &mut Vec<Change>| {
        changes.extend((0..len).filter_map(|at| sides.marked_apart(old + at, new + at)));
    };
    for op in &ops {
        if let DiffOp::Equal {
            old_index,
            new_index,
            len,
        } = *op
        {
            marked(old_index, new_index, len, &mut changes);
        }
    }
    for (source, target, inner) in paired {
        let (old, new) = (
            cut_old[source].clone().expect("paired"),
            cut_new[target].clone().expect("paired"),
        );
        let (old_start, new_start) = (stretches[source].0.start, stretches[target].1.start);
        let in_place = old.len().min(new.len()) >= PASSAGE
            && beside.contains(&(theirs[old.start].page, ours[new.start].page));
        let shifted = |range: Range<usize>, by: usize| range.start + by..range.end + by;
        for (o, n) in stretches_of(&inner) {
            changes.push(sides.change(shifted(o, old_start), shifted(n, new_start)));
        }
        for op in &inner {
            let DiffOp::Equal {
                old_index,
                new_index,
                len,
            } = *op
            else {
                continue;
            };
            let (o, n) = (old_start + old_index, new_start + new_index);
            if in_place {
                marked(o, n, len, &mut changes);
            } else if len >= MOVED_AT_LEAST {
                // Each is said where the other file had the passage.
                let (gone, came) = (stretches[source].1.start, stretches[target].0.start);
                let number = changes.len();
                let mut left = sides.change(o..o + len, gone..gone);
                let mut arrived = sides.change(came..came, n..n + len);
                (left.moved, arrived.moved) = (Some(number + 1), Some(number));
                changes.push(left);
                changes.push(arrived);
            }
        }
    }

    // In the document's order, with each change that moved still naming
    // the other.
    let begins = |change: &Change| {
        let (here, there) = (change.places[0], change.places_there[0]);
        (here.0, here.1, there.0, there.1)
    };
    let mut order: Vec<usize> = (0..changes.len()).collect();
    order.sort_by_key(|&at| begins(&changes[at]));
    let mut now_at = vec![0; order.len()];
    for (new, &old) in order.iter().enumerate() {
        now_at[old] = new;
    }
    let mut changes: Vec<Option<Change>> = changes.into_iter().map(Some).collect();
    Ok(order
        .into_iter()
        .map(|at| {
            let mut change = changes[at].take().expect("each once");
            change.moved = change.moved.map(|other| now_at[other]);
            change
        })
        .collect())
}

/// How many words two runs of words have in common, in order.
fn equal(ops: &[similar::DiffOp]) -> usize {
    (ops.iter())
        .map(|op| match *op {
            similar::DiffOp::Equal { len, .. } => len,
            _ => 0,
        })
        .sum()
}

/// Where two runs of words differ, each a range of the old run and a range
/// of the new. Changes next to each other are one. So are two changes with
/// a few words between them, no more than [`BETWEEN`], where each change is
/// as long as those words and one of them is three times as long.
fn stretches_of(ops: &[similar::DiffOp]) -> Vec<(Range<usize>, Range<usize>)> {
    let mut out: Vec<(Range<usize>, Range<usize>)> = Vec::new();
    for op in ops {
        if matches!(op, similar::DiffOp::Equal { .. }) {
            continue;
        }
        out.push((op.old_range(), op.new_range()));
        while let [.., before, after] = out.as_slice() {
            let between = after.0.start - before.0.end;
            let size = |(old, new): &(Range<usize>, Range<usize>)| old.len().max(new.len());
            let (small, large) = (size(before).min(size(after)), size(before).max(size(after)));
            if between > 0 && (between > BETWEEN || between > small || 3 * between > large) {
                break;
            }
            let (old, new) = out.pop().expect("two");
            let last = out.last_mut().expect("two");
            (last.0.end, last.1.end) = (old.end, new.end);
        }
    }
    out
}

/// The words of both files, for making changes out of ranges of them.
struct Sides<'a> {
    ours: &'a [Word],
    theirs: &'a [Word],
    vocabulary: &'a Vocabulary,
}

impl Sides<'_> {
    /// The change from the words `old` of the other file to the words `new`
    /// of the document. An empty range is the place between two words.
    fn change(&self, old: Range<usize>, new: Range<usize>) -> Change {
        let (mut places_here, mut places_there) = (
            places(self.ours, new.clone()),
            places(self.theirs, old.clone()),
        );
        if let ([one], [other]) = (&self.ours[new.clone()], &self.theirs[old.clone()]) {
            let (word, word_there) = (self.chars(one), self.chars(other));
            let shared = shared_ends(&word, &word_there);
            letters_only(&mut places_here, one, word.len(), shared);
            letters_only(&mut places_there, other, word_there.len(), shared);
        }
        Change {
            places: places_here,
            places_there,
            here: self.said(&self.ours[new.clone()]),
            there: self.said(&self.theirs[old.clone()]),
            count: new.len(),
            count_there: old.len(),
            moved: None,
        }
    }

    fn chars(&self, word: &Word) -> Vec<char> {
        self.vocabulary.words[word.shown as usize].chars().collect()
    }

    /// The words as they were written, cut short after `SHOWN` characters.
    fn said(&self, words: &[Word]) -> String {
        let (mut said, mut length) = (String::new(), 0);
        for word in words {
            if word.spaced && !said.is_empty() {
                said.push(' ');
                length += 1;
            }
            let shown = &self.vocabulary.words[word.shown as usize];
            length += shown.chars().count();
            said.push_str(shown);
            if length > SHOWN {
                let (cut, _) = (said.char_indices().nth(SHOWN)).expect("longer");
                said.truncate(cut);
                said.push('\u{2026}');
                break;
            }
        }
        said
    }

    /// The change where the word `old` of the other file and the word `new`
    /// of the document, which have the same letters, differ in their
    /// punctuation or their hyphens. A hyphen is not compared where one of
    /// them was joined at the end of a line, since the hyphen may then have
    /// been the typesetter's.
    fn marked_apart(&self, old: usize, new: usize) -> Option<Change> {
        let (one, other) = (&self.theirs[old], &self.ours[new]);
        if one.shown == other.shown {
            return None;
        }
        let hyphens = |word: &Word| -> String {
            let chars = self.chars(word);
            let folded: String = crate::search::fold(&chars, false)
                .text
                .into_iter()
                .collect();
            (folded.trim_matches(|c: char| !c.is_alphanumeric()).chars())
                .filter(|c| c.is_alphanumeric() || HYPHENS.contains(c))
                .map(|c| if HYPHENS.contains(&c) { '-' } else { c })
                .collect()
        };
        let hyphened = !one.joined && !other.joined && hyphens(one) != hyphens(other);
        // Punctuation before a word that only one file has belongs to the
        // change that word is.
        let next = (self.theirs.get(old + 1)).map(|word| word.key)
            == (self.ours.get(new + 1)).map(|word| word.key);
        if !hyphened && (one.marks == other.marks || !next) {
            return None;
        }
        // Punctuation that differs and is shown with the next word, as an
        // opening bracket is, is said and marked with that word too.
        let led = |word: &Word| {
            (self.vocabulary.words[word.shown as usize].chars().next()).is_some_and(|c| !in_word(c))
        };
        let wide = one.marks != other.marks
            && next
            && (self.theirs.get(old + 1).zip(self.ours.get(new + 1)))
                .is_some_and(|(one, other)| led(one) || led(other));
        let mut change = match wide {
            true => self.change(old..old + 2, new..new + 2),
            false => self.change(old..old + 1, new..new + 1),
        };
        if !hyphened {
            (change.count, change.count_there) = (0, 0);
        }
        Some(change)
    }
}

/// How many letters two words share at their start, and how many after
/// that at their end, whatever their case.
fn shared_ends(one: &[char], other: &[char]) -> (usize, usize) {
    let same = |a: &char, b: &char| a.to_lowercase().eq(b.to_lowercase());
    let start = one
        .iter()
        .zip(other)
        .take_while(|(a, b)| same(a, b))
        .count();
    let end = (one[start..].iter().rev())
        .zip(other[start..].iter().rev())
        .take_while(|(a, b)| same(a, b))
        .count();
    (start, end)
}

/// Where one word stands in place of another, its place narrowed to the
/// letters that differ, leaving out the `shared` letters at its start and
/// end. A word whose letters are not one run on its page, such as a word
/// broken across two lines, keeps its whole place.
fn letters_only(
    places: &mut [(usize, usize, usize)],
    word: &Word,
    letters: usize,
    (start, end): (usize, usize),
) {
    if let [(_, from, to)] = places {
        if (word.to - word.from) as usize == letters {
            (*from, *to) = (*from + start, *to - end);
        }
    }
}

/// What is compared of each word.
fn keys(words: &[Word]) -> Vec<u32> {
    words.iter().map(|word| word.key).collect()
}

/// The words `range` of `all` as ranges of characters, one range per page
/// they are on. No words are a place between two: before the word that
/// follows, after the last word of the document, or at the top of the first
/// page if the document has no words at all.
fn places(all: &[Word], range: Range<usize>) -> Vec<(usize, usize, usize)> {
    if range.is_empty() {
        return match (all.get(range.start), all.last()) {
            (Some(next), _) => vec![(next.page as usize, next.from as usize, next.from as usize)],
            (None, Some(last)) => vec![(last.page as usize, last.to as usize, last.to as usize)],
            (None, None) => vec![(1, 0, 0)],
        };
    }
    let mut out: Vec<(usize, usize, usize)> = Vec::new();
    for word in &all[range] {
        match out.last_mut() {
            Some((page, _, to)) if *page == word.page as usize => *to = word.to as usize,
            _ => out.push((word.page as usize, word.from as usize, word.to as usize)),
        }
    }
    out
}

/// Every word of a document, in the order its pages have them, without
/// its running heads, page numbers and line numbers.
fn words_of(document: &dyn PageSource, vocabulary: &mut Vocabulary) -> Result<Vec<Word>, String> {
    let mut words = Vec::new();
    let count = document.pages();
    // The pages from two before the page to two after it, each with its
    // place, since a running head is found by the pages around it.
    let mut held: std::collections::VecDeque<(usize, PageText)> = Default::default();
    for index in 0..count {
        while held.back().map_or(0, |(at, _)| at + 1) < (index + 3).min(count) {
            if crate::export::GIVE_UP.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(String::new());
            }
            let next = held.back().map_or(0, |(at, _)| at + 1);
            let text = (document.text_aside(next)).ok_or(crate::export::CLOSED_WHILE_READ)?;
            held.push_back((next, text));
        }
        while held.front().is_some_and(|(at, _)| at + 2 < index) {
            held.pop_front();
        }
        let text = &held
            .iter()
            .find(|(at, _)| *at == index)
            .expect("read above")
            .1;
        let around: Vec<&PageText> = (held.iter())
            .filter(|(at, _)| *at != index)
            .map(|(_, text)| text)
            .collect();
        let height = document.size_of(index).height;
        let mut skip = crate::arrange::furniture(text, &around, height);
        line_numbers(text, &mut skip);
        words_on(text, &skip, index + 1, vocabulary, &mut words);
    }
    Ok(words)
}

/// How many numbers a page needs outside its text, on one side, to be taken
/// for line numbers.
const LINE_NUMBERS: usize = 5;

/// Line numbers, as a manuscript has them in its margin, marked in `skip`:
/// numbers standing on their own to the left of all the page's other text,
/// or to the right of it, where there are at least [`LINE_NUMBERS`] of them
/// on that side.
fn line_numbers(text: &PageText, skip: &mut [bool]) {
    let chars = &text.chars;
    let blank = |at: usize| chars[at].is_whitespace() || skip.get(at) == Some(&true);
    // Each run of characters between white space, with how far left and
    // right it reaches, and whether it is a number.
    let mut runs = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if blank(at) {
            at += 1;
            continue;
        }
        let from = at;
        let (mut left, mut right) = (f32::MAX, f32::MIN);
        while at < chars.len() && !blank(at) {
            if let Some(cell) = text.boxes.get(at).filter(|cell| cell.width > 0.0) {
                (left, right) = (left.min(cell.left), right.max(cell.left + cell.width));
            }
            at += 1;
        }
        if left <= right {
            let number = chars[from..at].iter().all(char::is_ascii_digit);
            runs.push((from..at, left, right, number));
        }
    }
    let words = runs.iter().filter(|(.., number)| !number);
    let left = (words.clone())
        .map(|(_, left, ..)| *left)
        .fold(f32::MAX, f32::min);
    let right = words
        .map(|(_, _, right, _)| *right)
        .fold(f32::MIN, f32::max);
    if left > right {
        return;
    }
    let outside: [&dyn Fn(f32, f32) -> bool; 2] = [&|_, end| end < left, &|start, _| start > right];
    for outside in outside {
        let found: Vec<&Range<usize>> = (runs.iter())
            .filter(|(_, start, end, number)| *number && outside(*start, *end))
            .map(|(range, ..)| range)
            .collect();
        if found.len() >= LINE_NUMBERS {
            for range in found {
                skip[range.clone()].fill(true);
            }
        }
    }
}

/// Whether a character belongs to a word rather than being punctuation of
/// its own.
fn in_word(character: char) -> bool {
    character.is_alphanumeric()
        || HYPHENS.contains(&character)
        || APOSTROPHES.contains(&character)
        || crate::search::ignored(character)
        || unicode_normalization::char::is_combining_mark(character)
}

/// What is compared of a word or of punctuation: folded as the search folds
/// it, without hyphens or dashes, and with every shape of apostrophe and of
/// quotation mark one shape. A dash on its own, of any length, is "-".
fn key_of(chars: &[char]) -> String {
    let mut key: String = (crate::search::fold(chars, false).text.into_iter())
        .filter(|&c| !HYPHENS.contains(&c) && !matches!(c, '\u{2012}'..='\u{2015}'))
        .map(|c| match c {
            c if APOSTROPHES.contains(&c) => '\'',
            '\u{201a}' | '\u{201b}' => '\'',
            '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{201f}' | '\u{ab}' | '\u{bb}' => '"',
            c => c,
        })
        .collect();
    if key.is_empty() {
        key.push('-');
    }
    key
}

/// The words on one page, leaving out the characters `skip` says. What is
/// compared of the punctuation is all of it between a word and the next,
/// whatever white space there is, and it goes with the word before; on the
/// first word of the page, so does any before it. What is shown of it goes
/// with the word it is written against: the word before where nothing comes
/// between, and otherwise the word after. A word that ends in a hyphen at
/// the end of a line is joined to the word that begins the next one.
fn words_on(
    text: &PageText,
    skip: &[bool],
    page: usize,
    vocabulary: &mut Vocabulary,
    out: &mut Vec<Word>,
) {
    let chars = &text.chars;
    let blank = |at: usize| chars[at].is_whitespace() || skip.get(at) == Some(&true);
    let first = out.len();
    // Each word's punctuation on this page, compared: before it, which only
    // the first word has, and after it.
    let mut marks: Vec<(String, String)> = Vec::new();
    let mut leading = String::new();
    // Punctuation shown with the word after it: where it begins, and how it
    // is written.
    let mut waiting: Option<(usize, usize, String)> = None;
    let mut at = 0;
    let (mut spaced, mut joining) = (true, false);
    while at < chars.len() {
        if blank(at) {
            (at, spaced) = (at + 1, true);
            continue;
        }
        let from = at;
        let wordy = in_word(chars[at]);
        at += 1;
        while wordy && at < chars.len() && !blank(at) && in_word(chars[at]) {
            at += 1;
        }
        let piece: String = (chars[from..at].iter())
            .filter(|&&c| !crate::search::ignored(c))
            .collect();
        if piece.is_empty() {
            continue;
        }
        let key = key_of(&chars[from..at]);
        let gap = if spaced { " " } else { "" };
        let on_page = out.len() > first;
        if !(wordy && piece.chars().any(char::is_alphanumeric)) {
            match marks.last_mut() {
                Some((_, after)) => after.push_str(&key),
                None => leading.push_str(&key),
            }
            match (&mut waiting, on_page && !spaced) {
                (None, true) => {
                    let last = out.last_mut().expect("a word on the page");
                    let shown = format!("{}{piece}", vocabulary.words[last.shown as usize]);
                    (last.shown, last.to) = (vocabulary.number(&shown), at as u32);
                }
                (Some((_, end, shown)), _) => {
                    shown.push_str(gap);
                    shown.push_str(&piece);
                    *end = at;
                }
                (None, false) => waiting = Some((from, at, piece)),
            }
            (joining, spaced) = (false, false);
            continue;
        }
        let last = chars[at - 1];
        let broken = (crate::search::hyphen(last) || HYPHENS.contains(&last) && at - from > 1)
            && chars.get(at).is_some_and(|&c| c == '\r' || c == '\n');
        // pdfium's mark for a hyphen at the end of a line, inside the word.
        let hyphened = chars[from..at].iter().any(|&c| crate::search::hyphen(c));
        match out.last_mut() {
            Some(last) if joining && on_page => {
                let shown = (vocabulary.words[last.shown as usize])
                    .trim_end_matches(HYPHENS)
                    .to_string();
                let joined = format!("{}{key}", vocabulary.words[last.key as usize]);
                last.key = vocabulary.number(&joined);
                last.shown = vocabulary.number(&format!("{shown}{piece}"));
                (last.to, last.joined) = (at as u32, true);
            }
            _ => {
                let (start, shown) = match waiting.take() {
                    Some((start, _, shown)) => (start, format!("{shown}{gap}{piece}")),
                    None => (from, piece),
                };
                let before = if on_page {
                    String::new()
                } else {
                    std::mem::take(&mut leading)
                };
                out.push(Word {
                    key: vocabulary.number(&key),
                    shown: vocabulary.number(&shown),
                    marks: 0,
                    page: page as u32,
                    from: start as u32,
                    to: at as u32,
                    spaced: spaced || start < from,
                    joined: hyphened,
                });
                marks.push((before, String::new()));
            }
        }
        (joining, spaced) = (broken, false);
    }
    // Punctuation with no word after it on the page is shown with the word
    // before it.
    if let (Some((_, end, shown)), Some(last)) = (waiting, out[first..].last_mut()) {
        let shown = format!("{} {shown}", vocabulary.words[last.shown as usize]);
        (last.shown, last.to) = (vocabulary.number(&shown), end as u32);
    }
    for (word, (before, after)) in out[first..].iter_mut().zip(marks) {
        word.marks = vocabulary.number(&format!("{before}\u{0}{after}"));
    }
}

/// A thin bar where words are missing: at the left edge of the character
/// at `at`, or at the right edge of the one before it at the end of a line.
pub fn bar(text: &PageText, at: usize) -> Option<Rect> {
    let drawn = |i: usize| Some(text.glyph(i)).filter(|g| g.width > 0.0 && g.height > 0.0);
    let (left, glyph) = match (at < text.chars.len()).then(|| drawn(at)).flatten() {
        Some(glyph) => (glyph.left, glyph),
        None => {
            let glyph = (0..at.min(text.chars.len())).rev().find_map(drawn)?;
            (glyph.left + glyph.width, glyph)
        }
    };
    Some(Rect {
        left: left - 1.0,
        top: glyph.top,
        width: 2.0,
        height: glyph.height,
    })
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

    fn read(texts: &[&str]) -> (Vocabulary, Vec<Vec<Word>>) {
        let mut vocabulary = Vocabulary::default();
        let words = (texts.iter())
            .map(|text| {
                let mut out = Vec::new();
                words_on(&page(text), &[], 1, &mut vocabulary, &mut out);
                out
            })
            .collect();
        (vocabulary, words)
    }

    /// Each word's letters, and its punctuation.
    fn keys(text: &str) -> Vec<(String, String)> {
        let (vocabulary, words) = read(&[text]);
        let said = |number: u32| vocabulary.words[number as usize].replace('\0', "|");
        (words[0].iter())
            .map(|word| (said(word.key), said(word.marks)))
            .collect()
    }

    fn letters(text: &str) -> Vec<String> {
        keys(text).into_iter().map(|(key, _)| key).collect()
    }

    /// A word is a handful of numbers, which is what makes a long book
    /// affordable. See the note on [`compare`].
    #[test]
    fn a_word_costs_28_bytes() {
        assert_eq!(std::mem::size_of::<Word>(), 28);
    }

    #[test]
    fn a_word_broken_at_a_line_is_one_word() {
        assert_eq!(
            letters("the typo-\r\ngraphy of it"),
            ["the", "typography", "of", "it"]
        );
        assert_eq!(letters("a well-known word"), ["a", "wellknown", "word"]);
        assert_eq!(letters("ends with -\r\nnext"), ["ends", "with", "next"]);
        assert_eq!(letters("re\u{2}\r\ntrieved"), ["retrieved"]);
        let (vocabulary, words) = read(&["Re\u{2}trieved"]);
        assert_eq!(&*vocabulary.words[words[0][0].shown as usize], "Retrieved");
    }

    /// What a typesetter changes is not compared: capitals, the space
    /// around punctuation, and the shapes of quotation marks and dashes.
    #[test]
    fn the_way_text_is_set_is_not_compared() {
        assert_eq!(keys("(p. 73)."), keys("(p.73)."));
        assert_eq!(keys("(> 30,000"), keys("(>30,000"));
        assert_eq!(keys("Rule-of-Thumb Bias"), keys("rule-of-thumb bias"));
        assert_eq!(
            keys("\u{201c}cut\u{201d} \u{2014} it\u{2019}s"),
            keys("\"cut\" -- it's")
        );
        assert_ne!(keys("practice, it"), keys("practice it"));
    }

    /// A hyphen added or taken out is a change, unless it ended a line,
    /// where it may have been the typesetter's.
    #[test]
    fn a_hyphen_is_a_change_unless_a_line_ended_there() {
        let (vocabulary, words) =
            read(&["cost-efficient non-valid", "cost-\r\nefficient nonvalid"]);
        let (theirs, ours) = (&words[0], &words[1]);
        assert_eq!(super::keys(theirs), super::keys(ours));
        let sides = Sides {
            ours,
            theirs,
            vocabulary: &vocabulary,
        };
        assert!(sides.marked_apart(0, 0).is_none());
        let change = sides.marked_apart(1, 1).expect("a change");
        assert_eq!((&*change.there, &*change.here), ("non-valid", "nonvalid"));
        assert_eq!((change.count, change.count_there), (1, 1));
    }

    /// A comma added is marked by itself, said with its word, and is not a
    /// word added.
    #[test]
    fn a_punctuation_mark_is_said_with_its_word() {
        let (vocabulary, words) = read(&["in practice it", "in Practice, it"]);
        let sides = Sides {
            ours: &words[1],
            theirs: &words[0],
            vocabulary: &vocabulary,
        };
        assert!(sides.marked_apart(0, 0).is_none());
        let change = sides.marked_apart(1, 1).expect("a change");
        assert_eq!((&*change.there, &*change.here), ("practice", "Practice,"));
        assert_eq!(change.places, [(1, 11, 12)]);
        assert_eq!((change.count, change.count_there), (0, 0));
    }

    /// A word as common as "in", which a diff matches in two passages that
    /// have nothing else in common, does not cut a long change in two. A
    /// short change keeps the words between it and the next.
    #[test]
    fn a_few_words_between_long_changes_are_part_of_them() {
        use similar::DiffOp::{Equal, Replace};
        let replace = |old_index, old_len, new_index, new_len| Replace {
            old_index,
            old_len,
            new_index,
            new_len,
        };
        let equal = |old_index, new_index, len| Equal {
            old_index,
            new_index,
            len,
        };
        let ops = [
            replace(0, 10, 0, 20),
            equal(10, 20, 1),
            replace(11, 4, 21, 3),
            equal(15, 24, 10),
            replace(25, 1, 34, 1),
            equal(26, 35, 1),
            replace(27, 1, 36, 2),
        ];
        assert_eq!(
            stretches_of(&ops),
            [(0..15, 0..24), (25..26, 34..35), (27..28, 36..38)]
        );
    }

    /// A manuscript's line numbers are left out, and a number inside the
    /// text is not.
    #[test]
    fn line_numbers_in_the_margin_are_left_out() {
        let mut text = PageText::default();
        for line in 1..=6 {
            for (at, word) in [
                (10.0, line.to_string()),
                (60.0, format!("Line {line} here")),
            ] {
                for (n, c) in word.chars().enumerate() {
                    text.chars.push(c);
                    text.boxes.push(Cell {
                        left: at + 5.0 * n as f32,
                        top: 12.0 * line as f32,
                        width: 4.0,
                        height: 10.0,
                    });
                }
                text.chars.push(' ');
                text.boxes.push(Cell::default());
            }
            text.chars.extend(['\r', '\n']);
            text.boxes.extend([Cell::default(), Cell::default()]);
        }
        let mut skip = vec![false; text.chars.len()];
        line_numbers(&text, &mut skip);
        let (mut vocabulary, mut out) = (Vocabulary::default(), Vec::new());
        words_on(&text, &skip, 1, &mut vocabulary, &mut out);
        let said: Vec<&str> = (out.iter().take(4))
            .map(|word| &*vocabulary.words[word.shown as usize])
            .collect();
        assert_eq!(said, ["Line", "1", "here", "Line"]);
    }

    /// One word in place of another is marked only where its letters
    /// differ, and a word broken across two lines is marked whole.
    #[test]
    fn only_the_letters_that_differ_are_marked() {
        let chars = |text: &str| text.chars().collect::<Vec<_>>();
        assert_eq!(shared_ends(&chars("colour"), &chars("color")), (4, 1));
        assert_eq!(shared_ends(&chars("cat"), &chars("dog")), (0, 0));
        assert_eq!(shared_ends(&chars("word"), &chars("Words")), (4, 0));
        let word = |from, to| Word {
            key: 0,
            shown: 0,
            marks: 0,
            page: 1,
            from,
            to,
            spaced: true,
            joined: false,
        };
        let mut places = vec![(1, 10, 16)];
        letters_only(&mut places, &word(10, 16), 6, (4, 1));
        assert_eq!(places, [(1, 14, 15)]);
        let mut places = vec![(1, 10, 14)];
        letters_only(&mut places, &word(10, 14), 4, (4, 0));
        assert_eq!(places, [(1, 14, 14)]);
        let mut places = vec![(1, 10, 19)];
        letters_only(&mut places, &word(10, 19), 6, (4, 1));
        assert_eq!(places, [(1, 10, 19)]);
    }
}
