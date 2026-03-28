#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <binary-path> <bundle-root>" >&2
  exit 1
fi

TARGET_BIN="$1"
BUNDLE_ROOT="$2"
LIB_DIR="$BUNDLE_ROOT/lib"
SCHEMA_DIR="$BUNDLE_ROOT/share/glib-2.0/schemas"

mkdir -p "$LIB_DIR" "$SCHEMA_DIR"

should_bundle_dependency() {
  case "$(basename "$1")" in
    libc.so.6|libm.so.6|libpthread.so.0|librt.so.1|libdl.so.2|libutil.so.1|libresolv.so.2|libanl.so.1|libnsl.so.1)
      return 1
      ;;
    *)
      return 0
      ;;
  esac
}

copy_library() {
  local src="$1"
  local real
  local real_name
  local link_name

  real="$(readlink -f "$src")"
  real_name="$(basename "$real")"
  link_name="$(basename "$src")"

  if [[ ! -f "$LIB_DIR/$real_name" ]]; then
    cp -L "$real" "$LIB_DIR/$real_name"
    chmod 0644 "$LIB_DIR/$real_name"
  fi

  if [[ "$link_name" != "$real_name" && ! -e "$LIB_DIR/$link_name" ]]; then
    ln -s "$real_name" "$LIB_DIR/$link_name"
  fi
}

queue=()
seen=""

enqueue_dependencies() {
  local item="$1"
  while IFS= read -r dependency; do
    [[ -n "$dependency" ]] || continue
    case "$dependency" in
      linux-vdso.so.*|/lib64/ld-linux-*|/lib/x86_64-linux-gnu/ld-linux-*|/lib/*/ld-linux-*)
        continue
        ;;
    esac
    if ! should_bundle_dependency "$dependency"; then
      continue
    fi
    queue+=("$dependency")
  done < <(ldd "$item" | awk '/=> \/.*/ { print $3 } /^\/.*/ { print $1 }' | sort -u)
}

enqueue_dependencies "$TARGET_BIN"

while [[ ${#queue[@]} -gt 0 ]]; do
  current="${queue[0]}"
  queue=("${queue[@]:1}")
  real="$(readlink -f "$current")"

  if grep -Fqx "$real" <<<"$seen"; then
    continue
  fi

  seen+="$real"$'\n'
  copy_library "$current"
  enqueue_dependencies "$real"
done

if pkg-config --exists gdk-pixbuf-2.0; then
  loader_base="$(pkg-config --variable=gdk_pixbuf_moduledir gdk-pixbuf-2.0)"
  loader_root="$LIB_DIR/gdk-pixbuf-2.0/2.10.0"
  loader_dir="$loader_root/loaders"

  mkdir -p "$loader_dir"
  if [[ -d "$loader_base" ]]; then
    find "$loader_base" -maxdepth 1 -name '*.so' -type f -exec cp -L {} "$loader_dir" \;

    while IFS= read -r loader; do
      [[ -n "$loader" ]] || continue
      enqueue_dependencies "$loader"
    done < <(find "$loader_dir" -maxdepth 1 -name '*.so' -type f | sort)

    while [[ ${#queue[@]} -gt 0 ]]; do
      current="${queue[0]}"
      queue=("${queue[@]:1}")
      real="$(readlink -f "$current")"

      if grep -Fqx "$real" <<<"$seen"; then
        continue
      fi

      seen+="$real"$'\n'
      copy_library "$current"
      enqueue_dependencies "$real"
    done

    if command -v gdk-pixbuf-query-loaders >/dev/null 2>&1; then
      GDK_PIXBUF_MODULEDIR="$loader_dir" \
        gdk-pixbuf-query-loaders "$loader_dir"/*.so > "$loader_root/loaders.cache"
    fi
  fi
fi

if [[ -f /usr/share/glib-2.0/schemas/gschemas.compiled ]]; then
  cp /usr/share/glib-2.0/schemas/gschemas.compiled "$SCHEMA_DIR/"
fi