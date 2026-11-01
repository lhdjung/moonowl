//! SyncTeX: which line of the LaTeX source made which part of a page.
//!
//! LaTeX run with `-synctex=1` writes `paper.synctex.gz` beside
//! `paper.pdf`. It lists, page by page, the boxes TeX made and the places
//! between words, each with the file and the line it came from, in scaled
//! points from the page's top left. A click on the page with ⇧⌘ goes from
//! a place to a line ([`line_at`]) and opens the editor there; an editor
//! asks for the other way with `moonowl --synctex-forward LINE:COLUMN:FILE
//! PDF` ([`place_of`]).
//!
//! The file is read a line at a time and never held whole, since a long
//! book's runs to tens of megabytes once unpacked.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Where a document's SyncTeX file is, if LaTeX wrote one.
pub fn beside(pdf: &str) -> Option<PathBuf> {
    let pdf = Path::new(pdf);
    ["synctex.gz", "synctex"]
        .into_iter()
        .map(|extension| pdf.with_extension(extension))
        .find(|path| path.is_file())
}

/// Said when there is no SyncTeX file beside the document.
pub const MISSING: &str =
    "There is no SyncTeX file beside this document. Run LaTeX with -synctex=1 to make one.";

/// One record of the file: a box, or a place between words.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Record {
    kind: char,
    tag: usize,
    line: u32,
    /// Where it is on the page, in points from the top left: the left end
    /// of its baseline.
    left: f64,
    baseline: f64,
    /// A box's width, height above the baseline and depth below it. A
    /// place between words has none.
    width: f64,
    height: f64,
    depth: f64,
}

impl Record {
    fn is_line_box(&self) -> bool {
        matches!(self.kind, '(' | 'h')
    }

    fn is_between_words(&self) -> bool {
        matches!(self.kind, 'x' | 'k' | 'g' | '$')
    }

    fn holds(&self, x: f64, y: f64) -> bool {
        (self.left..=self.left + self.width).contains(&x)
            && (self.baseline - self.height..=self.baseline + self.depth).contains(&y)
    }
}

/// The file, read a record at a time: each record with the page it is on,
/// and every input file named so far by its tag.
struct Reader {
    lines: Box<dyn BufRead>,
    /// From the file's units to PDF points: its `Unit:`, in scaled points.
    unit: f64,
    /// The file's `Magnification:`, which scales every place but not the
    /// offsets.
    magnified: f64,
    /// The file's `X Offset:` and `Y Offset:`, in its units, which are
    /// added to every place, as SyncTeX's own parser adds them.
    offset: (f64, f64),
    page: usize,
    inputs: Vec<(usize, String)>,
}

/// Scaled points to a PDF point: 65,536 scaled points to a TeX point, and
/// 72.27 TeX points to the 72 points of an inch a PDF counts in.
const SCALED: f64 = 72.0 / 72.27 / 65536.0;

impl Reader {
    fn open(path: &Path) -> Result<Reader, String> {
        let file = std::fs::File::open(path)
            .map_err(|e| format!("The SyncTeX file could not be read: {e}."))?;
        let lines: Box<dyn BufRead> = if path.extension().is_some_and(|e| e == "gz") {
            Box::new(std::io::BufReader::new(flate2::read::GzDecoder::new(file)))
        } else {
            Box::new(std::io::BufReader::new(file))
        };
        Ok(Reader::of(lines))
    }

    fn of(lines: Box<dyn BufRead>) -> Reader {
        Reader {
            lines,
            unit: SCALED,
            magnified: 1.0,
            offset: (0.0, 0.0),
            page: 0,
            inputs: Vec::new(),
        }
    }

