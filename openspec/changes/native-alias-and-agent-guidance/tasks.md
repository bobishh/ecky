# Tasks

- [x] Reproduce camera shape alias failure with a native planning integration test.
- [x] Add scalar-folding unit regressions; preserve geometry aliases and scalar values.
- [x] Prove native render parity and continued rejection of invalid shape operands.
- [x] Reproduce incorrect API-only instructions through MCP resource tests.
- [x] Separate mode contracts while preserving shared language content and API artifacts.
- [x] Run relevant Rust suites and synchronize this change with final evidence.

## Evidence — 2026-09-06

- `native_shape_aliases`: 3 passing tests. The camera-cutter fixture first failed
  with the session's exact `union` arg 1 / expected shape / got text error.
  Chained aliases preserve native plan structure in both compiler paths; actual
  text operands still fail compilation.
- Scalar-folding unit tests: 2 pass after the unresolved alias regression first
  returned `Some(String("camera-opening"))` instead of preserving the reference.
- Agent prompt unit tests: 5 pass, including mode, shared-content, and size guards.
- MCP/resource tests: 11 pass. The route regression first failed on the generic
  language guide, then on the older FreeCAD alias. All nine language/backend
  resource routes now use the MCP contract.
- `agent_language_source_contract`: 6 pass, including unchanged generated API
  prompt artifacts and shared canonical language content.
- Real native renders of direct and chained-alias fixtures produce byte-identical
  520-triangle STLs (SHA-256
  `f38a61b023a8a370f47030c1b3523a85f35c7a51715fe9c0876da5a17ad410ef`).
- A snapshot of the current 1,904-line iPhone case, with the failed version's 39
  saved parameters, exports native STL (13,872 triangles) and STEP. Source and
  app history were not modified for this proof.
- Broader `ecky_cad_host::direct_occt` run: 275/290 pass. A control run with only
  the scalar-folding fix removed reproduces the same 15 failures plus the new
  alias regression (274/290 pass). Existing failures involve thread-angle
  planning, runner/domain expectations, runtime fixtures, and parameter-group
  manifest parity; this change does not claim that broader suite is green.
