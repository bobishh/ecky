#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${ECKY_OCCT_RUNTIME_DIR:-$ROOT/.dist/runtime/occt}"
OCCT_ROOT="${ECKY_OCCT_SOURCE_ROOT:-}"
TBB_ROOT="${ECKY_TBB_SOURCE_ROOT:-}"

if [[ -z "$OCCT_ROOT" ]]; then
  OCCT_ROOT="$(brew --prefix opencascade 2>/dev/null || true)"
fi

if [[ -z "$OCCT_ROOT" || ! -d "$OCCT_ROOT/include/opencascade" || ! -d "$OCCT_ROOT/lib" ]]; then
  echo "OpenCascade SDK missing. Install it or set ECKY_OCCT_SOURCE_ROOT." >&2
  echo "Expected include/opencascade and lib under: ${OCCT_ROOT:-<empty>}" >&2
  exit 1
fi

if [[ -z "$TBB_ROOT" ]]; then
  TBB_ROOT="$(brew --prefix tbb 2>/dev/null || true)"
fi

case "$(uname -s)" in
  Darwin)
    PLATFORM="macos"
    ABI_TAG="macos"
    LIB_EXT="dylib"
    ;;
  Linux)
    PLATFORM="linux"
    ABI_TAG="linux-gnu"
    LIB_EXT="so"
    ;;
  *)
    echo "Unsupported OCCT runtime packaging platform: $(uname -s)" >&2
    exit 1
    ;;
esac

ARCH="$(uname -m)"
case "$ARCH" in
  arm64|aarch64) ARCH="arm64" ;;
  x86_64|amd64) ARCH="x86_64" ;;
esac

REQUIRED_HEADERS=()
while IFS= read -r item; do
  REQUIRED_HEADERS+=("$item")
done < <(
  sed -n '/pub const REQUIRED_OCCT_HEADERS/,/];/p' \
    "$ROOT/src-tauri/src/ecky_cad_host/direct_occt_sdk.rs" \
    | sed -n 's/.*"\([^"]*\)".*/\1/p'
)
REQUIRED_LIBS=()
while IFS= read -r item; do
  REQUIRED_LIBS+=("$item")
done < <(
  sed -n '/pub const REQUIRED_OCCT_LIBS/,/];/p' \
    "$ROOT/src-tauri/src/ecky_cad_host/direct_occt_sdk.rs" \
    | sed -n 's/.*"\([^"]*\)".*/\1/p'
)

if [[ "${#REQUIRED_HEADERS[@]}" -eq 0 || "${#REQUIRED_LIBS[@]}" -eq 0 ]]; then
  echo "Could not read required OCCT headers/libs from direct_occt_sdk.rs" >&2
  exit 1
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/include/opencascade" "$OUT_DIR/lib" "$OUT_DIR/licenses"
rsync -a --delete "$OCCT_ROOT/include/opencascade/" "$OUT_DIR/include/opencascade/"

copy_lib_family() {
  local lib="$1"
  local found=0
  shopt -s nullglob
  for path in "$OCCT_ROOT/lib/lib${lib}"*."$LIB_EXT"; do
    copy_library_path "$path"
    found=1
  done
  shopt -u nullglob
  if [[ "$found" -eq 0 ]]; then
    echo "Missing required OCCT library family: lib${lib}*.${LIB_EXT}" >&2
    exit 1
  fi
}

copy_library_path() {
  local path="$1"
  local dest="$OUT_DIR/lib/$(basename "$path")"
  if [[ ! -e "$dest" && ! -L "$dest" ]]; then
    cp -a "$path" "$OUT_DIR/lib/"
  fi
  if [[ -L "$path" ]]; then
    local resolved
    resolved="$(python3 - "$path" <<'PY'
from pathlib import Path
import sys
print(Path(sys.argv[1]).resolve())
PY
)"
    local resolved_dest="$OUT_DIR/lib/$(basename "$resolved")"
    if [[ -f "$resolved" && ! -e "$resolved_dest" && ! -L "$resolved_dest" ]]; then
      cp -a "$resolved" "$OUT_DIR/lib/"
    fi
  fi
}

for lib in "${REQUIRED_LIBS[@]}"; do
  copy_lib_family "$lib"
done

while IFS= read -r symlink_path; do
  resolved_path="$(python3 - "$symlink_path" <<'PY'
from pathlib import Path
import sys
print(Path(sys.argv[1]).resolve())
PY
)"
  if [[ -f "$resolved_path" ]]; then
    rm "$symlink_path"
    cp -a "$resolved_path" "$symlink_path"
  fi
done < <(find "$OUT_DIR/lib" -maxdepth 1 -type l)

convert_runtime_symlinks() {
  while IFS= read -r symlink_path; do
    resolved_path="$(python3 - "$symlink_path" <<'PY'
from pathlib import Path
import sys
print(Path(sys.argv[1]).resolve())
PY
)"
    if [[ -f "$resolved_path" ]]; then
      rm "$symlink_path"
      cp -a "$resolved_path" "$symlink_path"
    fi
  done < <(find "$OUT_DIR" -type l)
}

