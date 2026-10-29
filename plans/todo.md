## Big-picture issues

Reviewed 29 October 2026, after the organiser landed. The design holds: one
`Edit::Arrange` for every change, a draft that writes nothing until Save,
pdfium for the pages and lopdf after it, every lopdf result read back and
compared. What follows is what still looks wrong or risky.

1. **Every "Save into" rewrites the reader's own file through lopdf**, and
   when pages come in from another file, `slim_pages` slims *every* page of
   the document, not only the ones that came in. The read-back check in
   `objects::smaller` catches a bad write, not a wrong decision: a resource
   slimmed away that pdfium resolves differently from lopdf would show up
   as a page drawn wrong in the reader's original, with undo lasting only
   until the window closes. A new file from "Save as…" is disposable; the
   open document is not. Slim only the pages that came in (`Work` can carry
   their positions), and consider keeping compaction for new files and
   removals (which need `prune` to shrink) but leaving the rest of the
   reader's file as pdfium wrote it.
3. **"Save as…" has no size ceiling.** "Save into" refuses past
   `IN_FILE_LIMIT`; `assemble` reads the document and every other file
   whole, loads each in pdfium, saves, loads again in `reorder`, and sends a
   copy down the pipe to the helper. A 400MB scanned book is held several
   times over. Either say so in the plan as a known ceiling or refuse past a
   limit as "Save into" does.
4. **No quick path for the commonest edit.** Saving pages 3–5 of a paper
   is: Arrange pages → Select → type "3-5" → Keep only → Save as. The plan
   accepts the organiser as the one place; a "Save pages…" entry that opens
   the organiser with the page typed in would cut it to two steps without
   adding a mode. Nothing in the keymap opens the organiser either.
5. **The lopdf helper is the app's own binary.** Where it cannot start, a
   PNG over 2.5 megapixels (`LIMIT / 4`) cannot become a page, and any
   document over 10MB is written as pdfium saved it, with labels, outline
   and links left wrong and said so in the notice. Fine as a fallback, but
   the notice is the only sign; a dev build run from `target/` and an
   installed app behave differently here.
6. **Of the plan's "Beyond page edits"**, items 1–3 (exports, pictures into
   PDF, PDF into pictures and text) are built. Forms, OCR, notes, passwords
   and the smaller-copy save are not started; "Keep the trimmed margins in
   the file" still waits for a request.

## Concrete issues

Commit each point separately after fixing it, unless multiple consecutive commits change the previous ones a lot (and would therefore introduce a lot of spurious edits):

1. **The page menu's height is stale** (`organiser.rs`, `sheet_menu`):
   `tall` is `7.0 * 35.0 + …`, but the menu now has nine items, a "Move to
   page" row and a rule, so a menu opened near the bottom of the window is
   placed as if it were ~70px shorter and runs off the window. Compute it
   from what the menu holds, or measure it.
2. **Numbering pages reads marks off every page.** `write_edit` passes
   `touched = None` to `write_step_on`, so the reopen after `Edit::Number`
   walks every page for highlights; a 1,700-page book pays a full markup
   walk for one label. Numbering moves no page, so it can pass
   `marked_pages` as `write_step` does.
3. **A page put in from another file is labelled by its position**
   (`labels::follow`), which can duplicate a label already in the document
   ("7" twice when the front matter is roman). Go-to by label becomes
   ambiguous. Label added pages by continuing the range they land in, or
   leave them unlabelled and let the reader number them.
5. **`Edit::spaces` indexes `sheets[to]`** with `to` bounded by the reopened
   page count, not by `sheets.len()`; they agree when the write succeeded,
   which is the only time `Paging::Edit` is kept, but `sheets.get(to)` costs
   nothing.
6. **`write_parts` compares paths as strings** (`files.contains(to)`) where
   the first check uses `same_file`; a file reached by another spelling is
   written over.
7. **Adding many files waits `STEADY` per file.** `open_for_organiser` and
   `read_whole` call `watch::whole`, which sleeps 150ms to see the size hold;
   twenty files chosen together wait three seconds serially on the thread
   before the first appears. Read the sizes once, sleep once, read again.
8. **A PDF added to the organiser is never `release`d** (`Source::drop`
   releases only a made file), so a thread still drawing its thumbnail goes
   on loading its pages after the organiser is gone. The document itself is
   released at once on a reopen; added sources should be too.
9. **Each blank page is a file of its own** in the temporary directory and
   a `Source` with its own pdfium document; ten blank pages are ten files
   and ten documents. One source per page size would do.
10. **"Split at each chapter" cannot be cancelled.** `busy` disables Cancel
    while the chapters' pages are read on a thread; a long book with many
    chapters reads a few hundred pages of text with no way out but waiting.
11. **Two questions in a row for a signed document** with many pages
    removed: "Change anyway" and then "Remove N pages?". One dialog could
    say both.
12. **`same_file` in `write_arranged` checks only the draft's sources.**
    Saving over a document open in another window is allowed and relies on
    the watcher there to reload it; that works, but a notice in the other
    window would say why its document just changed.
