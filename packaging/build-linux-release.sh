#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
. "$ROOT_DIR/packaging/versioning.sh"
export PACKAGE_VERSION BUILD_DATE

# -------------------------------------------------------------------
# Pre-flight dependency check
# Reports all missing tools/packages in one pass so users don't have
# to iterate through failures one-by-one.
# -------------------------------------------------------------------
check_build_dependencies() {
  local missing=()
  local missing_pkgconfig=()

  # Required system tools
  for tool in cargo patchelf file wget; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      missing+=("$tool")
    fi
  done

  # appimagetool (not in apt, must be fetched from GitHub)
  if ! command -v appimagetool >/dev/null 2>&1; then
    missing+=("appimagetool (run: sudo wget -q https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage -O /usr/local/bin/appimagetool && sudo chmod +x /usr/local/bin/appimagetool)")
  fi

  # Required GTK/WebKit dev packages via pkg-config
  for pkg in \
    "libgtk-4-dev:gtk-4" \
    "libadwaita-1-dev:libadwaita-1" \
    "libvte-2.91-gtk4-dev:vte-2.91-gtk4" \
    "libgraphene-1.0-dev:graphene-1.0" \
    "libsoup-3.0-dev:libsoup-3.0" \
    "libjavascriptcoregtk-6.0-dev:javascriptcoregtk-6.0" \
    "libwebkitgtk-6.0-dev:webkitgtk-6.0"; do
    local pkg_name="${pkg%%:*}"
    local pkg_check="${pkg##*:}"
    if ! pkg-config --exists "$pkg_check" 2>/dev/null; then
      missing_pkgconfig+=("$pkg_name")
    fi
  done

  if [[ ${#missing[@]} -gt 0 || ${#missing_pkgconfig[@]} -gt 0 ]]; then
    echo "build-linux-release.sh: missing dependencies" >&2
    echo "" >&2
    if [[ ${#missing[@]} -gt 0 ]]; then
      echo "Tools not found (install via apt or as noted):" >&2
      for item in "${missing[@]}"; do
        echo "  - $item" >&2
      done
      echo "" >&2
    fi
    if [[ ${#missing_pkgconfig[@]} -gt 0 ]]; then
      echo "Dev packages not found (install via apt):" >&2
      for item in "${missing_pkgconfig[@]}"; do
        echo "  - $item" >&2
      done
      echo "" >&2
    fi
    echo "Alternatively, use the containerized build (no host dependencies):" >&2
    echo "  bash packaging/build-in-container.sh" >&2
    exit 1
  fi
}

if [[ "${SKIP_DEPENDENCY_CHECK:-0}" != "1" ]]; then
  check_build_dependencies
fi

if [[ "${SKIP_CARGO_BUILD:-0}" != "1" ]]; then
  echo "building release binary"
  cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml"
else
  echo "using existing release binary"
fi

echo "packaging Linux artifacts version $PACKAGE_VERSION"
SKIP_CARGO_BUILD=1 bash "$ROOT_DIR/packaging/build-deb.sh"
SKIP_CARGO_BUILD=1 bash "$ROOT_DIR/packaging/build-appimage.sh"

DEB_PATH="$(deb_output_path)"
APPIMAGE_PATH="$(appimage_output_path)"
if [[ ! -f "$DEB_PATH" || ! -f "$APPIMAGE_PATH" ]]; then
  echo "expected Linux artifacts were not created for version $PACKAGE_VERSION" >&2
  exit 1
fi

if [[ -z "${IN_PACKAGING_CONTAINER:-}" ]]; then
  echo "note: local Linux packaging may bundle the host GLIBC baseline; use packaging/release-verify.sh for pinned Debian 12 release artifacts"
fi
