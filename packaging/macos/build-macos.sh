#!/usr/bin/env bash
# Build FSearch for macOS (Universal 2: x86_64 + Apple Silicon arm64) and package as:
#   - FSearch.app application bundle
#   - FSearch-<version>-macos.zip
#   - FSearch-<version>-macos.dmg
#
# Can run natively on macOS (with Xcode / Command Line Tools) or cross-compiled
# on Linux with cargo-zigbuild and an Apple SDK in SDKROOT or ~/.local/share/sdks/.
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

echo "==> Building FSearch $VERSION for macOS (x86_64 & arm64)"

export PCRE2_SYS_STATIC=1
mkdir -p "$OUT"

IS_DARWIN=false
if [ "$(uname -s)" = "Darwin" ]; then
    IS_DARWIN=true
fi

# Locate SDK if on Linux
if [ "$IS_DARWIN" = "false" ]; then
    if [ -z "${SDKROOT:-}" ]; then
        if [ -d "$HOME/.local/share/sdks/MacOSX14.5.sdk" ]; then
            export SDKROOT="$HOME/.local/share/sdks/MacOSX14.5.sdk"
        elif [ -d "$HERE/../tools/sdk/MacOSX14.5.sdk" ]; then
            export SDKROOT="$HERE/../tools/sdk/MacOSX14.5.sdk"
        fi
    fi
    export PATH="$HOME/.local/bin:$PATH"
fi

BUILD_CMD="cargo build"
if [ "$IS_DARWIN" = "false" ] && command -v cargo-zigbuild >/dev/null 2>&1; then
    BUILD_CMD="cargo zigbuild"
fi

# 1. Compile x86_64
echo "--> Compiling for x86_64-apple-darwin..."
(cd "$APP_DIR" && $BUILD_CMD --release --locked --target x86_64-apple-darwin)

# 2. Compile aarch64 (Apple Silicon)
echo "--> Compiling for aarch64-apple-darwin..."
(cd "$APP_DIR" && $BUILD_CMD --release --locked --target aarch64-apple-darwin)

# 3. Create Universal 2 binary
echo "--> Creating Universal 2 binary..."
STAGE_BIN="/tmp/fsearch-bin-universal-$VERSION"
X86_BIN="$APP_DIR/target/x86_64-apple-darwin/release/fsearch"
ARM_BIN="$APP_DIR/target/aarch64-apple-darwin/release/fsearch"

if command -v lipo >/dev/null 2>&1; then
    lipo -create -output "$STAGE_BIN" "$X86_BIN" "$ARM_BIN"
else
    python3 "$HERE/lipo.py" "$X86_BIN" "$ARM_BIN" "$STAGE_BIN"
fi
chmod +x "$STAGE_BIN"

# 4. Assemble FSearch.app bundle
echo "--> Assembling FSearch.app..."
APP_BUNDLE="$OUT/FSearch.app"
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS" "$APP_BUNDLE/Contents/Resources"

# Copy binary
cp "$STAGE_BIN" "$APP_BUNDLE/Contents/MacOS/fsearch"
chmod +x "$APP_BUNDLE/Contents/MacOS/fsearch"
rm -f "$STAGE_BIN"

# Generate ICNS icon
python3 "$HERE/make-icns.py" "$APP_DIR/assets/fsearch-256.png" "$APP_BUNDLE/Contents/Resources/fsearch.icns"

# Generate Info.plist
sed "s/VERSION_PLACEHOLDER/$VERSION/g" "$HERE/Info.plist" > "$APP_BUNDLE/Contents/Info.plist"

# Generate PkgInfo
printf "APPL????" > "$APP_BUNDLE/Contents/PkgInfo"

# 5. Create README.txt and LICENSE.txt
cat > "$OUT/README.txt" <<TXT
FSearch $VERSION for macOS (Universal 2: Apple Silicon M1/M2/M3/M4 & Intel x86_64)

Fast file search utility: index your folders once, then results appear as you type.

INSTALLATION:
  Drag "FSearch.app" into your Applications folder (/Applications).

SECURITY / GATEKEEPER NOTE:
  Because FSearch is distributed directly without an Apple Developer ID signature,
  macOS may show "FSearch cannot be opened because the developer cannot be verified"
  on first launch.
  To allow it to run:
    Option A: Right-click (or Control-click) FSearch.app in Finder and choose "Open",
              then click "Open" in the dialog.
    Option B: In Terminal, remove the quarantine attribute:
              xattr -cr /Applications/FSearch.app

COMMAND LINE USAGE:
  /Applications/FSearch.app/Contents/MacOS/fsearch -s PATTERN    open with a search
  /Applications/FSearch.app/Contents/MacOS/fsearch -u            update the index and exit
  /Applications/FSearch.app/Contents/MacOS/fsearch -v            print the version

SETTINGS & INDEX:
  Settings: ~/.config/fsearch/fsearch.conf
  Database: ~/.local/share/fsearch/fsearch.db

License: GPL-2.0-or-later. Pure Rust port of FSearch by Christian Boxdörfer.
TXT
cp "$LICENSE_FILE" "$OUT/LICENSE.txt"

# 6. Create ZIP archive
echo "--> Creating FSearch-$VERSION-macos.zip..."
(cd "$OUT" && rm -f "FSearch-$VERSION-macos.zip" && zip -r -y -q "FSearch-$VERSION-macos.zip" "FSearch.app" "README.txt" "LICENSE.txt")

# 7. Create DMG disk image if tools available
echo "--> Creating FSearch-$VERSION-macos.dmg..."
DMG_OUT="$OUT/FSearch-$VERSION-macos.dmg"
rm -f "$DMG_OUT"

if command -v hdiutil >/dev/null 2>&1; then
    DMG_TEMP="/tmp/fsearch-dmg-stage-$VERSION"
    rm -rf "$DMG_TEMP"
    mkdir -p "$DMG_TEMP"
    cp -R "$APP_BUNDLE" "$DMG_TEMP/"
    cp "$OUT/README.txt" "$OUT/LICENSE.txt" "$DMG_TEMP/"
    ln -s /Applications "$DMG_TEMP/Applications"
    hdiutil create -volname "FSearch" -srcfolder "$DMG_TEMP" -ov -format UDZO "$DMG_OUT" >/dev/null
    rm -rf "$DMG_TEMP"
else
    # Linux host: use genisoimage to create HFS hybrid disk image
    GENISO="$HERE/../tools/genisoimage"
    if [ ! -x "$GENISO" ] && command -v genisoimage >/dev/null 2>&1; then
        GENISO="$(command -v genisoimage)"
    fi
    if [ -x "$GENISO" ]; then
        DMG_TEMP="/tmp/fsearch-dmg-stage-$VERSION"
        rm -rf "$DMG_TEMP"
        mkdir -p "$DMG_TEMP"
        cp -R "$APP_BUNDLE" "$DMG_TEMP/"
        cp "$OUT/README.txt" "$OUT/LICENSE.txt" "$DMG_TEMP/"
        "$GENISO" -V "FSearch" -D -R -hfs -part -hfs-volid "FSearch" -o "$DMG_OUT" "$DMG_TEMP" >/dev/null 2>&1 || true
        rm -rf "$DMG_TEMP"
    fi
fi

# 8. Checksums
echo "--> Generating SHA256SUMS..."
(cd "$OUT" && sha256sum "FSearch-$VERSION-macos.zip" $([ -f "FSearch-$VERSION-macos.dmg" ] && echo "FSearch-$VERSION-macos.dmg") > SHA256SUMS)

echo "==> macOS dist package built successfully in $OUT"
ls -la "$OUT"
