#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_DIR="$ROOT_DIR/packaging/.build/release-smoke"
. "$ROOT_DIR/packaging/versioning.sh"
export PACKAGE_VERSION BUILD_DATE
SKIP_PACKAGE_BUILD="${SKIP_PACKAGE_BUILD:-0}"
SMOKE_PROFILE_KIND="${SMOKE_PROFILE_KIND:-mixed}"
SMOKE_LAUNCH_TIMEOUT="${SMOKE_LAUNCH_TIMEOUT:-60s}"

APPIMAGE_PATH="$(appimage_output_path)"
DEB_PATH="$(deb_output_path)"
METAINFO_PATH="$ROOT_DIR/resources/app.terminaltiler.metainfo.xml"

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


duration_to_seconds() {
  local duration="$1"

  case "$duration" in
    *s) printf '%s\n' "${duration%s}" ;;
    *m) printf '%s\n' "$(( ${duration%m} * 60 ))" ;;
    *h) printf '%s\n' "$(( ${duration%h} * 3600 ))" ;;
    ''|*[!0-9]*) printf '%s\n' "30" ;;
    *) printf '%s\n' "$duration" ;;
  esac
}

terminate_process_tree() {
  local pid="$1"
  local signal="${2:-TERM}"
  local child

  while IFS= read -r child; do
    [[ -n "$child" ]] || continue
    terminate_process_tree "$child" "$signal"
  done < <(pgrep -P "$pid" 2>/dev/null || true)

  kill "-$signal" "$pid" 2>/dev/null || true
}

log_has_restore_success() {
  local log_path="$1"
  local profile_kind="$2"

  [[ -f "$log_path" ]] || return 1
  grep -E -q "restored workspace tab .*" "$log_path" || return 1
  if [[ "$profile_kind" == "mixed" ]]; then
    grep -q "web tile web-smoke load event Finished uri='https://example.com/'" "$log_path" || return 1
  fi
}

find_first() {
  local root="$1"
  shift
  find "$root" "$@" -print -quit
}

assert_packaged_runtime_assets() {
  local runtime_root="$1"

  test -f "$runtime_root/share/glib-2.0/schemas/gschemas.compiled"
  test -f "$runtime_root/lib/gio/modules/giomodule.cache"
  test -n "$(find_first "$runtime_root/lib/gio/modules" -maxdepth 1 -name '*.so' -type f)"
  test -f "$runtime_root/libexec/webkitgtk-6.0/WebKitNetworkProcess"
  test -f "$runtime_root/libexec/webkitgtk-6.0/WebKitWebProcess"
  test -n "$(find_first "$runtime_root/libexec/webkitgtk-6.0/injected-bundle" -maxdepth 1 -name '*.so' -type f)"
}

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

need_cmd dpkg-deb
need_cmd appstreamcli
if [[ "$SKIP_PACKAGE_BUILD" != "1" ]]; then
  need_cmd cargo
  need_cmd appimagetool
fi

seed_restore_profile() {
  local sandbox_root="$1"
  local profile_kind="$2"
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
    if [[ "$profile_kind" == "terminal-only" ]]; then
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
kind = "tile"
id = "terminal-smoke"
title = "Primary"
agent_label = "Shell"
accent_class = "accent-cyan"

[tabs.preset.layout.working_directory]
type = "workspace-root"
EOF
    else
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
    fi
  done
}

assert_restore_log() {
  local sandbox_root="$1"
  local label="$2"
  local profile_kind="$3"
  local log_path

  log_path="$(find "$sandbox_root" -name terminaltiler-session.log -print -quit)"

  if [[ -z "$log_path" || ! -f "$log_path" ]]; then
    fail_smoke "$sandbox_root" "$label" "$label did not produce a session log"
  fi
  if ! grep -E -q "restored workspace tab .*" "$log_path"; then
    fail_smoke "$sandbox_root" "$label" "$label did not restore a workspace tab"
  fi
  if [[ "$profile_kind" == "mixed" ]] && ! grep -q "web tile web-smoke load event Finished uri='https://example.com/'" "$log_path"; then
    fail_smoke "$sandbox_root" "$label" "$label did not restore the web tile"
  fi
}

