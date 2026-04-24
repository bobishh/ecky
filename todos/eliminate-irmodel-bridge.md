# Eliminate IrModel Bridge Layer

**Status:** In Progress  
**Created:** 2026-04-27  
**Tracks:** build123d-lowering-review.md §3.1, Phase 3

## Problem

`CoreProgram` (typed) → `core_node_to_ir_expr()` → `IrModel`/`IrExpr` (untyped) → lowerers.

The bridge `core_node_to_ir_expr` (model.rs:751-964) **strips all type information** from `CoreNode.value_kind`, converting typed nodes to untyped `IrExpr` symbols/lists. The lowerers then re-infer types by trial-and-error:

```rust
// freecad_lowering.rs:259-299 — lower_binding_value tries each type in order:
fn lower_binding_value(&mut self, value: &IrExpr, scope: &LoweringScope) -> ... {
    if let Ok(geom) = self.lower_geom_expr(value, scope) { return Ok(Geom(...)) }
    if let Ok(frame) = self.lower_frame_expr(value, scope) { return Ok(Frame(...)) }
    if let Ok(list) = self.lower_runtime_list_expr(value, scope) { return Ok(RuntimeList(...)) }
    if let Ok(number) = self.lower_num_expr(value, scope) { return Ok(Number(...)) }
    if let Ok(boolean) = self.lower_bool_expr(value, scope) { return Ok(Boolean(...)) }
    if let Ok(stringish) = self.lower_stringish_expr(value, scope) { return Ok(Stringish(...)) }
    Err(...)
}
```

Both lowerers (build123d: 3,726 LoC, freecad: 3,278 LoC) do this identically.

### What's lost in translation

| CoreNode field | What the bridge does | What the lowerer must re-infer |
|---|---|---|
| `value_kind: CoreValueKind::Solid` | Dropped | Trial `lower_geom_expr` → `lower_frame_expr` → ... |
| `value_kind: CoreValueKind::Number` | Dropped | Trial `lower_num_expr` |
| `value_kind: CoreValueKind::Sketch` | Dropped | Lowerer treats Sketch2d the same as geom, classifies after |
| `CoreNodeKind::Group` vs `List` | Both → flat `IrExpr::List` | Lost — can't distinguish group-of-solids from data-list |
| `CoreNodeKind::Range` | → `(range start end)` symbol list | Lowerer must re-match `"range"` string |
| `CoreNodeKind::Map` | → `(map (lambda ...) ...)` | Lowerer must re-parse lambda structure from list |
| `CoreNodeKind::Apply` | → `(apply op-name ...)` | Lowerer must re-match `"apply"` string |
| `CoreKeywordArg.name` | → `IrExpr::Keyword("name")` | Lowerer re-parses keyword prefix from expr |

## Strategy

**Incremental, per-lowerer, behind the existing `try_compile_to_core_program` gate.**

The legacy text path (`parse_model()` → `IrModel`) stays for raw `.ecky` without `(model ...)` wrapper. The new path has lowerers operate on `CoreProgram` directly when available. No big-bang rewrite.

## Consumers of IrModel

| Consumer | File | Can bypass IrModel? |
|---|---|---|
| `lower_model_to_build123d` | build123d_lowering.rs:28 | **Yes** — add `lower_core_program_to_build123d` |
| `lower_model_to_freecad` | freecad_lowering.rs:16 | **Yes** — add `lower_core_program_to_freecad` |
| `derive_controls_from_model` | runtime.rs:129 | **Yes** — params come from `CoreProgram.parameters` directly |
| `render_model_from_model` | runtime.rs:159 | **Yes** — calls lowerers + param env, both replaceable |
| `parse_model` (legacy text) | model.rs:322 | **Keep** — needed for non-Steel `.ecky` |
| mesh_ops `eval_geometry_expr` | mesh_ops.rs | **Later** — mesh backend already uses IrExpr, lower priority |

## Execution Plan

### Step 0: Baseline (pre-work)
- [ ] Run full test suite, record passing count
- [ ] `cargo check` green, `cargo test` green

### Step 1: Typed lowering trait for build123d
Create `lower_core_program_to_build123d(program: &CoreProgram) -> AppResult<String>`.

**Approach:** Not a rewrite. Add a thin `CoreNode`-aware dispatch layer that extracts `value_kind` before delegating to the existing `lower_geom_expr`/`lower_num_expr`/etc. functions.

**Current status:** `lower_core_program_to_build123d` no longer builds a whole legacy `IrModel` or whole legacy `IrPart`, and it no longer calls shared `core_node_to_ir_expr`. Param defaults come from `CoreProgram` directly, compiled part roots plus `build` / `let` / `group` / `if` bodies lower directly from `CoreNode`, and compiled literals/refs/list materialization now stay inside `build123d_lowering.rs`. Remaining work here is structural cleanup, not an active bridge call in the compiled build123d entry.

