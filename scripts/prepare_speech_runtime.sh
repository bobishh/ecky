#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUNTIME_DIR="$ROOT/.dist/speech-runtime"
SEED_PYTHON="${SPEECH_PYTHON:-${PYTHON_CMD:-python3}}"
RIVA_VERSION="${NVIDIA_RIVA_CLIENT_VERSION:-2.25.1}"

rm -rf "$RUNTIME_DIR"
"$SEED_PYTHON" -m venv "$RUNTIME_DIR"

BUNDLED_PYTHON="$RUNTIME_DIR/bin/python3"
if [[ ! -x "$BUNDLED_PYTHON" ]]; then
  BUNDLED_PYTHON="$RUNTIME_DIR/bin/python"
fi
if [[ ! -x "$BUNDLED_PYTHON" ]]; then
  echo "Bundled speech python missing at $RUNTIME_DIR/bin/python3" >&2
  exit 1
fi

"$BUNDLED_PYTHON" -m pip install --upgrade pip
"$BUNDLED_PYTHON" -m pip install "nvidia-riva-client==$RIVA_VERSION"

"$BUNDLED_PYTHON" - <<'PY'
import riva.client
import sys

print(f"Prepared NVIDIA Speech runtime: {sys.executable}")
print(f"riva.client module: {riva.client.__file__}")
PY
