//! The mailbox: everything a window is told, answered.
//!
//! One task per reader, spawned by `Reader` and woken by whoever posts — the
//! watcher thread, the shell, the clock in `emit::after`. The `match` is on
//! [`Event`], so an event nobody answers does not compile.

use super::*;

pub(super) async fn listen(
    mut viewer: Signal<Viewer>,
    post: crate::emit::Post,
    pointer: Pointer,
    resting: Rc<Resting>,
    screen: Screen,
    appearance: Appearance,
    frame: Frame,
) {
    loop {
        let news = post.next().await;
        match news.event {
            // Four seconds after something was said, said by a thread of its
            // own. It clears the line only if nothing has been said since.
            Event::NoticeTimeout(token) => {
                if viewer.read().notice_token.get() == token {
                    viewer.write().notice.clear();
                }
            }
            // The way back from a link has been up for a while. A later link
            // raised it again with its own timer, and a pointer over the chip
            // keeps it up until it leaves, which starts a timer of its own.
            Event::BackTimeout(offers) => {
                let held = viewer.read();
                if held.back_offered && held.back_offers == offers && !held.back_held {
                    drop(held);
                    viewer.write().dismiss_back();
                }
            }
            // The pointer is still on the link it came to rest on, so the
            // card showing where the link leads goes up.
            Event::SyncTexForward => viewer.write().take_forward(),
            Event::GlanceDue(rests) => {
                if viewer.read().glance_waiting(rests) {
                    viewer.write().show_glance();
                }
            }
            // A second after the reader stopped scrolling, and only if
            // nothing has scrolled since — see `Viewer::flash_pill`.
            //
            // Every timer but the last is stale, and a `write` is a render
            // whether it changes anything or not: asked under `read` first, a
            // scroll's hundred timers cost nothing.
            Event::PillTimeout(token) => {
                if viewer.read().pill_token.get() == token {
                    viewer.write().unflash_pill(token);
                }
            }
            // And a few seconds after it, the bar goes the same way.
            Event::BarTimeout(token) => {
                if viewer.read().bar_token.get() == token {
                    viewer.write().unflash_bar(token);
                }
            }
            // The pointer has been left alone for a while. It is asked for
            // once per rest rather than once per move of the mouse — a timer
            // armed by every move would be a hundred a second, each outliving
            // what armed it — so what fires here may be early, and an early
            // one arms the remainder rather than hiding anything.
            Event::CursorTimeout => {
                resting.waiting.set(false);
                if !viewer.read().hides_cursor() {
                    continue;
                }
                let Some(moved) = resting.moved.get() else {
                    continue;
                };
                let rests = viewer.read().cursor_rests();
                let still_for = super::since(moved);
                if still_for >= rests {
                    viewer.write().pointer_away = true;
                    pointer.show(false);
                } else {
                    resting.waiting.set(true);
                    crate::emit::after(
                        rests - still_for,
                        post.clone(),
                        crate::emit::News {
                            event: Event::CursorTimeout,
                            target: None,
                        },
                    );
                }
            }
            // One step of the stationary scroll, and the next one armed — a
            // clock the gesture starts and stops rather than one that runs.
            // See the block on it in `Viewer`.
            //
            // The speed is read before anything is written, because the
            // pointer spends most of the gesture inside the dead zone:
            // writing the viewer to move it nothing would be a render a frame
            // for as long as the anchor is up.
            Event::StillTick(token) => {
                let Some((across, down)) = viewer.read().still_speed(token) else {
                    continue;
                };
                if (across, down) != (0.0, 0.0) {
                    viewer.write().drift(across, down);
                }
                crate::emit::after(
                    STILL_TICK,
                    post.clone(),
                    crate::emit::News {
                        event: Event::StillTick(token),
                        target: None,
                    },
                );
            }
            // A sweep held at the edge of the document. The roll ends itself
            // when the pointer comes back in or lets go.
            Event::SweepTick(token) => {
                let Some(down) = viewer.read().sweep_speed(token) else {
                    if viewer.read().sweep_roll == Some(token) {
                        viewer.write().sweep_roll = None;
                    }
                    continue;
                };
                viewer.write().roll(down);
                crate::emit::after(
                    STILL_TICK,
                    post.clone(),
                    crate::emit::News {
                        event: Event::SweepTick(token),
                        target: None,
                    },
                );
            }
            Event::SheetTick(token) => {
                if viewer.write().roll_sheets(token) {
                    crate::emit::after(
                        STILL_TICK,
                        post.clone(),
                        crate::emit::News {
                            event: Event::SheetTick(token),
                            target: None,
                        },
                    );
                }
            }
            // The fingers stopped moving. See [`Viewer::settle_zoom`].
            Event::ZoomSettled(token) => {
                if viewer.read().zoom_token == token {
                    viewer.write().settle_zoom(token);
                }
            }
            Event::CropMeasured(crop, token) => viewer.write().measured(crop, token),
            Event::ThemesChanged(themes) => viewer.write().themes_changed(themes),
            Event::PalettesChanged(palettes) => viewer.write().palettes_changed(palettes),
            // A theme worn in this window or another: the settings are one
            // table, and this puts what it says on the pages.
            Event::ThemeWorn => viewer.write().theme_worn(),
            // A setting changed in another window that this one only has to
            // draw again to show.
            Event::SettingsChanged => {
                let restarted = viewer.write().settings_changed();
                rescan(viewer, restarted);
            }
            // Documents named another way: see `Viewer::names_changed`.
            Event::NamesChanged => viewer.write().names_changed(),
            // Reload pressed on the Keyboard page of any window.
            Event::KeysReloaded => viewer.write().read_keys(),
            // The interface's size, changed in this window or another.
            Event::UiScaled => viewer.write().ui_scaled(),
            // A settings or library write the disk would not take — a file
            // broken by hand while the app runs. See `store::refused`.
            Event::DiskRefused(why) => viewer.write().notice = why,
            Event::DocumentChanged(path) => {
                let restarted = viewer.write().document_changed(&path);
                rescan(viewer, restarted);
            }
            // The markup of a document just opened. See
            // [`Viewer::read_markup`].
            Event::MarkupRead => viewer.write().markup_landed(),
            // The Sign window's lists. See [`Viewer::read_signed`].
            Event::SignedRead => viewer.write().signed_landed(),
            Event::Shelved => viewer.write().shelved(),
            // A write of this window's own, back from its thread.
            Event::DocumentWritten => {
                let restarted = viewer.write().landed();
                rescan(viewer, restarted);
            }
            Event::DocumentOpened => viewer.write().opened(),
            Event::AnnotationsRead => viewer.write().annotations_read(),
            // A document is over the window, and whether it is one this
            // reader would open. Both answers are worth having: a hint that
            // says "drop to open" over a folder is a promise nothing keeps.
            Event::DragOver(takeable) => viewer.write().dragging = Some(takeable),
            Event::DragLeft => viewer.write().dragging = None,
            // Let go on something that is not a document, said on the
            // notice line.
            Event::DragRefused => {
                let mut held = viewer.write();
                held.dragging = None;
                held.notice = "That is not a PDF or a picture.".into();
            }
            // A document handed to this window by the process: a second
            // launch, "Open with", a double-click. It arrives here rather
            // than at a new window because this one is showing nothing — see
            // `Desk::hand_over` — and the bookkeeping afterwards is ⌘O's,
            // because this is ⌘O with somebody else choosing the file.
            // Handed to this window because the desk had it down as empty.
            // Trusted from the window, not the bookkeeping: if something got
            // here first — or a password is being asked for — the document
            // goes beside it, never over it.
            Event::HandedOver(ref path) | Event::OpenDocument(ref path) => {
                let path = path.clone();
                viewer.write().dragging = None;
                let handed = matches!(news.event, Event::HandedOver(_));
                take_document(&mut viewer, &frame, path, handed);
            }
            // Files let go on the window. Dropped on the organiser, they are
            // added to the draft together, as "Add files…" would add them. A
            // picture is a page for the organiser, never a document: it
            // opens the organiser on whatever this window reads, unless the
            // window is in the middle of opening something. Anything else is
            // a document handed over.
            Event::Dropped(paths) => {
                viewer.write().dragging = None;
                let settled = viewer.read().locked.is_none() && viewer.read().opening.is_none();
                let picture = |path: &String| crate::shell::is_picture(std::path::Path::new(path));
                if viewer.read().organiser.is_some() || (settled && paths.iter().any(picture)) {
                    viewer.write().take_dropped(paths);
                    continue;
                }
                // A picture is no document to send on, and the organiser
                // cannot open over a document still being opened. It is said
                // last, so that a document dropped with it does not say
                // something over it.
                let (pictures, documents): (Vec<_>, Vec<_>) = paths.into_iter().partition(picture);
                for path in documents {
                    take_document(&mut viewer, &frame, path, true);
                }
                if !pictures.is_empty() {
                    let waiting = viewer.read().locked.is_some();
                    viewer.write().notice = match waiting {
                        true => "This document is waiting for its password. Drop the picture again once it is open.",
                        false => "This window is still opening a document. Drop the picture again when it has.",
                    }
                    .into();
                }
            }
            // The system's close button, or its Quit: the same question the
            // keymap's ⌘W and ⌘Q ask, and the same answer.
            Event::CloseAsked(quit) => {
                let may = match quit {
                    true => !viewer.write().asks_before_closing(Ask::QuitAnswered),
                    false => viewer.write().may_close(),
                };
                if may {
                    frame.ask(if quit { Ask::QuitAnswered } else { Ask::Close });
                }
            }
            // The same document, through the other door: the picker was
            // opened by "Open document in new window…", so what it chose goes
            // beside this window rather than into it. See `Pick` — a picker
            // cannot answer where it was asked, so which door it was is
            // carried in the event.
            Event::OpenDocumentBeside(path) => {
                if !path.is_empty() {
                    frame.ask(Ask::NewWindowOn(path));
                }
            }
            Event::OpenDocumentInTab(path) => {
                if !path.is_empty() {
                    frame.ask(Ask::NewTabOn(path));
                }
            }
            // A theme file chosen under Appearance, answered here for
            // `Pick`'s reason. See `theme_file_dialog` in `prefs.rs`.
            Event::ImportTheme(path) => viewer.write().import_theme(&path),
            Event::ExportTheme(path) => viewer.write().export_theme(&path),
            // A file a page edit asked for. See `app::Choose`.
            Event::Chosen(choosing, paths) => viewer.write().chosen(choosing, paths),
            // The window changed size, which nothing else in this process
            // will tell the layout — Blitz resizes its own viewport and asks
            // for a redraw, and a redraw of a layout computed for the old
            // window is the old layout. See `Shell::on_resized`, which is the
            // other half.
            Event::WindowResized(full) => {
                let (width, height, _scale) = screen.get();
                viewer.write().fit_screen(width, height);
                // The window is what is in full screen. See
                // [`Viewer::window_full`].
                if let Some(full) = full {
                    viewer.write().window_full(full);
                }
            }
            // Two fingers, moving apart or together. macOS gives the change
            // since the last event as a fraction, so the gesture's whole
            // scale is the product of them and each one is a proportion to
            // zoom by. See `Viewer::zoom_by`.
            Event::Pinched(delta) => viewer.write().pinch_by(1.0 + delta),
            Event::PinchEnded => viewer.write().end_pinch(),
            // The machine went light or dark while the reader was reading.
            // Nothing carries the answer — the event says only that there is
            // a new one, exactly as a resize does, and it is asked of the
            // window. See `Shell::on_theme`.
            Event::AppearanceChanged => viewer.write().follow_system(appearance.get()),
            // A tab opened or closed beside this one takes the focus with it,
            // so coming forward is when "Close tab" is asked again.
            Event::Focused => {
                let tabbed = viewer.read().tabs.get();
                if viewer.read().tabbed != tabbed {
                    viewer.write().tabbed = tabbed;
                }
            }
        }
    }
}

/// A document for this window: opened here, or, when it was handed over
/// and this window is showing something, sent on to another. A window
/// arranging pages is showing something even with no document open.
fn take_document(viewer: &mut Signal<Viewer>, frame: &Frame, path: String, handed: bool) {
    if path.is_empty() {
        return;
    }
    let full = !viewer.read().empty()
        || viewer.read().locked.is_some()
        || viewer.read().opening.is_some()
        || viewer.read().organiser.is_some();
    if handed && full {
        frame.ask(Ask::SendOn(path));
        return;
    }
    viewer.write().open_here(&path);
}
