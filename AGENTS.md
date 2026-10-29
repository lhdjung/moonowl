# Moonowl, a smooth reading experience

Note: everything down to the horizontal rule describes what the project SHOULD be like. Below it, "Architecture of the built app" describes what it currently is.

## General
Moonowl is a PDF reader written in Rust, drawn by Dioxus Native rather than
by a webview. Cross-plattform, ergonomic, with a calm UI, and efficient: fast with no lags, little memory and CPU consumption, and a small binary.

Importantly, all settings are preserved throughout sessions, and all of them are independent of each other: changing one setting does not change any other setting.

## UI
The UI is clean and sleek. It is close to full-screen by default. In particular, the app should reserve much or most or all of the vertical axis for the document, as there is likely more room to the sides. No clunky or overbearing UI covers the document. However, true full-screen – no UI elements at all – is easily toggleable, and leaving it should be at least as easy and obvious.

Page progression is continuous scrolling by default, and it's a strong default: it changes only when the user chooses another mode in a menu or in Settings, never through a shortcut, because continuous scrolling is so much better than the alternatives, and hitting such a keybind by accident would be frustrating.

There is no clutter in the UI. All elements are nice, modern, polished, and look straight out of professional web design. However, they should not have the typical vibe coded look, i.e., small caps (or caps in general), italics, exotic fonts, and a kind of dead, technical, sterile look. On the contrary, the look should be friendly and open; fresh and lively but in a subtle way.

UI elements might include symbols but they are definitely not just symbols, and not just tiny symbols. For each element, a combination of one symbol and one succinct text label would probably be good. When the window is too narrow for the labels, the toolbar's controls fold down to their symbols alone; that is intended.

No animations unless the user takes an action. No pop-up windows that get into people's way. The one deliberate exception: selecting text offers the highlight colours right under the selection, by default (it can be switched off in Settings).

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
and the traps, not the history. `plans/dormant/experiments/PROGRESS.md` is the
record of the port from the retired Tauri/TypeScript/pdf.js app. Only the
parity tests and the harness still refer to it.

## Shape

**One Rust binary.** [Dioxus] Native draws the interface — [Blitz] laying out
real HTML and CSS with Stylo, Parley and Taffy, composited by `vello-hybrid` —
and pages are rasterised by **pdfium** through `pdfium-render`. No webview, no
JavaScript, no framework beyond Dioxus's own signals. **lopdf** (default
features off) reads and writes PDF objects where pdfium cannot; it runs only
inside an edit, never while reading — see "Page edits".

**Parley and Blitz come from our forks**, named in
`Cargo.toml`: the `moonowl-2` branches of `lhdjung/parley` (`trak` read at the
CSS size — see `body` in styles.rs) and `lhdjung/blitz`, which sit on
upstream main (`blitz-paint` paints a selection, in a field or not, in
`--selection-background`/`--selection-color`; two or three presses on text
select a word or a paragraph; an anonymous block is restyled with the node
that owns it, so bare text in a button follows a theme; a text input undoes and redoes
its own typing, and reads ⌘Z, ⌘C, ⌘X, ⌘V and ⌘A off the physical key where
the layout types no Latin letter; a drag over `user-select: text` selects — upstream only ever
started one over `auto`; an attribute Dioxus sets is in no namespace, or no
`[data-…]` selector matches it; a textarea starts from its `value`; measuring
text leaves the lines it is painted in as they were; a cursor style that
changes under a still pointer reaches the window; hit testing skips a node
removed since the last layout). Bumping a
pin means rebasing the branch and moving the rev; the older `moonowl`
branches stay, so older commits still build.

[Dioxus]: https://dioxuslabs.com
[Blitz]: https://github.com/DioxusLabs/blitz

