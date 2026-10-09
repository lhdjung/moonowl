//! The Viewer's search: the find bar, the index behind it, and stepping
//! through its matches.

use super::*;

impl Viewer {
    /* ------------------------------------------------------- the search */

    /// Put the find bar up. Nothing is searched for until something is typed —
    /// unless the bar went down with a query in it, which comes back and is
    /// looked for again where the reader is, on the same match if they are
    /// still on its page, moving nothing until ⌘G or a keystroke asks; the token is the scan's, for [`rescan`].
    pub fn open_find(&mut self) -> Option<u64> {
        // Nothing to search. **One line more than the app has**, deliberately:
        // `find` is not `needsDocument` in `keys.ts`, so ⌘F on the app's start
        // screen puts up a bar that will never find anything. The flag is left
        // agreeing with the app, because that table is a port and this is a
        // judgement about one key.
        if self.empty() {
            return None;
        }
        // The colour popover is about a passage, and somebody opening the
        // find bar has moved on from it. Closing it here rather than leaving
        // Escape to do it keeps that key's order the one it claims to be:
        // outward, in the order the reader arrived — and the bar was arrived
        // at second.
        self.markup_at = None;
        // A menu and the bar do not stand up together; see `show_menu`.
        self.close_menu();
        self.find_open = true;
        self.find_asked += 1;
        if self.sidebar_open && !self.search.query().is_empty() {
            self.show_results_tab();
        }
        if self.find_query.is_empty() {
            return None;
        }
        // The match the bar went down on, while the reader is still on its
        // page; read on past it and the nearest one to where they are is ⌘G's.
        let page = self.page();
        let was = (self.find_left.take())
            .or(self.search.current())
            .filter(|hit| hit.page == page);
        self.find_again(was)
    }

    /// Show the list behind the count.
    ///
    /// **The count in the find bar is the way through to the results**: "3 of
    /// 128" answers *is it in here* and not *which one did I mean*, and the
    /// second is what somebody searching a long document is usually asking. An
    /// empty count leads nowhere and does not look pressable.
    ///
    /// A second press puts a panel the count borrowed back down: the way
    /// through is also the way back.
    pub fn show_results(&mut self) {
        if self.search.state().total == 0 {
            return;
        }
        if self.results_borrowed {
            self.results_borrowed = false;
            self.set_sidebar(false, false);
            self.leave_results_tab();
            return;
        }
        if !self.sidebar_open {
            self.set_sidebar(true, false);
            self.results_borrowed = true;
        }
        self.show_results_tab();
    }

    /// Take the find bar down, and the index with it.
    ///
    /// **The index goes when the bar does**: every page scanned is kept, so a
    /// long book costs tens of megabytes for as long as it is open — a fair
    /// trade while somebody is searching and none once they have stopped.
    /// Reopening rescans, in under half a second. See [`Search::forget`].
    /// The query itself stays, so that reopening looks for the same thing.
    pub fn close_find(&mut self) {
        self.find_left = self.search.current();
        self.put_find_away();
        self.search.forget();
    }

    /// The bar down and its scan stopped, and the pages it has read kept.
    pub(super) fn put_find_away(&mut self) {
        self.find_open = false;
        self.search_noted = false;
        self.scan += 1;
        // A panel that came up to hold the results goes back down with them,
        // so that one Escape undoes the whole of what one search did. See
        // `results_borrowed`, which is what keeps this from shutting a panel
        // the reader had open before any of this started.
        if self.results_borrowed {
            self.results_borrowed = false;
            self.set_sidebar(false, false);
        }
        self.leave_results_tab();
    }

    /// Back to the tab the reader had, not to whichever one the document
    /// has: Pages, searched and put away, came back as Contents.
    fn leave_results_tab(&mut self) {
        if self.tab == Tab::Results {
            self.tab = self
                .tab_before_results
                .take()
                .unwrap_or(if self.headings.is_empty() {
                    Tab::Pages
                } else {
                    Tab::Contents
                });
        }
    }

