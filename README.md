# moonowl

What the [Moonowl](../moonowl) reader does to documents, from the command
line: combining files, saving some pages as a new file, splitting a book
at its chapters, comparing two
versions with a PDF report, recognising text on scanned pages, and SyncTeX
lookups. Every command is a thin layer over `moonowl-core`, the same code
the reader runs.

```sh
moonowl combine all.pdf one.pdf two.pdf
moonowl pages book.pdf 1-3,7,10- excerpt.pdf
moonowl split --chapters book.pdf chapters/
moonowl compare draft-1.pdf draft-2.pdf changes.pdf
moonowl ocr *.pdf                          # each as NAME-searchable.pdf
moonowl synctex paper.pdf 3 120 400        # → paper.tex:57
moonowl synctex paper.pdf paper.tex 57     # → page 3 at 72.0 410.2
```

No command writes over a file that is already there.

## Building

It needs the Moonowl checkout beside it (`../moonowl`), and pdfium, which
`../moonowl/scripts/pdfium.sh` downloads. A debug build finds pdfium there
by itself. A release build looks beside its own executable, then in
`MOONOWL_PDFIUM`.

`Cargo.lock` is the reader's own, copied, so that both build against the
same pdfium-render.
