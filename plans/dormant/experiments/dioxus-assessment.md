# Dioxus Native (Blitz) — the assessment, and the plan

Whether Moonowl should be rewritten on **Dioxus Native**: Rust all the way
down, HTML and CSS rendered by **Blitz** onto the GPU through **Vello**, with
no webview anywhere. Not Dioxus Desktop, which is a webview and would buy
nothing over Tauri.

`brief.md` is the ask. **`PROGRESS.md` is what building it found**, and where
it and this file disagree, that one was measured and this one was reasoned.
Phases 0, 1 and 2 are built and Phase 3 has started; every number this file
used to carry has moved there, so that there is one place to read them.

---

## The answer

**Yes, it was worth building, and it still is.** Three things make it a real
proposition rather than a fantasy:

1. **The bridge that killed the pdfium prototype does not exist here.** That
   experiment failed because 43MB of bitmap per page had to cross an IPC
   boundary into a web content process. In Blitz the renderer, the page
   bitmaps and the DOM are all in one process and the bitmap goes to the GPU
   as a `wgpu::Texture` — no copy, no boundary. *Measured: 43ms a page became
   4.7ms.*
2. **Blitz is HTML and CSS.** That is the only reason this is a rewrite of the
   runtime and not a rewrite of the product. `styles.css` is 2,129 lines that
   encode the whole look the brief asks for, and Blitz is the only non-webview
   Rust UI stack that can read it. egui, iced and Slint would each mean
   redrawing the app from nothing in a foreign idiom.
3. **Roughly 2,450 lines of the Rust side port unchanged.** `settings.rs`,
   `theme.rs`, `library.rs`, `keys.rs` and `watch.rs` know nothing about Tauri.
   *Measured on all five: every one is mounted by `#[path]` and compiled
   unchanged, with its own tests. Not even the attribute this file expected to
   remove, and not even the emit in `watch.rs` — the two names it imports are
   supplied on the other side rather than edited out of it.*

And three things that keep expectations honest:

1. **Blitz is alpha turning beta.** `dioxus-native 0.8.0-alpha.1`, on a `main`
   at `0.3.0-beta.2`; the published `0.7.10` predates the Custom Widget API and
   is not the version to build against. Its own status page scores 48% on the
   WPT `css` subsuite, and production readiness is "sometime in 2026" by the
   project's account. The API moves: between `0.7.10` and `main`, custom
   painting, the window lifecycle and the event queue all changed shape. That
   cost is paid in the shell and nowhere else.
2. **The binary is twice the size.** 6.2MB today against 12MB plus 7.2MB of
   pdfium. The brief permits this as a price for memory and it is paid: 144MB
   against 373MB on the same document.
3. **The entire test apparatus goes.** Seventeen test files, the Playwright
   WebKit harness, and the browser twin in `api.ts` that makes them possible.
   The replacement is good and is built (`PROGRESS.md`, "The harness"), but it is a
   rewrite, and pretending otherwise is how a rewrite loses its safety net.

---

## What Dioxus Native actually is

```
your #[component] fns            Dioxus VirtualDom
        │  mutations
        ▼
DioxusDocument  ──wraps──▶  blitz-dom  (Stylo for CSS, Taffy for layout,
        │                              Parley for text shaping)
        ▼
   blitz-paint  ──▶  anyrender  ──▶  Vello / Vello Hybrid / Vello CPU
        │                                    │
   blitz-shell (winit, AccessKit, muda)      ▼
                                        wgpu → Metal / DX12 / Vulkan
```

**Stylo** is Servo's style engine, so CSS parsing and cascade are the real
thing. **Taffy** does flexbox and grid. **Parley** shapes and lays out text
with real kerning and subpixel positioning. **Vello** rasterises on the GPU
with compute shaders; `vello_hybrid` splits the work CPU/GPU, and `vello_cpu`
is a pure software fallback. *`vello_hybrid` turned out to be the right
default for memory rather than a fallback for weak hardware — see "The floor"
in `PROGRESS.md`, which is the largest single correction to this document.*

