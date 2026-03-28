#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
. "$ROOT_DIR/packaging/versioning.sh"

APPDIR="$ROOT_DIR/packaging/.build/appimage/TerminalTiler.AppDir"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
TARGET_BIN="$TARGET_DIR/release/terminaltiler"
APP_PREFIX="$APPDIR/usr"
OUTPUT_APPIMAGE="$(appimage_output_path)"
LATEST_APPIMAGE="$(appimage_latest_path)"
TEMP_APPIMAGE="$ROOT_DIR/packaging/.build/appimage/TerminalTiler-x86_64.AppImage"

mkdir -p "$(dirname "$OUTPUT_APPIMAGE")" "$(dirname "$APPDIR")"

if [[ "${SKIP_CARGO_BUILD:-0}" != "1" ]]; then
  echo "building release binary"
  cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml"
else
  echo "using existing release binary"
fi

echo "packaging AppImage artifact version $PACKAGE_VERSION"

if ! command -v appimagetool >/dev/null 2>&1; then
  echo "appimagetool is required to build the AppImage"
  exit 1
fi

rm -rf "$APPDIR"
mkdir -p "$APP_PREFIX/bin" "$APP_PREFIX/share/applications" "$APP_PREFIX/share/icons/hicolor/scalable/apps" "$APP_PREFIX/share/metainfo"

cp "$TARGET_BIN" "$APP_PREFIX/bin/terminaltiler-bin"
cp "$ROOT_DIR/packaging/run-bundled.sh" "$APP_PREFIX/bin/terminaltiler"
cp "$ROOT_DIR/resources/dev.zethrus.terminaltiler.desktop" "$APP_PREFIX/share/applications/dev.zethrus.terminaltiler.desktop"
cp "$ROOT_DIR/resources/terminaltiler.svg" "$APP_PREFIX/share/icons/hicolor/scalable/apps/terminaltiler.svg"
cp "$ROOT_DIR/resources/dev.zethrus.terminaltiler.appdata.xml" "$APP_PREFIX/share/metainfo/dev.zethrus.terminaltiler.appdata.xml"
set_appdata_release "$APP_PREFIX/share/metainfo/dev.zethrus.terminaltiler.appdata.xml"
cp "$ROOT_DIR/resources/dev.zethrus.terminaltiler.desktop" "$APPDIR/dev.zethrus.terminaltiler.desktop"
cp "$ROOT_DIR/resources/terminaltiler.svg" "$APPDIR/terminaltiler.svg"
cp "$ROOT_DIR/packaging/appimage/AppRun" "$APPDIR/AppRun"
ln -sf terminaltiler.svg "$APPDIR/.DirIcon"

bash "$ROOT_DIR/packaging/bundle-runtime.sh" "$TARGET_BIN" "$APP_PREFIX"

chmod +x "$APP_PREFIX/bin/terminaltiler" "$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun"

rm -f "$TEMP_APPIMAGE" "$OUTPUT_APPIMAGE"
(
  cd "$(dirname "$APPDIR")"
  APPIMAGE_EXTRACT_AND_RUN=1 appimagetool --no-appstream "$APPDIR"
)
mv "$TEMP_APPIMAGE" "$OUTPUT_APPIMAGE"
update_latest_symlink "$OUTPUT_APPIMAGE" "$LATEST_APPIMAGE"
record_successful_build_version
echo "wrote $OUTPUT_APPIMAGE"
echo "updated $LATEST_APPIMAGE"
