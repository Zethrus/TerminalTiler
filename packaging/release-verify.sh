#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
. "$ROOT_DIR/packaging/versioning.sh"
export PACKAGE_VERSION BUILD_DATE

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

echo "==> validating the pinned container-built artifacts"
SMOKE_PROFILE_KIND=terminal-only SKIP_PACKAGE_BUILD=1 bash "$ROOT_DIR/packaging/release-smoke-test.sh"

echo "release verification passed"
