#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${ECKY_OCCT_RUNTIME_DIR:-$ROOT/.dist/runtime/occt}"
MANIFOLD_DIR="${ECKY_MANIFOLD_RUNTIME_DIR:-$ROOT/.dist/runtime/manifold}"
SOURCE="$ROOT/src-tauri/native/direct_occt_runner.cpp"
TEST_SOURCE="$ROOT/src-tauri/native/direct_occt_runner_integration_test.cpp"
YYJSON_SOURCE="$ROOT/src-tauri/native/vendor/yyjson/yyjson.c"
YYJSON_INCLUDE_DIR="$ROOT/src-tauri/native/vendor/yyjson"

if [[ ! -d "$OUT_DIR/include/opencascade" || ! -d "$OUT_DIR/lib" ]]; then
  echo "OCCT runtime missing. Run scripts/prepare_occt_runtime.sh first." >&2
  exit 1
fi

if [[ ! -d "$MANIFOLD_DIR/include/manifold" || ! -f "$MANIFOLD_DIR/lib/libmanifold.a" ]]; then
  echo "Manifold runtime missing. Run scripts/prepare_manifold_runtime.sh first." >&2
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
  # DT_RPATH also resolves transitive private SDK libraries after relocation.
  Linux) RPATH="-Wl,--disable-new-dtags,-rpath,\$ORIGIN/../lib" ;;
  *)
    echo "Unsupported runner build platform: $(uname -s)" >&2
    exit 1
    ;;
esac

mkdir -p "$OUT_DIR/bin"

CXX_BIN="${CXX:-c++}"
CC_BIN="${CC:-cc}"
YYJSON_OBJECT="$OUT_DIR/bin/yyjson.o"

# macOS SDKs may omit the unversioned dylib link; Linux uses normal -l names.
resolve_occt_lib_path() {
  local lib="$1"
  local lib_dir="$OUT_DIR/lib"
  if [[ "$(uname -s)" == "Linux" ]]; then
    [[ -f "$lib_dir/lib${lib}.so" ]] || { echo "Missing OCCT shared library: $lib" >&2; exit 1; }
    printf '%s' "-l${lib}"
    return
  fi
  local bare="$lib_dir/lib${lib}.dylib"
  if [[ -f "$bare" || -L "$bare" ]]; then
    printf '%s' "-l${lib}"
    return
  fi
  local versioned
  versioned="$(find "$lib_dir" -maxdepth 1 -name "lib${lib}.*.dylib" 2>/dev/null | sort | head -n 1)"
  if [[ -z "$versioned" ]]; then
    echo "Could not find OCCT dylib for ${lib} in ${lib_dir}" >&2
    exit 1
  fi
  printf '%s' "$versioned"
}

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
  -isystem
  "$OUT_DIR/include/opencascade"
  -isystem
  "$MANIFOLD_DIR/include"
  -I"$YYJSON_INCLUDE_DIR"
  "$SOURCE"
  "$YYJSON_OBJECT"
  "$MANIFOLD_DIR/lib/libmanifold.a"
  -L"$OUT_DIR/lib"
  $RPATH
)

for lib in "${REQUIRED_LIBS[@]}"; do
  command+=("$(resolve_occt_lib_path "$lib")")
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

if [[ "${ECKY_DIRECT_OCCT_RUNNER_TEST:-0}" == "1" ]]; then
  if [[ ! -f "$TEST_SOURCE" ]]; then
    echo "Native runner test source missing: $TEST_SOURCE" >&2
    exit 1
  fi
  test_command=(
    "$CXX_BIN"
    -std=c++17
    -O2
    -isystem
    "$OUT_DIR/include/opencascade"
    -isystem
    "$MANIFOLD_DIR/include"
    -I"$YYJSON_INCLUDE_DIR"
    "$TEST_SOURCE"
    "$YYJSON_OBJECT"
    "$MANIFOLD_DIR/lib/libmanifold.a"
    -L"$OUT_DIR/lib"
    $RPATH
  )
  for lib in "${REQUIRED_LIBS[@]}"; do
    test_command+=("$(resolve_occt_lib_path "$lib")")
  done
  test_command+=(-o "$OUT_DIR/bin/direct-occt-runner-integration-test")
  "${test_command[@]}"
  "$OUT_DIR/bin/direct-occt-runner-integration-test"
  echo "Passed native runner integration tests"
fi