    /// The next record and its page, or `None` at the end of the file.
    fn next(&mut self) -> Option<(usize, Record)> {
        let mut line = String::new();
        loop {
            line.clear();
            if self.lines.read_line(&mut line).ok()? == 0 {
                return None;
            }
            let line = line.trim_end();
            if let Some(input) = line.strip_prefix("Input:") {
                if let Some((tag, path)) = input.split_once(':') {
                    if let Ok(tag) = tag.parse() {
                        self.inputs.push((tag, path.to_string()));
                    }
                }
            } else if let Some(unit) = line.strip_prefix("Unit:") {
                self.unit *= unit.parse::<f64>().unwrap_or(1.0);
            } else if let Some(magnified) = line.strip_prefix("Magnification:") {
                self.magnified = magnified.parse::<f64>().unwrap_or(1000.0) / 1000.0;
            } else if let Some(x) = line.strip_prefix("X Offset:") {
                self.offset.0 = x.parse().unwrap_or(0.0);
            } else if let Some(y) = line.strip_prefix("Y Offset:") {
                self.offset.1 = y.parse().unwrap_or(0.0);
            } else if let Some(page) = line.strip_prefix('{') {
                self.page = page.parse().unwrap_or(self.page + 1);
            } else if let Some(record) = self.record(line) {
                return Some((self.page, record));
            }
        }
    }

    /// A line such as `x1,4:11280787,10300473` or
    /// `(1,5:8799518,10300473:22609920,455111,0`.
    fn record(&self, line: &str) -> Option<Record> {
        let kind = line.chars().next()?;
        if !matches!(kind, '[' | '(' | 'v' | 'h' | 'x' | 'k' | 'g' | '$') || self.page == 0 {
            return None;
        }
        let mut parts = line[kind.len_utf8()..].split(':');
        let numbers = |part: Option<&str>| -> Vec<f64> {
            part.unwrap_or("")
                .split(',')
                .filter_map(|n| n.parse().ok())
                .collect()
        };
        let source = numbers(parts.next());
        let place = numbers(parts.next());
        let size = numbers(parts.next());
        if place.len() < 2 {
            return None;
        }
        let scale = self.unit * self.magnified;
        let at = |numbers: &[f64], i: usize| numbers.get(i).copied().unwrap_or(0.0) * scale;
        Some(Record {
            kind,
            tag: *source.first()? as usize,
            line: *source.get(1)? as u32,
            left: at(&place, 0) + self.offset.0 * self.unit,
            baseline: at(&place, 1) + self.offset.1 * self.unit,
            width: at(&size, 0),
            height: if kind == 'k' { 0.0 } else { at(&size, 1) },
            depth: if kind == 'k' { 0.0 } else { at(&size, 2) },
        })
    }

    /// The file the tag names, if it is there. TeX names a file from the
    /// folder it was run in. That is usually the document's folder, but a
    /// document compiled into a folder of its own (latexmk's `-outdir`) is
    /// one below it, and the folder of a file the SyncTeX file names in
    /// full, such as the main file, is then the likelier one.
    fn input(&self, tag: usize, pdf: &Path) -> Option<PathBuf> {
        let (_, named) = self.inputs.iter().find(|(t, _)| *t == tag)?;
        let named_in_full = (self.inputs.iter())
            .map(|(_, path)| Path::new(path))
            .filter(|path| path.is_absolute())
            .filter_map(Path::parent);
        let path = std::iter::once(pdf.parent().unwrap_or(Path::new(".")))
            .chain(named_in_full)
            .map(|folder| folder.join(named))
            .find(|path| path.is_file())?;
        Some(std::fs::canonicalize(&path).unwrap_or(path))
    }
}

