#!/bin/sh
# Build the command-line tool and install it as `moonowl` in ~/.local/bin,
# with pdfium in ~/.local/lib/Moonowl, which is where the tool looks for it
# beside ../lib/Moonowl. It is built as `moonowl-cli`, because it shares the
# target folder with the app's `Moonowl`, and macOS does not tell the two
# names apart. Windows is not covered yet.
set -eu
cd "$(dirname "$0")/.."

sh scripts/pdfium.sh
cargo build --release -p moonowl-cli

case "$(uname -s)" in
Darwin) library=libpdfium.dylib ;;
Linux) library=libpdfium.so ;;
*) echo "Unknown system $(uname -s): this script is for macOS and Linux." >&2; exit 1 ;;
esac

mkdir -p "$HOME/.local/bin" "$HOME/.local/lib/Moonowl"
cp target/release/moonowl-cli "$HOME/.local/bin/moonowl"
cp "pdfium/lib/$library" "$HOME/.local/lib/Moonowl/"
echo "Installed ~/.local/bin/moonowl — run 'moonowl' if ~/.local/bin is on your PATH."
