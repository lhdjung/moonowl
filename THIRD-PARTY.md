# Third-party components

Moonowl's own source is MIT or Apache-2.0, at your option (see `LICENSE`). A
built app also carries the components below, each under its own licence. All of them are permissive:
nothing here places a condition on what you may do with Moonowl, and nothing
here has to be shared back. What they do ask for is attribution, which is why
this file exists and why the licence texts travel with the files they cover.

## Bundled in the app

| Component | Licence | Where its licence text lives |
| --- | --- | --- |
| [PDFium](https://pdfium.googlesource.com/pdfium/) — renders every page, and writes the markup | BSD-3-Clause | with the library, from [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) |
| [Dioxus](https://dioxuslabs.com) and [Blitz](https://github.com/DioxusLabs/blitz) — the interface, laid out and painted | MIT or Apache-2.0 | in each crate |
| Stylo, Parley, Taffy, Vello and the rest of the Rust crates | MIT or Apache-2.0, a few MPL-2.0 or BSD | in each crate |

The pdfium build shipped beside the binary is Google's own source, compiled by
`bblanchon/pdfium-binaries`. It has FreeType, libjpeg-turbo, libpng, OpenJPEG,
LittleCMS, ICU and abseil compiled into it, and every one of those wants its
notice reproduced with a binary distribution. So the `licenses/` folder from
that archive travels with the app — `Contents/Resources/licenses` in the `.app`,
`/usr/lib/Moonowl/licenses` on Linux, `licenses` beside the executable on
Windows — together with this file and Moonowl's own `LICENSE`. The About pane
has a button that opens it. `cargo tree` is the current answer for the rest,
and `cargo metadata` prints every licence field at once.

## The colour themes

Nine of the fifteen shipped themes carry the palette, and the name, of a
scheme somebody else designed. A handful of hex values is not much of a work,
but every one of these is released under MIT and the credit is owed either way:

| Theme | Palette | Author |
| --- | --- | --- |
| Dracula | [draculatheme.com](https://draculatheme.com) | Zeno Rocha |
| Gruvbox | [morhetz/gruvbox](https://github.com/morhetz/gruvbox) | Pavel Pertsev |
| Nord | [nordtheme.com](https://www.nordtheme.com) | Sven Greb |
| Solarized Light and Dark | [ethanschoonover.com/solarized](https://ethanschoonover.com/solarized/) | Ethan Schoonover |
| Tokyo Night and Storm | [enkia/tokyo-night-vscode-theme](https://github.com/enkia/tokyo-night-vscode-theme) | Enkia |
| Rosé Pine | [rosepinetheme.com](https://rosepinetheme.com) | Rosé Pine contributors |
| Glamour | after the [Charm](https://charm.sh) aesthetic; the colours are Moonowl's own | — |

Each theme file says where it departs from the palette it is named for.

## Not bundled: a webview

There isn't one any more. The interface used to be HTML in the system's web
engine — WebKit, WebView2, WebKitGTK — and is now HTML laid out by Blitz inside
the binary. Nothing links against a browser.

## The typeface you are reading

None. Every label in the app is set in whatever the machine resolves
`ui-sans-serif` to, so nothing has to be shipped and nothing looks foreign.
