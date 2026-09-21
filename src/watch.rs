//! What changes on the disk while the app is running.
//!
//! Two of the files the reader is looking at are not the app's to change. A
//! theme is TOML precisely so that somebody — or something — can open it in an
//! editor, and until now an edit was only seen at the next launch, which is a
//! poor way to pick a colour. The document is often a paper being recompiled
//! by LaTeX under the reader, and reopening it by hand to see the new draft is
//! the same poor way.
//!
//! Both are news the file system already has, and asking for it belongs here
//! rather than on the other side of the bridge: the work is on the disk, the
//! payload is a filename, and nothing crosses that is bigger than the answer.
//!
//! One thread owns one watcher and both subjects. Three things shape it.
//!
//! *A change is a burst, not an event.* A save from an editor is three or four
//! events, an atomic write is a create and a rename, and a LaTeX run is
//! hundreds over several seconds. Everything is collected until the file
//! system has been quiet for `SETTLE` and then acted on once.
//!
//! *A theme reload is decided by what the files say, not by what moved.* The
//! app writes into this directory itself — the shipped themes on every run, a
//! saved theme, the staging file `atomic_write` renames over — so an event is
//! not news. The themes are loaded and compared against the last set handed
//! over, and nothing is emitted unless they actually differ.
//!
//! *A document is believed only once it is whole.* A compiler writes its
//! output across the whole of a run, and what is on the disk in the middle of
//! one starts like a PDF and stops mid-sentence. Handing that over would take
//! the reader's document away and leave them on the start screen, so the file
//! has to end the way a PDF ends, and hold still, before anyone hears about
//! it.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use notify::{EventKind, RecursiveMode, Watcher};

use crate::emit::{Exchange, News, Payload};
use crate::theme;

/// How long the file system has to be quiet before a burst counts as one
/// change.
const SETTLE: Duration = Duration::from_millis(250);

/// And how long a document has to hold its size before it is believed.
const STEADY: Duration = Duration::from_millis(150);

/// How much of the end of a document to read looking for `%%EOF`. Generous:
/// the marker is the last line, but nothing forbids whitespace after it.
const TAIL: u64 = 1024;

/// The handle the rest of the app holds. Its only verb is "this window is
/// reading that document now", which `open_for_reading` and `close_document`
/// say between them — and a window on its way out.
pub struct Watching(Mutex<Sender<Signal>>);

impl Watching {
    /// Follow this document on behalf of a window, or stop following one.
    ///
    /// Per window because there is more than one, and each has a document of
    /// its own: a single subject meant the second window to open a document
    /// took the first window's watch away, so the paper being recompiled in
    /// the window nobody had touched last simply never reloaded.
    pub fn document(&self, window: &str, path: Option<&str>) {
        let signal = Signal::Follow(window.to_string(), path.map(PathBuf::from));
        if let Ok(sender) = self.0.lock() {
            let _ = sender.send(signal);
        }
    }

    /// Tell the watcher that this window just wrote `path` itself — a
    /// highlight, most likely — so the burst of file-system events that write
    /// produces is not news. Sent right after the write returns, which is
    /// well inside the settle window the real burst is collected in, so the
    /// baseline this sets is what the burst is compared against rather than
    /// something it races.
    ///
    /// Only this window's baseline moves. A second window with the same
    /// document open did not do this write, its transport is genuinely stale,
    /// and it still needs the ordinary reload — which is exactly what happens
    /// when its own entry is left alone to find a real mismatch.
    pub fn wrote(&self, window: &str, path: &Path) {
        let signal = Signal::Wrote(window.to_string(), path.to_path_buf());
        if let Ok(sender) = self.0.lock() {
            let _ = sender.send(signal);
        }
    }
}

enum Signal {
    Touched(Vec<PathBuf>),
    Follow(String, Option<PathBuf>),
    Wrote(String, PathBuf),
}

/// What a whole document looked like: its length, when it was last written,
/// and a fingerprint of the end of it.
///
/// The tail is in here because the first two are not enough on their own. A
/// file system with a coarse modification time — HFS+ keeps whole seconds —
/// answers a recompile that came out the same length within the same second
/// with a mark identical to the one before it, and the reload never fires. The
/// end of a PDF is its cross-reference table and trailer, which carry byte
/// offsets into everything above them, so two different drafts of the same
/// paper agreeing there is not a thing that happens. It is also free: `whole`
/// has already read those bytes to look for `%%EOF`.
type Mark = (u64, SystemTime, u64);

