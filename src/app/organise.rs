//! The Viewer and the page organiser: opening it, its keys, the drag, the
//! file dialogs, and saving. [`crate::organiser`] holds the draft.

use super::pages::{file_safe, same_file, NO_CHAPTERS};
use super::*;
use crate::arrange::Edit;
use crate::layout::Size;
use crate::organiser::{Organiser, Popup, Source};

/// How far a pressed page moves before it is a drag rather than a click,
/// in pixels.
const DRAG_THRESHOLD: f64 = 6.0;

/// How near the top or bottom of the grid the pointer has to be for a drag
/// to scroll it, in pixels.
const EDGE: f64 = 40.0;

/// A file opened for the organiser on a thread: the file, where its pages
/// are read from, the document, and whether that is a PDF made from a
/// picture.
type Taken = (String, String, Arc<dyn PageSource>, bool);

/// The path and password of the window's document, when the draft takes
/// pages from it, and every file the draft reads.
type Reads = (Option<(String, Option<String>)>, Vec<String>);

impl Viewer {
    /// How large the organiser draws its pages: a setting.
    fn organiser_size(&self) -> f64 {
        match self.store.number("arrange_size") {
            size if size > 0.0 => size.clamp(crate::organiser::SMALLEST, crate::organiser::LARGEST),
            _ => 180.0,
        }
    }

    /// The organiser's grid: the window under its bar.
    fn organiser_room(&self) -> (f64, f64) {
        (
            self.window_width,
            (self.window_height - crate::organiser::BAR).max(100.0),
        )
    }

    /// "Arrange pages…": the organiser on the document, with `page` (one-
    /// based) selected and in view.
    pub fn open_organiser(&mut self, page: usize) {
        self.close_menu();
        if self.empty() {
            return;
        }
        // An organiser opened while a write goes into the document would be
        // put away, with anything put into it, when the write lands.
        if self.writing.is_some() {
            self.busy();
            return;
        }
        // The document being opened would take this one's place under the
        // organiser, and Save into would write this one's pages into it.
        if self.opening.is_some() {
            self.notice = "This window is still opening a document. Try again when it has.".into();
            return;
        }
        let own = Source::new(
            self.document.path().to_string(),
            self.document.path().to_string(),
            true,
            self.document.password().map(str::to_string),
            self.document.clone(),
            Chosen::new(self.palette()),
            false,
        );
        let (width, height) = self.organiser_room();
        let mut organiser = Organiser::new(vec![own], width, height, self.organiser_size());
        if (1..=organiser.sheets.len()).contains(&page) {
            organiser.click(page - 1, false, false);
            organiser.reveal(page - 1);
        }
        self.organiser = Some(organiser);
    }

    /// "Combine files…": the organiser with nothing in it, and the files
    /// asked for at once.
    pub fn combine_files(&mut self) {
        self.close_menu();
        let (width, height) = self.organiser_room();
        self.organiser = Some(Organiser::new(
            Vec::new(),
            width,
            height,
            self.organiser_size(),
        ));
        self.choose.ask(Choosing::AddFiles, None);
    }

    pub fn add_files(&mut self) {
        self.choose.ask(Choosing::AddFiles, None);
    }

    /// The organiser goes. With changes in it, it asks first, unless
    /// `asked`.
    pub fn close_organiser(&mut self, asked: bool) {
        let Some(organiser) = &self.organiser else {
            return;
        };
        if organiser.busy {
            return;
        }
        if organiser.changed() && !asked {
            self.asking = Some(Asking {
                title: "Close without saving?".into(),
                says: "The pages as they are arranged here are not saved anywhere yet.".into(),
                go: "Close without saving",
                of_document: false,
                other: Other::Keep("Keep arranging"),
                then: Box::new(|viewer| viewer.close_organiser(true)),
            });
            return;
        }
        self.organiser = None;
    }

