# Moonowl, a smooth reading experience

Note: everything down to the horizontal rule describes what the project SHOULD be like. Below it, "Architecture of the built app" describes what it currently is.

## General
Moonowl is a PDF reader written in Rust, drawn by Dioxus Native rather than
by a webview. Cross-plattform, ergonomic, with a calm UI, and efficient: fast with no lags, little memory and CPU consumption, and a small binary.

Importantly, all settings are preserved throughout sessions, and all of them are independent of each other: changing one setting does not change any other setting.

## UI
The UI is clean and sleek. It is close to full-screen by default. In particular, the app should reserve much or most or all of the vertical axis for the document, as there is likely more room to the sides. No clunky or overbearing UI covers the document. However, true full-screen – no UI elements at all – is easily toggleable, and leaving it should be at least as easy and obvious.

Page progression is continuous scrolling by default, and it's a strong default: it can only ever be changed to anything else if the user explicitly opts into it. Not sure changing this should even be possible using shortcuts because continuous scrolling is so much better than the alternatives, and hitting such a keybind by accident would be frustrating.

There is no clutter in the UI. All elements are nice, modern, polished, and look straight out of professional web design. However, they should not have the typical vibe coded look, i.e., small caps (or caps in general), italics, exotic fonts, and a kind of dead, technical, sterile look. On the contrary, the look should be friendly and open; fresh and lively but in a subtle way.

UI elements might include symbols but they are definitely not just symbols, and not just tiny symbols. For each element, a combination of one symbol and one succinct text label would probably be good.

No animations unless the user takes an action. No pop-up windows that get into people's way.

## Theme settings
The app has dark mode that is easy to toggle (via UI or shortcuts) and that has a customizable definition: text, background, accent, and link colors can be any color chosen by the user, but with sensible defaults. It isn't black by default because the contrast would be too high. The text selection color should be customizable in the same way, and harmonize with each individual theme.

The app supports multiple themes, where each theme is a text-background color combination. Some themes are preinstalled, but users can define and name their own themes. Each theme has a name.

I guess, but I'm not certain, that themes are stored in some kind of config files (one per theme). Possible advantage: easily LLM-able if people want to create a theme but don't want to get in the technical weeds. If we do go this way, choose a good config file format, like TOML or whatever Ghostty uses.

## Preinstalled themes
Ignoring some settings, we have:
- Moonowl Light: the default light theme, and the overall default theme. Doesn't change colors at all.
- Moonowl Dark: the default dark theme. Text is white. Background is a dark grey, with maybe a tint of slate blue.
- Glamour: cool and glamorous dark theme inspired by the Charm / Bubble Tea aesthetic.
- Dracula: text is pink, background is dark blue-ish. Some light blue and/or green is sprinkled in. Maybe that's not accurate – check the Dracula themes other apps use, and how that would translate into PDF theming.
- Gruvbox, for the oldies.
- Sepia: background is sepia, text is dark. Use whatever good sepia themes use.
- Bay Brown: warm yellow ink on a deep brown ground, with a coral accent.
- Dark Forest: cozy and warm — glowing yellow ink on a dark green ground, with ember orange and coral for the rest.
- High contrast: background is perfect black, text is white.
- Nord: the arctic, north-bluish palette — dark slate background, frost blue accents.
- Solarized Light and Solarized Dark: Ethan Schoonover's palette, both halves.
- Tokyo Night and Tokyo Night Storm: the two dark variants — Storm a step lighter than Night.
- Rosé Pine: dark, with a muted rose accent against a deep plum background.


---


# Architecture of the built app

Everything above is the brief. What follows describes the app as it actually
stands, so that a change can be made without reading every file first.

## Shape

**One Rust binary.** [Dioxus] Native draws the interface — [Blitz] laying out
real HTML and CSS with Stylo, Parley and Taffy, composited by `vello-hybrid` —
and pages are rasterised by **pdfium** through `pdfium-render`. No webview, no
JavaScript, no framework beyond Dioxus's own signals.

[Dioxus]: https://dioxuslabs.com
[Blitz]: https://github.com/DioxusLabs/blitz

```
src/
  app.rs          the Viewer: state, menus, keyboard, every window   ← the heart
  keymap.rs       every action, its default chords, and event → chord
  layout.rs       where each page sits, and what is on screen
  page.rs         the page widget: pdfium into a texture, or into ImageData
  pdfium.rs       the one pdfium instance, behind the one lock it needs
  gpu.rs          the shader that recolours, selects and draws links
  recolor.rs      the same recolouring on the CPU — the reference, and the
                  half the screenshot tests read
  styles.rs       all of the CSS, as one string
  icons.rs        the hand-drawn icon set
  store.rs        the library: where you were, what was open, what you marked
  markup.rs       highlights written into the document, and taken back out
  search.rs       the full-document index, the fold, and match stepping
  sidebar.rs      contents, marks, thumbnails, search results
  sign.rs         a drawn signature as the specification's own /Ink annotation
  prefs.rs        the Settings window and the theme editor
  shell.rs        the window, the menus and the cascade
  windows.rs      what a second window is, and what closing one means
  single.rs       one process per user, over a Unix socket
  harness.rs      the reader driven with no window and no screen
  emit.rs         news, and the mailbox each window reads it out of
  theme.rs settings.rs keys.rs library.rs watch.rs
                  came over from the Tauri app; see `lib.rs`
vendor/parley     parley 0.11.1 with one line changed — see its README, and
                  `body` in styles.rs for what the line cost on a 2x screen
themes/*.toml     the fourteen packaged themes, embedded with include_str!
keys.toml         the commented template a new install gets, include_str!
icons/            what the bundler puts on the three platforms
build.rs          the shipped theme table, generated from themes/ and checked
tests/            `cargo test`; one test file per thing the reader does
  parity/         what the retired app's interface measured, frozen as the spec
  fixtures/       nothing committed; every PDF is written in Rust by
                  `fixture.rs`, the 400-page book included. No Node anywhere
examples/         `fixture.rs`, which writes one of those PDFs from the
                  command line — the packaging job's smoke document
experiments/      the port's own record: PROGRESS.md, the assessments, and the
                  four Phase 0 spikes, which still compile
```

**This app was a Tauri 2 app with a TypeScript frontend and pdf.js until the
port took over**, and several sections below were written about that one. Their
*rules* hold — the viewer's layout, the marks, the keyboard, the traps — but
where they name `viewer.ts`, `main.ts` or `themes.ts`, the file to open now is
`layout.rs` with `page.rs`, `app.rs`, and `theme.rs` with `styles.rs`.
`experiments/PROGRESS.md` is the port's own account of what moved and what it
found.

## What lives where

**Rust never renders anything.** It hands over bytes, remembers things, and
asks the system to open a link or show a file. It also decides when the window
appears: the frontend calls `ready` once it can paint, so a dark theme never
flashes white on the way in.

**A document is read in pieces, never whole.** `open_for_reading` opens the
file and reports its length without reading any of it; `read_range` serves
slices of the document the asking window has open, raw rather than base64'd
through the JSON bridge. `FileRange` in `viewer.ts` is a pdf.js
`PDFDataRangeTransport` over those two, with `disableAutoFetch` and
`disableStream` so nothing is fetched speculatively. Handing pdf.js the whole
file instead meant three copies of every document in memory — the Rust buffer,
the JS array, and the worker's own — and reading all of a scanned volume before
showing any of it.

**Every command that touches the disk is `async`, and that is the only reason
for the keyword.** None of them await anything. A synchronous Tauri command
runs on the thread that received the IPC message, which is the thread drawing
the window: `remember_position` fires on every pause in a scroll, so a
whole-file rewrite of `library.toml` was landing in the middle of the one
gesture this app exists to make smooth. The price of moving them off is that
two can now run at once, which is what the locks in `settings.rs` and
`library.rs` are for, and why `atomic_write` gives every write its own temp
file.

**`api.ts` is the only door.** Nothing else imports `@tauri-apps/api`. It also
carries a browser fallback — settings in `localStorage`, a file input instead of
the native picker, and the packaged themes read out of `src-tauri/themes/*.toml`
at build time — so `npm run dev` can be opened in an ordinary browser while
working on the interface. The themes are read rather than restated because the
restated copy went stale, and a stale copy of a theme is invisible: the file is
right, and what is on screen is the copy.

**Settings are written a group at a time.** Each write still changes only the
entries it names and leaves unknown keys alone; the defaults table in
`settings.rs` doubles as a whitelist. What changed is the batching: `App.set`
records the change in memory and queues it, and `flushSettings` sends whatever
has collected on the next turn of the event loop. Settings almost never move
alone — a theme comes with the light or dark slot it fills, a zoom with its fit
mode — and one call per key meant two whole-file rewrites per change, each
re-reading what the other had just done. `App.setSoon` queues the same way but
waits 400ms, for values that move continuously like zoom during a pinch.
Anything still queued is flushed on the way out, before the window goes.

**Themes are files.** Fourteen built-ins are written into the user's themes
directory on every run so they can be read and copied, and so a change to a
shipped theme reaches a machine that already has the old one; the embedded
copies are authoritative, and a built-in file edited in place is overwritten.
Editing a built-in through the app saves a copy under an id of its own, which
is never touched, and every shipped file carries a banner saying so — silently
reverting someone's edit is a trap however defensible the policy is. A theme
names colours and a `recolor` flag, and nothing else; `selection_area` is
optional and derived from the accent when it is absent, and `selection_text` —
the ink on that area — is optional and derived from `selection_area` when it is
absent. That first key was `selection` until it was not: a theme naming
`selection` beside `selection_text` was naming the whole of what selecting does
and then one half of it again, and the editor's own labels had said "Selection
area" and "Selected text" all along. `theme.rs` still *reads* the old spelling
(`#[serde(alias)]`, mirrored in `api.ts` for the browser path) and writes only
the new one, because a theme somebody wrote is a file on their disk that this
app does not own — the same reason a built-in edited in place is left alone
rather than reverted. `applyTheme` derives every shade
the chrome uses — surface, line, three grades of muted text, the positive green
— from those colours, which is why a five-line file is enough.

