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
C. **Can't print.** Done on macOS: ⌘P is now the system's own print panel,
   as a sheet on the reader's window (PDFKit's `PDFDocument` →
   `NSPrintOperation`, `src/print.rs`). Windows and Linux still hand the file
   to Edge / `xdg-open`. Windows could print through pdfium's own
   `FPDF_RenderPage(HDC, …)` into a GDI printer DC behind `PrintDlg`; Linux has
   no system print dialog without GTK, so the hand-off stays.
D. **⌘P's notice lands after focus has left.** Moot: there is no success
   notice any more, and on macOS focus never leaves.


## Concrete issues