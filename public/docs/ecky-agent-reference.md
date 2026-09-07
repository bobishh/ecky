# Ecky language reference

Current fileExtension: `.ecky`.
Current sourceLanguage: `ecky`.

Return one complete `(model ...)` program. Use millimetres for length and degrees for angles. Suffixed literals convert into those base units; they do not provide dimensional type checking.

## Program structure

- Put reusable pure `(define ...)` helpers and `define-component` declarations before `(model ...)`.
- Direct `model` clauses are `params`, `verify`, `part`, `feature`, `meta`,
  `tag-vertex`, `tag-face`, `tag-edge`, `tag-edges`, `view`, and `analysis`.
- `meta` stores literal model metadata; it creates no geometry. Use
  `(meta :title "Name")` for the exported document label and
  `(meta units strict)` for dimensional checking. Values must be string,
  symbol, number, or boolean literals; duplicate keys reject.
- `component_get` is vendor mode: paste its closed `define-component` source;
  it creates no package dependency.
- `(import-component "package.id" :version "1.2.0" :component "component-id"
  :as alias)` is live mode. Use literal exact coordinates and the committed
  exact dependency lock; never use ranges, `latest`, or implicit upgrades.
- Never put `define` inside `model`. Use `let*` inside a part when later values depend on earlier values or parameters.
- Give every part a stable key. Use `build`, named `shape` stages, and one `result` when a part needs intermediate geometry.
- Keep `ui_spec`, `initial_params`, and source parameter keys aligned. Use `number`, `select`, `toggle`, or `image`; never invent parameter forms.

```scheme
(model
  (params
    (number width 60 :label "Width" :min 20 :max 120 :step 1)
    (number thickness 4 :label "Thickness" :min 1 :max 12 :step 0.5))
  (part plate
    (let* ((hole-r 3))
      (build
        (shape blank (extrude (rounded-rect width 36 4) thickness))
        (shape bore (translate 0 0 -0.5 (cylinder hole-r (+ thickness 1))))
        (result (difference blank bore))))))
```

## Parameters and derived values

- Parameter declarations accept `:label`; numbers also accept `:min`, `:max`,
  `:step`, and `:unit`; selects accept `:options`; every control accepts
  `:frozen`. Suffixed numeric defaults infer their unit.
- Put cross-parameter bounds in `:relations`, for example
  `(params ... :relations ((< inner-radius outer-radius)))`. Supported relation
  operators are `<`, `<=`, `>`, and `>=`.
- Defaults belong to source. Current target parameter values may override them at
  render time. Changing a default does not prove the active value changed.
- One parameter key means one physical concept. Remove unused controls and old
  version suffixes; do not retain dead compatibility parameters.
- Use model-level `let*` for shared derived dimensions, part-local `let*` for
  part-only math, and a top-level pure `define` for reusable functions. Never
  repeat fit math across parts.

## Components

`define-component` is a closed reusable source declaration. Its signature owns
defaults and labels. Instantiate it with keyword arguments. Model parameters are
not implicit globals: pass them explicitly. A component may contain optional
`verify` clauses before its single geometry body; their tags are namespaced per
instantiating part.

```scheme
(define-component vent
  ((number radius 4mm :label "Vent radius")
   (number depth 3mm :label "Cut depth"))
  (cylinder radius depth 32))

(model
  (part cutter (vent :radius 5mm :depth 4mm)))
```

Mounted components stay local. Named source/target ports move unchanged geometry.

```scheme
(define-component dryer-latch ((number clearance 0.3))
  (ports (port mount :type "dryer.latch.mount.v1"
    :params ((clearance clearance)) :frame
    (frame :origin '(0 0 0) :x-axis '(1 0 0) :z-axis '(0 0 1))))
  (box 20 4 2))
(model
  (part enclosure
    (ports
      (port front :type "dryer.latch.mount.v1" :frame
        (frame :origin '(0 -25 15) :x-axis '(1 0 0) :z-axis '(0 -1 0)))
      (port side-left :type "dryer.latch.mount.v1" :frame
        (frame :origin '(50 0 15) :x-axis '(0 1 0) :z-axis '(1 0 0)))
      (port side-right :type "dryer.latch.mount.v1" :frame
        (frame :origin '(-50 0 15) :x-axis '(0 -1 0) :z-axis '(-1 0 0))))
    (box 100 50 30))
  (part front-latch (place-component (dryer-latch) :from mount
    :to (port-ref enclosure front) :normal opposed))
  (part side-latch (place-component (dryer-latch) :from mount
    :to (port-ref enclosure side-left) :normal opposed))
  (part mirrored-side-latch (place-component (dryer-latch) :from mount
    :to (port-ref enclosure side-right) :normal opposed :mirror x)))
```

`frame` derives `yAxis = zAxis × xAxis`; axes must be finite and orthogonal.
Normal mode is mandatory. Optional `:roll`, target-local `:offset`, and local
`:mirror x|y|none` compose before placement. Name fit offsets in port `:params`.
Inspect `shapeGraph.instances[].placement` for solved frames and mate inputs;
never infer component orientation from preview triangles.

