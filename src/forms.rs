//! Filling in a form: a value typed into a text field, or a box or a radio
//! button clicked, written into the document.
//!
//! The change goes through pdfium's form-fill environment, as if the reader
//! had typed or clicked in a program built on pdfium. pdfium then writes
//! the field's value, draws the field's appearance again, and turns off the
//! other buttons of a radio group, which setting `/V` by hand does not do.

use crate::objects::Update;
use crate::render::Spot;
use pdfium_render::prelude::{
    PdfiumLibraryBindings, PdfiumLibraryBindingsAccessor, FPDF_ANNOTATION, FPDF_DOCUMENT,
    FPDF_FORMFILLINFO, FPDF_FORMHANDLE, FPDF_PAGE, FS_RECTF,
};

/// What is done to a field.
#[derive(Clone, Debug, PartialEq)]
pub enum Fill {
    /// The text field's words replaced by these.
    Text(String),
    /// The check box or radio button clicked.
    Click,
}

/// Fill in the field at `spot` on `page` (one-based), and write the
/// document back. A signed document is written
/// as an incremental update, which a certification that allows form filling
/// accepts.
pub fn fill(path: &str, page: usize, spot: Spot, fill: Fill) -> Result<(), String> {
    crate::markup::rewrite(path, Update::Fields, |bytes| {
        let _library = crate::pdfium::library();
        let bindings = crate::pdfium::pdfium()?.bindings();
        let document = unsafe { bindings.FPDF_LoadMemDocument64(&bytes, None) };
        if document.is_null() {
            return Err(format!("{path} could not be read."));
        }
        let signed = crate::markup::signed(bindings, document);
        let filled = fill_in(bindings, document, page, spot, &fill)
            .and_then(|()| crate::arrange::save_raw(bindings, document));
        unsafe { bindings.FPDF_CloseDocument(document) };
        Ok(crate::markup::Saved {
            bytes: filled?,
            signed,
        })
    })
}

fn fill_in(
    bindings: &dyn PdfiumLibraryBindings,
    document: FPDF_DOCUMENT,
    page: usize,
    spot: Spot,
    fill: &Fill,
) -> Result<(), String> {
    // pdfium keeps a pointer to this for as long as the environment lasts,
    // so it is boxed and outlives it. No callback is needed: nothing is
    // drawn or shown. pdfium-render makes an environment of its own for a
    // document it loads, but keeps its handle private, and its fields are
    // filled by writing `/V`, so this one is made here.
    // Every callback is left out, which pdfium allows: the struct is
    // pointers and function pointers, for which zero is none.
    let mut info = Box::new(FPDF_FORMFILLINFO {
        version: 2,
        xfa_disabled: 1,
        ..unsafe { std::mem::zeroed() }
    });
    let gone = || GONE.to_string();
    let form = unsafe { bindings.FPDFDOC_InitFormFillEnvironment(document, &mut *info) };
    if form.is_null() {
        return Err(gone());
    }
    let sheet = unsafe { bindings.FPDF_LoadPage(document, page.saturating_sub(1) as i32) };
    let done = match sheet.is_null() {
        true => Err(gone()),
        false => unsafe { fill_page(bindings, form, sheet, spot, fill) }.map_err(|e| e.to_string()),
    };
    unsafe {
        if !sheet.is_null() {
            bindings.FORM_OnBeforeClosePage(sheet, form);
            bindings.FPDF_ClosePage(sheet);
        }
        bindings.FPDFDOC_ExitFormFillEnvironment(form);
    }
    drop(info);
    done
}