Licences: `dioxus-native` and `blitz` are MIT OR Apache-2.0; `stylo_taffy`
brings MPL-2.0, which this tree already tolerates (the MPL-2.0 crates under
Tauri are copyleft per file and unmodified, and Stylo is the same shape).

Feature flags that matter: `accessibility` (AccessKit), `menu` (muda — the same
crate Tauri already uses, so the macOS menu bar survives), `hot-reload`.

---

## Why this is a different question from the pdfium prototype

`AGENTS.md` records that experiment and its conclusion was **no**, on one axis:
the transport. The 47ms it measured was 3.6ms of drawing and ~43ms of getting
the pixels into a canvas in another process, and the three ways out were costed
and all three failed — draw small and refine (pays twice for every page
actually read), keep the bitmap on the native side (a different application),
or wait for shared memory (not coming; Tauri's IPC is message-passing by
design).

**"Keep the bitmap on the native side" is exactly what Dioxus Native is**, and
the objection recorded against it was that the text layer, selection,
find-in-page, links, outline and thumbnails would all have to follow the pixels
into native drawing and stop being DOM. In Blitz they *are* DOM — Blitz's DOM
just happens to be composited in the same process as the pixels. The objection
was to a native *layer under a webview*. It does not apply to a native *engine
under the whole app*.

| | today (Tauri + pdf.js) | Dioxus Native + pdfium |
| --- | --- | --- |
| page draw | 27-82ms in JS | 3.2ms in Rust |
| pixels → screen | canvas, same process | `wgpu::Texture`, same process |
| copies of a page | worker + JS heap + canvas | one CPU buffer, then one texture |
| recolouring | blend chain 1.5-5.6ms, or a pixel walk at 34-70ms | a compute pass, effectively free |

The recolouring line is the sleeper. `themes.ts` is 835 lines of luma ramps,
`colouredRows()` sampling, blend-mode probing, a pixel fallback and a mask
allocator — all of it there because a `<canvas>` gives you composite operations
and nothing else. On the GPU it is one shader over the page texture, with no
readback and no cache invalidation on theme change at all. `keyFor()` loses its
theme component.

---

## The port map

### Rust that survives nearly unchanged (~2,450 lines)

| file | lines | change needed |
| --- | ---: | --- |
| `settings.rs` | 502 | **none. Built — mounted by `#[path]`, tests and all** |
| `theme.rs` | 463 | **none. Built — same, with `build.rs` and the fourteen `themes/*.toml` shared rather than copied** |
| `library.rs` | 601 | **none. Built — mounted by `#[path]`, its eight tests with it; only `touch` and `toggle_mark` are called so far** |
| `keys.rs` | 215 | **none. Built — mounted by `#[path]`, its five tests and its `keys.toml` template with it** |
| `watch.rs` | 668 | **none. Built — mounted by `#[path]`, its fourteen tests with it; what changed is on the other side, `src/emit.rs` supplying the two names it imports** |

The `atomic_write`, the per-file locks, `whole()`, the settle window, the
`Exiting` flag, `library.open` as a list — all of it is about the disk and none
of it is about Tauri. This is the part of the app the seam rule protected, and
it pays off exactly as advertised.

### Rust that is rewritten but keeps its logic (`lib.rs`, 2,431 lines)

33 commands become plain function calls or Dioxus signals — no IPC, no
serialisation, no `async` for the sake of the main thread (though the disk
writes should stay off it). The window management (`hand_over`, `Placements`,
`OpenFiles`, `OpenDocuments`, `spawn_window`, `tidy_after`, the Dock menu) is
rewritten against winit and a shell of our own over `BlitzApplication`. **Built
— see Phase 3 item 9, and it came out smaller than this line expects: the rules
separate from the windows and become a tested module of their own, `Placements`
is not needed at all, and `Pending`/`ready` were a handshake across a bridge
that is not there.**

Doors that need a crate rather than a Tauri plugin:

