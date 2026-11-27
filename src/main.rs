//! `moonowl`: what the Moonowl reader does to documents, from the command
//! line. Every command is a thin layer over `moonowl-core`, which is the
//! same code the reader runs.

use moonowl_core::{arrange, compare, objects, ocr, render, report, synctex};
use std::process::ExitCode;

const USAGE: &str = "\
Usage:
  moonowl combine OUT.pdf IN.pdf...        Put the pages of several files into one.
  moonowl pages IN.pdf PAGES OUT.pdf       Save some pages as a new file, for
                                           example PAGES = 1-3,7,10-.
  moonowl split --chapters IN.pdf FOLDER   Save each chapter as a file of its own,
                                           as the reader's organiser does.
  moonowl compare A.pdf B.pdf REPORT.pdf   Compare two versions word by word and
                                           save the report as a PDF.
  moonowl ocr IN.pdf|FOLDER...             Save a copy of each file whose scanned
                                           pages have searchable text, beside it
                                           as IN-searchable.pdf. A folder means
                                           every PDF in it. Several files are
                                           recognised at once.
      --to OUT.pdf                         The copy's name, for one file.
      --into FOLDER                        Where the copies go, for any number.
  moonowl synctex IN.pdf PAGE X Y          The source line that made a place on a
                                           page (PAGE from 1, X and Y in points
                                           from the top left).
  moonowl synctex IN.pdf FILE.tex LINE     The place in the document that a
                                           source line made.

