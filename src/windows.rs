//! Which window is showing what, where the next one goes, and what a window
//! going means.
//!
//! This is `OpenDocuments`, `Placements`, `Exiting`, `placement()` and the
//! decision half of `hand_over()` out of the app's `lib.rs`, **with every
//! mention of a webview taken out** — which is the point, because in the app
//! none of it can be tested: it is written against `AppHandle`, `State` and
//! `WebviewWindow`, so asking "what would happen if a second file arrived
//! now" means having an application running to ask. Here the rules are a
//! module with no window in it and the tests are at the bottom of the file.
//! `session.rs` is the half that actually makes windows.
//!
//! The three rules, in the order they were learned:
//!
//! *Nothing is ever displaced.* A document handed over by the system goes to
//! a window with nothing in it, or to a window made for it — never over the
//! top of what somebody is reading.
//!
//! *A document already open is brought to the front rather than opened
//! again.*
//!
//! *A window going means two things, and only [`Desk::leaving`] tells them
//! apart.* Closed by the reader it is a document they have finished with;
//! closed because the app is going it means only that it was open at the end,
//! which is what the next launch puts back.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The window the app launches with, and the only one with a name of its own.
///
/// Every other is `reader-N`. The names matter for one reason and it is
/// `watch.rs`: a document is followed per window and reported by
/// `emit_to(label, …)`, so a label is how a recompiled paper finds the window
/// reading it. See [`crate::emit::Exchange`].
pub const MAIN: &str = "main";

/// How far a new window is stepped down from the one in front of it, so that
/// two windows are two windows rather than one with a stack behind it. The
/// app's number.
pub const CASCADE: f64 = 28.0;

/// What to do with a document somebody has handed us.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Handover {
    /// It is already open in this window. Bring that window forward — the
    /// reader is asking to look at it, not for a second copy of it.
    Front(String),
    /// This window has nothing in it. Give it the document.
    Fill(String),
    /// Every window is busy. Make one.
    Spawn,
}

/// What each window is showing, which window is in front, and whether the app
/// is on its way out.
///
/// Shared rather than owned: the shell has it, and so does whatever answers
/// the door — the Dock menu item, the single-instance listener — both of
/// which are on threads of their own.
#[derive(Clone, Default)]
pub struct Desk(Arc<Inner>);

#[derive(Default)]
struct Inner {
    /// Window label to the document it has open, in the order the windows
    /// were made. The order is what `library.toml` records, so a session
    /// comes back in the order it was left.
    showing: Mutex<Vec<(String, String)>>,
    /// Every window that exists, in the order they were made — which is not
    /// the same list as `showing`, and the difference is the whole point of
    /// it: a window with nothing in it is a window, and it is exactly the one
    /// a handed-over document should go to.
    ///
    /// It was not needed while a window was always made *for* a document,
    /// because then the two lists were the same list. See [`Desk::idle`].
    made: Mutex<Vec<String>>,
    /// Which window has the keyboard, as winit last reported it.
    front: Mutex<Option<String>>,
    exiting: AtomicBool,
    next: AtomicU64,
}

impl Desk {
    pub fn new() -> Desk {
        Desk::default()
    }

    /// The name for the next window: `main` for the first, then `reader-1`.
    ///
    /// Minting a name is also how a window is entered on the desk, because
    /// this is the one thing every window goes through and it happens before
    /// the window exists. A window whose document then fails to open is
    /// entered and never removed — which is a label in a list and no window
    /// behind it, so a document handed to it would go nowhere. `Session` calls
    /// [`Desk::gone`] on that path for the same reason it calls it on a close.
    pub fn name(&self) -> String {
        let label = match self.0.next.fetch_add(1, Ordering::Relaxed) {
            0 => MAIN.to_string(),
            n => format!("reader-{n}"),
        };
        self.0
            .made
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(label.clone());
        label
    }

