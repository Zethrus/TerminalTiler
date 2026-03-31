#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_DIR="$ROOT_DIR/packaging/.build/release-smoke"
. "$ROOT_DIR/packaging/versioning.sh"
export PACKAGE_VERSION BUILD_DATE

APPIMAGE_PATH="$(appimage_output_path)"
DEB_PATH="$(deb_output_path)"
METAINFO_PATH="$ROOT_DIR/resources/dev.zethrus.terminaltiler.appdata.xml"

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

need_cmd cargo
need_cmd dpkg-deb
need_cmd appstreamcli
need_cmd appimagetool
need_cmd timeout

validate_appstream() {
  local output
  if output="$(appstreamcli validate "$METAINFO_PATH" 2>&1)"; then
    printf '%s\n' "$output"
    return 0
  fi

  printf '%s\n' "$output"
  local filtered
  filtered="$(printf '%s\n' "$output" |
    grep -E '^(E|W|I|P):' |
    grep -v 'url-homepage-missing' || true)"
  if [[ -n "$filtered" ]]; then
    echo "unexpected AppStream validation issue" >&2
    exit 1
  fi
}

echo "==> validating AppStream metadata"
validate_appstream

echo "==> release version $PACKAGE_VERSION"

echo "==> building release binary"
cd "$ROOT_DIR"
cargo build --release

echo "==> building Debian package"
SKIP_CARGO_BUILD=1 bash "$ROOT_DIR/packaging/build-deb.sh"

echo "==> building AppImage"
SKIP_CARGO_BUILD=1 bash "$ROOT_DIR/packaging/build-appimage.sh"

test -f "$DEB_PATH"
test -f "$APPIMAGE_PATH"

echo "==> preparing smoke-test sandbox"
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR/deb" "$BUILD_DIR/appimage"

echo "==> checking Debian package payload"
dpkg-deb -x "$DEB_PATH" "$BUILD_DIR/deb"
test -f "$BUILD_DIR/deb/usr/share/metainfo/dev.zethrus.terminaltiler.appdata.xml"
test -f "$BUILD_DIR/deb/usr/share/applications/dev.zethrus.terminaltiler.desktop"
test -f "$BUILD_DIR/deb/opt/terminaltiler/lib/libgtk-4.so.1"
test -f "$BUILD_DIR/deb/opt/terminaltiler/lib/libadwaita-1.so.0"
test -f "$BUILD_DIR/deb/opt/terminaltiler/lib/libvte-2.91-gtk4.so.0"
test -f "$BUILD_DIR/deb/opt/terminaltiler/share/glib-2.0/schemas/gschemas.compiled"

if command -v xvfb-run >/dev/null 2>&1; then
  echo "==> smoke-testing extracted Debian runtime"
  status=0
  timeout 5s xvfb-run -a "$BUILD_DIR/deb/opt/terminaltiler/bin/terminaltiler" || status=$?
  if [[ ${status:-0} -ne 0 && ${status:-0} -ne 124 ]]; then
    exit "${status}"
  fi
else
  echo "==> skipping Debian runtime launch smoke test because xvfb-run is unavailable"
fi

echo "==> checking AppImage payload"
cd "$BUILD_DIR/appimage"
"$APPIMAGE_PATH" --appimage-extract >/dev/null
test -f "$BUILD_DIR/appimage/squashfs-root/usr/share/metainfo/dev.zethrus.terminaltiler.appdata.xml"
test -f "$BUILD_DIR/appimage/squashfs-root/usr/share/applications/dev.zethrus.terminaltiler.desktop"
test -f "$BUILD_DIR/appimage/squashfs-root/usr/lib/libgtk-4.so.1"
test -f "$BUILD_DIR/appimage/squashfs-root/usr/lib/libadwaita-1.so.0"
test -f "$BUILD_DIR/appimage/squashfs-root/usr/lib/libvte-2.91-gtk4.so.0"

if command -v xvfb-run >/dev/null 2>&1; then
  echo "==> smoke-testing AppImage launch"
  cd "$ROOT_DIR"
  status=0
  timeout 5s xvfb-run -a "$APPIMAGE_PATH" || status=$?
  if [[ ${status:-0} -ne 0 && ${status:-0} -ne 124 ]]; then
    exit "${status}"
  fi
else
  echo "==> skipping AppImage launch smoke test because xvfb-run is unavailable"
fi

echo "release smoke test passed"
