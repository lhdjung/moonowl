//! Spike 4: chrome that looks right.
//!
//! The toolbar, a popover menu and the notice line, rebuilt from the app's own
//! `styles.css` with the four things Blitz cannot do taken out. If this comes
//! out looking like the app, the CSS gap list in the assessment holds; if it
//! does not, the list is longer than it says.
//!
//! What was changed, and why, is the whole content of this spike:
//!
//! *`position: fixed` is gone*, and it is an improvement. The root is a flex
//! column — chrome row, viewer, status strip — so nothing is over a scrolling
//! body and nothing needs taking out of flow. The popover is a child of the
//! root with `position: absolute` and coordinates worked out by hand, which is
//! what `showPopover` already does today.
//!
//! *`overflow: auto` becomes `overflow: scroll`*, which Blitz has.
//!
//! *`text-overflow: ellipsis` is not implemented* (Parley #304), and the
//! document title is the place the app leans on it. The fallback here is the
//! one the assessment recommends: a `mask-image` fading the last few
//! characters out, which Blitz does support. It is arguably nicer than an
//! ellipsis and it is certainly less code than measuring text.
//!
//! *An icon is an SVG with its colour baked in.* CSS does not reach inside an
//! SVG in Blitz, so `stroke: currentColor` paints nothing and every icon comes
//! out in whatever the default is. Each icon here is generated with the
//! colour written into the presentation attributes, which is what the
//! `HashMap<(Icon, Colour), String>` in the assessment amounts to.

use std::env;

use dioxus::prelude::*;
use dioxus_native::{LogicalSize, WindowAttributes};
use dioxus_spike::shell::{Shell, WindowSpec};

fn main() {
    let quit_after: u64 = env::args()
        .skip_while(|a| a != "--quit")
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let event_loop = blitz_shell::create_default_event_loop();
    let (proxy, queue) = blitz_shell::BlitzShellProxy::new(event_loop.create_proxy());
    let mut shell = Shell::new(proxy, queue);
    shell.trace = false;
    let windows = shell.windows();

    windows.open(WindowSpec::new(
        VirtualDom::new(Chrome),
        WindowAttributes::default()
            .with_title("Spike: chrome")
            .with_surface_size(LogicalSize::new(1000.0, 640.0)),
    ));

    if quit_after > 0 {
        let remote = windows.remote();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(quit_after));
            remote.quit();
        });
    }

    event_loop.run_app(shell).unwrap();
}