```
src/
  main.rs         the process: the shell, the first window, the socket
  lib.rs          the modules, for the binary, the harness and the tests
  app.rs          the Viewer: state, menus, keyboard, every window   ← the heart
  app/            the Viewer's halves that stand alone: menus.rs, actions.rs
                  (`perform`), listen.rs (the mailbox), toolbar.rs,
                  dialogs.rs (Details, Sign, the password), ink.rs,
                  markup.rs, search.rs, disk.rs, pages.rs (writing page
                  edits), organise.rs (the organiser's Viewer half) —
                  child modules, so they see
                  the Viewer's private fields
  keymap.rs       every action, its default chords, and event → chord
  layout.rs       where each page sits, and what is on screen
  page.rs         the page widget: pdfium into a texture, or into ImageData
  render.rs       the renderer trait pdfium sits behind
  pdfium.rs       the one pdfium instance, behind the one lock it needs
  gpu.rs          the shader that recolours, selects and draws links:
                  recolor.wgsl for a page, regions.wgsl for its runs
  recolor.rs      the same recolouring on the CPU — the reference, and the
                  half the screenshot tests read
  palette.rs      a theme's colours resolved, and the chrome shades derived
  crop.rs         trimmed margins, measured off a sample
  select.rs       selecting words on a page
  styles.rs       all of the CSS, as one string
  icons.rs        the hand-drawn icon set
  store.rs        the library: where you were, what was open, what you marked
  markup.rs       highlights written into the document, and taken back out
  objects.rs      the file's own objects through lopdf, after pdfium has saved:
                  incremental updates, smaller files, the document's details
  outline.rs      the outline kept true when pages change, through lopdf
  labels.rs       page labels kept with their pages, and numbered by the reader
  arrange.rs      page edits: one `Edit::Arrange` for the document, and
                  `assemble` for new files
  organiser.rs    the page organiser: the draft, its grid, and its view
  search.rs       the full-document index, the fold, and match stepping
  export.rs       a page saved as a picture, and the document's text
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
  shelf.rs        a folder of named TOML files with a shipped set: the rules
                  themes and highlight palettes share, as one trait
  palettes.rs     highlight palettes, kept on the shelf
  theme.rs settings.rs keys.rs library.rs watch.rs
themes/*.toml     the packaged themes, embedded with include_str!
palettes/*.toml   the packaged highlight palettes, likewise
keys.toml         the commented template a new install gets, include_str!
icons/            generated from the two SVGs by scripts/icons.sh; never edited
build.rs          the shipped theme and palette tables, generated from
                  themes/ and palettes/ and checked
tests/            `cargo test`; one test file per thing the reader does
  parity/         what the retired app's interface measured, frozen as the spec
examples/         `fixture.rs` from the command line — packaging's smoke document
plans/            the tour (ARCHITECTURE.md), the assessments, reviews, todo
  dormant/        what is finished: the port's PROGRESS.md and Phase 0
                  spikes, the audit
```

## Settings, themes and the disk

**Anything that touches the disk stays off the thread that draws the window.**
Position is remembered on every pause in a scroll; a whole-file rewrite there
lands in the middle of the gesture this app exists to make smooth. Because
writes can then overlap, `settings.rs` and `library.rs` hold locks
(`config::hold`: a mutex, and a lock file across processes) and
`atomic_write` gives every write its own temp file. A settings or library
file that does not parse is never written over: writes are refused and the
reader is told. Marks, signatures, themes and palettes are
written off the main thread too.

**Every window shares one settings table** (`store::shared`); a theme worn
in one is sent to the rest as `Event::ThemeWorn`, a Reload of `keys.toml` as `Event::KeysReloaded`, and a write the disk refuses is
said once as `Event::DiskRefused`.

