# The Dioxus Native experiment: where it stands

> **The experiment ended and the reader took over.** `experiments/dioxus-reader`
> is now the repository root, the crate is `moonowl`, and the Tauri app it was
> measured against is gone. Paths below are as they were while it was being
> built; the reader's files are at `src/`, its tests at `tests/`.

`brief.md` is the ask, `dioxus-assessment.md` is the plan, and this is what
building it found. It is the only status file.

**The experiment is passing its gates and is not blocked on anything upstream.**
Phases 0-3 are complete, plus the interface work that followed them. Eighteen
upstream faults were found; all are worked around here, most with a test that
will fail the day they are fixed. The list is at the end of this file and is the
most reusable thing in it.

**What is not here is the history.** This file used to be 4,400 lines of
session-by-session bug narrative. Every fault it recorded is either fixed (and
in git), or consolidated into the upstream list below. What is kept is what a
change still has to know.

## Running it

```
cd dioxus-reader
cargo run --release                          # what you were reading last
cargo run --release -- ~/paper.pdf           # a document of your own
cargo run --release -- --theme 4             # …in the fifth theme in the list
cargo run --release -- --measure 60          # read it, and say what it cost
cargo run --release -- --quit 5              # open, sit still, report, close
cargo test                                   # about 450 tests, ~90s
cargo test -- --ignored                      # the one that aborts on purpose
```

With no path it opens whatever was open when it was last put down, and the
start screen when there was nothing. `--measure` and `--quit` deliberately do
*not* restore, and open the app's own `tests/fixtures/book.pdf` instead: every
number below was taken on that fixture, and a measuring run that quietly used
whatever had last been read would not be comparable with any of them.

**The keys are the app's own**, because `keys.ts` and `keys.toml` are ported.
Any of them can be rebound in `keys.toml`. Three actions are this experiment's
and are listed separately in `keymap::EXTRA` so the app's table stays exactly
the app's: `t` (next theme), `s` (spreads), and ⌘C — the last because the
selection here is the reader's own rather than a webview's, which is the
clearest thing the port has found that leaving the webview costs.

## The numbers

One machine, one sitting, macOS 15 on Apple silicon, release builds, a
1100×900 window at 2×, `tests/fixtures/book.pdf` (400 pages of plain text).
Every memory figure is **physical footprint**, never RSS — see below.

| | Tauri + pdf.js | Dioxus Native + pdfium |
| --- | ---: | ---: |
| document open, nobody scrolling | 373MB | **144MB** |
| after ~60 screenfuls | 466MB | ~200MB, settling to 140MB |
| one page drawn | 27ms | **3.2ms** |
| …uploaded to the GPU | — | 4.7ms |
| opening a 400-page document | ~2 orders slower (worker start) | 30-150ms |
| pages resident while reading | canvas + proxy + worker copy | 2, at 23MB |
| binary | 6.2MB | 12MB + 7.2MB pdfium |
| the sidebar open, thumbnails and all | measured with it shut | +24MB |
| the whole document indexed for a search | tens of MB | 15MB |
| …and reading it to build that index | mostly worker | **62ms for 400 pages** |

**The Phase 1 gate — under 150MB against 346MB — is met**: a factor of 2.6, for
twice the binary. That is the trade the brief's goal 2 permits.

What is *not* measured: Windows, Linux, and any document but this one. A
scanned volume is the shape most likely to behave differently.

A reading session on a paper, maximised, measured with `vmmap`: 155MB idle,
267MB at 900×700, 313MB maximised, 320-390MB peak during a fast scroll,
settling to 150-190MB a second after it stops. Flat from ten screenfuls to
sixty, which is what says it is a working set rather than a leak. The levers,
if it has to come down, are the 12MP page ceiling and the two-textures-per-page
upload; `device.poll` was measured and ruled out.

### Measure footprint, never RSS

On macOS **a GPU buffer is charged to a process's physical footprint and only
partly to its resident size**, and the two disagree by a factor of three on
this workload. Measured on an empty window with one frame drawn:

| renderer | rss | footprint |
| --- | ---: | ---: |
| `vello` | 95MB | **208MB** |
| `vello_hybrid` | 85MB | **19MB** |

Eleven times, in the number that matters, and invisible in the one that was
being read — Phase 1 measured the two at 3% apart and concluded they were
equivalent. `stats.rs` reports both and summarises on footprint, which costs a
few hundred milliseconds (`vmmap --summary`) and so is read where a session
ends and never in a frame.

The exception is Linux, where `tests/cost.rs` reads `VmRSS` out of
`/proc/self/status` — no separate footprint counter exists, and the one caller
runs the whole reader down the **CPU** path, where there is no device and no
driver and everything the process holds is resident by construction.

### Where the 144MB is

46MB of page textures (two mounted, and it does not grow with the book), 43MB
of swapchain (three IOSurfaces at 2200×1800, winit's and wgpu's rather than
ours), 24MB for the page buffer, 21MB of small allocations across Rust, Stylo,
Parley and pdfium, ~10MB of everything else. **None of the floor belongs to
Blitz**: an ablation, one process per stage, put the process alone at 1.8MB, a
winit window at 15.7MB, and a wgpu device and swapchain at 16.4MB. What cost
190MB was the first frame `vello` draws, and it costs the same whether the
frame is empty or full — seven scene-independent constants in
`vello_encoding`'s `BufferSizes::new` totalling 173MB. **`vello_hybrid` is the
default now**, not as a fallback for hardware without compute but for memory.
`vello` stays behind a cargo feature so the comparison stays runnable.

