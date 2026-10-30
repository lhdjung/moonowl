//! References that have no link behind them, such as "[12]" or "Figure 3",
//! and the line each one leads to: the entry in the bibliography, the
//! caption, the heading or the equation.
//!
//! Only the patterns a reader can predict are matched: a number in brackets,
//! and "Figure", "Table", "Section", "Equation" or "Theorem" (or their usual
//! short forms) with a number after it.

use crate::render::PageText;

/// What a reference names: a number in brackets when `word` is empty, or a
/// word and a number, with the word always in its long form.
#[derive(Clone, Debug, PartialEq)]
pub struct Reference {
    pub word: &'static str,
    pub number: String,
    /// Where it was found on its page, as a range of characters.
    pub from: usize,
    pub to: usize,
}

impl Reference {
    /// How the reference is written in a sentence, such as "[12]" or
    /// "Figure 3".
    pub fn said(&self) -> String {
        match self.word {
            "" => format!("[{}]", self.number),
            word => format!("{word} {}", self.number),
        }
    }
}

/// Each word a reference can begin with, and the ways it is written. The
/// first spelling is the long form.
const WORDS: &[&[&str]] = &[
    &["Figure", "Fig."],
    &["Table", "Tab."],
    &["Section", "Sec.", "§"],
    &["Equation", "Eq.", "Eqn."],
    &["Theorem", "Thm."],
];

/// The reference at character `caret` of a page, if there is one there.
pub fn at(text: &PageText, caret: usize) -> Option<Reference> {
    let chars = &text.chars;
    let (start, end) = line(chars, caret.min(chars.len().saturating_sub(1)));
    let holds = |from: usize, to: usize| (from..=to).contains(&caret);
    // A number in brackets. In a list such as "[3, 12]", the number under
    // the caret is the one meant, or the first one when the caret is on a
    // bracket.
    let mut at = start;
    while let Some(open) = (at..end).find(|&i| chars[i] == '[') {
        let Some(close) = (open + 1..end).find(|&i| chars[i] == ']') else {
            break;
        };
        let numbers = numbers_in(chars, open + 1, close);
        let under = (numbers.iter())
            .find(|(_, from, to)| holds(*from, *to))
            .or(numbers.first());
        if let (true, Some((number, from, to))) = (holds(open, close + 1), under) {
            let alone = numbers.len() == 1;
            return Some(Reference {
                word: "",
                number: number.clone(),
                from: if alone { open } else { *from },
                to: if alone { close + 1 } else { *to },
            });
        }
        at = close + 1;
    }
    // A word and a number.
    for at in start..end {
        if at > start && chars[at - 1].is_alphanumeric() {
            continue;
        }
        let Some((word, after)) = word_at(chars, at, end) else {
            continue;
        };
        let mut from = after;
        while from < end && chars[from].is_whitespace() {
            from += 1;
        }
        let bracketed = from < end && chars[from] == '(';
        if bracketed {
            from += 1;
        }
        let Some((number, to)) = number_at(chars, from, end) else {
            continue;
        };
        let to = if bracketed && to < end && chars[to] == ')' {
            to + 1
        } else {
            to
        };
        if holds(at, to) {
            return Some(Reference {
                word,
                number,
                from: at,
                to,
            });
        }
    }
    None
}

/// Where on a page the reference leads: the character that begins each
/// line it could lead to, in order. The line is one that begins with the
/// reference, such as "[12] A. Author…" or "Figure 3: …", except for a
/// section, whose heading begins with its number, and an equation, whose
/// line ends with its number in brackets.
pub fn leads_to(text: &PageText, reference: &Reference) -> Vec<usize> {
    let chars = &text.chars;
    let mut out = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let (from, end) = line(chars, start);
        start = end + 1;
        let Some(from) = (from..end).find(|&i| !chars[i].is_whitespace()) else {
            continue;
        };
        let found = match reference.word {
            "" => begins_with(chars, from, end, &format!("[{}]", reference.number)).then_some(from),
            "Equation" => {
                let wanted = format!("({})", reference.number);
                let last = (from..end).rev().find(|&i| !chars[i].is_whitespace());
                let first = last.and_then(|last| (last + 1).checked_sub(wanted.chars().count()));
                first.filter(|&first| first >= from && begins_with(chars, first, end, &wanted))
            }
            "Section" => {
                let numbered = begins_with(chars, from, end, &reference.number)
                    && ends_a_heading_number(chars, from + reference.number.chars().count(), end);
                (numbered || caption(chars, from, end, reference)).then_some(from)
            }
            _ => caption(chars, from, end, reference).then_some(from),
        };
        out.extend(found);
    }
    out
}