**Settings are written a group at a time.** A write changes only the keys it
names and leaves unknown keys alone; the defaults table in `settings.rs` is
also the whitelist. Changes are queued and flushed together (a theme comes with
its light/dark slot, a zoom with its fit mode); continuously moving values like
zoom wait 700ms (the scribe's `SETTLE`). Anything queued is flushed before the window goes.
No setting writes another: a theme chosen against the system while following
it holds until the system next switches, and a spread too wide for a fixed
zoom is fitted for the moment — the zoom is not written, only that the fit is
the pair's (`spread_fitted`), so the next launch fits it again.

**Themes are files.** The built-ins are rewritten into the user's themes
directory on every run: embedded copies are authoritative, a built-in edited in
place is overwritten (each file carries a banner saying so), and editing one
through the app saves a copy under its own id, which is never touched. A theme
is colours plus a `recolor` flag; `selection_area` is derived from the accent
when absent, `selection_text` from `selection_area`, and `surround` (around the
page) from the background. `palette.rs` derives every chrome shade from those,
which is why a five-line file is enough.

**The page shows a theme's colours as named; only the app's own words are
kept readable.** Any surround goes with any paper: where the ink cannot be read
on the surround (a light theme on a dark one, `Palette::inks_surround`), what
stands on the surround — the start screen, the scrollbar — swaps ink and paper
(`Palette::on_surround`), and an accent the surround swallows gives way to the
ink. Selected text in the interface is the selection
pair moved until it reads (`Palette::selected`); the page — selected words,
search bands — shows the pair exactly as chosen, since the editor is WYSIWYG.
While the reader behind a window is a preview (the theme editor, the highlight
colours), the scrim is clear: a wash of the surround is a colour the theme
does not put on the page.

**Highlight palettes are kept as themes are**, by the same code: `shelf.rs` is
the folder's rules, generic over `Kept`, and `theme.rs` and `palettes.rs` are
only the type, its check, its file form and its words. A palette is a name and
six colours; `highlight_palette` names the one new marks are made in, and a
mark already in a document keeps its colour. The Highlight colours window
edits a draft that the swatches offer at once, saved by Save or by the window
closing — a picker reports every move of a drag, and a save can move the file.
Opened over a highlight, a colour changed is that highlight's, and the
palette keeps it only on Save.

**The shipped set is the directory.** `build.rs` globs `themes/` and
`palettes/` and *checks* them: a file that does not parse or names an
unreadable colour is a build failure. The menu's order is each folder's
`order` file, one id per line; a file it does not list, or an id with no file,
fails the build. User themes list after the built-ins by name. Adding a theme
is adding a file and a line.

**A theme's colours are hex and nothing else** — `#abc`, `#abcd`, `#aabbcc`,
`#aabbccdd`, checked against the alphabet, alpha dropped. Anything else is
refused and reported, never guessed. Nothing may show a theme colour without
going through the parser: a swatch that hands a raw string to CSS shows a
colour the renderer cannot read.

**`watch.rs` follows the themes and palettes directories and each window's
document.**
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
- *Every page is measured when the document opens, by index* — size and
  label without loading a page, because loading one parses all of it (a pass
  that did was three seconds on a 1,700-page book). A page's space is kept
  when something loads that page: the markup walk after open loads every
  one, a page at a time behind `library_when_free`, which yields pdfium's
  lock to any page being drawn; links and text are read as a page comes into
  view. Nothing loads a page to *place* something on it: a heading or a link
  into a page not yet known jumps to the page's top and lands on its spot
  once the page has been read (`land_on`, `PageSource::place`). A reopen
  after a write of our own takes the spaces over (`learn_spaces`) and reads
  marks off only the pages that can hold one (`markup_on`). A document put
  down is `release`d at once, so a thread still reading it stops loading its
  pages.
  `boxes` is ordered, and scroll lookups binary-search it.
- *In paged mode `boxes` has holes* — every page but one. The binary searches,
  current-page tracking (`page_at`) and mounting all know; read that block
  before touching relayout.
- *Only pages near the viewport are mounted* (`OVERSCAN`), rendered
  nearest-the-middle first.
- *A page is drawn whole under `MAX_PIXELS`, and past it only what is on
  screen is sharp*: a second widget per page (`PageWidget::detail`) draws the
  visible part plus a margin at full size, as a tighter crop, over the page
  stretched. It keeps its part until the next lands, and renders after every
  whole page in the queue. `Ramped::shown` is what it reads.
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
pages and the thumbnail column (only its mounted band of rows) hold pixels; the search index
holds text (`search::INDEX_BUDGET`, beyond the pages that match), and the
pages waiting for their links and notes are a queue (`ANNOTATIONS_QUEUED`).
The thumbnails once had no accounting at all. `stats.rs` and `tests/cost.rs` are the
instruments — measure with the Pages tab open and scrolled.

**A page's pixels exist once.** `PageSource::render` *lends* the pixels
(pdfium's `scratch` buffer, for the length of the call); anything that has to
keep them or send them to another thread uses `render_owned`, which pdfium
draws straight into a buffer the caller keeps. Copying out of `render` instead
(`bitmap.bgra.to_vec()`) passes every test and costs a whole page, up to 48MB,
for the life of the process: macOS keeps a freed block that size as dirty
`MALLOC_LARGE (empty)`. Trace one with `MallocStackLogging=1` and
`malloc_history <pid> <address>` on the debug build (release is stripped).

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
  bottom edges). Letting the last-moved window own it drifts. Full screen
  and presenting are the launch window's too, remembered as switches; a new
  window adopts full screen from the window itself without remembering it.
- **On macOS a window's position does not survive `show()`**: `Placements`
  holds the target and `place` applies it right after `show`, same turn.
- **`library.open` is one path per window; a launch reopens one** — the one read
  or opened last (`store::reopening`), maximized. `Exiting` separates "closed by
  the reader" (forget it) from "open at quit" (keep it), and is raised by every
  path that ends the app. A close never writes an *empty* list, since closing
  the last window is how most people quit. This write goes through the
  scribe, in order, so racing closes cannot leave the stalest list.
- **The close button and the application menu's Quit ask the window
  first** (`Shell::on_closing`, `Event::CloseAsked`): the window answers
  with `Ask::Close` or `Ask::QuitAnswered` when it may go, so a draft of
  arranged pages is asked about whichever way the window is closed. A Quit
  asks every window in turn, the one in front (or the one that heard ⌘Q)
  first, and the app ends once each has answered (`Shell::quitting`). A
  window that keeps its draft ends the round, and the next Quit asks every
  window again: the keymap's sends `Ask::Quit`, never an answer.
- Windows other than the first are made after the launch window reports ready,
  not during setup (on macOS an early window is "visible" and not on screen).

## Markup (`markup.rs`, `plans/markup-assessment.md`)

A mark is a real `/Subtype /Highlight` with `/QuadPoints`, `/C` and an
appearance stream, readable by Preview, Acrobat and Zotero. Only highlights:
no underline, strike-out or squiggly.

- **pdfium writes it, and the save is a full rewrite** (`FPDF_SaveAsCopy`,
  flags 0). A signed document is the exception: `objects::incremental`
  compares pdfium's save with the file object by object (pdfium keeps every
  object's number, though it writes every generation as 0, which
  `objects::generations` puts back) and appends only what changed, so the
  signed bytes stay as they were. A signed document that cannot be written
  that way is not written at all, and neither is a certified one whose
  `/DocMDP` does not allow the change: highlights and drawn signatures
  need `/P 3`, and no certification allows anything else (`objects::Update`). Removal is `FPDFPage_RemoveAnnot`. The file is released before it
  is written, nothing is left beside the document, and the write reloads the
  document through the same path a recompile uses — so there is no
  pending-markup layer. That round trip is ~0.5s on a paper, so the page
  paints a stand-in (`Ramped::marking`) from the click until the new draft's
  pixels replace it — a removal too, painted back to plain page; its ramp is solved through the shown paper and ink, with
  ends past a byte (`recolor::End`), so it is the real mark's colour.