    /// The panel on its results, remembering the tab it was on.
    fn show_results_tab(&mut self) {
        if self.tab != Tab::Results {
            self.tab_before_results = Some(self.tab);
        }
        self.tab = Tab::Results;
    }

    /// Look for what is in the field. Returns the token of the scan it
    /// started, or `None` when there is nothing to scan — which is what the
    /// caller needs to know before spawning a task to drive it.
    pub fn find(&mut self, query: &str) -> Option<u64> {
        self.revealed = false;
        self.offered_results = false;
        self.look_for(query)?;
        if self.sidebar_open {
            self.show_results_tab();
        }
        Some(self.scan)
    }

    /// The query in the bar looked for again, **without moving the reader**:
    /// a new draft under an open bar, or the bar brought back with last
    /// time's words in it. Nobody typed, so nothing is revealed and the
    /// sidebar is left on its tab; the match the reader was on, `was`, stays
    /// theirs if it is still there.
    pub(super) fn find_again(&mut self, was: Option<crate::search::Hit>) -> Option<u64> {
        self.revealed = true;
        let query = self.find_query.clone();
        let token = self.look_for(&query)?;
        if let Some(hit) = was {
            self.search.prefer(hit);
        }
        Some(token)
    }

    fn look_for(&mut self, query: &str) -> Option<u64> {
        self.find_query = query.to_string();
        self.scan += 1;
        let (page, pages) = (self.page(), self.pages());
        self.search.find(query, page, pages).then_some(self.scan)
    }

