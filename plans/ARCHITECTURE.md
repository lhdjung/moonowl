# How Moonowl works on the inside

A guided tour of the codebase for somebody who knows Rust but has not lived
inside Dioxus Native. [`AGENTS.md`](../AGENTS.md) is the reference — dense,
full of the rules and the traps behind individual decisions. This document is the map you read
first: what the pieces are, which one talks to which, and how a keystroke or a
scroll turns into pixels.

---

## 1. The one-paragraph version

Moonowl is **one Rust process with no webview and no JavaScript**. The interface
is written as HTML and CSS, but nothing resembling a browser runs it: the
*Blitz* engine parses the CSS with Firefox's style system, lays boxes out with a
flexbox library, shapes text with a text library, and paints the result on the
GPU. *Dioxus* sits on top of that and gives you React-style components and
reactive state in Rust. PDF pages are rasterised by *pdfium* (Chrome's PDF
library, loaded as a shared library), uploaded to the GPU as textures, recoloured
by a compute shader, and handed to Blitz as a custom widget that sits inside the
HTML tree like a `<canvas>` would.

So there is no "backend" and "frontend" in the client/server sense. There *is* a
clean split, though, and it is worth holding on to:

| layer | what it is | files |
| --- | --- | --- |
| **Process** | the event loop, windows, single-instance, file watching | `main.rs`, `lib.rs`, `shell.rs`, `steady.rs`, `nav.rs`, `session.rs`, `windows.rs`, `single.rs`, `watch.rs`, `emit.rs` |
| **Interface** | components, state, keyboard, CSS | `app.rs`, `sidebar.rs`, `prefs.rs`, `keymap.rs`, `styles.rs`, `icons.rs` |
| **Document engine** | layout maths, rendering, recolouring, search, selection, markup | `layout.rs`, `render.rs`, `pdfium.rs`, `page.rs`, `gpu.rs`, `recolor.rs`, `search.rs`, `select.rs`, `markup.rs`, `sign.rs`, `crop.rs` |
| **Persistence** | settings, themes, highlight palettes, library, key bindings | `store.rs`, `settings.rs`, `shelf.rs`, `theme.rs`, `palettes.rs`, `palette.rs`, `library.rs`, `keys.rs`, `config.rs` |
| **Platform glue** | the bits that are AppKit / Win32 | `dock.rs`, `openfiles.rs`, `tabs.rs`, `print.rs` |
| **Test rig** | the whole UI driven headlessly | `harness.rs`, `fixture.rs`, `stats.rs`, `tests/` |

---

## 2. The stack under the app, for people new to Dioxus Native

You can read most of the code without knowing this, but the odd-looking parts
all trace back to it.

```
      your components (rsx!)                 app.rs, sidebar.rs, prefs.rs
              │
        ┌─────▼──────┐
        │   Dioxus   │   virtual DOM, signals, hooks, async tasks
        └─────┬──────┘
              │  mutations ("create div", "set attribute", …)
        ┌─────▼──────┐
        │ blitz-dom  │   a real DOM tree, in Rust
        │            │   · Stylo   – Firefox's CSS engine: cascade, selectors
        │            │   · Taffy   – flexbox / grid / block layout
        │            │   · Parley  – text shaping, line breaking, editing
        └─────┬──────┘
              │  a painted scene (rectangles, glyphs, images, textures)
        ┌─────▼──────┐
        │ anyrender  │   renderer abstraction
        │ vello-hybrid│  CPU/GPU 2D renderer over wgpu   (vello_cpu in tests)
        └─────┬──────┘
        ┌─────▼──────┐
        │   winit    │   the OS window, the event loop, raw input
        └────────────┘
```

**Dioxus** is the part you program against. A component is a function returning
`rsx! { div { class: "toolbar", … } }`. State lives in *signals*
(`Signal<Viewer>`); writing to a signal marks the components that read it as
dirty, and Dioxus re-runs them and diffs the result. It is React's model, in
Rust, without a browser.

**Blitz** is what makes the `div` real. It is *not* a browser — there is no
JavaScript, no networking to speak of, and a number of CSS features are missing
(`position: fixed`, and `static` positioning behaves differently). But it is a
real CSS engine, which is why `styles.rs` is 2,000 lines of ordinary CSS in a
string and the app looks like professional web design rather than like a native
toolkit.

