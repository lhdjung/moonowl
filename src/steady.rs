//! The renderer, with two things put right on the way through: a failed
//! frame, below, and a resize while the surface is being made (see
//! `complete_resume`).
//!
//! **A frame that fails to reach the screen is not thrown away, and the next
//! one is drawn on top of it.** `render` in `anyrender_vello_hybrid` resets
//! the scene only after a successful present, and there are two early returns
//! before that — a surface texture that cannot be had, and a present that
//! fails. So a window whose surface has gone away accumulates one whole frame
//! per redraw in a single scene until `vello_common` asserts:
//!
//! ```text
//! thread 'main' panicked at vello_common-0.1.0/src/strip.rs:205:
//! `alpha_idx` too large
//! ```
//!
//! — a `u32` running into the bit `Strip` packs its fill flag into, at about
//! two billion. The way to see it is to leave the reader open and let the
//! display sleep; the app dies while nobody is looking at it. Blitz declines
//! to paint an *occluded* window, which is the case macOS reports; this is the
//! case it does not, and a `Timeout` from `get_current_texture` during any
//! hiccup poisons the scene the same way and never recovers.
//!
//! The fix upstream is to reset on those two paths. From outside the crate the
//! one hook is that the draw closure is handed the scene, so this wrapper
//! resets at the *start* of every frame. The base colour is filled back in
//! because the reset takes the inner renderer's own fill with it.
//!
//! Everything else here is delegation, written out only because
//! `WindowRenderer` has eight methods and no blanket impl.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use anyrender::{
    PaintScene, RegisterResourceError, RenderContext, ResourceId, WindowHandle, WindowRenderer,
};
use dioxus_native::DioxusNativeWindowRenderer;
use peniko::kurbo::{Affine, Rect};
use peniko::{Color, Fill};

/// The renderer every window in this app is given. See the module comment.
#[derive(Clone)]
pub struct Steady {
    inner: DioxusNativeWindowRenderer,
    /// What to fill the ground with, in the size the surface was last told
    /// about. `set_size` is the only thing that knows it and `render` is the
    /// only thing that wants it.
    size: Rc<Cell<(u32, u32)>>,
}

impl Steady {
    pub fn new() -> Self {
        Steady {
            inner: DioxusNativeWindowRenderer::new(),
            size: Rc::new(Cell::new((0, 0))),
        }
    }
}

impl Default for Steady {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderContext for Steady {
    fn try_register_custom_resource(
        &mut self,
        resource: Box<dyn Any>,
    ) -> Result<ResourceId, RegisterResourceError> {
        self.inner.try_register_custom_resource(resource)
    }

    fn unregister_resource(&mut self, resource_id: ResourceId) {
        self.inner.unregister_resource(resource_id);
    }

    fn renderer_specific_context(&self) -> Option<Box<dyn Any>> {
        self.inner.renderer_specific_context()
    }
}

impl WindowRenderer for Steady {
    type ScenePainter<'a>
        = <DioxusNativeWindowRenderer as WindowRenderer>::ScenePainter<'a>
    where
        Self: 'a;

    fn resume<F: FnOnce() + 'static>(
        &mut self,
        window: Arc<dyn WindowHandle>,
        width: u32,
        height: u32,
        on_ready: F,
    ) {
        self.size.set((width, height));
        self.inner.resume(window, width, height, on_ready);
    }

    /// **A resize that arrives while the surface is being made is lost.**
    /// `set_size` in `anyrender_vello_hybrid` makes a scene of the new size at
    /// once but can resize the surface only once it exists, and it decides
    /// whether anything changed by the scene's size. So the size Blitz sends
    /// again after the resume matches the scene and is ignored, and the
    /// surface keeps the size it was made at. A window opened maximized is
    /// made at its own size and maximized before its surface exists: macOS
    /// then stretched a 1280-pixel frame over a 1512-pixel window, and cut off
    /// its right edge, until the window was next resized. So once the surface
    /// exists it is told its size with a scene that differs from it.
    fn complete_resume(&mut self) -> bool {
        let pending = self.inner.is_pending();
        let active = self.inner.complete_resume();
        let (width, height) = self.size.get();
        if pending && active && width > 0 && height > 0 {
            self.inner.set_size(width, height + 1);
            self.inner.set_size(width, height);
        }
        active
    }

    fn suspend(&mut self) {
        self.inner.suspend();
    }

    fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    fn is_pending(&self) -> bool {
        self.inner.is_pending()
    }

    fn set_size(&mut self, width: u32, height: u32) {
        self.size.set((width, height));
        self.inner.set_size(width, height);
    }

    fn render<F: FnOnce(&mut Self::ScenePainter<'_>)>(&mut self, draw_fn: F) {
        let (width, height) = self.size.get();
        self.inner.render(|scene| {
            // Whatever a failed frame left behind, and the inner renderer's
            // own base fill with it.
            scene.reset();
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                Color::WHITE,
                None,
                &Rect::new(0.0, 0.0, width as f64, height as f64),
            );
            draw_fn(scene);
        });
    }
}
