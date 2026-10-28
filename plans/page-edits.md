# Page edits, and what comes after them

Asked while reading: *should Moonowl split and merge PDFs, remove pages, and
do that kind of thing?* This is the assessment and the plan. The first
version put every edit in the thumbnail column. A second review, in October
2026, moved them into a page organiser of their own ("Arrange pages"); that
is what this plan now describes. "Later, if anyone asks" is not built.

A second question came after the first: *what do other PDF programs
(Readest, an ebook reader, and JOPDF, a free editor) have that Moonowl should
have too?* The answer is in "Beyond page edits" at the end.

## Does it belong in Moonowl?

Yes, as long as reading does not change because of it.

The case for it is the reader's own work. People take a cover page or a blank
page out of a paper, turn the one scanned page that came out sideways, pull a
chapter out of a book to send to someone, or put two scans together into one
file. Each of these is a minute's job that otherwise means opening another
program, and Acrobat charges for all of them.

The case against it is the brief. Moonowl is a reader, and an editor is a
different kind of program, with modes, toolbars and a document that is
"unsaved". So this plan has three limits:

1. **The reading view does not change.** Every edit happens in the page
   organiser, which covers the window while it is open and is gone when it
   is closed. The toolbar, the thumbnail column and the keys of the reading
   view are about reading only. The one exception is "Number pages from
   here…" in the thumbnail menu, which names pages and does not move them.
2. **Nothing is half saved.** The organiser holds a draft, and nothing is
   written until the reader presses Save. Saving writes a new file, or
   writes into the document itself with undo. Closing the organiser with
   changes in it asks first. The reading view never has an unsaved state.
3. **The page's own text is not rewritten.** Adding to a page is a
   different matter: a form filled in, a note, a line of text typed onto a
   scanned form, or the invisible text that OCR puts under a scan. See
   "Beyond page edits" for why rewriting text is not planned.

### Why an organiser, and not the thumbnail column

The first version put selection, dragging, removing, turning, moving and
inserting in the thumbnail column, and saving pages, combining files and
splitting by chapter in the Document menu. That worked, but it had three
costs:

- **The Document menu became a list of everything.** Seventeen rows mixed
  what the file is (Show in Finder, Information), reading (bookmarks,
  highlights), what is done to the document (Sign, Print) and page tools.
- **Combining and splitting were in different places**, although they are
  the same job: arrange pages, then decide where each file begins.
  Combining had no way to put the files in order or to see their pages
  before writing, and splitting by chapter wrote its files without showing
  where it would cut.
- **The column had to be two things**: a map of the document for moving
  around in, and an editor. One key, ⌘A, meant two things depending on
  where the reader had clicked last.

The organiser shows many pages at once, at a size the reader picks, which a
column in a side panel cannot do. It is a view over the window, as Settings
is, rather than a window of its own: the harness can test it, it wears the
window's theme, and it uses the window's file dialogs and notices.

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

All of them are done in the organiser, in any order, and written together:

1. **Remove pages.**
2. **Turn pages in the file.** Not the view rotation in the toolbar, which
   turns the whole document for this reader and is never written. This
   writes `/Rotate` on the pages, so a sideways scan is upright in every
   program.
3. **Move pages**, by dragging them.
4. **Add the pages of other files**, PDFs or pictures, after the selection
   or at the end. The other files are not changed.
5. **Split into several files**, by starting a new file before any page, at
   each chapter, or every so many pages.
6. **Number pages** ("Number pages from here…"), which stays in the
   thumbnail menu, because it changes what pages are called and not where
   they are.

Also in the organiser, because every arranger has them and each is a few
lines over the draft: **duplicate a page**, **add a blank page** (the size
of the page it follows, for two-sided printing), **reverse** the selected
pages or the whole draft, **keep only the selected pages**, and **select
pages by number, or the odd or even ones**. Odd, Reverse and one drag put
a two-sided scan in order.

Later, if anyone asks:

- **Keep the trimmed margins in the file.** `crop.rs` already measures them.
  Writing them as `/CropBox` makes a paper readable on an e-reader. It
  changes what every other program shows, so it waits for a request.