No command writes over a file that is already there.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The reader runs lopdf's work in a second copy of its own executable,
    // and so does this tool.
    if args.first().map(String::as_str) == Some(objects::APART) {
        return match objects::serve() {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        };
    }
    if let Ok(executable) = std::env::current_exe() {
        objects::work_apart(executable);
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let done = match args.as_slice() {
        ["combine", out, inputs @ ..] if !inputs.is_empty() => combine(out, inputs),
        ["pages", input, pages, out] => pages_of(input, pages, out),
        ["split", "--chapters", input, folder] => split_at_chapters(input, folder),
        ["compare", a, b, out] => compare_into(a, b, out),
        ["ocr", rest @ ..] => ocr_command(rest),
        ["synctex", pdf, tex, line] => synctex_forward(pdf, tex, line),
        ["synctex", pdf, page, x, y] => synctex_inverse(pdf, page, x, y),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match done {
        Ok(said) => {
            if !said.is_empty() {
                println!("{said}");
            }
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("moonowl: {why}");
            ExitCode::FAILURE
        }
    }
}

/// The document at `path`, or what kept it from opening.
fn open(path: &str) -> Result<std::sync::Arc<dyn render::PageSource>, String> {
    render::open(path).map_err(|refused| format!("{path}: {refused}"))
}

/// Every page of every file in `inputs`, in order, written to `out`.
fn combine(out: &str, inputs: &[&str]) -> Result<String, String> {
    let mut sheets = Vec::new();
    for input in inputs {
        let pages = open(input)?.pages();
        sheets.extend((1..=pages).map(|page| arrange::Sheet {
            from: Some(input.to_string()),
            page,
            turn: 0,
        }));
    }
    write_new(&sheets, out)
}

/// The pages `wanted` names, of the file at `input`, written to `out`.
fn pages_of(input: &str, wanted: &str, out: &str) -> Result<String, String> {
    let count = open(input)?.pages();
    let sheets: Vec<arrange::Sheet> = page_list(wanted, count)?
        .into_iter()
        .map(|page| arrange::Sheet {
            from: Some(input.to_string()),
            page,
            turn: 0,
        })
        .collect();
    write_new(&sheets, out)
}

fn write_new(sheets: &[arrange::Sheet], out: &str) -> Result<String, String> {
    let links_lost = arrange::assemble_new(None, sheets, arrange::Layout::One, out, &[])?;
    let pages = sheets.len();
    let mut said = format!("Saved {pages} page{} as {out}.", plural(pages));
    if links_lost {
        said += " Some links led to pages that were left out, and lead nowhere now.";
    }
    Ok(said)
}

/// Pages as a reader types them: "3", "1-4", "10-" (to the end), joined by
/// commas, numbered from 1. Answers the page numbers in the order given.
fn page_list(wanted: &str, count: usize) -> Result<Vec<usize>, String> {
    let number = |text: &str| -> Result<usize, String> {
        match text.trim().parse::<usize>() {
            Ok(page) if (1..=count).contains(&page) => Ok(page),
            _ => Err(format!(
                "\"{text}\" is not a page of this document, which has {count}."
            )),
        }
    };
    let mut pages = Vec::new();
    for part in wanted.split(',') {
        match part.split_once('-') {
            Some((first, last)) => {
                let first = number(first)?;
                let last = if last.trim().is_empty() {
                    count
                } else {
                    number(last)?
                };
                if first > last {
                    return Err(format!("\"{part}\" runs backwards."));
                }
                pages.extend(first..=last);
            }
            None => pages.push(number(part)?),
        }
    }
    Ok(pages)
}

/// Each chapter of `input` written to `folder` as a file of its own, named
/// as the reader names them: its number and its title. A page where one
/// chapter ends and the next begins goes into both files.
fn split_at_chapters(input: &str, folder: &str) -> Result<String, String> {
    let document = open(input)?;
    let chapters = arrange::chapters(&document.outline(), document.pages());
    if chapters.len() < 2 {
        return Err(format!(
            "{input} has no chapters in its contents to split at."
        ));
    }
    let cuts = arrange::cuts(&*document, chapters.clone());
    document.release();
    let digits = chapters.len().to_string().len();
    let parts: Vec<(Vec<arrange::Sheet>, std::path::PathBuf)> = (chapters.iter().enumerate())
        .map(|(at, chapter)| {
            let mut pages = chapter.pages.clone();
            if let Some((_, first, true)) = cuts.get(at + 1) {
                pages.push(*first);
            }
            let sheets = (pages.into_iter())
                .map(|page| arrange::Sheet {
                    from: Some(input.to_string()),
                    page,
                    turn: 0,
                })
                .collect();
            let mut title = arrange::file_safe(&chapter.title);
            if title.is_empty() {
                title = "Chapter".into();
            }
            let name = format!("{:0digits$}-{title}.pdf", at + 1);
            (sheets, std::path::Path::new(folder).join(name))
        })
        .collect();
    if let Some((_, taken)) = parts.iter().find(|(_, to)| to.exists()) {
        return Err(format!("{} is already there.", taken.display()));
    }
    std::fs::create_dir_all(folder).map_err(|e| format!("{folder}: {e}"))?;
    let mut links_lost = false;
    for (written, (sheets, to)) in parts.iter().enumerate() {
        let to = to.to_string_lossy();
        match arrange::assemble_new(None, sheets, arrange::Layout::One, &to, &[]) {
            Ok(lost) => links_lost |= lost,
            // No half of the set is left behind.
            Err(said) => {
                for (_, done) in &parts[..written] {
                    let _ = std::fs::remove_file(done);
                }
                return Err(said);
            }
        }
    }
    let count = parts.len();
    let mut said = format!("Saved {count} chapters in {folder}.");
    if links_lost {
        said += " Some links led to other chapters, and lead nowhere now.";
    }
    Ok(said)
}

/// `a` and `b` compared word by word, and the report saved as `out`.
fn compare_into(a: &str, b: &str, out: &str) -> Result<String, String> {
    if std::path::Path::new(out).exists() {
        return Err(format!("{out} is already there."));
    }
    let (there, here) = (open(a)?, open(b)?);
    let changes = compare::compare(&*here, &*there)?;
    let (there_file, here_file) = (report::File::of(&*there), report::File::of(&*here));
    there.release();
    here.release();
    let lacking = report::save(report::Asked {
        there: there_file,
        here: here_file,
        changes: changes.clone(),
        colours: report::COLOURS,
        to: out.to_string(),
    })?;
    let count = changes.len();
    Ok(format!(
        "{count} difference{}. Saved the comparison as {out}{}",
        plural(count),
        lacking.said()
    ))
}

/// A copy of `input` with the words on its scanned pages written in, as
/// `out`.
/// `moonowl ocr`: the files and folders named, and where each copy goes.
/// A copy is called `IN-searchable.pdf`, the name the reader suggests,
/// beside its file or in the folder `--into` names, unless `--to` names
/// the copy of a single file.
fn ocr_command(args: &[&str]) -> Result<String, String> {
    // One of several files, recognised by a copy of this program that
    // `recognise_all` started.
    if let [ALONE, input, out] = args {
        return recognise(input, out, true);
    }
    let (mut inputs, mut to, mut into) = (Vec::new(), None, None);
    let mut given = args.iter();
    while let Some(&arg) = given.next() {
        match arg {
            "--to" => to = Some(*given.next().ok_or("--to needs the name of the copy.")?),
            "--into" => into = Some(*given.next().ok_or("--into needs a folder.")?),
            flag if flag.starts_with("--") => {
                return Err(format!("{flag} is not an option of ocr."))
            }
            path => inputs.extend(pdfs_in(path)?),
        }
    }
    if inputs.is_empty() {
        return Err("Name the files or folders to recognise.".into());
    }
    if to.is_some() && (into.is_some() || inputs.len() > 1) {
        return Err("--to names the copy of one file. For several, use --into FOLDER.".into());
    }
    if let Some(folder) = into {
        std::fs::create_dir_all(folder).map_err(|e| format!("{folder}: {e}"))?;
    }
    let jobs: Vec<(String, String)> = (inputs.into_iter())
        .map(|input| {
            let beside = arrange::beside(&input, "searchable");
            let out = match (to, into) {
                (Some(to), _) => to.to_string(),
                (None, Some(folder)) => {
                    let name = std::path::Path::new(&beside)
                        .file_name()
                        .unwrap_or_default();
                    std::path::Path::new(folder)
                        .join(name)
                        .to_string_lossy()
                        .into_owned()
                }
                (None, None) => beside,
            };
            (input, out)
        })
        .collect();
    let mut taken = std::collections::HashSet::new();
    if let Some((_, out)) = jobs.iter().find(|(_, out)| !taken.insert(out)) {
        return Err(format!("Two files would both be saved as {out}."));
    }
    match jobs.as_slice() {
        [(input, out)] => recognise(input, out, false),
        _ => recognise_all(&jobs),
    }
}

/// The PDFs `path` names: the file itself, or every PDF directly in a
/// folder, in order of name, without the copies an earlier run made.
fn pdfs_in(path: &str) -> Result<Vec<String>, String> {
    let folder = std::path::Path::new(path);
    if !folder.is_dir() {
        return Ok(vec![path.to_string()]);
    }
    let entries = std::fs::read_dir(folder).map_err(|e| format!("{path}: {e}"))?;
    let mut found: Vec<String> = (entries.flatten())
        .map(|entry| entry.path())
        .filter(|file| file.is_file())
        .filter(|file| {
            file.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
        })
        .filter(|file| {
            !file
                .file_stem()
                .is_some_and(|stem| stem.to_string_lossy().ends_with("-searchable"))
        })
        .map(|file| file.to_string_lossy().into_owned())
        .collect();
    found.sort();
    if found.is_empty() {
        return Err(format!("{path} has no PDFs in it."));
    }
    Ok(found)
}

/// A copy of `input` with the words on its scanned pages written in, as
/// `out`. `alone` is for one file of several, which reads one page at a
/// time, since the other files use the other cores.
fn recognise(input: &str, out: &str, alone: bool) -> Result<String, String> {
    if std::path::Path::new(&out).exists() {
        return Err(format!("{out} is already there."));
    }
    let document = open(input)?;
    let read = document.stamp();
    let found = match alone {
        true => ocr::recognise_alone(&*document, &Default::default()),
        false => ocr::recognise(&*document, &Default::default()),
    }
    .map_err(|why| format!("{input}: {why}"))?;
    document.release();
    let pages = (found.pages.iter())
        .filter(|(_, words)| !words.is_empty())
        .count();
    let failed = match found.failed {
        0 => String::new(),
        n => format!(" {n} page{} could not be read.", plural(n)),
    };
    if pages == 0 {
        return Ok(format!("{input} has no text to recognise.{failed}"));
    }
    ocr::write(input, read, &found.pages, out).map_err(|why| format!("{input}: {why}"))?;
    Ok(format!(
        "Recognised text on {pages} page{}, saved as {out}.{failed}",
        plural(pages)
    ))
}

/// Said to a copy of this program to recognise one file of several.
const ALONE: &str = "--one-of-several";

/// Each file of `jobs` recognised into its copy by a copy of this program
/// of its own, as many at once as the machine has cores. Processes rather
/// than threads, because pdfium answers one call at a time in each
/// process. What each copy says is passed on as it finishes.
fn recognise_all(jobs: &[(String, String)]) -> Result<String, String> {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let program = std::env::current_exe().map_err(|e| format!("this program: {e}"))?;
    let at_once = std::thread::available_parallelism()
        .map_or(1, |cores| cores.get())
        .clamp(1, 8);
    let (next, failed) = (AtomicUsize::new(0), AtomicBool::new(false));
    std::thread::scope(|scope| {
        for _ in 0..at_once.min(jobs.len()) {
            scope.spawn(|| {
                while let Some((input, out)) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let ran = std::process::Command::new(&program)
                        .args(["ocr", ALONE, input, out])
                        .output();
                    match ran {
                        Ok(ran) => {
                            print!("{}", String::from_utf8_lossy(&ran.stdout));
                            eprint!("{}", String::from_utf8_lossy(&ran.stderr));
                            if !ran.status.success() {
                                failed.store(true, Ordering::Relaxed);
                            }
                        }
                        Err(e) => {
                            eprintln!("moonowl: {input}: {e}");
                            failed.store(true, Ordering::Relaxed);
                        }
                    }
                }
            });
        }
    });
    match failed.load(Ordering::Relaxed) {
        true => Err("Some files were not recognised.".into()),
        false => Ok(String::new()),
    }
}