Use `component_get` output as copied source: paste its complete
`define-component`; no dependency remains. Use
`(import-component "package.id" :version "1.2.0" :component "component-id" :as alias)`
for a live package dependency. Versions must be exact and lock-backed; never use
ranges or `latest`.

## Semantic features, topology tags, and preview views

- `feature` renders like `part` but adds a semantic `:role` and optional primary
  `:params`. Compiler-inferred dependencies remain authoritative.
- Top-level `tag-face`/`tag-edges` declarations name interaction-critical
  topology for later operations and analysis. Prefer semantic selectors, stable
  target ids, or `:created-by` scoping over raw indices.
- `view` changes preview placement only. Each
  `(offset-part part-id dx dy dz)` references a stable part id; export geometry
  remains unchanged.

## Geometry rules

- Start with primitives or closed 2D profiles. Use `extrude`, `revolve`, `sweep`, or `loft` to create solids.
- `(hole :type solid|sketch|path|shape :goal "...")` is a typed planning
  placeholder. It deliberately fails lowering; never ship it as finished geometry.
- Boolean cutters must cross the target completely. Overshoot instead of leaving coincident faces.
- Author repeated shelves, ribs, holes, doors, or clips with `repeat`, array forms, or component instances. Do not copy shape blocks.
- Name every fit-critical dimension or relation: wall thickness, clearance, bore radius, pitch, seat height, and mating axis. Do not hide physical fit in anonymous offsets.
- Prefer selectors based on physical meaning or stable tags. Boolean operations rebuild topology, so raw face or edge indices are not stable design intent.
- Backend support is authoritative. If a diagnostic rejects an operation on the active backend, change the operation or backend; do not retry unchanged source.
- STEP-backed live components require locked analytic provenance and native
  Direct OCCT import. Never route them through FreeCAD, STL, `solidify`, hidden
  repair, or implicit fusion.

## Verification

Write top-level `verify` clauses from measurable requirements. Keep them during repair.

```scheme
(model
  (verify
    (tag mesh-clean)
    (metric bad-edges (stl non-manifold-edge-count))
    (expect bad-edges (= 0)))
  (verify
    (tag preview-exists)
    (metric preview (manifest has-model-stl))
    (expect preview (= true)))
  (verify
    (tag body-grounded)
    (intent "Printed body needs broad bed contact")
    (severity warning)
    (when (not assembly-preview))
    (metric contact (stl bed-contact-area-ratio body))
    (expect contact (>= 0.75)))
  (part body (box 30 20 10)))
```

Use `manifest` metrics for artifact and part claims, `stl` metrics for mesh structure, `clearance` for physical gaps, `selector` for measured placement, and `relation` for comparisons between named targets. `error` is default and blocks. `warning` failures remain amber/non-blocking. False `when` conditions return explicit skipped evidence. A failing clause means repair geometry or parameters; never weaken the requirement to manufacture green output.

Use `bed-contact-area-ratio`, `bed-contact-x-span-ratio`, and
`bed-contact-y-span-ratio` for print-bed grounding. Optional part id scopes the
metric to one part STL; omit it for the combined model STL.

For a separated print layout, set the model's `assembly-preview` control false.
Disconnected parts are then expected layout evidence. Non-manifold edges, invalid
solids, and authored verification failures remain blocking errors in every mode.

## Engineering analysis

`analysis` stores an authored engineering contract. Supported nested clauses are
`linear-static`, `question`, `acceptance-criterion`, `idealization`, `evidence`,
`input-evidence`, `assumption`, `validation-evidence`, `material`, `volume-mesh`,
`refine`, `passive-solid`, `passive-void`, `fixed`, `prescribed-displacement`,
`surface-force`, `traction`, `pressure`, and `solve`. Loads and constraints should
reference topology tags. Analysis results are evidence, not automatic acceptance
of geometry or printability.

## Mesh and image geometry

- Use `mesh` and `polyhedron` with bounded vertex and triangle lists. Prefer formula-generated lists over copied face blocks.
- `mesh` may be open. `polyhedron` must be one closed, orientable, nonzero-volume component. Check boundary edges, non-manifold edges, connected components, and volume before solid or printability claims.
- Use raster `extrude` for thresholded/traced artwork; use `protrude` for continuous luminance relief. One physical dimension preserves source aspect ratio. Two dimensions contain and center by default; use `:fit stretch` only for intentional non-uniform scaling. Alpha is coverage and transparent pixels remain empty. Add any physical backing as an explicit solid. Missing image selection is pending input, not substitute geometry.
- Front, Top, and Side raster references require physical calibration, contour review, editable sketch primitives, and normal exact-candidate validation. Raster extraction alone is not accepted CAD.
- A single perspective image provides inferred geometry only. Never call it an exact reconstruction, measured model, or accepted CAD without independent dimensions and validation.
- Pure mesh output supports mesh exports such as STL. STEP produced after solidifying a closed mesh is faceted poly-BRep, not analytic source CAD. Read artifact provenance before making export or exactness claims.

## Operating contract

Output source and required response fields only. Do not claim compilation, rendering, verification, STEP availability, or printability before runtime evidence exists. When the compiler returns a diagnostic, fix the named cause and emit a complete corrected program.