**Custom widgets** are the escape hatch that makes a PDF reader possible. An
`<object>` element can be handed a Rust object implementing Blitz's `Widget`
trait; at paint time Blitz calls `widget.paint(ctx, width, height)` and
composites whatever scene comes back. `PageWidget` in `page.rs` is that object:
its scene is "draw this GPU texture here". This is what a `<canvas>` became.

**winit** owns the actual OS window and the event loop. Everything — a key, a
resize, a redraw — starts as a winit event.

Three consequences of this stack show up all over the code:

1. **Nothing can query the DOM from inside an event handler.** Blitz runs
   handlers while it holds a borrow of the document, so `get_client_rect()` or
   `scroll()` from a handler panics ("RefCell already borrowed"). Hence the
   app keeps its own numbers: the scroll offset is a field in `Viewer`, the
   window size is asked of winit (`Screen`), and pages are positioned by
   arithmetic rather than by the engine's scroller.
2. **Dioxus Native's `launch()` makes exactly one window.** Moonowl wants many,
   so it owns the event loop itself (`shell.rs`) and does the per-window Dioxus
   set-up by hand.
3. **Blitz and Parley come from our forks**, each pinned to one git revision in
   `Cargo.toml`: every Blitz crate names `lhdjung/blitz` directly, and parley
   is swapped in under `[patch]`.
   `tests/upstream.rs` documents the upstream faults the app is written around.

---

## 3. Start-up: `main.rs` line by line, in prose

`main()` is wiring and nothing else. In order:

1. **A panic hook** appends every panic, with a backtrace, to `crash.log` in
   the config directory — a release build on Windows has no console.
2. **Settings are read once** for the window's remembered size.
3. **Command line** — `--theme N`, and every other argument that is not a flag
   is a document (made absolute at the door by `config::absolute`, so the
   library, the watcher and the window registry all key on the same string).
   The first goes in the launch window; the rest are handed over like any
   document from outside.
4. **Single instance** — `single::claim` tries to lock `instance.lock` in the
   config directory. Failure means another Moonowl holds it: the paths are
   written down that one's Unix socket and this process exits 0. Success means
   we bind the socket and become the one instance.
5. **The named document is opened before any window exists**, so a headless CI
   runner can read `reader: 5 pages` off stdout — the packaging job's proof that
   the installed binary found its pdfium.
6. **The process-wide objects are made**: the `Shell` (event loop handler), the
   `Desk` (which window shows what), the `Exchange` (per-window mailboxes) and
   one `watch::Watching` (a watcher thread over the themes and palettes
   directories and every open document). They are bundled into a `Session`,
   with the config directory, `--theme` and the window's size, and the
   `Session` is the factory for windows.
7. **Closures are hung on the shell**: `on_launch`, `on_request`, `on_close`,
   `on_swap`, `on_focus`, `on_resized`, `on_pinch`, `on_theme`, `on_drop`,
   `on_quit`. The shell deliberately knows nothing about documents; these
   closures are where window events meet app meaning. Most of them do one
   thing: **post a piece of `News` into a window's mailbox**.
8. **Doors from outside are installed**: the socket listener thread, the macOS
   Dock menu, and the macOS Apple-Event delegate (`openfiles.rs`) — a
   double-clicked PDF in the Finder is an Apple Event, not an argument.
9. `event_loop.run_app(shell)` — and the process lives in there until quit.
10. **After the loop**, `farewell`: the launch window's geometry is written,
    the socket is removed (by the process that holds it, and only that one),
    `store::flush()` writes the last reading position, and a document write
    still on its thread is waited for (`stats::WRITING`). A log-out never
    gets here, so `openfiles.rs` runs the same closure itself.

The launch window is decided lazily, at winit's first `can_create_surfaces`,
because that is the first moment a Finder launch's documents are known. Its
source, in priority order: the command line → the Finder → the most recently
read document (`store::reopening`) → the start screen.

---

## 4. Windows: `shell.rs`, `session.rs`, `windows.rs`

This trio replaces what `dioxus_native::launch()` would have done, and it is the
part of the app most unlike a normal Dioxus program.

### `shell.rs` — the mechanism

`Shell` wraps Blitz's `BlitzApplication` and implements winit's
`ApplicationHandler`. For each window it:

- builds a `DioxusDocument` from a `VirtualDom` (passing the HTML parser and a
  navigation provider that `launch()` would otherwise have supplied),