    /// Before the window goes, or the app: a draft with changes is asked
    /// about first, and `then` is asked of the window once the reader
    /// closes without saving. A draft being saved keeps the window until it
    /// is. Answers whether the window stays for now.
    pub(super) fn asks_before_closing(&mut self, then: Ask) -> bool {
        // A draft being saved is waited for, or a set of files would be
        // left half written. Chapters being read are not: the cuts are then
        // not made.
        let saving = (self.organiser.as_ref())
            .is_some_and(|o| o.busy || (o.own().is_some() && self.writing.is_some()));
        if saving {
            self.notice = Self::BUSY.into();
            return true;
        }
        if !self.organiser.as_ref().is_some_and(|o| o.changed()) {
            return false;
        }
        self.asking = Some(Asking {
            title: "Close without saving?".into(),
            says: "The pages as they are arranged here are not saved anywhere yet.".into(),
            go: "Close without saving",
            of_document: false,
            other: Other::Keep("Keep arranging"),
            then: Box::new(move |viewer| {
                viewer.organiser = None;
                viewer.frame.ask(then);
            }),
        });
        true
    }

    /// Escape in the organiser: a menu or field, then a drag, then the
    /// selection, and then the organiser itself.
    pub fn organiser_escape(&mut self) {
        let Some(organiser) = self.organiser.as_mut() else {
            return;
        };
        if organiser.popup.take().is_some() {
            return;
        }
        if organiser.drag.take().is_some() {
            organiser.press = None;
            organiser.roll = None;
            organiser.dragged = true;
            return;
        }
        if !organiser.picked.is_empty() {
            organiser.picked.clear();
            return;
        }
        self.close_organiser(false);
    }

    /// Answer `action` in the organiser, if it is one of the organiser's.
    pub fn organiser_action(&mut self, action: Action) -> bool {
        let Some(organiser) = self.organiser.as_mut() else {
            return false;
        };
        match action {
            Action::Dismiss => self.organiser_escape(),
            Action::SelectPage => organiser.pick_all(),
            Action::RemovePages => self.remove_sheets(),
            Action::TurnPagesLeft => self.turn_sheets(-1),
            Action::TurnPagesRight => self.turn_sheets(1),
            Action::DuplicatePages => self.duplicate_sheets(),
            Action::ZoomIn => self.grow_sheets(true),
            Action::ZoomOut => self.grow_sheets(false),
            Action::Undo if !organiser.busy => {
                organiser.undo();
            }
            Action::Redo if !organiser.busy => {
                organiser.redo();
            }
            Action::Undo | Action::Redo => {}
            _ => return false,
        }
        true
    }

    /// Whether the organiser hears the window's keys: it is open, and no
    /// question or Settings is over it.
    pub fn organiser_hears(&self) -> bool {
        self.organiser.is_some() && self.asking.is_none() && self.pane.is_none()
    }

    /// A key the organiser answers before the keymap sees it: the arrows
    /// move the selection, and with Shift grow it; Enter is "Save as…".
    /// Answers whether the key was one of those. A field that has the
    /// keyboard keeps these keys itself, so they never arrive here.
    pub fn organiser_key(&mut self, key: &Key, shift: bool) -> bool {
        let Some(organiser) = self.organiser.as_mut() else {
            return false;
        };
        // A menu with a field in it has the keyboard; a page's menu does
        // not, and goes away.
        match organiser.popup {
            None | Some(Popup::Sheet(..)) => {}
            Some(_) => return false,
        }
        let (dx, dy) = match key {
            Key::ArrowLeft => (-1, 0),
            Key::ArrowRight => (1, 0),
            Key::ArrowUp => (0, -1),
            Key::ArrowDown => (0, 1),
            Key::Enter => {
                organiser.popup = None;
                self.save_arranged();
                return true;
            }
            _ => return false,
        };
        organiser.popup = None;
        if let Some(to) = organiser.step_to(dx, dy, shift) {
            organiser.reveal(to);
        }
        true
    }

    /// Whether the draft is being saved, into a new file or into the
    /// document, or its chapters read, which it then says: the draft does
    /// not change until then, since a page added or moved meanwhile would go
    /// with the organiser, or be cut in the wrong place.
    fn saving_pages(&mut self) -> bool {
        let busy = (self.organiser.as_ref())
            .is_some_and(|o| o.busy || o.reading || (o.own().is_some() && self.writing.is_some()));
        if busy {
            self.notice = Self::BUSY.into();
        }
        busy
    }

    /// Whatever is open over the grid is put away.
    pub fn close_popup(&mut self) {
        if let Some(organiser) = self.organiser.as_mut() {
            organiser.popup = None;
        }
    }