#[component]
fn Chrome() -> Element {
    // Which menu is open, and where its button was. `showPopover` tracks its
    // anchor for the same reason: clicking the button that opened a menu
    // closes it, and the menu has to be told where to appear.
    // `--menu theme` opens one on the way in, so that a screenshot can be
    // taken of it without anybody clicking anything.
    let opened = use_hook(|| {
        let args: Vec<String> = env::args().collect();
        args.iter()
            .position(|a| a == "--menu")
            .and_then(|at| args.get(at + 1).cloned())
    });
    let mut menu = use_signal(|| match opened.as_deref() {
        Some("theme") => Some(("theme", 640.0)),
        Some("contents") => Some(("contents", 12.0)),
        _ => None,
    });
    let mut notice = use_signal(|| Some("Toolbar hidden, ⌘T brings it back".to_string()));
    let mut theme = use_signal(|| "Moonowl Light");

    // A menu button toggles its own menu, which is what `showPopover`'s
    // anchor tracking is for in the app.
    fn toggle(
        mut menu: Signal<Option<(&'static str, f64)>>,
        name: &'static str,
        at: f64,
    ) {
        let open = menu.read().as_ref().map(|(open, _)| *open) == Some(name);
        menu.set(if open { None } else { Some((name, at)) });
    }

    rsx! {
        style { {STYLE} }
        div { class: "shell",
            div { class: "toolbar",
                div { class: "bar-group bar-left",
                    Btn { icon: Icon::Sidebar, label: "Contents", onclick: move |_| toggle(menu, "contents", 12.0) }
                    // The title is the one place the app relies on an ellipsis.
                    div { class: "doc-title",
                        "A rather long document title that has to be cut off somewhere.pdf"
                    }
                }
                div { class: "bar-group bar-center",
                    Btn { icon: Icon::Minus, label: "", onclick: move |_| {} }
                    div { class: "page-field", "12 / 400" }
                    Btn { icon: Icon::Plus, label: "", onclick: move |_| {} }
                }
                div { class: "bar-group bar-right",
                    Btn {
                        icon: Icon::Theme,
                        label: "{theme}",
                        pressed: menu.read().as_ref().map(|(name, _)| *name) == Some("theme"),
                        onclick: move |_| toggle(menu, "theme", 640.0),
                    }
                    Btn { icon: Icon::Search, label: "Find", onclick: move |_| {} }
                }
            }

            div { class: "viewer",
                div { class: "page-sheet",
                    p { class: "page-text", "A page would be here." }
                }
                div { class: "page-sheet",
                    p { class: "page-text", "And another below it." }
                }
            }

            if let Some(text) = notice.read().clone() {
                div { class: "notice",
                    span { "{text}" }
                    button { class: "notice-close", onclick: move |_| notice.set(None), "Dismiss" }
                }
            }

            // The popover: a child of the root, placed by hand, because there
            // is no `position: fixed` and an absolutely positioned box is
            // positioned against its immediate parent.
            if let Some((name, at)) = *menu.read() {
                div {
                    class: "popover",
                    style: "left: {at}px; top: 52px;",
                    if name == "theme" {
                        div { class: "popover-section", "Themes" }
                        for option in ["Moonowl Light", "Moonowl Dark", "Moonowl Ember", "Sepia", "Nord"] {
                            div {
                                class: if *theme.read() == option { "popover-item current" } else { "popover-item" },
                                onclick: move |_| { theme.set(option); menu.set(None); },
                                span { class: "swatch", style: "background: {swatch(option)};" }
                                span { class: "popover-label", "{option}" }
                            }
                        }
                    } else {
                        div { class: "popover-section", "Contents" }
                        for (depth, title) in OUTLINE {
                            div {
                                class: "popover-item",
                                style: "padding-left: {12 + depth * 14}px;",
                                span { class: "popover-label", "{title}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

const OUTLINE: [(u32, &str); 6] = [
    (0, "Front matter"),
    (1, "A note on the text"),
    (0, "Chapter one: the argument"),
    (1, "Where it begins"),
    (1, "A digression, with a title long enough to want cutting off"),
    (0, "Chapter two"),
];

fn swatch(theme: &str) -> &'static str {
    match theme {
        "Moonowl Dark" => "#22242b",
        "Moonowl Ember" => "#7a2318",
        "Sepia" => "#e9dcc3",
        "Nord" => "#2e3440",
        _ => "#e7e6e2",
    }
}

/// A button: an icon and a word, which is what the brief asks for — "not just
/// symbols, and not just tiny symbols".
#[component]
fn Btn(
    icon: Icon,
    label: String,
    #[props(default = false)] pressed: bool,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let class = if pressed { "btn on" } else { "btn" };
    rsx! {
        div { class: "{class}", onclick: move |e| onclick.call(e),
            span { class: "icon", dangerous_inner_html: "{icon.svg()}" }
            if !label.is_empty() {
                span { "{label}" }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Icon {
    Sidebar,
    Search,
    Theme,
    Plus,
    Minus,
}

impl Icon {
    /// The colour is written into the SVG rather than inherited, because CSS
    /// does not reach inside one here. In the app this would be memoised per
    /// theme — thirty-three icons and a colour that changes rarely.
    fn svg(self) -> String {
        let ink = "#6a6d73";
        let body = match self {
            Icon::Sidebar => format!(
                r#"<rect x="2.5" y="3.5" width="15" height="13" rx="2.5" fill="none" stroke="{ink}" stroke-width="1.5"/>
                   <line x1="8" y1="3.5" x2="8" y2="16.5" stroke="{ink}" stroke-width="1.5"/>"#
            ),
            Icon::Search => format!(
                r#"<circle cx="9" cy="9" r="5.5" fill="none" stroke="{ink}" stroke-width="1.5"/>
                   <line x1="13" y1="13" x2="17" y2="17" stroke="{ink}" stroke-width="1.5" stroke-linecap="round"/>"#
            ),
            Icon::Theme => format!(
                r#"<circle cx="10" cy="10" r="6.5" fill="none" stroke="{ink}" stroke-width="1.5"/>
                   <path d="M10 3.5 A6.5 6.5 0 0 1 10 16.5 Z" fill="{ink}"/>"#
            ),
            Icon::Plus => format!(
                r#"<line x1="10" y1="5" x2="10" y2="15" stroke="{ink}" stroke-width="1.5" stroke-linecap="round"/>
                   <line x1="5" y1="10" x2="15" y2="10" stroke="{ink}" stroke-width="1.5" stroke-linecap="round"/>"#
            ),
            Icon::Minus => format!(
                r#"<line x1="5" y1="10" x2="15" y2="10" stroke="{ink}" stroke-width="1.5" stroke-linecap="round"/>"#
            ),
        };
        format!(r#"<svg width="20" height="20" viewBox="0 0 20 20">{body}</svg>"#)
    }
}

/// Lifted from `styles.css`, with the four unsupported properties replaced.
const STYLE: &str = r#"
:root {
  --bg: #e7e6e2;
  --page-paper: #ffffff;
  --surface: #f7f6f3;
  --surface-hover: #ececE7;
  --line: #dedcd6;
  --bar-hover: #ebebec;
  --bar-sunk: #f3f3f3;
  --bar-line: #dedee0;
  --bar-accent: #e5ecef;
  --text: #2f3237;
  --text-soft: #6a6d73;
  --text-faint: #9a9da2;
  --accent: #3f7d94;
  --accent-soft: #dfeaee;
  --radius: 9px;
  --toolbar-height: 46px;
}

body { margin: 0; background: var(--bg); color: var(--text);
  font-family: ui-sans-serif, -apple-system, "Helvetica Neue", Arial, sans-serif;
  font-size: 13.5px; line-height: 1.45; }

/* The root is a column, so nothing has to be taken out of flow. */
.shell { display: flex; flex-direction: column; height: 100vh; }

.toolbar {
  display: flex; align-items: center; gap: 12px;
  height: var(--toolbar-height); flex: 0 0 auto; padding: 0 10px;
  background: var(--page-paper); border-bottom: 1px solid var(--line);
}
.bar-group { display: flex; align-items: center; gap: 6px; min-width: 0; }
.bar-left { flex: 1 1 auto; }
.bar-center { flex: 0 0 auto; }
.bar-right { flex: 1 0 auto; justify-content: flex-end; }

.btn {
  display: flex; align-items: center; gap: 7px; height: 30px; padding: 0 10px;
  border-radius: var(--radius); background: transparent; color: var(--text-soft);
  font-weight: 500; white-space: nowrap;
}
.btn:hover { background: var(--bar-hover); color: var(--text); }
.btn.on { background: var(--bar-accent); color: var(--accent); }
.icon { display: flex; width: 20px; height: 20px; }

/* No ellipsis in Parley yet, so the last few characters are faded out
   instead. `mask-image` is supported; `text-overflow` is not. */
.doc-title {
  margin-left: 6px; min-width: 0; max-width: 34ch; height: 30px;
  padding: 0 8px; border-radius: var(--radius); background: transparent;
  overflow: hidden; white-space: nowrap; color: var(--text-faint);
  font-size: 13px; display: flex; align-items: center;
  mask-image: linear-gradient(to right, #000 calc(100% - 28px), transparent);
}

.page-field {
  height: 30px; min-width: 74px; padding: 0 10px; border-radius: var(--radius);
  background: var(--bar-sunk); color: var(--text-soft);
  display: flex; align-items: center; justify-content: center;
}

.viewer {
  flex: 1 1 auto; overflow: scroll; scrollbar-width: thin;
  display: flex; flex-direction: column; align-items: center; gap: 24px;
  padding: 24px 0;
}
.page-sheet {
  flex-shrink: 0; width: 520px; height: 300px; background: var(--page-paper);
  box-shadow: 0 1px 3px rgba(0,0,0,0.08), 0 8px 24px rgba(0,0,0,0.06);
}
.page-text { margin: 32px; color: #45484d; }

.notice {
  flex: 0 0 auto; display: flex; align-items: center; justify-content: space-between;
  gap: 12px; padding: 8px 14px; background: var(--surface);
  border-top: 1px solid var(--line); color: var(--text-soft);
}
.notice-close { color: var(--accent); font-weight: 500; }

.popover {
  position: absolute; min-width: 220px; max-height: 60vh; overflow: scroll;
  padding: 6px; border-radius: 12px; border: 1px solid var(--line);
  background: var(--surface);
  box-shadow: 0 1px 2px rgba(0,0,0,0.06), 0 12px 32px rgba(0,0,0,0.14);
}
.popover-section {
  padding: 6px 10px 4px; color: var(--text-faint); font-size: 12px; font-weight: 600;
}
.popover-item {
  display: flex; align-items: center; gap: 10px; height: 32px;
  padding: 0 10px; border-radius: 7px; color: var(--text);
}
.popover-item:hover { background: var(--surface-hover); }
.popover-item.current { color: var(--accent); background: var(--accent-soft); }
.popover-label { overflow: hidden; white-space: nowrap;
  mask-image: linear-gradient(to right, #000 calc(100% - 20px), transparent); }
.swatch { width: 14px; height: 14px; border-radius: 4px; border: 1px solid var(--line); }
"#;
