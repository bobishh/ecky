# Tasks

- [x] Reproduce the real parametric spoon-rest compiler failure in integration.
- [x] Add focused dynamic list regression and retain strict failure coverage.
- [x] Implement deferred concatenation through existing Core sequence nodes.
- [x] Prove native parameter overrides change actual geometry inputs.
- [x] Restore controls in the existing bound model and verify exact output.
- [x] Run relevant compiler/planner suites and record proof.
- [x] Preserve two- and three-number list sources in sequence contexts while
  retaining geometry Point2/Point3 parsing.
- [x] Preserve the primary expanded-AST diagnostic when runtime fallback also
  fails, while keeping successful runtime fallback available.
- [x] Replace quoted tuple AST dumps with concise `zip`/static `enumerate` guidance.
- [x] Add read-only live-source diagnostic and valid seven-control
  zip/map/helper/append fixture; keep the live draft and history unchanged.
- [x] Correct the published loft signature and compile its shipped example.

Proof: ecky-render package passed 217 tests; focused native planner regressions
passed, including explicit/default parameter equivalence. Final CLI build and
actual STL/STEP export passed. Live source constraints validate all eight controls.
The single-semicolon header regression failed with 0 controls instead of 8, then
passed after dialect detection accepted Scheme comment lines. Live version
`d706a694-8855-4e8e-96ce-2150a65510f9` reports eight parameters and eight UI fields
with a clean source binding and STL/STEP artifacts.

Compiler regressions: exact live `grown-form-zip-regression.ecky` first failed
with `/ expects a number, found: 'body-width`; after preserving the primary
expanded diagnostic it reports unsupported `(apply loft ...)` and gives the
direct `loft` signature with required `distance`. Separate invalid `meta` + parameter arithmetic first
failed with `/ expects a number, found: 'body-radius`, then reports
`meta expects exactly one key and one literal value`. Quoted tuple mapping first
printed a full `Quote(Quote { ... })` AST, then returns short `zip`/`enumerate`
guidance. The three-number map/helper source first failed with `map source
expected list, got point3`; contextual sequence parsing now compiles it. Seven
integration tests pass, including a valid fixture using all seven controls in
helper geometry. Native parameter override proof is recorded in
`src-tauri/tests/parametric_sequence_planning.rs`.
The published book signature now matches the compiler; eight language contract
tests pass, including compilation of the shipped loft example. Native planning
retains 21 profile extrusions and proves each of the seven controls changes geometry.
