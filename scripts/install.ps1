# Build Moonowl from this checkout and install it as a real app. One command,
# nothing to piece together: pdfium, the release build, the installer, and the
# install itself. macOS and Linux have scripts/install.sh.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

& (Join-Path $PSScriptRoot "pdfium.ps1")
# The versions bundle.yml builds with.
if (-not (Get-Command cargo-packager -ErrorAction SilentlyContinue)) {
    cargo install cargo-packager --version 0.11.8 --locked
}
if (-not (Get-Command cargo-about -ErrorAction SilentlyContinue)) {
    cargo install cargo-about --version 0.9.2 --locked --features cli
}
# The crates' licence notices, which the bundle ships.
New-Item -ItemType Directory -Force licenses | Out-Null
cargo about generate --locked about.hbs -o licenses/rust-crates.txt
cargo build --release
cargo packager --release --formats nsis

# SmartScreen will warn — the installer is not signed. "More info" → "Run anyway".
$setup = Get-ChildItem target\release\*-setup.exe | Select-Object -First 1
Start-Process -FilePath $setup.FullName -Wait
Write-Host "Installed Moonowl — it is in the Start menu."
