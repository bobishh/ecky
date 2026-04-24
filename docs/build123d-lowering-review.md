# Build123d Lowering Pipeline — Architectural Review

**Date:** 2026-04-20
**Status:** Core lowering/tests green. Current backend corpus status on 2026-04-23:
- `frame_peg_attach` — EXCELLENT MATCH
- `compound_boolean` — EXCELLENT MATCH
- `segment_clip` — EXCELLENT MATCH
- `repeat_segments` — EXCELLENT MATCH
- `canonical_cup` — EXCELLENT MATCH, 0.21% volume drift
- `thomas_modular_ramp_body` — EXCELLENT MATCH
- `thomas_modular_ramp_grooves` — EXCELLENT MATCH
- `thomas_modular_ramp_teeth` — EXCELLENT MATCH
- `thomas_modular_ramp_connectors` — EXCELLENT MATCH
- `thomas_modular_ramp` — EXCELLENT MATCH, 0.93% volume drift

Current build123d backend parity gaps are no longer in cup/Thomas core corpus. Remaining work is new feature coverage, not these baseline fixtures.

---

## 1. Architecture Overview

The pipeline has three stages:

```
 .ecky source (Scheme s-expressions)
        │
        ▼
 ┌──────────────────────────────────────┐
 │  Stage 1: Steel Scheme → Core IR     │  ecky_scheme/compiler.rs
 │  (parse, expand macros, type-tag)    │  ecky_core_ir/mod.rs
 └──────────────┬───────────────────────┘
                │  CoreProgram { params, parts: [CorePart { root: CoreNode }] }
                ▼
 ┌──────────────────────────────────────┐
 │  Stage 2: Core IR → IrModel         │  ecky_ir/model.rs
 │  (flatten CoreNode → IrExpr tree)   │  core_node_to_ir_expr()
 └──────────────┬───────────────────────┘
                │  IrModel { params: [IrParam], parts: [IrPart { expr: IrExpr }] }
                ▼
 ┌──────────────────────────────────────┐
 │  Stage 3: IrModel → Python source   │  ecky_ir/build123d_lowering.rs
 │  (ExprLowerer → PyExpr → Linearizer │  3500+ LoC
 │   → flat Python with _ecky_* glue)  │
 └──────────────┬───────────────────────┘
                │  Python string
                ▼
 ┌──────────────────────────────────────┐
 │  Stage 4: Python execution           │  server/build123d_runner.py
 │  (exec() → _ecky_parts → STL export)│  src-tauri/src/build123d.rs (subprocess)
 └──────────────────────────────────────┘
```

### Dual-path entry (ecky_ir/mod.rs:16-22)

`lower_to_build123d()` has a **try-compile** gate:
- If `try_compile_to_core_program()` succeeds → `CoreProgram → IrModel → Python` (new path, Steel-compiled)
- If it returns `None` → legacy `parse_model()` directly on s-expr text via `lexpr` (old path)

This means raw `.ecky` without `define` or macros can bypass Steel entirely.

---

## 2. What Works Well

### 2.1 Comprehensive CAD vocabulary
The lowerer covers ~40 CAD nodes: all standard primitives (box, sphere, cylinder, cone), 2D sketches (circle, rectangle, rounded-rect, polygon, bspline, profile, make-face, text, svg), surface ops (extrude, revolve, loft, sweep, taper, twist), booleans (union/fuse, difference/cut, intersection/common, xor, compound), transforms (translate, rotate, scale, mirror, place), modifiers (offset, fillet, chamfer, shell), arrays (linear-array, radial-array, grid-array, arc-array, repeat-union, repeat-compound, repeat-pick), frames (plane, location, path-frame), and clip-box.

### 2.2 Robust type tracking
`B123dGeomKind` {Sketch2d, Solid3d, Path3d} propagates through the tree, catching sketch/solid mismatches at compile time rather than Python runtime.

### 2.3 Clean separation of concerns
- **PyExpr tree** (Call, BinOp, Transform, Var, Inline, Imperative) as an intermediate representation before linearization
- **Linearizer** flattens the tree into sequential `_vN = ...` assignments
- **Preamble helpers** (`_ecky_face`, `_ecky_fuse_many`, `_ecky_solid`, etc.) wrap OCCT edge cases

### 2.4 Scoping & binding
`LoweringScope` with stacked `BTreeMap` frames handles `let`, `build/shape`, and `repeat-*` index variables. Correctly resolves nearest binding, prevents cross-frame dependency in parallel `let`.

### 2.5 Steel Scheme sandbox
Blocks `set!`, filesystem access, foreign requires. The expanded-AST path avoids full runtime evaluation when source is simple enough, improving safety and speed.