| what | today | native |
| --- | --- | --- |
| file picker | `tauri-plugin-dialog` | `rfd` |
| open a link / reveal a file | Tauri shell | `webbrowser` (already a dep) + `opener` |
| single instance | `tauri-plugin-single-instance` | **built — a Unix socket, where binding it is the claim and connecting to it carries the document. Windows wants a named pipe and there is no std type for one; `RunEvent::Opened` needs an application bundle before it needs an `NSApplicationDelegate`** |
| clipboard | webview | `arboard` |
| title-bar buttons, Dock menu | `objc2` (already direct) | **the Dock menu is built and is unchanged — it posts the same event ⌘N does, so the thread the app has to spawn for it is not needed here** |
| print | shells out | unchanged |

### TypeScript that is reinvented (~11,900 lines + 2,129 CSS)

| file | lines | fate |
| --- | ---: | --- |
| `viewer.ts` | 3,674 | the layout math, `boxes[]`, `rows()`, the LRU, the binary searches, `measureCrop`, `keyFor` — all ports to Rust nearly line for line. **The layout half is done, and so is the highlight half**: `paintHighlights` is `div`s over the page at the character boxes pdfium reports, which is two lines of CSS. `paintSelection`, `tintLinks`, `restoreImages` and most of `recolor` **delete**, replaced by a shader and real glyph rects. |
| `main.ts` | 3,539 | Dioxus components + signals. The `App` object is a `Store`. |
| `api.ts` | 898 | **deletes.** There is no bridge to be the only door to. |
| `themes.ts` | 835 | the derived shades stay (as `palette.rs`, done); the recolouring half becomes WGSL (done). `parseColor`/`readColor` stay and stay strict (done). |
| `ui.ts` | 813 | menus, switches, modal, notice line → components. The stepper's two load-bearing behaviours (no unit in the field, arriving selects) must be re-earned. |
| `settings.ts` | 801 | the settings window → a second Dioxus window |
| `sidebar.ts` | 699 | contents / marks / thumbnails / results → components — **done** (`sidebar.rs`, 600 with its tests), and the thumbnail LRU does *not* stay: a thumbnail belongs to its row and the mounting window is the cache |
| `keys.ts` | 677 | the action table and `chordsOf` port to Rust against `keyboard-types` — **done** (`keymap.rs`, 640 lines), and `isMac` becomes a parameter rather than a module constant compiled twice |
| `search.ts` | 540 | **done** (`search.rs`). `fold` is a straight translation and needed one thing *removed*, the code-point iteration JavaScript forces. The rest is much smaller than this line expected: pdfium answers per character, so the runs, `starts[]`, `position()` and the text layer all go, and a highlight is a `div` rather than a repainted strip of canvas |
| `icons.ts` | 86 | needs a genuinely different design — see SVG below |
| `styles.css` | 2,129 | mostly survives; see the gap list |

---

## The renderer under it

