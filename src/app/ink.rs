//! The Viewer's ink: the Sign window, and a signature put on a page.

use super::*;

impl Viewer {
    /* --------------------------------------------------------------- ink */

    /// Open the Sign window, or say why it cannot be.
    ///
    /// The refusals are asked *here* rather than when the signature is placed,
    /// which is the opposite of markup and right for the opposite reason: a
    /// mark is one gesture and may as well try and report, where signing is a
    /// window, a drawing and a click.
    pub fn open_signing(&mut self) -> bool {
        // The menu comes down whichever way this goes: a refusal is a notice,
        // and a notice under an open menu is one nobody reads.
        self.menu = None;
        self.sign_here = None;
        if self.empty() {
            return false;
        }
        let standing = crate::sign::standing(
            self.document.path(),
            self.document.encrypted(),
            self.document.sealed(),
        );
        if !standing.into_file {
            self.notice = format!("{}, so it cannot be signed.", standing.refused);
            return false;
        }
        self.signing = Some(Signing {
            kept: self.signatures(),
            // The pad opens empty and the name opens empty with it. A default
            // of "Signature" is what `sign::save` falls back to, and putting
            // it in the field would mean a reader who types their own name has
            // to clear somebody else's word out of the way first.
            ..Default::default()
        });
        // Read once per draft: the lists already read of this document are
        // still its, and a read still on its way lands in the window now
        // open. Only a document not yet read is read, on a thread.
        let known = self
            .signed_known
            .as_ref()
            .filter(|(of, _)| {
                of.upgrade().is_some_and(|of| {
                    std::ptr::addr_eq(Arc::as_ptr(&of), Arc::as_ptr(&self.document))
                })
            })
            .map(|(_, read)| read.clone());
        match known {
            Some((signed_here, seals)) => {
                if let Some(signing) = self.signing.as_mut() {
                    signing.signed_here = signed_here;
                    signing.seals = seals;
                }
            }
            None if self.signed_reading.is_none() => self.read_signed(),
            None => {}
        }
        true
    }

    /// Take it down. `false` when it was not up, which is what lets Escape go
    /// on to the next thing it means.
    pub fn close_signing(&mut self) -> bool {
        self.signing.take().is_some()
    }

    /// A press beside the window: it closes one with nothing in it, and
    /// leaves one holding a drawing or words — a hand signing a name runs off
    /// the pad more often than not, and one stray press lost the lot. Escape
    /// and the window's own × still close it.
    pub fn close_signing_unless_drawn(&mut self) {
        let started = self.signing.as_ref().is_some_and(|pad| {
            !pad.strokes.is_empty() || !pad.name.trim().is_empty() || !pad.line.trim().is_empty()
        });
        if !started {
            self.close_signing();
        }
    }

    /// Where the signatures are kept, which is the config directory this
    /// reader was given rather than the ambient one. See [`crate::sign::dir`].
    pub fn signatures(&self) -> Vec<crate::sign::Signature> {
        crate::sign::load_all(self.store.dir())
    }

    /// A press on the pad: begin a stroke, and note where the pad is.
    ///
    /// The difference between `on` (inside the pad) and `client` (in the
    /// window) is the pad's top left corner, which a later move needs and
    /// cannot ask for. `begin_sweep` records the same number for the same
    /// reason.
    pub fn draw_from(&mut self, on: (f64, f64), client: (f64, f64), pad: (f64, f64)) {
        let Some(signing) = self.signing.as_mut() else {
            return;
        };
        signing.pad = pad;
        signing.origin = (client.0 - on.0, client.1 - on.1);
        signing.drawing = true;
        signing.strokes.push(vec![[on.0, on.1]]);
    }

    /// The pointer moved in the window with the button down, while a stroke is
    /// running. Taken into the pad's own space and clamped to it: a hand that
    /// runs off the edge should stop at the edge rather than write a signature
    /// whose box is the whole window.
    pub fn draw_on_pad(&mut self, client: (f64, f64)) {
        let Some((origin, pad)) = self
            .signing
            .as_ref()
            .filter(|signing| signing.drawing)
            .map(|signing| (signing.origin, signing.pad))
        else {
            return;
        };
        self.draw_to((
            (client.0 - origin.0).clamp(0.0, pad.0),
            (client.1 - origin.1).clamp(0.0, pad.1),
        ));
    }

