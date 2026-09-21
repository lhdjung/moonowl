//! Where a page becomes a texture, and where the theme is applied.
//!
//! This is the thing the whole experiment is about. Under Tauri a rendered
//! page is a CPU buffer that has to reach a canvas in another process, and the
//! recolouring is a chain of canvas composite operations or, when a colour has
//! to be kept, a walk over every pixel at 34-70ms a page. Here the page goes
//! to the GPU once and the recolouring is a compute pass over it: the same
//! ramp, the same tolerance, and no readback.
//!
//! Two costs disappear on the way, and they are worth naming because they were
//! measured in Phase 0 and are not measured again:
//!
//! *The swizzle.* pdfium writes BGRA; a texture registered with Vello is read
//! as RGBA. The spike swapped two channels on the CPU, which is 1.6ms a page
//! at 3.3 megapixels and 5.1ms at 10.1. Uploading as `Bgra8Unorm` and letting
//! the shader read it is free, and it is why the pass runs even for a theme
//! that recolours nothing — the shader's passthrough branch is the swizzle.
//!
//! *Theme invalidation* was meant to be the third, with the page as drawn
//! kept on the GPU and a theme change a compute pass over it. It is not: the
//! source costs 25MB a page (see [`PageTexture`]) and is dropped once read,
//! so a theme change re-renders the page, and the page's key carries the
//! colours it wears — see `page.rs`.

use std::rc::Rc;

use anyrender::{RenderContext, ResourceId};
use dioxus_native::DeviceHandle;

use crate::palette::Palette;
use crate::recolor::{Region, Rgb, REGIONS, SHADER};
use crate::render::Bitmap;

/// One rectangle of a page painted through a ramp of its own: a link, or a line
/// the reader has swept over.
///
/// Three places in one struct, and they are not the same place. `span` is where
/// the run sits in the *dispatch grid*, which is the runs stacked one above
/// another so that a pass costs their area rather than the page's. `on` is
/// where it goes on the page. `from` is where the untouched pixels are: the
/// same point as `on` for a link, whose source is still on the GPU while the
/// page is being uploaded, and its place in the backup for a selection, which
/// is ramped from the page as shown and therefore has to keep what was under
/// it. See `regions.wgsl` and [`Recolorer::select`].
#[derive(Clone, Copy, Debug, PartialEq)]
struct Run {
    /// Left, top, width, height in the dispatch grid.
    span: [u32; 4],
    /// Left and top on the page.
    on: [u32; 2],
    /// Left and top in whatever is being read.
    from: [u32; 2],
    /// What the darkest pixel in it becomes, and what the lightest becomes.
    ink: Rgb,
    paper: Rgb,
}

/// A page on the GPU: one texture, wearing the theme.
///
/// The obvious design keeps two — what pdfium drew and what the theme made of
/// it — so a theme change is a compute pass rather than a re-render. Measured,
/// the source copy costs 25MB a page: 100MB for the three pages a window holds
/// against 50MB without it, to save 7ms on something changed a few times a day.
/// So the source is uploaded, read once by the pass, and dropped in `upload`.
///
/// Which is the answer `keyFor()` gives in the app for the opposite reason:
/// there the theme is in the key because a canvas has nowhere to keep the
/// original, here because the original is the more expensive half.
pub struct PageTexture {
    painted: wgpu::Texture,
    id: ResourceId,
    /// The theme the painted copy was made with, or `None` while it is stale.
    themed: Option<Palette>,
    pub width: u32,
    pub height: u32,
    /// The selection currently painted into `painted`, colours and all. Empty
    /// when there is none.
    selected: Vec<Run>,
    /// What was under it, so that it can be taken up again. As big as the runs
    /// and no bigger: a line of type is a hundredth of a page, which is the
    /// whole reason this is not a second copy of the page.
    kept: Option<wgpu::Texture>,
}

impl PageTexture {
    pub fn id(&self) -> ResourceId {
        self.id
    }

    /// Bytes resident on the GPU for this page.
    ///
    /// **The selection's backup is deliberately not in this**, and the reason
    /// is the accounting rather than the memory. `stats::RESIDENT` is added to
    /// when a page is uploaded and taken from when it goes, and a number that
    /// had grown in between would take away more than it put in — on an
    /// unsigned counter, which is not a small error but a wrapped one. The
    /// backup is a few lines of type and is gone the moment the reader lets go;
    /// the page is 25MB and is what this is for.
    pub fn bytes(&self) -> u64 {
        self.width as u64 * self.height as u64 * 4
    }

