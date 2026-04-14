# Ecky Core IR v1 Track

## Snapshot Contract
- Canonical cup authored source: `src-tauri/tests/fixtures/cad/surface/canonical_cup.ecky`
- Thomas ramp authored source: `src-tauri/tests/fixtures/cad/surface/thomas_modular_ramp.ecky`
- Authored corpus additions:
  - `src-tauri/tests/fixtures/cad/surface/list_zip_points.ecky`
  - `src-tauri/tests/fixtures/cad/surface/linspace_bspline.ecky`
  - `src-tauri/tests/fixtures/cad/surface/filter_repeat_cutters.ecky`
  - `src-tauri/tests/fixtures/cad/surface/reduce_compound_boolean.ecky`

## Source Inputs
- Native surface input: `src-tauri/tests/fixtures/cad/surface/canonical_cup.build123d.py`
- Steel surface input: `src-tauri/tests/fixtures/cad/surface/frame_peg_attach.ecky`
- Legacy reference input: `src-tauri/tests/fixtures/cad/reference/thomas_modular_ramp_legacy.py`

## Check
- Lowering tests read authored `*.ecky` files only.
- Translation tests read reference source only.
- Steel front-end landed.
- External legacy string bridge removed from `derive_controls`, `render_model`, `lower_to_build123d`.
- Expanded-AST compiler now emits structural simple `define` / `let` / `if` cases.
- Runtime path now reads typed `IrExpr` and no longer needs raw `lexpr::Value` on hot render path.
- Backend consumer seam now clean: `mesh_ops`, `runtime`, `eval_scalar`, `sketch`, `edge_ops`, `build123d_lowering` no longer carry `lexpr::Value`.
- Lowerer typed seam started for:
  - `build`
  - `path-frame`
  - `place`
  - `clip-box`
  - `linear-array`
- MCP/workspace docs now describe authored `.ecky` Scheme source on the old `eckyIrV0` compatibility label instead of raw “IR v0” authoring.
- Thomas phase 1 uses authored Steel surface body slice and proves `CoreIR -> typed backend` path with `EXCELLENT MATCH`.
- Snapshot `.eckyir` fixtures removed from active parity path.
- Authored corpus coverage now includes:
  - list materialization before CAD primitive calls (`list_zip_points`)
  - helper-generated numeric sampling feeding path ops (`linspace_bspline`)
  - list filtering before boolean recursion (`filter_repeat_cutters`)
  - recursive solid folds across union/difference chains (`reduce_compound_boolean`)
- Example lowering targets:
  - `list_zip_points` -> `Polygon(...)` then `extrude(...)`
  - `linspace_bspline` -> periodic `Spline(...)` then `extrude(...)`
  - `filter_repeat_cutters` -> repeated translated cylinders folded through `_ecky_cut_many`
  - `reduce_compound_boolean` -> fused base solids, reduced cuts, then fused caps
- Next tranche adds exact groove semantics on top of body phase without regressing typed backend seam.
