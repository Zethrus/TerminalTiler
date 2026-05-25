#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXE_PATH=""
OUTPUT_DIR="$ROOT_DIR/packaging/.build/linux-gtk-visuals"
CAPTURE_SET="launch-dashboard,saved-workspaces,restored-workspace,workspace-with-web"
THEME="dark"
DENSITY="compact"
STARTUP_TIMEOUT_SECONDS=20
KEEP_PROCESS=0

usage() {
  cat <<'EOF'
Usage: capture-linux-gtk-visuals.sh [options]

Capture Ubuntu/Linux GTK reference screenshots with the same seeded profiles as
the Windows GTK capture helper.

Options:
  --exe PATH                  TerminalTiler executable (default: target/release/terminaltiler)
  --output-dir DIR            Output directory (default: packaging/.build/linux-gtk-visuals)
  --capture-set CSV           launch-dashboard,saved-workspaces,restored-workspace,workspace-with-web
                              (default: all)
  --theme system|light|dark   Seeded app theme (default: dark)
  --density comfortable|standard|compact
                              Seeded app density (default: compact)
  --startup-timeout SECONDS   Seconds to wait for app windows (default: 20)
  --keep-process              Leave TerminalTiler running after capture
  -h, --help                  Show this help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --exe)
      EXE_PATH="${2:?--exe requires a path}"
      shift 2
      ;;
    --output-dir)
      OUTPUT_DIR="${2:?--output-dir requires a path}"
      shift 2
      ;;
    --capture-set)
      CAPTURE_SET="${2:?--capture-set requires a comma-separated value}"
      shift 2
      ;;
    --theme)
      THEME="${2:?--theme requires a value}"
      shift 2
      ;;
    --density)
      DENSITY="${2:?--density requires a value}"
      shift 2
      ;;
    --startup-timeout)
      STARTUP_TIMEOUT_SECONDS="${2:?--startup-timeout requires seconds}"
      shift 2
      ;;
    --keep-process)
      KEEP_PROCESS=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$THEME" in
  system|light|dark) ;;
  *) echo "--theme must be system, light, or dark" >&2; exit 2 ;;
esac

case "$DENSITY" in
  comfortable|standard|compact) ;;
  *) echo "--density must be comfortable, standard, or compact" >&2; exit 2 ;;
esac

if [[ -z "$EXE_PATH" ]]; then
  EXE_PATH="$ROOT_DIR/target/release/terminaltiler"
fi

if [[ ! -x "$EXE_PATH" ]]; then
  echo "TerminalTiler executable was not found or is not executable at $EXE_PATH" >&2
  echo "Build it first with: cargo build --release --features voice-cpal" >&2
  exit 1
fi

if ! command -v xdotool >/dev/null 2>&1; then
  echo "xdotool is required to locate TerminalTiler windows for capture." >&2
  exit 1
fi

if ! command -v import >/dev/null 2>&1 && ! command -v gnome-screenshot >/dev/null 2>&1; then
  echo "ImageMagick 'import' or gnome-screenshot is required to capture windows." >&2
  exit 1
fi

toml_path() {
  local path="$1"
  printf '%s' "${path//\\/\\\\}"
}

safe_name() {
  local value="$1"
  value="$(printf '%s' "$value" | tr -cs 'A-Za-z0-9._-' '-')"
  value="${value#-}"
  value="${value%-}"
  if [[ -z "$value" ]]; then
    value="window"
  fi
  printf '%s' "$value"
}