    pub fn is(&self, width: u32, height: u32) -> bool {
        self.width == width && self.height == height
    }

    pub fn wears(&self, theme: &Palette) -> bool {
        self.themed.as_ref() == Some(theme)
    }
}

/// The compute pipeline, built once for a device and shared by every page.
pub struct Recolorer {
    device: DeviceHandle,
    pipeline: wgpu::ComputePipeline,
    /// The second pass: the parts of a page painted through a ramp of their
    /// own. See `regions.wgsl`.
    regions: wgpu::ComputePipeline,
}

thread_local! {
    /// One pipeline per window, because there is a device per window rather
    /// than per process: each window's renderer builds a `WGPUContext` of its
    /// own, and so an instance, an adapter and a device of its own. This was
    /// one slot, on the theory that there was one device, and a second window
    /// then drew its first page through the first window's pipeline — a
    /// texture made on one device handed to another's scene, which is not a
    /// blank page but a dead process. Open a window with ⌘N and a document in
    /// it and that was the crash.
    ///
    /// **The key is the instance and not the device**, which is the half that
    /// took the longest to see: `wgpu::Device` compares by its id, and an id
    /// is an index into the registry of the instance that made it, so two
    /// windows' two devices are both `Id(0,1)` and compare *equal*. A cache
    /// keyed on the device is not keyed on anything. An `Instance` compares by
    /// the address of the `Global` behind it and is genuinely one per window.
    ///
    /// A list rather than a map because it holds one entry per window.
    static SHARED: std::cell::RefCell<Vec<(wgpu::Instance, Rc<Recolorer>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

impl Recolorer {
    pub fn shared(device: &DeviceHandle) -> Rc<Recolorer> {
        SHARED.with(|held| {
            let mut held = held.borrow_mut();
            if let Some((_, existing)) = held.iter().find(|(by, _)| *by == device.instance) {
                return Rc::clone(existing);
            }
            let made = Rc::new(Recolorer::new(device.clone()));
            held.push((device.instance.clone(), Rc::clone(&made)));
            made
        })
    }

    /// A renderer going away takes every pipeline built against its device
    /// with it. `destroy_surfaces` is where a widget hears about that, and
    /// this is the shared half of what it has to forget — that window's entry,
    /// not every window's.
    pub fn forget(device: &DeviceHandle) {
        SHARED.with(|held| held.borrow_mut().retain(|(by, _)| *by != device.instance));
    }

    fn new(device: DeviceHandle) -> Self {
        let module = device
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("recolor"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
        let pipeline = device
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("recolor"),
                layout: None,
                module: &module,
                entry_point: Some("recolor"),
                compilation_options: Default::default(),
                cache: None,
            });
        let over = device
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("regions"),
                source: wgpu::ShaderSource::Wgsl(REGIONS.into()),
            });
        let regions = device
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("regions"),
                layout: None,
                module: &over,
                entry_point: Some("ramp_runs"),
                compilation_options: Default::default(),
                cache: None,
            });
        Recolorer {
            device,
            pipeline,
            regions,
        }
    }

    /// Put a freshly drawn page on the GPU and paint the theme onto it.
    pub fn upload(
        &self,
        ctx: &mut dyn RenderContext,
        bitmap: &Bitmap,
        theme: &Palette,
        regions: &[Region],
    ) -> Option<PageTexture> {
        let extent = wgpu::Extent3d {
            width: bitmap.width,
            height: bitmap.height,
            depth_or_array_layers: 1,
        };
        let painted = self.device.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("page: themed"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            // `COPY_DST` as well as `COPY_SRC`, which the selection needs both
            // halves of: what is under a run is copied out before the ramp goes
            // on, and copied back when the reader lets go.
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let id = ctx
            .try_register_custom_resource(Box::new(painted.clone()))
            .ok()?;
        let mut page = PageTexture {
            painted,
            id,
            themed: None,
            width: bitmap.width,
            height: bitmap.height,
            selected: Vec::new(),
            kept: None,
        };
        self.repaint(&mut page, bitmap, theme, regions);
        Some(page)
    }

    /// Draw a page again into the texture it already has — a new draft of
    /// the document, at the same size — so that what is on screen changes
    /// without a texture being registered or released. The bitmap must be
    /// the texture's size; one that is not is ignored.
    pub fn repaint(
        &self,
        page: &mut PageTexture,
        bitmap: &Bitmap,
        theme: &Palette,
        regions: &[Region],
    ) {
        if !page.is(bitmap.width, bitmap.height) {
            return;
        }
        let extent = wgpu::Extent3d {
            width: bitmap.width,
            height: bitmap.height,
            depth_or_array_layers: 1,
        };
        // pdfium's own byte order, uploaded as it stands. The shader reads it
        // in RGBA order because the format says so, so there is no swizzle.
        let source = self.device.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("page: as drawn"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.device.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &source,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bitmap.bgra,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bitmap.width * 4),
                rows_per_image: Some(bitmap.height),
            },
            extent,
        );
        self.paint(page, &source, theme);
        // **The links, before the source goes.** They are tinted wherever the
        // page is recoloured — a link that reads exactly like the sentence
        // around it is a link nobody can see; a theme that leaves the document
        // alone hands over none, see `PageWidget::links` —
        // and they are the one thing here that can be ramped from the page as
        // pdfium drew it, because for these few frames it is still on the GPU.
        // `tintLinks` in `viewer.ts` takes a whole copy of the canvas to do the
        // same; this reads the copy that is already there.
        let over = runs_over(regions, bitmap.width, bitmap.height);
        if !over.is_empty() {
            let mut encoder = self
                .device
                .device
                .create_command_encoder(&Default::default());
            self.ramp_runs(&mut encoder, &source, &page.painted, &over);
            self.device.queue.submit([encoder.finish()]);
        }
        // The source has been read; the GPU keeps only what is shown.
        //
        // Keeping it and reusing it for the next page was tried, on the theory
        // that a 24MB texture per page drawn is what piles up during a scroll.
        // It is not: the pile is the *themed* textures, which wgpu cannot free
        // until the submission that read them has retired, and holding a
        // source costs a permanent 24MB for no measurable saving. Measured:
        // 144MB idle without it, 169MB with, and the same 177-228MB of
        // graphics memory mid-scroll either way.
        drop(source);
    }

    /// Run the ramp from the page as drawn onto the page as shown.
    fn paint(&self, page: &mut PageTexture, source: &wgpu::Texture, theme: &Palette) {
        // Two vec4s and nothing else, so that there is one possible layout
        // rather than a std140 rule to be right about: `w` on the ink carries
        // whether a pixel keeps a colour of its own, `w` on the paper carries
        // whether the pixel is left alone entirely.
        let mut uniform = [0f32; 8];
        for channel in 0..3 {
            uniform[channel] = theme.text[channel] as f32 / 255.0;
            uniform[4 + channel] = theme.background[channel] as f32 / 255.0;
        }
        uniform[3] = if theme.keep_colour { 1.0 } else { 0.0 };
        uniform[7] = if theme.recolor { 0.0 } else { 1.0 };

        let buffer = self.device.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("theme"),
            size: std::mem::size_of_val(&uniform) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.device
            .queue
            .write_buffer(&buffer, 0, as_bytes(&uniform));

        let bind_group = self
            .device
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("recolor"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(
                            &source.create_view(&Default::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(
                            &page.painted.create_view(&Default::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: buffer.as_entire_binding(),
                    },
                ],
            });

        let mut encoder = self
            .device
            .device
            .create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(page.width.div_ceil(8), page.height.div_ceil(8), 1);
        }
        self.device.queue.submit([encoder.finish()]);
        page.themed = Some(*theme);
        // A page redrawn is a page with nothing selected on it: whatever was
        // kept belonged to the pixels that have just been replaced.
        page.selected.clear();
        page.kept = None;
    }

    /// The theme's selection ramp over what the reader has swept, and off again
    /// when they sweep somewhere else.
    ///
    /// The colours arrive the right way round for the page and the runs in
    /// fractions of it. Nothing happens when neither has moved, which is most
    /// frames of a drag.
    ///
    /// **Where a link is tinted from the page as drawn, a selection is ramped
    /// from the page as *shown*** — which is `paintSelection` copying off the
    /// page canvas rather than the pristine copy, because a recoloured dark page
    /// is already light ink on dark paper and which way the ramp goes is a
    /// question about the pixels. Here the pass has nowhere untouched to read
    /// from, the source having been dropped at upload, so what is under the runs
    /// is copied out first — and copying it back is how a selection is taken
    /// up.
    pub fn select(&self, page: &mut PageTexture, runs: &[[f32; 4]], ink: Rgb, paper: Rgb) {
        let mut wanted = pack(runs, page.width, page.height);
        // A device has a ceiling on a texture's side, and the backup is one.
        // Shelving keeps the grid under it for any page of type; a run that
        // still lands past it goes unpainted rather than taking the window.
        let limit = self.device.device.limits().max_texture_dimension_2d;
        wanted.retain(|run| run.span[1] + run.span[3] <= limit);
        for run in &mut wanted {
            run.ink = ink;
            run.paper = paper;
        }
        if wanted == page.selected {
            return;
        }

        let mut encoder = self
            .device
            .device
            .create_command_encoder(&Default::default());

        // Up first. The backup holds the page as it was before the last
        // selection went on, so putting it back is the only way to a clean page
        // — and it has to happen before the new runs are copied out, or the new
        // backup would keep the old selection's colours.
        if let Some(kept) = page.kept.as_ref() {
            for run in &page.selected {
                copy(
                    &mut encoder,
                    kept,
                    run.from,
                    &page.painted,
                    run.on,
                    [run.span[2], run.span[3]],
                );
            }
        }
        page.selected.clear();
        page.kept = None;
        if wanted.is_empty() {
            self.device.queue.submit([encoder.finish()]);
            return;
        }

        // The runs on shelves no wider than the page, which for a paragraph
        // of type is about a hundredth of it.
        let (across, down) = grid(&wanted);
        let kept = self.device.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("page: under the selection"),
            size: wgpu::Extent3d {
                width: across,
                height: down,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for run in &wanted {
            copy(
                &mut encoder,
                &page.painted,
                run.on,
                &kept,
                run.from,
                [run.span[2], run.span[3]],
            );
        }
        self.ramp_runs(&mut encoder, &kept, &page.painted, &wanted);
        self.device.queue.submit([encoder.finish()]);

        page.selected = wanted;
        page.kept = Some(kept);
    }

    /// One dispatch over the runs, reading `untouched` and writing `painted`.
    ///
    /// **Over the runs and not over the page**, which is the whole reason this
    /// is a pass of its own rather than a branch in the recolouring one. A
    /// bibliography page carries a couple of hundred links; asking every one of
    /// ten million pixels whether it is inside any of two hundred rectangles is
    /// two billion tests for work that touches a fiftieth of the page. The grid
    /// is the runs stacked instead, so it costs their area and nothing else —
    /// which is what the app gets from a clip, arrived at the only way a
    /// compute pass can.
    fn ramp_runs(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        untouched: &wgpu::Texture,
        painted: &wgpu::Texture,
        runs: &[Run],
    ) {
        if runs.is_empty() {
            return;
        }
        let count = [runs.len() as f32, 0.0, 0.0, 0.0];
        let how_many = self.device.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("how many runs"),
            size: std::mem::size_of_val(&count) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.device
            .queue
            .write_buffer(&how_many, 0, as_bytes(&count));

        let table = table_of(runs);
        let places = self.device.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("runs"),
            size: (table.len() * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.device.queue.write_buffer(&places, 0, as_bytes(&table));

        let bind_group = self
            .device
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("regions"),
                layout: &self.regions.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(
                            &untouched.create_view(&Default::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(
                            &painted.create_view(&Default::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: how_many.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: places.as_entire_binding(),
                    },
                ],
            });

        let (across, down) = grid(runs);
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.regions);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(across.div_ceil(8), down.div_ceil(8), 1);
    }
}

