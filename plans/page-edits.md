# Page edits

Asked while reading: *should Moonowl split and merge PDFs, remove pages, and
do that kind of thing?* This is the answer and what the reader gets. All of
it is built. How it is built is in AGENTS.md, under "Page edits" and "What
lopdf does". What other programs have that Moonowl might have too is in
`roadmap.md`.

## Does it belong in Moonowl?

Yes, as long as reading does not change because of it.

The case for it is the reader's own work. People take a cover page or a blank
page out of a paper, turn the one scanned page that came out sideways, pull a
chapter out of a book to send to someone, or put two scans together into one
file. Each of these is a minute's job that otherwise means opening another
program, and Acrobat charges for all of them.

The case against it is the brief. Moonowl is a reader, and an editor is a
different kind of program, with modes, toolbars and a document that is
"unsaved". So page edits have three limits:

1. **The reading view does not change.** Every edit happens in the page
   organiser, which covers the window while it is open and is gone when it
   is closed. The toolbar, the thumbnail column and the keys of the reading
   view are about reading only. The one exception is "Number pages from
   here…" in the thumbnail menu, which names pages and does not move them.
2. **Nothing is half saved.** The organiser holds a draft, and nothing is
   written until the reader presses Save or "Save as…". Closing the
   organiser with changes in it asks first. The one exception is the
   document changing on disk: its pages may no longer be the ones being
   arranged, so the draft is put away and a notice says so. The reading
   view never has an unsaved state.
3. **The page's own text is not rewritten.** Adding to a page is a
   different matter: a form filled in, a note, a line of text typed onto a
   scanned form, or the invisible text that recognising a scan puts under
   it. `roadmap.md` says why rewriting text is not planned.

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
is, rather than a window of its own: the harness can test it, it uses the
window's theme, and it uses the window's file dialogs and notices.

## The edits

All of them are done in the organiser, in any order, and written together:

1. **Remove pages.**
2. **Turn pages in the file.** Not the view rotation in the toolbar, which
   turns the whole document for this reader and is never written. This
   writes `/Rotate` on the pages, so a sideways scan is upright in every
   program.
3. **Move pages**, by dragging them or by typing the page to move them to.
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
pages by number, or the odd or even ones**. **Interleave** puts a
two-sided scan in order: the backs, selected as the file added and
reversed if the stack was flipped, go in one behind each front. **Layout**
writes two pages to a sheet, or a booklet, when saving as new files.

Printing several pages to a sheet is left to the print dialog. Page numbers
stamped on pages and watermarks could be drawn with pdfium's text objects,
but nobody has asked for them; `roadmap.md` lists them.

## The page organiser

### Opening it

- **"Arrange pages…"** in the Document menu opens it on the document being
  read, with the page being read selected and in view. Under its name it
  says "Split, combine, turn or remove".
- **"Arrange pages…"** in the thumbnail menu does the same for the page that
  was right-clicked.
- **"Combine or split…"** on the start screen, beside "Open a document",
  and **"Combine or split files…"** in the Open menu open it with nothing in
  it and ask for files at once. It needs no document open. Someone who
  wants to merge files or split one looks for those words, not for
  "arrange".
- **A picture dropped on the window** with no organiser open opens one, on
  the document if there is one, with the picture in it.

### What it shows

```
┌────────────────────────────────────────────────────────────────────────┐
│ Arrange pages   [+ Add files…]  [↺ Left] [↻ Right] [🗑 Remove]          │
│                 [✂ New file here] [Split ▾] [Select ▾] [Layout ▾]       │
│                 [− +]                     [Cancel] [Save] [Save as…]   │
├────────────────────────────────────────────────────────────────────────┤
│  File 1 · 5 pages                                                      │
│  ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐                                     │
│  │    │ │ ✓  │ │    │ │    │ │    │                                     │
│  └────┘ └────┘ └────┘ └────┘ └────┘                                     │
│    1      2      3      4      5                                       │
│  File 2 · 3 pages                                   [Join with above]   │
│  ┌────┐ ┌────┐ ┌────┐                                                  │
│  │    │ │    │ │    │                                                  │
│  └────┘ └────┘ └────┘                                                  │
│    6   scan.pdf · 1 scan.pdf · 2                                       │
└────────────────────────────────────────────────────────────────────────┘
```

