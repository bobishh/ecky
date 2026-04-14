# Ecky Scheme Program Track

## Boundary
- Surface fixture: `src-tauri/tests/fixtures/cad/surface/canonical_cup.build123d.py`
- Reference fixture: `src-tauri/tests/fixtures/cad/reference/thomas_modular_ramp_legacy.py`
- Authored model fixtures: `src-tauri/tests/fixtures/cad/surface/*.ecky`

## Rule
- Code paths stop at `src-tauri/tests/fixtures/cad`.
- `model-runtime/fixtures` no longer source of truth for parity checks.

## Next
- Keep harness refs on cad fixture root only.
- CoreIR now routes to in-memory `IrModel` adapter for lower/render/control.
- Steel compiler now has real expanded-AST path for:
  - simple model/build/shape/result
  - simple `define`
  - simple `let`
  - simple `if`
- Primitive parity row landed: `surface/frame_peg_attach.ecky` + `surface/frame_peg_attach.build123d.py` + `python3 server/check_frame_peg_attach.py`.
- Primitive parity rows landed:
  - `compound_boolean`
  - `segment_clip`
  - `repeat_segments`
- Authored corpus rows landed:
  - `list_zip_points`
    - semantics: pair scalar X/Y lists into polygon points before `extrude`
    - example: zipped `(-24 -10 0 10 24 24 -24)` with `(-10 10 18 10 -10 -18 -18)`
  - `linspace_bspline`
    - semantics: evenly spaced angle samples drive closed `bspline` control loop
    - example: `(linspace 0 315 8)` -> oval control points -> `extrude`
  - `filter_repeat_cutters`
    - semantics: repeated cutter centers filtered before recursive boolean subtraction
    - example: `(-24 -12 0 12 24)` filtered to negative plus far-right cuts
  - `reduce_compound_boolean`
    - semantics: recursive fold over solid lists for union and difference
    - example: three base boxes unioned, then cylinders/sphere removed, then caps re-added
- Active runtime path now evaluates typed `IrExpr`; backend consumer cold shim is gone. `lexpr::Value` stays parser boundary only.
- MCP/bootstrap guidance now teaches current `.ecky` Scheme authoring surface. Compatibility labels still say `eckyIrV0`; canonical guide resource is `ecky://guides/ecky-source` with `ecky://guides/ecky-ir-v0` kept as alias.
- Thomas phase 1 landed:
  - `surface/thomas_modular_ramp_body.ecky` now true body-only target
  - `python3 server/check_thomas_body_parity.py` -> `EXCELLENT MATCH`
- Thomas phased authored sources landed:
  - `surface/thomas_modular_ramp_grooves.ecky`
  - `surface/thomas_modular_ramp.ecky`
- Next tranche:
  - close Thomas groove exactness gap
  - then connectors/teeth/segments/full
  - keep authored corpus smoke on lower-only path until Python oracle equivalents exist
