#!/usr/bin/env bash

if [[ -n "${TERMINALTILER_VERSIONING_LOADED:-}" ]]; then
  return 0
fi
TERMINALTILER_VERSIONING_LOADED=1

ROOT_DIR="${ROOT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
VERSION_STATE_DIR="$ROOT_DIR/packaging/.build/versioning"
LAST_SUCCESSFUL_VERSION_FILE="$VERSION_STATE_DIR/last-successful-version"
DIST_DIR="$ROOT_DIR/dist"

BASE_VERSION="$({
  awk -F'"' '
    /^\[package\]$/ { in_package = 1; next }
    /^\[/ && $0 != "[package]" { in_package = 0 }
    in_package && /^version = "/ { print $2; exit }
  ' "$ROOT_DIR/Cargo.toml"
})"

if [[ -z "$BASE_VERSION" ]]; then
  echo "failed to read package version from $ROOT_DIR/Cargo.toml" >&2
  exit 1
fi

ensure_version_state_dir() {
  mkdir -p "$VERSION_STATE_DIR"
}

is_clean_semver() {
  local version="$1"
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
}

is_nonnegative_integer() {
  local value="$1"
  [[ "$value" =~ ^[0-9]+$ ]]
}

read_last_successful_version() {
  if [[ -f "$LAST_SUCCESSFUL_VERSION_FILE" ]]; then
    tr -d '[:space:]' < "$LAST_SUCCESSFUL_VERSION_FILE"
  fi
}

compare_semver() {
  local left="$1"
  local right="$2"

  local left_major left_minor left_patch
  local right_major right_minor right_patch

  IFS='.' read -r left_major left_minor left_patch <<< "$left"
  IFS='.' read -r right_major right_minor right_patch <<< "$right"

  if (( left_major > right_major )); then
    printf '%s\n' 1
    return
  fi
  if (( left_major < right_major )); then
    printf '%s\n' -1
    return
  fi

  if (( left_minor > right_minor )); then
    printf '%s\n' 1
    return
  fi
  if (( left_minor < right_minor )); then
    printf '%s\n' -1
    return
  fi

  if (( left_patch > right_patch )); then
    printf '%s\n' 1
    return
  fi
  if (( left_patch < right_patch )); then
    printf '%s\n' -1
    return
  fi

  printf '%s\n' 0
}

bump_patch_version() {
  local version="$1"
  local semver_major semver_minor semver_patch

  IFS='.' read -r semver_major semver_minor semver_patch <<< "$version"
  printf '%s\n' "${semver_major}.${semver_minor}.$((semver_patch + 1))"
}

same_major_minor_version() {
  local left="$1"
  local right="$2"

  local left_major left_minor left_patch
  local right_major right_minor right_patch

  IFS='.' read -r left_major left_minor left_patch <<< "$left"
  IFS='.' read -r right_major right_minor right_patch <<< "$right"

  [[ "$left_major" == "$right_major" && "$left_minor" == "$right_minor" ]]
}

derive_package_version() {
  local last_successful_version="$1"

  if [[ -z "$last_successful_version" ]]; then
    printf '%s\n' "$BASE_VERSION"
    return
  fi

  if ! is_clean_semver "$last_successful_version"; then
    echo "invalid version in $LAST_SUCCESSFUL_VERSION_FILE: $last_successful_version" >&2
    exit 1
  fi

  if same_major_minor_version "$last_successful_version" "$BASE_VERSION" && [[ "$(compare_semver "$last_successful_version" "$BASE_VERSION")" -ge 0 ]]; then
    printf '%s\n' "$(bump_patch_version "$last_successful_version")"
  else
    printf '%s\n' "$BASE_VERSION"
  fi
}

parse_release_tag_version() {
  local tag="$1"

  if [[ "$tag" =~ ^v([0-9]+\.[0-9]+\.[0-9]+)$ ]]; then
    printf '%s\n' "${BASH_REMATCH[1]}"
    return
  fi

  echo "release tags must look like v0.2.0" >&2
  exit 1
}