/// The runs as the shader reads them: four vec4s each — where it lands in the
/// grid, where it goes and where it comes from, and the two colours it is
/// ramped between.
fn table_of(runs: &[Run]) -> Vec<f32> {
    let mut table: Vec<f32> = Vec::with_capacity(runs.len() * 16);
    for run in runs {
        table.extend_from_slice(&[
            run.span[0] as f32,
            run.span[1] as f32,
            run.span[2] as f32,
            run.span[3] as f32,
            run.on[0] as f32,
            run.on[1] as f32,
            run.from[0] as f32,
            run.from[1] as f32,
        ]);
        for channel in run.ink {
            table.push(channel as f32 / 255.0);
        }
        table.push(0.0);
        for channel in run.paper {
            table.push(channel as f32 / 255.0);
        }
        table.push(0.0);
    }
    table
}

/// The whole of what the regions pass is handed for a page's links: the run
/// table, how many runs it holds, and how big the dispatch grid is.
///
/// Public so that `tests/recolor.rs` can run the pass against `duotone_cpu`
/// through the same packing the reader uses — the rounding, the clipping and
/// the splitting of overlaps included, which is where the answers can differ
/// without either the ramp or the shader being wrong.
pub fn tint_runs(regions: &[Region], width: u32, height: u32) -> (Vec<f32>, u32, (u32, u32)) {
    let runs = runs_over(regions, width, height);
    (table_of(&runs), runs.len() as u32, grid(&runs))
}

