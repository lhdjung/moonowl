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
- **CI never installs Tesseract**, so recognising text is not tested on
  Linux or Windows. A test on a saved TSV string would at least cover the
  parser.

## From the audit of the fifty-nine commits (31 October 2026)

Fixed in the audit: the organiser's "Every … pages" selecting a page that
was gone, a glide surviving a new document, Tesseract with no time limit,
and the stand-in appearances left behind for links. Still open:

- **A scan drawn through a Form XObject is not counted as a picture**
  (`pictured_aside` reads only the page's own image objects), so a stamped
  scan from Word, InDesign or an Acrobat "optimise" is taken for a typed
  page and skipped.
- **Any typeset page with a figure over a tenth of it is recognised whole**
  at 300 dpi, and the words are filtered afterwards (`over_text`): a book of
  figures is minutes behind one notice, and a recognised word whose box
  misses every glyph centre is written in twice. No test covers a typeset
  page with a figure.
- **A booklet holds pdfium's lock for the whole save** (`arrange::lay_out`),
  where `export::flattened` takes it a page at a time.
- "Recognise text…" does not check `standing.large`, and `ocr::write` reads
  the file with no `watch::whole()` check.
- `busy()` refuses highlights, form fills and signatures while any thread
  job runs, saving a page as a picture or an attachment included, which
  need no document handle.
- `synctex::take_for` canonicalizes two paths on the drawing thread; the
  SyncTeX notice sends the reader to Settings before anyone checks that a
  `.synctex(.gz)` is beside the document; the `X Offset:`/`Y Offset:`
  header lines are read as records and dropped (not confirmed for
  xdvipdfmx).
- The reading line runs `select::lines` for every scroll frame while it is
  on; a cached answer keyed on the scroll position and page would make it
  nothing.
- A draft of only blank pages is offered as `blank (arranged).pdf` with no
  folder.
- `plans/page-edits.md` still narrates what the organiser's button "said at
  first".

## Concrete issues

Commit each point separately after fixing it, unless multiple consecutive commits change the previous ones a lot (and would therefore introduce a lot of spurious edits):

### Flattening and attachments

- **The flattened copy is loaded and saved under pdfium's lock**, as every
  new file "Save as…" writes is. The pages themselves are flattened one lock
  at a time.
- Files attached to a page (`/FileAttachment` annotations) are not listed;
  only `/EmbeddedFiles` is.

### Forms and added text

- `Filling.new_text` with a stand-in `index: 0` would be clearer as an enum.
- The commit for forms moved `NoteField`'s doc comment onto `FormField`.
- Missing tests: a password field (`/Ff 8192`), and adding text to an
  encrypted or certified document.

### Reading features

- **⌘-click reads every page with `text_aside`** rather than the search
  index, and holds the `making` slot while it does.
- **SyncTeX may map a click on a page with its own `/Rotate`, or a
  `/CropBox` offset from the `/MediaBox`, to the wrong line** (not
  confirmed). No fixture has either.
- Going to a page with no box and scrolling to a line is written three
  times (`reveal_line`, `go_to_change`, `step_reading_line`).
- `refs.rs` splits text into lines its own way, as `select.rs` and
  `search.rs` already do.
- The scroll-speed stepper starts at 5 and `still_speed` clamps from 1.
- The "Editor for LaTeX sources" note in Settings puts three sentences of
  instructions in one hint, and "Editor command" has no note.

### Tests to add

- An area box recoloured and removed; the test only writes one.
- Underline and strike-out on a page with `/Rotate`.
- A compressed attachment, and a large one.
- Recognising text on a turned page (the fixture has `/Rotate 90`), the
  refusal of a signed document, and words that are not Latin.
- A comparison of a large document, for time and memory, and a comparison
  whose result lands after a reload.
- Scrolling by itself reaching the end, with `stats::RENDERS` then still.
- `text_is_not_waited_for…` in `pdfium.rs` polls with `try_lock` every 1ms
  for at most 5s, and under heavy load (Windows CI) it can miss every gap.

### Wording

- Comments with clipped phrasing, against AGENTS.md "Writing": "An area is
  its box alone.", "An area is its box, and has no runs.", "Names only: a
  file's size is known only by decoding it.", "Asked of pdfium directly,
  because…", "FLAT_NORMALDISPLAY, and FLATTEN_FAIL is nought.", and some
  `ponytail:` notes in `pages.rs` and `ocr.rs`.
