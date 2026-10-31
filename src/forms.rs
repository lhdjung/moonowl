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
    let mut info = Box::new(FPDF_FORMFILLINFO {
        version: 2,
        Release: None,
        FFI_Invalidate: None,
        FFI_OutputSelectedRect: None,
        FFI_SetCursor: None,
        FFI_SetTimer: None,
        FFI_KillTimer: None,
        FFI_GetLocalTime: None,
        FFI_OnChange: None,
        FFI_GetPage: None,
        FFI_GetCurrentPage: None,
        FFI_GetRotation: None,
        FFI_ExecuteNamedAction: None,
        FFI_SetTextFieldFocus: None,
        FFI_DoURIAction: None,
        FFI_DoGoToAction: None,
        m_pJsPlatform: std::ptr::null_mut(),
        xfa_disabled: 1,
        FFI_DisplayCaret: None,
        FFI_GetCurrentPageIndex: None,
        FFI_SetCurrentPage: None,
        FFI_GotoURL: None,
        FFI_GetPageViewRect: None,
        FFI_PageEvent: None,
        FFI_PopupMenu: None,
        FFI_OpenFile: None,
        FFI_EmailTo: None,
        FFI_UploadTo: None,
        FFI_GetPlatform: None,
        FFI_GetLanguage: None,
        FFI_DownloadFromURL: None,
        FFI_PostRequestURL: None,
        FFI_PutRequestURL: None,
        FFI_OnFocusChange: None,
        FFI_DoURIActionWithKeyboardModifier: None,
    });
    let gone = || "That field is no longer there.".to_string();
    let form = unsafe { bindings.FPDFDOC_InitFormFillEnvironment(document, &mut *info) };
    if form.is_null() {
        return Err(gone());
    }
    let sheet = unsafe { bindings.FPDF_LoadPage(document, page.saturating_sub(1) as i32) };
    let done = !sheet.is_null() && unsafe { fill_page(bindings, form, sheet, spot, fill) };
    unsafe {
        if !sheet.is_null() {
            bindings.FORM_OnBeforeClosePage(sheet, form);
            bindings.FPDF_ClosePage(sheet);
        }
        bindings.FPDFDOC_ExitFormFillEnvironment(form);
    }
    drop(info);
    done.then_some(()).ok_or_else(gone)
}

/// The fill itself, on a page loaded into the form-fill environment.
/// Answers whether the field was there to fill.
unsafe fn fill_page(
    bindings: &dyn PdfiumLibraryBindings,
    form: FPDF_FORMHANDLE,
    sheet: FPDF_PAGE,
    spot: Spot,
    fill: &Fill,
) -> bool {
    unsafe {
        bindings.FORM_OnAfterLoadPage(sheet, form);
        let annotation = annotation_at(bindings, sheet, spot);
        if annotation.is_null() {
            return false;
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
                if placed {
                    let x = f64::from(rect.left + rect.right) / 2.0;
                    let y = f64::from(rect.top + rect.bottom) / 2.0;
                    bindings.FORM_OnLButtonDown(form, sheet, 0, x, y);
                    bindings.FORM_OnLButtonUp(form, sheet, 0, x, y);
                }
                placed
            }
        };
        // Leaving the field is what makes pdfium keep its value and
        // draw its appearance.
        bindings.FORM_ForceToKillFocus(form);
        bindings.FPDFPage_CloseAnnot(annotation);
        done
    }
}

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
/// puts the one in `spot` in order.
unsafe fn annotation_at(
    bindings: &dyn PdfiumLibraryBindings,
    sheet: FPDF_PAGE,
    spot: Spot,
) -> FPDF_ANNOTATION {
    unsafe {
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
        let order = std::iter::once(spot.index).chain((0..count).filter(|&at| at != spot.index));
        for at in order {
            let annotation = bindings.FPDFPage_GetAnnot(sheet, at as i32);
            if annotation.is_null() {
                continue;
            }
            let widget = bindings.FPDFAnnot_GetSubtype(annotation) == WIDGET
                && bindings.FPDFAnnot_GetFlags(annotation) & UNSEEN == 0;
            if widget && bounds(annotation).is_some_and(same) {
                return annotation;
            }
            bindings.FPDFPage_CloseAnnot(annotation);
        }
        std::ptr::null_mut()
    }
}
