# Design: AST Visual Construction Blocks

## Goal

A read-mostly semantic tree projection of `.ecky` models: construction groups,
roles, derived values, geometry summaries, per-binding debug previews, and
node-attached verification, all backed by the same AST identity model as the
spatial map view.

## Decisions

- `.ecky` source remains canonical; the block tree is a projection rebuilt from
  every parse. No block-view state may exist that cannot be regenerated from
  source.
- One AST identity model across surfaces. Block ids reuse the semantic-path
  scheme from `macro-ast-map-editor`
  (`model/part:case/let/bindings/pocket-cutter`), extended one level: `let*`
  bindings inside `part` become addressable nodes with byte spans. The existing
  `ecky_ast_get` path grammar (`.../let/bindings/<name>`) is the reference.
- Backend owns parse, id assignment, byte spans, role inference, scalar
  evaluation, and reference edges. Frontend owns tree rendering, collapse
  state, selection, and preview requests. Frontend never re-infers roles.
- The projection is served by a Tauri command beside `macro_ast_source_map`
  (working name `macro_ast_block_tree`), returning one tree per `model` form.
  It parses the ORIGINAL text via `Parser::parse_without_lowering` for spans
  and cross-checks against the compiled `CoreProgram` for evaluation and
  reference edges — same dual sourcing `macro_ast.rs` already uses.
- Boundary structs use `#[serde(rename_all = "camelCase")]`; TS payloads
  camelCase, Rust snake_case.
- Debug previews go through the existing preview pipeline and debug-overlay
  channel. They never enter export geometry (STL/STEP mandate).
- Parameter editing is the only mutation, and it goes through the existing
  structured param patch flow — no string splicing from the block view.
- UI follows Tactical Midnight, square borders, bronze accents; containers
  `overflow: hidden`.

## Role Inference

Each block node carries `role` + `roleSource` (`inferred-name`,
`inferred-usage`, `explicit` reserved for future annotations). Precedence,
strongest last:

1. Structural default by head: `params` children → `parameter`; scalar-valued
   `let*` binding (evaluates to number/bool) → `derived-value`; 2D constructor
   head (`rounded-rect`, `circle`, `polygon`, `profile`) → `profile`;
   `translate`/`rotate`/`scale` wrapping → `transform` facet on the summary;
   `union`/`difference`/`intersection` → `boolean`; `verify` clauses →
   `verification`; other shape-valued bindings → `solid`.
2. Name suffix: `-solid|-body|-bumper|-rib|-boss` → solid;
   `-cutter|-cut|-opening|-hole|-pocket` (as suffix) → cutter.
3. Usage: any operand of `difference` after the first → cutter, including
   through reference chains (a binding referenced only as a subtrahend is a
   cutter even if its name says nothing). Usage beats name; conflicts surface
   as a role-conflict marker instead of silently picking one.

Inference is expected to be imperfect; the UI must show the role AND its
source, so a wrong inference reads as "inferred", not as ground truth.

## Grouping

A long `let*` is segmented into collapsible construction groups:

- Section comments (`;; ...` lines between bindings) start a named group with
  the comment text as title — the zero-cost authoring convention.
- Otherwise contiguous runs of same-role bindings group by role (derived
  dimensions, profiles, structural solids, cutters).
- The binding chain feeding the part result (`difference`/`union` tail) is
  always its own "final boolean" group.

Groups are projection-only: no group syntax is added to the language.

## Derived Values

For a scalar binding the node exposes `formula` (exact source slice),
`evaluatedValue` (from compile-time evaluation with current params), and
`references` (list of `{name, nodeId}` for identifiers in the formula, from
core IR `Reference` nodes). Clicking a reference selects that node. Bindings
that cannot be evaluated statically (depend on geometry probes) show the
formula with value "runtime".

## Geometry Summaries

For a shape binding, the backend flattens the wrapping chain into a summary
struct: ordered transform list (translate vector, rotate angles), base
operation (`extrude` depth/direction, `cylinder` r/h, ...), and profile
parameters when the base is a profile constructor. Frontend renders the
collapsed one-liner ("translated rotated extrusion, depth 2.4") and the
expanded detail. Bounding boxes come from preview renders when one has been
requested, not from static analysis, and are marked as such.

## Debug Previews

Three preview requests, all keyed by node id:

- `shape-only` — render just the selected binding's shape;
- `accumulated` — render the model with the `let*` truncated after the selected
  binding, showing construction state up to that point;
- `all-cutters` — render every cutter-role shape together as debug overlay over
  the base solid.

Implementation reuses the macro-buffer preview path: backend synthesizes a
preview variant of the model whose part result is the requested selection and
runs it through the normal render pipeline, tagged as debug so the mandate
(preview-only, never export) is enforced in one place.

## Verification Attachment

Verify clauses are already core IR (`verify_clauses`). Each clause gets a node
id and, where its selectors name a binding or part, an `anchorNodeId`. Results
render at the anchor block with pass/fail/error and the raw message; clauses
with no resolvable anchor attach to the part block. Future semantic checks
(cutter-reaches-wall, min-wall-thickness) plug in by emitting results carrying
an anchor node id — no UI change needed.

## Source Sync

Both directions run on byte spans from the same parse:

- block → source: selection sets `MacroSourcePane` scope/highlight to the
  node's span;
- source → block: cursor offset selects the innermost block whose span contains
  it, expanding collapsed ancestors.

Spans are recomputed on every source change; ids are stable across unrelated
edits per the identity rules in `macro-ast-map-editor`.

## Rejected Paths

- Frontend-only projection from raw text (regex/paren counting). Rejected:
  roles and evaluation need the compiler; two parsers would drift.
- Extending `macro_ast_source_map`'s flat list in place. Rejected: block tree
  needs nesting, roles, values, and edges; keeping the flat command untouched
  protects the existing map view during rollout.
- Explicit role annotations in the language now. Rejected for this change:
  inference covers the MVP and annotation syntax deserves its own design; the
  contract only reserves `roleSource: "explicit"`.
- Computing bounding boxes statically in Rust. Rejected: duplicates geometry
  kernel work; preview-derived boxes are cheap and honest.
- A second selection/identity scheme for the tree. Rejected: map view and
  block view must select the same entities or the two surfaces will fight.

## Data Contract (shape, not final)

```text
MacroAstBlockTree {
  root: BlockNode
}
BlockNode {
  id, kind,                      // part | group | binding | boolean | verify | param
  label, role, roleSource,
  span { startByte, endByte },
  formula?, evaluatedValue?, references?: [{name, nodeId}],
  geometrySummary?: { transforms[], base, profile? },
  verify?: { status, message, anchorNodeId },
  children: [BlockNode]
}
```
