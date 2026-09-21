//! Where the app's own files live, and the one way anything writes to them.
//!
//! This is the part of `src-tauri/src/lib.rs` that `theme.rs` and
//! `settings.rs` actually need — `atomic_write` and a config directory — with
//! Tauri's path resolver replaced by the platform conventions it was resolving
//! to. Nothing else of those 2,431 lines is required to make the two modules
//! beside this one compile and run, which is the assessment's claim about them
//! demonstrated rather than restated.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Write by writing something else and renaming it over the top.
///
/// Copied out of `src-tauri/src/lib.rs` unchanged. A plain `fs::write`
/// truncates and then fills, so there is a moment when the file on disk is a
/// settings file with no settings in it — and this directory is watched, and
/// read by anything the reader has open beside the app.
pub fn atomic_write(target: &Path, body: &[u8]) -> Result<(), String> {
    replace(target, body, false)
}

/// The same, over a file that is *somebody's*: the new file takes the old
/// one's permissions and, on macOS, its ACL and extended attributes.
///
/// A rename puts a new inode under the name, and a new inode has none of what
/// the reader hung on the old one — so the first highlight took a document's
/// Finder tags, its comment, its "where from" and any permissions it had been
/// given. None of that matters for a settings file; all of it does for a
/// paper. Hard links are still parted, which is what a rename is.
pub fn atomic_write_keeping(target: &Path, body: &[u8]) -> Result<(), String> {
    replace(target, body, true)
}

fn replace(target: &Path, body: &[u8], keeping: bool) -> Result<(), String> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let dir = target
        .parent()
        .ok_or("That path has no folder to write into.")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;

    let stem = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let ticket = COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = dir.join(format!(".{stem}.{}.{ticket}.tmp", std::process::id()));

    if let Err(e) = std::fs::write(&temp, body) {
        // A full disk leaves part of one behind, and it is ours.
        let _ = std::fs::remove_file(&temp);
        return Err(e.to_string());
    }
    if keeping {
        dress(target, &temp);
    }
    std::fs::rename(&temp, target).map_err(|e| {
        // A failed rename leaves the staging file behind; it is ours and
        // nobody else's, so cleaning it up cannot take anything with it.
        let _ = std::fs::remove_file(&temp);
        e.to_string()
    })
}

/// Put what `from` wears onto `to`. Best effort: a document with its tags
/// lost is still better than a highlight refused.
// ponytail: extended attributes are carried on macOS only, where the Finder
// keeps things in them; Linux wants the `xattr` crate if anybody misses theirs.
fn dress(from: &Path, to: &Path) {
    if let Ok(data) = std::fs::metadata(from) {
        let _ = std::fs::set_permissions(to, data.permissions());
    }
    #[cfg(target_os = "macos")]
    {
        use std::ffi::{c_char, c_int, c_void, CString};
        use std::os::unix::ffi::OsStrExt;

        extern "C" {
            fn copyfile(
                from: *const c_char,
                to: *const c_char,
                state: *mut c_void,
                flags: u32,
            ) -> c_int;
        }
        // The ACL and the attributes, and not `COPYFILE_STAT`: that one
        // carries the modification time across, and a document that was just
        // written has to say so.
        const COPYFILE_ACL: u32 = 1 << 0;
        const COPYFILE_XATTR: u32 = 1 << 2;

        let name = |path: &Path| CString::new(path.as_os_str().as_bytes());
        if let (Ok(from), Ok(to)) = (name(from), name(to)) {
            // SAFETY: two NUL-terminated paths that outlive the call, and a
            // null state, which is what copyfile(3) takes for a one-off copy.
            unsafe {
                copyfile(
                    from.as_ptr(),
                    to.as_ptr(),
                    std::ptr::null_mut(),
                    COPYFILE_ACL | COPYFILE_XATTR,
                );
            }
        }
    }
}

/// A document's path the way the rest of the app keys it: absolute, so that
/// `paper.pdf` from a terminal and the same file from the Finder are one
/// document to the library, the watch and the desk. Every door a path comes
/// in by calls this once; nothing downstream does.
pub fn absolute(path: &str) -> String {
    std::path::absolute(path)
        .map(|whole| whole.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

/// The directory the settings file and the themes directory live in, named
/// for the bundle identifier.
///
/// `MOONOWL_CONFIG` overrides it, which is what the tests use and what makes
/// a run reproducible.
pub fn config_dir() -> PathBuf {
    match std::env::var_os("MOONOWL_CONFIG") {
        Some(stated) => PathBuf::from(stated),
        None => base().join("app.moonowl"),
    }
}

/// The themes directory inside it, which is what `theme::load_all` reads.
pub fn themes_dir() -> PathBuf {
    config_dir().join("themes")
}

/// Where an application's own files go on this platform. Tauri's
/// `app_config_dir()` resolves to the same three answers; there is no reason
/// to take a dependency to get them.
fn base() -> PathBuf {
    let home = || {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
    };
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join("AppData/Roaming"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".config"))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A document written over keeps what its reader hung on it.
    #[test]
    fn a_document_written_over_keeps_what_it_wore() {
        let path = std::env::temp_dir().join(format!("moonowl-keeping-{}.pdf", std::process::id()));
        std::fs::write(&path, b"before").expect("a document");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("a mode");
        #[cfg(target_os = "macos")]
        let tagged = std::process::Command::new("xattr")
            .args(["-w", "com.moonowl.test", "kept"])
            .arg(&path)
            .status()
            .is_ok_and(|status| status.success());

        atomic_write_keeping(&path, b"after").expect("written");

        assert_eq!(std::fs::read(&path).expect("read"), b"after");
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o640);
        #[cfg(target_os = "macos")]
        if tagged {
            let out = std::process::Command::new("xattr")
                .args(["-p", "com.moonowl.test"])
                .arg(&path)
                .output()
                .expect("xattr");
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "kept");
        }
        let _ = std::fs::remove_file(&path);
    }
}