    /// One point onto the stroke that is running, in the pad's own space.
    pub fn draw_to(&mut self, at: (f64, f64)) {
        let Some(signing) = self.signing.as_mut() else {
            return;
        };
        if !signing.drawing {
            return;
        }
        let Some(stroke) = signing.strokes.last_mut() else {
            return;
        };
        // A point that has not moved is a point that says nothing, and a
        // signature drawn slowly would otherwise be a thousand of them. Half a
        // pixel is below what anybody can see and above what a jittering
        // trackpad reports while a finger rests.
        if stroke
            .last()
            .is_some_and(|last| (last[0] - at.0).abs() < 0.5 && (last[1] - at.1).abs() < 0.5)
        {
            return;
        }
        stroke.push([at.0, at.1]);
    }

    /// And the button let go. A stroke of one point is kept: a dot over an i
    /// is a stroke of one point, and [`crate::sign::place`] knows what to do
    /// with one.
    pub fn draw_done(&mut self) {
        if let Some(signing) = self.signing.as_mut() {
            signing.drawing = false;
        }
    }

    /// Throw away what is on the pad, leaving the window up. The thing anybody
    /// wants after the first attempt at signing with a trackpad.
    pub fn clear_pad(&mut self) {
        if let Some(signing) = self.signing.as_mut() {
            signing.strokes.clear();
            signing.drawing = false;
        }
    }

    /// Keep what is on the pad, and answer whether it was kept.
    ///
    /// **The strokes go across as drawn**, in the pad's own pixels, and
    /// `sign::save` normalises them — one division rather than two. Two was
    /// the bug: dividing x by the pad's width and y by its height scales the
    /// axes differently, so a wide name arrived narrow. A pad pixel is square,
    /// so handing over pixels loses nothing.
    pub fn keep_signature(&mut self) -> bool {
        let Some(signing) = self.signing.clone() else {
            return false;
        };
        if signing.strokes.iter().all(|stroke| stroke.is_empty()) {
            self.notice = "Draw a signature first, and this keeps it.".into();
            return false;
        }
        let drawn = crate::sign::Signature {
            name: signing.name.trim().to_string(),
            id: String::new(),
            strokes: signing.strokes.clone(),
        };
        // Named here and written by the scribe: the answer — what it was
        // stored as — is wanted now, and the file is not on this thread's
        // account. See [`crate::store::later`].
        let pending = signing.kept.clone();
        match crate::sign::named(self.store.dir(), &drawn, &pending) {
            Ok(stored) => {
                // A write the disk refuses is said on the notice line, over
                // the "Kept" this says now. See `store::refused`.
                let (dir, writing) = (self.store.dir().to_path_buf(), stored.clone());
                crate::store::later(move || {
                    crate::store::refused(&dir, crate::sign::write(&dir, &writing));
                });
                self.notice = format!("Kept {}.", stored.name);
                // The pad is cleared rather than the window closed: keeping a
                // signature and using one are two things, and a reader who has
                // just drawn one very often wants to draw the initials too.
                self.clear_pad();
                // The list with it in, rather than the directory read again:
                // the file is the scribe's to write and is not there yet, and
                // neither is the last save's. Read off the disk, the third of
                // three quick saves took the first one's name and replaced
                // its file.
                let mut kept = pending;
                kept.push(stored.clone());
                kept.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
                if let Some(signing) = self.signing.as_mut() {
                    signing.name.clear();
                    signing.kept = kept;
                }
                true
            }
            Err(why) => {
                self.notice = why;
                false
            }
        }
    }

    /// Take one off the list, and off the disk.
    pub fn forget_signature(&mut self, id: &str) {
        match crate::sign::named_file(self.store.dir(), id) {
            Ok(file) => {
                let dir = self.store.dir().to_path_buf();
                crate::store::later(move || {
                    let removed = std::fs::remove_file(&file).map_err(|e| e.to_string());
                    crate::store::refused(&dir, removed);
                });
            }
            Err(why) => {
                self.notice = why;
                return;
            }
        }
        // The list without it, rather than the directory read again: the file
        // is the scribe's to remove and has not gone yet.
        if let Some(signing) = self.signing.as_mut() {
            signing.kept.retain(|kept| kept.id != id);
        }
    }

