//! What the document actually contains, with no window in front of it.
//!
//! A widget that is never asked to paint is either not attached as far as
//! blitz-dom is concerned, or is one that nothing paints. This tells the two
//! apart by building the document and reading the node back.
//!
//! It is also the first piece of the harness the assessment asks for in Phase
//! 2: a `DioxusDocument` built in process, driven, and inspected, with no GPU
//! and no window. It is what found `display: block` on a canvas in an
//! afternoon that a screenshot could only call "blank", and the same trick
//! answers the same question about an `<object>`.

use anyrender::{RenderContext, Scene};
use blitz_dom::node::ComputedStyles;
use blitz_dom::{DocumentConfig, Widget};
use blitz_traits::shell::{ColorScheme, Viewport};
use dioxus::prelude::*;
use dioxus_native::{CustomWidgetAttr, DioxusDocument};

fn main() {
    let vdom = VirtualDom::new(App);
    let mut doc = DioxusDocument::new(
        vdom,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            ..Default::default()
        },
    );
    doc.initial_build();
    doc.inner.borrow_mut().resolve(0.0);

    let inner = doc.inner.borrow();
    println!("{} nodes", inner.tree().len());
    println!(
        "{} custom widget nodes: {:?}",
        inner.custom_widget_node_ids().len(),
        inner.custom_widget_node_ids()
    );
    println!("animating: {}", inner.is_animating());
    for (id, node) in inner.tree().iter() {
        let Some(element) = node.element_data() else {
            continue;
        };
        let name = element.name.local.as_ref();
        if let Some(named) = element.attrs().iter().find(|a| a.name.local.as_ref() == "id") {
            println!("#{}: <{name}> {:?}", named.value, node.final_layout().size);
        }
        if name != "object" {
            continue;
        }
        println!(
            "node {id}: <{name}> {:?} widget {}",
            node.final_layout().size,
            element.custom_widget_data().is_some()
        );
    }
}

/// A widget that draws nothing. All that is being asked here is whether it is
/// attached, whether the box it is in has a size, and — the question the whole
/// port turned on — whether a document holding one says it is animating.
struct Nothing;

impl Widget for Nothing {
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

#[component]
fn App() -> Element {
    let widget = use_hook(|| CustomWidgetAttr::new(Nothing));

    rsx! {
        object { "data": widget, style: "display: block; width: 100px; height: 100px;" }
        // Does a line that says it must not wrap, wrap? In the chrome spike
        // the document title came out on two lines, and `text-overflow` being
        // missing does not explain that on its own.
        div {
            id: "nowrap",
            style: "width: 120px; overflow: hidden; white-space: nowrap; font-size: 13px;",
            "A rather long document title that has to be cut off somewhere.pdf"
        }
        div {
            id: "wrap",
            style: "width: 120px; font-size: 13px;",
            "A rather long document title that has to be cut off somewhere.pdf"
        }
    }
}