1. [x] Add `lower_core_program_to_build123d` entry point in `build123d_lowering.rs`
2. [ ] Add `CoreLoweringScope` that carries `CoreValueKind` per binding (replaces trial-and-error)
3. [ ] Add `lower_core_binding_value` that uses `node.value_kind` to dispatch directly:
   ```rust
   fn lower_core_binding_value(&mut self, node: &CoreNode, scope: &CoreLoweringScope) -> ... {
       match node.value_kind {
           CoreValueKind::Solid | CoreValueKind::Sketch | CoreValueKind::Compound
               => self.lower_core_geom(node, scope),
           CoreValueKind::Frame => self.lower_core_frame(node, scope),
           CoreValueKind::Number => self.lower_core_num(node, scope),
           CoreValueKind::Boolean => self.lower_core_bool(node, scope),
           CoreValueKind::Text => self.lower_core_stringish(node, scope),
           CoreValueKind::List => self.lower_core_list(node, scope),
           CoreValueKind::Path => self.lower_core_path(node, scope),
           CoreValueKind::Point2 | CoreValueKind::Point3 => self.lower_core_point(node, scope),
           CoreValueKind::Any => self.lower_core_binding_fallback(node, scope), // trial-error for Any
       }
   }
   ```
4. [ ] `lower_core_geom` pattern-matches on `CoreNodeKind::Call { op, .. }` directly (no string head dispatch)
5. [x] Wire into `ecky_ir/mod.rs`: when `try_compile_to_core_program` succeeds, call `lower_core_program_to_build123d` instead of `core_program_to_model` + `lower_model_to_build123d`
6. [x] All existing build123d lowering tests pass

### Step 2: Typed lowering for freecad
Same pattern as Step 1 for `freecad_lowering.rs`.

1. [x] Add `lower_core_program_to_freecad` entry point
2. [x] Port minimal direct `CoreProgram` seam first (defaults + parts path shared with legacy lowering)
3. [x] Wire into `ecky_ir/mod.rs`
4. [x] All existing freecad lowering tests pass

### Step 3: Typed controls derivation
`derive_controls_from_model` currently converts `CoreProgram` → `IrModel` just to read `IrModel.params`. Skip this.

1. [x] Add `derive_controls_from_core_program(program: &CoreProgram) -> AppResult<ParsedParamsResult>` in runtime.rs
2. [x] Map `CoreParameter` → `UiField` directly (logic already exists in `core_param_to_ir_param`, just skip the `IrModel` wrapper)
3. [x] Wire into `ecky_ir/mod.rs::derive_controls`
4. [x] Controls tests pass

### Step 4: Typed render dispatch
`render_model_from_model` calls lowerers + param env. Once Steps 1-3 land, this is just plumbing.

1. [x] Add `render_core_program` that builds param env from `CoreProgram.parameters` directly
2. [x] Wire into `ecky_ir/mod.rs::render_model`
3. [x] Render/parity tests pass

### Step 5: Restrict IrModel to legacy-only
1. [x] Make `core_program_to_model` test-only
2. [x] Make shared `core_part_to_ir_part` / `core_node_to_ir_expr` bridge test-only; compiled build123d/freecad/runtime paths now use local conversion seams
2. [ ] `IrModel` only constructed via `parse_model()` (legacy text path)
3. [ ] Remove `core_node_to_ir_expr` and `allocate_legacy_local_name` (dead code)
4. [ ] Clean up imports

### Step 6 (optional, later): Mesh backend
The mesh backend (`mesh_ops.rs`) uses `IrExpr` directly via `eval_geometry_expr`. It could also operate on `CoreNode`, but it's lower priority because:
- The mesh backend is in-process Rust (no Python codegen), so the type-stripping matters less
- It already does runtime evaluation, not code generation
- It's 2,229 LoC and stable

## Risk Mitigation

- **Each step is independently shippable.** Steps 1-4 can land in any order. The `try_compile_to_core_program` gate means the legacy path is always there as fallback.
- **No behavioral change.** Every step must produce identical output for the same input. Parity tests are the proof.
- **Rollback is trivial.** If a typed path produces wrong output, the `mod.rs` dispatch just falls back to `IrModel`.

## Success Criteria

- `lower_binding_value` trial-and-error eliminated for the Steel-compiled path
- `CoreValueKind` propagated from compiler to lowerer without intermediate loss
- `core_node_to_ir_expr` function removed
- All 127+ existing tests pass
- Parity corpus unchanged (build123d + freecad, all EXCELLENT MATCH)

## Estimated Effort

| Step | Est. | Notes |
|------|------|-------|
| 1 | 2-3 days | Largest — build123d lowerer is 3,726 LoC |
| 2 | 1-2 days | Pattern established in Step 1 |
| 3 | 0.5 day | Small — just param mapping |
| 4 | 0.5 day | Plumbing |
| 5 | 0.5 day | Cleanup |
| **Total** | **5-7 days** | |
