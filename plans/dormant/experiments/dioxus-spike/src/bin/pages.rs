//! Spike 2: a page on the screen, and then twenty of them in a column.
//!
//! The question the whole experiment turns on. pdfium draws a page in single
//! milliseconds — `render.rs` beside this says 3.4ms at 10.1 megapixels — and
//! the reason that was not worth having under Tauri is that the pixels then
//! had to cross into another process. Here they do not: the bitmap becomes a
//! `wgpu::Texture` in the process that is already drawing the window.
//!
//! So what is being measured is not the drawing. It is everything the page
//! costs *after* pdfium is done with it.
//!
//! **This is the second version of this spike.** The first was built on
//! `dioxus-native 0.7.10`'s `use_wgpu` and a `<canvas src=…>` custom paint
//! source, and its headline finding was that one canvas anywhere in the
//! document makes the whole document animate for ever: `load_custom_paint_src`
//! sets `has_canvas`, `is_animating()` is `has_canvas | …`, and `View::redraw`
//! asks for another frame whenever the document is animating. Blitz's main
//! branch removed that API in "New Custom Widget API (#425)" and replaced it
//! with `blitz_dom::Widget`, whose `requires_redraw()` **defaults to false**
//! and is what `is_animating()` now consults. So a page that is not moving
//! costs no frames, and there was never a patch to Blitz to be made.
//!
//! What is new to watch instead: `build_custom_widget_scenes` calls `paint()`
//! on **every** widget in the document each frame, not only the ones on
//! screen. A canvas paint source was asked to render only when its canvas was
//! painted; a widget is asked always. So the mounting window that `viewer.ts`
//! calls `mount()`/`OVERSCAN` is ours to write after all — a widget that is
//! not near the viewport must not be in the DOM.
//!
//! `--pages N` mounts N pages. `--quit N` ends after N seconds, which is what
//! makes this measurable without anybody sitting in front of it.

use std::env;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::Instant;

use anyrender::{PaintScene, RenderContext, ResourceId, Scene};
use blitz_dom::node::ComputedStyles;
use blitz_dom::Widget;
use dioxus::prelude::*;
use dioxus_native::{CustomWidgetAttr, DeviceHandle, LogicalSize, WindowAttributes};
use dioxus_spike::pdf::Document;
use dioxus_spike::shell::{Shell, WindowSpec};
use peniko::kurbo::{Affine, Rect};
use peniko::{Fill, ImageBrush, ImageSampler};

fn main() {
    let args: Vec<String> = env::args().collect();
    let flag = |name: &str, fallback: usize| -> usize {
        args.iter()
            .position(|a| a == name)
            .and_then(|at| args.get(at + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(fallback)
    };
    let pages = flag("--pages", 20);
    let quit_after = flag("--quit", 0);
    // How wide a page is drawn. Narrow pages put more of them on the screen at
    // once, which is the only way to ask what N resident page textures cost
    // without a scroll gesture to make.
    let page_width = flag("--width", 800);
    let path = args
        .iter()
        .skip(1)
        .find(|a| a.ends_with(".pdf"))
        .cloned()
        .unwrap_or_else(|| {
            format!(
                "{}/../../tests/fixtures/book.pdf",
                env!("CARGO_MANIFEST_DIR")
            )
        });

    let document = match Document::open(&path) {
        Ok(doc) => Rc::new(doc),
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    println!(
        "pages: {} pages in {path}, opened in {:.0}ms",
        document.pages(),
        document.opened_in
    );

    let event_loop = blitz_shell::create_default_event_loop();
    let (proxy, queue) = blitz_shell::BlitzShellProxy::new(event_loop.create_proxy());
    let mut shell = Shell::new(proxy, queue);
    shell.trace = false;
    let windows = shell.windows();

    let attributes = WindowAttributes::default()
        .with_title("Spike: pages")
        .with_surface_size(LogicalSize::new(900.0, 900.0));
    let vdom = VirtualDom::new_with_props(
        Reader,
        ReaderProps {
            document: document.clone(),
            pages: pages.min(document.pages()),
            page_width: page_width as f64,
        },
    );
    windows.open(WindowSpec::new(vdom, attributes));

    // A line a second, printed from a thread of its own rather than from
    // inside a frame — because the whole point of the widget API is that
    // there may be no frames at all, and "no frames" is exactly the answer
    // that has to be visible.
    let reporter = std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if !report() {
            return;
        }
    });

    if quit_after > 0 {
        let remote = windows.remote();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(quit_after as u64));
            remote.quit();
        });
    }

    event_loop.run_app(shell).unwrap();
    stats().lock().unwrap().done = true;
    let _ = reporter.join();
    let held = stats().lock().unwrap();
    println!(
        "pages: {} paints, {} page draws, {:.1}ms each in pdfium, {:.1}ms swizzling, \
         {:.1}ms uploading | {:.0}MB of texture resident at the end",
        held.paints,
        held.drawn,
        held.drew_ms / held.drawn.max(1) as f64,
        held.swizzled_ms / held.drawn.max(1) as f64,
        held.uploaded_ms / held.drawn.max(1) as f64,
        held.resident as f64 * 4.0 / 1e6,
    );
}