- **One command that interleaves two-sided scans**, if selecting the odd
  pages, reversing and dragging turns out to be too many steps.
- **The organiser in a window of its own**, beside the reader.

Not planned: N-up printing, because the print dialog does it. Page numbers
stamped on pages and watermarks could be drawn with pdfium's text objects,
but nobody has asked for them.

## The page organiser

### Opening it

- **"Arrange pages…"** in the Document menu opens it on the document being
  read, with the page being read selected and in view.
- **"Arrange pages…"** in the thumbnail menu does the same for the page that
  was right-clicked.
- **"Combine or split…"** on the start screen, beside "Open a document",
  and **"Combine or split files…"** in the Open menu open it with nothing in
  it and ask for files at once. It needs no document open. Someone who
  wants to merge files or split one looks for those words, not for
  "arrange", so the entries that start from files say them, and "Arrange
  pages…" in the Document menu says "Split, combine, turn or remove" under
  its name.

### What it shows

```
┌────────────────────────────────────────────────────────────────────────┐
│ Arrange pages   [+ Add files…]  [↺ Left] [↻ Right] [🗑 Remove]          │
│                 [✂ New file here] [Split ▾]  [− Size +]                 │
│                                    [Cancel] [Save into paper.pdf] [Save as…]│
├────────────────────────────────────────────────────────────────────────┤
│  paper.pdf · 6 pages                                                   │
│  ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐                                     │
│  │    │ │ ✓  │ │    │ │    │ │    │                                     │
│  └────┘ └────┘ └────┘ └────┘ └────┘                                     │
│    1      2      3      4      5                                       │
│  File 2 · 3 pages                                   [Join with above]   │
│  ┌────┐ ┌────┐ ┌────┐                                                  │
│  │    │ │    │ │    │                                                  │
│  └────┘ └────┘ └────┘                                                  │
│    6    scan.pdf 1  scan.pdf 2                                         │
└────────────────────────────────────────────────────────────────────────┘
```

- **A grid of pages**, drawn as the file has them (no theme rotation, no
  trim), in the theme's colours as the thumbnails are. "Smaller" and
  "Larger" change the size of every page, between 80 and 480 pixels wide,
  and the size is a setting. Only the rows in view are drawn, with the same
  margin the thumbnail column keeps, so a 1,700-page book costs a screenful.
- **Each file that will be written is a section**, with a heading that says
  how many pages it has. With one file there is one section. A section
  after the first has "Join with the file above", and with several, each
  heading's name can be typed.
- **Under each page**, what the page is called in its own file, and the
  file's name when pages come from more than one file.
- **A turned page is shown turned.**

### Working in it

- **Selecting** is as in a file manager: a click selects a page alone,
  ⌘-click (Ctrl elsewhere) adds or takes away a page, ⇧-click selects every
  page from the last one clicked, and ⌘A selects every page. The arrows
  move the selection a page along or a row up or down, and with Shift grow
  it. **Select** in the bar offers every page, the odd pages, the even
  pages, and a field for pages by number ("3, 5-8, 10-", positions in the
  grid). Escape closes a menu, then clears the selection, and then closes
  the organiser.
- **Left, Right and Remove** act on the selection. Delete (and ⌘⌫ on a Mac)
  removes too. The last page cannot be removed.
- **A right-click on a page** opens its menu, over the selection (which
  becomes that page alone unless it is part of it): Turn left, Turn right,
  Duplicate, Add a blank page after, Reverse (these pages, or all with one
  or none selected), New file here or Join with the file above, Keep only
  these pages, and Remove.
- **A double-click on a page** of the document closes the organiser and
  reads that page; with changes in the draft it asks first.
- **⌘+ and ⌘−** size the pages, as the − and + buttons do.
- **Enter** is "Save as…".
- **Dragging** a page more than a few pixels lifts it, or the selection,
  and a line shows where it will land. Near the top or bottom of the grid,
  the grid scrolls. Escape, or letting go outside the grid, puts it back.
  A click never moves a page.
- **"Add files…"** asks for PDFs or pictures (JPEG, PNG; each picture is one
  page) and puts their pages after the last selected page, or at the end.
  An encrypted file is refused, since its password is not known. A PDF
  dropped on the window while the organiser is open is added the same way,
  never opened over the document behind.
