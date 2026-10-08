//! What the floor is made of.
//!
//! Phase 1 ended on one number and one question — see "The floor" in
//! `PROGRESS.md`. The number is that an empty Blitz window costs about 110MB
//! resident on this machine, as much as the whole of Tauri's app process, and
//! the question is what that is made of. This
//! binary answers it by building the stack one layer at a time and measuring
//! after each, so that the cost lands on the layer that incurs it rather than
//! on the app that happens to be on top.
//!
//! ```
//! cargo run --release --bin floor -- --stage window
//! cargo run --release --bin floor -- --all          # every stage, in order
//! ```
//!
//! Two things about the measurement, because the first pass got one of them
//! wrong. **`ps -o rss` is not what a Mac means by memory.** GPU allocations
//! are owned by the process and counted in its *physical footprint*, which is
//! what Activity Monitor shows and what the kernel charges against a memory
//! limit; they are not all in `resident_size`. Every number here is both, and
//! where they disagree the footprint is the one that matters. And the peak is
//! reported beside the settled figure, because a stack that allocates 330MB to
//! settle at 227MB has told you something about its allocator.

use std::env;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyrender::WindowRenderer;
use anyrender_vello::VelloWindowRenderer;
use anyrender_vello_hybrid::VelloHybridWindowRenderer;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

/// How far up the stack to build.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    /// The process and nothing else: what Rust, malloc and the dynamic linker
    /// cost before anything is asked of the machine.
    Bare,
    /// …plus a winit event loop and one window, with nothing drawing into it.
    Window,
    /// …plus a wgpu instance, adapter and device. No surface, no swapchain.
    Wgpu,
    /// …plus a surface configured to the window. This is the swapchain, and on
    /// macOS it is where CoreAnimation's IOSurfaces appear.
    Surface,
    /// …plus Vello: the renderer Blitz composites through, resumed on the
    /// window and asked for one empty frame. Everything above this is Blitz's
    /// own DOM, style and text, which `chrome` and `widget` already measure.
    Vello,
    /// The same, through `vello_hybrid`, which is upstream's default and a
    /// different architecture rather than a cheaper setting of the same one.
    Hybrid,
}

impl Stage {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "bare" => Stage::Bare,
            "window" => Stage::Window,
            "wgpu" => Stage::Wgpu,
            "surface" => Stage::Surface,
            "vello" => Stage::Vello,
            "hybrid" => Stage::Hybrid,
            _ => return None,
        })
    }

    fn all() -> [Stage; 6] {
        [
            Stage::Bare,
            Stage::Window,
            Stage::Wgpu,
            Stage::Surface,
            Stage::Vello,
            Stage::Hybrid,
        ]
    }

    fn name(self) -> &'static str {
        match self {
            Stage::Bare => "bare",
            Stage::Window => "window",
            Stage::Wgpu => "wgpu",
            Stage::Surface => "surface",
            Stage::Vello => "vello",
            Stage::Hybrid => "hybrid",
        }
    }

    fn needs_window(self) -> bool {
        !matches!(self, Stage::Bare)
    }
}

/// Resident size and physical footprint, in megabytes, read from the platform.
///
/// `vmmap --summary` is slow (a few hundred milliseconds) and is the only
/// thing that reports the footprint without linking against mach, which is the
/// trade this makes: it is called at most a handful of times per run.
fn memory() -> (f64, f64, f64) {
    let pid = std::process::id().to_string();
    let rss = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|text| text.trim().parse::<f64>().ok())
        .map(|kb| kb / 1024.0)
        .unwrap_or(0.0);

    let (mut footprint, mut peak) = (0.0, 0.0);
    if let Ok(out) = Command::new("vmmap").args(["--summary", &pid]).output() {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let Some((label, value)) = line.split_once(':') else {
                continue;
            };
            let parsed = parse_size(value.trim());
            match label.trim() {
                "Physical footprint" => footprint = parsed,
                "Physical footprint (peak)" => peak = parsed,
                _ => {}
            }
        }
    }
    (rss, footprint, peak)
}

/// "227.3M", "1.8G", "996K" — vmmap's own spelling, in megabytes.
fn parse_size(text: &str) -> f64 {
    let (number, scale) = match text.chars().last() {
        Some('K') => (&text[..text.len() - 1], 1.0 / 1024.0),
        Some('M') => (&text[..text.len() - 1], 1.0),
        Some('G') => (&text[..text.len() - 1], 1024.0),
        _ => (text, 1.0 / (1024.0 * 1024.0)),
    };
    number.trim().parse::<f64>().unwrap_or(0.0) * scale
}