- wraps the renderer in `Steady` (`steady.rs`), which resets the scene at the
  top of every frame — a workaround for an upstream bug where frames that fail
  to present accumulate until the renderer asserts and the app dies while the
  display sleeps,
- **injects contexts into the component tree**: the renderer, the `Windows`
  handle, the winit window, Blitz's shell provider (clipboard, file dialog,
  redraw requests), and four small "doors" — `Screen` (how big am I?),
  `Appearance` (is the OS dark?), `Frame` (close me / full-screen me / new
  window) and `Printer`,
- places it (cascading one step down from the front window) and resumes it.

**Every window verb is deferred through the event-loop proxy.** A component
cannot close its own window from inside an event handler — it is running inside
a borrow of that very window. So `Frame::ask(Ask::Close)` posts an
`embedder_event`, and `Shell::proxy_wake_up` answers it on the next turn. The
small private structs at the top of `shell.rs` (`Spawn`, `Wanted`, `CloseOne`,
`Swapped`, `Show`, `SelectTab`, `StepTab`, `FullScreen`, `Minimize`,
`UiScale`, `Print`, `Quit`) are that
vocabulary. `Remote` is the `Send` half of the same door, which is how the
socket thread and the Dock menu ask for windows from other threads.

`window_event` also does a few things Blitz does not: it reports resizes,
pinches, OS light/dark changes and file drags outward (Blitz has no DOM events
for them), gives keyboard focus back to the root after clicks and keys, and keeps macOS's
IME enabled so Backspace works in text fields (on a Mac, Backspace is delivered
as a "standard key binding", not as a key).

### `session.rs` — the factory

`Session::window_on` is where a window is *born*: it claims a label (`main`,
then `reader-1`, …) on the `Desk`, writes the restore list, makes a `Post`
(mailbox) and joins it to the `Exchange`, builds a `VirtualDom` whose root
component is `app::Reader`, and provides the mailbox, exchange, watcher and
desk as contexts. A locked PDF still gets a window — with the password prompt
over an empty document, and the desk told it is showing the document it asks
about.

`Session::hand_over` is what happens to a document arriving from outside
(second launch, Finder, drag on the Dock): ask the `Desk`, then bring an
existing window forward, fill an empty one (via an `Event::HandedOver`,
which the window sends on beside itself if it turns out not to be empty), or
spawn a new window/tab.

### `windows.rs` — the rules

`Desk` is pure bookkeeping with no window type in it, so it is unit-testable:
which label shows which path, which window is in front, whether the app is
quitting. Its three rules: *nothing is ever displaced*, *an already-open
document comes forward instead of opening twice*, and *a window closing means
"I'm done with this" unless the app is quitting* — and the last window never
writes an empty restore list, because closing the last window is how most
people quit.

---

## 5. How the outside world reaches a component: `emit.rs`

This is the closest thing the app has to a backend→frontend channel.

A component can only be changed by something running *inside* its virtual DOM.
A watcher thread, a timer, or winit itself are all outside it. The bridge:

- **`Post`** — one window's mailbox: a queue plus a `Waker`.
- **`Exchange`** — every window's `Post`, by label. `post(news)` delivers to the
  named window, or to all of them when `target` is `None`.
- **`News`** — an `Event`, which carries its own data, and a target.
- **`after(delay, post, news)`** — one process-wide timer thread (a heap of
  deadlines and a condvar) that delivers news later. One thread rather than
  one per timer, which a long scroll would run the process out of.

Inside `Reader`, one long-lived async task (`listen` in `app/listen.rs`)
loops on `post.next().await` and matches on `emit::Event`, an enum whose
variants carry their own data: the document changed or was written, markup
and annotations read, themes and palettes changed, settings, keys and the
interface's size, windows resized and pinched, the appearance, documents
handed over, opened or dragged, themes imported and exported, and the
timers. The match is exhaustive, so an event nobody answers does not
compile.

Waking is real, not polled: sending to a `Post` wakes the task's waker, which
wakes the virtual DOM, which puts an event on the winit loop. An idle Moonowl
draws zero frames. In the test harness the same wake simply makes the next
`pump()` run the task.

```
 watcher thread ─┐
 timer thread  ──┤                        ┌─ Reader's mailbox task ─► viewer.write()
 winit (resize,  ├─► Exchange ─► Post ───►│                               │
  pinch, drop) ──┤    (by window label)   └───────────────────────────────▼
 socket thread ──┘                                              Dioxus re-renders
```

---

## 6. The interface: `app.rs` and `app/`

