# Proposal: Macro AST Map Editor

## Intent

Add a source-backed `New Params` visual view beside the existing parameter
panel, then grow it into a broader macro AST authoring surface after proof.
Primary loop: find the target node in the map, apply a structured AST patch,
verify the source roundtrip.

Authors should first see params as spatial source projection: nested ownership,
ports, values, and affected regions rendered near the place they influence the
model. Changing values happens in that view. The existing parameter panel stays
available until the new flow proves editing, failure display, search focus,
persistence, and preview updates.

The visual metaphor should not lock to literal molecular biology. Target
direction is Vertex/futuristic: dark irregular plates, luminous contours,
ports, traces, and focused regions. Product contract stays technical: source
remains authority, map is projection, every interaction roundtrips through
structured AST operations.

## Scope

- Add a separate `New Params` view or mode without replacing the existing
  parameter panel.
- Render `.ecky` macro source params as an AST-backed visual scene, not a
  literal nested DOM tree.
- Preserve current `.ecky` source as source of truth.
- Give every rendered AST node stable identity across parse, layout, selection,
  patch, preview, undo, verification links, and agent-assisted find/apply.
- Show macro hierarchy as nested visual regions for model, parts, repeated
  structures, constraints, params, and verify clauses.
- Render authored inputs as ports on the region they feed.
- Render numeric, enum, boolean, and text params as compact modules attached
  to their owning AST node, not as full-width list rows.
- Change params through inline controls in the new view while the old parameter
  panel remains available elsewhere.
- Search params and source-backed labels from the new view.
- Focus and frame the matching visual region when search selects a result.
- Use SVG as the primary structural layer, HTML as the editable overlay, and
  canvas only as an optional background or glow underlay.
- Let a subagent or worker surface candidate matches, but keep AST projection
  and patch application authoritative in the backend.
- Persist all visual edits through existing source/version/config flows.
- Convert inline map edits into AST patches, not string splices.
- Support a find-and-apply loop where a search result or agent suggestion can
  be applied as a structured AST patch.
- Let authors click a valid insertion region and create a new part, input,
  relation, repeat, instance, or verify clause.
- Let authors type into a focused insertion point and resolve typed intent into
  a structured AST patch.
- Show backend/provider error bodies at the node whose edit caused the failure.
- Render verification clauses as first-class map nodes or overlays.
- Let authors create named physical fit constraints from selected geometry or
  selected AST nodes.
- Keep debug overlays preview-only and out of export geometry.
- Keep Tauri boundary naming idiomatic: camelCase in frontend payloads,
  snake_case in Rust structs/functions, `#[serde(rename_all = "camelCase")]`
  on boundary structs.
- Prove UI behavior on real routes with Playwright BDD before claiming UI
  completion.
- Use parallel subagents for disjoint read/write scopes, then merge through the
  spec artifacts and proof gates.

## Out of Scope

- Replacing `.ecky` source syntax with a new storage format.
- Canvas-only state that cannot regenerate source.
- Anonymous geometry offsets for physical fit relations.
- Copy-paste authored repeated shelves/ribs/clips/doors/corridors instead of
  `repeat` or `instance`.
- Separate agent status bar or live agent terminal stream in app logs.
- Emitting debug overlay primitives into STL/STEP production exports.
- Direct SQLite writes.
- Broad unrelated theme redesign.

## Product Shape

The map has four visible layers:

- Structure layer: model, part, feature, repeat, instance, and nested blocks.
- Port layer: inputs, references, and dataflow from params into parts.
- Control layer: inline param controls anchored to owning AST nodes.
- Verification layer: named constraints, measurements, checks, pending states,
  pass/fail/error states, and raw error bodies.

Each layer is optional at view time, but all visible elements share the same AST
identity model. Selecting a node in one layer selects the same source-backed
entity everywhere.

## Find And Apply

The rollout loop is intentional:

1. Find the relevant source-backed node by search, region focus, or agent
   suggestion.
2. Apply a structured AST patch to that node.
3. Reparse, reproject, and verify identity unless semantics changed.

Subagents may help locate candidates, rank matches, or prepare proof slices.
Subagents do not own source truth, patch application, or final acceptance.

## Approach

Phase work by contract, not by visual polish:

1. Define AST identity and params map projection contract.
2. Render source-backed `New Params` view for existing macros.
3. Move parameter editing inline through AST patches inside the new view.
4. Add insertion/editing actions for new AST nodes.
5. Add verification authoring and result overlays.
6. Consider retiring detached parameter panel only after inline happy path,
   failure state, search focus, persistence, and preview proof pass.

## Non-Goals

- The first implementation does not need final visual art direction.
- The first implementation must not require removing the current parameter
  panel.
- The first implementation does not need freeform node graph wiring.
- The first implementation does not need LLM-specific controls.
- The first implementation does not need rich animation.

## Success Criteria

- An author can open `New Params` and understand source-backed params in spatial
  context while the old parameter panel still exists.
- Editing an inline control updates source, preview, and persisted version state.
- Failed edits show exact raw backend/provider error at the responsible node.
- Searching a parameter focuses the matching region of the map.
- New parts and inputs can be authored from the map without opening a separate
  parameter panel.
- Verification intent can be created and inspected in-map.
- Browser proof covers one happy path and one failure or pending path for every
  UI increment.
