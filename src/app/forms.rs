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
use crate::render::{Field, FieldKind};

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
    /// Onto the page, as a line of text ("Add text here…").
    Page,
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
        self.save_field();
        match field.kind {
            FieldKind::Text { .. } => {
                self.filling = Some(Filling {
                    page,
                    rect: field.rect,
                    draft: field.value.clone(),
                    goes: Goes::Field(field),
                });
            }
            FieldKind::Check(_) | FieldKind::Radio(_) => {
                self.fill_field(page, field.index, Fill::Click)
            }
        }
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
            Goes::Page => filling.draft.trim().is_empty(),
        };
        if unchanged {
            self.filling = None;
            return;
        }
        if self.busy() {
            self.fields_owed.extend(self.filling.take());
            self.notice = "The words will be written into the form once the work under way \
                           is done."
                .into();
            return;
        }
        let Some(filling) = self.filling.take() else {
            return;
        };
        match &filling.goes {
            Goes::Page => self.add_text(filling),
            Goes::Field(field) => {
                self.fill_field(filling.page, field.index, Fill::Text(filling.draft))
            }
        }
    }

    /// Before the document is put down, by closing it, its window or the
    /// app, or by opening another in its place: the words typed into a field
    /// are written first, and the document stays until they are. Answers
    /// whether it stays.
    pub(super) fn keeps_typed_words(&mut self) -> bool {
        if self.filling.is_none() && self.fields_owed.is_empty() {
            return false;
        }
        self.save_field();
        if !self.fields_owed.is_empty() {
            // `save_field` has said that the words wait for the work under way.
            return true;
        }
        let writing = self.writing.is_some();
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
        let room = self.document.size_of(index).width - x;
        // The field is drawn with its words at 70% of its height, so this
        // height shows them at the size they are written.
        let height = crate::sign::LINE_HEIGHT / 0.7;
        self.filling = Some(Filling {
            page,
            rect: Rect {
                left: x,
                top: y - height / 2.0,
                width: room.clamp(40.0, 300.0),
                height,
            },
            draft: String::new(),
            goes: Goes::Page,
        });
    }

    /// The words typed after "Add text here…", written on the page with
    /// their middle where the click was.
    fn add_text(
        &mut self,
        Filling {
            page, rect, draft, ..
        }: Filling,
    ) {
        let at = Rect {
            left: rect.left,
            top: rect.top + (rect.height - crate::sign::LINE_HEIGHT) / 2.0,
            width: 0.0,
            height: crate::sign::LINE_HEIGHT,
        };
        let done = format!("Written on page {}.", self.label(page));
        self.write_step(
            self.store.journal().to_vec(),
            move |path| crate::sign::place_text(path, page, at, &draft, crate::sign::INK),
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

    pub(super) fn fill_field(&mut self, page: usize, index: usize, fill: Fill) {
        // Whether the document can be written is known only once its
        // highlights have been read, and `busy` says so until then.
        if self.markup_reading.is_none() && !self.standing.fills {
            self.notice = format!(
                "{}, so the form cannot be filled in here.",
                self.standing.refused
            );
            return;
        }
        // A press elsewhere saves the words typed in a text field, so a box
        // clicked right after typing finds that write under way.
        if matches!(fill, Fill::Click) && self.writing.is_some() && !self.reloading {
            self.clicks_owed.push((page, index));
            return;
        }
        if self.busy() {
            return;
        }
        self.write_step(
            self.store.journal().to_vec(),
            move |path| crate::forms::fill(path, page, index, fill),
            |viewer, written| {
                if let Err(refused) = written {
                    viewer.notice = refused;
                }
            },
        );
    }
}
