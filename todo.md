## Big-picture issues

A. **Very high zoom goes soft.** `MAX_CANVAS_PIXELS` is 12 million
   (`src/viewer.ts`), so past roughly 300-400% on a large page the render is
   downsampled and the type blurs — at precisely the moment somebody is zooming
   in to read a footnote or inspect a figure. The cap is right; the answer is to
   render only the visible tile at full density rather than the whole page.
B. **The two platforms disagree about what a menu bar is.** Tauri installs its
   default macOS menu, so a Mac gets Copy, Select All, Close Window, Hide and
   Quit for free. Windows and Linux get none of it and the app supplies no menu
   bar of its own, so there is no discoverable Copy, Open Recent, Print or File
   menu at all. Decide it once rather than inherit it differently per platform.
C. **Can't print.** Done on macOS and Windows: ⌘P is the system's own print
   panel as a sheet on the reader's window (PDFKit, `src/print.rs`), or on
   Windows the system print dialog and pdfium drawing each page into the
   printer's GDI DC. Windows is compiled and clippy-clean via the msvc target
   but has not been run on a Windows machine. Linux has no system print dialog
   without GTK, so the `xdg-open` hand-off stays. A locked document is refused
   on Windows (no password is passed through).
D. **⌘P's notice lands after focus has left.** Moot: there is no success
   notice any more, and on macOS focus never leaves.


## Concrete issues
7. Done: the Windows CI failure was `tests/tracking.rs`, failing since it was
   added on 2026-09-11 — Segoe UI has no `trak` table and lands 1px apart at
   1x and 2x from rounding, against a 1px tolerance. The tolerance is 1% now.
8. Yes, with the `.exe`: it is an NSIS per-user install (`RequestExecutionLevel
   user`, into `%LOCALAPPDATA%\Moonowl`), settings go to the user's own
   directory, pdfium is loaded from beside the executable. The `.msi` is
   per-machine by cargo-packager's fixed WiX template and needs admin; there
   is no packager option to change that. The README says which is which.