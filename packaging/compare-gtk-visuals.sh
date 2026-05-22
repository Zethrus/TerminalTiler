#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LINUX_DIR="$ROOT_DIR/packaging/.build/linux-gtk-visuals"
WINDOWS_DIR="$ROOT_DIR/packaging/.build/windows-gtk-visuals"
OUTPUT_DIR="$ROOT_DIR/packaging/.build/gtk-visual-diffs"
CAPTURE_SET="launch-dashboard,restored-workspace"
THEME="dark"
DENSITY="compact"
THRESHOLD="0.035"

usage() {
  cat <<'EOF'
Usage: compare-gtk-visuals.sh [options]

Compare Linux GTK reference captures with Windows GTK captures. The capture
helpers intentionally write matching <index>-<scenario>-<theme>-<density>-*.png names;
this verifier pairs by index/scenario/theme/density and ignores OS-specific window
title suffixes.

Options:
  --linux-dir DIR             Linux captures root (default: packaging/.build/linux-gtk-visuals)
  --windows-dir DIR           Windows captures root (default: packaging/.build/windows-gtk-visuals)
  --output-dir DIR            Diff/report output root (default: packaging/.build/gtk-visual-diffs)
  --capture-set CSV           launch-dashboard,restored-workspace (default: both)
  --theme system|light|dark   Capture theme to compare (default: dark)
  --density comfortable|standard|compact
                              Capture density to compare (default: compact)
  --threshold FLOAT           Max normalized RMSE allowed (default: 0.035)
  -h, --help                  Show this help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --linux-dir)
      LINUX_DIR="${2:?--linux-dir requires a directory}"
      shift 2
      ;;
    --windows-dir)
      WINDOWS_DIR="${2:?--windows-dir requires a directory}"
      shift 2
      ;;
    --output-dir)
      OUTPUT_DIR="${2:?--output-dir requires a directory}"
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
    --threshold)
      THRESHOLD="${2:?--threshold requires a float}"
      shift 2
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

if ! command -v compare >/dev/null 2>&1; then
  echo "ImageMagick 'compare' is required for GTK visual diffs." >&2
  exit 1
fi

if ! command -v identify >/dev/null 2>&1; then
  echo "ImageMagick 'identify' is required for GTK visual diffs." >&2
  exit 1
fi

shopt -s nullglob

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"
REPORT_PATH="$OUTPUT_DIR/report.tsv"
printf 'scenario\tindex\ttheme\tdensity\tstatus\tnormalized_rmse\tlinux\twindows\tdiff\n' >"$REPORT_PATH"

failures=0

find_single_capture() {
  local root="$1"
  local scenario="$2"
  local index="$3"
  local matches=("$root/$scenario/captures/$index-$scenario-$THEME-$DENSITY-"*.png)

  if (( ${#matches[@]} == 0 )); then
    return 1
  fi
  if (( ${#matches[@]} > 1 )); then
    echo "Multiple captures matched $root/$scenario/captures/$index-$scenario-$THEME-$DENSITY-*.png" >&2
    printf '%s\n' "${matches[@]}" >&2
    return 2
  fi

  printf '%s' "${matches[0]}"
}

normalized_rmse_from_output() {
  local output="$1"
  if [[ "$output" =~ \(([0-9.]+([eE][-+]?[0-9]+)?)\) ]]; then
    printf '%s' "${BASH_REMATCH[1]}"
  else
    printf '1'
  fi
}

float_leq() {
  awk -v left="$1" -v right="$2" 'BEGIN { exit !(left <= right) }'
}

compare_pair() {
  local scenario="$1"
  local index="$2"
  local linux_png="$3"
  local windows_png="$4"
  local scenario_diff_dir="$OUTPUT_DIR/$scenario"
  local diff_png="$scenario_diff_dir/$index-$scenario-$THEME-$DENSITY-diff.png"
  local status="pass"
  local rmse="1"
  local compare_output

  mkdir -p "$scenario_diff_dir"

  local linux_size windows_size
  linux_size="$(identify -format '%wx%h' "$linux_png")"
  windows_size="$(identify -format '%wx%h' "$windows_png")"
  if [[ "$linux_size" != "$windows_size" ]]; then
    status="fail-dimensions"
    failures=$((failures + 1))
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      "$scenario" "$index" "$THEME" "$DENSITY" "$status" "$rmse" "$linux_png" "$windows_png" "$diff_png" >>"$REPORT_PATH"
    echo "FAIL $scenario#$index dimensions differ: linux=$linux_size windows=$windows_size"
    return
  fi

  set +e
  compare_output="$(compare -metric RMSE "$linux_png" "$windows_png" "$diff_png" 2>&1)"
  local compare_status=$?
  set -e
  rmse="$(normalized_rmse_from_output "$compare_output")"

  if (( compare_status > 1 )); then
    status="fail-compare"
    failures=$((failures + 1))
  elif ! float_leq "$rmse" "$THRESHOLD"; then
    status="fail-threshold"
    failures=$((failures + 1))
  fi

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$scenario" "$index" "$THEME" "$DENSITY" "$status" "$rmse" "$linux_png" "$windows_png" "$diff_png" >>"$REPORT_PATH"

  if [[ "$status" == pass ]]; then
    echo "PASS $scenario#$index normalized_rmse=$rmse"
  else
    echo "FAIL $scenario#$index status=$status normalized_rmse=$rmse"
  fi
}

IFS=',' read -r -a scenarios <<<"$CAPTURE_SET"
for scenario in "${scenarios[@]}"; do
  case "$scenario" in
    launch-dashboard|restored-workspace) ;;
    *) echo "Unknown capture scenario: $scenario" >&2; exit 2 ;;
  esac

  linux_files=("$LINUX_DIR/$scenario/captures/"??-"$scenario"-"$THEME"-"$DENSITY"-*.png)
  if (( ${#linux_files[@]} == 0 )); then
    echo "No Linux captures found for $scenario/$THEME/$DENSITY under $LINUX_DIR" >&2
    failures=$((failures + 1))
    continue
  fi

  for linux_png in "${linux_files[@]}"; do
    base="$(basename "$linux_png")"
    index="${base%%-*}"
    if ! windows_png="$(find_single_capture "$WINDOWS_DIR" "$scenario" "$index")"; then
      echo "Missing Windows capture for $scenario#$index theme=$THEME density=$DENSITY" >&2
      failures=$((failures + 1))
      printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$scenario" "$index" "$THEME" "$DENSITY" "fail-missing-windows" "1" "$linux_png" "" "" >>"$REPORT_PATH"
      continue
    fi
    compare_pair "$scenario" "$index" "$linux_png" "$windows_png"
  done
done

echo "GTK visual comparison report written to $REPORT_PATH"

if (( failures > 0 )); then
  echo "GTK visual comparison failed with $failures failing pair(s)." >&2
  exit 1
fi

echo "GTK visual comparison passed."
