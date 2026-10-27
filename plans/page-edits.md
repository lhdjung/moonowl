# Page edits, and what comes after them

Asked while reading: *should Moonowl split and merge PDFs, remove pages, and
do that kind of thing?* This is the assessment and the plan. All seven steps
of "Order of work" are built; "Later, if anyone asks" is not.

A second question came after it: *what do other PDF programs (Readest, an
ebook reader, and JOPDF, a free editor) have that Moonowl should have too?*
The answer is in "Beyond page edits" at the end. It replaces the third limit
below, which ruled out anything that adds to a page.

## Does it belong in Moonowl?

Yes, as long as it stays small and stays in the Pages panel.

The case for it is the reader's own work. People take a cover page or a blank
page out of a paper, turn the one scanned page that came out sideways, pull a
chapter out of a book to send to someone, or put two scans together into one
file. Each of these is a minute's job that currently means opening another
program, and Acrobat charges for all of them. Preview shows that a reader can
offer them without clutter: they live in the thumbnail column, and nobody who
does not look for them ever sees them.

The case against it is the brief. Moonowl is a reader, and an editor is a
different kind of program, with modes, toolbars and a document that is
"unsaved". So this plan has three limits:

1. **No editing mode and no new toolbar.** Every edit is reached from the
   thumbnails (a right-click, a drag, or a key while pages are selected) or
   from the Document menu. The reading view does not change.
2. **No unsaved state, and the original is left alone.** The first page edit
   in a document asks where it goes, and by default it goes into a copy the
   reader names. Every later edit is written into that copy at once and can
   be undone with ⌘Z. A new file (split, combine) is written to a place the
   reader chooses in a save dialog before anything else happens. See "The
   first edit asks where it goes".
3. **The page's own text is not rewritten.** Adding to a page is a
   different matter: a form filled in, a note, a line of text typed onto a
   scanned form, or the invisible text that OCR puts under a scan. See
   "Beyond page edits" for why rewriting text is not planned.

Most of the hard part is already built. `markup::edit` loads the document from
bytes, changes it, saves it with `FPDF_SaveAsCopy` and writes it back
atomically. `Viewer::write_step` keeps a copy of the file for undo
(`markup::Before`), tells the watch that the change is ours, and reopens the
document on a thread. `markup::standing` already knows when a file cannot or
should not be written: encrypted, read-only, over `IN_FILE_LIMIT`, or signed.
A page edit is one more kind of `change` handed to that path. The first edit
also needs one thing that path does not have yet: writing the changed
document to a different file, and then reading on in that file.

## What pdfium offers

All of these are in `pdfium-render` 0.9.3 or in its raw bindings:

| Edit | Call |
| --- | --- |
| Remove a page | `PdfPage::delete` (`FPDFPage_Delete`) |
| Move pages | `FPDF_MovePages`, through `pdfium.bindings()`; there is no wrapper |
| Turn a page in the file | `PdfPage::set_rotation` (`/Rotate`) |
| Insert a blank page | `PdfPages::create_page_at_index` |
| Duplicate a page, or insert pages from another file | `copy_page_range_from_document` (`FPDF_ImportPagesByIndex`) |
| Append a whole file | `PdfPages::append` |
| Write some pages to a new file | `Pdfium::create_new_pdf`, then the same copy, then `save_to_bytes` and `config::atomic_write` |
| Write the trimmed margins into the file | `PdfPageBoundaries::set_crop` (`/CropBox`) |

The pdfium the app ships (`chromium/8021` in `scripts/pdfium.sh`) has all of
them; `FPDF_MovePages` is the youngest.

pdfium **cannot** write the outline or `/PageLabels`, and it copies a link
between imported pages without its destination. lopdf does all three after
pdfium has saved (`outline.rs`, `labels.rs`, `outline::relink`; AGENTS.md,
"What lopdf does"):

- **The outline.** A removal takes out every heading that led to a removed
  page, and the headings under it take its place. Moving pages needs nothing,
  because a heading points at the page object, not at a position. Saved pages
  and combined files carry their headings.
- **`/PageLabels`.** Every page keeps the label it had, and a page with no
  label is labelled with its place. "Number pages from here…" types new ones.
- **Links between imported pages** are pointed at their pages again. Only a
  link to a page that did not come along is lost, and the notice says so.

Where lopdf cannot read the file, the outline and the labels are left as
pdfium wrote them, and the notice says what that means.