The heart, in three parts, with the halves that stand alone in child modules
under `src/app/`. A child module sees `Viewer`'s private fields, so nothing
was made `pub` to move them out; a method called from outside its module is
`pub(super)`.

| `app/` | what is in it |
|---|---|
| `menus.rs` | the Document menu, the right-click menus on a page and a mark, Copy over a window's words |
| `actions.rs` | `perform`, one arm per `Action` |
| `listen.rs` | `listen`, the mailbox task (§5) |
| `toolbar.rs` | `Toolbar` and `FindCard` |
| `dialogs.rs` | the Details, Sign and password windows |
| `ink.rs`, `markup.rs`, `search.rs`, `disk.rs` | `impl Viewer` blocks: signing; highlights, comments, the highlight colours and undo; the find bar and its index; opening, writing, reloading and putting a document down |

Line numbers move with every commit, so each part is placed by what sits
around it.

### 6a. `Viewer` — all of one window's state (`struct Viewer` and its `impl`, after the doors and the small types it holds, and the four `impl` modules above)

One big struct, held in one `Signal<Viewer>`. It contains the open document
(`Arc<dyn PageSource>`), the `Layout`, the scroll offset, the `Store`
(settings + themes + palettes + library), the keymap, the search, the selection, the
markup list, which menu/panel/dialog is open, and a lot of small gesture state.

Nearly everything the app *does* is a method on `Viewer`: `nudge`, `zoom_by`,
`jump_to`, `find`, `mark_selection`, `open_here`, `document_changed`,
`fit_window`… Handlers in the component tree are one-liners that call
`viewer.write().something()`. That shape is what makes the app testable and
what keeps the `rsx!` readable.

Two design points worth understanding:

- **Scrolling is the app's own.** `scroll_top` is an `f64`. The wheel handler
  adds to it; pages are absolutely positioned at `box.top - scroll_top`. Blitz's
  `overflow: scroll` is not used for the document, because the app must also
  *move* the document (page jumps, zoom-keeping-place) and cannot call the
  DOM's scroll API from a handler. The scrollbar is therefore drawn by the app
  too. The cost is no platform fling; the benefit is that position is a plain
  number that layout maths, tests and the library all share.
- **Timers are tokens.** "Hide the page pill 1.1s after the last scroll" is:
  bump `pill_token`, arm `after(…, Token(n))`; when the news arrives, act only
  if `n` is still current. No timer is ever cancelled; stale ones are ignored.

### 6b. `Reader` — the root component (straight after `impl Viewer`; the field helpers
`select_on_arrival`, `caret_on_arrival` and `leave_field` sit just before it)

`Reader` runs on every state change. Its body, in order:

1. **Consume contexts** with fallbacks (`Screen`, `Appearance`, `Frame`,
   `Printer`, `Clip`, `Pick`, `Away`, `Reveal`, `Pointer`). Each is a tiny
   `Rc<dyn Fn>` wrapper made by the `door!` macro. The shell provides the real
   ones; the test harness provides recording fakes; with neither there is a
   sane default. **This is the app's dependency-injection seam**, and it is why
   the whole UI runs without a window.
2. **Create the `Viewer` signal** — sized from the window *before the first
   frame* (laying out at a default size and correcting on mount re-keyed every
   page at once, which crashed the renderer).
3. **Build the key handler.** One `onkeydown` on the root element. The event is
   turned into a chord, looked up in the `Keymap`, and dispatched to `perform()`
   (`app/actions.rs`) — one `match` arm per `Action`, so a missing action is a
   compile error.
4. **Spawn the mailbox task**, `listen` in `app/listen.rs` (§5).
5. **Three `use_effect`s** that arm the notice, pill/scrollbar and zoom-settle
   timers. Note the trap documented there: what an effect remembers between
   runs must live in a `use_hook`, never in the closure — the closure is
   replaced on every render. Getting that wrong once cost 100% of a core at
   idle; `tests/settle.rs` guards it.
6. **Read the state once** into locals, compute which pages are mounted, and
7. **Emit the `rsx!` tree**: toolbar → find bar → body (start screen | sidebar +
   viewer) → notice line → modal windows (Settings, password, details, note,
   sign, colours…).

**`Reader` renders on every write to the viewer**, which is every frame of a
scroll. A component under it that takes `Signal<Viewer>` and reads it does
too. `Toolbar` instead reads `Bar`, a `use_memo` of the values it shows, and
renders only when one changes; `tests/frames.rs` counts it across a scroll.
The windows in `dialogs.rs` read the viewer directly, because they are only
mounted while open.

