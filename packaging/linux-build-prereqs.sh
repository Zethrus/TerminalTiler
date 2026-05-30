#!/usr/bin/env bash

if [[ -n "${TERMINALTILER_LINUX_BUILD_PREREQS_LOADED:-}" ]]; then
  # shellcheck disable=SC2317 # exit fallback is only used if someone executes this helper directly.
  return 0 2>/dev/null || exit 0
fi
TERMINALTILER_LINUX_BUILD_PREREQS_LOADED=1

linux_packaging_dependency_help() {
  local -a apt_packages=(
    appstream
    build-essential
    desktop-file-utils
    dpkg-dev
    libadwaita-1-dev
    libasound2-dev
    libgraphene-1.0-dev
    libgtk-4-dev
    libjavascriptcoregtk-6.0-dev
    librsvg2-bin
    libsoup-3.0-dev
    libvte-2.91-gtk4-dev
    libwebkitgtk-6.0-dev
    pkg-config
    wget
  )

  echo "Install Ubuntu/Debian workflow dependencies with:" >&2
  echo "  bash packaging/install-ubuntu-workflow-deps.sh" >&2
  echo "" >&2
  echo "Or install the build prerequisites directly:" >&2
  echo "  sudo apt-get update && sudo apt-get install -y ${apt_packages[*]}" >&2
  echo "" >&2
  echo "Alternatively, use the containerized release build:" >&2
  echo "  bash packaging/build-in-container.sh" >&2
}


find_alsa_runtime_library() {
  local lib_path=""

  if command -v ldconfig >/dev/null 2>&1; then
    lib_path="$(ldconfig -p 2>/dev/null | awk '/libasound\.so\.2[[:space:]]/ { print $NF; exit }')"
    if [[ -n "$lib_path" && -f "$lib_path" ]]; then
      printf '%s\n' "$lib_path"
      return 0
    fi
  fi

  for lib_path in \
    /lib/x86_64-linux-gnu/libasound.so.2 \
    /usr/lib/x86_64-linux-gnu/libasound.so.2 \
    /lib64/libasound.so.2 \
    /usr/lib64/libasound.so.2 \
    /lib/libasound.so.2 \
    /usr/lib/libasound.so.2; do
    if [[ -f "$lib_path" ]]; then
      printf '%s\n' "$lib_path"
      return 0
    fi
  done

  return 1
}

ensure_alsa_pkg_config_for_cpal() {
  if ! command -v pkg-config >/dev/null 2>&1; then
    return 0
  fi

  if pkg-config --exists alsa 2>/dev/null; then
    return 0
  fi

  local runtime_lib
  if ! runtime_lib="$(find_alsa_runtime_library)"; then
    return 0
  fi

  local pc_dir pc_file
  pc_dir="${CARGO_TARGET_DIR:-$ROOT_DIR/target}/pkgconfig/alsa-runtime-link"
  pc_file="$pc_dir/alsa.pc"

  mkdir -p "$pc_dir"
  ln -sfn "$runtime_lib" "$pc_dir/libasound.so"
  cat > "$pc_file" <<EOF_ALSA_PC
Name: alsa
Description: ALSA runtime library shim for TerminalTiler CPAL builds
Version: 1.0.0
Libs: -L$pc_dir -lasound
Cflags:
EOF_ALSA_PC

  export PKG_CONFIG_PATH="$pc_dir${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
  echo "note: alsa.pc was not found; using $runtime_lib for the CPAL voice build" >&2
  echo "note: install libasound2-dev for standard ALSA pkg-config metadata" >&2
}

check_linux_packaging_dependencies() {
  local script_name="$1"
  local artifact_kind="$2"
  local building_cargo="${3:-1}"

  local -a missing_tools=()
  local -a missing_pkgconfig=()

  for tool in patchelf file objdump; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      missing_tools+=("$tool")
    fi
  done

  # Icon rasterizer: software centers need raster PNG icons, rendered from the
  # SVG by render-icons.sh. Any one of these renderers satisfies the build.
  if ! command -v rsvg-convert >/dev/null 2>&1 \
    && ! command -v inkscape >/dev/null 2>&1 \
    && ! command -v magick >/dev/null 2>&1 \
    && ! command -v convert >/dev/null 2>&1; then
    missing_tools+=("an SVG rasterizer (install librsvg2-bin for rsvg-convert, or inkscape/imagemagick)")
  fi

  case "$artifact_kind" in
    deb)
      if ! command -v dpkg-deb >/dev/null 2>&1; then
        missing_tools+=("dpkg-deb")
      fi
      ;;
    appimage|release)
      if ! command -v appimagetool >/dev/null 2>&1; then
        missing_tools+=("appimagetool (install with: sudo wget -q https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage -O /usr/local/bin/appimagetool && sudo chmod +x /usr/local/bin/appimagetool)")
      fi
      if [[ "$artifact_kind" == "release" ]] && ! command -v dpkg-deb >/dev/null 2>&1; then
        missing_tools+=("dpkg-deb")
      fi
      ;;
    *)
      echo "$script_name: unknown artifact kind: $artifact_kind" >&2
      exit 1
      ;;
  esac

  if [[ "$building_cargo" == "1" ]]; then
    ensure_alsa_pkg_config_for_cpal

    if ! command -v cargo >/dev/null 2>&1; then
      missing_tools+=("cargo")
    fi

    if ! command -v pkg-config >/dev/null 2>&1; then
      missing_tools+=("pkg-config")
    else
      for pkg in \
        "libgtk-4-dev:gtk4" \
        "libadwaita-1-dev:libadwaita-1" \
        "libvte-2.91-gtk4-dev:vte-2.91-gtk4" \
        "libgraphene-1.0-dev:graphene-1.0" \
        "libsoup-3.0-dev:libsoup-3.0" \
        "libjavascriptcoregtk-6.0-dev:javascriptcoregtk-6.0" \
        "libwebkitgtk-6.0-dev:webkitgtk-6.0" \
        "libasound2-dev:alsa"; do
        local pkg_name="${pkg%%:*}"
        local pkg_check="${pkg##*:}"
        if ! pkg-config --exists "$pkg_check" 2>/dev/null; then
          missing_pkgconfig+=("$pkg_name ($pkg_check.pc)")
        fi
      done
    fi
  fi

  if [[ ${#missing_tools[@]} -eq 0 && ${#missing_pkgconfig[@]} -eq 0 ]]; then
    return 0
  fi

  echo "$script_name: missing Linux packaging dependencies" >&2
  echo "" >&2

  if [[ ${#missing_tools[@]} -gt 0 ]]; then
    echo "Tools not found:" >&2
    for item in "${missing_tools[@]}"; do
      echo "  - $item" >&2
    done
    echo "" >&2
  fi

  if [[ ${#missing_pkgconfig[@]} -gt 0 ]]; then
    echo "Dev packages not found by pkg-config:" >&2
    for item in "${missing_pkgconfig[@]}"; do
      echo "  - $item" >&2
    done
    echo "" >&2
    echo "The voice-cpal release build uses CPAL, which links ALSA on Linux; libasound2-dev provides alsa.pc." >&2
    echo "" >&2
  fi

  linux_packaging_dependency_help
  exit 1
}
