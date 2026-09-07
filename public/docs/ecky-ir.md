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

Scope here:

- `ecky/cad` exported CAD forms and ops
- `ecky/core` helper functions shipped with Ecky
- `ecky/params` parameter forms
- lowerer-visible keywords people otherwise guess from source

Out of scope here:

- full Steel standard library reference
- backend implementation internals
- UI behavior outside `.ecky` authoring

Mental model:

- `.ecky` is Scheme surface syntax
- compiler lowers it into Core IR
- verifier checks value kinds and op signatures
- native execution maps Core IR into `OcctPlan`, then the precompiled Direct OCCT runner; FreeCAD lowering is optional interop

Read this order if new:

- `Forms and Structure`
- `Params and Controls`
- `Primitive Signatures`
- `Boolean and Transform Signatures`
- `Surface and Path Signatures`
- `Array and Frame Signatures`
- `Special / Custom Operations`
- `Selector Strings and Named Keywords`

## Forms and Structure

This is top-level authoring grammar. If source feels mysterious, start here.

### `model`

```scheme
(model
  ...)
```

- root form for one design
- source must start with `(model ...)`
- accepts the direct clauses listed in `Complete Compiler Surface`: `params`,
  `verify`, `part`, `feature`, topology tags, `view`, and `analysis`
- reusable helper `define`s and `define-component` declarations belong before
  `(model ...)`; derived values depending on model params belong in `let*`

### `part`

```scheme
(part body expr)
(part body "Human Label" expr)
```

- positional 1: part id symbol
- positional 2: optional display label text
- final positional: expression producing geometry

### `feature`

Two forms exist:

```scheme
(feature body :role shell expr)
(feature body :role shell :params (width height) expr)
```

- positional 1: feature id symbol
- required keyword: `:role`
- optional keyword: `:params`
- final positional: expression producing geometry

Use `feature` when geometry needs explicit semantic identity, role, and parameter-key tracking.

### `build`

```scheme
(build
  (shape outer expr)
  (shape cavity expr)
  (result expr))
```

- local binding block
- accepts `shape` bindings plus one `result`
- `result` must come once
- do not place new `shape` bindings after `result`

### `shape`

```scheme
(shape ribs expr)
```

`shape` is not geometry op. It is bind statement inside `build`.

- positional 1: local binding name
- positional 2: expression producing value

Read it as:

- bind intermediate value
- give later code a name
- keep boolean stacks readable

### `result`

```scheme
(result expr)
```

- final value returned by `build`

### `assembly` (planned)

Reserved shape sketch:

```scheme
(model
  (assembly exploded_preview
    ...))
```

- planned top-level clause for explicit multi-part assembly recipes
- spelling reserved in book now; runtime/compiler support deferred
- spec'd grammar reserved now; implementation deferred until views prove the display/manufacturing split
- intended to formalize what component packages already do at the package layer
- assemblies stay placement-based as today; no mate/joint solver implied
- examples here mark intent only, not accepted source today
- until implementation lands, keep physical bodies as `part`s, use `view` for preview-only offsets, and use component packages for solved assembly workflows

### `export` (planned)

Reserved shape sketch:

```scheme
(model
  (export manufacturing
    ...))
```

- planned top-level clause for authored export/manufacturing policy
- spelling reserved in book now; runtime/compiler support deferred
- reserved until views prove the display/manufacturing split
- preview transforms never affect STL or STEP artifacts
- examples here mark intent only, not accepted source today
- until implementation lands, use current export commands, artifact manifests, and package output modes outside `.ecky` source

## Components

A component is a named, parameterized, closed geometry unit. Define once,
instantiate anywhere, override knobs at the call site. `model` and `part`
stay valid forever; components add reuse on top without changing them.

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
(define-component pin ((number d 2))
  (verify (tag pin_ok) (metric min_wall_thickness "body") (expect (>= value 1)))
  (cylinder d 10 48))

(part left (pin :d 3))   ; verify tag becomes left/pin_ok
```

A pasted component therefore carries its own checks — reuse includes proof.

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
    (tag front_gap body.front_window_1)
    (intent "Keep lid clearance printable")
    (severity error)
    (when assembly-preview)
    (metric gap (clearance min-distance body lid))
    (expect gap (>= 3)))
  (part body (box 10 10 10))
  (part lid (box 10 10 10)))
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

Authoring rule:

- fix geometry or exports until `verify` passes
- do not remove `verify` clauses to bypass authored requirements

## Params and Controls

Parameter forms live in `ecky/params`.

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

Humans may use bare numbers because Ecky's base units are millimetres and
degrees. Agent-generated physical dimensions should use suffixed literals like
mm/cm/in/deg/rad when the suffix makes intent clearer.

Examples:

- `12mm`
- `2.54cm`
- `0.25in`
- `45deg`
- `1.5708rad`

Prompt generators use suffixed literals for physical lengths and angles. Bare
numbers remain appropriate for counts, ratios, segments, and unitless math.

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

Helpers here come from `ecky/core`.

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

Verifier-backed value kinds:

- `Any`
- `Number`
- `Boolean`
- `Text`
- `List`
- `Point2`
- `Point3`
- `Sketch`
- `Path`
- `Frame`
- `Compound`
- `Solid`

Core node kinds:

- `Literal`
- `Reference`
- `Build`
- `Let`
- `If`
- `Call`
- `Range`
- `Map`
- `Apply`
- `List`
- `Group`

If typecheck fails, compiler is checking these kinds, not backend Python text.

## Primitive Signatures

These are explicit authored calls. When backend diverges, caveat is spelled out.

### `box`

- signature: `box width depth height`
- result: `Solid`
- keywords:
  - `:align (x y z)` with each axis one of `min | center | max`

### `sphere`

- signature: `sphere radius`
- result: `Solid`
- keywords:
  - `:align (x y z)`

### `cylinder`

- signature: `cylinder radius height`
- signature: `cylinder radius height segments`
- result: `Solid`
- keywords:
  - `:align (x y z)`

### `cone`

- signature: `cone radius1 radius2 height`
- signature: `cone radius1 radius2 height segments`
- result: `Solid`
- keywords:
  - `:align (x y z)`

### `circle`

- signature: `circle radius`
- signature: `circle radius segments`
- result: `Sketch`

### `rectangle`

- signature: `rectangle width height`
- result: `Sketch`

### `rounded-rect`

- signature: `rounded-rect width height radius`
- result: `Sketch`

### `rounded-polygon`

- signature: `rounded-polygon points radius`
- signature: `rounded-polygon points radius segments`
- `points`: list of 2D points
- result: `Sketch`

### `polygon`

- signature: `polygon points`
- `points`: list of 2D points
- result: `Sketch`

### `profile`

- signature: `profile loop1 loop2 ...`
- signature: `profile :outer outer-loop :holes hole-loop-or-list`
- result: `Sketch`

Rules:

- positional form treats every argument as sketch/wire loop
- keyword form accepts `:outer` and `:holes` only
- current hole-aware lowerers expect exactly one outer loop when `:holes` is used

### `make-face`

- signature: `make-face wire1 wire2 ...`
- result: `Sketch`
- use when you already have wire-like geometry and need face/sketch result

### `text`

- signature: `text string size [:font selector]`
- result: `Sketch`
- normal use: feed into `extrude`
- `:font` belongs to `text`, not `extrude`
- `selector` accepts an installed font family name or an absolute `.ttf`/`.otf` path
- a literal selector changes one call; a shared `select` parameter can drive several calls

Example:

```scheme
(extrude (text "HELLO" 12 :font "Arial") 2)
```

One label only:

```scheme
(union
  (extrude (text "MORNING" 12 :font "Arial") 2)
  (translate 0 20 0
    (extrude (text "EVENING" 12 :font "Impact") 2)))