Because Blitz has no `position: fixed`, the root is a flex column and overlays
are absolutely positioned children with explicit `z-index` (which also matters
for hit-testing in Blitz).

### 6c. `Page` — one mounted page (after `Reader`; `Start`, `Roll`, `Scrawl`,
`Icon` and `NoteField` sit before it, and `find_quote` follows it)

```rust
div.page  (absolute; top = box.top - scroll_top)
 ├─ object { data: PageWidget }     ← the pixels
 ├─ object { data: PageWidget::detail } ← the part on screen, sharp, past MAX_PIXELS
 ├─ div.selected …, div.kept …      ← hit areas / overlays, all plain nodes
 ├─ div.hit … (search matches)
 ├─ note badges and comment cards, markup popover
 ├─ div[role=link] …                ← one node per PDF link (not an `<a>`)
```

The widgets are created once in a `use_hook` and handed to Blitz by attribute.
The component **key** is `"{index}:{view}:{opened}"` — page number,
rotation/crop, and which document this is. A key change means a new node and a
new texture; anything *not* in the key (size, the theme, a new draft of the
same file, the selection) is handled inside the widget without losing the old
picture.

There is no text layer. pdfium reports a box per *character*, so a search hit, a
selection and a link are all just rectangles in PDF points, multiplied by the
page's scale.

### The rest of the interface

- **`sidebar.rs`** — contents/outline, marks, thumbnails (its own virtualised
  column with its own scroll number), search results.
- **`prefs.rs`** — the Settings window, the theme editor and the highlight
  colours window; a draft theme is installed live, so the app around you is
  the preview, and a draft palette is offered by the swatches at once.
- **`keymap.rs`** — every `Action`, its default chords, `keys.toml` overrides,
  chord computation (offers both `key` and physical `code`, so ⌥⌘G works
  although Option turns G into ©), and sequences like `g g`.
- **`styles.rs`** — all CSS as one string. Colours come in as CSS variables that
  `Palette` derives from a theme's handful of colours.
- **The toolbar gives way in steps** (GitHub issue 3). Its three groups hold
  about 1100px of chips, and `.bar-left` shrinks to nothing while its chips do
  not, so without the steps a narrower window runs them on under the page
  controls. Four `@media` steps above `.chip` in `styles.rs`: at 1400px the
  left group's chips lose their words, at 1200px every chip does (each label
  is a `span.chip-label`) and keeps its symbol, at 720px the rotations and
  Close go, at 600px Sidebar, Search and the document's name.
  What is left needs 450px, and `session.rs` gives every window a minimum of
  480. The find card is wider than a bar of symbols has room for to the right
  of the Search chip, so under 1200px `Reader` hangs `FindCard` at the
  window's edge instead (`bar_tight`). `tests/chrome.rs` walks the widths.
- **`icons.rs`** — inline SVG strings, stroked with a colour passed from Rust
  (the CSS cascade cannot reach into an SVG rendered by `usvg`), and the owl
  the start screen wears on top of its scroll, in the theme's colours.

---

## 7. The document engine

### `render.rs` — the one door to the PDF library

`trait PageSource` is everything the rest of the app may ask of a document:
`pages`, `size_of`, `render`, `render_owned`, `text_of`/`try_text_of`,
`links_of`, `notes_of`, `outline`, `place`, `spaces`/`learn_spaces`, `labels`,
`title`, `details`, `markup`/`markup_on`, `signatures`, `seals`, `stamp`,
`release`/`retake`/`released`, `encrypted`, `sealed`, `path`, `password`,
`opened_in`. Everything but `pages`, `size_of`, `render` and `opened_in` has a
default (`render_owned` copies what `render` lends; the rest do nothing), so
swapping pdfium for another renderer is a contained job.

Two things to notice:

- `render` takes a **callback** that *borrows* the pixels: one scratch buffer per
  document is reused for every page. What has to keep the pixels or send them
  to another thread uses `render_owned`, which pdfium draws straight into a
  buffer the caller keeps. A page's pixels exist once; a copy is up to 48MB
  that the macOS allocator never gives back.
- `Nothing` is a `PageSource` with zero pages. The start screen is simply a
  window whose document is `Nothing`; no `Option<Document>` is threaded through
  the app.

