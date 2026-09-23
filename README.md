<p align="center"><img src="icons/128x128@2x.png" alt="" width="128"></p>

# Moonowl: free and user-friendly PDF reader

Calm and spacious reading experience. Tidy interface, rich color themes,
and flexible options. Respect for user experience and preferences.
No cluttered UI, no paywall. Written in Rust.

![example-light](./images/example-screenshot-light.png)
![example-dark](./images/example-screenshot-dark.png)

Moonowl is free and open-source forever, so it won't let you down
for basic features until you upgrade to some paid version. It was
designed by a user, for users – not by a company.

Features include:

- Choose between many color themes
- Create your own themes
- Rotate pages easily
- Hide toolbar for undistracted reading
- Use keybinds for fast navigation (optional)

Moonowl is fast, lean, and 100% Rust. Binary size is just ~22 MB.
Respect for computer resources as well as for user experience.

## Installation

Download and run:

| | |
|---|---|
| **macOS** | [Apple silicon](../../releases/latest/download/Moonowl-macos-arm64.dmg) / [Intel](../../releases/latest/download/Moonowl-macos-x64.dmg) |
| **Linux** | [AppImage](../../releases/latest/download/Moonowl-linux-x86_64.AppImage) / [.deb](../../releases/latest/download/Moonowl-linux-amd64.deb) / [.rpm](../../releases/latest/download/Moonowl-linux-x86_64.rpm) |
| **Windows** | [Installer](../../releases/latest/download/Moonowl-windows-setup.exe) / [.msi](../../releases/latest/download/Moonowl-windows.msi) |

Those links always point at the newest build; [every release](../../releases)
is listed if you want a particular one.

> **macOS first launch:** macOS blocks the app because it is not signed. After
> the warning, open *System Settings → Privacy & Security*, scroll to the
> *Security* section, click *Open Anyway*, and confirm. Do the same again when
opening a PDF with Moonowl for the first time.

> **Windows first launch:** SmartScreen blocks it. Click *More info* on the
> warning, then *Run anyway*. The installer needs no administrator rights: it
> installs for the current user alone. The `.msi` installs for every user of
> the machine and does need them.

## Developer build

With a local copy of the repo, a single command packages and installs the app:

```sh
# macOS and Linux
./scripts/install.sh
```
```powershell
# Windows
.\scripts\install.ps1
```

It fetches pdfium, builds the release, packages it and installs
it — on macOS into `/Applications`, on Linux through `apt`, `dnf` or an
AppImage in `~/.local/bin`, on Windows through the installer.

You need the Rust toolchain, plus the Xcode command line tools on macOS or
`libfontconfig1-dev` on Linux. Nothing else because pdfium comes from
[pdfium-binaries] and everything else is a crate. As you build the app
rather than download it, neither Gatekeeper nor SmartScreen is triggered,
so there is no *Open Anyway* step.

The app is pure Rust: [Dioxus] Native with [Blitz] laying out real HTML
and CSS instead of a webview. To work on it, run `./scripts/pdfium.sh` once and
then the usual `cargo run`, `cargo run -- FILE` and `cargo test`.

[Dioxus]: https://dioxuslabs.com
[Blitz]: https://github.com/DioxusLabs/blitz
[pdfium-binaries]: https://github.com/bblanchon/pdfium-binaries

## AI usage
The code was written by Claude (Opus 5.0, Opus 5.5, and Fable 5.1), but I had a strong vision
for the UI and kept complaining to Claude until I liked the result.

## Naming

I named the app after the [rusty-barred owl](https://en.wikipedia.org/wiki/Rusty-barred_owl).
Night owls might appreciate dark themes. Also, Rust.