## The edits

In order of how often a reader needs them:

1. **Remove pages.** One page or a selection.
2. **Turn pages in the file.** Not the view rotation in the toolbar, which
   turns the whole document for this reader and is never written (and stays
   where it is). This writes `/Rotate` on the selected pages, so the sideways
   scan is upright in every program.
3. **Move pages.** By dragging thumbnails, and by "Move to page…" for a long
   document where dragging across three hundred pages is no fun.
4. **Save pages as a new file** (splitting). The selected pages, or a range
   typed in, written to a file the reader names. The document itself is not
   changed. The notice offers to open the new file.
5. **Insert pages from another file** (merging). Before or after the selected
   page, or at the end. The other file is not changed.
6. **Combine files into a new file.** Several files picked in the open dialog,
   in the order picked, written to a new file and opened. This is the one
   merge that does not touch any existing file, and it can start from the
   start screen.

Later, if anyone asks:

- **Insert a blank page** (the same size as its neighbour, for two-sided
  printing) and **duplicate a page**. Each is one more `arrange` function and
  one menu row, but nobody has asked for either, and every row in the
  thumbnail menu is read by everyone who opens it.
- **Keep the trimmed margins in the file.** `crop.rs` already measures them.
  Writing them as `/CropBox` makes a paper readable on an e-reader. It is one
  menu item, but it changes what every other program shows, so it waits for a
  request.
- **Split by chapter.** *Built*: "Save each chapter as a file…" in the
  Document menu saves one file per chapter into a folder, named after it,
  with the pages before the first chapter as "Front matter". AGENTS.md
  ("Page edits") gives the rules for what a chapter is.
- **Put two-sided scans in order.** A scanner that does fronts and then backs
  gives pages 1, 3, 5, … 6, 4, 2. One command that interleaves them is a real
  help to the people who need it and nothing to anyone else.
- **A grid of pages in a window of its own**, for arranging a long document.
  The single column is enough to start with.

Not planned: N-up printing, because the print dialog does it. Page numbers
stamped on pages and watermarks could be drawn with pdfium's text objects,
but nobody has asked for them. Page labels, which name pages without
changing them, and the outline are written through lopdf (`labels.rs`,
`outline.rs`).

## The UI

### Selecting pages

A thumbnail is a button that goes to its page, and it keeps doing that. On
top of that, the column learns the selection every file manager has:

- **Click** goes to the page and selects it alone.
- **⌘-click** (Ctrl elsewhere) adds or takes away a page without going to it.
- **⇧-click** selects every page from the last one clicked.
- **⌘A** selects every page when the column was the last thing clicked
  (see "The keyboard").

The current page has the existing `current` ring. A selected page gets a
filled accent outline and a tick in the corner, so a selection is visible in a
column that is mostly scrolled away. A selection is counted at the foot of
the column, with the actions for it:

```
┌ Contents   Pages ───────────┐
│  ┌──────┐                   │
│  │      │                   │
│  │  ✓   │  ← selected       │
│  └──────┘                   │
│     3                       │
│  ┌──────┐                   │
│  │      │                   │
│  │  ✓   │  ← selected       │
│  └──────┘                   │
│     4                       │
│  ┌──────┐                   │
│  │      │                   │
│  │      │                   │
│  └──────┘                   │
│     5                       │
├─────────────────────────────┤
│ 2 pages selected            │
│ [⟲ Turn] [↗ Save as…] [🗑 Remove] │
└─────────────────────────────┘
```

The bar appears while any page is selected, and Deselect on it, in the
thumbnail menu, or Escape after a click in the column puts it away. "Select
pages…" in the Document menu opens the thumbnails with the page being read
selected. When the panel is too narrow, the buttons fold down to their
symbols, like the toolbar's. The bar's buttons go through the same first-edit question as every
other edit: if the document has not been edited yet, Remove asks where the
edit goes before any page is removed.

### The thumbnail menu

A right-click on a thumbnail selects it (unless it is already in the
selection) and opens a menu in the style of the existing context menus:

```
┌───────────────────────────────┐
│ ↺  Turn left                  │
│ ↻  Turn right                 │
│ ⇡  Insert pages before…       │
│ ⇣  Insert pages after…        │
│ ↕  Move to page…              │
│ #  Number pages from here…    │
│ ───────────────────────────── │
│ ↗  Save as a new file…        │
│ ───────────────────────────── │
│    Deselect page              │
│ 🗑  Remove page               │   (red under the pointer, like Clear list)
└───────────────────────────────┘
```