### `pdfium.rs` — pdfium behind a lock

pdfium has process-wide state and no thread safety, so there is **one global
mutex** (`library()`), taken by every call — including `Drop`, which is the
call site nobody sees. Lock order is always library → document. Work nobody
is looking at yet (the markup walk after open) takes it through
`library_when_free`, which yields to any page being drawn.

At open it reads what decides the UI's shape, without loading a page: page
sizes and labels by index (including a heuristic that reads *printed* page
numbers off the margins of journal offprints), outline, title, metadata, and
whether the file is signed. Loading a page parses all of it, so nothing loads
a page to *place* something on it: a heading or link into a page not yet read
lands on the page's top, then on its spot once the page is read (`place`).
Text, links and notes are read lazily per page, links and notes on a thread.

pdfium keeps the file open for the document's life, so writing to that file
(highlights) requires `release()` first and a reopen after.

### `layout.rs` — where every page is

Pure arithmetic, no UI types, heavily tested. Given page sizes, the viewport,
fit mode (`Width`/`Page`/`Actual`), zoom, spread (`Single`/`Two`/`Cover`),
rotation, crop and mode (`Continuous`/`Paged`), it produces `boxes[]`: the
position and scale of every page. Binary searches over `boxes` answer "which
pages are near the viewport" (`mounted`, with `OVERSCAN`), "which page am I on",
and "where is this PDF rectangle on screen" (`place_on` / `unplace_on`).
An `Anchor` (page + fraction down it) is how a reading position survives zoom,
resize, reload and restart.

`crop.rs` measures margins once over a sample of eight pages (so the document
does not "breathe" page to page).

### `page.rs` + `gpu.rs` — from pdfium to the screen

This is the performance-critical path, so here is the whole life of a page:

```
 scroll brings page 12 within OVERSCAN
   └► Reader renders Page{index:11}; use_hook builds PageWidget
        └► Blitz paints; calls PageWidget::paint(ctx, w, h)      [main thread]
             └► ensure(): no texture → queue a job, draw nothing / old texture
                  └► "render" thread: pdfium draws BGRA into a buffer     [render thread]
                     the job keeps (render_owned), sends it back, asks
                     the shell for a redraw
             └► next paint: ensure() receives the bitmap
                  └► Recolorer::upload
                       · write BGRA into a temporary Bgra8 texture
                       · compute pass 1 (recolor.wgsl): luma ramp ink↔paper,
                         keeping hue if "keep colours"; output Rgba8 texture
                       · compute pass 2 (regions.wgsl): links ramped toward
                         the link colour
                       · drop the source texture (25MB saved per page)
                       · register the output with the renderer → ResourceId
             └► the frame after: scene.fill(texture) — page is on screen
```

Details that carry weight:

- **One render thread**, because every pdfium call takes the global lock anyway.
  It draws the job nearest the page the reader is on first
  (`Chosen::set_middle`); jobs for pages scrolled past are cancelled by an
  atomic flag.
- **Past `MAX_PIXELS` a page is drawn whole at a lower resolution**, and a
  second widget (`PageWidget::detail`) draws the part on screen, plus a
  margin, at full size over it.
- **A texture registered in one frame is drawn from the next** (`fresh`), and a
  replaced texture is retired three frames later (`RETIRES_IN`). Both dance
  around a Vello panic when registration and unregistration share a frame.
- **Zoom by pinch does not re-render.** `Chosen::holding` freezes every page's
  texture; the layout grows and the texture is stretched; 180ms after the last
  pinch event the pages re-render sharp.
- **The selection is painted into the texture** by the same region shader
  (`Recolorer::select`), with a small backup texture of what was underneath. So
  selected words take the theme's selection ink instead of sitting under a
  tinted rectangle.
- **A highlight being written is painted at once** (`Ramped::marking`) from
  the page's own pixels, and stays in the texture until the next draft's
  pixels, which carry the real mark, replace it.
- **`Chosen`** is the shared cell (theme, current document, per-page `Ramped`
  — links, selection, marks being written, the part on screen — the holding
  flag and the middle page) that lets the app talk to widgets it cannot pass
  props to. A widget is given to Blitz once and is thereafter opaque.
- **The pipeline cache is keyed by `wgpu::Instance`**, not `Device` — each
  window has its own device, and two devices compare equal by id.
- **Software fallback.** With no GPU device (the test harness, or `vello_cpu`),
  the same widget recolours on the CPU (`recolor.rs`, the reference
  implementation the shader is tested against) into a `peniko::ImageData`.

