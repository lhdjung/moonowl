# Audit, 30 September 2026

**Status, 8 October: done.** Every item was fixed on 1 October (`3d1c860` to
`6965f16`), except UI 5, invisible keyboard focus, which is still open and is
question 6 of `../ux-review.md`. The naming question under "Outside the code"
is settled (`legal.md`).

A read of the whole tree at commit `1658b90` (Blitz and Parley follow upstream
main), against the brief and the rules in `AGENTS.md`. `cargo clippy
--all-targets -- -D warnings` is clean and every test binary passes (33
binaries, one test ignored). Every finding below was checked against the code
by hand; nothing was run in a window, so anything about how something looks
on screen is arithmetic or geometry read off the stylesheet, not a screenshot.
Nothing was edited.

Ranked most severe first within each section. Each item names where to look,
what goes wrong, and how sure the reading is.

## Bugs

### 1. Undo can delete its own safety copy

`src/app.rs:7321` (`adopt`, the `Err` arm) with `src/app.rs:5849`
(`step_back`) and `src/markup.rs:577` (`Drop for Before`).

A highlight write goes: `write_step` → the file is rewritten on disk → the
step with its `Before` copy is pushed → the document is reopened. If that
reopen fails, `adopt` retakes the *old* handle, whose stamp was captured when
it was opened, before the write. ⌘Z then goes through `write_file`, which
compares the stamp on disk (post-write) against that stale stamp, answers
"The document changed on disk", and the error arm calls `forget_steps`. That
drops `Before`, whose `Drop` deletes the pre-write copy from the config
directory. The document on disk is the output pdfium could not reopen, and
the only good copy is gone.

Rare, because pdfium's own `SaveAsCopy` output normally reopens. It is
exactly the case undo exists for. Either the stamp check should accept the
stamp of a file this reader itself just wrote, or the `Err` arm of `adopt`
should keep the steps. Confidence: medium that it can be reached, high on
what happens when it is.

### 2. A launch during a quit runs as a second, lockless instance

`src/single.rs:76` (`claim`), `:113` (`handed_to`), `:176` (`serve`).

⌘Q sets `CLOSING` at `on_quit`, but the socket stays bound until `release()`
in `farewell`. A launch in that window: `try_lock` fails because the holder
still holds; `handed_to` connects at once; `serve` sees `CLOSING` and
`continue`s, dropping the stream unanswered; the client's `read_to_string`
returns EOF immediately and `handed_to` returns `false`. The 50ms sleep is
only on the failed-connect path, so the five rounds of `QUIT_TRIES` finish in
microseconds and `claim` returns `Claim::Alone`. That process holds no lock
and serves no socket. After the old one exits, the next launch takes the lock,
and there are two readers with two settings tables for good.

The doc comment at `:72` ("asked again, for as long as a quit can take")
describes behaviour the code only has once the socket file is gone. Fix:
sleep before retrying when the door answered with silence, or have `serve`
answer a "closing" word the client waits on. Confidence: high on the logic;
the window is tens of milliseconds in the usual case, longer when a quit is
waiting on a write.

### 3. Trimmed margins cut off print when a margin is wider than 30%

`src/crop.rs:189` (`refine`).

Width and height are computed from the *unclamped* origin, then the origin
is clamped to `MAX`, and the far edge moves inward by the same amount. Ink
starting at 45% across on every sampled page: `x = 0.438`, `width = 0.524`;
`x` is clamped to 0.3, `width` stays 0.524, so the crop's right edge is at
0.824 while the ink runs to 0.95. Nothing lets the reader pan to the rest:
pdfium draws only the window. The same on the vertical axis for slides with a
blank upper band.

`no_more_than_a_share_of_any_side_comes_off` passes only because its ink is a
10% square. Fix: recompute from the far edge after clamping,
`crop.width = (right + PAD).min(1.0) - crop.x`, and height likewise.
Confidence: high on the arithmetic; how often eight samples all have a
30%-plus margin on one side is the open question.

### 4. A typed signature line is clipped to one glyph on rotated pages

