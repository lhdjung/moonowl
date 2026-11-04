//! The Viewer and "Compare with…": another version of the document read
//! and compared on a thread, the changes painted on the pages, and the
//! Differences tab that lists them. `crate::compare` does the comparing.

use super::*;

/// The other file, and where the document's words differ from its words.
/// Neither is taken to be the earlier one: the other file may be a later
/// version, or not a version of the document at all.
pub struct Comparing {
    /// The other file's name, as the Differences tab says it.
    pub name: String,
    /// The other file, and what it says about itself.
    other: crate::report::File,
    pub changes: Vec<crate::compare::Change>,
    /// The change the reader went to last, from the list.
    pub at: Option<usize>,
    /// That change in full, as the Differences tab shows it under its row.
    pub open: Option<Rc<OpenChange>>,
    /// What the Differences tab lists, made once, since the sidebar is worked
    /// out again on every scroll frame.
    pub rows: Rc<Vec<ChangeRow>>,
    /// Each page's changes, as (change, from, to), so that drawing a page
    /// looks only at its own.
    on_page: HashMap<usize, Vec<(usize, usize, usize)>>,
}

/// A row of the Differences tab: the page (one-based), what the page is
/// called, and the words of the other file and of the document, cut short.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeRow {
    pub page: usize,
    pub label: String,
    pub there: String,
    pub here: String,
    /// Whether its words moved, rather than being only in one file.
    pub moved: bool,
}

/// The change gone to, as the Differences tab shows it in full: each side's
/// words and what its page is called.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenChange {
    pub here: String,
    pub there: String,
    pub label: String,
    pub label_there: String,
}

/// A comparison to make once the other file has opened in this window: the
/// file that opens, the file it is compared with and the password that
/// file was opened with, the place in it of the change to go to, and
/// whether the report window opens once the changes are found. See
/// [`Viewer::swap_comparison`] and [`Viewer::compare_two`].
pub(super) struct Swap {
    open: String,
    compare_with: String,
    password: Option<String>,
    at: Option<(usize, usize, usize)>,
    report: bool,
}

/// What the reader chooses in the report window before the report is
/// saved. See [`Viewer::save_report`].
#[derive(Clone, Debug, PartialEq)]
pub struct ReportOptions {
    /// Whether the document is A and the other file B, rather than the
    /// other way round.
    pub swapped: bool,
    /// The colours of words only in A, words only in B, and passages that
    /// moved, as hex.
    pub colours: [String; 3],
}

/// The settings that keep the report's colours, in the order of
/// [`ReportOptions::colours`].
pub(super) const REPORT_COLOURS: [&str; 3] =
    ["report_only_in_a", "report_only_in_b", "report_moved"];

/// How many changes the Differences tab lists. The pages show every one.
const CHANGE_ROWS: usize = 500;

/// How many characters of each side a row shows. The change gone to shows
/// all that was kept.
const ROW_SHOWN: usize = 200;

/// `words` cut short after [`ROW_SHOWN`] characters.
fn row_shown(words: &str) -> String {
    match words.char_indices().nth(ROW_SHOWN) {
        Some((at, _)) => format!("{}\u{2026}", &words[..at]),
        None => words.to_string(),
    }
}

impl Viewer {
    /// Whether `path` is the file the document is being compared with. A
    /// save must not replace it, since the report reads it again.
    pub(super) fn compared_with(&self, path: &str) -> bool {
        let key = |path: &str| crate::windows::same_file_key(&crate::config::absolute(path));
        (self.comparing.as_ref()).is_some_and(|comparing| key(&comparing.other.path) == key(path))
    }

    /// "Compare with…" in the Document menu: asks for the other version.
    pub fn compare_with(&mut self) {
        self.close_menu();
        if !self.empty() && !self.refuses_job(Reads::Edition) {
            self.choose.ask(Choosing::Compare, None);
        }
    }

