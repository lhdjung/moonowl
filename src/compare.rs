//! Two versions of a document compared word by word: what the document has
//! that the other file does not, and what the other file had that the
//! document no longer has. Nothing is written to either file.
//!
//! The comparison is about the text, not about how it is set, so what a
//! typesetter changes is not a difference:
//! - Words are compared as the search compares them, after
//!   [`crate::search::fold`], so a ligature or an accent is not a change,
//!   and neither is a capital letter, which a journal's headings change.
//!   For the same reason "ß" is the same as "ss", as capitals set it, and
//!   Hebrew points and Arabic vowel marks are not compared.
//! - A word broken across two lines with a hyphen is one word, since the
//!   same word in the other version may sit in the middle of a line. Where
//!   the hyphen could be the word's own, as in "cost-efficient" broken
//!   after "cost-", the word is the same with or without it. Elsewhere a
//!   hyphen added or taken out is a change.
//! - Punctuation is compared apart from the words, as what comes between
//!   two of them, so that a comma added is marked by itself, and so is a
//!   space after a full stop. A space before punctuation is not compared,
//!   since a picture or a raised mark beside a word leaves one, and neither
//!   are the shapes of quotation marks, apostrophes and dashes, or the
//!   spaces around a dash or a slash, or between the full stops of an
//!   ellipsis. A space where a line ended is not known, since a web address
//!   is broken after a full stop with no space, so it is the same as a
//!   space and as none.
//! - A range of numbers is the same however its dash is spaced, and a dash
//!   between two words is the same set as part of one word, as in
//!   "results–only", and with spaces around it.
//! - An accent drawn as a character of its own, as "¨" beside its letter,
//!   is not compared.
//! - A raised mark of a footnote or an affiliation, as in "Hussey¹", is
//!   not compared.
//! - Where the two have the same letters with a hyphen or a space put in
//!   or taken out, as "tau-equivalence" and "tau equivalence" do, that is
//!   the change, and only the hyphen or the space is marked.
//! - Running heads and page numbers are not compared: a page added before
//!   them would otherwise make every one after it a change. They are found
//!   as the split at each chapter finds them (`arrange::furniture`). Line
//!   numbers in the margin of a manuscript are left out too.
//! - A footnote is compared right after the raised mark that refers to
//!   it, wherever the page sets it (`at_marks`), so a word broken at the
//!   foot of a column is one word, and a footnote that one version sets
//!   on another page, or an affiliation set after the authors' names, is
//!   in the same place in both.
//! - What only one file has in the margins of its first page, such as a
//!   journal's name, its copyright line, the DOI and the dates, is not a
//!   change (`margins`): it is the journal's, not the authors'.
//! - A passage that one file has in one place and the other in another is
//!   found even where words in it changed. Where it is on pages that the
//!   rest of the comparison puts side by side, and is drawn before and
//!   after the same words in both, it is in the same place on the page,
//!   and only the order the file keeps its text in differs, as it does for
//!   an abstract in a journal's layout. Then only its changed words are
//!   changes. Elsewhere it is a passage that moved: what is the same in
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

/// How many words two passages need to share, in runs of three or more,
/// to be one passage that moved or is kept in another order, unless they
/// share half the words of the longer. Two passages that have nothing to
/// do with each other hardly ever do.
const SHARED_AT_LEAST: usize = 8;

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
    /// Its letters and its hyphens.
    spelled: u32,
    /// The word as it is written, with its punctuation.
    shown: u32,
    /// The punctuation after it, and before it if it begins the document.
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
    /// Whether a footnote was put between it and the word before it, so
    /// that the two are not next to each other on the page ([`at_marks`]).
    apart: bool,
}

/// The pages of one file read again, to know where a word is drawn. Only
/// the pages that hold a passage paired in place are read, and only where
/// each letter is drawn is kept, of at most [`DRAWN_PAGES`] pages at once.
struct Drawn<'a> {
    source: &'a dyn PageSource,
    pages: std::collections::HashMap<u32, Option<Vec<crate::render::Cell>>>,
}

/// How many pages [`Drawn`] keeps. The passages are weighed in the order
/// of the document, so the pages they are on are mostly a few at a time.
const DRAWN_PAGES: usize = 16;

impl<'a> Drawn<'a> {
    fn new(source: &'a dyn PageSource) -> Self {
        Drawn {
            source,
            pages: Default::default(),
        }
    }

    /// Where the first letter of `word` is drawn on its page.
    fn at(&mut self, word: &Word) -> Option<(u32, crate::render::Cell)> {
        let source = self.source;
        if self.pages.len() >= DRAWN_PAGES && !self.pages.contains_key(&word.page) {
            self.pages.clear();
        }
        let boxes = (self.pages.entry(word.page))
            .or_insert_with(|| {
                source
                    .text_aside(word.page as usize - 1)
                    .map(|text| text.boxes)
            })
            .as_ref()?;
        let cell = *boxes.get(word.from as usize)?;
        (cell.height > 0.0).then_some((word.page, cell))
    }

    /// Whether `word` is read before `other`: on an earlier page, on a line
    /// above it, or to its left on the same line. `None` when either one's
    /// place is not known.
    fn before(&mut self, word: &Word, other: &Word) -> Option<bool> {
        let ((page, a), (other_page, b)) = (self.at(word)?, self.at(other)?);
        if page != other_page {
            return Some(page < other_page);
        }
        let same_line = a.top.max(b.top) < (a.top + a.height).min(b.top + b.height);
        Some(match same_line {
            true => a.left < b.left,
            false => a.top < b.top,
        })
    }
}

/// Every different word of both files, kept once. A word of a document is
/// its number here, so a long book costs a few bytes a word rather than two
/// strings a word. The list and the lookup share each word.
#[derive(Default)]
struct Vocabulary {
    numbers: std::collections::HashMap<std::rc::Rc<str>, u32>,
    words: Vec<std::rc::Rc<str>>,
}

impl Vocabulary {
    fn number(&mut self, word: &str) -> u32 {
        if let Some(&number) = self.numbers.get(word) {
            return number;
        }
        let number = self.words.len() as u32;
        let word: std::rc::Rc<str> = word.into();
        self.words.push(word.clone());
        self.numbers.insert(word, number);
        number
    }
}

/// One change. Neither file is taken to be the earlier one, so each side
/// is said in the same way.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
    /// A word whose punctuation differs is not counted. A word both files
    /// have, which the change takes in for the punctuation beside it, is
    /// counted too, so this says whether words differ rather than how many.
    pub count: usize,
    /// How many words the other file has here that the document does not.
    pub count_there: usize,
    /// The change that holds the same words in the other file, where these
    /// words moved.
    pub moved: Option<usize>,
    /// Which passage that moved these words are part of, by number. A
    /// passage with words changed inside it moves as several runs, and
    /// each is part of the same passage.
    pub passage: Option<usize>,
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
            passage: self.passage,
        }
    }
}

/// Every change from `other` to `document`, in the document's order.
// ponytail: every word of both files is held at once, about 32 bytes a
// word, or 45MB for two versions of a 1,700-page book. Comparing page by
// page would need the pages to line up.
pub fn compare(document: &dyn PageSource, other: &dyn PageSource) -> Result<Vec<Change>, String> {
    compare_until(document, other, &Default::default())
}

