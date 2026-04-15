#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_DIR="$ROOT_DIR/packaging/.build/release-smoke"
. "$ROOT_DIR/packaging/versioning.sh"
export PACKAGE_VERSION BUILD_DATE
SKIP_PACKAGE_BUILD="${SKIP_PACKAGE_BUILD:-0}"

APPIMAGE_PATH="$(appimage_output_path)"
DEB_PATH="$(deb_output_path)"
METAINFO_PATH="$ROOT_DIR/resources/dev.zethrus.terminaltiler.appdata.xml"

dump_smoke_logs() {
  local sandbox_root="$1"
  local label="$2"
  local found=0
  local log_path

  while IFS= read -r log_path; do
    [[ -n "$log_path" ]] || continue
    found=1
    echo "==> $label log dump: $log_path" >&2
    cat "$log_path" >&2 || true
  done < <(find "$sandbox_root" \( -name terminaltiler-session.log -o -name launcher-stderr.log \) -type f | sort)

  if [[ $found -eq 0 ]]; then
    echo "==> $label produced no smoke logs under $sandbox_root" >&2
  fi
}

fail_smoke() {
  local sandbox_root="$1"
  local label="$2"
  local reason="$3"

  echo "$reason" >&2
  dump_smoke_logs "$sandbox_root" "$label"
  exit 1
}

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

need_cmd dpkg-deb
need_cmd appstreamcli
need_cmd timeout

if [[ "$SKIP_PACKAGE_BUILD" != "1" ]]; then
  need_cmd cargo
  need_cmd appimagetool
fi

seed_restore_profile() {
  local sandbox_root="$1"
  local workspace_root="$sandbox_root/workspace"
  local app_config_root="$sandbox_root/config"
  local app_data_root="$sandbox_root/data"
  local app_state_root="$sandbox_root/state"

  mkdir -p "$workspace_root/src"
  mkdir -p "$app_config_root/TerminalTiler" "$app_config_root/terminaltiler"
  mkdir -p "$app_data_root/TerminalTiler" "$app_data_root/terminaltiler"
  mkdir -p "$app_state_root/TerminalTiler/logs" "$app_state_root/terminaltiler/logs"

  for config_dir in "$app_config_root/TerminalTiler" "$app_config_root/terminaltiler"; do
    cat > "$config_dir/preferences.toml" <<'EOF'
version = 1
default_restore_mode = "shell-only"
EOF
  done

  for data_dir in "$app_data_root/TerminalTiler" "$app_data_root/terminaltiler"; do
    cat > "$data_dir/session.toml" <<EOF
version = 1
active_tab_index = 0

[[tabs]]
workspace_root = "$workspace_root"
custom_title = "Smoke Restore"
terminal_zoom_steps = 0

[tabs.preset]
id = "smoke-restore"
name = "Smoke Restore"
description = "Packaged restore smoke test"
tags = ["smoke", "restore"]
root_label = "Workspace root"
theme = "system"
density = "compact"

[tabs.preset.layout]
kind = "split"
axis = "horizontal"
ratio = 0.5

[tabs.preset.layout.first]
kind = "tile"
id = "terminal-smoke"
title = "Primary"
agent_label = "Shell"
accent_class = "accent-cyan"

[tabs.preset.layout.first.working_directory]
type = "workspace-root"

[tabs.preset.layout.second]
kind = "tile"
id = "web-smoke"
title = "Docs"
agent_label = "Browser"
accent_class = "accent-amber"
tile_kind = "web-view"
url = "https://example.com"

[tabs.preset.layout.second.working_directory]
type = "workspace-root"
EOF
  done
}

assert_restore_log() {
  local sandbox_root="$1"
  local label="$2"
  local log_path

  log_path="$(find "$sandbox_root" -name terminaltiler-session.log -print -quit)"

  if [[ -z "$log_path" || ! -f "$log_path" ]]; then
    fail_smoke "$sandbox_root" "$label" "$label did not produce a session log"
  fi
  if ! grep -E -q "restored workspace tab .*" "$log_path"; then
    fail_smoke "$sandbox_root" "$label" "$label did not restore a workspace tab"
  fi
  if ! grep -q "web tile web-smoke load event Finished uri='https://example.com/'" "$log_path"; then
    fail_smoke "$sandbox_root" "$label" "$label did not restore the web tile"
  fi
}

run_restore_smoke() {
  local label="$1"
  local sandbox_root="$2"
  shift 2

  seed_restore_profile "$sandbox_root"

  local home_root="$sandbox_root/home"
  local -a launch_command
  mkdir -p "$home_root"

  if command -v dbus-run-session >/dev/null 2>&1; then
    launch_command=(dbus-run-session -- xvfb-run -a "$@")
  else
    launch_command=(xvfb-run -a "$@")
  fi

  local status=0
  HOME="$home_root" \
  XDG_CONFIG_HOME="$sandbox_root/config" \
  XDG_DATA_HOME="$sandbox_root/data" \
  XDG_STATE_HOME="$sandbox_root/state" \
  timeout 12s "${launch_command[@]}" || status=$?

  if [[ ${status:-0} -ne 0 && ${status:-0} -ne 124 ]]; then
    fail_smoke "$sandbox_root" "$label" "$label exited with unexpected status ${status:-0}"
  fi

  assert_restore_log "$sandbox_root" "$label"
}

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

if [[ "$SKIP_PACKAGE_BUILD" != "1" ]]; then
  echo "==> building release binary"
  cd "$ROOT_DIR"
  cargo build --release

  echo "==> building Debian package"
  SKIP_CARGO_BUILD=1 bash "$ROOT_DIR/packaging/build-deb.sh"

  echo "==> building AppImage"
  SKIP_CARGO_BUILD=1 bash "$ROOT_DIR/packaging/build-appimage.sh"
else
  echo "==> using existing Linux artifacts"
fi

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
  run_restore_smoke \
    "Debian runtime" \
    "$BUILD_DIR/deb-smoke-home" \
    "$BUILD_DIR/deb/opt/terminaltiler/bin/terminaltiler"
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
  run_restore_smoke \
    "AppImage runtime" \
    "$BUILD_DIR/appimage-smoke-home" \
    "$APPIMAGE_PATH"
else
  echo "==> skipping AppImage launch smoke test because xvfb-run is unavailable"
fi

echo "release smoke test passed"
