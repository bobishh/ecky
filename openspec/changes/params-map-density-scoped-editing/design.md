# Design: Params Map Density And Scoped Editing

## Decisions

### D1. Client-side splice, no backend change

APPLY keeps submitting a full macro string through the existing
`onApplyMacroCode` flow (preview render + version apply already hang off it).
The scoping happens client-side: the pane doc is the node slice, and apply
computes `base.slice(0, scopeStart) + paneText + base.slice(scopeEnd)`.

Rejected: wiring the UI to a backend byte-range patch command. The apply
pipeline (preview, error surface, version flow) is built around full-source
apply; rerouting it is a bigger change with no observable benefit here. The
mandate's intent — the author edits only the node, not the document — is met.

### D2. Pane state holds a base snapshot

`MacroSourcePaneState` becomes:

```ts
type MacroSourcePaneState = {
  label: string;
  /** Full macro document the slice offsets are valid against. */
  baseCode: string;
  scopeStart: number;
  scopeEnd: number;
  busy: boolean;
  error: string | null;
  revision: number; // forces CodeMirror doc rebuild on slice swap
};
```

`baseCode` is snapshotted when the pane opens (for ADD PART it is the draft
with the template already spliced in, so the slice is exactly the template).
The editor receives `baseCode.slice(scopeStart, scopeEnd)`. This keeps the
ADD PART flow and the node-edit flow on one mechanism.

Known limitation (unchanged from today): if the macro changes externally while
the pane is open, the snapshot is stale and APPLY resubmits the snapshot's
surroundings. The whole-document pane had the same window; not worsened, not
fixed.

### D3. Dirty-switch guard instead of draft-preserving offset swap

The current bug: switching nodes keeps the draft text but installs offsets
computed against pristine `macroCode`. With slice editing the equivalent
"keep draft, move scope" is meaningless (the doc IS the scope), so:

- clean pane → switching nodes rebuilds the pane with the new slice
  (bump `revision`);
- dirty pane → refuse the switch, keep the draft, set an inline notice in the
  existing `error` slot (e.g. "Draft has unsaved edits — APPLY or CLOSE
  before editing another node."). No modal dialogs.

Dirtiness = editor doc differs from the original slice. `MacroSourcePane`
exposes it via a `onDirtyChange`/bindable or an exported `isDirty()` —
implementer's choice, but the guard lives in `MacroAstMap` where the switch
originates.

The guard applies only to author-initiated node selection (dblclick/keyboard,
ADD PART). System-driven jumps — diagnostic retarget after a failed apply,
external focus requests via `focusNodeId` — bypass it: the draft was already
rejected or superseded, and the retargeted pane keeps the existing error so
the raw backend message stays visible at the responsible node.

### D4. Scope decoration dies

`MacroSourcePane` drops the `StateField`/`Decoration` scope-highlight
machinery and the `scopeStart/scopeEnd` props: the whole document is the
scope. It keeps: label, busy, error, APPLY/CLOSE, `currentCode()`.

### D5. Density threshold and layout contract

- `PART_COLLAPSE_THRESHOLD = 6` params. `> 6` → collapsed by default.
- `buildMacroAstSceneLayout(projection, hints)` gains
  `hints.expandedPartIds?: ReadonlySet<string>`.
- Collapsed part: fixed height (`partHeaderH` + chip row + padding, ~64px),
  no param module nodes emitted, no param connectors. Minimap is unaffected
  (it already renders parts only).
- Expanded part (`expandedPartIds.has(id)` or count ≤ threshold): exactly
  today's layout.
- `MacroAstMap.svelte` owns `expandedParts = $state<Set<string>>` (session
  memory only, no persistence). Toggle = click on the count chip / a dedicated
  expand affordance on the part header; dblclick keeps meaning "edit source".
- Auto-expand: `focusSceneField` / `selectSceneFieldValue` /
  `highlightedParamKey`-driven focus must, when the param's owning part is
  collapsed, add the part to `expandedParts` and defer the DOM focus to after
  the re-layout (existing rAF pattern).

### D6. Test placement

- Unit (`tsx --test`): `macroAstSceneLayout.test.ts` — collapsed height is
  param-count independent; collapsed parts emit no param nodes;
  `expandedPartIds` restores the full grid; threshold boundary (6 vs 7).
  New `spliceMacroSource` unit tests: replace middle, at start, at end,
  empty replacement, degenerate range.
- Playwright (`e2e/params.spec.ts`): update the existing in-place-edit
  (~line 1149) and ADD PART (~line 1251) specs for slice-only pane; add
  dirty-switch guard spec; add collapse/expand/edit spec for a dense part.
  Red first, per AGENTS.md.