**And the shipped set is the directory, not a list.** `BUILT_IN` in `theme.rs`
is generated by `build.rs`, which globs `themes/`, and `api.ts` sorts the files
Vite's `import.meta.glob` already hands it. Both sides used to write the set
out by hand, and the copies drifted in the way that is hardest to notice: a
theme missing from the TypeScript list shipped in the binary, appeared in the
real app, and was simply absent under `npm run dev`, with nothing anywhere
saying why. Adding a theme is adding a file.

The one thing a directory cannot say is what order to list them in — the Moonowl
family first, then the rest, is an editorial decision — so each shipped file
carries an `order`: 1, 2, 3, so that the number is the position in the theme
menu and can be read straight off it. Inserting one in the middle means
renumbering the ones below, which is a `sed` over a directory of fourteen
files, and a number used twice is a build failure rather than a theme quietly
outranking another. Gaps would avoid the renumbering and cost the one property
worth having, which is that the file says where the theme actually appears.
It means nothing in a theme of your own: those are listed after the
built-ins, by name, and `ThemeFile` does not write it, so a copy made through
the editor does not inherit one.

`build.rs` also *checks* what it globs, and that is the half worth keeping.
A shipped theme that will not parse is dropped by `load_all` in silence, and
one naming a colour the renderer cannot read reaches the reader as the notice
`unreadableColors` raises — which is the right answer for a theme somebody
wrote themselves and the wrong one for a theme we shipped, because it means
finding out from a bug report. Both are build failures now, named by file and
field. `tests/seams.test.mjs` covers the same ground for the browser path,
which cannot wait for a cargo build: `orderOf` in `api.ts` falls back rather
than throwing, deliberately, so that a half-written theme sorts to the end
instead of taking the list apart.

**Two of the files the app reads can change without the app changing them, and
Rust says when they do.** A theme is TOML so that somebody can open it in an
editor, and a document is often a paper being recompiled underneath the reader;
`watch.rs` follows the themes directory always and each window's open document
while there is one, and emits `themes-changed` to everybody (with the whole set
— fourteen themes of five colours is cheaper to send than to ask for) or
`document-changed` to the one window it concerns (with the path). The frontend
reapplies the theme in use without remembering it, or reopens the document and
puts the reader back where they were. This is the
shape of work that belongs on the Rust side: it lives on the disk, and what
crosses the bridge is a filename.

## The viewer

`viewer.ts` earns its size. Six things are worth knowing before changing it.

*The side margin belongs to the modes that have something to frame.* `PAD_X`
is what sets a page off from the window when it is narrower than one, and fit
width is the mode whose whole point is that it is not — so fit width computes
against the full `clientWidth` and the content ends up exactly as wide as the
viewport. Charging it for the margin left forty pixels of ground either side
of a page that had supposedly reached both edges. `PAD_Y` is not conditional,
because there is always something above a page.

*The layout is rows, and most documents are rows of one.* `rows()` groups the
pages: one each, or two side by side, or two with the first page alone — which
is how a book falls open. Everything downstream works in rows, so single pages
are the same code rather than a special case. The gap between two pages of a
spread comes off the room *before* the scale is worked out, because it is a
distance on the screen and not part of the paper; scaled along with the page it
left the pair off centre.

*Landing on a page means landing on the space above it.* `scrollTo` with an
offset of zero backs off by the empty space directly above the page — the gap
from the row before, or `PAD_Y` at the start of the document, and they are not
the same number. That distance is recorded on the box at layout time. It used
to be read back off the page before, which is right until two pages stand side
by side and the box before this one is its neighbour, sharing its top exactly.

*The first page is measured; the rest are estimated and then corrected.* Page
one's size stands in for every page, the layout is built from it, and the app
paints. `measureRest` then walks the document in batches and lays out again
whenever a real size differs. Most documents are one size throughout, so the
correction is usually a no-op — and measuring all of them first meant a blank
window until the last of two thousand page proxies came back. `boxes[]` holds
the position and scale of every page, in order, which is what lets
`firstBoxEndingAfter` and `lastBoxStartingAbove` binary-search it instead of
scanning the whole book on every scroll frame.

*Only the pages near the viewport exist in the DOM.* `mount()` keeps a window of
slots around the viewport (`OVERSCAN`), discards the rest, and queues the rest
for rendering nearest-to-the-middle first. A nine hundred page book costs about
what a two page letter costs.

*Page proxies are an LRU, and dropping one means `cleanup()`.* pdf.js holds a
page's parsed operator list — every decoded image on it — from the first render
until `cleanup()` is called. `pageCache` keeps `PAGE_CACHE` of them, never one
that is mounted, and cleans up what it evicts. Without that, a long illustrated
book grew for as long as it was read and gave none of it back. `discard()` does
the same for the canvas, by resizing it to nothing: dropping the reference
makes it collectable, not collected.

*A rendered page is identified by `keyFor()` — its scale, the screen's density,
its rotation, its crop and its theme.* If the key still matches, the canvas is
reused; change any of them and the page repaints. The density is in there because it is half of how
many pixels the canvas gets, and `watchDensity` re-arms a `matchMedia` query so
a window dragged between screens of different densities actually hears about
it. This is the whole invalidation story.

*The text layer is built once per mounted page and thereafter only rescaled.*
pdf.js positions its spans in percentages and sizes them from
`--total-scale-factor`, which `place()` sets on every layout, so `renderText`
calls `TextLayer.update()` rather than rebuilding. Rebuilding meant re-streaming
the page's text out of the worker on every zoom step — and throwing away
whatever the reader had selected.

*Recolouring is baked into the bitmap, not applied by CSS.* `recolor()` in
`themes.ts` maps the page onto the theme and bakes the result in, so scrolling
afterwards costs nothing — which a CSS filter over every page could not
promise. What it maps is *lightness*: a pixel's luma says where on the ramp
between the theme's ink and its paper it belongs, and a pixel that has a colour
of its own is put there with that colour intact. Hue and saturation are the
page's business and lightness is the theme's. So on a dark theme a plot's blue
curve comes back light blue the same way black type comes back white, and four
series that differed only in hue still do.

It was a flattening until it was not, and the reason is worth keeping: mapping
luma alone is right for everything printed in grey and throws away the whole of
what a figure is for. `duotone()` is that old mapping, still exactly itself,
because two things do want it — a link and a selected word, both of which are
saying *this part of the page is different* in a colour the theme chose, and a
link that keeps the blue it was printed in says nothing. The two are one
mapping where there is no colour: `COLOUR_FLOOR` is where keeping any begins,
so a page of type comes out identical either way, to the level.

Keeping a colour means HSL, and only because of what it guarantees: at a given
lightness there is exactly so much chroma available, so a colour rescaled to
the room at its new lightness lands in the box by construction — no clipping,
and so no hue quietly bending as a channel is clamped. Which lightness it moves
to is still read off luma, because luma is what says a yellow is light and a
blue is dark.

The ramp is straight but for its top: `WHITE_POINT` calls everything
above level 235 paper, because a hairline printed at 90% white is invisible on
paper and, carried across by the same fraction, arrives as a bright cage around
every hyperref box. The blend path reaches it as a `color-dodge` fill and the
pixel path as a clamp in the lookup table, which is why the fallback's luma is
rounded rather than truncated — the dodge multiplies any disagreement between
the two by 255/235. What it costs is the other thing that lives up there: a
code block shaded 4% grey now merges into the background rather than showing as
a slightly paler block. That is the trade the constant makes, and the constant
is the only place to change it. It is also what keeps a scan's warm paper from
surviving as a tint: paper is paper whatever colour it is, so nothing that
light keeps any.

*Only the rows that have a colour on them are walked pixel by pixel.* Blend
modes cannot keep a hue — the chain flattens by construction — so keeping one
means reading the pixels, and at the ten megapixels a page is drawn at that is
50-70ms against 4ms for the chain. So `colouredRows()` reads the page small
first: one `drawImage` down to cells of about twelve pixels, `medium` quality
because `low` samples rather than averages and loses a hairline outright, and
then a row is coloured if any of its cells is. A paper with a figure on it
turns out to have colour on a quarter to a half of its rows and pays for that
much of a page; a page of type has none and goes down the chain entire. The
bands and the gaps between them are rectangles, which is what both paths take —
the chain fills a list instead of the canvas, the pixel path treats a single
rectangle as its own bounding box and skips the mask. Measured on one paper, at
10.4MP: 13ms to read the page small, then 34ms, 42ms and 50ms for pages with a
quarter, a half and rather more of their rows coloured, against 54-68ms for
walking all of it and 4ms for the chain alone.

Two things are painted back on top of that result: pictures, if
"Recolour pictures too" is off (pdf.js reports where images landed via
`recordImages` — on `page.imageCoordinates`, which is where the `complete`
callback writes them; the `RenderTask` getter of the same name reads a field
pdf.js never sets, so it is always null), and links, which are redrawn from the
untouched copy and recoloured towards the link colour. Both need a pristine
copy of the canvas, taken before recolouring, and both put it back under a
*single* clip covering every rectangle at once — one clip and one `drawImage`
per page, not one per picture, which on a page of typeset mathematics is the
difference between a frame and a stall.

That setting is on by default now, and the default is the interesting half. It
was off, for the good reason that flattening a photograph is the one thing
recolouring could do that made a page harder to read — and it never actually
ran, because the coordinates were read off the wrong object. So the behaviour
everyone has seen all along is the one it now names. Turning it off is for
wanting a photograph exactly as printed, and it costs a figure drawn half in
lines and half in pictures the agreement between its halves: the lines take the
theme and the picture keeps its white ground. There is no seam to fix there —
exempting some of a page is what was asked for — which is why the exemption is
not the default and why keeping a picture's colours is done by recolouring it
rather than by leaving it alone.

## What a reading session costs

**Where it stands.** Two changes, measured together in the harness on one
machine in one sitting so the columns are like for like:

