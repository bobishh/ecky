# Proposal: Params Map Density And Scoped Editing

## Intent

Fix two UX-debt problems in the shipped "New Params" macro AST map
(`src/lib/MacroAstMap.svelte` + `src/lib/macroAstSceneLayout.ts` +
`src/lib/MacroSourcePane.svelte`):

1. **Density.** A part node renders every bound parameter as an always-visible
   module row. Height is `header + ceil(params / columns) × 66px` with at most
   3 param columns, so a part with 30 params becomes a ~700px blob. Semantic
   zoom only hides input overlays at the far tier — the footprint never
   shrinks, and there is no collapse, grouping, or cap.
2. **Edit scope.** Double-clicking a part/model/verify node opens
   `MacroSourcePane` seeded with the ENTIRE macro source; the node's byte range
   is only a highlight decoration. APPLY submits the whole document through
   `onApplyMacroCode`, i.e. every "in-place" edit is a full macro rewrite —
   against the AGENTS.md mandate to prefer scoped AST patches over full macro
   rewrites. It also carries a live bug: switching the pane to another node
   keeps the current draft text but installs byte offsets computed against the
   pristine `macroCode`, so once the draft has diverged the highlight and
   scroll land on the wrong bytes.

## Problem (evidence)

- `macroAstSceneLayout.ts:116-138` — part height grows linearly with param
  count, unconditionally; `resolvePartColumns` caps columns at 3.
- `MacroAstMap.svelte:847-850` — far zoom tier hides `.macro-ast-node__overlay`
  via CSS only; node geometry is unchanged.
- `MacroAstMap.svelte:405-429` (`openMacroNodeEditor`) — pane state seeded with
  full `macroCode`; on re-open for a different node only `label/scopeStart/
  scopeEnd` are replaced while the (possibly edited) draft `code` is kept.
- `MacroSourcePane.svelte` — editor doc is the full source; scope is a
  `Decoration.mark`, not an edit boundary; `onApply(currentCode())` returns the
  full document.
- AGENTS.md mandate: "Prefer AST patch operations over full macro rewrites."

## Scope

- **Collapsible part density in the map.** Part nodes whose param count
  exceeds a threshold render collapsed by default: header, syntax badge, and a
  param-count chip instead of the param grid. Expanding is an explicit map
  interaction and is remembered per node for the session. The layout engine
  computes collapsed heights (constant, param-count independent) and omits
  param module nodes for collapsed parts. Focus flows (search/highlight
  `focusNodeId`, `highlightParam`, param focus from errors) auto-expand the
  owning part before focusing.
- **Scoped source editing.** The source pane edits only the selected node's
  byte slice. The pane doc is the slice; APPLY splices the edited slice back
  into the base document and submits the spliced whole through the existing
  `onApplyMacroCode` flow. Switching nodes with a dirty draft is refused with
  an inline message instead of silently misaligning offsets. The ADD PART
  ghost-slot flow keeps its template mechanics but the pane shows only the
  template slice.

## Out of Scope

- Backend changes. The splice is client-side; the existing apply/preview flow
  is unchanged. (A true backend byte-range patch command already exists for
  MCP flows and is not rewired here.)
- Grouping params inside an expanded part (roles, sections) — that is
  `ast-visual-blocks` territory.
- Search-focus zoom/pulse behavior and the in-map verification layer — those
  remain `macro-ast-map-editor` sections 3 and 5.
- Rebasing an open pane when the macro changes externally mid-edit. Today the
  whole-document pane has the same staleness window; this change neither fixes
  nor worsens it, and the splice base is snapshotted at pane-open time.

## Approach

BDD dual-loop per AGENTS.md, two sequential slices (both write to
`MacroAstMap.svelte`, so no parallel workers):

1. **Scoped editing** (bug fix first): extract a pure
   `spliceMacroSource(base, start, end, slice)` helper with unit tests;
   rework `MacroSourcePaneState` to hold the base snapshot + slice; simplify
   `MacroSourcePane` to a slice editor (no scope decoration — the whole doc is
   the scope); dirty-switch guard; update the existing in-place-edit and
   ADD PART Playwright specs red-first.
2. **Density**: extend `buildMacroAstSceneLayout` hints with
   `expandedPartIds`; collapsed-part layout + count chip + toggle in
   `MacroAstMap.svelte`; auto-expand on param focus; unit tests for layout
   invariants and Playwright proof for collapse/expand/edit.