/// [`compare`], which stops reading the two files when `stop` is set and
/// answers with an empty error.
pub fn compare_until(
    document: &dyn PageSource,
    other: &dyn PageSource,
    stop: &std::sync::atomic::AtomicBool,
) -> Result<Vec<Change>, String> {
    use similar::DiffOp;
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;
    let mut vocabulary = Vocabulary::default();
    let (ours, margins_here) = words_of(document, &mut vocabulary, stop)?;
    let (theirs, margins_there) = words_of(other, &mut vocabulary, stop)?;
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
    let sides = Sides {
        ours: &ours,
        theirs: &theirs,
        vocabulary: &vocabulary,
    };
    let mut stretches = stretches_of(&ops);
    sides.reach_out(&mut stretches, (0, 0), (theirs.len(), ours.len()));

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

    // Passages only one file has are paired with passages only the other
    // has that share enough of their words in runs of three or more: half
    // the words of the longer, or [`SHARED_AT_LEAST`] words. A passage can be paired
    // in pieces, since one file may keep several captions together that the
    // other keeps apart. What a pair leaves of each passage, before the
    // first run they share and after the last, can be paired again, or
    // stays where it was.
    let free = |side: &Range<usize>, other: &Range<usize>| {
        side.len() >= MOVED_AT_LEAST && (other.is_empty() || side.len() >= PASSAGE)
    };
    // The pieces not yet paired: the stretch each is part of, and its words.
    let mut olds: Vec<(usize, Range<usize>)> = (stretches.iter().enumerate())
        .filter(|(_, (old, new))| free(old, new))
        .map(|(at, (old, _))| (at, old.clone()))
        .collect();
    // A piece that is paired is emptied rather than taken out, so that
    // each keeps its place, which `runs` names it by.
    let mut news: Vec<Option<(usize, Range<usize>)>> = (stretches.iter().enumerate())
        .filter(|(_, (old, new))| free(new, old))
        .map(|(at, (_, new))| Some((at, new.clone())))
        .collect();
    // The pieces of the document each run of three words is in. Two pieces
    // share words in runs of three or more only where they share such a
    // run, so a piece of the other file is weighed only against the pieces
    // that have one of its runs, and not against every piece there is.
    let mut runs: HashMap<&[u32], Vec<usize>> = HashMap::new();
    fn index<'k>(runs: &mut HashMap<&'k [u32], Vec<usize>>, at: usize, keys: &'k [u32]) {
        for run in keys.windows(MOVED_AT_LEAST) {
            let pieces = runs.entry(run).or_default();
            if pieces.last() != Some(&at) {
                pieces.push(at);
            }
        }
    }
    for (at, piece) in news.iter().enumerate() {
        if let Some((_, new)) = piece {
            index(&mut runs, at, &our_keys[new.clone()]);
        }
    }
    // What each stretch gave to pairs, of the other file's words and of the
    // document's.
    let mut cut_old: Vec<Vec<Range<usize>>> = vec![Vec::new(); stretches.len()];
    let mut cut_new: Vec<Vec<Range<usize>>> = vec![Vec::new(); stretches.len()];
    let mut paired = Vec::new();
    let long = |op: &DiffOp| matches!(*op, DiffOp::Equal { len, .. } if len >= MOVED_AT_LEAST);
    let shared_in_runs = |ops: &[DiffOp]| -> usize {
        (ops.iter().filter(|op| long(op)))
            .map(|op| op.old_range().len())
            .sum()
    };
    // A short piece is taken for one that moved only where that is the
    // one answer: no other short piece of its file has the same words, and
    // no two pieces of the other file share as much with it. "in order to"
    // taken out in two places and put in in one has not moved.
    let mut alike: HashMap<(bool, &[u32]), usize> = HashMap::new();
    for (_, old) in &olds {
        if old.len() < PASSAGE {
            *alike.entry((false, &their_keys[old.clone()])).or_insert(0) += 1;
        }
    }
    for (_, new) in news.iter().flatten() {
        if new.len() < PASSAGE {
            *alike.entry((true, &our_keys[new.clone()])).or_insert(0) += 1;
        }
    }
    let lone = |keys: &[u32], here: bool| {
        keys.len() >= PASSAGE || alike.get(&(here, keys)).is_none_or(|&count| count <= 1)
    };
    let deadline = Instant::now() + PATIENCE;
    let (mut had, mut used) = (HashMap::new(), HashMap::new());
    let mut next = 0;
    while next < olds.len() && Instant::now() <= deadline {
        let (source, old) = olds[next].clone();
        next += 1;
        let words = &their_keys[old.clone()];
        let short = words.len() < PASSAGE;
        if !lone(words, false) {
            continue;
        }
        let mut tied = false;
        had.clear();
        for &key in words {
            *had.entry(key).or_insert(0usize) += 1;
        }
        let mut best: Option<(usize, usize, Vec<DiffOp>)> = None;
        let mut candidates: Vec<usize> = (words.windows(MOVED_AT_LEAST))
            .filter_map(|run| runs.get(run))
            .flatten()
            .copied()
            .collect();
        candidates.sort_unstable();
        candidates.dedup();
        for at in candidates {
            let Some((_, new)) = &news[at] else {
                continue;
            };
            let others = &our_keys[new.clone()];
            if !lone(others, true) {
                continue;
            }
            let need = (words.len().max(others.len()))
                .div_ceil(2)
                .clamp(MOVED_AT_LEAST, SHARED_AT_LEAST);
            if words.len().min(others.len()) < need {
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
            // A short piece is weighed against an equal candidate too, to
            // find a tie.
            let beaten = |most: usize| shared < most || (shared == most && !short);
            if shared < need || best.as_ref().is_some_and(|(most, ..)| beaten(*most)) {
                continue;
            }
            let ops = diff(words, others, deadline);
            // Only the part [`cluster`] keeps is paired, so that is the part
            // that has to share enough.
            let shared = match ops.iter().any(long) {
                true => {
                    let (first, last) = cluster(&ops);
                    shared_in_runs(&ops[first..=last])
                }
                false => 0,
            };
            match &best {
                Some((most, ..)) if shared == *most && shared >= need => tied = true,
                Some((most, ..)) if shared <= *most => {}
                _ if shared >= need => {
                    best = Some((shared, at, ops));
                    tied = false;
                }
                _ => {}
            }
        }
        let Some((_, at, ops)) = best.filter(|_| !(short && tied)) else {
            continue;
        };
        let Some((target, new)) = news[at].take() else {
            continue;
        };
        // What both pieces have before the passage, and after it, goes with
        // it where one of the two is short, the other not long, and the two
        // share a word: "Ruben Arslan" after an author list that the other
        // file has with other punctuation.
        let (mut first, mut last) = cluster(&ops);
        let short =
            |o: usize, n: usize| o > 0 && n > 0 && o.min(n) <= PASSAGE && o.max(n) <= 2 * PASSAGE;
        let shares = |ops: &[DiffOp]| ops.iter().any(|op| matches!(op, DiffOp::Equal { .. }));
        let (o, n) = (ops[first].old_range().start, ops[first].new_range().start);
        if short(o, n) && shares(&ops[..first]) {
            first = 0;
        }
        let (o, n) = (
            words.len() - ops[last].old_range().end,
            new.len() - ops[last].new_range().end,
        );
        if short(o, n) && shares(&ops[last + 1..]) {
            last = ops.len() - 1;
        }
        let ops = ops[first..=last].to_vec();
        let (begin, end) = (ops[0].old_range().start, ops[ops.len() - 1].old_range().end);
        let cut = old.start + begin..old.start + end;
        let (begin, end) = (ops[0].new_range().start, ops[ops.len() - 1].new_range().end);
        let cut_there = new.start + begin..new.start + end;
        for rest in [old.start..cut.start, cut.end..old.end] {
            if rest.len() >= MOVED_AT_LEAST {
                olds.push((source, rest));
            }
        }
        for rest in [new.start..cut_there.start, cut_there.end..new.end] {
            if rest.len() >= MOVED_AT_LEAST {
                index(&mut runs, news.len(), &our_keys[rest.clone()]);
                news.push(Some((target, rest)));
            }
        }
        cut_old[source].push(cut);
        cut_new[target].push(cut_there);
        paired.push((source, target, old.start, new.start, ops));
    }

    let mut changes = Vec::new();
    for (at, (old, new)) in stretches.iter().enumerate() {
        // What is left of each side once pairs have taken their parts. Each
        // piece left of one side goes with the piece left of the other in
        // the same place.
        let left = |range: &Range<usize>, cuts: &mut Vec<Range<usize>>| -> Vec<Range<usize>> {
            cuts.sort_by_key(|cut| cut.start);
            let mut pieces = Vec::new();
            let mut from = range.start;
            for cut in cuts.iter() {
                pieces.push(from..cut.start);
                from = cut.end;
            }
            pieces.push(from..range.end);
            pieces.retain(|piece| !piece.is_empty());
            pieces
        };
        let (olds, news) = (left(old, &mut cut_old[at]), left(new, &mut cut_new[at]));
        for piece in 0..olds.len().max(news.len()) {
            let old = (olds.get(piece).cloned()).unwrap_or(old.start..old.start);
            let new = (news.get(piece).cloned()).unwrap_or(new.start..new.start);
            if !sides.dashed_alike(&old, &new) {
                changes.push(sides.change(old, new));
            }
        }
    }
    // A change that takes in the word after it and the change of that word
    // are one change. A word a stretch took in at its edge, in
    // [`Sides::reach_out`], is part of that stretch's change already.
    let marked = |old: usize,
                  new: usize,
                  len: usize,
                  taken: &[(Range<usize>, Range<usize>)],
                  changes: &mut Vec<Change>| {
        let in_stretch = |word: usize| {
            let at = taken.partition_point(|(old, _)| old.end <= word);
            taken.get(at).is_some_and(|(old, _)| old.contains(&word))
        };
        let mut open: Option<Range<usize>> = None;
        for at in 0..len {
            if in_stretch(old + at) {
                continue;
            }
            let Some(width) = sides.marked_width(old + at, new + at) else {
                continue;
            };
            open = match open {
                Some(words) if at < words.end => Some(words.start..words.end.max(at + width)),
                Some(words) => {
                    changes.push(sides.respaced(old, new, words));
                    Some(at..at + width)
                }
                None => Some(at..at + width),
            };
        }
        if let Some(words) = open {
            changes.push(sides.respaced(old, new, words));
        }
    };
    for op in &ops {
        if let DiffOp::Equal {
            old_index,
            new_index,
            len,
        } = *op
        {
            marked(old_index, new_index, len, &stretches, &mut changes);
        }
    }
    // The words both files have, as runs: where each begins in the other
    // file, where it begins in the document, and how long it is.
    let equal: Vec<(usize, usize, usize)> = (ops.iter())
        .filter_map(|op| match *op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => Some((old_index, new_index, len)),
            _ => None,
        })
        .collect();
    // The first word both files have at or after word `at` of the other
    // file, or of the document, as its place in each.
    let shared_from = |at: usize, in_document: bool| {
        let run = equal.partition_point(|&(old, new, len)| match in_document {
            false => old + len <= at,
            true => new + len <= at,
        });
        equal.get(run).map(|&(old, new, _)| {
            let skip = at.saturating_sub(if in_document { new } else { old });
            (old + skip, new + skip)
        })
    };
    let (mut drawn_there, mut drawn_here) = (Drawn::new(other), Drawn::new(document));
    for (passage, (source, target, old_start, new_start, inner)) in paired.into_iter().enumerate() {
        let (old, new) = (
            old_start + inner[0].old_range().start,
            new_start + inner[0].new_range().start,
        );
        // How long the paired part is in the file that has less of it.
        let last = &inner[inner.len() - 1];
        let length = (last.old_range().end - inner[0].old_range().start)
            .min(last.new_range().end - inner[0].new_range().start);
        let ends = (
            old_start + last.old_range().end,
            new_start + last.new_range().end,
        );
        // A passage kept in another order is drawn on the same side of the
        // words both files have after it, in either file's order. A
        // passage that moved on its page is drawn on the other side of the
        // words it moved past.
        let mut drawn_alike = || {
            [shared_from(ends.0, false), shared_from(ends.1, true)]
                .into_iter()
                .flatten()
                .all(|(old_after, new_after)| {
                    let there = drawn_there.before(&theirs[old], &theirs[old_after]);
                    let here = drawn_here.before(&ours[new], &ours[new_after]);
                    there.zip(here).is_none_or(|(there, here)| there == here)
                })
        };
        let in_place = (source == target
            || length >= PASSAGE && beside.contains(&(theirs[old].page, ours[new].page)))
            && drawn_alike();
        let shifted = |range: Range<usize>, by: usize| range.start + by..range.end + by;
        // In a passage that moved, a run too short to be said as moved is
        // part of the change beside it, so that no word in it goes unmarked.
        let within = match in_place {
            true => stretches_of(&inner),
            false => between_runs(&inner),
        };
        let mut within: Vec<(Range<usize>, Range<usize>)> = (within.into_iter())
            .map(|(o, n)| (shifted(o, old_start), shifted(n, new_start)))
            .collect();
        // In a passage that moved, the words both files have around a
        // change inside it are marked as moved, so a change there does not
        // reach out over them as well.
        if in_place {
            sides.reach_out(&mut within, (old, new), ends);
        }
        for (o, n) in &within {
            if !sides.dashed_alike(o, n) {
                changes.push(sides.change(o.clone(), n.clone()));
            }
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
                marked(o, n, len, &within, &mut changes);
            } else if len >= MOVED_AT_LEAST {
                // Each is said where the other file had the passage.
                let (gone, came) = (stretches[source].1.start, stretches[target].0.start);
                let number = changes.len();
                let mut left = sides.change(o..o + len, gone..gone);
                let mut arrived = sides.change(came..came, n..n + len);
                (left.moved, arrived.moved) = (Some(number + 1), Some(number));
                (left.passage, arrived.passage) = (Some(passage), Some(passage));
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
    // What only one file has in the margins of its first page is what a
    // journal prints there, and not the authors' words. A change that moved
    // is kept, since the other one names it.
    let in_margins = |places: &[(usize, usize, usize)], margins: &[bool]| {
        (places.iter()).all(|&(page, from, to)| {
            page == 1 && (from..to).all(|at| margins.get(at) == Some(&true))
        })
    };
    let journal = |change: &Change| {
        change.moved.is_none()
            && (change.there.is_empty() && in_margins(&change.places, &margins_here)
                || change.here.is_empty() && in_margins(&change.places_there, &margins_there))
    };
    let mut order: Vec<usize> = (0..changes.len())
        .filter(|&at| !journal(&changes[at]))
        .collect();
    order.sort_by_key(|&at| begins(&changes[at]));
    let mut now_at = vec![0; changes.len()];
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

/// The part of a pair that is one passage, as its first and last
/// operations: from a run of three or more
/// shared words to the last such run before a stretch of more than twice
/// [`PASSAGE`] words that differ, taking the part with the most words in
/// such runs. Captions are much alike, and a passage that holds several
/// shares a phrase with each caption of the other file.
fn cluster(ops: &[similar::DiffOp]) -> (usize, usize) {
    let long = |op: &similar::DiffOp| matches!(*op, similar::DiffOp::Equal { len, .. } if len >= MOVED_AT_LEAST);
    // Each part, as the first and the last of its runs, and its words.
    let mut parts: Vec<(usize, usize, usize)> = Vec::new();
    for (at, op) in ops.iter().enumerate() {
        if !long(op) {
            continue;
        }
        let len = op.old_range().len();
        match parts.last_mut() {
            Some((_, last, words)) => {
                let between = |range: fn(&similar::DiffOp) -> std::ops::Range<usize>| {
                    range(op).start - range(&ops[*last]).end
                };
                if between(similar::DiffOp::old_range).max(between(similar::DiffOp::new_range))
                    <= 2 * PASSAGE
                {
                    (*last, *words) = (at, *words + len);
                    continue;
                }
                parts.push((at, at, len));
            }
            None => parts.push((at, at, len)),
        }
    }
    let (first, last, _) = (parts.into_iter())
        .max_by_key(|&(first, _, words)| (words, std::cmp::Reverse(first)))
        .expect("a run");
    (first, last)
}

/// Where two runs of words differ, each a range of the old run and a range
/// of the new. Changes next to each other are one. So are two changes with
/// a few words between them, no more than [`BETWEEN`], where each change is
/// twice as long as those words and one of them three times as long.
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
            if between > 0 && (between > BETWEEN || 2 * between > small || 3 * between > large) {
                break;
            }
            let (old, new) = out.pop().expect("two");
            let last = out.last_mut().expect("two");
            (last.0.end, last.1.end) = (old.end, new.end);
        }
    }
    out
}