| document                        | before | after |
| ------------------------------- | -----: | ----: |
| 400 pages of plain text         |  348MB | 346MB |
| 40 pages of bitonal scan        |  905MB | 351MB |
| 27 pages of photographs         |  440MB | 323MB |
| one page of 12000×16000 bitonal | 2521MB | 327MB |

**Those columns were measured with the sidebar shut.** The thumbnail column is
the third place in this app that holds pages, after the viewer's slots and its
proxy cache, and it was the one with no accounting at all: it drew a thumbnail
for every page scrolled past, kept the proxy and the canvas, and gave neither
back. It has caps of its own now — `THUMB_CACHE`, `cleanup()` after every
draw, and a `RenderTask` per page so a theme change calls off what it is
replacing — but if you are measuring, open the Pages tab and scroll it, because
that is where a fourth leak would hide next.

If and only if you need to know more about app memory, see changes in:
645032673fcc51947c4164a360177a666d6b5fa9

*Only the part of the page being shown is drawn.* With the margins trimmed,
`offsetX`/`offsetY` slide the page under a canvas that is the size of the crop
— so a trimmed document costs less to draw than an untrimmed one, and nothing
is rendered that will be clipped. What cannot follow the canvas is anything
measured in whole pages: pdf.js lays its spans out as percentages of the page,
and a link rectangle is a fraction of one, so the text layer and the link
layer stay a whole page wide and hang out past the box, which `.page` was
already clipping. `placeOverlay` is that, in one place — and it puts the
styles back rather than computing zeros when there is no crop, so an untrimmed
page keeps pdf.js's own device-pixel rounding.

*Where the ink is, is measured over a sample and not per page.* A per-page
crop changes the scale from page to page, and in continuous scrolling that is
a document that breathes as you read it. `measureCrop` draws eight pages small
— first, last, and evenly spaced, because the shapes that vary are the front
matter, the plates and the index — reads them for anything that is not paper,
takes the union, pads it, and refuses to remove more than a third of any one
side. The probe is drawn on white rather than on the theme's paper: this is a
question about the document, and the answer must not move when the reader
changes theme.

## Two documents at once

A window is a whole second reader. The interface is one `App` object inside one
webview, so a second window is a second viewer, a second search index and a
second sidebar, and none of them needed a line of code: they came with the
webview. What did not come with it was on the Rust side, where three things
were global because there had only ever been one of anything.

*The file handle is keyed by window.* `OpenFiles` is a map from window label to
the document that window has open, and `open_for_reading`, `read_range` and
`close_document` all take the asking window. One slot was the reason two
documents could not work at once: the second window's `open_for_reading`
replaced the first window's handle, and every `read_range` after that came back
"That is not the document that is open", in the middle of a scroll. A window
gives its handle back when it is destroyed, which is `tidy_after`.

*So is the document watch.* `watch.rs` follows one document per window and
tells that window, by name, when it has been rewritten — `emit_to`, not `emit`.
The watch itself is on the *directory*, which is shared, so `follow` counts
what wants a directory rather than unwatching it along with the document that
named it: two papers being recompiled in the same folder is the ordinary case,
not a corner.

*And the frontend listens on its window, not on the app.* A plain `listen` from
`@tauri-apps/api/event` registers for **any** target and hears everything,
including an `emit_to` naming somebody else — so `open-document` and
`document-changed` go through `getCurrentWindow().listen`. Get this wrong and
there is nothing to see until two windows are open, at which point every window
opens every document. `tests/seams.test.mjs` greps for it.

**Opening a second document no longer closes the first, which was the whole
complaint.** `hand_over` looks for a window with nothing in it — the one with
the keyboard first — and makes one if there is none, claiming it in
`OpenDocuments` as it goes so that two files double-clicked in the same instant
do not both land in the same empty window. Nothing is ever displaced.

*"Nothing in it" is asked twice, and the second answer is the one to trust.*
`OpenDocuments` is what the frontend says it is showing, reported by a call
nothing waits for; `OpenFiles` is the handle the window is reading through,
kept by the read path itself. A window reading a file is never idle, whatever
the bookkeeping says — because if the bookkeeping were ever missed, a document
would be handed to a window that already has one, and the reader would either
lose their place or watch the app come to the front with no sign of the file
they had just double-clicked. *And if no window can be made at all*, the
document goes into the window that is there rather than nowhere: a file that
vanishes on a double-click reads as a broken app and leaves nobody anything to
do about it. ⌘N is an
empty window; "Open in a new window…" under the document's title is the same
thing with the picker attached, which is the one-step version of what anybody
actually wants.

**What is shared is what belongs to the app.** Settings, themes and the library
are one process's, which is exactly what the single-instance plugin has always
been protecting. The consequence to know is that a setting changed in one
window is not seen by the other until it is opened again — the file is right,
and the copy in the other window's memory is stale. Themes are the exception,
because the watcher already broadcasts them.

**Geometry belongs to the launch window.** There is one remembered size and
place and there are several windows, so somebody has to own it.
`save_window_state` records only from `main`; every other window steps
straight down off the window in front of it — same left and right edges, same
bottom edge, so a step shorter — stepping on while the spot is taken. It
stepped across as well until it did not: off a maximized window every step
was a strip of the new window past the right and bottom of the screen. Letting
whichever window last moved own the setting was tried and it drifts: the number
creeps down and across every session, because what it reads back are windows
that were themselves cascaded off it. Full screen is the one window fact that
is still a setting, so a new window asks the window itself at startup and
adopts the answer without remembering it — otherwise a window made beside a
full-screen one spends its life wearing the chrome of a window it is not.

**A window's position does not survive being shown, on macOS.** Give the
builder a position and the window comes up where it was told to; then `show()`
moves it onto the launch window's frame, so every window lands exactly on top
of every other — which looks precisely like the app still only having one, and
took a while to see for that reason. Setting it again right after `build` does
not help, and neither does setting it just before `show`. `Placements` holds
where each window is meant to go and `place` puts it there immediately after
`show`, in the same turn of the main thread, so nothing is seen in between.

**`library.open` is a list, one path per window, and a launch opens one of
them.** The list is what each window says it is showing, and a window closing
takes its entry out — a document put down does not come back. But restoring
*every* window cascaded them from the top left, so the window in front — the
one the reader would actually look at — came up clipped at the bottom and the
right. So a launch is one maximized window on the document with the latest
`opened_at`: `store::reopening`. The order of the list is the order the windows
were made, which is the wrong end of the question.

*Except that a window going means two things, and `Exiting` is what tells them
apart.* Closed by the reader it is a document they have finished with; closed
because the app is quitting it means only that it was open at the end, which is
the whole of what the next launch puts back. So everything that ends the app
raises that flag before any window goes — `quit_app`, `RunEvent::ExitRequested`,
and `RunEvent::Exit`, which is what ⌘Q from the macOS menu bar arrives as, with
no window events at all — and `tidy_after` forgets nothing while it is up.

*One case no flag can reach: closing the last window, which ends the app on
every platform and is how most people quit it.* Nothing separates "I have
finished with this" from "goodbye" there. So a close never writes an **empty**
list: it can forget any window but the last, and quitting with one document
open still comes back to it. The reader who means the other thing has Close,
which empties the list from the frontend and is the gesture that says so.
Closing three windows one at a time therefore comes back to the third alone,
not to all three, which is what a reader who closed them means and as close to
it as the platforms allow.

The write happens on the main thread, unlike every other write in this app: a
quit closes windows one after another, and two threads racing over
`library.toml` would leave whichever finished last rather than whichever knew
most.

**The restore windows are made from `ready`, not from `setup`.** A window built
during `setup` comes out wrong on macOS: Tauri reports it as visible, and it is
not on screen and not in the accessibility tree, because it was made before the
application had finished launching. Made once the launch window's interface has
reported in, it is an ordinary window — which is what the handover path had
been producing all along.

**The Dock has a "New Window" item, and it is not Tauri's.** Neither Tauri nor
muda reaches the Dock menu, so `dock::install` goes to AppKit the way
`set_titlebar_buttons` does: `NSApplication` has a `dockMenu` property, and
what is in it appears above the standard Options / Show All Windows / Quit.
The only awkward part is that a menu item needs a *target*, and a target is an
Objective-C object with a selector on it — so one class is built at runtime
with `ClassBuilder`, one instance of it is made, and both are left alive for
the life of the process. The method's receiver is `*mut AnyObject` rather than
`&AnyObject`, because `MethodImplementation` cannot be satisfied by a function
whose signature is higher-ranked over a lifetime. The window is made on a
thread of its own, because the item is invoked on the main thread and
`spawn_window` asks the windows where they are — questions the main thread is
the one to answer.

It is also the only place in this app that writes "New Window" in title case.
The Dock menu is the system's furniture and is spelled the system's way; the
app's own menus keep the sentence case they use everywhere else.

**A second window needs the capability to say so.** `capabilities/default.json`
names the windows it applies to, and it named `main` alone. A window outside
that list gets no permissions at all, so every `invoke` from it is denied and
the failure is a webview that never reports in and therefore never becomes
visible. The list is `["main", "reader-*"]`, and new windows are named for the
pattern.

**Two things will waste an hour when checking this by hand.** The first is that
a `cargo build` run straight from `src-tauri/` produces a binary that serves
the *bundled* frontend — `tauri-build` runs `beforeBuildCommand` and embeds
`dist/` — so a change to `src/` is invisible in it until `npm run build` and a
rebuild that actually re-embeds. Use `npm run tauri dev`, which serves from
vite. The second is worse and looks identical: if an installed
`/Applications/Moonowl.app` is running, the single-instance plugin routes the
development binary's launch into *it* and the new one exits with status 0. The
app on screen is then the installed one, and every change appears to have had
no effect. `pgrep -fl Moonowl` is the check.

