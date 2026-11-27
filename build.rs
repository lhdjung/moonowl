//! On Windows, the app's icon and name are compiled into the executable.
//! The themes and palettes are checked by `core/build.rs`.

fn main() {
    // See `winresource` in `Cargo.toml`.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=icons/icon.ico");
        winresource::WindowsResource::new()
            .set_icon("icons/icon.ico")
            // What Windows calls the app — Search, Task Manager, "Open
            // with". Both default to the crate's name, which is lower case.
            .set("FileDescription", "Moonowl")
            .set("ProductName", "Moonowl")
            .compile()
            .expect("icons/icon.ico could not be compiled into the executable");
    }
}