/// What a run of this costs, counted where it happens.
#[derive(Default)]
struct Stats {
    /// Every call to `Widget::paint`, whether it drew anything or not. With
    /// the canvas API this was the frame counter and it never stopped
    /// climbing; the number to look at now is whether it stops.
    paints: u64,
    /// Pages drawn by pdfium, which should settle at one per mounted page.
    drawn: u64,
    /// What pdfium spent, and what the swizzle and the upload spent after it.
    drew_ms: f64,
    swizzled_ms: f64,
    uploaded_ms: f64,
    /// The pixels alive on the GPU, as pages come and go.
    resident: u64,
    reported: Option<Instant>,
    last_paints: u64,
    done: bool,
}

fn stats() -> &'static Mutex<Stats> {
    static STATS: Mutex<Stats> = Mutex::new(Stats {
        paints: 0,
        drawn: 0,
        drew_ms: 0.0,
        swizzled_ms: 0.0,
        uploaded_ms: 0.0,
        resident: 0,
        reported: None,
        last_paints: 0,
        done: false,
    });
    &STATS
}

/// One line a second. Returns false once the app is over.
fn report() -> bool {
    let mut held = stats().lock().unwrap();
    if held.done {
        return false;
    }
    let now = Instant::now();
    let since = match held.reported {
        Some(then) => (now - then).as_secs_f64(),
        None => {
            held.reported = Some(now);
            held.last_paints = held.paints;
            return true;
        }
    };
    held.reported = Some(now);
    let paints = held.paints - held.last_paints;
    held.last_paints = held.paints;
    println!(
        "pages: {:.0} widget paints/s | {} pages drawn, {:.1}ms each in pdfium, \
         {:.1}ms swizzling, {:.1}ms uploading | {:.0}MB of texture resident",
        paints as f64 / since,
        held.drawn,
        held.drew_ms / held.drawn.max(1) as f64,
        held.swizzled_ms / held.drawn.max(1) as f64,
        held.uploaded_ms / held.drawn.max(1) as f64,
        held.resident as f64 * 4.0 / 1e6,
    );
    true
}

/// One page, drawn by pdfium and handed to Vello as a texture.
///
/// This is what a `CustomPaintSource` became. The lifecycle is the same three
/// moments — the renderer came up, the renderer went away, draw yourself — and
/// the fourth is the one that matters: `requires_redraw`, which says whether
/// the document has to keep painting on this widget's account. A page does
/// not, so it says no, and that is the whole of the fix that `PROGRESS.md`
/// thought would have to be made to Blitz.
struct PageWidget {
    document: Rc<Document>,
    index: usize,
    device: Option<DeviceHandle>,
    texture: Option<(wgpu::Texture, ResourceId)>,
    /// The size the texture was drawn at, in device pixels. This is `keyFor()`
    /// in `viewer.ts`, with everything the app does not have yet left out.
    drawn_at: Option<(u32, u32)>,
    pixels: u64,
}

impl PageWidget {
    fn new(document: Rc<Document>, index: usize) -> Self {
        Self {
            document,
            index,
            device: None,
            texture: None,
            drawn_at: None,
            pixels: 0,
        }
    }

    /// Draw the page and put it on the GPU. Returns the id the scene refers to
    /// it by, which is stable until the page is redrawn at another size.
    fn ensure(&mut self, ctx: &mut dyn RenderContext, width: u32, height: u32) -> Option<ResourceId> {
        if self.drawn_at == Some((width, height)) {
            return self.texture.as_ref().map(|(_, id)| *id);
        }
        let device = self.device.clone()?;

        let bitmap = match self.document.render(self.index, width, height) {
            Ok(bitmap) => bitmap,
            Err(err) => {
                eprintln!("{err}");
                return None;
            }
        };

        // pdfium's BGRA into Vello's Rgba8Unorm. The real app would do this in
        // the recolouring pass, which reads every pixel anyway.
        let began = Instant::now();
        let mut rgba = bitmap.bgra;
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let swizzled = began.elapsed().as_secs_f64() * 1000.0;

        let began = Instant::now();
        // The old texture goes back *before* the new one is registered, which
        // is safe now in a way it was not under the canvas API: a widget's
        // scene is rebuilt from scratch every frame, so nothing that was
        // submitted is still pointing at the id being taken out.
        if let Some((_, old)) = self.texture.take() {
            ctx.unregister_resource(old);
        }
        let texture = device.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("page"),
            size: wgpu::Extent3d {
                width: bitmap.width,
                height: bitmap.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        device.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bitmap.width * 4),
                rows_per_image: Some(bitmap.height),
            },
            wgpu::Extent3d {
                width: bitmap.width,
                height: bitmap.height,
                depth_or_array_layers: 1,
            },
        );
        let id = ctx
            .try_register_custom_resource(Box::new(texture.clone()))
            .ok()?;
        let uploaded = began.elapsed().as_secs_f64() * 1000.0;

        let was = self.pixels;
        self.pixels = bitmap.width as u64 * bitmap.height as u64;
        self.drawn_at = Some((width, height));
        self.texture = Some((texture, id));

        let mut held = stats().lock().unwrap();
        held.drawn += 1;
        held.drew_ms += bitmap.drew_in;
        held.swizzled_ms += swizzled;
        held.uploaded_ms += uploaded;
        held.resident = held.resident + self.pixels - was;
        Some(id)
    }
}

