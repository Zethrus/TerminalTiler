#!/usr/bin/env bash
# Shared desktop-entry + AppStream metadata validation for TerminalTiler packaging.
#
# Source this file and call:
#   validate_app_metadata <desktop_file> <metainfo_file>
#
# Trustworthy software-center metadata is the whole point of these checks:
# malformed desktop or AppStream files are silently dropped by GNOME Software /
# Ubuntu App Center, which is how publisher/license/icon end up "Unknown".
#
# Validators are treated as optional so minimal build hosts still produce
# artifacts, but when a validator is present a hard error fails the build.

if [[ -n "${TERMINALTILER_VALIDATE_METADATA_LOADED:-}" ]]; then
  # shellcheck disable=SC2317 # exit fallback only used if executed directly.
  return 0 2>/dev/null || exit 0
fi
TERMINALTILER_VALIDATE_METADATA_LOADED=1

validate_app_metadata() {
  local desktop_file="$1"
  local metainfo_file="$2"

  if command -v desktop-file-validate >/dev/null 2>&1; then
    echo "validating desktop entry: $(basename "$desktop_file")"
    desktop-file-validate "$desktop_file"
  else
    echo "note: desktop-file-validate not found; skipping desktop entry validation" >&2
    echo "      install desktop-file-utils to enable this trust check" >&2
  fi

  if command -v appstreamcli >/dev/null 2>&1; then
    echo "validating AppStream metadata: $(basename "$metainfo_file")"
    # appstreamcli exits non-zero for advisory warnings/infos too (e.g. the
    # 2-segment rDNS hint for app.terminaltiler, or the deprecated
    # <developer_name> kept for older consumers). Those are acceptable, so the
    # gate prints the full report but only fails on hard errors ("E:").
    local report
    report="$(appstreamcli validate --no-net "$metainfo_file" 2>&1)" || true
    printf '%s\n' "$report"
    if printf '%s\n' "$report" | grep -q '^E:'; then
      echo "AppStream validation reported errors in $(basename "$metainfo_file")" >&2
      return 1
    fi
  else
    echo "note: appstreamcli not found; skipping AppStream validation" >&2
    echo "      install the appstream package to enable this trust check" >&2
  fi
}
