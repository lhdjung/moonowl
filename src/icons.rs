//! The app's icon set.
//!
//! **The paths are the drawing.** They came over from the retired frontend's
//! `icons.ts` character for character — two drawings of the same 24px grid
//! that drift apart being exactly the kind of copy `AGENTS.md` warns about —
//! and this is the only copy now. Only the icons this reader's chrome
//! actually uses are here.

/// The owl off `icons/app-icon.svg`, without its scroll, for the start screen:
/// its points and its `scale(1.22,1.32)` restated (one half, drawn twice
/// rather than `use`d), so a change to the owl's face is a change in all three
/// drawings. Lines take the `<svg>`'s stroke; eyes and beak its CSS `color`.
pub fn owl() -> String {
    const HALF: &str = "M0,-400 L95,-385 L165,-320 L185,-230 L180,-150 L175,-60 L150,20 L95,60 L0,70 M0,-315 L60,-345 L130,-300 L150,-230 L110,-170 L0,-140 M95,-385 L60,-345 M165,-320 L130,-300 M185,-230 L150,-230 M180,-150 L110,-170 M0,-140 L70,-60 L180,-150 M110,-170 L70,-60 M70,-60 L0,10 L0,70 M70,-60 L125,10 L175,-60 M125,10 L95,60 M0,10 L95,60 M70,62 L40,100 M70,62 L72,106 M70,62 L104,100";
    format!(
        r#"<g transform="scale(1.22,1.32)"><path d="{HALF}"/><path transform="scale(-1,1)" d="{HALF}"/><path d="M0,-215 L22,-195 L0,-165 L-22,-195 Z" fill="currentColor" stroke="currentColor" stroke-width="14"/><circle cx="75" cy="-255" r="12" fill="currentColor" stroke="none"/><circle cx="-75" cy="-255" r="12" fill="currentColor" stroke="none"/></g>"#
    )
}

