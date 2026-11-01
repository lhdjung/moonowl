//! Filling in a form on the page. A click on a check box or a radio button
//! writes the click into the document at once; a click on a text field
//! opens a field over it, and what is typed there is written when the
//! reader presses Enter or Escape or clicks anywhere else. Every change is
//! a step of its own, so ⌘Z takes it back. See `forms.rs` for the writing.
//!
//! "Add text here…" in a page's right-click menu opens the same field where
//! the click was, for a form that is only a scan or a flat page. What is
//! typed goes in as a line of text, as the Sign window's typed line does
//! (`sign::place_text`).

use super::*;
use crate::forms::Fill;
use crate::render::{Field, FieldKind, Spot};

/// The text field being typed in: its page (one-based), where it is on the
/// page, what has been typed, and where the words go.
#[derive(Clone, Debug, PartialEq)]
pub struct Filling {
    pub page: usize,
    pub rect: Rect,
    pub draft: String,
    pub goes: Goes,
}

/// Where the words typed into a [`Filling`] go.
#[derive(Clone, Debug, PartialEq)]
pub enum Goes {
    /// Into this field of the form.
    Field(Field),
    /// Onto the page, as a line of text ("Add text here…"). `turn` is how
    /// far the view was turned when the field opened, so that the line
    /// reads across the page as the reader saw it.
    Page { turn: u32 },
}

impl Viewer {
    /// The form fields on a page, asked for once and kept, as the notes are.
    pub fn fields_on(&self, index: usize) -> Rc<Vec<Field>> {
        if let Some(known) = self.fields.borrow().get(&index) {
            return known.clone();
        }
        self.ask_annotations(index);
        self.fields
            .borrow()
            .get(&index)
            .cloned()
            .unwrap_or_default()
    }

    /// The form fields on a mounted page, placed in the same space as the
    /// links.
    pub fn field_areas(&self, page: usize) -> Vec<(Rect, Field)> {
        let Some(index) = page.checked_sub(1) else {
            return Vec::new();
        };
        if self.layout.box_of(index).is_none() {
            return Vec::new();
        }
        self.fields_on(index)
            .iter()
            .map(|field| (self.layout.place_on(index, field.rect), field.clone()))
            .collect()
    }

    /// A click on a field: a text field opens for typing, and a box or a
    /// button is clicked in the document.
    pub fn press_field(&mut self, page: usize, field: Field) {
        // The field open already, such as one whose words a signature's
        // lock refused, stays open with its words.
        if (self.filling.as_ref()).is_some_and(|filling| {
            filling.page == page && matches!(&filling.goes, Goes::Field(f) if f.spot == field.spot)
        }) {
            return;
        }
        self.save_field();
        match field.kind {
            FieldKind::Text { .. } => {
                if self.refuses_fields() {
                    return;
                }
                let this = (page, field.spot);
                if self.locked(page, field.spot) {
                    self.notice = crate::markup::LOCKED.into();
                    return;
                }
                if self.filled == Some(this) && self.writing.is_some() {
                    self.notice =
                        "Still writing the words into this field. Try again in a moment.".into();
                    return;
                }
                // Words typed into it before, which wait for a write, are
                // what it opens with, and are written when it closes.
                let owed = (self.fields_owed.iter()).rposition(|owed| {
                    matches!(&owed.goes, Goes::Field(f) if f.spot == field.spot)
                        && owed.page == page
                });
                let draft = match owed {
                    Some(at) => self.fields_owed.remove(at).draft,
                    None => field.value.clone(),
                };
                self.filling = Some(Filling {
                    page,
                    rect: field.rect,
                    draft,
                    goes: Goes::Field(field),
                });
            }
            // A radio button that is on stays on when it is clicked. While
            // another button of its group waits to be written, the page
            // still shows this one on: the reader has changed their mind,
            // and the waiting click is dropped. While another button of
            // its group is being written, this one is chosen after it.
            FieldKind::Radio(true) => {
                let (spot, group) = (field.spot, field.spot.field);
                if group != 0 {
                    (self.clicks_owed).retain(|&(on, other)| on != page || other.field != group);
                }
                let written = (self.filled).is_some_and(|(on, other)| {
                    on == page && group != 0 && other.field == group && other != spot
                });
                if written && self.writing.is_some() {
                    self.fill_field_then(page, spot, Fill::Click, None, false);
                }
            }
            FieldKind::Check(_) => self.fill_field(page, field.spot, Fill::Click),
            // Another button of the group chosen while the last choice
            // waits replaces it, and one chosen while the last choice is
            // written is written after it: the last button chosen is the
            // one that is on.
            FieldKind::Radio(false) => {
                let group = field.spot.field;
                if group != 0 {
                    (self.clicks_owed).retain(|&(on, other)| on != page || other.field != group);
                }
                self.fill_field_then(page, field.spot, Fill::Click, None, false);
            }
        }
    }

