//! Filling in a form on the page. A click on a check box or a radio button
//! writes the click into the document at once; a click on a text field
//! opens a field over it, and what is typed there is written when the
//! reader presses Enter or Escape or clicks anywhere else. Every change is
//! a step of its own, so ⌘Z takes it back. See `forms.rs` for the writing.

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
        if filling.draft == filling.field.value {
            self.filling = None;
            return true;
        }
        if self.busy() {
            return false;
        }
        let Some(Filling { page, field, draft }) = self.filling.take() else {
            return true;
        };
        self.fill_field(page, field.index, Fill::Text(draft));
        true
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