derive_ci_package_version() {
  local build_number="$1"
  local semver_major semver_minor semver_patch

  if ! is_nonnegative_integer "$build_number"; then
    echo "CI build number must be a non-negative integer" >&2
    exit 1
  fi

  IFS='.' read -r semver_major semver_minor semver_patch <<< "$BASE_VERSION"
  if (( build_number < semver_patch )); then
    build_number="$semver_patch"
  fi

  printf '%s\n' "${semver_major}.${semver_minor}.${build_number}"
}

derive_default_package_version() {
  local last_successful_version="$1"

  if [[ -n "${RELEASE_TAG:-}" ]]; then
    parse_release_tag_version "$RELEASE_TAG"
    return
  fi

  if [[ "${GITHUB_ACTIONS:-}" == "true" && "${GITHUB_REF_TYPE:-}" == "tag" && -n "${GITHUB_REF_NAME:-}" ]]; then
    parse_release_tag_version "$GITHUB_REF_NAME"
    return
  fi

  if [[ -n "${CI_BUILD_NUMBER:-}" ]]; then
    derive_ci_package_version "$CI_BUILD_NUMBER"
    return
  fi

  if [[ "${GITHUB_ACTIONS:-}" == "true" && -n "${GITHUB_RUN_NUMBER:-}" ]]; then
    derive_ci_package_version "$GITHUB_RUN_NUMBER"
    return
  fi

  derive_package_version "$last_successful_version"
}

current_build_date() {
  date -u +%F
}

record_successful_build_version() {
  ensure_version_state_dir
  printf '%s\n' "$PACKAGE_VERSION" > "$LAST_SUCCESSFUL_VERSION_FILE"
}

if ! is_clean_semver "$BASE_VERSION"; then
  echo "package version in Cargo.toml must be a clean semver like 0.2.0" >&2
  exit 1
fi

LAST_SUCCESSFUL_VERSION="${LAST_SUCCESSFUL_VERSION:-$(read_last_successful_version)}"
BUILD_DATE="${BUILD_DATE:-$(current_build_date)}"

if [[ -z "${PACKAGE_VERSION:-}" ]]; then
  PACKAGE_VERSION="$(derive_default_package_version "$LAST_SUCCESSFUL_VERSION")"
fi

if ! is_clean_semver "$PACKAGE_VERSION"; then
  echo "package version must be a clean semver like 0.1.1" >&2
  exit 1
fi

deb_output_path() {
  printf '%s\n' "$DIST_DIR/terminaltiler_${PACKAGE_VERSION}_amd64.deb"
}

deb_latest_path() {
  printf '%s\n' "$DIST_DIR/terminaltiler_latest_amd64.deb"
}

appimage_output_path() {
  printf '%s\n' "$DIST_DIR/TerminalTiler-${PACKAGE_VERSION}-x86_64.AppImage"
}

appimage_latest_path() {
  printf '%s\n' "$DIST_DIR/TerminalTiler-latest-x86_64.AppImage"
}

set_control_version() {
  local control_path="$1"
  sed -i -E "s/^Version: .*/Version: ${PACKAGE_VERSION}/" "$control_path"
}

set_control_glibc_floor() {
  local control_path="$1"
  local glibc_floor="$2"
  sed -i -E "s/@GLIBC_FLOOR@/${glibc_floor}/" "$control_path"
}

set_appdata_release() {
  local appdata_path="$1"
  sed -i -E "0,/<release version=\"[^\"]+\" date=\"[^\"]+\"\/>/s//<release version=\"${PACKAGE_VERSION}\" date=\"${BUILD_DATE}\"\/>/" "$appdata_path"
}

update_latest_symlink() {
  local target_path="$1"
  local link_path="$2"

  rm -f "$link_path"
  ln -s "$(basename "$target_path")" "$link_path"
}
