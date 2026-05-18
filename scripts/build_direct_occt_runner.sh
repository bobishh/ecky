#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${ECKY_OCCT_RUNTIME_DIR:-$ROOT/.dist/runtime/occt}"
SOURCE="$ROOT/src-tauri/native/direct_occt_runner.cpp"
YYJSON_SOURCE="$ROOT/src-tauri/native/vendor/yyjson/yyjson.c"
YYJSON_INCLUDE_DIR="$ROOT/src-tauri/native/vendor/yyjson"

if [[ ! -d "$OUT_DIR/include/opencascade" || ! -d "$OUT_DIR/lib" ]]; then
  echo "OCCT runtime missing. Run scripts/prepare_occt_runtime.sh first." >&2
  exit 1
fi

if [[ ! -f "$SOURCE" ]]; then
  echo "Runner source missing: $SOURCE" >&2
  exit 1
fi

if [[ ! -f "$YYJSON_SOURCE" ]]; then
  echo "yyjson source missing: $YYJSON_SOURCE" >&2
  exit 1
fi

REQUIRED_LIBS=()
while IFS= read -r item; do
  REQUIRED_LIBS+=("$item")
done < <(
  sed -n '/pub const REQUIRED_OCCT_LIBS/,/];/p' \
    "$ROOT/src-tauri/src/ecky_cad_host/direct_occt_sdk.rs" \
    | sed -n 's/.*"\([^"]*\)".*/\1/p'
)

if [[ "${#REQUIRED_LIBS[@]}" -eq 0 ]]; then
  echo "Could not read required OCCT libraries from direct_occt_sdk.rs" >&2
  exit 1
fi

case "$(uname -s)" in
  Darwin) RPATH="-Wl,-rpath,@loader_path/../lib" ;;
  Linux) RPATH="-Wl,-rpath,\$ORIGIN/../lib" ;;
  *)
    echo "Unsupported runner build platform: $(uname -s)" >&2
    exit 1
    ;;
esac

mkdir -p "$OUT_DIR/bin"

CXX_BIN="${CXX:-c++}"
CC_BIN="${CC:-cc}"
YYJSON_OBJECT="$OUT_DIR/bin/yyjson.o"

"$CC_BIN" \
  -std=c99 \
  -O2 \
  -I"$YYJSON_INCLUDE_DIR" \
  -c "$YYJSON_SOURCE" \
  -o "$YYJSON_OBJECT"

command=(
  "$CXX_BIN"
  -std=c++17
  -O2
  -I"$OUT_DIR/include/opencascade"
  -I"$YYJSON_INCLUDE_DIR"
  "$SOURCE"
  "$YYJSON_OBJECT"
  -L"$OUT_DIR/lib"
  $RPATH
)

for lib in "${REQUIRED_LIBS[@]}"; do
  command+=("-l${lib}")
done

command+=(
  -o "$OUT_DIR/bin/direct-occt-runner"
)

"${command[@]}"

if [[ "$(uname -s)" == "Darwin" ]]; then
  codesign --force --sign - "$OUT_DIR/bin/direct-occt-runner" >/dev/null 2>&1 || true
fi
chmod u+rwx,go+rx "$OUT_DIR/bin/direct-occt-runner"

echo "Built direct OCCT runner: $OUT_DIR/bin/direct-occt-runner"