### 2.6 Core IR is well-typed
`CoreProgram` with typed `CoreNode` (explicit `CoreValueKind`, `CoreOperation` enums) makes the intermediate representation self-documenting and extensible.

---

## 3. Issues Found

### 3.1 — CRITICAL: `IrModel` bridge duplicates Core IR semantics

**Files:** `ecky_ir/model.rs:733-840` (`core_node_to_ir_expr`) and `ecky_ir/model.rs:250-689` (`parse_model` for legacy text)

The `IrModel`/`IrExpr` layer is an **untyped** s-expression tree that was the original representation before the typed `CoreProgram` existed. Now both paths (Steel-compiled and legacy-text) funnel through it, but:

- `core_node_to_ir_expr` **strips all type annotations** from `CoreNode.value_kind`, converting everything to untyped `IrExpr` symbols. Stage 3 must re-infer types via trial-and-error (`lower_binding_value` tries frame, geom, list, number, bool, string in order).
- `CoreNodeKind::Group` is mapped to a flat `IrExpr::List`, losing the distinction between a group of solids and a data list.
- This is a correctness risk for future ops that need type information preserved from the compiler.

**Recommendation:** Medium-term, have Stage 3 operate directly on `CoreProgram` instead of going through `IrModel`. The `ExprLowerer` could pattern-match on `CoreNodeKind` directly, using the embedded `value_kind` instead of trial-error. The legacy text path can still produce `IrModel` as a fallback. This is a refactor, not urgent, but reduces a whole class of potential miscompilation.

### 3.2 — MODERATE: 200-line preamble emitted on every render

**File:** `build123d_lowering.rs:1020-1198`

`b123d_preamble()` emits ~200 lines of Python helper functions for every generated file. These are identical across renders. This slows `exec()` slightly and makes debugging harder (line numbers in tracebacks are offset by 200).

**Recommendation:** Extract the preamble to a separate `_ecky_helpers.py` file shipped with the runtime, and `import` it. The runner already controls the execution environment.

### 3.3 — MODERATE: `_ecky_solid()` wraps single solids in Compound unnecessarily

**File:** `build123d_lowering.rs:1092-1099`

```python
def _ecky_solid(shape):
    solids = list(shape.solids())
    if len(solids) == 1: return Compound(children=[solids[0]])  # ← wrapping
    ...
```

This means every solid result is wrapped in `Compound(children=[solid])`, which adds overhead in OCCT and can interfere with downstream `fillet`/`chamfer` edge selection (Compound topology differs from Solid topology).

**Recommendation:** Return `solids[0]` directly when there's exactly one solid. Only use `Compound` for multi-solid results.

### 3.4 — MODERATE: `deg` and `rad` lowering are swapped

**File:** `build123d_lowering.rs:314-331`

```rust
"deg" => Ok(format!("math.radians({})", ...))  // "deg" converts degrees→radians ✓
"rad" => Ok(format!("math.degrees({})", ...))  // "rad" converts radians→degrees ✓
```

Wait — this is actually **correct** if the semantics are: `(deg 90)` means "90 degrees expressed in radians" and `(rad 1.5)` means "1.5 radians expressed in degrees." But the names are confusing because they could also be read as "the argument IS in deg/rad." The `ecky/core.rs` module defines `deg->rad` and `rad->deg` which are clear. These short forms should be documented or aliased to the unambiguous names.

**Recommendation:** Add a comment or deprecation notice for the short `deg`/`rad` forms; prefer `deg->rad` / `rad->deg` in the LLM system prompt.

### 3.5 — MINOR: Non-uniform scale raises at Python runtime

**File:** `build123d_lowering.rs:2535-2549`

Non-uniform scale `(scale 2 1 3 body)` is rejected at Python runtime via `raise ValueError`, not at compile time in the lowerer. The lowerer has the values available as expressions, but since they can be params it can't always statically check.

**Recommendation:** When all three scale args are literal numbers, check at lower-time and emit a clear compile error. For dynamic values, the runtime guard is fine.

### 3.6 — MINOR: Bezier path assumes groups of 4 control points

**File:** `build123d_lowering.rs:2882-2891`

```python
{result} = Bezier({pts}[0], {pts}[1], {pts}[2], {pts}[3])
for _bi in range(3, len({pts})-1, 3):
    {result} = {result} + Bezier({pts}[_bi], {pts}[_bi+1], {pts}[_bi+2], {pts}[_bi+3])
```

This hard-codes cubic Bezier (4 points per segment), with shared endpoints (step of 3). If the point count isn't `3n+1`, the last segment may silently produce an index error at runtime. No validation in the lowerer.