    /// "Compare two documents…" on the start screen: asks for both.
    pub fn compare_two_documents(&mut self) {
        self.close_menu();
        self.choose.ask(Choosing::CompareTwo, None);
    }

    /// The two documents chosen to compare: the second opens in this
    /// window, is compared with the first, and the report window opens, as
    /// "Save as a report…" opens it. The first is A and the second B, in
    /// the order the dialog gives them, and the report window can swap them.
    /// A second document open in another window is not opened here, and
    /// the reader is told to compare it in that window.
    pub(super) fn compare_two(&mut self, paths: Vec<String>) {
        let [first, second] = paths.as_slice() else {
            self.notice = "Choose two documents to compare.".into();
            return;
        };
        let (first, second) = (
            crate::config::absolute(first),
            crate::config::absolute(second),
        );
        if first == second {
            self.notice = "Those are the same document. Choose two different ones.".into();
            return;
        }
        if self.shown_elsewhere(&second) {
            self.notice = format!(
                "{} is open in another window. Compare it there, with Compare with… in the Document menu.",
                crate::store::file_name(&second)
            );
            return;
        }
        self.swapping = Some(Swap {
            open: second.clone(),
            compare_with: first,
            password: None,
            at: None,
            report: true,
        });
        self.open_here(&second);
        if self.opening.is_none() {
            self.swapping = None;
        }
    }

    /// Compare the document with the file at `path`, on a thread, and show
    /// the changes in the Differences tab.
    pub(super) fn compare(&mut self, path: String) {
        self.compare_going(path, None, None, false);
    }

    /// [`Viewer::compare`] with the file opened with `password`, going to
    /// the change at `at` in the document once the changes are found, and
    /// opening the report window then if `report` says so.
    pub(super) fn compare_going(
        &mut self,
        path: String,
        password: Option<String>,
        at: Option<(usize, usize, usize)>,
        report: bool,
    ) {
        let path = crate::config::absolute(&path);
        if path == self.document.path() {
            self.notice = "That is this document. Choose another version of it.".into();
            return;
        }
        let document = self.document.clone();
        let name = crate::store::file_name(&path);
        let taken = self.on_thread_reading(
            {
                let path = path.clone();
                move || {
                    // ponytail: the other file is never asked for its
                    // password; a prompt of its own would let a locked
                    // file be chosen in the dialog too.
                    let other = (crate::render::open_with(&path, password.as_deref())).map_err(
                        |refused| match refused {
                            crate::render::Refusal::Locked => format!(
                                "{} is locked with a password. Open it, then compare it \
                                 from there with Compare with… in the Document menu.",
                                crate::store::file_name(&path)
                            ),
                            refused => refused.to_string(),
                        },
                    )?;
                    let changes = crate::compare::compare(&*document, &*other);
                    let about = crate::report::File::of(&*other);
                    other.release();
                    changes.map(|changes| (changes, about))
                }
            },
            move |viewer, changes| match changes {
                Ok((changes, about)) => {
                    viewer.show_changes(name, about, changes);
                    if let Some(at) = at {
                        viewer.go_to_change_at(at);
                    }
                    if report {
                        viewer.save_report();
                    }
                }
                Err(said) => viewer.notice = said,
            },
        );
        if !taken {
            self.notice = self.why_refused().into();
        }
    }

    fn show_changes(
        &mut self,
        name: String,
        other: crate::report::File,
        changes: Vec<crate::compare::Change>,
    ) {
        self.notice = match changes.len() {
            0 => format!("The text is the same as in {name}."),
            1 => format!("1 difference from {name}."),
            n => format!("{n} differences from {name}."),
        };
        if changes.is_empty() {
            self.stop_comparing();
            return;
        }
        let rows = (changes.iter().take(CHANGE_ROWS))
            .map(|change| ChangeRow {
                page: change.page(),
                label: self.label(change.page()),
                there: row_shown(&change.there),
                here: row_shown(&change.here),
                moved: change.moved.is_some(),
            })
            .collect();
        let mut on_page: HashMap<usize, Vec<(usize, usize, usize)>> = HashMap::new();
        for (number, change) in changes.iter().enumerate() {
            for &(page, from, to) in &change.places {
                on_page.entry(page).or_default().push((number, from, to));
            }
        }
        self.comparing = Some(Comparing {
            name,
            other,
            changes,
            at: None,
            open: None,
            rows: Rc::new(rows),
            on_page,
        });
        self.set_sidebar(true, false);
        self.tab = Tab::Changes;
    }

