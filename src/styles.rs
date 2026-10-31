//! The stylesheet, and the theme as a set of variables written onto the root.
//!
//! **The theme is variables rather than text interpolated into the sheet**,
//! because a changed sheet panics. A `<style>` whose text changes is a
//! stylesheet mutation, and Stylo answers one by walking the tree with
//! `StylesheetInvalidationSet`, which calls `each_class` on any element
//! *snapshot* it finds — and Blitz's cheap state-only snapshot
//! (`snapshot_node_state_only`) has no attributes, which
//! `ServoElementSnapshot::each_class` unwraps. An attribute change is a
//! snapshot that does carry attributes, and a theme change re-resolves
//! variables instead of re-parsing the sheet.
//!
//! Three properties are missing in Blitz and are handled here: `position:
//! fixed` (the root is a flex column, so nothing needs to leave the flow),
//! `overflow: auto` (`scroll`, with `scrollbar-width: thin`), and
//! `text-overflow: ellipsis` (a `mask-image` fade).

use crate::palette::Palette;

/// Give a window the font that the start screen's "Moonowl" is set in.
///
/// The font is Cinzel Decorative, under the SIL Open Font Licence (see
/// `fonts/`). It is embedded in the binary rather than looked up on the
/// system, so the title looks the same on every platform.
pub fn title_font(doc: &mut blitz_dom::BaseDocument) {
    use blitz_dom::net::{Resource, ResourceLoadResponse};
    const FONT: &[u8] = include_bytes!("../fonts/CinzelDecorative-Regular.ttf");
    doc.load_resource(ResourceLoadResponse {
        // No real request has this number, so loading the font cannot mark
        // one as finished.
        request_id: usize::MAX,
        node_id: None,
        resolved_url: None,
        result: Ok(Resource::Font(
            blitz_traits::net::Bytes::from_static(FONT),
            Default::default(),
        )),
    });
}

/// The theme, as the declarations that go in the root's `style` attribute.
///
/// Every shade the chrome uses is derived from the five colours a theme names,
/// which is why a five-line theme file is enough.
pub fn variables(theme: &Palette) -> String {
    let hex = crate::palette::hex;
    format!(
        // **The arithmetic for each is in `Palette`**, where each is named
        // after the variable it stands for. The bar's four are a family of their own because the
        // toolbar sits on the *paper* and everything else floats on the
        // surface.
        "--text: {}; --paper: {}; --accent: {}; --surface: {}; --line: {}; \
         --muted: {}; --faint: {}; --note: {}; --hover: {}; --sunk: {}; \
         --surround: {}; --scrim: {}; --accent-soft: {}; --accent-contrast: {}; \
         --positive: {}; --negative: {}; --negative-contrast: {}; \
         --bar-hover: {}; --bar-sunk: {}; --bar-line: {}; --bar-accent: {}; \
         --accent-ink: {};",
        hex(theme.text),
        hex(theme.background),
        hex(theme.accent),
        hex(theme.surface()),
        hex(theme.line()),
        hex(theme.muted()),
        hex(theme.faint()),
        hex(theme.note()),
        hex(theme.surface_hover()),
        hex(theme.surface_sunk()),
        hex(theme.surround),
        theme.scrim(),
        hex(theme.accent_soft()),
        hex(theme.accent_contrast()),
        hex(theme.positive()),
        hex(theme.negative()),
        hex(theme.negative_contrast()),
        hex(theme.bar_hover()),
        hex(theme.bar_sunk()),
        hex(theme.bar_line()),
        hex(theme.bar_accent()),
        hex(theme.accent_ink()),
    ) + &format!(
        // What a page is before the renderer has reached it, which under a
        // recolouring theme is the theme's paper and otherwise is white.
        // On a zoom step or a jump a re-keyed page is a new node with no
        // texture, and for the frame or two before pdfium answers, `.page`'s
        // own background is what is on screen. A white rectangle on a dark
        // theme would be a flash; the theme's paper is the page arriving.
        " --page: {};",
        hex(theme.page()),
    ) + &format!(
        // The theme's paper with an alpha on it, which is what the drop hint
        // is drawn over: the window has to stay visible under it. Written as
        // an eight-digit hex rather than through `color-mix`, so that what
        // reaches the renderer is a colour and not a function it may or may
        // not implement.
        " --veil: {}e0;",
        hex(theme.background),
    ) + &format!(
        // What a match is painted in. The theme's own selection colours,
        // because a found word and a selected word are the same statement —
        // *this part of the page is the part you asked about* — and a theme
        // that has thought about one has thought about the other. The
        // current match is the accent, so that stepping through matches is
        // visible without reading the count.
        " --found: {}; --found-now: {}; --found-ink: {};",
        hex(theme.selection_area),
        hex(theme.accent),
        // The ink on a selected passage, which a theme names and otherwise
        // derives.
        hex(theme.selection_text),
    ) + &{
        // The scrollbar's two shades, which stand on the surround.
        let on = theme.on_surround();
        format!(
            " --surround-muted: {}; --surround-faint: {};",
            hex(on.muted()),
            hex(on.faint()),
        )
    } + &format!(
        // A real selection in any field, in the theme's selection colours
        // kept readable: the page draws them as named, the interface's own
        // words must read. Blitz has no `::selection`; the Blitz fork's
        // painter reads these instead.
        " --selection-background: {}; --selection-color: {};",
        hex(theme.selected().0),
        hex(theme.selected().1),
    )
}

/// The sheet itself, which never changes and is therefore parsed once.
pub const SHEET: &str = r#"
/* Every `width`, `min-width` and `height` in this sheet includes the padding.
   Some rules below say it again for themselves; each one is load-bearing
   where it stands and agrees with this rule. */
* { box-sizing: border-box; }

/* The window's height handed down as a percentage rather than said as `100vh`
   on `.root`: Stylo re-evaluates viewport units only where it remembers using
   them, and after a move to a screen of another density it may not, which
   leaves the reader short above a white band. A
   percentage is layout, and layout always has the window's size. `#main` is
   the element dioxus-native mounts the app in. */
html, body, #main { height: 100%; }

body { margin: 0;
  font-family: ui-sans-serif, -apple-system, "Helvetica Neue", Arial, sans-serif;
  font-size: 13.5px; line-height: 1.45;
  /* **`opsz` 17, and letter-spacing nought.** `-apple-system` resolves to
     SF, a variable font whose `opsz` axis runs from 17 to 96 and defaults to
     28, the design for a headline. Parley sets no variation coordinates of
     its own, so the axis is set here to 17, the text design; 17 rather than
     the size because the axis starts there. The two rules above that size
     set their own.

     Tracking is not in this sheet: SF's `trak` table supplies it, and the
     parley fork (`moonowl-2` in `Cargo.toml`) reads that table at the CSS
     size rather than at the size on screen. A letter-spacing on top would
     count it twice. */
  letter-spacing: 0px;
  font-variation-settings: "opsz" 17; }

 /* `position: relative` so that the Settings scrim, which is absolute, is
   measured against the window rather than against whatever Blitz would
   otherwise pick — without it the scrim would start below the toolbar and
   leave the bar bright behind a window that claims to be modal. */
.root { position: relative; display: flex; flex-direction: column; height: 100%;
  /* The surround, not the paper. */
  background: var(--surround); color: var(--text);
  /* **The chrome is not text to be selected**, or a button could be
     highlighted instead of pressed. Blitz decides a gesture is a selection
     as soon as the pointer moves two pixels with the button down — which is
     most presses made with a mouse — and from then on the press is a drag
     and the click never happens. A browser does not have this
     problem because its user-agent stylesheet says `user-select: none` for a
     button; Blitz's does not, so the rule is said here for the whole window.
     See `blitz-button-select.md`. */
  user-select: none;
  /* **And the window itself does not scroll.** A wheel with nowhere to go
     chains outward in Blitz — a pane that cannot move sideways hands the
     gesture to its parent, and the last parent is the viewport, which scrolls
     whatever sticks out of the root, and the whole interface would slide
     sideways. Everything in this app that scrolls has a box of its own that
     does it; the root is not one of them. */
  overflow: hidden; }

/* …and on everything in it, because it does not inherit: Blitz reads the
   property off the node the press landed on, and a button under a root that
   says `none` still answers `auto`. */
.root * { user-select: none; }
/* …which misses a bare label in a flex button: it sits in an anonymous box
   that no selector reaches, and Blitz shows the text cursor over it. The
   arrow is said once here instead, and inherits into those boxes.
   **One rule for the hand: it is for what is in the document** — a link, a
   comment's spot and card — and the chrome, buttons and all, is the arrow a
   desktop app wears. */
.root { cursor: default; }
/* The resting pointer is hidden by the page itself, so that every cursor
   Blitz sets while it rests is an invisible one. See `Viewer::pointer_away`. */
.root[data-pointer="away"], .root[data-pointer="away"] * { cursor: none !important; }

/* …and the place where selecting *is* the point: a field being typed in.
   The document itself is not among them —
   its selection is drawn by `select.rs` from pdfium's own character boxes and
   never goes through the DOM. */
.root input, .root textarea { user-select: text; cursor: auto; }
/* **A text area is in the interface's type, not the user agent's.** Blitz's
   own sheet sets `textarea { font-family: monospace }`, as a browser's does,
   and a comment would turn to a typewriter's face the moment it is edited. */
textarea { font-family: inherit; }
/* …and what a window says, as opposed to what it lets you press: titles,
   notes, the details and the keyboard's list can be selected and copied
   (⌘C is Blitz's own there — the keymap stands down over a window). Never a
   control's label, for the reason at the top of this rule. */
.window-title, .pane-title, .pane-group, .pane-lede, .pane-note, .field-note,
.details-label, .details-value, .key-what, .key-chord {
  user-select: text; cursor: text;
}

/* Every row of the window that is not the document carries `z-index`, and it
   is not decoration. Blitz paints by the rules — `.viewer` has
   `overflow: hidden` and a page scrolled past the top is clipped, which the
   screenshots show — but it **hit-tests without clipping**: a page whose box
   starts at -2789px is still hit-tested where its box is, which is over the
   toolbar and the find bar, and a click there would land on the page.
   `position: relative` and a `z-index` put these back in front, which is the
   same trap and the same fix as `.sidebar-resize` below, one level out. */
.toolbar { position: relative; z-index: 2; }
.sidebar { position: relative; z-index: 1; }

.toolbar {
  /* Twelve between the three groups and six inside each of them: the seam
     between two groups reads as a seam only if it is wider than the one
     between two chips. */
  display: flex; align-items: center; gap: 12px;
  height: 46px; flex: 0 0 auto; padding: 0 10px;
  /* `--paper` here is the theme's bars colour: the toolbar sets its own
     variables from `Palette::on_bars` in its `style` attribute. */
  background: var(--paper); border-bottom: 1px solid var(--bar-line);
}
/* **Three groups, and the middle one is why there are three.** The two side
   groups share the slack, which keeps the page controls in the middle. When
   there is not enough room for everything the left gives way, because it is
   the side with a title in it and a title has an ellipsis to shrink into.

   **The bases are nought, and that is what puts the page controls in the
   middle.** With a basis of `auto` each side would start from what it holds
   and the slack would be split evenly on top, so the middle would sit off
   centre by half the difference between the two sides. A basis of nought
   asks for nothing, so both sides are given an equal share of the whole bar.
   What stops the right side from being squeezed off the edge is `min-width`:
   `auto` on a flex item is its content, so `.bar-right` can grow past its
   share and never below it, and `.bar-left` says `0` and is therefore the
   side that gives way. Below about 1500px there is not enough bar for an
   even split and the right takes what it needs; above it the middle is
   centred. */
.bar-group { display: flex; align-items: center; gap: 6px; }
.bar-left { flex: 1 1 0; min-width: 0; }
.bar-center { flex: 0 0 auto; }
.bar-right { flex: 1 0 0; justify-content: flex-end; }

/* **A bar narrower than what it holds loses its words before anything
   overlaps.** `.bar-left` gives way down to nothing, and its chips do not,
   so without these steps they would run on under the page controls. Three
   steps, each at the width
   where the step before stops fitting: the words go and the symbols stay,
   then the two rotations and the way to close go, which the keyboard still
   has, and last the panel, search and the name. What is left needs 450px,
   and `session.rs` does not let a window get narrower than 480. Before all
   of that, the left group's words go first: the document's name is what
   gives way, and with them on it would be cut short at 1300px. */
@media (max-width: 1400px) {
  .toolbar:not(.empty) .bar-left .chip-label { display: none; }
  .toolbar:not(.empty) .bar-left .chip:not(.title) { padding: 0 7px; }
}
@media (max-width: 1200px) {
  .toolbar:not(.empty) .chip-label { display: none; }
  .toolbar:not(.empty) .chip { padding: 0 7px; }
}
/* The start screen's bar holds five chips, which keep their words down to
   a window this narrow. */
@media (max-width: 640px) {
  .toolbar.empty .chip-label { display: none; }
  .toolbar.empty .chip { padding: 0 7px; }
}
@media (max-width: 720px) {
  .toolbar .rotate-left, .toolbar .rotate-right, .toolbar .close-doc { display: none; }
}
@media (max-width: 600px) {
  .toolbar { gap: 6px; }
  .toolbar .contents, .toolbar .chip.find, .toolbar .chip.title { display: none; }
}

