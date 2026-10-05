//! Five faults that belong to somebody else, kept as the smallest thing that
//! shows each — to be sent upstream, and to say so the day any is fixed.
//!
//! Four of them run with the suite, because each catches the thing it is
//! about and therefore *passes while the bug is there*: the day one fails is
//! the day the workaround it names can go. The fifth aborts the
//! process rather than panicking, which is not something to do to a test run, so it is
//! `#[ignore]`d:
//!
//! ```text
//! cargo test --test upstream -- --ignored
//! ```

use blitz_test_harness::Harness;
use dioxus::prelude::*;

/// **Stylo panics when a stylesheet changes while anything is hovered.**
///
/// `StylesheetInvalidationSet::invalidation_kind_for` calls `each_class` on
/// every element *snapshot* it finds while walking the tree, and
/// `ServoElementSnapshot::each_class` goes through `get_attr`, which is
/// `self.attrs.as_ref().unwrap()`. Blitz takes a cheap snapshot for a state
/// change alone — `snapshot_node_state_only`, which is what a hover or a press
/// produces — and that snapshot has `attrs: None`.
///
/// So: hover a button, change a stylesheet, and the process panics from a stack
/// with nothing of the application in it — which is a plain click on the button
/// that changes the theme, where the `t` shortcut for the same action is fine.
///
/// Either side could fix it: Stylo's element-wrapper path guards with
/// `has_attrs()` and this one does not, and equally Blitz could fill the
/// attributes in. Against `stylo 0.20.0`, `blitz-dom 0.3.0-beta.2`.
///
/// The reader works around it by never rewriting its stylesheet — the theme is
/// custom properties on the root, and an attribute change is a snapshot that
/// *does* carry attributes. See `styles.rs`.
#[test]
fn a_stylesheet_that_changes_under_a_hovered_node() {
    #[component]
    fn Themed() -> Element {
        let mut dark = use_signal(|| false);
        // Two things in this sheet are load-bearing and neither is obvious.
        // The `:hover` rule is what makes Blitz take a snapshot at all — it
        // snapshots a state change only when some rule depends on the state
        // bits, which is the check at `document.rs`'s `snapshot_node_impl`.
        // And the *class* selector is what makes Stylo ask that snapshot for
        // its classes: the walk skips that branch when the changed sheet has
        // no class selectors in it. Drop either and this test passes and says
        // nothing.
        let sheet = if dark() {
            ".chip { color: #ffffff; } .chip:hover { color: #eeeeee; }"
        } else {
            ".chip { color: #000000; } .chip:hover { color: #111111; }"
        };
        rsx! {
            style { "{sheet}" }
            button {
                class: "chip",
                onclick: move |_| dark.set(!dark()),
                "theme"
            }
        }
    }

    // The click is two things at once: the pointer lands on the button, which
    // is a state-only snapshot, and the handler rewrites the stylesheet.
    //
    // Caught rather than left to fly, so that this test *passes while the bug
    // is there* and fails the day it is fixed — which is the day the
    // workaround in `styles.rs` can go.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut harness = Harness::from_component(Themed);
        harness.click(".chip");
    }));
    std::panic::set_hook(hook);
    assert!(
        outcome.is_err(),
        "no panic: this is fixed upstream, and `styles.rs` can stop working \
         around it"
    );
}