    /// **Read on a thread** what is already in the document this window is
    /// showing: every signature on its pages, which loads every page, and
    /// what it is *digitally* signed with (see [`crate::sign::Seal`]). On the
    /// thread that draws, opening the Sign window on a book is seconds of a
    /// frozen window. The lists fill in when [`Viewer::signed_landed`] takes
    /// them. A read, not a write: quit waits for writes and not for this.
    pub(super) fn read_signed(&mut self) {
        // An index is a place in the draft before: a row still holding one
        // would take the wrong annotation out of the file.
        if let Some(signing) = self.signing.as_mut() {
            signing.signed_here.clear();
        }
        self.signed_known = None;
        let landing = Arc::new(Mutex::new(None));
        self.signed_reading = Some(Arc::clone(&landing));
        let (document, post) = (self.document.clone(), self.post.clone());
        let reading = crate::stats::Reading::begin();
        std::thread::spawn(move || {
            let _reading = reading;
            let read = (document.signatures(), document.seals());
            *landing.lock().unwrap_or_else(|e| e.into_inner()) =
                Some((Arc::downgrade(&document), read));
            post.send(crate::emit::News {
                event: Event::SignedRead,
                target: None,
            });
        });
    }

    /// The read [`Viewer::read_signed`] started, if it is the latest: one for
    /// a draft since replaced landed in a slot nobody holds any more.
    pub fn signed_landed(&mut self) {
        let Some((of, (signed_here, seals))) = self
            .signed_reading
            .as_ref()
            .and_then(|landing| landing.lock().unwrap_or_else(|e| e.into_inner()).take())
        else {
            return;
        };
        self.signed_reading = None;
        if let Some(signing) = self.signing.as_mut() {
            signing.signed_here = signed_here.clone();
            signing.seals = seals.clone();
        }
        self.signed_known = Some((of, (signed_here, seals)));
    }

    /// **Take a signature back out of the document.**
    ///
    /// The assessment's one caveat was *it cannot be removed afterwards*, and
    /// that is true of the app alone: here it is `FPDFPage_RemoveAnnot`, the
    /// same call a highlight comes out through. A signature on the wrong page
    /// is the ordinary case, not the corner.
    ///
    /// Written off the main thread, as everything that writes the document
    /// is. See [`Viewer::write_step`].
    pub fn unsign(&mut self, page: usize, index: usize, kind: crate::sign::Written) {
        if self.busy() {
            return;
        }
        if !self.standing.into_file {
            self.notice = format!(
                "{}, so nothing can be taken out of it.",
                self.standing.refused
            );
            return;
        }
        // Asked once, as signing asks: see [`Viewer::sign_at`].
        if self.standing.signed && !self.said_rewrites {
            self.ask_to_break(
                CHANGING_BREAKS_A_SIGNATURE,
                "Remove anyway",
                move |viewer| {
                    viewer.said_rewrites = true;
                    viewer.unsign(page, index, kind);
                },
            );
            return;
        }
        let called = self.label(page);
        self.write_step(
            self.store.journal().to_vec(),
            move |path| crate::markup::remove(path, page, index),
            move |viewer, taken| match taken {
                Ok(()) => {
                    viewer.notice = match kind {
                        crate::sign::Written::Hand => format!("Signature taken off page {called}."),
                        crate::sign::Written::Line => format!("Text taken off page {called}."),
                    }
                }
                Err(refused) => viewer.notice = refused,
            },
        );
    }

    /// Choose a signature and go looking for somewhere to put it.
    ///
    /// The window closes, because the next question is *where* and the answer
    /// to it is on the page the window is covering.
    pub fn sign_with(&mut self, signature: crate::sign::Signature) {
        self.signing = None;
        self.placing = Some(Placing::Hand(signature));
        self.notice = "Click on the page where the signature should go.".into();
        self.place_where_asked();
    }

