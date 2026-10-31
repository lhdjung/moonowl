## Big-picture issues

These come from a review of the twenty commits from `775a7c1` to `0553089`
(31 October 2026). Items marked "not confirmed" were found by reading the
code and have not been reproduced.

- **The link card is a second pop-up.** The brief allows one: the highlight
  colours under a selection. The link card is on by default
  (`show_link_previews`). Decide once it has been seen in use whether the
  brief or the default changes.
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
  Nothing refers to them, but highlights are not compacted, so they stay.
  Setting the Hidden flag while the page is drawn would stop pdfium making
  them, at the cost of writing `/F 0` into annotations that had no flags.
  An annotation whose `/AP` has no `/N` is not held back at all.
- **No highlight can be made or removed while a thread reads the
  document**, even one that is only written into the journal and never
  lets go of the document (`Viewer::busy`).
- **A field a signature locks (`/Lock`, FieldMDP) can be filled** if it is
  not also read-only. Only the certification (DocMDP) is checked.
- **A flattened copy keeps `/AcroForm` and `/Perms`** when lopdf cannot read
  it, or the helper is missing and the copy is over `objects::LIMIT`; the
  reader is not told.
- **A box clicked twice while a write is under way is turned twice**, since
  it still shows its old state, and a click pdfium ignores still writes the
  file and adds an undo step.
- **A request from an editor can be let go of by the wrong window**: a
  busy window that opens a document lets go of whatever request is
  waiting, even one for another document.
- **Two pages to a sheet still copy every file under one hold of pdfium's
  lock** before the sheets are laid out a page at a time.


## Concrete issues

Commit each point separately after fixing it, unless multiple consecutive commits change the previous ones a lot (and would therefore introduce a lot of spurious edits):

### Flattening and attachments

- **The flattened copy is loaded and saved under pdfium's lock**, as every
  new file "Save as…" writes is. The pages themselves are flattened one lock
  at a time.
- Files attached to a page (`/FileAttachment` annotations) are not listed;
  only `/EmbeddedFiles` is.

### Reading features

- **⌘-click reads every page with `text_aside`** rather than the search
  index, and holds the `making` slot while it does.
- **SyncTeX may map a click on a page with its own `/Rotate`, or a
  `/CropBox` offset from the `/MediaBox`, to the wrong line** (not
  confirmed). No fixture has either.
