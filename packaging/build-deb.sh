#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
. "$ROOT_DIR/packaging/versioning.sh"

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
TARGET_BIN="$TARGET_DIR/release/terminaltiler"
OUTPUT_DEB="$(deb_output_path)"
LATEST_DEB="$(deb_latest_path)"
STAGE_ROOT="$ROOT_DIR/packaging/.build/deb-root"
APP_ROOT="$STAGE_ROOT/opt/terminaltiler"

mkdir -p "$(dirname "$OUTPUT_DEB")"

if [[ "${SKIP_CARGO_BUILD:-0}" != "1" ]]; then
  echo "building release binary"
  cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml"
else
  echo "using existing release binary"
fi

echo "packaging Debian artifact version $PACKAGE_VERSION"

rm -rf "$STAGE_ROOT"
mkdir -p "$STAGE_ROOT/DEBIAN" "$APP_ROOT/bin" "$STAGE_ROOT/usr/bin" "$STAGE_ROOT/usr/share/applications" "$STAGE_ROOT/usr/share/icons/hicolor/scalable/apps" "$STAGE_ROOT/usr/share/metainfo"

cp "$ROOT_DIR/packaging/deb/DEBIAN/control" "$STAGE_ROOT/DEBIAN/control"
cp "$ROOT_DIR/resources/dev.zethrus.terminaltiler.desktop" "$STAGE_ROOT/usr/share/applications/dev.zethrus.terminaltiler.desktop"
cp "$ROOT_DIR/resources/terminaltiler.svg" "$STAGE_ROOT/usr/share/icons/hicolor/scalable/apps/terminaltiler.svg"
cp "$ROOT_DIR/resources/dev.zethrus.terminaltiler.appdata.xml" "$STAGE_ROOT/usr/share/metainfo/dev.zethrus.terminaltiler.appdata.xml"
set_control_version "$STAGE_ROOT/DEBIAN/control"
set_appdata_release "$STAGE_ROOT/usr/share/metainfo/dev.zethrus.terminaltiler.appdata.xml"
cp "$TARGET_BIN" "$APP_ROOT/bin/terminaltiler-bin"
cp "$ROOT_DIR/packaging/run-bundled.sh" "$APP_ROOT/bin/terminaltiler"
cp "$ROOT_DIR/packaging/run-bundled.sh" "$STAGE_ROOT/usr/bin/terminaltiler"
sed -i "s#APP_ROOT=\"\$(cd \"\$(dirname \"\$0\")/..\" && pwd)\"#APP_ROOT=\"/opt/terminaltiler\"#" "$STAGE_ROOT/usr/bin/terminaltiler"

bash "$ROOT_DIR/packaging/bundle-runtime.sh" "$TARGET_BIN" "$APP_ROOT"

detect_glibc_floor() {
  (
    objdump -T "$APP_ROOT/bin/terminaltiler-bin" 2>/dev/null
    find "$APP_ROOT/lib" -maxdepth 1 -type f -print0 | xargs -0 objdump -T 2>/dev/null
  ) | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -n 1 | sed 's/^GLIBC_//'
}

GLIBC_FLOOR="${GLIBC_FLOOR:-$(detect_glibc_floor)}"
if [[ -z "$GLIBC_FLOOR" ]]; then
  echo "failed to detect GLIBC floor for bundled runtime" >&2
  exit 1
fi

set_control_glibc_floor "$STAGE_ROOT/DEBIAN/control" "$GLIBC_FLOOR"
chmod 0755 "$APP_ROOT/bin/terminaltiler-bin" "$APP_ROOT/bin/terminaltiler" "$STAGE_ROOT/usr/bin/terminaltiler"

rm -f "$OUTPUT_DEB"
dpkg-deb --build "$STAGE_ROOT" "$OUTPUT_DEB"
update_latest_symlink "$OUTPUT_DEB" "$LATEST_DEB"
record_successful_build_version
echo "wrote $OUTPUT_DEB"
echo "updated $LATEST_DEB"
echo "detected bundled runtime GLIBC floor $GLIBC_FLOOR"
