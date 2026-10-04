# Ecky Language Reference

Exact forms, signatures, selectors, and verification grammar.

## Operation Index

Documented forms and operations. Select a name to open its signature.

<!-- ECKY_GENERATED_OP_INDEX_START -->
| Form | Reference |
| --- | --- |
| [`arc-array`](#arc-array) | Array and Frame Signatures |
| [`bezier-path`](#bezier-path) | Surface and Path Signatures |
| [`box`](#box) | Primitive Signatures |
| [`bspline`](#bspline) | Surface and Path Signatures |
| [`build`](#build) | Forms and Structure |
| [`chamfer`](#chamfer) | Surface and Path Signatures |
| [`circle`](#circle) | Primitive Signatures |
| [`clip-box`](#clip-box) | Array and Frame Signatures |
| [`common`](#common) | Boolean and Transform Signatures |
| [`compound`](#compound) | Special / Custom Operations |
| [`cone`](#cone) | Primitive Signatures |
| [`cut`](#cut) | Boolean and Transform Signatures |
| [`cylinder`](#cylinder) | Primitive Signatures |
| [`define-component`](#define-component) | Components |
| [`difference`](#difference) | Boolean and Transform Signatures |
| [`extrude`](#extrude) | Surface and Path Signatures |
| [`feature`](#feature) | Forms and Structure |
| [`fillet`](#fillet) | Surface and Path Signatures |
| [`for-compound`](#for-compound) | Array and Frame Signatures |
| [`for-union`](#for-union) | Array and Frame Signatures |
| [`fuse`](#fuse) | Boolean and Transform Signatures |
| [`grid-array`](#grid-array) | Array and Frame Signatures |
| [`helical-ridge`](#helical-ridge) | Special / Custom Operations |
| [`import-stl`](#import-stl) | Primitive Signatures |
| [`intersection`](#intersection) | Boolean and Transform Signatures |
| [`linear-array`](#linear-array) | Array and Frame Signatures |
| [`location`](#location) | Array and Frame Signatures |
| [`loft`](#loft) | Surface and Path Signatures |
| [`make-face`](#make-face) | Primitive Signatures |
| [`mirror`](#mirror) | Boolean and Transform Signatures |
| [`offset`](#offset) | Surface and Path Signatures |
| [`offset-rounded`](#offset-rounded) | Surface and Path Signatures |
| [`params`](#params) | Params and Controls |
| [`part`](#part) | Forms and Structure |
| [`path`](#path) | Surface and Path Signatures |
| [`path-frame`](#path-frame) | Array and Frame Signatures |
| [`place`](#place) | Array and Frame Signatures |
| [`plane`](#plane) | Array and Frame Signatures |
| [`polygon`](#polygon) | Primitive Signatures |
| [`polyline`](#polyline) | Surface and Path Signatures |
| [`profile`](#profile) | Primitive Signatures |
| [`radial-array`](#radial-array) | Array and Frame Signatures |
| [`rectangle`](#rectangle) | Primitive Signatures |
| [`repeat`](#repeat) | Array and Frame Signatures |
| [`repeat-compound`](#repeat-compound) | Array and Frame Signatures |
| [`repeat-pick`](#repeat-pick) | Array and Frame Signatures |
| [`repeat-union`](#repeat-union) | Array and Frame Signatures |
| [`result`](#result) | Forms and Structure |
| [`revolve`](#revolve) | Surface and Path Signatures |
| [`ring`](#ring) | Primitive Signatures |
| [`rotate`](#rotate) | Boolean and Transform Signatures |
| [`rounded-polygon`](#rounded-polygon) | Primitive Signatures |
| [`rounded-rect`](#rounded-rect) | Primitive Signatures |
| [`sampled-radial-loft`](#sampled-radial-loft) | Special / Custom Operations |
| [`scale`](#scale) | Boolean and Transform Signatures |
| [`shape`](#shape) | Forms and Structure |
| [`shell`](#shell) | Surface and Path Signatures |
| [`sphere`](#sphere) | Primitive Signatures |
| [`surface-trim`](#surface-trim) | Special / Custom Operations |
| [`svg`](#svg) | Primitive Signatures |
| [`sweep`](#sweep) | Surface and Path Signatures |
| [`taper`](#taper) | Surface and Path Signatures |
| [`text`](#text) | Primitive Signatures |
| [`translate`](#translate) | Boolean and Transform Signatures |
| [`twist`](#twist) | Surface and Path Signatures |
| [`union`](#union) | Boolean and Transform Signatures |
| [`wall-pattern`](#wall-pattern) | Special / Custom Operations |
| [`xor`](#xor) | Boolean and Transform Signatures |
<!-- ECKY_GENERATED_OP_INDEX_END -->

## Language Overview

An `.ecky` file describes geometry with parenthesized expressions. A call starts with a function name followed by its arguments: `(box 60 30 4)` makes a box. Calls can contain other calls: `(translate 10 0 0 (box 60 30 4))` moves that box along X.

Lengths use millimetres and rotations use degrees. Trigonometric helpers such as `sin` use radians. A value like `2cm` converts to 20 millimetres; it is not a different geometry type.

A complete file contains one `model`. Its `params` declare controls and its `part` forms name the output geometry. Reusable functions and components go before the model.

```scheme
(model
  (params (number width 60mm :min 20 :max 120))
  (part plate (box width 30 4)))
```

A `Solid` has volume; a `Sketch` is a planar profile; a `Path` describes a route; a `Frame` describes position and orientation. The argument type matters: `extrude` takes a profile, while `translate` can move a profile or a solid.

For a first model, start with [the bracket chapter](/docs/chapters/level-01-corner-bracket/). Use this reference to look up a call while editing. Square brackets in signatures mark optional arguments; do not type the brackets. Examples containing names such as `body` or `profile` are fragments: those names must be defined in the surrounding model.

## Forms and Structure

These forms organize a model. They do not describe dimensions or shapes by themselves.

### `model`

```scheme
(model
  (params (number width 60))
  (part body (box width 30 4)))
```

Use one `model` per file. Its direct clauses include `params`, `part`, `feature`, `verify`, topology tags, `view`, and `analysis`. Put reusable `define` and `define-component` declarations before it. Use `let*` for derived values that depend on model parameters.

### `part`

```scheme
(part body geometry)
(part body "Display name" geometry)
```

`body` is a stable part identifier. The optional string is its display label. The last expression produces its geometry. Separate `part` forms let you export and inspect pieces independently; they do not force the geometry inside each part to be connected.

### `feature`

```scheme
(feature body :role shell geometry)
(feature body :role shell :params (width height) geometry)
```

A feature gives geometry an identifier and a role. `:params` lists the parameters associated with it. Use it when you need to refer to a semantic feature in checks or downstream operations.

### `build`

```scheme
(build
  (shape blank (box 60 30 4))
  (shape bore (translate 0 0 -1 (cylinder 3 6)))
  (result (difference blank bore)))
```

`build` evaluates named intermediate values in order and returns one result. Later shapes can use earlier names. It requires exactly one `result`, after the shape bindings.

### `shape`

```scheme
(shape blank (box 60 30 4))
```

Inside `build`, bind a value to a name. Here `blank` can be used by later expressions in that build. `shape` does not create an extra exported part.

### `result`

```scheme
(result (difference blank bore))
```

Return the final value from `build`. Do not put more `shape` bindings after it.

### `assembly` (planned)

This is not accepted model syntax yet. Declare physical pieces with `part`. For exploded placement that must not affect exports, use `view` and `offset-part`.

### `export` (planned)

This is not accepted model syntax yet. Export through the app or CLI. An ordinary transform inside a part affects the geometry you export; only a `view` offset is preview-only.

## Components

A component is reusable geometry with its own parameters. Declare it before `model`, then call it by name inside a part. Each call can supply different parameter values.

### `define-component`

```scheme
(define-component knuckle
  ((number pin_d 8 :label "Pin diameter" :min 4 :max 12 :step 0.5)
   (number clearance 0.3))
  (difference
    (cylinder (* 2 pin_d) 10 96)
    (cylinder (+ pin_d clearance) 12 96)))
```

- positional 1: component name symbol
- positional 2: signature list; entries use the same grammar as `params`
  entries (kind, key, optional default, keyword metadata)
- final positional: one geometry expression
- optional `(verify ...)` clauses may sit alongside the geometry expression
- declaration is top-level, before `(model ...)`

### Instantiation

```scheme
(part hinge_a (knuckle :pin_d 6))   ; override pin_d, clearance defaults
(part hinge_b (knuckle))            ; all defaults apply
```

- arguments are keywords only: `(name :key value ...)`
- omitted keys take their signature defaults
- a signature entry without a default is required at every call site
- unknown keyword or missing required key fails compile with the component
  name and its signature listed
- components instantiate other components; cycles are rejected and nesting
  is capped at depth 32

### Closedness

A component body sees its signature keys plus bindings made inside the body
(`let`, `let*`, lambda parameters, `repeat` indices, `build` shapes) and
nothing else. Referencing a model param or outer binding is a compile error
naming the variable and the component. Closedness is what makes a component
copy-inlineable: paste the `define-component` into any model and it works.

### Verify travel

`verify` clauses inside a component expand once per instantiation, with the
tag namespaced by the instantiating part key:

```scheme
(define-component pin ((number radius 2))
  (verify (tag pin_ok)
    (metric bad_edges (stl non-manifold-edge-count))
    (expect bad_edges (= 0)))
  (cylinder radius 10))

(model (part left (pin :radius 3)))
```

Each instance runs its own declared check. A passing check establishes only the expectation it measures.

### Component Library Workflow (MCP)

Agents lift proven parts into the shared library and reuse them by source:

1. `component_extract` — pass the model source and a `partKey`. Referenced
   model params become the signature with metadata preserved; scalar outer
   `let`/`let*` bindings become plain defaults; non-scalar free references
   are reported as blockers. Set `save: true` to store the component.
2. `component_search` — compact headers only (name, one-liner, param keys,
   tags). Bodies are never returned by search.
3. `component_get` — full copy-inline `define-component` source for one
   component by name. Paste it into the model and instantiate it.

Extraction is copy-inline only: the returned source is self-contained and
no registry reference is created implicitly.

### Live package references

Use a live reference when the authored model must retain an installed package
coordinate instead of vendoring source:

```scheme
(import-component
  "bike.bottle-holder-kit"
  :version "1.2.0"
  :component "bottle-cage"
  :as cage)

(model
  (part holder
    (cage :diameter 74)))
```

Package id, version, component id, and alias are mandatory literal values.
Resolution is exact. No semver ranges, `latest`, network fallback, or
transitive package lookup occurs.

Copy-inline and live reference are separate modes:

- `component_get` is vendor mode: paste closed source; no package dependency
  or dependency lock exists afterward.
- `import-component` is live mode: the committed model version owns a
  canonical exact lock containing package coordinates and payload digests.
- preview, render, export, reopen, and historical rerender read the appended
  lock and never update it.
- installing a newer package changes nothing until an explicit upgrade
  previews and appends a new model version. The old version keeps its old lock.

Payloads live once in the application-global content-addressed store. Models
do not receive `node_modules`-style dependency trees. Uninstall removes package
discovery; committed locks continue resolving their immutable payload digests.
Garbage collection removes a payload only after installed coordinates,
committed versions, and in-flight operations stop retaining it.

Filesystem projects mirror the canonical lock as `ecky.lock.edn`. Normal
export references the global store. Portable export vendors locked payloads by
digest; portable import verifies every digest before publishing anything.

STEP-backed live components preserve analytic BRep provenance and import
through native Direct OCCT. This path never calls FreeCAD, converts through
STL, invokes `solidify`, repairs geometry, or implicitly fuses multiple roots.
STL remains the separate `import-stl` → `solidify` mesh bridge.

## Verify Clauses

Use `verify` when source should declare structural expectations explicitly.

```scheme
(model
  (verify
    (tag plate_connected)
    (intent "The plate must be one connected mesh")
    (metric pieces (stl connected-component-count))
    (expect pieces (= 1)))
  (part plate (box 60 30 4)))
```

- model verification is top-level under `model`
- component-owned verification is the one exception: a `verify` clause may
  sit directly inside `define-component`, before its geometry expression, and
  expands once per instance
- one verify clause requires exactly one `tag`, `metric`, and `expect`
- optional `intent`, `severity`, and `when` sections may each appear once
- sections may be authored in any order; emitted source uses `tag`, `intent`,
  `severity`, `when`, `metric`, `expect`
- nested `verify` inside geometry or helper expressions is rejected
- empty `(verify)` is rejected

### `tag`

```scheme
(tag body_shell body.front_window_1)
```

- carries authored labels, ids, or references
- payload stays opaque to compiler/core IR
- useful for human grouping and later diagnostics

### `intent`

```scheme
(intent "Assembly must remain connected")
```

- optional human explanation of the invariant
- does not create a second requirement id; `tag` remains stable identity

### `severity`

```scheme
(severity error)
(severity warning)
```

- omission means `error`
- a failed `error` expectation blocks structural verification
- a failed `warning` expectation stays visible but does not make the model red
- invalid syntax, invalid conditions, and evaluation errors always block;
  `warning` cannot hide a broken check

### `when`

```scheme
(when assembly-preview)
(when (and assembly-preview (not print-layout)))
```

- optional boolean gate evaluated from effective render parameters
- accepts booleans, boolean parameter names, and nested `not`, `and`, `or`
- false returns explicit `skipped` evidence; metric and expectation do not run
- unknown/non-boolean parameters, bad arity, and unknown operators are errors

### `metric`

```scheme
(metric check (manifest has-step))
(metric triangles (stl triangle-count))
(metric gap (clearance min-distance body.front_window_1 lid.front_skirt))
```

- first item usually names local check alias
- second item is metric expression
- current runtime metric namespaces:
  - `manifest`
  - `stl`
  - `clearance`

Current shipped metric keys:

- `manifest has-step`
- `manifest has-model-stl`
- `manifest edge-target-count`
- `manifest face-target-count`
- `manifest export-format-count`
- `manifest part-count`
- `stl triangle-count`
- `stl connected-component-count`
- `stl non-manifold-edge-count`
- `stl overhang-face-count`
- `stl bed-contact-area-ratio [part-id]`
- `stl bed-contact-x-span-ratio [part-id]`
- `stl bed-contact-y-span-ratio [part-id]`
- `clearance min-distance`

Bed-contact ratios compare downward planar faces touching the lowest Z plane
against all downward planar faces. Area ratio catches models whose nominal
bottom is mostly suspended. X/Y span ratios catch models resting only on one
small island, edge, or corner. Pass an optional part id to analyze that part's
STL instead of the combined model STL.

`clearance min-distance` compares the minimum distance between two named
selectors.

- selectors can name parts, selection targets, or correspondence outputs
- part selectors use manifest bounds
- edge and face selectors use runtime mesh target geometry when available
- unresolved selectors fail authored verify with a raw runtime error

### `expect`

```scheme
(expect check (= true))
(expect triangles (> 100))
```

- first item should reference the metric alias used above
- second item is comparator form
- current shipped comparators:
  - `=`
  - `>`
  - `>=`
  - `<`
  - `<=`

A failed expectation records which metric missed its threshold. Inspect the measurement before changing geometry. Removing the check also removes the requirement it was meant to test.

## Params and Controls

Declare editable inputs inside `params`. The key is the name used by geometry expressions; `:label` is the text shown in the control. A saved project can override a source default with its current parameter value.

### `params`

```scheme
(params
  decl
  decl
  :relations ((<= wall shell) (>= shell 1.6)))
```

- container for parameter declarations
- optional `:relations` list attaches cross-parameter constraints

Supported relation operators:

- `<`
- `<=`
- `>`
- `>=`

### `number`

```scheme
(number wall 2.4
  :label "Wall"
  :min 0.8
  :max 8
  :step 0.1
  :unit length
  :frozen #f)
```

- positional 1: parameter key symbol
- positional 2: default number
- keywords:
  - `:label` text
  - `:min` number
  - `:max` number
  - `:step` number
  - `:unit` one of `length | angle | ratio | count | text`
  - `:frozen` boolean

### Units and suffixed literals

Bare lengths are millimetres; bare rotation angles are degrees. Suffixes make conversions explicit:

| Literal | Base-unit value |
| --- | --- |
| `12mm` | 12 mm |
| `2.54cm` | 25.4 mm |
| `0.25in` | 6.35 mm |
| `45deg` | 45 degrees |
| `1.5708rad` | Approximately 90 degrees |

Use bare numbers for counts, ratios, and segment counts. Suffix conversion does not by itself provide dimensional type checking.

### `toggle`

```scheme
(toggle useFillet #t
  :label "Use fillet"
  :frozen #f)
```

- positional 1: parameter key symbol
- positional 2: default boolean
- keywords:
  - `:label`
  - `:frozen`

### `select`

```scheme
(select material "PLA"
  :label "Material"
  :unit text
  :options
    ((option "PLA" "PLA")
     (option "PETG" "PETG")
     (option "ABS" "ABS"))
  :frozen #f)
```

- positional 1: parameter key symbol
- positional 2: default choice value
- required keyword for practical use: `:options`
- optional keywords:
  - `:label`
  - `:unit`
  - `:frozen`

### `image`

```scheme
(image decal "assets/logo.svg"
  :label "Decal"
  :frozen #f)
```

- positional 1: parameter key symbol
- positional 2: default image path text
- optional keywords:
  - `:label`
  - `:frozen`

### `option`

```scheme
(option "Large" 42)
(option "PLA" "PLA")
```

- positional 1: display label
- positional 2: value
- valid value kinds:
  - number
  - string / text symbol

## Core Helper Library

These helpers return values rather than solids. Use them to calculate dimensions, generate profile points, and build lists for repeated geometry. Angles passed to trigonometric functions are radians; use `deg->rad` to convert degrees.

### Constructors and Symbols

#### `vec2`

- signature: `vec2 x y`
- returns: 2D point

#### `vec3`

- signature: `vec3 x y z`
- returns: 3D point

#### `start`

- constant anchor symbol for path/frame usage

#### `end`

- constant anchor symbol for path/frame usage

#### `xy`

- constant plane symbol

#### `yz`

- constant plane symbol

#### `xz`

- constant plane symbol

#### `true`

- constant boolean alias for `#t`

#### `false`

- constant boolean alias for `#f`

### Sequence Helpers

#### `zip`

- signature: `zip list1 list2 ...`
- returns: list of tuples

#### `enumerate`

- signature: `enumerate list`
- signature: `enumerate start-index list`
- returns: list of `(index item)` pairs

#### `flat-map`

- signature: `flat-map fn list1 list2 ...`
- returns: concatenated mapped list

#### `concat-map`

- signature: `concat-map fn list1 list2 ...`
- same behavior as `flat-map`

#### `linspace`

- signature: `linspace start stop count`
- returns: evenly spaced number list
- special cases:
  - `count <= 0` -> empty list
  - `count == 1` -> single-item list containing `start`

### Scalar Math Helpers

#### `pi`

- constant `3.141592653589793`

#### `tau`

- constant `6.283185307179586`

#### `clamp`

- signature: `clamp value lower upper`
- returns: value clamped into `[lower, upper]`

#### `lerp`

- signature: `lerp start end t`
- returns: linear interpolation

#### `invlerp`

- signature: `invlerp start end value`
- returns: normalized interpolation factor

#### `remap`

- signature: `remap value in-start in-end out-start out-end`
- returns: value remapped from one range into another

#### `deg`

- signature: `deg degrees`
- returns: radians

#### `rad`

- signature: `rad radians`
- returns: degrees

#### `deg->rad`

- signature: `deg->rad degrees`
- returns: radians

#### `rad->deg`

- signature: `rad->deg radians`
- returns: degrees

#### `smoothstep`

- signature: `smoothstep edge0 edge1 x`
- returns: smoothed `0..1` interpolation

#### `square`

- signature: `square x`
- returns: `x * x`

#### `cube`

- signature: `cube x`
- returns: `x * x * x`

### Noise and Field Helpers

#### `hash01`

- signature: `hash01 x y seed`
- returns: deterministic `0..1` scalar

#### `hash-signed`

- signature: `hash-signed x y seed`
- returns: deterministic `-1..1` scalar

#### `noise2`

- signature: `noise2 x y seed`
- returns: smoothed 2D value noise

#### `fbm2`

- signature: `fbm2 x y seed octaves lacunarity gain`
- returns: fractal Brownian motion sample

#### `voronoi2`

- signature: `voronoi2 x y seed`
- returns: Voronoi-style scalar field

#### `cell-distance2`

- signature: `cell-distance2 x y seed`
- returns: normalized cell distance field

#### `jitter2`

- signature: `jitter2 x y amount seed`
- returns: jittered 2D point

#### `jittered-grid`

- signature: `jittered-grid rows cols dx dy amount seed`
- returns: list of jittered 2D points

### Shape-Driving Point Generators

#### `polar-points`

- signature: `polar-points count radius`
- returns: closed-style circular 2D sample list

#### `organic-loop`

- signature: `organic-loop count radius amount seed`
- returns: noisy radial 2D loop

#### `wave-loop`

- signature: `wave-loop count rx ry amp waves seed`
- returns: wavy ellipse-like 2D loop

#### `superellipse-point`

- signature: `superellipse-point rx ry n t`
- returns: single 2D point on superellipse

#### `voronoi-cells`

- signature: `voronoi-cells rows cols dx dy amount seed`
- returns: jittered cell-center point list

### Chaotic / Generative Point Clouds

#### `lorenz-points`

- signature: `lorenz-points count dt scale`
- returns: list of 3D points

#### `rossler-points`

- signature: `rossler-points count dt scale`
- returns: list of 3D points

#### `logistic-bifurcation-points`

- signature: `logistic-bifurcation-points count seed scale`
- returns: list of 2D points

#### `henon-points`

- signature: `henon-points count seed scale`
- returns: list of 2D points

Use helper outputs as inputs to `polygon`, `bspline`, `path`, `bezier-path`, `map`, and repetition logic.

## Value Kinds and IR Nodes

Type errors tell you what kind of value a function expected. These are the types you will encounter while authoring:

| Kind | Meaning | Example |
| --- | --- | --- |
| Number | Scalar dimension, angle, count, or other numeric value | `12`, `4mm` |
| Boolean | True or false | `true`, `(< width 20)` |
| Text | A string | `"lid"` |
| List | Ordered values | `(list 1 2 3)` |
| Point2 | Two coordinates | `(vec2 10 20)` |
| Point3 | Three coordinates | `(vec3 10 20 5)` |
| Sketch | Planar geometry used as a profile | `(circle 8)` |
| Path | Route through points | `(path (0 0 0) (0 0 20))` |
| Frame | Position and orientation | `(plane :origin '(0 0 10))` |
| Solid | Geometry with volume | `(box 20 10 4)` |
| Compound | Grouped geometry | `(compound a b)` |
| Any | No narrower type required at this position | Depends on the call |

For example, passing `(box 20 10 4)` to a function that expects a sketch is a type mismatch. Use a 2D profile such as `(rectangle 20 10)` instead.

IR means the compiler's intermediate representation. Names such as `Literal`, `Call`, `Reference`, `Build`, and `Let` describe internal nodes in diagnostics; they are not additional geometry functions.

## Primitive Signatures

Solid primitives create volume. Sketch primitives create profiles for `extrude`, `revolve`, or another operation that needs a cross-section. Dimensions below are in millimetres.

### `box`

`(box width depth height [:align '(x y z)])` → Solid.

X and Y are centered by default; Z starts at 0. A 60 × 30 × 4 box therefore spans X = −30…30, Y = −15…15, and Z = 0…4. Dimensions must be positive.

```scheme
(model
  (part plate (box 60 30 4)))
```

`:align` takes three values, each `min`, `center`, or `max`. `min` places the lower bound of that axis at the origin; `max` places its upper bound there.

### `sphere`

`(sphere radius [:align '(x y z)])` → Solid.

The sphere is centered on all three axes by default. Radius is half the diameter: `(sphere 10)` has a 20 mm diameter.

### `cylinder`

`(cylinder radius height [segments] [:align '(x y z)])` → Solid.

The axis is Z. X and Y are centered; the base starts at Z = 0. The first argument is radius, not diameter. `(cylinder 3 8)` is 6 mm across and 8 mm tall.

The optional segment count controls polygonal approximations on paths that use them. Native OCCT keeps the cylinder analytic.

### `cone`

`(cone radius1 radius2 height [segments] [:align '(x y z)])` → Solid.

`radius1` is the bottom radius and `radius2` the top radius. The axis is Z, with its base at 0 by default. Set one radius to zero for a pointed cone; keep both positive for a truncated cone.

### `circle`

`(circle radius [segments])` → Sketch.

A circular profile in XY, centered at the origin. It has no height until used by an operation such as `(extrude (circle 8) 4)`.

### `rectangle`

`(rectangle width height)` → Sketch.

A centered rectangle in XY. Its second dimension is along Y, not an extrusion height.

### `rounded-rect`

`(rounded-rect width height radius)` → Sketch.

A centered rectangle with rounded corners. `radius` controls the corner arcs. Choose a radius no larger than half the shorter dimension.

### `rounded-polygon`

`(rounded-polygon points radius [segments])` → Sketch.

Round the corners of a polygon defined by 2D points. The radius must fit the neighboring edges. Start with a small radius if the rounding fails.

### `polygon`

`(polygon points)` → Sketch.

The points are an ordered list of XY coordinates. The boundary closes from the last point to the first. Use at least three non-collinear points and avoid a self-intersecting outline.

```scheme
(model
  (part wedge
    (extrude (polygon ((0 0) (30 0) (0 20))) 4)))
```

### `profile`

`(profile loop1 loop2 ...)` → Sketch.

The explicit hole form is `(profile :outer outer-loop :holes hole-loop-or-list)`. It accepts one outer loop and the enclosed holes. Use it when the hole is part of the cross-section, before extrusion.

```scheme
(model
  (part washer
    (extrude (profile :outer (circle 12) :holes (circle 4)) 2)))
```

### `make-face`

`(make-face wire1 wire2 ...)` → Sketch.

Build a face from wire-like loops. Use this when the boundary already exists as wires; use `profile` when explicitly combining an outer profile and holes.

### `text`

`(text string size [:font selector])` → Sketch.

`size` sets text size. `:font` accepts an installed font family or an absolute `.ttf`/`.otf` path. It belongs to `text`, not to the following extrusion. The requested font must exist on the machine doing the render.

```scheme
(model
  (part label
    (extrude (text "OPEN" 12 :font "Arial") 2)))
```

### `svg`

`(svg path)` → Sketch on the native renderer.

Import an SVG profile and use it in a geometry operation. The optional `target-width`, `target-height`, and `fit-mode` positional arguments belong to the FreeCAD interop form; they are not the native signature. Its fit modes are `"contain"`, `"cover"`, `"stretch"`, and `"fill"`.

### `import-stl`

`(import-stl path)` → imported mesh geometry.

Read triangles from an STL file. Importing a mesh does not recover its original analytic surfaces. A later `solidify` operation can make an eligible closed mesh usable in the mesh-to-solid path; it does not reconstruct the original CAD design.

### `ring`

`(ring outer-radius inner-radius [segments])` → Sketch.

A centered circular profile with a circular hole. The inner radius must be smaller than the outer radius. `(extrude (ring 12 4) 2)` makes the same washer cross-section as the `profile` example above.

## Boolean and Transform Signatures

Boolean operations combine or remove material. Transforms change where geometry is or how large it is. Expressions are evaluated inside out, so rotating then translating differs from translating then rotating.

### `union`

`(union shape1 shape2 ...)` → combined geometry.

Join the supplied shapes. Overlapping solids can become one connected body; separated solids stay disconnected. Use `compound` when you only need a group and do not want a boolean join.

### `fuse`

Alias of `union`, with the same arguments.

### `difference`

`(difference base cut1 cut2 ...)` → remaining geometry.

Subtract every cutter from `base`. The first argument is the material to keep. A cutter outside the base removes nothing. For through-holes, extend cutters slightly beyond both surfaces.

```scheme
(model
  (part plate
    (difference
      (box 60 30 4)
      (translate 0 0 -1 (cylinder 3 6)))))
```

The cutter runs from Z = −1 to 5 while the plate runs from 0 to 4.

### `cut`

Alias of `difference`, with the same arguments.

### `intersection`

`(intersection shape1 shape2 ...)` → shared geometry.

Keep only the region common to the supplied shapes. Shapes with no overlap have no shared volume.

### `common`

Alias of `intersection`, with the same arguments.

### `xor`

`(xor shape1 shape2 ...)` → exclusive regions.

For two inputs, retain their non-overlapping regions and remove their shared region. Boolean forms require at least one shape argument.

### `translate`

`(translate x y z shape)` → same kind as `shape`.

Move by the specified offsets. `(translate 20 0 0 shape)` moves it 20 mm along X.

### `rotate`

`(rotate x y z shape)` → same kind as `shape`.

Angles are degrees around the axes through the origin. Rotating an already translated object also moves it around the origin. To rotate in place before positioning, put `rotate` inside `translate`.

### `scale`

`(scale factor shape)` or `(scale x y z shape)` → same kind as `shape`.

Scale coordinates from the origin. A uniform factor of 2 doubles every dimension, including holes. XYZ factors allow different scaling on each axis. The native renderer supports both forms; FreeCAD interop requires explicit XYZ factors.

### `mirror`

`(mirror axis offset shape)` → same kind as `shape`.

Reflect across the plane perpendicular to the named axis at `offset`. For example, `(mirror 'x 0 shape)` reflects X across the YZ plane.

```scheme
(model
  (part block
    (translate 20 0 0
      (rotate 0 0 45 (box 10 6 4)))))
```

This rotates the block at the origin, then moves it 20 mm along X.

## Surface and Path Signatures

### `extrude`

Extend a planar profile by `distance`, normally along Z for an XY sketch. `:symmetric true` distributes the extrusion about its profile plane.

- signature: `extrude profile distance`
- result: `Solid`
- optional keyword:
  - `:symmetric` boolean

### `revolve`

Rotate the profile through `angle` degrees to make a solid of revolution. Use 360 for a complete turn.

- signature: `revolve profile angle`
- result: `Solid`

### `loft`

Join two or more profiles along the loft distance. With two profiles, the distance separates the first and last sections.

- signature: `loft distance profile1 profile2 ...`
- requires at least two profiles after distance
- result: `Solid`

### `sweep`

- signature: `sweep profile path`
- result: `Solid`

Example:

```scheme
(model
  (part rail
    (sweep
      (circle 1.2)
      (bezier-path ((0 0 0) (0 0 12) (12 0 20) (24 0 20))))))
```

The circle is the cross-section. The Bézier path carries it upward, then
through the bend, producing a capped solid rail.

### `shell`

Hollow a solid using the requested wall thickness. `:faces` selects openings. Wall thickness must fit the local geometry; reduce it if adjacent walls or tight corners cause a failure.

- signature: `shell thickness solid`
- result: `Solid`
- optional keyword:
  - `:faces selector`

### `offset`

Expand or contract a planar profile by an amount. Use the resulting profile in a later solid operation.

- signature: `offset amount profile`
- result: `Sketch`
- optional keyword:
  - `:openings sketch-or-sketch-list`

### `offset-rounded`

Offset a profile with rounded transitions at corners.

- signature: `offset-rounded amount profile`
- result: `Sketch`
- optional keyword:
  - `:openings sketch-or-sketch-list`

### `fillet`

Round selected solid edges with a constant radius. Omit `:edges` to use the default selection. A radius too large for the adjacent faces can fail; test a smaller radius and a narrower edge selection.

- signature: `fillet radius solid`
- result: `Solid`
- optional keyword:
  - `:edges selector`

### `chamfer`

Cut a flat bevel on selected solid edges. `distance` sets its size; `:edges` restricts the selection.

- signature: `chamfer distance solid`
- result: `Solid`
- optional keyword:
  - `:edges selector`

### `taper`

- signature: `taper height scale profile`
- signature: `taper height scale-x scale-y profile`
- result: `Solid`
- FreeCAD caveat: non-uniform taper currently rejected

### `twist`

- signature: `twist height angle profile`
- result: `Solid`
- all three positional arguments are required

### `path`

- signature: `path point1 point2 ...`
- signature: `path point-list`
- each point is 3D
- result: `Path`

### `polyline`

- alias of `path`

### `bezier-path`

- signature: `bezier-path point-list`
- point list must be 3D
- result: `Path`

### `bspline`

- signature: `bspline point-list`
- optional second positional: `closed`
- optional keywords:
  - `:closed` boolean
  - `:tangents` point-list
  - `:tangent-scalars` numeric list
- result: `Sketch`

Notes:

- point-list is required
- tangent hints are optional
- tangents list may use 2 entries or one per point in native path

Example:

```scheme
(model
  (part body
    (extrude
      (bspline
        ((-14 -8) (-8 -14) (8 -14) (14 -8)
         (14 8) (8 14) (-8 14) (-14 8))
        :closed #t)
      4)))
```

## Array and Frame Signatures

### `linear-array`

Copy the shape `count` times. XYZ values are the step between copies, not the final overall displacement.

- signature: `linear-array count x y z shape`
- result: same geometry family as input

### `radial-array`

Copy around Z. `angle` is the angular step in degrees and `radius` is the radial offset. Four copies spaced by 90 degrees make a full circle.

- signature: `radial-array count angle radius shape`
- result: same geometry family as input

### `grid-array`

Copy across rows and columns. X and Y set the spacing between adjacent copies.

- signature: `grid-array rows cols x y shape`
- result: same geometry family as input

### `arc-array`

- signature: `arc-array count radius start-angle end-angle shape`
- result: same geometry family as input

### `repeat`

- signature: `repeat index count expr`
- verifier recognizes form
- use `repeat-union` to join copies or `repeat-compound` to keep a group when rendering native solid geometry

### `repeat-union`

Evaluate the body for each index from 0 to `count - 1`, then join the resulting geometry. Use the index in a transform to put each copy in a different place.

- signature: `repeat-union index count expr`
- index must be symbol
- body should produce geometry
- result: union/fused geometry

### `repeat-compound`

Evaluate one body per index and group the results without a boolean join.

- signature: `repeat-compound index count expr`
- index must be symbol
- body should produce geometry
- result: compound geometry
- native caveat: currently solid-only

### `repeat-pick`

- signature: `repeat-pick index count predicate expr`
- index must be symbol
- predicate decides whether current body instance is selected
- result: last matching geometry

### `for-union`

- macro alias:
  - `for-union (index count) body`
- lowers to `repeat-union`

### `for-compound`

- macro alias:
  - `for-compound (index count) body`
- lowers to `repeat-compound`

### `plane`

- signature: `plane`
- keywords:
  - `:origin (x y z)`
  - `:x (x y z)`
  - `:normal (x y z)`
- result: `Frame`

Defaults:

- origin `(0 0 0)`
- x direction `(1 0 0)`
- normal `(0 0 1)`

### `location`

- signature: `location frame`
- optional keywords:
  - `:offset (x y z)`
  - `:rotate (x y z)`
- result: `Frame`

### `path-frame`

Construct a frame on a path. `start` and `end` choose its endpoints; a numeric `:at` selects a position along it. `:up` helps choose the frame orientation.

- signature: `path-frame path`
- optional keywords:
  - `:at start | end | number`
  - `:up (x y z)`
- result: `Frame`

### `place`

Put geometry into a frame. This is useful for attaching a feature to a path or an inclined plane without reconstructing the orientation by hand.

- signature: `place frame shape`
- optional keywords:
  - `:offset (x y z)`
  - `:rotate (x y z)`
- result: placed shape

### `clip-box`

Keep the portion of a shape inside the given XYZ bounds. Each bound is a two-number list in the shape coordinate system.

- signature: `clip-box shape`
- required keywords:
  - `:x (min max)`
  - `:y (min max)`
  - `:z (min max)`
- result: clipped shape

Example:

```scheme
(model
  (part body
    (build
      (shape rail (path (0 0 0) (20 0 10) (20 10 10)))
      (shape peg (box 4 2 6 :align '(min min min)))
      (shape frame (path-frame rail :at 0.5))
      (result (place frame peg :offset (1 2 3) :rotate (10 20 30))))))
```

## Special / Custom Operations

These operations cover grouping, incomplete geometry, threads, and sampled surfaces.

### `hole`

Typed placeholder op. Use to mark missing geometry intentionally.

- signature: `hole :type kind`
- signature: `hole :type kind :goal "why this hole exists"`
- required keyword:
  - `:type`
- optional keyword:
  - `:goal`

Allowed `:type` values:

- `solid`
- `sketch`
- `path`
- `shape`

Current behavior:

- compiler accepts it as typed placeholder
- lowerers reject it until replaced with real geometry

### `compound`

- signature: `compound shape1 shape2 ...`
- groups shapes without boolean merge semantics

### `helical-ridge`

Keyword-only thread-like ridge generator.

- required keywords:
  - `:radius`
  - `:pitch`
  - `:height`
  - `:base-width`
  - `:crest-width`
  - `:depth`
- optional keywords:
  - `:female`
  - `:clearance`
  - `:lefthand`

Example:

```scheme
(helical-ridge
  :radius 10
  :pitch 2
  :height 18
  :base-width 1.2
  :crest-width 0.4
  :depth 0.7
  :female #t
  :clearance 0.15
  :lefthand #t)
```

### `sampled-radial-loft`

Sample radial sections along Z and join them into a loft. `:radius` is evaluated at each sample using the three bound coordinates.

```scheme
(sampled-radial-loft
  (theta z fz)
  :height 40
  :z-steps 6
  :theta-steps 24
  :radius expr
  :z-map expr)
```

- binder list must be exactly `(theta z fz)`
- required keywords:
  - `:height`
  - `:z-steps`
  - `:theta-steps`
  - `:radius`
- optional keyword:
  - `:z-map`

### `wall-pattern`

Apply a procedural mesh pattern to a supported shell or solid surface.

Example call:

```scheme
(wall-pattern
  (:mode gyroid :depth 0.6 :uFreq 4 :vFreq 5 :phase 0.2)
  shape)
```

Options:

- `:mode`
- `:depth`
- `:uFreq`
- `:vFreq`
- `:phase`

Modes include:

- `gyroid`
- `cellular`
- `fbm`
- `ribs`

Backend caveat:

- native OCCT handles BREP operations; `wall-pattern` remains mesh-only

### `surface-trim`

Trim an imported triangle mesh along an anchored loop. This is the source form written by the surface-trim tool; anchors must refer to the exact imported mesh.

```scheme
(surface-trim
  (import-stl path)
  :schema-version 1
  :source-digest digest
  :loop (anchor-a anchor-b anchor-c)
  :keep-seed anchor-inside
  :path-mode "shortest"
  :cap "open")
```

This is a fragment. `path` and `digest` identify the source mesh. Each anchor has the form `(mesh-anchor triangle-index b0 b1 b2)`: one triangle index and three barycentric weights. The loop requires at least three anchors; `keep-seed` selects the region to retain.

All six keywords are required. `:path-mode` accepts `"shortest"` or `"feature"`. `:cap` accepts `"open"`, `"flat"`, or `"surface-fill"`. Use the app's mesh selection tools to create anchors; editing triangle indices after replacing the source mesh invalidates their meaning.

## Selector Strings and Named Keywords

Selectors choose which edges or faces an operation modifies. Keywords also carry coordinates and orientation; their expected value shapes are listed below.

### Shared keyword value expectations

Expected keyword values:

- `:offset` -> 3D point
- `:rotate` -> 3D point
- `:origin` -> 3D point
- `:x` -> 3D point on frame ops
- `:normal` -> 3D point
- `clip-box :x/:y/:z` -> 2-item numeric list
- `:openings` -> sketch or sketch-list
- `:edges` -> edge selector payload
- `:faces` -> face selector payload

### `:align`

Supported on:

- `box`
- `sphere`
- `cylinder`
- `cone`

Example:

```scheme
(box 4 4 4 :align '(min center max))
```

Rules:

- expects 3-axis tuple
- each axis must be `min`, `center`, or `max`

### Edge selectors

Used by ops like `fillet` and `chamfer`.

Examples:

- `:edges top`
- `:edges "bottom"`
- `:edges "left+vertical"`
- `:edges "target-id:body:edge:0:0-0-0_10-0-0"`

Named boundary selectors:

- `top` -> boundary `z max`
- `bottom` -> boundary `z min`
- `left+vertical` -> `x-min + axis-z`

### Face selectors

Used by ops like `shell`.

Examples:

- `:faces "top"`
- `:faces "planar+normal-z+area-max"`
- `:faces "target-id:body:face:5:0-0-10:100"`

### `path-frame :at`

Accepted anchor values:

- `start`
- `end`
- numeric position

## Bound Project Lifecycle

When you open a file-backed project, its `.ecky` file is the editable source. Save a change in Ecky or an external editor. The app detects the saved edit, records a version, then validates and renders it. A failed edit remains in history with its diagnostics.

Source defaults and current parameter values are separate. Changing a default in the file does not prove the current render uses it: inspect the active parameter controls.

Export from a rendered version. Available formats depend on the resulting geometry and renderer. Native analytic geometry can retain STEP surfaces; mesh-only operations do not imply an analytic STEP result.

An ordinary `translate` or `rotate` inside a part changes exported geometry. A `view` with `offset-part` changes only preview placement. Keep those separate when laying out a multipart model for inspection.

## Complete Compiler Surface

Additional callable forms and helpers. Entries already explained in the preceding sections are omitted from this list.

### `*`

`(* a b...)`

Multiplies numbers.

Example fragment:

```scheme
(* radius 2)
```

### `+`

`(+ a b...)`

Adds numbers.

Example fragment:

```scheme
(+ width clearance)
```

### `-`

`(- a b...)`

Subtracts numbers or negates one number.

Example fragment:

```scheme
(- outer inner)
```

### `/`

`(/ a b...)`

Divides numbers.

Example fragment:

```scheme
(/ width 2)
```

### `<`

`(< a b)`

Boolean predicate or comparator for conditionals and filtering.

Example fragment:

```scheme
(< 2 1)
```

### `<=`

`(<= a b)`

Boolean predicate or comparator for conditionals and filtering.

Example fragment:

```scheme
(<= 2 1)
```

### `=`

`(= a b)`

Boolean predicate or comparator for conditionals and filtering.

Example fragment:

```scheme
(= 2 1)
```

### `>`

`(> a b)`

Boolean predicate or comparator for conditionals and filtering.

Example fragment:

```scheme
(> 2 1)
```

### `>=`

`(>= a b)`

Boolean predicate or comparator for conditionals and filtering.

Example fragment:

```scheme
(>= 2 1)
```

### `abs`

`(abs value)`

Returns absolute value.

Example fragment:

```scheme
(abs offset)
```

### `analysis`

`(analysis id analysis-clause...)`

Declares an authored FEM/engineering analysis contract tied to model parts and selector tags.

Example fragment:

```scheme
(analysis load-case (linear-static :part body) (fixed :face-tag mounting) (solve :method direct))
```

### `and`

`(and value...)`

Boolean predicate or comparator for conditionals and filtering.

Example fragment:

```scheme
(and true false)
```

### `append`

`(append list...)`

Concatenates lists.

Example fragment:

```scheme
(append front-points back-points)
```

### `apply`

`(apply fn args)`

Calls a function with arguments from a list.

Example fragment:

```scheme
(apply union cutters)
```

### `atan`

`(atan value)`

Single-argument arctangent returning radians.

Example fragment:

```scheme
(atan slope)
```

### `atan2`

`(atan2 y x)`

Two-argument arctangent returning radians.

Example fragment:

```scheme
(atan2 y x)
```

### `attractor-field`

`attractor-field`

Seeded chaotic attractor-style field.

Example fragment:

```scheme
(wall-pattern (:mode attractor-field :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)
```

### `begin`

`(begin clause...)`

Groups multiple model clauses where a single clause position is expected.

Example fragment:

```scheme
(model (begin (params ...) (part body ...)))
```

### `cellular`

`cellular`

Seeded cellular/Voronoi-like displacement field.

Example fragment:

```scheme
(wall-pattern (:mode cellular :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)
```

### `clip-plane`

`(clip-plane geometry :origin '(x y z) :normal '(x y z) [:keep "positive"|"negative"])`

Clips geometry against an oriented plane. `:keep` is text; quote it to avoid unresolved local symbols.

Example fragment:

```scheme
(clip-plane body :origin '(0 0 10) :normal '(0 0 1) :keep "positive")
```

### `cos`

`(cos radians)`

Trigonometric helper using radians.

Example fragment:

```scheme
(cos (deg->rad 45))
```

### `define`

`(define name value)`

Defines a helper value or function in expression scope.

Example fragment:

```scheme
(define wall 2)
```

### `diamond`

`diamond`

Cross-hatched diamond displacement field.

Example fragment:

```scheme
(wall-pattern (:mode diamond :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)
```

### `diamond-field`

`diamond-field`

Alias-style diamond periodic implicit field.

Example fragment:

```scheme
(wall-pattern (:mode diamond-field :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)
```

### `draft`

`(draft angle solid)`

Applies a draft angle to a solid.

Example fragment:

```scheme
(draft 2deg body)
```

### `ellipse`

`(ellipse rx ry)`

Creates an elliptical 2D profile with radii along X and Y.

Example fragment:

```scheme
(ellipse 10 4)
```

### `empty?`

`(empty? value)`

Boolean predicate or comparator for conditionals and filtering.

Example fragment:

```scheme
(empty? '())
```

### `even?`

`(even? number)`

Boolean predicate or comparator for conditionals and filtering.

Example fragment:

```scheme
(even? 2)
```

### `fbm`

`fbm`

Fractal noise displacement field.

Example fragment:

```scheme
(wall-pattern (:mode fbm :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)
```

### `filter`

`(filter fn list)`

Keeps list items where predicate returns true.

Example fragment:

```scheme
(filter (lambda (i) (even? i)) (range 8))
```

### `floor`

`(floor value)`

Rounds down to an integer-valued number.

Example fragment:

```scheme
(floor segments)
```

### `fold`

`(fold fn initial list)`

Reduces a list into a single accumulated value.

Example fragment:

```scheme
(fold + 0 (range 5))
```

### `fourier`

`fourier`

Layered sine/cosine Fourier-style displacement field.

Example fragment:

```scheme
(wall-pattern (:mode fourier :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)
```

### `frame`

`(frame :origin '(x y z) :x-axis '(x y z) :z-axis '(x y z))`

Defines origin/x/z; derives y as z cross x.

Example fragment:

```scheme
(frame :origin '(50 0 15) :x-axis '(0 1 0) :z-axis '(1 0 0))
```

### `groove`

`(groove solid profile path)`

Removes material: sweeps `profile` along `path` and subtracts it from `solid`.

Example fragment:

```scheme
(groove (box 20 20 20) (circle 3) (path (0 0 0) (0 0 30)))
```

### `gyroid`

`gyroid`

triply periodic gyroid implicit field.

Example fragment:

```scheme
(wall-pattern (:mode gyroid :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)
```

### `hammered`

`hammered`

Seeded hammered texture using deterministic noise.

Example fragment:

```scheme
(wall-pattern (:mode hammered :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)
```

### `hull`

`(hull solid...)`

Convex hull of the child solids as a single closed BREP solid.

Example fragment:

```scheme
(hull (sphere 6) (translate 30 0 0 (sphere 6)))
```

### `if`

`(if condition then else)`

Chooses between two expressions from a boolean condition.

Example fragment:

```scheme
(if useCap (sphere r) (cylinder r h))
```

### `import-step`

`(import-step path)`

Imports an exact STEP payload through native Direct OCCT.

Example fragment:

```scheme
(import-step "/absolute/path/component.step")
```

### `lambda`

`(lambda (arg...) body)`

Creates an anonymous function for map/filter/fold helpers.

Example fragment:

```scheme
(lambda (i) (translate (* i pitch) 0 0 cutter))
```

### `let`

`(let ((name value)...) clause...)`

Binds model-level constants for following clauses; bindings in one let are parallel.

Example fragment:

```scheme
(model (let ((r 20)) (part body (sphere r))))
```

### `let*`

`(let* ((name value)...) clause...)`

Sequential model-level binding form; later bindings can use earlier bindings.

Example fragment:

```scheme
(model (let* ((r 20) (h (* r 3))) (part body (cylinder r h))))
```

### `list`

`(list value...)`

Builds a list value.

Example fragment:

```scheme
(list x y z)
```

### `list?`

`(list? value)`

Boolean predicate or comparator for conditionals and filtering.

Example fragment:

```scheme
(list? '())
```

### `map`

`(map fn list ...)`

Transforms each list item with a function.

Example fragment:

```scheme
(map (lambda (i) (* i 10)) (range 4))
```

### `max`

`(max a b...)`

Returns largest number.

Example fragment:

```scheme
(max wall 1.2)
```

### `mesh`

`(mesh :vertices ((x y z) ...) :triangles ((a b c) ...))`

Creates bounded indexed triangle geometry. Open orientable surfaces are allowed; invalid indices, degenerate faces, duplicates, non-manifold edges, or inconsistent winding reject.

Example fragment:

```scheme
(mesh :vertices ((0 0 0) (10 0 0) (0 10 0)) :triangles ((0 1 2)))
```

### `mesh-anchor`

`(mesh-anchor triangle-index barycentric-0 barycentric-1 barycentric-2)`

Declares one exact triangle seed used inside a native mesh `surface-trim` path.

Example fragment:

```scheme
(mesh-anchor 42 0.2 0.3 0.5)
```

### `meta`

`(meta key value)`

Stores literal model metadata in Core IR; `:title` labels the exported document and `units strict` enables dimensional checks.

Example fragment:

```scheme
(meta :title "Bottle cage")
```

### `min`

`(min a b...)`

Returns smallest number.

Example fragment:

```scheme
(min wall max-wall)
```

### `neovius`

`neovius`

Triply periodic Neovius implicit field.

Example fragment:

```scheme
(wall-pattern (:mode neovius :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)
```

### `not`

`(not value)`

Boolean predicate or comparator for conditionals and filtering.

Example fragment:

```scheme
(not false)
```

### `null?`

`(null? value)`

Boolean predicate or comparator for conditionals and filtering.

Example fragment:

```scheme
(null? '())
```

### `odd?`

`(odd? number)`

Boolean predicate or comparator for conditionals and filtering.

Example fragment:

```scheme
(odd? 2)
```

### `or`

`(or value...)`

Boolean predicate or comparator for conditionals and filtering.

Example fragment:

```scheme
(or true false)
```

### `place-component`

`(place-component (component :param value ...) :from port-id :to (port-ref part-id port-id) :normal aligned|opposed [:roll degrees] [:offset '(x y z)] [:mirror none|x|y])`

Mates source and target ports without Euler math.

Example fragment:

```scheme
(place-component (latch) :from mount :to (port-ref enclosure side-left-latch) :normal opposed)
```

### `polyhedron`

`(polyhedron :vertices ((x y z) ...) :triangles ((a b c) ...))`

Creates one closed orientable indexed triangle solid after deterministic topology validation.

Example fragment:

```scheme
(polyhedron :vertices ((0 0 0) (10 0 0) (0 10 0) (0 0 10)) :triangles ((0 2 1) (0 1 3) (1 2 3) (2 0 3)))
```

### `port`

`(port id :type type-id :frame frame [:compatible-with '(type-id ...)] [:params ((name value) ...)])`

Declares one stable typed local interface.

Example fragment:

```scheme
(port mount :type "mount.v1" :frame local-frame)
```

### `port-ref`

`(port-ref part-id port-id)`

References one target port.

Example fragment:

```scheme
(port-ref enclosure side-left-latch)
```

### `ports`

`(ports (port ...) ...)`

Groups local interfaces.

Example fragment:

```scheme
(ports (port mount :type "mount.v1" :frame local-frame))
```

### `protrude`

`(protrude image-path height [:width w] [:depth d] [:fit contain|stretch] [:foreground dark|light])`

Raises continuous raster foreground coverage above local Z=0. One physical dimension preserves source aspect ratio; two contain and center by default. `:fit stretch` explicitly fills a non-matching box. Transparent pixels remain empty; an internal closure epsilon stays below the authored base plane.

Example fragment:

```scheme
(protrude image-path 4 :width 100 :depth 70 :fit contain :foreground dark)
```

### `quote`

`(quote value) or 'value`

Prevents evaluation of symbols/lists for literal data such as align tuples.

Example fragment:

```scheme
'(center center min)
```

### `range`

`(range count)`

Builds integer indices from 0 to count - 1.

Example fragment:

```scheme
(range 8)
```

### `reduce`

`(reduce fn initial list)`

Reduces a list into a single accumulated value.

Example fragment:

```scheme
(fold + 0 (range 5))
```

### `regular-polygon`

`(regular-polygon sides radius :rotation deg)`

Creates a regular n-gon 2D profile by side count and circumradius.

Example fragment:

```scheme
(regular-polygon 6 10)
```

### `reverse`

`(reverse list)`

Returns list items in reverse order.

Example fragment:

```scheme
(reverse points)
```

### `rib`

`(rib solid profile path)`

Adds material: sweeps `profile` along `path` and unions it onto `solid`.

Example fragment:

```scheme
(rib (box 20 20 20) (circle 3) (path (0 0 0) (0 0 30)))
```

### `ribs`

`ribs`

Straight rib pattern along the shell parameter direction.

Example fragment:

```scheme
(wall-pattern (:mode ribs :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)
```

### `rings`

`rings`

Ring bands around the shell parameter direction.

Example fragment:

```scheme
(wall-pattern (:mode rings :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)
```

### `schwarz-d`

`schwarz-d`

Triply periodic Schwarz D implicit field.

Example fragment:

```scheme
(wall-pattern (:mode schwarz-d :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)
```

### `schwarz-p`

`schwarz-p`

Triply periodic Schwarz P implicit field.

Example fragment:

```scheme
(wall-pattern (:mode schwarz-p :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)
```

### `sin`

`(sin radians)`

Trigonometric helper using radians.

Example fragment:

```scheme
(sin (deg->rad 45))
```

### `slot-arc`

`(slot-arc radius start end width)`

Curved (annular) obround: a circular-arc centerline of given radius from `start` to `end` degrees, thickened by width.

Example fragment:

```scheme
(slot-arc 20 0 90 10)
```

### `slot-center-point`

`(slot-center-point cx cy px py width)`

Obround 2D profile from a center point to an end point, with width.

Example fragment:

```scheme
(slot-center-point 0 0 20 0 10)
```

### `slot-center-to-center`

`(slot-center-to-center separation width)`

Obround 2D profile specified by the distance between the two end-arc centers.

Example fragment:

```scheme
(slot-center-to-center 30 10)
```

### `slot-overall`

`(slot-overall length width)`

Creates an obround (stadium) 2D profile of given overall length and width.

Example fragment:

```scheme
(slot-overall 40 10)
```

### `spiral`

`spiral`

Spiral rib pattern across shell parameters.

Example fragment:

```scheme
(wall-pattern (:mode spiral :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)
```

### `tag-edge`

`(tag-edge id :edge or :edges selector target)`

Names a stable edge selection for downstream operations and analysis.

Example fragment:

```scheme
(tag-edge rim :edges "top" body)
```

### `tag-edges`

`(tag-edges id :edge or :edges selector target)`

Names a stable edge selection for downstream operations and analysis.

Example fragment:

```scheme
(tag-edges rim :edges "top" body)
```

### `tag-face`

`(tag-face id :face or :faces selector target)`

Names a stable face selection for downstream operations and analysis.

Example fragment:

```scheme
(tag-face mounting :faces "bottom" body)
```

### `tag-vertex`

`(tag-vertex id :vertex selector target)`

Names a stable vertex selection for downstream operations and analysis.

Example fragment:

```scheme
(tag-vertex datum :vertex "top" body)
```

### `tan`

`(tan radians)`

Trigonometric helper using radians.

Example fragment:

```scheme
(tan (deg->rad 45))
```

### `tapped-hole`

`(tapped-hole :iso "M8" :length len [:radius r] [:pitch p] [:depth d] [:base-width w] [:crest-width w] [:lefthand #t])`

A tapped (internal female) thread cut as a positive cavity: a named-radius bore cylinder at the ISO minor diameter unioned with a helical relief ridge whose crest reaches the major diameter. `:iso "M8"` decodes a metric designation; an equal-nominal `thread` mates with it.

Example fragment:

```scheme
(tapped-hole :iso "M8" :length 14)
```

### `thread`

`(thread :radius r :pitch p :length len :depth d [:base-width w] [:crest-width w] [:female #t] [:clearance c] [:lefthand #t] [:iso "M4"])`

Parametric helical thread: a core cylinder plus a `helical-ridge` (male), or a ridge cutter (`:female`). `:iso "M4"` decodes a metric designation into pitch/radius.

Example fragment:

```scheme
(thread :radius 8 :pitch 2 :length 16 :depth 1)
```

### `torus`

`(torus major minor)`

Creates a ring torus: tube of radius `minor` swept at distance `major` from the Z axis.

Example fragment:

```scheme
(torus 20 5)
```

### `trapezoid`

`(trapezoid bottom top height :skew s)`

Creates a trapezoid 2D profile (parallel bottom/top widths, given height, optional skew).

Example fragment:

```scheme
(trapezoid 20 10 8 :skew 3)
```

### `verify`

`(verify (tag id) [(intent text)] [(severity error|warning)] [(when bool-expr)] (metric id metric-expr) (expect id predicate))`

Declares one conditional runtime check with intent, severity, and typed evidence.

Example fragment:

```scheme
(verify (tag mesh-clean) (metric bad-edges (stl non-manifold-edge-count)) (expect bad-edges (= 0)))
```

### `view`

`(view id (offset-part part dx dy dz)...) `

Declares a preview-only exploded or print-layout view without changing export geometry.

Example fragment:

```scheme
(view print-layout (offset-part lid 90 0 0) (offset-part body 0 0 0))
```

### `voronoi-cell`

`(voronoi-cell sites index width height inset)`

Creates one exact bounded Voronoi polygon, uniformly inset and expressed relative to its selected site.

Example fragment:

```scheme
(voronoi-cell (voronoi-cells 3 3 12 12 1.5 23) 4 40 40 1.2)
```

### `wedge`

`(wedge dx dy dz xmin zmin xmax zmax :align '(x y z))`

Creates a wedge/ramp solid: a dx×dy×dz box whose top face is shrunk to the xmin..xmax / zmin..zmax window.

Example fragment:

```scheme
(wedge 20 10 20 5 5 15 15)
```

### `zero?`

`(zero? number)`

Boolean predicate or comparator for conditionals and filtering.

Example fragment:

```scheme
(zero? 2)
```
