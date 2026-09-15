//! One page, drawn by the renderer and handed to Vello as a texture.
//!
//! This is what a `<canvas>` and its 2D context became. The lifecycle is the
//! same three moments a `CustomPaintSource` had — the renderer came up, the
//! renderer went away, draw yourself — and the fourth is the one that decided
//! the experiment: `requires_redraw`, which says whether the document has to
//! keep painting on this widget's account. A page is not an animation, so it
//! says no, and a document sitting still costs no frames.
//!
//! What replaces `keyFor()` is the component key: the page, its size, the
//! colours it wears, its view and the draft of the document. A change to any
//! of them is a new node and a fresh render — the theme included, because the
//! page as pdfium drew it is not kept on the GPU (see `gpu.rs`), so a theme
//! change has nothing to re-run a compute pass over. What the key buys is
//! that the old texture is given back by Blitz, between frames, where it is
//! safe; a widget replacing its own texture cannot do that (see below).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};

use anyrender::{PaintScene, RenderContext, Scene};
use blitz_dom::node::ComputedStyles;
use blitz_dom::Widget;
use dioxus_native::DeviceHandle;
use peniko::kurbo::{Affine, Rect};
use peniko::{Blob, Fill, ImageAlphaType, ImageBrush, ImageData, ImageFormat, ImageSampler};

use blitz_traits::shell::ShellProvider;

use crate::gpu::{PageTexture, Recolorer};
use crate::layout::{View, MAX_PIXELS};
use crate::palette::Palette;
use crate::recolor::Region;
use crate::render::{Bitmap, PageSource};
use crate::stats;

/// What a page has painted *into* it rather than drawn over it.
///
/// **Two things here are the colour of the page rather than a rectangle on top
/// of it.** A link, because a tinted box blended into the ink below is at the
/// mercy of the compositor and a dropped blend is a solid band across the line.
/// A selected passage, because a translucent rectangle leaves the words the
/// colour they were printed in — it says *something is selected here* rather
/// than *these words are selected*.
///
/// **In fractions of the page's box**, the one space both ends agree about: a
/// texture is drawn at the box's size times the density, held under a ceiling
/// and frozen mid-pinch, and a fraction survives all three.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ramped {
    /// Left, top, right, bottom. The document's own links.
    pub links: Vec<[f32; 4]>,
    /// The same, for what the reader has swept over.
    pub selection: Vec<[f32; 4]>,
}

/// What every page needs to know and none of them owns: which theme is on, and
/// what it has to paint into itself.
///
/// A widget is handed to Blitz once, by a write-once attribute, so it cannot be
/// given new props the way a component can. This is the shared cell the app
/// writes and every mounted page reads on its next paint — which is the frame
/// the theme change causes anyway.
#[derive(Clone)]
pub struct Chosen {
    theme: Rc<Cell<Palette>>,
    /// The document as it stands. **Here rather than in the page's key**, so
    /// that a mark written into the file — a reload of the same document —
    /// redraws each mounted page in place, the old texture staying on screen
    /// until the new one lands. In the key, a reload was a fresh widget with
    /// nothing to show for as long as pdfium took: every page went blank and
    /// came back, once per highlight.
    document: Rc<RefCell<Arc<dyn PageSource>>>,
    /// What each mounted page has to paint into itself, by page index.
    ///
    /// A shared cell for the reason the theme is one: a widget is handed to
    /// Blitz once and cannot be given new props, and a selection changes on
    /// every frame of a drag — putting it in the component's key would redraw
    /// the page from pdfium for every pixel the pointer moves.
    ramped: Rc<RefCell<HashMap<usize, Ramped>>>,
    /// Whether a zoom gesture is under way, in which case a page keeps the
    /// texture it has and is stretched to whatever size the layout is asking
    /// for this frame. See [`Chosen::holding`].
    holding: Rc<Cell<bool>>,
}

impl PartialEq for Chosen {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.theme, &other.theme)
    }
}