if [[ "$PLATFORM" == "macos" ]]; then
  while :; do
    copied=0
    while IFS= read -r dylib; do
      while IFS= read -r dep; do
        [[ "$dep" == /usr/lib/* || "$dep" == /System/* ]] && continue
        dep_base="$(basename "$dep")"
        if [[ -f "$OUT_DIR/lib/$dep_base" ]]; then
          continue
        fi
        if [[ "$dep" == "$OCCT_ROOT/lib/"* || "$dep" == /opt/homebrew/opt/opencascade/lib/* || "$dep" == /opt/homebrew/Cellar/opencascade/*/lib/* ]]; then
          copy_library_path "$dep" 2>/dev/null || true
          [[ -f "$OUT_DIR/lib/$dep_base" ]] && copied=1
        elif [[ "$dep" == @rpath/lib*.dylib && -f "$OCCT_ROOT/lib/$dep_base" ]]; then
          copy_library_path "$OCCT_ROOT/lib/$dep_base" 2>/dev/null || true
          [[ -f "$OUT_DIR/lib/$dep_base" ]] && copied=1
        elif [[ -n "$TBB_ROOT" && "$dep" == "$TBB_ROOT/lib/"* || "$dep" == /opt/homebrew/opt/tbb/lib/* || "$dep" == /opt/homebrew/Cellar/tbb/*/lib/* ]]; then
          copy_library_path "$dep" 2>/dev/null || true
          [[ -f "$OUT_DIR/lib/$dep_base" ]] && copied=1
        elif [[ -n "$TBB_ROOT" && "$dep" == @rpath/libtbb*.dylib && -f "$TBB_ROOT/lib/$dep_base" ]]; then
          copy_library_path "$TBB_ROOT/lib/$dep_base" 2>/dev/null || true
          [[ -f "$OUT_DIR/lib/$dep_base" ]] && copied=1
        fi
      done < <(otool -L "$dylib" | tail -n +2 | awk '{print $1}')
    done < <(find "$OUT_DIR/lib" -type f -name "*.${LIB_EXT}")
    [[ "$copied" -eq 0 ]] && break
  done

  while IFS= read -r dylib; do
    base="$(basename "$dylib")"
    chmod u+w "$dylib" 2>/dev/null || true
    install_name_tool -id "@rpath/$base" "$dylib" 2>/dev/null || true
    while IFS= read -r dep; do
      [[ "$dep" == /usr/lib/* || "$dep" == /System/* ]] && continue
      dep_base="$(basename "$dep")"
      if [[ -e "$OUT_DIR/lib/$dep_base" ]]; then
        install_name_tool -change "$dep" "@rpath/$dep_base" "$dylib" 2>/dev/null || true
      fi
    done < <(otool -L "$dylib" | tail -n +2 | awk '{print $1}')
    codesign --force --sign - "$dylib" >/dev/null 2>&1 || true
  done < <(find "$OUT_DIR/lib" -type f -name "*.${LIB_EXT}")
fi

for required in "${REQUIRED_HEADERS[@]}"; do
  if [[ ! -f "$OUT_DIR/include/opencascade/$required" ]]; then
    echo "Missing required OCCT header after copy: $required" >&2
    exit 1
  fi
done

for lib in "${REQUIRED_LIBS[@]}"; do
  if ! find "$OUT_DIR/lib" -maxdepth 1 -name "lib${lib}*.${LIB_EXT}" | grep -q .; then
    echo "Missing required OCCT library after copy: lib${lib}*.${LIB_EXT}" >&2
    exit 1
  fi
done

OCCT_VERSION="$(
  sed -n 's/^#define OCC_VERSION_COMPLETE "\(.*\)"/\1/p' \
    "$OUT_DIR/include/opencascade/Standard_Version.hxx" \
    | head -n 1
)"

python3 - "$OUT_DIR" "$PLATFORM" "$ARCH" "$ABI_TAG" "$OCCT_VERSION" \
  "${REQUIRED_HEADERS[@]}" -- "${REQUIRED_LIBS[@]}" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

out_dir = Path(sys.argv[1])
platform = sys.argv[2]
arch = sys.argv[3]
abi_tag = sys.argv[4]
occt_version = sys.argv[5]
split = sys.argv.index("--")
required_headers = sys.argv[6:split]
required_libraries = sys.argv[split + 1 :]

hashes = {}
for path in sorted((out_dir / "lib").iterdir()):
    if path.is_file():
        hashes[path.name] = hashlib.sha256(path.read_bytes()).hexdigest()

manifest = {
    "schemaVersion": "1",
    "platform": platform,
    "arch": arch,
    "occtVersion": occt_version,
    "abiTag": abi_tag,
    "includeDir": "include/opencascade",
    "libDir": "lib",
    "requiredHeaders": required_headers,
    "requiredLibraries": required_libraries,
    "libraryHashes": hashes,
}
(out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
PY

cp "$OCCT_ROOT/LICENSE_LGPL_21.txt" "$OUT_DIR/licenses/" 2>/dev/null || true
cp "$OCCT_ROOT/LICENSE_EXCEPTION.txt" "$OUT_DIR/licenses/" 2>/dev/null || true

bash "$ROOT/scripts/build_direct_occt_runner.sh"
convert_runtime_symlinks
chmod -R u+rwX,go+rX "$OUT_DIR"

du -sh "$OUT_DIR"
echo "Prepared OCCT runtime: $OUT_DIR"