/// The shapes of one icon, as the inside of an `<svg viewBox="0 0 24 24">`.
///
/// Named by the app's own key, so that a button here and a button there ask
/// for the same drawing by the same name.
pub fn path(name: &str) -> Option<&'static str> {
    Some(match name {
        "contents" => r#"<path d="M4 6.5h.01M4 12h.01M4 17.5h.01M9 6.5h11M9 12h11M9 17.5h7"/>"#,
        "pages" => {
            r#"<rect x="4" y="4" width="7" height="7" rx="1.4"/><rect x="13" y="4" width="7" height="7" rx="1.4"/><rect x="4" y="13" width="7" height="7" rx="1.4"/><rect x="13" y="13" width="7" height="7" rx="1.4"/>"#
        }
        "search" => r#"<circle cx="11" cy="11" r="6.5"/><path d="M16 16l4.5 4.5"/>"#,
        "minus" => r#"<path d="M5.5 12h13"/>"#,
        "plus" => r#"<path d="M12 5.5v13M5.5 12h13"/>"#,
        "close" => r#"<path d="M6.5 6.5l11 11M17.5 6.5l-11 11"/>"#,
        "check" => r#"<path d="M5 12.5l4.6 4.5L19 7.5"/>"#,
        "box" => r#"<rect x="4.5" y="4.5" width="15" height="15" rx="3"/>"#,
        "boxChecked" => {
            r#"<rect x="4.5" y="4.5" width="15" height="15" rx="3"/><path d="M8 12.2l2.8 2.8L16 9.5"/>"#
        }
        "document" => {
            r#"<path d="M13.5 3.5H7.5A2 2 0 0 0 5.5 5.5v13a2 2 0 0 0 2 2h9a2 2 0 0 0 2-2V8.5z"/><path d="M13.5 3.5v5h5"/>"#
        }
        "fitWidth" => {
            r#"<path d="M4 6.5v11M20 6.5v11M8 12h8M8 12l2.5-2.5M8 12l2.5 2.5M16 12l-2.5-2.5M16 12l2.5 2.5"/>"#
        }
        "fitPage" => {
            r#"<rect x="6" y="4" width="12" height="16" rx="1.6"/><path d="M12 8.5v7M12 8.5l-2 2M12 8.5l2 2M12 15.5l-2-2M12 15.5l2-2"/>"#
        }
        "rotateRight" => {
            r#"<path d="M7.2 18.1A7.5 7.5 0 1 1 18.5 12"/><path d="M15 8.5l3.5 3.5L22 8.5"/>"#
        }
        "rotateLeft" => {
            r#"<path d="M16.8 18.1A7.5 7.5 0 1 0 5.5 12"/><path d="M9 8.5l-3.5 3.5L2 8.5"/>"#
        }
        "plusCircle" => r#"<circle cx="12" cy="12" r="8.2"/><path d="M12 8.5v7M8.5 12h7"/>"#,
        "trash" => {
            r#"<path d="M5.5 7h13M9.5 7V5.5h5V7M7 7l.8 12a1.6 1.6 0 0 0 1.6 1.5h5.2a1.6 1.6 0 0 0 1.6-1.5L17 7"/>"#
        }
        "edit" => {
            r#"<path d="M4.5 19.5h4l10-10a2.1 2.1 0 0 0-3-3l-10 10z"/><path d="M14.5 6.5l3 3"/>"#
        }
        "print" => {
            r#"<path d="M7 9V4h10v5"/><rect x="4" y="9" width="16" height="7" rx="1.6"/><path d="M7 14h10v6H7z"/>"#
        }
        "copy" => {
            r#"<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M15 5.5A1.5 1.5 0 0 0 13.5 4H5.5A1.5 1.5 0 0 0 4 5.5v8A1.5 1.5 0 0 0 5.5 15"/>"#
        }
        "mark" => r#"<path d="M7 4h10v16l-5-4-5 4z"/>"#,
        "comment" => {
            r#"<path d="M5.5 5h13A1.5 1.5 0 0 1 20 6.5v8a1.5 1.5 0 0 1-1.5 1.5H11l-4.5 3.5V16h-1A1.5 1.5 0 0 1 4 14.5v-8A1.5 1.5 0 0 1 5.5 5z"/>"#
        }
        "fullscreen" => {
            r#"<path d="M4 9V5.5A1.5 1.5 0 0 1 5.5 4H9M15 4h3.5A1.5 1.5 0 0 1 20 5.5V9M20 15v3.5a1.5 1.5 0 0 1-1.5 1.5H15M9 20H5.5A1.5 1.5 0 0 1 4 18.5V15"/>"#
        }
        "link" => {
            r#"<path d="M10.5 13.5a3.8 3.8 0 0 0 5.4 0l3-3a3.8 3.8 0 0 0-5.4-5.4l-1.2 1.2"/><path d="M13.5 10.5a3.8 3.8 0 0 0-5.4 0l-3 3a3.8 3.8 0 0 0 5.4 5.4l1.2-1.2"/>"#
        }
        // A drawing per window item of the Open menu: a window with a plus
        // is the empty one "New" makes; a tab is that window with its title
        // bar split and the left tab open into the page; a second window
        // stands behind the first for "in new window".
        "windowPlus" => {
            r#"<rect x="3.5" y="4.5" width="17" height="15" rx="2"/><path d="M3.5 9.2h17M12 11.6v5.4M9.3 14.3h5.4"/>"#
        }
        "tab" => {
            r#"<rect x="3.5" y="4.5" width="17" height="15" rx="2"/><path d="M11 4.5v4.7h9.5"/>"#
        }
        "tabPlus" => {
            r#"<rect x="3.5" y="4.5" width="17" height="15" rx="2"/><path d="M11 4.5v4.7h9.5M12 11.6v5.4M9.3 14.3h5.4"/>"#
        }
        "windows" => {
            r#"<path d="M7.5 8.5V5.5a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-2"/><rect x="3.5" y="8.5" width="13" height="12" rx="2"/><path d="M3.5 12.3h13"/>"#
        }
        "folder" => {
            r#"<path d="M3 7.5A2.5 2.5 0 0 1 5.5 5h3.2l2 2.2h7.8A2.5 2.5 0 0 1 21 9.7v7.8A2.5 2.5 0 0 1 18.5 20h-13A2.5 2.5 0 0 1 3 17.5z"/>"#
        }
        "theme" => {
            r#"<circle cx="12" cy="12" r="8.2"/><path d="M12 3.8a8.2 8.2 0 0 1 0 16.4z" fill="currentColor" stroke="none"/>"#
        }
        "up" => r#"<path d="M6 14.5L12 8.5l6 6"/>"#,
        "back" => r#"<path d="M19 12H5.5M11 6l-6 6 6 6"/>"#,
        "down" => r#"<path d="M6 9.5l6 6 6-6"/>"#,
        "book" => {
            r#"<path d="M4 5.5A1.5 1.5 0 0 1 5.5 4H10a2 2 0 0 1 2 2v13a1.8 1.8 0 0 0-1.8-1.5H4z"/><path d="M20 5.5A1.5 1.5 0 0 0 18.5 4H14a2 2 0 0 0-2 2v13a1.8 1.8 0 0 1 1.8-1.5H20z"/>"#
        }
        "sidebar" => {
            r#"<rect x="3.5" y="4.5" width="17" height="15" rx="2.2"/><path d="M10 4.5v15"/>"#
        }
        "keyboard" => {
            r#"<rect x="2.8" y="6.5" width="18.4" height="11" rx="2.2"/><path d="M6.5 10h.01M9.8 10h.01M13.1 10h.01M16.4 10h.01M6.5 13.2h.01M9.8 13.2h.01M13.1 13.2h.01M16.4 13.2h.01M8.5 16h7"/>"#
        }
        "info" => {
            r#"<circle cx="12" cy="12" r="8.2"/><path d="M12 11v5.2"/><path d="M12 7.9h.01"/>"#
        }
        "settings" => {
            r#"<path d="M9.4 3.4h5.2l.2 2.3 1.3.7 2-1 2.7 4.6-1.9 1.3v1.4l1.9 1.3-2.7 4.6-2-1-1.3.7-.2 2.3H9.4l-.2-2.3-1.3-.7-2 1L3.2 14l1.9-1.3v-1.4L3.2 10l2.7-4.6 2 1 1.3-.7zM15.1 12a3.1 3.1 0 1 0-6.2 0 3.1 3.1 0 0 0 6.2 0z" fill="currentColor" fill-rule="evenodd"/><circle cx="12" cy="12" r="3.1"/>"#
        }
        // **Not in `icons.ts`**: the app cannot sign a document, so it never
        // needed a drawing for it. A nib with a name trailing off it — the shape every
        // application uses for this, and the one that reads as *ink* rather
        // than as a certificate, which is the whole distinction
        // [`crate::sign`] is built around.
        "sign" => {
            r#"<path d="M3.5 18.5c3-.6 4.4-2.6 5.2-5.1.7-2.2 1-4.6 2.3-4.6 1.1 0 1 1.6.3 2.8-.8 1.3-2 1.8-2 3 0 .8.7 1.2 1.6 1.2 1.6 0 2.6-1.1 3.6-1.1.7 0 1 .5 1 1.1"/><path d="M17.5 15.8c1.4 0 2.2-.6 3-1.4"/><path d="M14.2 8.4l4.6-4.6a1.7 1.7 0 0 1 2.4 2.4l-4.6 4.6-2.9.5z"/>"#
        }
        _ => return None,
    })
}