/// The words between the runs of [`MOVED_AT_LEAST`] or more that two
/// passages share, each stretch a range of the old passage and one of the
/// new, with the shorter runs inside them.
fn between_runs(ops: &[similar::DiffOp]) -> Vec<(Range<usize>, Range<usize>)> {
    let mut out: Vec<(Range<usize>, Range<usize>)> = Vec::new();
    let mut open = false;
    for op in ops {
        if matches!(*op, similar::DiffOp::Equal { len, .. } if len >= MOVED_AT_LEAST) {
            open = false;
            continue;
        }
        let (old, new) = (op.old_range(), op.new_range());
        match out.last_mut() {
            Some(last) if open => (last.0.end, last.1.end) = (old.end, new.end),
            _ => out.push((old, new)),
        }
        open = true;
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
    /// Only the characters that differ are marked, where the words are one
    /// run on each page. Where the two sides have the same letters, as
    /// "tau-equivalence" and "tau equivalence" have, the change is a hyphen
    /// or a space, and no word is counted as changed.
    fn change(&self, old: Range<usize>, new: Range<usize>) -> Change {
        let (ours, theirs) = (&self.ours[new.clone()], &self.theirs[old.clone()]);
        let (mut places_here, mut places_there) = (
            places(self.ours, new.clone()),
            places(self.theirs, old.clone()),
        );
        let (here, there) = (self.said(ours), self.said(theirs));
        let letters = |words: &[Word]| -> String {
            (words.iter())
                .map(|word| &*self.vocabulary.words[word.key as usize])
                .collect()
        };
        let respaced = !ours.is_empty()
            && !theirs.is_empty()
            && (ours.len() > 1 || theirs.len() > 1)
            && letters(ours) == letters(theirs);
        if !ours.is_empty() && !theirs.is_empty() {
            let (said, said_there): (Vec<char>, Vec<char>) =
                (here.chars().collect(), there.chars().collect());
            let shared = shared_ends(&said, &said_there);
            letters_only(&mut places_here, ours, said.len(), shared);
            letters_only(&mut places_there, theirs, said_there.len(), shared);
        }
        let counted = |words: &[Word]| if respaced { 0 } else { words.len() };
        Change {
            places: places_here,
            places_there,
            here,
            there,
            count: counted(ours),
            count_there: counted(theirs),
            moved: None,
            passage: None,
        }
    }

    /// Whether the words `old` of the other file and `new` of the document
    /// differ only in how a dash between two of them is set: an en dash
    /// with no spaces is part of its word, as in "results–only", and a
    /// dash with spaces is punctuation between two words. A house style
    /// chooses between them, so that is no change.
    fn dashed_alike(&self, old: &Range<usize>, new: &Range<usize>) -> bool {
        if old.is_empty() || new.is_empty() || old.len() == new.len() {
            return false;
        }
        // The words with their hyphens, and what stands between them: a
        // dash, other punctuation, or a space.
        let written = |words: &[Word]| -> String {
            let mut out = String::new();
            for (at, word) in words.iter().enumerate() {
                out.push_str(&self.vocabulary.words[word.spelled as usize]);
                if at + 1 < words.len() {
                    let marks = &self.vocabulary.words[word.marks as usize];
                    match marks.split('\0').nth(1) {
                        Some("") | None => out.push(' '),
                        Some(after) => out.push_str(after),
                    }
                }
            }
            out
        };
        written(&self.theirs[old.clone()]) == written(&self.ours[new.clone()])
            && !self.marks_differ(old.end - 1, new.end - 1)
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

    /// Each stretch begun a word earlier where that word, which both files
    /// have, is followed by other punctuation in each, so that a dash in
    /// "0–3%" against "0% to 3%" is part of the change. And each ended a
    /// word later where the punctuation before that word differs and is
    /// shown with it, so that the brackets of "Industrial-Organizational (I/O)" against "I/O" are
    /// part of it. A stretch does not reach past the ones beside it, or
    /// past `from` and `to`, each a word of the other file and one of the
    /// document.
    fn reach_out(
        &self,
        stretches: &mut [(Range<usize>, Range<usize>)],
        from: (usize, usize),
        to: (usize, usize),
    ) {
        let (mut floor_old, mut floor_new) = from;
        for at in 0..stretches.len() {
            let (ceiling_old, ceiling_new) =
                (stretches.get(at + 1)).map_or(to, |(old, new)| (old.start, new.start));
            let (o, n) = &mut stretches[at];
            if o.start > floor_old
                && n.start > floor_new
                && self.theirs[o.start - 1].key == self.ours[n.start - 1].key
                && self.marks_differ(o.start - 1, n.start - 1)
            {
                (o.start, n.start) = (o.start - 1, n.start - 1);
            }
            if o.end < ceiling_old
                && n.end < ceiling_new
                && o.end > 0
                && n.end > 0
                && self.theirs[o.end].key == self.ours[n.end].key
                && (self.led(&self.theirs[o.end]) || self.led(&self.ours[n.end]))
                && self.marks_differ(o.end - 1, n.end - 1)
            {
                (o.end, n.end) = (o.end + 1, n.end + 1);
            }
            (floor_old, floor_new) = (o.end, n.end);
        }
    }

    /// Whether a word is shown with punctuation before it, as an opening
    /// bracket or quotation mark is. No word begins with an apostrophe, so
    /// one before a word is a single quotation mark.
    fn led(&self, word: &Word) -> bool {
        (self.vocabulary.words[word.shown as usize].chars().next())
            .is_some_and(|c| !in_word(c) || APOSTROPHES.contains(&c))
    }

    /// Whether the punctuation after the word `old` of the other file and
    /// after the word `new` of the document differ. A space after the last
    /// word of a page is not known, since the next page begins without one,
    /// so it is not compared there. Neither is a space where a line ended,
    /// which the punctuation holds as a line feed: it is the same as a
    /// space and as none.
    fn marks_differ(&self, old: usize, new: usize) -> bool {
        let last = |words: &[Word], at: usize| {
            (words.get(at + 1)).is_none_or(|next| next.page != words[at].page)
        };
        let trim = last(self.theirs, old) || last(self.ours, new);
        let marks = |word: &Word| -> &str {
            let marks = &*self.vocabulary.words[word.marks as usize];
            if trim {
                marks.trim_end_matches([' ', '\n'])
            } else {
                marks
            }
        };
        let (one, other) = (marks(&self.theirs[old]), marks(&self.ours[new]));
        one != other
            && one.replace('\n', " ") != other.replace('\n', " ")
            && one.replace('\n', "") != other.replace('\n', "")
    }

    /// The change where the word `old` of the other file and the word `new`
    /// of the document, which have the same letters, differ in their
    /// punctuation or their hyphens. A hyphen is not compared where one of
    /// them was joined at the end of a line, since the hyphen may then have
    /// been the typesetter's.
    #[cfg(test)]
    fn marked_apart(&self, old: usize, new: usize) -> Option<Change> {
        (self.marked_width(old, new)).map(|width| self.respaced(old, new, 0..width))
    }

    /// The words from `old` and `new` on, `words` counted from there, as a
    /// change in which no word is counted as changed.
    fn respaced(&self, old: usize, new: usize, words: Range<usize>) -> Change {
        let mut change = self.change(
            old + words.start..old + words.end,
            new + words.start..new + words.end,
        );
        (change.count, change.count_there) = (0, 0);
        change
    }

    /// How many words the change made by [`Sides::marked_apart`] takes in:
    /// one, or two where the punctuation that differs is shown with the
    /// next word, or where only the space after it differs, as in "p. 73"
    /// against "p.73". `None` if the words are written alike.
    fn marked_width(&self, old: usize, new: usize) -> Option<usize> {
        let (one, other) = (&self.theirs[old], &self.ours[new]);
        let hyphened = !one.joined && !other.joined && one.spelled != other.spelled;
        // Punctuation before a word that only one file has is part of the
        // change that word is ([`Sides::reach_out`]).
        let next = (self.theirs.get(old + 1)).map(|word| word.key)
            == (self.ours.get(new + 1)).map(|word| word.key);
        let marked = self.marks_differ(old, new);
        if !hyphened && (!marked || !next) {
            return None;
        }
        // Punctuation that differs and is shown with the next word, as an
        // opening bracket is, is said and marked with that word too. So is
        // a space after punctuation that both words show alike.
        let led = |word: &Word| self.led(word);
        let wide = marked
            && next
            && (one.shown == other.shown
                || (self.theirs.get(old + 1).zip(self.ours.get(new + 1)))
                    .is_some_and(|(one, other)| led(one) || led(other)));
        Some(if wide { 2 } else { 1 })
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

/// Where words stand in place of others, their place narrowed to the
/// characters that differ, leaving out the `shared` characters at the start
/// and the end of the `letters` they are said in. Words whose characters on
/// the page are not those, such as a word broken across two lines, keep
/// their whole place.
fn letters_only(
    places: &mut [(usize, usize, usize)],
    words: &[Word],
    letters: usize,
    (start, end): (usize, usize),
) {
    if let ([(_, from, to)], [first, ..], [.., last]) = (places, words, words) {
        if (last.to - first.from) as usize == letters {
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
            Some((page, _, to)) if *page == word.page as usize && !word.apart => {
                *to = word.to as usize
            }
            _ => out.push((word.page as usize, word.from as usize, word.to as usize)),
        }
    }
    out
}

/// Every word of a document, in the order its pages have them, without
/// its running heads, page numbers and line numbers, and with each
/// footnote after its mark ([`at_marks`]). Also which characters of the
/// first page are in its margins ([`margins`]).
fn words_of(
    document: &dyn PageSource,
    vocabulary: &mut Vocabulary,
    stop: &std::sync::atomic::AtomicBool,
) -> Result<(Vec<Word>, Vec<bool>), String> {
    let mut words = Vec::new();
    let mut margins_of_first = Vec::new();
    let count = document.pages();
    // The pages from two before the page to two after it, each with its
    // place, since a running head is found by the pages around it.
    let mut held: std::collections::VecDeque<(usize, PageText, Vec<crate::arrange::Line>)> =
        Default::default();
    // Whether the page before ended in a word broken with a hyphen.
    let mut broken = false;
    for index in 0..count {
        while held.back().map_or(0, |(at, ..)| at + 1) < (index + 3).min(count) {
            let halted = |flag: &std::sync::atomic::AtomicBool| {
                flag.load(std::sync::atomic::Ordering::Relaxed)
            };
            if halted(&crate::export::GIVE_UP) || halted(stop) {
                return Err(String::new());
            }
            let next = held.back().map_or(0, |(at, ..)| at + 1);
            let text = (document.text_aside(next)).ok_or(crate::export::CLOSED_WHILE_READ)?;
            let lines = crate::arrange::lines(&text);
            held.push_back((next, text, lines));
        }
        while held.front().is_some_and(|(at, ..)| at + 2 < index) {
            held.pop_front();
        }
        let (_, text, lines) = (held.iter())
            .find(|(at, ..)| *at == index)
            .expect("read above");
        let around: Vec<&[crate::arrange::Line]> = (held.iter())
            .filter(|(at, ..)| *at != index)
            .map(|(.., lines)| lines.as_slice())
            .collect();
        let height = document.size_of(index).height;
        let mut skip = crate::arrange::furniture_of(lines, text.chars.len(), &around, height);
        line_numbers(text, &mut skip);
        let row = rows(text, &skip);
        let size = body_size(&row);
        if index == 0 {
            margins_of_first = margins(text, &skip, &row, size);
        }
        let next = (held.iter())
            .find(|(at, ..)| *at == index + 1)
            .map(|(_, text, _)| body_size(&rows(text, &[])));
        let page = (text, skip.as_slice(), row.as_slice(), size, next);
        broken = words_noted(page, index + 1, broken, vocabulary, &mut words);
    }
    Ok((words, margins_of_first))
}

/// [`words_on`], with each footnote after its mark ([`at_marks`]). The
/// page is its text, what to skip of it, its lines, the size of its type,
/// and the size of the next page's type.
fn words_noted(
    (text, skip, rows, size, next): (&PageText, &[bool], &[Row], f32, Option<f32>),
    page: usize,
    carried: bool,
    vocabulary: &mut Vocabulary,
    out: &mut Vec<Word>,
) -> bool {
    let Some((moved, moved_skip, order)) = at_marks(text, skip, rows, size, next) else {
        return words_on(text, skip, page, carried, vocabulary, out);
    };
    let first = out.len();
    let broken = words_on(&moved, &moved_skip, page, carried, vocabulary, out);
    // Each word's place is said in the page's own text again. A word is
    // said from the last line end put in before its end, since only the
    // characters after that are next to each other on the page.
    let mut end = 0;
    for word in &mut out[first..] {
        let (from, to) = (word.from as usize, word.to as usize);
        let start = (from..to)
            .rev()
            .find(|&at| order[at] == usize::MAX)
            .map_or(from, |at| at + 1);
        word.apart = (end..from).any(|at| order[at] == usize::MAX);
        (word.from, word.to) = (order[start] as u32, order[to - 1] as u32 + 1);
        end = to;
    }
    broken
}

/// How much smaller than the page's text a line must be to be taken for a
/// footnote, or for what a journal prints in the margins of the first page.
/// The quotations set apart in a journal's text are smaller too, by a
/// tenth.
const SMALL: f32 = 0.85;

/// A line of a page, as pdfium reads it, and cut where a character is drawn
/// below the one before it: a footnote's mark at the foot of a column is
/// read as part of the column's last line.
struct Row {
    /// Its characters, from the first drawn to the last.
    chars: Range<usize>,
    /// How many characters are drawn.
    count: usize,
    /// The height of its letters, as the middle one has it.
    size: f32,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

impl Row {
    /// Whether `other` is below this line and shares some of its width.
    fn above(&self, other: &Row) -> bool {
        other.top + other.size / 2.0 > self.bottom
            && other.left < self.right
            && self.left < other.right
    }
}

/// The lines of a page, leaving out what `skip` says.
fn rows(text: &PageText, skip: &[bool]) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    // The heights of the letters of the last line, and of its other
    // characters.
    let (mut letters, mut others): (Vec<f32>, Vec<f32>) = Default::default();
    let finish = |row: Option<&mut Row>, letters: &mut Vec<f32>, others: &mut Vec<f32>| {
        let heights = if letters.is_empty() {
            &mut *others
        } else {
            &mut *letters
        };
        heights.sort_by(f32::total_cmp);
        if let (Some(row), Some(&size)) = (row, heights.get(heights.len() / 2)) {
            row.size = size;
        }
        letters.clear();
        others.clear();
    };
    let (mut open, mut last) = (false, None::<crate::render::Cell>);
    for (at, (&character, &cell)) in text.chars.iter().zip(&text.boxes).enumerate() {
        if character == '\n' {
            open = false;
            continue;
        }
        if character.is_whitespace() || skip.get(at) == Some(&true) || cell.height <= 0.0 {
            continue;
        }
        let below = last.is_some_and(|last| cell.top >= last.top + last.height);
        if !open || below {
            finish(out.last_mut(), &mut letters, &mut others);
            out.push(Row {
                chars: at..at,
                count: 0,
                size: cell.height,
                left: cell.left,
                right: cell.left + cell.width,
                top: cell.top,
                bottom: cell.top + cell.height,
            });
            open = true;
        }
        let row = out.last_mut().expect("a line was begun");
        row.chars.end = at + 1;
        row.count += 1;
        row.left = row.left.min(cell.left);
        row.right = row.right.max(cell.left + cell.width);
        row.top = row.top.min(cell.top);
        row.bottom = row.bottom.max(cell.top + cell.height);
        match character.is_alphanumeric() {
            true => letters.push(cell.height),
            false => others.push(cell.height),
        }
        last = Some(cell);
    }
    finish(out.last_mut(), &mut letters, &mut others);
    out
}

/// The height of most of a page's letters, to the nearest half point.
fn body_size(rows: &[Row]) -> f32 {
    let mut counts: std::collections::HashMap<i32, usize> = Default::default();
    for row in rows {
        *counts.entry((row.size * 2.0).round() as i32).or_default() += row.count;
    }
    (counts.into_iter())
        .max_by_key(|&(size, count)| (count, size))
        .map_or(0.0, |(size, _)| size as f32 / 2.0)
}

/// Which characters of the first page are in its margins, where a journal
/// puts its name, the issue, the copyright line, the DOI and the dates:
/// the lines above the title, and the lines in small type that have
/// nothing in the page's type above them or below them. The title is the
/// largest type in a line of four characters or more, so that a drop cap
/// or a chapter's number is not taken for it. A line at the foot that
/// begins with a mark raised in the page's type is the authors' footnote,
/// such as an affiliation, and is not in the margins. Spaces, and what
/// `skip` says, count as margins too.
///
/// ponytail: a footnote at the foot with no mark, such as a funding line,
/// is still taken for the journal's.
fn margins(text: &PageText, skip: &[bool], rows: &[Row], size: f32) -> Vec<bool> {
    let mut out: Vec<bool> = (0..text.chars.len())
        .map(|at| text.chars[at].is_whitespace() || skip.get(at) == Some(&true))
        .collect();
    let small = |row: &Row| row.size < SMALL * size;
    let largest = (rows.iter())
        .filter(|row| row.count >= 4)
        .map(|row| row.size)
        .fold(0.0, f32::max);
    let title = match largest > size / SMALL {
        true => (rows.iter())
            .filter(|row| row.count >= 4 && row.size >= SMALL * largest)
            .map(|row| row.top)
            .fold(f32::MAX, f32::min),
        false => f32::MIN,
    };
    let is_mark = |c: &char| c.is_ascii_digit() || NOTE_MARKS.contains(c);
    // The marks raised in the page's type, as "1,2" after a name is two.
    let lifted = raised(text);
    let mut marks: Vec<String> = Vec::new();
    for row in rows.iter().filter(|row| !small(row)) {
        let mut mark = String::new();
        for at in row.chars.clone() {
            let c = text.chars[at];
            if lifted[at] && is_mark(&c) {
                mark.push(c);
            } else if !mark.is_empty() {
                marks.push(std::mem::take(&mut mark));
            }
        }
        marks.extend(Some(mark).filter(|mark| !mark.is_empty()));
    }
    for row in rows {
        let text_rows = || rows.iter().filter(|other| !small(other));
        let head = text_rows().all(|other| !other.above(row));
        let foot = text_rows().all(|other| !row.above(other));
        let mark: String = text.chars[row.chars.clone()]
            .iter()
            .take_while(|c| is_mark(c))
            .collect();
        let footnote = !mark.is_empty() && marks.contains(&mark);
        if row.bottom <= title || small(row) && (head || foot && !footnote) {
            out[row.chars.clone()].fill(true);
        }
    }
    out
}

/// The marks of a footnote: digits, or one of these.
const NOTE_MARKS: [char; 7] = [
    '*', '\u{2020}', '\u{2021}', '\u{a7}', '\u{b6}', '\u{2016}', '#',
];

/// The page's text with each footnote put after the raised mark that
/// refers to it, where pdfium reads it at the foot of its column, in the
/// middle of the text that runs on to the next column or page. A word broken
/// there is then one word again, and a footnote is compared where the
/// other version, which sets it on another page or after the authors'
/// names, has it.
///
/// A footnote is a line in type smaller than the page's text
/// ([`SMALL`]), with no line of the page's type below it in its column.
/// One begins at a line that begins with a mark the page raises in its
/// text ([`raised`]), and goes on to the next one; it is put after the
/// first such mark not taken already. One whose mark is not found, as one
/// running on from the page before, is put after the last sentence the
/// text ends on the page. Where the footnotes would have as many
/// characters as the rest of the page, or the next page's type is as small
/// as theirs, the small lines are a list of references or a table, and
/// they are left where they are.
///
/// Answers the text, what to `skip` of it, and where each of its
/// characters is in the page's own text, with `usize::MAX` for the line
/// ends put in around each footnote. `None` where there is no footnote.
fn at_marks(
    text: &PageText,
    skip: &[bool],
    rows: &[Row],
    size: f32,
    next: Option<f32>,
) -> Option<(PageText, Vec<bool>, Vec<usize>)> {
    let small = |row: &Row| row.size < SMALL * size;
    let notes: Vec<&Row> = (rows.iter())
        .filter(|row| small(row) && !rows.iter().any(|other| !small(other) && row.above(other)))
        .collect();
    let in_notes: usize = notes.iter().map(|row| row.count).sum();
    let all: usize = rows.iter().map(|row| row.count).sum();
    let largest = notes.iter().map(|row| row.size).fold(0.0, f32::max);
    if notes.is_empty() || 2 * in_notes >= all || next.is_some_and(|next| next < largest / SMALL) {
        return None;
    }
    let chars = &text.chars;
    let mut noted = vec![false; chars.len()];
    for row in &notes {
        noted[row.chars.clone()].fill(true);
    }
    // The raised marks of the text: where each run of them ends, and the
    // marks in it, as "1,2" has two.
    let lifted = raised(text);
    let mut marks: Vec<(usize, Vec<String>)> = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if !lifted[at] || noted[at] {
            at += 1;
            continue;
        }
        let from = at;
        while at < chars.len() && lifted[at] && !noted[at] {
            at += 1;
        }
        let run: String = chars[from..at].iter().collect();
        // A raised icon, such as ORCID's, is not a mark.
        let each = (run.split(|c: char| !c.is_ascii_digit() && !NOTE_MARKS.contains(&c)))
            .filter(|mark| !mark.is_empty());
        marks.push((at, each.map(String::from).collect()));
    }
    // Where each footnote goes, by the character it goes after, and its lines.
    let mut placed: Vec<(usize, Vec<Range<usize>>)> = Vec::new();
    let mut loose: Vec<Range<usize>> = Vec::new();
    for row in &notes {
        let mark: String = (chars[row.chars.clone()].iter())
            .take_while(|c| c.is_ascii_digit() || NOTE_MARKS.contains(c))
            .collect();
        let found = (!mark.is_empty())
            .then(|| {
                (marks.iter_mut()).find_map(|(end, each)| {
                    let taken = each.iter().position(|one| *one == mark)?;
                    each.remove(taken);
                    Some(*end)
                })
            })
            .flatten();
        match (found, placed.last_mut()) {
            (Some(end), _) => placed.push((end, vec![row.chars.clone()])),
            (None, Some((_, lines))) => lines.push(row.chars.clone()),
            (None, None) => loose.push(row.chars.clone()),
        }
    }
    // The end of the last sentence of the text, after any closing quotation
    // mark or bracket, or else the end of the text.
    let text_at =
        |at: usize| !noted[at] && !chars[at].is_whitespace() && skip.get(at) != Some(&true);
    let last = (0..chars.len()).rev().find(|&at| text_at(at))?;
    let closing = |c: char| matches!(c, ')' | ']' | '"' | '\'' | '\u{201d}' | '\u{2019}');
    let ends = (0..=last).rev().filter(|&at| text_at(at)).find(|&at| {
        let mut after = at + 1;
        while after <= last && closing(chars[after]) {
            after += 1;
        }
        matches!(chars[at], '.' | '?' | '!') && (after > last || chars[after].is_whitespace())
    });
    let sentence = ends.map_or(last, |at| {
        let mut at = at;
        while at < last && closing(chars[at + 1]) {
            at += 1;
        }
        at
    });
    if !loose.is_empty() {
        placed.push((sentence + 1, loose));
    }
    placed.sort_by_key(|(after, _)| *after);
    let mut moved = PageText::default();
    let (mut moved_skip, mut order) = (Vec::new(), Vec::new());
    let mut put = |moved: &mut PageText, at: usize| {
        moved
            .chars
            .push(if at == usize::MAX { '\n' } else { chars[at] });
        moved
            .boxes
            .push(text.boxes.get(at).copied().unwrap_or_default());
        moved_skip.push(skip.get(at) == Some(&true));
        order.push(at);
    };
    let mut notes = placed.into_iter().peekable();
    for at in 0..=chars.len() {
        while let Some((_, lines)) = notes.next_if(|(after, _)| *after == at) {
            for line in lines {
                put(&mut moved, usize::MAX);
                line.for_each(|at| put(&mut moved, at));
            }
            put(&mut moved, usize::MAX);
        }
        if at == chars.len() {
            break;
        }
        match noted[at] {
            // A line put in where a footnote was keeps the words on either side apart.
            true if at == 0 || !noted[at - 1] => put(&mut moved, usize::MAX),
            true => {}
            false => put(&mut moved, at),
        }
    }
    Some((moved, moved_skip, order))
}

/// How many numbers a page needs outside its text, on one side, to be taken
/// for line numbers.
const LINE_NUMBERS: usize = 5;

/// Line numbers, as a manuscript has them in its margin, marked in `skip`:
/// numbers standing on their own to the left of all the page's other text,
/// or to the right of it, where there are at least [`LINE_NUMBERS`] of them
/// on that side, and they go up in equal steps, as 1, 2, 3 or 5, 10, 15 do.
/// The page numbers of a table of contents, or the last column of a
/// table, do not, and are compared.
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
        let values: Vec<u64> = (found.iter())
            .filter_map(|range| {
                chars[(*range).clone()]
                    .iter()
                    .collect::<String>()
                    .parse()
                    .ok()
            })
            .collect();
        let steady = values.len() == found.len()
            && match values[..] {
                [first, second, ..] if second > first => (values.windows(2))
                    .all(|pair| pair[1].checked_sub(pair[0]) == Some(second - first)),
                _ => false,
            };
        if found.len() >= LINE_NUMBERS && steady {
            for range in found {
                skip[range.clone()].fill(true);
            }
        }
    }
}

/// Whether a character is an accent standing on its own, as "¨" is. Some
/// files draw an accent as a character of its own beside its letter, or
/// apart from it, and the other version has the letter with its accent.
fn accent(character: char) -> bool {
    matches!(
        character,
        '\u{a8}' | '\u{af}' | '\u{b4}' | '\u{b8}' | '\u{2c6}' | '\u{2c7}' | '\u{2d8}'..='\u{2dd}'
    )
}

/// Whether a character belongs to a word rather than being punctuation of
/// its own.
fn in_word(character: char) -> bool {
    character.is_alphanumeric()
        || accent(character)
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
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            c if APOSTROPHES.contains(&c) => '\'',
            '\u{201a}' | '\u{201b}' => '\'',
            // Single and double quotation marks are one shape, since a
            // house style changes them all.
            '"' | '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{201f}' | '\u{ab}' | '\u{bb}' => '\'',
            c => c,
        })
        .collect();
    if key.is_empty() {
        key.push('-');
    }
    key
}