/// The source line that made the place `x`, `y` on page `page` (one-based),
/// in points from the page's top left.
///
/// A line of a paragraph is a box tagged with the line the paragraph ends
/// on, and the places between its words carry the lines the words were
/// typed on. So the place is looked for in the smallest line box that holds
/// it, and the line is the one of the last place between words before it in
/// that box, or the box's own when there is none.
pub fn line_at(pdf: &str, page: usize, x: f64, y: f64) -> Result<(PathBuf, u32), String> {
    let path = beside(pdf).ok_or(MISSING)?;
    let mut reader = Reader::open(&path)?;
    let mut records = Vec::new();
    while let Some((on, record)) = reader.next() {
        if on == page {
            records.push(record);
        } else if on > page {
            break;
        }
    }
    let area = |r: &Record| r.width * (r.height + r.depth);
    let distance = |r: &Record| {
        let gap = |v: f64, from: f64, to: f64| (from - v).max(v - to).max(0.0);
        gap(y, r.baseline - r.height, r.baseline + r.depth) * 1000.0
            + gap(x, r.left, r.left + r.width)
    };
    let boxes = records.iter().filter(|r| r.is_line_box());
    let line_box = (boxes.clone().filter(|r| r.holds(x, y)))
        .min_by(|a, b| area(a).total_cmp(&area(b)))
        .or_else(|| boxes.min_by(|a, b| distance(a).total_cmp(&distance(b))))
        .ok_or("SyncTeX says nothing about this page.")?;
    // A paragraph's indent is an empty box of its own, with the line of
    // the paragraph's first word.
    let found = (records.iter())
        .filter(|r| r.is_between_words() || (r.kind == 'h' && !std::ptr::eq(*r, line_box)))
        .filter(|r| line_box.holds(r.left, r.baseline) && r.left <= x)
        .max_by(|a, b| a.left.total_cmp(&b.left))
        .unwrap_or(line_box);
    let source = reader
        .input(found.tag, Path::new(pdf))
        .ok_or("The source file of this place is not where the SyncTeX file says.")?;
    Ok((source, found.line))
}

/// Where line `line` of the source file `tex` is in the document: the page
/// (one-based) and the left end of its baseline, in points from the page's
/// top left. A line that made nothing on a page, such as an empty one, is
/// taken to mean the next line that did.
pub fn place_of(pdf: &str, tex: &str, line: u32) -> Result<Option<(usize, f64, f64)>, String> {
    let path = beside(pdf).ok_or(MISSING)?;
    let mut reader = Reader::open(&path)?;
    let wanted = std::fs::canonicalize(tex).unwrap_or_else(|_| PathBuf::from(tex));
    let named = wanted.file_name().map(|name| name.to_os_string());
    let pdf = Path::new(pdf);
    // How each tag names the file, decided once per tag: by its path (0),
    // by its name alone (1), or not at all. A file with the same name in
    // another folder counts only when none is named by its path.
    let mut tags: Vec<(usize, Option<usize>)> = Vec::new();
    // For each way of naming it, the nearest line at or after the one
    // asked for, and the last before.
    let mut after: [Option<(u32, usize, f64, f64)>; 2] = [None; 2];
    let mut before: [Option<(u32, usize, f64, f64)>; 2] = [None; 2];
    while let Some((page, record)) = reader.next() {
        if !(record.is_line_box() || record.is_between_words()) {
            continue;
        }
        let ours = match tags.iter().find(|(tag, _)| *tag == record.tag) {
            Some(&(_, ours)) => ours,
            None => {
                let input = reader.input(record.tag, pdf);
                let ours = input.and_then(|input| {
                    if input == wanted {
                        Some(0)
                    } else if named.is_some() && input.file_name() == named.as_deref() {
                        Some(1)
                    } else {
                        None
                    }
                });
                tags.push((record.tag, ours));
                ours
            }
        };
        let Some(ours) = ours else {
            continue;
        };
        let found = (record.line, page, record.left, record.baseline);
        let (after, before) = (&mut after[ours], &mut before[ours]);
        if record.line >= line && after.is_none_or(|(best, ..)| record.line < best) {
            *after = Some(found);
        } else if record.line < line && before.is_none_or(|(best, ..)| record.line > best) {
            *before = Some(found);
        }
    }
    let found = (0..2).find_map(|ours| after[ours].or(before[ours]));
    Ok(found.map(|(_, page, x, y)| (page, x, y)))
}

/* --------------------------------------------------------------- editors */

/// The editors Settings offers, by name, with the command each is opened
/// with. `%file` and `%line` are put in when it is run.
pub const EDITORS: &[(&str, &str)] = &[
    ("VS Code", "code --goto %file:%line"),
    ("Zed", "zed %file:%line"),
    ("Sublime Text", "subl %file:%line"),
    ("TeXstudio", "texstudio --line %line %file"),
    ("Emacs", "emacsclient --no-wait +%line %file"),
];

