//! Spike 1: two windows, and whether they can be made at all.
//!
//! The question from the assessment: `dioxus_native::Config` exposes one
//! window, `DioxusNativeApplication::add_window` is public but off the
//! documented path, and the app's whole "Two documents at once" architecture
//! depends on this working. If it does not, the rewrite is not worth doing.
//!
//! What this opens: a window with a button that opens another, each numbered,
//! each cascading off the last by 32 points the way `spawn_window` does today.
//! Closing the last one ends the app, as it does on every platform now. Every
//! window prints where it asked to be and where it landed, because the fault
//! this is really looking for is the one `Placements` exists for — a window
//! that is moved onto the launch window's frame the moment it is shown.
//!
//! `--auto N` opens N windows on a timer from another thread and then quits,
//! so the answer can be had without taking the screen away from anybody. That
//! path is also the one worth having: a window asked for from a thread is
//! exactly what the Dock menu item does today.

use std::env;
use std::thread;
use std::time::Duration;

use dioxus::prelude::*;
use dioxus_native::{LogicalSize, WindowAttributes};
use dioxus_spike::shell::{Shell, WindowSpec, Windows};
use winit::dpi::LogicalPosition;

fn main() {
    let auto: Option<u32> = env::args()
        .skip_while(|a| a != "--auto")
        .nth(1)
        .and_then(|n| n.parse().ok());

    let event_loop = blitz_shell::create_default_event_loop();
    let (proxy, queue) = blitz_shell::BlitzShellProxy::new(event_loop.create_proxy());
    let mut shell = Shell::new(proxy, queue);
    let windows = shell.windows();

    // One place makes every window, and it counts them, which is what lets a
    // window be asked for from a thread that knows nothing about the others.
    let mut made = 0u32;
    shell.on_request(move || {
        made += 1;
        spec(made)
    });

    windows.request();

    if let Some(count) = auto {
        let remote = windows.remote();
        thread::spawn(move || {
            for _ in 1..count {
                thread::sleep(Duration::from_millis(700));
                remote.request();
            }
            thread::sleep(Duration::from_millis(1500));
            println!("windows: {count} asked for; quitting");
            remote.quit();
        });
    }

    event_loop.run_app(shell).unwrap();
    println!("windows: event loop ended");
}

/// Where the next window goes: 32 points on and down from the last, which is
/// what `spawn_window` does today.
fn spec(n: u32) -> WindowSpec {
    let step = 32.0 * (n as f64 - 1.0);
    // Where the first window goes. Only so that a screenshot taken while this
    // runs can be pointed at a part of the screen nothing else is covering.
    let origin: f64 = env::var("SPIKE_ORIGIN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120.0);
    let at = LogicalPosition::new(origin + step, origin + step);
    let attributes = WindowAttributes::default()
        .with_title(format!("Spike window {n}"))
        .with_surface_size(LogicalSize::new(520.0, 380.0))
        .with_position(at);

    WindowSpec::new(VirtualDom::new_with_props(Pane, PaneProps { n }), attributes).at(at)
}

/// A counter, so that "does this window's own virtualdom actually run" has a
/// visible answer, and a field, so that the keyboard is answering the window
/// that has focus rather than the one that was made first.
#[component]
fn Pane(n: u32) -> Element {
    let mut count = use_signal(|| 0);
    let mut typed = use_signal(String::new);
    let windows = use_context::<Windows>();

    rsx! {
        style { {STYLE} }
        div { class: "page",
            h1 { "Window {n}" }
            div { class: "row",
                button { onclick: move |_| count += 1, "Count: {count}" }
                button { onclick: move |_| windows.request(), "New window" }
            }
            input {
                value: "{typed}",
                placeholder: "type here — focus should follow the window",
                oninput: move |e| typed.set(e.value()),
            }
            p { class: "muted", "{typed}" }
        }
    }
}

const STYLE: &str = r#"
body { margin: 0; font: 14px system-ui, sans-serif; background: #f6f5f3; color: #26221f; }
.page { padding: 24px; display: flex; flex-direction: column; gap: 12px; }
h1 { margin: 0; font-size: 22px; font-weight: 600; }
.muted { margin: 0; color: #8a8177; font-size: 12px; }
.row { display: flex; gap: 8px; }
button {
  padding: 8px 14px; border-radius: 8px; border: 1px solid #d9d3cb;
  background: #fff; color: #26221f; font: inherit; cursor: pointer;
}
button:hover { background: #efece7; }
input {
  padding: 8px 10px; border-radius: 8px; border: 1px solid #d9d3cb;
  background: #fff; color: #26221f; font: inherit;
}
"#;