/// A document being followed, the directory the watch is actually on, and
/// what the file looked like when it was last whole. One per window that has
/// something open.
struct Followed {
    path: PathBuf,
    /// The same document as the file system names it. See [`real`].
    real: PathBuf,
    dir: PathBuf,
    mark: Option<Mark>,
}

/// Start watching the themes directory. The document half stays idle until
/// something is opened.
///
/// A watcher that cannot be created, or a directory that cannot be watched, is
/// not worth a message: the app then behaves exactly as it did before any of
/// this, which is to notice at the next launch.
pub fn start(exchange: Exchange, themes: PathBuf) -> Watching {
    let (sender, receiver) = mpsc::channel();
    let events = sender.clone();

    std::thread::spawn(move || {
        let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            if let Ok(event) = result {
                // Reading a file is not changing it, and on macOS the reads
                // are ours: every page of the open document comes through
                // `read_range`.
                if !matches!(event.kind, EventKind::Access(_)) {
                    let _ = events.send(Signal::Touched(event.paths));
                }
            }
        });
        let Ok(mut watcher) = watcher else { return };
        let _ = watcher.watch(&themes, RecursiveMode::NonRecursive);
        run(exchange, themes, receiver, &mut watcher);
    });

    Watching(Mutex::new(sender))
}

