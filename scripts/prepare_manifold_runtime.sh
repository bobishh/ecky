#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${ECKY_MANIFOLD_RUNTIME_DIR:-$ROOT/.dist/runtime/manifold}"
CACHE_DIR="${ECKY_MANIFOLD_SOURCE_CACHE:-$ROOT/.dist/cache/manifold}"
BUILD_ROOT="${ECKY_MANIFOLD_BUILD_ROOT:-$ROOT/.dist/build/manifold}"

MANIFOLD_VERSION="3.5.2"
MANIFOLD_COMMIT="11235e6b8ebea2dbed8aec4285685aafd3d95667"
MANIFOLD_ARCHIVE_SHA256="4fa8ba091b4b905fe19f9acd550484640426c5f10ccc0e0100143aa89fa8f5b9"
MANIFOLD_ARCHIVE_URL="https://github.com/elalish/manifold/archive/${MANIFOLD_COMMIT}.tar.gz"
MANIFOLD_ARCHIVE="$CACHE_DIR/manifold-${MANIFOLD_VERSION}-${MANIFOLD_COMMIT}.tar.gz"
SOURCE_DIR="$BUILD_ROOT/source"
BUILD_DIR="$BUILD_ROOT/cmake"

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Required command missing: $1" >&2
    exit 1
  fi
}

require_command cmake
require_command curl
require_command shasum
require_command tar
require_command python3

mkdir -p "$CACHE_DIR"
if [[ ! -f "$MANIFOLD_ARCHIVE" ]]; then
  curl --fail --location --retry 3 --output "$MANIFOLD_ARCHIVE.tmp" "$MANIFOLD_ARCHIVE_URL"
  mv "$MANIFOLD_ARCHIVE.tmp" "$MANIFOLD_ARCHIVE"
fi

archive_hash="$(shasum -a 256 "$MANIFOLD_ARCHIVE" | awk '{print $1}')"
if [[ "$archive_hash" != "$MANIFOLD_ARCHIVE_SHA256" ]]; then
  echo "Manifold archive SHA-256 mismatch: expected $MANIFOLD_ARCHIVE_SHA256, got $archive_hash" >&2
  exit 1
fi

rm -rf "$SOURCE_DIR" "$BUILD_DIR" "$OUT_DIR"
mkdir -p "$SOURCE_DIR" "$BUILD_DIR" "$OUT_DIR"
tar -xzf "$MANIFOLD_ARCHIVE" --strip-components=1 -C "$SOURCE_DIR"

cmake -S "$SOURCE_DIR" -B "$BUILD_DIR" \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$OUT_DIR" \
  -DBUILD_SHARED_LIBS=OFF \
  -DMANIFOLD_PAR=OFF \
  -DMANIFOLD_CROSS_SECTION=OFF \
  -DMANIFOLD_DOWNLOADS=OFF \
  -DMANIFOLD_TEST=OFF \
  -DMANIFOLD_PYBIND=OFF \
  -DMANIFOLD_CBIND=OFF
cmake --build "$BUILD_DIR" --config Release --parallel "${ECKY_NATIVE_BUILD_JOBS:-2}"
cmake --install "$BUILD_DIR" --config Release

LIBRARY="$OUT_DIR/lib/libmanifold.a"
HEADER="$OUT_DIR/include/manifold/manifold.h"
LICENSE="$OUT_DIR/licenses/LICENSE"
if [[ ! -f "$LIBRARY" || ! -f "$HEADER" ]]; then
  echo "Manifold static runtime incomplete: expected lib/libmanifold.a and include/manifold/manifold.h" >&2
  exit 1
fi
mkdir -p "$(dirname "$LICENSE")"
cp "$SOURCE_DIR/LICENSE" "$LICENSE"

library_hash="$(shasum -a 256 "$LIBRARY" | awk '{print $1}')"
python3 - "$OUT_DIR/manifest.json" "$MANIFOLD_VERSION" "$MANIFOLD_COMMIT" \
  "$MANIFOLD_ARCHIVE_SHA256" "$MANIFOLD_ARCHIVE_URL" "$library_hash" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
manifest = {
    "schemaVersion": 1,
    "manifoldVersion": sys.argv[2],
    "sourceCommit": sys.argv[3],
    "sourceArchiveSha256": sys.argv[4],
    "sourceArchiveUrl": sys.argv[5],
    "license": "Apache-2.0",
    "parallel": False,
    "library": "lib/libmanifold.a",
    "librarySha256": sys.argv[6],
}
path.write_text(json.dumps(manifest, indent=2) + "\n")
PY

chmod -R u+rwX,go+rX "$OUT_DIR"
echo "Prepared Manifold runtime: $OUT_DIR"
