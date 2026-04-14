# Plan: Eliminate `emit` from build123d Lowering

## Goal

Replace the imperative `emit`-based string-builder in `B123dLowerer` with an
expression-returning pipeline: **lower → linearize → serialize**. The lowerer
becomes a pure function that returns an expression tree; a separate pass
flattens it into Python assignment statements.

## Why

1. **`shell` duplication** — `shell` re-implements every target node (cylinder,
   cone, extrude, revolve, sweep, loft, twist) to emit outer+inner pairs.
   ~180 lines of near-copy. With expression trees, `shell` lowers the inner
   node normally, then wraps it in `offset` + `difference`.
2. **No intermediate representation** — currently IR goes straight to Python
   strings during recursion. Can't retarget, can't inspect, can't optimize.
3. **Order-dependent side effects** — `emit` mutates `self.lines` as a side
   effect of recursion. Interleaves emission order with recursion order.
4. **Testing** — must compare full Python output strings. With an expression
   tree, tests can assert on tree structure.

## What stays

- `LoweringScope`, `LoweredBinding`, `B123dGeomKind` — good abstractions, keep.
- `lower_num_expr`, `lower_bool_expr`, `lower_stringish_expr` — already pure
  (return `String` expressions without `emit`). Keep as-is.
- `parse_properties` — stays.
- All existing test assertions on final Python output — verify the linearized
  result still matches.

## Data types

```rust
/// A lowered Python expression (not yet assigned to a variable).
enum PyExpr {
    /// build123d constructor: Box(10, 20, 30, align=...)
    Call {
        func: String,
        args: Vec<String>,
        kwargs: Vec<(String, String)>,
    },
    /// Binary op chain: _v0 + _v1 - _v2
    BinOp {
        op: &'static str,
        operands: Vec<PyExpr>,
    },
    /// Transform prefix: Pos(x,y,z) * inner
    Transform {
        prefix: String,
        inner: Box<PyExpr>,
    },
    /// Reference to a previously-bound variable
    Var(String),
    /// Raw Python expression (list comps, ternaries, edge cases)
    Raw(String),
}

struct LoweredNode {
    expr: PyExpr,
    kind: B123dGeomKind,
}
```

## Three-phase pipeline

### Phase 1: Lower (pure)

`lower_geom_expr` returns `AppResult<LoweredNode>` — an expression tree.
No `self.emit()`, no `self.counter`, no `self.lines`. Pure function of
`(value, scope) → LoweredNode`.

### Phase 2: Linearize

Walk the `PyExpr` tree depth-first. Assign each non-trivial subexpression
to a gensym `_v{N}`. Produce `Vec<(String, String)>` — list of
`(var_name, rhs_expression)` pairs. This is SSA / let-binding form.

### Phase 3: Serialize

Join `"{var} = {rhs}"` lines, prepend imports/helpers, append
`_ecky_parts = [...]`. Produces the final Python string.

## Implementation steps

### Step 0: Scaffolding
- [x] Define `PyExpr` and `LoweredNode` types
- [x] Write `Linearizer` with `linearize()` method
- [x] Write `serialize_b123d_program()` + `b123d_preamble()`
- [x] Add expression-lowering entry point and wire it into `lower_to_build123d`
- [x] Verify it compiles (`cargo check`)

### Step 1: Primitives (leaf nodes)
- [x] `box` → `PyExpr::Call { func: "Box", ... }`
- [x] `cylinder` → `PyExpr::Call { func: "Cylinder", ... }`
- [x] `sphere` → `PyExpr::Call { func: "Sphere", ... }`
- [x] `cone` → `PyExpr::Call { func: "Cone", ... }`
- [x] `circle` → `PyExpr::Call { func: "Circle", ... }`
- [x] `rounded_rect` / `rounded-rect` → `PyExpr::Call { func: "RectangleRounded", ... }`
- [x] `polygon` → `PyExpr::Call { func: "Polygon", ... }`
- [x] Wire up linearizer for `PyExpr::Call`
- [x] Test: existing `lower_to_build123d_minimal_extrude` passes via v2

### Step 2: Boolean combinators
- [x] `union` → `PyExpr::BinOp { op: "+", ... }`
- [x] `difference` → `PyExpr::BinOp { op: "-", ... }`
- [x] `intersection` → `PyExpr::BinOp { op: "&", ... }`
- [x] `xor` → compound `BinOp("+") - BinOp("&")`
- [x] Wire up linearizer for `PyExpr::BinOp`

