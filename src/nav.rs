//! The navigation provider, restated.
//!
//! `dioxus-native` has one (`link_handler.rs`) and keeps it private, so a
//! shell that does not go through `launch` has to bring its own. It is six
//! lines: a link with an http, https or mailto address goes to the system
//! browser, and nothing else navigates — which is exactly what
//! `onExternalLink` in the current app decides.

use std::sync::Arc;

use blitz_traits::navigation::{NavigationOptions, NavigationProvider};
use blitz_traits::net::Method;

/// Whether this app opens an address at all: the web and mail, and nothing
/// else — a `file:` or a `javascript:` in somebody's document is not a thing
/// this app opens because the document asked. The one place that says so.
pub(crate) fn opens(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

struct External;

impl NavigationProvider for External {
    fn navigate_to(&self, options: NavigationOptions) {
        if options.method == Method::GET && opens(options.url.as_str()) {
            if let Err(err) = webbrowser::open(options.url.as_str()) {
                eprintln!("nav: could not open {}: {err}", options.url);
            }
        }
    }
}

pub fn provider() -> Arc<dyn NavigationProvider> {
    Arc::new(External)
}
