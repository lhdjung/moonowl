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
  moonowl compare A.pdf B.pdf REPORT.pdf   Compare two versions word by word and
                                           save the report as a PDF.
  moonowl ocr IN.pdf OUT.pdf               Save a copy whose scanned pages have
                                           searchable text.
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
        ["compare", a, b, out] => compare_into(a, b, out),
        ["ocr", input, out] => recognise(input, out),
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
fn recognise(input: &str, out: &str) -> Result<String, String> {
    if std::path::Path::new(out).exists() {
        return Err(format!("{out} is already there."));
    }
    let document = open(input)?;
    let read = document.stamp();
    let found = ocr::recognise(&*document, &Default::default())?;
    document.release();
    let pages = (found.pages.iter())
        .filter(|(_, words)| !words.is_empty())
        .count();
    let failed = match found.failed {
        0 => String::new(),
        n => format!(" {n} page{} could not be read.", plural(n)),
    };
    if pages == 0 {
        return Ok(format!("No text to recognise.{failed}"));
    }
    ocr::write(input, read, &found.pages, out)?;
    Ok(format!(
        "Recognised text on {pages} page{}, saved as {out}.{failed}",
        plural(pages)
    ))
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

    #[test]
    fn pages_are_read_as_a_reader_types_them() {
        assert_eq!(page_list("1-3,7,9-", 10), Ok(vec![1, 2, 3, 7, 9, 10]));
        assert_eq!(page_list(" 2 ", 10), Ok(vec![2]));
        assert!(page_list("0", 10).is_err());
        assert!(page_list("11", 10).is_err());
        assert!(page_list("5-3", 10).is_err());
    }
}
