#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUNTIME_DIR="$ROOT/.dist/build123d-runtime"
SEED_PYTHON="${BUILD123D_PYTHON:-${PYTHON_CMD:-python3}}"

INFO_JSON="$("$SEED_PYTHON" - <<'PY'
import json
import os
import sys

try:
    import build123d  # noqa: F401
except Exception as exc:  # pragma: no cover
    raise SystemExit(f"Seed python missing build123d: {exc}")

print(json.dumps({
    "executable": sys.executable,
    "prefix": sys.prefix,
    "relative_executable": os.path.relpath(sys.executable, sys.prefix),
}))
PY
)"

PREFIX="$("$SEED_PYTHON" -c 'import json,sys; print(json.loads(sys.argv[1])["prefix"])' "$INFO_JSON")"
RELATIVE_EXECUTABLE="$("$SEED_PYTHON" -c 'import json,sys; print(json.loads(sys.argv[1])["relative_executable"])' "$INFO_JSON")"

rm -rf "$RUNTIME_DIR"
mkdir -p "$(dirname "$RUNTIME_DIR")"
rsync -a --delete "$PREFIX/" "$RUNTIME_DIR/"

if [[ ! -x "$RUNTIME_DIR/bin/python3" && -x "$RUNTIME_DIR/$RELATIVE_EXECUTABLE" ]]; then
  ln -sf "$(basename "$RELATIVE_EXECUTABLE")" "$RUNTIME_DIR/bin/python3"
fi

if [[ ! -x "$RUNTIME_DIR/bin/python" && -x "$RUNTIME_DIR/bin/python3" ]]; then
  ln -sf python3 "$RUNTIME_DIR/bin/python"
fi

BUNDLED_PYTHON="$RUNTIME_DIR/bin/python3"
if [[ ! -x "$BUNDLED_PYTHON" ]]; then
  echo "Bundled python missing at $BUNDLED_PYTHON" >&2
  exit 1
fi

"$BUNDLED_PYTHON" - <<'PY'
import build123d
import sys

print(f"Prepared build123d runtime: {sys.executable}")
print(f"build123d module: {build123d.__file__}")
PY

bash "$SCRIPT_DIR/prepare_occt_headers.sh"
