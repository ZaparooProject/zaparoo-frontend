#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="${BUILD_DIR:-$ROOT_DIR/build-macos}"
APP_DIR="${APP_DIR:-$ROOT_DIR/output/macos/Zaparoo Frontend.app}"
EXECUTABLE="$BUILD_DIR/bin/frontend"
MACDEPLOYQT="${MACDEPLOYQT:-macdeployqt}"

if [[ ! -x "$EXECUTABLE" ]]; then
    echo "Missing executable: $EXECUTABLE" >&2
    echo "Build first with: cmake --build $BUILD_DIR" >&2
    exit 1
fi

rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"

cp "$EXECUTABLE" "$APP_DIR/Contents/MacOS/frontend"

cat > "$APP_DIR/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>Zaparoo Frontend</string>
  <key>CFBundleExecutable</key>
  <string>frontend</string>
  <key>CFBundleIdentifier</key>
  <string>org.zaparoo.frontend</string>
  <key>CFBundleName</key>
  <string>Zaparoo Frontend</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>0.1.0</string>
  <key>LSMinimumSystemVersion</key>
  <string>15.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

"$MACDEPLOYQT" "$APP_DIR" -qmldir="$ROOT_DIR/src/ui"

codesign --force --deep --sign - "$APP_DIR"
codesign --verify --deep --strict "$APP_DIR"

echo "$APP_DIR"