```

Shared parameter:

```scheme
(model
  (params
    (select label-font "Arial" :label "Label Font"
      :options (("Arial" "Arial") ("Impact" "Impact"))))
  (part labels
    (extrude (text "HELLO" 12 :font label-font) 2)))
```

### `svg`

- native signature: `svg path`
- FreeCAD interop signature: `svg path [target-width] [target-height] [fit-mode]`
- result: `Sketch`

Known fit modes from lowerers/tests:

- `"contain"`
- `"cover"`
- `"stretch"`
- `"fill"`

### `import-stl`

- signature: `import-stl path`
- result: imported solid/mesh-like geometry

### `ring`

- signature: `ring outer-radius inner-radius`
- signature: `ring outer-radius inner-radius segments`
- result: `Sketch`
- lowering behavior: alias for profile-with-hole semantics

## Boolean and Transform Signatures

### `union`

- signature: `union shape1 shape2 ...`
- result: shape-like value

### `fuse`

- alias of `union`

### `difference`

- signature: `difference base cut1 cut2 ...`
- result: shape-like value

### `cut`

- alias of `difference`

### `intersection`

- signature: `intersection shape1 shape2 ...`
- result: shape-like value

### `common`

- alias of `intersection`

### `xor`

- signature: `xor shape1 shape2 ...`
- result: shape-like value

Boolean rule:

- minimum arity: one shape

### `translate`

- signature: `translate x y z shape`
- result kind follows input shape kind

### `rotate`

- signature: `rotate x y z shape`
- result kind follows input shape kind

### `scale`

- verifier accepts:
  - `scale factor shape`
  - `scale x y z shape`
- native planner supports both forms
- FreeCAD lowerer currently expects explicit `x y z shape`
- result kind follows input shape kind

### `mirror`

- signature: `mirror axis offset shape`
- `axis`: string or symbol naming mirror axis
- `offset`: numeric plane offset
- result kind follows input shape kind

Examples:

```scheme
(translate 20 0 0 (box 10 10 10))
(rotate 0 0 45 (box 10 10 10))
(scale 2 2 1 (circle 10))
(mirror 'x 0 (box 10 10 10))
```

## Surface and Path Signatures

### `extrude`

- signature: `extrude profile distance`
- result: `Solid`
- backend keyword:
  - `:symmetric` boolean

### `revolve`

- signature: `revolve profile angle`
- result: `Solid`

### `loft`

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

- signature: `shell thickness solid`
- result: `Solid`
- optional keyword:
  - `:faces selector`

### `offset`

- signature: `offset amount profile`
- result: `Sketch`
- optional keyword:
  - `:openings sketch-or-sketch-list`

### `offset-rounded`

- signature: `offset-rounded amount profile`
- result: `Sketch`
- optional keyword:
  - `:openings sketch-or-sketch-list`

### `fillet`

- signature: `fillet radius solid`
- result: `Solid`
- optional keyword:
  - `:edges selector`

### `chamfer`

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
- verifier-backed form is 3 positional args

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
- optional second positional in lowerers: `closed`
- optional keywords:
  - `:closed` boolean
  - `:tangents` point-list
  - `:tangent-scalars` numeric list
- result: `Sketch`

Notes:

- verifier only requires point-list first
- lowerers accept tangent hints
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

- signature: `linear-array count x y z shape`
- result: same geometry family as input

### `radial-array`

- signature: `radial-array count angle radius shape`
- result: same geometry family as input

### `grid-array`

- signature: `grid-array rows cols x y shape`
- result: same geometry family as input

### `arc-array`

- signature: `arc-array count radius start-angle end-angle shape`
- result: same geometry family as input

### `repeat`

- signature: `repeat index count expr`
- verifier recognizes form
- geometry lowerers do not currently expose dedicated authored lowering path like `repeat-union` / `repeat-compound` / `repeat-pick`

### `repeat-union`

- signature: `repeat-union index count expr`
- index must be symbol
- body should produce geometry
- result: union/fused geometry

### `repeat-compound`

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

- verifier signature: `location [frame]`
- authored backend-safe signature: `location frame`
- optional keywords:
  - `:offset (x y z)`
  - `:rotate (x y z)`
- result: `Frame`

### `path-frame`

- signature: `path-frame path`
- optional keywords:
  - `:at start | end | number`
  - `:up (x y z)`
- result: `Frame`

### `place`

- signature: `place frame shape`
- optional keywords:
  - `:offset (x y z)`
  - `:rotate (x y z)`
- result: placed shape

### `clip-box`

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

These are exported authored ops outside generic primitive/boolean/surface families.

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

Procedural sampled shell / loft op.

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

Pattern op applied to shell/solid target.

Pattern shape seen in repo:

```scheme
(wall-pattern
  (:mode gyroid :depth 0.6 :uFreq 4 :vFreq 5 :phase 0.2)
  shape)
```

Observed options:

- `:mode`
- `:depth`
- `:uFreq`
- `:vFreq`
- `:phase`

Observed modes:

- `gyroid`
- `cellular`
- `fbm`
- `ribs`

Backend caveat:

- native OCCT handles BREP operations; `wall-pattern` remains mesh-only

## Selector Strings and Named Keywords

This is where people waste time guessing.

### Shared keyword value expectations

Verifier enforces:

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

Observed canonical meaning:

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

- When target metadata supplies `sourcePath`, edit that exact file. `sourceFolder`
  is its workspace; `sourceState` reports clean, pending, or failed source.
- The folder watcher waits for a settled edit, appends one version, validates,
  renders a preview, and records status. Read raw diagnostics before success claims.
- Do not export over bound source, write history storage directly, or invent a
  commit/finalize step. Compatibility buffers apply only without `sourcePath`.

## Complete Compiler Surface

Generated from the same Rust registry used by MCP manifests and agent prompts.
Do not edit rows by hand; run `npm run generate:prompt`.

<!-- ECKY_GENERATED_SURFACE_REFERENCE_START -->
| Form | Kind | Signature | Backends | Description | Example |
| --- | --- | --- | --- | --- | --- |
| `*` | numericHelper | `(* a b...)` | freecad, legacy-build123d, mesh/native | Multiplies numbers. | `(* radius 2)` |
| `+` | numericHelper | `(+ a b...)` | freecad, legacy-build123d, mesh/native | Adds numbers. | `(+ width clearance)` |
| `-` | numericHelper | `(- a b...)` | freecad, legacy-build123d, mesh/native | Subtracts numbers or negates one number. | `(- outer inner)` |
| `/` | numericHelper | `(/ a b...)` | freecad, legacy-build123d, mesh/native | Divides numbers. | `(/ width 2)` |
| `<` | booleanHelper | `(< a b)` | freecad, legacy-build123d, mesh/native | Boolean predicate or comparator for conditionals and filtering. | `(< 2 1)` |
| `<=` | booleanHelper | `(<= a b)` | freecad, legacy-build123d, mesh/native | Boolean predicate or comparator for conditionals and filtering. | `(<= 2 1)` |
| `=` | booleanHelper | `(= a b)` | freecad, legacy-build123d, mesh/native | Boolean predicate or comparator for conditionals and filtering. | `(= 2 1)` |
| `>` | booleanHelper | `(> a b)` | freecad, legacy-build123d, mesh/native | Boolean predicate or comparator for conditionals and filtering. | `(> 2 1)` |
| `>=` | booleanHelper | `(>= a b)` | freecad, legacy-build123d, mesh/native | Boolean predicate or comparator for conditionals and filtering. | `(>= 2 1)` |
| `abs` | numericHelper | `(abs value)` | freecad, legacy-build123d, mesh/native | Returns absolute value. | `(abs offset)` |
| `analysis` | modelClause | `(analysis id analysis-clause...)` | freecad, legacy-build123d, mesh/native | Declares an authored FEM/engineering analysis contract tied to model parts and selector tags. | `(analysis load-case (linear-static :part body) (fixed :face-tag mounting) (solve :method direct))` |
| `and` | booleanHelper | `(and value...)` | freecad, legacy-build123d, mesh/native | Boolean predicate or comparator for conditionals and filtering. | `(and true false)` |
| `append` | expressionForm | `(append list...)` | freecad, legacy-build123d, mesh/native | Concatenates lists. | `(append front-points back-points)` |
| `apply` | expressionForm | `(apply fn args)` | freecad, legacy-build123d, mesh/native | Calls a function with arguments from a list. | `(apply union cutters)` |
| `arc-array` | cadOp | `(arc-array count radius start-angle end-angle geometry)` | freecad, legacy-build123d, mesh/native | Repeats geometry along an arc. | `(arc-array 8 30 0 180 notch)` |
| `atan` | numericHelper | `(atan value)` | freecad, legacy-build123d, mesh/native | Single-argument arctangent returning radians. | `(atan slope)` |
| `atan2` | numericHelper | `(atan2 y x)` | freecad, legacy-build123d, mesh/native | Two-argument arctangent returning radians. | `(atan2 y x)` |
| `attractor-field` | wallPatternMode | `attractor-field` | mesh/native | Seeded chaotic attractor-style field. | `(wall-pattern (:mode attractor-field :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)` |
| `begin` | modelWrapper | `(begin clause...)` | freecad, legacy-build123d, mesh/native | Groups multiple model clauses where a single clause position is expected. | `(model (begin (params ...) (part body ...)))` |
| `bezier-path` | cadOp | `(bezier-path points)` | freecad, legacy-build123d, mesh/native | Builds a Bezier path from control points. | `(bezier-path points)` |
| `box` | cadOp | `(box x y z :align '(x y z))` | freecad, legacy-build123d, mesh/native | Creates an axis-aligned rectangular solid. | `(box 40 20 10 :align '(min center min))` |
| `bspline` | cadOp | `(bspline points :closed #t\|#f)` | freecad, legacy-build123d, mesh/native | Builds a 2D B-spline sketch from control points. | `(bspline points :closed #t)` |
| `build` | cadOp | `(build expr...)` | freecad, legacy-build123d, mesh/native | Build container for grouped construction forms. | `(build (shape body) (result body))` |
| `cell-distance2` | numericHelper | `(cell-distance2 x y seed)` | freecad, legacy-build123d, mesh/native | Distance-like deterministic value to nearest jittered cellular site. | `(cell-distance2 x y seed)` |
| `cellular` | wallPatternMode | `cellular` | mesh/native | Seeded cellular/Voronoi-like displacement field. | `(wall-pattern (:mode cellular :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)` |
| `chamfer` | cadOp | `(chamfer distance [:edges selector] solid)` | freecad, legacy-build123d, mesh/native | Bevels edges of a solid. \`:edges\` accepts coarse selectors like \`bottom\`, \`front\`, \`axis-z\`, \`y-max\`, or \`x-min+z-max\`; exact backends also accept \`target-id:<id>\` and \`target-ids:<id>\|<id>\`. | `(chamfer 1 :edges "bottom" body)` |
| `circle` | cadOp | `(circle radius segments)` | freecad, legacy-build123d, mesh/native | Creates a circular sketch/profile. | `(circle 20 64)` |
| `clamp` | numericHelper | `(clamp value min max)` | freecad, legacy-build123d, mesh/native | Constrains value to a numeric interval. | `(clamp depth 0 3)` |
| `clip-box` | cadOp | `(clip-box geometry :x '(min max) :y '(min max) :z '(min max))` | freecad, legacy-build123d, mesh/native | Clips geometry by an axis-aligned box. | `(clip-box body :x '(0 100) :y '(-30 30) :z '(0 40))` |
| `clip-plane` | cadOp | `(clip-plane geometry :origin '(x y z) :normal '(x y z) [:keep positive\|negative])` | freecad, legacy-build123d, mesh/native | Clips geometry against an oriented plane. | `(clip-plane body :origin '(0 0 10) :normal '(0 0 1) :keep positive)` |
| `common` | cadOp | `(common solid...)` | freecad, legacy-build123d, mesh/native | Keeps shared volume of solids. | `(common a b)` |
| `compound` | cadOp | `(compound geometry...)` | freecad, legacy-build123d, mesh/native | Groups geometry without fusing into one solid. | `(compound body bolts)` |
| `concat-map` | expressionForm | `(concat-map fn list)` | freecad, legacy-build123d, mesh/native | Maps each item to a list and concatenates the results. | `(flat-map (lambda (i) (list i (- i))) (range 3))` |
| `cone` | cadOp | `(cone r1 r2 height segments)` | freecad, legacy-build123d, mesh/native | Creates a cone or tapered cylinder along local Z. | `(cone 12 6 30 48)` |
| `cos` | numericHelper | `(cos radians)` | freecad, legacy-build123d, mesh/native | Trigonometric helper using radians. | `(cos (deg->rad 45))` |
| `cube` | numericHelper | `(cube value)` | freecad, legacy-build123d, mesh/native | Raises a number to a small fixed power. | `(cube radius)` |
| `cut` | cadOp | `(cut base cutter...)` | freecad, legacy-build123d, mesh/native | Subtracts cutter solids from a base solid. | `(cut body hole)` |
| `cylinder` | cadOp | `(cylinder radius height segments)` | freecad, legacy-build123d, mesh/native | Creates a cylinder along local Z. | `(cylinder 8 30 48)` |
| `define` | expressionForm | `(define name value)` | freecad, legacy-build123d, mesh/native | Defines a helper value or function in expression scope. | `(define wall 2)` |
| `define-component` | componentPlacementForm | `(define-component id (signature...) [(ports ...)] [(verify ...)] geometry)` | freecad, legacy-build123d, mesh/native | Declares closed reusable local geometry and optional ports. | `(define-component latch () (ports (port mount :type "mount.v1" :frame (frame :origin '(0 0 0) :x-axis '(1 0 0) :z-axis '(0 0 1)))) (box 20 4 2))` |
| `deg` | numericHelper | `(deg radians)` | freecad, legacy-build123d, mesh/native | Converts radians to degrees. | `(deg angle-rad)` |
| `deg->rad` | numericHelper | `(deg->rad degrees)` | freecad, legacy-build123d, mesh/native | Converts degrees to radians. | `(deg->rad 90)` |
| `diamond` | wallPatternMode | `diamond` | mesh/native | Cross-hatched diamond displacement field. | `(wall-pattern (:mode diamond :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)` |
| `diamond-field` | wallPatternMode | `diamond-field` | mesh/native | Alias-style diamond periodic implicit field. | `(wall-pattern (:mode diamond-field :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)` |
| `difference` | cadOp | `(difference base cutter...)` | freecad, legacy-build123d, mesh/native | Subtracts cutter solids from a base solid. | `(difference body hole)` |
| `draft` | cadOp | `(draft angle solid)` | freecad, legacy-build123d, mesh/native | Applies a draft angle to a solid. | `(draft 2deg body)` |
| `ellipse` | cadOp | `(ellipse rx ry)` | freecad, legacy-build123d, mesh/native | Creates an elliptical 2D profile with radii along X and Y. | `(ellipse 10 4)` |
| `empty?` | booleanHelper | `(empty? value)` | freecad, legacy-build123d, mesh/native | Boolean predicate or comparator for conditionals and filtering. | `(empty? '())` |
| `end` | coreConstant | `end` | freecad, legacy-build123d, mesh/native | Symbolic endpoint accepted by path-frame \`:at\`. | `(path-frame rail :at end :up '(0 0 1))` |
| `enumerate` | expressionForm | `(enumerate list)` | freecad, legacy-build123d, mesh/native | Pairs each index with its list item. | `(map (lambda ((index value)) (list index value)) (enumerate (range 4)))` |
| `even?` | booleanHelper | `(even? number)` | freecad, legacy-build123d, mesh/native | Boolean predicate or comparator for conditionals and filtering. | `(even? 2)` |
| `extrude` | cadOp | `(extrude sketch-or-image height [:symmetric #t\|#f] [:width w] [:depth d] [:fit contain\|stretch] [:threshold 0..1] [:foreground dark\|light])` | freecad, legacy-build123d, mesh/native | Extrudes a sketch, or traces raster foreground coverage into contours before the same extrusion. One raster dimension preserves source aspect ratio; two contain and center by default. \`:fit stretch\` explicitly fills a non-matching box. | `(extrude image-path 3 :width 40 :depth 30 :fit contain :threshold 0.5 :foreground dark)` |
| `false` | coreConstant | `false` | freecad, legacy-build123d, mesh/native | Boolean constant equivalent to \`#t\` or \`#f\`. | `(if false body fallback)` |
| `fbm` | wallPatternMode | `fbm` | mesh/native | Fractal noise displacement field. | `(wall-pattern (:mode fbm :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)` |
| `fbm2` | numericHelper | `(fbm2 x y seed octaves lacunarity gain)` | freecad, legacy-build123d, mesh/native | fractal Brownian motion built from deterministic noise2 octaves. | `(fbm2 x y seed 4 2.0 0.5)` |
| `feature` | modelClause | `(feature id :role role [:params (key...)] geometry)` | freecad, legacy-build123d, mesh/native | Declares renderable geometry plus semantic role and primary control metadata. | `(feature shell :role enclosure :params (width wall) (box width 40 wall))` |
| `fillet` | cadOp | `(fillet radius [:edges selector] solid)` | freecad, legacy-build123d, mesh/native | Rounds edges of a solid. \`:edges\` accepts coarse selectors like \`top\`, \`left\`, \`axis-z\`, \`x-min\`, or \`x-min+z-max\`; exact backends also accept \`target-id:<id>\` and \`target-ids:<id>\|<id>\`. | `(fillet 2 :edges "x-min+z-max" body)` |
| `filter` | expressionForm | `(filter fn list)` | freecad, legacy-build123d, mesh/native | Keeps list items where predicate returns true. | `(filter (lambda (i) (even? i)) (range 8))` |
| `flat-map` | expressionForm | `(flat-map fn list)` | freecad, legacy-build123d, mesh/native | Maps each item to a list and concatenates the results. | `(flat-map (lambda (i) (list i (- i))) (range 3))` |
| `floor` | numericHelper | `(floor value)` | freecad, legacy-build123d, mesh/native | Rounds down to an integer-valued number. | `(floor segments)` |
| `fold` | expressionForm | `(fold fn initial list)` | freecad, legacy-build123d, mesh/native | Reduces a list into a single accumulated value. | `(fold + 0 (range 5))` |
| `for-compound` | cadOp | `(for-compound list fn)` | freecad, legacy-build123d, mesh/native | Maps list values to geometry and compounds the result. | `(for-compound points (lambda (p) ...))` |
| `for-union` | cadOp | `(for-union list fn)` | freecad, legacy-build123d, mesh/native | Maps list values to solids and unions the result. | `(for-union (range 6) (lambda (i) ...))` |
| `fourier` | wallPatternMode | `fourier` | mesh/native | Layered sine/cosine Fourier-style displacement field. | `(wall-pattern (:mode fourier :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)` |
| `frame` | componentPlacementForm | `(frame :origin '(x y z) :x-axis '(x y z) :z-axis '(x y z))` | freecad, legacy-build123d, mesh/native | Defines origin/x/z; derives y as z cross x. | `(frame :origin '(50 0 15) :x-axis '(0 1 0) :z-axis '(1 0 0))` |
| `fuse` | cadOp | `(fuse solid...)` | freecad, legacy-build123d, mesh/native | Boolean union/fuse of solids. | `(fuse a b c)` |
| `grid-array` | cadOp | `(grid-array rows cols dx dy geometry)` | freecad, legacy-build123d, mesh/native | Repeats geometry on a 2D grid. | `(grid-array 3 5 12 12 hole)` |
| `groove` | cadOp | `(groove solid profile path)` | freecad, legacy-build123d, mesh/native | Removes material: sweeps \`profile\` along \`path\` and subtracts it from \`solid\`. | `(groove (box 20 20 20) (circle 3) (path (0 0 0) (0 0 30)))` |
| `gyroid` | wallPatternMode | `gyroid` | mesh/native | triply periodic gyroid implicit field. | `(wall-pattern (:mode gyroid :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)` |
| `hammered` | wallPatternMode | `hammered` | mesh/native | Seeded hammered texture using deterministic noise. | `(wall-pattern (:mode hammered :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)` |
| `hash-signed` | numericHelper | `(hash-signed x y seed)` | freecad, legacy-build123d, mesh/native | Deterministic signed hash value for offsets and jitter. | `(hash-signed ix iy seed)` |
| `hash01` | numericHelper | `(hash01 x y seed)` | freecad, legacy-build123d, mesh/native | Deterministic hash value in the 0..1 range for procedural variation. | `(hash01 ix iy seed)` |
| `helical-ridge` | cadOp | `(helical-ridge :radius r :pitch p :height h :base-width w :crest-width w :depth d [:female #t] [:clearance c] [:lefthand #t])` | freecad, legacy-build123d, mesh/native | Creates a printable trapezoid ridge swept along a cylindrical helix. | `(helical-ridge :radius 32 :pitch 5.25 :height 16.8 :base-width 1.45 :crest-width 0.55 :depth 1.5)` |
| `henon-points` | pointListHelper | `(henon-points count scale)` | freecad, legacy-build123d, mesh/native | Samples deterministic Henon map points. | `(henon-points 100 12)` |
| `hull` | cadOp | `(hull solid...)` | mesh/native | Convex hull of the child solids as a single closed BREP solid. | `(hull (sphere 6) (translate 30 0 0 (sphere 6)))` |
| `if` | expressionForm | `(if condition then else)` | freecad, legacy-build123d, mesh/native | Chooses between two expressions from a boolean condition. | `(if useCap (sphere r) (cylinder r h))` |
| `import-step` | cadOp | `(import-step path)` | mesh/native | Imports an exact STEP payload through native Direct OCCT. | `(import-step "/absolute/path/component.step")` |
| `import-stl` | cadOp | `(import-stl path [:target-triangles n :max-error d [:preserve-boundaries #t\|#f]])` | freecad, legacy-build123d, mesh/native | Imports an STL file as geometry. Optional preparation keywords keep the raw source and derive a bounded indexed mesh. | `(import-stl "/tmp/part.stl" :target-triangles 4000 :max-error 0.05 :preserve-boundaries #t)` |
| `intersection` | cadOp | `(intersection solid...)` | freecad, legacy-build123d, mesh/native | Keeps shared volume of solids. | `(intersection a b)` |
| `invlerp` | numericHelper | `(invlerp start end value)` | freecad, legacy-build123d, mesh/native | Maps a value from an interval to its unbounded interpolation factor. | `(invlerp 0 100 height)` |
| `jitter2` | pointListHelper | `(jitter2 x y amount seed)` | freecad, legacy-build123d, mesh/native | Returns a deterministic jittered 2D point from a base coordinate. | `(jitter2 10 20 2 seed)` |
| `jittered-grid` | pointListHelper | `(jittered-grid rows cols dx dy amount seed)` | freecad, legacy-build123d, mesh/native | Builds a deterministic grid of jittered 2D points. | `(jittered-grid 4 6 12 12 2 seed)` |
| `lambda` | expressionForm | `(lambda (arg...) body)` | freecad, legacy-build123d, mesh/native | Creates an anonymous function for map/filter/fold helpers. | `(lambda (i) (translate (* i pitch) 0 0 cutter))` |
| `lerp` | numericHelper | `(lerp a b t)` | freecad, legacy-build123d, mesh/native | Linear interpolation from a to b by t. | `(lerp 10 20 0.25)` |
| `let` | modelWrapper | `(let ((name value)...) clause...)` | freecad, legacy-build123d, mesh/native | Binds model-level constants for following clauses; bindings in one let are parallel. | `(model (let ((r 20)) (part body (sphere r))))` |
| `let*` | modelWrapper | `(let* ((name value)...) clause...)` | freecad, legacy-build123d, mesh/native | Sequential model-level binding form; later bindings can use earlier bindings. | `(model (let* ((r 20) (h (* r 3))) (part body (cylinder r h))))` |
| `linear-array` | cadOp | `(linear-array count dx dy dz geometry)` | freecad, legacy-build123d, mesh/native | Repeats geometry in a linear sequence. | `(linear-array 4 12 0 0 rib)` |
| `linspace` | expressionForm | `(linspace start end count)` | freecad, legacy-build123d, mesh/native | Builds evenly spaced samples including endpoints. | `(linspace 0 360 12)` |
| `list` | expressionForm | `(list value...)` | freecad, legacy-build123d, mesh/native | Builds a list value. | `(list x y z)` |
| `list?` | booleanHelper | `(list? value)` | freecad, legacy-build123d, mesh/native | Boolean predicate or comparator for conditionals and filtering. | `(list? '())` |
| `location` | cadOp | `(location frame :offset '(x y z) :rotate '(x y z))` | freecad, legacy-build123d, mesh/native | Creates a placement from a frame and optional local transform. | `(location (plane :origin '(80 0 6)) :rotate '(0 90 0))` |
| `loft` | cadOp | `(loft sketch...)` | freecad, legacy-build123d, mesh/native | Creates a solid through multiple sketch sections. | `(loft bottom top)` |
| `logistic-bifurcation-points` | pointListHelper | `(logistic-bifurcation-points r-count samples transient scale)` | freecad, legacy-build123d, mesh/native | Builds deterministic points from the logistic map bifurcation diagram. | `(logistic-bifurcation-points 24 8 16 30)` |
| `lorenz-points` | pointListHelper | `(lorenz-points count dt scale)` | freecad, legacy-build123d, mesh/native | Samples a deterministic Lorenz attractor projection. | `(lorenz-points 80 0.01 4)` |
| `make-face` | cadOp | `(make-face sketch)` | freecad, legacy-build123d, mesh/native | Turns a closed sketch into a face-like profile for downstream ops. | `(make-face (polygon points))` |
| `map` | expressionForm | `(map fn list ...)` | freecad, legacy-build123d, mesh/native | Transforms each list item with a function. | `(map (lambda (i) (* i 10)) (range 4))` |
| `max` | numericHelper | `(max a b...)` | freecad, legacy-build123d, mesh/native | Returns largest number. | `(max wall 1.2)` |
| `mesh` | cadOp | `(mesh :vertices ((x y z) ...) :triangles ((a b c) ...))` | mesh/native | Creates bounded indexed triangle geometry. Open orientable surfaces are allowed; invalid indices, degenerate faces, duplicates, non-manifold edges, or inconsistent winding reject. | `(mesh :vertices ((0 0 0) (10 0 0) (0 10 0)) :triangles ((0 1 2)))` |
| `mesh-anchor` | cadOp | `(mesh-anchor triangle-index barycentric-0 barycentric-1 barycentric-2)` | mesh/native | Declares one exact triangle seed used inside a native mesh \`surface-trim\` path. | `(mesh-anchor 42 0.2 0.3 0.5)` |
| `meta` | modelClause | `(meta key value)` | freecad, legacy-build123d, mesh/native | Stores literal model metadata in Core IR; \`:title\` labels the exported document and \`units strict\` enables dimensional checks. | `(meta :title "Bottle cage")` |
| `min` | numericHelper | `(min a b...)` | freecad, legacy-build123d, mesh/native | Returns smallest number. | `(min wall max-wall)` |
| `mirror` | cadOp | `(mirror axis offset geometry)` | freecad, legacy-build123d, mesh/native | Mirrors geometry across the \`x\`, \`y\`, or \`z\` plane at offset. | `(mirror "x" 0 body)` |
| `neovius` | wallPatternMode | `neovius` | mesh/native | Triply periodic Neovius implicit field. | `(wall-pattern (:mode neovius :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)` |
| `noise2` | numericHelper | `(noise2 x y seed)` | freecad, legacy-build123d, mesh/native | smooth deterministic value noise sampled at 2D coordinates. | `(noise2 (* x 0.1) (* y 0.1) seed)` |
| `not` | booleanHelper | `(not value)` | freecad, legacy-build123d, mesh/native | Boolean predicate or comparator for conditionals and filtering. | `(not false)` |
| `null?` | booleanHelper | `(null? value)` | freecad, legacy-build123d, mesh/native | Boolean predicate or comparator for conditionals and filtering. | `(null? '())` |
| `odd?` | booleanHelper | `(odd? number)` | freecad, legacy-build123d, mesh/native | Boolean predicate or comparator for conditionals and filtering. | `(odd? 2)` |
| `offset` | cadOp | `(offset distance sketch)` | freecad, legacy-build123d, mesh/native | Offsets a sketch/profile by distance. | `(offset 2 profile)` |
| `offset-rounded` | cadOp | `(offset-rounded distance sketch)` | freecad, legacy-build123d, mesh/native | Offsets a sketch with rounded joins where supported. | `(offset-rounded 2 profile)` |
| `or` | booleanHelper | `(or value...)` | freecad, legacy-build123d, mesh/native | Boolean predicate or comparator for conditionals and filtering. | `(or true false)` |
| `organic-loop` | pointListHelper | `(organic-loop count radius amount seed)` | freecad, legacy-build123d, mesh/native | Builds a deterministic irregular loop around a radius. | `(organic-loop 32 30 4 seed)` |
| `params` | modelClause | `(params control...)` | freecad, legacy-build123d, mesh/native | Declares user-visible controls and default parameter values for the model. | `(params (number radius 20 :label "Radius" :min 5 :max 80))` |
| `part` | modelClause | `(part id geometry)` | freecad, legacy-build123d, mesh/native | Declares a named renderable part from a solid, sketch, path, or compound expression. | `(part body (cylinder radius height 48))` |
| `path` | cadOp | `(path segment...)` | freecad, legacy-build123d, mesh/native | Builds a path from path segments. | `(path (polyline points))` |
| `path-frame` | cadOp | `(path-frame path :at start\|end\|t :up '(x y z))` | freecad, legacy-build123d, mesh/native | Computes a local frame along a path parameter. | `(path-frame rail :at end :up '(0 0 1))` |
| `pi` | numericConstant | `pi` | freecad, legacy-build123d, mesh/native | Built-in circle constant. | `(* radius pi)` |
| `place` | cadOp | `(place frame geometry :offset '(x y z) :rotate '(x y z))` | freecad, legacy-build123d, mesh/native | Places geometry in a local coordinate frame. | `(place end-frame (cylinder 4 18) :offset '(0 0 -9))` |
| `place-component` | componentPlacementForm | `(place-component (component :param value ...) :from port-id :to (port-ref part-id port-id) :normal aligned\|opposed [:roll degrees] [:offset '(x y z)] [:mirror none\|x\|y])` | freecad, legacy-build123d, mesh/native | Mates source and target ports without Euler math. | `(place-component (latch) :from mount :to (port-ref enclosure side-left-latch) :normal opposed)` |
| `plane` | cadOp | `(plane :origin '(x y z) :x '(x y z) :normal '(x y z))` | freecad, legacy-build123d, mesh/native | Creates a local coordinate plane. | `(plane :origin '(80 0 6) :normal '(0 0 1))` |
| `polar-points` | pointListHelper | `(polar-points count radius)` | freecad, legacy-build123d, mesh/native | Builds evenly spaced points around a circle. | `(polar-points 32 20)` |
| `polygon` | cadOp | `(polygon ((x y)...))` | freecad, legacy-build123d, mesh/native | Creates a closed polygon sketch from 2D points. | `(polygon ((0 0) (40 0) (40 20) (0 20)))` |
| `polyhedron` | cadOp | `(polyhedron :vertices ((x y z) ...) :triangles ((a b c) ...))` | mesh/native | Creates one closed orientable indexed triangle solid after deterministic topology validation. | `(polyhedron :vertices ((0 0 0) (10 0 0) (0 10 0) (0 0 10)) :triangles ((0 2 1) (0 1 3) (1 2 3) (2 0 3)))` |
| `polyline` | cadOp | `(polyline points)` | freecad, legacy-build123d, mesh/native | Builds a connected line path from points. | `(polyline ((0 0) (10 0) (10 5)))` |
| `port` | componentPlacementForm | `(port id :type type-id :frame frame [:compatible-with '(type-id ...)] [:params ((name value) ...)])` | freecad, legacy-build123d, mesh/native | Declares one stable typed local interface. | `(port mount :type "mount.v1" :frame local-frame)` |
| `port-ref` | componentPlacementForm | `(port-ref part-id port-id)` | freecad, legacy-build123d, mesh/native | References one target port. | `(port-ref enclosure side-left-latch)` |
| `ports` | componentPlacementForm | `(ports (port ...) ...)` | freecad, legacy-build123d, mesh/native | Groups local interfaces. | `(ports (port mount :type "mount.v1" :frame local-frame))` |
| `profile` | cadOp | `(profile :outer sketch :holes sketch-or-list)` | freecad, legacy-build123d, mesh/native | Builds a face profile from an outer loop and optional hole loops. | `(profile :outer (circle 20) :holes (circle 6))` |
| `protrude` | cadOp | `(protrude image-path height [:width w] [:depth d] [:fit contain\|stretch] [:foreground dark\|light])` | mesh/native | Raises continuous raster foreground coverage above local Z=0. One physical dimension preserves source aspect ratio; two contain and center by default. \`:fit stretch\` explicitly fills a non-matching box. Transparent pixels remain empty; an internal closure epsilon stays below the authored base plane. | `(protrude image-path 4 :width 100 :depth 70 :fit contain :foreground dark)` |
| `quote` | expressionForm | `(quote value) or 'value` | freecad, legacy-build123d, mesh/native | Prevents evaluation of symbols/lists for literal data such as align tuples. | `'(center center min)` |
| `rad` | numericHelper | `(rad degrees)` | freecad, legacy-build123d, mesh/native | Converts degrees to radians. | `(rad 90)` |
| `rad->deg` | numericHelper | `(rad->deg radians)` | freecad, legacy-build123d, mesh/native | Converts radians to degrees. | `(rad->deg pi-angle)` |
| `radial-array` | cadOp | `(radial-array count radius geometry)` | freecad, legacy-build123d, mesh/native | Repeats geometry around a circle. | `(radial-array 12 30 spoke)` |
| `range` | expressionForm | `(range count)` | freecad, legacy-build123d, mesh/native | Builds integer indices from 0 to count - 1. | `(range 8)` |
| `rectangle` | cadOp | `(rectangle width height)` | freecad, legacy-build123d, mesh/native | Creates a rectangular sketch/profile. | `(rectangle 40 20)` |
| `reduce` | expressionForm | `(reduce fn initial list)` | freecad, legacy-build123d, mesh/native | Reduces a list into a single accumulated value. | `(fold + 0 (range 5))` |
| `regular-polygon` | cadOp | `(regular-polygon sides radius :rotation deg)` | freecad, legacy-build123d, mesh/native | Creates a regular n-gon 2D profile by side count and circumradius. | `(regular-polygon 6 10)` |
| `remap` | numericHelper | `(remap value in-start in-end out-start out-end)` | freecad, legacy-build123d, mesh/native | Linearly maps a value between two intervals. | `(remap height 0 100 1 3)` |
| `repeat` | cadOp | `(repeat count fn-or-geometry)` | freecad, legacy-build123d, mesh/native | Repeat helper for patterned geometry generation. | `(repeat 6 rib)` |
| `repeat-compound` | cadOp | `(repeat-compound count fn-or-geometry)` | freecad, legacy-build123d, mesh/native | Repeat helper for patterned geometry generation. | `(repeat-compound 6 rib)` |
| `repeat-pick` | cadOp | `(repeat-pick count fn-or-geometry)` | freecad, legacy-build123d, mesh/native | Repeat helper for patterned geometry generation. | `(repeat-pick 6 rib)` |
| `repeat-union` | cadOp | `(repeat-union count fn-or-geometry)` | freecad, legacy-build123d, mesh/native | Repeat helper for patterned geometry generation. | `(repeat-union 6 rib)` |
| `result` | cadOp | `(result geometry)` | freecad, legacy-build123d, mesh/native | Selects final geometry from a build context. | `(result body)` |
| `reverse` | expressionForm | `(reverse list)` | freecad, legacy-build123d, mesh/native | Returns list items in reverse order. | `(reverse points)` |
| `revolve` | cadOp | `(revolve sketch angle)` | freecad, legacy-build123d, mesh/native | Revolves a sketch profile around an axis. | `(revolve profile 360)` |
| `rib` | cadOp | `(rib solid profile path)` | freecad, legacy-build123d, mesh/native | Adds material: sweeps \`profile\` along \`path\` and unions it onto \`solid\`. | `(rib (box 20 20 20) (circle 3) (path (0 0 0) (0 0 30)))` |
| `ribs` | wallPatternMode | `ribs` | mesh/native | Straight rib pattern along the shell parameter direction. | `(wall-pattern (:mode ribs :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)` |
| `ring` | cadOp | `(ring outer-radius inner-radius segments)` | freecad, legacy-build123d, mesh/native | Creates an annular sketch aliasing to a profile with one outer and one hole circle. | `(ring 20 10 64)` |
| `rings` | wallPatternMode | `rings` | mesh/native | Ring bands around the shell parameter direction. | `(wall-pattern (:mode rings :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)` |
| `rossler-points` | pointListHelper | `(rossler-points count dt scale)` | freecad, legacy-build123d, mesh/native | Samples a deterministic Rossler attractor projection. | `(rossler-points 80 0.03 6)` |
| `rotate` | cadOp | `(rotate x-deg y-deg z-deg geometry)` | freecad, legacy-build123d, mesh/native | Rotates geometry in degrees around local axes. | `(rotate 0 0 45 body)` |
| `rounded-polygon` | cadOp | `(rounded-polygon points radius)` | freecad, legacy-build123d, mesh/native | Creates a polygon profile with rounded corners. | `(rounded-polygon points 2)` |
| `rounded-rect` | cadOp | `(rounded-rect width height radius)` | freecad, legacy-build123d, mesh/native | Creates a rectangle profile with rounded corners. | `(rounded-rect 40 20 3)` |
| `sampled-radial-loft` | cadOp | `(sampled-radial-loft (theta z fz) :height h :z-steps n :theta-steps n :radius expr :z-map expr?)` | freecad, legacy-build123d, mesh/native | Samples radial sections across height, then lofts the wires/faces into a solid. | `(sampled-radial-loft (theta z fz) :height 40 :z-steps 24 :theta-steps 72 :radius (+ 18 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793))))))` |
| `scale` | cadOp | `(scale x y z geometry)` | freecad, legacy-build123d, mesh/native | Scales geometry by XYZ factors. | `(scale 1 1 0.5 body)` |
| `schwarz-d` | wallPatternMode | `schwarz-d` | mesh/native | Triply periodic Schwarz D implicit field. | `(wall-pattern (:mode schwarz-d :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)` |
| `schwarz-p` | wallPatternMode | `schwarz-p` | mesh/native | Triply periodic Schwarz P implicit field. | `(wall-pattern (:mode schwarz-p :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)` |
| `shape` | cadOp | `(shape geometry)` | freecad, legacy-build123d, mesh/native | Marks or wraps a geometry expression in build contexts. | `(shape body)` |
| `shell` | cadOp | `(shell thickness [:faces selector] solid)` | freecad, legacy-build123d, mesh/native | Hollows or thickens a solid by wall thickness. Exact backends also accept \`:faces\` with \`target-id:<id>\` or \`target-ids:<id>\|<id>\` to choose shell opening faces. | `(shell 2 :faces "target-id:body:face:0-0-20:1256.637" (cylinder 20 80))` |
| `sin` | numericHelper | `(sin radians)` | freecad, legacy-build123d, mesh/native | Trigonometric helper using radians. | `(sin (deg->rad 45))` |
| `slot-arc` | cadOp | `(slot-arc radius start end width)` | freecad, legacy-build123d, mesh/native | Curved (annular) obround: a circular-arc centerline of given radius from \`start\` to \`end\` degrees, thickened by width. | `(slot-arc 20 0 90 10)` |
| `slot-center-point` | cadOp | `(slot-center-point cx cy px py width)` | freecad, legacy-build123d, mesh/native | Obround 2D profile from a center point to an end point, with width. | `(slot-center-point 0 0 20 0 10)` |
| `slot-center-to-center` | cadOp | `(slot-center-to-center separation width)` | freecad, legacy-build123d, mesh/native | Obround 2D profile specified by the distance between the two end-arc centers. | `(slot-center-to-center 30 10)` |
| `slot-overall` | cadOp | `(slot-overall length width)` | freecad, legacy-build123d, mesh/native | Creates an obround (stadium) 2D profile of given overall length and width. | `(slot-overall 40 10)` |
| `smoothstep` | numericHelper | `(smoothstep edge0 edge1 x)` | freecad, legacy-build123d, mesh/native | Smooth Hermite interpolation useful for soft transitions. | `(smoothstep 0 1 t)` |
| `sphere` | cadOp | `(sphere radius)` | freecad, legacy-build123d, mesh/native | Creates a sphere. | `(sphere 12)` |
| `spiral` | wallPatternMode | `spiral` | mesh/native | Spiral rib pattern across shell parameters. | `(wall-pattern (:mode spiral :depth 0.6 :uFreq 5 :vFreq 5 :seed 7) target)` |
| `square` | numericHelper | `(square value)` | freecad, legacy-build123d, mesh/native | Raises a number to a small fixed power. | `(square radius)` |
| `start` | coreConstant | `start` | freecad, legacy-build123d, mesh/native | Symbolic endpoint accepted by path-frame \`:at\`. | `(path-frame rail :at start :up '(0 0 1))` |
| `superellipse-point` | pointListHelper | `(superellipse-point angle rx ry exponent)` | freecad, legacy-build123d, mesh/native | Samples one point from a superellipse. | `(superellipse-point (deg->rad 45) 30 20 4)` |
| `surface-trim` | cadOp | `(surface-trim ...)` | mesh/native | Supported \`.ecky\` surface entry. Read backend guide and validation errors for exact constraints. | `(surface-trim ...)` |
| `svg` | cadOp | `(svg path-or-data)` | freecad, legacy-build123d, mesh/native | Imports SVG profile/path data where backend lowering supports it. | `(svg iconData)` |
| `sweep` | cadOp | `(sweep profile path)` | freecad, legacy-build123d, mesh/native | Sweeps a profile along a path. | `(sweep (circle 2 16) rail)` |
| `tag-edge` | modelClause | `(tag-edge id :edge or :edges selector target)` | freecad, legacy-build123d, mesh/native | Names a stable edge selection for downstream operations and analysis. | `(tag-edge rim :edges "top" body)` |
| `tag-edges` | modelClause | `(tag-edges id :edge or :edges selector target)` | freecad, legacy-build123d, mesh/native | Names a stable edge selection for downstream operations and analysis. | `(tag-edges rim :edges "top" body)` |
| `tag-face` | modelClause | `(tag-face id :face or :faces selector target)` | freecad, legacy-build123d, mesh/native | Names a stable face selection for downstream operations and analysis. | `(tag-face mounting :faces "bottom" body)` |
| `tag-vertex` | modelClause | `(tag-vertex id :vertex selector target)` | freecad, legacy-build123d, mesh/native | Names a stable vertex selection for downstream operations and analysis. | `(tag-vertex datum :vertex "top" body)` |
| `tan` | numericHelper | `(tan radians)` | freecad, legacy-build123d, mesh/native | Trigonometric helper using radians. | `(tan (deg->rad 45))` |
| `taper` | cadOp | `(taper height scale sketch) or (taper height scale-x scale-y sketch)` | freecad, legacy-build123d, mesh/native | Extrudes a sketch while scaling the top section. | `(taper 30 0.7 0.7 (circle 12 32))` |
| `tapped-hole` | cadOp | `(tapped-hole :iso "M8" :length len [:radius r] [:pitch p] [:depth d] [:base-width w] [:crest-width w] [:lefthand #t])` | freecad, legacy-build123d, mesh/native | A tapped (internal female) thread cut as a positive cavity: a named-radius bore cylinder at the ISO minor diameter unioned with a helical relief ridge whose crest reaches the major diameter. \`:iso "M8"\` decodes a metric designation; an equal-nominal \`thread\` mates with it. | `(tapped-hole :iso "M8" :length 14)` |
| `tau` | numericConstant | `tau` | freecad, legacy-build123d, mesh/native | Built-in circle constant. | `(* radius tau)` |
| `text` | cadOp | `(text value size [:font selector])` | freecad, legacy-build123d, mesh/native | Creates a text profile. \`:font\` selects the face for this call before downstream extrusion. | `(extrude (text "A" 12 :font "Arial") 2)` |
| `thread` | cadOp | `(thread :radius r :pitch p :length len :depth d [:base-width w] [:crest-width w] [:female #t] [:clearance c] [:lefthand #t] [:iso "M4"])` | freecad, legacy-build123d, mesh/native | Parametric helical thread: a core cylinder plus a \`helical-ridge\` (male), or a ridge cutter (\`:female\`). \`:iso "M4"\` decodes a metric designation into pitch/radius. | `(thread :radius 8 :pitch 2 :length 16 :depth 1)` |
| `torus` | cadOp | `(torus major minor)` | freecad, legacy-build123d, mesh/native | Creates a ring torus: tube of radius \`minor\` swept at distance \`major\` from the Z axis. | `(torus 20 5)` |
| `translate` | cadOp | `(translate x y z geometry)` | freecad, legacy-build123d, mesh/native | Moves geometry by XYZ offset. | `(translate 10 0 0 body)` |
| `trapezoid` | cadOp | `(trapezoid bottom top height :skew s)` | freecad, legacy-build123d, mesh/native | Creates a trapezoid 2D profile (parallel bottom/top widths, given height, optional skew). | `(trapezoid 20 10 8 :skew 3)` |
| `true` | coreConstant | `true` | freecad, legacy-build123d, mesh/native | Boolean constant equivalent to \`#t\` or \`#f\`. | `(if true body fallback)` |
| `twist` | cadOp | `(twist height angle sketch)` | freecad, legacy-build123d, mesh/native | Extrudes a sketch while rotating sections along height. | `(twist 40 90 profile)` |
| `union` | cadOp | `(union solid...)` | freecad, legacy-build123d, mesh/native | Boolean union/fuse of solids. | `(union a b c)` |
| `vec2` | pointListHelper | `(vec2 x y)` | freecad, legacy-build123d, mesh/native | Constructs a two-coordinate point list. | `(vec2 10 20)` |
| `vec3` | pointListHelper | `(vec3 x y z)` | freecad, legacy-build123d, mesh/native | Constructs a three-coordinate point list. | `(vec3 10 20 30)` |
| `verify` | modelClause | `(verify (tag id) [(intent text)] [(severity error\|warning)] [(when bool-expr)] (metric id metric-expr) (expect id predicate))` | freecad, legacy-build123d, mesh/native | Declares one conditional runtime check with intent, severity, and typed evidence. | `(verify (tag mesh-clean) (metric bad-edges (stl non-manifold-edge-count)) (expect bad-edges (= 0)))` |
| `view` | modelClause | `(view id (offset-part part dx dy dz)...) ` | freecad, legacy-build123d, mesh/native | Declares a preview-only exploded or print-layout view without changing export geometry. | `(view print-layout (offset-part lid 90 0 0) (offset-part body 0 0 0))` |
| `voronoi-cell` | cadOp | `(voronoi-cell sites index width height inset)` | mesh/native | Creates one exact bounded Voronoi polygon, uniformly inset and expressed relative to its selected site. | `(voronoi-cell (voronoi-cells 3 3 12 12 1.5 23) 4 40 40 1.2)` |
| `voronoi-cells` | pointListHelper | `(voronoi-cells rows cols dx dy amount seed)` | freecad, legacy-build123d, mesh/native | Builds jittered grid points suitable as Voronoi-ish perforation centers. | `(voronoi-cells 4 6 14 12 2 seed)` |
| `voronoi2` | numericHelper | `(voronoi2 x y seed)` | freecad, legacy-build123d, mesh/native | Deterministic cellular field: high near cell centers, lower near cell borders. | `(voronoi2 (* x 0.15) (* y 0.15) seed)` |
| `wall-pattern` | cadOp | `(wall-pattern (:mode mode :depth n :uFreq n :vFreq n :seed n) shell-target)` | mesh/native | Applies mesh/eckyRust procedural displacement/perforation-style wall patterns to supported shell surface targets. | `(wall-pattern (:mode gyroid :depth 0.6 :uFreq 4 :vFreq 5) (shell 2 (cylinder 20 80)))` |
| `wave-loop` | pointListHelper | `(wave-loop count radius amplitude frequency phase)` | freecad, legacy-build123d, mesh/native | Builds a circular wave profile. | `(wave-loop 48 20 3 5 0)` |
| `wedge` | cadOp | `(wedge dx dy dz xmin zmin xmax zmax :align '(x y z))` | freecad, legacy-build123d, mesh/native | Creates a wedge/ramp solid: a dx×dy×dz box whose top face is shrunk to the xmin..xmax / zmin..zmax window. | `(wedge 20 10 20 5 5 15 15)` |
| `xor` | cadOp | `(xor solid...)` | freecad, legacy-build123d, mesh/native | Boolean exclusive-or for solids where supported. | `(xor a b)` |
| `xy` | coreConstant | `xy` | freecad, legacy-build123d, mesh/native | Symbolic principal plane value. | `(list xy)` |
| `xz` | coreConstant | `xz` | freecad, legacy-build123d, mesh/native | Symbolic principal plane value. | `(list xz)` |
| `yz` | coreConstant | `yz` | freecad, legacy-build123d, mesh/native | Symbolic principal plane value. | `(list yz)` |
| `zero?` | booleanHelper | `(zero? number)` | freecad, legacy-build123d, mesh/native | Boolean predicate or comparator for conditionals and filtering. | `(zero? 2)` |
| `zip` | expressionForm | `(zip list-a list-b)` | freecad, legacy-build123d, mesh/native | Pairs items from two lists by index. | `(map (lambda ((x y)) (list x y)) (zip xs ys))` |
<!-- ECKY_GENERATED_SURFACE_REFERENCE_END -->