/// **A click takes the keyboard away, and the page cannot take it back.**
///
/// Two halves, and each is defensible on its own:
///
/// *Blitz clears the focus on a click that lands on nothing it knows how to
/// focus.* `handle_pointerup` walks up from the target looking for a text
/// input, a checkbox, a radio, a summary, a label or a link, and clears the
/// focus outright when it finds none. A plain `<button>` is not on that list,
/// so clicking one takes the focus off whatever had it.
///
/// *And a key with nothing focused goes to the root element*, which is
/// `<html>` — above anything a component can put a handler on, because events
/// bubble upwards. So an application whose shortcuts live on its own root
/// stops answering any of them from the first click onwards.
///
/// The way out ought to be `MountedData::set_focus`, and it is not: it takes
/// `doc_mut()` the moment it is called, and every place a component can call
/// it from is already inside a borrow of the document — a DOM event handler,
/// a mounted handler, and a task spawned from either, which is polled inside
/// that same borrow. It panics with "RefCell already borrowed".
///
/// So the fix has to come from outside the page: `shell.rs` gives the
/// keyboard back after a click, and the harness does the same for a window
/// that does not exist. What would end that is either a focusable root —
/// blitz honouring `tabindex` in that walk, which is what a browser does — or
/// a `set_focus` that queues rather than borrowing.
///
/// Against `blitz-dom 0.3.0-beta.2`.
#[test]
fn a_click_clears_the_focus_and_a_component_cannot_restore_it() {
    #[component]
    fn Buttons() -> Element {
        rsx! {
            div {
                class: "root",
                tabindex: 0,
                onkeydown: move |_| {},
                button { class: "chip", onclick: move |_| {}, "press me" }
            }
        }
    }

    let mut harness = Harness::from_component(Buttons);
    let root = harness.node(".root");
    harness.base_mut().set_focus_to(root);
    harness.pump();
    assert_eq!(harness.focused(), Some(root), "the page has the keyboard");

    harness.click(".chip");
    let focused = harness.focused();
    assert_ne!(
        focused,
        Some(root),
        "this is fixed upstream: a click no longer costs the page its          keyboard, and `shell.rs` can stop giving it back",
    );
    // And where it went is above everything the application owns.
    assert_eq!(focused, harness.query("html"));
}