/// One rectangle of one texture into another, at whole pixels.
fn copy(
    encoder: &mut wgpu::CommandEncoder,
    from: &wgpu::Texture,
    from_at: [u32; 2],
    to: &wgpu::Texture,
    to_at: [u32; 2],
    size: [u32; 2],
) {
    if size[0] == 0 || size[1] == 0 {
        return;
    }
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: from,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: from_at[0],
                y: from_at[1],
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: to,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: to_at[0],
                y: to_at[1],
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
}

/// How big the dispatch grid is: the far edge of the runs [`stack`] laid out.
fn grid(runs: &[Run]) -> (u32, u32) {
    let across = runs
        .iter()
        .map(|run| run.span[0] + run.span[2])
        .max()
        .unwrap_or(1)
        .max(1);
    let down = runs
        .iter()
        .map(|run| run.span[1] + run.span[3])
        .max()
        .unwrap_or(1)
        .max(1);
    (across, down)
}

/// Cut every run back to the part of it no later run covers.
///
/// **Two invocations of a compute shader that write the same texel have no
/// order between them**, and the dispatch grid is the runs stacked — so a pixel
/// under two runs is reached from two grid cells and which lands last is up to
/// the hardware. The rule is that the later run wins, so making that true on the
/// GPU means the runs cannot overlap: the earlier is clipped against the later.
///
/// Nearly always free — links are laid out by a typesetter and do not overlap,
/// and the two passes never meet. It is here so that the day they do, the answer
/// is a decision rather than a race.
fn disjoint(runs: Vec<Run>) -> Vec<Run> {
    let mut kept: Vec<Run> = Vec::with_capacity(runs.len());
    for run in runs.into_iter().rev() {
        let mut pieces = vec![run];
        for over in &kept {
            pieces = pieces
                .into_iter()
                .flat_map(|piece| minus(piece, over))
                .collect();
        }
        kept.extend(pieces);
    }
    kept.reverse();
    kept
}