write_visual_profile() {
  local sandbox_root="$1"
  local scenario="$2"
  local workspace_root="$sandbox_root/workspace"
  local profile_root="$sandbox_root/profile"
  local config_root="$profile_root/config"
  local data_root="$profile_root/data"
  local logs_root="$profile_root/state/logs"
  local restore_mode="prompt"

  if [[ "$scenario" == "restored-workspace" || "$scenario" == "workspace-with-web" ]]; then
    restore_mode="shell-only"
  fi

  mkdir -p "$workspace_root" "$config_root" "$data_root" "$logs_root"

  cat >"$config_root/preferences.toml" <<EOF
version = 1
default_restore_mode = "$restore_mode"
default_theme = "$THEME"
default_density = "$DENSITY"
EOF

  local workspace_path
  workspace_path="$(toml_path "$workspace_root")"

  if [[ "$scenario" == "saved-workspaces" ]]; then
    cat >"$config_root/presets.toml" <<EOF
version = 1

[[presets]]
id = "visual-qa-saved-fleet"
name = "Visual QA Saved Fleet"
description = "Seeded saved workspace card for Linux and Windows visual parity review."
tags = ["visual", "qa", "saved"]
root_label = "QA workspace"
workspace_root = "$workspace_path"
theme = "$THEME"
density = "$DENSITY"

[presets.layout]
kind = "split"
axis = "horizontal"
ratio = 0.55

[presets.layout.first]
kind = "tile"
id = "saved-builder"
title = "Builder"
agent_label = "Build"
accent_class = "accent-cyan"

[presets.layout.first.working_directory]
type = "workspace-root"

[presets.layout.second]
kind = "tile"
id = "saved-reviewer"
title = "Reviewer"
agent_label = "QA"
accent_class = "accent-rose"

[presets.layout.second.working_directory]
type = "workspace-root"

[[presets]]
id = "visual-qa-docs-shell"
name = "Visual QA Docs + Shell"
description = "Seeded web plus terminal card to expose saved tile badges and actions."
tags = ["visual", "web", "shell"]
root_label = "Docs workspace"
workspace_root = "$workspace_path"
theme = "$THEME"
density = "$DENSITY"

[presets.layout]
kind = "split"
axis = "vertical"
ratio = 0.48

[presets.layout.first]
kind = "tile"
id = "saved-docs"
title = "Docs"
agent_label = "Browser"
accent_class = "accent-violet"
tile_kind = "web-view"
url = "about:blank"

[presets.layout.first.working_directory]
type = "workspace-root"

[presets.layout.second]
kind = "tile"
id = "saved-shell"
title = "Shell"
agent_label = "Terminal"
accent_class = "accent-amber"

[presets.layout.second.working_directory]
type = "workspace-root"
EOF
  fi

  if [[ "$scenario" == "restored-workspace" ]]; then
    cat >"$data_root/session.toml" <<EOF
version = 1
active_tab_index = 0

[[tabs]]
workspace_root = "$workspace_path"
custom_title = "Visual QA Restore"
terminal_zoom_steps = 0

[tabs.preset]
id = "visual-qa-restore"
name = "Visual QA Restore"
description = "Visual QA restored workspace"
tags = ["visual", "qa"]
root_label = "Workspace root"
theme = "$THEME"
density = "$DENSITY"

[tabs.preset.layout]
kind = "split"
axis = "horizontal"
ratio = 0.5

[tabs.preset.layout.first]
kind = "tile"
id = "terminal-primary"
title = "Primary"
agent_label = "Shell"
accent_class = "accent-cyan"

[tabs.preset.layout.first.working_directory]
type = "workspace-root"

[tabs.preset.layout.second]
kind = "split"
axis = "vertical"
ratio = 0.5

[tabs.preset.layout.second.first]
kind = "tile"
id = "terminal-secondary"
title = "Secondary"
agent_label = "Agent"
accent_class = "accent-purple"

[tabs.preset.layout.second.first.working_directory]
type = "workspace-root"

[tabs.preset.layout.second.second]
kind = "tile"
id = "terminal-logs"
title = "Logs"
agent_label = "Monitor"
accent_class = "accent-amber"

[tabs.preset.layout.second.second.working_directory]
type = "workspace-root"
EOF
  fi

  if [[ "$scenario" == "workspace-with-web" ]]; then
    cat >"$data_root/session.toml" <<EOF
version = 1
active_tab_index = 0

[[tabs]]
workspace_root = "$workspace_path"
custom_title = "Visual QA Web Workspace"
terminal_zoom_steps = 0

[tabs.preset]
id = "visual-qa-web-workspace"
name = "Visual QA Web Workspace"
description = "Visual QA restored web and terminal workspace"
tags = ["visual", "qa", "web"]
root_label = "Workspace root"
theme = "$THEME"
density = "$DENSITY"

[tabs.preset.layout]
kind = "split"
axis = "horizontal"
ratio = 0.52

[tabs.preset.layout.first]
kind = "tile"
id = "terminal-control"
title = "Control"
agent_label = "Shell"
accent_class = "accent-cyan"

[tabs.preset.layout.first.working_directory]
type = "workspace-root"

[tabs.preset.layout.second]
kind = "tile"
id = "web-docs"
title = "Docs"
agent_label = "Browser"
accent_class = "accent-violet"
tile_kind = "web-view"
url = "about:blank"

[tabs.preset.layout.second.working_directory]
type = "workspace-root"
EOF
  fi
}

