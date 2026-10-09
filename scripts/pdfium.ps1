# Fetch libpdfium into .\pdfium, which is where `cargo run` (via
# .cargo\config.toml) and `cargo packager` (via Cargo.toml) both look for it.
# The tag matches PDFIUM_TAG in .github\workflows\bundle.yml, and the archive
# has to match its sum in scripts\pdfium.sha256.
$ErrorActionPreference = "Stop"
$tag = "chromium%2F8021"

$arch = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "x64" }
Set-Location (Join-Path $PSScriptRoot "..")
New-Item -ItemType Directory -Force -Path pdfium, pdfium\lib | Out-Null
Write-Host "pdfium-win-$arch"
Invoke-WebRequest -Uri "https://github.com/bblanchon/pdfium-binaries/releases/download/$tag/pdfium-win-$arch.tgz" -OutFile pdfium\pdfium.tgz
$want = ((Select-String -Path scripts\pdfium.sha256 -Pattern " pdfium-win-$arch.tgz$").Line -split " ")[0]
if ((Get-FileHash pdfium\pdfium.tgz -Algorithm SHA256).Hash -ne $want) {
    Remove-Item pdfium\pdfium.tgz
    throw "pdfium-win-$arch.tgz does not match its sum in scripts\pdfium.sha256."
}
tar xzf pdfium\pdfium.tgz -C pdfium
Remove-Item pdfium\pdfium.tgz

# The archive puts the DLL in bin\, which is where the packager wants it; the
# copy in lib\ is what MOONOWL_PDFIUM points at, so one path works on every system.
Copy-Item pdfium\bin\pdfium.dll pdfium\lib\ -Force
Get-ChildItem pdfium\lib