- **A grid of pages**, drawn as the file has them (no theme rotation, no
  trim), in the theme's colours as the thumbnails are. "Smaller" and
  "Larger" change the size of every page, which fits in a square between
  80 and 480 pixels on a side, and the size is a setting. Only the rows in view are drawn, with the same
  margin the thumbnail column keeps, so a 1,700-page book costs a screenful.
- **A bar narrower than what it holds** drops the count first, then the
  words of the tools, then the title, and last the turn buttons and the
  page size, which the keyboard still has.
- **Each file that will be written is a section**, with a heading that says
  how many pages it has. With one file there is one section. A section
  after the first has "Join with the file above", and with several, each
  heading's name can be typed.
- **Under each page**, what the page is called in its own file, and the
  file's name when pages come from more than one file. A blank page is
  called "Blank page", and comes from no file.
- **A turned page is shown turned.**

### Working in it

- **Selecting** is as in a file manager: a click selects a page alone,
  ⌘-click (Ctrl elsewhere) adds or takes away a page, ⇧-click selects every
  page from the last one clicked, and ⌘A selects every page. The arrows
  move the selection a page along or a row up or down, and with Shift grow
  it. **Select** in the bar offers every page, the odd pages, the even
  pages, and a field for pages by number ("3, 5-8, 10-", positions in the
  grid) with "Select" and "Keep only" beside it, so that a few pages of a
  document are typed, kept and saved. Escape closes a menu, then clears
  the selection, and then closes the organiser.
- **Left, Right and Remove** act on the selection. Delete (and ⌘⌫ on a Mac)
  removes too. The last page cannot be removed. ⌘L and ⌘R, which turn the
  view in the reading view, turn the selected pages here.
- **A right-click on a page** opens its menu, over the selection (which
  becomes that page alone unless it is part of it): Turn left, Turn right,
  Duplicate, Add a blank page after, Reverse (these pages, or all with one
  or none selected), Interleave (the selected pages one behind each of
  the others, or the second half behind the first with every page
  selected), a field to move the pages to a typed page, New file here or
  Join with the file above, Keep only these pages, and Remove.
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
  or picture dropped on the window while the organiser is open is added
  the same way, never opened over the document behind. A picture's page is
  the picture's own shape, fitted inside an A4 page (turned for a picture
  wider than it is tall).
- **"New file here"** starts a new file at the first selected page. A
  file always begins at the first page, so selected pages that run on from
  it are passed over: the file begins at the next selected page, or after
  them when no other page is selected. Where a file already begins there,
  the button is "Join files" and takes the cut back. With several files, a click on
  a file's heading types its name, which is the file's name when it is saved
  ("02 Appendix.pdf"). **Split** offers "At each
  chapter" (for a document with chapters) and "Every … pages",
  with a field for the number. Both replace the cuts that were there.
  Where a chapter begins and ends is decided as AGENTS.md says under
  "Split at each chapter".
- **⌘Z and ⇧⌘Z** undo and redo steps in the draft, and select the pages the
  step was about, so that a removal undone shows what came back. The steps
  go when the organiser closes.

### Saving

- **"Save as…"** writes new files. With one section it asks for a file name
  (`paper (arranged).pdf`, or `paper (combined).pdf` when pages come from
  several files) and says so in a notice with an Open button. With several
  sections it asks for a folder and writes one file per section into it,
  writing over nothing there. When any section has a name, from a chapter
  or typed in its heading, every file is numbered and named after its
  section (`1 Introduction.pdf`, and `2 part 2.pdf` for one with no
  name), so that the files sort in order; with ten files or more the
  numbers have two digits (`01 Introduction.pdf`); otherwise they are numbered after
  the first file (`paper (part 1).pdf`). The organiser closes, and a window
  that had no document open opens the new file when there is one.
