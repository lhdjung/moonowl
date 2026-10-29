## Big-picture issues

Reviewed 29 October 2026, after the organiser landed. The design holds: one
`Edit::Arrange` for every change, a draft that writes nothing until Save,
pdfium for the pages and lopdf after it, every lopdf result read back and
compared. What follows is what still looks wrong or risky.

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