**pdfium (`pdfium-render`) is the choice, and it is behind one trait.** BSD-3,
already measured on the `pdfium-prototype` branch and now in this crate. It
brings everything pdf.js gives us and some things it does not: **per-character
text boxes** (which is what search, selection and markup quads all need —
pdf.js's text layer exists only because the DOM needed something selectable);
outline, destinations, links, page labels; annotations read **and written**,
including deletion; encrypted documents opened with a password rather than
refused; forms. Cost: 7.2MB on macOS arm64, 15MB universal, against 5.5MB of
pdf.js runtime data that stops shipping.

**hayro is the one to watch.** `hayro` 0.7 (Apache-2.0) is the most
feature-complete pure-Rust PDF renderer, adopted by Typst, and would make the
app one static Rust binary with no C++ blob. It is not ready to be the only
renderer here and the docs say so: performance "has not been a focus at all so
far", and there is **no text extraction support**, no annotation handling and
no encryption in the documented API. Text extraction is the blocker —
`hayro-interpret` emits glyph draw commands into a `Device` you implement, so a
text layer is buildable, but it is a project of its own and it is the single
most correctness-sensitive part of the app.

**Design for this**, and the crate does: `render.rs` names four things,
`pdfium.rs` is the only file that mentions pdfium, nothing else imports it.
That is what makes "swap to hayro in 2027" a decision rather than a rewrite.

**mupdf: still no.** Faster and smaller than both, AGPL. A licensing decision,
not a technical one.

---

## What Blitz cannot do that this app currently does

The standing gap list, checked against `blitz.is/status` and then against the
thing itself. Where Phase 0 or 1 confirmed or changed an entry it says so.

**`position: fixed` and `position: sticky` are not supported.** Neither is
`position: static`, which has a second consequence: *an absolutely positioned
node is always positioned relative to its immediate parent*. `styles.css` uses
`fixed` in four places — toolbar, find bar, notice line, modal backdrop.
*Workaround, and it is an improvement:* the window root is a flex column, so
nothing is over a scrolling body and nothing needs to be. Overlays become
direct children of the root with `position: absolute` and coordinates computed
by hand — which `showPopover` already does. **Confirmed in Phase 0 and used
throughout Phase 1.**

**`overflow: auto` is not supported** (`scroll`, `hidden`, `clip`, `visible`
are). Five uses. Change to `scroll` and use `scrollbar-width` /
`scrollbar-color`, both supported.

**`text-overflow: ellipsis` is not supported** (Parley #304). Eight uses.
`mask-image` *is* supported and the fade looks better than an ellipsis;
**`white-space: nowrap` was itself broken on `0.7.10` and is fixed on `main`**,
which is what made the fade possible at all. Measuring and truncating in Rust
stays the fallback for a place that really wants a literal "…"; nothing in the
chrome needs it.

**SVG styling is not supported.** Static SVG renders; CSS applied to it does
not, so `stroke: currentColor` paints nothing and every icon renders in
whatever the default is. *Workaround:* generate each icon as a complete SVG
with the colour baked into presentation attributes, memoised per theme — a
`HashMap<(Icon, Color), String>` and a regeneration on theme change. **And the
`svg` feature has to be on**, which it is not in a `default-features = false`
build; the two failures look identical.

**`mix-blend-mode` is not supported.** Already forbidden by `AGENTS.md`, and
the recolouring moves to a shader anyway. No cost.

**`text-shadow` is not supported; `filter` and `backdrop-filter` are partial.**
One `backdrop-filter: blur(2px)` on the modal backdrop. Drop it or accept a
plain scrim.

**Clipboard events do not exist.** Copying selected text is a keybinding into
`arboard`, which is a native app's answer anyway and is *better* than the
DOM's: we have the real text, not what happens to be in a text layer.
**Built in Phase 3 item 10, and it came out even cheaper than this predicted**:
`arboard` is already under `blitz-shell` behind its `clipboard` feature and
reachable through the `ShellProvider` every window is handed, so it is a
feature flag rather than a dependency — 96 bytes on the release binary. What
this entry did not foresee is that ⌘C has to become an *action*, because in the
app it is not a key at all: the webview owns the selection and therefore owns
copying it.

**And nothing selects text for you.** Blitz has a selection of its own for the
text it lays out, and a page is a custom widget with no text nodes in it, so a
reader's sweep over a page is the reader's own from the pointer up. That is
`select.rs` and it is item 10; what it cannot do is what a browser's selection
does for nothing — the keyboard, a real notion of a word, right-to-left. A
custom widget also never sees a `click` or a `dblclick`, because both are
default actions and Blitz forwards a widget's events to the widget instead of
running them.

**IME does not exist** — no `compositionstart` / `update` / `end`. The find
field and the settings fields cannot take composed input, which means CJK,
Vietnamese and accent composition into search is broken. **This is the gap with
no workaround at this layer and it needs a fix upstream. It is a real
regression for a class of readers** and the decision below is whether to accept
it or wait.

**Struck, and it was the only item on either risk list that a decision had to
be made about.** It was true when this was written and is not true of
`c6dec888`: `packages/blitz-dom/src/events/ime.rs` applies a composition to the
focused element's editor through Parley and `blitz-shell` routes all four of
winit's `WindowEvent::Ime` variants into it, cursor area included. What never
arrives is a DOM `CompositionEvent` — and that turns out to be the right shape
rather than a shortfall, because a composition event is a *notification* that
composing is under way and what a find bar wants is the result. `tests/ime.rs`
types 日本語 into the field and finds a composed word in a document; nothing in
the reader had to change. See `PROGRESS.md`'s upstream list.

**`ResizeObserver`, `IntersectionObserver` and the `resize` event do not
exist.** Replace with the window size from winit and element geometry from
`onmounted` + `get_client_rect()`, recomputed on the events that actually
change things. **Phase 1 found `get_client_rect` unusable as well** — see
"`MountedData` panics" in `PROGRESS.md` — so the viewport is the window's size
minus a stated chrome height.

**`change` and `select` events do not exist; `input` does.** Fine — the app's
fields are already driven on `input` with explicit commit semantics.

**`<input type="password">` is not supported.** The encrypted-document prompt
uses one. Masking by hand over a `type="text"` field, done well (the caret,
selection, paste), is fiddlier than it sounds. Or ask in a native dialog.

**`<select>`, `<dialog>`, `<progress>`, `<meter>` are not supported.** The app
uses none of them — every menu and the modal are hand-built already. This is
the payoff for the "no clunky UI" rule having been taken seriously.

**Drag and drop events do not exist.** Nothing uses them today, and winit's own
`DroppedFile` event covers dropping a PDF on the window, which is the case that
matters.

### Things that work and were worth checking

A custom widget drawing into a `wgpu::Texture` ✅ (`blitz_dom::Widget` on an
`<object data=…>`). `clip-path` ✅. `mask-image`/`mask-composite` ✅.
`opacity`, `box-shadow`, `border-radius`, `z-index`, 2D `transform`,
`filter: blur` ✅. Flexbox and grid ✅. `@font-face` and variable fonts ✅.
Transitions and animations ✅. `wheel` and `scroll` events ✅. `user-select` ✅.
`cursor` keywords ✅. `pointer-events` ✅. `keydown`/`keyup` ✅. `contextmenu`
✅ (the app suppresses the webview's own; here there is none to suppress).
AccessKit is a first-class feature flag.

### Multi-window: supported, but not through `launch()`

The app's whole "Two documents at once" architecture depends on this. It works
— three windows, one asked for from another thread, closing the last ends the
app — and it takes about three hundred lines of shell of our own, because
`DioxusNativeApplication::add_window` does not do what its name suggests. The
mechanics are in `PROGRESS.md`.

**It remains the highest-risk item on the list**, and Phase 3 item 9 has now
built the whole of the app's window story on it — two documents at once, the
cascade, the Dock menu, the quit-versus-close rule, one instance and the
handover — so what follows is what that found rather than what it feared.

It holds on macOS. It is still off the documented path and still written
against public fields rather than a supported API, and **it has been shown to
work on macOS and nowhere else**: single instance is a Unix socket, so Windows
has none, and Apple Events need an application bundle this experiment does not
have. If it does not hold up on all three platforms the rewrite is not worth
doing, because a reader who cannot open two papers side by side has lost the
thing this app most recently gained.

The one thing it cost was a crash, and it is worth knowing because it was not
where it looked. Two windows died on the third frame inside Vello's atlas
upload — and so did *one* window, made after the event loop had started, which
is a path that had existed since Phase 0 and had never been taken. The cause
was a page's texture being registered in the same frame as every other page in
the document was being replaced, which happened on every launch because the
viewer was laid out at a default size and corrected on mount. Sizing it from
the window before the first frame removed the collision and a wasted round of
renders with it. Multi-window found the bug; it did not cause it.

---

## What gets better

Not a consolation list — several of these the current architecture cannot have
at all.

**Selection stops being a pixel trick.** `paintSelection` copies pixels off the
page canvas, runs them through a luminance ramp and lays the copy back over the
line, because giving `::selection` a colour would put pdf.js's text layer on
screen and a page's bold type would come back as regular. With pdfium's
character boxes there is no text layer and no substitute font: draw the
selection rectangles under the page and tint the glyphs in the shader. ~200
lines of careful, load-bearing code deletes.

**Recolouring becomes a shader.** `colouredRows()`, `canBlend()`,
`recolorByPixel` and its mask allocator, the `WHITE_POINT` clamp done twice
because two paths must agree to within one level in 255 — all of it exists
because a canvas offers composite operations and a pixel array and nothing
between. **Done, and it is one WGSL function held to the same tolerance.**

**Markup stops being clunky, which is the thing the brief flags.** The current
implementation is shaped entirely around one pdf.js limitation: `saveDocument()`
can create five annotation types and `Annotation.save()` is not overridden by
any markup subtype, so **an annotation already in the file cannot be edited or
deleted at all**. Everything downstream — the journal, `annotation_id: null`,
rebuild-from-backup for removal, byte-truncation undo — is scaffolding around
that hole. pdfium can create, edit and delete annotations, and can write
underline, strike-out and squiggly as well as highlight. The journal shrinks
back to what it should be: a cache and a recovery log for documents that were
rebuilt underneath the reader.

**Startup and idle cost.** No webview process, no worker spin-up (which is most
of pdf.js's two-orders-slower document open), no `WKWebView` floor.

**Encrypted documents open.** pdf.js gets a password prompt and nothing else;
pdfium decrypts. The `password.test.mjs` path — ask, refuse, give up — stays,
and the third branch stops being "give up".

**One language.** `settings.test.mjs` exists solely because the settings table
is written three times. **Two of those copies are already gone.**

---

## What gets worse, stated plainly

**Binary size.** 6.2MB against 12MB + 7.2MB of pdfium — twice, which the
brief's goal 2 permits only as a price for memory. It is paid: 144MB against
373MB.

**The test apparatus.** `npm test` starts a dev server, generates fixtures and
runs 17 files against a real WebKit. All of it depends on `api.ts` having a
browser twin, and `api.ts` disappears. The replacement is built and is better
in one respect — it can test rendering, which the current one cannot — and it
is several thousand lines of infrastructure to rewrite.

**GPU variance replaces engine variance.** Today's risk is "WebKitGTK might
drop a blend mode", which is why `canBlend()` and `recolorByPixel` exist. The
new risk is drivers: Vello wants compute shaders, `vello_hybrid` and
`vello_cpu` are the fallbacks, and choosing between them per machine is a new
thing to get right. Different problem, same shape, and at least the fallback is
first-party.

**Maturity.** Tauri 2 is production software with a large user base. Blitz is
alpha-to-beta with 48% of the WPT css subsuite. Bugs found here are bugs to be
fixed upstream or worked around, not bugs to be looked up. **Two have been
found already** — a Stylo panic on a mouse click, and a `pdfium-render` feature
called `thread_safe` that serialises nothing — and both were worked around in
a day, which is the shape to expect rather than a reason to stop.

**IME.** Named above — and answered upstream since: composition reaches a
focused field at the revision this tree is pinned to, with a test.

---

## Risks, and what would kill it

| risk | signal | verdict if it fires |
| --- | --- | --- |
| a shell of our own does not work on all three platforms | **passed on macOS; Windows and Linux unrun** | **stop** — multi-document is not negotiable |
| a widget per mounted page is slow or capped | passed; but every widget is painted every frame, so the mounting window is load-bearing | fall back to one viewport-sized texture; if that is also bad, **stop** |
| Vello unusable on common Linux hardware | Phase 1 on a VM and an Intel iGPU — **unrun** | ship `vello_hybrid` by default (**done, for memory**), `vello_cpu` as fallback (**built**); if neither is smooth, **stop** |
| memory win is small | **passed: 144MB against 373MB** | — |
| text quality worse than the webview | screenshots side by side — **unrun** | probably livable; the brief cares about the look |
| Blitz bugs block a UI element | continuous — two found, two worked around | fix upstream or work around |
| ~~IME~~ | **it exists at `c6dec888`; `tests/ime.rs`** | — |

---

## The plan

Branch `dioxus-experiment`. Nothing lands on `main` until the whole thing is at
parity, exactly as the pdfium prototype was handled.

**Phase 0 — the four spikes.** Done. `dioxus-spike/`.

**Phase 1 — a reader that reads.** Done, and the memory gate is met.
`dioxus-reader/`.

**Phase 2 — the harness, before the app grows.** Done, and it paid for itself
on its first run.

**Phase 3 — parity, in the order the app was built.** 6-10 weeks. Roughly the
order of the existing commit history, which is a reasonable order because each
step was chosen to be testable:

1. themes, settings, the settings window, the theme editor — *themes,
   settings and the settings window done; the theme **editor** is what is
   left of this item and of the whole list. The theme **menu** is built and
   is not the same thing: choosing among the fourteen is not editing one.
   Dark mode and following the machine's own light and dark landed after
   Phase 3 — see `PROGRESS.md`: `follow_system_theme` was
   listed here as needing a signal this reader does not get, and the signal
   is `WindowEvent::ThemeChanged`*
2. the keyboard: the action table, chords, `keys.toml`, the Keyboard page —
   *done but for the Keyboard page, which is a settings window; `keys.rs` is
   mounted from the app like `theme.rs`, and `keys.ts` is ported as
   `keymap.rs`*
3. sidebar: contents, thumbnails with their LRU, marks — *done, results tab
   and all; the LRU turned out to be unnecessary, and `library.rs` came across
   for the marks*
4. search: `fold`, the index, match stepping, the find bar and its three
   switches — *done, and it took the results tab of item 3 with it. Half of
   `search.ts` and all of `paintHighlights` turned out to be pdf.js's text
   layer rather than searching*
5. links, destinations, page labels, the go-to field — *done; the history came
   with them, because following a cross-reference and typing a page number are
   the two moves that leave a reader stranded. The page field is a readout that
   becomes a field, which is Blitz's focus rule and not a preference*
6. spreads, trim, rotation, paged mode, presenting — *all done; presenting
   landed with item 9, being the window's rather than the page's. Two things
   the port improved on the app: every rectangle stays in the page's own
   unturned points and one function places it, so a rotation throws no cache
   away; and the sparse `boxes` array paged mode needs is
   `Vec<Option<PageBox>>`, which is the app's most carefully commented trap
   made into a type*
7. the library: position, open documents, marks, the restore list — *done.
   The one write in this crate that had to move off the thread drawing the
   window, because a position changes on every wheel event and every change is
   a whole-file rewrite; a document's own `/Title` came with it, and pdfium
   answers that at open where pdf.js cannot, so the toolbar is never briefly
   wrong. What is not built is the shelf: there is nowhere to show a
   recently-read list in a reader that always has a document open*
8. the watchers: themes directory, the document being recompiled — *done, and
   the file was mounted rather than ported: `watch.rs` is the fifth of the
   app's modules compiled here unchanged, with its fourteen tests. The one
   line this entry predicted it would need turned out to be needed on the
   other side of it — `extern crate self as tauri;` and a hundred lines
   supplying the two names it imports, rather than a change to the file. The
   wire on the reader's side is a `Waker` in a mailbox, so a theme saved in an
   editor reaches the screen with nothing polling a clock; owning the window
   cost nothing here*
9. multi-window: `hand_over`, placements, the Dock menu, `Exiting` — *done,
   with presenting, which item 6 left here. The finding is that most of this
   was never untestable: the app's window code is rules and windows in one
   coat, and the rules — which window a document goes to, what a window going
   means, where the next one lands — need no window to be true. Separated out,
   they are `windows.rs` and fourteen tests, against a list of things
   `AGENTS.md` records as checked by hand. What the port lost is most of
   `spawn_window`: no `Placements` map, because a window is made and placed in
   one turn with nothing shown in between; no `Pending`/`ready` handshake,
   because there is no bridge to shake hands across. What has no equivalent is
   the empty window, and it decides ⌘N: with no start screen, a new window is a
   second one on the document already in front. One instance is a Unix socket
   where binding it is the claim and connecting to it carries the document;
   Apple Events are the one route out of reach, and only because there is no
   application bundle to send them to*
10. selecting text — *done, and it was not on this list, because in the app it
    is the webview's. Markup needs something to mark and this reader had
    nothing: no text layer, no selection. It is `select.rs`, 240 lines against
    `paintSelection` and the text layer it undoes, because pdfium answers per
    character and a selection is therefore two indices rather than a DOM range
    measured against spans that exist only to be selected. It is also the first
    place in the port where the webview was doing something worth having —
    keyboard selection, a real notion of a word, and right-to-left all come
    with a browser and none of them come with this. The fifth upstream fault
    turned up here: a custom widget swallows every default action, so `click`
    and `dblclick` never happen over a page, and a double click has to be
    counted rather than heard about*
11. markup — and this is where it stops being a port, because the annotation
    hole that shaped the current design is gone. Rebuild it as it should have
    been: create, edit and delete real annotations; keep the journal only for
    what a rebuilt document lost.

**And the interface the list does not have was taken next, out of order.**
The eleven items are the order the app was *built* in, which is the order of
its engine — and the engine is not what somebody comparing the two apps sees
first. Menus and a way to open a second document were on no item and therefore
were never going to be reached by working down this list; the theme chip cycled
through fourteen themes one keystroke at a time and there was no ⌘O at all,
and both of those read as "unfinished" rather than as "not on the plan".
Done, 2026-09-01: three toolbar menus, ⌘O and "Open in a new window…", each
menu item's chord read off the keymap, and the thumbnail column following a
sidebar drag. See `PROGRESS.md`, and the
section at the top of that file about why this was missing. **The settings
window and the Keyboard page are the same category and are what is left of
it**, along with markup.

**And the platform work was taken out of order, before item 11.**
`dioxus-fit.md` argued for it and the argument is about sequence rather than
scope: the two structural risks left are both about platforms and both were
un-probed, while what remains of the features is large and well understood, so
finishing markup on macOS and *then* finding out the shell does not hold on
Windows is the worst available order. Done: Blitz is a pinned git revision
rather than a path into a clone, so a fresh checkout builds; `experiment.yml`
runs the whole suite on three runners; IME is struck; and the memory bound now
binds on Linux as well as on a Mac. See `PROGRESS.md`'s upstream list.

**And after Phase 3, the three keys that still answered "not built yet".**
Dark mode, help and print — the three actions that are about something
*outside* the document, which is why they were last rather than because they
are hard. The catch-all arm that carried that sentence is gone with them, so
an action added to the table and not handled is now a compile error. Done,
2026-09-01; see `PROGRESS.md`. The same pass explained the `SIGSEGV` this
tree had recorded as seen once and not understood: `FPDF_CloseDocument` runs
from `Drop`, which is the one call into pdfium not taken behind the lock.

**Phase 4 — the decision.** Same shape as the pdfium write-up: a table of
measurements, a plain verdict, and either a merge or a parked branch with its
reasoning intact. The parked branch is a perfectly good outcome and the pdfium
one has already paid for itself twice.

**Total, honestly: three to five months of sustained work** to reach parity.
Anyone budgeting less has not counted `viewer.ts`.

---

## Two things still to decide

**1. The binary.** The brief says "small binary" and this trades it away —
19MB against 6.2MB, for a memory win of 2.6× and the end of the webview floor.
The brief's goal 2 explicitly allows it, and the measurement is now in rather
than estimated, so this is a decision that can be made on facts. If the answer
is that the binary matters more, the alternative is to stop and spend the same
weeks reducing memory inside the current architecture — where the last such
pass took 2521MB to 327MB, so there may well be more to find.

**2. ~~IME~~**, which stopped being hypothetical with Phase 3 item 4 — the
search field was built and somebody composing CJK could not type into it — and
then stopped being a decision altogether. Blitz has the input path; the field
takes 日本語; there is a test. The cheapest thing on this list turned out to be
reading the pinned revision rather than asking when it was coming.

## Sources

- [Blitz status: CSS](https://blitz.is/status/css), [elements](https://blitz.is/status/elements), [events](https://blitz.is/status/events), [WPT](https://blitz.is/status/wpt)
- [Blitz — About](https://blitz.is/about), [the repository](https://github.com/DioxusLabs/blitz), [roadmap #119](https://github.com/DioxusLabs/blitz/issues/119)
- [dioxus-native](https://docs.rs/dioxus-native) and [blitz-shell](https://docs.rs/blitz-shell) — but `main` is what this builds against; the Custom Widget API is PR [#425](https://github.com/DioxusLabs/blitz/pull/425)
- [Vello](https://github.com/linebender/vello) and [vello_hybrid](https://docs.rs/vello_hybrid)
- [hayro](https://github.com/LaurenzV/hayro) and [hayro-interpret](https://docs.rs/hayro-interpret)
- This tree: `AGENTS.md`, and the `pdfium-prototype` branch's measurements