impl Chosen {
    pub fn new(theme: Palette) -> Self {
        Chosen {
            theme: Rc::new(Cell::new(theme)),
            document: Rc::new(RefCell::new(crate::render::nothing())),
            ramped: Rc::new(RefCell::new(HashMap::new())),
            holding: Rc::new(Cell::new(false)),
        }
    }

    /// What every mounted page has to paint into itself, all of it at once.
    ///
    /// The whole map rather than an entry, because the caller has the whole
    /// answer — it is building the pages — and because replacing it is what
    /// keeps this from growing by a page for every page ever scrolled past.
    pub fn place(&self, ramped: HashMap<usize, Ramped>) {
        *self.ramped.borrow_mut() = ramped;
    }

    pub fn ramped(&self, index: usize) -> Ramped {
        self.ramped
            .borrow()
            .get(&index)
            .cloned()
            .unwrap_or_default()
    }

    pub fn get(&self) -> Palette {
        self.theme.get()
    }

    /// The document every page draws from, replaced on open and on reload.
    pub fn show(&self, document: Arc<dyn PageSource>) {
        *self.document.borrow_mut() = document;
    }

    pub fn document(&self) -> Arc<dyn PageSource> {
        self.document.borrow().clone()
    }

    pub fn set(&self, theme: Palette) {
        self.theme.set(theme);
    }

    /// **A pinch is not a hundred zoom steps to be drawn one at a time.**
    ///
    /// A trackpad sends a magnification event a frame, each changing the size
    /// every page is laid out at. Redrawing on each asks pdfium for a page sixty
    /// times a second — and because a texture registered on one frame must not
    /// be drawn until the next (see `fresh`), every one of those frames paints
    /// nothing, so the document goes blank until the fingers stop.
    ///
    /// While this is on, a page keeps the texture it has and is stretched to the
    /// box the layout asks for: the words grow under the fingers, a little soft,
    /// and come back sharp when the gesture settles. The frozen size is in the
    /// component key too, so nothing is re-keyed on the way.
    pub fn holding(&self) -> bool {
        self.holding.get()
    }

    pub fn hold(&self, holding: bool) {
        self.holding.set(holding);
    }
}

/// A page's texture is replaced in place, and the old one goes two frames
/// later.
///
/// It used to belong to the node: the component key carried the page, its
/// size and the theme, and a change to any of them was a new node, a new
/// widget and a new texture, with the old node's resources released by Blitz
/// between frames. What that cost was a blank page for as long as pdfium took,
/// every time — a mark written in, a zoom settled — because the new node had
/// nothing to show until its own texture arrived.
///
/// So a widget now outlives its texture. A new draft of the same document at
/// the same size is drawn *into* the texture already registered
/// ([`Recolorer::repaint`]), and nothing on the GPU changes hands at all. A
/// new size is a new texture, registered on one frame and drawn from the next
/// (see `fresh`), with the old texture painted, stretched, until then — and
/// then retired: unregistered by the widget itself a couple of frames on,
/// once no scene in flight can still be naming it. Unregistering in the frame
/// that registered its replacement is what used to panic Vello ("tried to draw
/// an invalid empty image"); a frame or two later, it is a hash-map removal.
///
/// The theme is still in the key: a theme change re-keys every page at once,
/// which `fresh` was written around, and is left as it was.
pub struct PageWidget {
    index: usize,
    /// How the page is turned and how much of it is drawn.
    ///
    /// Given once, like the page number, because it is in the component's key
    /// — the same reason the size is not stored here and the theme is. A turn
    /// or a trim is a different node, a new widget and a new texture, and the
    /// old one is released by Blitz between frames where it is safe.
    view: View,
    chosen: Chosen,
    /// How a widget asks for another frame. The `fresh` dance below needs the
    /// frame after the one that registered the texture, and
    /// `requires_redraw()` cannot ask for it: `is_animating()` is read at the
    /// start of a frame, before the paint that would set the flag. So the
    /// widget asks the shell directly, which is what the shell provider is
    /// for.
    shell: Option<Arc<dyn ShellProvider>>,
    device: Option<DeviceHandle>,
    recolorer: Option<Rc<Recolorer>>,
    texture: Option<PageTexture>,
    /// The document the texture was drawn from. See [`Chosen::show`]: a
    /// texture from another draft is shown, stretched if need be, while this
    /// draft is drawn.
    drawn_from: Option<Arc<dyn PageSource>>,
    /// Textures replaced and not yet released, each with the frames left
    /// before it is. See the note above the struct.
    retired: Vec<(PageTexture, u8)>,
    /// Whether the texture was registered during the frame being painted.
    ///
    /// Registering a texture and drawing it in the same frame works until
    /// something else is unregistered in that frame too — which is what happens
    /// when every page is replaced at once. The new texture's override is
    /// missing at submit and Vello panics with "tried to draw an invalid empty
    /// image", from a stack naming neither cause.
    ///
    /// So a page is registered on one frame and drawn from the next.
    /// `requires_redraw` asks for that frame and then stops: a page is not an
    /// animation, but a page that has just arrived is one frame's worth of one.
    ///
    /// **And that only moves the collision a frame along.** The frame where
    /// every page is replaced at once is the whole of the problem, and it used
    /// to happen on every launch because the viewer was laid out at a default
    /// viewport and corrected on mount. `Reader` sizes it from the window before
    /// the first frame now. **Anything that re-keys every page at once brings
    /// this back.**
    fresh: bool,
    /// The same page, for a renderer that is not wgpu. See [`Software`].
    software: Option<Software>,
    /// A render of this page on its way from the render thread. See
    /// [`PageWidget::draw_on_thread`].
    pending: Option<Pending>,
    /// Whether the render thread said no. Remembered, because the answer to
    /// a page that will not draw is a blank page and not a page asked for
    /// again on every frame the last attempt requested.
    failed: bool,
}

