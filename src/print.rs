//! Printing, which on macOS is the system's own print panel and nothing else.
//!
//! The reader draws nothing for a printer: PDFKit's `PDFDocument` makes an
//! `NSPrintOperation` from the file, and that operation is the whole of what
//! Preview shows — the panel, the preview, page ranges, scaling, PDF-as-output.
//! It runs as a sheet on the reader's own window, so nothing leaves the app
//! and there is no other program to hand the document to. The other two
//! platforms still hand it off; see [`crate::app::Printer::to_the_system`].
//!
//! Main thread only, like every AppKit call: the shell answers the ask from
//! its event loop, which is that thread.

use objc2::runtime::{AnyObject, Sel};
use objc2::{class, msg_send};
use winit::window::Window;

#[link(name = "PDFKit", kind = "framework")]
extern "C" {}

/// `kPDFPrintPageScaleDownToFit`: a page larger than the paper is shrunk to
/// it and a smaller one is left alone, which is Preview's own default.
const SCALE_DOWN_TO_FIT: isize = 2;

/// The print panel for the document at `path`, as a sheet on `window`.
/// Returns once the sheet is up; the panel runs on its own from there.
pub fn sheet(window: &dyn Window, path: &str) -> Result<(), String> {
    let Some(window) = crate::tabs::ns_window(window) else {
        return Err("There is no window to print from.".to_string());
    };
    let c_path =
        std::ffi::CString::new(path).map_err(|_| "The path could not be read.".to_string())?;
    // SAFETY: every receiver below is a live object from the call above it,
    // checked for null where the call can fail, and every selector carries
    // the arguments AppKit and PDFKit document for it.
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
        // The operation holds what it prints from; our own reference is the
        // `alloc` above and goes back either way.
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