With several pages selected, the words follow: "Remove 3 pages", "Save 3
pages as a new file…". "Move to page…" opens a small page field in the menu
itself, the same stepper as the toolbar's page field, and Enter moves the
selection to just before that page. "Number pages from here…" opens a field
holding the clicked page's label; what is typed there ("iv", "1", "A-1")
numbers the pages from that one up to the next range the document starts.

"Turn left" here is not the toolbar's "Turn the page left": the menu sits on
the thumbnails, which show the file and do not turn with the view, so the
thumbnail turns and the reader can see what was written. The notice says it
too: "Page 4 is turned left in the file."

### Dragging

A thumbnail dragged more than a few pixels lifts a small copy of itself (or a
stack, for a selection) and shows an accent line between the rows where it
will land. Near the top or bottom of the column, the column scrolls. Dropping
moves the pages; Escape or a drop outside the column puts them back. A click
never moves a page, because the drag begins only past the threshold.

A PDF dragged from the Finder onto the column is not planned. Blitz does not
deliver file drops at all (nothing in the fork handles winit's `DroppedFile`),
so it would be a fork change on top of everything else here, and the menu
item does the same job.

### The keyboard

The column has no focus of its own: Blitz moves focus between buttons, and
the keymap has no notion of where a key was typed, only of whether a document
is open (`needs_document`). A chord is a lookup, so one chord cannot mean two
things, and both chords the first draft of this plan wanted are taken:
Backspace is Back (the way back), and ⌘A selects the text of the current
page. So:

- The Viewer remembers whether the thumbnail column was the last place
  clicked (`column_last`), set by a click in it and cleared by a click or a
  plain key anywhere else. Keys act on the column only while it is also on
  screen (`column_has_keys`): hiding the sidebar or changing its tab leaves
  the selection alone.
- One new action, `remove-pages`, on **Delete** alone, which no action has
  yet, and on ⌘⌫ on a Mac, whose laptops have no Delete key. It removes the
  column's selection when the column has the keys and the selection is not
  empty, and otherwise does nothing. Backspace keeps meaning Back everywhere.
- A second new action, `select-all-pages`, with no default key. ⌘A stays the
  page's text; `perform(SelectPage)` selects every page instead when
  `column_last` is set, which is the one context-dependent key in the app and
  is said so in its comment.

Turning and moving have no default keys either; all of these can be bound in
`keys.toml` like every other action.

### The Document menu

Five items, below Print:

```
  Select pages…
  Insert pages from a file…
  Combine files into a new file…
  Save pages as a new file…
  Save each chapter as a file…
```

"Save pages as a new file…" opens a save dialog after asking which pages, in
one line in the menu: a range field ("3–7, 12") prefilled with the selection
or the current page. The field speaks in the labels on screen, as go-to does,
so an offprint's pages are typed as "408–409". "Combine files…" is also on the start screen, under Open,
because it needs no document open.

### The first edit asks where it goes

The first page edit in a document (removing, turning, moving or inserting
pages) does not change anything yet. It asks one question, in the same window
the app already asks before writing into a signed document (`Asking`). That
window has one button that goes ahead and one that cancels; this question
needs two that go ahead, so `Asking` gains an optional second `go` with its
own `then`, and a Cancel that is pressed by Escape as now:

```
┌──────────────────────────────────────────────────────────┐
│ Page edits go into a copy, so paper.pdf stays as it is.  │
│                                                          │
│            [ Change this file ]   [ Edit a copy… ]       │
└──────────────────────────────────────────────────────────┘
```

**Edit a copy…** is the default and the button Enter presses. It opens a save
dialog that suggests `paper (edited).pdf` in the same folder. When the reader
saves, the edit is written into the new file, the window switches to it, and
every later edit in this window is written into that copy. No second copy is
ever made. Cancelling the save dialog cancels the edit.

**Change this file** writes the edit into the document itself, with undo, as
a highlight is written. Later edits in this window go in the same way without
asking again. Because the original is changed in this mode, it is also the one
mode that asks before a large removal: removing more than five pages at once
(`arrange::ASK_BEYOND`) says how many pages will go and waits for a second
press. In the copy, undo is enough, and nothing more is asked.

The answer lasts until the document is closed. It is not a setting: whether
the reader wants this particular file changed is a question about the file.

The copy is a continuation of the original, not a new document:

- Its library entry starts with the original's bookmarks, highlights and
  reading position, moved through the edit (see `Moved` below). Without this
  the copy would open with none of the reader's marks, because the library
  keeps them by path.
- The original stays in the recent list beside the copy, with its own marks
  untouched.
- The watch follows the copy from now on, and so does `library.open`.
- ⌘Z after the first edit puts the copy back to what the original was. The
  copy stays on disk; the reader made it, so the app does not delete it.

Highlights still go into the file itself, as they do now. The two rules are
easy to explain: a highlight only adds to a file, while a page edit can take
something away.

This also helps with a LaTeX draft. An edit written into the draft itself
would be lost at the next compile; written into a copy, it is kept.

### What the reader is told

Every edit ends in a notice that says what happened and how to take it back:

- "Saved the edit as paper (edited).pdf. From now on, page edits go into this
  copy." (only after the first edit)
- "Removed pages 3 and 4. ⌘Z puts them back."
- "Moved 3 pages to before page 10."
- "Saved pages 12–30 as Chapter 2.pdf." with an Open button.
- "Removed page 5. The 2 headings that pointed at it are taken out of the
  contents." (only when that is true)
- "Page labels in this document did not move with the pages." (only when the
  document has labels lopdf could not read)

Refusals reuse the markup sentences: an encrypted or very large document says
it cannot be changed, and so does a certified one, at once, since no
certification allows a page edit. The rest depends on where the edit goes:

- **Into a copy**, a read-only document can be edited, because the original
  is only read. A signed document is not asked about either: the copy is
  written as an incremental update after the signed bytes, as the document
  itself would be.
- **Into the file itself**, a read-only document refuses "Change this file",
  and a signed one asks once, before the first write, exactly as a highlight
  does.

"Save as a new file" and "Combine files" work for a read-only document too,
because they do not write to it.

## How it is built

### A new module, `arrange.rs`

Plain functions over `markup::edit`, one per edit, in the style of
`markup::remove_all`:

```rust
pub enum Edit {
    Remove(Vec<usize>),
    Turn(Vec<usize>, i32),
    Move(Vec<usize>, usize),
    Insert(String, usize),
    Number(usize, String, Option<usize>),
}
impl Edit {
    pub fn write(&self, path: &str, to: &str) -> Result<Wrote, String>;
}
pub fn save_pages(path: &str, password: Option<&str>, pages: &[usize], to: &str)
    -> Result<bool, String>;
pub fn combine(paths: &[String], to: &str) -> Result<(usize, bool), String>;
```

`save_pages` and `combine` write a new file through `config::atomic_write`
and never go through `write_step`: there is nothing to undo in the open
document. `insert_from` and `combine` check every source with
`watch::whole` and refuse an encrypted one, since its password is not known.

Each of the others also returns where every old page went:

```rust
/// Where each page of the document before an edit is after it: `None` for a
/// page that was removed.
pub struct Moved(Vec<Option<usize>>);
```

`Moved` is the one new idea in this plan, and the reason it needs one.

### The rule it changes

`Viewer::offload` and `Viewer::adopt` rest on one sentence: **a write of our
own moved no page.** That is why the reopen keeps the pages' spaces
(`learn_spaces`), why the way back (`past`, `future`) survives a write, why a
comment being typed survives one, and why the reader's place is kept by page.
Every page edit except turning breaks that sentence.

So the write carries a `Moved` from the thread back to `adopt`, and `adopt`
applies it in one place:

- **Spaces:** `learn_spaces` takes the old spaces through the map, so the
  pages that were only moved keep theirs and the new ones are measured. (It
  currently refuses any change in page count, which stays the rule for a
  draft that is not ours.)
- **The reader's place:** the anchor's page goes through the map. A removed
  page lands on the page that followed it.
- **The way back:** each entry goes through the map, and entries on removed
  pages are dropped.
- **Bookmarks and the journal:** `Entry.marks` and the journal's highlights
  are moved through the map *before* the write, the way a removal empties the
  journal before it writes, and put back if the write fails. Bookmarks on a
  removed page are dropped.
- **Undo:** `Step` gains the bookmarks beside the journal, so ⌘Z puts back the
  file, the journal and the bookmarks together. The file needs no inverse
  map, because it is put back whole, but the step carries the edit's `Moved`
  inverted (`Moved::inverse`, given the page count after the edit), so that
  the reader's place, the selection and the way back follow the pages back.
- **The thumbnail selection:** moved through the map, so the pages just moved
  are still selected where they landed.
- **What is closed:** a comment being typed, an open mark's popover and an
  armed Remove are closed, as `adopt` already does for a rebuild, because the
  indices they hold may now be another page's.

Turning a page in the file moves nothing, but it changes that page's `Space`.
`Space` holds `/Rotate` as `turns` beside the unturned box, so the turned
pages' spaces are carried over with `turns` rewritten rather than measured
again. Highlights on them stay where they were: `/QuadPoints` are in the
page's own unturned space, and `Space` already turns them.

`relay_column`, `measure_crop` (when trimming) and `search.forget()` already
run on every reopen and need no change.

### Writing into a copy

The first edit into a copy is one write with two paths: read the original,
change it, and write the result to the path the save dialog gave. So
`markup::edit` gains a target:

```rust
pub(crate) fn edit_into(
    path: &str,
    to: &str,
    change: impl FnOnce(&mut PdfDocument<'static>) -> Result<(), String>,
) -> Result<(), String>;
```

`edit` becomes `edit_into(path, path, change)`. Written to a new path,
nothing is released and nothing is taken for undo of the original, because
the original is not touched; the `Before` for that step is a copy of the
original, so that ⌘Z puts the copy back to it.

Two checks in the write path are about the file being replaced and have to
be told apart when it is not. `write_file` refuses a write when the stamp of
`path` is not the draft on screen, and that check stays, on the original: the
copy is made from the document the reader is looking at. `write_over` asks
the same question of its *target* right before the rename (`into_draft`), and
a target that does not exist yet fails it, so a write into a new path goes
through `config::atomic_write` plainly and never through `into_draft`. The
same holds for `save_pages` and `combine`.

The reopen then opens `to` instead of `path`, and `offload` does not call
`Watching::wrote`, since nothing on the original's disk changed. `adopt`
already takes a new document into the window; what is new is that the
window's path changes, so the same step also:

- copies the original's library entry to the copy's path, with its marks,
  journal and place moved through `Moved`, written through the scribe like
  any other library change, and records the copy as what this window has
  open (`Store::opened`), which is also what replaces the window's entry in
  `library.open`;
- moves the watch with one call, `Watching::document(window, Some(copy))`,
  which replaces the window's subject; there is nothing to forget separately.

The Viewer holds where edits go (`Edits::Ask`, `Edits::Copy`,
`Edits::InPlace`). It starts as `Ask` for every document that is opened, and
becomes `Copy` once the copy has been written or `InPlace` once the reader
chose "Change this file".

### The thumbnail column

`sidebar.rs` gains the selection (a `BTreeSet<usize>` on the Viewer, cleared
when the document changes under it), the foot bar, the context menu, and the
drag. The drag is the largest piece of new UI: a pointer captured on
`.thumb-picture`, the threshold, the insertion line computed from the
column's existing row positions, and scrolling at the edges. The panel's own
resize drag is the pattern to follow. Clicks keep reaching the button through
`pointer-events: none` on the widget, as they do now.

### Tests

`tests/page-edits.rs`, with documents from `fixture.rs`:

- Removing page 2 of three leaves two pages, and the text of page 2 is the
  old page 3. A bookmark on page 3 is now on page 2. ⌘Z gives back three
  pages and the bookmark.
- A highlight on a removed page leaves the journal, and comes back with ⌘Z.
- Moving pages 1–2 to the end reads 3, 1, 2, keeps each page's highlights,
  and keeps the reader on the page they were reading.
- Turning page 1 in the file writes `/Rotate 90`, and its highlight is drawn
  in the same place on the turned page.
- `save_pages` writes a file with exactly those pages and does not change the
  document's bytes.
- `insert_from` and `combine` count the pages of both sources.
- A heading pointing at a removed page is counted in the notice.
- Through the harness: a plain click on a thumbnail selects one page and
  moves no page; Delete after a click on the page removes nothing, and
  Backspace is still Back after a click in the column; ⌘A after a click in
  the column selects every page and after a click on the page selects its
  text; a drag past the threshold moves a page and a drag under it does not.
- A fixture with internal links, imported into another document, to settle the
  open question about links.
- The first edit into a copy leaves the original's bytes unchanged, writes
  the copy with the edit in it, and switches the window to the copy. The copy
  has the original's bookmarks, moved through the edit, and the original's
  library entry is unchanged.
- A second edit writes into the copy and makes no new file.
- Cancelling the save dialog changes nothing, in the file or in the library.
- With "Change this file", removing six pages waits for a second press, and
  removing five does not. In a copy, removing six pages does not ask.
- A read-only document can be edited into a copy and refuses "Change this
  file".

## Order of work

Each step is a commit, and each is useful on its own:

1. **`Moved` through `offload` and `adopt`, and `arrange::remove`**, with the
   bookmarks in `Step`, behind a context menu on one thumbnail. This is the
   step that changes the rule, so it carries most of the tests.
2. **The first-edit question and the copy**: `edit_into`, `Edits`, the
   library entry carried over, the watch moved, and the question before a
   large removal in "Change this file".
3. **Selection in the column**, the foot bar, `remove-pages` and
   `select-all-pages`.
4. **Turn in the file**, and the measuring of turned pages again.
5. **Save as a new file** and **Combine files into a new file**, which do not
   touch the open document and so need none of steps 1 and 2.
6. **Move to page…**, then **dragging**.
7. **Insert from a file.**

Step 5 goes first: it is the least risky, it answers "split" and "merge" on
its own, and it shows whether anyone uses this before the rule in step 1 is
changed.

## Decided

- **The original is not changed unless the reader says so.** Writing every
  edit into the file at once was the first draft of this plan. It was set
  aside because undo lasts only until the window closes, and a removed page is
  a bigger loss than a highlight. Making a new copy for every edit was set
  aside too, because it fills the reader's folder with files. What is planned
  is one copy per document, made at the first edit, with "Change this file"
  for readers who want the original changed. Keeping the original in the
  config directory for a week, with a "Restore the original" item, was
  considered and not chosen: the reader cannot see that backup, so it does
  not reassure them in the way a copy does.
- **Large removals are asked about only where they cannot be undone
  safely**, which is in "Change this file". In a copy, undo covers them.
- **The selection bar exists.** It appears while any page is selected, and
  it is the only new element that appears on its own.

## Beyond page edits

Asked in October 2026: *what do other PDF programs have that Moonowl
should have too?* The two looked at were Readest, an ebook reader, and
JOPDF, a free editor for Windows. Only their lists of features were used,
never their code: Readest is AGPL, and JOPDF is closed.

The test for each feature was whether it is the reader's own work, as page
edits are, whether the stack can do it well, and what it costs the binary.

### What is built first

These three are small and need no new dependency. `serde_json` is already a
dependency, and the `image` crate is already compiled into the app through
Blitz, with PNG and JPEG.

1. **Export highlights and comments.** "Export highlights as" in the
   Document menu, shown while the document has any, with a button for each
   format: Markdown, Text and JSON. The format is a button in the menu
   because the macOS save dialog offers no choice of format. The save dialog
   then suggests `paper (highlights).md` (or `.txt`, or `.json`).
   - **Markdown**: the document's title as a heading, then one quotation
     per highlight, with its page, its date and its comment. The reader's
     words are escaped, so a comment that begins with `#` stays a comment.
   - **Plain text**: the same, without the Markdown marks.
   - **JSON**, for other programs and scripts: the document's title and
     file name, and for each highlight the page (its position in the file),
     its label (what is printed on it), the quote, the comment, the date it
     was made (RFC 3339), the colour as hex, and whether it is in the file.

   The colour is only in the JSON, since a hex code means nothing to a
   person reading notes. A highlight that is only in Moonowl's journal says
   so in all three.
   The date is the highlight's `/CreationDate` in the file, or `/M` when the
   file has no creation date; for a highlight that is only in the journal,
   it is when the journal recorded it.

   The rows are the ones the Highlights panel lists (`markup_rows`), in the
   same order, so what is exported is what the reader sees.
2. **Pictures into PDF.** "Combine files into a new file…" and "Insert
   pages from a file…" take JPEG and PNG files as well as PDFs, and each
   picture becomes one page. That is how a phone photo of a letter becomes
   a PDF. A page is the picture's own shape, fitted inside an A4 page
   (turned for a picture wider than it is tall). A JPEG goes into the file
   as it is, without being compressed again. A JPEG whose EXIF data says it
   is turned is turned and encoded once more, because pdfium ignores EXIF.
   A PNG is drawn onto white, since paper is what a page has under it. The
   pictures are decoded before pdfium's lock is taken, so a large photo does
   not keep a page from being drawn.
3. **PDF into pictures and text.** Two items in the Document menu:
   - "Save page as an image…" writes the page being read (or the page
     that was right-clicked) as a PNG at 300 dpi, as printed, without the
     theme's colours.
   - "Save the text as a file…" writes the text of every page to a `.txt`
     file, with a blank line between pages. It reads the pages on a thread,
     as "Save each chapter" does. A scan has no text until OCR (below)
     gives it some, and the notice says so instead of writing an empty file.

   "Copy page as image" would need the clipboard to take pictures, which
   the `Clip` door does not yet do, so it waits.

### What comes next

4. **Filling in forms.** This is the largest gap: Moonowl can sign a form
   but not fill one in. pdfium-render 0.9.3 already sets a text field's
   value and checks a box or a radio button (`set_value`, `set_checked`).
   Each field would be a Blitz input placed over the field's box on the
   page, in the same way the selection popover is placed, and a change is
   written through `write_step`, so it has undo and a signed document gets
   an incremental update. pdfium's form-fill environment
   (`FPDF_InitFormFillEnvironment`) draws the fields' appearances. Lists,
   combo boxes and a form's own scripts come later, if at all.
   **Adding text anywhere** belongs with it: a `/FreeText` annotation typed
   where the reader clicks, for forms that are only a scan or a flat page,
   as Preview's Text tool does.
5. **OCR for scans.** Today a scan cannot be searched, selected,
   highlighted or copied, and the reader is told "there is no text on this
   page". OCR would make every one of those work. The plan is to use the
   engine each system already has, which costs the binary almost nothing:
   Vision (`VNRecognizeTextRequest`) on macOS, `Windows.Media.Ocr` on
   Windows, and Tesseract on Linux when it is installed. The words are
   written into the page as invisible text over the picture (text render
   mode 3), which is what ocrmypdf does, and pdfium has the calls for it
   (`FPDFText_SetText`, `FPDFTextObj_SetTextRenderMode`). A first version
   could keep the recognised words in memory only, and write them into the
   file when the reader asks.

### Worth doing after that

- **Notes and drawing.** A note pinned to a point on the page (`/Text`),
  for figures and scans that have no text to highlight. Moonowl already
  shows such notes but cannot make them. Freehand drawing (`/Ink`) is the
  signature canvas in `sign.rs`, used more widely.
- **Read aloud**, with the system's own voices (AVSpeechSynthesizer on
  macOS, WinRT on Windows, speech-dispatcher on Linux), and the sentence
  being read painted by the selection shader. It helps with accessibility
  and with proofreading. Reading order in a paper with two columns is the
  hard part.
