#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUNTIME_DIR="$ROOT/.dist/speech-runtime"
TMP_RUNTIME_DIR="$ROOT/.dist/speech-runtime.next"
SEED_PYTHON="${SPEECH_PYTHON:-${PYTHON_CMD:-python3}}"
RIVA_VERSION="${NVIDIA_RIVA_CLIENT_VERSION:-2.25.1}"

runtime_python() {
  local runtime_dir="$1"
  if [[ -x "$runtime_dir/bin/python3" ]]; then
    echo "$runtime_dir/bin/python3"
  elif [[ -x "$runtime_dir/bin/python" ]]; then
    echo "$runtime_dir/bin/python"
  fi
}

runtime_has_riva() {
  local runtime_dir="$1"
  local python_bin
  python_bin="$(runtime_python "$runtime_dir")"
  [[ -n "${python_bin:-}" ]] || return 1
  "$python_bin" - <<'PY' >/dev/null 2>&1
import riva.client
PY
}

print_runtime() {
  local runtime_dir="$1"
  local python_bin
  python_bin="$(runtime_python "$runtime_dir")"
  [[ -n "${python_bin:-}" ]] || return 1
  "$python_bin" - <<'PY'
import riva.client
import sys

print(f"Prepared NVIDIA Speech runtime: {sys.executable}")
print(f"riva.client module: {riva.client.__file__}")
PY
}

restore_seed_runtime() {
  local seed_dir="$1"
  [[ -d "$seed_dir" ]] || return 1
  runtime_has_riva "$seed_dir" || return 1
  rm -rf "$RUNTIME_DIR"
  cp -R "$seed_dir" "$RUNTIME_DIR"
  runtime_has_riva "$RUNTIME_DIR" || return 1
  print_runtime "$RUNTIME_DIR"
}

if runtime_has_riva "$RUNTIME_DIR"; then
  print_runtime "$RUNTIME_DIR"
  exit 0
fi

for seed_dir in \
  "$ROOT/src-tauri/target/release/bundle/macos/Ecky CAD.app/Contents/Resources/runtime/speech" \
  "$ROOT/src-tauri/target/debug/bundle/macos/Ecky CAD.app/Contents/Resources/runtime/speech"
do
  if restore_seed_runtime "$seed_dir"; then
    exit 0
  fi
done

rm -rf "$TMP_RUNTIME_DIR"
"$SEED_PYTHON" -m venv "$TMP_RUNTIME_DIR"

BUNDLED_PYTHON="$(runtime_python "$TMP_RUNTIME_DIR")"
if [[ -z "${BUNDLED_PYTHON:-}" ]]; then
  echo "Bundled speech python missing at $TMP_RUNTIME_DIR/bin/python3" >&2
  exit 1
fi

"$BUNDLED_PYTHON" -m pip install "nvidia-riva-client==$RIVA_VERSION"

rm -rf "$RUNTIME_DIR"
mv "$TMP_RUNTIME_DIR" "$RUNTIME_DIR"
print_runtime "$RUNTIME_DIR"
