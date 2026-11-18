#!/bin/sh
# FSearch @VERSION@ for Linux x86_64 — self-extracting installer.
#
#   sh fsearch-@VERSION@-linux-x86_64.sh             install for this user (~/.local)
#   sudo sh fsearch-@VERSION@-linux-x86_64.sh --system   install for everyone (/usr/local)
#   sh fsearch-@VERSION@-linux-x86_64.sh --prefix DIR    install into DIR
#   sh fsearch-@VERSION@-linux-x86_64.sh --uninstall [--system | --prefix DIR]
#   sh fsearch-@VERSION@-linux-x86_64.sh --extract DIR   only unpack the files
#
# Needs glibc 2.34+ and an X11 or Wayland session with OpenGL. Settings and the index are kept
# in ~/.config/fsearch and ~/.local/share/fsearch (shared with the C version of FSearch).
set -eu
VERSION="@VERSION@"
PREFIX="${HOME:-/root}/.local"
MODE=install
while [ $# -gt 0 ]; do
    case "$1" in
        --system) PREFIX=/usr/local ;;
        --prefix) shift; PREFIX="${1:?--prefix needs a directory}" ;;
        --prefix=*) PREFIX="${1#--prefix=}" ;;
        --uninstall) MODE=uninstall ;;
        --extract) MODE=extract; shift; DEST="${1:?--extract needs a directory}" ;;
        -h|--help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "Unknown option: $1 (see --help)" >&2; exit 2 ;;
    esac
    shift
done
MANIFEST="$PREFIX/share/fsearch-rs/installed-files"

refresh() {
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$PREFIX/share/applications" 2>/dev/null || true
    command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true
}

if [ "$MODE" = uninstall ]; then
    if [ ! -f "$MANIFEST" ]; then
        echo "FSearch is not installed in $PREFIX (no $MANIFEST)." >&2
        exit 1
    fi
    while IFS= read -r f; do rm -f "$PREFIX/$f"; done < "$MANIFEST"
    rm -f "$MANIFEST"; rmdir "$PREFIX/share/fsearch-rs" 2>/dev/null || true
    refresh
    echo "FSearch removed from $PREFIX. Settings in ~/.config/fsearch are kept."
    exit 0
fi

ARCH="$(uname -m)"
[ "$ARCH" = x86_64 ] || { echo "This package is for x86_64 (this machine: $ARCH)." >&2; exit 1; }
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
LINE=$(awk '/^__FSEARCH_PAYLOAD__$/ { print NR + 1; exit 0 }' "$0")
tail -n +"$LINE" "$0" | tar -xzf - -C "$TMP"

if [ "$MODE" = extract ]; then
    mkdir -p "$DEST"; cp -R "$TMP"/. "$DEST"/; echo "Extracted to $DEST"; exit 0
fi

mkdir -p "$PREFIX" 2>/dev/null || true
if [ ! -w "$PREFIX" ] && [ ! -w "$(dirname "$PREFIX")" ]; then
    echo "Cannot write to $PREFIX. Run with sudo for --system, or choose --prefix." >&2
    exit 1
fi
cd "$TMP"
find . -type f | sed 's|^\./||' > files.list
grep -v '^files.list$' files.list > files.install
while IFS= read -r f; do
    mkdir -p "$PREFIX/$(dirname "$f")"
    cp "$f" "$PREFIX/$f"
done < files.install
chmod 755 "$PREFIX/bin/fsearch"
mkdir -p "$PREFIX/share/fsearch-rs"
cp files.install "$MANIFEST"
# the desktop entry needs the absolute program path when the prefix is not on PATH
sed -i "s|^Exec=fsearch|Exec=$PREFIX/bin/fsearch|; s|^TryExec=fsearch|TryExec=$PREFIX/bin/fsearch|" \
    "$PREFIX/share/applications/io.github.cboxdoerfer.FSearch.desktop"
refresh
echo "FSearch $VERSION installed in $PREFIX."
case ":$PATH:" in *":$PREFIX/bin:"*) ;; *) echo "Note: $PREFIX/bin is not on your PATH; start FSearch from the applications menu or $PREFIX/bin/fsearch." ;; esac
"$PREFIX/bin/fsearch" --version >/dev/null 2>&1 || echo "Warning: the program did not start here (needs glibc 2.34+)." >&2
exit 0
__FSEARCH_PAYLOAD__
