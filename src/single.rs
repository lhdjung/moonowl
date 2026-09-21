//! One Moonowl at a time, and how a second launch hands its document over.
//!
//! The app uses `tauri-plugin-single-instance`, and `AGENTS.md` says why it
//! has to have one: three double-clicked documents mean three launches, and
//! three processes writing over each other's `settings.toml` is a race no
//! lock inside one of them can help with. The same is true here and rather
//! more so — this reader keeps a library, a themes directory and a settings
//! table on disk, and holds a process-wide lock around pdfium.
//!
//! The assessment's table offers "`single-instance`, or a Unix socket / named
//! pipe". This is the socket, with a lock file beside it: **holding the lock is
//! the claim, and connecting to the socket is how the document gets across**.
//! The socket did both jobs until two launches in one instant showed why it
//! cannot — see [`claim`].
//!
//! **Unix only, deliberately.** Windows wants a named pipe and there is no
//! std type for one, so a second launch there is a second process — which is
//! the state this experiment has been in since Phase 0, and is honest about:
//! multi-window "has been shown to work on macOS and nowhere else". The
//! branch is where the port would go, not what it would look like.
//!
//! **What this cannot reach is Apple Events**, which is how macOS tells an
//! application that is *already running* to open a document — and that is
//! `openfiles.rs`'s job now that there is a bundle for the Finder to send one
//! to. It ends in the same place: `Remote::request`, the door this serves.
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
    if lock.try_lock().is_err() {
        return hand_to(&socket, path);
    }
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
}

/// Give the document to the process holding the claim.
///
/// Tried for a while rather than once: the holder takes the lock and *then*
/// binds, and a launch in the same instant arrives between the two.
#[cfg(unix)]
fn hand_to(socket: &Path, path: Option<&str>) -> Claim {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    for _ in 0..40 {
        if let Ok(mut stream) = UnixStream::connect(socket) {
            let _ = stream.write_all(path.unwrap_or("").as_bytes());
            return Claim::Second;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Claim::Alone
}

#[cfg(not(unix))]
pub fn claim(_dir: &Path, _path: Option<&str>) -> Claim {
    Claim::Alone
}

/// Answer the door for as long as the process lives.
///
/// A thread, because accepting blocks and the main thread is drawing. What
/// arrives is a path or nothing, and either way it goes to the shell as a
/// request for a window — `Session::hand_over` is what decides whether that
/// means a new window or bringing one forward.
#[cfg(unix)]
pub fn serve(listener: std::os::unix::net::UnixListener, shell: crate::shell::Remote) {
    use std::io::Read;

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut said = String::new();
            if stream.read_to_string(&mut said).is_err() {
                continue;
            }
            let said = said.trim();
            shell.request((!said.is_empty()).then(|| said.to_string()));
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
        assert!(matches!(claim(&dir, Some("/paper.pdf")), Claim::Second));

        let mut said = String::new();
        let (mut stream, _) = listener.accept().expect("a caller");
        stream.read_to_string(&mut said).expect("a path");
        assert_eq!(said, "/paper.pdf");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