/// Whether a space after the punctuation `compared` ends in is compared.
/// A house style sets dashes and slashes with spaces or without, so the
/// spaces around them are not.
fn spaced_after(compared: &str) -> bool {
    !compared.is_empty() && !compared.ends_with(['-', '/'])
}

/// A word as [`key_of`] has it, but with its hyphens, each as "-".
fn spelled_of(chars: &[char]) -> String {
    (crate::search::fold(chars, false).text.into_iter())
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            c if HYPHENS.contains(&c) => '-',
            '\u{2012}' | '\u{2013}' => '-',
            c if APOSTROPHES.contains(&c) => '\'',
            c => c,
        })
        .collect()
}

/// The words on one page, leaving out the characters `skip` says. What is
/// compared of the punctuation is all of it between a word and the next,
/// and the spaces after each mark, and it goes with the word before. Any
/// before the page's first word goes with the last word of the page
/// before, or with the first word of the document. A space before the first
/// mark is not compared, since a picture or a raised mark beside a word
/// leaves one. Neither are raised marks ([`raised`]). What is shown of the
/// punctuation goes with the word it is written against: the word before
/// where nothing comes between, and otherwise the word after. A word that
/// ends in a hyphen at the end of a line is joined to the word that begins
/// the next one. A word broken at the foot of a page, which `carried` says
/// the page before ended in, is joined to the first word of this page:
/// the joined word is compared, and is placed on the page it begins on.
/// Answers whether this page ends in such a word.
fn words_on(
    text: &PageText,
    skip: &[bool],
    page: usize,
    carried: bool,
    vocabulary: &mut Vocabulary,
    out: &mut Vec<Word>,
) -> bool {
    let chars = &text.chars;
    let blank = |at: usize| chars[at].is_whitespace() || skip.get(at) == Some(&true);
    let first = out.len();
    let lifted = raised(text);
    // Each word's punctuation on this page, compared: before it, which only
    // the first word has, and after it.
    let mut marks: Vec<(String, String)> = Vec::new();
    let mut leading = String::new();
    // Punctuation shown with the word after it: where it begins, and how it
    // is written.
    let mut waiting: Option<(usize, usize, String)> = None;
    let mut at = 0;
    let (mut spaced, mut joining) = (true, carried);
    // Whether a line ended in the white space before this character. A
    // space after punctuation there is not known: the line may have been
    // broken at a space, or in the middle of a web address.
    let mut broke = false;
    // Whether the first word here finished the last word of the page before,
    // whose punctuation then goes on from there.
    let mut finished = false;
    while at < chars.len() {
        if blank(at) {
            broke |= matches!(chars[at], '\r' | '\n');
            (at, spaced) = (at + 1, true);
            continue;
        }
        let from = at;
        let space = if broke { '\n' } else { ' ' };
        broke = false;
        // A quotation mark is not an apostrophe: an apostrophe is in a word
        // only where a letter or a digit comes after it, as in "don't".
        let wordy = in_word(chars[at]) && !APOSTROPHES.contains(&chars[at]);
        at += 1;
        // An en dash between two letters or digits, as in "US–China", is
        // part of the word, as a hyphen is. Between two numbers it is with
        // spaces around it too, and so is a hyphen, so that "12–15",
        // "12 – 15" and "12- 15" are one range. The end of a line is a
        // space there. Answers how many characters the dash and its spaces
        // take.
        let joins = |at: usize| -> usize {
            let dash = |at: usize| matches!(chars.get(at), Some('\u{2012}' | '\u{2013}'));
            let hyphen = |at: usize| chars.get(at).is_some_and(|c| HYPHENS.contains(c));
            // How many characters of white space there are, at most one
            // space or one end of a line.
            let space = |at: usize| match (chars.get(at), chars.get(at + 1)) {
                (Some('\r'), Some('\n')) => 2,
                (Some(' ' | '\r' | '\n'), _) => 1,
                _ => 0,
            };
            let digit = |at: usize| chars.get(at).is_some_and(|c| c.is_numeric());
            let alike = |to: usize| (at..to).all(|at| lifted[at] == lifted[from]);
            if dash(at)
                && chars[at - 1].is_alphanumeric()
                && chars.get(at + 1).is_some_and(|c| c.is_alphanumeric())
                && alike(at + 1)
            {
                return 1;
            }
            if !digit(at - 1) {
                return 0;
            }
            let before = space(at);
            let after = space(at + before + 1);
            let to = at + before + 1 + after;
            // A hyphen with no space after it is part of the word already.
            let ranged = dash(at + before) || (hyphen(at + before) && before + after > 0);
            match ranged && digit(to) && alike(to) {
                true => to - at,
                false => 0,
            }
        };
        while wordy && at < chars.len() {
            let step = joins(at);
            if step > 0 {
                at += step;
            } else if !blank(at)
                && in_word(chars[at])
                && (!APOSTROPHES.contains(&chars[at])
                    || chars.get(at + 1).is_some_and(|c| c.is_alphanumeric()))
                && lifted[at] == lifted[from]
            {
                at += 1;
            } else {
                break;
            }
        }
        let piece: String = (chars[from..at].iter())
            .filter(|&&c| !crate::search::ignored(c) && !accent(c) && c != '\r' && c != '\n')
            .collect();
        if piece.is_empty() {
            continue;
        }
        let key = key_of(&chars[from..at]);
        let gap = if spaced { " " } else { "" };
        let on_page = out.len() > first;
        if lifted[from] || !(wordy && piece.chars().any(char::is_alphanumeric)) {
            let compared = match marks.last_mut() {
                Some((_, after)) => after,
                None => &mut leading,
            };
            if !lifted[from] {
                // The full stops of an ellipsis are set with spaces between
                // them or without, so those spaces are not compared.
                let ellipsis = compared.ends_with('.') && key.starts_with('.');
                if spaced && spaced_after(compared) && !ellipsis {
                    compared.push(space);
                }
                compared.push_str(&key);
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
            && chars.get(at).is_none_or(|&c| c == '\r' || c == '\n');
        // pdfium's mark for a hyphen at the end of a line, inside the word.
        let hyphened = chars[from..at].iter().any(|&c| crate::search::hyphen(c));
        match out.last_mut() {
            Some(last) if joining => {
                let shown = (vocabulary.words[last.shown as usize])
                    .trim_end_matches(HYPHENS)
                    .to_string();
                let joined = format!("{}{key}", vocabulary.words[last.key as usize]);
                last.key = vocabulary.number(&joined);
                let spelled = format!(
                    "{}{}",
                    vocabulary.words[last.spelled as usize].trim_end_matches('-'),
                    spelled_of(&chars[from..at])
                );
                last.spelled = vocabulary.number(&spelled);
                last.shown = vocabulary.number(&format!("{shown}{piece}"));
                last.joined = true;
                // A word that began on the page before keeps its place there.
                match on_page {
                    true => last.to = at as u32,
                    false => finished = true,
                }
            }
            _ => {
                let compared = match marks.last_mut() {
                    Some((_, after)) => after,
                    None => &mut leading,
                };
                if spaced && spaced_after(compared) {
                    compared.push(space);
                }
                let (start, shown) = match waiting.take() {
                    Some((start, _, shown)) => (start, format!("{shown}{gap}{piece}")),
                    None => (from, piece),
                };
                // The punctuation before a page's first word goes on from
                // the last word of the page before, as it would in the
                // middle of a page, where the other file may have it. The
                // page's end is a space, unless the word went on over it.
                let before = if on_page {
                    String::new()
                } else if first > 0 {
                    let after = std::mem::take(&mut leading);
                    let word = &mut out[first - 1];
                    let marks = &vocabulary.words[word.marks as usize];
                    let space = !finished && !after.is_empty() && !marks.ends_with('\u{0}');
                    let joined = format!("{marks}{}{after}", if space { " " } else { "" });
                    word.marks = vocabulary.number(&joined);
                    String::new()
                } else {
                    std::mem::take(&mut leading)
                };
                out.push(Word {
                    key: vocabulary.number(&key),
                    spelled: vocabulary.number(&spelled_of(&chars[from..at])),
                    shown: vocabulary.number(&shown),
                    marks: 0,
                    page: page as u32,
                    from: start as u32,
                    to: at as u32,
                    spaced: spaced || start < from,
                    joined: hyphened,
                    apart: false,
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
    joining
}

/// Which characters are raised above the text before them and smaller, as
/// the mark of a footnote or an affiliation is: each is measured against
/// the last character on its line that is not. A raised number right
/// after a number of one or two digits, as in "10⁴", or after a single
/// letter, as in "R²", is an exponent, and is not counted. After a longer
/// number, as in "2019¹", or a longer word, it is a footnote's mark. A
/// hyphen does not end the word, so "COVID-19¹" ends in a footnote's mark.
/// A single letter after a number is still a single letter, as in "4x²".
/// A footnote's mark on a single letter is then taken for an exponent.
/// An exponent is compared as part of its word, so "10⁴" is the same as
/// "104", which a file that lost the raise has.
fn raised(text: &PageText) -> Vec<bool> {
    let mut out = vec![false; text.chars.len()];
    let mut base: Option<(crate::render::Cell, char)> = None;
    // How many letters or digits come right before, with no space between,
    // and how many of the last of those are letters.
    let (mut run, mut letters) = (0, 0);
    for (at, &character) in text.chars.iter().enumerate() {
        if character == '\r' || character == '\n' {
            (base, run, letters) = (None, 0, 0);
            continue;
        }
        let Some(&cell) = (text.boxes.get(at)).filter(|cell| cell.height > 0.0) else {
            continue;
        };
        if character.is_whitespace() {
            (run, letters) = (0, 0);
            continue;
        }
        // pdfium gives each letter of a ligature the box of the one glyph
        // that draws them. That box can be lower than its neighbours'
        // boxes, and the letters are still not raised.
        let shares = |other: usize| text.boxes.get(other) == Some(&cell);
        if at.checked_sub(1).is_some_and(shares) || shares(at + 1) {
            (run, letters) = (run + 1, letters + 1);
            continue;
        }
        match base {
            Some((base, below))
                if cell.height < 0.8 * base.height
                    && cell.top + cell.height < base.top + 0.85 * base.height =>
            {
                let short = match below {
                    below if below.is_ascii_digit() => run <= 2,
                    below => below.is_alphabetic() && letters == 1,
                };
                let exponent = short
                    && (character.is_ascii_digit() || matches!(character, '-' | '+' | '\u{2212}'));
                out[at] = !exponent;
            }
            _ => {
                base = Some((cell, character));
                run = if character.is_alphanumeric() || HYPHENS.contains(&character) {
                    run + 1
                } else {
                    0
                };
                letters = if character.is_alphabetic() {
                    letters + 1
                } else {
                    0
                };
            }
        }
    }
    out
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
                words_on(&page(text), &[], 1, false, &mut vocabulary, &mut out);
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
    fn a_word_costs_32_bytes() {
        assert_eq!(std::mem::size_of::<Word>(), 32);
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

    /// What a typesetter changes is not compared: capitals, a space
    /// before punctuation, and the shapes of quotation marks and dashes. A
    /// space after punctuation is compared, since leaving it out is a typo.
    #[test]
    fn the_way_text_is_set_is_not_compared() {
        assert_ne!(keys("see p. 73)."), keys("see p.73)."));
        assert_ne!(keys("(> 30,000"), keys("(>30,000"));
        assert_eq!(keys("word , next"), keys("word, next"));
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
        assert_eq!(change.places_there, [(1, 18, 19)]);
        assert_eq!((change.count, change.count_there), (0, 0));
    }

    /// A raised mark beside a word, as an affiliation's is, is not
    /// compared, and neither is the space it leaves before a comma.
    #[test]
    fn a_raised_mark_is_not_compared() {
        let mut text = PageText::default();
        for (at, c) in "Hussey1 , Taym".chars().enumerate() {
            let (top, height) = if at == 6 { (-3.0, 6.0) } else { (0.0, 10.0) };
            text.chars.push(c);
            text.boxes.push(Cell {
                left: 5.0 * at as f32,
                top,
                width: 4.0,
                height,
            });
        }
        let (mut vocabulary, mut raised) = (Vocabulary::default(), Vec::new());
        words_on(&text, &[], 1, false, &mut vocabulary, &mut raised);
        let mut plain = Vec::new();
        words_on(
            &page("Hussey, Taym"),
            &[],
            1,
            false,
            &mut vocabulary,
            &mut plain,
        );
        let compared = |words: &[Word]| -> Vec<(u32, u32)> {
            words.iter().map(|word| (word.key, word.marks)).collect()
        };
        assert_eq!(compared(&raised), compared(&plain));
    }

    /// The two letters of a ligature share one box, which is lower than
    /// the boxes of the letters around it, and they are part of their word.
    #[test]
    fn a_ligature_with_a_low_box_is_not_a_raised_mark() {
        let mut text = PageText::default();
        for (at, c) in "scientific".chars().enumerate() {
            // The "fi" is one glyph.
            let (left, height) = match at {
                7 | 8 => (35.0, 6.0),
                _ => (5.0 * at as f32, 10.0),
            };
            text.chars.push(c);
            text.boxes.push(Cell {
                left,
                top: 0.0,
                width: 4.0,
                height,
            });
        }
        let (mut vocabulary, mut words) = (Vocabulary::default(), Vec::new());
        words_on(&text, &[], 1, false, &mut vocabulary, &mut words);
        let keys: Vec<&str> = (words.iter())
            .map(|word| &*vocabulary.words[word.key as usize])
            .collect();
        assert_eq!(keys, ["scientific"]);
    }

    /// An accent drawn as a character of its own is not compared, beside
    /// its letter or apart from it.
    #[test]
    fn an_accent_on_its_own_is_not_compared() {
        assert_eq!(
            keys("Ho\u{a8}lzel, Y. \u{a8} (2020)"),
            keys("H\u{f6}lzel, Y. (2020)")
        );
        assert_eq!(keys("The \u{b4} chrysalis"), keys("The chrysalis"));
    }

    /// A web address broken after a full stop at the end of a line is the
    /// same as one that is not broken, and a sentence that ends a line is
    /// the same as one with a space after it.
    #[test]
    fn a_space_where_a_line_ended_is_not_compared() {
        let (vocabulary, words) = read(&[
            "see doi.org/10.1038 now. Then",
            "see doi.\r\norg/10.\r\n1038 now.\r\nThen",
            "see doi. org/10.1038 now.Then",
        ]);
        let differ = |theirs: usize, ours: usize| {
            let sides = Sides {
                ours: &words[ours],
                theirs: &words[theirs],
                vocabulary: &vocabulary,
            };
            assert_eq!(super::keys(sides.theirs), super::keys(sides.ours));
            (0..sides.ours.len())
                .filter(|&at| sides.marks_differ(at, at))
                .collect::<Vec<_>>()
        };
        assert_eq!(differ(0, 1), [] as [usize; 0]);
        assert_eq!(differ(0, 2), [1, 5]);
    }

    /// A dash or a slash with spaces around it is the same as one without.
    #[test]
    fn spaces_around_a_dash_are_not_compared() {
        assert_eq!(keys("word \u{2014} next"), keys("word\u{2014}next"));
        assert_eq!(keys("this / that"), keys("this/that"));
        assert_eq!(keys("M., . . . Rabiee"), keys("M., \u{2026} Rabiee"));
        assert_eq!(keys("M., . . . Rabiee"), keys("M., ... Rabiee"));
        assert_ne!(keys("end. Next"), keys("end.Next"));
    }

    /// Punctuation that begins a page is compared as it is in the middle of
    /// a page, so that a page that breaks somewhere else is no change.
    #[test]
    fn punctuation_that_begins_a_page_is_compared_as_within_one() {
        let mut vocabulary = Vocabulary::default();
        let mut read = |pages: &[&str]| {
            let mut out = Vec::new();
            for (at, text) in pages.iter().enumerate() {
                words_on(&page(text), &[], at + 1, false, &mut vocabulary, &mut out);
            }
            let compared: Vec<(u32, u32)> = out.iter().map(|word| (word.key, word.marks)).collect();
            compared
        };
        assert_eq!(
            read(&["The end.", "(Next one"]),
            read(&["The end. (Next one"])
        );
    }

    /// A raised number after a digit is an exponent, which is compared.
    #[test]
    fn an_exponent_is_compared() {
        let raised = |text: &str, up: usize| {
            let mut page = PageText::default();
            for (at, c) in text.chars().enumerate() {
                let (top, height) = if at == up { (-3.0, 6.0) } else { (0.0, 10.0) };
                page.chars.push(c);
                page.boxes.push(Cell {
                    left: 5.0 * at as f32,
                    top,
                    width: 4.0,
                    height,
                });
            }
            page
        };
        let mut vocabulary = Vocabulary::default();
        let mut keys = |page: &PageText| {
            let mut out = Vec::new();
            words_on(page, &[], 1, false, &mut vocabulary, &mut out);
            out.iter().map(|word| word.key).collect::<Vec<_>>()
        };
        assert_ne!(keys(&raised("104 cells", 2)), keys(&raised("105 cells", 2)));
        assert_eq!(keys(&raised("104 cells", 2)), keys(&page("104 cells")));
        // After a single letter too.
        assert_ne!(keys(&raised("R2 fit", 1)), keys(&raised("R3 fit", 1)));
        assert_ne!(keys(&raised("4x2 fit", 2)), keys(&raised("4x3 fit", 2)));
        // After a year or a word, a raised number is a footnote's mark.
        assert_eq!(keys(&raised("in 20191", 7)), keys(&page("in 2019")));
        assert_eq!(keys(&raised("Hussey1 said", 6)), keys(&page("Hussey said")));
        // A hyphen does not end the word before it.
        assert_eq!(
            keys(&raised("COVID-191 cases", 8)),
            keys(&page("COVID-19 cases"))
        );
    }

    /// Quotation marks are punctuation, of one shape, and an apostrophe
    /// inside a word is part of it.
    #[test]
    fn single_and_double_quotation_marks_are_the_same() {
        assert_eq!(
            keys("\u{2018}cut\u{2019} it"),
            keys("\u{201c}cut\u{201d} it")
        );
        assert_eq!(letters("\u{2018}cut\u{2019} it"), ["cut", "it"]);
        assert_eq!(letters("don\u{2019}t go"), ["don't", "go"]);
        assert_ne!(keys("the students\u{2019} work"), keys("the students work"));
    }

    /// A word broken with a hyphen at the foot of a page is one word with
    /// its end at the top of the next, placed where it begins.
    #[test]
    fn a_word_broken_across_a_page_is_one_word() {
        let mut vocabulary = Vocabulary::default();
        let mut split = Vec::new();
        let broken = words_on(&page("a typo-"), &[], 1, false, &mut vocabulary, &mut split);
        assert!(broken);
        let ends = words_on(
            &page("graphy, here."),
            &[],
            2,
            broken,
            &mut vocabulary,
            &mut split,
        );
        assert!(!ends);
        let mut whole = Vec::new();
        words_on(
            &page("a typography, here."),
            &[],
            1,
            false,
            &mut vocabulary,
            &mut whole,
        );
        let compared = |words: &[Word]| -> Vec<(u32, u32)> {
            words.iter().map(|word| (word.key, word.marks)).collect()
        };
        assert_eq!(compared(&split), compared(&whole));
        assert_eq!((split[1].page, split[1].from, split[1].to), (1, 2, 7));
    }

    /// "12–15" is one word, as "12-15" and "12 – 15" are, and the three are
    /// the same.
    #[test]
    fn an_en_dash_between_numbers_is_a_hyphen() {
        let (vocabulary, words) = read(&[
            "pages 12\u{2013}15.",
            "pages 12-15.",
            "pages 12 \u{2013} 15.",
            "pages 12- 15.",
            "pages 12\u{2013}\r\n15.",
        ]);
        let spelled = |words: &[Word]| -> Vec<(u32, u32, u32)> {
            words
                .iter()
                .map(|word| (word.key, word.spelled, word.marks))
                .collect()
        };
        assert_eq!(spelled(&words[0]), spelled(&words[1]));
        assert_eq!(spelled(&words[0]), spelled(&words[2]));
        assert_eq!(spelled(&words[0]), spelled(&words[3]));
        assert_eq!(spelled(&words[0]), spelled(&words[4]));
        let shown = &*vocabulary.words[words[4][1].shown as usize];
        assert_eq!(shown, "12\u{2013}15.");
        assert_eq!(&*vocabulary.words[words[0][1].key as usize], "1215");
        // Between words, a dash with spaces around it is punctuation.
        let (_, words) = read(&["this \u{2013} that"]);
        assert_eq!(words[0].len(), 2);
    }

    /// In a passage that moved, the words between its long shared runs are
    /// one stretch, short shared runs and all.
    #[test]
    fn a_short_run_in_a_moved_passage_is_part_of_the_change_beside_it() {
        use similar::DiffOp::{Equal, Replace};
        let ops = [
            Equal {
                old_index: 0,
                new_index: 0,
                len: 5,
            },
            Replace {
                old_index: 5,
                old_len: 1,
                new_index: 5,
                new_len: 1,
            },
            Equal {
                old_index: 6,
                new_index: 6,
                len: 1,
            },
            Replace {
                old_index: 7,
                old_len: 1,
                new_index: 7,
                new_len: 2,
            },
            Equal {
                old_index: 8,
                new_index: 9,
                len: 5,
            },
        ];
        assert_eq!(between_runs(&ops), [(5..8, 5..9)]);
    }

    /// "tau-equivalence" against "tau equivalence" is a hyphen in place of
    /// a space, and no word changed.
    #[test]
    fn a_hyphen_in_place_of_a_space_is_the_change() {
        let (vocabulary, words) = read(&["tau equivalence", "tau-equivalence"]);
        let sides = Sides {
            ours: &words[1],
            theirs: &words[0],
            vocabulary: &vocabulary,
        };
        let change = sides.change(0..2, 0..1);
        assert_eq!(
            (change.places_there, change.places),
            (vec![(1, 3, 4)], vec![(1, 3, 4)])
        );
        assert_eq!((change.count, change.count_there), (0, 0));
    }

    /// A dash between two words is the same as part of one word and as
    /// punctuation with spaces around it. A hyphen in place of a space is
    /// still a change.
    #[test]
    fn a_dash_between_words_is_the_same_with_spaces_and_without() {
        let (vocabulary, words) = read(&[
            "results\u{2013}only now",
            "results \u{2013} only now",
            "results only now",
            "results \u{2013} only, now",
        ]);
        let alike = |theirs: usize, ours: usize| {
            let sides = Sides {
                ours: &words[ours],
                theirs: &words[theirs],
                vocabulary: &vocabulary,
            };
            let width = |side: usize| words[side].len() - 1;
            sides.dashed_alike(&(0..width(theirs)), &(0..width(ours)))
        };
        assert!(alike(0, 1) && alike(1, 0));
        assert!(!alike(0, 2));
        assert!(!alike(0, 3));
    }

    /// "0/3%" against "0% to 3%": the slash taken out is part of the change
    /// that puts "to" in, which begins at the "0" before it.
    #[test]
    fn punctuation_before_a_change_is_part_of_it() {
        let (vocabulary, words) = read(&["of 0/3% and", "of 0% to 3% and"]);
        let sides = Sides {
            ours: &words[1],
            theirs: &words[0],
            vocabulary: &vocabulary,
        };
        let mut stretches = vec![(2..2, 2..3)];
        sides.reach_out(&mut stretches, (0, 0), (4, 5));
        assert_eq!(stretches, [(1..2, 1..3)]);
        let change = sides.change(1..2, 1..3);
        assert_eq!((&*change.there, &*change.here), ("0/", "0% to"));
    }

    /// "Industrial-Organizational (I/O) dataset" against "I/O data set":
    /// the brackets taken out are part of the changes beside them.
    #[test]
    fn brackets_beside_a_change_are_part_of_it() {
        let (vocabulary, words) = read(&[
            "the Industrial-Organizational (I/O) dataset",
            "the I/O data set",
        ]);
        let sides = Sides {
            ours: &words[1],
            theirs: &words[0],
            vocabulary: &vocabulary,
        };
        let mut stretches = vec![(1..2, 1..1), (4..5, 3..5)];
        sides.reach_out(&mut stretches, (0, 0), (5, 5));
        assert_eq!(stretches, [(1..3, 1..2), (3..5, 2..5)]);
    }

    /// Single quotation marks beside a change are part of it, as brackets
    /// and double quotation marks are.
    #[test]
    fn single_quotation_marks_beside_a_change_are_part_of_it() {
        for quoted in ["(IO)", "\u{201c}IO\u{201d}", "\u{2018}IO\u{2019}", "'IO'"] {
            let (vocabulary, words) = read(&[&format!("the Foo {quoted} set"), "the IO set"]);
            let sides = Sides {
                ours: &words[1],
                theirs: &words[0],
                vocabulary: &vocabulary,
            };
            let mut stretches = vec![(1..2, 1..1)];
            sides.reach_out(&mut stretches, (0, 0), (4, 3));
            assert_eq!(stretches, [(1..3, 1..2)], "{quoted}");
        }
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
        words_on(&text, &skip, 1, false, &mut vocabulary, &mut out);
        let said: Vec<&str> = (out.iter().take(4))
            .map(|word| &*vocabulary.words[word.shown as usize])
            .collect();
        assert_eq!(said, ["Line", "1", "here", "Line"]);
    }

    /// The page numbers of a table of contents, past its dot leaders, are
    /// compared, since they do not go up in equal steps.
    #[test]
    fn page_numbers_in_a_table_of_contents_are_compared() {
        let mut text = PageText::default();
        for (line, page) in [1, 4, 9, 15, 22, 30].into_iter().enumerate() {
            for (at, word) in [
                (10.0, "Chapter ......".to_string()),
                (200.0, page.to_string()),
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
        assert!(skip.iter().all(|skipped| !skipped));
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
            spelled: 0,
            shown: 0,
            marks: 0,
            page: 1,
            from,
            to,
            spaced: true,
            joined: false,
            apart: false,
        };
        let mut places = vec![(1, 10, 16)];
        letters_only(&mut places, &[word(10, 16)], 6, (4, 1));
        assert_eq!(places, [(1, 14, 15)]);
        let mut places = vec![(1, 10, 14)];
        letters_only(&mut places, &[word(10, 14)], 4, (4, 0));
        assert_eq!(places, [(1, 14, 14)]);
        let mut places = vec![(1, 10, 19)];
        letters_only(&mut places, &[word(10, 19)], 6, (4, 1));
        assert_eq!(places, [(1, 10, 19)]);
    }

    /// A page's text from lines, each with where it begins, how far down
    /// it is and the size of its type. A character after "^" is raised, as
    /// a footnote's mark is.
    fn laid(lines: &[(&str, f32, f32, f32)]) -> PageText {
        let mut text = PageText::default();
        for &(words, left, top, size) in lines {
            if !text.chars.is_empty() {
                text.chars.extend(['\r', '\n']);
                text.boxes.extend([Cell::default(); 2]);
            }
            let (mut x, mut raise) = (left, false);
            for character in words.chars() {
                if character == '^' {
                    raise = true;
                    continue;
                }
                let (height, y) = match raise {
                    true => (0.6 * size, top - 0.2 * size),
                    false => (size, top),
                };
                text.chars.push(character);
                text.boxes.push(Cell {
                    left: x,
                    top: y,
                    width: height / 2.0,
                    height,
                });
                (x, raise) = (x + height / 2.0, false);
            }
        }
        text
    }

    /// A footnote at the foot of a page is read after its mark, so a word
    /// broken at the end of the page's text is still one word, and each
    /// word is placed where it is drawn.
    #[test]
    fn a_footnote_is_read_after_its_mark() {
        let text = laid(&[
            (
                "Science is self-correcting^1 they say, and",
                72.0,
                100.0,
                10.0,
            ),
            ("the text runs on to self-", 72.0, 112.0, 10.0),
            ("1 A note in small type.", 72.0, 300.0, 7.0),
        ]);
        let rows = rows(&text, &[]);
        let size = body_size(&rows);
        assert_eq!(size, 10.0);
        let mut vocabulary = Vocabulary::default();
        let mut out = Vec::new();
        let page = (&text, &[][..], &rows[..], size, Some(10.0));
        let broken = words_noted(page, 1, false, &mut vocabulary, &mut out);
        assert!(broken, "the page ends in a broken word");
        let said: Vec<&str> = (out.iter())
            .map(|word| &*vocabulary.words[word.key as usize])
            .collect();
        assert_eq!(
            said,
            [
                "science",
                "is",
                "selfcorrecting",
                "1",
                "a",
                "note",
                "in",
                "small",
                "type",
                "they",
                "say",
                "and",
                "the",
                "text",
                "runs",
                "on",
                "to",
                "self"
            ]
        );
        let drawn = |word: &Word| -> String {
            text.chars[word.from as usize..word.to as usize]
                .iter()
                .collect()
        };
        assert_eq!(drawn(&out[5]), "note");
        assert_eq!(drawn(&out[9]), "they");
        let apart: Vec<usize> = (0..out.len()).filter(|&at| out[at].apart).collect();
        assert_eq!(apart, [3, 9]);
    }

    /// Small lines at the foot of a page that the next page goes on with
    /// in the same small type are a list of references, not footnotes.
    #[test]
    fn small_type_that_goes_on_to_the_next_page_stays_where_it_is() {
        let text = laid(&[
            ("The end of the text^1 is here.", 72.0, 100.0, 10.0),
            ("and the rest of it", 72.0, 112.0, 10.0),
            ("1 Smith, J. (2020). A paper.", 72.0, 300.0, 7.0),
        ]);
        let rows = rows(&text, &[]);
        let size = body_size(&rows);
        assert!(at_marks(&text, &[], &rows, size, Some(10.0)).is_some());
        assert!(at_marks(&text, &[], &rows, size, Some(7.0)).is_none());
    }

    /// What a journal prints above the title and in small type at the head
    /// and the foot of the first page is in its margins, and the title and
    /// the text are not.
    #[test]
    fn the_first_pages_margins_are_the_journals() {
        let lines = [
            ("Original Article", 72.0, 40.0, 10.0),
            ("Journal of Things, Vol. 1", 400.0, 60.0, 6.0),
            ("A Title", 72.0, 80.0, 18.0),
            ("Abstract in small type", 72.0, 110.0, 7.0),
            ("Some text of the paper by Ada^1 here", 72.0, 130.0, 10.0),
            ("and more of the paper", 72.0, 142.0, 10.0),
            ("Received 1 May 2020", 72.0, 400.0, 7.0),
            ("1 A University, Somewhere", 72.0, 410.0, 7.0),
        ];
        let text = laid(&lines);
        let rows = rows(&text, &[]);
        let margins = margins(&text, &[], &rows, body_size(&rows));
        let all: String = text.chars.iter().collect();
        let marked: Vec<bool> = (lines.iter())
            .map(|(words, ..)| {
                let words = words.replace('^', "");
                let from = all[..all.find(&words).unwrap()].chars().count();
                margins[from..from + words.chars().count()]
                    .iter()
                    .all(|&margin| margin)
            })
            .collect();
        assert_eq!(
            marked,
            [true, true, false, false, false, false, true, false]
        );
    }

    /// A drop cap, the largest type on the page, is not the title, so the
    /// lines above it are not in the margins.
    #[test]
    fn a_drop_cap_is_not_the_title() {
        let lines = [
            ("A Title", 72.0, 80.0, 18.0),
            ("By Ada and Grace", 72.0, 100.0, 10.0),
            ("T", 72.0, 130.0, 30.0),
            ("he text of the paper begins", 100.0, 130.0, 10.0),
        ];
        let text = laid(&lines);
        let rows = rows(&text, &[]);
        let margins = margins(&text, &[], &rows, body_size(&rows));
        let all: String = text.chars.iter().collect();
        let from = all[..all.find("By Ada").unwrap()].chars().count();
        assert!(!margins[from], "the authors are not in the margins");
    }
}
