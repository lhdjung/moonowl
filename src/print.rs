//! Printing, which is the system's on macOS, pdfium's on Windows, and a
//! hand-off to another program on Linux — see
//! [`crate::app::Printer::to_the_system`], which is also what both halves
//! here fall back to.
//!
//! **macOS.** The reader draws nothing for a printer: PDFKit's `PDFDocument`
//! makes an `NSPrintOperation` from the file, and that operation is the whole
//! of what Preview shows — the panel, the preview, page ranges, scaling,
//! PDF-as-output. It runs as a sheet on the reader's own window, so nothing
//! leaves the app. Main thread only, like every AppKit call: the shell
//! answers the ask from its event loop, which is that thread.
//!
//! **Windows.** There is no system print panel that takes a file, but there is
//! a system print *dialog* that hands back a printer, and pdfium has the one
//! function that draws a page into it: `FPDF_RenderPage` onto an `HDC`, which
//! is how Chrome prints. So `dialog` is comdlg32's dialog, `StartDoc`, a page
//! loop, `EndDoc`. It blocks for the length of the dialog and the length of
//! the job, so the shell runs it on a thread of its own; pdfium is not thread
//! safe, so every call into it here is taken behind [`crate::pdfium::library`],
//! one page at a time, so a reader can go on scrolling while a long document
//! spools.

#[cfg(target_os = "macos")]
mod mac {
    use objc2::runtime::{AnyObject, Sel};
    use objc2::{class, msg_send};
    use winit::window::Window;

    #[link(name = "PDFKit", kind = "framework")]
    extern "C" {}

    /// `kPDFPrintPageScaleDownToFit`: a page larger than the paper is shrunk
    /// to it and a smaller one is left alone, which is Preview's own default.
    const SCALE_DOWN_TO_FIT: isize = 2;