**None of this can be tested in the harness**, which has no Rust behind it and
no windows. What is testable is on either side of the seam and is tested there:
`library.rs` for the list and its compatibility with the single path it used to
be, `watch.rs` for two windows reading one folder, `seams.test.mjs` for the
window-targeted listens and the keyed handle, and `keys.test.mjs` for ⌘N and
for ⌘W and ⌘Q meaning two different things now. The rest was checked by running
the real app: two documents handed over, three windows restored from a session,
⌘N, a window closed, and "New Window" chosen from the Dock.

## Markup, which is written into the document

A reader who marks a passage gets a `/Subtype /Highlight` in the PDF, with
`/QuadPoints`, `/C` and an appearance stream — the spec's own annotation, and
the same one Preview, Acrobat and Zotero read. It is in the file the next time
it is opened, by this app or any other. `markup-assessment.md` is the long
form: what a highlight is, what pdf.js can and cannot write, and the plan this
was built from.

**pdf.js does the writing, and only for highlights.**
`Viewer.markSelection` builds the entry pdf.js's own annotation editor would
build — `annotationStorage.setValue` under the `pdfjs_internal_editor_` prefix
— and `saveDocument()` returns the whole file back as an incremental update:
original bytes untouched, new objects appended. That shape is read out of
`pdf.mjs` and `pdf.worker.mjs` rather than out of any documented API, which is
the risk it carries; `markup.test.mjs` saves a file and reads a `/Highlight`
back out of it, which is what keeps the risk visible. Two things in that
worker decide most of what this feature is: `saveNewAnnotations` has cases for
`FREETEXT`, `HIGHLIGHT`, `INK`, `STAMP` and `SIGNATURE` alone, so underline,
strike-out and squiggly stay readable and are not writable; and `Annotation.save()`
is not overridden by any markup subtype, so **an annotation already in the file
cannot be edited or deleted through `saveDocument()` at all**. Nothing removes
a highlight. A button that could only take the entry out of the journal, which
the next open would put straight back, is worse than no button.

**The write goes through Rust, because Rust owns the disk and the watch.**
`write_document` takes the same per-window lock the read path does, writes
atomically, leaves nothing beside the document (a `.moonowl-original` backup
was once kept there and read as junk), tells `watch.rs` the burst about to arrive is ours, and
emits `document-changed` to the writing window itself. So a mark reloads the
document through the path a recompile already used, and every cache a reload
rebuilds — `keyFor`, the markup cache, the journal — is invalidated by having
been rebuilt. That is why there is no pending-markup layer and no markup
revision in `keyFor`: saving immediately made the deferred machinery
unnecessary rather than merely delayed.

**The journal is a cache and a recovery log, never an authority.** `Highlight`
in `library.rs` keeps the quads, the colour, the quote and the page beside
`marks`; on open, `syncMarkup` rebuilds it from what `markupOf` reads out of
the file. What survives that rebuild is only what the file cannot carry:
markup on a document that could not be written, and markup a rebuilt document
lost. Both are held with `annotation_id: null`, which is what they have in
common, and the sidebar marks them as not being in the document.

**Step 7 is the edges, and they are most of what makes this trustworthy.**
`readMarkupStanding` asks four questions once per open and says nothing;
`markSelection` is where the answer is finally worth a sentence.

- *Encrypted, unwritable, or very large* (`MARKUP_IN_FILE_LIMIT`, 100MB, because
  `saveDocument()` pulls the whole file into the worker and hands the whole
  file back): the mark is kept in the journal with real quads and its quote,
  and the reader is told once, in one line, what happened and where it is.
- *Read-only* is asked of the disk rather than discovered from a failed
  rename: `document_writability` opens the file for writing and closes it
  again, which is the only question whose answer is actually true — permission
  bits, a read-only volume, another owner and a sandbox all come back the same
  way. It names a syncing folder while it is there.
- *Signed*: asked, not refused. It is their document and an incremental update
  is exactly what a signature detects. Once per document.
- *Syncing*: one sentence about which copy wins, said once, and then the write.
- *A scan*: "there is no text in this document to mark", which is a different
  sentence from "select something first" and the one worth saying.

**A rebuilt document is where the journal earns its keep.** A paper recompiled
by LaTeX is a new file and every annotation in the old one went with it — the
case this app goes out of its way to support everywhere else. The words are
usually still there, so `Viewer.findQuote` looks the quote up again through
`fold` from `search.ts` (the one thing `viewer.ts` borrows from the search:
ligatures split and soft hyphens dropped, because a passage that moved has very
often been re-typeset on the way), starting at the page it used to be on and
working outwards. What it finds goes back in as one write for the lot; what it
does not find is left in the journal and counted out loud, because a passage
that was rewritten is not a passage that moved. The offer is a button in the
Contents panel and never a thing that happens on its own: re-anchoring is a
guess, however good a one, and this app does not write to somebody's file
without being asked.

## The keyboard

Every key the app answers to is an **action** with a name, and a chord is only
ever a way of asking for one. `keys.ts` holds the whole table — the actions,
what each is called on the Keyboard page, and the chords it ships with — and
`main.ts` holds one handler per action and a dispatch of about thirty lines.

*This is not only for the sake of the file.* What decided which shortcut had
been pressed was twenty-five `if` branches whose **order** was load-bearing:
⌘⇧F had to be tested before ⌘F or full screen dropped the reader into the find
bar, ⌘G had to say `!event.altKey` or it took ⌥⌘G's keystroke, and the plain
keys sat under a blanket `if (metaKey || ctrlKey || altKey) return`. Adding a
shortcut meant finding the one place in that order where it did not break
something else. A chord is now computed from the event and looked up, so two
actions cannot both answer to ⌘F — that is one key in one map, and it is a
collision the reader is *told* about rather than a bug that depends on which
branch ran first.

*An event offers several spellings, best first.* `chordsOf` returns what the
key says (`event.key`) and what key was hit (`event.code`), and for anything
that is not a letter it offers the chord with Shift and again without. That is
what makes ⌥⌘G work with no special case — Option turns the G into a © and the
physical key is still `KeyG` — and what lets ⇧Space mean "up a screen" while
Space still means "down a screen". Shift is never dropped for a letter: G and
g are two keys to a reader.

*`mod` is the only thing that changes meaning between platforms* — ⌘ on a Mac,
Ctrl elsewhere — and a literal `ctrl` is normalised to `mod` off the Mac,
because there it *is* Ctrl. That is why Vim's ⌃D and ⌃U do not ship: they would
collide with dark mode on two platforms out of three. Half-screen scrolling is
on `d` and `u`, which are free everywhere, and the template says how to add ⌃D
on a Mac.

*The file replaces, it does not add.* `keys.toml` names an action and the keys
it should answer to; anything it does not name keeps what it shipped with, so a
file that rebinds one key stays one line long and a key added in a later
version still arrives. An empty list unbinds. Nothing in the path throws — a
file somebody wrote by hand is a file this app does not own, so an unreadable
chord, an action that does not exist and a key claimed twice are all *reported*
and the rest of the file still lands. The reports appear on the Keyboard page,
where the keys they are about are, and one line of notice points at it.

*Sequences, which exist for `g g`.* Two chords with a space between them, and
the reason the page field is on `p`: a chord that both does something and
begins something longer is a conflict, and the shorter one keeps the key,
because a key that waits to find out what it means is a key that feels broken.
So a lone `g` on the page field would have made Vim's `gg` unreachable, and `p`
costs nothing — nothing else wanted it. `G` is the other end.

*The Keyboard page is drawn from the keymap*, not from a list of its own, so it
cannot describe a key the app does not answer to and it shows a rebound one.
The hand-written table it replaced had already drifted.

*`keys.toml` is not watched, and that is deliberate.* It lives in the config
directory, which the app writes to itself several times a minute while somebody
is scrolling — `remember_position` alone — so a watch there would be answering
its own writes. There is a Reload button on the Keyboard page instead.

## Things that will bite

**pdf.js runtime data must be given absolute URLs.** `cMapUrl`,
`standardFontDataUrl`, `iccUrl` and `wasmUrl` are handed to the pdf.js *worker*,
where a relative address resolves against the worker script rather than the
page. When they are wrong the worker silently drops what it cannot fetch, and
the failure is oblique: scanned documents lose all their text, because that text
lives in image masks. `asset()` in `viewer.ts` exists for this.

**pdf.js's image defaults are tuned for a browser tab, not for a reader.**
`isOffscreenCanvasSupported` defaults to on and hands the main thread
`ImageBitmap`s, which is the right call when a page is opened once and thrown
away and the wrong one when a document is read for an hour: a bitmap is four
bytes a pixel in the GPU process for as long as its page proxy lives. It is off
here, and "Images cross the worker boundary compressed" above has the numbers.
Anything that reaches for a pdf.js default around images is worth measuring
rather than trusting.

**A theme's colours are hex and nothing else.** `parseColor` takes `#abc`,
`#abcd`, `#aabbcc` and `#aabbccdd`, checked against the alphabet, alpha
dropped; `readColor` is the same function saying `null` instead of guessing.
Everything else — `steelblue`, `rgb()` — is refused, and `unreadableColors`
plus the notice in `useTheme` is how a hand-written theme finds out rather than
silently rendering black on white. Nothing may show a theme's colour without
going through this: a swatch that hands its raw string to CSS shows a colour
the renderer cannot read, which is the picker lying about the page.

**`use_effect`'s closure is replaced on every render, so nothing it captures
survives one.** `use_effect` hands what it is given to `use_callback`, which
"replaces the inner callback with the new callback" every time the hook runs —
so an effect that remembers what it saw last time in a `let mut` captured by
`move` compares against the initial value for ever. Three effects in `app.rs`
did, and the pill's also *wrote* the viewer, which dirtied the component that
had just run it: a render, a style pass, a layout and a full paint per frame
for as long as the window was open. **The app sat at 100% of a core with
nobody touching it**, and there was nothing on screen to say so — the frames
were identical. The same run of `--measure 20` costs 24,083 paints before and
82 after, on the same pages and the same memory. What an effect remembers goes
in a `use_hook` beside it (a `Cell` or a `RefCell`), never in the closure, and
`tests/settle.rs` is the assertion — `stats::RENDERS` stops climbing when
nothing is happening. Nothing else in the suite would have noticed:
`Reader::settle` pumps a fixed three times and every assertion after it passes
either way.

