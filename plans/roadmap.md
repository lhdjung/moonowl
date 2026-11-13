# What Moonowl might do next

Asked in October 2026: *what do other PDF programs have that Moonowl should
have too?* The first look was at Readest, an ebook reader, and JOPDF, a free
editor for Windows. A second look, at the end of October, was at readers
built for papers and books: Skim, Sioyek, Zotero, Okular, Preview, Acrobat,
Stirling-PDF and KOReader.

Only their lists of features were used, never their code. Readest is AGPL;
Okular, Sioyek, Xournal++ and PDF Arranger are GPL; JOPDF and the commercial
programs are closed. Names that are trademarks, such as Acrobat's "Liquid
Mode", are not used either.

The test for each feature is whether it is the reader's own work, whether
the stack can do it well, and what it costs the binary.

This file lists only what is not built. What has been built is described in
AGENTS.md and the code. The plan as it stood before it was split, with the
reasoning behind each built feature, is `plans/page-edits.md` at commit
`f30fdef` (`git show f30fdef:plans/page-edits.md`).

## Next

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
  blacked out, and then recognising the text gives it back its other words.
- **Look up a word** from the selection popover: the system dictionary on
  macOS, a web search elsewhere.

## Additions to what is built

- **Following a reference that has no link, through the link preview.**
  ⌘-click follows "[12]" or "Figure 3" (`Viewer::follow_reference`);
  resting on such a phrase could show where it leads in the same card a
  link shows.
- **"Copy page as image".** It needs the clipboard to take pictures, which
  the `Clip` door does not yet do.
- **Forms: lists, combo boxes and a form's own scripts**, later, if at all.
  Text fields, check boxes and radio buttons are filled in now.
- **Recognising text on Windows with `Windows.Media.Ocr`**, the engine the
  system already has, rather than Tesseract, which most Windows machines do
  not have. Tesseract is also asked for English only.
- **Keep the trimmed margins in the file.** `crop.rs` already measures them.
  Writing them as `/CropBox` makes a paper readable on an e-reader. It
  changes what every other program shows, so it waits for a request.
- **The page organiser in a window of its own**, beside the reader, if
  readers want it. See "Decided" in `page-edits.md`.

## Further down

- A presenter view with the next page and a timer (Skim, pdfpc).
- Shapes and arrows (Preview, Okular).
- Stamps.
- Page numbers stamped on pages. They could be drawn with pdfium's text
  objects, but nobody has asked for them.

## Not planned

- **Rewriting the page's own text.** A PDF usually carries only the
  letters its text uses, so a new letter often has no shape to draw it
  with, and lines never reflow. It works in a demonstration and fails on
  real files. Adding text ("Add text here…") gives most of what people want
  from it.
- **Word, PowerPoint or OpenDocument into PDF.** That needs an office
  suite's layout engine. At most, Moonowl could hand the file to
  LibreOffice when it is installed.
- **PDF into Word or EPUB.** The results are poor in every program,
  because a PDF does not record its paragraphs or its reading order.
- **Reading EPUB, syncing, translation and reading statistics**, which are
  Readest's own ground. They would make Moonowl a different program, and
  everything in it is built on fixed pages.
- **Watermarks, Bates numbers and headers**, which are office work.
- **Printing several pages to a sheet**, because the print dialog does it.
  The organiser's Layout writes two pages to a sheet, or a booklet, into a
  new file.