### Step 3: Transforms
- [x] `translate` → `PyExpr::Transform { prefix: "Pos(...)", ... }`
- [x] `rotate` → `PyExpr::Transform { prefix: "Rot(...)", ... }`
- [x] `scale` → `PyExpr::Imperative` with runtime guard
- [x] `mirror` → `PyExpr::Call` with inner operand
- [x] Wire up linearizer for `PyExpr::Transform`

### Step 4: Sketch-to-solid operations
- [x] `extrude` → `Call("extrude", [Call("_ecky_face", [sketch]), h])`
- [x] `revolve` → `Transform("Rot(90,0,0)", face) + Call("revolve", ...)`
- [x] `loft` → positioned sketches + `Call("loft", [list])`
- [x] `taper` → `Imperative` with bottom + scaled top + loft
- [x] `twist` → `Imperative` with list comp sections + loft
- [x] `sweep` → `Call("sweep", [section], path=path)`
- [x] `make-face` → `Call("_ecky_face", [inner])`
- [x] Face coercion: sketch exprs wrapped in `_ecky_face(...)` via `PyExpr::Call`

### Step 5: Modifiers
- [x] `fillet` → `Call("fillet", [edges_expr, radius])`
- [x] `chamfer` → `Call("chamfer", [edges_expr, radius])`
- [x] `offset` / `offset-rounded` → `Call("offset", ..., kwargs)`
- [x] `if` → `PyExpr::Ternary`

### Step 6: Sketches & paths
- [x] `bspline` → `Call("Spline", ..., kwargs)`
- [x] `rounded-polygon` → compound: `Polygon` + `fillet`
- [x] `profile` → compound: outer union - holes via `BinOp`
- [x] `path` → `Call("Polyline", ...)`
- [x] `bezier-path` → `Imperative` (loop-based concatenation)
- [x] `text` → `Call("Text", ...)`
- [x] `svg` → `Call("import_svg", ...)`
- [x] `import-stl` → `Call("import_stl", ...)`

### Step 7: Arrays
- [x] `linear-array` → `Imperative` (for-loop accumulation)
- [x] `radial-array` → `Imperative` (for-loop accumulation)
- [x] `grid-array` → `Imperative` (nested for-loop)
- [x] `arc-array` → `Imperative` (for-loop accumulation)

### Step 8: `let` bindings
- [x] `let` → lower bindings into scope, return body `LoweredNode`
- [x] Linearizer emits binding assignments before body

### Step 9: `shell` (the payoff)
- [x] Shell handlers using `PyExpr` composition (cylinder, cone, sphere,
      extrude, revolve, sweep, loft, twist)
- [x] Generic `shell` handler: lower inner node generically instead of
      per-target match — eliminate ~180 lines of duplication
- [x] Verify all shell tests pass

### Step 10: Cutover
- [x] Replace `lower_to_build123d` body to use the expression lowerer
- [x] Remove old `B123dLowerer` struct, `emit`, `lines`, `counter`
- [x] Run full test suite (`cargo test`)
- [x] Run `cargo check` clean

### Step 11: Cleanup
- [x] Remove `_v2` naming and delete expression-lowerer duplicate test block
- [x] Remove dead helper files (`generated_source_v2.py`, `server/test_lower.rs`)
- [x] Update this file with completion status

## Current status (2026-04-17)

**Steps 0–11 complete.** Expression lowerer is the only build123d lowering
path now. Generic `shell` now rewrites inner targets as Ecky IR AST and lowers
them through the same solid path instead of hand-emitting each inner Python
shape. `_v2` cutover naming is gone. Dead duplicate helpers are gone.

Verification:
- `cd src-tauri && cargo test lower_to_build123d --lib` -> pass (`59 passed`)
- `cd src-tauri && cargo test` -> pass (`367 passed`)
- `cd src-tauri && cargo check` -> pass
- `python3 server/test_lower.py` -> `Canonical cup fixture OK`

Key design decisions:
- `PyExpr` has 7 variants: `Call`, `BinOp`, `Transform`, `Var`, `Inline`,
  `Ternary`, `Imperative`
- `Imperative` handles for-loops, list comps, runtime guards — uses `_b{N}`
  var namespace to avoid collision with linearizer's `_v{N}` namespace
- `ExprLowerer` owns a `Linearizer` for cases that need pre-linearization
  (fillet/chamfer edge refs, loft positioning, arrays, let bindings)
- Pure nodes compose as expression trees; imperative nodes pre-linearize
  sub-expressions and return `PyExpr::Imperative`
- Generic `shell` uses AST rewrite helpers for inner targets plus one special
  solid-offset path for `shell revolve`