descendant_pids() {
  local root_pid="$1"
  local children child
  children="$(pgrep -P "$root_pid" 2>/dev/null || true)"
  for child in $children; do
    printf '%s\n' "$child"
    descendant_pids "$child"
  done
}

process_tree_pids() {
  local root_pid="$1"
  printf '%s\n' "$root_pid"
  descendant_pids "$root_pid"
}

window_ids_for_process_tree() {
  local root_pid="$1"
  local pid
  process_tree_pids "$root_pid" | while read -r pid; do
    [[ -n "$pid" ]] || continue
    xdotool search --onlyvisible --pid "$pid" 2>/dev/null || true
  done | awk '!seen[$0]++'
}

wait_for_windows() {
  local root_pid="$1"
  local deadline=$((SECONDS + STARTUP_TIMEOUT_SECONDS))
  local windows
  while (( SECONDS < deadline )); do
    if ! kill -0 "$root_pid" 2>/dev/null; then
      # The direct portable on Windows can exit after spawning descendants;
      # Linux should not, but check descendants before failing for symmetry.
      if [[ -z "$(descendant_pids "$root_pid")" ]]; then
        echo "TerminalTiler exited before visual capture." >&2
        return 1
      fi
    fi
    windows="$(window_ids_for_process_tree "$root_pid")"
    if [[ -n "$windows" ]]; then
      printf '%s\n' "$windows"
      return 0
    fi
    sleep 0.25
  done
  echo "Timed out waiting for TerminalTiler windows" >&2
  return 1
}

capture_window() {
  local window_id="$1"
  local path="$2"

  if command -v import >/dev/null 2>&1; then
    import -window "$window_id" "$path"
  else
    xdotool windowactivate --sync "$window_id"
    gnome-screenshot -w -f "$path"
  fi
}

stop_process_tree() {
  local root_pid="$1"
  local pids
  mapfile -t pids < <(process_tree_pids "$root_pid" | tac)
  for pid in "${pids[@]}"; do
    kill "$pid" 2>/dev/null || true
  done
  sleep 0.5
  for pid in "${pids[@]}"; do
    kill -9 "$pid" 2>/dev/null || true
  done
}

capture_scenario() {
  local scenario="$1"
  local scenario_root="$OUTPUT_DIR/$scenario"
  local sandbox_root="$scenario_root/sandbox"
  local capture_root="$scenario_root/captures"
  local pid windows window_id index title safe_title path

  rm -rf "$sandbox_root" "$capture_root"
  mkdir -p "$capture_root"
  write_visual_profile "$sandbox_root" "$scenario"
  mkdir -p "$sandbox_root/home"

  TERMINALTILER_PROFILE_ROOT="$sandbox_root/profile" \
  HOME="$sandbox_root/home" \
  "$EXE_PATH" &
  pid=$!

  if ! windows="$(wait_for_windows "$pid")"; then
    if (( KEEP_PROCESS == 0 )); then
      stop_process_tree "$pid"
    fi
    return 1
  fi

  # Give GTK one more frame after the first window appears before capture.
  sleep 1

  index=0
  while read -r window_id; do
    [[ -n "$window_id" ]] || continue
    title="$(xdotool getwindowname "$window_id" 2>/dev/null || printf 'window')"
    safe_title="$(safe_name "$title")"
    path="$capture_root/$(printf '%02d-%s-%s-%s-%s.png' "$index" "$scenario" "$THEME" "$DENSITY" "$safe_title")"
    capture_window "$window_id" "$path"
    echo "Captured $path"
    index=$((index + 1))
  done <<<"$windows"

  if (( KEEP_PROCESS == 0 )); then
    stop_process_tree "$pid"
  fi
}

mkdir -p "$OUTPUT_DIR"
IFS=',' read -r -a scenarios <<<"$CAPTURE_SET"
for scenario in "${scenarios[@]}"; do
  case "$scenario" in
    launch-dashboard|saved-workspaces|restored-workspace|workspace-with-web) capture_scenario "$scenario" ;;
    *) echo "Unknown capture scenario: $scenario" >&2; exit 2 ;;
  esac
done

echo "Linux GTK visual captures written to $OUTPUT_DIR"
