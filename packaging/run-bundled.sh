#!/usr/bin/env bash
set -euo pipefail

APP_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LIB_DIR="$APP_ROOT/lib"
SHARE_DIR="$APP_ROOT/share"
if [[ -n "${XDG_STATE_HOME:-}" ]]; then
  :
elif [[ -n "${HOME:-}" ]]; then
  XDG_STATE_HOME="$HOME/.local/state"
else
  echo "TerminalTiler launcher requires HOME or XDG_STATE_HOME" >&2
  exit 1
fi
LOG_DIR="$XDG_STATE_HOME/terminaltiler/logs"
BOOT_LOG="$LOG_DIR/launcher-stderr.log"

mkdir -p "$LOG_DIR"
exec 2>>"$BOOT_LOG"

printf '[%s] launcher start app_root=%s argc=%s\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  "$APP_ROOT" \
  "$#" >&2

export GSETTINGS_SCHEMA_DIR="$SHARE_DIR/glib-2.0/schemas"
export XDG_DATA_DIRS="$SHARE_DIR${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"

printf '[%s] launcher runtime lib_dir=%s runpath=embedded\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  "$LIB_DIR" >&2

for candidate in "$LIB_DIR"/gdk-pixbuf-2.0/*; do
  if [[ -d "$candidate/loaders" ]]; then
    export GDK_PIXBUF_MODULEDIR="$candidate/loaders"
    if [[ -f "$candidate/loaders.cache" ]]; then
      export GDK_PIXBUF_MODULE_FILE="$candidate/loaders.cache"
    fi
    break
  fi
done

exec "$APP_ROOT/bin/terminaltiler-bin" "$@"
