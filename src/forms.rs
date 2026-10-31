//! Filling in a form: a value typed into a text field, or a box or a radio
//! button clicked, written into the document.
//!
//! The change goes through pdfium's form-fill environment, as if the reader
//! had typed or clicked in a program built on pdfium. pdfium then writes
//! the field's value, draws the field's appearance again, and turns off the
//! other buttons of a radio group, which setting `/V` by hand does not do.

use crate::objects::Update;
use pdfium_render::prelude::{
    PdfiumLibraryBindings, PdfiumLibraryBindingsAccessor, FPDF_DOCUMENT, FPDF_FORMFILLINFO,
    FPDF_FORMHANDLE, FPDF_PAGE, FS_RECTF,
};

/// What is done to a field.
#[derive(Clone, Debug, PartialEq)]
pub enum Fill {
    /// The text field's words replaced by these.
    Text(String),
    /// The check box or radio button clicked.
    Click,
}

/// Fill in the field that is annotation `index` (zero-based) on `page`
/// (one-based), and write the document back. A signed document is written
/// as an incremental update, which a certification that allows form filling
/// accepts.
pub fn fill(path: &str, page: usize, index: usize, fill: Fill) -> Result<(), String> {
    crate::markup::rewrite(path, Update::Fields, |bytes| {
        let _library = crate::pdfium::library();
        let bindings = crate::pdfium::pdfium()?.bindings();
        let document = unsafe { bindings.FPDF_LoadMemDocument64(&bytes, None) };
        if document.is_null() {
            return Err(format!("{path} could not be read."));
        }
        let signed = crate::markup::signed(bindings, document);
        let filled = fill_in(bindings, document, page, index, &fill)
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
    index: usize,
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
    let done = !sheet.is_null() && unsafe { fill_page(bindings, form, sheet, index, fill) };
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
    index: usize,
    fill: &Fill,
) -> bool {
    unsafe {
        bindings.FORM_OnAfterLoadPage(sheet, form);
        let annotation = bindings.FPDFPage_GetAnnot(sheet, index as i32);
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