### `search.rs`, `select.rs`, `markup.rs`, `sign.rs`

- **Search** folds text (NFKD, ligatures, soft hyphens, case), scans from the
  current page to the end and then wraps (`pages_from_here`; only
  `find_quote`, which re-finds a lost highlight, searches outward) in 8ms slices driven by an async task that yields
  between slices (`Breathe`), caps at 100k matches, keeps at most
  `INDEX_BUDGET` (64MB) of text beyond the pages that match, and drops its
  index when the find bar closes.
- **Select** maps pointer positions to character indices (`caret_at`), handles
  word/line units for double/triple click, and spans pages.
- **Markup** writes real `/Highlight` annotations with pdfium: load bytes →
  edit → `FPDF_SaveAsCopy` (a full rewrite, not an incremental update) →
  atomic rename over the original → reopen, all of it on a thread of its own
  (`Viewer::write_step`, whose thread, `offload`, `document_changed` shares
  for a rebuild's reopen). A mark can be removed, recoloured or given a
  comment (its own `/Contents`), and every write keeps a copy of the file
  first (`markup::Before`, in the config directory's `undo/`), which is what
  undo puts back. Marks that cannot go into the file (read-only, encrypted)
  are kept in the library's journal "beside" the document.
- **Sign** places a drawn signature as an `/Ink` annotation or typed text (a
  date, a name) as a stamp, through the same `markup::edit` path.

---

## 8. Persistence

Everything lives in one config directory (`config.rs`; `MOONOWL_CONFIG`
overrides it), and every write goes through `atomic_write` (temp file + rename).
A *document* is written through `atomic_write_keeping`, the same thing with the
old file's permissions, ACL and extended attributes put on the new one first.

| file | module | contents |
| --- | --- | --- |
| `settings.toml` | `settings.rs` | flat key/value; the defaults table is also the whitelist; `set_many` rewrites only named keys, under a lock |
| `themes/*.toml` | `theme.rs`, `shelf.rs` | one file per theme. Built-ins are embedded (`build.rs` globs and *validates* `themes/` at compile time) and rewritten on every run; user themes are never touched, except that `save` renames a file the app itself named when its theme is renamed |
| `palettes/*.toml` | `palettes.rs`, `shelf.rs` | one file per highlight palette (a name and six colours), kept by the same rules as themes: `shelf.rs` is those rules, generic over `Kept` |
| `library.toml` | `library.rs` | per-document entries (last place, title, marks, trimmed margins, markup journal) and the `open` restore list |
| `keys.toml` | `keys.rs` | key overrides; not watched — there is a Reload button |
| `signatures/*.toml` | `sign.rs` | one file per drawn signature |
| `undo/` | `markup.rs` | the copies of a document taken before each write, for undo |
| `instance.lock`, `instance.sock` | `single.rs` | the single-instance claim, and the socket a second launch hands its document over |
| `crash.log` | `main.rs` | every panic, with its backtrace |

`settings.toml` and `library.toml` are written under `config::hold` (a mutex,
and a lock file beside them across processes), and one that does not parse is
never written over. `store.rs` is the façade the `Viewer` talks to.
`palette.rs` turns a theme's handful of colours (text, background, accent,
link, and optionally the selection's and the `ground` around the page) into
every shade the chrome needs (surface, lines, three greys, accent contrast…),
which is why a theme file can be five lines.

Almost nothing in the window writes the library or the settings itself: every
write goes down one channel to the **`Scribe`** thread (`moonowl-library`), in
order. The exceptions are the theme and palette editors' own files, and
`library::touch` at open, which is the read and the one place an unwritable
library is reported.
The reading position and a pinch's zoom change 60×/s, so those are coalesced
and written 700ms after they stop; marks, the journal, titles, theme slots,
the restore list and ordinary settings are written as they arrive.
`store::flush()` waits for the lot, and is called at quit, when a window
opens (`Store::at`), when a document is opened (`Store::opened`, so a return
within the settle reads the place just left) and when one is closed.

`watch.rs` runs one `notify` watcher thread for the themes and palettes
directories and the
*parent directory* of each window's document (a file is replaced by rename, so
watching the file itself would follow the dead inode) — the directory's *real*
name, so two spellings of one folder are one watch. An event is matched
against the path as opened *and* as the file system names it, because FSEvents
reports real paths and a document is often reached through a link; a document
that is itself a link is followed where it points. Events are collected
until 250ms of quiet; themes and palettes are reloaded and compared (the app
writes there itself, so an event is not news); a document is only reported
once `%PDF-` is at its start, `%%EOF` at its end, and it has held its size for
150ms.

Every window of the process shares one settings table (`store::shared`), so a
setting changed in one window is the setting in all of them; a theme worn in
one is sent to the rest as `Event::ThemeWorn` so that they repaint, and a
Reload of `keys.toml` as `Event::KeysReloaded`.

---

## 9. Threads

| thread | job | talks to the UI via |
| --- | --- | --- |
| **main** | winit loop, Dioxus, Blitz layout + paint, wgpu submission | — |
| `render` | pdfium rasterisation | channel + `request_redraw` |
| `moonowl-clock` | all delayed news | `Post::send` |
| watcher | file-system events | `Exchange::post` |
| scribe (`moonowl-library`) | every `library.toml` and `settings.toml` write, positions debounced | — |
| socket listener (unix) | second launches | `Remote::request` |
| print (Windows) | dialog + GDI job | — |
| file dialogs | Open, theme import and export (a modal dialog re-enters winit's handler) | `Post::send` |
| document work | a highlight, a signature or a rebuild: rewrite (or not), then reopen; the markup read after open; the margins measured; links and notes of pages coming into view | `Post::send` |

pdfium's global lock serialises the render thread and the document work
against main-thread pdfium calls (a page's text). `stats::WRITING` counts the
document-work threads, through a drop guard (`stats::Writing`), and is what
the harness and the end of `main` wait on.

---

## 10. Three end-to-end traces

**Pressing `j`.** winit `KeyboardInput` → `Shell::window_event` → Blitz
dispatches `keydown` to the root → `on_key` → `keymap.press()` →
`Press::Act(ScrollDown)` → `perform()` → `viewer.write().nudge(LINE)` → signal
dirty → `Reader` re-renders → `layout.mounted(scroll_top)` → `Page` components
re-positioned (same keys, so same widgets and textures) → Blitz relayout + paint
→ vello-hybrid → frame. The pill/scrollbar effect sees a new `scroll_top` and
arms two timers; `Store::remember` hands the anchor to the scribe.

**LaTeX rewrites the open PDF.** `notify` events → watcher thread collects
until quiet → `whole()` passes → `Exchange::post(Event::DocumentChanged, target =
"reader-1")` → mailbox task wakes → `Viewer::document_changed` → `offload`: a
thread opens the new `Document` and reads its markup (the old document stays
on screen meanwhile) and posts `Event::DocumentWritten` → `landed` → `adopt`: anchor
taken, the old document released, `Chosen::show(new)`, outline/labels/links/text
caches cleared, sizes replaced, margins re-measured on a thread,
`go_to(anchor)`, the search run again if the find bar is open, and
`Ask::Showing` if the `/Title` changed → page keys unchanged, so each mounted
`PageWidget` notices `drawn_from` ≠ current document, keeps showing the old
texture, and repaints *into it* when the render thread delivers. The reader
sees the text change in place with no flash.

**Double-clicking a second PDF in the Finder.** Apple Event → `openfiles.rs`
delegate → `Remote::request(Some(path))` → proxy wake → `Shell::proxy_wake_up`
→ factory closure → `Session::hand_over` → `Desk::hand_over` says `Spawn` →
`Session::window` → new `VirtualDom` → `Shell::open` as a tab or window.

---

## 11. Testing

`cargo test` turns on the `harness` feature. `harness::Reader` builds the same
`Reader` component in Blitz's headless test harness with a stated viewport,
delivers real pointer/key events through the real event pipeline, reads state
*off the interface* (page from the pill, zoom from its chip) rather than out of
`Viewer`, and can rasterise screenshots through `vello_cpu`. It lays out in one
font on every platform (`tests/fonts/DejaVuSans.ttf`), so a width tuned on a
Mac holds on Windows. PDFs are generated in Rust by `fixture.rs` — nothing is
committed. There are 31 integration test files, one per concern, plus unit
tests inside the pure modules (`layout`, `windows`, `emit`, `watch`, `library`,
`settings`, `search`, `store`…).

What it cannot cover is anything that needs a real window or GPU: the shell, the
cascade, full screen, tabs, the socket, the async render thread, and the
texture-lifetime dance in `PageWidget::ensure`.