    /// The print panel for the document at `path`, as a sheet on `window`.
    /// Returns once the sheet is up; the panel runs on its own from there.
    pub fn sheet(window: &dyn Window, path: &str) -> Result<(), String> {
        let Some(window) = crate::tabs::ns_window(window) else {
            return Err("There is no window to print from.".to_string());
        };
        let c_path =
            std::ffi::CString::new(path).map_err(|_| "The path could not be read.".to_string())?;
        // SAFETY: every receiver below is a live object from the call above
        // it, checked for null where the call can fail, and every selector
        // carries the arguments AppKit and PDFKit document for it.
        unsafe {
            let string: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: c_path.as_ptr()];
            let url: *mut AnyObject = msg_send![class!(NSURL), fileURLWithPath: string];
            let document: *mut AnyObject = msg_send![class!(PDFDocument), alloc];
            let document: *mut AnyObject = msg_send![document, initWithURL: url];
            if document.is_null() {
                return Err("The document could not be read for printing.".to_string());
            }
            let info: *mut AnyObject = msg_send![class!(NSPrintInfo), sharedPrintInfo];
            let operation: *mut AnyObject = msg_send![
                document,
                printOperationForPrintInfo: info,
                scalingMode: SCALE_DOWN_TO_FIT,
                autoRotate: true
            ];
            // The operation holds what it prints from; our own reference is
            // the `alloc` above and goes back either way.
            let _: () = msg_send![document, release];
            if operation.is_null() {
                return Err("The document cannot be printed.".to_string());
            }
            let _: () = msg_send![
                operation,
                runOperationModalForWindow: window,
                delegate: std::ptr::null_mut::<AnyObject>(),
                didRunSelector: None::<Sel>,
                contextInfo: std::ptr::null_mut::<std::ffi::c_void>()
            ];
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub use mac::sheet;

#[cfg(target_os = "windows")]
mod win {
    use pdfium_render::prelude::*;
    use windows::Win32::Foundation::{GlobalFree, HWND};
    use windows::Win32::Graphics::Gdi::{
        DeleteDC, GetDeviceCaps, HORZRES, LOGPIXELSX, LOGPIXELSY, VERTRES,
    };
    use windows::Win32::Storage::Xps::{EndDoc, EndPage, StartDocW, StartPage, DOCINFOW};
    use windows::Win32::UI::Controls::Dialogs::{
        PrintDlgW, PD_NOSELECTION, PD_PAGENUMS, PD_RETURNDC, PD_USEDEVMODECOPIESANDCOLLATE,
        PRINTDLGW,
    };

    /// pdfium's own `FPDF_ANNOT | FPDF_PRINTING`, spelled out because the
    /// crate keeps its bindgen module private: draw the annotations, and
    /// draw for a printer — which honours a page's print-only content and
    /// skips its screen-only.
    const FLAGS: i32 = 0x01 | 0x800;
    /// `FPDF_ERR_PASSWORD`: the one open failure with a sentence of its own.
    const NEEDS_PASSWORD: u64 = 4;

    /// The print dialog for the document at `path`, owned by `hwnd`, and then
    /// the job. Blocks until both are done, so call it off the event loop.
    ///
    /// The name is what the spooler shows for the job.
    pub fn dialog(hwnd: isize, path: &str, name: &str) -> Result<(), String> {
        let pdfium = crate::pdfium::pdfium()?;
        let bindings = pdfium.bindings();

        // Opened again rather than through the reader's own `Document`: the
        // shell has a path and no document, and the file is what is printed.
        // ponytail: no password is offered, so a locked document is refused
        // here; pass the reader's password through `Printer` if that is ever
        // wanted.
        let (document, count) = {
            let _library = crate::pdfium::library();
            // SAFETY: plain pdfium calls behind the library's lock.
            let document = unsafe { bindings.FPDF_LoadDocument(path, None) };
            if document.is_null() {
                let error = unsafe { bindings.FPDF_GetLastError() } as u64;
                return Err(if error == NEEDS_PASSWORD {
                    "A locked document cannot be printed.".to_string()
                } else {
                    "The document could not be read for printing.".to_string()
                });
            }
            (document, unsafe { bindings.FPDF_GetPageCount(document) })
        };
        let close = || {
            let _library = crate::pdfium::library();
            unsafe { bindings.FPDF_CloseDocument(document) };
        };

        let mut dlg = PRINTDLGW {
            lStructSize: std::mem::size_of::<PRINTDLGW>() as u32,
            hwndOwner: HWND(hwnd as *mut _),
            Flags: PD_RETURNDC | PD_USEDEVMODECOPIESANDCOLLATE | PD_NOSELECTION,
            nFromPage: 1,
            nToPage: count.max(1) as u16,
            nMinPage: 1,
            nMaxPage: count.max(1) as u16,
            nCopies: 1,
            ..Default::default()
        };
        // SAFETY: a fully initialised PRINTDLGW, sized as the dialog checks.
        if !unsafe { PrintDlgW(&mut dlg) }.as_bool() {
            // Cancelled, which is nothing to report.
            close();
            return Ok(());
        }
        let hdc = dlg.hDC;
        let free = || unsafe {
            if !dlg.hDevMode.is_invalid() {
                let _ = GlobalFree(Some(dlg.hDevMode));
            }
            if !dlg.hDevNames.is_invalid() {
                let _ = GlobalFree(Some(dlg.hDevNames));
            }
        };

        let (first, last) = if dlg.Flags.contains(PD_PAGENUMS) {
            (dlg.nFromPage as i32, dlg.nToPage as i32)
        } else {
            (1, count)
        };

        // SAFETY: GDI calls on the DC the dialog just handed back.
        let printed = unsafe {
            let paper_w = GetDeviceCaps(Some(hdc), HORZRES);
            let paper_h = GetDeviceCaps(Some(hdc), VERTRES);
            let dpi_x = GetDeviceCaps(Some(hdc), LOGPIXELSX) as f64;
            let dpi_y = GetDeviceCaps(Some(hdc), LOGPIXELSY) as f64;
            let name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
            let info = DOCINFOW {
                cbSize: std::mem::size_of::<DOCINFOW>() as i32,
                lpszDocName: windows::core::PCWSTR(name.as_ptr()),
                ..Default::default()
            };
            if StartDocW(hdc, &info) <= 0 {
                Err("The printer would not start the job.".to_string())
            } else {
                for index in first..=last {
                    let _library = crate::pdfium::library();
                    let page = bindings.FPDF_LoadPage(document, index - 1);
                    if page.is_null() {
                        continue;
                    }
                    // Scaled down to fit the paper and never up, centred:
                    // what `kPDFPrintPageScaleDownToFit` does on the Mac.
                    let page_w = bindings.FPDF_GetPageWidthF(page) as f64 / 72.0 * dpi_x;
                    let page_h = bindings.FPDF_GetPageHeightF(page) as f64 / 72.0 * dpi_y;
                    let scale = (paper_w as f64 / page_w)
                        .min(paper_h as f64 / page_h)
                        .min(1.0);
                    let w = (page_w * scale).round() as i32;
                    let h = (page_h * scale).round() as i32;
                    let x = (paper_w - w) / 2;
                    let y = (paper_h - h) / 2;
                    if StartPage(hdc) > 0 {
                        bindings.FPDF_RenderPage(hdc, page, x, y, w, h, 0, FLAGS);
                        EndPage(hdc);
                    }
                    bindings.FPDF_ClosePage(page);
                }
                EndDoc(hdc);
                Ok(())
            }
        };
        unsafe {
            let _ = DeleteDC(hdc);
        }
        free();
        close();
        printed
    }
}

#[cfg(target_os = "windows")]
pub use win::dialog;