    /// Put the comparison away: the bands go, and so does the tab.
    pub fn stop_comparing(&mut self) {
        self.comparing = None;
        self.report_options = None;
        if self.tab == Tab::Changes {
            self.tab = if self.headings.is_empty() {
                Tab::Pages
            } else {
                Tab::Contents
            };
        }
    }

    /// What changed on page `page`, in CSS pixels from the top left of its
    /// box, each with its class: the words added, and a bar where words were
    /// removed. The change the reader went to last is `current` as well.
    pub fn change_areas(&self, page: usize) -> Vec<(Rect, &'static str)> {
        let Some(comparing) = &self.comparing else {
            return Vec::new();
        };
        if self.layout.box_of(page - 1).is_none() {
            return Vec::new();
        }
        let Some(here) = comparing.on_page.get(&page) else {
            return Vec::new();
        };
        let text = self.text_on(page);
        let mut out = Vec::new();
        for &(number, from, to) in here {
            let current = comparing.at == Some(number);
            let moved = comparing.changes[number].moved.is_some();
            if from < to {
                let class = match (moved, current) {
                    (false, false) => "added",
                    (false, true) => "added current",
                    (true, false) => "moved",
                    (true, true) => "moved current",
                };
                for quad in text.quads(from, to) {
                    out.push((self.layout.place_on(page - 1, quad), class));
                }
            } else if let Some(bar) = crate::compare::bar(&text, from) {
                let class = match (moved, current) {
                    (false, false) => "removed",
                    (false, true) => "removed current",
                    (true, false) => "moved bar",
                    (true, true) => "moved bar current",
                };
                out.push((self.layout.place_on(page - 1, bar), class));
            }
        }
        out
    }

    /// Go to change number `number`, from the Differences tab.
    pub fn go_to_change(&mut self, number: usize) {
        let Some(comparing) = &mut self.comparing else {
            return;
        };
        let Some(change) = comparing.changes.get(number) else {
            return;
        };
        let first = comparing.at.is_none();
        comparing.at = Some(number);
        let page_there = change.page_there();
        let label_there = (comparing.other.labels.get(page_there - 1).cloned())
            .unwrap_or_else(|| page_there.to_string());
        let (here, there) = (change.here.clone(), change.there.clone());
        let &(page, from, to) = &change.places[0];
        let label = self.label(page);
        if let Some(comparing) = self.comparing.as_mut() {
            comparing.open = Some(Rc::new(OpenChange {
                here,
                there,
                label,
                label_there,
            }));
        }
        let text = self.text_on(page);
        let line = text
            .quads(from, to)
            .first()
            .copied()
            .or_else(|| crate::compare::bar(&text, from));
        let from_here = self.layout.anchor(self.scroll_top);
        let placed = |viewer: &Self| Some(viewer.layout.place_on(page - 1, line?));
        self.show_on(page, placed, 0.8, REVEAL);
        // The first change gone to is a jump, so that ⌘[ goes back to where
        // the reader was before they began going through them.
        if first {
            self.note_jump(from_here);
        }
    }

    /// Go to the change that begins at `at` in the document, or failing
    /// that the nearest one after it: where a change seen in the other file
    /// is, once the two have changed places.
    fn go_to_change_at(&mut self, at: (usize, usize, usize)) {
        let Some(comparing) = &self.comparing else {
            return;
        };
        let begins = |change: &crate::compare::Change| change.places[0];
        let found = (comparing
            .changes
            .iter()
            .position(|change| begins(change) == at))
        .or_else(|| {
            let (page, from, _) = at;
            (comparing.changes.iter())
                .position(|change| (begins(change).0, begins(change).1) >= (page, from))
        })
        .unwrap_or(comparing.changes.len().saturating_sub(1));
        self.go_to_change(found);
    }