/// How many frames a replaced texture is kept before it is unregistered:
/// the frame that registered its replacement and drew it instead, the frame
/// that first drew the replacement, and one more for a scene still in
/// flight. See the note above [`PageWidget`].
const RETIRES_IN: u8 = 3;

/// A page being drawn on the render thread, and how to tell it not to bother.
struct Pending {
    width: u32,
    height: u32,
    document: Arc<dyn PageSource>,
    done: Receiver<Result<Rendered, String>>,
    cancelled: Arc<AtomicBool>,
}

impl Pending {
    fn is(&self, width: u32, height: u32, document: &Arc<dyn PageSource>) -> bool {
        self.width == width && self.height == height && Arc::ptr_eq(&self.document, document)
    }
}

/// What comes back: the page as pdfium drew it, copied out of the renderer's
/// own buffer because that buffer is borrowed for the length of the call.
struct Rendered {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
    drew_in: f64,
}

type Job = Box<dyn FnOnce() + Send>;

/// Hand a render to the one thread that draws pages.
///
/// One thread rather than a pool, because every render takes the process's
/// pdfium lock and a second thread would only queue on it. Jobs run in the
/// order they were mounted.
// ponytail: FIFO; nearest-to-the-middle first if a fast scroll feels late.
// Warning: nothing in `cargo test` reaches this thread. The harness has no
// device, so its pages go through `ensure_software`, which draws synchronously
// — the pending/cancel/redraw dance below is checked only by running the app.
fn render_thread(job: Job) {
    static QUEUE: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
    let queue = QUEUE.get_or_init(|| {
        let (sender, jobs) = channel::<Job>();
        std::thread::Builder::new()
            .name("render".into())
            .spawn(move || {
                for job in jobs {
                    job();
                }
            })
            .expect("a render thread");
        Mutex::new(sender)
    });
    let _ = queue.lock().unwrap_or_else(|e| e.into_inner()).send(job);
}