    /// A window that is not there any more, or never was.
    pub fn gone(&self, window: &str) {
        self.0
            .made
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|label| label != window);
    }

    /// Every window, in the order they were made.
    pub fn windows(&self) -> Vec<String> {
        self.0
            .made
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Record what a window is showing, and report the whole list as it now
    /// stands — which is what `library.toml` wants.
    pub fn set(&self, window: &str, path: Option<&str>) -> Vec<String> {
        let mut held = self.0.showing.lock().unwrap_or_else(|e| e.into_inner());
        match path {
            Some(path) => match held.iter_mut().find(|(label, _)| label == window) {
                Some(slot) => slot.1 = path.to_string(),
                None => held.push((window.to_string(), path.to_string())),
            },
            None => held.retain(|(label, _)| label != window),
        }
        held.iter().map(|(_, path)| path.clone()).collect()
    }

    /// What every window is showing, in the order they were made.
    pub fn open(&self) -> Vec<String> {
        let held = self.0.showing.lock().unwrap_or_else(|e| e.into_inner());
        held.iter().map(|(_, path)| path.clone()).collect()
    }

    pub fn count(&self) -> usize {
        self.0
            .showing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// Whichever window winit last said had the keyboard.
    pub fn focused(&self, window: Option<&str>) {
        let mut front = self.0.front.lock().unwrap_or_else(|e| e.into_inner());
        *front = window.map(str::to_string);
    }

    pub fn front(&self) -> Option<String> {
        self.0
            .front
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Where a document handed to us by the system should go.
    ///
    /// `Fill` is for a window with nothing in it: the start screen, or one
    /// whose document was closed. It is this desk's *belief* that the window
    /// is empty, and the belief can be a turn old — three documents opened at
    /// once all see the same empty window — so the window is what decides: it
    /// sends on what it has no room for. See `"handed-over"` in `app.rs`.
    pub fn hand_over(&self, path: &str) -> Handover {
        if let Some(label) = self.shown_by(path) {
            return Handover::Front(label);
        }
        match self.idle() {
            Some(label) => Handover::Fill(label),
            None => Handover::Spawn,
        }
    }

    /// The window showing this document, if one is. The reader asks before
    /// opening a document in a window of its own, because two windows on one
    /// file each write the whole of its marks and journal, and the last one
    /// wins.
    pub fn shown_by(&self, path: &str) -> Option<String> {
        let held = self.0.showing.lock().unwrap_or_else(|e| e.into_inner());
        held.iter()
            .find(|(_, open)| open == path)
            .map(|(label, _)| label.clone())
    }

    /// A window with nothing in it, the one with the keyboard first.
    ///
    /// **The one with the keyboard first, and then any of them**, which is
    /// the app's own rule stated in full. It used to be the front window
    /// alone, which was not a shortcut: there was no list of windows that are
    /// not showing anything to walk, because there were no such windows. Now
    /// that closing a document leaves one, a reader with two windows open and
    /// the empty one behind the full one would otherwise get a third.
    fn idle(&self) -> Option<String> {
        let busy: Vec<String> = {
            let held = self.0.showing.lock().unwrap_or_else(|e| e.into_inner());
            held.iter().map(|(label, _)| label.clone()).collect()
        };
        if let Some(front) = self.front() {
            if !busy.contains(&front) {
                return Some(front);
            }
        }
        self.windows()
            .into_iter()
            .find(|label| !busy.contains(label))
    }

    /// The app is going. Raised before the first window closes, by everything
    /// that ends the run.
    pub fn leaving(&self) {
        self.0.exiting.store(true, Ordering::Relaxed);
    }

    pub fn is_leaving(&self) -> bool {
        self.0.exiting.load(Ordering::Relaxed)
    }

    /// A window has gone. Answers the list to write down, or `None` for
    /// "write nothing".
    ///
    /// Two cases write nothing, and they are the whole of the rule. The app
    /// is quitting, in which case every window is about to go and the list as
    /// it stands is what the next launch is meant to put back. Or this was
    /// the last window — which on every platform ends the app and is how most
    /// people quit it, and where nothing separates "I have finished with
    /// this" from "goodbye". So a close can forget any window but the last,
    /// and quitting with one document open still comes back to it.
    pub fn closing(&self, window: &str) -> Option<Vec<String>> {
        self.gone(window);
        if self.is_leaving() {
            return None;
        }
        let remaining = self.set(window, None);
        // The last *window*, not the last document: with a start screen
        // still up the app goes on, and the document just closed was closed.
        if self.windows().is_empty() {
            return None;
        }
        Some(remaining)
    }
}

/// Where to put a new window: one step down from the window in front of it,
/// keeping its left and right edges and its bottom edge — so the new window
/// is `CASCADE` shorter, and nothing is ever cut off at the right or the
/// bottom. Stepping down *and across* was the classic cascade, and off a
/// maximized window it put every step's worth of the new window past the
/// edge of the screen. On again while the spot is taken.
///
/// Off the *front* window's frame rather than the remembered position:
/// restoring three windows makes them in one burst, so all three would cascade
/// off the same number and land within a few pixels of each other.
///
/// `front` is `(x, y, width, height)` in logical pixels — the outer corner and
/// the surface size, which is what a window can be asked to be. `None` means
/// there is nothing to cascade from, and the window is left where the platform
/// puts it.
///
/// **What the app needs here and this does not is `Placements`**, because
/// showing a window on macOS moves it onto the launch window's frame — so the
/// spot has to be applied again afterwards, and windows still coming up have to
/// be counted as taken. Here a window is made and positioned in one function
/// with nothing on screen in between.
pub fn cascade(
    front: Option<(f64, f64, f64, f64)>,
    taken: &[(f64, f64)],
) -> Option<(f64, f64, f64, f64)> {
    let (x, mut y, width, mut height) = front?;
    y += CASCADE;
    height -= CASCADE;
    // Bounded: a screen this far down is a screen nobody has, and an unbounded
    // walk here would be an unbounded walk off the display.
    for _ in 0..16 {
        let clash = taken
            .iter()
            .any(|(tx, ty)| (tx - x).abs() < 2.0 && (ty - y).abs() < 2.0);
        if !clash {
            break;
        }
        y += CASCADE;
        height -= CASCADE;
    }
    Some((x, y, width, height.max(CASCADE * 8.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_are_named_the_way_the_app_names_them() {
        let desk = Desk::new();
        assert_eq!(desk.name(), "main");
        assert_eq!(desk.name(), "reader-1");
        assert_eq!(desk.name(), "reader-2");
    }

    #[test]
    fn a_document_already_open_comes_to_the_front() {
        let desk = Desk::new();
        desk.set("main", Some("/papers/one.pdf"));
        desk.set("reader-1", Some("/papers/two.pdf"));
        assert_eq!(
            desk.hand_over("/papers/two.pdf"),
            Handover::Front("reader-1".into())
        );
    }

    #[test]
    fn a_document_nothing_is_showing_gets_a_window_of_its_own() {
        let desk = Desk::new();
        desk.set("main", Some("/papers/one.pdf"));
        assert_eq!(desk.hand_over("/papers/three.pdf"), Handover::Spawn);
    }

    /// The arm this reader cannot reach on its own — see [`Desk::hand_over`].
    /// A window that exists and is showing nothing is what a failed open
    /// leaves behind, and the rule for it is the app's.
    #[test]
    fn a_window_with_nothing_in_it_is_filled_rather_than_displaced() {
        let desk = Desk::new();
        desk.set("main", Some("/papers/one.pdf"));
        desk.focused(Some("reader-1"));
        assert_eq!(
            desk.hand_over("/papers/two.pdf"),
            Handover::Fill("reader-1".into())
        );
    }

    #[test]
    fn nothing_is_ever_displaced() {
        let desk = Desk::new();
        desk.set("main", Some("/papers/one.pdf"));
        desk.focused(Some("main"));
        assert_eq!(desk.hand_over("/papers/two.pdf"), Handover::Spawn);
    }

    #[test]
    fn what_is_open_is_in_the_order_the_windows_were_made() {
        let desk = Desk::new();
        desk.set("main", Some("/a.pdf"));
        desk.set("reader-1", Some("/b.pdf"));
        desk.set("reader-2", Some("/c.pdf"));
        assert_eq!(desk.open(), vec!["/a.pdf", "/b.pdf", "/c.pdf"]);
        // A window that opens something else replaces its own entry rather
        // than adding one.
        desk.set("reader-1", Some("/d.pdf"));
        assert_eq!(desk.open(), vec!["/a.pdf", "/d.pdf", "/c.pdf"]);
    }

    #[test]
    fn a_window_the_reader_closed_is_forgotten() {
        let desk = Desk::new();
        for _ in 0..2 {
            desk.name();
        }
        desk.set("main", Some("/a.pdf"));
        desk.set("reader-1", Some("/b.pdf"));
        assert_eq!(desk.closing("reader-1"), Some(vec!["/a.pdf".to_string()]));
    }

    /// The case no flag can reach. Closing the last window ends the app on
    /// every platform, and there is nothing there to separate "finished with
    /// it" from "goodbye" — so the list is left alone and the next launch
    /// comes back to it.
    #[test]
    fn closing_the_last_window_writes_nothing() {
        let desk = Desk::new();
        desk.set("main", Some("/a.pdf"));
        assert_eq!(desk.closing("main"), None);
    }

    /// …but a start screen left up is a window, and the app goes on: the
    /// document closed beside it was closed, and does not come back.
    #[test]
    fn closing_the_last_document_beside_an_empty_window_forgets_it() {
        let desk = Desk::new();
        for _ in 0..2 {
            desk.name();
        }
        desk.set("main", Some("/a.pdf"));
        assert_eq!(desk.closing("main"), Some(vec![]));
    }

    #[test]
    fn quitting_forgets_nothing() {
        let desk = Desk::new();
        desk.set("main", Some("/a.pdf"));
        desk.set("reader-1", Some("/b.pdf"));
        desk.leaving();
        assert_eq!(desk.closing("reader-1"), None);
        assert_eq!(desk.closing("main"), None);
        // And what was open at the end is still the whole of it.
        assert_eq!(desk.open(), vec!["/a.pdf", "/b.pdf"]);
    }

    /// Three windows closed one at a time come back as the third alone, which
    /// is what a reader who closed them means and as close to it as the
    /// platforms allow.
    #[test]
    fn three_windows_closed_one_at_a_time_come_back_as_one() {
        let desk = Desk::new();
        for _ in 0..3 {
            desk.name();
        }
        desk.set("main", Some("/a.pdf"));
        desk.set("reader-1", Some("/b.pdf"));
        desk.set("reader-2", Some("/c.pdf"));
        assert_eq!(
            desk.closing("main"),
            Some(vec!["/b.pdf".into(), "/c.pdf".into()])
        );
        assert_eq!(desk.closing("reader-1"), Some(vec!["/c.pdf".to_string()]));
        assert_eq!(desk.closing("reader-2"), None);
    }

    #[test]
    fn a_new_window_steps_down_off_the_one_in_front_and_keeps_its_edges() {
        assert_eq!(
            cascade(Some((100.0, 100.0, 800.0, 600.0)), &[]),
            Some((100.0, 128.0, 800.0, 572.0))
        );
    }

    #[test]
    fn and_on_again_while_the_spot_is_taken() {
        let taken = [(100.0, 100.0), (100.0, 128.0), (100.0, 156.0)];
        assert_eq!(
            cascade(Some((100.0, 100.0, 800.0, 600.0)), &taken),
            Some((100.0, 184.0, 800.0, 516.0))
        );
    }

    #[test]
    fn with_no_window_in_front_there_is_nothing_to_step_off() {
        assert_eq!(cascade(None, &[]), None);
    }

    /// A screen this far down is a screen nobody has. The walk is bounded, so
    /// a display full of windows lands on top of one rather than off the end
    /// of the world — and never shorter than a strip a page can be read in.
    #[test]
    fn the_walk_is_bounded() {
        let taken: Vec<(f64, f64)> = (0..40)
            .map(|n| (100.0, 100.0 + n as f64 * CASCADE))
            .collect();
        let (_, y, _, height) =
            cascade(Some((100.0, 100.0, 800.0, 600.0)), &taken).expect("a spot");
        assert!(y <= 100.0 + 17.0 * CASCADE, "walked off: {y}");
        assert!(height >= CASCADE * 8.0, "squeezed to {height}");
    }
}