**A press lands on a custom widget; a click never comes out of one.** Blitz
hit-tests the `object` a page or a thumbnail is drawn into and delivers the
press — which is how a sweep over a page begins — and then makes no `click` of
it, so nothing around it hears one. A thumbnail was therefore clickable only
on the number under it, eighteen pixels of a row three hundred tall, and it
looked like a hit-testing bug rather than an event one. `pointer-events: none`
on the widget is the whole fix: the press lands on the box around it and the
click reaches the button. Anything wrapping a widget in something pressable
wants the same line.

**Nothing in this window scrolls the window.** A wheel Blitz cannot spend
chains outward, and the last parent is the viewport, which scrolls whatever
sticks out of the root — so a sideways swipe over the Settings window could
carry the whole interface off to the left, cutting the nav column off at the
edge. `.root` is `overflow: hidden` for that reason and `.window-pane` scrolls
on one axis only. Everything here that scrolls has a box of its own that does
it; the root is not one of them.

**A wgpu device is not a key, and a window is not a process.** Each window's
renderer builds a `WGPUContext` of its own, so each window has its own
instance, adapter and device. `wgpu::Device` compares by its *id*, and an id is
an index into the registry of the instance that made it — so two windows' two
devices are both `Id(0,1)` and compare equal. The recolouring pipelines were
cached on that, so a second window drew its first page through the first
window's pipeline: a texture made on one device, registered in another's
bindings, which wgpu-core reports from inside the next frame as
`TextureView[Id(4,6)] is no longer alive`. The app dies the moment a second
window shows a document. `Recolorer::shared` keys on the `wgpu::Instance`
instead, which compares by the address of the `Global` behind it and is
genuinely one per window. Anything else that caches per device wants the same
key.

**Do not tint the document with `mix-blend-mode`.** WebKit drops the blend
against a composited canvas, and a dropped blend renders as a solid band across
the line. Anything that has to change the colour of ink goes onto the canvas.

**Selected words are repainted from the page, never from the text layer.**
The obvious way to colour selected text is to give `::selection` a `color`, and
it puts pdf.js's text layer on screen — spans that exist to be selected rather
than seen, carrying no weight, no style and a generic family, each stretched
horizontally to the total width the printer used. A page's bold type comes back
as regular, its mathematics comes back as boxes, and every letter shifts as its
line is stretched to fit. So `paintSelection` in `viewer.ts` copies the pixels
under each selected line off the page canvas, runs the copy through the same
luminance ramp that recolours a page — ink to the selection's text colour,
paper to its own — and lays it back over the line. Real glyphs, real weight,
real symbols. Three things about it are load-bearing: which way round the ramp
goes depends on the *page*, not the theme, because a recoloured dark page is
already light ink on dark paper; the rectangles are rounded outwards and
adjacent runs on a line are joined, because pdf.js's spans do not abut and the
gaps otherwise show as white rules through a highlighted sentence; and a run is
kept between repaints when its place, its colours and its density are
unchanged, which is what keeps a drag down a page to about half a millisecond a
frame rather than redrawing all of it. `::selection` keeps a plain background
underneath for the frame before the copies land.

**Canvas blend modes are checked before they are trusted.** The fast path is
built on `saturation`, which is non-separable and not implemented on a canvas
by every engine — WebKitGTK on Linux is the one we have least visibility into,
and a dropped blend mode does not throw, it silently does nothing and the page
comes out as printed under a theme meant to recolour it. `canBlend()` probes
once (an unsupported value is refused and the property keeps what it had) and
`recolorByPixel` is the fallback — which is also the only path that can keep a
colour, so it runs on the coloured rows of every page whatever the engine says.
Flattening the two ways round agree to within one level out of 255, and a page
of type recolours to the same pixels either way; `recolor.test.mjs` is what
says so.

**`putImageData` ignores the clipping path.** It is the one drawing operation
that does. That is why `duotone()` takes an optional list of rectangles as well
as being called inside a clip: the fast path is bounded by the clip, the pixel
fallback has to be told, and `tintLinks` passes both. Get this wrong and
colouring the links repaints the entire page.

**A window fits its content unless it asks not to.** `showWindow` sizes to
what is in it; Settings passes `"full"` for the fixed 860×600 frame it needs
for its nav column. The default used to be the fixed frame, which gave a
one-field password prompt a half-empty window five hundred pixels tall.

**One instance, on every platform.** `RunEvent::Opened` is Apple Events and
fires on macOS alone; everywhere else the system answers "open this PDF" by
launching the app again with the path in `argv`. `tauri-plugin-single-instance`
routes that into `hand_over` — without it, three double-clicked documents meant
three processes writing over each other's `settings.toml`, which no lock inside
one of them can help with. The variant does not merely go unused off Apple
platforms, it does not exist there, so matching on it has to be `#[cfg]`-gated
or Linux and Windows will not compile — which is invisible from a Mac and is
the whole of what CI caught.

**One instance is not one window.** See "Two documents at once" below: three
double-clicked documents are one process and three windows, which is the point
of routing them through `hand_over` rather than through three copies of the
app.

**Declining a password is not an empty password.** pdf.js reads any string
handed to `onPassword` as another attempt, so answering `""` when the reader
presses "Not now" got the question asked straight back, with no way out of it
at all. Giving up means passing an `Error`, which rejects the load. The
rejection travels through the worker and comes out as something else entirely,
so `load` remembers the decline itself and throws `Cancelled` — which `open`
recognises and says nothing about, because there is nothing to report.

**Document links are not anchors.** They carry `role="link"` and no `href`. An
anchor that carries the address navigates on a middle click, which never
reaches the `click` handler — so the webview left the app, taking the open
document with it. Both `click` and `auxclick` go through `onExternalLink`,
which is the only thing allowed to decide what opening a link means.

**The top of the window is the app's, not the system's.** `titleBarStyle:
Overlay` runs the document up under the title bar, so on macOS there is no
native strip to drag the window by: `#toolbar` carries
`data-tauri-drag-region="deep"`, and that needs
`core:window:allow-start-dragging` in the capability, which `core:window:default`
does *not* include. With the toolbar hidden and the window not in full screen,
`.title-drag` stands in for it — inert until the pointer reaches the top eight
pixels, the same reach that brings the peek handle down — and
`set_titlebar_buttons` takes the three traffic lights away to match. All three
hang off `applyChrome()`, which is why `syncFullscreen` calls it.

**`core:window:default` grants almost nothing but getters.** Every window verb
the frontend uses has to be named in the capability one at a time, and the
failure is a line in the console rather than anything visible: without
`allow-destroy` the window will not close, because `onCloseRequested` in the JS
API destroys the window itself unless the handler prevents the default.

**A full-screen change costs the page its keyboard.** The webview stops being
the window's first responder, so every shortcut dies until something is
clicked, and `el.viewer.focus()` is not enough to get it back — only
`setFocus()` on the window is. `reclaimKeyboard()` does both, and it runs from
`syncFullscreen`, once the window has stopped moving, rather than the moment
the switch is thrown: AppKit passes focus around until the animation ends.

**A file is watched through its directory, and a change is a burst.** A
document is replaced by writing another one beside it and renaming it over the
top — which is what `atomic_write` does and what compilers do — and a watch on
a file follows the file rather than the name, so it would go on watching
something nobody can see. Hence the watch on the parent directory, filtered by
name. For the same reason nothing acts on a single event: a save is three or
four, an atomic write is a create and a rename, and a LaTeX run is hundreds
over several seconds, so events are collected until the disk has been quiet for
`SETTLE`. And because the app writes into the themes directory itself, a theme
reload is decided by loading the themes and comparing them against the set the
frontend already has, never by the fact that something moved. Lastly, `follow`
returns without touching anything when it is handed the document it is already
following, which is what every reload does on its way back through
`open_for_reading`: remaking the watch would lose whatever landed in the gap,
and retaking the baseline from the disk would swallow a draft that arrived
during the reload — the next burst would find the file matching its own mark
and say nothing.

**A document is not believed until it ends the way a PDF ends.** A compiler
writes its output across the whole of a run, and reopening what is on the disk
in the middle of one would take the reader's document away and leave them on
the start screen. `whole()` reads five bytes at the front and a kilobyte at the
back, wants `%PDF-` and `%%EOF`, and then wants the size to be the same a
moment later. It does not prove the document is readable; it rules out the case
that actually happens.

**The webview's own context menu is suppressed** everywhere except editable
fields and live text selections, because it offers to reload the app (which
closes the document) and to open the inspector.

**`library.toml` has plain keys and tables, and TOML puts the plain keys
first.** `Library.open` is serialised before `file`, and `Entry.marks` after
every other field of an entry, for the same reason in both directions: a plain
key written after an array of tables lands *inside* the last table of it and
comes back empty. Two tests say so, because nothing else would.

**A page's number is not its position in the file.** `getPageLabels` is what a
book's front matter is numbered by, and the toolbar, the pill, the thumbnails
and the go-to field all speak in labels where a document has them. The library
still records positions — a label is a name, and names repeat.

**Escape and menus.** A popover registers its own capturing key handler, and so
does the modal window. The app-level handler stands down entirely while a
window is up, and while a *menu* is open it stands down for one action —
`dismiss`, whatever key that is on — because Escape is the menu's way out and
an Escape the webview leaves unhandled reaches AppKit, which drops the window
out of full screen behind our back. Clicking the button that opened a menu closes it — `showPopover` tracks
its anchor for exactly that. That handler captures at the document, so it sees
every key before the thing that was typed into does: arrows, Home and End are
handed back when the target is a field, or the caret in a stepper cannot move
without walking the menu instead. Escape and Tab stay the menu's.