.chip {
  display: flex; align-items: center; gap: 7px;
  height: 30px; padding: 0 10px; border-radius: 9px; border: 0;
  background: transparent; color: var(--muted); font-size: 13.5px; font-weight: 500;
  white-space: nowrap;
}
.icon { flex: 0 0 auto; }
.chip:hover { background: var(--hover); color: var(--text); }
.chip:not(:hover) > .icon.hot, .chip:hover > .icon.rest { display: none; }
/* Greyed out, and no hover to suggest otherwise. */
/* **The button that has the keyboard is outlined in the accent.** Tab moves
   the focus from button to button, and Enter or Space presses the one it is
   on (`app::keys_on_controls`). `:focus` and not `:focus-visible`, which
   Blitz never matches. A click leaves no focus on a button, so the outline
   is only ever the keyboard's. A row in a menu or in the Settings list draws
   it inside its own edge, where the menu does not cut it off. */
button:focus { outline: 2px solid var(--accent); outline-offset: 1px; }
.menu-item:focus, .nav-item:focus { outline-offset: -2px; }
.chip:disabled, .chip:disabled:hover { background: transparent; color: var(--muted); opacity: 0.45; }
/* On the bar, a hover and a held-down state come from the paper the bar sits
   on rather than from the surface: the bar belongs to the document, so a chip
   mixed from the backdrop is a chip from another theme. Everything else
   wearing `.chip` floats on `--surface` and keeps the surface's. */
.toolbar .chip:hover { background: var(--bar-hover); }
.toolbar .chip.on, .toolbar .chip.on:hover { background: var(--bar-accent); }
/* A chip whose thing is in force. **The tint carries the theme, not the
   colour of the word alone.** Every theme here names a near-monochrome text
   colour, so a bar written in a shade of it is a bar of grey words whatever
   theme is on, and the accent alone would arrive as one bright word among
   them with nothing under it. */
.chip.on { background: var(--accent-soft); color: var(--accent-ink); }
.chip.on:hover { background: var(--accent-soft); color: var(--accent-ink); }

/* Minus, the readout, plus — one control rather than three labels.

   The group is the widest thing in the bar, so every pixel here moves the
   page controls off the middle. It has `padding: 2px`, the three buttons are
   a chip's own 30px tall with the two ends 30px square, and `.zoom-level`
   asks for 62. The readout is wider than that in practice — "Fit width" is
   about 75 — so the minimum only decides what "100%" gets. */
.zoom-group {
  display: flex; align-items: center; gap: 2px;
  padding: 2px; border-radius: 9px;
}
.zoom-group .chip { height: 30px; border-radius: 9px; }
.zoom-group .chip.zoom-out, .zoom-group .chip.zoom-in {
  width: 30px; padding: 0; text-align: center;
}
.zoom-group .chip.fit { min-width: 62px; text-align: center; }

/* What a menu hangs off. The button is in the flow and the menu is not, so
   the row is the height of the button and the panel is under whichever
   button opened it — see `app.rs`. `calc(100% + 8px)` is the toolbar's own lower edge
   read off the button rather than written down: the chip is 30px in a 46px
   row, and 8px is the space under it. */
.anchor.titled { margin-left: 6px; min-width: 0; }
.anchor { position: relative; display: flex; align-items: center; }

/* The menu itself, out of the flow inside its `.anchor`, so a panel taller
   than the toolbar costs the 46px row nothing.

   `z-index` is not decoration: Blitz hit-tests a positioned node ahead of
   what it is painted over only when it carries a non-zero one, and a menu
   that cannot be clicked is worse than no menu. 5 rather than 1 because it
   is over the toolbar as well as over the document. */
/* **A menu is written a size larger than the bar it hangs off**, 14.5 over
   13.5: the size is what makes a menu read as somewhere you have arrived
   rather than as more chrome. It is 232 to 340 wide, with six of padding,
   and has a shadow, because a menu floats over the document and a border
   alone does not say so. */
.menu {
  position: absolute; top: calc(100% + 8px); z-index: 5;
  min-width: 232px; max-width: 340px; font-size: 14.5px;
  padding: 6px; border-radius: 12px;
  border: 1px solid var(--line); background: var(--surface);
  box-shadow: 0 10px 34px rgba(0,0,0,0.16);
}
/* Under the left edge of the button, except the two near the right end of the
   bar, which would otherwise run off it. */
.menu.document, .menu.open, .menu.view { left: 0; }
.menu.theme, .menu.settings { right: 0; }
/* Every menu here is taller than a short window — the theme list, the
   settings — and rows past the window's bottom cannot be reached. The height
   is set on the element, from the window's. */
.menu.document, .menu.open, .menu.view, .menu.theme, .menu.settings {
  overflow: scroll; scrollbar-width: thin; }
/* Wide enough for "Show page number while scrolling" and its note beside a
   switch, which is the widest row any menu here has. */
.menu.settings { min-width: 330px; }
/* No `width: 100%`. A menu is absolutely positioned and therefore shrinks to
   fit, and a percentage width inside a shrink-to-fit box is resolved against
   a width computed as though the percentage were not there, which would clip
   the chord on the right of the widest row.
   These are block-level flex containers and fill the line on their own. */
/* **Padding rather than a fixed height**: a row given 30px and no padding is
   30px whatever is in it, and a row given seven pixels above and below grows
   with the type, to 35 at 14.5. The ink is the quiet shade until the pointer
   is on it: a menu of every theme in full-strength ink reads as a crowd
   shouting. */
.menu-item {
  display: flex; align-items: center; gap: 10px;
  padding: 7px 10px; border: 0; border-radius: 8px;
  background: transparent; color: var(--muted);
  text-align: left;
}
.menu-item:hover { background: var(--hover); color: var(--text); }
.menu-item.on { color: var(--accent); }
/* A row that takes something away goes red under the pointer, as Clear list
   does. Its icon is drawn twice, and the red one shows on hover. */
.menu-item.danger:hover { color: var(--negative); }
.menu-item.danger:not(:hover) > .icon.hot, .menu-item.danger:hover > .icon.rest { display: none; }
/* A column of its own so the labels line up whether or not a row is ticked.
   Fourteen, which is `.popover-item .check`'s own width and the width of the
   drawing that goes in it. */
.menu-tick { flex: 0 0 14px; display: flex; align-items: center; }
.menu-tick .icon { width: 14px; height: 14px; }
/* `min-width: 0` and `overflow` are what `max-width` on the menu needs: a
   theme somebody named at length would otherwise push the menu past its cap
   rather than being cut at it. Blitz has no ellipsis. */
.menu-label { flex: 1 1 auto; min-width: 0; overflow: hidden; white-space: nowrap; }
/* …and where the cut actually happens, which is the shelf and nowhere else:
   a document's own name is the one label in these menus long enough to run
   out of box, and it would stop mid-letter against the page number beside
   it. The fade is what an ellipsis would be — `.title-name`'s reason, further
   down this sheet — and it is unconditional here because the
   label is a flex item that grows: a short name leaves the faded band on
   empty ground. */