`src/sign.rs:776` (`text_one`).

`object.width()` is asked *after* `rotate_counter_clockwise_degrees`.
pdfium-render's `width()` is the x extent of the rotated bounds, which for a
quarter-turned line is the text's height, not its length. That number becomes
the annotation's `/Rect`, and `FPDFAnnot_AppendObject` builds the appearance
`/BBox` from `/Rect` on the first append. Every viewer clips the date or
name. Unrotated pages are fine. Fix: measure before rotating, or take
`height()` when turned. Confidence: high.

### 5. A recompile loses every comment, and "Put N passages back" says nothing

`src/app.rs:4983` (`restore_markup`), `src/app.rs:5053`, `src/library.rs:70`
(`Highlight`).

A comment lives only in the highlight's `/Contents`, by design. The journal
carries colour, quote and quads, so a rebuilt document has nothing to restore
a comment from, and `markup::add` writes plain marks. The notice counts
passages, not comments. At minimum the notice should say the comments are
gone; better, the journal carries the note beside the quote. Confidence:
high.

### 6. The Contents chip misbehaves after a search has borrowed the panel

`src/app.rs:9841` (the chip's `onclick`), with `src/app.rs:6297`
(`close_find`/`put_find_away`) and `src/app.rs:2295` (`toggle_sidebar`).

⌘F, matches arrive, `show_the_matches` opens the panel with
`results_borrowed = true`, and the chip lights. Clicking it to shut the panel
runs `close_find()`, which closes the borrowed panel, and then
`toggle_sidebar()`, which finds it closed, reopens it, and writes
`show_sidebar = true`. The panel stays up on the previous tab, and a setting
the reader never chose is on disk. ⌘B (`Action::Sidebar`) takes the other
path and closes it correctly. Confidence: high.

### 7. A right-click on the document ends the search

`src/app.rs:3018` (`open_context`) → `src/app.rs:2614` (`show_menu`).

`show_menu` unconditionally puts the find bar away and clears the index,
which its own comment justifies for menus that share the toolbar's corner.
The context menu is at the pointer and one of its rows is "Find …".
Right-clicking a match to copy it takes the bar down, drops every highlight,
closes a borrowed results panel, and reflows the page under the pointer
before the menu opens. Confidence: high that it happens, medium that it is
unintended.

### 8. "Recolour pictures too" is not sent to other windows

`src/app.rs:2868` (`set_recolor_images`).

It goes through `Store::set`, which has no `tell`; only `wear`,
`set_ui_scale` and `keys_reloaded` broadcast. Each window's palette is
refreshed only by `theme-worn` or `themes-changed`, so the second window
keeps drawing pictures the old way until the next theme event. Against the
"every window shares one settings table" rule for this one palette input.
Confidence: high.

### 9. A shipped theme with a typo swaps to the embedded copy silently

`src/theme.rs:205` (`load_all`), and `build.rs:105` (`check`).

A custom theme that fails to parse is listed under problems on the Settings
page. A shipped one falls back to the embedded copy through `or_else` and
nothing is said. Edit `nord.toml` in place while the app runs, make a typo,
and the watcher wears the shipped colours without a word. The banner warns
about overwrite at launch, not about this.

Same class at build time: `build.rs` checks name, six colour fields and
`order`, but never deserialises the file into `Theme`. `recolor = "false"`
(a string) builds, and at runtime `parse` returns `None` and the theme is
dropped. Only `tests/palette::the_shipped_themes_all_resolve` catches it,
contrary to "a theme that does not parse is a build failure". Confidence:
high, low severity.

### 10. The Settings "Fixed zoom" stepper is live

`src/prefs.rs:461`.

`Stepper` is `live: true` by default, and its own doc comment at `:234`
names zoom as the case that must be off: typing "150" passes through 1 and
15, each clamped to 25% and the whole document relaid at it. The toolbar
zoom field at `src/app.rs:10447` passes `live: false`; this one was missed.
Confidence: high.

### 11. The reopen-list write is the one library write still swallowed

`src/session.rs:298` (`set_open`).

`let _ =` on the result. With a broken `library.toml`, marks and places
report "not saved until fixed" through `disk-refused`, but `library.open`
quietly stops being written, and the next launch reopens whatever the list
last said. Confidence: high.

### 12. Every settings write rewrites the whole file

`src/settings.rs:243` (`write`).

The file is read into a `toml::Table`, the named keys are set, and the table
is serialised back alphabetically with only the banner comment. The header
says "yours to edit too", but any comment the reader wrote is gone at the
next zoom pinch. No setting is lost. Confidence: high.

### 13. Adrift highlight rows hand a raw colour string to CSS

`src/sidebar.rs:498` with `src/app.rs:5131` (`build_markup_rows`).

Rows for marks held beside the document put `held.color` straight from
`library.toml` into `style: "background: {colour};"`. `Highlight.color`'s
own doc comment (`src/library.rs:81`) says it never reaches CSS without the
parser. A hand-edited or foreign value draws a dot the page renderer cannot
read. Route through the same parser the in-file rows use. Confidence: high.

### 14. "N of N" on every scroll position when the content barely overflows

`src/layout.rs:640` (`page_at`).

`max > 0.0 && scroll_top >= max - 1.0`: for `0 < max_scroll < 1` the right
side is negative, and every position reports the last page. Two-up at a
small fixed zoom, or a tall page plus a short one at fit-page, can produce
it. Guard with `max >= 1.0`. Confidence: high, very low reach.

### 15. The software render path copies pages and recolours under the lock

`src/page.rs:910`, `:915`, `:997`.

`bitmap.bgra.to_vec()` is the exact copy `AGENTS.md` names; `rgba.clone()`
makes a second full page whenever the page has links; and every selection
change clones the whole page again. All of it runs inside `Document::render`'s
callback, holding pdfium's process lock and the document lock, and
synchronously in `paint`. Confined to the vello_cpu fallback and the harness,
but on hardware that falls to it, scrolling a paper with links allocates two
page-sized buffers per mounted page per draw, and macOS keeps the freed
blocks. Confidence: high on what the code does.

### 16. Every keydown is a full render, bare modifiers included

`src/app.rs:8305` and `:8325` (`on_key`).

`pending_at.elapsed() > SEQUENCE_LASTS` is true on nearly every press after
1.2s idle, and `viewer.write().pending.clear()` runs even when `pending` is
empty. `chords_of` returns nothing for a bare Shift, Ctrl, Alt or Meta, so
`Press::Wait("")` and `Press::Nothing` each write too. Pressing ⌘ on the way
to ⌘F rebuilds comment cards, selection areas (text extraction) and markup
rows. The file's own rule: a `write` is a render whether it changes anything
or not. Read first, write only when `pending` changes. Confidence: high;
severity low.

### 17. Disk work on the thread that draws

`src/app.rs:4878` (`read_markup`, from `Viewer::new` at `:2096` and
`take_up` at `:7603`).

`MarkupRead::of` writes a probe file beside the document and extracts the
text under every mark, and its own doc comment at `:1402` says it belongs on
a thread. Only the write/reload path (`offload`) does it off-thread; ⌘O and
the first open do it on the UI thread. On a slow or syncing volume with a
heavily annotated paper, the window stalls. Same class, smaller:
`export_theme` (`:3866`, `atomic_write`), `import_theme` (`:3835`,
`read_to_string`), `renamed_elsewhere` (`:7064`, `settings::load`).
Confidence: high on the path; ⌘O already stalls for page sizes.

### 18. Smaller

- `src/watch.rs:352` (`whole`). `%PDF-` must be at byte 0 and `%%EOF` within
  the last 1KB, stricter than pdfium and the spec. Such a document is never
  believed whole, so it never auto-reloads, and an own write's `absorb` does
  the same. Medium confidence on how often it matters.
- `src/markup.rs:446` (`recolour`). Dates are carried over only when
  `read_date` parses them, and it returns `None` for a date without a zone
  (`D:20240101120000`, which many producers write). Recolouring drops both
  dates; `/Subj`, `/CA` and `/F` are not carried either.
- `src/markup.rs:168` (`folder_takes_a_file`). The probe file is named per
  process, but `standing()` runs per window on concurrent threads. Two
  documents in one folder reloading together: the second `create_new` fails
  with EEXIST, and the write is refused with "File exists" until the next
  reload. A per-call counter in the name fixes it.
- `src/markup.rs:378` (`set_note` with an empty string). Taking a comment off
  writes `/Contents ()` rather than removing the key; Preview and Acrobat
  show the mark as having an empty note.
- `src/app.rs:12378` (`find_quote`). Plain substring search, nearest page
  first, no word boundary. A short quote lands on the first occurrence, even
  inside a longer word. Acknowledged in the doc comment; a whole-word
  preference and a nearest-to-old-y tie-break would cut most misses.
- `src/layout.rs:666` (`anchor`) with `:698` (`scroll_target`). A position
  inside the band `(next.top - next.above - 0.5, next.top]` but below
  `top - above` is called "next page, offset 0", which `scroll_target` puts
  at `top - above`. So a position sitting exactly on a page top moves up by
  a gap plus `PAD_Y` on every relayout: resize, sidebar toggle, zoom step,
  crop. Bounded; keep the raw negative offset instead of clamping to 0.
- `src/layout.rs:496` with `src/app.rs:9230`. Page tops are f64 and are
  handed to the DOM as `top: {top}px`, which Stylo and Taffy store as f32.
  Past 2^23 px of content (about 12,800 letter pages at fit-width 1000px,
  2,600 at 600%) painted boxes are off by up to a pixel while hit-testing
  uses the f64 box. Medium confidence, not run.
- `src/library.rs:251` (`remember`). Returns `Ok(())` when the document has
  no entry, so a lost entry drops the reader's place silently for the rest
  of the session. Reachable on Windows (one process per launch) with 24-plus
  documents. Low confidence on exposure.

## UI

### 1. `--note` has no readability floor

`src/palette.rs:184` (`note`).

A flat 0.28 mix of text into background, unlike `muted()` and `faint()`,
which walk back until they reach 4.5:1. On the worst of background, surface
and ground: Solarized Light 2.2:1, Solarized Dark 2.6:1, Sepia 3.8, Tokyo
Night 4.1, Moonowl Light 4.3. `--note` is the ink of `.pane-lede`,
`.pane-note`, `.field-note`, `.details-value`, `.menu-row-note`,
`.start-sub` and `.menu-key`: every explanatory sentence in Settings, the
chords in menus, and the shelf's page numbers. Give it the same `readable`
walk the others have. Confidence: high (arithmetic).

### 2. Accent on tint drops under 3:1 on three shipped themes

`src/palette.rs:191` (`accent_soft`), used by `.chip.on`,
`.toolbar .chip.on`, `.tab.on`, `.nav-item.on`, `.segment.on`,
`.outline-item.current`, `.result.current`, `.find-option.on`
(`src/styles.rs:321`, `330`, `968`, `1703`, `1868`, `993`, `1016`, `657`).

`--accent` on `--accent-soft` or `--bar-accent`: Solarized Dark 2.7:1,
Solarized Light 2.9 and 3.0, Glamour 3.0; most others 3.4 to 4.0; only High
Contrast, Rosé Pine and Tokyo Night pass 4.5. The selected tab, nav row and
outline row are 12.5 to 13.5px text. `accent_soft` has no contrast loop.
Confidence: high on the numbers; whether to accept the tint trade-off is a
design call.

### 3. The colour picker is clipped, and unclickable, under the last fields

`src/styles.rs:1778` (`.color-picker`), `src/prefs.rs:826` (the "Selection
area" and "Selected text" fields), `src/prefs.rs:1185`.

The picker is absolute at `top: 32px` and about 300px tall (square, strip,
five rows of swatches), inside `.window-pane`, which is `overflow-y: scroll`.
After the last two colour fields only a switch and the actions follow, about
140px. `src/styles.rs:1733` already records that Blitz does not count
overflow into the scroll extent, and `AGENTS.md` says Blitz does not
hit-test what overflows a parent. So the swatch grid under those two fields
hangs below the pane, clipped and dead. Open it upward when there is no room
below, or give the pane bottom padding the picker's height. Confidence:
medium (geometry from the sheet, not run).

### 4. A long comment cannot be read to the end

`src/styles.rs:1541` (`.note-window`), `src/app.rs:11220`.

`height: auto; max-height: 70%`, and neither `.note-body` nor `.window`
sets overflow; the whole text is one `<p>`. Past 70% of the window the tail
paints outside the frame or is cut with no way to scroll. `.sign-body` at
`:1574` scrolls. Confidence: medium-high.

### 5. Keyboard focus is invisible outside three fields

`src/styles.rs:670` (`.find-field:focus`), `:784` (`.note-card-field:focus`),
`src/prefs.rs:184` (switches).

Page, text and step fields remove the outline and put a border or background
back; the find field and the comment field remove it and put nothing back.
No button, switch, segment or theme card has a `:focus` rule. Only `.root`
has `tabindex`, so Tab reaches fields only, and the first thing it lands on,
the find field, shows nothing. Confidence: high on the CSS.

### 6. The pointer cursor is on a dozen controls and not the rest

`src/styles.rs:210` (`.root { cursor: default }`), against `:608`
(`.of.choice`), `:1114` and `:1122` (`.markup-more`, `.markup-close`,
`.markup-copy`), `:1464` (`.back-go`, `.back-close`), `:1759`
(`.color-swatch`), `:1795` (`.color-choice`), `:766` (`.note-card`), `:737`
(`.note-spot`), `:728` (`.link`), `:446`.

Those say `pointer`; every chip, menu item, tab, switch, segment, theme card
and outline row is an arrow. A reader gets a hand over "of 400" and an arrow
over "Open…". Pick one rule. Confidence: high.

### 7. Two menus use `100vh` after the sheet explains why `.root` does not

`src/styles.rs:390` (`.menu.theme, .menu.settings { max-height:
calc(100vh - 62px) }`), against the comment at `:123`.

Stylo did not re-evaluate viewport units after a move to a screen of another
density; the same fault would cut the theme list after such a move. A
percentage of the window, or the height handed down from the shell, does not
have it. Confidence: medium.

### 8. Tooltips never render, and folded chips have no name

`src/app.rs:10343`, `:10350`, `:12241` (the only `title` attributes),
`src/styles.rs:292` (`.chip-label` folding under 1200px).

Blitz draws no native tooltip, so the three that exist are inert. Under
1200px Contents, Open, Close, Search, Theme and Settings fold to bare symbols
with no hover text and no `aria-label` (only the always-icon chips have
one). Folding is by design; the missing name is not. Confidence: high.

### 9. Smaller

- `src/app.rs:10750`. "Only when the toolbar is hidden." Everywhere else the
  reader sees "menu bar" (`src/prefs.rs:503`, `509`, `1562`, `1672`;
  `src/app.rs:2509`, `10642`, `11132`; `src/keymap.rs:412`).
- `src/app.rs:10094` (`.chip.title`) and `:10385` (`.chip.fit`) set `color`
  inline. Inline beats `.chip:hover { color: var(--text) }`, so those two are
  the only chips whose text does not brighten on hover, and
  `.chip.title:hover` at `src/styles.rs:549` is dead.
- Menu ticks are the "✓" glyph in seventeen `.menu-tick`s
  (`src/app.rs:10286` onward) and the close buttons are "×"
  (`src/sidebar.rs:454`, `535`; `src/app.rs:12252`), while `src/icons.rs`
  has `check` and `close`. A glyph comes from whatever font the fallback
  lands on and does not match the checkbox tick or the 1.7px stroke beside
  it.
- `src/styles.rs:1119` (`.markup-close`). `--negative` is picked by
  `dark()` with no contrast check (`src/palette.rs:229`): Glamour 2.8:1,
  Nord 2.9, Dracula 3.2 on `--bar-sunk`. The one destructive control in the
  popover.
- `src/styles.rs:1740` and `:1804`. `.chip.action` is declared twice; the
  first is shadowed, and the second sets `display: block`, which would turn
  any future icon-plus-label action chip into a non-flex box.
- `src/app.rs:9846`, `:10345`, `:10352`. "Contents" opens a panel whose tabs
  are Contents, Pages, Results; "Left" and "Right" name a rotation as a
  direction, and beside "Search" read as pan or previous/next. The
  placement is settled; this is about the words.
- `src/styles.rs:651` (`.find-option`). 22px tall, 12.5px type in `--faint`,
  and it carries three persistent settings; every other chip is 26 to 30px.

## Checked and found sound

So that nobody re-audits it:

- The three `use_effect`s in `app.rs` keep their memory in `use_hook` cells.
- Page arithmetic (`page()-1`, `at-1`, `hit.page-1`) is guarded by
  `empty()` or returns page 1 on an empty layout; `label()` uses
  `wrapping_sub`. No panic path on user data was found.
- The two binary searches, `mounted`, `page_at` and `anchor` against
  paged-mode holes, spreads and cover rows.
- `place_on`, `unplace_on` and `Crop::turned` for 90, 180, 270 and crop
  offsets; pdfium-render's rotate plus target-size plus origin path,
  including square pages; zero-size pages are guarded.
- Lock ordering: library then document everywhere; `config::hold` takes the
  mutex then the file lock; settings and library locks are never nested.
- The scribe: `Forget` precedes the `Now` write in `Store::set`; `flush()`
  runs at exit and before `touch` at open; a quit closes every window
  through `tidy` after `leaving()`, so the reopen list is kept.
- `library.toml` key order: `open` before `file`, `marks` and `highlights`
  last in an entry.
- Every key read or written in `src` is in `settings::defaults()`, and
  nothing writes another setting beyond the documented groups.
- All fifteen theme files: six-digit hex only, every field readable, `order`
  1 to 15 unique, none contradicts the brief. Dracula's text is Dracula's own
  off-white, which the brief allows. The hex parser accepts exactly the four
  forms and refuses the rest.
- `keys.toml`: 53 of 53 action names exist in `keymap.rs`, and the reverse.
- GPU and CPU recolouring agree step for step, including rounded luma, the
  white point, HSL room, half-to-even and the unclamped ramp ends.
- `select.rs` index bounds in caret, word, line, sentence and paragraph;
  `fold` and `origin` mapping including ligature ends, dotted İ, whole-word
  on folded text, `MATCH_LIMIT`, step wrap, empty and soft-hyphen-only
  queries.
- QuadPoints corner order; `Space` up and down for all four turns.
- The theme editor never writes a built-in in place; every swatch and card
  goes through the parser.
- Texture release on unmount; the detail widget's part lifecycle;
  `render_owned` on the GPU path lends nothing and copies nothing.
- The Escape chain, the field key handlers, the innermost-asker rule for the
  keyboard, menu toggle on its own button.
- Dock, tabs, print and shell ObjC: `setDockMenu:` exists, `install` runs
  once on the main thread, the print sheet returns after presenting, tab
  window access is guarded, Quit and CloseOne reuse the close path.
- No uppercase, small caps, italics, transitions or animations anywhere in
  the stylesheet. Every scroller has its own box except the note window
  (UI 4). The toolbar is 46px and everything else floats, so the document
  keeps the vertical axis. `ui_scale` is a Blitz zoom, so fixed pixel sizes
  scale with it.

## Outside the code

`legal.md` is untracked and records a registered German word mark HYLO in
class 9, expressly covering "Computersoftware", held by Ursapharm. If a
rename to "HyloPDF" is under consideration, that analysis argues against it
for Germany. `todo.md` has three open decisions on the comment feature (the
right-click recolour item, where the menu opens, merging the selection and
highlight menus) that are not repeated here.
