//! One Moonowl at a time, and how a second launch hands its document over.
//!
//! Three double-clicked documents mean three launches, and three processes
//! writing over each other's `settings.toml` is a race no lock inside one of
//! them can help with. This reader keeps a library, a themes directory and a
//! settings table on disk, and holds a process-wide lock around pdfium.
//!
//! This is a Unix socket with a lock file beside it: **holding the lock is the
//! claim, and connecting to the socket is how the document gets across**. The
//! socket cannot do both jobs when two launches happen in one instant — see
//! [`claim`].
//!
//! **Unix only, deliberately.** Windows wants a named pipe and there is no
//! std type for one, so a second launch there is a second process.
//!
//! **What this cannot reach is Apple Events**, which is how macOS tells an
//! application that is *already running* to open a document — and that is
//! `openfiles.rs`'s job. It ends in the same place: `Remote::request`, the door this serves.
//! Every other route a document arrives by — a second `cargo run -- paper.pdf`,
//! a cold launch from the Finder, "Open with" on Linux — is a launch with an
//! argument, and a launch with an argument comes through here.

use std::path::{Path, PathBuf};

/// What a launch found.
pub enum Claim {
    /// This process is the one. Hold the listener and serve it.
    #[cfg(unix)]
    First(std::os::unix::net::UnixListener),
    /// Somebody else is; the document has been handed to them.
    Second,
    /// Nothing could be claimed and nothing is being served — the platform
    /// has no socket, or the socket could not be made. A second reader is
    /// worse than one and better than none.
    Alone,
}

fn socket_path(dir: &Path) -> PathBuf {
    dir.join("instance.sock")
}