/// A page drawn for a renderer with no GPU behind it.
///
/// Two things want this. The harness builds its scene for `vello_cpu`, where
/// `renderer_specific_context()` has no `DeviceHandle` and every page would
/// otherwise come out blank — a screenshot test passing because it photographed
/// nothing. And `vello_cpu` is the fallback for hardware Vello's compute path
/// will not run on, where a reader whose pages are the one thing it cannot draw
/// is not a fallback.
///
/// It costs a copy the GPU path does not pay: a `peniko::ImageData` owns its
/// bytes and the recolouring reference works in RGBA, so the page is swizzled
/// and recoloured into a buffer of its own, one per mounted page.
struct Software {
    image: ImageData,
    width: u32,
    height: u32,
    /// What `PageTexture::wears` answers on the other path.
    theme: Palette,
    /// And what `drawn_from` answers.
    document: Arc<dyn PageSource>,
    /// The page with its links tinted and nothing selected on it, kept so that
    /// a selection can be taken up again without going back to pdfium.
    ///
    /// **The same allocation as `image` until something is selected**, which is
    /// what an `Arc` is for: while there is no selection the two point at one
    /// buffer and this costs a word, and a selection makes the second copy that
    /// the ramp is written into. The GPU path keeps only what is under the
    /// runs, because there a copy of the page is 25MB of texture.
    plain: Arc<Vec<u8>>,
    /// What is painted through the selection ramp on top of it.
    selected: Vec<[f32; 4]>,
}

impl PageWidget {
    pub fn new(
        index: usize,
        view: View,
        chosen: Chosen,
        shell: Option<Arc<dyn ShellProvider>>,
    ) -> Self {
        stats::add(&stats::MOUNTED, 1);
        PageWidget {
            index,
            view,
            chosen,
            shell,
            device: None,
            recolorer: None,
            texture: None,
            drawn_from: None,
            retired: Vec::new(),
            fresh: false,
            software: None,
            pending: None,
            failed: false,
        }
    }

    /// Ask the render thread for this page at this size. It answers through
    /// the channel and then asks the shell for a frame, which is the frame
    /// [`PageWidget::ensure`] uploads on.
    fn draw_on_thread(&self, width: u32, height: u32) -> Pending {
        let (sender, done) = channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let still_wanted = Arc::clone(&cancelled);
        let document = self.chosen.document();
        let drawn_from = Arc::clone(&document);
        let (index, view, shell) = (self.index, self.view, self.shell.clone());
        render_thread(Box::new(move || {
            // Scrolled past before its turn came: nothing to draw for.
            if still_wanted.load(Ordering::Relaxed) {
                return;
            }
            let mut drawn = None;
            let outcome = document.render(index, width, height, view, &mut |bitmap| {
                drawn = Some(Rendered {
                    width: bitmap.width,
                    height: bitmap.height,
                    bgra: bitmap.bgra.to_vec(),
                    drew_in: bitmap.drew_in,
                });
            });
            let answer =
                outcome.and_then(|()| drawn.ok_or_else(|| "the page was not drawn".to_string()));
            if sender.send(answer).is_ok() {
                if let Some(shell) = shell {
                    shell.request_redraw();
                }
            }
        }));
        Pending {
            width,
            height,
            document: drawn_from,
            done,
            cancelled,
        }
    }

    /// How many pixels this page is drawn at: what the layout asked for, held
    /// under the ceiling. A page drawn at more pixels than the screen can show
    /// is bytes nobody reads, and at high zoom on a large page it is a great
    /// many of them.
    fn drawn_size(width: u32, height: u32) -> (u32, u32) {
        let pixels = width as f64 * height as f64;
        if pixels <= MAX_PIXELS {
            return (width.max(1), height.max(1));
        }
        let shrink = (MAX_PIXELS / pixels).sqrt();
        (
            ((width as f64 * shrink).round() as u32).max(1),
            ((height as f64 * shrink).round() as u32).max(1),
        )
    }

