# A press that moves two pixels is not a click

Found against upstream `c6dec888`, on macOS 15 (Apple Silicon), and still
true of our fork at `755718f`: its `default.css` has no `user-select` for a
button, and `handle_pointermove` still looks no further up than the parent.
Working notes; not filed upstream.

## What happens

Press a button, move the mouse three pixels, release. The button does not
fire, and its label is highlighted as selected text instead. Three pixels is
most presses made with a mouse rather than a trackpad, so in practice the
toolbar of this reader answered perhaps one press in three.

`handle_pointermove` in `blitz-dom/src/events/pointer.rs` sets
`DragMode::Selecting` once the pointer has moved more than 2px with a button
down, and `handle_pointerup` dispatches a click only when the drag mode is
`None`. That is the right rule for a paragraph. A button is not a paragraph,
and no browser lets you select its label: Chrome, Safari and Firefox all carry
`user-select: none` for `button` in their user-agent stylesheets.
`blitz-dom/assets/default.css` does not.

`blitz-button-select.patch` adds it, beside the rest of the button block.

## And `user-select` does not reach the element from an ancestor

`user-select` is an inherited property, so saying it once on the window's root
should be enough. It is not: with `user-select: none` on the root element, a
button several levels down still behaves as `auto` and the drag still wins.
The property has to be set on the element the press lands on, or on its
immediate parent — which is as far as the check in `handle_pointermove` walks.

So `styles.rs` in this reader says it twice, once on `.root` and once on
`.root *`. That is the workaround, and it is only needed because of this.

## Reproducing it

`tests/chrome.rs::a_press_that_slides_a_little_is_still_a_press`, through
`Reader::press_and_drag`. The shared test harness cannot express this on its
own: `Harness::move_mouse_to` sends its moves with no buttons held, which is
exactly the field Blitz reads to decide a drag has begun, so a press-and-drag
is not expressible with it. `press_and_drag` builds the move events itself. A
`drag_to` on the harness that carries the buttons would be worth having.
