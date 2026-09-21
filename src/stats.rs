//! What a reading session costs, counted where it happens.
//!
//! The app's own memory table was made by running the real thing and watching
//! it from outside. That works and it says nothing about *which* of the three
//! places that hold a page is holding it, which is the fault that let the
//! thumbnail column leak for as long as it did. So the counters live in the
//! code that allocates, and `tests/cost.rs` reads them beside the process's
//! own footprint.

use std::sync::atomic::{AtomicU64, Ordering};

/// Pages drawn by the renderer, which should settle at one per mounted page.
pub static DRAWN: AtomicU64 = AtomicU64::new(0);
/// Every render of the reader's own component.
///
/// The number to look at is whether it *stops*. A component that dirties
/// itself as it renders costs a full style, layout and paint pass per frame
/// for as long as the window is open — 100% of a core with nobody touching
/// the app, and nothing on screen to say so. `tests/cost.rs` asserts it
/// settles; see the note there.
pub static RENDERS: AtomicU64 = AtomicU64::new(0);
/// Writes of a document still on their thread — see `Viewer::write`. What
/// the harness waits on before it looks, and what `main` waits on before it
/// goes, so that a quit does not cut a highlight off half way.
pub static WRITING: AtomicU64 = AtomicU64::new(0);
/// Bytes of texture alive on the GPU, source and painted copies both.
pub static RESIDENT: AtomicU64 = AtomicU64::new(0);
/// Pages in the document right now — the mounting window, observed rather
/// than asserted.
pub static MOUNTED: AtomicU64 = AtomicU64::new(0);
/// Pages of *text* held for the sake of a selection — see `Viewer::texts`.
///
/// The newest count rather than a total, so it is a level and not a tally:
/// what it says is how full that cache was the last time anything filled it,
/// and the only thing worth asserting about it is that it stays under its cap.
/// A page of text is a hundred kilobytes, which is small next to a texture and
/// large next to nothing — it is here because an unbounded cache of them was
/// the obvious way to write it and would have cost 40MB on a book read end to
/// end.
pub static TEXT_PAGES: AtomicU64 = AtomicU64::new(0);

pub fn add(counter: &AtomicU64, by: u64) {
    counter.fetch_add(by, Ordering::Relaxed);
}

pub fn sub(counter: &AtomicU64, by: u64) {
    counter.fetch_sub(by.min(counter.load(Ordering::Relaxed)), Ordering::Relaxed);
}

/// A level rather than a tally: the counter is *told* what it is now. See
/// [`TEXT_PAGES`], which is the one counter here that is not cumulative.
pub fn set(counter: &AtomicU64, to: u64) {
    counter.store(to, Ordering::Relaxed);
}

pub fn get(counter: &AtomicU64) -> u64 {
    counter.load(Ordering::Relaxed)
}

/// This process's physical footprint and its peak, in megabytes.
///
/// The number GPU allocations actually land in. `vmmap --summary` is the only
/// thing that reports it without linking against mach; it costs a few hundred
/// milliseconds, so it is read where a session is summarised and never in a
/// frame.
#[cfg(target_os = "macos")]
pub fn footprint_mb() -> (f64, f64) {
    let pid = std::process::id().to_string();
    let Ok(out) = std::process::Command::new("vmmap")
        .args(["--summary", &pid])
        .output()
    else {
        return (0.0, 0.0);
    };
    let (mut settled, mut peak) = (0.0, 0.0);
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Some((label, value)) = line.split_once(':') else {
            continue;
        };
        // "227.3M", "1.8G", "996K" — vmmap's own spelling.
        let text = value.trim();
        let (number, scale) = match text.chars().last() {
            Some('K') => (&text[..text.len() - 1], 1.0 / 1024.0),
            Some('M') => (&text[..text.len() - 1], 1.0),
            Some('G') => (&text[..text.len() - 1], 1024.0),
            _ => (text, 1.0 / (1024.0 * 1024.0)),
        };
        let parsed = number.trim().parse::<f64>().unwrap_or(0.0) * scale;
        match label.trim() {
            "Physical footprint" => settled = parsed,
            "Physical footprint (peak)" => peak = parsed,
            _ => {}
        }
    }
    (settled, peak)
}