- **Passwords.** "Save an unlocked copy…" is one pdfium flag
  (`FPDF_REMOVE_SECURITY` in `FPDF_SaveAsCopy`). Adding a password is
  possible too, because lopdf 0.46 can encrypt.
- **"Save a smaller copy…"**, which shrinks the pictures in a file
  (downsampled and encoded as JPEG again). Page edits already compact
  everything else. This matters for scans that are too large to email.
- **Redaction, done the safe way.** The common approach, a black box drawn
  over the text, leaves the text in the file, which is how most redactions
  fail. The safe way turns the affected page into a picture with the area
  blacked out, and then OCR gives it back its other text.
- **Look up a word** from the selection popover: the system dictionary on
  macOS, a web search elsewhere.
- **Put two-sided scans in order**, and **keep the trimmed margins in the
  file**, both from "Later, if anyone asks" above.

### Not planned

- **Rewriting the page's own text.** A PDF usually carries only the
  letters its text uses, so a new letter often has no shape to draw it
  with, and lines never reflow. It works in a demonstration and fails on
  real files. Adding text, as in step 4, gives most of what people want
  from it.
- **Word, PowerPoint or OpenDocument into PDF.** That needs an office
  suite's layout engine. At most, Moonowl could hand the file to
  LibreOffice when it is installed.
- **PDF into Word or EPUB.** The results are poor in every program,
  because a PDF does not record its paragraphs or its reading order.
- **Reading EPUB, syncing, translation and reading statistics**, which are
  Readest's own ground. They would make Moonowl a different program, and
  everything in it is built on fixed pages.
