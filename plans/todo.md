## Big-picture issues

These come from a review of the twenty commits from `775a7c1` to `0553089`
(31 October 2026). Items marked "not confirmed" were found by reading the
code and have not been reproduced.

- **The link card is a second pop-up.** The brief allows one: the highlight
  colours under a selection. The link card is on by default
  (`show_link_previews`). Decide once it has been seen in use whether the
  brief or the default changes.
- **Recognising text writes straight into the original.** Page edits write a
  copy by default ("Save as…" is what Enter presses), but "Recognise text"
  rewrites the pages of the open file. Undo covers it only while the window
  is open.
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

## Concrete issues

Commit each point separately after fixing it, unless multiple consecutive commits change the previous ones a lot (and would therefore introduce a lot of spurious edits):

### Markup

- **A box kept only in the journal is put back around its text**, not the
  area drawn. Over a scan there is no text, so it is always counted as lost.
- **No mark sets the Print flag (`/F 4`)**, so Acrobat leaves them out when
  it prints.
- The error message in `append` says "only a highlight has runs", but
  underlines and strike-outs have them too. A square's error says "the
  highlight could not be made".

### Flattening and attachments

- **Saving an attachment holds pdfium's lock while it decodes the file
  twice** (`save_to_bytes` calls `FPDFAttachment_GetFile` once for the size
  and once for the bytes) and keeps it whole in memory. A large attachment
  stops pages being drawn in every window for that long. pdfium cannot say
  the size without decoding, so there is no cheap cap. Reading the file with
  lopdf in the helper process would avoid both, at the cost of sending it
  the document.
- **The flattened copy is loaded and saved under pdfium's lock**, as every
  new file "Save as…" writes is. The pages themselves are flattened one lock
  at a time.
- **`FPDFPage_Flatten` may take links and comments without an appearance
  out of the copy** (not confirmed). No test checks that links and headings
  survive, or that a turned page flattens correctly. `/AcroForm` stays in the
  catalog with no widgets, and signature fields keep their `/V`.
- Files attached to a page (`/FileAttachment` annotations) are not listed;
  only `/EmbeddedFiles` is.

### Recognising text

- **Scans over 100MB are refused** (`markup::IN_FILE_LIMIT`), and large scans
  are what recognising text is most for.
- **A page with any text is skipped.** A scan with a stamp such as
  "Downloaded from JSTOR", or a text page number, is never recognised.
- **On Windows**, Tesseract opens a console window for every page (no
  `CREATE_NO_WINDOW`), and the common installer does not put it on the
  path (not confirmed).
- **Tesseract is given no `--dpi 300`**, so it assumes 70 dpi (not
  confirmed).
- **When Vision's classes are missing**, a Mac user is told that Tesseract
  is needed (`NO_ENGINE`).
- **Each page is drawn in colour at up to 48MB and then copied to grey.**
  pdfium can draw straight into a grey bitmap a quarter of the size, and
  macOS keeps the freed block.
- `CStr::from_ptr` is called on Vision's `UTF8String` with no check for
  null (`ocr.rs`).
- One font is used for a whole page. Where no font has every letter, the
  words Helvetica cannot set are left out, and a search does not find them.

### Forms and added text

- `FieldKind::Check(bool)` and `Radio(bool)` are read only by tests. Expose
  the state (`aria-checked`) or drop it.
- `forms.rs` repeats the signed-document check in
  `markup::saved_by_pdfium`, and builds the 35-field `FPDF_FORMFILLINFO` by
  hand; pdfium-render may already have a form-fill environment to reuse.
- `Filling.new_text` with a stand-in `index: 0` would be clearer as an enum.
- The commit for forms moved `NoteField`'s doc comment onto `FormField`.
- Missing tests: a password field (`/Ff 8192`), and adding text to an
  encrypted or certified document.

### Reading features

- **⌘-click reads every page with `text_aside`** rather than the search
  index, and holds the `making` slot while it does.
- **The reading line is not reset or moved on a reload or a page edit**, so
  it lands on the wrong line, or another page after a reorder.
- **`first_line_in_view` can read a page's text on the window's thread**
  under pdfium's lock, for pages up to three ahead, on every frame while no
  line has been chosen.
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
- The reading line after a recompile.
- `text_is_not_waited_for…` in `pdfium.rs` polls with `try_lock` every 1ms
  for at most 5s, and under heavy load (Windows CI) it can miss every gap.

### Wording

- Comments with clipped phrasing, against AGENTS.md "Writing": "An area is
  its box alone.", "An area is its box, and has no runs.", "Names only: a
  file's size is known only by decoding it.", "Asked of pdfium directly,
  because…", "FLAT_NORMALDISPLAY, and FLATTEN_FAIL is nought.", and some
  `ponytail:` notes in `pages.rs` and `ocr.rs`.
