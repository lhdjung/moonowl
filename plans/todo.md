## Big-picture issues

These come from a review of the twenty commits from `775a7c1` to `0553089`
(31 October 2026). Items marked "not confirmed" were found by reading the
code and have not been reproduced.

- **SyncTeX's answers are not checked against a new draft.** Compare,
  references and recognising text drop an answer that lands after a reload
  (`Viewer::on_thread_reading`). SyncTeX's are left alone, because an
  editor's request usually comes just as a compile lands, and dropping it
  would lose the jump. Its answers are places on the page, which a new
  draft moves only a little.
- **Tests cover each feature alone and seldom two at once**: a reload while
  the link card, a comparison or the reading line is up, a write during a
  thread's work, and other programs' annotations on a page Moonowl writes to.
- **CI never installs Tesseract**, so recognising text is not tested end
  to end on Linux or Windows. Its table of words is (`ocr::read_table`).

## From the audit of the fifty-nine commits (31 October 2026)

Fixed in the audit: the organiser's "Every … pages" selecting a page that
was gone, a glide surviving a new document, Tesseract with no time limit,
and the stand-in appearances left behind for links.

Still open after the review of the same commits:

- **A highlight leaves an empty appearance stream** in the file for every
  other annotation on its page that has none of its own (`markup::appear`).
  Each is 50 to 80 bytes, and nothing refers to it, but highlights are not
  compacted, so they stay. Setting the Hidden flag while the page is drawn
  would stop pdfium making them, at the cost of writing `/F 0` into
  annotations that had no flags, which a signed document then carries in
  its update. An annotation whose `/AP` has no `/N` is not held back at all.

## Concrete issues

Commit each point separately after fixing it, unless multiple consecutive commits change the previous ones a lot (and would therefore introduce a lot of spurious edits):

### Flattening and attachments

- **Every new file "Save as…" writes is saved under pdfium's lock**, which
  takes up to a second for a large book. The flattened copy is made in the
  other process instead (`objects::flatten`); the organiser's files could
  be too.

### Reading features

- **⌘-click reads page after page with `text_aside`** until it finds the
  line, every page for a reference that leads nowhere. It holds nothing up
  any more, but on a long book the answer can take seconds.
- **A line is measured across the page as drawn** (`select::line_around`),
  so a triple click, or the line an editor's SyncTeX request selects, takes
  only part of a line on a turned page.