fn run(exchange: Exchange, themes: PathBuf, receiver: Receiver<Signal>, watcher: &mut dyn Watcher) {
    // What the frontend already has. Compared against, never emitted blindly.
    let mut known = theme::load_all(&themes);
    let real_themes = std::fs::canonicalize(&themes).unwrap_or_else(|_| themes.clone());
    // Keyed by window label. Two windows may well be reading two documents in
    // the same folder, which is why `follow` counts the folder rather than
    // taking the watch off with the document that named it.
    let mut documents: HashMap<String, Followed> = HashMap::new();

    while let Ok(first) = receiver.recv() {
        let mut touched: Vec<PathBuf> = Vec::new();
        let mut pending = Some(first);

        // Collect until things go quiet. `Follow` is handled as it arrives —
        // a document opened in the middle of a burst is still the document to
        // watch when the burst ends.
        loop {
            match pending.take() {
                Some(Signal::Touched(paths)) => touched.extend(paths),
                Some(Signal::Follow(window, next)) => {
                    follow(watcher, &themes, &mut documents, window, next)
                }
                Some(Signal::Wrote(window, path)) => absorb(&mut documents, &window, &path),
                None => {}
            }
            match receiver.recv_timeout(SETTLE) {
                Ok(next) => pending = Some(next),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }

        if touched.iter().any(|path| {
            path.parent() == Some(themes.as_path()) || path.parent() == Some(real_themes.as_path())
        }) {
            let current = theme::load_all(&themes);
            if current != known {
                known = current;
                exchange.post(News {
                    event: "themes-changed".into(),
                    target: None,
                    payload: Payload::Themes(known.clone()),
                });
            }
        }

        for (window, followed) in documents.iter_mut() {
            if !touched
                .iter()
                .any(|path| path == &followed.path || path == &followed.real)
            {
                continue;
            }
            if changed(followed, whole(&followed.path)) {
                let path = followed.path.to_string_lossy().to_string();
                // To that window and no other. A broadcast would tell every
                // window that *its* document had been rewritten, and each
                // would reopen the one it is holding for no reason.
                exchange.post(News {
                    event: "document-changed".into(),
                    target: Some(window.clone()),
                    payload: Payload::Text(path),
                });
            }
        }
    }
}

/// Whether a document just settled on a mark worth telling its window about,
/// updating the baseline either way. Pulled out of `run` so the decision can
/// be tested without a channel or a real watcher underneath it.
fn changed(followed: &mut Followed, disk: Option<Mark>) -> bool {
    let Some(mark) = disk else { return false };
    if Some(mark) == followed.mark {
        return false;
    }
    followed.mark = Some(mark);
    true
}

/// Pull a window's own write out of the next burst before it arrives: read
/// what the write actually left on disk and make that the baseline right now,
/// so `changed` above finds nothing new to report when the matching
/// file-system events turn up.
fn absorb(held: &mut HashMap<String, Followed>, window: &str, path: &Path) {
    if let Some(followed) = held.get_mut(window) {
        if followed.path == path {
            followed.mark = whole(path);
        }
    }
}

/// Point the document watch somewhere else, or nowhere.
///
/// The watch goes on the directory rather than on the file. A document is
/// replaced by writing another one beside it and renaming it over the top —
/// which is what `atomic_write` does here and what most compilers do — and a
/// watch on a file follows the file it was opened on rather than the name,
/// so it would go on watching something nobody can see any more.
fn follow(
    watcher: &mut dyn Watcher,
    themes: &Path,
    held: &mut HashMap<String, Followed>,
    window: String,
    next: Option<PathBuf>,
) {
    // Being pointed at the document already being followed is not a change of
    // subject, and treating it as one is worse than doing nothing. It is also
    // the ordinary case: a reload goes back through `open_for_reading`, which
    // says "follow this" about the document it has just reopened. Remaking the
    // watch would lose whatever landed in the gap, and — the part that bites —
    // the baseline would be retaken from what is on the disk *now* rather than
    // from the version just handed over, so a draft that arrived during the
    // reload would match its own mark and never be reported at all.
    if let (Some(current), Some(next)) = (held.get(&window), next.as_ref()) {
        if &current.path == next {
            return;
        }
    }

    if let Some(old) = held.remove(&window) {
        // The themes directory is watched for its own sake, and must not be
        // dropped along with a document that happened to be sitting in it —
        // and nor must a folder another window is still reading a document
        // out of. A watch is on a directory, so it is shared, and `unwatch`
        // takes it away from everybody.
        let wanted = old.dir == themes || held.values().any(|other| other.dir == old.dir);
        if !wanted {
            let _ = watcher.unwatch(&old.dir);
        }
    }
    let Some(path) = next else { return };
    let Some(dir) = path.parent().map(Path::to_path_buf) else {
        return;
    };
    let already = dir == themes || held.values().any(|other| other.dir == dir);
    if !already && watcher.watch(&dir, RecursiveMode::NonRecursive).is_err() {
        return;
    }
    // What it looks like now is the baseline: opening a document is not a
    // reason to reload it.
    held.insert(
        window,
        Followed {
            mark: whole(&path),
            real: real(&path),
            path,
            dir,
        },
    );
}

/// A document's path as the file system reports it: the real directory, and
/// the name in it.
///
/// FSEvents names what changed by its real path, and the path a reader opened
/// may run through a link — `/tmp`, a linked project folder, a `~/Documents` a
/// sync tool replaced — so an event compared against the opened path alone
/// never matched and the paper never reloaded. The opened path is still what
/// the window is told, because that is the one it knows its document by.
// ponytail: a document that is *itself* a link into another folder is still
// not followed; watch the target's directory too if anyone reads that way.
fn real(path: &Path) -> PathBuf {
    match (path.parent().map(std::fs::canonicalize), path.file_name()) {
        (Some(Ok(dir)), Some(name)) => dir.join(name),
        _ => path.to_path_buf(),
    }
}

/// A document's size and time, if what is on the disk is a whole PDF.
///
/// Cheap on purpose — five bytes at the front, a kilobyte at the back, and one
/// look at the size again after a pause. None of it proves the document is
/// readable; all of it rules out the case that actually happens, which is
/// catching a compiler halfway through writing one.
fn whole(path: &Path) -> Option<Mark> {
    let (length, modified) = identity(path)?;
    let mut file = File::open(path).ok()?;

    let mut head = [0u8; 5];
    file.read_exact(&mut head).ok()?;
    if &head != b"%PDF-" {
        return None;
    }

    file.seek(SeekFrom::Start(length.saturating_sub(TAIL)))
        .ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    if !tail.windows(5).any(|window| window == b"%%EOF") {
        return None;
    }

    // Still being written to? Then this is a moment in the middle of a run
    // that happens to end in `%%EOF`, and the next burst will ask again.
    std::thread::sleep(STEADY);
    if identity(path)? != (length, modified) {
        return None;
    }
    Some((length, modified, fingerprint(&tail)))
}

/// FNV-1a over the bytes already read. Not a checksum of the document — it
/// only has to tell two trailers apart, and it must not cost a read of its
/// own.
fn fingerprint(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn identity(path: &Path) -> Option<(u64, SystemTime)> {
    let data = std::fs::metadata(path).ok()?;
    let length = data.len();
    if length == 0 {
        return None;
    }
    Some((length, data.modified().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gate that stands between the reader and a compiler's half-written
    /// output. Everything else here is the file system's word for it; this is
    /// the one judgement the module makes on its own.
    fn scratch(name: &str, body: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("moonowl-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch directory");
        let path = dir.join(name);
        std::fs::write(&path, body).expect("scratch file");
        path
    }

    #[test]
    fn a_finished_document_is_whole() {
        let path = scratch("done.pdf", b"%PDF-1.7\n... objects ...\ntrailer\n%%EOF\n");
        assert!(whole(&path).is_some());
    }

    #[test]
    fn a_document_still_being_written_is_not() {
        // What pdflatex leaves on the disk in the middle of a run: the header
        // is there from the first write, and the marker that ends a PDF is the
        // last thing to arrive.
        let path = scratch("running.pdf", b"%PDF-1.7\n... objects ...\n");
        assert!(whole(&path).is_none());
    }

    #[test]
    fn an_empty_file_is_not() {
        let path = scratch("empty.pdf", b"");
        assert!(whole(&path).is_none());
    }

    #[test]
    fn something_that_is_not_a_pdf_is_not() {
        let path = scratch("notes.txt", b"%%EOF is in here but this is not a document");
        assert!(whole(&path).is_none());
    }

    /// A long document must not be judged by its first kilobyte: the marker is
    /// at the end, and only the end is read.
    #[test]
    fn the_marker_is_looked_for_at_the_end() {
        let mut body = b"%PDF-1.7\n".to_vec();
        body.extend(std::iter::repeat_n(b'x', 64 * 1024));
        body.extend_from_slice(b"\n%%EOF\n");
        let path = scratch("long.pdf", &body);
        assert!(whole(&path).is_some());
    }

    /// The case a coarse modification time hides: a recompile that came out
    /// the same length, inside the same tick. Length and time alone said
    /// nothing had happened; the trailer says otherwise, and the trailer is
    /// already in hand.
    #[test]
    fn a_rewrite_of_the_same_length_is_still_a_rewrite() {
        let path = scratch("redraft.pdf", b"%PDF-1.7\ntrailer\n0000000042\n%%EOF\n");
        let first = whole(&path).expect("whole");

        std::fs::write(&path, b"%PDF-1.7\ntrailer\n0000000317\n%%EOF\n").expect("rewrite");
        let second = whole(&path).expect("whole again");

        assert_eq!(
            first.0, second.0,
            "the test needs both drafts the same length"
        );
        assert_ne!(first, second, "a new draft went unnoticed");
    }

    /// And the other half of it: reading the same file twice must not look
    /// like a change, or every burst would reload the document.
    #[test]
    fn the_same_document_marks_the_same_way_twice() {
        let path = scratch("steady.pdf", b"%PDF-1.7\ntrailer\n%%EOF\n");
        assert_eq!(whole(&path), whole(&path));
    }

    /// A watcher that does nothing but say what it was asked to do, so that
    /// `follow` can be checked without a file system underneath it.
    #[derive(Default)]
    struct Recorded {
        watched: Vec<PathBuf>,
        unwatched: Vec<PathBuf>,
    }

    impl Watcher for Recorded {
        fn new<F: notify::EventHandler>(_: F, _: notify::Config) -> notify::Result<Self> {
            unreachable!("built directly, never through the trait")
        }

        fn watch(&mut self, path: &Path, _: RecursiveMode) -> notify::Result<()> {
            self.watched.push(path.to_path_buf());
            Ok(())
        }

        fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
            self.unwatched.push(path.to_path_buf());
            Ok(())
        }

        fn kind() -> notify::WatcherKind {
            notify::WatcherKind::NullWatcher
        }
    }

    /// Every reload says "follow this" about the document it has just reopened,
    /// and that must change nothing. Remaking the watch would lose whatever
    /// landed in the gap, and retaking the baseline would swallow a draft that
    /// arrived during the reload — the next burst would find the file matching
    /// its own mark and report nothing.
    #[test]
    fn reopening_the_same_document_leaves_the_watch_alone() {
        let path = scratch("reopened.pdf", b"%PDF-1.7\ntrailer\n%%EOF\n");
        let dir = path.parent().expect("a parent").to_path_buf();
        let themes = dir.join("themes");
        let mut watcher = Recorded::default();
        let mut held = HashMap::new();

        follow(&mut watcher, &themes, &mut held, one(), Some(path.clone()));
        let first = held.get("main").expect("followed").mark;
        assert_eq!(watcher.watched, vec![dir.clone()]);

        // A new draft lands, and the reload it causes comes back through here.
        std::fs::write(&path, b"%PDF-1.7\nrather longer than it was\n%%EOF\n").expect("rewrite");
        follow(&mut watcher, &themes, &mut held, one(), Some(path.clone()));

        assert!(watcher.unwatched.is_empty(), "the watch was remade");
        assert_eq!(watcher.watched, vec![dir]);
        assert_eq!(
            held.get("main").expect("still followed").mark,
            first,
            "the baseline moved"
        );
    }

    fn one() -> String {
        "main".to_string()
    }

    /// The write door's whole reason for touching this module: a window that
    /// wrote its own document must not be told its document changed. `absorb`
    /// is what `write_document` calls right after the write returns, and
    /// `changed` is the comparison the real burst runs into afterwards.
    #[test]
    fn a_windows_own_write_is_absorbed_before_it_is_reported() {
        let path = scratch("own-write.pdf", b"%PDF-1.7\ntrailer\n%%EOF\n");
        let dir = path.parent().expect("a parent").to_path_buf();
        let mut held = HashMap::new();
        held.insert(
            one(),
            Followed {
                mark: whole(&path),
                real: path.clone(),
                path: path.clone(),
                dir,
            },
        );

        // The write happens; the mark on disk moves.
        std::fs::write(&path, b"%PDF-1.7\ntrailer\nnow rather longer\n%%EOF\n").expect("rewrite");
        absorb(&mut held, "main", &path);

        // The burst the write itself caused arrives and finds nothing new.
        let followed = held.get_mut("main").expect("followed");
        assert!(
            !changed(followed, whole(&path)),
            "an absorbed write was still reported"
        );
    }

    /// Without `absorb`, the same rewrite is exactly the case `changed` exists
    /// to catch — this is the control the test above is a contrast to.
    #[test]
    fn an_unabsorbed_write_is_still_reported() {
        let path = scratch("other-write.pdf", b"%PDF-1.7\ntrailer\n%%EOF\n");
        let dir = path.parent().expect("a parent").to_path_buf();
        let mut followed = Followed {
            mark: whole(&path),
            real: path.clone(),
            path: path.clone(),
            dir,
        };

        std::fs::write(&path, b"%PDF-1.7\ntrailer\nnow rather longer\n%%EOF\n").expect("rewrite");
        assert!(
            changed(&mut followed, whole(&path)),
            "a real change went unreported"
        );
    }

    /// Two windows, one document: only the window that actually wrote it has
    /// its baseline moved. The other window's copy is genuinely stale and
    /// must still hear about it.
    #[test]
    fn absorbing_one_windows_write_does_not_silence_the_other() {
        let path = scratch("shared-write.pdf", b"%PDF-1.7\ntrailer\n%%EOF\n");
        let dir = path.parent().expect("a parent").to_path_buf();
        let baseline = whole(&path);
        let mut held = HashMap::new();
        held.insert(
            "main".to_string(),
            Followed {
                mark: baseline,
                real: path.clone(),
                path: path.clone(),
                dir: dir.clone(),
            },
        );
        held.insert(
            "reader-1".to_string(),
            Followed {
                mark: baseline,
                real: path.clone(),
                path: path.clone(),
                dir,
            },
        );

        std::fs::write(&path, b"%PDF-1.7\ntrailer\nnow rather longer\n%%EOF\n").expect("rewrite");
        absorb(&mut held, "main", &path);

        let disk = whole(&path);
        assert!(!changed(held.get_mut("main").expect("main"), disk));
        assert!(changed(held.get_mut("reader-1").expect("reader-1"), disk));
    }

    /// `absorb` only ever moves the baseline for the window and the path it
    /// names — a stray signal for a window that has since moved on to a
    /// different document must not clobber that document's own baseline.
    #[test]
    fn absorb_does_nothing_for_a_window_reading_something_else() {
        let written = scratch("absorbed.pdf", b"%PDF-1.7\ntrailer\n%%EOF\n");
        let current = scratch("current.pdf", b"%PDF-1.7\nsomething else\n%%EOF\n");
        let dir = current.parent().expect("a parent").to_path_buf();
        let baseline = whole(&current);
        let mut held = HashMap::new();
        held.insert(
            "main".to_string(),
            Followed {
                mark: baseline,
                real: current.clone(),
                path: current,
                dir,
            },
        );

        absorb(&mut held, "main", &written);

        assert_eq!(held.get("main").expect("still followed").mark, baseline);
    }

    /// A document opened through a link is followed by both of its names: the
    /// one the window knows and the one the file system reports.
    #[cfg(unix)]
    #[test]
    fn a_document_behind_a_link_is_followed_by_its_real_path() {
        let path = scratch("linked.pdf", b"%PDF-1.7\n%%EOF\n");
        let dir = std::fs::canonicalize(path.parent().expect("a parent")).expect("real");
        let link = dir.with_file_name(format!("moonowl-link-{}", std::process::id()));
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&dir, &link).expect("a link");

        let named = link.join("linked.pdf");
        let mut held = HashMap::new();
        follow(
            &mut Recorded::default(),
            &dir.join("themes"),
            &mut held,
            one(),
            Some(named.clone()),
        );

        let followed = held.get("main").expect("followed");
        assert_eq!(followed.path, named);
        assert_eq!(followed.real, dir.join("linked.pdf"));
        let _ = std::fs::remove_file(&link);
    }

    /// A different document, though, is a different subject.
    #[test]
    fn another_document_moves_the_watch() {
        let first = scratch("first.pdf", b"%PDF-1.7\n%%EOF\n");
        let dir = first.parent().expect("a parent").to_path_buf();
        let elsewhere = dir.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).expect("second directory");
        let second = elsewhere.join("second.pdf");
        std::fs::write(&second, b"%PDF-1.7\n%%EOF\n").expect("second document");

        let themes = dir.join("themes");
        let mut watcher = Recorded::default();
        let mut held = HashMap::new();

        follow(&mut watcher, &themes, &mut held, one(), Some(first));
        follow(
            &mut watcher,
            &themes,
            &mut held,
            one(),
            Some(second.clone()),
        );

        assert_eq!(watcher.unwatched, vec![dir.clone()]);
        assert_eq!(watcher.watched, vec![dir, elsewhere]);
        assert_eq!(held.get("main").expect("followed").path, second);
    }

    /// Two windows, two documents, one folder. The watch is on the folder, so
    /// it is shared — and the second window putting its document down must not
    /// take the first window's watch with it.
    #[test]
    fn a_second_window_reading_the_same_folder_keeps_the_watch() {
        let first = scratch("shared-one.pdf", b"%PDF-1.7\n%%EOF\n");
        let dir = first.parent().expect("a parent").to_path_buf();
        let second = dir.join("shared-two.pdf");
        std::fs::write(&second, b"%PDF-1.7\n%%EOF\n").expect("second document");
        let themes = dir.join("themes");

        let mut watcher = Recorded::default();
        let mut held = HashMap::new();

        follow(&mut watcher, &themes, &mut held, one(), Some(first.clone()));
        follow(
            &mut watcher,
            &themes,
            &mut held,
            "reader-1".to_string(),
            Some(second),
        );
        // Watched once, by the window that got there first.
        assert_eq!(watcher.watched, vec![dir.clone()]);

        // The second window closes its document.
        follow(
            &mut watcher,
            &themes,
            &mut held,
            "reader-1".to_string(),
            None,
        );
        assert!(
            watcher.unwatched.is_empty(),
            "the first window's document stopped being watched"
        );
        assert_eq!(held.get("main").expect("still followed").path, first);

        // And when the last of them lets go, the folder does come off.
        follow(&mut watcher, &themes, &mut held, one(), None);
        assert_eq!(watcher.unwatched, vec![dir]);
        assert!(held.is_empty());
    }
}