/// Claim this process as the only one, or hand `path` to whoever already has.
///
/// `Second` means the caller should exit, quietly and successfully: the
/// document is on its way to a window that already exists, which is what the
/// reader asked for.
///
/// **The claim is a lock on a file beside the socket, and the socket is only
/// the door.** It was the socket alone — connect, and on failure remove the
/// file and bind — and three documents double-clicked at once are three
/// launches in the same instant: all fail to connect, one binds, and the next
/// one's `remove_file` takes the live socket away and binds its own. Two
/// readers, one of them unreachable for ever. A socket file cannot be told
/// from a corpse without removing it, and a lock can: the kernel drops it with
/// the process, however the process went.
#[cfg(unix)]
pub fn claim(dir: &Path, path: Option<&str>) -> Claim {
    use std::os::unix::net::UnixListener;

    let socket = socket_path(dir);
    let _ = std::fs::create_dir_all(dir);
    let Ok(lock) = std::fs::File::create(dir.join("instance.lock")) else {
        return Claim::Alone;
    };
    // **Asked again until one of the two answers**, for as long as a quit
    // can take: a holder on its way out still has the lock and no longer
    // takes documents — see [`closing`] — and giving up on it after one try
    // would run this launch alone, beside the next one to take the lock. Its door
    // answers with silence at once, so the rounds are paced by the clock,
    // not by the door.
    let until = std::time::Instant::now() + QUIT_LASTS;
    let first = |lock: std::fs::File| {
        // Ours, so whatever socket file is there belongs to nobody alive.
        let _ = std::fs::remove_file(&socket);
        match UnixListener::bind(&socket) {
            Ok(listener) => {
                // Held for as long as the process lives, which is what not
                // closing it means.
                std::mem::forget(lock);
                Claim::First(listener)
            }
            Err(_) => Claim::Alone,
        }
    };
    loop {
        match lock.try_lock() {
            Ok(()) => return first(lock),
            Err(std::fs::TryLockError::WouldBlock) => {}
            // A file system that cannot lock answers neither way, so the
            // door is asked once, if there is one, and a launch nobody
            // answers is the first.
            Err(std::fs::TryLockError::Error(_)) => {
                return if socket.exists() && handed_to(&socket, path) {
                    Claim::Second
                } else {
                    first(lock)
                };
            }
        }
        if handed_to(&socket, path) {
            return Claim::Second;
        }
        if std::time::Instant::now() >= until {
            return Claim::Alone;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// How long a launch goes on asking lock and door: a quit waits on a
/// document being written.
#[cfg(unix)]
const QUIT_LASTS: std::time::Duration = std::time::Duration::from_secs(10);

/// Give the document to the process holding the claim, and hear it taken.
///
/// Tried for a while rather than once: the holder takes the lock and *then*
/// binds, and a launch in the same instant arrives between the two. **Handed
/// over is answered**: a holder that is quitting reads the path and says
/// nothing, and a launch that took silence for yes lost its document.
#[cfg(unix)]
fn handed_to(socket: &Path, path: Option<&str>) -> bool {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    for _ in 0..40 {
        if let Ok(mut stream) = UnixStream::connect(socket) {
            let _ = stream.write_all(path.unwrap_or("").as_bytes());
            let _ = stream.shutdown(std::net::Shutdown::Write);
            // Longer than the holder waits for its window, so that an
            // answer it sends arrives before this stops listening.
            let _ =
                stream.set_read_timeout(Some(ANSWER_WITHIN + std::time::Duration::from_secs(1)));
            let mut answer = String::new();
            let _ = stream.read_to_string(&mut answer);
            return answer == TAKEN;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}

/// How long the holder waits for its event loop to take a document.
#[cfg(unix)]
const ANSWER_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

/// What the holder answers once a document is on its way to a window.
#[cfg(unix)]
const TAKEN: &str = "taken";

/// Whether this process has begun to quit, after which a document handed to
/// it would go to an event loop that is not there any more.
static CLOSING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The app is going: documents arriving from now on are left for the next
/// launch to take. Called as the quit begins, before any window goes.
pub fn closing() {
    CLOSING.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Whether [`closing`] has been called.
pub fn is_closing() -> bool {
    CLOSING.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(not(unix))]
pub fn claim(_dir: &Path, _path: Option<&str>) -> Claim {
    Claim::Alone
}

/// Hand one more document to the running reader, after `claim` found it.
#[cfg(unix)]
pub fn hand(dir: &Path, path: &str) {
    handed_to(&socket_path(dir), Some(path));
}

#[cfg(not(unix))]
pub fn hand(_dir: &Path, _path: &str) {}

/// Answer the door for as long as the process lives.
///
/// A thread, because accepting blocks and the main thread is drawing. What
/// arrives is a path or nothing, and either way it goes to the shell as a
/// request for a window — `Session::hand_over` is what decides whether that
/// means a new window or bringing one forward.
#[cfg(unix)]
pub fn serve(listener: std::os::unix::net::UnixListener, shell: crate::shell::Remote) {
    use std::io::Read;

    use std::io::Write;

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            // One door and one thread: a caller that connects and never
            // closes would otherwise hold it shut for every launch after.
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut said = String::new();
            if (&mut stream)
                .take(64 * 1024)
                .read_to_string(&mut said)
                .is_err()
            {
                continue;
            }
            // Unanswered, so the launch asks the lock again and takes it
            // once this process has gone. See [`claim`].
            if is_closing() {
                continue;
            }
            // As sent, with no trim: a path may end in a space. Answered
            // only once the shell has asked for the window: a request still
            // queued when the last window closes goes with the event loop,
            // and the launch, hearing nothing, opens the document itself.
            //
            // A main thread too busy to ask in time finds the channel closed
            // and opens nothing, since the launch has gone on without it.
            let (taken, heard) = std::sync::mpsc::sync_channel(0);
            shell.hand((!said.is_empty()).then_some(said), taken);
            if heard.recv_timeout(ANSWER_WITHIN).is_ok() {
                let _ = stream.write_all(TAKEN.as_bytes());
            }
        }
    });
}

/// Take the socket away, so that the next launch is not left connecting to a
/// process that has gone.
///
/// A best effort and nothing more: a process that is killed outright leaves
/// the file behind, which is why the claim is the lock and not the socket.
/// `instance.lock` stays where it is — removing a lock file is a race of its
/// own, and an unlocked one claims nothing.
pub fn release(dir: &Path) {
    closing();
    let _ = std::fs::remove_file(socket_path(dir));
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Read;

    /// The second launch hands over even with a dead socket file in the way,
    /// and never takes the first one's socket.
    #[test]
    fn the_second_claim_hands_its_document_to_the_first() {
        // Short, because a socket path has about a hundred characters.
        let dir = PathBuf::from(format!("/tmp/moonowl-single-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        std::fs::write(socket_path(&dir), b"").expect("a corpse");

        let Claim::First(listener) = claim(&dir, None) else {
            panic!("the first launch did not claim");
        };
        let door = std::thread::spawn(move || {
            use std::io::Write;
            let mut said = String::new();
            let (mut stream, _) = listener.accept().expect("a caller");
            stream.read_to_string(&mut said).expect("a path");
            stream.write_all(TAKEN.as_bytes()).expect("an answer");
            said
        });
        assert!(matches!(claim(&dir, Some("/paper.pdf")), Claim::Second));
        assert_eq!(door.join().expect("the door"), "/paper.pdf");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
