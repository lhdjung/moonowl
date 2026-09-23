**Moonowl's copy of blitz-paint at the pinned rev** (`c6dec888` in the root
`Cargo.toml`), with one change: a text field's selection is painted in the
colours its style names in `--selection-background` and `--selection-color`
(standing in for `::selection`, which Blitz does not have) instead of a fixed
pale blue under the field's own ink. The selection is drawn *over* the text,
clipped to itself, so the selected glyphs take the second colour. See
`selection_colors` and `draw_text_input_text` in `src/render.rs`, and
`stroke_text`'s `ink` in `src/text.rs`. `Cargo.toml` is upstream's with the
workspace values written in. The root `Cargo.toml` patches it in under
`[patch."https://github.com/DioxusLabs/blitz"]`; bumping the Blitz rev means
re-copying and re-applying this.
