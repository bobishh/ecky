# Tasks

- [x] Reproduce acceptance of a fake six-sided cylinder with a failing compiler test.
- [x] Remove optional segment arguments from shared compilation and ring expansion.
- [x] Remove old segment handling from FreeCAD and mesh fallback evaluation.
- [x] Update built-in components, examples, fixture sources and registry signatures.
- [x] Regenerate and verify human documentation and agent prompt artifacts.
- [x] Run compiler, backend authoring and native rendering proof.

## Evidence — 2026-10-12

- Compiler acceptance regression first failed because a six-segment cylinder
  compiled successfully; it now rejects all four removed argument forms.
- `cargo test -p ecky-render`: 229 tests pass.
- Backend filter: all 48 `ecky_ir::tests` pass, including direct mesh and
  FreeCAD rejection checks. The substring filter also selected legacy Python
  tests: two pass; the unrelated Thomas parity harness fails because its
  script launches the CLI without the required `cli` feature.
- Authoring and language-source integration suites: 11 tests pass. Generated
  API prompt artifacts match the shared source.
- Language and document projection checks: 14 tests pass; source sync and
  documentation site build pass (23 pages).
- Fresh CLI rejects segment arguments for cylinder, cone, circle, and ring.
- Fresh native CLI exports a five-part STL/STEP fixture containing all four
  round operations and an explicit hexagonal prism. Analytic STEP contains
  four cylindrical surfaces, one conical surface and sixteen planes: two
  end planes per round solid and eight planes for the hexagonal prism.

Follow-up — 2026-10-12: the obsolete Thomas Python converter, tooling target,
reference macro and parity scripts were removed under `cad-transpile-engine`.
Repeating the same backend filter now passes all 48 selected tests, with no
retired converter tests or nested Cargo subprocess.
