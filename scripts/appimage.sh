#!/bin/sh
# cargo-packager fetches the tools an AppImage is made with from release tags
# that move ("continuous"), and checks nothing. It does not fetch a tool that
# is already in its cache, so this puts pinned, checked copies there first.
# bundle.yml and install.sh both run it.
set -eu
sums="$(cd "$(dirname "$0")" && pwd)/appimage.sha256"
# Where cargo-packager looks, which follows XDG_CACHE_HOME when it is set.
tools="${XDG_CACHE_HOME:-$HOME/.cache}/.cargo-packager/AppImage"
# Downloaded and checked beside the cache, so a copy that fails the check
# never lands where cargo-packager would use it.
fetched="$(mktemp -d)"
trap 'rm -rf "$fetched"' EXIT
cd "$fetched"
curl -fsSL -o AppRun-x86_64 \
    https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64
curl -fsSL -o linuxdeploy-x86_64.AppImage \
    https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage
curl -fsSL -o linuxdeploy-plugin-appimage.AppImage \
    https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/1-alpha-20250213-1/linuxdeploy-plugin-appimage-x86_64.AppImage
sha256sum -c "$sums"
chmod 764 ./*
mkdir -p "$tools"
mv -f ./* "$tools"/