/// One rectangle less another, as up to four rectangles: the strip above, the
/// strip below, and what is left either side between them.
fn minus(run: Run, over: &Run) -> Vec<Run> {
    let (left, top) = (run.on[0], run.on[1]);
    let (right, bottom) = (left + run.span[2], top + run.span[3]);
    let (over_left, over_top) = (over.on[0], over.on[1]);
    let (over_right, over_bottom) = (over_left + over.span[2], over_top + over.span[3]);
    if over_right <= left || over_left >= right || over_bottom <= top || over_top >= bottom {
        return vec![run];
    }
    let piece = |x: u32, y: u32, width: u32, height: u32| Run {
        span: [0, 0, width, height],
        // Where it reads from moves with it, because `from` is the *page* for a
        // link and is set again from the grid for a selection.
        on: [x, y],
        from: [run.from[0] + (x - left), run.from[1] + (y - top)],
        ink: run.ink,
        paper: run.paper,
    };
    let mut out = Vec::new();
    let middle_top = over_top.max(top);
    let middle_bottom = over_bottom.min(bottom);
    if middle_top > top {
        out.push(piece(left, top, right - left, middle_top - top));
    }
    if over_left > left {
        out.push(piece(
            left,
            middle_top,
            over_left - left,
            middle_bottom - middle_top,
        ));
    }
    if over_right < right {
        out.push(piece(
            over_right,
            middle_top,
            right - over_right,
            middle_bottom - middle_top,
        ));
    }
    if middle_bottom < bottom {
        out.push(piece(
            left,
            middle_bottom,
            right - left,
            bottom - middle_bottom,
        ));
    }
    out.retain(|run| run.span[2] > 0 && run.span[3] > 0);
    out
}