/// The same question on Linux, answered out of `/proc/self/status`.
///
/// **`VmRSS` here, when "never RSS" is the rule this file exists to state.**
/// The rule is about a Mac and about the GPU: a `wgpu` allocation is charged
/// to the physical footprint and only partly to the resident size, which is
/// how Phase 1 measured `vello` and `vello_hybrid` at 3% apart when they are
/// eleven times apart. Neither half of that holds here. Linux has no separate
/// footprint counter to prefer — `VmRSS` and its high-water mark `VmHWM` are
/// what the kernel offers — and the one caller of this is the memory test,
/// which runs the whole reader down the **CPU** path: a page is an `ImageData`
/// on the heap, there is no device and no driver, and everything the process
/// holds is resident by construction. So on the platform this is read on, RSS
/// is the honest number rather than the misleading one.
///
/// What that buys is the reason to write it at all: `tests/cost.rs` was a
/// growth bound that only bound anything on a Mac, and CI runs on Linux. A
/// leak of the shape that cost 96MB and went unnoticed through the whole of
/// Phase 1 is now caught by the machine that runs on every push.
#[cfg(target_os = "linux")]
pub fn footprint_mb() -> (f64, f64) {
    match std::fs::read_to_string("/proc/self/status") {
        Ok(status) => read_status(&status),
        Err(_) => (0.0, 0.0),
    }
}

/// `VmRSS` and `VmHWM` out of the text `/proc/self/status` holds, in
/// megabytes.
///
/// A function of a string rather than of the file, and compiled on every
/// platform under `cfg(test)`, because the machine this was written on cannot
/// run it: a cross-check for Linux from a Mac stops at `fontconfig`, whose
/// build script wants pkg-config and a sysroot, and whose one documented
/// escape (`RUST_FONTCONFIG_DLOPEN`) changes the crate's API enough that
/// `fontique` no longer compiles against it. So the parsing is tested here and
/// the reading is three lines above.
#[cfg(any(target_os = "linux", test))]
fn read_status(status: &str) -> (f64, f64) {
    let (mut settled, mut peak) = (0.0, 0.0);
    for line in status.lines() {
        let Some((label, value)) = line.split_once(':') else {
            continue;
        };
        // "VmRSS:\t  123456 kB" — always kilobytes, whatever unit the line
        // states, which is the kernel's own promise about this file.
        let kb = value
            .trim()
            .trim_end_matches("kB")
            .trim()
            .parse::<f64>()
            .unwrap_or(0.0);
        match label {
            "VmRSS" => settled = kb / 1024.0,
            "VmHWM" => peak = kb / 1024.0,
            _ => {}
        }
    }
    (settled, peak)
}

/// And nothing on Windows, which is the one platform with neither a `vmmap`
/// nor a `/proc`: `GetProcessMemoryInfo` is the answer and it means linking
/// against `windows-sys` for one number. `tests/cost.rs` says what it does
/// with a zero — the counters still stand, and the growth bound is skipped.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn footprint_mb() -> (f64, f64) {
    (0.0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::read_status;

    /// A real `/proc/self/status`, cut down to the lines that are read and the
    /// two either side of them — including `VmRSS`'s neighbours, which are
    /// spelled the same way and must not be picked up instead.
    #[test]
    fn the_two_lines_that_matter_are_read_out_of_a_proc_status() {
        let status = "Name:\tdioxus-reader\n\
             VmPeak:\t 2530812 kB\n\
             VmSize:\t 2530812 kB\n\
             VmHWM:\t  392704 kB\n\
             VmRSS:\t  147456 kB\n\
             RssAnon:\t  120000 kB\n\
             Threads:\t8\n";
        let (settled, peak) = read_status(status);
        assert_eq!(settled, 144.0);
        assert_eq!(peak, 383.5);
    }

    /// And a file with nothing in it worth reading is zero rather than a
    /// panic, which is the answer `tests/cost.rs` knows how to treat as "this
    /// platform does not say".
    #[test]
    fn a_status_without_those_lines_is_no_answer_rather_than_a_wrong_one() {
        assert_eq!(read_status("Name:\tdioxus-reader\n"), (0.0, 0.0));
        assert_eq!(read_status(""), (0.0, 0.0));
    }
}
