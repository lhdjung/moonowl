# pdfmt: what Moonowl might take from it

**Status, 27 November 2026.** The clean copy (items 1 and 2 below) is
built: "Clean copy" under Export, `markup::copy_without`. The rest waits in
`roadmap.md`.

Asked on 27 November 2026: does pdfmt's issue 28 hold ideas worth adopting
for annotations, and does pdfmt have good ideas in general?

[pdfmt](https://github.com/eifelcode/pdfmt) is a command-line PDF tool
written in Bash. Each command is a short script over pdftk, qpdf, jq,
ImageMagick, OCRmyPDF and SANE. It is MIT-licensed. This assessment takes
ideas from its documentation, its command list and the behaviour of its
annotation commands, and none of its code, which is shell and of no use to
Moonowl in any case.

## Issue 28: removing highlights and comments

The request was for a way to remove all highlights, and perhaps all
comments, before a document is shared. The example was a lecturer who
uploads assigned reading and does not want to pass on their own notes. The
discussion settled on three commands, which pdfmt now has:

- `annotation list` counts the annotations in a file by type.
- `annotation show` prints each annotation's page, type, author and
  `/Contents`, or a link's address.
- `annotation remove` writes a copy without the annotations of the chosen
  types.

`show` and `remove` take the same filter: exact subtypes (`highlight`,
`text`, `ink` and so on) or three groups. `highlights` is the four text
markups, `comments` is notes, popups, drawings, shapes and stamps, and
`links` is links.

### What Moonowl has already

- **Removing marks.** "Remove all highlights" in the sidebar takes out
  every mark Moonowl recognises as one of the reader's: the four text
  markups and the area boxes it draws. It changes the document itself,
  asks first, and can be undone.
- **Extracting marks.** "Highlights" under Export writes Markdown, plain
  text or JSON, with each mark's quoted words and its comment. pdfmt's
  `show` prints only `/Contents`, so it cannot say which words a
  highlight covers. Moonowl's export is the better answer to "extract".
- **A flattened copy**, which draws the marks into the pages. That is the
  opposite of what the lecturer wants.

### What is worth adopting

1. **A clean copy for sharing.** An Export item, "Without annotations…",
   that writes a copy with the reader's marks and comments taken out and
   leaves the document itself as it is. Today the lecturer would have to
   remove every highlight, save a copy, and undo. This is the actual use
   case in the issue, and Moonowl has every part of it: removal through
   pdfium (which already takes a mark's popup with it), saving to a new
   file with `atomic_write_new`, and the Export menu.
2. **Choosing what goes.** The copy should offer two groups, not pdfmt's
   fourteen subtypes:
   - *Highlights*: the four text markups and Moonowl's area boxes.
   - *Notes and comments*: sticky notes (`/Text`), text boxes (`/FreeText`),
     and their popups and replies. These are often a reviewer's or the
     reader's own notes from another program, and Moonowl shows them but
     cannot remove them today.
   Links, form fields and signatures are never removed by this command.
   See "What to avoid" for why.
3. **Notes in the export.** The highlights export could include sticky
   notes and text boxes, with their page and author, so that "extract the
   comments" covers notes left by other programs too. `PageSource::notes_of`
   already reads them.
4. **The same commands in `moonowl-cli`.** `moonowl annotations list`,
   `moonowl annotations export` and `moonowl annotations remove`, with the
   same two groups. This is the command-line half of the issue, and it would
   use the same core functions as the Export items.

### What to avoid

pdfmt's version has faults that Moonowl's design already avoids, and the
new features should keep avoiding them:

- **It finds annotations by `/Type /Annot`.** The specification makes
  `/Type` optional in an annotation's dictionary, so an annotation written
  without it is not listed, shown or removed. Moonowl reads a page's
  `/Annots` through pdfium, which does not depend on `/Type`.
- **Removing a highlight leaves its popup.** pdfmt's `highlights` group
  does not include `/Popup`, so the popup stays in the page's `/Annots`
  with a `/Parent` that is gone. Replies to a note (`/IRT`) are left the
  same way. Moonowl removes a mark's popup with it, and should remove a
  note's replies with the note.
- **`all` removes form fields and links.** It takes `/Widget` out of every
  page's `/Annots` and leaves the fields in `/AcroForm`, which leaves a
  broken form. Links are part of the document, not something a reader
  added, as the issue's discussion itself pointed out.
- **`comments` includes `/Ink` and `/Stamp`.** In Moonowl those are drawn
  and typed signatures (`sign.rs`). A command meant to remove someone's
  notes before sharing must not remove a signature with them. If drawings
  and shapes are ever offered for removal, they should be a third group,
  with signatures left out or named separately.
- **The copy is a full rewrite.** qpdf writes the whole file again, which
  invalidates a digital signature without a word. Moonowl's writes keep a
  signed document's signature through an incremental update
  (`objects::incremental`), and a copy should do the same, or say plainly
  that it cannot.

## pdfmt's other ideas

Most of pdfmt's commands are page operations that Moonowl's organiser
already does:

| pdfmt | Moonowl |
| --- | --- |
| `merge all`, `merge insert` | Combine files in the organiser, at any place |
| `merge duplex`, `sort duplex` | The organiser's Interleave |
| `extract even`/`odd`, `remove even`/`odd` | Select odd or even pages in the organiser |
| `extract range`, `remove range` | The organiser; `moonowl pages` |
| `split all`, `split length` | Split every N pages |
| `split range` | Section starts in the organiser |
| `sort reverse`, `move`, `swap` | Drag in the organiser |
| `ocr add`, `ocr show` | "Recognise text…"; `moonowl ocr` |
| `stamp text` | "Add text here…" in a page's right-click menu |

What remains:

- **`split range` with a page in several files**, such as a cover page in
  every part (`1-7 1,8-10 1,11-14`). The organiser can do this with
  Duplicate, but only by hand. It is worth a line in `moonowl-cli`
  (`moonowl split --ranges`), where it costs almost nothing, and not in the
  app.
- **A grammar of `<area> <action>`.** pdfmt's `pdfmt split length …` and
  `pdfmt annotation remove …` read well and scale as commands are added,
  and every area has its own `help`. `moonowl-cli` should follow it as it
  grows: `moonowl annotations remove` rather than `moonowl remove-marks`.
- **Shell completions.** pdfmt ships a Bash completion file. Worth adding
  to `moonowl-cli` once its commands settle.
- **Refusing to write over a file.** pdfmt refuses an output path that
  exists. Moonowl and `moonowl-cli` already do the same.

What does not fit:

- **Scanning** through SANE. Moonowl is a reader, and a scanner is
  another program's job.
- **Workflows**, which are shell scripts that pdfmt runs from a folder of
  its own. A shell script that calls `moonowl` already is one, so a runner
  adds nothing.
- **`sort random`**, which has no use in reading.

## Recommendation

Build the clean copy (items 1 and 2) first. It is the use case of issue 28,
it is small given what exists, and it covers the reader's own marks and
other programs' notes, which pdfmt's design shows people expect. Notes in
the export (item 3) and the command-line commands (item 4) follow cheaply
from the same core functions. Of the rest of pdfmt, only `split --ranges`
and the command grammar are worth taking, and both belong to `moonowl-cli`.