- **"New file here"** starts a new file at the first selected page, and
  "Join with the file above" takes that back. With several files, a click on
  a file's heading types its name, which is the file's name when it is saved
  ("02 Appendix.pdf"). **Split** offers "At each
  chapter" (for a document with chapters; see below) and "Every … pages",
  with a field for the number. Both replace the cuts that were there.
- **⌘Z and ⇧⌘Z** undo and redo steps in the draft, and select the pages the
  step was about, so that a removal undone shows what came back. The steps
  go when the organiser closes.

### Saving

- **"Save as…"** writes new files. With one section it asks for a file name
  (`paper (arranged).pdf`, or `paper (combined).pdf` when pages come from
  several files) and says so in a notice with an Open button. With several
  sections it asks for a folder and writes one file per section into it,
  writing over nothing there. A section cut at a chapter is named after the
  chapter (`01 Introduction.pdf`); other sections are numbered after the
  first file (`paper (part 1).pdf`). The organiser closes, and a window
  that had no document open opens the new file.
- **"Save into paper.pdf"** writes the arrangement into the document itself,
  with undo in the reading view (⌘Z puts the whole file back). It is
  offered when the organiser was opened on a document, there is one
  section, and no page of the document is in it twice. Writing into the
  document follows the rules every write of the reader's own follows: a
  read-only, encrypted, certified or very large document refuses, a signed
  one asks once before its signature is broken, and removing more than
  `arrange::ASK_BEYOND` pages asks first. The organiser closes once the
  write lands.
- **"Cancel"** with changes asks "Close without saving?", with "Keep
  arranging" as the default.

### Splitting at chapters

The chapters are the headings at the top level of the contents. Where the
top level has only one heading, usually the document's title, the headings
under it are the chapters. Two chapters that begin on the same page are
one, under the first one's name. The pages before the first chapter are
"Front matter".

A chapter runs to the page before the next one begins. It also takes that
page when there is text above the next chapter's heading there, so that
page is in both files. A line does not count if one of the two pages before
or after it has the same words at the same height (within 2pt), comparing
without digits or spaces, since running heads and page numbers repeat. A
line with only a page number never counts. A page where the heading's place
is not known yet goes into both chapters, since a page saved twice loses
nothing and a page left out does. On a page with no text, such as a scan,
the page goes into both when the heading is more than 15% of the way down.

The text is read off the window's thread, and the cuts appear when it has
been read.

### Writing into the document itself

Only two things write into the open document: "Save into paper.pdf" in the
organiser and "Number pages from here…". Both are the reader asking for
that file to be changed, so neither asks where the change goes. Both go
through `write_step`, as a highlight does, so ⌘Z in the reading view puts
the whole file back, with the bookmarks and the journal as they were.

The first version asked at the first page edit whether edits went into a
copy, and then read on in the copy. The organiser makes that question
unnecessary: "Save as…" is the copy, and it is the button Enter presses.

### What the reader is told

Every save ends in a notice that says what happened and how to take it
back:

- "Saved paper (arranged).pdf, 12 pages." with an Open button.
- "Saved 3 files in Chapters." when there were several sections.
- "Arranged paper.pdf: removed 2 pages, turned 1 and added 3 from other
  files. ⌘Z puts it back." Only the parts that happened are said.
- "The 2 headings that pointed at removed pages are taken out of the
  contents." (only when that is true)
- "Page labels in this document did not move with the pages." (only when the
  document has labels lopdf could not read)
- "Links from one of those pages to another do not work here." (only when a
  link between added pages could not be written again)

Refusals reuse the markup sentences: an encrypted or very large document
says it cannot be changed, and so does a certified one, at once, since no
certification allows a page edit. A read-only document refuses "Save into",
and a signed one asks once, before the first write, exactly as a highlight
does. "Save as…" works for a read-only or signed document too, because it
does not write to it.

## The reading view

### The Document menu

