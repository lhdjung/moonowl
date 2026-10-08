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

struct External;

impl NavigationProvider for External {
    fn navigate_to(&self, options: NavigationOptions) {
        if options.method == Method::GET
            && matches!(options.url.scheme(), "http" | "https" | "mailto")
        {
            if let Err(err) = webbrowser::open(options.url.as_str()) {
                eprintln!("nav: could not open {}: {err}", options.url);
            }
        }
    }
}

pub fn provider() -> Arc<dyn NavigationProvider> {
    Arc::new(External)
}
