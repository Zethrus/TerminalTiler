#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE_TAG="${IMAGE_TAG:-terminaltiler-build:debian12}"
CONTAINER_TOOL="${CONTAINER_TOOL:-}"
HOST_UID="$(id -u)"
HOST_GID="$(id -g)"

pick_container_tool() {
  if [[ -n "$CONTAINER_TOOL" ]]; then
    printf '%s\n' "$CONTAINER_TOOL"
    return
  fi

  if command -v docker >/dev/null 2>&1; then
    printf '%s\n' docker
    return
  fi

  if command -v podman >/dev/null 2>&1; then
    printf '%s\n' podman
    return
  fi

  echo "docker or podman is required for containerized packaging" >&2
  exit 1
}

CONTAINER_TOOL="$(pick_container_tool)"
CONTAINER_SCRIPT="$(cat <<'EOF'
set -euo pipefail
export CARGO_TARGET_DIR=/workspace/packaging/.build/container-target
cargo build --release
SKIP_CARGO_BUILD=1 bash packaging/build-deb.sh
SKIP_CARGO_BUILD=1 bash packaging/build-appimage.sh
chown -R "$HOST_UID:$HOST_GID" /workspace/dist /workspace/packaging/.build
EOF
)"

echo "==> building Debian 12 packaging image"
"$CONTAINER_TOOL" build \
  -f "$ROOT_DIR/packaging/container/debian12/Dockerfile" \
  -t "$IMAGE_TAG" \
  "$ROOT_DIR"

echo "==> running release packaging in container"
"$CONTAINER_TOOL" run --rm \
  -e HOST_UID="$HOST_UID" \
  -e HOST_GID="$HOST_GID" \
  -v "$ROOT_DIR:/workspace" \
  -w /workspace \
  "$IMAGE_TAG" \
  bash -lc "$CONTAINER_SCRIPT"