    /// "Show in …" under the change gone to: this window shows the other
    /// file instead, compared with the document it showed, at the same
    /// change. What one file has and the other does not is then on the
    /// pages, whichever file it is in.
    pub fn swap_comparison(&mut self) {
        // A report being written still reads the document this would put
        // down.
        if self.comparing.is_none() || self.refuses_job(Reads::Document) {
            return;
        }
        let Some(comparing) = &self.comparing else {
            return;
        };
        let other = comparing.other.path.clone();
        let other_password = comparing.other.password.clone();
        let at = (comparing.at)
            .and_then(|at| comparing.changes.get(at))
            .map(|change| change.places_there[0]);
        // A file open in another window comes forward instead, as it does
        // for any open.
        if !self.shown_elsewhere(&other) {
            self.swapping = Some(Swap {
                open: other.clone(),
                compare_with: self.document.path().to_string(),
                password: self.document.password().map(str::to_string),
                at,
                report: false,
            });
        }
        self.open_here_with(&other, other_password.as_deref());
        if self.opening.is_none() {
            self.swapping = None;
        }
    }

    /// "Save as a report…" in the Differences tab: opens the report
    /// window, where the reader chooses which file is A and the colours.
    pub fn save_report(&mut self) {
        if self.comparing.is_none() {
            return;
        }
        self.report_options = Some(ReportOptions {
            swapped: false,
            colours: REPORT_COLOURS.map(|key| self.store.text(key)),
        });
    }

    /// Put the report window away, or the colour picker open in it first.
    pub fn close_report_options(&mut self) -> bool {
        if self.report_options.is_none() {
            return false;
        }
        if self.picking.take().is_none() {
            self.report_options = None;
        }
        true
    }

    /// The names of A and B, as the report window shows them.
    pub fn report_files(&self) -> Option<(String, String)> {
        let (comparing, options) = (self.comparing.as_ref()?, self.report_options.as_ref()?);
        let (other, document) = (
            comparing.name.clone(),
            crate::store::file_name(self.document.path()),
        );
        Some(match options.swapped {
            false => (other, document),
            true => (document, other),
        })
    }

    /// "Swap A and B" in the report window.
    pub fn swap_report_files(&mut self) {
        if let Some(options) = &mut self.report_options {
            options.swapped = !options.swapped;
        }
    }

    /// One of the report's three colours, as `hex`.
    pub fn set_report_colour(&mut self, index: usize, hex: String) {
        if let Some(colour) = (self.report_options.as_mut()).and_then(|o| o.colours.get_mut(index))
        {
            *colour = hex;
        }
    }

    /// "Default colours" in the report window.
    pub fn default_report_colours(&mut self) {
        if let Some(options) = &mut self.report_options {
            let colours = crate::report::COLOURS;
            options.colours = [colours.a, colours.b, colours.moved]
                .map(|(r, g, b)| crate::palette::hex([r, g, b]));
        }
    }

    /// "Save…" in the report window: keeps the colours for the next report
    /// and asks where this one goes. The window stays up until the report
    /// is being written, so that a save dialog put away returns to it.
    pub fn choose_report_file(&mut self) {
        let (Some(comparing), Some(options)) = (&self.comparing, &self.report_options) else {
            return;
        };
        let stem = |path: &str| {
            let path = std::path::Path::new(path);
            (path.file_stem()).map_or_else(String::new, |stem| stem.to_string_lossy().into_owned())
        };
        let (other, document) = (comparing.other.path.as_str(), self.document.path());
        // Named for B, compared with A, beside the document.
        let (a, b) = match options.swapped {
            false => (other, document),
            true => (document, other),
        };
        let named_for = std::path::Path::new(document)
            .with_file_name(std::path::Path::new(b).file_name().unwrap_or_default());
        let suggested = crate::arrange::beside(
            &named_for.to_string_lossy(),
            &format!("compared with {}", stem(a)),
        );
        self.choose.ask(Choosing::SaveReport, Some(suggested));
    }