    /// `popup` opened, or put away if it was the one open.
    pub fn toggle_popup(&mut self, popup: Popup) {
        if let Some(organiser) = self.organiser.as_mut() {
            organiser.popup = match organiser.popup == Some(popup.clone()) {
                true => None,
                false => Some(popup),
            };
        }
    }

    /// A right-click on page `at`: its menu, over the selection, which
    /// becomes that page alone unless it is part of it.
    pub fn open_sheet_menu(&mut self, at: usize, pointer: (f64, f64)) {
        if let Some(organiser) = self.organiser.as_mut() {
            organiser.press = None;
            if !organiser.picked.contains(&at) {
                organiser.click(at, false, false);
            }
            organiser.popup = Some(Popup::Sheet(at, pointer));
        }
    }

    /// A double-click on page `at`: the reader goes to it. A draft with
    /// changes in it asks first, as closing does.
    pub fn read_sheet(&mut self, at: usize) {
        let Some(organiser) = self.organiser.as_ref() else {
            return;
        };
        let Some(own) = organiser.own() else {
            return;
        };
        let Some(sheet) = organiser.sheets.get(at).filter(|sheet| sheet.source == own) else {
            return;
        };
        let page = sheet.page + 1;
        if organiser.changed() {
            self.asking = Some(Asking {
                title: "Close without saving?".into(),
                says: "The pages as they are arranged here are not saved anywhere yet.".into(),
                go: "Close without saving",
                of_document: false,
                other: Other::Keep("Keep arranging"),
                then: Box::new(move |viewer| {
                    viewer.organiser = None;
                    viewer.go_to_page(page);
                }),
            });
            return;
        }
        self.organiser = None;
        self.go_to_page(page);
    }

    /// The Select menu: every page, or the odd or even ones.
    pub fn pick_sheets(&mut self, odd: Option<bool>) {
        if let Some(organiser) = self.organiser.as_mut() {
            organiser.popup = None;
            match odd {
                None => organiser.pick_all(),
                Some(odd) => organiser.pick_parity(odd),
            }
        }
    }

    /// "Pages …" in the Select menu: the pages typed.
    pub fn pick_typed_sheets(&mut self) {
        let Some(organiser) = self.organiser.as_mut() else {
            return;
        };
        let typed = organiser.typed.clone();
        if organiser.pick_typed(&typed) {
            organiser.popup = None;
            if let Some(&first) = organiser.picked.first() {
                organiser.reveal(first);
            }
        } else {
            self.notice = "Type which pages to select, like 3, 5-8 or 10-.".into();
        }
    }

    /// The page typed in a page's menu: the selection moves there.
    pub fn move_typed_sheets(&mut self) {
        if self.saving_pages() {
            return;
        }
        let Some(organiser) = self.organiser.as_mut() else {
            return;
        };
        let page = organiser.moving_to.trim().parse().unwrap_or(0);
        if organiser.move_to_page(page) {
            organiser.popup = None;
            organiser.moving_to.clear();
            if let Some(&first) = organiser.picked.first() {
                organiser.reveal(first);
            }
        } else {
            self.notice = format!(
                "Type the page to move them to, from 1 to {}.",
                organiser.sheets.len() + 1 - organiser.picked.len()
            );
        }
    }

    /// A click on a file's heading: a field with its name.
    pub fn open_name_field(&mut self, first: usize) {
        if let Some(organiser) = self.organiser.as_mut() {
            organiser.typed = (organiser.sheets.get(first))
                .and_then(|sheet| sheet.begins.clone())
                .unwrap_or_default();
            organiser.popup = Some(Popup::Name(first));
        }
    }

    /// Enter in that field: the name typed.
    pub fn name_section(&mut self) {
        if self.saving_pages() {
            return;
        }
        if let Some(organiser) = self.organiser.as_mut() {
            if let Some(Popup::Name(first)) = organiser.popup.take() {
                let typed = organiser.typed.clone();
                organiser.name(first, &typed);
            }
        }
    }

    pub fn duplicate_sheets(&mut self) {
        if self.saving_pages() {
            return;
        }
        if let Some(organiser) = self.organiser.as_mut() {
            organiser.popup = None;
            organiser.duplicate();
        }
    }

