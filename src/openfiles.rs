//! Documents opened from the Finder, which do not arrive as arguments.
//!
//! **This is the hole `single.rs` names.** Every other way a document reaches
//! this reader is a launch with a path in `argv` — a terminal, `open -a`, "Open
//! with" on Linux, a second launch handing its path down the socket. The Finder
//! is not that: it activates the application and *then* sends it an Apple
//! Event, `'aevt'`/`'odoc'`. An application that does not answer it launches,
//! shows the start screen, and lets macOS report that it "cannot open files in
//! the PDF document format" — which is what somebody who has just made this
//! their default PDF reader sees, every time.
//!
//! **It is a delegate of our own, and that is measured rather than chosen.**
//! Two other routes were tried. Adding `application:openURLs:` to winit's
//! delegate class fails because there is no such object: `[NSApp delegate]` is
//! nil for the life of the process. Taking `'aevt'`/`'odoc'` off
//! `NSAppleEventManager` works for an application that is already running and
//! **loses the document on a cold launch** — the Finder's event is queued
//! before `NSApplication` finishes launching and dispatched as part of it, and
//! anything armed after that is armed too late, while anything armed before it
//! is replaced by AppKit's own handler. So the object below is set as the
//! application's delegate before the event loop starts, and AppKit's own
//! machinery delivers the queued event to it at the moment it is meant to.
//! Nothing is displaced: winit sets no delegate, and this one answers three
//! selectors and no others — the two that open a document, and the one that
//! turns ⌘Q into the app's own quit.

use std::ffi::{c_char, CStr};
use std::sync::OnceLock;

use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, NSObject, Sel};
use objc2::{msg_send, sel, ClassType};

use crate::shell::Remote;

/// How the handler reaches the shell. AppKit calls the method below with no
/// context of its own, so the only way to hand it anything is a static — the
/// same arrangement, and the same reason, as `dock.rs`.
static SHELL: OnceLock<Remote> = OnceLock::new();

fn tracing() -> bool {
    std::env::var_os("MOONOWL_TRACE").is_some()
}

/// What the Finder handed over before the launch window was made, or `None`
/// once it has been. AppKit delivers a cold launch's documents before
/// `applicationDidFinishLaunching:`, which is when winit asks for surfaces —
/// so the launch window can be *on* them rather than on the restored document
/// with them in a second window behind it. See [`launched`].
static EARLY: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(Some(Vec::new()));

/// Hand one path to the shell, which is where every other route ends too.
fn opened(path: String) {
    let Some(shell) = SHELL.get() else { return };
    if tracing() {
        eprintln!("openfiles: {path}");
    }
    if let Some(early) = EARLY.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        early.push(path);
        return;
    }
    shell.request(Some(path));
}

/// The launch is over: what arrived before it, and every later document goes
/// to the shell as it comes.
pub fn launched() -> Vec<String> {
    EARLY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .unwrap_or_default()
}

/// The POSIX path of an `NSURL`, or nothing if it is not a file URL.
unsafe fn path_of(url: *mut AnyObject) -> Option<String> {
    unsafe {
        if url.is_null() {
            return None;
        }
        let path: *mut AnyObject = msg_send![url, path];
        if path.is_null() {
            return None;
        }
        let utf8: *const c_char = msg_send![path, UTF8String];
        if utf8.is_null() {
            return None;
        }
        Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }
}

/// `-[NSApplicationDelegate application:openURLs:]`, which is what AppKit
/// calls on any system still supported. One call carries the whole of a
/// multiple selection.
extern "C" fn open_urls(
    _this: *mut AnyObject,
    _cmd: Sel,
    _app: *mut AnyObject,
    urls: *mut AnyObject,
) {
    unsafe {
        let count: usize = msg_send![urls, count];
        for index in 0..count {
            let url: *mut AnyObject = msg_send![urls, objectAtIndex: index];
            if let Some(path) = path_of(url) {
                opened(path);
            }
        }
    }
}

/// `-[NSApplicationDelegate application:openFile:]`, the older selector, kept
/// because it costs four lines and is what some senders still reach for.
extern "C" fn open_file(
    _this: *mut AnyObject,
    _cmd: Sel,
    _app: *mut AnyObject,
    path: *mut AnyObject,
) -> Bool {
    unsafe {
        if path.is_null() {
            return Bool::NO;
        }
        let utf8: *const c_char = msg_send![path, UTF8String];
        if utf8.is_null() {
            return Bool::NO;
        }
        opened(CStr::from_ptr(utf8).to_string_lossy().into_owned());
    }
    Bool::YES
}

/// `-[NSApplicationDelegate applicationShouldTerminate:]`.
///
/// ⌘Q from the application menu, Quit from the Dock and a log-out all arrive
/// as `terminate:`, and with nobody answering it AppKit ends the process from
/// inside `run_app` — so nothing after the event loop in `main.rs` ran: the
/// window's size was not written, `store::flush` never put down where the
/// reader was, and the socket was left behind. The answer is "not yet", and
/// the app's own quit is asked for instead, which closes every window through
/// the door a window closes through and returns from the event loop the
/// ordinary way.
///
/// **Warning: `NSTerminateCancel` is also the answer to a log-out and a
/// shutdown**, which arrive through the same selector, and to those it means
/// "this app refuses" — the system aborts the log-out and names the app, even
/// though the app then quits on its own. The right answer there is
/// `NSTerminateLater` (2) with `replyToApplicationShouldTerminate:` once the
/// writes are done, or the writes done here and `NSTerminateNow` (1).
extern "C" fn should_terminate(_this: *mut AnyObject, _cmd: Sel, _app: *mut AnyObject) -> usize {
    const NS_TERMINATE_CANCEL: usize = 0;
    if let Some(shell) = SHELL.get() {
        shell.quit();
    }
    NS_TERMINATE_CANCEL
}

/// Become the application's delegate, once, before the event loop starts.
pub fn install(shell: Remote) {
    if SHELL.set(shell).is_err() {
        return;
    }
    unsafe {
        let Some(mut builder) = ClassBuilder::new(c"MoonowlDelegate", NSObject::class()) else {
            return;
        };
        builder.add_method(
            sel!(application:openURLs:),
            open_urls as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject),
        );
        builder.add_method(
            sel!(application:openFile:),
            open_file as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject) -> Bool,
        );
        builder.add_method(
            sel!(applicationShouldTerminate:),
            should_terminate as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize,
        );
        let class = builder.register();
        // Never released: AppKit does not retain a delegate, and this one is
        // the delegate for as long as the process lives.
        let target: *mut AnyObject = msg_send![class, new];

        let app: *mut AnyObject = msg_send![
            AnyClass::get(c"NSApplication").expect("NSApplication"),
            sharedApplication
        ];
        let existing: *mut AnyObject = msg_send![app, delegate];
        if !existing.is_null() {
            // Somebody else's — winit's, one day. Theirs stays; a reader who
            // cannot close a window is worse off than one who cannot
            // double-click a document.
            if tracing() {
                eprintln!("openfiles: the application already has a delegate; left alone");
            }
            return;
        }
        let _: () = msg_send![app, setDelegate: target];
        if tracing() {
            eprintln!("openfiles: delegate set");
        }
    }
}
