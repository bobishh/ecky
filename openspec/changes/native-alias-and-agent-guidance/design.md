# Design

Scalar evaluation must distinguish an environment value from a symbolic shape
reference. The stringish evaluator accepts unresolved symbols as text for other
language uses; it is not evidence that an untyped binding is a scalar string.
Native normalization retains references absent from its scalar environment.
Existing Core IR validation before planning remains authoritative.
Source compilation already validates Core IR; the defect occurred afterward in
native normalization. No duplicate compiler check or relaxed type rule is added.

Language prose stays in `public/docs/ecky-agent-reference.md`; operation examples
stay registry-generated. `agent_prompt.rs` assembles the same body beneath a
mode-specific contract. Existing MCP resource URIs serve the MCP variant, while
API generation and committed API prompt artifacts retain their API contract.
Older FreeCAD, CAD SDK, Ecky Rust, and mesh guide aliases use the same MCP variant.
This corrects the archived `agent-prompt-single-source` design's accidental
sharing of operating instructions as well as language content.

Proof uses native planning integration tests, scalar-folding unit tests, real
native renders, and tests reading the MCP resource routes. No UI changes.