    pub fn keep_sheets(&mut self) {
        if self.saving_pages() {
            return;
        }
        if let Some(organiser) = self.organiser.as_mut() {
            organiser.popup = None;
            organiser.keep_only();
        }
    }

    pub fn reverse_sheets(&mut self) {
        if self.saving_pages() {
            return;
        }
        if let Some(organiser) = self.organiser.as_mut() {
            organiser.popup = None;
            organiser.reverse();
        }
    }

    pub fn interleave_sheets(&mut self) {
        if self.saving_pages() {
            return;
        }
        if let Some(organiser) = self.organiser.as_mut() {
            organiser.popup = None;
            organiser.interleave();
        }
    }

    /// Files dropped on the window: the organiser opens if it is not open,
    /// on the document if there is one, and the files go into it together.
    pub fn take_dropped(&mut self, paths: Vec<String>) {
        if self.saving_pages() {
            return;
        }
        if self.organiser.is_none() {
            if self.empty() {
                let (width, height) = self.organiser_room();
                self.organiser = Some(Organiser::new(
                    Vec::new(),
                    width,
                    height,
                    self.organiser_size(),
                ));
            } else {
                self.open_organiser(self.page());
            }
        }
        self.take_files(paths);
    }

    /// "Add a blank page after": a page the size of page `at`, as it is
    /// shown, put in after the selection.
    pub fn add_blank(&mut self, at: usize) {
        if self.saving_pages() {
            return;
        }
        let Some(organiser) = self.organiser.as_mut() else {
            return;
        };
        organiser.popup = None;
        let Some(sheet) = organiser.sheets.get(at) else {
            return;
        };
        let size = organiser.sources[sheet.source].document.size_of(sheet.page);
        let (width, height) = match sheet.turn % 2 {
            0 => (size.width, size.height),
            _ => (size.height, size.width),
        };
        // A blank page of this size made before is a source already, and
        // another page of it is enough.
        let same =
            |size: Size| (size.width - width).abs() < 0.5 && (size.height - height).abs() < 0.5;
        let blank = (organiser.sources.iter())
            .position(|source| source.file == BLANK && same(source.document.size_of(0)));
        if let Some(blank) = blank {
            organiser.add_from(blank);
            return;
        }
        let palette = self.palette();
        let taken = self.on_thread(
            move || made_for_organiser(crate::arrange::blank_pdf(width, height), BLANK),
            move |viewer, opened| {
                let Some(organiser) = viewer.organiser.as_mut() else {
                    return;
                };
                match opened {
                    Ok((file, path, document, made)) => {
                        let mut source = Source::new(
                            file,
                            path,
                            false,
                            None,
                            document,
                            Chosen::new(palette),
                            made,
                        );
                        source.name = "Blank page".into();
                        organiser.add(vec![source]);
                    }
                    Err(said) => viewer.notice = said,
                }
            },
        );
        if !taken {
            self.notice = Self::BUSY.into();
        }
    }

    /// Keep the grid fitted to the window.
    pub(super) fn relay_organiser(&mut self) {
        let (width, height) = self.organiser_room();
        if let Some(organiser) = self.organiser.as_mut() {
            if organiser.width != width || organiser.height != height {
                organiser.resize(width, height);
            }
        }
    }

    pub fn scroll_organiser(&mut self, delta: f64) {
        if let Some(organiser) = self.organiser.as_mut() {
            organiser.scroll_by(delta);
        }
    }

    /* ---------------------------------------------------------- editing */

    pub fn click_sheet(&mut self, at: usize, adding: bool, extending: bool) {
        if let Some(organiser) = self.organiser.as_mut() {
            organiser.click(at, adding, extending);
        }
    }

    pub fn remove_sheets(&mut self) {
        if self.saving_pages() {
            return;
        }
        if let Some(organiser) = self.organiser.as_mut() {
            if !organiser.remove() {
                self.notice = "A document needs at least one page.".into();
            }
        }
    }

    pub fn turn_sheets(&mut self, quarters: i32) {
        if self.saving_pages() {
            return;
        }
        if let Some(organiser) = self.organiser.as_mut() {
            organiser.turn(quarters);
        }
    }

    pub fn cut_sheets(&mut self) {
        if self.saving_pages() {
            return;
        }
        if let Some(organiser) = self.organiser.as_mut() {
            organiser.cut();
        }
    }

