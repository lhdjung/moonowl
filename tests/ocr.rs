//! `moonowl ocr` with several files, run as the program itself, since each
//! file is recognised by a copy of it.

/// **Every file gets its searchable copy beside it**, and none is written
/// over when the command is run again.
#[test]
fn several_files_are_recognised_at_once() {
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
        std::process::Command::new(env!("CARGO_BIN_EXE_moonowl"))
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
    assert!(String::from_utf8_lossy(&again.stderr).contains("is already there"));
    let _ = std::fs::remove_dir_all(&dir);
}
