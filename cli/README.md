# moonowl

What the Moonowl reader does to documents, from the command line: combining
files, saving some pages as a new file, splitting a book at its chapters,
comparing two versions with a PDF report, recognising text on scanned
pages, and SyncTeX lookups. Every command is a thin layer over
`moonowl-core` (`../core`), the same code the reader runs.

```sh
moonowl combine all.pdf one.pdf two.pdf
moonowl pages book.pdf 1-3,7,10- excerpt.pdf
moonowl split --chapters book.pdf chapters/
moonowl compare draft-1.pdf draft-2.pdf changes.pdf
moonowl ocr scans/                          # every PDF in it, each as NAME-searchable.pdf
moonowl ocr scans/ --skip-existing          # only the files without a copy yet
moonowl ocr a.pdf b.pdf --into searchable/  # the copies in another folder
moonowl ocr scan.pdf --to scan-text.pdf     # one copy, named
moonowl synctex paper.pdf 3 120 400         # the source line of a place
moonowl synctex paper.pdf paper.tex 57      # the place of a source line
```

No command writes over an existing file.

## Installing

`scripts/install-cli.sh` builds it and installs it as `~/.local/bin/moonowl`,
with pdfium beside it in `~/.local/lib/Moonowl`, on macOS and Linux.

In the workspace it is built as `moonowl-cli` (`cargo build -p moonowl-cli`),
since it shares the target folder with the app's `Moonowl`, and macOS does
not tell `moonowl` and `Moonowl` apart. A debug build finds the pdfium that
`scripts/pdfium.sh` downloaded by itself.
