#!/usr/bin/env bash
# Cross-compile FSearch for Windows (x86_64) and package it.
#   rust/packaging/windows/build-windows.sh <out_dir>
# Needs: rustup target x86_64-pc-windows-gnu, mingw-w64, nsis.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
RUST="$(cd "$HERE/../.." && pwd)"
REPO="$(dirname "$RUST")"
OUT="${1:-$HERE/dist}"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

APP_DIR="$RUST"
if [ -d "$RUST/app" ]; then
    APP_DIR="$RUST/app"
fi

LICENSE_FILE="$REPO/LICENSE"
if [ ! -f "$LICENSE_FILE" ]; then
    LICENSE_FILE="$RUST/LICENSE"
fi

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$APP_DIR/Cargo.toml" | head -1)"

echo "==> building fsearch.exe $VERSION"
(cd "$APP_DIR" && CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc PCRE2_SYS_STATIC=1 \
    cargo build --release --locked --target x86_64-pc-windows-gnu)

STAGE="$OUT/FSearch-$VERSION-portable"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$APP_DIR/target/x86_64-pc-windows-gnu/release/fsearch.exe" "$STAGE/"
cp "$APP_DIR/assets/fsearch.ico" "$STAGE/"
sed 's/$/\r/' "$LICENSE_FILE" > "$STAGE/LICENSE.txt"
sed 's/$/\r/' > "$STAGE/README.txt" <<TXT
FSearch $VERSION for Windows (64-bit)

A fast file search utility: index folders once, then results appear as you type.

Run fsearch.exe. Add folders in Preferences > Index (or on the first screen),
then search. Search syntax: https://github.com/cboxdoerfer/fsearch/wiki/Search-syntax

Settings: %APPDATA%\\fsearch\\fsearch.conf
Index:    %LOCALAPPDATA%\\fsearch\\fsearch.db

Command line (from a terminal):
  fsearch.exe -s PATTERN      open with a search
  fsearch.exe -u              update the index and exit
  fsearch.exe -v              print the version

This folder is portable: it runs from anywhere. Use the setup program for a
Start menu entry and an uninstaller.

License: GPL-2.0-or-later (LICENSE.txt). Rust port of FSearch by Christian Boxdörfer.
TXT

echo "==> building the installer"
makensis -V2 -DVERSION="$VERSION" -DSRC="$STAGE" -DOUT="$OUT/FSearch-$VERSION-setup.exe" "$HERE/fsearch.nsi"
(cd "$OUT" && rm -f "FSearch-$VERSION-portable.zip" && python3 -c "
import shutil,sys; shutil.make_archive('FSearch-$VERSION-portable','zip','.', 'FSearch-$VERSION-portable')")
ls -la "$OUT"