    /// The links on this page, as regions of the drawn page.
    ///
    /// **Links are tinted only where the page is recoloured.** A theme with
    /// `recolor = false` leaves the page exactly as printed, links included —
    /// it used to tint them anyway, which made "Off leaves every page exactly
    /// as it was printed" untrue of Moonowl Light.
    fn links(&self, theme: &Palette, width: u32, height: u32) -> Vec<Region> {
        if !theme.recolor {
            return Vec::new();
        }
        let paper = theme.background;
        self.chosen
            .ramped(self.index)
            .links
            .iter()
            .map(|area| Region {
                area: [
                    area[0] * width as f32,
                    area[1] * height as f32,
                    area[2] * width as f32,
                    area[3] * height as f32,
                ],
                ink: theme.link,
                paper,
            })
            .collect()
    }

    /// The two colours a selection is painted between, the right way round for
    /// the page as it stands. See `regions.wgsl` and `selectionPaint` in
    /// `viewer.ts`.
    fn selection_ramp(theme: &Palette) -> (crate::recolor::Rgb, crate::recolor::Rgb) {
        let lit = theme.recolor
            && crate::palette::luminance(theme.text) > crate::palette::luminance(theme.background);
        if lit {
            (theme.selection_area, theme.selection_text)
        } else {
            (theme.selection_text, theme.selection_area)
        }
    }

    /// The same two questions, answered on the CPU. See [`Software`].
    ///
    /// The one difference that matters is where the theme goes on: there is no
    /// pass to re-run over a copy already uploaded, so a theme change is a
    /// re-render here. That is `keyFor()`'s own answer, and it is what makes
    /// this path a fallback rather than a design.
    fn ensure_software(&mut self, width: u32, height: u32) -> Option<()> {
        let theme = self.chosen.get();
        let document = self.chosen.document();
        let selection = self.chosen.ramped(self.index).selection;
        if let Some(page) = self.software.as_ref() {
            if page.theme == theme
                && Arc::ptr_eq(&page.document, &document)
                && (self.chosen.holding() || (page.width == width && page.height == height))
            {
                if page.selected != selection {
                    self.paint_selection_software(&theme, selection);
                }
                return Some(());
            }
        }

        let mut pixels: Option<Vec<u8>> = None;
        let outcome = document.render(self.index, width, height, self.view, &mut |bitmap| {
            // BGRA as pdfium wrote it, in RGBA order because that is what
            // the reference ramp reads — the swizzle the GPU path gets for
            // free by uploading as `Bgra8Unorm`.
            let mut rgba = bitmap.bgra.to_vec();
            for pixel in rgba.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            if theme.recolor {
                crate::recolor::recolor_cpu(
                    &mut rgba,
                    theme.text,
                    theme.background,
                    theme.keep_colour,
                );
            }
            crate::recolor::duotone_cpu(
                &mut rgba,
                width,
                height,
                &self.links(&theme, width, height),
            );
            pixels = Some(rgba);
        });
        if let Err(err) = outcome {
            eprintln!("{err}");
            return None;
        }
        let pixels = pixels?;

        if let Some(old) = self.software.take() {
            stats::sub(
                &stats::RESIDENT,
                (old.width as u64) * (old.height as u64) * 4,
            );
        }
        stats::add(&stats::DRAWN, 1);
        stats::add(&stats::RESIDENT, (width as u64) * (height as u64) * 4);
        let pixels = Arc::new(pixels);
        self.software = Some(Software {
            image: ImageData {
                data: Blob::new(pixels.clone()),
                format: ImageFormat::Rgba8,
                alpha_type: ImageAlphaType::AlphaPremultiplied,
                width,
                height,
            },
            width,
            height,
            theme,
            document,
            plain: pixels,
            selected: Vec::new(),
        });
        if !selection.is_empty() {
            self.paint_selection_software(&theme, selection);
        }
        Some(())
    }