    pub fn join_sheets(&mut self, first: usize) {
        if self.saving_pages() {
            return;
        }
        if let Some(organiser) = self.organiser.as_mut() {
            organiser.join(first);
        }
    }

    pub fn grow_sheets(&mut self, larger: bool) {
        if let Some(organiser) = self.organiser.as_mut() {
            let size = organiser.grow(larger);
            self.store.set(vec![("arrange_size".into(), json!(size))]);
        }
    }

    /// "Every … pages": the number typed.
    pub fn split_every(&mut self) {
        if self.saving_pages() {
            return;
        }
        let Some(organiser) = self.organiser.as_mut() else {
            return;
        };
        match organiser.every.trim().parse::<usize>() {
            Ok(every) if every > 0 => {
                organiser.popup = None;
                organiser.split_every(every);
            }
            _ => self.notice = "Type how many pages each file has, like 10.".into(),
        }
    }

    /// "At each chapter": whether a chapter's first page also ends the
    /// chapter before is read off the pages' text on a thread, and the
    /// draft is cut when it has been.
    pub fn split_at_chapters(&mut self) {
        if self.saving_pages() {
            return;
        }
        let chapters = self.chapters();
        let Some(organiser) = self.organiser.as_mut() else {
            return;
        };
        organiser.popup = None;
        if chapters.len() < 2 {
            self.notice = NO_CHAPTERS.into();
            return;
        }
        organiser.reading = true;
        let document = self.document.clone();
        let taken = self.on_thread(
            move || {
                let mut shared = false;
                (chapters.into_iter())
                    .map(|chapter| {
                        let starts = (chapter.title, chapter.pages[0], shared);
                        shared = chapter.next.is_some_and(|(page, spot)| {
                            crate::arrange::ends_there(&*document, page, spot)
                        });
                        starts
                    })
                    .collect::<Vec<_>>()
            },
            |viewer, chapters| {
                // An organiser closed meanwhile, or opened again since, is
                // not the one that asked.
                if let Some(organiser) = viewer.organiser.as_mut().filter(|o| o.reading) {
                    organiser.reading = false;
                    organiser.split_at(&chapters);
                }
            },
        );
        if !taken {
            if let Some(organiser) = self.organiser.as_mut() {
                organiser.reading = false;
            }
            self.notice = Self::BUSY.into();
        }
    }

    /// The files "Add files…" asked for, opened on a thread and added to
    /// the draft. A picture is made into a PDF of one page first.
    pub(super) fn take_files(&mut self, paths: Vec<String>) {
        if self.saving_pages() {
            return;
        }
        if self.organiser.is_none() {
            return;
        }
        let taken = self.on_thread(
            move || {
                // Asked of every file at once, so that the wait that tells a
                // whole file from one being written is one wait.
                let whole = crate::watch::all_whole(
                    &paths
                        .iter()
                        .map(|path| std::path::Path::new(path.as_str()))
                        .collect::<Vec<_>>(),
                );
                (paths.iter().zip(whole))
                    .map(|(path, whole)| open_for_organiser(path, whole))
                    .collect::<Vec<Result<Taken, String>>>()
            },
            |viewer, opened| {
                let palette = viewer.palette();
                let Some(organiser) = viewer.organiser.as_mut() else {
                    return;
                };
                let mut refused = Vec::new();
                let mut sources = Vec::new();
                for opened in opened {
                    match opened {
                        Ok((file, path, document, made)) => sources.push(Source::new(
                            file,
                            path,
                            false,
                            None,
                            document,
                            Chosen::new(palette),
                            made,
                        )),
                        Err(said) => refused.push(said),
                    }
                }
                // Every file chosen together is selected together.
                if !sources.is_empty() {
                    organiser.add(sources);
                    if let Some(&first) = organiser.picked.first() {
                        organiser.reveal(first);
                    }
                }
                if !refused.is_empty() {
                    viewer.notice = refused.join(" ");
                }
            },
        );
        if !taken {
            self.notice = Self::BUSY.into();
        }
    }

    /* ------------------------------------------------------ the drag */

    pub fn press_sheet(&mut self, at: usize, pointer: (f64, f64)) {
        if let Some(organiser) = self.organiser.as_mut() {
            organiser.dragged = false;
            organiser.press = Some((at, pointer));
        }
    }

