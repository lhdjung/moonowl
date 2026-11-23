//! What the command line asks for.

use crate::synctex::Forward;

/// The command line, read.
#[derive(Debug, Default, PartialEq)]
pub struct Launch {
    /// `--theme N`: a place in the theme list, which is every theme the app
    /// ships, read out of the `themes/` files. Without it, the last run's
    /// theme is worn.
    pub theme: Option<usize>,
    /// The first document named, for the launch window.
    pub named: Option<String>,
    /// The rest, which go where any document from outside goes.
    pub others: Vec<String>,
    /// `--synctex-forward LINE:COLUMN:FILE.tex FILE.pdf`, from an editor:
    /// show the place that line made in the document. See `synctex.rs`.
    pub forward: Option<Forward>,
}

/// The arguments after the program's name. Every argument that is not a flag
/// is a document, whatever it is called: pdfium decides what a PDF is, and
/// says so if it is not. A flag the app does not know is passed over.
pub fn read(args: &[String]) -> Launch {
    let after = |flag: &str| {
        let at = args.iter().position(|arg| arg == flag)?;
        args.get(at + 1)
    };
    let mut paths = Vec::new();
    let mut given = args.iter();
    while let Some(arg) = given.next() {
        if arg == "--theme" || arg == "--synctex-forward" {
            given.next();
        } else if !arg.starts_with('-') {
            paths.push(crate::config::absolute(arg));
        }
    }
    let mut paths = paths.into_iter();
    let named = paths.next();
    let forward = after("--synctex-forward")
        .zip(named.as_deref())
        .and_then(|(spot, pdf)| Forward::parse(spot, pdf));
    Launch {
        theme: after("--theme").and_then(|value| value.parse().ok()),
        named,
        others: paths.collect(),
        forward,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &[&str]) -> Vec<String> {
        line.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn documents_are_every_argument_that_is_not_a_flag() {
        let launch = read(&args(&[
            "/a.pdf", "--theme", "3", "/b.pdf", "-psn_0_1", "/c",
        ]));
        assert_eq!(launch.theme, Some(3));
        assert_eq!(launch.named.as_deref(), Some("/a.pdf"));
        assert_eq!(launch.others, ["/b.pdf", "/c"]);
        assert_eq!(launch.forward, None);
    }

    #[test]
    fn nothing_named_opens_nothing() {
        assert_eq!(read(&[]), Launch::default());
        // A theme that is not a number is no theme, and is not a document.
        assert_eq!(read(&args(&["--theme", "dark"])), Launch::default());
    }

    #[test]
    fn a_forward_search_names_its_line_and_its_document() {
        let launch = read(&args(&[
            "--synctex-forward",
            "12:4:/p/paper.tex",
            "/p/paper.pdf",
        ]));
        assert_eq!(launch.named.as_deref(), Some("/p/paper.pdf"));
        let forward = launch.forward.expect("a forward search");
        assert_eq!((forward.line, forward.tex.as_str()), (12, "/p/paper.tex"));
        assert_eq!(forward.pdf, "/p/paper.pdf");
        // Without a document, there is nothing to search in.
        assert_eq!(
            read(&args(&["--synctex-forward", "12:4:/p/paper.tex"])).forward,
            None
        );
    }

    #[test]
    fn a_relative_document_is_made_absolute() {
        let named = read(&args(&["paper.pdf"])).named.expect("a document");
        assert!(std::path::Path::new(&named).is_absolute(), "{named}");
    }
}