    /// The selection ramp over the page the CPU path has already drawn, and off
    /// again — from the copy kept beside it. See [`Software::plain`].
    fn paint_selection_software(&mut self, theme: &Palette, runs: Vec<[f32; 4]>) {
        let Some(page) = self.software.as_mut() else {
            return;
        };
        let (ink, paper) = Self::selection_ramp(theme);
        let (width, height) = (page.width, page.height);
        let regions: Vec<Region> = runs
            .iter()
            .map(|area| Region {
                area: [
                    (area[0] * width as f32).floor(),
                    (area[1] * height as f32).floor(),
                    (area[2] * width as f32).ceil(),
                    (area[3] * height as f32).ceil(),
                ],
                ink,
                paper,
            })
            .collect();
        // Nothing selected is the page as it was drawn, which is the buffer
        // already in hand — no copy at all rather than a second identical page.
        // See [`Software::plain`].
        let data = if regions.is_empty() {
            Blob::new(page.plain.clone())
        } else {
            let mut pixels = page.plain.as_ref().clone();
            crate::recolor::duotone_cpu(&mut pixels, width, height, &regions);
            Blob::new(Arc::new(pixels))
        };
        page.image = ImageData {
            data,
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::AlphaPremultiplied,
            width,
            height,
        };
        page.selected = runs;
    }

    /// Draw the page if it is not already drawn at this size, and put the
    /// theme on it if it is not already wearing it.
    fn ensure(&mut self, ctx: &mut dyn RenderContext, width: u32, height: u32) -> Option<()> {
        if self.failed {
            return None;
        }
        let theme = self.chosen.get();
        let recolorer = Rc::clone(self.recolorer.as_ref()?);
        let document = self.chosen.document();
        let same_draft = self
            .drawn_from
            .as_ref()
            .is_some_and(|drawn| Arc::ptr_eq(drawn, &document));

        let selection = self.chosen.ramped(self.index).selection;
        if let Some(texture) = self.texture.as_ref() {
            // Size and theme together are what `keyFor()` is: a page that
            // matches both is the page already on the screen.
            //
            // …except under a zoom gesture, where the size is the one thing
            // deliberately allowed to differ: the texture is kept and
            // stretched rather than redrawn. See [`Chosen::holding`]. The
            // theme is still asked, because a theme changed mid-gesture is a
            // page that is the wrong colour rather than the wrong sharpness.
            if texture.wears(&theme)
                && same_draft
                && (self.chosen.holding() || texture.is(width, height))
            {
                // The selection is the one thing that moves without the page
                // being redrawn, and it is asked here rather than in the key
                // for exactly that reason.
                let (ink, paper) = Self::selection_ramp(&theme);
                let texture = self.texture.as_mut()?;
                recolorer.select(texture, &selection, ink, paper);
                return Some(());
            }
        }

        // **Drawn off the thread that paints.** pdfium takes 10-80ms on a
        // scan or a page of figures, and drawing inside `paint` was that long
        // a stall in the frame — three pages mounted on a fast scroll was a
        // quarter of a second in one. So the page is drawn on the render
        // thread and this frame draws what it has: the old texture
        // stretched, if there is one, or nothing. The thread asks for a frame
        // when it is done, and that frame uploads.
        let rendered = match self.pending.take() {
            Some(pending) if pending.is(width, height, &document) => {
                match pending.done.try_recv() {
                    Ok(Ok(rendered)) => rendered,
                    Ok(Err(err)) => {
                        eprintln!("{err}");
                        self.failed = true;
                        return None;
                    }
                    Err(TryRecvError::Empty) => {
                        self.pending = Some(pending);
                        return Some(());
                    }
                    Err(TryRecvError::Disconnected) => {
                        self.failed = true;
                        return None;
                    }
                }
            }
            stale => {
                // Asked at another size — a zoom that settled — so that one
                // is told not to bother and this size is asked for.
                if let Some(stale) = stale {
                    stale.cancelled.store(true, Ordering::Relaxed);
                }
                self.pending = Some(self.draw_on_thread(width, height));
                return Some(());
            }
        };
        let bitmap = Bitmap {
            width: rendered.width,
            height: rendered.height,
            bgra: &rendered.bgra,
            drew_in: rendered.drew_in,
        };
        let links = self.links(&theme, width, height);
        stats::add(&stats::DRAWN, 1);
        match self.texture.as_mut() {
            // The same size: drawn into the texture on screen, which changes
            // nothing the renderer holds and costs no frame.
            Some(texture) if texture.is(width, height) => {
                recolorer.repaint(texture, &bitmap, &theme, &links);
            }
            // A new size is a new texture, and the old one is shown for the
            // frame the new one cannot be, then released.
            _ => {
                let texture = recolorer.upload(ctx, &bitmap, &theme, &links)?;
                stats::add(&stats::RESIDENT, texture.bytes());
                if let Some(old) = self.texture.replace(texture) {
                    self.retired.push((old, RETIRES_IN));
                }
                self.fresh = true;
                // And a frame to draw it in.
                if let Some(shell) = &self.shell {
                    shell.request_redraw();
                }
            }
        }
        self.drawn_from = Some(document);
        if !selection.is_empty() {
            let (ink, paper) = Self::selection_ramp(&theme);
            let texture = self.texture.as_mut()?;
            recolorer.select(texture, &selection, ink, paper);
        }
        Some(())
    }
}

