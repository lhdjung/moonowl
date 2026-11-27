//! `moonowl ocr` with several files, run as the program itself, since each
//! file is recognised by a copy of it.

/// Whether this machine can recognise text: Vision on macOS, Tesseract
/// elsewhere, which CI does not install.
fn engine() -> bool {
    cfg!(target_os = "macos")
        || std::process::Command::new("tesseract")
            .arg("--version")
            .output()
            .is_ok()
}

/// **Every file gets its searchable copy beside it**, and none is written
/// over when the command is run again.
#[test]
fn several_files_are_recognised_at_once() {
    if !engine() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("moonowl-cli-ocr-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let scan = moonowl_core::fixture::scanned_pdf();
    let files: Vec<String> = ["a", "b"]
        .iter()
        .map(|name| {
            let to = dir.join(format!("{name}.pdf"));
            std::fs::copy(&scan, &to).unwrap();
            to.to_string_lossy().into_owned()
        })
        .collect();
    let run = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_moonowl-cli"))
            .arg("ocr")
            .args(&files)
            .output()
            .unwrap()
    };
    let ran = run();
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    for name in ["a", "b"] {
        let copy = dir.join(format!("{name}-searchable.pdf"));
        let pages = moonowl_core::render::open(copy.to_str().unwrap())
            .unwrap()
            .pages();
        assert_eq!(pages, 6);
    }
    let again = run();
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("already exists"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// **A folder means every PDF in it**, leaving out copies an earlier run
/// made; `--into` puts the copies elsewhere, and `--to` names the copy of
/// one file only.
#[test]
fn a_folder_and_the_names_of_the_copies() {
    if !engine() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("moonowl-cli-folder-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (scans, copies) = (dir.join("scans"), dir.join("copies"));
    std::fs::create_dir_all(&scans).unwrap();
    let scan = moonowl_core::fixture::scanned_pdf();
    for name in ["a.pdf", "b.PDF", "old-searchable.pdf"] {
        std::fs::copy(&scan, scans.join(name)).unwrap();
    }
    std::fs::write(scans.join("notes.txt"), "not a PDF").unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_moonowl-cli"))
            .arg("ocr")
            .args(args)
            .output()
            .unwrap()
    };
    let ran = run(&[scans.to_str().unwrap(), "--into", copies.to_str().unwrap()]);
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let mut made: Vec<String> = std::fs::read_dir(&copies)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    made.sort();
    assert_eq!(made, ["a-searchable.pdf", "b-searchable.pdf"]);

    let named = dir.join("named.pdf");
    let a = scans.join("a.pdf");
    let ran = run(&[a.to_str().unwrap(), "--to", named.to_str().unwrap()]);
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(named.exists());

    // Run again with a new scan in the folder: only the new one is read.
    std::fs::copy(&scan, scans.join("c.pdf")).unwrap();
    let skipping = run(&[
        scans.to_str().unwrap(),
        "--into",
        copies.to_str().unwrap(),
        "--skip-existing",
    ]);
    assert!(
        skipping.status.success(),
        "{}",
        String::from_utf8_lossy(&skipping.stderr)
    );
    let said = String::from_utf8_lossy(&skipping.stdout);
    assert!(said.contains("c-searchable.pdf"), "{said}");
    assert!(said.contains("Skipped 2 files"), "{said}");
    let again = run(&[scans.to_str().unwrap(), "--into", copies.to_str().unwrap()]);
    assert!(
        !again.status.success(),
        "without it, a copy there is refused"
    );

    let two = run(&[scans.to_str().unwrap(), "--to", "x.pdf"]);
    assert!(!two.status.success());
    assert!(String::from_utf8_lossy(&two.stderr).contains("--into"));
    let _ = std::fs::remove_dir_all(&dir);
}