/// Give each run its place in the grid: left to right along a shelf no wider
/// than the page, and a new shelf under it when the next one does not fit.
///
/// They were stacked one above another, and the backup texture a selection
/// takes is the grid's size — so a whole page of type, whose lines add up to
/// more than the 8192 texels a device promises, was a texture wgpu refused
/// and a panic that took the window with it. Shelves keep the grid about the
/// page's own shape: the area is the same, the height is a fraction of it.
fn stack(runs: &mut [Run], width: u32) {
    let (mut x, mut y, mut shelf) = (0u32, 0u32, 0u32);
    for run in runs.iter_mut() {
        if x > 0 && x + run.span[2] > width {
            x = 0;
            y += shelf;
            shelf = 0;
        }
        run.span[0] = x;
        run.span[1] = y;
        x += run.span[2];
        shelf = shelf.max(run.span[3]);
    }
}

/// The links on a page, as runs that read from the page itself.
///
/// The source is still on the GPU while the page is being uploaded, so a link
/// reads the pixels pdfium drew straight from it: `from` and `on` are the same
/// point, and there is no copy at all. That is `tintLinks`'s pristine copy,
/// without the copy.
fn runs_over(regions: &[Region], width: u32, height: u32) -> Vec<Run> {
    let runs: Vec<Run> = regions
        .iter()
        .filter_map(|region| {
            let left = (region.area[0].floor().max(0.0) as u32).min(width);
            let top = (region.area[1].floor().max(0.0) as u32).min(height);
            let right = (region.area[2].ceil().max(0.0) as u32).min(width);
            let bottom = (region.area[3].ceil().max(0.0) as u32).min(height);
            (right > left && bottom > top).then_some(Run {
                span: [0, 0, right - left, bottom - top],
                on: [left, top],
                from: [left, top],
                ink: region.ink,
                paper: region.paper,
            })
        })
        .collect();
    let mut runs = disjoint(runs);
    stack(&mut runs, width);
    runs
}

/// Selection rectangles in fractions of the page, as whole pixels of it with a
/// place in the backup.
///
/// Rounded outwards, which is `roundedRuns` in `viewer.ts` and its reason: two
/// rectangles that meet — the end of one run of type and the start of the next
/// — meet at a fraction, and a copy that stopped short of it left a hairline of
/// unselected page between them.
fn pack(runs: &[[f32; 4]], width: u32, height: u32) -> Vec<Run> {
    let packed: Vec<Run> = runs
        .iter()
        .filter_map(|run| {
            let left = (run[0] * width as f32).floor().clamp(0.0, width as f32) as u32;
            let top = (run[1] * height as f32).floor().clamp(0.0, height as f32) as u32;
            let right = (run[2] * width as f32).ceil().clamp(0.0, width as f32) as u32;
            let bottom = (run[3] * height as f32).ceil().clamp(0.0, height as f32) as u32;
            (right > left && bottom > top).then_some(Run {
                span: [0, 0, right - left, bottom - top],
                on: [left, top],
                from: [0, 0],
                ink: [0, 0, 0],
                paper: [0, 0, 0],
            })
        })
        .collect();
    let mut packed = disjoint(packed);
    stack(&mut packed, width);
    // A selection reads from the backup, so where it came from is where it was
    // put — which is its own place in the grid.
    for run in &mut packed {
        run.from = [run.span[0], run.span[1]];
    }
    packed
}

/// Safe: `f32` has no padding and no invalid bit patterns, and the slice is
/// read only.
fn as_bytes(values: &[f32]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(values.as_ptr() as *const u8, std::mem::size_of_val(values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page of lines stacked one above another overran a device's texture
    /// side; on shelves the grid stays about the page's own shape.
    #[test]
    fn runs_are_shelved_within_the_page_width() {
        let mut runs: Vec<Run> = (0..100)
            .map(|at| Run {
                span: [0, 0, 400, 100],
                on: [0, at * 100],
                from: [0, 0],
                ink: [0; 3],
                paper: [0; 3],
            })
            .collect();
        stack(&mut runs, 1000);
        let (across, down) = grid(&runs);
        assert_eq!(across, 800);
        assert_eq!(down, 5000);
        assert_eq!(runs[1].span[..2], [400, 0]);
        assert_eq!(runs[2].span[..2], [0, 100]);
    }
}
