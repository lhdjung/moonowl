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

Everything above is the brief. What follows is the app as it stands: the rules
and the traps, not the history. `experiments/PROGRESS.md` is the record of the
port from the retired Tauri/TypeScript/pdf.js app; module doc comments still
name that app's files (`viewer.ts`, `themes.ts`) as where a rule came from.

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
  render.rs       the renderer trait pdfium sits behind
  pdfium.rs       the one pdfium instance, behind the one lock it needs
  gpu.rs          the shader that recolours, selects and draws links
  recolor.rs      the same recolouring on the CPU — the reference, and the
                  half the screenshot tests read
  palette.rs      a theme's colours resolved, and the chrome shades derived
  crop.rs         trimmed margins, measured off a sample
  select.rs       selecting words on a page
  styles.rs       all of the CSS, as one string
  icons.rs        the hand-drawn icon set
  store.rs        the library: where you were, what was open, what you marked
  markup.rs       highlights written into the document, and taken back out
  search.rs       the full-document index, the fold, and match stepping
  sidebar.rs      contents, marks, thumbnails, search results
  sign.rs         a drawn signature as the specification's own /Ink annotation
  prefs.rs        the Settings window and the theme editor
  shell.rs        the window, the menus and the cascade
  windows.rs      what a second window is, and what closing one means (the
                  deciding half); session.rs is the acting half
  single.rs       one process per user, over a Unix socket
  openfiles.rs dock.rs tabs.rs print.rs
                  the macOS/AppKit corners — see "Platform corners"
  steady.rs nav.rs  small restatements of renderer/navigation upstream pieces
  config.rs       the config directory and `atomic_write`
  stats.rs        counters for what a session costs
  harness.rs      the reader driven with no window and no screen
  fixture.rs      every test PDF, written in Rust
  emit.rs         news, and the mailbox each window reads it out of
  theme.rs settings.rs keys.rs library.rs watch.rs
vendor/parley     parley 0.11.1 with one line changed — see its README, and
                  `body` in styles.rs
vendor/blitz-paint  Blitz's painter at the pinned rev, painting a field's
                  selection in `--selection-background`/`--selection-color`