Mid-scroll is the one number still worth chasing: the peak is themed textures
dropped and not yet retired. The fix, when it is worth making, is what
`viewer.ts` already has — a pool of page-sized textures rather than a new one
per page.

## What is built

The reader opens a document, scrolls it, fits it, zooms it, themes it, and
remembers all of that between runs. Beyond that: the document's own links,
destinations, page labels and a go-to field, with the jump history that
following one needs; the sidebar (contents, marks, thumbnails, search results);
search; margin trimming, rotation and one-page-at-a-time; the library (where
you were, what a document calls itself, what was open last); watchers on the
themes directory and the open document; multiple windows, presenting, and a
start screen; text selection and copying; markup; the Settings window with its
theme editor; the password prompt; dark mode following the machine; help and
print.

**Every one of the app's forty-three keyboard actions answers**, and the
catch-all that used to say "not built yet" is gone — so an action added to
`keymap` and not handled in `app` is a compile error rather than a sentence in
the notice line.

**And one thing is built that the app has no counterpart for**: `sign.rs`. A
reader draws their name once, keeps it, and drops it onto a page as the
specification's own `/Ink` annotation, with a date or a line of type beside it
by the same click. It is not parity and does not pretend to be — `tests/parity.rs`
names it as an exception. It is also not a *cryptographic* signature and the
window says so in its first sentence; see `signing-assessment.md`.

### What is not built

- **No underline, strike-out or squiggly.** pdfium can write all three; a list
  showing a mark this reader cannot make would be a list with a dead row in it.
  The app arrives at the same place from the other side.
- **No area drag for scans**, the one thing `markup-assessment.md` still lists
  as unbuilt on both sides.
- **No text layer, and there is not going to be one** — `select.rs` is what
  that was for.
- **No typed-name-in-a-script-face**: it needs a cursive font in the binary,
  and a signature in somebody else's handwriting is a strange thing to offer.
- **No cryptographic signing.** Not blocked by this codebase; blocked by the
  fact that a signature nobody's software trusts is not a signature.
- **No keyboard link-following** — there is no focus ring to walk and
  `tabindex` is not honoured in Blitz's focus walk.
- **Windows is still one process per launch.** The single-instance socket wants
  a named pipe there and there is no std type for one.
- **`popover-*` against `menu-*`** is a naming difference and stays one. What a
  port owes the app is the same elements, labels, order, behaviour and look,
  not the same class names.

## The rules the port turned up

Ranked by how much a change is likely to need them.

**Work belongs where its data already is.** The seam rule from `AGENTS.md`
holds here with no bridge to enforce it: `render.rs` names seven questions —
draw a page, its size, its text, its outline, its links, its labels, its title —
and `pdfium.rs` is the only file that mentions pdfium. Each question was added
when something asked, because a trait method with no caller is a guess about
what the caller will want. That is what would make `hayro` a swap rather than a
rewrite when it grows text extraction.

