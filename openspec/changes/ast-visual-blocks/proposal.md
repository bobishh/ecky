# Proposal: AST Visual Construction Blocks

## Intent

Add a semantic block view for `.ecky` CAD models: a collapsible construction
tree projected from the existing Lisp AST, where a human can understand a
non-trivial model by expanding groups, reading roles (solid / cutter / profile /
derived value), tracing derived dimensions, previewing intermediate shapes, and
jumping between blocks and source text in both directions.

The `.ecky` source stays the canonical model. The block view is an inspection
and (for parameters only) editing projection — not a new graphical CAD format
and not a second model that can drift.

This is a sibling of `macro-ast-map-editor`: that change builds a spatial
scene-map projection; this change builds the semantic tree/outline projection.
Both must share one AST identity model (stable node ids + byte spans), one
source-map backend, and one selection contract, so a node selected in either
surface is the same source-backed entity.

## Problem (evidence)

Raw `.ecky` text is readable only while a model is small. In a real part, a
long `let*` is a flat list of dozens of bindings, and the reader has to mentally
reconstruct:

- which bindings are structural solids vs subtractive cutters vs 2D profiles vs
  derived scalar dimensions;
- which transform chain belongs to which physical feature — e.g.
  `(translate x y z (rotate 0 -90 0 (extrude (rounded-rect h l r) depth)))` is
  valid CSG but does not communicate "right-side power button through-wall
  cutter";
- which boolean produces the final part, and whether each cutter actually
  reaches the wall it is supposed to cut.

This is acute for LLM-generated CAD: the agent emits syntactically valid
geometry that is mechanically wrong, and today the only inspection surfaces are
raw text and the final rendered solid. Intermediate shapes (individual cutters,
accumulated model up to a binding) cannot be previewed in the UI, and
verification results land in a global log instead of on the node that failed.

Existing infrastructure this change builds on (investigated, not assumed):

- `commands/macro_ast.rs` — `macro_ast_source_map` returns byte ranges for
  top-level model clauses (`model`, `params`, `part`, `verify`), but does NOT
  descend into `let*` bindings inside a part, so the map cannot address the
  bindings that matter.
- `ecky_core_ir` — `CoreNode` carries `span: Option<SourceSpan>` and
  `CoreNodeKind::{Build, Let, Call, ...}`; MCP `ecky_ast_get` already walks
  nodes with paths like `.../let/bindings/<name>`.
- `mcp/handlers/ecky_ast/shape_graph.rs` — packet with parts, constraints,
  dependencies, debug overlays.
- Frontend `MacroAstMap.svelte`, `macroAstMap.ts` (node kinds
  `model|part|port|param|verify`), `MacroSourcePane.svelte` (scoped CodeMirror
  editing over byte ranges).
- Preview pipeline: `macro_buffer_preview_render` / `params_preview_render`;
  debug overlays are preview-only by mandate.

## Scope

- Deep source map: extend the AST source-map projection to descend into `let*`
  bindings inside `part`, with stable ids, byte spans, and binding names.
- Role inference: classify each binding/expression as parameter, derived value,
  profile, solid, cutter, transform, boolean, or verification. First version
  infers from binding-name suffixes and usage (operands of `difference` after
  the first are cutters); the inferred role is always visible and marked as
  inferred, leaving room for future explicit annotations.
- Construction groups: present a long `let*` as collapsible groups (derived
  dimensions, structural solids, cutters, final boolean), not one flat list.
- Derived values: show formula and evaluated value together
  (`outer-case-width = 77.2 = phone-pocket-width + 2 × side-wall-thickness`),
  with clickable references that select the source binding.
- Geometry summaries: collapse transform chains into a compact summary
  ("translated rotated extrusion") expandable to translation vector, rotation
  angles, extrusion depth, profile dimensions.
- Debug previews: select any binding and preview only that shape, the
  accumulated model up to that binding, or all cutters together — via the
  existing preview pipeline, rendered as preview-only debug geometry.
- Final boolean structure: the `difference`/`union` producing the part shows
  base solid and subtractive operands explicitly, cutters badged.
- Node-attached verification: existing verify results render next to the block
  they concern; the attachment contract accepts future semantic checks
  (cutter-reaches-wall, min-wall-thickness) without UI rework.
- Bidirectional sync: block selection highlights the source span; source cursor
  selects the containing block; collapsing never loses access to the raw form.
- Parameter editing from the block view through the existing AST-patch flow.
  Everything else is read-only in this change.
- Acceptance fixture: a non-trivial phone-case `.ecky` model with a long `let*`
  (pocket cutter, rim cutter, camera cutters, port cutter, button cutters,
  final difference) added under `src-tauri/tests/fixtures/cad/`.

## Out of Scope

- A new high-level DSL or any change to `.ecky` syntax (explicit role
  annotations are future work; this change only reserves the concept).
- Editing geometry, inserting nodes, or authoring verification from the block
  view (that is `macro-ast-map-editor` territory).
- Semantic verification checks themselves (cutter-reaches-cavity, wall
  thickness); only the node-attachment surface they will plug into.
- Freeform node-graph wiring, spatial layout (belongs to the map view).
- Emitting debug preview geometry into STL/STEP exports.

## Approach

1. Backend first: deepen `macro_ast_source_map` (or a sibling
   `macro_ast_block_tree` command) to return the block tree — ids, spans,
   binding names, inferred roles, evaluated scalar values, reference edges —
   compiled from the same parse the renderer uses.
2. Frontend tree: collapsible block components reusing the AST identity and
   selection contract from `macroAstMap.ts`, side-by-side with
   `MacroSourcePane` for sync.
3. Previews: wire binding selection to the preview pipeline for
   shape-only / accumulated / all-cutters debug renders.
4. Verification attachment: map existing verify results onto node ids.
5. Parameter editing reuses the existing param patch path.

BDD dual-loop per AGENTS.md: each slice starts from a failing integration test
(Rust for projection/eval, Playwright for the view), then unit red-green.
