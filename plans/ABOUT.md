# Moonowl, for readers

This page is for people who read PDFs and wonder whether Moonowl is worth
trying. It needs no technical knowledge. How the app is built is described in
[`ARCHITECTURE.md`](ARCHITECTURE.md), for programmers.

## What it is

Moonowl is a free PDF reader for macOS, Windows and Linux. It is made for
long reading: papers, books, reports, theses. It tries to stay out of the
way. The page fills most of the window, the controls are few and labelled in
words as well as symbols, and nothing moves on screen unless you made it
move.

It is free and open source, under the MIT and Apache licences. There is no
paid version, no account, no advertising, and nothing held back for an
upgrade. The app never connects to the internet on its own. Your settings,
themes and reading history stay on your computer as ordinary files.

## How it differs from other readers

**The page takes your colours.** Most readers with a dark mode either leave
the page white, which glares at night, or invert it, which turns photographs
and charts into negatives. Moonowl repaints the page in the colours of the
theme you chose, so dark text on white becomes, for example, light text on a
dark slate. Pictures, charts and coloured text keep their colours, adjusted to
the new background. You can turn that off for pictures if you prefer them
exactly as printed.

**There are twenty-one themes, and you can make your own.** They include
light ones (Moonowl Light, Sepia, Dim Paper, Solarized Light, Catppuccin
Latte, Rosé Pine Dawn, Professional) and dark ones (Moonowl Dark, Dracula,
Nord, Gruvbox, Tokyo Night, Amber Night for late reading, a green terminal
look, a high-contrast one, and more). A theme is a name and a few colours: the
text, the background, the accent, links and the selection. You can make one in
Settings with colour pickers and see the result on the page as you go. Each
theme is saved as a small text file, so you can also share one, or ask an AI
assistant to write one for you.

**Scrolling is continuous, and it stays that way.** Pages follow each other in
one long column, as on a web page. You can choose one page at a time, or two
pages side by side, in a menu or in Settings, but no stray key press will
switch it for you.

**It remembers.** Every document opens where you left it, at the same zoom.
When you start the app, it reopens what you were reading last. Every setting
is kept between sessions, and changing one setting never changes another.

**Highlights go into the file.** When you highlight, underline or strike out
a passage, or box in part of a figure, the mark is written into the PDF
itself, in the standard form. Preview, Acrobat, Zotero and other readers show
it too, and it is still there if you move or email the file. You can add a
comment to a highlight, change its colour, or undo it. Highlight colours come
in palettes, which you can edit.

**It is small and quick.** The whole app, PDF engine included, is about
22 MB. It uses Chrome's PDF engine to draw pages, so documents look as they do
in Chrome. When you are not doing anything, it uses no processor time at all.

## What it can do

### Reading

- Fit the page to the width or to the window, zoom, trim white margins, and
  show two pages side by side, with or without a cover page.
- Hide the toolbar for reading with nothing else on screen, or go full
  screen, and get the toolbar back with one click.
- Present a document as slides.
- See where a link leads before you click it: resting the pointer on a link
  shows a preview of the place. After following a link, one click takes you
  back.
- Jump to a reference such as "[12]" or "Figure 3" even when the document has
  no link there: hold ⌘ (Ctrl on Windows and Linux) and click it.
- Search the whole document, with options for matching case and whole words,
  and see every result listed in the sidebar.
- Browse the table of contents, page thumbnails and your marks in the
  sidebar.
- Use the page numbers printed in the book (such as "iv" or "213") rather
  than only the position in the file.
- Turn on the reading line, a band under the line you are reading that moves
  a line at a time with the arrow keys and helps you keep your place.
- Use the keyboard for everything. Every shortcut can be changed, and people
  used to Vim will find `j`, `k` and `g g` familiar.
- Open several documents at once, each in its own window (or tab, on macOS).

### For people who write in LaTeX

Moonowl reloads a document the moment it is recompiled, and keeps your place
and zoom. It does not flash or jump while the new version loads. With
SyncTeX, a click with ⇧⌘ (Ctrl and Shift on Windows and Linux) in the PDF
opens the matching line in your editor, and your
editor can show the matching place in the PDF.

### Working with the document

- **Fill in forms** (text fields, check boxes and radio buttons), and type
  text anywhere on a page that has no form fields, such as a scanned form.
- **Sign** with a signature you draw once and reuse, or with typed text such
  as a name or a date. A document that was signed digitally keeps its
  signature valid when you highlight in it.
- **Arrange pages**: remove, rotate, reorder, or copy pages; combine several
  files, or pictures, into one; split a file into several; split a book at
  each chapter; or lay pages out two to a sheet, or as a booklet, in a new
  file. All of this
  happens in a separate view. Nothing is saved until you press Save, and you
  can undo the save afterwards. The table of contents, the page numbers and
  links inside the document are kept correct. New files are made as small as
  they can be.
- **Compare two versions** of a document word by word, and save the
  comparison as a PDF report.
- **Make scanned pages searchable.** "Recognise text…" saves a copy in which
  the words on scanned pages can be searched, selected and copied.
- **Export** a page as a picture, the document's text, or a copy with its
  highlights and signatures drawn into the pages.
- **Print.**

## What it does not do

- It does not change the existing text of a PDF. No reader can do that
  reliably, because a PDF usually stores only the letters it already uses.
- It does not open EPUB, Word or other formats, and it does not convert PDFs
  into them.
- It does not sync between devices and has no mobile version.
- It does not read aloud yet.

## Things to know before you try it

- **The app is young and made by one person**, with the help of an AI
  assistant. It is tested thoroughly by automated tests, but it has not been
  used by many people yet.
- **The downloads are not signed by Apple or Microsoft.** macOS and Windows
  warn you the first time you open the app, and you have to allow it once.
  The [README](../README.md) explains the steps.
- **Printing on Windows has not been tried on a real Windows computer yet.**

## Getting it

Downloads for macOS (Apple silicon and Intel), Linux (AppImage, .deb and
.rpm) and Windows (installer and .msi) are linked from the
[README](../README.md).