/// The fill itself, on a page loaded into the form-fill environment.
/// Answers why not, if the field was not there or did not change.
unsafe fn fill_page(
    bindings: &dyn PdfiumLibraryBindings,
    form: FPDF_FORMHANDLE,
    sheet: FPDF_PAGE,
    spot: Spot,
    fill: &Fill,
) -> Result<(), &'static str> {
    unsafe {
        bindings.FORM_OnAfterLoadPage(sheet, form);
        let annotation = annotation_at(bindings, form, sheet, spot);
        if annotation.is_null() {
            return Err(GONE);
        }
        let done = match fill {
            Fill::Text(text) => {
                let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
                let focused = bindings.is_true(bindings.FORM_SetFocusedAnnot(form, annotation));
                if focused {
                    bindings.FORM_SelectAllText(form, sheet);
                    bindings.FORM_ReplaceSelection(form, sheet, wide.as_ptr());
                }
                focused
            }
            Fill::Click => {
                let mut rect = FS_RECTF {
                    left: 0.0,
                    top: 0.0,
                    right: 0.0,
                    bottom: 0.0,
                };
                let placed = bindings.is_true(bindings.FPDFAnnot_GetRect(annotation, &mut rect));
                // pdfium clicks whatever is at the point, which may be
                // another widget over this one, so the click counts only if
                // this box changed.
                let on = || bindings.is_true(bindings.FPDFAnnot_IsChecked(form, annotation));
                let was = on();
                if placed {
                    let x = f64::from(rect.left + rect.right) / 2.0;
                    let y = f64::from(rect.top + rect.bottom) / 2.0;
                    bindings.FORM_OnLButtonDown(form, sheet, 0, x, y);
                    bindings.FORM_OnLButtonUp(form, sheet, 0, x, y);
                }
                match placed {
                    false => false,
                    true if on() == was => {
                        bindings.FORM_ForceToKillFocus(form);
                        bindings.FPDFPage_CloseAnnot(annotation);
                        return Err(UNMOVED);
                    }
                    true => true,
                }
            }
        };
        // Leaving the field is what makes pdfium keep its value and
        // draw its appearance.
        bindings.FORM_ForceToKillFocus(form);
        bindings.FPDFPage_CloseAnnot(annotation);
        done.then_some(()).ok_or(GONE)
    }
}

const GONE: &str = "That field is no longer there.";
const UNMOVED: &str = "That box did not change. Something else on the page is over it.";

/// pdfium's `FPDF_ANNOT_WIDGET`, the subtype of a form field's annotation.
const WIDGET: i32 = 20;

/// pdfium's `FPDF_ANNOT_FLAG_HIDDEN` and `FPDF_ANNOT_FLAG_NOVIEW`: a widget
/// that is not shown, which is not offered as a field.
const UNSEEN: i32 = (1 << 1) | (1 << 5);

/// How far apart two rectangles' edges may be, in points, and still be one
/// widget's. pdfium writes numbers with fewer digits than some producers
/// do, so a rectangle read again after a full rewrite is a little off.
const SAME_PLACE: f32 = 0.01;

/// The field's annotation, found by its rectangle. It is usually still at
/// its place in the page's list, but a write that took out an annotation
/// before it, such as a highlight, has moved it up. A rectangle written
/// right to left or top to bottom is put in order first, as pdfium-render
/// puts the one in `spot` in order. Two widgets can share a rectangle, so
/// a widget of another field is passed over.
unsafe fn annotation_at(
    bindings: &dyn PdfiumLibraryBindings,
    form: FPDF_FORMHANDLE,
    sheet: FPDF_PAGE,
    spot: Spot,
) -> FPDF_ANNOTATION {
    unsafe {
        let field = |annotation: FPDF_ANNOTATION| {
            let length =
                bindings.FPDFAnnot_GetFormFieldName(form, annotation, std::ptr::null_mut(), 0);
            let mut name = vec![0u16; (length as usize).div_ceil(2)];
            if length > 0 {
                bindings.FPDFAnnot_GetFormFieldName(form, annotation, name.as_mut_ptr(), length);
            }
            let name = String::from_utf16_lossy(&name);
            crate::pdfium::field_named(Some(name.trim_end_matches('\0')))
        };
        let bounds = |annotation: FPDF_ANNOTATION| {
            let mut rect = FS_RECTF {
                left: 0.0,
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
            };
            bindings
                .is_true(bindings.FPDFAnnot_GetRect(annotation, &mut rect))
                .then_some([
                    rect.left.min(rect.right),
                    rect.bottom.min(rect.top),
                    rect.left.max(rect.right),
                    rect.bottom.max(rect.top),
                ])
        };
        let same = |found: [f32; 4]| {
            (found.iter().zip(spot.bounds)).all(|(one, other)| (one - other).abs() <= SAME_PLACE)
        };
        let count = bindings.FPDFPage_GetAnnotCount(sheet).max(0) as usize;
        // Outward from its old place, nearest first, since a removal moves a
        // field by only a step or two.
        let mut order: Vec<usize> = (0..count).collect();
        order.sort_by_key(|&at| at.abs_diff(spot.index));
        for at in order {
            let annotation = bindings.FPDFPage_GetAnnot(sheet, at as i32);
            if annotation.is_null() {
                continue;
            }
            let widget = bindings.FPDFAnnot_GetSubtype(annotation) == WIDGET
                && bindings.FPDFAnnot_GetFlags(annotation) & UNSEEN == 0;
            if widget && bounds(annotation).is_some_and(same) && field(annotation) == spot.field {
                return annotation;
            }
            bindings.FPDFPage_CloseAnnot(annotation);
        }
        std::ptr::null_mut()
    }
}