fn synctex_inverse(pdf: &str, page: &str, x: &str, y: &str) -> Result<String, String> {
    let number = |text: &str, what: &str| -> Result<f64, String> {
        text.parse::<f64>()
            .map_err(|_| format!("\"{text}\" is not a number for {what}."))
    };
    let page = number(page, "the page")? as usize;
    if page == 0 {
        return Err("Pages are numbered from 1.".into());
    }
    let (file, line) = synctex::line_at(pdf, page, number(x, "x")?, number(y, "y")?)?;
    Ok(format!("{}:{line}", file.display()))
}

fn synctex_forward(pdf: &str, tex: &str, line: &str) -> Result<String, String> {
    let line: u32 = line
        .parse()
        .map_err(|_| format!("\"{line}\" is not a line number."))?;
    match synctex::place_of(pdf, tex, line)? {
        Some((page, x, y)) => Ok(format!("page {page} at {x:.1} {y:.1}")),
        None => Err(format!("Line {line} of {tex} made nothing in {pdf}.")),
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use super::page_list;

    /// Three chapters. The first ends above the second's heading on page 3,
    /// so page 3 is in both files. The third begins under nothing but the
    /// running head on page 5, so page 5 is in its file alone.
    #[test]
    fn a_document_is_split_at_its_chapters() {
        let book = moonowl_core::fixture::midpage_chapters_pdf();
        let folder = std::env::temp_dir().join(format!("moonowl-cli-split-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let said = super::split_at_chapters(&book, folder.to_str().unwrap()).unwrap();
        assert!(said.starts_with("Saved 3 chapters"), "{said}");
        let pages = |name: &str| {
            moonowl_core::render::open(folder.join(name).to_str().unwrap())
                .unwrap()
                .pages()
        };
        assert_eq!(
            [
                pages("1-Opening.pdf"),
                pages("2-Middle.pdf"),
                pages("3-End.pdf")
            ],
            [3, 2, 2]
        );
        // A second time, nothing is written over.
        assert!(super::split_at_chapters(&book, folder.to_str().unwrap()).is_err());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn pages_are_read_as_a_reader_types_them() {
        assert_eq!(page_list("1-3,7,9-", 10), Ok(vec![1, 2, 3, 7, 9, 10]));
        assert_eq!(page_list(" 2 ", 10), Ok(vec![2]));
        assert!(page_list("0", 10).is_err());
        assert!(page_list("11", 10).is_err());
        assert!(page_list("5-3", 10).is_err());
    }
}