    /// Whether the document is known not to take a filled-in form, which
    /// is said. Whether it can is known only once its highlights have been
    /// read.
    fn refuses_fields(&mut self) -> bool {
        let refuses = self.markup_reading.is_none() && !self.standing.fills;
        if refuses {
            self.notice = format!(
                "{}, so the form cannot be filled in here.",
                self.standing.refused
            );
        }
        refuses
    }

    pub fn type_field(&mut self, typed: &str) {
        if let Some(filling) = &mut self.filling {
            filling.draft = typed.to_string();
        }
    }

    /// Enter, Escape, or a press anywhere else: the words go into the
    /// document, if they changed. While another write or a thread reading
    /// the document is under way, the field closes and its words are
    /// written when that lands.
    pub fn save_field(&mut self) {
        let Some(filling) = &self.filling else {
            return;
        };
        let unchanged = match &filling.goes {
            Goes::Field(field) => filling.draft == field.value,
            Goes::Page { .. } => filling.draft.trim().is_empty(),
        };
        if unchanged {
            self.filling = None;
            return;
        }
        let onto_page = matches!(filling.goes, Goes::Page { .. });
        if self.busy() {
            self.notice = match onto_page {
                false => {
                    "The words will be written into the form once the work \
                                   under way is done."
                }
                true => {
                    "The words will be written on the page once the work \
                                      under way is done."
                }
            }
            .into();
            self.fields_owed.extend(self.filling.take());
            return;
        }
        let Some(filling) = self.filling.take() else {
            return;
        };
        match &filling.goes {
            &Goes::Page { turn } => self.add_text(filling, turn),
            // Its words were refused once and are not written again; the
            // reader was told, and has had the field open to copy them.
            Goes::Field(field) if self.locked(filling.page, field.spot) => {}
            Goes::Field(field) => {
                let (page, spot) = (filling.page, field.spot);
                let draft = filling.draft.clone();
                self.fill_field_then(page, spot, Fill::Text(draft), Some(filling), true);
            }
        }
    }

    /// Before the document is put down, by closing it, its window or the
    /// app, or by opening another in its place: the words typed into a field,
    /// and the boxes clicked while a write was under way, are written first,
    /// and the document stays until they are. Answers whether it stays.
    pub(super) fn keeps_typed_words(&mut self) -> bool {
        if self.filling.is_none() && self.fields_owed.is_empty() && self.clicks_owed.is_empty() {
            return false;
        }
        let idle = self.writing.is_none();
        self.save_field();
        if !self.fields_owed.is_empty() || !self.clicks_owed.is_empty() {
            if self.writing.is_none() {
                // Nothing is being written: a thread reading the document
                // holds the form back. Text being recognised is stopped,
                // since the window is going.
                self.notice = match &self.recognising {
                    Some(stop) => {
                        stop.store(true, std::sync::atomic::Ordering::Relaxed);
                        "Recognising text was stopped so that the form can be written. \
                         Try again in a moment."
                    }
                    None => "The form is written once the work under way is done. Try again then.",
                }
                .into();
            } else if self.fields_owed.is_empty() {
                self.notice =
                    "Writing the boxes clicked in the form. Try again in a moment.".into();
            }
            // Otherwise `save_field` has said that the words wait.
            return true;
        }
        // Only a write `save_field` began is one of words typed. A field
        // closed with nothing changed in it writes nothing.
        let writing = idle && self.writing.is_some();
        if writing {
            self.notice = "Writing the words typed into the form. Try again in a moment.".into();
        }
        writing
    }

    /// "Add text here…": an empty field where the right-click was, as wide
    /// as the page leaves room for, up to a long line.
    pub fn add_text_at(&mut self, page: usize, on: (f64, f64)) {
        self.close_menu();
        if self.markup_reading.is_some() {
            self.notice = "Still reading this document. Try again in a moment.".into();
            return;
        }
        if !self.standing.into_file {
            self.notice = format!("{}, so no text can be added to it.", self.standing.refused);
            return;
        }
        self.save_field();
        let index = page.saturating_sub(1);
        let (x, y) = self.layout.unplace_on(index, on.0, on.1);
        let turn = self.layout.rotation;
        let size = self.document.size_of(index);
        // How far the page goes to the right of the click, as it is seen.
        let room = match turn {
            90 => y,
            180 => x,
            270 => size.height - y,
            _ => size.width - x,
        };
        // The field is drawn with its words at 70% of its height, so this
        // height shows them at the size they are written. It runs to the
        // right of the click as the page is seen, with its middle on it.
        let height = crate::sign::LINE_HEIGHT / 0.7;
        let (right, down) = crate::sign::view_axes(turn);
        let at = |along: f64, across: f64| {
            (
                x + right.0 * along + down.0 * across,
                y + right.1 * along + down.1 * across,
            )
        };
        let (one, two) = (
            at(0.0, -height / 2.0),
            at(room.clamp(40.0, 300.0), height / 2.0),
        );
        self.filling = Some(Filling {
            page,
            rect: Rect {
                left: one.0.min(two.0),
                top: one.1.min(two.1),
                width: (one.0 - two.0).abs(),
                height: (one.1 - two.1).abs(),
            },
            draft: String::new(),
            goes: Goes::Page { turn },
        });
    }