**A stepper is a field, and the number in it can be typed.** `ui.stepper`'s
readout is an `<input>`, because stepping alone is fine for nudging and
hopeless for arriving: 150% is six presses from 25%, and a page gap of 30px is
not on the ladder at all. A typed value is clamped to the range and never
snapped to the step — the step is how far one press moves, not a list of the
answers allowed. Two things about the field are load-bearing. *The unit is not
in it*: it was, written in by a `format` callback and read back out with a
regular expression, and that made the field a trap, because clicking "16 px"
puts the caret wherever the pointer landed and typing 30 gives "3016 px" and a
setting at its maximum. Everything about that is right — a click does place a
caret — so the fix is that the field holds nothing but the number and the unit
sits after it, unselectable. And *a click into a field selects what is in
it*, so typing replaces it (`app::select_on_arrival`, asked when the button
comes up, since a press that slides a pixel is a drag); a second click puts the
caret where it landed, and a field reached by Tab has it after the number
(`app::caret_on_arrival`). A stepper never asks for the keyboard
(`data-keyboard`), because the innermost element asking wins every event —
each one asking meant the last stepper on a page held the focus from the
moment Settings opened and took it back from any other field clicked into.

**The find bar is not a popover and dismisses itself by hand.** It holds the
keyboard and a query, so it cannot live in `#popovers`; `App.FIND_KEEPS_OPEN`
is the list of places the pointer may go without putting it away — itself, the
top strip, and the two layers that only ever open from up there. Everything
below the toolbar closes it, which is what the menus beside it do.

**Two of the three search switches change what is found, and one does not.**
"Match case" is a parameter to `fold`, and "Whole words" a boundary test done
against the folded text — so a word hyphenated across a line, whose soft hyphen
the fold has already taken out, is one whole word by the time it is tested.
"Highlight all" changes only how much of the result is painted, and so lives on
the viewer. All three are settings and outlive both the bar and the session. A
page already extracted is refolded rather than re-extracted when the case
setting moves: the trip into the worker is the expensive half.

**Option is not a letter on a Mac.** ⌥⌘G — go to a page, which is what Preview
binds it to — arrives as a ©, and `event.key` has nothing left to compare. That
used to be one branch matching on `event.code` by hand, with ⌘G guarded by
`!event.altKey` beneath it or it took the keystroke first. Now every event
offers *both* spellings, `event.key` first and `event.code` second, and a chord
matches if either does — so ⌥⌘G and ⌘G are two different chords and neither
needs to know the other exists. See "The keyboard" above.

## Testing the interface without taking the screen

**`cargo test` is the first thing to run and the first thing to add to.** The
whole interface answers to it: `harness.rs` builds the reader's DOM, resolves
style and layout against a stated viewport, delivers real pointer and key
events, and draws pages through `vello_cpu` — no GPU, no window, no compositor,
which is why the suite runs on three platforms for the price of a runner. Ten
tests take about a quarter of a second. The excuse not to write one is gone.

Three things about it are worth knowing before adding to it.

**The CPU path is real code, not a test fixture.** A widget that draws through
wgpu needs its `Software` half kept working, or the screenshot tests quietly
stop covering what the reader actually sees.

**Wait for the condition, not for the clock.** Everything after a keystroke —
recolouring, remounting a neighbourhood of pages, indexing four hundred of them
— takes as long as the machine takes, and a CI runner is not this machine.
`settle()` is the harness's answer; a fixed sleep is a test that passes here and
fails there, in a different place each run.

**Reading with it is still the only instrument that finds some things.** A page
pinned to the left of a window with the rest unreachable, every undrawn page
flashing white on a dark theme, a toolbar wearing one grey under fourteen
themes: each was a *correct* answer, placed or coloured or timed in a way nobody
would sit in front of. No test asks that question.

What the suite cannot cover is the window itself — the shell, the cascade, full
screen, the Dock menu, the socket. `windows.rs` states those rules in one place
precisely so that the part which can be tested is.

## The renderer is the replaceable part, and it was tried

pdf.js is the largest thing in the app by some distance: 1.2MB of worker, plus
cmaps, standard fonts and wasm decoders, against a Rust binary that is a
rounding error. It is also where the remaining costs are — a canvas per page in
JavaScript, recolouring done with composite operations because that is what a
canvas offers, and no encryption support beyond a password prompt.

The alternative was **`pdfium-render`**: rasterise pages in Rust and hand the
frontend a bitmap. That is no longer a thought experiment. It was built, run in
the real app, measured against the renderer it would replace, and parked on the
**`pdfium-prototype`** branch — a Rust side in `src-tauri/src/render.rs` and two
viewers over the same layout, `proto/pdfium.ts` through pdfium and
`proto/pdfjs.ts` through the app's own `Viewer`, so the only difference between
a run of one and a run of the other is which renderer answered. The branch
carries its own instructions. Nothing of it is on `main`, because the answer
came back no.

**pdfium draws faster than pdf.js, by between a third and eight times.** Per
page, at the 10.5 megapixels the app actually renders (fit width, retina, the
12M ceiling just above):

| document                | pdf.js | pdfium |
| ----------------------- | -----: | -----: |
| 400 pages of plain text | 27ms   | 3.6ms  |
| a typeset paper         | 33ms   | 21ms   |
| a paper full of figures | 82ms   | 36ms   |
| a scanned manual        | 80ms   | 10ms   |
| slides                  | 66ms   | 52ms   |

Text extraction is three times quicker, opening a document is two orders
quicker (pdf.js is mostly starting its worker), and recolouring in Rust is
13-17ms a page scalar, 2.8-3.5ms across the cores — against 1.5-5.6ms for the
canvas blend chain, which is the one thing pdf.js's side does better by
default.

**And none of that survives the trip into the webview.** A page at that size is
43MB of bitmap, and it has to cross a process boundary that pdf.js never
crosses, because pdf.js draws into a canvas that already lives in the web
content process. Measured end to end, in the app, on the same document and the
same scroll:

| what                          | pdf.js    | pdfium via `invoke` | pdfium via `page://` image |
| ----------------------------- | --------: | ------------------: | -------------------------: |
| per page, end to end          | —         | 47ms                | 82ms                       |
| app process                   | 89MB      | 172MB               | 124MB                      |
| web content process           | 92MB      | 158MB               | 573MB                      |
| **total, after 60 viewports** | **182MB** | **330MB**           | **697MB**                  |

The 3.6ms render becomes 47ms by the time the pixels are in a canvas, and the
memory roughly doubles: the bitmap exists in the Rust buffer, again as an
`ArrayBuffer` on the JavaScript heap, and again inside the canvas. Serving the
same bitmap as an image over a custom URI scheme — which sounds better, and
lets pdfium draw straight into the response with no copy at all — is worse
still, because WebKit's image cache holds every page it is given and the decode
costs more than the copy did. Both numbers are after fixing the obvious faults
(rendering off the main thread, `no-store`, releasing bitmaps on unmount); what
is left is the transport itself.

**So: no, and the three ways out were costed one at a time.** The costs that
would have justified the swap are not where they were assumed to be. What
pdfium is plainly better at is *drawing*, and drawing is not the bottleneck —
the frontier is the bridge, and every version of the swap still has to get 43MB
a page across it.

*Draw at the window's scale and refine.* The bytes crossing are a quarter of
what they are now and the time goes very nearly with them: the transport is
linear in bytes, and the two parts of it that can be timed from inside a web
content process say the same — a 42MB page costs 5ms to `putImageData` and 11ms
to copy once, the same page at a quarter of the size costs 1ms and 1ms. (That
pair is measured in headless WebKit rather than in the app, but it is the same
work, and it accounts for 16 of the 43ms; the rest is the crossing itself.) So a first
pass would land in about 12ms against pdf.js's 27ms on plain text and 80ms on a
scan. What it does not do is finish. A page the reader stops on still has to be
redrawn at the device's scale, so the settled cost is that 12ms *and* the full
47ms, every page that is actually read crosses the bridge one and a quarter
times over, and the memory — the other half of the objection — is unchanged for
exactly the pages that hold it. The reader also sees the seam: a page that
arrives soft and sharpens a moment later is the opposite of what "no animations
unless the user takes an action" is asking for. It buys latency during a fast
scroll through documents pdf.js is already slow on, and charges for it
everywhere else.

*Keep the bitmap on the native side.* This is the only one that wins outright,
because it does not cross the bridge at all — and it is no longer a change of
renderer. A layer under or over the webview owns the pixels, so the text layer,
the selection, find-in-page, the links, the outline and the thumbnails either
follow it into native drawing or stop lining up with the page they are drawn
over. `paintSelection`, `tintLinks`, `restoreImages` and every line of
`renderText` are "the pixels and the DOM are in one process" code, and that is
most of what makes this app pleasant to read in. It is a different application,
and a much larger one than the thing it would be replacing.

*Wait for shared memory.* This one is not a wait. Tauri's IPC is message
passing by deliberate choice, taken over shared memory for the isolation it
buys, and shared memory is not planned; the `SharedArrayBuffer` route that gets
raised whenever this comes up shares between frontend contexts and not between
the frontend and Rust, which the maintainers said in as many words in 2024.
There is nothing coming that this plan could arrive on.

Two more were weighed and are worse. *pdfium as wasm, inside the webview*
attacks the right thing — from wasm memory to a canvas is a copy, not a process
boundary — but the prebuilt that does not fall over on a document longer than a
few pages is the 13MB one, against the 5.5MB of pdf.js it would replace; it is
single-threaded, so the drawing advantage narrows to whatever wasm leaves of
it; and it is a C++ blob either way, so "more Rust" is not what it buys.
*pdfium for the small answers only* — text extraction, the outline, opening an
encrypted document, thumbnails, everywhere the answer is kilobytes rather than
megabytes — is the one shape the bridge does not spoil, and it fails on the
other axis: it ships both renderers to keep pdf.js drawing the pages anyway.