fn report(what: &str) {
    let (rss, footprint, peak) = memory();
    println!("{what:<28} rss {rss:>7.1}MB   footprint {footprint:>7.1}MB   peak {peak:>7.1}MB");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let flag = |name: &str| args.iter().position(|a| a == name).map(|i| i + 1);
    let hold = flag("--hold")
        .and_then(|i| args.get(i))
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(2.0);

    if args.iter().any(|a| a == "--all") {
        // One process per stage, because a stage cannot be unbuilt: a wgpu
        // device that has existed has already made its allocator's arenas, and
        // measuring the layer below it afterwards would measure the layer
        // above it.
        let exe = env::current_exe().expect("current exe");
        for stage in Stage::all() {
            let status = Command::new(&exe)
                .args(["--stage", stage.name(), "--hold", &hold.to_string()])
                .status();
            if let Err(error) = status {
                eprintln!("floor: could not run {}: {error}", stage.name());
            }
        }
        return;
    }

    let stage = flag("--stage")
        .and_then(|i| args.get(i))
        .and_then(|name| Stage::parse(name))
        .unwrap_or(Stage::Bare);

    report(&format!("{}: at start", stage.name()));
    if !stage.needs_window() {
        std::thread::sleep(Duration::from_secs_f64(hold));
        report(&format!("{}: settled", stage.name()));
        return;
    }

    let event_loop = EventLoop::builder().build().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let app = Box::leak(Box::new(Floor {
        stage,
        hold,
        window: None,
        gpu: None,
        renderer: None,
        hybrid: None,
        built: None,
    }));
    event_loop.run_app(app).expect("run");
}

/// The wgpu half, kept whole so that a stage which does not want it never
/// creates it.
struct Gpu {
    _instance: wgpu::Instance,
    _adapter: wgpu::Adapter,
    device: wgpu::Device,
    surface: Option<wgpu::Surface<'static>>,
}

struct Floor {
    stage: Stage,
    hold: f64,
    window: Option<Arc<dyn Window>>,
    gpu: Option<Gpu>,
    renderer: Option<VelloWindowRenderer>,
    hybrid: Option<VelloHybridWindowRenderer>,
    /// When the stack finished being built, so that "settled" is a fixed
    /// interval after it rather than after the process started.
    built: Option<Instant>,
}

impl ApplicationHandler for Floor {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = WindowAttributes::default()
            .with_title("floor")
            .with_surface_size(LogicalSize::new(1100.0, 900.0));
        let window: Arc<dyn Window> =
            Arc::from(event_loop.create_window(attributes).expect("window"));
        let size = window.surface_size();
        self.window = Some(window.clone());
        report(&format!("{}: window", self.stage.name()));

        if matches!(self.stage, Stage::Window) {
            self.done(event_loop);
            return;
        }

        if matches!(self.stage, Stage::Hybrid) {
            let mut renderer = VelloHybridWindowRenderer::new();
            renderer.resume(Arc::new(window.clone()), size.width, size.height, || {});
            renderer.complete_resume();
            report(&format!("{}: renderer resumed", self.stage.name()));
            renderer.render(|_scene| {});
            report(&format!("{}: one empty frame", self.stage.name()));
            self.hybrid = Some(renderer);
            self.done(event_loop);
            return;
        }

        if matches!(self.stage, Stage::Vello) {
            // Vello brings its own instance, adapter and device — measuring
            // ours beside it would count two of everything.
            let mut renderer = VelloWindowRenderer::new();
            renderer.resume(Arc::new(window.clone()), size.width, size.height, || {});
            renderer.complete_resume();
            report(&format!("{}: renderer resumed", self.stage.name()));
            renderer.render(|_scene| {});
            report(&format!("{}: one empty frame", self.stage.name()));
            self.renderer = Some(renderer);
            self.done(event_loop);
            return;
        }

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        report(&format!("{}: instance", self.stage.name()));
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("floor"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .expect("device");
        drop(queue);
        report(&format!("{}: device", self.stage.name()));

        let surface = if matches!(self.stage, Stage::Surface) {
            let surface = instance.create_surface(window.clone()).expect("surface");
            let config = surface
                .get_default_config(&adapter, size.width, size.height)
                .expect("surface config");
            surface.configure(&device, &config);
            report(&format!("{}: surface configured", self.stage.name()));
            Some(surface)
        } else {
            None
        };

        self.gpu = Some(Gpu {
            _instance: instance,
            _adapter: adapter,
            device,
            surface,
        });
        self.done(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::CloseRequested) {
            event_loop.exit();
        }
    }

    fn new_events(&mut self, event_loop: &dyn ActiveEventLoop, _cause: winit::event::StartCause) {
        let Some(built) = self.built else { return };
        if built.elapsed().as_secs_f64() >= self.hold {
            report(&format!("{}: settled", self.stage.name()));
            if let Some(gpu) = self.gpu.as_ref() {
                // Keep the device and surface alive to the last measurement:
                // dropping them early would report a floor nothing stands on.
                let _ = (&gpu.device, &gpu.surface);
            }
            event_loop.exit();
        }
    }
}

impl Floor {
    /// Everything this stage builds is built; wait, then measure and go.
    fn done(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.built = Some(Instant::now());
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + Duration::from_secs_f64(self.hold),
        ));
    }
}
