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
Commit each point separately after fixing it:
1. Done: "Wait before hiding it" takes 0 to 30 seconds, one decimal place, with a point or a comma. Zero hides the pointer a fifth of a second after it stops, so a slow move does not flicker.
2. Done: a unit is the muted grade at the number's own size, rather than the faint grade a size smaller.
3. Done: a click into any text field that did not have the keyboard selects all of it; a second click places the caret, and a drag keeps its own selection.
4. But then, the same characters should be visible when selected – in dark mode, they are currently near-invisible.
5. Some themes have link colors that make them hard to read. Thinking of Gruvbox, Sepia, Dark Forest, Nord, Solarized Light, Solarized Dark, and even Moonowl Dark. Maybe same for other themes, too. Fix the link colors without changing any other colors.