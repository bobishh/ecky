#!/usr/bin/env bash
# Build the same pinned OCCT SDK on both CLI release platforms.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE="${ECKY_CLI_OCCT_CACHE:-$ROOT/.dist/cache/cli-occt}"
PREFIX="${ECKY_CLI_OCCT_SDK:-$ROOT/.dist/build/cli-occt/sdk}"
JOBS="${ECKY_NATIVE_BUILD_JOBS:-2}"
VERSION=7.9.3
SHA256=5ecf094ec6b12d5413dfb851d8c3590c354058aee556e32e408bdfbf8c357d57
mkdir -p "$CACHE" "$PREFIX"
ARCHIVE="$CACHE/occt-$VERSION.tar.gz"
if [[ ! -f "$ARCHIVE" ]]; then
  curl --fail --location --retry 3 'https://codeload.github.com/Open-Cascade-SAS/OCCT/tar.gz/refs/tags/V7_9_3' --output "$ARCHIVE.part"
  mv "$ARCHIVE.part" "$ARCHIVE"
fi
[[ "$(shasum -a 256 "$ARCHIVE" | awk '{print $1}')" == "$SHA256" ]] || { echo 'OCCT source checksum mismatch' >&2; exit 1; }
SOURCE="$CACHE/source-$VERSION"
if [[ ! -f "$SOURCE/CMakeLists.txt" ]]; then mkdir -p "$SOURCE"; tar -xzf "$ARCHIVE" --strip-components=1 -C "$SOURCE"; fi
TOOLKITS="$(sed -n '/pub const REQUIRED_OCCT_LIBS/,/];/p' "$ROOT/src-tauri/src/ecky_cad_host/direct_occt_sdk.rs" | sed -n 's/.*"\([^"]*\)".*/\1/p' | tr '\n' ' ')"
cmake -S "$SOURCE" -B "$CACHE/build-$VERSION" \
  -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
  -DINSTALL_DIR_LIB=lib -DINSTALL_DIR_INCLUDE=include/opencascade \
  -DBUILD_MODULE_FoundationClasses=OFF -DBUILD_MODULE_ModelingData=OFF \
  -DBUILD_MODULE_ModelingAlgorithms=OFF -DBUILD_MODULE_Visualization=OFF \
  -DBUILD_MODULE_ApplicationFramework=OFF -DBUILD_MODULE_DataExchange=OFF \
  -DBUILD_MODULE_DETools=OFF -DBUILD_MODULE_Draw=OFF \
  -DBUILD_ADDITIONAL_TOOLKITS="$TOOLKITS" \
  -DUSE_TK=OFF -DUSE_FREETYPE=OFF -DUSE_TBB=OFF -DUSE_OPENGL=OFF \
  -DUSE_XLIB=OFF -DUSE_GLES2=OFF -DUSE_FREEIMAGE=OFF -DUSE_RAPIDJSON=OFF -DUSE_DRACO=OFF
cmake --build "$CACHE/build-$VERSION" --parallel "$JOBS"
cmake --install "$CACHE/build-$VERSION"
cp "$SOURCE/LICENSE_LGPL_21.txt" "$SOURCE/OCCT_LGPL_EXCEPTION.txt" "$PREFIX/"
# Existing runtime packager uses this exception filename.
cp "$SOURCE/OCCT_LGPL_EXCEPTION.txt" "$PREFIX/LICENSE_EXCEPTION.txt"
printf 'Prepared pinned OCCT SDK: %s\n' "$PREFIX"
