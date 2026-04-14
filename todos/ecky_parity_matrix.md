# Ecky Parity Matrix

| Flow | Input | Output | Check |
| --- | --- | --- | --- |
| Canonical cup | `src-tauri/tests/fixtures/cad/surface/canonical_cup.ecky` | `src-tauri/tests/fixtures/cad/surface/canonical_cup.build123d.py` | `python3 server/check_canonical_cup_parity.py` |
| Frame peg attach | `src-tauri/tests/fixtures/cad/surface/frame_peg_attach.ecky` | `src-tauri/tests/fixtures/cad/surface/frame_peg_attach.build123d.py` | `python3 server/check_frame_peg_attach.py` |
| Compound boolean | `src-tauri/tests/fixtures/cad/surface/compound_boolean.ecky` | `src-tauri/tests/fixtures/cad/surface/compound_boolean.build123d.py` | `python3 server/check_compound_boolean.py` |
| Segment clip | `src-tauri/tests/fixtures/cad/surface/segment_clip.ecky` | `src-tauri/tests/fixtures/cad/surface/segment_clip.build123d.py` | `python3 server/check_segment_clip.py` |
| Repeat segments | `src-tauri/tests/fixtures/cad/surface/repeat_segments.ecky` | `src-tauri/tests/fixtures/cad/surface/repeat_segments.build123d.py` | `python3 server/check_repeat_segments.py` |
| Thomas body phase | `src-tauri/tests/fixtures/cad/surface/thomas_modular_ramp_body.ecky` | `src-tauri/tests/fixtures/cad/reference/thomas_modular_ramp_legacy.py` | `python3 server/check_thomas_body_parity.py` |
| Thomas grooves phase | `src-tauri/tests/fixtures/cad/surface/thomas_modular_ramp_grooves.ecky` | `src-tauri/tests/fixtures/cad/reference/thomas_modular_ramp_legacy.py` | `python3 server/check_thomas_grooves_parity.py` |
| Thomas connectors phase | `src-tauri/tests/fixtures/cad/surface/thomas_modular_ramp.ecky` | `src-tauri/tests/fixtures/cad/reference/thomas_modular_ramp_legacy.py` | `python3 server/check_thomas_connectors_parity.py` |
| Thomas teeth phase | `src-tauri/tests/fixtures/cad/surface/thomas_modular_ramp.ecky` | `src-tauri/tests/fixtures/cad/reference/thomas_modular_ramp_legacy.py` | `python3 server/check_thomas_teeth_parity.py` |
| Thomas segments phase | `src-tauri/tests/fixtures/cad/surface/thomas_modular_ramp.ecky` | `src-tauri/tests/fixtures/cad/reference/thomas_modular_ramp_legacy.py` | `python3 server/check_thomas_segments_parity.py` |
| Thomas ramp | `src-tauri/tests/fixtures/cad/surface/thomas_modular_ramp.ecky` | `src-tauri/tests/fixtures/cad/reference/thomas_modular_ramp_legacy.py` | `python3 server/check_thomas_ramp_parity.py` |

## Authored Corpus Smoke

| Flow | Input | Output | Check |
| --- | --- | --- | --- |
| List zip points | `src-tauri/tests/fixtures/cad/surface/list_zip_points.ecky` | lower-only generated build123d | `src-tauri/target/debug/lower_ecky_ir_to_build123d src-tauri/tests/fixtures/cad/surface/list_zip_points.ecky` |
| Linspace bspline | `src-tauri/tests/fixtures/cad/surface/linspace_bspline.ecky` | lower-only generated build123d | `src-tauri/target/debug/lower_ecky_ir_to_build123d src-tauri/tests/fixtures/cad/surface/linspace_bspline.ecky` |
| Filter repeat cutters | `src-tauri/tests/fixtures/cad/surface/filter_repeat_cutters.ecky` | lower-only generated build123d | `src-tauri/target/debug/lower_ecky_ir_to_build123d src-tauri/tests/fixtures/cad/surface/filter_repeat_cutters.ecky` |
| Reduce compound boolean | `src-tauri/tests/fixtures/cad/surface/reduce_compound_boolean.ecky` | lower-only generated build123d | `src-tauri/target/debug/lower_ecky_ir_to_build123d src-tauri/tests/fixtures/cad/surface/reduce_compound_boolean.ecky` |

## Boundary
- One fixture root. `src-tauri/tests/fixtures/cad`.
- No direct parity read from `model-runtime/fixtures`.
- Steel front-end landed.
- MCP guide surface now points agents at `.ecky` authoring via `ecky://guides/ecky-source`; old `ecky-ir-v0` guide URI remains compat alias only.
- Cup path now verifies through in-memory `CoreIR -> IrModel` adapter.
- Primitive rows green:
  - frame/place
  - compound vs boolean
  - segment clip
  - repeat segments
- Thomas phase 1 green:
  - body-only curve/profile
  - exact bounds
  - exact volume
- Authored corpus rows stay lower-only until Python oracle fixtures exist. No new `server/check_*.py` rows needed yet.
- Snapshot `.eckyir` parity inputs removed.
- Next tranche closes groove exactness before connector/teeth/segments/full.
