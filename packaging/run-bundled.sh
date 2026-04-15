#!/usr/bin/env bash
set -euo pipefail

APP_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LIB_DIR="$APP_ROOT/lib"
SHARE_DIR="$APP_ROOT/share"
GTK_MODULE_BASE="$LIB_DIR/gtk-4.0"
WEBKIT_EXEC_DIR="$APP_ROOT/libexec/webkitgtk-6.0"
WEBKIT_INJECTED_BUNDLE_DIR="$WEBKIT_EXEC_DIR/injected-bundle"
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
export GTK_DATA_PREFIX="$APP_ROOT"
export GTK_EXE_PREFIX="$APP_ROOT"
export GTK_PATH="$GTK_MODULE_BASE"
export WEBKIT_EXEC_PATH="$WEBKIT_EXEC_DIR"
export WEBKIT_INJECTED_BUNDLE_PATH="$WEBKIT_INJECTED_BUNDLE_DIR"

printf '[%s] launcher runtime lib_dir=%s runpath=embedded\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  "$LIB_DIR" >&2

if [[ -z "${WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS:-}" ]]; then
  webkit_sandbox_reasons=()
  if [[ -r /proc/sys/kernel/unprivileged_userns_clone ]] && [[ "$(tr -d '[:space:]' </proc/sys/kernel/unprivileged_userns_clone)" == "0" ]]; then
    webkit_sandbox_reasons+=("kernel.unprivileged_userns_clone=0")
  fi
  if [[ -r /proc/sys/user/max_user_namespaces ]] && [[ "$(tr -d '[:space:]' </proc/sys/user/max_user_namespaces)" == "0" ]]; then
    webkit_sandbox_reasons+=("user.max_user_namespaces=0")
  fi
  if [[ -r /proc/sys/kernel/apparmor_restrict_unprivileged_userns ]] && [[ "$(tr -d '[:space:]' </proc/sys/kernel/apparmor_restrict_unprivileged_userns)" == "1" ]]; then
    webkit_sandbox_reasons+=("kernel.apparmor_restrict_unprivileged_userns=1")
  fi

  if (( ${#webkit_sandbox_reasons[@]} > 0 )); then
    export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
    printf '[%s] launcher disabled WebKit sandbox because %s\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
      "$(IFS=', '; echo "${webkit_sandbox_reasons[*]}")" >&2
  fi
fi

for candidate in "$LIB_DIR"/gdk-pixbuf-2.0/*; do
  if [[ -d "$candidate/loaders" ]]; then
    export GDK_PIXBUF_MODULEDIR="$candidate/loaders"
    if [[ -f "$candidate/loaders.cache" ]]; then
      export GDK_PIXBUF_MODULE_FILE="$candidate/loaders.cache"
    fi
    break
  fi
done

if [[ -d "$WEBKIT_EXEC_DIR" ]]; then
  printf '[%s] launcher webkit_exec=%s injected_bundle=%s\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    "$WEBKIT_EXEC_DIR" \
    "$WEBKIT_INJECTED_BUNDLE_DIR" >&2
fi

exec "$APP_ROOT/bin/terminaltiler-bin" "$@"
