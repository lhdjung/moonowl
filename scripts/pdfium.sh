#!/bin/sh
# Fetch libpdfium into ./pdfium, which is where `cargo run` (via
# .cargo/config.toml) and `cargo packager` (via Cargo.toml) both look for it.
# The tag matches PDFIUM_TAG in .github/workflows/bundle.yml, and the archive
# has to match its sum in scripts/pdfium.sha256.
set -eu
TAG=chromium%2F8021

case "$(uname -s)" in
    Darwin) os=mac ;;
    Linux)  os=linux ;;
    *) echo "Unknown system $(uname -s) — use scripts/pdfium.ps1 on Windows." >&2; exit 1 ;;
esac
case "$(uname -m)" in
    arm64|aarch64) arch=arm64 ;;
    x86_64|amd64)  arch=x64 ;;
    *) echo "No pdfium build for $(uname -m)." >&2; exit 1 ;;
esac

cd "$(dirname "$0")/.."
mkdir -p pdfium
echo "pdfium-$os-$arch"
archive="pdfium-$os-$arch.tgz"
curl -sSfL -o "pdfium/$archive" "https://github.com/bblanchon/pdfium-binaries/releases/download/$TAG/$archive"
(cd pdfium && grep " $archive\$" ../scripts/pdfium.sha256 | shasum -a 256 -c)
tar xzf "pdfium/$archive" -C pdfium
rm "pdfium/$archive"
ls pdfium/lib