themes/*.toml     the fifteen packaged themes, embedded with include_str!
keys.toml         the commented template a new install gets, include_str!
icons/            generated from the two SVGs by scripts/icons.sh; never edited
build.rs          the shipped theme table, generated from themes/ and checked
tests/            `cargo test`; one test file per thing the reader does
  parity/         what the retired app's interface measured, frozen as the spec
examples/         `fixture.rs` from the command line — packaging's smoke document
experiments/      PROGRESS.md, the assessments, the Phase 0 spikes
```

## Settings, themes and the disk

**Anything that touches the disk stays off the thread that draws the window.**
Position is remembered on every pause in a scroll; a whole-file rewrite there
lands in the middle of the gesture this app exists to make smooth. Because
writes can then overlap, `settings.rs` and `library.rs` hold locks
(`config::hold`: a mutex, and a lock file across processes) and
`atomic_write` gives every write its own temp file. A settings or library
file that does not parse is never written over: writes are refused and the
reader is told. Marks and signatures are
written off the main thread too.

**Every window shares one settings table** (`store::shared`); a theme worn
in one is sent to the rest as `theme-worn`, and a write the disk refuses is
said once as `disk-refused`.

**Settings are written a group at a time.** A write changes only the keys it
names and leaves unknown keys alone; the defaults table in `settings.rs` is
also the whitelist. Changes are queued and flushed together (a theme comes with
its light/dark slot, a zoom with its fit mode); continuously moving values like
zoom wait 700ms (the scribe's `SETTLE`). Anything queued is flushed before the window goes.
No setting writes another: a theme chosen against the system while following
it holds until the system next switches, and a spread too wide for a fixed
zoom is fitted for the moment, not written.

**Themes are files.** The built-ins are rewritten into the user's themes
directory on every run: embedded copies are authoritative, a built-in edited in
place is overwritten (each file carries a banner saying so), and editing one
through the app saves a copy under its own id, which is never touched. A theme
is colours plus a `recolor` flag; `selection_area` is derived from the accent
when absent, `selection_text` from `selection_area`. `palette.rs` derives every
chrome shade from those, which is why a five-line file is enough.

**The shipped set is the directory.** `build.rs` globs `themes/` and *checks*
it: a theme that does not parse or names an unreadable colour is a build
failure. Each shipped file carries `order` = its position in the menu (1, 2,
3…; a duplicate fails the build; inserting means renumbering). User themes have
no `order` and list after the built-ins by name. Adding a theme is adding a file.

**A theme's colours are hex and nothing else** — `#abc`, `#abcd`, `#aabbcc`,
`#aabbccdd`, checked against the alphabet, alpha dropped. Anything else is
refused and reported, never guessed. Nothing may show a theme colour without
going through the parser: a swatch that hands a raw string to CSS shows a
colour the renderer cannot read.

**`watch.rs` follows the themes directory and each window's document.**
- A file is watched through its *directory*, filtered by name — atomic writes
  and compilers replace files by rename, and a file watch follows the inode.
  `follow` counts what wants a directory; two papers in one folder is normal.
- A change is a burst: events are collected until the disk is quiet for `SETTLE`.
- A document is not believed until `whole()` sees `%PDF-`, `%%EOF`, and a
  stable size — otherwise a mid-compile reload takes the document away.
- Theme reloads are decided by comparing loaded themes against what the app
  has, never by "something moved" (the app writes there itself).
- `follow` on the document already followed does nothing; remaking the watch or
  retaking the baseline swallows a draft that arrived during the reload. The
  watch is told about our own write only *after* writing.
- `keys.toml` is deliberately not watched (the config dir is written several
  times a minute); the Keyboard page has a Reload button.

**`library.toml`: plain keys before tables.** `Library.open` is serialised
before `file`, and `Entry.marks` last in an entry; a plain key written after an
array of tables lands inside the last table. Two tests say so.

## The viewer (`layout.rs`, `page.rs`)

- *Fit width computes against the full width*; `PAD_X` frames only modes where
  the page is narrower than the window. `PAD_Y` is unconditional.
- *The layout is rows*: one page, two side by side, or two with the first
  alone. Everything downstream works in rows. A spread's gap comes off the room
  *before* the scale is computed — it is screen distance, not paper.
- *Landing on a page means landing on the space above it*, recorded on the box
  at layout time (the gap, or `PAD_Y` at the start; not read off the previous
  box, which in a spread is its neighbour).
- *Every page is measured when the document opens* — pdfium loads each for
  its size, under its one lock, on the thread that asked. A scanned book
  opened with ⌘O stalls the window for that long; a reload does it on a thread.
  `boxes` is ordered, and scroll lookups binary-search it.
- *In paged mode `boxes` has holes* — every page but one. The binary searches,
  current-page tracking (`page_at`) and mounting all know; read that block
  before touching relayout.
- *Only pages near the viewport are mounted* (`OVERSCAN`), rendered
  nearest-the-middle first.
- *A page's number is not its position*: toolbar, pill, thumbnails and go-to
  speak in `/PageLabels` where they exist; the library records positions.

**Recolouring maps lightness, and keeps hue.** Luma places a pixel on the ramp
between the theme's ink and paper; a pixel with colour of its own keeps its hue
and saturation (HSL, chroma rescaled to the room at the new lightness, so
nothing clips). `COLOUR_FLOOR` is where keeping colour begins, so plain type is
identical either way. `duotone` — the flat mapping — is for links and selected
words, which must take the theme's colour. `WHITE_POINT` (235) calls everything
above it paper: it kills hyperref's bright cages and a scan's warm tint, at the
cost of 4%-grey code blocks merging into the background. The constant is the
only place to change that trade. `gpu.rs`/`*.wgsl` and `recolor.rs` must agree;
`tests/recolor.rs` says so. "Recolour pictures too" is on by default.

**Selected words and links are painted from the page's own pixels** by the
shader — real glyphs through the duotone ramp — never from a text layer.

**Trimmed margins are measured over a sample, not per page** (`crop.rs`): eight
pages (first, last, evenly spaced), union, padded, never more than a third off
a side, probed on white so the answer does not move with the theme. Per-page
crops make a continuous scroll breathe.

**Memory: every place that holds pages needs a cap.** The viewer's mounted
pages and the thumbnail column (`THUMB_CACHE`) are the two; the thumbnails once
had no accounting at all. `stats.rs` and `tests/cost.rs` are the
instruments — measure with the Pages tab open and scrolled.

## Windows

- **Opening a document never displaces one.** `hand_over` looks for an empty
  window (the focused one first), else makes one, claiming it immediately so
  two simultaneous opens do not share a window. "Empty" is trusted from the
  read handle, not the bookkeeping. If no window can be made, the document goes
  into the one that exists rather than nowhere.
- File handle and document watch are keyed per window; news for one window is
  addressed to it (`emit.rs`).
- **Geometry belongs to the launch window.** Only it saves its size (not its place);
  others cascade straight down off the window in front (same left, right and
  bottom edges). Letting the last-moved window own it drifts. A new window
  adopts full screen from the window itself without remembering it.
- **On macOS a window's position does not survive `show()`**: `Placements`
  holds the target and `place` applies it right after `show`, same turn.
- **`library.open` is one path per window; a launch reopens one** — the one read
  or opened last (`store::reopening`), maximized. `Exiting` separates "closed by
  the reader" (forget it) from "open at quit" (keep it), and is raised by every
  path that ends the app. A close never writes an *empty* list, since closing
  the last window is how most people quit. This write goes through the
  scribe, in order, so racing closes cannot leave the stalest list.
- Windows other than the first are made after the launch window reports ready,
  not during setup (on macOS an early window is "visible" and not on screen).

## Markup (`markup.rs`, `markup-assessment.md`)

A mark is a real `/Subtype /Highlight` with `/QuadPoints`, `/C` and an
appearance stream, readable by Preview, Acrobat and Zotero. Only highlights:
no underline, strike-out or squiggly.

- **pdfium writes it, and the save is a full rewrite** (`FPDF_SaveAsCopy`,
  flags 0) — not an incremental update. Fine for a paper; the end of a
  signature. Removal is `FPDFPage_RemoveAnnot`. The file is released before it
  is written, nothing is left beside the document, and the write reloads the
  document through the same path a recompile uses — so there is no
  pending-markup layer.
- **The journal (`Highlight` in `library.rs`) is a cache and recovery log,
  never an authority.** It is rebuilt from the file on open; what survives is
  only what the file cannot carry, held with `annotation_id: null` and marked
  in the sidebar as not in the document.
- **Edges, each said once in one line:** encrypted, read-only (asked of the
  disk by opening for write — the only true answer) or over
  `MARKUP_IN_FILE_LIMIT` (100MB) → journal only. Signed → asked, once per
  document. Syncing folder → one sentence, then the write. A page with no
  text (a scan, a figure) → "there is no text on this page to highlight".
- **A rebuilt document loses its annotations**; `find_quote` re-finds each
  quote through `search::fold` (ligatures split, soft hyphens dropped), outward
  from its old page, and writes the lot in one go. Offered as a button, never
  automatic: this app does not write to somebody's file without being asked.
  What is not found stays in the journal and is counted out loud.

## The keyboard (`keymap.rs`, `keys.rs`)

Every key is an **action** with a name; a chord is a lookup, never an ordered
`if` chain, so a collision is reported rather than decided by branch order.

- An event offers several spellings, best first: the key's character, then the
  physical key; non-letters also with and without Shift. That is what makes
  ⌥⌘G work (Option turns G into ©) and ⇧Space differ from Space. Shift is never
  dropped for a letter.
- `mod` is ⌘ on a Mac and Ctrl elsewhere; a literal `ctrl` is normalised to
  `mod` off the Mac — why Vim's ⌃D/⌃U do not ship (half-screen is `d`/`u`).
- `keys.toml` replaces per action and never adds: unnamed actions keep their
  defaults, an empty list unbinds. Nothing in that path throws; bad chords,
  unknown actions and double claims are reported on the Keyboard page and the
  rest of the file still lands.
- Sequences exist for `g g`. A chord that both acts and begins a sequence is a
  conflict and the shorter keeps the key — hence the page field on `p`.
- The Keyboard page is drawn from the keymap, never from a list of its own.
- A plain key typed in a text field is also a shortcut unless the field stops it.

## Things that will bite

**`use_effect`'s closure is replaced on every render, so nothing it captures
survives one.** A `let mut` captured by `move` compares against its initial
value for ever; three such effects once cost a render and full paint per frame
— 100% of a core, idle, with identical frames. What an effect remembers goes in
a `use_hook` (`Cell`/`RefCell`) beside it. `tests/settle.rs` asserts
`stats::RENDERS` stops climbing.

**A press lands on a custom widget; a click never comes out of one.** Blitz
delivers the press to the `object` and makes no `click`. `pointer-events: none`
on the widget lets the click reach the button around it.

**Blitz hit-testing.** A z-indexed child is only hit-tested inside its stacking
context's union, and `z-index: 0` is not a layer. Blitz also shrinks a flex
item past its own padding and does not hit-test what overflows a parent —
`.doc-title` went to 0px, and at 16px was painted but unclickable.

**Nothing in this window scrolls the window.** An unspent wheel chains out to
the viewport. `.root` is `overflow: hidden`, `.window-pane` scrolls on one axis;
everything that scrolls has a box of its own.

**A wgpu device is not a key.** Each window has its own instance, adapter and
device, and `wgpu::Device` compares by an id that repeats across instances —
a second window drew through the first's pipeline and the app died.
`Recolorer::shared` keys on `wgpu::Instance`. Anything cached per device wants
the same key.

**A stepper is a field.** Typed values are clamped to the range, never snapped
to the step. The unit sits *outside* the field, unselectable ("16 px" plus a
typed 30 was "3016 px"). A click selects the contents (`select_on_arrival`), a
second click places the caret, Tab arrives with the caret after the number
(`caret_on_arrival`). A stepper never asks for the keyboard (`data-keyboard`):
the innermost asker wins every event.

**Escape and menus.** While a menu is open the app-level handler stands down
for `dismiss`. A menu's handler sees every key first, so arrows, Home and End
are handed back when the target is a field; Escape and Tab stay the menu's.
Clicking the button that opened a menu closes it.

**The find bar is not a popover.** It holds the keyboard and a query;
`FIND_KEEPS_OPEN` lists where the pointer may go without closing it. "Match
case" is a parameter to `fold`; "Whole words" is tested against the *folded*
text (so a word hyphenated across a line is one word); "Highlight all" only
changes what is painted. All three are settings. A case change refolds, never
re-extracts.

**Running a dev build beside the installed app.** The single-instance socket
routes the dev launch into the running app and exits 0 — every change appears
to do nothing. `pgrep -fl Moonowl`, or `MOONOWL_CONFIG=/tmp/hcfg` (short path:
the socket dies past ~104 characters).

**Builds are expensive.** One cargo run at a time, in the foreground.

## Testing

**`cargo test` is the first thing to run and the first thing to add to.**
`harness.rs` builds the reader's DOM, resolves style and layout against a stated
viewport, delivers real pointer and key events, and draws through `vello_cpu` —
no GPU, no window, three platforms.

- The CPU path is real code: a widget that draws through wgpu needs its
  `Software` half kept working, or screenshot tests stop covering what is seen.
- Wait for the condition (`settle()`), never the clock.
- **The harness lays out in one font on every platform**
  (`tests/fonts/DejaVuSans.ttf`, no system fonts), because SF Pro, DejaVu and
  Segoe UI set the same words 10% apart and every width tuned on a Mac then
  failed on Windows. `Options::system_font` opts out, and only
  `tests/parity.rs` and `tests/tracking.rs` do, on macOS:
  `tests/parity/app-inventory.json` is a *macOS* measurement (SF Pro), and off
  macOS its allowances are proportional. `tests/tracking.rs` pins one width as
  a number — if that fails on one platform, the font is not pinned there.
- The suite cannot cover the window itself — shell, cascade, full screen, Dock,
  socket. `windows.rs` states those rules in one place so the testable part is.
- Reading with the app is still the only instrument for answers that are
  correct but wrongly placed, coloured or timed.

## Platform corners

- **pdfium is a shared library, not in the binary or the repo.**
  `MOONOWL_PDFIUM` names its directory; else `library_dir()` in `pdfium.rs`
  checks `Contents/Frameworks` (.app), `/usr/lib/Moonowl` (.deb), and the
  executable's directory (.msi). For bundling it lives in `pdfium/`.
- **A Finder double-click is an Apple Event, not an argument.** `openfiles.rs`
  sets an application delegate of its own before the event loop starts
  (winit sets none); `NSAppleEventManager` loses the cold-launch document.
- **Printing** (`print.rs`): macOS runs PDFKit's `NSPrintOperation` as a
  *sheet* (a modal run loop inside a Dioxus handler re-enters the window).
  Windows is `PrintDlg` plus pdfium's `FPDF_RenderPage` into the DC, on a
  thread, one page per lock — behind `pdfium_use_win32`; compiled from a Mac,
  not yet run on Windows. Linux hands off to the system.
- **Tabs** (`tabs.rs`): `allowsAutomaticWindowTabbing` is turned off at startup
  so ⌘N is always a window; "New tab" is explicit (`addTabbedWindow:ordered:`).
  ⌘1–⌘9 choose a tab and ⌘W closes one; the zoom modes are ⌥⌘1/⌥⌘2 on macOS.
- **Dock "New Window"** (`dock.rs`): AppKit's `dockMenu` with a runtime-built
  ObjC target. The window is made on its own thread, because the item fires on
  the main thread, which making a window needs to question. It is the one place
  spelled in title case — the Dock is the system's furniture.
- **Windows is still one process per launch**: the single-instance socket
  wants a named pipe.
- `mupdf` is AGPL; that is a licensing decision, not a technical one.

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

## Git

Make a separate commit for every fix or new feature. Before committing, run
`cargo fmt`.

## CI and releasing

`checks.yml` (the suite, three platforms) and `bundle.yml` (installers) are
*reusable*, because a push and a release both need them. `ci.yml` runs checks
on pushes and PRs; `nightly.yml` builds into a `nightly-next` draft and swaps
it in for the rolling `nightly` release once every bundle is there;
`release.yml` is the only thing that names a version and is
`workflow_dispatch` only.

**To release:** main green → Actions → Release → Run workflow, branch `main`,
version `0.1.0` (three numbers, no `v`). The run does checks, then `tag`
(writes the version into `Cargo.toml` and `Cargo.lock`, commits, tags, opens a
*draft*), four bundles in parallel, then publishes — so nobody downloads half a
set. Dispatching the version the tree is already at is fine.

- *A bundle failed:* dispatch the same version again; it reuses the tag and
  the draft. Only a published release refuses.
- *Wrong version:* delete release and tag, `git push origin :refs/tags/vX`,
  revert the "Release X" commit if there is one.
- *`git push` 403:* Settings → Actions → Workflow permissions → read and write.
- `tag` refuses any branch but `main`.

**Assets carry no version in their names** (`Moonowl-macos-arm64.dmg`,
`-macos-x64.dmg`, `-linux-x86_64.AppImage`, `-linux-amd64.deb`,
`-linux-x86_64.rpm`, `-windows-setup.exe`, `-windows.msi`), so the notes'
download table and the README's `releases/latest/download/<name>` links never
need editing. The table is in the notes because GitHub collapses assets.

**Runner images are decisions.** Both macOS builds run on Apple silicon (Intel
cross-compiles); two DMGs, not a universal one, for size. Linux builds on
`ubuntu-22.04` for its old glibc — when that image goes (unsupported April
2027), build in an old-glibc container; do not just bump the label.

**macOS signing: ad-hoc, and two traps.** `signing-identity = "-"` seals the
whole bundle; an unsigned `.app` around a linker-signed binary fails
`codesign --verify` and Gatekeeper offers no *Open Anyway* at all.
cargo-packager always signs with the hardened runtime, which refuses an ad-hoc
`libpdfium.dylib` — hence `disable-library-validation` in `entitlements.plist`,
which stays right even with a real certificate. Nothing is certificate-signed
or notarised; the README tells readers the first-launch steps. The `APPLE_*`
secrets are promoted under other names, because an absent secret arrives as an
empty string and a bundler goes by *presence*.

**Icons.** `icons/app-icon.svg` is the source; `document-icon.svg` is the
owl's head alone for small sizes. `scripts/icons.sh` generates everything else.
macOS documents wear `document.icns` via `Info.plist`; on Windows `build.rs`
compiles `icon.ico` into the exe with `winresource` (the bundler only puts it
on the installer); Linux ships `32x32.png` only — a file's icon there comes
from the MIME theme, and installing our own would dress every PDF on the machine.

## The licence

`MIT OR Apache-2.0`. **One `LICENSE` file carrying both texts**, not the usual
pair, because a bundler takes one path. There is deliberately no
`license-file` in `Cargo.toml` (see the comment there). `THIRD-PARTY.md` says
what is bundled and where each licence lives; adding a bundled component means
a row there and a licence text that ships.
