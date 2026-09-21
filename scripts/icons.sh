#!/bin/sh
# Regenerates everything under icons/ from icons/app-icon.svg, and from
# icons/document-icon.svg — the owl's head alone — wherever the result is small.
# Needs resvg and ImageMagick (brew install resvg imagemagick); iconutil is macOS's.
set -e
cd "$(dirname "$0")/../icons"
png() { resvg -w "$1" "${3:-app-icon.svg}" "$2"; }
png 1024 app-icon.png
png 512 icon.png
png 32 32x32.png document-icon.svg
png 64 64x64.png
png 128 128x128.png
png 256 128x128@2x.png
# The frames Explorer uses for a file in a list are the small ones, and a PDF
# there wears this icon: the installers register `Moonowl.exe,0`.
for s in 16 24 32 48; do png $s ico-$s.png document-icon.svg; done
png 64 ico-64.png; png 256 ico-256.png
magick ico-256.png ico-64.png ico-48.png ico-32.png ico-24.png ico-16.png icon.ico
rm ico-*.png
rm -rf icon.iconset && mkdir icon.iconset
for s in 16 32 128 256 512; do
  png $s icon.iconset/icon_${s}x${s}.png
  png $((s*2)) icon.iconset/icon_${s}x${s}@2x.png
done
iconutil --convert icns --output icon.icns icon.iconset
rm -rf icon.iconset
# The document icon: what a PDF wears in the Finder. See `Info.plist`.
mkdir document.iconset
for s in 16 32 128 256 512; do
  png $s document.iconset/icon_${s}x${s}.png document-icon.svg
  png $((s*2)) document.iconset/icon_${s}x${s}@2x.png document-icon.svg
done
iconutil --convert icns --output document.icns document.iconset
rm -rf document.iconset