- **A comment is the highlight's own `/Contents`** — what Preview and
  Acrobat write — so it exists only in the file, never beside it.
- **Undo puts the whole file back** from a copy taken before the change
  (`markup::Before`, in the config dir), with the journal as it was. Every
  write of the reader's own — highlights, signatures, passages put back —
  goes through `write_step`; a change on disk forgets every step.
- **The journal (`Highlight` in `library.rs`) is a cache and recovery log,
  never an authority.** It is rebuilt from the file on open; what survives is
  only what the file cannot carry, held with `annotation_id: null` and marked
  in the sidebar as not in the document.
- **Edges, each said once in one line:** encrypted, certified with a
  `/P` below 3, read-only (asked of the
  disk by opening for write — the only true answer) or over
  `markup::IN_FILE_LIMIT` (100MB) → journal only. Signed → appended after
  the signed bytes, without a question; only a page edit asks, and
  numbering pages is not one. A certified document refuses a page edit at
  once, since no certification allows one (`PageSource::certified`). A page with no text (a scan, a figure) → "there is no text on
  this page to highlight". A syncing folder gets no warning yet.
- **A rebuilt document loses its annotations**; `find_quote` re-finds each
  quote through `search::fold` (ligatures split, soft hyphens dropped), outward
  from its old page, and writes the lot in one go. Offered as a button, never
  automatic: this app does not write to somebody's file without being asked.
  What is not found stays in the journal and is counted out loud.

