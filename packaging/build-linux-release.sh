#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
. "$ROOT_DIR/packaging/versioning.sh"
. "$ROOT_DIR/packaging/linux-build-prereqs.sh"
export PACKAGE_VERSION BUILD_DATE

if [[ "${SKIP_DEPENDENCY_CHECK:-0}" != "1" ]]; then
  cargo_dependency_check=1
  if [[ "${SKIP_CARGO_BUILD:-0}" == "1" ]]; then
    cargo_dependency_check=0
  fi
  check_linux_packaging_dependencies "build-linux-release.sh" "release" "$cargo_dependency_check"
fi

if [[ "${SKIP_CARGO_BUILD:-0}" != "1" ]]; then
  echo "building release binary"
  cargo build --locked --release --features voice-cpal --manifest-path "$ROOT_DIR/Cargo.toml"
else
  echo "using existing release binary"
fi

echo "packaging Linux artifacts version $PACKAGE_VERSION"
SKIP_DEPENDENCY_CHECK=1 SKIP_CARGO_BUILD=1 bash "$ROOT_DIR/packaging/build-deb.sh"
SKIP_DEPENDENCY_CHECK=1 SKIP_CARGO_BUILD=1 bash "$ROOT_DIR/packaging/build-appimage.sh"

DEB_PATH="$(deb_output_path)"
APPIMAGE_PATH="$(appimage_output_path)"
if [[ ! -f "$DEB_PATH" || ! -f "$APPIMAGE_PATH" ]]; then
  echo "expected Linux artifacts were not created for version $PACKAGE_VERSION" >&2
  exit 1
fi

if [[ -z "${IN_PACKAGING_CONTAINER:-}" ]]; then
  echo "note: local Linux packaging may bundle the host GLIBC baseline; use packaging/release-verify.sh for pinned Debian 12 release artifacts"
fi
