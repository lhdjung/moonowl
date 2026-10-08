# UX review, 3–4 October 2026

A UX review of the whole app, and the fixes made from it. Five reviewers
read the code by area (toolbar and keys, Settings and themes, reading and
navigation, markup and messages) and one took 38 headless screenshots. Fixes
are 33 commits on `dev`, `9d2cd3e` to `619de0f` (pushed), and a second round
from checking them against the code (section 3, "Follow-up", `b35a555` to `1751de7`; not pushed).
`cargo clippy --all-targets -- -D warnings` is clean and `cargo test` passes in
full. Nothing below was tried in a real window.

**Status, 8 October (`ec8e955`).** Sections 2 and 6 are marked item by item:
*done* with its commit, or *partly*. Unmarked items are still open. Since the
review the bar's colours were redone (`887ed82` to `5dbfbda`), so the look
items in section 1 want a fresh look.

Screenshots from the review were in the session's scratchpad, which is
temporary. Regenerate with a throwaway `tests/zz_*.rs` that calls
`Reader::save_png`.

---

## 1. Check in the real app first

The harness cannot see these. Each one is a guess until someone reads with the
app.

- **Full screen and presenting come back at launch** (`f59926e`). The launch
  window asks for full screen the first time it reports a size
  (`Viewer::window_full`, `restoring_full`). On macOS, check that this first
  report comes after the window is on screen. If it comes before, the ask may
  be ignored. Also check that quitting while presenting brings presenting back,
  and that the way out is still obvious.
- **The current row of Contents/Results scrolls into view** (`cd563a3`).
  `sidebar::reveal_rows` runs in the shell after each event, beside
  `place_carets`. Check that it scrolls without waiting for a second event,
  and that a list you scrolled by hand stays where you left it.
- **Search never waits for pdfium** (`66444e6`). A slice reads no page while
  the renderer holds pdfium's lock. Check search on a large scan while
  scrolling: it should not stall the window. It may lag the rendering, and the
  scan loop wakes every frame until the lock is free, so watch CPU. Since
  `d972b88` the markup walk after open takes the lock a page at a time too.
- **Toolbar between 1200 and 1400px, with SF Pro** (`838a273`). The left
  group folds to symbols below 1400px. That width was measured with DejaVu
  (the harness font), which runs about 10% wider than SF Pro, so on a Mac it
  may fold sooner than needed. Check the name's fade at the edge.
- **Zoom around the middle** (`3777b1e`). ⌘+ at the top of a document now
  scrolls a little, so the first line can move up and out of view. Check
  whether that feels right.
- **Escape now keeps a comment** (`3a05b8b`). It saves what was typed, and ⌘Z
  takes it back. Escape on a new selection's comment makes a highlight in a
  random colour. See whether that feels right in use.
- **A comment while a write is under way.** The field stays up and the notice
  says "Still writing…", and it stays up after the write lands. You have to
  press Done again; nothing retries on its own.
- **Look and feel:** the off switch knob (`9d2cd3e`), the quiet "Default" tags
  (`47bed13`), dark button text on both Solarized themes (`86ead64`),
  Solarized Light's darker text (`149158b`), the Presenting switch in the
  Settings menu (`bedde1e`).

## 2. Questions waiting on you

1. **Author name on comments.** Currently `AUTHOR = ""` (`app.rs`), so cards
   say "Unknown author". The name can be read from the system:
   - macOS: `NSFullUserName`, which is what Preview uses.
   - Linux: the name stored with the user account (in `/etc/passwd`).
   - Windows: the login name.

   It would then be written into every PDF you annotate and share.
   *Proposal:* a "Your name" field in Settings, filled in from the account, and
   cards that show only the date when no name is set.
2. **"Beside the document"** is the sidebar's label for a highlight Moonowl
   could not write into the PDF (read-only, encrypted, over 100MB, or a refused
   write). It lives only in `library.toml`, and other apps don't see it.
   *Proposal:* "Only in Moonowl". Also "Put N passages back" → "Restore N
   highlights".