    pub fn pressed_on_sheet(&self) -> bool {
        self.organiser.as_ref().is_some_and(|o| o.press.is_some())
    }

    /// The pointer carried with a page pressed. Past a few pixels the pages
    /// lift, and where they would land is worked out from the grid; near
    /// the top or bottom of the grid, the grid scrolls.
    pub fn drag_sheets(&mut self, pointer: (f64, f64)) {
        let bottom = self.window_height;
        let Some(organiser) = self.organiser.as_mut() else {
            return;
        };
        let Some((at, from)) = organiser.press else {
            return;
        };
        if organiser.drag.is_none() {
            if (pointer.0 - from.0).hypot(pointer.1 - from.1) < DRAG_THRESHOLD {
                return;
            }
            if !organiser.picked.contains(&at) {
                organiser.click(at, false, false);
            }
        }
        let inside = pointer.1 >= crate::organiser::BAR && pointer.1 <= bottom;
        let landing = inside.then(|| {
            organiser.grid.landing(
                pointer.0,
                pointer.1 - crate::organiser::BAR + organiser.scroll,
            )
        });
        organiser.drag = Some(crate::organiser::Drag {
            at: pointer,
            landing,
        });
        if organiser.roll.is_none() && self.sheet_speed().is_some() {
            self.sheet_rolls += 1;
            if let Some(organiser) = self.organiser.as_mut() {
                organiser.roll = Some(self.sheet_rolls);
            }
            crate::emit::after(
                STILL_TICK,
                self.post.clone(),
                crate::emit::News {
                    event: crate::emit::Event::SheetTick(self.sheet_rolls),
                    target: None,
                },
            );
        }
    }

    /// How far the grid scrolls in one step under a drag held at its edge.
    fn sheet_speed(&self) -> Option<f64> {
        let y = self.organiser.as_ref()?.drag.as_ref()?.at.1;
        let (top, bottom) = (crate::organiser::BAR, self.window_height);
        let past = if y < top + EDGE {
            y - top - EDGE
        } else if y > bottom - EDGE {
            y - bottom + EDGE
        } else {
            return None;
        };
        Some((past * 0.4).clamp(-24.0, 24.0))
    }

    /// One step of that clock. Answers whether it goes on.
    pub fn roll_sheets(&mut self, token: u64) -> bool {
        let rolling = self
            .organiser
            .as_ref()
            .is_some_and(|o| o.roll == Some(token));
        let speed = rolling.then(|| self.sheet_speed()).flatten();
        let at = (self.organiser.as_ref())
            .and_then(|o| o.drag.as_ref())
            .map(|drag| drag.at);
        let (Some(speed), Some(at)) = (speed, at) else {
            if let Some(organiser) = self.organiser.as_mut().filter(|o| o.roll == Some(token)) {
                organiser.roll = None;
            }
            return false;
        };
        self.scroll_organiser(speed);
        self.drag_sheets(at);
        true
    }

    /// The pages let go of: they move to where the line is. Let go outside
    /// the grid, they stay where they were.
    pub(super) fn drop_sheets(&mut self) {
        let Some(organiser) = self.organiser.as_mut() else {
            return;
        };
        organiser.press = None;
        organiser.roll = None;
        let Some(drag) = organiser.drag.take() else {
            return;
        };
        organiser.dragged = true;
        if let Some((before, leads)) = drag.landing.filter(|_| !organiser.busy) {
            organiser.move_to(before, leads);
        }
    }

    /* ----------------------------------------------------------- saving */

    /// "Save as…": one file, or a folder for several.
    pub fn save_arranged(&mut self) {
        let Some(organiser) = (self.organiser.as_ref()).filter(|o| !o.busy && !o.reading) else {
            return;
        };
        if organiser.sheets.is_empty() {
            return;
        }
        if organiser.parts().len() > 1 {
            self.choose.ask(Choosing::SaveParts, None);
            return;
        }
        let (file, how) = match organiser.own() {
            Some(own) if organiser.sources.len() == 1 => (&organiser.sources[own].file, "arranged"),
            Some(own) => (&organiser.sources[own].file, "combined"),
            None => (&organiser.sources[0].file, "combined"),
        };
        let suggested = crate::arrange::beside(file, how);
        self.choose.ask(Choosing::SaveArranged, Some(suggested));
    }

