//! The smallest custom widget there is, through `dioxus_native::launch`.
//!
//! A control for the page spike, and the demonstration of the thing `PROGRESS.md`
//! originally wanted a patch to Blitz for. It draws one flat colour into a
//! texture and prints a line every time it is painted. What to look for: the
//! lines *stop*. Under the `<canvas src=…>` custom paint source this replaces,
//! they never did — `has_canvas` made `is_animating()` true for as long as the
//! canvas was in the document, and `View::redraw` asked for another frame every
//! time it drew one, at 52-62% of a core with nothing moving.
//!
//! `Widget::requires_redraw` is the difference, and it defaults to false.
//! Return true here (`--animate`) to get the old behaviour back on purpose,
//! which is the other half of what makes this a control: the frames are
//! available to a widget that genuinely animates, and are not charged to one
//! that does not.

use std::env;
use std::time::Instant;

use anyrender::{PaintScene, RenderContext, ResourceId, Scene};
use blitz_dom::node::ComputedStyles;
use blitz_dom::Widget;
use dioxus::prelude::*;
use dioxus_native::{CustomWidgetAttr, DeviceHandle};
use peniko::kurbo::{Affine, Rect};
use peniko::{Fill, ImageBrush, ImageSampler};

fn main() {
    dioxus_native::launch(App);
}

struct Solid {
    device: Option<DeviceHandle>,
    texture: Option<(wgpu::Texture, ResourceId)>,
    size: Option<(u32, u32)>,
    animate: bool,
    painted: u64,
    began: Option<Instant>,
}

impl Widget for Solid {
    fn can_create_surfaces(&mut self, render_ctx: &mut dyn RenderContext) {
        eprintln!("widget: surfaces can be created");
        self.device = render_ctx
            .renderer_specific_context()
            .and_then(|ctx| ctx.downcast::<DeviceHandle>().ok())
            .map(|device| *device);
    }

    fn destroy_surfaces(&mut self) {
        eprintln!("widget: surfaces destroyed");
        self.device = None;
        self.texture = None;
        self.size = None;
    }

    fn requires_redraw(&self) -> bool {
        self.animate
    }

    fn paint(
        &mut self,
        ctx: &mut dyn RenderContext,
        _styles: &ComputedStyles,
        width: u32,
        height: u32,
        _scale: f64,
    ) -> Scene {
        self.painted += 1;
        let began = *self.began.get_or_insert_with(Instant::now);
        eprintln!(
            "widget: paint {} at {width}x{height}, {:.1}s in",
            self.painted,
            began.elapsed().as_secs_f64()
        );

        let mut scene = Scene::new();
        if self.device.is_none() {
            self.can_create_surfaces(ctx);
        }
        let Some(device) = self.device.clone() else {
            return scene;
        };
        let want = (width.max(1), height.max(1));
        if self.size != Some(want) {
            if let Some((_, old)) = self.texture.take() {
                ctx.unregister_resource(old);
            }
            let texture = device.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("solid"),
                size: wgpu::Extent3d {
                    width: want.0,
                    height: want.1,
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
            let pixels: Vec<u8> = (0..want.0 * want.1)
                .flat_map(|_| [220u8, 90, 60, 255])
                .collect();
            device.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(want.0 * 4),
                    rows_per_image: Some(want.1),
                },
                wgpu::Extent3d {
                    width: want.0,
                    height: want.1,
                    depth_or_array_layers: 1,
                },
            );
            match ctx.try_register_custom_resource(Box::new(texture.clone())) {
                Ok(id) => {
                    self.texture = Some((texture, id));
                    self.size = Some(want);
                }
                Err(_) => {
                    eprintln!("widget: this renderer does not take wgpu textures");
                    return scene;
                }
            }
        }

        let Some((_, id)) = self.texture.as_ref() else {
            return scene;
        };
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            anyrender::PaintRef::Resource(ImageBrush {
                image: *id,
                sampler: ImageSampler::default(),
            }),
            None,
            &Rect::from_origin_size((0.0, 0.0), (width as f64, height as f64)),
        );
        scene
    }
}

#[component]
fn App() -> Element {
    let widget = use_hook(|| {
        CustomWidgetAttr::new(Solid {
            device: None,
            texture: None,
            size: None,
            animate: env::args().any(|a| a == "--animate"),
            painted: 0,
            began: None,
        })
    });

    rsx! {
        div { style: "padding: 40px; background: #222; height: 100vh;",
            object { "data": widget, style: "display: block; width: 320px; height: 240px;" }
        }
    }
}