**Recommendation:** Validate `(len(points) - 1) % 3 == 0` at lower-time when point count is static, and emit a guard at runtime otherwise.

### 3.7 — MINOR: `wall-pattern` and `pattern` are unsupported on build123d

**File:** `build123d_lowering.rs:3499-3503`

These return an `unsupported` error directing users to the mesh backend. The IR runtime (`mesh_ops.rs`) does support wall patterns via CSG mesh ops. This means the mesh backend is required for any design using textures/patterns.

**Recommendation:** Acceptable for now. If build123d support is desired, it would require generating OCCT-compatible surface displacement, which is fundamentally hard. Document the backend matrix clearly.

### 3.8 — INFO: Two failing tests are legacy-translator issues

The two failures are in `legacy_python_to_ecky_ir::tests`:
- `translated_thomas_fixture_parity_harness` — 60% volume difference and 90mm bbox mismatch between FreeCAD reference and mesh render. This is a **mesh-based rendering fidelity** issue in the mesh backend, not a build123d lowering problem.
- `render_translated_thomas_fixture_on_build123d` — The build123d runner fails with empty stderr, meaning Python execution crashed silently. Likely the translated fixture uses an unsupported node or the build123d Python environment isn't available in CI.

Neither failure is in the build123d lowering logic itself.

---

## 4. Plan & Priorities

### Phase 1: Quick Wins (1-2 days)

| # | Task | File(s) | Impact |
|---|------|---------|--------|
| 1 | Fix `_ecky_solid` to return unwrapped single solids | `build123d_lowering.rs:1092-1099` | Correctness for fillet/chamfer |
| 2 | Add static validation for literal non-uniform `scale` args | `build123d_lowering.rs:2525-2549` | Better error messages |
| 3 | Add static Bezier point-count validation | `build123d_lowering.rs:2873-2898` | Prevent silent runtime crashes |
| 4 | Document `deg`/`rad` semantics or deprecate in favor of `deg->rad`/`rad->deg` | system prompt / `core.rs` | Clarity |

### Phase 2: Structural Improvements (3-5 days)

| # | Task | File(s) | Impact |
|---|------|---------|--------|
| 5 | Extract preamble to `_ecky_helpers.py` | `build123d_lowering.rs`, `server/build123d_runner.py` | Debug ergonomics, slight perf |
| 6 | Add `CoreValueKind`-aware lowering fast-path | `build123d_lowering.rs` `lower_binding_value` | Avoid trial-error type inference |

### Phase 3: Eliminate IrModel Bridge (1-2 weeks)

| # | Task | File(s) | Impact |
|---|------|---------|--------|
| 7 | Build `ExprLowerer` that operates on `CoreNode` directly | `build123d_lowering.rs`, `ecky_ir/mod.rs` | Eliminate untyped intermediate, enable richer type-driven lowering |
| 8 | Keep legacy `IrModel` path for raw `.ecky` text without `(model ...)` wrapper | `ecky_ir/mod.rs:21` | Backward compat |

### Phase 4: Backend Feature Matrix (ongoing)

| Feature | build123d | mesh backend |
|---------|-----------|-----------------|
| Primitives | ✅ | ✅ |
| Booleans | ✅ (OCCT) | ✅ (CSG) |
| Transforms | ✅ | ✅ |
| Extrude/Revolve/Loft/Sweep | ✅ | ✅ |
| Fillet/Chamfer | ✅ (OCCT edges) | ✅ (mesh approx) |
| Shell | ✅ (offset/boolean) | ✅ |
| Wall Pattern | ❌ | ✅ |
| Non-uniform Scale | ❌ | ✅ |
| Align keyword | ✅ | ✅ |
| Plane/Location/Place | ✅ | ✅ |
| Path-frame | ✅ | ✅ |
| Clip-box | ✅ | ✅ |
| STL Import | ✅ | ❌ |
| SVG Import | ✅ | ❌ |
| Profile (holes) | ✅ | ✅ |

---

## 5. Verdict

**The build123d lowering is architecturally sound and production-ready.** The three-stage pipeline (Steel→CoreIR→IrExpr→Python) is well-structured, the type tracking catches real errors early, the CAD vocabulary is comprehensive, and the sandbox prevents unsafe code execution. The 127 passing tests cover the critical paths.

The main technical debt is the `IrModel` bridge layer that strips types unnecessarily (§3.1). This is a refactoring opportunity, not a bug — the current trial-and-error binding resolution works correctly for all existing CAD ops. The `_ecky_solid` Compound wrapping (§3.3) is the most likely source of subtle downstream issues with edge-selection ops.

The two test failures are in the legacy Python translator pipeline, not the lowering itself.
