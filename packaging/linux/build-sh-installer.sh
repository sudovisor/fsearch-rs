#!/usr/bin/env bash
# Build the self-extracting Linux installer: packaging/linux/build-sh-installer.sh <out_dir>
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"

OUT="${1:-$ROOT/target/dist}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
ID=io.github.cboxdoerfer.FSearch
(cd "$ROOT" && PCRE2_SYS_STATIC=1 cargo build --release --locked)
P="$(mktemp -d)"; trap 'rm -rf "$P"' EXIT
mkdir -p "$P/bin" "$P/share/applications" "$P/share/icons/hicolor/scalable/apps" "$P/share/icons/hicolor/256x256/apps" \
    "$P/share/man/man1" "$P/share/doc/fsearch-rs"
install -m 755 "$ROOT/target/release/fsearch" "$P/bin/fsearch"
install -m 644 "$ROOT/data/$ID.svg" "$P/share/icons/hicolor/scalable/apps/$ID.svg"
install -m 644 "$ROOT/assets/fsearch-256.png" "$P/share/icons/hicolor/256x256/apps/$ID.png"
install -m 644 "$ROOT/data/fsearch.1" "$P/share/man/man1/fsearch.1"
install -m 644 "$ROOT/LICENSE" "$P/share/doc/fsearch-rs/LICENSE"
cat > "$P/share/applications/$ID.desktop" <<DESK
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
DESK
mkdir -p "$OUT"
F="$OUT/fsearch-$VERSION-linux-x86_64.sh"
sed "s/@VERSION@/$VERSION/g" "$HERE/installer-head.sh" > "$F"
tar -czf - -C "$P" . >> "$F"
chmod 755 "$F"
ls -la "$F"
