#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
. "$ROOT_DIR/packaging/versioning.sh"

DEFAULT_BRANCH="${DEFAULT_BRANCH:-main}"
DRY_RUN=0
VERIFY_BEFORE_TAG=1

usage() {
  cat <<EOF
Usage: bash packaging/create-release-tag.sh [--dry-run] [--skip-verify]

Options:
  --dry-run      Print the next release tag without creating it.
  --skip-verify  Skip packaging/release-verify.sh before tagging.
  --help         Show this help text.
EOF
}

require_git_checkout() {
  if ! git -C "$ROOT_DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "release tagging must be run from a git checkout" >&2
    exit 1
  fi
}

fetch_release_refs() {
  git -C "$ROOT_DIR" fetch --quiet --tags origin "$DEFAULT_BRANCH"
}

require_default_branch_checkout() {
  local current_branch

  current_branch="$(git -C "$ROOT_DIR" branch --show-current)"
  if [[ -z "$current_branch" ]]; then
    if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
      return 0
    fi

    echo "release tagging requires checking out ${DEFAULT_BRANCH}" >&2
    exit 1
  fi

  if [[ "$current_branch" != "$DEFAULT_BRANCH" ]]; then
    echo "release tagging only runs from ${DEFAULT_BRANCH}" >&2
    exit 1
  fi
}

require_clean_tree() {
  if ! git -C "$ROOT_DIR" diff --quiet --ignore-submodules HEAD --; then
    echo "release tagging requires a clean working tree" >&2
    exit 1
  fi

  if ! git -C "$ROOT_DIR" diff --cached --quiet --ignore-submodules --; then
    echo "release tagging requires an empty index" >&2
    exit 1
  fi

  if [[ -n "$(git -C "$ROOT_DIR" ls-files --others --exclude-standard)" ]]; then
    echo "release tagging requires no untracked files" >&2
    exit 1
  fi
}

require_synced_with_origin() {
  local local_head
  local remote_head

  if ! remote_head="$(git -C "$ROOT_DIR" rev-parse "refs/remotes/origin/${DEFAULT_BRANCH}" 2>/dev/null)"; then
    echo "could not resolve origin/${DEFAULT_BRANCH}; run git fetch origin ${DEFAULT_BRANCH} --tags" >&2
    exit 1
  fi

  local_head="$(git -C "$ROOT_DIR" rev-parse HEAD)"
  if [[ "$local_head" != "$remote_head" ]]; then
    echo "release tagging requires HEAD to match origin/${DEFAULT_BRANCH}; run git pull --ff-only" >&2
    exit 1
  fi
}

ensure_tag_does_not_exist() {
  local tag="$1"

  if git -C "$ROOT_DIR" rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
    echo "tag ${tag} already exists" >&2
    exit 1
  fi
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --dry-run)
        DRY_RUN=1
        ;;
      --skip-verify)
        VERIFY_BEFORE_TAG=0
        ;;
      --help|-h)
        usage
        exit 0
        ;;
      *)
        echo "unknown option: $1" >&2
        usage >&2
        exit 1
        ;;
    esac
    shift
  done
}

main() {
  local next_version
  local next_tag

  parse_args "$@"
  require_git_checkout
  fetch_release_refs

  next_version="$(next_release_version_for_base_version "$BASE_VERSION")"
  next_tag="$(release_tag_for_version "$next_version")"
  ensure_tag_does_not_exist "$next_tag"

  echo "next release version: ${next_version}"
  echo "next release tag: ${next_tag}"

  if [[ "$DRY_RUN" == "1" ]]; then
    return 0
  fi

  require_default_branch_checkout
  require_clean_tree
  require_synced_with_origin

  if [[ "$VERIFY_BEFORE_TAG" == "1" ]]; then
    bash "$ROOT_DIR/packaging/release-verify.sh"
  fi

  git -C "$ROOT_DIR" tag -a "$next_tag" -m "Release ${next_tag}"
  git -C "$ROOT_DIR" push origin "$next_tag"

  echo "created and pushed ${next_tag}"
}

main "$@"