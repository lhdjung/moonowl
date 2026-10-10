//! What every action does, once the keymap has said which.

use super::menus::copy_quote;
use super::*;

/// One handler per action, and a dispatch of about thirty lines: the table
/// decides *which* action, so nothing here knows anything about keys.
///
/// **There are no arms missing.** All forty-four of the app's actions answer
/// here, and the catch-all that used to say "not built yet" is gone — so an
/// action added to [`crate::keymap`] and not handled here is a compile error
/// rather than a sentence in the notice line.
pub(super) fn perform(
    mut viewer: Signal<Viewer>,
    action: Action,
    screen: f64,
    frame: &Frame,
    clip: &Clip,
    pick: &Pick,
    printer: &Printer,
) {
    // Every movement goes through the viewer rather than through an offset,
    // because in paged mode an offset is not where a reader ends up: the page
    // has to be turned first. See [`Viewer::nudge`] and [`Viewer::go_to`].
    fn by(mut viewer: Signal<Viewer>, delta: f64) {
        viewer.write().nudge(delta);
    }

    match action {
        // Handled where the keystroke is, because which tab was asked for is
        // the digit that was pressed. See the key handler in `Reader`.
        Action::GoToTab => {}
        Action::ScrollDown => by(viewer, LINE),
        Action::ScrollUp => by(viewer, -LINE),
        Action::ScrollLeft => viewer.write().pan(-LINE),
        Action::ScrollRight => viewer.write().pan(LINE),
        Action::HalfScreenDown => by(viewer, (screen - OVERLAP) / 2.0),
        Action::HalfScreenUp => by(viewer, -(screen - OVERLAP) / 2.0),
        Action::ScreenDown => by(viewer, screen - OVERLAP),
        Action::ScreenUp => by(viewer, -(screen - OVERLAP)),
        Action::FirstPage => viewer.write().to_start(),
        Action::LastPage => viewer.write().to_end(),
        Action::NextPage => viewer.write().next_page(),
        Action::PreviousPage => viewer.write().previous_page(),
        Action::ZoomIn => viewer.write().zoom(true),
        Action::UiLarger => viewer.write().scale_ui(Some(true)),
        Action::UiSmaller => viewer.write().scale_ui(Some(false)),
        Action::UiReset => viewer.write().scale_ui(None),
        Action::ZoomOut => viewer.write().zoom(false),
        Action::FitWidth => viewer.write().set_fit(Fit::Width),
        Action::FitPage => viewer.write().set_fit(Fit::Page),
        Action::ActualSize => viewer.write().actual_size(),
        Action::RotateRight => viewer.write().rotate(1),
        Action::RotateLeft => viewer.write().rotate(-1),
        Action::NextTheme => viewer.write().next_theme(),
        Action::Dark => viewer.write().toggle_dark(),
        // F1 and ⌘/, and the app's own answer to both: the Keyboard page,
        // which is the list of everything this reader answers to and is drawn
        // from the keymap rather than from a list of its own. "Help" behind a
        // cog is a strange place to keep the answer to "what can this thing
        // do", which is why it is a key at all.
        Action::Help => viewer.write().show_pane(Pane::Keyboard),
        // ⌘P, and it prints nothing: the document is handed to a program that
        // does. See [`Printer`].
        Action::Print => {
            let path = viewer.read().document.path().to_string();
            if path.is_empty() {
                return;
            }
            if let Err(said) = printer.print(&path) {
                viewer.write().notice = said;
            }
        }
        Action::Sidebar => viewer.write().toggle_sidebar(),
        Action::Mark => {
            let page = viewer.read().page();
            viewer.write().mark_page(page);
        }
        Action::GoToPage => viewer.write().open_page_field(),
        // Silence would be the wrong answer at the end of the history: the
        // reader pressed a key and has no way to tell a shortcut that did
        // nothing from one that is not bound. The app says the same two
        // sentences.
        Action::Back => {
            if !viewer.write().go_back() {
                viewer.write().notice = "Nowhere further back".to_string();
            }
        }
        Action::Forward => {
            if !viewer.write().go_forward() {
                viewer.write().notice = "Nowhere further forward".to_string();
            }
        }
        Action::Find => {
            let token = viewer.write().open_find();
            rescan(viewer, token);
        }
        // After Escape the matches are gone and the query is not: ⌘G brings
        // the search back, as ⌘F does, rather than saying "No matches" for a
        // word that was found a moment ago. And with nothing asked yet, it
        // asks: "No matches" answered a search nobody had made.
        Action::FindNext | Action::FindPrevious
            if !viewer.read().find_open || viewer.read().find_query.is_empty() =>
        {
            let token = viewer.write().open_find();
            rescan(viewer, token);
        }
        Action::FindNext => viewer.write().step_match(true),
        Action::FindPrevious => viewer.write().step_match(false),
        // Escape, which is the way out of four things and says nothing when
        // there is nothing to leave — a key that answers with a complaint
        // about what it did not do is worse than one that does nothing.
        Action::Dismiss => {
            // Outward, in the order the reader arrived at them. A menu is
            // outermost of all — over everything and the thing the pointer is
            // inside — so it goes first, ahead of a field opened underneath it.
            // Then the page field: Escape while typing a number means "not that
            // after all", not "close the find bar I opened a minute ago".
            // Escape typed *into* the field never reaches here, so this is the
            // case where the pointer took the focus elsewhere.
            if viewer.write().close_menu() {
                return;
            }
            // A removal waiting for its second press: Escape is "not that".
            if viewer.write().arming.take().is_some() {
                return;
            }
            // "Delete this theme?" or "Break the signature?", which is over
            // everything, Settings included.
            if viewer.write().close_asking() {
                return;
            }
            // The highlight colours window, before the swatches it was opened
            // from: it is over them, and Escape means the thing on top.
            if viewer.write().close_markup_colours() {
                return;
            }
            // The colour popover, which is a menu in everything but name and
            // sits in the same place in this list.
            if viewer.write().close_markup() {
                return;
            }
            // …and the one a mark clicked on puts up, which is the same kind
            // of thing in the same place. See [`Viewer::mark_open`].
            if viewer.write().close_mark() {
                return;
            }
            // The theme editor's colour picker, which is the same kind of
            // thing one line further in: a popover inside the Settings window,
            // so Escape means it before it means the window around it. The
            // fields below it answer Escape themselves — see
            // `prefs::typing_is_not_a_shortcut` — and never reach here.
            if viewer.write().close_picker() {
                return;
            }
            // Settings next, and above everything below it: it is a window
            // over the reader, and Escape inside a window means that window.
            // Below the menus because a menu opened *from* Settings is inside
            // it, which is the same "outward, in the order the reader arrived"
            // this whole list is.
            if viewer.write().close_settings() {
                return;
            }
            // And a note opened on its page, which a press elsewhere puts
            // away as it does the mark's menu.
            if viewer.write().close_note() {
                return;
            }
            // The Information window, which is the note's neighbour in every
            // other respect and was missing from this list: Escape closed the
            // note beside it and left this one up. `showDocumentDetails` puts
            // up the same kind of window and the app's modal closes on Escape
            // whatever is in it.
            if viewer.write().close_details() {
                return;
            }
            // The Sign window, then a signature looking for somewhere to go.
            // Two states and the reader is in one of them: the window is over
            // the page, so Escape means the window; and a signature waiting to
            // be placed has no window, so Escape is the only way to put it down
            // without signing something.
            if viewer.write().close_signing() {
                return;
            }
            if viewer.write().put_down() {
                return;
            }
            // And the password window. Below the rest because a reader inside
            // it is inside a field, so this is only reached when the pointer has
            // taken the keyboard elsewhere. Withdrawing the question is not
            // answering it with an empty password.
            if viewer.write().stop_unlocking() {
                return;
            }
            let (typing, finding, selected, presenting, full) = {
                let held = viewer.read();
                (
                    held.typing_page,
                    held.find_open,
                    held.has_selection(),
                    held.presenting,
                    held.full_screen,
                )
            };
            if typing {
                viewer.write().cancel_page();
            } else if finding {
                viewer.write().close_find();
            } else if selected {
                // Between the find bar and presenting: a selection is a thing
                // on the page, where full screen and presenting are things the
                // window is doing. A reader presenting who has swept a sentence
                // to point at means to put the sentence down first.
                viewer.write().clear_selection();
            } else if presenting {
                let full = viewer.write().present(false);
                frame.ask(Ask::FullScreen(full));
            } else if full {
                viewer.write().set_full_screen(false);
                frame.ask(Ask::FullScreen(false));
            }
        }
        // The three a selection is for. `Copy` is this experiment's own —
        // see `keymap::EXTRA`: in the app ⌘C is the webview's, because the
        // browser owns the selection and therefore owns copying it.
        Action::SelectPage => {
            viewer.write().select_page();
        }
        Action::Copy => copy_selection(viewer, clip),
        Action::Undo => viewer.write().undo(),
        Action::Redo => viewer.write().redo(),
        Action::CopyQuote => copy_quote(viewer, clip),
        // The window's own three, which the page can only ask for. See
        // [`Frame`]: what answers is the shell in the app and a list in the
        // harness, and the reader's side is the same either way.
        // The picker, through `Pick` rather than the shell directly, because a
        // modal window belonging to the operating system is the one door a test
        // must not be able to open. A window of its own is a menu item and not
        // a key, there being no `open-new-window` in `keys.ts` either.
        Action::Open => pick.ask(Opening::Here),
        Action::NewWindow => frame.ask(Ask::NewWindow),
        Action::NewTab => frame.ask(Ask::NewTab),
        Action::PreviousTab => frame.ask(Ask::StepTab(false)),
        Action::NextTab => frame.ask(Ask::StepTab(true)),
        Action::CloseWindow => frame.ask(Ask::Close),
        Action::Minimize => frame.ask(Ask::Minimize),
        Action::Quit => frame.ask(Ask::Quit),
        Action::Toolbar => viewer.write().toggle_toolbar(),
        Action::Fullscreen => {
            // While presenting the window is in full screen whatever the
            // reader's own switch says, so the key leaves it — and presenting,
            // which has nowhere else to be.
            if viewer.read().presenting {
                viewer.write().present(false);
                viewer.write().set_full_screen(false);
                frame.ask(Ask::FullScreen(false));
                return;
            }
            let on = !viewer.read().full_screen;
            viewer.write().set_full_screen(on);
            frame.ask(Ask::FullScreen(on));
        }
        Action::Present => {
            let on = !viewer.read().presenting;
            let full = viewer.write().present(on);
            frame.ask(Ask::FullScreen(full));
        }
        Action::Settings => viewer.write().open_settings(),
        // ⌘⇧H. The key opens the popover rather than marking in the last
        // colour used, which is the app's arrangement: a colour is a decision
        // and a keystroke that made one silently would be a keystroke nobody
        // could undo.
        Action::Markup => {
            if !viewer.write().open_markup() {
                let held = viewer.read();
                let nothing = held.text_on(held.page()).is_empty();
                drop(held);
                viewer.write().notice = if nothing {
                    "There is no text on this page to highlight.".into()
                } else {
                    "Select something first, and this highlights it.".into()
                };
            }
        }
        // One page across and back to the pair the reader chose last, which
        // the key used to answer with Cover whatever that was: somebody on
        // "Two side by side" pressed `s` twice and was on Cover.
        Action::Spread => {
            let next = if viewer.read().layout.spread == Spread::Single {
                viewer.read().last_pair
            } else {
                Spread::Single
            };
            viewer.write().set_spread(next);
        }
    }
}
