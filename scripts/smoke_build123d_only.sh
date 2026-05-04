#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FREECAD_MISSING_PATH="/missing/freecadcmd"

npm run build123d:prepare

BUNDLED_PYTHON="$ROOT/.dist/build123d-runtime/bin/python3"
if [[ ! -x "$BUNDLED_PYTHON" ]]; then
  echo "Bundled python missing at $BUNDLED_PYTHON" >&2
  exit 1
fi

BUILD123D_PYTHON="$BUNDLED_PYTHON" FREECAD_CMD="$FREECAD_MISSING_PATH" "$BUNDLED_PYTHON" - <<'PY'
import build123d
import sys

print(f"BUILD123D ready: {sys.executable}")
print(f"build123d module: {build123d.__file__}")
PY

BUILD123D_PYTHON="$BUNDLED_PYTHON" FREECAD_CMD="$FREECAD_MISSING_PATH" cargo test \
  --manifest-path "$ROOT/src-tauri/Cargo.toml" \
  'runtime_capabilities::tests::collect_runtime_capabilities_prefers_build123d_when_freecad_missing' \
  --lib -- --exact --nocapture

BUILD123D_PYTHON="$BUNDLED_PYTHON" FREECAD_CMD="$FREECAD_MISSING_PATH" cargo test \
  --manifest-path "$ROOT/src-tauri/Cargo.toml" \
  'ecky_cad_host::direct_occt_sdk::tests::live_bundled_build123d_runtime_can_export_when_headers_are_available' \
  --lib -- --exact --nocapture

BUILD123D_PYTHON="$BUNDLED_PYTHON" FREECAD_CMD="$FREECAD_MISSING_PATH" cargo test \
  --manifest-path "$ROOT/src-tauri/Cargo.toml" \
  'services::render::tests::ecky_rust_dispatch_uses_direct_occt_step_when_sdk_ready' \
  --lib -- --exact --nocapture

BUILD123D_PYTHON="$BUNDLED_PYTHON" FREECAD_CMD="$FREECAD_MISSING_PATH" cargo test \
  --manifest-path "$ROOT/src-tauri/Cargo.toml" \
  'sketch_draft_runtime::tests::live_accepted_brep_candidate_source_exports_step_when_sdk_ready' \
  --lib -- --exact --nocapture

BUILD123D_PYTHON="$BUNDLED_PYTHON" FREECAD_CMD="$FREECAD_MISSING_PATH" "$BUNDLED_PYTHON" \
  "$ROOT/server/check_canonical_cup_parity.py" --allow-good
