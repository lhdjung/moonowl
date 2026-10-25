//! A "New Window" item on the icon in the Dock, above the standard ones.
//!
//! The item posts to the shell proxy rather than making a window itself: a
//! window can only be made from inside a winit callback, so everything that
//! asks for one posts an event, and the Dock item needs no special case.
//!
//! winit does not reach `NSApplication`'s `dockMenu`, so this goes to the
//! Objective-C runtime directly. A menu item needs a *target*, and a target is
//! an object with a selector on it — so one class is built at runtime, one
//! instance of it is made, and both are left alive for as long as the process
//! is.
//!
//! It is also the only place in this reader that writes "New Window" in title
//! case. The Dock menu is the system's furniture and is spelled the system's
//! way.

use std::ffi::CStr;
use std::sync::OnceLock;

use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, NSObject, Sel};
use objc2::{msg_send, sel, ClassType};

use crate::shell::Remote;

/// How the item reaches the shell. AppKit calls the method below with no
/// context of its own, so the only way to hand it anything is a static.
static SHELL: OnceLock<Remote> = OnceLock::new();

unsafe fn string(text: &CStr) -> *mut AnyObject {
    let class = AnyClass::get(c"NSString").expect("NSString");
    unsafe { msg_send![class, stringWithUTF8String: text.as_ptr()] }
}

/// What the item does. Called on the main thread, which is exactly where the
/// event it posts will be answered.
extern "C" fn new_window(_this: *mut AnyObject, _cmd: Sel, _sender: *mut AnyObject) {
    if let Some(shell) = SHELL.get() {
        shell.request(None);
    }
}

pub fn install(shell: Remote) {
    if SHELL.set(shell).is_err() {
        return;
    }
    unsafe {
        let Some(mut builder) = ClassBuilder::new(c"MoonowlDock", NSObject::class()) else {
            return;
        };
        builder.add_method(
            sel!(moonowlNewWindow:),
            new_window as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
        );
        let class = builder.register();
        // Never released: it is the target of a menu item that lives as long
        // as the application does.
        let target: *mut AnyObject = msg_send![class, new];

        let menu: *mut AnyObject = msg_send![AnyClass::get(c"NSMenu").expect("NSMenu"), new];
        let item: *mut AnyObject =
            msg_send![AnyClass::get(c"NSMenuItem").expect("NSMenuItem"), alloc];
        let item: *mut AnyObject = msg_send![
            item,
            initWithTitle: string(c"New Window"),
            action: sel!(moonowlNewWindow:),
            keyEquivalent: string(c""),
        ];
        let _: () = msg_send![item, setTarget: target];
        let _: () = msg_send![menu, addItem: item];

        let shared: *mut AnyObject = msg_send![
            AnyClass::get(c"NSApplication").expect("NSApplication"),
            sharedApplication
        ];
        let _: () = msg_send![shared, setDockMenu: menu];
    }
}