**The app's own modules are mounted by `#[path]`, never copied.** `theme.rs`,
`settings.rs`, `keys.rs`, `library.rs` and `watch.rs` are `src-tauri/src/`'s
files, compiled here with nothing removed, and their forty-four tests run with
them. A copy would go stale, and a stale copy of a theme loader is invisible:
the file is right and what is on screen is the copy. Mounting them means the
experiment cannot drift, and the day one grows a Tauri dependency this crate
stops compiling and says which line did it. `watch.rs` needs `AppHandle` and
`Emitter`, which `lib.rs` supplies with `extern crate self as tauri;` and
`emit.rs` — the whole of what the assessment budgeted a rewrite for, and it
happened outside the file. (A *module* called `tauri` does not work: a `use`
path on a bare identifier is looked up in the extern prelude, not among the
crate root's modules.)

**`MountedData` panics rather than failing.** `scroll`, `get_client_rect` and
`set_focus` all take `doc_mut()`, and every place a component can call one from
is already inside a borrow of the document — a DOM event handler inside
`EventDriver`'s borrow, a mounted handler inside `flush_queued_mounted_events`'s.
The result is `RefCell already borrowed` from a stack naming neither. So:

- *The scroll offset is ours.* The viewer holds a number, the wheel moves it,
  the pages are placed against it — which is what `viewer.ts` does in all but
  the last step anyway. What is lost is the scrollbar and the platform's fling,
  and it is the largest single thing given up.
- *The viewport is asked for, not observed.* No `ResizeObserver`, no `resize`
  event. A `Screen` context answers out of the real window in the binary and
  out of two numbers in the harness.
- *Where the content is comes off the press itself.* A press arrives carrying
  both its client coordinates and its coordinates within the page it landed on,
  and the layout knows where that page is — subtract.

**A texture belongs to its node, not to its widget.** A texture must not be
registered and drawn in the same frame (Vello panics from the atlas upload when
something else is unregistered in that frame, which is what happens when every
page is replaced at once), and unregistering from inside `paint` is what that
forbids. So what `keyFor()` carries — the page, the size, the theme, the view,
the `edition` a recompile bumps — is the *component key*: a change is a
different node, a new widget, a new texture, and the old node's resources
released by Blitz between frames, where it is safe. The viewer is also sized
from the window **before the first frame** rather than on mount, so there is
nothing to re-key and no round of renders drawn and thrown away on every launch.

**Every widget in the document is painted every frame, on screen or not.**
`build_custom_widget_scenes` walks all of them, so `mount()` and `OVERSCAN`
from `viewer.ts` are load-bearing rather than free. The same rule is why the
sidebar needs no thumbnail cache: **the thumbnail cache is the mounting
window**. A thumbnail is a widget on a node, the node exists only while its row
is in view, and scrolling away gives the texture back through `Drop`. That is
half of `sidebar.ts` gone — and it is not a saving so much as the only design
available.

**pdfium has process-wide state and no locking.** Its `thread_safe` feature is
two `unsafe impl`s and serialises nothing; two threads inside it abort the
process with `SIGABRT`, no panic and no stack. `pdfium.rs` takes a process-wide
lock — the *library's*, not the document's — in front of every call. **And a
`Drop` is a call site, and it is the one call site that does not appear at the
place it happens**: `FPDF_CloseDocument` runs from `PdfDocument`'s own `Drop`,
on whatever thread the last `Arc` dies on, and what it corrupts is a
process-wide map of stock fonts keyed by `CPDF_Document*` — so the crash lands
in a test that was *opening* a document, caused by a test that was finishing
one. `impl Drop for Document` takes the lock. That rule is worth carrying to
anything wrapping a C library behind a lock.

**A shell of our own is required for a second window**, and five things it
cannot state: resuming is two steps (a view must be in `inner.windows` before
`ResumeReady` is drained, or the first frame never lands); `blitz-shell` needs
its `custom-widget` feature on or a dropped widget's resources are never
unregistered; the navigation and HTML parser providers are private to
`dioxus-native` and have to be restated (`nav.rs`, six lines) or
`dangerous_inner_html` silently does nothing; `use_window_event` is closed to
us, because it consumes an `Rc<WindowEventHandlers>` from a private context;
and macOS places a window wrong by exactly a title bar until
`set_outer_position` is called right after `View::init`.

**Everything a window is asked to do is an event.** Closing a window or putting
one in full screen is reached from a Dioxus event handler, which runs inside a
borrow of the document *and* the shell's borrow of the window map, so taking a
window out of that map from in there cannot be written. Every ask is posted to
the shell proxy and answered on the next turn. It costs a frame nobody can see
and makes every window verb one shape — the Dock menu needed no special case.

**Rules and windows are two things.** `windows.rs` is `OpenDocuments`,
`Placements`, `Exiting` and the deciding half of `hand_over` with every mention
of a window taken out, and it has fourteen tests. `session.rs` is the half that
actually makes windows and cannot be tested, and it is eighty lines. The app's
equivalent is untestable only because it is written against `AppHandle`,
`State<'_, …>` and `WebviewWindow`.

**A key with nothing focused goes to `<html>`**, above anything a component can
put a handler on, and events bubble up. So the root takes focus when it mounts
and one `onkeydown` is the app-level handler `main.ts` has. Two consequences:

- *A plain key typed into a text field is also a shortcut*, because the root
  hears every key in the window. `prefs::typing_is_not_a_shortcut` is the rule,
  called by `TextField` and `ColorField`. Keys with a modifier still pass, so
  ⌘A, ⌘C, ⌘V and ⌘Z mean what they mean in a field.
- *A field that is always present either always asks for the keyboard or holds
  the focus while not asking* — both are a dead keyboard. So the page field is
  a button that becomes an input, and stops existing when it is done, which is
  what the find bar's field already did. Two fields must never both ask: the
  find field asks only while the page field is not up.

**Rectangles stay in the page's own unturned points**, and `Layout::place_on`
is the single place a link, a match, a mark or a caret meets the rotation and
the crop. `unplace_on` is its inverse and is how a press comes back the other
way — inverted rather than searched for, because a search has no answer for a
point in the gap between two words. The consequence is that a selection, a link
and a match survive a zoom, a turn, a trim and a spread with nothing
recomputed, and that no cache is thrown away on a rotation, where the app has
to clear three.

**Two `data-` attributes are the seam for state that has no pixels** — where
the reader is scrolled to, and which page each `.page` node is. Everything else
a test asserts on is text somebody could read off the screen, which is the
better bar.

## The harness

`src/harness.rs`, behind a `harness` feature that `cargo test` turns on and
`cargo build` leaves off. The release binary is 12MB either way.

```rust
use dioxus_reader::harness::{Options, Reader};

let mut reader = Reader::open(&Reader::book());
reader.press("j");                    // and "ArrowDown", "Home", " ", "+"
reader.wheel_screen();
reader.click_nth(".chip", 3);
let state = reader.state();           // page, pages, zoom, theme, notice, scroll, mounted
reader.screenshot().save("/tmp/page.png");
```

No window, no GPU, no compositor, no PDF worker in another process.
`Reader::open` is about 40ms and the whole reader suite is under half a second.

**Most of it is upstream's.** `blitz-test-harness` builds a `DioxusDocument`,
resolves style and layout against a stated viewport, synthesises pointer,
wheel, key and IME events through the real event pipeline, and offers the DOM
inspection the assertions are written against. What this crate added: a reader
to drive, a `state()` that reads the interface the way somebody looking at it
would (deliberately not out of the `Viewer`, because it was the *wiring* that
was broken both times something was), and a `screenshot()`.

Three things had to change in the reader, each an improvement on its own
account: a page can be drawn without a GPU (`Software` in `page.rs`, which is
also the CPU fallback the assessment's risk table asks for); the window is
asked for a number rather than for itself; and the two `data-` attributes.

`Options.settings` and `Options.keys` write a real `settings.toml` and
`keys.toml` through the app's own loaders before the reader opens, so what a
test exercises is the real path. `press_chord("mod+0")` keeps the platform out
of the test. Contexts stand in for everything that would otherwise reach the
machine: `Screen`, `Away` (a browser), `Frame` (the window), `Clip` (the
clipboard). A suite that took the real clipboard would empty the clipboard of
whoever is running `cargo test`.

**The watcher is off in the harness on purpose.** `Watching` has no way to
stop — the sender the notify callback holds keeps the receiver alive — which is
nothing at one per process and is a hundred threads on a `cargo test`. One test
asks for the real thing.

**There are no reference PNGs, and that is a decision.** The rasteriser is
deterministic; the *fonts* are not. So the pixel tests assert measurable
properties — paper is paper and the ground beside it is not, a band that should
hold text is not uniform, a recolouring theme moves the mean of the page below
80 while the light one leaves it above 200. Each is a sentence somebody could
check by looking, which is the right bar for a test that stands in for looking.

**The memory test is a growth bound, not a ceiling.** Ten screenfuls to reach a
steady state, then forty more, and the footprint may not climb by more than
60MB across them — it climbs by zero. It scrolls the thumbnail column too, in
the same test rather than a second one, because the counters are the process's.

**Parity is measured rather than asserted.** `tests/parity/app-inventory.json`
is taken from the running Tauri app in WebKit through `scripts/ui-harness.mjs`,
and `tests/parity.rs` asks the port the same questions: what each toolbar group
holds, what each menu lists in what order and where the rules fall, the
Settings window's pages with their fields and headings, the sidebar's tabs, the
find bar's switches, the start screen, the Information window, the theme editor
(the sentences under the fields, not just the labels), the Keyboard page's
labels and groups, the three things said over a page, and what all twenty-two
of `applyTheme`'s custom properties resolve to. Its first run found eight
divergences nobody had reported.

**What it compared, for a while, was words** — and the start screen is what
that cost. Every line of it matched while the screen stood on the toolbar's
colour instead of the ground, ran at the body's 13.5px instead of its own
14.5, and had a button across the whole 460px column where the app has one
176px wide. A reader looking at the two side by side could see all three and
name none of them; nothing in this file could see any of them. So three
questions were added, and each is the general form of what it caught:

- *What is painted, not what is named.* All twenty-two variables matched while
  a whole window wore the wrong one of them, so `chrome` in the inventory
  carries the app's **resolved** toolbar and ground, and the port answers off
  its own screenshot.
- *What the type does to a box.* The button's width and the shelf row's height
  are padding plus a word and padding plus a line, which is the same argument
  the toolbar's widths already made — it had simply never been asked of the
  one screen a reader meets first.
- *The recolouring, against the app's own arithmetic.* `recolor.rs` called
  itself a faithful port of `recolorByPixel` and `tests/recolor.rs` held the
  shader to it, but the thing it was faithful *to* was in neither comparison:
  both could have been wrong together and the only place it would show is a
  page. `take-recolor.mjs` runs the app's own function in WebKit over 525
  pixels picked to reach every branch and writes what comes out;
  `the_recolouring_is_the_app_s` holds the port to it within one level of 255.
  It passes, and one of its two ramps is the link case — Moonowl Light's copper
  on white — so a cross-reference is now *known* to be the colour the app
  paints it rather than assumed to be.

**The type was the last of it, and the explanation had been wrong.** The
chrome measured about 5% tight, and the sheet carried a flat
`letter-spacing: 0.6px` on `body` to make up the difference — chosen by
averaging the shortfall over strings, and explained by SF's `trak` table,
which parley genuinely does not read. It made the totals right and the
rhythm wrong: a constant added between letters cannot put back what a
narrower letterform took out, so the words came out the right length and
visibly spaced. The reader who reported it could see exactly that and had no
name for it.

The cause was `opsz`. SF is a variable font whose optical-size axis defaults
to 28 — a 28pt headline design — and parley sets no variation coordinates, so
every word in the application was drawn in it; WebKit sets the axis from the
font size. Measured in WebKit over "Contents" at 13.5px: 56.86px at `opsz`
17 against 50.89px at 28, which is 10.5% and the whole of the discrepancy.
Reaching it needed the Stylo pref above. With `"opsz" 17` on `body`, its own
number on the two rules above that size, and no letter-spacing at all, the
nine measured controls land at +0.1 to +1.0px of the app's, eight of them
exactly `ceil` of it. The 30px heading had always matched, which in
hindsight was the clue: 30 is close enough to 28 that the axis barely moved.

**And what is still asserted rather than measured**, because saying so is the
point of the section. Nothing in this file compares a *document*: a page's
glyphs are pdfium's and the app's are pdf.js's, and the two rasterise
differently — the ramp over them is now proven identical and the ink going
into it is not. Nor does anything compare vertical rhythm (margins and gaps
between elements, as against the heights of the elements themselves), any
colour but the two the chrome is mostly made of, or anything that is the
window's rather than the page's.

Two things about driving it: a harness that clicks a *point* rather than a
*selector* cannot reach a button below the foot of a scrollable pane, which is
why the theme editor had never been opened by a test; and the peek handle is
not on screen until somebody reaches for the top edge.

## What is tested

| file | what it holds |
| --- | --- |
| `tests/reader.rs` | the interface: opening, the wheel, ten keys, the mounting window, fit and zoom, keeping your place through a zoom, the toolbar, spreads, a window of another size, the theme list, settings surviving a restart |
| `tests/paint.rs` | the pixels: a page where the layout puts it, ink on it, a recolouring theme reaching page and chrome, the ink surviving the theme |
| `tests/keys.rs` | chords, the table, `keys.toml`, a rebound key, and the dispatch |
| `tests/sidebar.rs` | contents listed and indented, a heading clicked, the column's mounting window, a thumbnail with ink on it, a mark made, named, followed, taken off and remembered |
| `tests/search.rs` | opening and closing the bar, what is typed reaching the scan, stepping and wrapping, a highlight's rectangle, the three switches, one slice not reading a whole book |
| `tests/links.rs` | where a link is, where following it lands, the two ways a document writes a destination, one that points nowhere; the history, the labels, the page field |
| `tests/view.rs` | margins measured off a sample and taken away, the page turned, a link that turns with it |
| `tests/paged.rs` | one page at a time, and every chord in the keymap failing to leave the mode |
| `tests/library.rs` | where you were, kept and put back; what a document calls itself; what was open, and what has been deleted |
| `tests/watch.rs` | a theme edited and deleted, a document recompiled and one that got shorter, a rebuild that renames the paper — and one test with a real watcher behind it |
| `tests/windows.rs` | a second window, closing against quitting, full screen and the way out, presenting |
| `tests/select.rs` | what a sweep covers and reads as, backwards, across two pages, a second click taking a word, the page turned under the pointer, ⌘A, ⌘C, ⌘⇧C, Escape's order, the cap on pages of text kept |
| `tests/markup.rs` | a mark written into the file and read back, removal, the journal, a rebuilt document's quotes looked up again |
| `tests/sign.rs` | a name placed, its shape kept, the signatures a document already carries, a signature field that is not a signature |
| `tests/ime.rs` | a word from a candidate window reaching the field, one found in the document, the empty preedit before a commit |
| `tests/chrome.rs`, `tests/parity.rs` | the bar, the menus, and the port against the app's own inventory |
| `tests/cost.rs` | the memory bound |
| `tests/upstream.rs` | the upstream faults, as the smallest thing that shows each — written to **pass while the bug is there** and fail the day it is fixed |
| `tests/recolor.rs` | the shader against the reference, to one level in 255 |
| `src/*.rs` | unit tests beside the code: the ported layout, the crop, the search fold, the caret, the window rules, the switchboard, the palette |
| `src/{theme,settings,keys,library,watch}.rs` | forty-four, and they are the app's own |

**How a test asks matters.** What is selected is asked by *copying* it, because
that is the only way a reader can find out too. Whether a highlight is visible
is asked by rendering the page and looking at a pixel, because reading the
annotation back is exactly the check that passed on the invisible version.

Two harness traps worth knowing: `cargo test` runs tests as threads of one
process, so anything keyed by pid collides (the fixture writer uses a counter);
and a test writing into `/tmp` on macOS must canonicalise it, because `/var` is
a symlink to `/private/var` and the watcher compares real paths.

## CI

`.github/workflows/experiment.yml`, three platforms. `cargo test` needs no GPU,
no screen and no compositor, so the whole reader — Stylo, Parley, fontique,
Taffy, the layout, the shader's CPU twin, pdfium and all five mounted modules —
runs on Windows and Linux for the price of a runner. Before it, nothing in this
experiment had ever run on either. Four things it needs:

- **pdfium downloaded per platform**, from the same `chromium/8021` release.
  The Windows archive keeps the DLL in `bin/` and its import library in `lib/`,
  so the directory `MOONOWL_PDFIUM` names is not the same on all three.
- **`libfontconfig1-dev` on Linux**, or `yeslogic-fontconfig-sys` panics out of
  a build script before any test runs. Its escape hatch,
  `RUST_FONTCONFIG_DLOPEN=1`, is not one: it changes the crate's API enough
  that `fontique` stops compiling.
- **Node, for one file** — `Reader::book()` is the app's 400-page fixture,
  generated by `make-pdf.mjs`, which has no dependencies. `src/fixture.rs`'s
  own documents are written in Rust precisely so this is the only place the
  suite needs anything but cargo.
- **No `cargo fmt --check`**, deliberately: the keymap is one row per action so
  it can be read against `keys.ts`, and rustfmt explodes it. Clippy runs.

**It is green on all three, and what it took to get there was six things, five
of them in the tests.** The suite had never run to the end on Windows or Linux
and every fault it found was the same fault: the fixture in `tests/parity` was
taken from the running app in WebKit on macOS, where `ui-sans-serif` is SF Pro,
and neither runner has that typeface. Segoe UI sets the same words a couple of
per cent narrower and DejaVu Sans several per cent wider, so the fixed two
pixels of allowance became a proportional one — six per cent of the number
being measured, floor six pixels, still two on macOS. "Show in Finder" is
`file_manager_name()` on both sides. A chord reads `Shift+B` where it reads ⇧B
here, so the notice is read through the function that writes it. Four tests
open the Document menu, which hangs off the *document's name*, and the name is
on the side of the bar that gives way: at the harness's 1100px there is no bar
left to give it, so they open at 1280 — the width the app's own inventory was
taken at — and `sign.rs` borrows that width for the click and hands it back.
The sixth was not a test: see the flex item shrunk past its own padding, below.

What it cannot cover is the window — the shell, the cascade, full screen, the
Dock menu, the socket. `windows.rs` is what makes that a small hole.

## The bundles

`cargo packager`, driven by `[package.metadata.packager]` in the reader's
`Cargo.toml` — the bundler `tauri-bundler` is a fork of, with no app around it.
The `bundle` job in the same workflow builds a `.dmg`, a `.deb`, an AppImage
and an NSIS installer on dispatch, because four release builds of a crate whose
profile is `lto = true, codegen-units = 1` is not something to do on every push.
The fields are `tauri.conf.json`'s fields: identifier, icons, category, the PDF
association, the copyright. Two things are the port's own:

- **pdfium travels with the binary**, and the four formats disagree about
  where: `Contents/Frameworks` in a `.app`, where a signed dylib has to be;
  `/usr/lib/Moonowl` beside `/usr/bin/Moonowl` in a `.deb`; the executable's
  own directory in an `.msi`. `library_dir()` in `pdfium.rs` stats all three,
  after `MOONOWL_PDFIUM` and before the copy vendored beside the spike.
- **No `license-file`.** cargo-packager turns one into a DMG licence agreement
  — a dialogue to accept before the image will mount, which the app's own DMG
  has never shown.

The macOS bundle was mounted, copied out and run: 20MB on disk, and it opens a
document through the dylib in its own `Frameworks`. What is *not* done is
signing and notarisation — the bundler reads the same `APPLE_*` variables
`release.yml` already sets, so that step transplants, but nothing has been
signed yet.

Blitz comes in as a **git dependency pinned to `c6dec888`** in both crates, so
a fresh checkout builds with nothing beside it. It used to be a path dependency
into a clone that is not in this repository, and waiting for a release was
never going to work: `blitz-test-harness` is `publish = false` upstream, so the
harness this whole argument rests on can never come from crates.io.

## Three things to carry forward

1. **Write the test with the feature.** The harness is a quarter-second for ten
   tests. The excuse not to is gone.
2. **The CPU path is real code, not a test fixture.** A widget that draws
   through wgpu needs its `Software` half, or the screenshots quietly stop
   covering it.
3. **Reading with it is the only instrument that finds some things.** A long
   run of faults — a page pinned to the left of a window with the rest
   unreachable, every undrawn page flashing white on a dark theme, the page
   field emptying itself, a toolbar wearing one grey under fourteen themes —
   were each a *correct answer* placed, coloured or timed in a way nobody would
   sit in front of. No test asks that question. Neither does a list of items:
   the list was the order the app was built in, which is the engine, so the
   *interface* was never an item and its absence never showed up as an
   unfinished one.

## Nineteen things worth raising upstream, and none of them is blocking

- **A flex item is shrunk past its own padding.** `.chip.title` is `flex: 0 1
  auto; min-width: 0; padding: 0 8px`, which is the app's `.doc-title` exactly.
  Squeeze the app's bar and WebKit floors that button at sixteen pixels — the
  two paddings, no name, still a button. Blitz takes it to **zero**: on the
  Linux runner, whose `ui-sans-serif` is DejaVu rather than SF Pro, the
  document's name came out `(311, 8, 0, 30)` at the harness's 1100px and the
  Document menu could not be clicked at all. `min-width` in `styles.rs` now
  says the floor out loud, which is a workaround and not the rule: padding is
  not shrinkable in CSS, and a used width below it should not be reachable.
  There is a second half to it, and the floor makes it visible: a flex item
  that overflows its parent's box — `.bar-left` says `min-width: 0`, which is
  the app's own rule — is *painted* where CSS says and **not hit-tested there**.
  So the name comes back at sixteen pixels and still cannot be clicked, which
  is why four test files open a wider bar rather than relying on the floor.

- **`blitz-shell` reports Command as `Modifiers::SUPER` while `keyboard_types`'
  own `meta()` reads `Modifiers::META`.** `winit_modifiers_to_kbt_modifiers` in
  `packages/blitz-shell/src/convert_events.rs` answers winit's `meta_key()`
  with `SUPER`; every application asking `event.modifiers().meta()` — which is
  what the DOM calls that key — gets `false` with ⌘ held. So on macOS no ⌘
  shortcut in any Blitz application works, silently, and the keystroke arrives
  as its bare letter. One line, and either bit would do as long as the accessor
  and the sender agree.

- **A click clears the focus onto `<html>`, with no way for a component to take
  it back.** Either half alone is defensible; together they mean an application
  whose shortcuts live on its own root stops answering them the first time
  anybody clicks anything. The fix has an address: `handle_click` in
  `packages/blitz-dom/src/events/pointer.rs` walks up from the target matching
  on `el.name.local` and clears the focus if the walk reaches the root
  unmatched, **without ever consulting `is_focussable()`** — which is
  `node/element.rs`'s own predicate, already honours `tabindex >= 0`, and is
  already used by `focus_next_node`. One call, in a file that has the answer in
  it. It bit four times here and decided the shape of the page field.

- **A stacking context's hoisted children are hit-tested only inside the union
  of their own boxes**, so an absolutely positioned panel hanging *out* of a
  short context — a menu under a toolbar, the ordinary case — is drawn where
  nothing will hit it. Painting gets it right; only the hit test does not, and
  the failure is silent: the menu is there, it highlights on hover, and
  pressing it does nothing. Worse, an unrelated z-indexed box elsewhere in the
  same context can make it work by enlarging the union, which is how this went
  unnoticed for two phases.

- **Hit-testing does not clip on `overflow: hidden`**, so a node scrolled far
  out of its container is still clickable where its box says it is, over
  whatever is drawn there. Painting again gets it right.

- **Text keeps the colour it was built with when a custom property changes
  above it.** Blitz puts the brush into the parley layout when it builds a run,
  and a change several levels up is not among the damage — so a label alone in
  its own box (a `<button>`, which builds an inline layout of its own) stays in
  the old colour until something else touches it. A `<p>` in the same place is
  fine, which is what makes it easy to miss. In a themed application it means a
  bar that changes colour except for the two or three labels with nothing else
  in them.

- **No `user-select: none` for a button in the user-agent stylesheet**, so a
  press that slides two pixels becomes a text selection and the click is never
  dispatched. Every browser's UA sheet has this rule. Patch prepared:
  `blitz-button-select.patch`. The same investigation found that `user-select`
  **does not reach an element from an ancestor** — the check reads the pressed
  node and its parent and stops — and that `Harness::move_mouse_to` sends no
  buttons, so a drag is not expressible in the shared harness at all.

- **A custom widget swallows every default action**, so `click` and `dblclick`
  never happen over one: `handle_dom_event` forwards the event to the widget
  and returns before the match that generates them. The two it takes away are
  exactly the two a widget cannot generate for itself, because a click is a
  press and a release on the same node rather than a pointerup.

- **`ApplicationHandler::macos_handler` is an opt-in that a wrapper silently
  loses.** AppKit does not deliver the editing keys as keystrokes: it calls
  `doCommandBySelector:`, which winit surfaces through a callback separate from
  `window_event`. An application that delegates to `BlitzApplication` for
  everything else gets working text fields on Linux and Windows and write-only
  ones on macOS, with nothing at either end saying so.

- **A text input focused before its first layout never asks for IME**, and on
  macOS that is the whole of the editing keys. `Node::focus` asks the shell for
  IME when the node has `text_input_data()`, and that is built by
  `create_text_editor` during *layout construction* — so a field focused from a
  mount handler, the ordinary way to focus a field that has just appeared, is
  focused a moment before the data exists, and nothing asks again. Asking again
  on the next layout would fix it. `Shell::keep_ime_in_step` is the way round.

- **winit's `set_ime_allowed` cannot turn IME off.** Its first line is
  `if self.ivars().ime_capabilities.get().is_some() { return; }`, so
  `ImeRequest::Disable` returns before reaching anything and the only state
  after the first enable is enabled. It means "enable when a field has the
  focus" is the only rule any Blitz application can implement, whatever it
  writes for the other half. winit-appkit 0.31.0-beta.2, `src/view.rs`.

- **`type="password"` is accepted, given `Role::PasswordInput`, and rendered in
  the clear.** It is the one input type whose whole meaning is what it does
  *not* draw, and both workarounds are wrong: masking through `value` fights
  `set_text`'s selection collapse, and intercepting the keys cannot reach
  macOS's `doCommandBySelector:`.

- **A stylesheet mutation walks state-only snapshots and unwraps their absent
  attributes.** Stylo answers a `<style>` text change with
  `StylesheetInvalidationSet`, whose walk calls `each_class` on any element
  snapshot it finds; `ServoElementSnapshot::each_class` goes through `get_attr`,
  which is `self.attrs.as_ref().unwrap()`. Blitz takes a state-only snapshot for
  a hover or a press, and that snapshot has `attrs: None`. So *clicking* the
  button that changes the theme panicked in Stylo while pressing a key for the
  same action was fine. Two further conditions must hold, which is why the first
  three minimal reproductions all passed: the changed sheet must contain a class
  selector, and some rule must depend on the state bits. Either side could fix
  it. `stylo 0.20.0`, `blitz-dom 0.3.0-beta.2`.

- **`font-variation-settings` is behind a pref nothing turns on** — and this
  entry used to say the property was simply dropped, which was the symptom.
  `stylo_to_parley.rs` converts it correctly and parley applies it; what
  happens is that `stylo` puts the longhand behind `servo_pref =
  "layout.variable_fonts.enabled"`, which is `false` in
  `stylo_static_prefs/preferences.toml`, and `blitz-dom`'s `Document::new`
  turns five prefs on without turning this one on. So the declaration is
  parsed and thrown away, `'wght' 100` and `'wght' 900` lay out identically,
  and nothing anywhere says why. `styles::use_variable_fonts` is one
  `set_pref!` and it goes when Blitz sets the pref itself.
  `stylo 0.20.0`, `blitz-dom 0.3.0-beta.2`.

- **Nothing sets the `opsz` axis, so a variable UI font is drawn at its
  default optical size** — which for SF is 28, the design meant for a 28pt
  headline. Read off `fvar` in `/System/Library/Fonts/SFNS.ttf`: `opsz` runs
  17 to 96 and defaults to 28. A browser sets the axis from the font size
  (clamped to 17 at the bottom, which is why `opsz normal` and `opsz 17`
  measure identically in WebKit at 13.5px and at 14.5px); parley sets no
  coordinates at all. The cost is 10.5% of the width of a word — "Contents"
  at 13.5px is 56.86px at `opsz` 17 and 50.89px at 28 — and it is a
  difference of *letterform*, so what a reader sees is narrow letters in
  normal gaps. `font-optical-sizing: auto` is the CSS for this and does not
  exist here; the sheet names `opsz` per size instead. **This is the entry
  above's whole consequence**, and the two were filed separately for a while
  because the pref made the first look like a dead end.

- **`font-optical-sizing` is not read at all**, which is the property that
  would have made the entry above unnecessary. Stylo has the longhand (behind
  the same pref) and `stylo_to_parley.rs` never looks at it, so `auto` — which
  is a browser's default and the whole of what a browser does here — has no
  effect. Hence three explicit `"opsz"` declarations in the sheet, one per
  size band, where one line would do. `blitz-dom 0.3.0-beta.2`.

- **The system font's `trak` table is not read.** True, and much smaller than
  it looked: it was this file's explanation for a chrome that measured tight,
  and the flat `letter-spacing: 0.6px` in `body` was built on it. `opsz` was
  the fault. With the axis set and the letter-spacing gone, every measured
  control in `tests/parity.rs` lands within a pixel of the app's — eight of
  nine exactly on `ceil` of it — so whatever `trak` is worth at these sizes is
  under the pixel Blitz rounds a box to.

- **`anyrender_vello_hybrid` ignores `brush_transform` for a texture.** `fill`
  has an arm for `PaintRef::Resource` that reads the shape's *origin*, draws the
  registered texture there at its own size, and never looks at the brush
  transform. Every other paint honours it. So a texture cannot be scaled by the
  documented means, silently. The way round is to put the scale in the scene
  transform and make the shape the texture's own rectangle.
  `anyrender_vello_hybrid 0.10.0`, `src/scene.rs`.

- **A frame that fails to present is never discarded.** `render` in the same
  crate resets the scene only after a successful present, and returns early
  without resetting when the surface texture cannot be had and when the
  blit-and-present fails. A window whose surface has gone away — a display
  sleeping, a `Timeout` during any hiccup — accumulates one whole frame per
  redraw in a single scene until `vello_common` panics with `` `alpha_idx` too
  large``, which is a `u32` running into `Strip`'s packed fill flag at about two
  billion. The assert names strip generation and the cause is a hundred thousand
  stacked frames. Two lines on the two early returns.

- **`vello`'s `BufferSizes` sized from the scene rather than from paris-30k.**
  The comment in the source already says it should be. A tenth of every one of
  those constants would do for a reader, and it is not a fault only a PDF reader
  has. Worth 173MB — see "Where the 144MB is".

- **Four in `pdfium-render`.** `PdfQuadPoints::from_rect` numbers the corners
  the way the rectangle walks — bottom-left, bottom-right, top-right, top-left —
  where §12.5.6.10 and pdfium's own `RectFromQuadPointsArray` want upper-left,
  upper-right, lower-left, lower-right. A text-markup annotation built with it
  is written, saved, **read back correctly by the same crate**, and drawn by
  nothing: the appearance stream pdfium generates has a `/BBox` of no width, and
  `to_rect` takes the min and max of the four points so it undoes `from_rect`
  exactly. It is the obvious call and its result is invisible from inside the
  library that made it. `PdfBitmap::as_raw_bytes` should be named as the copy it
  is — a function that looks like a view and allocates 24MB is a trap anybody
  writing a reader will fall into. `PdfDocument::save_to_writer` should take the
  flags: `FPDF_INCREMENTAL` and `FPDF_NO_INCREMENTAL` are both in its own
  bindings and the flags word is hard-coded to zero under a `TODO` from 2022,
  and the one thing worth knowing about them decides a feature — an incremental
  save leaves the original bytes untouched, which is what a signature, a syncing
  folder and somebody else's software all care about. And `PdfRenderConfig::new()`
  defaults `FPDF_REVERSE_BYTE_ORDER` to *on* for `image`'s sake, so a bitmap
  asked for as BGRA is not BGRA — invisible on grey pages, which is why it
  survived nine phases and 313 tests until markup put a *known* colour on one
  and `#ff0000` came back `#0000ff`.

**IME used to be an entry here and the only blocking one**, on the strength of
there being no composition events at all. It is struck: Blitz applies a
composition to the focused element's editor through Parley, `blitz-shell` routes
all four of winit's IME variants into it, and `tests/ime.rs` types 日本語 into
the find field. Nothing in this reader had to change. What arrives is not a DOM
`CompositionEvent` and does not need to be — the DOM's composition events are a
*notification*, and what a find bar wants is the result. (A preedit is not a
query here, and that is free: Blitz answers one with a redraw and no `input`
event, where a browser fires `input` mid-composition with `isComposing` set for
the application to check — and `main.ts` does not check it, so the app scans the
whole document for every intermediate guess.)
