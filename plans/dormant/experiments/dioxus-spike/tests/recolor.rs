//! Spike 3: the shader against the reference, to the same tolerance the app
//! already holds its two recolouring paths to.
//!
//! `recolor.test.mjs` in the app holds the canvas blend chain and the pixel
//! walk to within one level out of 255 of each other. The GPU is a third path,
//! and the question is whether it lands in the same place. So: run the WGSL
//! over a page of made-up pixels, run `recolor_cpu` over the same pixels, and
//! compare.
//!
//! It runs headless — an adapter, a device, a compute pass, a readback — with
//! no window and no document, which is also the shape the Phase 2 harness
//! wants.

use anyrender_vello::wgpu;
use dioxus_spike::recolor::{recolor_cpu, Rgb, SHADER};

/// Moonowl Dark, near enough: light ink on a slate ground.
const TEXT: Rgb = [0xe8, 0xe6, 0xe3];
const BG: Rgb = [0x22, 0x24, 0x2b];

/// Pixels chosen to hit every branch: the greys, which is a page of type; the
/// saturated colours a figure is made of; the pale washes that fall above the
/// white point; and the near-neutrals either side of the colour floor.
fn fixture() -> Vec<u8> {
    let mut pixels = Vec::new();
    let mut push = |r: u8, g: u8, b: u8| pixels.extend_from_slice(&[r, g, b, 255]);
    for level in 0..=255u8 {
        push(level, level, level);
    }
    for level in (0..=255u8).step_by(5) {
        // A hair off neutral: the floor and the fade above it.
        push(level, level.saturating_add(6), level);
        push(level, level.saturating_add(20), level.saturating_add(40));
        // Plainly coloured.
        push(level, 40, 200);
        push(200, level, 40);
        push(40, 200, level);
    }
    for (r, g, b) in [
        (255, 255, 255),
        (0, 0, 0),
        (250, 248, 240),
        (236, 236, 236),
        (234, 234, 234),
        (31, 119, 180),
        (255, 127, 14),
        (44, 160, 44),
        (214, 39, 40),
    ] {
        push(r, g, b);
    }
    pixels
}

#[test]
fn shader_matches_the_reference() {
    for keep_colour in [false, true] {
        let pixels = fixture();
        let mut expected = pixels.clone();
        recolor_cpu(&mut expected, TEXT, BG, keep_colour);

        let Some(actual) = run_shader(&pixels, TEXT, BG, keep_colour) else {
            eprintln!("recolor: no GPU adapter; skipping");
            return;
        };

        let mut worst = 0i32;
        let mut worst_at = 0usize;
        for (index, (a, b)) in expected.iter().zip(actual.iter()).enumerate() {
            let difference = (*a as i32 - *b as i32).abs();
            if difference > worst {
                worst = difference;
                worst_at = index;
            }
        }
        let pixel = worst_at / 4 * 4;
        assert!(
            worst <= 1,
            "keep_colour {keep_colour}: off by {worst} levels at pixel {} \
             — in {:?}, reference {:?}, shader {:?}",
            worst_at / 4,
            &pixels[pixel..pixel + 4],
            &expected[pixel..pixel + 4],
            &actual[pixel..pixel + 4],
        );
    }
}

/// The whole of the GPU side: a texture in, a storage texture out, one
/// dispatch, one readback.
fn run_shader(pixels: &[u8], text: Rgb, bg: Rgb, keep_colour: bool) -> Option<Vec<u8>> {
    let width = (pixels.len() / 4) as u32;
    let height = 1u32;

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("recolor"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .ok()?;

    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let source = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("source"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &source,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        extent,
    );

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    // Two vec4s: ink with the keep-colour flag in `w`, then paper.
    let mut uniform = [0f32; 8];
    for channel in 0..3 {
        uniform[channel] = text[channel] as f32 / 255.0;
        uniform[4 + channel] = bg[channel] as f32 / 255.0;
    }
    uniform[3] = if keep_colour { 1.0 } else { 0.0 };
    let theme = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("theme"),
        size: std::mem::size_of_val(&uniform) as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&theme, 0, bytemuck_cast(&uniform));

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("recolor"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("recolor"),
        layout: None,
        module: &module,
        entry_point: Some("recolor"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("recolor"),
        layout: &pipeline.get_bind_group_layout(0),
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
                    &target.create_view(&Default::default()),
                ),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: theme.as_entire_binding(),
            },
        ],
    });

    // A readback buffer's rows are padded to 256 bytes, which is the one thing
    // about this that is not arithmetic.
    let stride = (width * 4).div_ceil(256) * 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (stride * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(height),
            },
        },
        extent,
    );
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).ok()?;
    let view = slice.get_mapped_range();
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height {
        let start = (row * stride) as usize;
        out.extend_from_slice(&view[start..start + (width * 4) as usize]);
    }
    drop(view);
    readback.unmap();
    Some(out)
}

fn bytemuck_cast(values: &[f32]) -> &[u8] {
    // Safe: f32 has no padding and no invalid bit patterns, and the slice is
    // read only.
    unsafe {
        std::slice::from_raw_parts(values.as_ptr() as *const u8, std::mem::size_of_val(values))
    }
}