    /// What `assemble` needs to read the window's document, if the draft
    /// takes pages from it, and every file the draft reads.
    fn organiser_reads(&self) -> Option<Reads> {
        let organiser = self.organiser.as_ref()?;
        let document = (organiser.own()).map(|own| {
            let source = &organiser.sources[own];
            (source.path.clone(), source.password.clone())
        });
        let files = organiser.sources.iter().map(|s| s.file.clone()).collect();
        Some((document, files))
    }

    /// The draft written to a new file at `to`.
    pub(super) fn write_arranged(&mut self, to: String) {
        let Some((document, files)) = self.organiser_reads() else {
            return;
        };
        let Some(organiser) = self.organiser.as_mut() else {
            return;
        };
        let sheets = organiser.arranged(0..organiser.sheets.len());
        let count = sheets.len();
        organiser.busy = true;
        let taken = self.on_thread(
            {
                let to = to.clone();
                move || {
                    if files.iter().any(|file| same_file(file, &to)) {
                        return Err("Choose a new name. The pages cannot be saved over a \
                                    file they come from."
                            .to_string());
                    }
                    let document = (document.as_ref()).map(|(p, w)| (p.as_str(), w.as_deref()));
                    crate::arrange::assemble(document, &sheets, &to)
                }
            },
            move |viewer, saved| match saved {
                Ok(lost) => {
                    viewer.organiser = None;
                    let name = crate::store::file_name(&to);
                    let pages = if count == 1 { "page" } else { "pages" };
                    let lost = if lost { LINKS_LEFT_BEHIND } else { "" };
                    let said = format!("Saved {name}, {count} {pages}.{lost}");
                    // A window with nothing in it opens the new file, which
                    // says what happened by itself.
                    if viewer.empty() {
                        viewer.open_here(&to);
                    } else {
                        viewer.offer_open(said, to);
                    }
                }
                Err(said) => {
                    if let Some(organiser) = viewer.organiser.as_mut() {
                        organiser.busy = false;
                    }
                    viewer.notice = said;
                }
            },
        );
        if !taken {
            if let Some(organiser) = self.organiser.as_mut() {
                organiser.busy = false;
            }
            self.notice = Self::BUSY.into();
        }
    }

    /// Every file of the draft written into `folder`, named after its
    /// chapter, or numbered. Nothing in the folder is written over.
    pub(super) fn write_parts(&mut self, folder: String) {
        let Some((document, files)) = self.organiser_reads() else {
            return;
        };
        let Some(organiser) = self.organiser.as_mut() else {
            return;
        };
        let parts = organiser.parts();
        let names = organiser.part_names();
        let named = (parts.iter()).any(|part| {
            (organiser.sheets[part.start].begins.as_ref()).is_some_and(|name| !name.is_empty())
        });
        let first = (organiser.own()).map_or_else(
            || organiser.sources[0].file.clone(),
            |own| organiser.sources[own].file.clone(),
        );
        let digits = parts.len().to_string().len();
        let written: Vec<(Vec<crate::arrange::Sheet>, String)> = (parts.iter().zip(&names))
            .enumerate()
            .map(|(at, (part, name))| {
                let file = match named {
                    true => {
                        let mut title = file_safe(name);
                        if title.is_empty() {
                            title = "Chapter".into();
                        }
                        format!("{:0digits$} {title}.pdf", at + 1)
                    }
                    false => crate::store::file_name(&crate::arrange::beside(&first, name)),
                };
                let to = std::path::Path::new(&folder).join(file);
                (
                    organiser.arranged(part.clone()),
                    to.to_string_lossy().into_owned(),
                )
            })
            .collect();
        organiser.busy = true;
        let count = written.len();
        let into = crate::store::file_name(&folder);
        let taken = self.on_thread(
            move || {
                if let Some((_, taken)) = (written.iter()).find(|(_, to)| {
                    std::path::Path::new(to).exists()
                        || files.iter().any(|file| same_file(file, to))
                }) {
                    return Err(format!(
                        "{} is already in that folder. Choose another folder.",
                        crate::store::file_name(taken)
                    ));
                }
                let document = (document.as_ref()).map(|(p, w)| (p.as_str(), w.as_deref()));
                let mut lost = false;
                for (saved, (sheets, to)) in written.iter().enumerate() {
                    // Asked again just before, since a file put in the
                    // folder meanwhile would be written over, and then
                    // taken away with the rest.
                    let made = match std::path::Path::new(to).exists() {
                        true => Err(format!(
                            "{} is already in that folder. Choose another folder.",
                            crate::store::file_name(to)
                        )),
                        false => crate::arrange::assemble(document, sheets, to),
                    };
                    match made {
                        Ok(left) => lost |= left,
                        // The files written before this one go again, so
                        // that no half of the set is left in the folder and
                        // the draft can be saved into it again.
                        Err(said) => {
                            for (_, to) in &written[..saved] {
                                let _ = std::fs::remove_file(to);
                            }
                            return Err(said);
                        }
                    }
                }
                Ok::<bool, String>(lost)
            },
            move |viewer, saved| {
                // A failure keeps the draft, so it can be saved again.
                match &saved {
                    Ok(_) => viewer.organiser = None,
                    Err(_) => {
                        if let Some(organiser) = viewer.organiser.as_mut() {
                            organiser.busy = false;
                        }
                    }
                }
                viewer.notice = match saved {
                    Ok(lost) => {
                        let lost = if lost { LINKS_LEFT_BEHIND } else { "" };
                        format!("Saved {count} files in {into}.{lost}")
                    }
                    Err(said) => said,
                };
            },
        );
        if !taken {
            if let Some(organiser) = self.organiser.as_mut() {
                organiser.busy = false;
            }
            self.notice = Self::BUSY.into();
        }
    }

