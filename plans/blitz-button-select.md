# A press that moves two pixels is not a click

Found against upstream `c6dec888`, on macOS 15 (Apple Silicon). Checked again
on 27 November 2026 against our fork at the rev `Cargo.toml` pins,
`69bbccb`: it is still true there. Its `default.css` has no `user-select`
for a button, and `handle_pointermove` still looks no further up than the
parent. These are working notes and have not been filed upstream.

## What happens

Press a button, move the mouse three pixels, release. The button does not
fire, and its label is highlighted as selected text instead. Most presses
made with a mouse rather than a trackpad move that far, so in practice the
toolbar of this reader answered perhaps one press in three.

`handle_pointermove` in `blitz-dom/src/events/pointer.rs` sets
`DragMode::Selecting` once the pointer has moved more than 2px with a button
down, and `handle_pointerup` dispatches a click only when the drag mode is
`None`. That is the right rule for a paragraph. A button is not a paragraph,
and no browser lets you select its label: Chrome, Safari and Firefox all carry
`user-select: none` for `button` in their user-agent stylesheets.
`blitz-dom/assets/default.css` does not.

`blitz-button-select.patch` adds it, beside the rest of the button block.

## And `user-select` on an ancestor does not reach the element

`user-select` is not an inherited property; Stylo keeps it in the `ui`
struct, which is not inherited. The specification says instead that `auto`
takes its meaning from the parent, and the parent's from its parent in turn.
Blitz follows that rule for one step only: when the node the press landed on
says `auto`, `handle_pointermove` reads its parent, and if the parent also
says `auto`, the drag becomes a selection. So with `user-select: none` on the
root element, a button several levels down still behaves as `auto` and the
drag still wins. The property has to be set on the element the press lands
on, or on its immediate parent.

The fork already has a helper that walks all the way up, `selectable` in the
same file, but `handle_pointermove` does not use it.

So `styles.rs` in this reader says it twice, once on `.root` and once on
`.root *`. That is the workaround, and it is only needed because of this.

## Reproducing it

`tests/chrome.rs::a_press_that_slides_a_little_is_still_a_press`, through
`Reader::press_and_drag`, which builds the move events itself with the main
button held. The shared harness's `move_mouse_to` sends its moves with no
buttons held, which is the field Blitz reads to decide that a drag has begun.
The harness at `69bbccb` also has `Harness::drag`, which does hold the
button, so `press_and_drag` could now be written with it.
