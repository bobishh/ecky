# Plan: build123d Live Gap Tranche

## Verified Environment
- Live app config is real, not stale:
  - `connectionType = "mcp"`
  - `mcp.mode = "passive"`
  - `defaultEngineKind = "eckyIrV0"`
  - `defaultGeometryBackend = "build123d"`
- Local toolchain is current and healthy:
  - `rustc 1.92.0`
  - `cargo 1.92.0`
- Live app reproduces same failure as local repo.
- Therefore current failure is in repo code, not config drift and not a broken Rust install.

## Reproduced Current Failure
- Minimal authored `.ecky` using:
  - params
  - arithmetic
  - `let`
  - `translate`
  - `polygon`
  - `extrude`
- Live MCP render failed in bundled build123d runner.
- Generated Python contained invalid identifiers:
  - `_##w2`
  - `_##h2`
- Local `cargo run --bin lower_ecky_ir_to_build123d` produced same invalid Python.

## What This Proves
- Basic parameter arithmetic is not globally broken.
- Lowering coverage exists for many current paths.
- One active lowering seam is still broken:
  - hygienic/compiled local symbols survive into build123d Python emission without identifier sanitization.

## Remaining Real Gaps
1. Python name hygiene
   - `let`, `build`, and repetition locals must emit valid Python identifiers for any Scheme symbol.
2. Live coverage drift
   - Tests/docs currently overstate support because simple local names pass while compiler-generated hygienic names still fail.
3. Unsupported lowerer surface still exists
   - `wall-pattern` and some backend-specific operations still intentionally reject on build123d.
4. `initialParams` contract still sharp
   - stale or mismatched keys fail validation independently of source declarations.
5. FreeCAD unavailable in this environment
   - first-class oracle/backend verification still blocked until installed.

## Execution Order
1. Add regression test for exact hygienic-local failure path.
2. Sanitize all emitted Python locals in build123d lowerer:
   - `let`
   - `build`
   - `repeat-union`
   - `repeat-pick`
   - `repeat-compound`
3. Re-run targeted Rust tests.
4. Re-run live MCP probe against running app with same minimal source.
5. Update support docs only after live probe is green.
6. Then continue next real lowerer gap, not random model poking.

## Acceptance
- Lowered Python contains no invalid identifiers from Scheme symbols.
- Minimal live MCP `.ecky` sample with params + `let` + `polygon` + `translate` + `extrude` renders successfully.
- `cd src-tauri && cargo check` passes.