    /// Where "Sign here…" was chosen, the click on the page has been made
    /// already.
    fn place_where_asked(&mut self) {
        if let Some((page, point)) = self.sign_here.take() {
            self.sign_on(page, point);
        }
    }

    /// "Sign here…": the Sign window, and wherever the right-click was is
    /// where what is chosen in it goes.
    pub fn open_signing_at(&mut self, page: usize, on: (f64, f64)) {
        if self.open_signing() {
            let point = self.layout.unplace_on(page.saturating_sub(1), on.0, on.1);
            self.sign_here = Some((page, point));
        }
    }

    /// The same, for a date or a line of text. One gesture and one armed
    /// slot, because a reader placing something on a page is doing one thing
    /// whichever of the two it is.
    pub fn type_with(&mut self, line: String) {
        if line.trim().is_empty() {
            self.notice = "There is nothing typed to put on the page.".into();
            return;
        }
        self.signing = None;
        self.placing = Some(Placing::Line(line));
        self.notice = "Click on the page where the text should go.".into();
        self.place_where_asked();
    }

    /// Put it down again unsigned. `false` when nothing was armed, which is
    /// what lets Escape go on to the next thing it means.
    pub fn put_down(&mut self) -> bool {
        if self.placing.take().is_some() {
            self.notice = "Signing cancelled.".into();
            true
        } else {
            false
        }
    }

    /// **The click that signs.** `on` is where the pointer landed on the page,
    /// in the screen pixels a page is laid out in; `unplace_on` takes it the
    /// rest of the way, through the crop and the rotation, into the page's own
    /// points.
    ///
    /// The point is the *middle of the left edge* of the signature, not its
    /// top left: what a reader clicking on a line is aiming at is the line, so
    /// the signature sits on it rather than hanging below it.
    ///
    /// Written off the main thread. See [`Viewer::write_step`].
    pub fn sign_at(&mut self, page: usize, on: (f64, f64)) {
        let Some(index) = page.checked_sub(1) else {
            return;
        };
        let point = self.layout.unplace_on(index, on.0, on.1);
        self.sign_on(page, point);
    }

    /// The same, at a point already in the page's own points — which is how
    /// "Sign here…" keeps it while the Sign window is up.
    fn sign_on(&mut self, page: usize, (x, y): (f64, f64)) {
        if self.placing.is_none() || self.busy() {
            return;
        }
        let Some(placing) = self.placing.take() else {
            return;
        };
        // A hand is drawn to a height it chose and a line of type to a
        // smaller one: the name is the thing being said, and a date under it
        // is a note about the name.
        let height = match &placing {
            Placing::Hand(_) => HAND_HEIGHT,
            Placing::Line(_) => crate::sign::LINE_HEIGHT,
        };
        let at = Rect {
            left: x,
            top: y - height / 2.0,
            width: 0.0,
            height,
        };
        // Asked once, *before* it happens, and only for the document that
        // has something to lose. The signature stays armed, so "Sign anyway"
        // is this click again and Cancel leaves it waiting for another.
        if self.standing.signed && !self.said_rewrites {
            self.placing = Some(placing);
            self.ask_to_break(
                crate::sign::BREAKS_A_SIGNATURE,
                "Sign anyway",
                move |viewer| {
                    viewer.said_rewrites = true;
                    viewer.sign_on(page, (x, y));
                },
            );
            return;
        }

        let done = match &placing {
            Placing::Hand(_) => format!("Signed on page {}.", self.label(page)),
            Placing::Line(_) => format!("Written on page {}.", self.label(page)),
        };
        self.write_step(
            self.store.journal().to_vec(),
            move |path| match &placing {
                Placing::Hand(signature) => {
                    crate::sign::place(path, page, at, signature, crate::sign::INK)
                }
                Placing::Line(line) => {
                    crate::sign::place_text(path, page, at, line, crate::sign::INK)
                }
            },
            move |viewer, written| match written {
                Ok(()) => viewer.notice = done,
                // Nothing is kept beside the document, where a mark would be.
                // A highlight in the journal is still a passage the reader
                // marked; a signature that did not reach the file is not a
                // signature, and a list of them would be a promise this
                // reader cannot keep.
                Err(refused) => viewer.notice = refused,
            },
        );
    }
}