.menu-item[data-item="recent"] .menu-label {
  mask-image: linear-gradient(to right, #000 calc(100% - 20px), transparent);
}
/* The chord, read off the keymap rather than written here — see
   `Viewer::chord_for`. An aside and not the item, in `--note`, which is only
   a little quieter than the label, because the note beside a row is meant to
   be read. */
.menu-key { flex: 0 0 auto; color: var(--note); }
.menu-rule { height: 1px; margin: 5px 8px; background: var(--line); }
/* A row of a menu that holds a control rather than a choice: the label
   carries the weight and the control sits at the end of it. */
.menu-row { display: flex; align-items: center; gap: 10px; padding: 6px 10px; }
/* **The note goes under the label, not beside it**: a column, and the row's
   one flexible box. Side by side the two would share the line, and each would
   wrap in a menu wide enough for both on one. The label keeps its own class
   so the parity test reads the label and not the sentence under it. */
.menu-row-text { display: flex; flex-direction: column; gap: 1px; flex: 1 1 auto; min-width: 0; }
.menu-row-label { color: var(--text); }
/* "Save pages", the pages typed, and the button that asks where they go. */
.menu-pages .menu-row-label { flex: 1 1 auto; }
.menu-pages-field {
  box-sizing: border-box; width: 84px; height: 26px; padding: 0 8px;
  border: 0; border-radius: 7px; background: var(--sunk); color: var(--text);
  font: inherit;
}
.chip.action.menu-pages-save, .chip.action.menu-export-format {
  height: 26px; line-height: 24px; padding: 0 10px; font-size: 12.5px; }
/* "Export highlights as", with the three formats in a line under it. */
.menu-export-label { display: flex; align-items: center; gap: 10px; }
/* A row with a note under its label keeps its symbol on the label's line. */
.menu-item.two-lines { align-items: flex-start; }
.menu-item.two-lines > .icon { margin-top: 2px; }
/* Under the label, past the symbol: 16px and the row's gap of 10. */
.menu-export-formats { display: flex; gap: 6px; margin-top: 5px; margin-left: 26px; }
/* `--note` and the row's own size: the note differs from the label by weight
   rather than size, and fading or shrinking it makes it unreadable. */
.menu-row-note { color: var(--note); white-space: nowrap; }
/* A theme, two letters wide, in its own colours: a wide shallow chip rather
   than a square, and an inset ring in a neutral grey rather than a border in
   the theme's line colour, so that a swatch on a dark theme is not outlined in
   something darker than it. */
.swatch {
  display: flex; align-items: center; justify-content: center;
  flex: 0 0 auto; width: 22px; height: 16px; border-radius: 4px;
  box-shadow: inset 0 0 0 1px rgba(127,127,127,0.35);
  font-size: 11px; font-weight: 600;
}
/* A heading over a run of items; the Document menu's shelf is the one place
   that has one. Told from an item by colour alone, at the menu's own size: a
   heading a size smaller than what it heads reads as a footnote over the list
   rather than as a title on it. */
.menu-section {
  padding: 8px 10px 4px; color: var(--faint);
}

/* The title is a button, because the document's menu hangs off it. It
   keeps the chip's shape and the title's own colour and truncation. */
/* Icon-only, so they get the zoom group's square rather than a chip's
   word-shaped one. */
.chip.page-previous, .chip.page-next { width: 30px; padding: 0; justify-content: center; }

/* The document's name: no icon, 13px, the faint ink, 8px of padding and 6px
   of air in front of it.

   **`flex: 0 1 auto`, not `1 1 0`.** A basis of zero asks for nothing and is
   given whatever the bar has left over, which in a bar holding fourteen
   controls is nothing at all. `auto` asks for the name and gives way under
   pressure, which is what an ellipsis is for. `min-width` is what lets it
   give way at all — without it the automatic minimum size of a `nowrap` run
   is the whole run, and the bar overflows instead. The `max-width` keeps a
   short name from leaving a button half the bar wide. */
.chip.title {
  /* The air in front of the name is on `.anchor.titled`, not here, so that
     the menu still comes down flush with the button it belongs to. */
  /* **`min-width` is the two paddings, not nought**, so that a squeezed name
     is still a box you can click to open the Document menu. Blitz shrinks a
     flex item past its own padding where WebKit floors at it, so the floor is
     said here rather than left to the engine. It matters on a machine whose
     `ui-sans-serif` is wider than SF Pro. */
  flex: 0 1 auto; min-width: 16px; max-width: 284px;
  padding: 0 0 0 8px; font-size: 13px; font-weight: 400; color: var(--faint);
  white-space: nowrap; overflow: hidden;
  /* **A name too long for the box has to lose its end, not its beginning.**
     Blitz's user-agent sheet gives every `button` `text-align: center` and
     `justify-content: center`, which is right for a chip holding an icon and
     a verb and wrong for the one chip whose content is longer than its box:
     centring negative free space takes the same number of pixels off *both*
     ends, and a long name would show only its middle. Both properties are
     named because either alone leaves the other doing it: `justify-content`
     places the anonymous flex item, `text-align` places the run inside it.
     With the start pinned, `.title-name`'s fade lands where the name actually
     runs out. */
  justify-content: flex-start; text-align: left;
}
/* **The fade is what an ellipsis would be, and only a name that has run out
   of box shows it.** Blitz has no `text-overflow`, so the name's own box
   fades over its last fourteen pixels, which are padding. The last four of
   them are clear, so a name that is cut short does not run into the chevron
   after it. A name that fits ends where the padding starts, and nothing of it
   is faded; a name squeezed runs on into the padding, and fades there, at
   whatever width the bar left it, which counting characters could not know. */
.title-name {
  min-width: 0; overflow: hidden; padding-right: 14px;
  mask-image: linear-gradient(to right, #000 calc(100% - 14px), transparent calc(100% - 4px));
}
/* The chevron after the name is smaller than a chip's icon. Its margin takes
   back the chip's gap and three of the name's clear pixels, so it sits close
   to a short name. */
.title-chevron { width: 12px; height: 12px; margin: 0 6px 0 -10px; }
/* The document's name is quieter than the verbs around it. */
.chip.title:not(.on) { color: var(--faint); }
.chip.title:not(.on):hover { color: var(--text); }

/* The page and the count: a box you can type in, and the total beside it
   rather than inside it, so that the two can be told apart. */
/* **Four pixels either side, and no ground of its own**, so that the count
   has room around it. The field keeps its own sunk ground, because it is the
   only part of this that is a control. */
.pill {
  height: 30px; padding: 0 4px;
  display: flex; align-items: center; gap: 6px;
}
/* The number, typed. The unit is deliberately *not* in the field: a field
   reading "16 px" puts the caret wherever the pointer landed, so typing 30
   gives "3016 px". The "/ 400" sits beside it in a span of its own.

   Centred, because Blitz lays a text input out from its leading edge whatever
   `text-align: right` says, which would push the number against the left
   wall. */
.page-field, .page-now {
  box-sizing: border-box;
  height: 28px; padding: 0 6px;
  /* The bar's own family rather than the surface's: this control stands on
     the paper the document is on, not on a floating surface. */
  border: 1px solid var(--bar-line); border-radius: 7px;
  background: var(--bar-sunk); color: var(--text); font-size: 13.5px;
  text-align: center;
}
.page-now:hover { border-color: var(--muted); }
/* The platform's focus ring is a blue rounded box that belongs to no theme in
   this app — under Bay Brown it is the one cold thing on screen. The border
   says the same thing in the theme's own accent. */
.page-field:focus { outline: none; border-color: var(--accent); }
.of { color: var(--faint); font-size: 13.5px; }
/* The count as a button, where the document numbers itself: the same text,
   a hover to say it can be pressed, and nothing else. */
.of.choice {
  height: 28px; padding: 0 6px; border: 1px solid transparent; border-radius: 7px;
  background: none; font: inherit; font-size: 13.5px; color: var(--faint);
}
.of.choice:hover, .of.choice.on { color: var(--text); border-color: var(--bar-line); }
.menu.numbering { left: 0; }

/* **The find bar is a card over the document**, not a row of the column: a
   row would take forty pixels off the document for as long as it is up, and
   opening it would move the page the reader is looking at. It sits under the
   toolbar in two rows: the query and its four buttons, and the three switches
   indented under the field so they read as belonging to the query rather
   than as three more buttons.

   **Where it hangs is set in `app.rs`, not here**, because it hangs off two
   different things: under the Search chip inside that chip's own `.anchor`,
   which is where every other panel in this bar comes down, and at the
   window's right edge when the toolbar is away and there is no chip left to
   hang under. */
/* Dressed as `.notice` is, for `.notice`'s reason: it floats over the page
   and belongs to the bar, so its hovers are the bar's too. */
.find-bar {
  position: absolute; z-index: 30;
  display: flex; flex-direction: column;
  padding: 6px 8px 8px 12px;
  background: var(--paper); border: 1px solid var(--bar-line); border-radius: 10px;
  box-shadow: 0 2px 8px rgba(0,0,0,0.08);
}
.find-bar .chip:hover { background: var(--bar-hover); }
.find-row { display: flex; align-items: center; gap: 8px; }
/* Indented to the field above them rather than to the bar, so the three read
   as belonging to the query and not as three more buttons. */
.find-options { display: flex; align-items: center; gap: 2px; margin-left: 20px; padding-top: 4px; }
/* On, and saying so as loudly as the buttons up in the bar do. These three
   are settings that outlive the bar and the session, so "Highlight all" being
   on is a thing to see at a glance rather than a tick to go looking for. The
   drawing is there whether it is ticked or not, so turning one on does not
   shuffle the other two sideways under the pointer — which is why the tick is
   faint rather than absent. */
.find-option {
  display: flex; align-items: center; gap: 5px;
  height: 26px; padding: 0 8px; border: 0; border-radius: 7px;
  background: transparent; color: var(--muted); font-size: 12.5px;
}
.find-option:hover { background: var(--bar-hover); color: var(--muted); }
.find-option.on { background: var(--accent-soft); color: var(--accent-ink); }
.find-icon { display: flex; align-items: center; color: var(--faint); }
.find-field {
  width: 230px; height: 28px; padding: 0;
  border: 0; background: transparent; color: var(--text); font-size: 13.5px;
  /* Medium, as Firefox's find field reads: CoreText thickens its strokes and
     Vello does not, so at 400 the query would be the thinnest type on the
     card. */
  font-weight: 435;
}
/* No outline, for the reason the page field has none: the ring Blitz draws
   round a focused input is a blue box the length of the field, which over a
   card that is already the thing being looked at reads as an error state. */
.find-field:focus { outline: none; }
/* The count, and the way to the list behind it — `Viewer::show_results`. It
   is a button, so it wears a button's clothes: transparent until there is
   something to show, and then a hover. `.ready` rather than `:empty`, which
   Blitz does not implement. */
.find-count {
  flex: 0 0 auto; min-width: 60px; height: 24px; padding: 0 6px;
  border: 0; border-radius: 6px;
  background: transparent; color: var(--faint); font-size: 12.5px;
  justify-content: flex-end;
}
.find-count.ready:hover { background: var(--bar-hover); color: var(--text); }
/* A chip wearing a drawing and no word: square, with the drawing centred in
   it. */
.chip.icon-only { width: 30px; padding: 0; justify-content: center; }

/* A match, as a rectangle over the page. Behind the ink rather than over it:
   `mix-blend-mode` is out (see `AGENTS.md`, and Blitz has none), and an
   opaque box over a word hides the word. Blitz composites the widget's
   texture and this on top of it, so the alpha is what keeps the type
   readable — 0.38 is where a highlight is plainly there on paper and does not
   grey the letters on a dark theme. */
/* A highlight kept beside the document: pdfium draws the ones in the file,
   and this is the same mark for one the file could not take. */
.kept { opacity: 0.4; border-radius: 2px; pointer-events: none; }
.hit { background: var(--found); opacity: 0.38; border-radius: 2px; }
.hit.current { background: var(--found-now); opacity: 0.45; }
/* What changed since the version the document is compared with: words
   added, and a bar where words were removed. See `Viewer::change_areas`. */
.change { border-radius: 2px; pointer-events: none; }
.change.added { background: var(--positive); opacity: 0.32; }
.change.removed { background: var(--negative); opacity: 0.85; }
.change.current { outline: 2px solid var(--accent); outline-offset: 1px; }
/* The Differences tab: the count, "Stop comparing" under it, and each change
   as the words removed, struck through, and the words added. */
.changes-head { margin-bottom: 6px; }
.changes-stop { margin: 0 4px; }
.change-removed { color: var(--negative); text-decoration: line-through; }
.change-added { color: var(--positive); }

/* What the reader has swept over. **Nothing is drawn**, for the reason
   `.link` below draws nothing: the colour of a selected word is the *page's*
   business and is baked into the bitmap by `regions.wgsl`. A translucent band
   over the words would leave the letters the colour they were printed in,
   rather than the theme's `selection_text` on its `selection_area`.

   The node stays, because it is where the selection *is*: the harness
   reads its rectangle, and a `div` with no paint on it is the cheapest way to
   say so. */
.selected { border-radius: 2px; }

/* A link, as the area the document says it is. Nothing is drawn: the colour
   of a link is the *page's* business and is baked into the bitmap — a
   rectangle drawn over the words would be the second thing saying so and the
   one that could disagree.

   `z-index` is not decoration. Blitz only hit-tests a positioned node ahead
   of its parent's normal-flow content when it carries a non-zero one, and
   this has to come out in front of the page's own widget — the same trap as
   `.sidebar-resize`, one level in. Above `.hit` for the
   same reason it is above the widget: where a match falls on a
   cross-reference, following the link is the gesture that meant something. */
.link { z-index: 2; cursor: pointer; }

/* **A note somebody left in the document.** pdfium paints the annotation's
   own appearance into the page — a sticky note arrives as the little icon it
   was drawn as — so nothing is painted here: this is the hit area over it,
   and it shows itself on hover. Above `.link` for the same reason `.link` is
   above the page: a note over a cross-reference is the more specific thing.
   See [`crate::render::Note`]. */
.note-spot { z-index: 3; cursor: pointer; border-radius: 3px; }
/* A form's field, where a click fills it in. pdfium draws the field. */
.form-spot { z-index: 3; cursor: pointer; border-radius: 2px; }
.form-spot.text { cursor: text; }
.form-spot:hover { background: var(--accent-soft); }
.form-field {
  z-index: 4; box-sizing: border-box; margin: 0; padding: 0 3px;
  border: 2px solid var(--accent); border-radius: 2px;
  background: var(--paper); color: var(--text); font-family: inherit; resize: none;
}
/* Where a link leads, shown while the pointer rests on the link. It takes no
   presses, so the pointer can still leave the link and click what is under the
   card. See `Viewer::glance_card`. */
.glance {
  position: absolute; z-index: 6; pointer-events: none; overflow: hidden;
  border-radius: 8px; background: var(--page);
  border: 2px solid var(--accent);
  box-shadow: 0 10px 34px rgba(0,0,0,0.16);
}
.note-spot:hover { background: var(--accent-soft); }
/* A comment with no room for its card beside the page: its passage
   underlined, in a deeper shade of the highlight (inline). The box is the
   passage's foot and the line under it, so it is pressed where it is seen and
   covers no words. See `Viewer::comment_lines`. */
.note-line {
  z-index: 3; cursor: pointer; height: 3px;
  border-bottom: 3px solid transparent; border-radius: 1px;
}
/* And the comment itself beside the page, where there is room: see
   `comment_cards`. As tall as its words, up to the window's height, past
   which they scroll: see `Card::capped`. */
.note-card {
  z-index: 5; box-sizing: border-box;
  display: flex; flex-direction: column;
  padding: 8px 10px; border-radius: 8px;
  background: var(--paper); border: 2px solid var(--accent);
  box-shadow: 0 1px 4px rgba(0,0,0,0.06);
}
/* A card lit while the pointer is on it or its passage, and the passage
   ringed while it is on the card: see `Viewer::note_under`. The ring is the
   highlight's own colour, inline. */
.note-card.hot { box-shadow: 0 2px 10px rgba(0,0,0,0.18); }
.note-passage {
  z-index: 3; pointer-events: none; box-sizing: border-box; border-radius: 3px;
  border: 2px solid var(--accent);
}
.note-card { cursor: pointer; }
.note-card-by { color: var(--muted); font-size: 11.5px; margin-bottom: 3px; }
/* A comment is what a reader wanted to stand out, so it is set larger than
   the chrome and than most pages' type: see `comment_cards` for the 16/22. */
.note-card-text {
  color: var(--text); font-size: 16px; line-height: 22px; white-space: pre-wrap;
  flex: 0 1 auto; min-height: 0;
}
.note-card-text.scrolls { overflow-y: scroll; scrollbar-width: thin; }
/* And written in place, in the card: its field, then Done. */
.note-card.editing { cursor: auto; box-shadow: 0 2px 12px rgba(0,0,0,0.20); }
/* A note opened under its marker, to be read. See `Viewer::open_note`. */
.note-card.read { cursor: auto; box-shadow: 0 2px 12px rgba(0,0,0,0.20); }
/* Out into the card's padding by its own padding and border, so the words
   stay exactly where they were read: only the box appears around them. */
.note-card-field {
  display: block; width: calc(100% + 14px); margin: -5px -7px;
  box-sizing: border-box; padding: 4px 6px;
  border-radius: 6px; border: 1px solid var(--bar-line); background: var(--bar-sunk);
  color: var(--text); font-size: 16px; line-height: 22px;
}
/* The card's own border is the highlight's colour already; a second, in the
   accent, just inside it would be one frame too many. */
.note-card-field:focus { outline: none; }
.form-field:focus { outline: none; }
/* Never squeezed out of a long card: the words give way instead. */
.note-card-actions { display: flex; justify-content: flex-end; margin-top: 6px; flex-shrink: 0; }
/* Edit, small: the card is the comment, not a form. And Done the same size,
   in the same place, so nothing but the box moves when it is edited. */
.chip.action.note-card-edit, .chip.action.note-card-done { height: 24px; line-height: 22px; padding: 0 10px; font-size: 12.5px; }

/* Words can be swept, so the pointer says so over a page — and says the other
   thing over a link, which is the rule above winning by coming after it. */
.page { cursor: text; }

/* The toolbar, then the document with the panel beside it. A row inside the
   column, which is the whole of what a sidebar is here — there is nothing
   floating over anything.

   **The `z-index` is load-bearing.** Blitz gathers every z-indexed box into
   the nearest stacking context and hit-tests that context only when the
   point falls inside the *union of the boxes it holds* — and a toolbar menu
   is inside `.toolbar`'s own context, so without this the root's union would
   be the toolbar's 47px strip and nothing below it. A click on a menu item
   hanging under the bar would miss the menu and land on the page behind it.
   The document is a layer of its own, under the toolbar's, and the union of
   the two is the window. */
.body { position: relative; z-index: 1; flex: 1 1 auto; display: flex; flex-direction: row; min-height: 0; }

.viewer {
  flex: 1 1 auto; overflow: hidden; background: var(--surround);
}

/* **The scrollbar, which this reader draws because it does not inherit one.**
   The document is moved by arithmetic rather than by `overflow: scroll` (see
   the note at the top of `app.rs`), so there is no platform bar to take.

   `top` and `bottom` rather than a height: the track is exactly as tall as
   the viewer, and the one number that must not be restated is the one the
   layout already knows.

   `right: 0` with no margin is the requirement and not a detail. A pointer
   thrown at the side of the screen stops at the edge, and a bar set in from
   it is one that has to be aimed at — so the whole twelve pixels are live,
   which is what the `z-index` is for: `.pages` is `.viewer`'s normal-flow
   content, and Blitz only hit-tests a positioned node ahead of it when it
   carries a non-zero one. See `.root`. */
.scrollbar {
  position: absolute; top: 0; right: 0; bottom: 0; width: 12px; z-index: 6;
}
/* **Nothing to see and nothing to press: it is the union.** Everything
   z-indexed under `.body` — the swatches, a mark's popover, links, notes,
   the scrollbar — is hit-tested only inside the union of those boxes, and
   Blitz measures that union *before* the layout of the frame. A popover that
   has just appeared is 0×0 in it, so with the scrollbar faded and no link
   nearby the union would be a point, and the swatches under a still pointer
   would not be hovered. A layer the size of the window,
   always there, makes the union the window — the reason `.body` has its
   `z-index`, one level down. */
.hit-layer {
  position: absolute; top: 0; left: 0; right: 0; bottom: 0; z-index: 1;
  pointer-events: none;
}
/* The theme's own quiet grey, which is what `--faint` is for — the surround's,
   since it stands on it — and it darkens under the hand rather than on hover:
   a bar that changes as the pointer passes over it is movement nobody asked
   for. Inset by two pixels so the
   thumb is a shape on the edge rather than a stripe down it — the *track* is
   still the full twelve, which is what a press lands on. */
.bar-thumb {
  position: absolute; left: 2px; right: 2px;
  border-radius: 4px; background: var(--surround-faint);
}
.bar-thumb.held { background: var(--surround-muted); }

/* **The stationary scroll's anchor**, dropped by the middle button. The
   document runs under it, the faster the further the pointer is carried away
   — see the block on it in `app.rs`.

   There is nothing here about how it looks: the ring, the chevrons and the
   size are all `still_mark` and `STILL_MARK` in `app.rs`, drawn as one path
   rather than as a bordered box — see the note there. `pointer-events: none`
   because a marker the pointer can catch is a marker that gets in the way of
   the very gesture it is describing, and the `z-index` for the reason the
   scrollbar has one. */
.still-anchor { position: absolute; z-index: 6; pointer-events: none; }
.still-anchor .icon { color: var(--muted); stroke: var(--muted); }

/* `box-sizing` so that the panel is exactly as wide as it says it is. The
   hairline down its right is a border, and a content box would put it
   *outside* the width, so the document would be laid out for a viewport one
   pixel wider than the box it is drawn into. */
/* And `font-size: 14.5px`: the panel is read the way a menu is read — at arm's
   length, in a column, a line at a time — so it is written a size larger than
   the bar, and every list in it inherits that rather than naming a size of its
   own. A contents list set in 13 is a contents list nobody reads. */
.sidebar {
  flex: 0 0 auto; display: flex; flex-direction: column; box-sizing: border-box;
  min-height: 0; font-size: 14.5px;
  background: var(--surface); border-right: 1px solid var(--line);
}
/* Centred on the border rather than beside it, so the grab target is wider
   than the one line it visually is. */
.sidebar-resize {
  /* `z-index` is not decoration here: Blitz only checks a positioned node
     ahead of its parent's normal-flow content during hit-testing when it
     carries a non-zero z-index (`pos_z_hoisted_children` in
     `blitz-dom/src/node/node.rs`) — without it this loses every hit test to
     `.panel`, which is later in the DOM and just as wide. */
  position: absolute; top: 0; right: -3px; width: 6px; height: 100%;
  /* Two, not one, since `.tabs` below has one: they are siblings in the
     same stacking context and they overlap for the height of the tab strip,
     so equal numbers would hand the top 35px of the drag edge to the tabs. */
  z-index: 2; cursor: col-resize;
}
.sidebar-resize:hover { background: var(--accent); opacity: 0.35; }
/* **And the tabs are the same trap again, one level in.** A thumbnail is
   absolutely positioned at `top - thumb_scroll`, so the column scrolled down
   at all puts rows at negative offsets — clipped by `.panel`'s
   `overflow: hidden` when it paints, and hit-tested where their boxes say
   they are, which is over this strip. Without the `z-index`, clicking
   Contents from a scrolled Pages tab would land on a thumbnail nobody can
   see. */
.tabs { position: relative; z-index: 1; display: flex; flex: 0 0 auto; gap: 4px; padding: 8px 8px 6px 8px; min-width: 0; }
/* **Three of these have to fit across a panel the reader can drag to any
   width, and a flex item does not shrink below its content unless it is told
   it may.** Without `min-width: 0` the tabs keep the width of their words,
   the strip overflows the panel, and the third tab is drawn outside the
   sidebar, over the document, where Blitz hit-tests the document's layer
   instead of the tab. The word gives way and the icon does not — a tab with
   half an icon on it reads as a rendering fault and a tab with a shortened
   word reads as a narrow panel — which is why the label is a span of its own
   and the shrinking happens there. */
.tab {
  display: flex; align-items: center; justify-content: center; gap: 5px;
  flex: 1 1 0; min-width: 0; overflow: hidden;
  height: 28px; padding: 0 4px; border: 0; border-radius: 7px;
  background: transparent; color: var(--muted); font-size: 12.5px; font-weight: 500;
}
.tab .icon { flex: 0 0 auto; }
/* `text-overflow: ellipsis` is not implemented — see the note at the top of
   this file — so the word fades out instead, the same mask `.chip.title` and
   `.result` use. */
.tab-label {
  flex: 0 1 auto; min-width: 0; overflow: hidden; white-space: nowrap;
}
/* **And only where a word can actually be cut.** A label is `flex: 0 1 auto`
   — its box *is* its word — so a mask on every label would fade the last ten
   pixels of every tab whether or not anything has run out of room.
   `sidebar.rs` says `tight` when there are three tabs and the panel is
   narrower than all three of them labelled. The rows below get away with an
   unconditional mask because their boxes are the full width of the panel, so
   the faded band falls on empty ground when the text is short. */
.tabs.tight .tab-label {
  mask-image: linear-gradient(to right, #000 calc(100% - 10px), transparent calc(100% - 2px));
}
.tab:hover { background: var(--hover); color: var(--text); }
/* **The tint carries the theme, here as in the bar**: the accent at a fifth
   of its strength with the accent written on it, as `.chip.on` has. Which tab
   is open is the one thing this strip exists to say. */
.tab.on { background: var(--accent-soft); color: var(--accent-ink); }

.panel { flex: 1 1 auto; overflow: hidden; min-height: 0; }
/* The outline scrolls on its own, as `.results` does: a paper's contents run past
   the bottom of the panel. */
.panel.contents { overflow-y: scroll; scrollbar-width: thin; }
.thumb-column { position: relative; }
.sidebar-empty { margin: 10px 12px; color: var(--faint); }

/* Padding rather than a fixed height, and the panel's own size rather than a
   size of its own, because a heading is a line of somebody's prose, and a list of them
   set in 13 in a 26px slot is a list nobody reads. The `padding-left` written
   on each row in `sidebar.rs` is the indent and overrides the 8 here. */
/* Inset by the tabs' own 8px, so that a row's tint lines up with the tab
   above it, and faded in its words rather than in the whole row, so that the
   tint keeps its rounded end. */
.outline-item {
  display: block; width: calc(100% - 16px); margin: 0 8px; border: 0; border-radius: 7px;
  padding: 5px 8px;
  background: transparent; color: var(--muted); text-align: left;
  white-space: nowrap; overflow: hidden;
}
.outline-words {
  display: block; overflow: hidden;
  mask-image: linear-gradient(to right, #000 calc(100% - 20px), transparent);
}
.outline-item:hover { background: var(--hover); color: var(--text); }
/* Where the reader is, said the way every other "this one" in this app says
   it: the accent under the words as well as in them — see `.tab.on`. */
.outline-item.current { background: var(--accent-soft); color: var(--accent-ink); }
/* An entry whose place is not in the file, and the words above each run of
   them. The words take the run's indent from `sidebar.rs`. */
.outline-item.nowhere, .outline-item.nowhere:hover { background: transparent; color: var(--faint); }
/* Its title fades at the edge as a row's does. The row itself lets its
   sentence hang below it, over the rows that follow. */
.outline-item.nowhere {
  position: relative; overflow: visible; mask-image: none; padding-right: 0;
}
.outline-title {
  display: block; overflow: hidden; opacity: 0.6;
  mask-image: linear-gradient(to right, #000 calc(100% - 20px), transparent);
}
/* **The sentence under a greyed-out row, while the pointer is on it.** Blitz
   has no `title` tooltips, so this is one: hidden until hovered, placed under
   the row without moving the rows below, and passed through by the pointer. */
.outline-tip {
  display: none; position: absolute; top: calc(100% + 2px); z-index: 3;
  width: 190px; padding: 6px 10px; border-radius: 8px;
  background: var(--surface); border: 1px solid var(--line);
  box-shadow: 0 2px 8px rgba(0,0,0,0.12);
  color: var(--text); font-size: 13px; line-height: 1.4; white-space: normal;
  pointer-events: none;
}
.outline-item.nowhere:hover .outline-tip { display: block; }
.outline-nowhere { margin: 8px 8px 2px 8px; color: var(--faint); font-size: 13px; }

/* The results list. A row is two lines' worth of text on one line, cut off at
   the end rather than wrapped: what places a match is the words in front of
   it, and the sentence behind it is there to be recognised, not read. */
.results { padding: 4px 8px 8px 8px; overflow: scroll; scrollbar-width: thin; }
.results-count { margin: 2px 4px 6px 4px; color: var(--faint); }
/* `justify-content` and the `min-width: 0` on the line below it are the same
   rule as `.tab` above, one panel down. A button is laid out as a *centring*
   flex box by the user-agent sheet, so a row wider than a narrow panel that
   cannot shrink would overflow at **both** ends and push the page number out
   of the panel on the left. */
.result {
  display: flex; align-items: baseline; justify-content: flex-start; gap: 8px;
  width: 100%; min-width: 0;
  border: 0; border-radius: 9px; padding: 7px 10px; margin-bottom: 1px;
  background: transparent; text-align: left; font-size: 12.5px;
  white-space: nowrap; overflow: hidden;
}
.result:hover { background: var(--hover); color: var(--text); }
.result.current { background: var(--accent-soft); color: var(--accent-ink); }
/* `min-width` so that the numbers line up down the column rather than every
   quote starting at a different place, in ems because it is a measure of
   digits and not of pixels. */
.result-page {
  flex: 0 0 auto; min-width: 2.2em; color: var(--faint); font-size: 12px;
}
/* The quote fades at the edge, and not the whole row, so that the current
   row's tint keeps its rounded end. */
.result-line {
  flex: 1 1 auto; min-width: 0; overflow: hidden; color: var(--muted);
  mask-image: linear-gradient(to right, #000 calc(100% - 24px), transparent);
}
/* `pre`, because the space either side of the match is the whole difference
   between a line that reads as a sentence and "A**needle**in the first page":
   HTML collapses whitespace at the edge of an inline run, and these two runs
   are cut out of the document precisely at those edges. `results()` keeps a
   single space there deliberately — see `search.rs`. */
.result-before, .result-after { white-space: pre; }
/* The matched words, drawn the way a swept passage on the page is drawn: the
   theme's own selection colours. A bold run in the ink colour would say "this
   is the word" in the one way that cannot be told apart from a heading. */
.result-hit {
  background: var(--found); color: var(--found-ink); border-radius: 2px;
}

.marks { padding: 4px 8px 8px 8px; border-bottom: 1px solid var(--line); }
.marks-title { margin: 2px 4px 6px 4px; color: var(--faint); }
.mark { display: flex; align-items: center; gap: 4px; }
.mark-go {
  /* `display: block` as well as `text-align`, because a button is laid out as
     a centring flex box by the user-agent sheet and the alignment inside it is
     the flex box's rather than the text's. `.outline-item` says the same two
     things. */
  display: block;
  flex: 1 1 auto; height: 26px; border: 0; border-radius: 7px;
  background: transparent; color: var(--text); text-align: left; font-size: 13px;
  white-space: nowrap; overflow: hidden;
  mask-image: linear-gradient(to right, #000 calc(100% - 20px), transparent);
}
.mark-go:hover { background: var(--hover); }
.mark-drop {
  flex: 0 0 auto; width: 26px; height: 26px; border: 0; border-radius: 6px;
  background: transparent; color: var(--faint);
}
.mark-drop:hover { background: var(--hover); color: var(--text); }
.mark-drop:not(:hover) > .icon.hot, .mark-drop:hover > .icon.rest { display: none; }
/* The second press of a removal: the × says what it does, in the colour of
   a thing that cannot be taken back. See `Viewer::arm`. */
.mark-drop.armed, .sign-forget.armed {
  width: auto; padding: 0 10px; color: var(--negative-contrast); background: var(--negative);
}

/* The passages, under the pages. A row is a colour, the words themselves, and
   the way to take it off — the colour first, because that is what a reader
   picked and is how they will find the one they are looking for. */
.markup { margin-top: 10px; }
/* The files the document carries: an icon, the name, and what a click does. */
.attachments { padding: 4px 8px 8px 8px; }
.mark-go.attachment {
  display: flex; align-items: center; gap: 6px; width: 100%; padding: 0 6px;
  mask-image: none;
}
.attachment-name { flex: 1 1 auto; min-width: 0; overflow: hidden; }
.attachment-save { flex: 0 0 auto; color: var(--faint); font-size: 12px; }
.mark-go.attachment:not(:hover) > .attachment-save { display: none; }
.markup-row { align-items: flex-start; }
.markup-dot {
  flex: 0 0 auto; width: 10px; height: 10px; margin-top: 8px; border-radius: 3px;
}
/* Two lines of the quote and then it stops. `text-overflow: ellipsis` is not
   implemented in Blitz — see the note on `.chip.title` — and a mask is what
   this file uses instead everywhere it wants the same thing. */
.markup-row .mark-go {
  height: auto; min-height: 26px; padding: 5px 6px; line-height: 1.35;
  white-space: normal; max-height: 40px;
  mask-image: linear-gradient(to bottom, #000 calc(100% - 12px), transparent);
}
.markup-restore {
  display: block; width: 100%; margin: 0 0 8px 0; padding: 6px 8px;
  border: 1px solid var(--line); border-radius: 8px;
  background: var(--accent-soft); color: var(--accent-ink);
  font-size: 12px; text-align: left;
}
.markup-restore:hover { border-color: var(--accent); }
.markup-clear {
  display: flex; align-items: center; gap: 6px; margin: 6px 0 0 0; padding: 4px 4px;
  border: 0; background: transparent; color: var(--faint); font-size: 12px;
}
.markup-clear:hover { color: var(--negative); }
.markup-clear:not(:hover) > .icon.hot, .markup-clear:hover > .icon.rest { display: none; }
.markup-beside {
  display: block; color: var(--faint); font-size: 11px; margin-top: 2px;
}
/* A mark's comment, under the words it is on. */
.markup-note { display: block; color: var(--muted); font-size: 12px; margin-top: 3px; }

/* The colour popover, over the passage it is about. It belongs to the page,
   because the rectangle it is placed against is the page's. Dressed as
   `.notice` is. */
.markup-popover {
  display: flex; gap: 6px; padding: 7px; z-index: 6;
  background: var(--paper); border: 1px solid var(--bar-line); border-radius: 9px;
  box-shadow: 0 2px 8px rgba(0,0,0,0.08);
}
.markup-swatch {
  width: 26px; height: 26px; border: 1px solid var(--bar-line); border-radius: 7px;
  padding: 0;
}
.markup-swatch:hover { border-color: var(--accent); }
/* Under Underline or Strike out, a swatch is a line in the colour on the
   paper, under or through where a word would be. */
.markup-swatch.lined {
  display: flex; justify-content: center; align-items: flex-end;
  background: var(--paper);
}
/* Over a boxed area, a swatch is a box in the colour. */
.markup-swatch.boxed, .markup-swatch.boxed:hover { border-width: 3px; background: var(--paper); }
/* The area being boxed, while the pointer is down and while its colours are
   offered. */
.area-box { border: 1.5px dashed var(--accent); pointer-events: none; }
.markup-line { width: 16px; height: 3px; border-radius: 2px; margin-bottom: 6px; }
.markup-line.through { align-self: center; margin-bottom: 0; }
.markup-style.on { border-color: var(--accent); color: var(--accent); }
/* The long way round and the way out, after the six: the same size as a
   swatch, so the row reads as one. The × only puts the swatches away, so it
   is grey and turns red under the pointer, as every other close does. A
   variable that `variables()` does not write is dropped without a warning,
   and the button then keeps whatever was under it. */
.markup-more, .markup-close {
  width: 26px; height: 26px; padding: 0; border-radius: 7px;
  border: 1px solid var(--bar-line); font-size: 14px; line-height: 1;
}
.markup-more { background: var(--bar-sunk); color: var(--muted); }
.markup-more:hover { border-color: var(--accent); color: var(--text); }
.markup-close { background: var(--bar-sunk); }
.markup-close:hover { border-color: var(--negative); }
.markup-close:not(:hover) > .icon.hot, .markup-close:hover > .icon.rest { display: none; }
.markup-copy {
  height: 26px; padding: 0 10px; border-radius: 7px;
  border: 1px solid var(--bar-line); background: var(--bar-sunk);
  color: var(--text); font-size: 13px; white-space: nowrap;
}
.markup-copy:hover { border-color: var(--accent); }
/* The window the … opens. */
/* As tall as what is in it, up to `.window`'s limit, and past that the body
   scrolls in a box of its own: the palettes are a list that grows. Two
   classes, because `.window` comes later and would otherwise fix it at
   600px with the foot hanging out of the bottom. */
.window.colours-window { height: auto; }
.colours-body {
  flex: 1 1 auto; min-height: 0; padding: 14px 18px 18px;
  display: flex; flex-direction: column; gap: 10px;
  overflow-x: hidden; overflow-y: auto; scrollbar-width: thin;
}
.colours-body .field-note { margin: 0 0 4px; }
.colours-row { display: flex; align-items: center; gap: 12px; }
.colours-label { width: 72px; color: var(--muted); font-size: 13.5px; }
/* How the colour comes out on this theme's page, beside the colour as
   written — only shown where the two differ. See `Palette::on_page`. */
.colours-on-page {
  width: 26px; height: 26px; border-radius: 7px; border: 1px solid var(--line);
}
/* "Apply", beside the colour it applies, where the window was opened over a
   highlight. */
/* "In use" is a chip with nothing to press, so its words stand where
   Apply's do. */
.chip.action.colours-use.in-use,
.chip.action.colours-use.in-use:hover { border-color: transparent; background: transparent; color: var(--muted); cursor: default; }
/* The palettes, one row each: its six colours as they go into the file, and
   its name. The one in use is the accent's, as a theme in the menu is. */
.palette-list { display: flex; flex-direction: column; gap: 4px; margin: 0 -10px 8px; }
.palette-choice {
  display: flex; align-items: center; justify-content: flex-start; gap: 14px;
  width: 100%; padding: 9px 10px;
  border: none; border-radius: 9px; background: transparent;
  color: var(--text); font-size: 15px; text-align: left;
}
.palette-choice:hover { background: var(--hover); }
.palette-choice.on { color: var(--accent-ink); background: var(--accent-soft); }
/* Which one is in use, in words: the tint alone is easy to take for a hover. */
.palette-current-tag {
  padding: 2px 8px; border-radius: 999px; font-size: 12.5px;
  background: var(--surface); color: var(--accent-ink);
}
.palette-dots { display: flex; gap: 5px; flex-shrink: 0; }
.palette-dot { width: 20px; height: 20px; border-radius: 6px; border: 1px solid var(--line); }
/* The palette in use, as Settings shows it above the way to change it. */
.palette-current { display: flex; align-items: center; gap: 14px; font-size: 15px; color: var(--text); }
.palette-name { white-space: nowrap; }

/* And what a mark already in the document says when it is clicked, or
   right-clicked: a `.menu`, with its comment and its six colours over the
   rows. `.menu`'s own `top` is the toolbar's; this one is placed inline. */
.mark-popover { z-index: 6; }
.mark-swatch {
  width: 22px; height: 22px; border: 1px solid var(--bar-line); border-radius: 6px;
  padding: 0;
}
.mark-swatch:hover { border-color: var(--accent); }
/* The one in use: a ring in the accent with the menu's own ground between,
   so a swatch in the accent's colour still shows its ring. */
.mark-swatch.on { box-shadow: 0 0 0 2px var(--surface), 0 0 0 4px var(--accent); }
.mark-row { display: flex; align-items: center; gap: 8px; padding: 4px 10px; }
/* Never folded or squeezed: a popover near the page's right edge is only as
   wide as the room left, and Blitz shrinks a flex item past its padding. */
.mark-row > *, .markup-popover > * { flex-shrink: 0; }
/* Its comment, as a `.note-card`, at the top. */
.note-card.mark-note { z-index: auto; margin-bottom: 6px; box-shadow: none; }

/* The column takes its shape from the pictures in it, and only the rows near
   the view are here at all — see `sidebar.rs`. */
.thumbs { position: relative; width: 100%; }
.thumb {
  border: 0; background: transparent; padding: 0;
  display: flex; flex-direction: column; align-items: center;
}
/* **A ring, not a drop shadow, and the page being read wears it in the
   accent**: a hairline round every thumbnail so a white page is told from a
   white panel, and a coloured one round the page you are on, which the
   twelve-pixel number under it cannot say alone. A shadow is what a page on
   the desk has; a thumbnail in a list is not on a desk. */
.thumb-picture {
  background: var(--page); border-radius: 4px;
  box-shadow: 0 0 0 1px var(--line);
}
.thumb.current .thumb-picture { box-shadow: 0 0 0 2px var(--accent); }
.thumb-number { height: 18px; color: var(--faint); font-size: 12px; }
.thumb.current .thumb-number { color: var(--accent); }
/* A page selected in the organiser has a tick in its corner, so a selection
   can be seen when the grid is mostly scrolled away. */
.thumb-tick {
  position: absolute; top: 6px; right: 6px;
  display: flex; align-items: center; justify-content: center;
  width: 20px; height: 20px; border-radius: 10px; background: var(--accent);
}
/* The document: one box the size of the whole thing, with the pages placed in
   it. Blitz has no `position: static`, so an absolutely positioned node is
   placed against its immediate parent — which is exactly what this wants. */
.pages { position: relative; width: 100%; height: 100%; }
/* `--page`, not white. A page whose texture has not arrived draws as this
   and nothing else, and every re-key — a zoom step, a jump, a theme, a turn —
   makes a page whose texture has not arrived. See `variables` above. */
.page { background: var(--page); box-shadow: 0 1px 3px rgba(0,0,0,0.16), 0 8px 24px rgba(0,0,0,0.10); }

/* A document being dragged over the window — the half of "or drop a PDF here"
   that makes the sentence true. Over the whole window rather than over the
   document, because "here" is the window, and `z-index` under Settings alone:
   a drag over a window whose Settings are open is not a drag onto the
   document. */
.drop-hint {
  position: absolute; top: 10px; left: 10px; right: 10px; bottom: 10px;
  z-index: 15; display: flex; align-items: center; justify-content: center;
  /* A solid ground reads as a curtain drawn over the window, and the one thing
     somebody dragging a file wants to see is the window they are dragging it
     onto. So it is a dashed border inset from the edges — the window itself
     saying it will catch this — over a veil of the theme's paper rather than
     the paper itself. `--veil` is that colour with an alpha on it, mixed in
     `variables` rather than written as `color-mix`, which Stylo's support for
     is not something to find out about from a screenshot. */
  border: 2px dashed var(--accent); border-radius: 16px;
  background: var(--veil); color: var(--accent);
  font-size: 16px; font-weight: 500;
}
/* Something that will not open. The same frame, so the hint does not jump
   when a folder crosses the window, and the theme's own muted ink rather than
   a red of its own: fourteen themes have no error colour between them, and
   inventing one here would be the only colour in this application that no
   theme chose. */
.drop-hint.refused { border-color: var(--faint); color: var(--muted); }
/* The word itself carries no ground of its own; it is the frame that says
   what will happen and the word that says what it is. */
.drop-hint-word { padding: 0 8px; }

/* -------------------------------------------------- the window with nothing
   in it

   It stands where the document would. Centred in both axes, on `--surround`
   — the same shade a page floats on, because it is the same place. On
   `--paper`, the toolbar's colour, the window would change shade the moment
   a document is opened.

   **And 14.5px, not the body's 13.5**, as menus, the sidebar and windows
   set their own size. */
.start {
  flex: 1 1 auto; display: flex; align-items: center; justify-content: center;
  background: var(--surround); font-size: 14.5px;
  overflow: scroll; scrollbar-width: thin;
}
/* Blitz resolves `min()` and `vw`, and the narrow half matters: a window
   dragged down to its minimum still has a list in it rather than a list with
   its right-hand column off the edge.

   A flex column so that the one button in it can be `align-self: center`.
   Everything else here stretches, as block layout would. */
.start-inner { width: min(460px, 72vw); display: flex; flex-direction: column; }
/* The icon's scroll around the column, at 0.17px a unit of `app-icon.svg` and
   a line of 2.8px (16.5 units). `margin: auto` rather than the parent's
   centring, so a window shorter than the scroll scrolls to its top. */
.start-scroll { margin: auto; padding: 24px 0; display: flex; flex-direction: column; }
/* Over the roll, which its feet grip and its body hides, as in the icon. */
.start-owl {
  align-self: center; position: relative; z-index: 1;
  margin-bottom: -12.8px; color: var(--text);
}
/* A roll: 90 units between its lines, and caps of 45. */
.start-roll {
  position: relative; height: 18.1px;
  border: 2.8px solid var(--accent); border-radius: 9.05px;
}
.start-curl { position: absolute; top: -2.8px; width: 18.1px; height: 18.1px; }
.start-curl.left { left: -2.8px; }
.start-curl.right { right: -2.8px; }
/* Its sides 15 units inside the caps' centres, as in the icon. */
.start-sheet {
  margin: 0 10.2px; padding: 28px 26px;
  border: solid var(--accent); border-width: 0 2.8px;
}
.start-name {
  margin: 0; text-align: center;
  /* Its own optical size, because this is above where the axis starts and
     `body`'s 17 would draw a headline in a face meant to be read at 17. See
     `body`. */
  font-size: 30px; font-variation-settings: "opsz" 30;
  font-weight: 600; letter-spacing: -0.01em; color: var(--text);
}
.start-sub {
  margin: 6px 0 22px; text-align: center; color: var(--note); font-size: 15.5px;
}
/* The decorative title, which the reader can choose in Settings or by
   right-clicking the title. The font is registered by `title_font`. The
   tracking is also added after the last letter, so the same amount of
   padding on the left keeps the word centred. */
.start-title.decorative .start-name {
  font-family: "Cinzel Decorative", serif;
  font-size: 32px; font-weight: 400; font-variation-settings: normal;
  letter-spacing: 0.16em; padding-left: 0.16em;
}
/* A serif, between the decorative title and the sans-serif interface below
   it. `ui-serif` is New York on a Mac. */
.start-title.decorative .start-sub {
  font-size: 16.5px; font-family: ui-serif, Georgia, "Times New Roman", serif;
}
/* The one filled button in this application. Everything else in the chrome is
   a quiet chip on a transparent ground, which is right for a bar of them and
   wrong for a screen whose whole purpose is one action. */
/* As wide as its own words, not a band across the whole column: a gap of 7,
   a height of 36, a padding of 0 16px and the inherited 14.5px. */
/* Open, and beside it the organiser, which needs no document. */
.start-actions { display: flex; justify-content: center; gap: 10px; flex-wrap: wrap; }
.start-combine {
  display: flex; align-items: center; justify-content: center; gap: 7px;
  height: 36px; padding: 0 16px; box-sizing: border-box;
  border: 1px solid var(--line); border-radius: 9px;
  background: transparent; color: var(--text);
  font: inherit; font-weight: 500;
}
.start-combine:hover { border-color: var(--accent); }
.start-open {
  display: flex; align-items: center; justify-content: center; gap: 7px;
  align-self: center; height: 36px; padding: 0 16px;
  border: 0; border-radius: 9px;
  background: var(--accent); color: var(--accent-contrast);
  font-weight: 500;
}
/* The notice's Open button: the notice itself lets the pointer through. */
.notice-open { pointer-events: auto; height: 24px; line-height: 22px; padding: 0 10px; font-size: 12.5px; }
.start-hint { margin-top: 22px; text-align: center; color: var(--note); font-style: italic; }
.start-tip { margin: 6px 0 0; text-align: center; color: var(--faint); font-size: 13px; }

.recents { margin-top: 26px; }
.recents-head {
  display: flex; align-items: center; justify-content: space-between;
  padding: 0 8px 6px 8px;
}
.recents-title { color: var(--note); }
/* A bordered chip, so it reads as a button. Like the
   crosses beside the rows, it turns red under the pointer, because clearing
   the list takes every row away. */
.recents-clear {
  display: flex; align-items: center;
  height: 26px; padding: 0 10px;
  border: 1px solid var(--line); border-radius: 8px;
  background: transparent; color: var(--muted);
  font: inherit; font-size: 13px; font-weight: 500;
}
.recents-clear:hover { border-color: var(--negative); color: var(--negative); }
/* A row is the button and the × beside it, and the × is a sibling rather than
   a child: a button inside a button is not a shape either the DOM or a
   pointer knows what to do with. */
/* Under the pointer a row comes up to the theme's paper, out of the surround it
   stands on, rather than taking `--hover`: that is mixed for a menu's surface
   and is a grey on a warm surround. */
.recent { display: flex; align-items: center; border-radius: 9px; }
.recent:hover { background: var(--paper); }
.recent:hover .recent-open { color: var(--text); }
.recent-open {
  display: flex; align-items: center; gap: 10px;
  /* 37px, a line of 1.45 × 14.5 with 8px above and below. The height is
     stated rather than left to the padding because the × beside it is a
     sibling, so the two have to agree about the row rather than about their
     own boxes. */
  flex: 1 1 auto; min-width: 0; height: 37px; padding: 0 4px 0 10px;
  border: 0; background: transparent; color: var(--muted);
  text-align: left;
}
/* The name takes what is left and the page number keeps its column. `min-width:
   0` on both this and the row above it, because a flex item's floor is its
   content and a long title would otherwise push the page number off the end
   rather than being cut. */
/* The fade is `.menu-label`'s and for the same reason: cut hard, a long title
   ends mid-letter against the page number in the next column, which reads as
   a fault rather than as a name that did not fit. Unconditional because the
   box grows — a short name fades over empty ground. */
.recent-name {
  flex: 1 1 auto; min-width: 0; overflow: hidden; white-space: nowrap;
  mask-image: linear-gradient(to right, #000 calc(100% - 20px), transparent);
}
/* Right-aligned and tabular, so a three-digit page lines up with a one-digit
   one. Quieter and a size smaller than the name: it is a page reference in
   the margin rather than part of the title. */
.recent-page {
  flex: 0 0 auto; min-width: 3.4em; text-align: right;
  color: var(--faint); font-size: 12.5px; font-variant-numeric: tabular-nums;
}
/* Always there rather than revealed on hover. `.recent:hover .recent-forget {
   visibility: visible }` is a rule about an ancestor's state, and Stylo
   resolves it correctly — but Blitz takes a state-only snapshot of the node
   the pointer is on, so the descendant is not re-resolved and the × appears on
   the first hover after something else forces a restyle rather than on this
   one. A control that appears late is worse than a control that is quietly
   always there. */
.recent-forget {
  flex: 0 0 auto; display: flex; align-items: center;
  height: 26px; padding: 0 8px; margin-right: 4px;
  border: 0; border-radius: 7px; background: transparent; color: var(--faint);
}
.recent-forget:hover { background: var(--surround); color: var(--text); }
.recent-forget:not(:hover) > .icon.hot, .recent-forget:hover > .icon.rest { display: none; }

/* What the app says out loud, over the document rather than under it, so
   that it costs the document no room. It takes no clicks, so a message over a
   page does not swallow a press on the page.

   **Under the toolbar's right-hand end, always**, with or without the bar, so
   that ⌘+ puts "171%" in one place. The zoom stepper lives in that corner
   when the bar is up, so the readout appears where its own control would be
   rather than over the line somebody is reading. It is not the pill's corner
   — that rides the scrollbar at the foot.

   Placed by a row rather than by `left: 50%` and a transform, which is not
   something to lean on in Blitz. 55px is the toolbar and its hairline
   (`TOOLBAR` + `HAIRLINE` in `app.rs`) and eight of gap. The right padding is
   the toolbar's own ten, so the chip's edge lines up with the bar's last
   control. */
.notice-line {
  position: absolute; left: 0; right: 0; top: 55px; z-index: 45;
  display: flex; align-items: center; justify-content: flex-end;
  padding-right: 10px;
  pointer-events: none;
}
/* With the bar away there is nothing above it, so it comes up to the band the
   bar occupied — above the handle that gives the bar back, which is hard
   against nothing for a reason of its own. See `.peek-line`. */
.notice-line.tucked {
  top: 8px;
}
/* The find bar hangs in the same corner, 72px tall; the notice goes under it. */
.notice-line.under-find { top: 134px; }
.notice-line.tucked.under-find { top: 88px; }
/* **Made of the toolbar, not of the menus.** It stands where the bar's zoom
   stepper is and says what the bar would, so it wears the bar's paper, its
   hairline and its chip radius. The shadow is faint: with nothing behind it, a hairline alone
   reads as a shape drawn on the page rather than something laid over it.
   `gap` is for the tick beside "Saved". */
/* `pre-line` because one notice is two sentences and reads better as two
   lines — "Menu bar hidden." and the key that brings it back. Every other
   notice is one line and is unaffected. */
.notice {
  display: flex; align-items: center; gap: 8px;
  white-space: pre-line; text-align: center;
  max-width: 70%; padding: 7px 13px; border-radius: 9px;
  background: var(--paper); border: 1px solid var(--bar-line);
  box-shadow: 0 2px 8px rgba(0,0,0,0.08);
  color: var(--text);
}

/* The handle that gives the toolbar back, placed by a row rather than by
   `left: 50%` and a transform — `.notice-line`'s reason again. It is in the
   DOM only while it is being reached for (see `Viewer::reach_for_toolbar`).

   To the right rather than the middle, which is where the switch that put the
   bar away is and where the notice about it goes: everything the reader has
   to do with a missing toolbar is in one corner. */
/* **Clear of the top edge, always.** At the edge itself, macOS drops the
   title bar and the traffic lights over it the moment a pointer goes up
   there. It sits below the notice, on the line the toolbar itself occupied,
   as a floating chip. The 74px clears a two-line notice at `top: 8px`. */
/* **The row takes no presses; only the button does.** The row is the width
   of the window, so without this a text selection could not begin anywhere
   along the band the button sits in. `pointer-events` inherits, which is why
   the button has to ask for them back. */
.peek-line {
  position: absolute; left: 0; right: 0; top: 74px; z-index: 30;
  display: flex; align-items: flex-start; justify-content: flex-end;
  padding-right: 10px;
  pointer-events: none;
}
/* Dressed as `.notice` is, for `.notice`'s reason. */
.toolbar-peek {
  display: flex; align-items: center; gap: 6px;
  white-space: pre-line; text-align: left;
  padding: 6px 12px; border: 1px solid var(--bar-line); border-radius: 9px;
  background: var(--paper); box-shadow: 0 2px 8px rgba(0,0,0,0.08);
  color: var(--muted); font-size: 13.5px;
  pointer-events: auto;
}
.toolbar-peek:hover { background: var(--bar-hover); color: var(--text); }

/* **The page pill**: where the reader is, said while they scroll with the
   toolbar away — the one thing that answers "which page is this" when the
   bar that usually answers it has been put down. Placed by a row rather than
   by `left: 50%` and a transform, for the reason `.notice-line` above is. No
   animation: nothing moves unless the reader moved it. */
/* **It rides the scrollbar**: over the foot of the page it would cover the
   thing it describes, and beside the bar it is where the eye already is
   during the one gesture it exists for. `app.rs` sets `top` to the middle of
   the thumb and this keeps the corner for a document short enough to have no
   bar at all. The padding is the bar's twelve pixels and a gap. */
.pill-line {
  position: absolute; left: 0; right: 0; bottom: 20px; z-index: 20;
  display: flex; align-items: center; justify-content: flex-end;
  padding-right: 20px;
  pointer-events: none;
}
/* The way back from a link: bottom right of the document, clear of the
   scrollbar, dressed as `.notice` is. */
.back-chip {
  position: absolute; right: 24px; bottom: 20px; z-index: 7;
  display: flex; align-items: center;
  border-radius: 9px;
  background: var(--paper); border: 1px solid var(--bar-line);
  box-shadow: 0 2px 8px rgba(0,0,0,0.08);
}
.back-go, .back-close {
  display: flex; align-items: center; gap: 6px;
  height: 32px; border: none; background: none;
  color: var(--text); font: inherit; font-size: 13.5px;
}
.back-go { padding: 0 10px 0 10px; border-radius: 8px 0 0 8px; }
.back-close { padding: 0 8px; border-left: 1px solid var(--bar-line); border-radius: 0 8px 8px 0; }
.back-go:hover, .back-close:hover { background: var(--bar-hover); }
.back-close:not(:hover) > .icon.hot, .back-close:hover > .icon.rest { display: none; }
/* Dressed as `.notice` is. */
.page-pill {
  padding: 6px 12px; border-radius: 9px;
  background: var(--paper); border: 1px solid var(--bar-line);
  box-shadow: 0 2px 8px rgba(0,0,0,0.08);
  color: var(--muted); font-size: 13.5px;
}

/* Presenting: full screen with nothing else on it. The chrome is gone from
   the DOM rather than hidden here — see `Viewer::chrome`, which is what gives
   the document the room the toolbar was using — so all that is left for CSS
   is the surround. It is the theme's paper rather than its `--surround`: with
   nothing else on screen the frame around the page is the only thing left
   that is not the page, and the darker shade reads as a border on a window
   that has none. */
.root.presenting .viewer { background: var(--paper); }

/* --------------------------------------------------------- the Settings window

   A scrim and a frame in the same document. Blitz has no `position: fixed`,
   so the scrim is absolute against the root — which is a flex column filling
   the window, so the two come to the same thing. `z-index` above the menus,
   because Settings is over everything including the bar it was opened from. */
/* **The scrim is the theme's backdrop at 62%, not black.** A black wash
   darkens a pale theme into something that looks switched off and turns a
   warm one grey; a wash of the theme's own surround leaves every theme
   recognisably itself. `--scrim` is that colour with its alpha already in
   it, mixed in `palette.rs` where the rest of the shades are.
   (There is no blur behind it. `backdrop-filter` is parsed by Stylo and
   carried through `blitz-paint` to `PaintScene::push_layer`, but every
   anyrender backend drops it — `anyrender_vello_hybrid 0.10.0`,
   `anyrender_vello 0.14.0`. Blurring the *document* instead, with an ordinary
   `filter`, is a filter over the whole window every frame a window is up, and
   it makes a stacking context of the reader — which is the one thing Blitz's
   hit testing is known to get wrong.) */
/* **The page organiser covers the window**, under Settings and the
   questions it can open, and over the bar and the panel. Its pages stand on
   the surround, as the document's do, and its bar is a window's bar. */
.organiser {
  position: absolute; top: 0; left: 0; right: 0; bottom: 0; z-index: 15;
  display: flex; flex-direction: column;
  background: var(--surround); color: var(--text);
}
.organiser-bar {
  display: flex; align-items: center; gap: 12px; flex: 0 0 auto;
  height: 52px; box-sizing: border-box; padding: 0 12px 0 18px;
  background: var(--surface); border-bottom: 1px solid var(--line);
}
.organiser-title { flex: 0 0 auto; font-size: 15px; font-weight: 600; }
.organiser-count { flex: 0 0 auto; color: var(--muted); font-size: 13px; white-space: nowrap; }
.organiser-tools { flex: 1 0 auto; display: flex; align-items: center; gap: 6px; justify-content: center; }
.organiser-ends { flex: 0 0 auto; display: flex; align-items: center; gap: 8px; }
.chip.organiser-remove:hover { color: var(--negative); }
.organiser-split { position: relative; display: flex; align-items: center; }
.menu.organiser-split-menu { top: calc(100% + 8px); left: 0; min-width: 260px; }
.organiser-size { margin-left: 6px; }
.organiser-size .chip { width: 30px; padding: 0; justify-content: center; font-size: 16px; }
/* A bar narrower than what it holds loses the count first, then the words
   of the tools, then the title, and last the turns and the page size, which
   the keyboard still has, and in the narrowest window the tools stand
   closer together. The tools never shrink below what they hold, so each
   step comes where the step before stops fitting, with room for a font that
   sets the words a tenth wider than the tests' font does. */
@media (max-width: 1460px) { .organiser-count { display: none; } }
@media (max-width: 1340px) {
  .organiser-tools .chip-label { display: none; }
  .organiser-tools .chip { padding: 0 7px; }
}
@media (max-width: 800px) { .organiser-title { display: none; } }
@media (max-width: 660px) {
  .organiser-tools [data-item=turn-left], .organiser-tools [data-item=turn-right],
  .organiser-size { display: none; }
}
@media (max-width: 520px) {
  .organiser-bar { padding-left: 12px; }
  .organiser-tools { gap: 2px; }
}
/* The grid scrolls by moving the sheet of pages under a window that does
   not, as the thumbnail column does: only the rows in view are drawn. */
.organiser-grid { position: relative; flex: 1 1 auto; min-height: 0; overflow: hidden; }
.organiser-sheets { position: absolute; left: 0; right: 0; }
.organiser-empty {
  position: absolute; top: 80px; left: 0; right: 0; margin: 0;
  text-align: center; color: var(--surround-muted);
}
.organiser-head {
  position: absolute; left: 24px; right: 24px; height: 32px;
  display: flex; align-items: center; gap: 12px;
  color: var(--surround-muted); font-size: 13.5px; font-weight: 600;
}
.chip.organiser-join { height: 26px; font-size: 12.5px; color: var(--surround-muted); }
/* With several files, a heading's name is a button: a click types a new one. */
.organiser-head-name.nameable {
  border: 0; padding: 2px 6px; margin-left: -6px; border-radius: 6px;
  background: transparent; color: inherit; font: inherit; text-align: left;
}
.organiser-head-name.nameable:hover { background: var(--hover); color: var(--text); }
.organiser-name { width: 280px; height: 28px; }
/* A page's menu sits under the pointer, in the window; the Select field
   is wide enough for a list of ranges. */
.organiser .menu.sheet-menu { position: absolute; z-index: 3; width: 280px; }
.organiser-typed { width: 130px; }
.menu-item[disabled] { opacity: 0.45; }
.sheet {
  position: absolute; border: 0; background: transparent; padding: 0;
  display: flex; flex-direction: column; align-items: center;
}
.sheet-picture {
  position: relative; background: var(--page); border-radius: 3px;
  box-shadow: 0 0 0 1px var(--line);
}
.sheet.picked .sheet-picture { box-shadow: 0 0 0 3px var(--accent); }
.sheet-label {
  height: 22px; margin-top: 4px; max-width: 100%; overflow: hidden;
  text-overflow: ellipsis; white-space: nowrap;
  color: var(--surround-faint); font-size: 12px;
}
.sheet.picked .sheet-label { color: var(--accent); }
.organiser-drop {
  position: absolute; width: 3px; border-radius: 2px;
  background: var(--accent); pointer-events: none;
}
.organiser-lift {
  position: absolute; z-index: 2; pointer-events: none;
  padding: 4px 10px; border-radius: 10px;
  background: var(--accent); color: var(--accent-contrast); font-size: 12.5px;
}

.window-scrim {
  position: absolute; top: 0; left: 0; right: 0; bottom: 0; z-index: 20;
  display: flex; align-items: center; justify-content: center;
  background: var(--scrim);
}
.window-scrim.clear { background: transparent; }
/* What a press beside the copy menu lands on, over every window: it puts the
   menu away and goes no further. Clear, because the menu is the news. */
.menu-catch { position: absolute; top: 0; left: 0; right: 0; bottom: 0; z-index: 30; }
.menu.copy-menu { min-width: 0; }
/* On `--surface`, the shade everything that floats is mixed to, not on
   `--paper`, the colour of the *page*: on a dark theme a window in `--paper`
   comes up the colour of a sheet of paper in a dark room. And 15.5px, a size
   up from the sidebar and the menus: this is somewhere you have arrived to
   read, not more chrome. */
.window {
  display: flex; flex-direction: column; font-size: 15.5px;
  width: 860px; height: 600px; max-width: 92%; max-height: 92%;
  border-radius: 14px; border: 1px solid var(--line);
  background: var(--surface); color: var(--text);
  box-shadow: 0 18px 60px rgba(0,0,0,0.28);
}
.window-bar {
  display: flex; align-items: center; gap: 10px; flex: 0 0 auto;
  height: 48px; padding: 0 10px 0 18px; border-bottom: 1px solid var(--line);
}
.window-title { flex: 1 1 auto; font-size: 16px; font-weight: 600; }
.window-search { flex: 0 0 auto; width: 200px; }
/* While Settings is searched, every page is shown with its title and its
   matches and nothing else, and a page with no matches is hidden. A heading
   or list that a page filters by the search has the class `matched`. */
.searching > :not(.pane-title):not(.field):not(.theme-grid):not(.matched) {
  display: none;
}
.searching { margin-bottom: 28px; }
.searching[data-hide], .no-match[data-hide], .settings-hit { display: none; }
.chip.window-close { width: 30px; padding: 0; justify-content: center; }
.window-body { flex: 1 1 auto; display: flex; flex-direction: row; min-height: 0; }

/* The password window, the one window in this reader that fits what is in it
   in both directions: a lede, a field and two buttons is four rows, and a
   600px frame around them would be a window asking a one-line question in a
   hall. */
.ask-window { width: 420px; height: auto; }
.ask-body { padding: 16px 20px 18px; display: flex; flex-direction: column; gap: 12px; }
.ask-body .pane-lede { margin: 0; }
/* The field holds the password and does not show it: the ink is taken away,
   the caret is given a colour of its own so that it is still there, and the
   bullets are a span over the top. See the field in `app.rs` for why it is
   this way round and not the obvious one. */
.ask-field-wrap { position: relative; display: block; }
.ask-field { width: 100%; height: 32px; color: transparent; caret-color: var(--text); }
/* 9px is the field's 8px of padding and its 1px of border, so a bullet stands
   where the character it stands for would have. */
.ask-bullets {
  position: absolute; left: 9px; top: 0; height: 32px; line-height: 32px;
  color: var(--text); font-size: 13.5px; letter-spacing: 1px;
}
/* The gap above is the column's, not the row's — see `.pane-actions`, which
   carries a margin for the pages in Settings where it follows a long list. */
.ask-actions { margin-top: 0; justify-content: flex-end; }

/* The Sign window — see `sign.rs`. Wider
   than a note and narrower than Settings, because the widest thing in it is
   the pad and the pad is as wide as a signature needs. Its size is on the
   element rather than here: see `app::PAD_WIDTH`. */
.sign-window { width: 520px; height: auto; max-height: 88%; }
.sign-body {
  padding: 16px 20px 18px; display: flex; flex-direction: column; gap: 12px;
  overflow: scroll; scrollbar-width: thin;
}
.sign-body .pane-lede { margin: 0; }
.sign-body .pane-group { margin: 4px 0 0 0; }
.sign-list { display: flex; flex-direction: column; gap: 6px; }
.sign-row { display: flex; align-items: center; gap: 6px; }
/* A row is the drawing and the name, and the drawing is the larger half of it
   — a list of signatures is chosen from by looking, not by reading. */
.sign-use {
  flex: 1 1 auto; display: flex; align-items: center; gap: 14px;
  min-width: 0; padding: 6px 10px; border: 1px solid var(--line); border-radius: 10px;
  background: var(--surface); color: var(--text); text-align: left;
}
.sign-use:hover { background: var(--hover); border-color: var(--accent); }
.sign-name { flex: 1 1 0; min-width: 0; font-weight: 500; }
/* A row for a signature already in the document is a fact rather than a
   choice: there is nothing to press on it but the bin, so it is not a button
   and does not look like one. */
.sign-placed {
  flex: 1 1 auto; display: flex; align-items: baseline; gap: 10px;
  min-width: 0; padding: 6px 10px;
}
.sign-where { color: var(--faint); font-size: 13.5px; }
/* A digital signature is the document's rather than the reader's: nothing here
   can add one and nothing can take one off, so the row carries no controls at
   all and is drawn as the statement it is. The name is what it is and the
   detail is whatever the document said, so the second half is the one that
   gets the room. */
.sign-seal .sign-placed {
  border: 1px solid var(--line); border-radius: 10px; background: var(--sunk);
}
.sign-seal .sign-name { flex: 0 0 auto; }
.sign-seal .sign-where { flex: 1 1 auto; min-width: 0; }
.sign-forget {
  flex: 0 0 auto; width: 30px; height: 30px; padding: 0;
  display: flex; align-items: center; justify-content: center;
  border: 0; border-radius: 9px; background: transparent;
}
.sign-forget:hover { background: var(--hover); }
/* The pad. A dashed edge, because it is a place to *do* something rather than
   a thing to read — the same shape the app draws around a document being
   dragged over the window, and for the same reason. */
.sign-pad {
  position: relative; display: block;
  border: 1px dashed var(--line); border-radius: 12px; background: var(--sunk);
}
/* **The hint fills the pad, and that is not a matter of taste.** An event
   here carries coordinates relative to the node it *landed on*, and the press
   handler on the pad turns them into a point in the pad — so a child sitting
   at any other origin measures the press from the wrong corner, and a
   signature begun on a hint in the middle of the pad would be measured from
   the middle. `pointer-events: none` says the same thing a second way, so
   that a change to the layout cannot bring the fault back. */
.sign-pad-hint {
  position: absolute; inset: 0; display: flex;
  align-items: center; justify-content: center;
  color: var(--faint); pointer-events: none;
}
.scrawl { display: block; }
/* A signature is waiting for somewhere to go, and until it has one every page
   is a target. The crosshair is the whole of what says so once the window has
   closed — that, and the line at the foot of the screen. */
.root.placing .page { cursor: crosshair; }

/* The Information window: what the document says about itself, a row a fact.

   As tall as its facts, and as wide as `.window`, so that a long producer
   string stays on one line. A row is the label at the left and its value
   ranged *right*, with a rule between rows: a column of right-ranged values
   has an edge to read down. The label is in the ink at 500 and only the value
   in the quiet shade, so that a row reads as a question and an answer rather
   than as two greys. */
.details-window { height: auto; }
/* `.pane-title` carries the size and the weight; only the air under it is
   its own, 2px, which is closer than the 4px a pane heading gets because the
   first rule follows immediately. */
.details-name { margin: 0 0 2px 0; }
.details-row {
  display: flex; align-items: center; gap: 18px;
  padding: 13px 0; border-bottom: 1px solid var(--line);
}
.details-row:last-child { border-bottom: 0; }
/* The details that can be changed: a field where the value would be, ranged
   right like the values below it. */
.details-field {
  flex: 0 1 300px; min-width: 0; box-sizing: border-box; height: 30px; padding: 0 9px;
  border: 0; border-radius: 7px; background: var(--sunk); color: var(--text);
  font: inherit; font-size: 15px; text-align: right;
}
.details-actions { display: flex; justify-content: flex-end; padding-top: 14px; }
.details-label { flex: 1 1 auto; min-width: 0; color: var(--text); font-weight: 500; }
/* `max-width` is what keeps a file path from
   pushing its label off the left of the window. `overflow-wrap` beside
   `word-break` because a path is one word: without it the box is 440px wide
   and the word is wider. */
.details-value {
  flex: 0 1 auto; max-width: 440px; text-align: right;
  color: var(--note); font-size: 15px;
  word-break: break-word; overflow-wrap: anywhere;
}
/* The nav column is the *sunk* shade, not the surface, which is what tells
   the column from the page beside it, since the window itself is the
   surface. */
.window-nav {
  flex: 0 0 186px; display: flex; flex-direction: column; gap: 2px;
  padding: 10px 8px; border-right: 1px solid var(--line); background: var(--sunk);
}
/* `justify-content` beside `text-align` for the reason `.chip.title` names:
   Blitz's user-agent sheet centres a button's flex items as well as its text,
   and the icon and the label here are two items with nothing between them to
   take up the slack, so the pair would float in the middle of the column.
   `.menu-item` escapes it because `.menu-label` grows;
   this has no such item, so the alignment has to be said. */
.nav-item {
  display: flex; align-items: center; justify-content: flex-start; gap: 9px;
  height: 32px; padding: 0 10px; border: 0; border-radius: 9px;
  background: transparent; color: var(--muted); font-weight: 500;
  text-align: left;
}
.nav-item:hover { background: var(--hover); color: var(--text); }
/* The same pair as a chip in force: the tint carries it and the accent is
   legible on the tint. See `.chip.on`. */
.nav-item.on { background: var(--accent-soft); color: var(--accent-ink); }
/* `scroll`, not `auto` — Blitz has no `auto`, which is the note at the top of
   this file. Reading is the longest page and does not fit in 600px. */
/* And `hidden` across, which is the other half of "a sideways swipe carries
   nothing away": nothing in this window is meant to be wider than the window,
   so a page that turns out to be has a row that is too wide and not a page to
   be scrolled sideways. See `.root`. */
.window-pane {
  flex: 1 1 auto; min-width: 0; padding: 18px 26px 28px 26px;
  overflow-x: hidden; overflow-y: scroll; scrollbar-width: thin;
}
/* `letter-spacing: -0.01em` closes a 20px title up a little. */
/* Its own optical size, for the reason `.start-name` has one. */
.pane-title { margin: 0 0 4px 0; font-size: 20px; font-variation-settings: "opsz" 20;
  font-weight: 600; letter-spacing: -0.01em; }
/* **The small print is in `--note`, not `--faint`**: anything quieter falls
   under 4.5:1 against the paper, and the sentence that explains a switch
   would be harder to read than the switch. It takes the window's size rather
   than a smaller one of its own. */
.pane-lede { margin: 0 0 12px 0; color: var(--note); }
.pane-group {
  margin: 22px 0 8px 0; font-weight: 500;
  color: var(--faint);
}
.pane-note { margin: 0 0 12px 0; color: var(--note); line-height: 1.5; }
/* Blitz counts neither the pane's bottom padding nor a last child's margin in
   how far the pane scrolls, so the last note would sit flush against the
   window's bottom edge. Padding inside the note is counted. */
.window-pane .pane-note:last-child { padding-bottom: 28px; }
/* **A Settings page that is taller than the window fades out at the bottom,**
   so that a card cut off by the edge reads as more to scroll to. The fade
   covers the last 28 pixels of the pane, which is its bottom padding, so at
   the end of the page nothing but empty space is under it. */
.settings-pane { mask-image: linear-gradient(to bottom, #000 calc(100% - 28px), transparent); }
.pane-actions + .pane-note { margin-top: 16px; }
.pane-actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 16px; }
/* The three shapes an action button takes; the plain one is below, with the
   colour picker. */
.chip.action.primary { background: var(--accent); color: var(--accent-contrast); border-color: var(--accent); }
.chip.action.primary:hover { background: var(--accent); color: var(--accent-contrast); }
.chip.action.danger { background: var(--negative); color: var(--negative-contrast); border-color: var(--negative); }
.chip.action.danger:hover { background: var(--negative); color: var(--negative-contrast); }

/* A field somebody types into, and a colour: the swatch shows what the page
   will use, the six digits say it. */
.text-field, .window-search {
  height: 28px; min-width: 0; padding: 0 8px;
  border: 1px solid var(--line); border-radius: 8px;
  background: var(--paper); color: var(--text); font-size: 13.5px;
}
.text-field:focus, .window-search:focus { outline: none; border-color: var(--accent); }
/* `position: relative`, because the grid of colours hangs off the swatch and
   has to be over the field below rather than pushing it down the page. */
.color-field { display: flex; align-items: center; gap: 6px; position: relative; }
.color-swatch {
  width: 26px; height: 26px; border-radius: 7px; border: 1px solid var(--line);
  padding: 0;
}
.color-hex { width: 96px; }
/* **What the field says about six digits it cannot read**, which is the whole
   of how a typed colour is allowed to be wrong for a moment: the text stays as
   typed, wearing this, and Enter or leaving the field puts the theme's own
   colour back. Nothing here corrects anybody mid-word. */
.color-hex.unreadable { border-color: var(--negative); }
.color-hint { color: var(--negative); font-size: 12.5px; white-space: nowrap; }
/* The picker: a saturation/value square, a hue strip and the forty swatches,
   in a column. Blitz has no `<input type="color">`, so the spectrum has to be
   drawn here.

   Their two sizes are not here: they are written in `prefs.rs` and inlined
   into the elements, because a pointer's position has to become a fraction of
   a box that cannot be measured from inside a handler, and one number is
   safer than two that have to agree. `background-origin` and `background-clip`
   so that the layers line up with what the pointer is measured against, which
   is the border box. Hung from the field's right edge, which is the
   window's side, so that it does not run past the edge of the dialog. */
.color-picker {
  position: absolute; top: 32px; right: 0; z-index: 5;
  display: flex; flex-direction: column; gap: 8px;
  padding: 8px; border-radius: 10px;
  background: var(--surface); border: 1px solid var(--line);
}
.color-picker.up { top: auto; bottom: 32px; }
.color-square, .color-strip {
  border: 1px solid var(--line);
  background-origin: border-box; background-clip: border-box;
  cursor: crosshair;
}
.color-square { border-radius: 8px; }
.color-strip { border-radius: 9px; }
/* Eight across, which is the grey row: the hues below it are four steps each,
   so two hues to a row and the pale ones line up down the left. */
.color-grid { display: grid; grid-template-columns: repeat(8, 24px); gap: 4px; }
.color-choice {
  width: 24px; height: 24px; padding: 0; border-radius: 6px;
  border: 1px solid var(--line);
}
.color-choice.on { box-shadow: 0 0 0 2px var(--surface), 0 0 0 3px var(--accent); }
/* A block with its label centred by `line-height`, not the flex row `.chip`
   is: Blitz does not restyle a bare text node inside a flex button when the
   theme's variables change, which would leave the label in the last theme's
   ink. An action chip has no
   icon, so it needs no row. */
.chip.action {
  border: 1px solid var(--line); background: var(--surface); color: var(--text);
  display: block; line-height: 28px; text-align: center;
}
.chip.action:hover { background: var(--hover); }

/* One setting. The control sits on the same line as the name and the sentence
   runs under both, which is what keeps a page of switches readable as prose
   rather than as a form. */
.field { padding: 13px 0; border-bottom: 1px solid var(--line); }
/* A rule separates; after the last setting there is nothing to separate. */
.field:last-child { border-bottom: 0; }
.field-head { display: flex; align-items: center; gap: 18px; }
.field-label { flex: 1 1 auto; color: var(--text); font-weight: 500; }
.field-control { flex: 0 0 auto; display: flex; align-items: center; }
.field-note { margin: 6px 0 0 0; color: var(--note); font-size: 15px; line-height: 1.5; }
/* A narrow window leaves the pane about two hundred pixels, too little for a
   row of segments beside its label. There the control goes under its
   label when the two do not fit side by side, and the nav column gives up a
   little of its width. */
@media (max-width: 720px) {
  .window-nav { flex-basis: 150px; }
  .field-head { flex-wrap: wrap; row-gap: 8px; }
  .field-control { max-width: 100%; }
  .segmented { flex-wrap: wrap; }
}

/* A small, quiet control: 34 by 20, a knob of 14 in the faint ink, and a
   hairline round the track when it is off, so that a switch reads as *off*
   when it is off — a white knob on a pale track is a lamp with the light on.
   `justify-content` is said because a switch is a button, and Blitz centres a
   button's contents, which would put the knob in the middle of the track. */
.switch {
  width: 34px; height: 20px; padding: 3px; border: 0; border-radius: 10px;
  background: var(--sunk); box-shadow: inset 0 0 0 1px var(--line);
  display: flex; align-items: center; justify-content: flex-start;
}
.switch.on { background: var(--accent); box-shadow: none; }
.switch-knob {
  width: 14px; height: 14px; border-radius: 7px; background: var(--faint);
}
/* On, the knob is the ink the accent is written in, which keeps it legible
   on the accent whatever the accent is. */
.switch.on .switch-knob { background: var(--accent-contrast); }
/* The knob moves by being pushed, not by being positioned: Blitz has the box
   model and this needs nothing else. Fourteen is the track less its two
   paddings less the knob. */
.switch.on .switch-knob { margin-left: 14px; }

.segmented {
  display: flex; gap: 2px; padding: 2px; border-radius: 10px; background: var(--sunk);
}
.segment {
  height: 26px; padding: 0 10px; border: 0; border-radius: 8px;
  background: transparent; color: var(--muted); font-size: 13px; font-weight: 500;
  white-space: nowrap;
}
.segment:hover { color: var(--text); }
/* The accent, as `.tab.on` says "this one": paper on a sunk track would all
   but vanish in a light theme, where the two are a shade apart. */
.segment.on { background: var(--accent-soft); color: var(--accent-ink); }
/* The choice a fresh install has, said as the menus say it: a quiet word
   beside the label rather than "(default)" inside it. */
.segment-default { margin-left: 6px; font-size: 11.5px; font-weight: 400; color: var(--note); }
.segment.on .segment-default { color: var(--accent-ink); opacity: 0.7; }

.stepper {
  display: flex; align-items: center; gap: 2px;
  padding: 2px; border-radius: 10px; background: var(--sunk);
}
.stepper .chip { height: 26px; width: 26px; padding: 0; justify-content: center; }
/* The number, typed. Sized to its digits for the reason `.page-field` is:
   Blitz gives parley no alignment for an input's text, so a box wider than
   what is in it is a number against its left wall. */
.step-field {
  box-sizing: border-box;
  height: 26px; padding: 0 6px; border: 0; border-radius: 7px;
  background: transparent; color: var(--text); font-size: 13.5px;
}
.step-field:focus { outline: none; background: var(--paper); }
/* The unit is the muted grade at the number's own size: told apart by colour
   alone, because the faint grade at a smaller size on the sunk ground is
   hard to read at all.

   Raised a pixel: Blitz sets a span's baseline one pixel under the baseline
   of an input's text in the same centred row — measured by typing an x into
   the field beside "px" — so the unit would read as sagging. */
.step-unit { color: var(--muted); font-size: 13.5px; padding-right: 4px;
  position: relative; top: -1px; }

/* The theme list: a swatch of the three colours that decide what a theme
   looks like, and its name. Resolved through the theme parser before they
   get here — a swatch showing a colour the renderer cannot read is the picker
   lying about the page. */
.theme-grid { display: flex; flex-wrap: wrap; gap: 10px; }
/* `justify-content` and `text-align` against the button's centring, so that
   a name on two lines ("Tokyo Night Storm") neither lifts its card's picture
   above its neighbours' nor runs left. */
.theme-card {
  display: flex; flex-direction: column; gap: 6px; justify-content: flex-start;
  box-sizing: border-box; width: 132px; padding: 8px;
  border: 1px solid var(--line); border-radius: 12px;
  background: transparent; color: var(--text); text-align: center;
}
.theme-card:hover { background: var(--hover); }
.theme-card.on { border-color: var(--accent); background: var(--accent-soft); }
/* `align-self` and `box-sizing`, both load-bearing: a flex item in a column
   does not stretch here on its own, and the padding and the hairline would
   otherwise be added to the 52. The rest is a line of the theme's own body ink
   and a link in the theme's own link colour, centred on a ground of the
   theme's paper, which is what puts a theme's link colour on screen at all.
   `align-items: baseline` so the two sizes sit on one line rather than on
   their own boxes. */
.theme-swatch {
  display: flex; align-items: baseline; justify-content: center; align-self: stretch;
  gap: 8px; box-sizing: border-box;
  height: 52px; padding: 15px 8px 0 8px; border-radius: 8px;
  border: 1px solid var(--line); font-size: 17px;
}
.swatch-link { font-size: 14px; text-decoration: underline; text-underline-offset: 2px; }
.theme-name { font-size: 14px; font-weight: 500; }
.theme-mine { font-size: 11.5px; color: var(--note); }

/* The Keyboard page's table: two columns, and the chord in the quiet shade
   because it is the answer and the action is the question. */
.keys { display: flex; flex-wrap: wrap; }
.key-what { flex: 0 0 62%; padding: 5px 0; font-size: 14.5px; }
.key-chord { flex: 0 0 38%; padding: 5px 0; color: var(--faint); font-size: 14px; }
"#;
