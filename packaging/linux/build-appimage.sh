#!/usr/bin/env bash
# Build the x86_64 AppImage: packaging/linux/build-appimage.sh [out_dir]
# Needs appimagetool and its type-2 runtime in packaging/linux/tools (see README).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
OUT="${1:-$ROOT/target/dist}"
TOOLS="$HERE/tools"
ID=io.github.cboxdoerfer.FSearch
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
if [ ! -x "$TOOLS/appimagetool-x86_64.AppImage" ] || [ ! -f "$TOOLS/runtime-x86_64" ]; then
    echo "Download appimagetool-x86_64.AppImage (github.com/AppImage/appimagetool) and runtime-x86_64"
    echo "(github.com/AppImage/type2-runtime) into $TOOLS first." >&2
    exit 1
fi
(cd "$ROOT" && PCRE2_SYS_STATIC=1 cargo build --release --locked)
APPDIR="$(mktemp -d)/AppDir"; trap 'rm -rf "$(dirname "$APPDIR")"' EXIT
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/scalable/apps" "$APPDIR/usr/share/man/man1"
install -m 755 "$ROOT/target/release/fsearch" "$APPDIR/usr/bin/fsearch"
install -m 644 "$ROOT/data/$ID.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/$ID.svg"
install -m 644 "$ROOT/data/fsearch.1" "$APPDIR/usr/share/man/man1/fsearch.1"
cat > "$APPDIR/usr/share/applications/$ID.desktop" <<DESK
[Desktop Entry]
Version=1.5
Type=Application
Name=FSearch
Comment=A file search application focusing on performance and advanced features
Icon=$ID
TryExec=fsearch
Exec=fsearch
Categories=Utility;
Keywords=search;fsearch;files;folders;music;video;documents;find;tool;everything;
StartupNotify=true
StartupWMClass=$ID
GenericName=File Search Utility
X-AppImage-Version=$VERSION
DESK
ln -s "usr/share/applications/$ID.desktop" "$APPDIR/$ID.desktop"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"
install -m 644 "$ROOT/assets/fsearch-256.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/$ID.png"
ln -s "usr/share/icons/hicolor/256x256/apps/$ID.png" "$APPDIR/$ID.png"
ln -s "$ID.png" "$APPDIR/.DirIcon"
printf '#!/bin/sh\nHERE="$(dirname "$(readlink -f "$0")")"\nexec "$HERE/usr/bin/fsearch" "$@"\n' > "$APPDIR/AppRun"
chmod 755 "$APPDIR/AppRun"
mkdir -p "$OUT"
ARCH=x86_64 "$TOOLS/appimagetool-x86_64.AppImage" --appimage-extract-and-run --runtime-file "$TOOLS/runtime-x86_64" \
    --no-appstream "$APPDIR" "$OUT/FSearch-$VERSION-x86_64.AppImage"