## Page edits (`arrange.rs`, `organiser.rs`, `app/organise.rs`, `app/pages.rs`, `plans/page-edits.md`)

- **Every page edit happens in the page organiser** ("Arrange pages…" in
  the Document menu and the thumbnail menu, "Combine or split…" on the
  start screen and "Combine or split files…" in the Open menu), a view over the whole window like Settings. It holds a draft
  (`Organiser::sheets`, with its own undo) and writes nothing until Save.
  The reading view has no page tools: the thumbnail column goes to pages
  and selects nothing, and its menu has only "Arrange pages…" and "Number
  pages from here…". Delete, Duplicate and the turn actions are the organiser's
  (`Viewer::organiser_action`) and do nothing outside it, and ⌘A selects
  every page there instead of the page's text; its arrows and
  Enter are read off the key before the keymap (`Viewer::organiser_key`),
  since Shift with an arrow is not a chord of its own. A file dropped on
  the window while the organiser is open is added to the draft, and a
  picture dropped with no organiser open opens one with the picture in it
  (`shell::is_document` accepts PNG and JPEG for that).
- **A file begins at a page** (`Sheet::begins`, with the file's name): the
  grid shows each file that will be written as a section. "Save as…"
  writes one file, or one per section into a folder (`assemble`, on a
  thread, writing over nothing). "Save into paper.pdf" is offered when the
  draft is one file with no page of the document twice, and writes one
  `Edit::Arrange`.
- **`Edit::Arrange` is every change at once**: removed, turned, moved and
  added pages. It is written in two passes, because pdfium-render keeps a
  document's handle to itself: pdfium-render copies in the other files'
  pages (one import call per file), turns pages and deletes the unused
  ones; then lopdf puts every page in place (`arrange::put_in_order`),
  straight under the root of the page tree and with what it inherited from
  the nodes above it, since a page `FPDF_MovePages` puts under another node
  takes that node's box, turn and fonts. Where lopdf cannot read the file,
  `arrange::reorder` uses `FPDF_MovePages` through the raw bindings.
- **A page edit is a write like a highlight**: `write_step_on` with a
  `Paging::Edit`, so it has undo, the stamp check and the reopen. Its thread
  works out an `arrange::Moved` (where every old page went) and `adopt`
  follows it: the reader's place, the way back, the bookmarks and the
  journal move with the pages, and the pages' spaces are carried over
  (`Edit::spaces`). A failed write moved nothing. Undo puts the whole file
  back and the bookmarks and journal with it (`Paging::Back`). The step
  carries the edit's `Moved` inverted, so the reader's place and the
  history follow the pages back. The pages are measured again. A removal
  of more than `arrange::ASK_BEYOND` pages asks first. An organiser open on
  the document is put away when the document changes on disk.
- Both ways of putting pages in order keep each page the same object, so
  headings still find their pages.
- pdfium writes neither the outline nor `/PageLabels`; `outline.rs` writes
  the outline through lopdf after pdfium has saved. A removal takes out
  every heading and link that led to a removed page (a heading's own
  headings take its place), and any other destination that led there,
  such as a named one, is made to lead nowhere. The structure tree
  forgets the page too: a part of it whose content was all on removed
  pages is taken out. A form field whose widgets were all on removed
  pages leaves the form, and an article's bead no longer names the
  page. Then nothing keeps the page in the file. Saved pages and combined files carry their headings, and a
  heading that only groups others comes along while one of them does. Where lopdf cannot read the file, the
  outline is left as pdfium wrote it, and the notice says a heading now
  leads nowhere.
  `labels.rs` gives every page the label it had (a page from another file
  is numbered on from the labelled page before it), and "Number pages from here…" in the thumbnail menu numbers
  the pages as a typed first label begins ("iv", "A-1") up to the next
  range. Where lopdf cannot read the file, labels stay with the places,
  and the notice says so.
- pdfium copies a link without its destination. `arrange::links_kept`
  reads each link's page before the copy, and `outline::relink` points it
  there again afterwards, by its place in the page's `/Annots`, which pdfium
  keeps. Only a link to a page that did not come along is lost, and said.
- **One import call per new file.** pdfium shares fonts and pictures only
  within a call, so a call per page or per run copies them once each.
- New files are written with `atomic_write`, never through `write_step`.
  File dialogs go through the `Choose` door.
- **Split at each chapter** (the organiser's Split menu, `Viewer::chapters`,
  `arrange::ends_there`, `Organiser::split_at`) starts a new file at each
  chapter, and the reader sees the cuts before anything is written.
  - The chapters are the headings at the top level of the contents. Where
    the top level has only one heading, usually the document's title, the
    headings under it are the chapters. Two chapters that begin on the
    same page are one, under the first one's name. The pages before the
    first chapter are "Front matter".
  - A chapter runs to the page before the next one begins. It also takes
    that page when there is text above the next chapter's heading there.
    A line does not count if one of the two pages before or after it has
    the same words at the same height (within 2pt), comparing without
    digits or spaces, since running heads and page numbers repeat. A line
    with only a page number never counts.
  - A page where the heading's place is not known yet goes into both
    chapters, since a page saved twice loses nothing and a page left out
    does. On a page with no text, such as a scan, the page goes into both
    when the heading is more than 15% of the way down.
  - A page in both chapters is in the draft twice. The text is read off
    the window's thread, and the cuts appear when it has been read.
- **A new file is slimmed by lopdf** (`arrange::slim_pages`). Many producers give
  every page one resource dictionary naming every font and picture, and
  pdfium copies a page with all of it: four pages of a 68-page paper came
  out at 4.4MB, and slimmed they are 250KB. Each page keeps only the names
  its content uses, and objects nothing refers to are dropped. Anything
  unsure is left as pdfium wrote it: a page whose content does not decode,
  or that draws a form, a Type3 font or an annotation appearance with no
  resources of its own, keeps its dictionary,
  and a file lopdf cannot read, or that comes out no smaller, is written as
  it was. Tests check that each page is drawn exactly as before. It runs
  outside pdfium's lock and holds the new file in memory for the length of
  the save. lopdf cost the release binary 485KB.
- **Every page edit and new file is compacted** (`objects::compact`):
  unreferenced objects dropped (pdfium keeps a removed page's), streams
  compressed, objects packed into object streams, and the result read back
  and compared before it is used. Pages inserted from another file are
  slimmed too; the document's own pages never are, since a wrong decision
  there would be written into the reader's file. Highlights are not
  compacted, so marking stays quick.

**What lopdf does.** pdfium reads and draws but writes little; lopdf can
change any object in the file, and does so after pdfium has saved:
- *The outline* after page edits, and "split by chapter" (`outline.rs`).
- *`/PageLabels`*, kept with their pages and typed by the reader
  (`labels.rs`).
- *Links between copied pages*, pointed at their pages again
  (`outline::relink`).
- *Incremental updates* for signed documents (`objects::incremental`), by
  comparing pdfium's save with the file. Marking a large file is no
  faster, since pdfium still loads and saves all of it; an annotation
  written by lopdf alone would be.
- *Smaller files*: page edits and new files.
- *The document's details*: Title, Author and Subject in the Information
  window write `/Info` (`objects::write_details`). XMP is left alone.
- *A PNG made into a page* for the organiser's "Add files…" (`arrange::picture_pdf`),
  decoded in the other process (`objects::picture`). A JPEG goes in through
  pdfium as it is, and its EXIF orientation is the image's matrix, so it is
  never decoded or encoded again.
- lopdf is less forgiving than pdfium with a broken file, so every path
  through it falls back to pdfium's bytes (`objects::rework`). A result is
  read back before it is used: every object must be as lopdf wrote it, the
  page count as it was, and no more references may lead to objects that
  are not there (`objects::Shape`). That catches a bad write, not a wrong
  decision, such as a resource slimmed away; those are the tests' job.
- **lopdf runs in a process of its own.** The memory it uses stays with
  the process that used it once freed, and handing it back to the system
  frees none of it: a 14MB book left the app 240MB larger. So the app runs
  its own executable with `objects::APART`, sends it an `objects::Work`
  (plain data: what to slim, which labels to follow, which links to point
  where) and the document over a pipe, and reads the result back. Anything
  new lopdf does goes into `Work`, never into a closure. Where the helper
  cannot be started, and under `cargo test`, the work is done in the app's
  own process, and only up to `objects::LIMIT` (10MB), except for the
  incremental update of a signed document, which has no other way to be
  written. `tests/apart.rs`
  drives the real binary.

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
- **Tab moves the focus from button to button** (Blitz does that), and the
  shell answers the rest before the document hears the key
  (`app::keys_on_controls`): Enter or Space on a focused button is a press
  and release over its middle, and ↑/↓ move through an open menu. The
  harness asks the same function, so tests see what the window does. The
  outline is `button:focus`: Blitz never matches `:focus-visible`.
- A plain key typed in a text field is also a shortcut unless the field stops
  it: the page, find and password fields' `onkeydown` handles its own Enter
  and Escape and hands the rest to `field_keeps`; the comment field keeps
  every plain key and Enter itself, since a newline there is typing.

## Things that will bite

**`use_effect`'s closure is replaced on every render, so nothing it captures
survives one.** A `let mut` captured by `move` compares against its initial
value for ever; three such effects once cost a render and full paint per frame
— 100% of a core, idle, with identical frames. What an effect remembers goes in
a `use_hook` (`Cell`/`RefCell`) beside it. `tests/settle.rs` asserts
`stats::RENDERS` stops climbing.

**A component that reads `Signal<Viewer>` renders on every scroll frame.**
`Reader` does, and that is the one it can afford. The toolbar reads `Bar`, a
`use_memo` of what it shows, and renders only when that changes;
`tests/frames.rs` counts `stats::TOOLBAR_RENDERS` across a scroll.

**A press lands on a custom widget; a click never comes out of one.** Blitz
delivers the press to the `object` and makes no `click`. `pointer-events: none`
on the widget lets the click reach the button around it.

**Blitz hit-testing.** A z-indexed child is only hit-tested inside its stacking
context's union, and `z-index: 0` is not a layer. The union is measured before the
frame's layout, so a box that has just appeared is 0×0 in it — `.hit-layer`
keeps `.body`'s union the whole window. Blitz also shrinks a flex
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
A press that stops at the find card, the Results tab or the results panel
does not reach the root's `onmousedown`, which closes it. "Match
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
- Wait for the condition (`settle()`), never the clock. The reader's timers
  count on a clock the harness stops (`emit::now`, `emit::after`), so a test
  moves it with `Reader::later` instead of sleeping.
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

- **The toolbar is the menu bar, on every platform.** There is no native menu
  bar anywhere: Open, recents, Print, Settings and the shortcut list are the
  toolbar's, and Copy is the selection popover's. macOS keeps the application
  menu winit installs (Hide, Quit) because that menu is the system's furniture;
  it has no Edit or Window menu, so ⌘W and ⌘C are the keymap's.
- **pdfium is a shared library, not in the binary or the repo.**
  `library_dir()` in `pdfium.rs` checks `Contents/Frameworks` (.app),
  `/usr/lib/Moonowl` (.deb) and the executable's directory (.msi), and only
  then `MOONOWL_PDFIUM` — an installed app never takes a library from the
  environment. For bundling it lives in `pdfium/`.
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
cargo about generate --locked about.hbs -o licenses/rust-crates.txt
cargo packager --release           # the installers, into target/release
```

There is deliberately no `cargo fmt --check`: the keymap is one row per action
so that it can be read down, and rustfmt explodes it.

## Writing

Comments, theme files and text in the interface are written in plain,
natural English, in whole sentences. No creative metaphors ("the theme as the
toolbar wears it") and no clipped phrasing ("Absent means the background").
Write "If the theme does not set it, the toolbar uses the background colour."
Much of the existing code does not follow this yet; do not copy its style.

## Git

Make a separate commit for every fix or new feature. Before committing, run
`cargo fmt`.

## CI and releasing

`checks.yml` (the suite, three platforms) and `bundle.yml` (installers) are
*reusable*, because a push and a release both need them. `ci.yml` runs checks
on PRs; `nightly.yml` runs them on a push to main, beside a build into a
`nightly-next` draft, and swaps that in for the rolling `nightly` release once
every bundle is there and the checks have passed;
`release.yml` is the only thing that names a version and is
`workflow_dispatch` only.

**Everything downloaded is pinned.** Actions by commit SHA (the version in a
comment beside it), `cargo-packager` and `cargo-about` by version, and pdfium by tag *and*
checksum: `scripts/pdfium.sha256` holds the archives' SHA-256 sums, checked
before unpacking. The tools cargo-packager makes the AppImage with are pinned
the same way, in `scripts/appimage.sha256`, and `scripts/appimage.sh` puts
them in its cache before it runs, so it does not fetch its own. Moving pdfium means the tag in `checks.yml`, `bundle.yml`,
`scripts/pdfium.sh` and `scripts/pdfium.ps1`, and new sums.

**To release:** main green → Actions → Release → Run workflow, branch `main`,
version `0.1.0` (three numbers, no `v`). The run does checks, then `tag`
(writes the version into `Cargo.toml` and `Cargo.lock`, commits, tags, opens a
*draft*), four bundles in parallel, then publishes — so nobody downloads half a
set. Dispatching the version the tree is already at is fine.

- *A bundle failed:* dispatch the same version again; it reuses the tag and
  the draft, as long as main has not moved since. Only a published release refuses.
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
`libpdfium.dylib` — hence `disable-library-validation` in `entitlements.plist`.
That is for the ad-hoc signature only: with a real certificate the bundler
signs the library under the same team, and the entitlement should then go
(the comment in `Cargo.toml` says so). Nothing is certificate-signed
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
a row there and a licence text that ships. The Rust crates' notices are
generated for every bundle by `cargo about` into `licenses/rust-crates.txt`
(`about.toml`, `about.hbs`); a crate under a licence `about.toml` does not
accept fails the bundle.
