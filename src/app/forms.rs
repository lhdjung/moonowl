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

/// The text field being typed in: its page (one-based), the field, and
/// what has been typed.
#[derive(Clone, Debug, PartialEq)]
pub struct Filling {
    pub page: usize,
    pub field: Field,
    pub draft: String,
    /// Whether this is new text on the page rather than a field of the
    /// form. The field is then only where the text goes.
    pub new_text: bool,
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
        if !self.save_field() {
            return;
        }
        match field.kind {
            FieldKind::Text { .. } => {
                self.filling = Some(Filling {
                    page,
                    draft: field.value.clone(),
                    field,
                    new_text: false,
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
    /// document, if they changed. While another write is under way the
    /// field stays open with the words in it, and `false` says so.
    pub fn save_field(&mut self) -> bool {
        let Some(filling) = &self.filling else {
            return true;
        };
        if filling.draft == filling.field.value
            || filling.new_text && filling.draft.trim().is_empty()
        {
            self.filling = None;
            return true;
        }
        if self.busy() {
            return false;
        }
        let Some(filling) = self.filling.take() else {
            return true;
        };
        match filling.new_text {
            true => self.add_text(filling),
            false => self.fill_field(filling.page, filling.field.index, Fill::Text(filling.draft)),
        }
        true
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
        if !self.save_field() {
            return;
        }
        let index = page.saturating_sub(1);
        let (x, y) = self.layout.unplace_on(index, on.0, on.1);
        let room = self.document.size_of(index).width - x;
        // The field is drawn with its words at 70% of its height, so this
        // height shows them at the size they are written.
        let height = crate::sign::LINE_HEIGHT / 0.7;
        self.filling = Some(Filling {
            page,
            field: Field {
                rect: Rect {
                    left: x,
                    top: y - height / 2.0,
                    width: room.clamp(40.0, 300.0),
                    height,
                },
                index: 0,
                kind: FieldKind::Text { multiline: false },
                value: String::new(),
            },
            draft: String::new(),
            new_text: true,
        });
    }

    /// The words typed after "Add text here…", written on the page with
    /// their middle where the click was.
    fn add_text(
        &mut self,
        Filling {
            page, field, draft, ..
        }: Filling,
    ) {
        let at = Rect {
            left: field.rect.left,
            top: field.rect.top + (field.rect.height - crate::sign::LINE_HEIGHT) / 2.0,
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

    fn fill_field(&mut self, page: usize, index: usize, fill: Fill) {
        if !self.standing.fills {
            self.notice = format!(
                "{}, so the form cannot be filled in here.",
                self.standing.refused
            );
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