impl Widget for PageWidget {
    /// The renderer came up. Anything held from before it did belongs to a
    /// renderer that no longer exists — `WindowRenderer::suspend` drains every
    /// registered texture, and an id that outlives that is what Vello reports
    /// as "tried to draw an invalid empty image", from inside the next frame's
    /// atlas upload rather than from the call that orphaned it.
    fn can_create_surfaces(&mut self, render_ctx: &mut dyn RenderContext) {
        if let Some(texture) = self.texture.take() {
            stats::sub(&stats::RESIDENT, texture.bytes());
        }
        for (texture, _) in self.retired.drain(..) {
            stats::sub(&stats::RESIDENT, texture.bytes());
        }
        // The pipelines are *not* forgotten here: `destroy_surfaces` did that
        // when the renderer went, and `paint` comes through here for every
        // page mounted after startup — which was two shader modules and two
        // compute pipelines rebuilt per page scrolled into view.
        // No device means this is not wgpu — a headless test or the CPU
        // fallback — and the page is drawn into a `peniko::ImageData` instead.
        // See [`Software`].
        if let Some(device) = render_ctx
            .renderer_specific_context()
            .and_then(|ctx| ctx.downcast::<DeviceHandle>().ok())
        {
            self.recolorer = Some(Recolorer::shared(&device));
            self.device = Some(*device);
        }
    }

    /// The renderer is going away, and with it every resource registered
    /// against it. Whatever is kept across this moment is a texture belonging
    /// to a renderer that no longer exists — which is not a blank page but a
    /// dead one: "tried to draw an invalid empty image".
    fn destroy_surfaces(&mut self) {
        if let Some(texture) = self.texture.take() {
            stats::sub(&stats::RESIDENT, texture.bytes());
        }
        for (texture, _) in self.retired.drain(..) {
            stats::sub(&stats::RESIDENT, texture.bytes());
        }
        if let Some(page) = self.software.take() {
            stats::sub(
                &stats::RESIDENT,
                (page.width as u64) * (page.height as u64) * 4,
            );
        }
        if let Some(device) = self.device.take() {
            Recolorer::forget(&device);
        }
        self.recolorer = None;
    }

    /// A page is not an animation — except for the single frame between
    /// registering its texture and drawing it. See `fresh`.
    fn requires_redraw(&self) -> bool {
        self.fresh || !self.retired.is_empty()
    }

