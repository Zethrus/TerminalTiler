#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
. "$ROOT_DIR/packaging/versioning.sh"

emit_output() {
  local key="$1"
  local value="$2"

  if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    printf '%s=%s\n' "$key" "$value" >> "$GITHUB_OUTPUT"
  else
    printf '%s=%s\n' "$key" "$value"
  fi
}

emit_output package_version "$PACKAGE_VERSION"
emit_output build_date "$BUILD_DATE"
emit_output deb_path "dist/terminaltiler_${PACKAGE_VERSION}_amd64.deb"
emit_output appimage_path "dist/TerminalTiler-${PACKAGE_VERSION}-x86_64.AppImage"
emit_output windows_zip_path "dist/TerminalTiler-${PACKAGE_VERSION}-windows-x86_64.zip"
emit_output windows_portable_exe_path "dist/TerminalTiler-${PACKAGE_VERSION}-portable-x86_64.exe"
emit_output windows_installer_path "dist/TerminalTiler-setup-${PACKAGE_VERSION}-x86_64.exe"
emit_output windows_msi_path "dist/TerminalTiler-setup-${PACKAGE_VERSION}-x86_64.msi"