    /// Read pages until the slice is up. Returns whether there is more to do.
    ///
    /// The whole of the streaming, here rather than in [`crate::search`]
    /// because it is the only part needing a clock and a document. The reason
    /// there is a slice at all is a book and not a page: pdfium reads a page
    /// in 0.18-1.3ms, so what is worth hiding is the 498ms a 376-page book of
    /// typeset mathematics takes.
    pub fn scan_slice(&mut self, token: u64) -> bool {
        if token != self.scan {
            return false;
        }
        let began = std::time::Instant::now();
        while let Some(page) = self.search.wants() {
            if self.search.knows(page) {
                self.search.feed(page, PageText::default);
            } else {
                // **Never waited for past the slice**: pdfium's one lock is
                // the renderer's for the whole of a page, which on a scan is
                // hundreds of milliseconds of a window that does not answer.
                // A page it is holding is asked for again until the slice is
                // up — not at once in the next, which spun a core redrawing
                // the window for as long as the render took.
                let Some(text) = self.document.try_text_of(page - 1) else {
                    if began.elapsed().as_secs_f64() * 1000.0 > crate::search::SLICE_MS {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    continue;
                };
                self.search.feed(page, || text);
            }
            if began.elapsed().as_secs_f64() * 1000.0 > crate::search::SLICE_MS {
                break;
            }
        }
        self.search.publish();
        // The first result to arrive is the one the reader is taken to, and
        // only the first: after that they are moved by asking rather than by
        // the scan catching up with them somewhere else in the book.
        if !self.revealed && self.search.current().is_some() {
            self.revealed = true;
            self.reveal_match();
        }
        self.show_the_matches();
        self.search.wants().is_some()
    }

    /// **A search puts its results in the panel, as soon as there are any.**
    ///
    /// With the first match rather than with the bar, so a search for
    /// something that is not in the document does not open a panel to say so.
    /// [`Viewer::show_results`] is the same door opened by hand.
    ///
    /// The panel is *borrowed*: it goes down with the bar that opened it, and
    /// one the reader had open before any of this is one they keep. See
    /// [`Viewer::close_find`].
    fn show_the_matches(&mut self) {
        // Once per search, and that is what the flag is for rather than
        // tidiness: a scan is dozens of slices, and a panel reopened on every
        // one of them is a panel the reader cannot close while the book is
        // still being read.
        if self.offered_results || !self.find_open || self.search.state().total == 0 {
            return;
        }
        // The reader's to turn off: the count in the bar still opens it.
        if !self.store.flag("search_shows_sidebar") {
            return;
        }
        self.offered_results = true;
        if !self.sidebar_open {
            self.set_sidebar(true, false);
            self.results_borrowed = true;
        }
        self.show_results_tab();
    }

    /// Move to the next match, or the one before, and go there.
    pub fn step_match(&mut self, forwards: bool) {
        // Nothing to step through, and nothing to say about it: "No matches"
        // on a start screen is an answer to a question nobody asked. The
        // second half of `open_find`'s note — the app leaves ⌘G, ⌘⇧G and ⌘F
        // unflagged, and all three of them are about a document.
        if self.empty() {
            return;
        }
        if self.search.matches().is_empty() {
            // Not yet is not none: the count says "Searching…", and the first
            // match to arrive is where the scan takes the reader anyway.
            if self.search.scanning() {
                return;
            }
            self.notice = if self.search.state().textless {
                "There is no text in this document to search".into()
            } else {
                "No matches".into()
            };
            return;
        }
        self.search.step(forwards);
        self.reveal_match();
    }

    /// Go to one result by its place in the list — a row of the Results tab.
    pub fn go_to_result(&mut self, at: usize) {
        self.search.go_to(at);
        self.reveal_match();
    }

    /// Bring the match the reader is on into view.
    ///
    /// A match is a range of characters and a character knows its box, so this
    /// is arithmetic: the top of the page, plus where the match is on it, less
    /// a third of a screen so there is something above it to read into.
    ///
    /// **A search's first move is a jump**, filed once per time the bar is up
    /// so that ⌘[ goes back to where the reader was before they searched, not
    /// through every match they stepped over.
    pub fn reveal_match(&mut self) {
        let from = self.layout.anchor(self.scroll_top);
        self.show_match();
        if !self.search_noted {
            self.search_noted = self.note_jump(from);
        }
    }

    fn show_match(&mut self) {
        let Some(hit) = self.search.current() else {
            return;
        };
        let page = match self.layout.box_of(hit.page - 1) {
            Some(page) => page,
            // Paged mode lays out one page and leaves the rest of `boxes`
            // empty, so a match anywhere else is a page to turn to first.
            // `revealMatch` in `viewer.ts` says the same thing: the other
            // pages have no place on the strip at all. **And then it is a
            // place on the page** — turning to it and stopping left a match
            // at the foot of a tall page off the bottom of the window.
            None => {
                self.go_to(Anchor {
                    page: hit.page,
                    offset: 0.0,
                });
                match self.layout.box_of(hit.page - 1) {
                    Some(page) => page,
                    None => return,
                }
            }
        };
        // Where the match lands on the *page as it is being shown*, which a
        // quad in the page's own points is not once the reader has turned or
        // trimmed it. One call rather than a multiplication by the scale —
        // see [`Layout::place_on`].
        let (top, bottom, left, right) = self
            .search
            .quads_on(hit.page)
            .into_iter()
            .filter(|(_, current)| *current)
            .map(|(quad, _)| self.layout.place_on(hit.page - 1, quad))
            .fold(
                (
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                ),
                |(top, bottom, left, right), rect| {
                    (
                        top.min(rect.top),
                        bottom.max(rect.top + rect.height),
                        left.min(rect.left),
                        right.max(rect.left + rect.width),
                    )
                },
            );
        // Across first, on a page wider than the window: a match in the
        // column out of sight is not on screen however far down it is.
        if left.is_finite() {
            self.reveal_across(page.left + left, page.left + right);
        }
        // A match already on screen stays where it is: stepping through a
        // paragraph of them jumped the page for every one, and each jump is
        // the reader finding their place again.
        let height = self.layout.viewport.height;
        if top.is_finite()
            && page.top + top >= self.scroll_top
            && page.top + bottom <= self.scroll_top + height
        {
            return;
        }
        let target = if top.is_finite() {
            page.top + top - height * REVEAL
        } else {
            // A match nothing drew — pdfium generates characters the printer
            // never put on the page — is still on a page.
            page.top
        };
        self.scroll_to(target);
    }

    /// Change one of the two switches that decide what is found, and look
    /// again with it.
    ///
    /// The extracted text stays: only the fold and the boundary test depend on
    /// these, so a rescan asks the renderer for nothing —
    /// `changing_the_case_setting_does_not_go_back_to_the_renderer` says so.
    pub fn set_find_options(&mut self, options: Find) -> Option<u64> {
        let was = self.search.options();
        self.search.set_options(options);
        if options.match_case != was.match_case {
            self.store
                .set_find_switch("search_match_case", options.match_case);
        }
        if options.whole_words != was.whole_words {
            self.store
                .set_find_switch("search_whole_words", options.whole_words);
        }
        let query = self.find_query.clone();
        self.find(&query)
    }

    /// A setting changed, here or in another window: draw again, and take up
    /// the find bar's switches, looking again if they moved.
    pub fn settings_changed(&mut self) -> Option<u64> {
        self.generation += 1;
        let options = Find {
            match_case: self.store.flag("search_match_case"),
            whole_words: self.store.flag("search_whole_words"),
        };
        if options == self.search.options() {
            return None;
        }
        self.search.set_options(options);
        // Matches found the other way are let go of, and looked for again
        // under an open bar. The text read stays: only the fold changed.
        let was = self.search.current();
        self.search.clear();
        if self.find_open {
            self.find_again(was)
        } else {
            None
        }
    }

    /// Paint every match, or only the one the reader is on.
    pub fn toggle_highlight_all(&mut self) {
        self.highlight_all = !self.highlight_all;
        self.store.set(vec![(
            "search_highlight_all".into(),
            json!(self.highlight_all),
        )]);
    }

    /// What to paint over one page, in CSS pixels from its top left.
    ///
    /// `(rectangle, is the one the reader is on)`. Empty when the find bar is
    /// down, because a highlight that outlives the bar that made it is a mark
    /// on the page nobody asked for.
    /// The highlights kept beside the document on a page, placed on its box.
    ///
    /// pdfium draws the ones in the file; these it has never heard of, and
    /// without this a reader who highlighted in a read-only or encrypted
    /// document was told "Highlighted" and saw nothing on the page.
    ///
    /// **Not one a rebuild lost**: its place is the old version's, and drawn
    /// there it lit up whatever words had moved under it. It waits in the
    /// sidebar until it is put back.
    pub fn kept_areas(&self, page: usize) -> Vec<(Rect, String)> {
        let Some(index) = page.checked_sub(1) else {
            return Vec::new();
        };
        if self.layout.box_of(index).is_none() {
            return Vec::new();
        }
        let height = self.document.size_of(index).height;
        self.markup_adrift()
            .into_iter()
            .filter(|held| held.page as usize == page && !held.lost)
            .flat_map(|held| {
                held.quads.as_chunks::<8>().0.iter().map(move |q| {
                    let rect = Rect {
                        left: q[0],
                        top: height - q[1],
                        width: q[2] - q[0],
                        height: q[1] - q[5],
                    };
                    (self.layout.place_on(index, rect), held.color.clone())
                })
            })
            .collect()
    }

    pub fn highlights(&self, page: usize) -> Vec<(Rect, bool)> {
        if !self.find_open {
            return Vec::new();
        }
        if self.layout.box_of(page - 1).is_none() {
            return Vec::new();
        }
        self.search
            .quads_on(page)
            .into_iter()
            .filter(|(_, current)| self.highlight_all || *current)
            .map(|(quad, current)| (self.layout.place_on(page - 1, quad), current))
            .collect()
    }

    /// What the find bar says beside the field: where the reader is in the
    /// matches, or why there are none.
    pub fn find_count(&self) -> String {
        let state = self.search.state();
        if state.query.trim().is_empty() {
            return String::new();
        }
        if state.total == 0 {
            return if state.scanning {
                "Searching…".into()
            } else if state.textless {
                "No text to search".into()
            } else {
                "None".into()
            };
        }
        let at = state.at.map(|at| at + 1).unwrap_or(0);
        let more = if state.capped {
            "+"
        } else if state.scanning {
            "…"
        } else {
            ""
        };
        format!("{at} of {}{more}", state.total)
    }
}