/// Whether another line on the page than the one beginning at `from` begins
/// with a number in brackets. An entry of a bibliography has others like it
/// on its page; a sentence that happens to wrap just before "[12]" seldom
/// does.
pub fn listed(text: &PageText, from: usize) -> bool {
    let chars = &text.chars;
    let mut start = 0;
    while start < chars.len() {
        let (begins, end) = line(chars, start);
        start = end + 1;
        let Some(first) = (begins..end).find(|&i| !chars[i].is_whitespace()) else {
            continue;
        };
        let digits = (first + 1..end)
            .take_while(|&i| chars[i].is_ascii_digit())
            .count();
        if first != from
            && chars[first] == '['
            && digits > 0
            && chars.get(first + 1 + digits) == Some(&']')
        {
            return true;
        }
    }
    false
}

/// The line `at` is on, as the range of characters between two of the line
/// breaks pdfium writes.
fn line(chars: &[char], at: usize) -> (usize, usize) {
    let is_break = |c: char| c == '\r' || c == '\n';
    let start = (0..at.min(chars.len()))
        .rev()
        .find(|&i| is_break(chars[i]))
        .map_or(0, |i| i + 1);
    let end = (at..chars.len())
        .find(|&i| is_break(chars[i]))
        .unwrap_or(chars.len());
    (start, end)
}

/// The word of [`WORDS`] that begins at `at`, in its long form, and where it
/// ends. Case is ignored, since some captions are set in capitals.
fn word_at(chars: &[char], at: usize, end: usize) -> Option<(&'static str, usize)> {
    WORDS.iter().find_map(|spellings| {
        spellings.iter().find_map(|spelling| {
            let to = at + spelling.chars().count();
            let same = to <= end
                && chars[at..to]
                    .iter()
                    .zip(spelling.chars())
                    .all(|(a, b)| a.to_lowercase().eq(b.to_lowercase()));
            // "Figure" must not be the start of "Figures" or "Figured".
            let whole = || {
                spelling.ends_with('.')
                    || *spelling == "§"
                    || to == end
                    || !chars[to].is_alphanumeric()
            };
            (same && whole()).then_some((spellings[0], to))
        })
    })
}

/// A number such as "3", "3.2" or "A.1" beginning at `at`, and where it
/// ends. A full stop after it is not part of it.
fn number_at(chars: &[char], at: usize, end: usize) -> Option<(String, usize)> {
    let mut to = at;
    while to < end && (chars[to].is_alphanumeric() || chars[to] == '.') {
        to += 1;
    }
    while to > at && chars[to - 1] == '.' {
        to -= 1;
    }
    let number: String = chars[at..to].iter().collect();
    let digits = number.chars().filter(char::is_ascii_digit).count();
    // A word with a digit in it is not a number.
    let letters = number.chars().filter(|c| c.is_alphabetic()).count();
    (digits > 0 && letters <= 1 && to - at <= 8).then_some((number, to))
}

/// The numbers in brackets between `from` and `to`, each with its range:
/// "3, 12" is two of them. A range such as "3–5" is its first number and
/// its last. Brackets that hold anything but numbers and the marks between
/// them, such as "[Knu84]" or "[0.5, 1]", or that hold a 0, which no list
/// of references begins with, such as the interval "[0, 1]", hold none.
fn numbers_in(chars: &[char], from: usize, to: usize) -> Vec<(String, usize, usize)> {
    let between = |c: char| matches!(c, ',' | ';' | '-' | '\u{2013}') || c.is_whitespace();
    let mut out = Vec::new();
    let mut at = from;
    while at < to {
        if between(chars[at]) {
            at += 1;
            continue;
        }
        if !chars[at].is_ascii_digit() {
            return Vec::new();
        }
        let mut end = at;
        while end < to && chars[end].is_ascii_alphanumeric() {
            end += 1;
        }
        let number: String = chars[at..end].iter().collect();
        if number.trim_start_matches('0').is_empty() {
            return Vec::new();
        }
        out.push((number, at, end));
        at = end;
    }
    out
}

fn begins_with(chars: &[char], from: usize, end: usize, wanted: &str) -> bool {
    let wanted: Vec<char> = wanted.chars().collect();
    from + wanted.len() <= end && chars[from..from + wanted.len()] == wanted[..]
}