- **"Save"** writes the arrangement into the document itself, with undo in
  the reading view (⌘Z puts the whole file back). It is offered when the
  organiser was opened on a document, there is one section, no page of
  the document is in it twice, and the pages are one to a sheet. Writing into the document follows the rules
  every write of the reader's own follows: a read-only, encrypted,
  certified or very large document refuses, a signed one asks once before
  its signature is broken, and removing more than `arrange::ASK_BEYOND`
  pages asks first. The organiser closes once the write lands.
- **"Cancel"** with changes asks "Close without saving?", with "Keep
  arranging" as the default.
- **"Save as…" has no size limit**, where "Save" refuses a document
  over 100MB. A very large scan is held in memory several times over
  while it is written, about five times its size.

### Writing into the document itself

Two page tools write into the open document: Save in the organiser and
"Number pages from here…". Both are the reader asking for that file to be
changed, so neither asks where the change goes. Both go through
`write_step`, as a highlight does, so ⌘Z in the reading view puts the whole
file back, with the bookmarks and the journal as they were.

The first version asked at the first page edit whether edits went into a
copy, and then read on in the copy. The organiser makes that question
unnecessary: "Save as…" is the copy, and it is the button Enter presses.

### What the reader is told

Every save ends in a notice that says what happened, and a save into the
document also says how to take it back:

- "Saved paper (arranged).pdf, 12 pages." with an Open button.
- "Saved 3 files in Chapters." when there were several sections.
- "Arranged paper.pdf: removed 2 pages, turned 1 page, added 3 pages
  from other files and added 1 blank page. ⌘Z puts it back." Only the
  parts that happened are said.
- "The 2 headings that pointed at removed pages are taken out of the
  contents." (only when that is true), or "2 headings pointed at removed
  pages and now lead nowhere." when lopdf could not read the outline
- "Page labels in this document did not move with the pages." (only when the
  document has labels lopdf could not read)
- "Links from one of those pages to another do not work here." (only when a
  link between added pages could not be written again)
- "Links on the pages put in that lead to pages left out do not work."
  after Save (only when an added page links to a page of its file that
  was not added)
- "Links in them to pages that were left out do not work." after "Save
  as…" (only when a saved page links to a page that was not saved with it)

Refusals reuse the markup sentences: an encrypted or very large document
says it cannot be changed, and so does a certified one, at once, since no
certification allows a page edit. A read-only document refuses Save, and a
signed one asks once before its signature is broken. A highlight on a
signed document asks nothing, because it is appended after the signed
bytes; a page edit cannot be. "Save as…" works for a read-only or signed
document too, because it does not write to it.

## The reading view

"Arrange pages…" in the Document menu is the only page tool there.

The thumbnail column is a map of the document: a click goes to a page, and
nothing is selected. A right-click offers "Arrange pages…" and "Number pages
from here…". "Number pages from here…" opens a field holding the clicked
page's label; what is typed there ("iv", "1", "A-1") numbers the pages from
that one up to the next range the document starts.

## Decided

- **The original is changed only when the reader says so.** "Save as…" is
  the default and the button Enter presses. Save writes into the document,
  as Save does in every program, beside "Save as…", which writes a new
  file. Large removals from the document itself are asked about first,
  since undo lasts only until the window closes.
- **A view over the window, not a window of its own.** A second kind of
  window would need its own place in `windows.rs` and `session.rs`, and the
  harness cannot test windows. If readers want the organiser beside the
  document, it can become one later; `roadmap.md` lists it.
- **A draft, saved at once.** Writing every change as it is made, as the
  column did, works for one change at a time, but a combine cannot be
  written halfway, and a split is a decision about the whole document.