run_restore_smoke() {
  local label="$1"
  local sandbox_root="$2"
  local profile_kind="$3"
  shift 3

  seed_restore_profile "$sandbox_root" "$profile_kind"

  local home_root="$sandbox_root/home"
  local runtime_root="$sandbox_root/run"
  local portal_root="$sandbox_root/portals"
  local -a launch_command
  mkdir -p "$home_root" "$runtime_root" "$portal_root"
  chmod 700 "$runtime_root"

  if command -v dbus-run-session >/dev/null 2>&1; then
    launch_command=(dbus-run-session -- xvfb-run -a "$@")
  else
    launch_command=(xvfb-run -a "$@")
  fi

  local status_file="$sandbox_root/launch-status"
  local timeout_seconds
  local deadline
  local launch_pid
  local log_path=""
  local status

  rm -f "$status_file"
  timeout_seconds="$(duration_to_seconds "$SMOKE_LAUNCH_TIMEOUT")"
  deadline=$((SECONDS + timeout_seconds))

  (
    set +e
    HOME="$home_root" \
    XDG_CONFIG_HOME="$sandbox_root/config" \
    XDG_DATA_HOME="$sandbox_root/data" \
    XDG_STATE_HOME="$sandbox_root/state" \
    XDG_RUNTIME_DIR="$runtime_root" \
    XDG_DESKTOP_PORTAL_DIR="$portal_root" \
    GIO_USE_VFS=local \
    GSETTINGS_BACKEND=memory \
    GTK_USE_PORTAL=0 \
    NO_AT_BRIDGE=1 \
    "${launch_command[@]}"
    printf '%s\n' "$?" > "$status_file"
  ) &
  launch_pid=$!

  while (( SECONDS < deadline )); do
    log_path="$(find "$sandbox_root" -name terminaltiler-session.log -print -quit)"
    if [[ -n "$log_path" ]] && log_has_restore_success "$log_path" "$profile_kind"; then
      terminate_process_tree "$launch_pid" TERM
      wait "$launch_pid" 2>/dev/null || true
      assert_restore_log "$sandbox_root" "$label" "$profile_kind"
      return
    fi

    if [[ -f "$status_file" ]]; then
      status="$(cat "$status_file")"
      wait "$launch_pid" 2>/dev/null || true
      if [[ "$status" -ne 0 ]]; then
        fail_smoke "$sandbox_root" "$label" "$label exited with unexpected status $status before restore completed"
      fi
      fail_smoke "$sandbox_root" "$label" "$label exited before restore completed"
    fi

    sleep 0.25
  done

  terminate_process_tree "$launch_pid" TERM
  sleep 0.5
  terminate_process_tree "$launch_pid" KILL
  wait "$launch_pid" 2>/dev/null || true
  fail_smoke "$sandbox_root" "$label" "$label did not complete restore within $SMOKE_LAUNCH_TIMEOUT"
}

validate_appstream() {
  # appstreamcli exits non-zero for advisory warnings/infos too (e.g. the
  # 2-segment rDNS hint for app.terminaltiler, or the deprecated
  # <developer_name> kept for older consumers). Mirror the packaging trust
  # gate in validate-metadata.sh: print the full report but only fail the
  # release on hard errors ("E:").
  local output
  output="$(appstreamcli validate --no-net "$METAINFO_PATH" 2>&1)" || true
  printf '%s\n' "$output"
  if printf '%s\n' "$output" | grep -q '^E:'; then
    echo "AppStream validation reported errors in $(basename "$METAINFO_PATH")" >&2
    exit 1
  fi
}

echo "==> validating AppStream metadata"
validate_appstream

echo "==> release version $PACKAGE_VERSION"

if [[ "$SKIP_PACKAGE_BUILD" != "1" ]]; then
  echo "==> building release binary"
  cd "$ROOT_DIR"
  cargo build --release --features voice-cpal

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
test -f "$BUILD_DIR/deb/usr/share/metainfo/app.terminaltiler.metainfo.xml"
test -f "$BUILD_DIR/deb/usr/share/applications/app.terminaltiler.desktop"
test -f "$BUILD_DIR/deb/usr/share/icons/hicolor/128x128/apps/terminaltiler.png"
test -f "$BUILD_DIR/deb/opt/terminaltiler/lib/libgtk-4.so.1"
test -f "$BUILD_DIR/deb/opt/terminaltiler/lib/libadwaita-1.so.0"
test -f "$BUILD_DIR/deb/opt/terminaltiler/lib/libvte-2.91-gtk4.so.0"
assert_packaged_runtime_assets "$BUILD_DIR/deb/opt/terminaltiler"

if command -v xvfb-run >/dev/null 2>&1; then
  echo "==> smoke-testing extracted Debian runtime"
  run_restore_smoke \
    "Debian runtime" \
    "$BUILD_DIR/deb-smoke-home" \
    "$SMOKE_PROFILE_KIND" \
    "$BUILD_DIR/deb/opt/terminaltiler/bin/terminaltiler"
else
  echo "==> skipping Debian runtime launch smoke test because xvfb-run is unavailable"
fi

echo "==> checking AppImage payload"
cd "$BUILD_DIR/appimage"
"$APPIMAGE_PATH" --appimage-extract >/dev/null
test -f "$BUILD_DIR/appimage/squashfs-root/usr/share/metainfo/app.terminaltiler.metainfo.xml"
test -f "$BUILD_DIR/appimage/squashfs-root/usr/share/applications/app.terminaltiler.desktop"
test -f "$BUILD_DIR/appimage/squashfs-root/usr/share/icons/hicolor/128x128/apps/terminaltiler.png"
test -f "$BUILD_DIR/appimage/squashfs-root/usr/lib/libgtk-4.so.1"
test -f "$BUILD_DIR/appimage/squashfs-root/usr/lib/libadwaita-1.so.0"
test -f "$BUILD_DIR/appimage/squashfs-root/usr/lib/libvte-2.91-gtk4.so.0"
assert_packaged_runtime_assets "$BUILD_DIR/appimage/squashfs-root/usr"

if command -v xvfb-run >/dev/null 2>&1; then
  echo "==> smoke-testing AppImage launch"
  cd "$ROOT_DIR"
  run_restore_smoke \
    "AppImage runtime" \
    "$BUILD_DIR/appimage-smoke-home" \
    "$SMOKE_PROFILE_KIND" \
    "$APPIMAGE_PATH"
else
  echo "==> skipping AppImage launch smoke test because xvfb-run is unavailable"
fi

echo "release smoke test passed"