/// **Hit-testing does not clip on `overflow: hidden`.**
///
/// A node scrolled far out of its container is still hit-tested where its box
/// says it is — over whatever else happens to be drawn there. *Painting* gets
/// this right: the same node is clipped and cannot be seen. So the failure is
/// a click that lands on something invisible, which is as hard to see as it
/// sounds.
///
/// In the reader this was the find bar. A page is absolutely positioned at
/// `top: box - scroll`, so with the document scrolled it starts at a large
/// negative offset and its box covers the toolbar and the find bar above it.
/// Clicking "Done" at the top of a document worked and clicking it anywhere
/// else did nothing at all, which is the worst way round.
///
/// The workaround is `position: relative` and a `z-index` on every row of the
/// window that is not the document — the same trap and the same fix as
/// `pos_z_hoisted_children` in item 3 of Phase 3, one level out. See
/// `styles.rs`.
#[test]
fn a_node_scrolled_out_of_its_container_is_still_hit_tested_there() {
    #[component]
    fn Overflowing() -> Element {
        rsx! {
            style { "
                .bar {{ position: absolute; top: 0; left: 0;
                        width: 200px; height: 40px; background: #cccccc; }}
                .clip {{ position: absolute; top: 40px; left: 0;
                         width: 200px; height: 100px; overflow: hidden; }}
                .far {{ position: absolute; top: -300px; left: 0;
                        width: 200px; height: 400px; background: #888888; }}
            " }
            div { class: "bar" }
            div { class: "clip",
                div { class: "far" }
            }
        }
    }

    let mut harness = Harness::from_component(Overflowing);
    harness.pump();
    let bar = harness.node(".bar");
    let far = harness.node(".far");
    // A point in the middle of the bar. `.far` is scrolled 300px above its
    // container and is clipped out of sight there, so nothing of it is drawn
    // over the bar.
    let hit = harness.hit(100.0, 20.0).map(|hit| hit.node_id);
    assert_ne!(
        hit,
        Some(bar),
        "this is fixed upstream: hit-testing clips, and the `z-index` on every    row of the reader's window can go",
    );
    assert_eq!(hit, Some(far), "and what it landed on is the clipped node");
}

/// **A custom widget swallows every default action, so `click` and `dblclick`
/// never happen over one.**
///
/// `handle_dom_event` in `blitz-dom` forwards an event whose target is a
/// custom widget to that widget and then *returns*, before the `match` that
/// runs the default actions. `click` is the default action of `pointerup` and
/// `dblclick` is the default action of `click`, so neither is ever generated
/// over a widget — and a component with an `onclick` on the element the widget
/// is in hears nothing at all. Handlers still run for the events the shell
/// sends directly, which is why `onmousedown` and `onmouseup` work and made
/// this so confusing to find: the pointer is plainly reaching the node.
///
/// It is a reasonable-looking line — a widget that draws its own contents
/// might well want the events raw — but the two it takes away are the two a
/// widget cannot generate for itself, because a click is *not* a pointerup: it
/// is a press and a release on the same node, and a double click is two of
/// those within half a second and two pixels. Every widget that wants either
/// has to reimplement both.
///
/// The reader works around it by counting the second press itself, with
/// Blitz's own rule so that a page and a text field in the same window agree —
/// see `Viewer::begin_sweep`.
#[test]
fn a_custom_widget_never_sees_a_click() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use anyrender::{RenderContext, Scene};
    use blitz_dom::node::ComputedStyles;
    use blitz_dom::Widget;
    use dioxus_native::CustomWidgetAttr;

    /// A widget that draws nothing. What is being tested is the event path,
    /// and a widget with no surfaces takes the same one.
    struct Nothing;

    impl Widget for Nothing {
        fn can_create_surfaces(&mut self, _ctx: &mut dyn RenderContext) {}
        fn destroy_surfaces(&mut self) {}
        fn requires_redraw(&self) -> bool {
            false
        }
        fn paint(
            &mut self,
            _ctx: &mut dyn RenderContext,
            _styles: &ComputedStyles,
            _width: u32,
            _height: u32,
            _scale: f64,
        ) -> Scene {
            Scene::new()
        }
    }

    // Counters rather than props, because what is being counted is what
    // reached a handler and nothing here has two of anything.
    static DOWN: AtomicUsize = AtomicUsize::new(0);
    static CLICKED: AtomicUsize = AtomicUsize::new(0);

    #[component]
    fn Widgeted() -> Element {
        let widget = use_hook(|| CustomWidgetAttr::new(Nothing));
        rsx! {
            div {
                class: "holder",
                style: "position: absolute; top: 0; left: 0; width: 200px; height: 200px;",
                onmousedown: move |_| { DOWN.fetch_add(1, Ordering::Relaxed); },
                onclick: move |_| { CLICKED.fetch_add(1, Ordering::Relaxed); },
                object {
                    "data": widget,
                    style: "display: block; width: 200px; height: 200px;",
                }
            }
        }
    }

    let mut harness = Harness::from_component(Widgeted);
    harness.pump();
    harness.click_at(100.0, 100.0);
    harness.pump();

    assert_eq!(
        DOWN.load(Ordering::Relaxed),
        1,
        "the press reaches the element the widget is in, which is what makes \
         the missing click so hard to see"
    );
    assert_eq!(
        CLICKED.load(Ordering::Relaxed),
        0,
        "this is fixed upstream: a widget no longer swallows the default \
         action, and `Viewer::begin_sweep` can stop counting double clicks \
         for itself"
    );
}

/// **`pdfium-render`'s `thread_safe` feature does not serialise anything.**
///
/// It is two `unsafe impl`s — `Send` and `Sync` for `Pdfium` — and a `Send +
/// Sync` bound on the bindings accessor. pdfium itself has process-wide state
/// and no locking of its own, so two threads inside it abort the process:
/// `SIGABRT`, exit 134, no panic, no message, no stack.
///
/// This is not a bug so much as a name that promises something it does not
/// deliver, and the cost of believing it is a test binary that vanishes.
/// `pdfium.rs` takes a process-wide lock in front of every call, which is what
/// the feature's name suggests is already happening.
///
/// **With the lock in place this passes**, which is what makes it a
/// regression test rather than a demonstration: take the lock out of
/// `pdfium.rs` and it aborts. It stays `#[ignore]`d for exactly that reason —
/// a failure here is not a failure, it is the whole binary going away, and
/// that is not a thing to have happen in a run somebody is reading.
#[test]
#[ignore = "aborts the process rather than failing"]
fn pdfium_is_not_thread_safe() {
    use moonowl::render;
    let path = format!("{}/tests/fixtures/book.pdf", env!("CARGO_MANIFEST_DIR"));
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let path = path.clone();
            std::thread::spawn(move || {
                let document = render::open(&path).unwrap();
                for page in 0..4 {
                    document
                        .render(
                            page,
                            400,
                            500,
                            moonowl::layout::View::WHOLE,
                            &mut |_bitmap| {},
                        )
                        .unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
}
