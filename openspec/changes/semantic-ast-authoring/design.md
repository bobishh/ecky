# Design: Semantic AST Authoring

## Constraints (from investigation)

- `ecky_scheme/compiler.rs` special forms today: `model`, `params`,
  `part`/`feature`, `let`/`let*`, `verify`, `tag`, `repeat`, `instance`.
  A `meta` clause is parsed in the expanded-model clause loop and dropped.
- `ecky_core_ir::CoreNode` has `span: Option<SourceSpan>` and
  `CoreNodeKind::Group(_)` already exists (emit-back renders it as
  `"group"`), so the IR has a natural landing spot for group nodes.
- `ast-visual-blocks` defines the identity contract: stable node id + byte
  span from the same parse the renderer uses; `ecky_ast_*` patches address
  nodes by path (`.../let/bindings/<name>`).
- Three lowerings (native OCCT, build123d, FreeCAD) must stay in parity;
  anything that changes geometry needs the differential harness.
- AGENTS.md mandates: fit-critical dimensions are named bindings; repeated
  structures use `repeat`/`instance`; debug overlay never enters exports.

## Decision 1 — Syntax strawman (spec is syntax-agnostic where possible)

Spec deltas mandate AST properties; concrete surface below is the working
proposal, finalizable during implementation without re-approving the spec.

### Groups

```lisp
(part case
  (let*-groups
    (group dimensions
      (outer-width  (+ pocket-width (* 2 side-wall)))
      (outer-height (+ pocket-height base-wall lip-height)))
    (group profiles
      (body-profile (rounded-rect outer-width outer-height corner-r)))
    (group solids
      (body (extrude body-profile outer-depth)))
    (group cutters
      (pocket-cutter :role cutter
        (pocket-cavity pocket-width pocket-height pocket-depth)))
    (group assembly
      (case-body (difference body pocket-cutter))))
  case-body)
```

- `let*-groups` (name TBD; could be `let*` accepting `group` items) keeps
  **one sequential scope across all groups** — a binding in `dimensions` is
  visible in `cutters`. Groups are organizational, not scoping, constructs.
  Rationale: per-group scopes would force artificial re-exports and break
  the "still normal CAD Lisp" goal.
- Groups nest. Group name is an identifier; optional `:label "..."` for
  human titles.
- Compiles to `CoreNodeKind::Group` wrapping the bindings' nodes; geometry
  evaluation flattens groups away.

### Roles

Role rides on the binding, keyword-style, before the value expression:

```lisp
(camera-opening-cutter :role cutter :target case-body
  (through-hole ...))
```

- Allowed roles: `dimension`, `profile`, `solid`, `cutter`, `transform`,
  `boolean-result`, `verification`. Closed set for now; unknown role =
  compile error (typo protection beats extensibility here).
- Precedence: declared > usage-inferred > name-inferred. Declared role that
  contradicts observed usage (declared `cutter` never used subtractively;
  declared `dimension` bound to a shape) → diagnostic with span, not silent
  override. Warning severity by default; verify pipeline can escalate.

### Metadata

Reuse the keyword channel on bindings/groups plus a general wrapper for
bare expressions:

```lisp
(side-wall :role dimension :units mm
           :doc "Wall between pocket and outer face" 2.4)

(annotate (:label "USB-C cut" :debug-visible true :printability "min 0.8mm bridge")
  (centered-port-cut ...))
```

- Metadata keys: `:label`, `:doc`, `:role`, `:units`, `:target`,
  `:debug-visible`, `:printability`. Open set — unknown keys are preserved
  in the AST and ignored (metadata is data, unlike roles which gate
  semantics).
- Stored on `CoreNode` as an optional metadata map (string → literal);
  serialized into manifests; **excluded from export digest inputs** and from
  all lowerings. Existing top-level `meta` clause stops being dropped and
  becomes model-level metadata under the same representation.

### Semantic cut helpers