    /// The comparison written to `to` as a report, on a thread, as the
    /// report window says.
    pub(super) fn write_report(&mut self, to: String) {
        // A save that cannot start keeps the report window and its choices,
        // and keeps no colours.
        if self.refuses_job(Reads::Document) {
            return;
        }
        let Some(comparing) = &self.comparing else {
            return;
        };
        // The colours chosen in the report window are kept once a report is
        // saved in them, not when the save dialog is only opened.
        if let Some(options) = &self.report_options {
            let kept = (REPORT_COLOURS.iter().zip(&options.colours))
                .filter(|(_, hex)| crate::palette::read_colour(hex).is_some())
                .map(|(key, hex)| (key.to_string(), json!(hex)))
                .collect();
            self.store.set(kept);
        }
        let options = self.report_options.take().unwrap_or_else(|| ReportOptions {
            swapped: false,
            colours: REPORT_COLOURS.map(|key| self.store.text(key)),
        });
        self.picking = None;
        let defaults = crate::report::COLOURS;
        let colour = |at: usize, default: crate::report::Colour| {
            crate::palette::read_colour(&options.colours[at]).map_or(default, |[r, g, b]| (r, g, b))
        };
        let colours = crate::report::Colours {
            a: colour(0, defaults.a),
            b: colour(1, defaults.b),
            moved: colour(2, defaults.moved),
        };
        let swapped = options.swapped;
        let other = comparing.other.clone();
        let changes = comparing.changes.clone();
        let here = crate::report::File::of(&*self.document);
        // The report is written without a password, so the reader is told
        // when a file it shows has one.
        let locked: Vec<String> = ([&other, &here].into_iter())
            .filter(|file| file.password.is_some())
            .map(|file| file.name())
            .collect();
        let taken = self.on_thread(
            {
                let to = to.clone();
                move || {
                    // The changes are marked on the words each file had
                    // when it was compared, and its pages are read again
                    // from the disk.
                    for file in [&other, &here] {
                        file.unchanged(crate::render::stamp_of(&file.path))?;
                    }
                    let asked = match swapped {
                        false => crate::report::Asked {
                            there: other,
                            here,
                            changes,
                            colours,
                            to,
                        },
                        true => crate::report::Asked {
                            there: here,
                            here: other,
                            changes: changes.into_iter().map(|change| change.swapped()).collect(),
                            colours,
                            to,
                        },
                    };
                    crate::report::save(&asked)
                }
            },
            move |viewer, saved| match saved {
                // The report opens as a document from outside does: in a
                // tab or a window, as the setting says.
                Ok(unfinished) => {
                    let name = crate::store::file_name(&to);
                    viewer.notice = format!("Saved the comparison as {name}{}", unfinished.said());
                    if !locked.is_empty() {
                        viewer.notice += &format!(
                            " It has no password, though {} {} one.",
                            crate::report::in_a_list(&locked),
                            if locked.len() == 1 { "has" } else { "have" }
                        );
                    }
                    viewer.frame.ask(Ask::SendOn(to));
                }
                Err(said) => viewer.notice = said,
            },
        );
        if !taken {
            self.notice = self.why_refused().into();
        }
    }

    /// The comparison [`Viewer::swap_comparison`] asked for, once the file
    /// at `path` has opened in this window.
    pub(super) fn swapped(&mut self, path: &str) {
        if let Some(swap) = self.swapping.take().filter(|swap| swap.open == path) {
            self.compare_going(swap.compare_with, swap.password, swap.at, swap.report);
        }
    }
}
