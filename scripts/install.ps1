# Build Moonowl from this checkout and install it as a real app. One command,
# nothing to piece together: pdfium, the release build, the installer, and the
# install itself. macOS and Linux have scripts/install.sh.
$ErrorActionPreference = "Stop"
# "Stop" does not cover a program's exit code, so each command is checked:
# a failed licence check must not let an older notices file ship.
function Checked {
    & $args[0] @($args | Select-Object -Skip 1)
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
}
Set-Location (Join-Path $PSScriptRoot "..")

& (Join-Path $PSScriptRoot "pdfium.ps1")
# The versions bundle.yml builds with.
if (-not (Get-Command cargo-packager -ErrorAction SilentlyContinue)) {
    Checked cargo install cargo-packager --version 0.11.8 --locked
}
if (-not (Get-Command cargo-about -ErrorAction SilentlyContinue)) {
    Checked cargo install cargo-about --version 0.9.2 --locked --features cli
}
# The crates' licence notices, which the bundle ships.
New-Item -ItemType Directory -Force licenses | Out-Null
Checked cargo about generate --locked about.hbs -o licenses/rust-crates.txt
Checked cargo build --release
Checked cargo packager --release --formats nsis

# SmartScreen will warn — the installer is not signed. "More info" → "Run anyway".
$setup = Get-ChildItem target\release\*-setup.exe | Select-Object -First 1
Start-Process -FilePath $setup.FullName -Wait
Write-Host "Installed Moonowl — it is in the Start menu."