Named-side forms, no raw rotations in the signature:

```lisp
(through-slot  :face right  :width w :height h :corner-r r :at (x z))
(panel-hole    :face back   :diameter d :at (x y))
(rim-cut       :face front  :depth d :inset i)
(pocket-cavity w h d :open-face front)
(centered-port-cut :face bottom :width w :height h)
(side-button-cut :face left :length l :height h :at z)
```

- `:face` takes the model-frame names already used by selectors
  (`front/back/left/right/top/bottom`).
- Each helper desugars in the compiler to today's CSG (profile + extrude +
  transforms) so all three lowerings see only existing primitives — **no
  new backend ops**, hence parity risk is confined to one desugar site.
  This is deliberately unlike `language-convenience-stdlib` primitives.
- Desugar guarantees over-cut: through-cuts extend past both walls by a
  fixed epsilon so "slot ends exactly at surface" coincident-face bugs
  cannot be authored via the helper.
- The AST keeps the helper form itself (head, named args, span, id); the
  desugared subtree is derived, addressable for debugging but not the
  patch target. Emit-back reproduces the helper form, not the expansion.

### Semantic checks

Check forms live in `verify` but name semantic nodes:

```lisp
(verify
  (check cutter-intersects  :cutter pocket-cutter :target body)
  (check cutter-reaches     :cutter usb-cut      :cavity pocket-cutter)
  (check min-wall           :between (pocket-cutter body) :min 1.6)
  (check contains-point     :node camera-hole :point (cam-x cam-y cam-z))
  (check centered-on-face   :node port-cut :face bottom :axis x))
```

- Implemented on rendered geometry via existing measurement machinery
  (bbox/volume/distance probes), not a constraint solver: e.g.
  `cutter-intersects` = intersection volume > 0; `cutter-reaches` =
  boolean of cutter with cavity region non-empty; `min-wall` = distance
  probe between resulting faces.
- Results carry `{check, anchor_node_id, measured, expected, pass}` —
  anchor id is the named binding/group — feeding the `ast-visual-blocks`
  node-attachment contract directly.
- Red semantic checks behave exactly like red `verify`: retried in the
  generation loop, block silent commit, reported honestly at cap.

## Decision 2 — Where structure lives in the pipeline

Parse → expanded model → Core IR → lowerings. Groups, roles, metadata, and
helper intent must survive to Core IR (that is what projections and
manifests read); lowerings receive the flattened/desugared view. One new
compiler pass boundary: `desugar_semantic_helpers` runs after parse,
records `origin: helper-form-node-id` on produced nodes, before planning.

## Decision 3 — Backward and forward compatibility

- Every addition is opt-in; a model using none of it compiles byte-identical
  to today. The fixture proves it: flat and semantic phone-case versions
  produce identical export digests.
- `ast-visual-blocks` inference remains the fallback for unannotated code;
  declared structure short-circuits inference per node, and mixed models
  (some groups, some flat) are legal.
- LLM generation guidance (prompts/MCP agent docs) updates to prefer the
  semantic forms — enforcement mirrors the existing `repeat`/`instance`
  mandate in AGENTS.md.

## Risks

- **Scope creep into a DSL.** Mitigation: hard rule — every form must
  desugar to existing core; anything needing a new backend op is rejected
  into `language-convenience-stdlib` instead.
- **Keyword channel collides with positional args** in existing binding
  parsing. Mitigation: keywords only recognized between binding name and
  value expression; parser slice covered by round-trip tests over the whole
  existing fixture corpus first (red before touching the grammar).
- **Helper desugar parity drift.** Mitigation: single desugar site +
  native-vs-build123d differential test per helper, same harness as
  `native-build123d-differential-parity`.
- **Check false confidence** (probe-based checks pass while geometry is
  wrong). Mitigation: each check ships with a known-bad fixture that must
  fail (red-side test is part of the definition of done).