**What it would cost on disk, measured.** `libpdfium.dylib` for macOS arm64 is
7.7MB (3.5MB compressed), the bindings add 0.4MB to the Rust binary, and what
pdf.js would stop shipping is 5.5MB — the 1.2MB worker, 1.6MB of cmaps, 1.5MB
of wasm decoders, 0.8MB of standard fonts and about 0.35MB of the bundle. Call
it +2.6MB on one architecture, and rather more on a universal build, where
pdfium is 15MB. "Roughly a wash" was the guess and it is close to right,
slightly the wrong side of it.

**The decision is to keep pdf.js.** The 47ms measured above is 3.6ms of drawing
and about 43ms of bridge, and the bridge does not care what is on the page — so
adding each document's draw time to it says where the swap would actually land:

| document                | pdf.js | pdfium, end to end |
| ----------------------- | -----: | -----------------: |
| 400 pages of plain text | 27ms   | 47ms (measured)    |
| a typeset paper         | 33ms   | ~64ms              |
| a paper full of figures | 82ms   | ~79ms              |
| a scanned manual        | 80ms   | ~53ms              |
| slides                  | 66ms   | ~95ms              |

Slower on three of the five, and ahead only where pdf.js is spending its time
decoding images. Against what the brief asks for — fast with no lag, little
memory, a small binary — that is 1.8 times the memory and 2.6MB more on the
smallest build, for no reliable gain in speed. Encrypted documents and forms
would come with it and those are real; they are not worth this. Two things
would reopen it: the second option above, taken deliberately as a rewrite of
the viewer rather than as a swap of renderer, or a webview that lets a native
buffer become a canvas without copying it. Neither is close.

**Three things that will bite whoever picks this up again.** pdfium renders
into a buffer of your choosing (`PdfBitmap::from_bytes`), which is how the
`page://` path gets away without a copy — but Skia CHECKs that the buffer is
four-byte aligned, and a failed CHECK is a trap instruction: the process
vanishes with no panic, no message and no stack. The BMP wrapper has to be a
plain `BITMAPINFOHEADER`, because ImageIO, which is what decodes an image
inside a WKWebView, rejects a `BITMAPV4HEADER` outright and reports it as
`EncodingError: Loading error`. And `register_asynchronous_uri_scheme_protocol`
hands you the request on the thread that draws the window: answer it there and
the scroll stops for as long as the page takes.

`mupdf-rs` is faster and smaller but AGPL, which is a licensing decision rather
than a technical one. Nothing measured here bears on it.

**This was a change to the renderer, not to the framework.** Tauri is the right
shell for a UI like this one, and nothing above suggests otherwise. What made
the renderer swappable is that `viewer.ts` is the only file that imports pdf.js
for rendering (`search.ts` and `sidebar.ts` use it only through a
`PDFDocumentProxy` they are handed) and `api.ts` is the only door into Rust.
Keep both of those true and this stays a decision that can be made later — the
prototype needed no change to either.

## The port that took over, and what shipping it turned up

The renderer question above was answered *inside* Tauri. The framework question
was asked separately, in `experiments/dioxus-reader`: the whole reader rewritten
against Dioxus Native, Blitz laying out real HTML and CSS with no webview. It
reached parity, it passes on all three platforms, and **it is now the app** —
this repository has no TypeScript, no webview and no Tauri in it.
`experiments/PROGRESS.md` is the port's own record; what belongs here is the
part that decides how it ships.

- **pdfium is not in the binary, and the formats disagree about where it goes**:
  `Contents/Frameworks` in a `.app` (where a signed dylib has to be),
  `/usr/lib/Moonowl` beside `/usr/bin/Moonowl` in a `.deb`, the executable's own
  directory in an `.msi`. `library_dir()` in `pdfium.rs` stats all three, after
  `MOONOWL_PDFIUM`.
- **`tests/parity/app-inventory.json` is a *macOS* measurement**, taken from the
  retired app in WebKit where `ui-sans-serif` is SF Pro. Segoe UI sets the same
  words a few per cent narrower and DejaVu Sans several per cent wider, so a
  fixed pixel allowance cannot survive the other two runners; off macOS it is
  proportional. Anything compared against that fixture has to think about the
  typeface first. The app it measured is gone, so those numbers are the spec
  now rather than a comparison.
- **Blitz shrinks a flex item past its own padding, and does not hit-test what
  overflows a parent.** Together: the document's name went to 0px where WebKit
  floors `.doc-title` at 16px, and at 16px it is painted but unclickable.
  Nineteenth entry in the port's upstream list.

**A document double-clicked in the Finder is an Apple Event, not an argument**,
and `openfiles.rs` is the whole of that: `'aevt'`/`'odoc'` taken directly off
`NSAppleEventManager`, because AppKit's own handler forwards to a delegate and
winit sets none — `[NSApp delegate]` is nil for the life of the process. Until
it was written, opening a PDF from the Finder gave a start screen and "Moonowl
cannot open files in the PDF document format".

**Printing is the system's on macOS, pdfium's on Windows, and a hand-off on
Linux.** `print.rs` has both halves. On macOS it asks PDFKit for the
document's own `NSPrintOperation` and runs it as a sheet on the reader's
window — the panel, the preview, page ranges and PDF-as-output are all
AppKit's, and nothing leaves the app. It is a sheet and not `runOperation` on
purpose: a modal run loop inside a Dioxus handler re-enters the window it was
borrowing. On Windows there is no panel that takes a file, so it is comdlg32's
`PrintDlg` for the printer and then pdfium's one `HDC` entry point,
`FPDF_RenderPage`, page by page into the DC — the same route Chrome prints
by. That binding only exists behind `pdfium-render`'s `pdfium_use_win32`
feature, which the Windows target block in `Cargo.toml` turns on. The dialog
and the job both block, so the shell runs them on a thread, and every pdfium
call there is taken behind `pdfium::library()` one page at a time so the
reader can keep scrolling while a long document spools. The shell provides
the `Printer` into the window's context on both platforms, which is why
`app.rs` and the tests know nothing about it: the default
`Printer::to_the_system` is what Linux uses and what both halves fall back to
when the system refuses the file. The Windows half has been compiled and
linted through the msvc target from a Mac and not yet run on Windows.

