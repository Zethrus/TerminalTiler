#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

require_clean_tree() {
  if ! git -C "$ROOT_DIR" diff --quiet --ignore-submodules HEAD --; then
    echo "release verification requires a clean working tree" >&2
    exit 1
  fi

  if ! git -C "$ROOT_DIR" diff --cached --quiet --ignore-submodules --; then
    echo "release verification requires an empty index" >&2
    exit 1
  fi

  if [[ -n "$(git -C "$ROOT_DIR" ls-files --others --exclude-standard)" ]]; then
    echo "release verification requires no untracked files" >&2
    exit 1
  fi
}

echo "==> verifying release prerequisites"
require_clean_tree

echo "==> building release artifacts from the pinned container baseline"
bash "$ROOT_DIR/packaging/build-in-container.sh"

echo "release verification passed"