3. **Full stops on notices.** Some end with one ("There is no text on this page
   to select.") and some don't ("Bookmarked page 3", "No matches", "Nowhere
   further back", "There is no page X in this document"). *Proposal:* every
   full sentence gets one.
4. **"None" in the find bar** means no matches. It is the same grey as "3 of
   12", and the arrows still look usable. *Proposal:* "No matches", with the
   arrows dimmed.
5. *Done (`0bea79e`): folder, tab, window, new tab, new window.* **Open menu
   icons.** Four of the five items share the window icon. Choose one:
   - **A:** draw a "tab" icon (a window with a raised tab); window for the
     window items, tab for the tab items.
   - **B:** the folder icon for the two "Open document in…" items (they open
     the picker), and window/tab for the two "New…" items.
6. **Keyboard focus** (also the audit's UI 5). Tab moves through controls with nothing showing which
   one has the keyboard: there is no `:focus` style outside text fields. Menus
   also don't take ↑/↓/Enter. *Proposal:* one `:focus-visible` outline for
   buttons, and arrow keys in `.menu`.

## 3. What changed

### Top list
| Commit | Change |
|---|---|
| `9d2cd3e` | An off switch shows its knob at the start of the track. It was centred because Blitz centres a button's content. |
| `3a05b8b` | A comment is never lost. It stays in the field while a write is busy, and Escape saves it. `cancel_comment` is gone. |
| `1cdc671` | Highlights a rebuild lost are not drawn at their old places, and the reload says how many were lost. New `Highlight.lost` flag in `library.toml`. |
| `dbccd57` | A reload, or reopening the find bar, searches again without moving the view, and keeps the match you were on (`find_again`, `Search::prefer`) — on a reopen, only while you are still on its page. |
| `c3f1b0a` | Home/End/`g g`/`G` and a search's first move are now in the back history. The link's "Back to page N" chip goes away after use or after another jump (`note_jump`). |
| `b1dfab4` | Enter in the theme editor only takes what was typed. It no longer saves the theme or closes Settings. |
| `44f80bb` | Write errors are plain sentences, and pdfium's own words go to stderr (`plainly`). Refusals start with a capital ("This document is read-only, so…"). The `library.toml` and `settings.toml` complaints are full sentences. |
| `838a273` | The document name fades wherever it is cut (padding plus mask on `.title-name`), and the left chips fold first below 1400px. |
| `983e8a6` | Signing, taking a signature off, and "Put back" are undo steps. `Viewer::write` is gone; everything goes through `write_step`. AGENTS.md updated. |

### Settings and themes
| Commit | Change |
|---|---|
| `bbc6f59` | The Selection area note gives the real default: a wash of the accent. |
| `8b428f5` | An "Interface size" stepper on the Window page. |
| `f59926e` | Full screen and presenting persist, for the launch window only. New settings `full_screen` and `presenting`. AGENTS.md updated. |
| `8d78bb8` | "Recolour pictures too" is hidden on themes that don't recolour. |
| `e2ff492` | A new or copied theme is numbered past a name already taken ("New theme 2"), as an import is (`theme::free_name`). |
| `c552876` | The colour picker hangs from the field's right edge, and its square is as wide as the swatch grid (220). |
| `d936203` | The theme editor opens at its top (the pane's key includes the editor). |
| `0f95642` | A hex typed without `#` is read, and an unreadable one shows "Like #2f3237". |
| `621dc44` | Theme cards line up when a name wraps, and every name is centred. |
| `149158b` | Solarized Light text is `#586e75` (base01), about 5:1 on its background. |
| `86ead64` | Words on a filled button use whichever text colour reads better on it. Accent buttons change only on the two Solarized themes; red (danger) buttons on every dark theme go from white to near-black (white was 3.5:1). Test: every shipped theme has text, accent and red buttons at 4.5:1 or better. |

### Reading and navigation
| Commit | Change |
|---|---|
| `9281861` | ⇧←/⇧→ and ⇧H/⇧L move a wide page across (`scroll-left`/`scroll-right`, also in `keys.toml`). A search brings its match across as well as down. |
| `9946c11` | A trimmed document opens under its last-measured margins (`Entry.crop` in `library.toml`), then measures again. |
| `c46c8c6` | A spread fitted "for the moment" is fitted again at launch. This adds the hidden setting `spread_fitted`; every fit or zoom you choose writes `false`. |
| `3777b1e` | ⌘+/⌘− zoom around the middle of the window (`keeping_point`). |
| `cd563a3` | The Contents and Results lists bring their current row into view (`data-reveal` + `sidebar::reveal_rows`, run by the shell and by the harness). |
| `66444e6` | Search reads no page the renderer is holding (`PageSource::try_text_of`, `Search::knows`). |

### Wording
| Commit | Change |
|---|---|
| `147075c` | The chip and keymap entry are called "Sidebar". The parity width check skips that chip now. |
| `16ad344` | Right-click "Find “…”" is now "Search for “…”". |
| `bedde1e` | The Settings menu's "Present" is a "Presenting" switch row. |
| `1268edb` | "Show page count while scrolling" is now "page number". |
| `47bed13` | "(default)" in segment labels is now a small "Default" tag. |
| `1b6cc1d` | Shortcuts in Settings notes read "Press X to Y", and are left out when a key is unbound. |
| `619de0f` | A copy the clipboard refused says so instead of "Copied." `Clip` now returns whether it worked, and copying a comment says "Comment copied." |

### Follow-up
Each of the 33 was checked against the code. All are in; these were wrong or
missing.
| Commit | Change |
|---|---|
| `b35a555` | Three doc comments sit on their own functions again (`read_colour`, `said_of`, `open_with`; a new function had been put between each and its doc). |
| `5bdc31d` | pdfium's own error text no longer reaches a notice through "could not be opened"; it goes to stderr. |
| `41a485f` | The red button's ink is tested at 4.5:1 on every theme, as the accent's is. |
| `dd146d4` | Two to Cover with the pair fitted for the moment wrote `spread_fitted = false`, and the next launch came back at the reader's zoom across both. AGENTS.md says what is written. |
| `4f50878` | An emptied colour field (which means "derive it") no longer shows "Like #2f3237" in red. |
| `28b1cea` | Reopening the find bar keeps the match it went down on while you are on its page (it went to the first match nearest the page, because closing the bar drops the index). |
| `bf3486d` | Presenting ends with its full screen even when the launch window's first report already says full screen. Test for presenting coming back at launch. |
| `251e6db` | A comment being typed outlives a write of our own landing: the reopen cleared the passage, so the field vanished with the words in it. The harness can now hold writes back (`Reader::while_writing`). |
| `6070b47` | The × on Settings and the other windows turns red under the pointer, as Close does. |
| `42527f1` | Comments that still named `.clipped`, `Viewer::write`, "Find “…”" and "highlight changes". |
| `784942c` | Tests for undoing "Put back" and taking a signature off, the "Search for" label, the "Default" tag and "Comment copied." |
| `1751de7` | A scan slice that meets pdfium's lock busy tries again within its own 8ms instead of ending at once: the next slice came straight after, so the window redrew every frame while a render held the lock. `one_slice_of_the_scan_does_not_read_the_whole_book` failed when other tests held the lock. |

## 4. Shortcuts that have a known limit

- **`reveal_rows` relies on a Blitz quirk**: Blitz offsets a scrolling box's
  own position by its own scroll amount. The row's place in the list is
  worked out on that assumption. If Blitz changes, `tests/sidebar.rs`
  (`the_current_*_is_scrolled_into_view`) fails.
- **Search during rendering** waits behind continuous rendering instead of
  stalling the window, at most 8ms a slice. A real fix is a text-reading
  thread feeding the scan.
- **`spread_fitted`** is a setting the reader never sets; it only records
  whose fit is in force. All five places that write fit or zoom also write it.
- **A comment being typed when the document changes on disk** (a recompile)
  is dropped with its passage: the passage's place in the old text means
  nothing in the new one. Only a write of our own keeps it.
- **The trimmed-margins test** checks that `crop` is written and the ratio
  after settling, not that the first layout is already trimmed: the harness
  measures before anything can look.
- **Lost vs. kept beside the document** is told apart by `Highlight.lost`.
  Journals written before this commit have no flag, so a lost highlight
  already in one is still drawn until it is restored or removed.
- **Parity:** `doc-title` is allowed +8px (the fade's padding), and the
  toolbar widths are measured at 1440 now, because labels fold below 1400.

## 5. Decided against

- **"Fixed" zoom resets to 100%.** That is what "Fixed" means. The brief's
  rule that no setting writes another didn't anticipate it; consider
  rewording that line in the brief.
- **The "Show menu bar" chip appears anywhere in the top 130px.** It only ever
  shows at the top right, which is fine.
- **"Left"/"Right" rotation labels** stay as they are: short, and they save
  space.

## 6. Found but not fixed

Not in the list you approved. Roughly in order of how much they matter.

**Settings and themes**
- *Done (`f4d03f0`).* The six highlight colours can't be reached from Settings. The only ways in
  are the "…" on a selection's swatches and "Change colour…".
- *Done (`d06cd12`, `8ae0510`): the palette changes only on Save.* "Change
  colour…" on a highlight edits the shared palette slot, and closing that
  window (Escape included) applies the colour.
- *Done (`d190b89`): closing Settings saves the draft.* A kept theme draft: `show_pane` doesn't preview it. The Theme menu's "New
  theme…" and "Copy…" overwrite it without asking.
- *Done (`8dd8bc7`): offered only for a saved theme.* The editor's
  Import/Export buttons are always disabled for a new theme, and Import from
  the editor throws the draft away.
- Settings action buttons have no icons, while the same actions in the Theme
  menu do.
- *Partly (`a0cd435`): the menu says when there is nothing to follow; the
  wording is still mixed.* The Theme menu's "Follow the system" switch looks dead when the machine
  reports no appearance. The wording also mixes "machine" and "system".
- Wording:
  - The Accent and Links notes are vague.
  - The Keyboard page says "keybinds" and explains the file watcher.
  - "Delete Nord" in one place, "Delete this theme" in another (the
    ellipses are gone, `e18323d`).
- *Partly: the Theme menu marks "Yours"; the Settings grid does not.* The
  theme grid doesn't mark your own themes, and light and dark themes are
  mixed in the menu order.
- The Settings dialog is a fixed height, so pages are cut off mid-card with no
  hint to scroll.

**Toolbar, menus, keys**
- The document name (where Print, Sign, Bookmark and Info live) doesn't look
  like a button: no chevron, faint text.
- "Page numbers" appears both in the Settings menu and in the count's own
  menu, worded differently.
- "One page at a time" is one click away in the Settings menu, and the brief
  wants continuous scrolling to be hard to leave by accident.
- *Done (`91328bf`), the Document menu too.* The View and Open menus have no maximum height, so in a short window their
  last rows are unreachable.
- "Close" folds to a lone ✕ at the top left, which reads as "close window".
- Keyboard page:
  - "— macOS only" suffixes.
  - Labels mix nouns and verbs.
  - "Documents" and "Looking at it" are odd group names.
- The page right-click menu puts "Full screen" first even when not in full
  screen.
- *Partly (`ec8e955`): ⌘B does nothing there now; presenting still goes full
  screen over the start screen, and says so.* ⌘B (and presenting) on the
  start screen changes a setting with nothing visible happening.

**Reading**
- Rotating blanks every page until it is redrawn, because rotation is in the
  page's key.
- Paged mode: one trackpad flick turns 2–3 pages. `TURN_GAP` counts from the
  last turn, not from the last wheel event. Counting from the last event
  would hold a hard-spun mouse wheel to one page; the `ponytail:` note on
  `TURN_GAP` names the real fix, winit's momentum phase.
- The sidebar rebuilds its rows (up to 300 results) on every scroll frame.
  The mark rows are cached (`Viewer::markup_rows`); what is left is clones
  and the diff.
- *Partly (`d972b88`): 3.3s to 0.17s on 1,668 pages.* Opening a large scan
  still freezes the window. A reload already opens on a thread.

**Markup and messages**
- *Done (`d29856a`), and on a selection in a document that cannot be
  written.* "Comment…" is offered on a highlight kept beside the document,
  and then refused.
- Copying keeps every line break and the hyphen at a line's end
  ("algo-\nrithm"). Soft hyphens are dropped and ligatures split already;
  the rest is deliberate (`select::quote`).
- "Remove all highlights?" reads as if it already happened, and doesn't say
  that comments go too or that ⌘Z undoes it.
- A drawn signature has to be kept before it can be placed, and the "Kept"
  rows give no hint that clicking one places it.
- Small things:
  - "Signing cancelled." is also said for a typed line.
  - Removal is "Take off" / "Remove" / "Delete" in different places.
  - A comment on a new selection gets a random colour.

**Looks**
- The selection popover and a highlight's popover look unrelated. The red ×
  only dismisses, but looks like delete.
- *Done (`f4d03f0`): the default palette is "soft".* The default highlight colours are loud. (The pure #ffff00 seen in the
  screenshots was a test document's own colour; the defaults are `#ffd60a`
  and so on, `settings.rs`.)
- The heavy filled "Edit" button on comment cards. In a narrow window there is
  a card inside a popover, a box within a box.
- About page: the paths wrap in a narrow column, and the page heading says
  "Moonowl" instead of "About".
- The current Contents row's tint fades out at the right with the text mask,
  and the rows aren't inset like the tabs above them.
- Solarized Light's toolbar text is faint. The bar's ground changed since
  (`887ed82`); check by eye.