```
  Show in Finder
  Bookmark this page
  Sign…
  Print…
  ──────────────────────────
  Arrange pages…
  Export  [Text] [Page picture] [Highlights]
  ──────────────────────────
  Copy name
  Copy path
  ──────────────────────────
  Information
```

"Highlights" is there while the document has any. Its save dialog offers
Markdown, plain text and JSON, and the extension typed decides the format
(Markdown when it says nothing else). "Remove all highlights" is at the foot
of the Highlights panel, beside the highlights themselves.

A right-click on a page has the items that are about that page: Bookmark,
"Sign here…", Print, "Save the page as a picture…", Copy name and path, and
Information.

### The thumbnail column

The column is a map of the document again: a click goes to a page, and
nothing is selected. A right-click offers "Arrange pages…" and "Number pages
from here…". "Number pages from here…" opens a field holding the clicked
page's label; what is typed there ("iv", "1", "A-1") numbers the pages from
that one up to the next range the document starts.

## How it is built

### `arrange.rs`

```rust
/// A page of the arrangement: which file it comes from (`None` is the
/// document itself), which page of it (one-based), and how many quarter
/// turns clockwise are added to it.
pub struct Sheet { pub from: Option<String>, pub page: usize, pub turn: i32 }

pub enum Edit {
    /// The document becomes these pages, in this order.
    Arrange(Vec<Sheet>),
    Number(usize, String, Option<usize>),
}
impl Edit {
    pub fn write(&self, path: &str, to: &str) -> Result<Wrote, String>;
    pub fn moved(&self, before: usize, after: usize) -> Moved;
}

/// New files: these pages of these files, each with its turn.
pub fn assemble(sources: &[Opened], sheets: &[Sheet], to: &str) -> Result<bool, String>;
```

**One edit says everything.** Removing, turning, moving and adding pages are
all an `Arrange`, so the organiser hands over its whole draft and one write
does it. `Arrange` is written in two passes over the bytes, because
pdfium-render keeps a document's handle to itself:

1. Through pdfium-render: every page used from another file is copied in at
   the end, one import call per file (pdfium shares fonts and pictures only
   within a call); the turned pages are turned; and the pages that are not
   used any more are deleted.
2. Through the raw bindings: `FPDF_MovePages` puts every page where the
   draft has it, one page at a time. It keeps each page the same object, so
   headings and links that point at a page still find it.

Then lopdf, in the other process (`objects::Work`): the labels follow their
pages (`labels::follow`), what led to a removed page is taken out
(`outline::forget_gone`), links between added pages are pointed at their
pages again (`outline::relink`), added pages are slimmed, and the whole is
compacted.

`assemble` is the same for a new file, and `save_pages` and `combine` are
two of its uses. Headings come with their pages (`outline::entries`), and a
heading that only groups others comes along while one of them does.

`save_pages` and `assemble` write through `config::atomic_write` and never
through `write_step`: there is nothing to undo in the open document.

Each write into the document also says where every old page went:

```rust
/// Where each page of the document before an edit is after it: `None` for a
/// page that was removed.
pub struct Moved(Vec<Option<usize>>);
```

### The rule it changes

`Viewer::offload` and `Viewer::adopt` rest on one sentence: **a write of our
own moved no page.** That is why the reopen keeps the pages' spaces
(`learn_spaces`), why the way back (`past`, `future`) survives a write, why a
comment being typed survives one, and why the reader's place is kept by page.
Every page edit except turning and numbering breaks that sentence.

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
  the reader's place and the way back follow the pages back.
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

### The organiser

`organiser.rs` holds the draft and the grid, and nothing else: which files
are open in it (`Source`, each with its own `PageSource` and `Chosen`, so
its pages are drawn by the same widget the thumbnails use), the pages in
order (`Draft`: sheets and cuts), the selection, the steps for undo, and
where every page sits (`Grid`, laid out once per change and binary-searched
while scrolling, as `Column` is). A picture added to it is made into a PDF
of one page first, kept in the temporary directory while the organiser is
open, so it can be drawn and copied like any other page.

`app/organise.rs` is the Viewer's half: opening, the keys, the drag, the
file dialogs, and saving. Saving a new file runs on a thread
(`Viewer::on_thread`); saving into the document goes through
`Viewer::write_step_on` with `Paging::Edit`, as numbering does.