    fn paint(
        &mut self,
        render_ctx: &mut dyn RenderContext,
        _styles: &ComputedStyles,
        width: u32,
        height: u32,
        _scale: f64,
    ) -> Scene {
        let mut scene = Scene::new();
        if self.device.is_none() && self.software.is_none() {
            // `complete_resume` calls this for every widget in the document
            // when the renderer comes up; a page mounted later is caught here.
            self.can_create_surfaces(render_ctx);
        }
        // `width` and `height` arrive in *device* pixels — blitz-paint takes
        // them off the content box, which is already scaled — and `scale` is
        // passed alongside for whatever wants to know. Multiplying by it again
        // draws every page at twice the size it is shown at.
        let (drawn_width, drawn_height) = Self::drawn_size(width, height);

        // No device means no wgpu behind the scene being built — a headless
        // test, or the CPU fallback. Everything below the size is the same
        // question asked of a `peniko::ImageData` instead of a texture, and
        // none of the frame-ordering dance applies: there is nothing to
        // register, so there is nothing to register too early.
        if self.device.is_none() {
            if self.ensure_software(drawn_width, drawn_height).is_none() {
                return scene;
            }
            let Some(page) = self.software.as_ref() else {
                return scene;
            };
            let stretch = Affine::scale_non_uniform(
                width as f64 / page.width as f64,
                height as f64 / page.height as f64,
            );
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                ImageBrush {
                    image: &page.image,
                    sampler: ImageSampler::default(),
                },
                Some(stretch),
                &Rect::from_origin_size((0.0, 0.0), (width as f64, height as f64)),
            );
            return scene;
        }

        // Textures replaced earlier, released once no scene in flight can
        // still name them. Before `ensure`, so that a release and a
        // registration never share a frame.
        for (_, left) in &mut self.retired {
            *left = left.saturating_sub(1);
        }
        let mut retired = std::mem::take(&mut self.retired);
        retired.retain(|(texture, left)| {
            if *left > 0 {
                return true;
            }
            render_ctx.unregister_resource(texture.id());
            stats::sub(&stats::RESIDENT, texture.bytes());
            false
        });
        self.retired = retired;

        if self.ensure(render_ctx, drawn_width, drawn_height).is_none() {
            return scene;
        }
        // A texture registered during this frame must not be drawn during it.
        // See `fresh` on the struct: this is the frame that registers, and
        // `requires_redraw` asks for the one that draws. What is drawn instead
        // is the texture being replaced, if there is one — stretched to the
        // box, which is what the zoom gesture has been showing all along.
        let texture = if self.fresh {
            self.fresh = false;
            match self.retired.last() {
                Some((old, _)) => old,
                None => return scene,
            }
        } else {
            match self.texture.as_ref() {
                Some(texture) => texture,
                None => return scene,
            }
        };

        // A page drawn at a different size from its box — held under the
        // ceiling, or frozen for the length of a zoom gesture — is scaled to
        // fill it. Everything else lands at 1:1.
        //
        // **The scale goes in the scene transform, not the brush transform.**
        // `fill` in `anyrender_vello_hybrid` has two arms and the one for a
        // `PaintRef::Resource` ignores `brush_transform` completely: it takes
        // the shape's *origin*, draws the texture there at its own size, and
        // stops. Only the other arm reads it, which is why the software path
        // below is written the ordinary way and passes. So the destination
        // rectangle is the *texture's* and the scene transform stretches it,
        // which `draw_texture_rects` composes.
        //
        // Invisible until a page's texture is not the size of its box, and then
        // it is a page whose border grows under the reader's fingers while the
        // print inside stays where it was.
        let stretch = Affine::scale_non_uniform(
            width as f64 / texture.width as f64,
            height as f64 / texture.height as f64,
        );
        scene.fill(
            Fill::NonZero,
            stretch,
            anyrender::PaintRef::Resource(ImageBrush {
                image: texture.id(),
                sampler: ImageSampler::default(),
            }),
            None,
            &Rect::from_origin_size((0.0, 0.0), (texture.width as f64, texture.height as f64)),
        );
        scene
    }
}

impl Drop for PageWidget {
    fn drop(&mut self) {
        // A render still queued for a page nobody is looking at any more.
        if let Some(pending) = &self.pending {
            pending.cancelled.store(true, Ordering::Relaxed);
        }
        // Unmounting is where the memory actually goes back, so this is the
        // half of `mount()`/`OVERSCAN` that the accounting can see.
        if let Some(texture) = self.texture.take() {
            stats::sub(&stats::RESIDENT, texture.bytes());
        }
        if let Some(page) = self.software.take() {
            stats::sub(
                &stats::RESIDENT,
                (page.width as u64) * (page.height as u64) * 4,
            );
        }
        stats::sub(&stats::MOUNTED, 1);
    }
}
