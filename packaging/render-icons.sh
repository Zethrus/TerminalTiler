#!/usr/bin/env bash
# Shared icon-rendering helper for TerminalTiler Linux packaging.
#
# Software centers (GNOME Software / Ubuntu App Center) build their icon cache
# from raster PNGs under hicolor/<size>x<size>/apps/. An SVG-only icon does not
# render in the store, so every artifact must ship rasterized icons alongside
# the scalable source.
#
# Source this file and call:
#   render_app_icons <source_svg> <hicolor_root> <icon_name>
#
# Both the Core and Pro build scripts reuse this helper to stay in parity.

if [[ -n "${TERMINALTILER_RENDER_ICONS_LOADED:-}" ]]; then
  # shellcheck disable=SC2317 # exit fallback only used if executed directly.
  return 0 2>/dev/null || exit 0
fi
TERMINALTILER_RENDER_ICONS_LOADED=1

# Raster sizes installed into the hicolor theme. 64 and 128 are what the store
# icon cache actually consumes; 256 covers HiDPI detail views.
TERMINALTILER_ICON_SIZES=(64 128 256)

# Render a single PNG of the given pixel size from an SVG, trying renderers in
# order of fidelity. Returns non-zero if no renderer is available.
_render_png() {
  local source_svg="$1"
  local dest_png="$2"
  local size="$3"

  if command -v rsvg-convert >/dev/null 2>&1; then
    rsvg-convert --width "$size" --height "$size" --output "$dest_png" "$source_svg"
  elif command -v inkscape >/dev/null 2>&1; then
    inkscape "$source_svg" --export-type=png --export-filename="$dest_png" \
      --export-width="$size" --export-height="$size" >/dev/null 2>&1
  elif command -v magick >/dev/null 2>&1; then
    magick -background none "$source_svg" -resize "${size}x${size}" "$dest_png"
  elif command -v convert >/dev/null 2>&1; then
    convert -background none "$source_svg" -resize "${size}x${size}" "$dest_png"
  else
    return 1
  fi
}

# render_app_icons <source_svg> <hicolor_root> <icon_name>
#
# Installs raster PNGs at every TERMINALTILER_ICON_SIZES size plus the scalable
# SVG into <hicolor_root>, named <icon_name>.{png,svg}. The icon name must match
# the desktop file's Icon= key and the AppStream <icon type="stock"> value.
render_app_icons() {
  local source_svg="$1"
  local hicolor_root="$2"
  local icon_name="$3"

  if [[ ! -f "$source_svg" ]]; then
    echo "render_app_icons: source SVG not found: $source_svg" >&2
    return 1
  fi

  if ! command -v rsvg-convert >/dev/null 2>&1 \
    && ! command -v inkscape >/dev/null 2>&1 \
    && ! command -v magick >/dev/null 2>&1 \
    && ! command -v convert >/dev/null 2>&1; then
    echo "render_app_icons: no SVG rasterizer found." >&2
    echo "  Install one of: librsvg2-bin (rsvg-convert), inkscape, or imagemagick." >&2
    return 1
  fi

  local size dest_dir
  for size in "${TERMINALTILER_ICON_SIZES[@]}"; do
    dest_dir="$hicolor_root/${size}x${size}/apps"
    mkdir -p "$dest_dir"
    if ! _render_png "$source_svg" "$dest_dir/$icon_name.png" "$size"; then
      echo "render_app_icons: failed to render ${size}px icon" >&2
      return 1
    fi
  done

  local scalable_dir="$hicolor_root/scalable/apps"
  mkdir -p "$scalable_dir"
  cp "$source_svg" "$scalable_dir/$icon_name.svg"
}