### Tests

`tests/organiser.rs` drives the organiser through the harness, and
`tests/page_edits.rs` checks what is written:

- Removing a page leaves the rest in order, and a bookmark on a later page
  moves up with it. ⌘Z gives the page and the bookmark back.
- A highlight on a removed page leaves the journal and comes back with ⌘Z.
- Moving pages keeps each page's highlights and the reader on their page.
- Turning a page writes `/Rotate`, and its highlight is drawn in the same
  place on the turned page.
- Adding a file counts the pages of both, keeps the links between its own
  pages, and takes a picture as a page of its own shape.
- A heading pointing at a removed page is taken out and counted in the
  notice; a link to it is taken off.
- Several sections are written as several files, named after their
  chapters where they were cut at chapters, and nothing in the folder is
  written over.
- Selecting, dragging, Escape and the keys: a click selects one page, ⌘A
  selects every page, Delete removes the selection, a drag past the
  threshold moves a page and a drag under it does not, and Escape puts a
  drag back. The arrows move and grow the selection, Enter is "Save as…",
  and ⌘+ sizes the grid and not the document.
- The page's menu duplicates, reverses and keeps only the selection, and
  adds a blank page the size of its neighbour; the Select menu picks odd,
  even and typed pages; undo reselects the pages a step was about; a
  heading names its file; a double-click reads the page; a dropped file is
  added.
- Closing with changes asks first, and closing without changes does not.
- In the reading view, a click on a thumbnail selects nothing, and Delete
  and ⌘A do what they do anywhere else.

## Order of work

1. This plan.
2. The Document menu, the start screen and the Open menu as above.
3. `Edit::Arrange` and `assemble`, with `Remove`, `Turn`, `Move` and
   `Insert` folded into it.
4. The organiser.
5. The thumbnail column as a map again: no selection, no foot bar, no drag,
   and Delete and ⌘A as they are anywhere else.

## Decided

- **The original is changed only when the reader says so.** "Save as…" is
  the default and the button Enter presses; "Save into paper.pdf" says
  which file it changes. Large removals from the document itself are asked
  about first, since undo lasts only until the window closes.
- **A view over the window, not a window of its own.** A second kind of
  window would need its own place in `windows.rs` and `session.rs`, and the
  harness cannot test windows. If readers want the organiser beside the
  document, it can become one later.
- **A draft, saved at once.** Writing every change as it is made, as the
  column did, works for one change at a time, but a combine cannot be
  written halfway, and a split is a decision about the whole document.

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

1. **Export highlights and comments.** "Highlights" in the Document menu's
   Export row, shown while the document has any. The save dialog suggests
   `paper (highlights).md`, and the extension typed decides the format:
   Markdown, plain text (`.txt`) or JSON (`.json`).
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
2. **Pictures into PDF.** "Combine files…" and "Add files…" in the
   organiser take JPEG and PNG files as well as PDFs, and each
   picture becomes one page. That is how a phone photo of a letter becomes
   a PDF. A page is the picture's own shape, fitted inside an A4 page
   (turned for a picture wider than it is tall). A JPEG goes into the file
   as it is, without being compressed again, and a JPEG whose EXIF data
   says it is turned is turned by the image's matrix, because pdfium ignores
   EXIF. A PNG is drawn onto white, since paper is what a page has under
   it, and made into a page by lopdf in the helper process, so that the
   memory its pixels take goes when that process ends. This is done before
   pdfium's lock is taken, so a large picture does not keep a page from
   being drawn.
3. **PDF into pictures and text.** Two buttons in the Document menu's
   Export row:
   - "Page picture" writes the page being read (or the
     page that was right-clicked) as a PNG at 300 dpi, as printed, without
     the theme's colours. A page larger than A0 is drawn at a lower
     resolution, so that the picture has at most 100 million pixels. The
     page is drawn in strips that go straight into the file.
   - "Text" writes the text of every page to a `.txt`
     file, with a blank line between pages. It reads the pages on a thread,
     as splitting at chapters does. A scan has no text until OCR (below)
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
