# Tasks

- [x] Reproduce the real parametric spoon-rest compiler failure in integration.
- [x] Add focused dynamic list regression and retain strict failure coverage.
- [x] Implement deferred concatenation through existing Core sequence nodes.
- [x] Prove native parameter overrides change actual geometry inputs.
- [x] Restore controls in the existing bound model and verify exact output.
- [x] Run relevant compiler/planner suites and record proof.

Proof: ecky-render package passed 217 tests; focused native planner regressions
passed, including explicit/default parameter equivalence. Final CLI build and
actual STL/STEP export passed. Live source constraints validate all eight controls.
The single-semicolon header regression failed with 0 controls instead of 8, then
passed after dialect detection accepted Scheme comment lines. Live version
`d706a694-8855-4e8e-96ce-2150a65510f9` reports eight parameters and eight UI fields
with a clean source binding and STL/STEP artifacts.