/// Whether a line that begins with a reference's word and number is a
/// caption or the head of a theorem rather than a sentence that happens to
/// begin with them: the number is followed by the end of the line, by a
/// colon, a full stop or a bracket, or by a capital letter, as in
/// "Fig. 3 Results of the trial".
fn caption(chars: &[char], from: usize, end: usize, reference: &Reference) -> bool {
    let Some((word, after)) = word_at(chars, from, end) else {
        return false;
    };
    let mut at = after;
    while at < end && chars[at].is_whitespace() {
        at += 1;
    }
    let Some((number, to)) = number_at(chars, at, end) else {
        return false;
    };
    word == reference.word && number == reference.number && ends_a_caption_number(chars, to, end)
}

fn ends_a_caption_number(chars: &[char], at: usize, end: usize) -> bool {
    if at == end || matches!(chars[at], ':' | '.' | '(' | '|' | '—' | '–') {
        return true;
    }
    chars[at].is_whitespace() && next_is_capital(chars, at, end)
}

/// Whether a line that begins with a section's number goes on as a heading
/// does: a space, or a full stop and a space, and then a capital letter.
fn ends_a_heading_number(chars: &[char], at: usize, end: usize) -> bool {
    let at = if at < end && chars[at] == '.' {
        at + 1
    } else {
        at
    };
    at < end && chars[at].is_whitespace() && next_is_capital(chars, at, end)
}

fn next_is_capital(chars: &[char], at: usize, end: usize) -> bool {
    (at..end)
        .find(|&i| !chars[i].is_whitespace())
        .is_some_and(|i| chars[i].is_uppercase())
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

    fn under(text: &PageText, what: &str) -> Option<Reference> {
        let all: String = text.chars.iter().collect();
        let byte = all.find(what).expect("the words are on the page");
        at(text, all[..byte].chars().count() + 1)
    }

    #[test]
    fn a_number_in_brackets_is_the_one_under_the_caret() {
        let text = page("as [3] found, and others [4, 12] did too");
        assert_eq!(under(&text, "[3]").unwrap().said(), "[3]");
        assert_eq!(under(&text, "12]").unwrap().said(), "[12]");
        assert_eq!(under(&text, "[4,").unwrap().said(), "[4]");
        assert_eq!(under(&text, "found"), None);
    }

    #[test]
    fn brackets_that_hold_more_than_numbers_are_not_references() {
        let text = page("for t in [0, 1] and [0.5, 2], as [Knu84] and [3a] say");
        assert_eq!(under(&text, "1]"), None);
        assert_eq!(under(&text, "2]"), None);
        assert_eq!(under(&text, "84]"), None);
        assert_eq!(under(&text, "[3a]").unwrap().said(), "[3a]");
    }

    #[test]
    fn a_word_and_a_number_is_said_in_its_long_form() {
        let text = page("see Fig. 3.2 and Eq. (7), Theorem 2.");
        assert_eq!(under(&text, "Fig.").unwrap().said(), "Figure 3.2");
        assert_eq!(under(&text, "(7)").unwrap().said(), "Equation 7");
        assert_eq!(under(&text, "Theorem").unwrap().said(), "Theorem 2");
        assert_eq!(under(&page("Figures 3 and 4"), "Figures"), None);
    }

    #[test]
    fn a_reference_leads_to_the_line_that_begins_with_it() {
        let text = page(
            "Figure 2 shows nothing here.\r\n[30] Not this one.\r\n[3] The one.\r\n\
             Figure 2: The caption.\r\n3 Methods\r\nE = mc^2 (7)\r\n",
        );
        let lands = |what: &str| {
            let reference = under(&page(&format!("x {what} y")), what).unwrap();
            leads_to(&text, &reference)
                .first()
                .map(|&at| text.chars[at..at + 4].iter().collect::<String>())
        };
        assert_eq!(lands("[3]").as_deref(), Some("[3] "));
        assert_eq!(lands("Fig. 2").as_deref(), Some("Figu"));
        let caption = text.chars.iter().collect::<String>().find("Figure 2:");
        let figure = under(&page("x Figure 2 y"), "Figure").unwrap();
        assert_eq!(leads_to(&text, &figure).first().copied(), caption);
        assert_eq!(lands("Section 3").as_deref(), Some("3 Me"));
        assert_eq!(lands("Eq. (7)").as_deref(), Some("(7)\r"));
        assert_eq!(lands("Table 2"), None);
    }

    #[test]
    fn an_entry_of_a_bibliography_has_others_like_it() {
        let text = page("[11] A. Author.\r\n[12] B. Author.\r\n");
        assert!(listed(&text, 17));
        let text = page("as the work of\r\n[12] showed, it is so.\r\n");
        assert!(!listed(&text, 16));
    }
}
