# Proposal: Semantic AST Authoring

## Intent

Make the `.ecky` language carry design intent in its AST — construction
groups, explicit binding roles, node metadata, intent-preserving cut helpers,
inspectable derived dimensions, and node-attached semantic checks — so a
visual editor can render meaningful blocks without guessing from variable
names, and validators can check what a shape is *for*, not only whether the
final STL is manifold.

This is not a new high-level DSL. Generated code stays normal CAD Lisp; the
change is that structure which today lives in comments, naming conventions,
and the reader's head becomes first-class, machine-readable AST.

## Problem (evidence)

A real part today is one long `let*` where everything is just a value —
number, profile, solid, cutter, temporary helper, final body — and meaning is
recovered late or never:

- A cutter is only "some shape that later appears as a non-first operand of
  `difference`". Its purpose is invisible until final assembly, so nothing
  can validate earlier that `phone-body-pocket-cutter` actually reaches the
  pocket it is named after.
- Grouping exists only as comments between bindings. `ast-visual-blocks`
  falls back to section-comment titling precisely because the AST has no
  group nodes.
- `(translate x y z (rotate 0 -90 0 (extrude profile depth)))` is valid CSG
  that does not encode "right-side through-wall slot"; LLM generation
  repeatedly gets the direction wrong because the language offers no form
  where the direction is named instead of derived.
- `meta` clauses are already parsed and silently discarded
  (`ecky_scheme/compiler.rs`, expanded-model clause loop) — there is no way
  to attach a label, units, or an intended target to a node.
- Verification (`verify` clauses, structural checks) addresses parts and the
  whole artifact, not semantic nodes: "this cutter intersects the outer
  solid" is not expressible.

`ast-visual-blocks` (sibling change) builds the block-view projection over
today's language and explicitly defers "explicit role annotations" as future
work. This change is that future work on the language side: it feeds the
same projection with declared — not inferred — structure.

## Scope

1. **First-class construction groups.** A section/group form usable inside a
   `part` binding sequence (derived dimensions, construction profiles,
   structural solids, cutters per feature, final boolean assembly). Groups
   survive parsing into the AST as named, nestable nodes; binding scope stays
   sequential across group boundaries; geometry evaluation is unaffected.
2. **Explicit binding roles.** A binding can declare a role — `dimension`,
   `profile`, `solid`, `cutter`, `transform`, `boolean-result`,
   `verification` — carried in the AST from parse time. Declared roles
   override the name/usage inference from `ast-visual-blocks`; a declared
   role contradicting actual usage (a declared `cutter` never subtracted) is
   a diagnostic, not a silent re-classification.
3. **Node metadata.** A lightweight annotation surface (label, description,
   role, group, units, intended target, debug visibility, printability
   notes) attachable to bindings, groups, and shape expressions. Metadata is
   preserved in the parsed AST and manifests, readable by UI and validators,
   and ignored by geometry evaluation and export digests.
4. **Semantic cut helpers.** Intent-preserving forms for the recurring
   dangerous CSG patterns: through-wall slot, rear-panel circular hole,
   front-opening rim cut, pocket/cavity cutter, centered port cut,
   side-button cut. Each takes named sides/faces/walls instead of raw
   rotations, desugars to ordinary CSG identically on all three backends
   (parity-tested), and keeps the original helper form and its named intent
   in the AST.
5. **Inspectable derived dimensions.** Derived values keep their formula as
   AST alongside the evaluated value, so both
   `outer-case-width = phone-pocket-width + 2 * side-wall-thickness` and
   `77.2` are queryable, with reference edges to the bindings the formula
   uses. (Display is `ast-visual-blocks`; this change guarantees the data
   survives compilation and reaches the projection/manifest.)
6. **Source ↔ AST one-to-one.** Every new form obeys the existing identity
   contract: stable node id + exact byte span, round-trips through
   parse/emit-back unchanged, and is addressable by `ecky_ast_*` patch
   operations. The visual editor remains a projection of source, never a
   second model format.
7. **Node-attached semantic checks.** Check forms that attach to a binding
   or group and verify semantic relationships: cutter intersects target
   solid, cutter reaches a named cavity, wall stays above minimum thickness,
   rim overlap is usable, hole contains an expected center point, cut is
   centered on a wall. Checks run in the existing verify pipeline, and their
   results carry the anchor node id required by the `ast-visual-blocks`
   attachment contract.
8. **Acceptance fixture.** The phone-case fixture from `ast-visual-blocks`
   re-authored with groups, roles, metadata, helpers, and semantic checks,
   compiling to the same geometry (digest-compared) as the flat version.

## Out of Scope

- The block-view UI itself — rendering groups, role badges, previews
  (`ast-visual-blocks`).
- A product-specific DSL (`make-phone-case`) or any form that cannot desugar
  to today's CSG core.
- Removing or deprecating flat `let*`, raw `translate`/`rotate`/`extrude`,
  or inference-based roles; all existing models must compile unchanged.
- New geometry kernels or backend capabilities; helpers compose existing
  primitives only.
- Constraint solving; declared relationships are checked, not solved.

## Approach

Language surface first, one slice at a time, each driven by a failing test
per AGENTS.md BDD dual-loop:

1. Metadata carrier: stop dropping `meta`; define the annotation attachment
   points on bindings/groups/expressions; thread through Core IR
   (`CoreNode`) and manifests; prove export digests unchanged.
2. Groups: parse/emit-back/identity for the group form; scope-transparency
   tests; projection sees group nodes instead of comment heuristics.
3. Roles: declared-role syntax on bindings; precedence over inference;
   usage-contradiction diagnostics.
4. Derived dimensions: formula-preserving compilation + reference edges in
   the manifest.
5. Helpers: one helper at a time, per the `language-convenience-stdlib`
   per-op recipe (surface → Core IR → three lowerings → native-vs-build123d
   parity), plus an AST-intent-preservation test.
6. Semantic checks: check forms lower into the verify pipeline; results
   anchored by node id; red blocks commit like existing verify.

## Relationship to other changes

- `ast-visual-blocks` — consumer. Declared groups/roles/metadata replace its
  inference and comment heuristics when present; its node-attached
  verification surface is where semantic check results land.
- `macro-ast-map-editor` — shares the AST identity model; new forms must be
  addressable by the same stable ids and patch ops.
- `language-convenience-stdlib` — helpers here follow its per-op parity
  recipe; the difference is that these forms also preserve intent in the AST
  rather than being plain primitives.
- `parametric-thread-feature` — precedent for a structural primitive whose
  AST keeps high-level intent while lowering to CSG.