impl Widget for PageWidget {
    fn can_create_surfaces(&mut self, render_ctx: &mut dyn RenderContext) {
        match render_ctx
            .renderer_specific_context()
            .and_then(|ctx| ctx.downcast::<DeviceHandle>().ok())
        {
            Some(device) => self.device = Some(*device),
            None => eprintln!("pages: the renderer is not wgpu; nothing can be drawn"),
        }
    }

    /// The renderer is going away, and with it every resource registered
    /// against it. Dropping the handles here is the honest half of the
    /// double-resume fault the first version of this spike hit: whatever is
    /// kept across this moment is a texture belonging to a renderer that no
    /// longer exists.
    fn destroy_surfaces(&mut self) {
        self.device = None;
        self.texture = None;
        self.drawn_at = None;
        let mut held = stats().lock().unwrap();
        held.resident = held.resident.saturating_sub(self.pixels);
        self.pixels = 0;
    }

    /// A page is not an animation. This is the line the whole of Phase 0's
    /// "decide the redraw question first" was about.
    fn requires_redraw(&self) -> bool {
        false
    }

    fn paint(
        &mut self,
        render_ctx: &mut dyn RenderContext,
        _styles: &ComputedStyles,
        width: u32,
        height: u32,
        _scale: f64,
    ) -> Scene {
        stats().lock().unwrap().paints += 1;

        let mut scene = Scene::new();
        if self.device.is_none() {
            // `complete_resume` calls `can_create_surfaces` for every widget
            // that is in the document when the renderer comes up; a widget
            // mounted later is caught here instead.
            self.can_create_surfaces(render_ctx);
        }
        // `width` and `height` arrive in *device* pixels — blitz-paint takes
        // them off the content box, which is already scaled — and `scale` is
        // passed alongside for whatever wants to know. Multiplying by it again
        // draws every page at twice the size it is shown at.
        let Some(id) = self.ensure(render_ctx, width.max(1), height.max(1)) else {
            return scene;
        };
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            anyrender::PaintRef::Resource(ImageBrush {
                image: id,
                sampler: ImageSampler::default(),
            }),
            None,
            &Rect::from_origin_size((0.0, 0.0), (width as f64, height as f64)),
        );
        scene
    }
}

impl Drop for PageWidget {
    fn drop(&mut self) {
        let mut held = stats().lock().unwrap();
        held.resident = held.resident.saturating_sub(self.pixels);
    }
}

#[component]
fn Reader(document: Rc<Document>, pages: usize, page_width: f64) -> Element {
    rsx! {
        style { {STYLE} }
        div { class: "column",
            for index in 0..pages {
                Page { document: document.clone(), index, width: page_width }
            }
        }
    }
}

/// One page in the column: a box of the page's own proportions, with the
/// widget filling it. The width is fixed here rather than fitted, because fit
/// width is the layout's question and this spike is about the pixels.
///
/// The widget is attached to an `<object data=…>`, which is the element Blitz
/// looks at for one; the write-once attribute hands the widget itself over the
/// first time it is set, so `use_hook` is what keeps a re-render from building
/// a second one.
#[component]
fn Page(document: Rc<Document>, index: usize, width: f64) -> Element {
    let size = document.sizes[index];
    let height = (width * size.height as f64 / size.width as f64).round();
    let widget = use_hook(|| {
        CustomWidgetAttr::new(PageWidget::new(document.clone(), index))
    });

    rsx! {
        div { class: "page", style: "width: {width}px; height: {height}px;",
            object {
                "data": widget,
                style: "display: block; width: {width}px; height: {height}px;",
            }
        }
    }
}

const STYLE: &str = r#"
body { margin: 0; background: #2b2b31; }
.column {
  height: 100vh; overflow: scroll;
  display: flex; flex-direction: column; align-items: center; gap: 24px;
  padding: 24px 0;
}
.page { flex-shrink: 0; background: #fff; box-shadow: 0 1px 6px rgba(0,0,0,0.4); }
"#;