**A window is a window, and a tab is asked for.** macOS turns a new window into
a tab of the one in front while the app is full screen — that is
`allowsAutomaticWindowTabbing`, on by default, against Apple's own default of
*Prefer tabs when opening documents: In Full Screen* — so ⌘N gave a tab and
there was no way to ask for the other thing. `shell.rs` turns it off once, at
startup, and `tabs.rs` is the explicit half that still works with it off:
`addTabbedWindow:ordered:`, which winit has no word for. So "New tab" is an
item under Open… and an action with no key (⌘T is the toolbar's), ⌘1 through ⌘9
choose a tab, and ⌘W closes one — which is also the first time ⌘W has done
anything on a Mac in this port, winit's default menu having an application menu
and no Window menu. The two zoom modes that used to hold ⌘1 and ⌘2 are ⌥⌘1 and
⌥⌘2 there, and unchanged on the platforms with no tabs.

What is still missing, and none of it is about signing: **Windows is one process
per launch** — the single-instance socket wants a named pipe, and there is no
std type for one — and there is no underline, strike-out or squiggly markup,
which the retired app did not have either.

## Running it

```
cargo run                          # the app
cargo run -- FILE                  # …opened on a document
cargo test                         # the whole interface, headlessly
cargo clippy --all-targets -- -D warnings
cargo packager --release           # the installers, into target/release
```

There is deliberately no `cargo fmt --check`: the keymap is one row per action
so that it can be read down, and rustfmt explodes it.

pdfium is a shared library and is not in this repository. `MOONOWL_PDFIUM` names
the directory holding it; without that, `pdfium.rs` looks inside the bundle and
then at the copy vendored with the Phase 0 spike.

Five workflows, and the split is deliberate. `checks.yml` is the suite on
macOS, Linux and Windows, and it is *reusable* rather than a job, because two
things need it — a push and a release — and a second copy would drift.
`bundle.yml` is the installers, reusable for the same reason: packaging is the
part that is easy to get subtly wrong twice. `ci.yml` runs the checks on every
push to main and every pull request. `nightly.yml` replaces the rolling
`nightly` release with a build of every push to main, which is where a person
actually downloads the app from. `release.yml` is the only thing that names a
version, and it runs only when you press the button.

**The assets carry no version in their names** — `Moonowl-macos-arm64.dmg`,
`Moonowl-linux-x86_64.AppImage`, `Moonowl-windows-setup.exe` and so on. The
release's tag says which version it is, and a fixed name is what lets
`bundle.yml` write the download table into the notes from a template, and the
README link to `releases/latest/download/<name>` without anyone editing it
after a release. It is also why the `notes` job exists at all: GitHub collapses
the assets list, so a release whose body is prose looks like it has nothing to
download.

**Two macOS signing traps, and both showed up as the app being broken rather
than as anything about signatures.**

*A bundle whose seal claims resources it does not have.* An unsigned `.app`
whose inner binary carries the linker's own ad-hoc signature is worse than one
with no signature anywhere: `codesign --verify` fails, and Gatekeeper reads that
as tampering — refusing to open it with no *Open Anyway* offered in *Privacy &
Security* at all. `signing-identity = "-"` in `Cargo.toml` seals the whole
bundle and a reader gets the ordinary unknown-developer path.

*A hardened runtime that will not load its own pdfium.* cargo-packager signs
with `--options runtime` unconditionally, and the hardened runtime validates
libraries by Team ID — which an ad-hoc signature does not have. So
`Contents/Frameworks/libpdfium.dylib` was refused, and the reader opened,
showed its start screen and said it could not open any document.
`entitlements.plist` is Apple's own exception for this,
`com.apple.security.cs.disable-library-validation`, and it stays true if there
is ever a real certificate: pdfium is somebody else's build and will never carry
our team.

**The macOS bundle is ad-hoc signed, and that is not a contradiction.** An
unsigned `.app` whose inner binary carries the linker's own ad-hoc signature is
worse than one with no signature anywhere: the seal claims resources the bundle
has not sealed, `codesign --verify` fails, and Gatekeeper treats it as
tampered-with — refusing to open it with no *Open Anyway* in *Privacy &
Security* at all. `signing-identity = "-"` in `Cargo.toml` is what makes
cargo-packager seal the whole bundle, and a reader then gets the ordinary
unknown-developer path. It is not a certificate and it is not notarisation.

**Nothing has ever been signed by a certificate.** The Tauri releases were not, and neither are
these: on macOS the first launch wants *Privacy & Security → Open Anyway* and
Windows SmartScreen wants "More info" → "Run anyway", which is what the README
tells a reader. The `APPLE_*` variables are still promoted into the environment
by a macOS-only step, under other names until they carry something — because a
bundler goes by whether `APPLE_CERTIFICATE` is *present*, and a secret the
repository does not have arrives as an empty string rather than as nothing at
all, which used to kill the macOS job at `security import` having compiled
perfectly.

## The licence

MIT **or** Apache-2.0, at the taker's option, which is the Rust ecosystem's
own arrangement. Both texts are in `LICENSE`; the pair is named as the SPDX
expression `MIT OR Apache-2.0` in `package.json`, in `Cargo.toml` and in
`tauri.conf.json`, and `bundle.licenseFile` points the installers at the same
file so a `.deb` and an `.msi` say what the repository says.

Nothing in the tree forced this or anything stronger: pdf.js is Apache-2.0,
Tauri and nearly every crate under it are MIT or Apache-2.0, the handful of
MPL-2.0 crates are copyleft per file and unmodified, and the webview is the
system's rather than ours. The choice was free, and offering both is strictly
more permissive than offering either — MIT is the short one every legal review
already knows, Apache-2.0 is the one with an express patent grant, and a taker
who needs one of those does not have to argue for it.

**One file, not two.** The convention elsewhere is `LICENSE-MIT` beside
`LICENSE-APACHE`, and the reason for departing from it is the reason this
codebase keeps departing from it: a bundler takes one path, and pointing it at
one of a pair would have shipped installers that name a licence the project
does not offer alone. So `LICENSE` opens with the "either of … at your option"
statement and carries both texts under it, and there is exactly one copy of
each in the tree.

**Attribution is the one obligation, and it is discharged by files that
travel.** `THIRD-PARTY.md` says what is bundled and where each licence text
lives, and `sync-pdfjs.mjs` copies pdf.js's own `LICENSE` beside the runtime
data it already copies — the fonts, the decoders and the colour profile each
carry theirs already. Adding a bundled component means a row in that table and,
if its licence text does not already come with it, a copy that ships.

## Releasing a version

Releases are manual and nothing else triggers them. `release.yml` is a
`workflow_dispatch` workflow, which means GitHub puts a **Run workflow** button
on it and never runs it on its own — no push, no tag, no schedule. There is
nothing to opt into beyond having Actions enabled for the repository; the
button appears because the file is on the default branch, which is the one
condition `workflow_dispatch` has.

### Doing it

1. Get everything you want in the release onto `main` and let CI go green.
2. GitHub → **Actions** → **Release** in the left-hand list → **Run workflow**.
3. Branch `main`, version `0.1.0` (three numbers, no `v`, no `-beta`), tick
   *pre-release* if it is one, → **Run workflow**.
4. Wait. The checks take two or three minutes, the four bundles fifteen to
   twenty-five between them, and the run publishes the release itself at the
   end. Nothing to do but read the log if it goes red.

That is the whole of it. What the run does, in order: `checks` (the same types
and tests CI runs, on the commit you are releasing), then `tag` — which writes
the version into the five files that carry it, commits, tags `v0.1.0`, pushes
both, and opens a **draft** release — then four `bundle` jobs in parallel, each
uploading its installers to that draft, and finally `publish`, which takes the
draft down. The release only becomes visible once every platform has produced
something, so nobody downloads half a set.

The first release is `0.1.0`, because that is what every file in the tree
already says. Dispatching a version the tree is already at is not a special
case and not an error: `set-version` finds nothing to change, `tag` finds
nothing to commit, and `v0.1.0` lands on the head of main as it stands. The
"Release" commit only appears from the second one onwards, when the number
actually moves — and `--generate-notes` has no previous release to work from,
so the notes come from the whole history.

### What comes out

Attached to the release, under names with no version in them — see the note
above on why:

| platform | files |
| -------- | ----- |
| macOS | `Moonowl-macos-arm64.dmg`, `Moonowl-macos-x64.dmg` |
| Linux | `Moonowl-linux-x86_64.AppImage`, `Moonowl-linux-amd64.deb`, `Moonowl-linux-x86_64.rpm` |
| Windows | `Moonowl-windows-setup.exe`, `Moonowl-windows.msi` |

The notes open with a three-line download list written by `bundle.yml`, one
line per platform, because GitHub collapses the assets list and a reader who
has never heard of CI should not have to expand it. Under that, for a numbered
release, are notes generated from the commits since the last one — as good as
the commit messages are, and editable on the releases page afterwards.

### When it goes wrong

*A bundle job failed.* Dispatch the same version again. The `tag` job finds the
tag, finds the draft still a draft, skips the version bump, and the bundles
rebuild and upload over what is there. Only when a release has been *published*
does a repeat dispatch refuse, because at that point the version is somebody
else's.

*The version was wrong.* Delete the release and the tag on GitHub, `git push
origin :refs/tags/v0.1.0`, and revert the "Release 0.1.0" commit if there is
one — a dispatch that changed nothing made none. Then start again.

*`git push` failed with 403.* Settings → Actions → General → Workflow
permissions is on "Read repository contents", and the `tag` job cannot push a
commit or a tag with a read-only token. "Read and write permissions".

*It ran on a branch.* The `tag` job refuses anything but `main`, before it
writes anything.

### Building one locally

The workflow does nothing you cannot do by hand, which is the way to debug a
bundling problem without waiting on CI:

```sh
cargo install cargo-packager --locked
cargo build --release
cargo packager --release                         # every format for this host
cargo packager --release --formats dmg
```

Installers land in `target/release/`, or `target/<target>/release/` when
`--target` was given. `cargo-packager` reads `[package.metadata.packager]` in
`Cargo.toml` — the fields `tauri.conf.json` used to hold — and the bundle needs
pdfium in `pdfium/` beside the checkout, which is where `resources` and
`frameworks` point.

The version lives in two files now, `Cargo.toml` and `Cargo.lock`, and the tag
job in `release.yml` writes both. It was five under Tauri, with a script and a
test to keep them agreeing.

### The runner images are a decision, not a default

Both macOS builds run on Apple silicon and the Intel one cross-compiles,
because the Intel runners were retired at the end of 2025 — Apple's toolchain
targets either architecture from either host, and bundling never runs what it
built. Two separate DMGs rather than one universal build, for the same reason
the brief asks for a small binary: a universal installer carries both
architectures to run one of them.

Linux builds on `ubuntu-22.04` on purpose. A binary runs on the glibc it was
built against and anything newer, never anything older, so the runner's age is
what decides whether the `.deb` installs on Debian stable. That image is
deprecated from September 2026 and unsupported from April 2027, and bumping the
label is the wrong fix when it goes: it silently narrows the set of machines
the release runs on. Build in a container of the oldest glibc worth supporting
instead.

The icon has a source, and it is `icons/app-icon.svg`: `app-icon.png` was once
the only copy of the design, so the first change to it began by measuring the
old bitmap back into numbers. Everything under `icons/` is generated from the
SVG by `scripts/icons.sh` and nothing there is edited by hand. The second
source beside it, `document-icon.svg`, is the sheet a PDF wears in the Finder
while Moonowl is its reader — the app's icon on a page, embedded by reference —
and `Info.plist` is what names it, because cargo-packager's
`file-associations` has no field for a document icon.

## What a critical read turned up

A pass over the whole tree looking for what was wrong rather than for what was
there. All of it is fixed and the fixes are in the code; what is kept here is
the part that would not be caught again by the same means, because four of the
answers were new *gates* rather than new code.

**The gates, which were not gates.** `cargo test` was not in CI, so eleven Rust
tests covering the settings write race and the half-written-document gate ran
nowhere. `tsconfig.json` named `vite.config.js` and `scripts/**/*.mjs` in
`include` with no `allowJs`, so `tsc` silently took neither and the harness read
as though it had been checked since it was written — splitting it into
`tsconfig.node.json` found twenty-one real errors. And `sidebar.ts` and
`settings.ts` had no tests at all, by source or through the harness; they were
also where the two worst bugs were.

**Four shapes that will come back.**

*A second place that holds pages.* `Sidebar.draw` called `doc.getPage` directly
and dropped the proxy on the floor, so scrolling the Pages tab down a scanned
book parsed every page and kept the lot — through a door the viewer's own
accounting cannot see. `cleanup()` after every thumbnail, `THUMB_CACHE`, and a
`RenderTask` per page so a theme change calls off what it is replacing.

*A cheap answer that admits no failure.* `parseInt("12345g", 16)` stops at the
character it cannot read and returns what it had, so `#12345g` came back as a
plausible colour from a string that is not one — the worst of the three possible
behaviours, because nobody notices. Every length is checked against the alphabet
now, and `unreadableColors` is how a hand-written theme finds out.

*A probe that checks some of what it stands for.* `canBlend` tested three of the
five blend modes `recolor` uses, so an engine dropping either of the other two
would have passed the probe, taken the fast path, and produced a page inverted
rather than recoloured.

*A per-pixel question asked per pixel.* `within(regions, x, y)` scanned the whole
rectangle list for every pixel in the bounding box — free for a whole-page
recolour with no regions, and 3965ms against 18ms for a bibliography's two
hundred links, whose bounding box is the whole page. `maskFor` fills each
rectangle once, so it costs the sum of their areas rather than the box around
them, and the overlap problem falls out.

**And the one thing that was right.** The paged-mode sparse `boxes` array is a
genuine trap — two binary searches, `trackCurrentPage`, `pointAt` and `mount`
all have to know about it — and every one of them did, each with a comment
saying why. That is the correct amount of defence for the shape. Read that block
before touching `relayout`.
