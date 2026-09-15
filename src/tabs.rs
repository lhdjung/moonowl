//! Tabs, which are macOS's and are three calls away from winit's.
//!
//! A window on this platform can be a tab of another, and the system does it
//! on its own: `allowsAutomaticWindowTabbing` is on by default, and with
//! *Prefer tabs when opening documents* set to "In Full Screen" — which is
//! Apple's own default — every window this reader made while it was full
//! screen arrived as a tab of the one already there. That is a fine thing to
//! be able to do and a bad thing to have happen: ⌘N is a *window*, and a
//! reader who wanted a second window beside the first got neither a second
//! window nor any way to ask for one.
//!
//! So automatic tabbing is off (`shell.rs`, once, at startup) and a tab is
//! something asked for by name. Which leaves one call winit has no word for:
//! `addTabbedWindow:ordered:`, the explicit half of the same feature, which
//! goes on working with the automatic half switched off. Selecting a tab and
//! counting them winit *does* have — see `WindowExtMacOS` — so this file is
//! only the part that is missing.

use objc2::msg_send;
use objc2::runtime::AnyObject;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

/// `NSWindowAbove`: the new tab goes to the right of the one it joins, which
/// is where a tab opened from a tab belongs.
const ABOVE: isize = 1;

/// The `NSWindow` behind a winit window. `None` off AppKit, which cannot
/// happen here and is not worth a panic.
pub(crate) fn ns_window(window: &dyn Window) -> Option<*mut AnyObject> {
    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::AppKit(appkit) => {
            let view: *mut AnyObject = appkit.ns_view.as_ptr().cast();
            // SAFETY: an `ns_view` from a live window, and `window` is a
            // property every `NSView` has.
            Some(unsafe { msg_send![view, window] })
        }
        _ => None,
    }
}

/// Put `joining` into `front`'s tab group, and bring it forward.
///
/// Ordering the new window front is not decoration: `addTabbedWindow:` adds
/// the tab without selecting it, so a "New tab" that left the reader looking
/// at the tab they already had would read as having done nothing.
pub fn tab_onto(front: &dyn Window, joining: &dyn Window) {
    let (Some(front), Some(joining)) = (ns_window(front), ns_window(joining)) else {
        return;
    };
    unsafe {
        let _: () = msg_send![front, addTabbedWindow: joining, ordered: ABOVE];
        let _: () = msg_send![joining, makeKeyAndOrderFront: std::ptr::null_mut::<AnyObject>()];
    }
}