/// The command an editor is opened with, as the program and its
/// arguments. Words are split at spaces, and a word in double quotes keeps
/// its spaces. The file goes in as one argument, whatever is in its name.
pub fn command(template: &str, file: &Path, line: u32) -> Vec<String> {
    let mut words = Vec::new();
    let (mut word, mut quoted, mut begun) = (String::new(), false, false);
    for c in template.chars() {
        match c {
            '"' => (quoted, begun) = (!quoted, true),
            c if c.is_whitespace() && !quoted => {
                if begun {
                    words.push(std::mem::take(&mut word));
                }
                begun = false;
            }
            c => {
                word.push(c);
                begun = true;
            }
        }
    }
    if begun {
        words.push(word);
    }
    let file = file.to_string_lossy();
    words
        .into_iter()
        .map(|word| {
            // The line first, so that a file whose name has "%line" in it
            // keeps its name.
            word.replace("%line", &line.to_string())
                .replace("%file", &file)
        })
        .collect()
}

/// Start the editor, and leave it running. An app opened from the Finder
/// or the Dock has only the system's own folders on its `PATH`, so the
/// folders that command-line tools are usually put in are added.
pub fn launch(command: &[String]) -> Result<(), String> {
    let Some((program, arguments)) = command.split_first() else {
        return Err("Choose the editor SyncTeX opens in Settings, under Reading.".into());
    };
    let mut path = std::env::var_os("PATH").unwrap_or_default();
    if cfg!(unix) {
        let home = std::env::var("HOME").unwrap_or_default();
        let more = [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            &format!("{home}/.local/bin"),
        ];
        let mut folders: Vec<PathBuf> = std::env::split_paths(&path).collect();
        folders.extend(more.iter().map(PathBuf::from));
        path = std::env::join_paths(folders).unwrap_or(path);
    }
    let mut child = std::process::Command::new(program)
        .args(arguments)
        .env("PATH", path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("The editor could not be started ({program}: {e})."))?;
    // Waited for on a thread of its own, so that it does not linger as a
    // finished process the system keeps a record of.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/* ------------------------------------------------- asked for by an editor */

/// What an editor asks for: show the place that line `line` of `tex`
/// made in the document `pdf`.
#[derive(Clone, Debug, PartialEq)]
pub struct Forward {
    pub pdf: String,
    pub tex: String,
    pub line: u32,
}

/// What begins a request on the single-instance socket, where everything
/// else that arrives is a path. No path begins with a NUL.
const ASKS: &str = "\0synctex\0";

impl Forward {
    /// `LINE:COLUMN:FILE`, as `--synctex-forward` takes it, and the
    /// document. The column is not used.
    pub fn parse(spot: &str, pdf: &str) -> Option<Forward> {
        let mut parts = spot.splitn(3, ':');
        let line = parts.next()?.trim().parse().ok()?;
        let _column = parts.next()?;
        let tex = parts.next().filter(|tex| !tex.is_empty())?;
        Some(Forward {
            pdf: crate::config::absolute(pdf),
            tex: crate::config::absolute(tex),
            line,
        })
    }

    /// The request as the socket carries it.
    pub fn said(&self) -> String {
        format!("{ASKS}{}\0{}\0{}", self.line, self.tex, self.pdf)
    }

    /// A request the socket carried, or `None` for a path.
    pub fn heard(said: &str) -> Option<Forward> {
        let mut parts = said.strip_prefix(ASKS)?.split('\0');
        Some(Forward {
            line: parts.next()?.parse().ok()?,
            tex: parts.next()?.to_string(),
            pdf: parts.next()?.to_string(),
        })
    }
}

/// A request waiting for its document to open: the window that opens it
/// takes it up. Each request has a number, so that a window can let go of
/// the one it was given without letting go of a newer one. See
/// `Viewer::take_forward`.
static WAITING: Mutex<Option<(u64, Forward)>> = Mutex::new(None);
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn wait(forward: Forward) {
    let number = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    *WAITING.lock().unwrap_or_else(|e| e.into_inner()) = Some((number, forward));
}

/// The number of the request waiting for some document, if one is.
pub fn waiting() -> Option<u64> {
    (WAITING.lock().unwrap_or_else(|e| e.into_inner()).as_ref()).map(|(number, _)| *number)
}

/// Whether the request waiting is likely for the document at `pdf`: the
/// two have the same file name. The disk is not asked, since this is asked
/// on the thread that draws the window, and the answer only decides what is
/// said.
pub fn waiting_for(pdf: &str) -> bool {
    let name = |path: &str| std::path::Path::new(path).file_name().map(|n| n.to_owned());
    (WAITING.lock().unwrap_or_else(|e| e.into_inner()).as_ref())
        .is_some_and(|(_, wanted)| name(&wanted.pdf).is_some() && name(&wanted.pdf) == name(pdf))
}

/// The request waiting for the document at `pdf` is dropped, if one is:
/// the document did not open, so nobody will take it up. The paths are
/// compared as they are written.
pub fn forget_for(pdf: &str) {
    let mut held = WAITING.lock().unwrap_or_else(|e| e.into_inner());
    if (held.as_ref()).is_some_and(|(_, wanted)| wanted.pdf == pdf) {
        *held = None;
    }
}

/// The request numbered `number` is dropped, if it is still waiting and is
/// for the document at `pdf`. The paths are compared as they are written,
/// since this is asked on the thread that draws the window.
pub fn forget(number: u64, pdf: &str) {
    let mut held = WAITING.lock().unwrap_or_else(|e| e.into_inner());
    if (held.as_ref()).is_some_and(|(held, wanted)| *held == number && wanted.pdf == pdf) {
        *held = None;
    }
}

/// The waiting request, if it is for the document at `pdf`. The two paths
/// are compared as the disk resolves them, so this belongs off the thread
/// that draws the window.
pub fn take_for(pdf: &str) -> Option<Forward> {
    let (number, wanted) = WAITING.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
    let same = wanted.pdf == pdf
        || (std::fs::canonicalize(&wanted.pdf).ok())
            .is_some_and(|a| Some(a) == std::fs::canonicalize(pdf).ok());
    if pdf.is_empty() || !same {
        return None;
    }
    // Taken only if it is still the request that was compared.
    let mut held = WAITING.lock().unwrap_or_else(|e| e.into_inner());
    (held.as_ref().map(|(held, _)| *held) == Some(number))
        .then(|| held.take().map(|(_, forward)| forward))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header's offsets are added to every place, and are not
    /// magnified, as in SyncTeX's own parser.
    #[test]
    fn the_offsets_in_the_header_move_every_place() {
        let file = "SyncTeX Version:1\nInput:1:a.tex\nOutput:pdf\nMagnification:2000\n\
                    Unit:1\nX Offset:65781\nY Offset:-65781\nContent:\n{1\n\
                    h1,3:65781,131563:0,0,0\n";
        let mut reader = Reader::of(Box::new(std::io::Cursor::new(file.as_bytes().to_vec())));
        let (page, record) = reader.next().expect("a record");
        assert_eq!(page, 1);
        assert!((record.left - 3.0).abs() < 0.01, "{record:?}");
        assert!((record.baseline - 3.0).abs() < 0.01, "{record:?}");
    }

    #[test]
    fn a_command_keeps_a_file_name_with_spaces_whole() {
        let file = Path::new("/My Papers/paper.tex");
        assert_eq!(
            command("code --goto %file:%line", file, 12),
            ["code", "--goto", "/My Papers/paper.tex:12"]
        );
        assert_eq!(
            command("\"/Applications/My Editor\" -l %line \"%file\"", file, 3),
            ["/Applications/My Editor", "-l", "3", "/My Papers/paper.tex"]
        );
        assert!(command("  ", file, 1).is_empty());
    }

    #[test]
    fn a_request_crosses_the_socket_whole() {
        let forward = Forward::parse("12:0:/a/paper.tex", "/a/paper.pdf").unwrap();
        assert_eq!(forward.line, 12);
        assert_eq!(Forward::heard(&forward.said()), Some(forward));
        assert_eq!(Forward::heard("/a/paper.pdf"), None);
        assert_eq!(
            Forward::parse("twelve:0:/a/paper.tex", "/a/paper.pdf"),
            None
        );
    }
}
