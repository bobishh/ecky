#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUNTIME_DIR="${BUILD123D_RUNTIME_DIR:-$ROOT/.dist/build123d-runtime}"
OCCT_VERSION="${OCCT_VERSION:-7.8.1}"
OCCT_TAG="${OCCT_TAG:-V7_8_1}"
OCCT_URL="${OCCT_URL:-https://github.com/Open-Cascade-SAS/OCCT/archive/refs/tags/$OCCT_TAG.tar.gz}"
INCLUDE_DIR="$RUNTIME_DIR/include/opencascade"
CACHE_DIR="$ROOT/.dist/cache"
ARCHIVE="$CACHE_DIR/occt-$OCCT_TAG.tar.gz"
WORK_DIR="$CACHE_DIR/occt-$OCCT_TAG"

mkdir -p "$CACHE_DIR" "$INCLUDE_DIR"

if [[ ! -f "$ARCHIVE" ]]; then
  echo "Fetching OCCT headers $OCCT_VERSION from $OCCT_URL"
  curl -fsSL "$OCCT_URL" -o "$ARCHIVE"
fi

rm -rf "$WORK_DIR"
mkdir -p "$WORK_DIR"
tar -xzf "$ARCHIVE" -C "$WORK_DIR" --strip-components=1

rm -rf "$INCLUDE_DIR"
mkdir -p "$INCLUDE_DIR"

find "$WORK_DIR/src" -type f \
  \( -name '*.h' -o -name '*.hxx' -o -name '*.hpp' -o -name '*.lxx' -o -name '*.gxx' \) \
  -exec cp {} "$INCLUDE_DIR/" \;

for required in \
  BRepPrimAPI_MakeBox.hxx \
  BRepMesh_IncrementalMesh.hxx \
  STEPControl_Writer.hxx \
  StlAPI_Writer.hxx \
  TopoDS_Shape.hxx; do
  if [[ ! -f "$INCLUDE_DIR/$required" ]]; then
    echo "Missing required OCCT header after extraction: $required" >&2
    exit 1
  fi
done

count="$(find "$INCLUDE_DIR" -type f | wc -l | tr -d ' ')"
echo "Prepared OCCT $OCCT_VERSION headers: $count files in $INCLUDE_DIR"