    /// "Save into paper.pdf": the draft written into the document itself,
    /// with undo in the reading view.
    pub fn save_into_document(&mut self) {
        let Some(organiser) = (self.organiser.as_ref()).filter(|o| !o.busy && !o.reading) else {
            return;
        };
        if !organiser.fits_document() {
            return;
        }
        if !organiser.changed() {
            self.organiser = None;
            return;
        }
        // A file still being added would land after the draft was written.
        if self.making.is_some() {
            self.notice = Self::BUSY.into();
            return;
        }
        let edit = Edit::Arrange(organiser.arranged(0..organiser.sheets.len()));
        if self.may_edit(&edit) {
            self.edit_in_place(edit, false);
        }
    }
}

/// A file added to the organiser, opened on a thread; `whole` says whether
/// it is a whole PDF (`watch::whole`). A picture is made into a PDF of one
/// page in the temporary directory, which the organiser removes when it
/// goes.
fn open_for_organiser(path: &str, whole: bool) -> Result<Taken, String> {
    let name = crate::store::file_name(path);
    let opened = |read: &str| {
        crate::render::open_with(read, None).map_err(|refused| match refused {
            crate::render::Refusal::Locked => {
                format!("{name} is encrypted, so its pages cannot be copied.")
            }
            crate::render::Refusal::Said(said) => format!("{name} could not be opened: {said}"),
        })
    };
    if whole {
        return Ok((path.to_string(), path.to_string(), opened(path)?, false));
    }
    made_for_organiser(crate::arrange::as_pdf(path), path)
}

/// `bytes`, a PDF made for the organiser, written to the temporary
/// directory and opened; the organiser removes the file when it goes.
/// `file` is what the source is said to come from.
fn made_for_organiser(bytes: Result<Vec<u8>, String>, file: &str) -> Result<Taken, String> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static MADE: AtomicUsize = AtomicUsize::new(0);
    let bytes = bytes?;
    let made = std::env::temp_dir().join(format!(
        "moonowl-{}-made-{}.pdf",
        std::process::id(),
        MADE.fetch_add(1, Ordering::Relaxed)
    ));
    crate::atomic_write(&made, &bytes)?;
    let made = made.to_string_lossy().into_owned();
    match crate::render::open_with(&made, None) {
        Ok(document) => Ok((file.to_string(), made, document, true)),
        Err(_) => {
            let _ = std::fs::remove_file(&made);
            Err(format!(
                "{} could not be made into a page.",
                crate::store::file_name(file)
            ))
        }
    }
}

/// What a blank page's source says it comes from.
const BLANK: &str = "blank";

/// Said after a save, when a page had a link to a page that did not come
/// along.
const LINKS_LEFT_BEHIND: &str = " Links in them to pages that were left out do not work.";
