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
LIBEXEC_DIR="$BUNDLE_ROOT/libexec"

if ! command -v patchelf >/dev/null 2>&1; then
  echo "patchelf is required to set bundled runtime rpaths" >&2
  exit 1
fi

mkdir -p "$LIB_DIR" "$SCHEMA_DIR" "$LIBEXEC_DIR"

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

patch_elf_rpath() {
  local target="$1"
  local rpath="$2"

  if file "$target" | grep -q 'ELF '; then
    patchelf --set-rpath "$rpath" "$target" || true
  fi
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
    patch_elf_rpath "$LIB_DIR/$real_name" "\$ORIGIN"
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

process_dependency_queue() {
  local current
  local real

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
}

copy_module_tree() {
  local src_dir="$1"
  local dest_dir="$2"
  local module_rpath="$3"
  local cache_name="${4:-}"
  local module
  local dest_module

  [[ -d "$src_dir" ]] || return 0

  mkdir -p "$dest_dir"
  while IFS= read -r module; do
    [[ -n "$module" ]] || continue
    dest_module="$dest_dir/$(basename "$module")"
    cp -L "$module" "$dest_module"
    chmod 0644 "$dest_module"
    patch_elf_rpath "$dest_module" "$module_rpath"
    enqueue_dependencies "$dest_module"
  done < <(find "$src_dir" -maxdepth 1 -name '*.so' -type f | sort)

  if [[ -n "$cache_name" ]] && compgen -G "$dest_dir/*.so" >/dev/null && command -v gio-querymodules >/dev/null 2>&1; then
    gio-querymodules "$dest_dir" > "$dest_dir/$cache_name"
  fi
}

copy_webkit_runtime() {
  local webkit_libdir
  local webkit_src_dir
  local webkit_dest_dir="$LIBEXEC_DIR/webkitgtk-6.0"
  local process_name
  local bundle_so
  local dest_bundle

  webkit_libdir="$(pkg-config --variable=libdir webkitgtk-6.0 2>/dev/null || true)"
  [[ -n "$webkit_libdir" ]] || return 0

  webkit_src_dir="$webkit_libdir/webkitgtk-6.0"
  [[ -d "$webkit_src_dir" ]] || return 0

  mkdir -p "$webkit_dest_dir"

  for process_name in WebKitNetworkProcess WebKitWebProcess WebKitGPUProcess; do
    if [[ -f "$webkit_src_dir/$process_name" ]]; then
      cp -L "$webkit_src_dir/$process_name" "$webkit_dest_dir/$process_name"
      chmod 0755 "$webkit_dest_dir/$process_name"
      patch_elf_rpath "$webkit_dest_dir/$process_name" "\$ORIGIN/../../lib"
      enqueue_dependencies "$webkit_dest_dir/$process_name"
    fi
  done

  if [[ -d "$webkit_src_dir/injected-bundle" ]]; then
    mkdir -p "$webkit_dest_dir/injected-bundle"
    while IFS= read -r bundle_so; do
      [[ -n "$bundle_so" ]] || continue
      dest_bundle="$webkit_dest_dir/injected-bundle/$(basename "$bundle_so")"
      cp -L "$bundle_so" "$dest_bundle"
      chmod 0644 "$dest_bundle"
      patch_elf_rpath "$dest_bundle" "\$ORIGIN/../../../lib"
      enqueue_dependencies "$dest_bundle"
    done < <(find "$webkit_src_dir/injected-bundle" -maxdepth 1 -name '*.so' -type f | sort)
  fi
}

enqueue_dependencies "$TARGET_BIN"
patch_elf_rpath "$BUNDLE_ROOT/bin/terminaltiler-bin" "\$ORIGIN/../lib"
process_dependency_queue

if pkg-config --exists gio-2.0; then
  gio_module_dir="$(pkg-config --variable=giomoduledir gio-2.0)"
  copy_module_tree "$gio_module_dir" "$LIB_DIR/gio/modules" "\$ORIGIN/../.." giomodule.cache
  process_dependency_queue
fi

if pkg-config --exists gtk4; then
  gtk_binary_version="$(pkg-config --variable=gtk_binary_version gtk4)"
  gtk_libdir="$(pkg-config --variable=libdir gtk4)"
  gtk_src_base="$gtk_libdir/gtk-4.0/$gtk_binary_version"
  gtk_dest_base="$LIB_DIR/gtk-4.0/$gtk_binary_version"

  copy_module_tree "$gtk_src_base/immodules" "$gtk_dest_base/immodules" "\$ORIGIN/../../.."
  copy_module_tree "$gtk_src_base/media" "$gtk_dest_base/media" "\$ORIGIN/../../.."
  copy_module_tree "$gtk_src_base/printbackends" "$gtk_dest_base/printbackends" "\$ORIGIN/../../.." giomodule.cache
  process_dependency_queue
fi

copy_webkit_runtime
process_dependency_queue

if pkg-config --exists gdk-pixbuf-2.0; then
  loader_base="$(pkg-config --variable=gdk_pixbuf_moduledir gdk-pixbuf-2.0)"
  loader_root="$LIB_DIR/gdk-pixbuf-2.0/2.10.0"
  loader_dir="$loader_root/loaders"
  loader=""
  dest_loader=""

  mkdir -p "$loader_dir"
  if [[ -d "$loader_base" ]]; then
    while IFS= read -r loader; do
      [[ -n "$loader" ]] || continue
      dest_loader="$loader_dir/$(basename "$loader")"
      cp -L "$loader" "$dest_loader"
      chmod 0644 "$dest_loader"
      patch_elf_rpath "$dest_loader" "\$ORIGIN/../../.."
      enqueue_dependencies "$dest_loader"
    done < <(find "$loader_base" -maxdepth 1 -name '*.so' -type f | sort)

    process_dependency_queue

    if command -v gdk-pixbuf-query-loaders >/dev/null 2>&1 && compgen -G "$loader_dir/*.so" >/dev/null; then
      GDK_PIXBUF_MODULEDIR="$loader_dir" \
        gdk-pixbuf-query-loaders "$loader_dir"/*.so > "$loader_root/loaders.cache"
    fi
  fi
fi

if [[ -f /usr/share/glib-2.0/schemas/gschemas.compiled ]]; then
  cp /usr/share/glib-2.0/schemas/gschemas.compiled "$SCHEMA_DIR/"
fi