    /// The words typed after "Add text here…", written on the page with
    /// their middle where the click was.
    fn add_text(
        &mut self,
        Filling {
            page, rect, draft, ..
        }: Filling,
        turn: u32,
    ) {
        // The field's top left corner as it was seen, and the line's top
        // below it by half of what the field has to spare.
        let corner = match turn {
            90 => (rect.left, rect.top + rect.height),
            180 => (rect.left + rect.width, rect.top + rect.height),
            270 => (rect.left + rect.width, rect.top),
            _ => (rect.left, rect.top),
        };
        let (_, down) = crate::sign::view_axes(turn);
        let spare = (crate::sign::LINE_HEIGHT / 0.7 - crate::sign::LINE_HEIGHT) / 2.0;
        let at = Rect {
            left: corner.0 + down.0 * spare,
            top: corner.1 + down.1 * spare,
            width: 0.0,
            height: crate::sign::LINE_HEIGHT,
        };
        let done = format!("Written on page {}.", self.label(page));
        self.write_step(
            self.store.journal().to_vec(),
            move |path| crate::sign::place_text(path, page, at, turn, &draft, crate::sign::INK),
            move |viewer, written| {
                viewer.notice = match written {
                    Ok(()) => done,
                    Err(refused) => refused,
                }
            },
        );
    }

    /// The text fields closed while the document was busy, written now
    /// that it is free. The first starts a write, and the others wait for
    /// it in turn.
    pub(super) fn write_owed_fields(&mut self) {
        // A field opened since then stays open.
        let open = self.filling.take();
        for filling in std::mem::take(&mut self.fields_owed) {
            self.filling = Some(filling);
            self.save_field();
        }
        self.filling = open;
    }

    /// Whether a signature's lock refused the field at `spot`. The lock
    /// names the field, so any widget of it is locked, and a write that
    /// moved it in the page's list does not forget it.
    fn locked(&self, page: usize, spot: Spot) -> bool {
        (self.fields_locked.iter()).any(|&(on, other)| match spot.field {
            0 => on == page && other.bounds == spot.bounds,
            field => other.field == field,
        })
    }

    pub(super) fn fill_field(&mut self, page: usize, spot: Spot, fill: Fill) {
        self.fill_field_then(page, spot, fill, None, true);
    }

    /// [`Self::fill_field`] for the words typed into `typed`. Words that a
    /// signature's lock refuses are put back in the field, so that they
    /// can be copied, and the field is not opened for typing again.
    ///
    /// `whole_field` counts a click on another widget of the same field as
    /// a click on this one: a check box drawn twice is one box, and a
    /// group of radio buttons is not.
    fn fill_field_then(
        &mut self,
        page: usize,
        spot: Spot,
        fill: Fill,
        typed: Option<Filling>,
        whole_field: bool,
    ) {
        // `busy` says so while the highlights are still being read.
        if self.refuses_fields() {
            return;
        }
        // pdfium draws the box, so it shows its old state until the click
        // is written. A second click on it meanwhile, or on another widget
        // of the same field, is taken as the reader seeing nothing happen,
        // not as a click to undo the first.
        let this = (page, spot);
        let same = |&(on, other): &(usize, Spot)| {
            on == page
                && (other == spot || (whole_field && spot.field != 0 && other.field == spot.field))
        };
        let pending = self.filled.as_ref().is_some_and(same) || self.clicks_owed.iter().any(same);
        let reading = self.making.is_some() && self.making_reads != Reads::Nothing;
        if matches!(fill, Fill::Click) && pending && (self.writing.is_some() || reading) {
            self.notice = "Still writing the last click into the document.".into();
            return;
        }
        // A press elsewhere saves the words typed in a text field, so a box
        // clicked right after typing finds that write under way.
        // A thread reading the document holds it up as long, and the click
        // waits for it in the same way.
        if matches!(fill, Fill::Click) && (self.writing.is_some() || reading) && !self.reloading {
            self.clicks_owed.push((page, spot));
            return;
        }
        if self.busy() {
            return;
        }
        self.filled = Some(this);
        self.write_step(
            self.store.journal().to_vec(),
            move |path| crate::forms::fill(path, page, spot, fill),
            move |viewer, written| {
                viewer.filled = None;
                let Err(refused) = written else {
                    return;
                };
                viewer.notice = refused;
                if viewer.notice != crate::markup::LOCKED {
                    return;
                }
                viewer.fields_locked.push((page, spot));
                if let Some(typed) = typed.filter(|_| viewer.filling.is_none()) {
                    viewer.filling = Some(typed);
                    viewer
                        .notice
                        .push_str(" The words are still in the field, to be copied.");
                }
            },
        );
    }
}
