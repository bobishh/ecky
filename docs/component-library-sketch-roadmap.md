# Sketch-First Modeling, Subagent Execution, And Component Packaging Roadmap

**Date:** 2026-04-28  
**Status:** Draft roadmap  
**Scope:** Sketch-first modeling, continuous agent execution, generated `.ecky`, validation-led CAD, and later reusable component packaging

## Summary

This roadmap defines Ecky's final modeling spine. Ecky should become a shared modeling workspace where users shape one object across sketch intent, draft form, and exact rebuild without losing state between those representations. Reusable CAD components, packages, assemblies, fusing, molding, and public sharing remain important, but the product center is now the live loop between sketch edits, draft-form edits, exact rebuild candidates, validation, and agent-guided local patches.

The core product decision:

- one workspace scene owns the live object state
- one object may carry `SketchIntent`, `MeshDraft`, and `ExactModel` at the same time
- users must be able to move back and forth between sketch and draft-form editing without destructive conversion
- exact rebuild should happen incrementally, per region or feature, when the shape is stable enough to commit
- accepted modeling steps should become structured source and validation evidence, not chat transcript
- every representation should expose useful projections, selection evidence, and stale/fresh status
- editable authoring sketches should exist only when source/history supports them
- derived projection sketches should explain models when source/history is missing
- thread authoring context is removed; source and backend ownership belong to model versions, generated artifacts, and session config
- agents may propose local patches, but solvers and validators own correctness
- geometry alone is not enough for later reuse
- reusable components need an explicit interface
- manual and agent-authored sketch changes should use the same `SketchDocument` -> generated `.ecky` path that exists today until a first-class `MeshDraftDocument` and shared-scene contract land

The target architecture is:

```text
user intent
  -> SketchIntent update
  -> MeshDraft seed/update
  -> local draft-form edits
  -> useful 2D projections update
  -> validation ledger update
  -> agent patch or rebuild request
  -> .ecky / Core IR exact rebuild candidate
  -> backend build
  -> sketch/draft/exact validation
  -> accepted exact model state
```

Later packaging and assembly use the accepted model state:

```text
accepted model
  -> component interface
  -> package header/payload
  -> assembly recipe
  -> optional fuse/mold operation
  -> BRep/STEP/3MF/STL export
```

## Product Goals

1. Ecky is a modeling workspace plus AI mentor where users shape one object through sketch intent, draft form, and exact rebuild.
2. Users can sketch first in orthographic views, push and pull a live draft form, return to sketch when intent changes, and only commit exact regions when they are stable.
3. Users can sketch stroke skeletons that become thickened, swept, or extruded draft bodies before exact rebuild.
4. The app renders feedback while users draw or edit draft form, then produces deterministic rebuild candidates without requiring an agent for every step.
5. Accepted sketch and draft steps generate structured source and remain editable as construction history.
6. Every rendered representation exposes useful 2D projections for inspection, learning, validation, and agent context.
7. Generated exact models are validated against sketch envelopes, draft-form intent, required regions, keepouts, and backend structural checks.
8. Agents can suggest local sketch patches, local draft-form patches, exact rebuild candidates, explanations, and repair steps inside the same validation loop.
9. Users can save accepted exact results as reusable components after the sketch/draft/exact loop works.
10. Components expose stable params, ports, clearances, keepouts, validation rules, and previews.
11. Components can later be packaged, shared, assembled, joined, fused, or molded.

## Final Interaction Loop

The final interaction loop is:

```text
seed shape in sketch
  -> touch draft form directly
  -> return to sketch when intent changes
  -> ask agent for a local patch or rebuild
  -> validate the result
  -> keep sketch, draft, and exact state alive together
  -> commit exact regions and features when they are ready
```

This loop should tolerate iteration:

- edit sketch, then draft
- edit draft, then sketch
- accept one agent patch and reject the next
- keep an exact rebuild candidate for one region while the rest stays draft
- preserve enough shared state that user and agent both see the same object without rediscovering it from scratch

## Core Concepts

### WorkspaceScene

`WorkspaceScene` is the shared object container for one modeling session. It should carry:

- `SketchIntent`
- `MeshDraft`
- `ExactModel`
- `CorrespondenceGraph`
- selection state
- validation state
- artifact/build state
- stale/fresh/rebuildable/failed/committed status for each representation

The user should move between lenses on the same object, not between disconnected documents.

### SketchIntent

`SketchIntent` is the user-authored 2D intent layer.

Purpose:

- orthographic structure
- profiles, axes, holes, keepouts, and dimensions
- semantic labels and constraints
- source-backed authoring history

Current implementation truth:

- `SketchDocument` is the live sketch structure today
- manual and deterministic sketch flows already feed generated `.ecky`

### MeshDraft

`MeshDraft` is the editable 3D draft-form layer.

Purpose:

- rapid form exploration
- region-local edits
- immediate volume feedback
- preview/subd-like shaping
- staging ground for later exact rebuild

Current implementation truth:

- preview mesh exists today
- first-class editable `MeshDraftDocument`, stable draft entity ids, and region-local editing remain next

### ExactModel

`ExactModel` is the exact rebuild and manufacturing layer.

Purpose:

- rebuildable exact feature graph
- BRep/STEP/FCStd outputs
- exact topology and validation targets
- commit/manufacture/export state

Current implementation truth:

- generated `.ecky` plus Core IR already drive exact backends today
- accepted exact validation/repair loop already exists

### CorrespondenceGraph

`CorrespondenceGraph` links representations.

Purpose:

- sketch primitive -> mesh region links
- mesh region -> exact feature/entity links
- validation target -> source intent links
- stale/fresh invalidation across lenses
- exact target evidence for user-visible repair and agent patches

This is the missing identity spine between current sketch, preview mesh, and exact validation work.

### AgentScenePacket

`AgentScenePacket` is the shared context contract for user+agent collaboration.

It should include:

- selected sketch primitives
- selected mesh regions
- selected exact entities
- active lens
- viewport/projection evidence
- local source fragments
- correspondence evidence
- stale/rebuildable status
- allowed patch target
- locked constraints

Agent output should be a local patch against one or more representations, not a blind whole-model rewrite.

### Modeling Playground And AI Mentor

Primary roles:

- Tutor: teaches the concept behind the current operation.
- Builder: proposes the next valid modeling step.
- Coach: suggests constraints, dimensions, feature order, and simplifications.
- Debugger: explains raw backend, solver, and validation failures.
- Math narrator: describes coordinates, geometry, transforms, and tolerances in context.

The mentor should stay grounded in visible model state:

- current sketch intent
- current draft form
- selected feature or region
- generated `.ecky` / exact source
- validation ledger
- rendered model
- 2D projections
- raw backend/provider errors

Mentor output should be concise Ecky bubble copy, not a separate status bar or terminal stream.

### Math Lens

Math Lens is the learning overlay that explains why a model works.

Concepts:

- coordinates and local frames
- orthographic projections
- points, lines, arcs, splines, profiles, planes, and solids
- functions and parameters
- transforms: translate, rotate, scale, mirror, pattern
- features: extrude, revolve, loft, shell, cut, hole, chamfer, fillet
- vectors, axes, normals, tangents, and face orientation
- calculus-lite shape language: slope, curvature, continuity, radius, and inflection
- tolerances, clearance, wall thickness, and manufacturing fit

Math Lens should connect explanation to geometry:

- highlight the coordinate or vector being discussed
- show projection evidence when explaining silhouettes
- tie parameters to dimensions and generated source
- describe tolerance failures with measured values and expected ranges
- narrate feature creation as profile plus operation plus direction

### Projection Principle

Every rendered model should have useful 2D projections.

Projection types:

- front, side, and top silhouettes
- section slices where useful
- face/profile outlines
- hole and port center marks
- bounding boxes and measured extents
- tolerance and clearance overlays

Authoring rule:

- Editable authoring sketches are shown when source/history contains real Sketch IR or feature profiles.
- Derived projection sketches are shown when a model only has solid/mesh geometry or hidden/compiled source.
- Derived projection sketches are inspectable and useful for validation, but not presented as original editable intent.

This avoids pretending that inferred outlines are true construction history while still making every model teachable, measurable, and comparable.

### Orthographic Reconstruction Algorithm Spine

The three sketch panes must not be treated as three decorative inputs to one extrusion. The old math is real, but it is not magic. Ecky should use it in layers:

1. Live preview should use silhouette/volume logic.
2. CAD acceptance should use BRep candidate reconstruction/search and projection validation.
3. LLMs should propose intent, constraints, and repairs, not own truth.

#### Track A: Visual Hull And Multi-View Extrusion Preview

Use this for immediate feedback while the user draws.

Algorithm:

1. Convert each closed orthographic sketch into a 2D region.
2. Extrude each region through the full model bounds along that view normal.
3. Search/select silhouette-consistent candidate cells over the shared orthographic coordinate grid.
4. Emit a rough `.ecky` cell-union preview mesh from the selected cells.
5. Reproject the preview into front/top/side and show mismatch overlays.

This is the shape-from-silhouette / visual hull family. Laurentini formalized the visual hull as the maximal object that gives the same silhouettes under the available views. Shum, Lau, Yuen, and Yu's 2-stage extrusion work uses a similar practical CAD idea: sweep exterior contour regions from the three views, intersect them into a basic solid, then use internal and dashed lines for feature refinement.

Implications for Ecky:

- Good for live "as you draw" feedback.
- Good for kids/noobs because it is visible and explainable.
- Deterministic and fast enough for mesh backend.
- Cannot infer hidden concavities or invisible features from silhouettes alone.
- Should be labeled `PREVIEW HULL`, not accepted BRep.

#### Track B: BRep Candidate Reconstruction

Use this for accepted CAD, source patches, and validation.

Classic BRep-oriented reconstruction follows the "fleshing out projections" family:

1. Parse front/top/side vector drawings into nodes, edges, visible lines, hidden lines, curves, and annotations.
2. Generate candidate 3D vertices by matching compatible projected coordinates across views.
3. Generate candidate 3D edges from candidate vertices whose projections are supported by 2D line/curve entities in the required views.
4. Generate candidate faces by finding valid loops on shared planes or curved surfaces.
5. Generate candidate cells/blocks from candidate faces.
6. Search/select cells that satisfy topology and projection constraints.
7. Reproject the chosen solid into the three views and compare against the source sketches.

Wesley and Markowsky's 1981 "Fleshing Out Projections" is the anchor paper here. Later work and surveys describe the same four-stage family as vertices -> edges -> faces -> blocks/solid. Liu, Hu, Chen, and Sun extend the approach to planar, quadric, and toroidal surfaces by classifying candidate conic edges from three orthographic views and using a conjugate diameter method before face/solid search.

Implications for Ecky:

- This is the right long-term CAD path.
- It needs vector Sketch IR, not pixels.
- It needs hidden/visible line state, construction lines, dimensions, and curve kinds.
- It can produce multiple valid solids from the same views.
- Search can grow exponentially, so user/agent intent labels and constraints are product features, not hacks.
- Validation must report raw projection/topology mismatches.

#### Track C: BRep To Sketch Projections

Use this so every model can return to useful 2D views.

Algorithm:

1. Given accepted BRep, run hidden-line removal for front/top/side.
2. Extract visible sharp edges, hidden sharp edges, smooth edges, sewn edges, outlines, and optional isoparameters.
3. Store derived projection sketches with provenance `derivedFromBRep`.
4. Let the user inspect, measure, dimension, and use them for validation.
5. Do not present them as original editable authoring history unless source contains matching Sketch IR.

Open CASCADE exposes this exact direction through `HLRBRep_Algo` for exact BRep hidden-line removal and `HLRBRep_PolyAlgo` for triangulated approximation. Ecky should use the backend already available through FreeCAD/build123d/OCCT instead of inventing hidden-line math in the frontend.

#### Track D: Learned Shape Programs Later

PlankAssembly shows a modern learned approach: convert three orthographic line drawings into shape-program sequences, with better robustness to noisy or missing lines than traditional exact pipelines in its cabinet domain.

Implications for Ecky:

- Useful later for proposal generation, examples, and repair suggestions.
- Not a core validator.
- Needs domain data if trained.
- Output should still become `.ecky` source and pass deterministic rebuild/projection checks.

#### Ecky Implementation Rule

The reconstruction stack should be:

```text
SketchDocument
  -> view regions / vector entities
  -> live preview hull mesh
  -> user/agent intent labels
  -> BRep candidate graph
  -> accepted .ecky feature source
  -> backend BRep build
  -> BRep hidden-line reprojection
  -> sketch/projection validation ledger
```

No UI should claim full 3-view reconstruction until the accepted BRep passes reprojection validation against all active views.

### Track E: Stroke-First CAD VM Authoring

Stroke-first authoring is a "draw the skeleton, get a solid" front door for the CAD VM. It is not a replacement for semantic CAD. It feeds the same `.ecky` source, Core IR, backend build, and validation loop.

Target stack:

```text
SketchDocument strokes
  -> typed path/profile entities
  -> width/cap/join constraints
  -> thicken / sweep / extrude source
  -> continuous-body draft
  -> BRep or mesh preview
  -> printability validation
  -> accepted .ecky feature source
  -> optional ports / datums / keepouts
```

Good fits:

- brackets, holders, rails, ribs, cable guides, lamp shades, and organic one-piece mounts
- "line plus thickness plus height" modeling
- fast printability-first ideation
- repair operations such as "make this connected", "increase minimum wall", or "blend junctions"

Bad fits:

- threads, gears, exact mating faces, and tight mechanical tolerance work
- assembly semantics that need stable ports and datum frames
- STEP-grade editable CAD where exact faces matter more than printable mass

Rules:

- Stroke bodies may create shape, but ports/faces/datums carry meaning.
- Generated junction blends and thickened paths must stay editable as source features.
- Continuous bodies must validate disconnected components, minimum wall, self-intersection, and overhang before being accepted.
- Stroke-derived ports must be explicit, accepted, and editable. No silent port inference.
- Mesh-only previews are acceptable while drawing. Accepted model state still needs rebuildable source and validation evidence.

Research references:

- Wesley and Markowsky, "Fleshing Out Projections", IBM Journal of Research and Development, 1981: https://scholarsmine.mst.edu/comsci_facwork/333/
- Kuo, "3-D objects from 2-D orthographic views - A survey", Computers & Graphics, 1988: https://doi.org/10.1016/0097-8493(88)90015-5
- Liu, Hu, Chen, and Sun, "Reconstruction of curved solids from engineering drawings", Computer-Aided Design, 2001: https://doi.org/10.1016/S0010-4485(00)00143-3
- Shum, Lau, Yuen, and Yu, "Solid reconstruction from orthographic views using 2-stage extrusion", Computer-Aided Design, 2001: https://doi.org/10.1016/S0010-4485(00)00079-8
- Laurentini, "The visual hull concept for silhouette-based image understanding", IEEE TPAMI, 1994: https://doi.org/10.1109/34.273735
- Open CASCADE Hidden Line Removal algorithms, `HLRBRep_Algo` and `HLRBRep_PolyAlgo`: https://documentation.help/Open-Cascade/occt_user_guides__modeling_algos.html
- Hu et al., "PlankAssembly: Robust 3D Reconstruction from Three Orthographic Views with Learnt Shape Programs", ICCV 2023: https://openaccess.thecvf.com/content/ICCV2023/papers/Hu_PlankAssembly_Robust_3D_Reconstruction_from_Three_Orthographic_Views_with_Learnt_ICCV_2023_paper.pdf

### Component

A component is a reusable model unit.

It contains:

- source or compiled geometry
- params
- named ports
- validation rules
- previews
- exports
- license and version metadata
- compatibility metadata

Examples:

- bicycle bottle cage
- dovetail frame rail
- lamp shade
- track segment
- socket insert

### Port

A port is a named interface on a component.

It is not only a point. It is:

- local coordinate frame
- one or more reference geometries
- parameters
- constraints
- compatibility rules
- optional helper geometry
- allowed operations

Example:

```json
{
  "id": "dovetail_slot",
  "typeId": "mechanical.dovetail.slot.v1",
  "frame": "rear_spine_frame",
  "params": {
    "baseWidth": 12.6,
    "topWidth": 17.6,
    "height": 5.6,
    "clearance": 0.6,
    "slideAxis": "z"
  },
  "interfaces": ["mechanical_slide", "linear_insert"],
  "allowedOps": ["mate", "cut", "fuse"],
  "compatibleWith": ["mechanical.dovetail.rail.v1"]
}
```

### Mate

A mate relates two ports.

It contains:

- source port
- target port
- solve rule
- tolerance rule
- direction or axis constraints
- optional limits
- output behavior

Example:

```json
{
  "type": "linear_insert",
  "a": "rail.dovetail_rail",
  "b": "cage.dovetail_slot",
  "clearance": 0.6,
  "stopAt": "slot_top_stop"
}
```

### Assembly

An assembly is a reproducible recipe.

It references components, param overrides, mates, transforms, operation order, and export intent.

Example:

```json
{
  "schemaVersion": 1,
  "components": [
    {
      "id": "rail",
      "source": "components/frame-rail",
      "params": {
        "mount_spacing": 64
      }
    },
    {
      "id": "cage",
      "source": "components/bottle-cage",
      "params": {
        "bottle_diameter": 74,
        "wall_thickness": 4
      }
    }
  ],
  "mates": [
    {
      "type": "linear_insert",
      "a": "rail.dovetail_rail",
      "b": "cage.dovetail_slot",
      "clearance": 0.6
    }
  ],
  "output": {
    "mode": "separate_parts"
  }
}
```

### Operation

Operations describe how components become final output.

Initial modes:

- `place`: position component without solving.
- `mate`: solve one or more port constraints.
- `join`: assembled parts touch or relate, but remain separate solids.
- `cut`: one component removes material from another.
- `fuse`: boolean union into one solid.
- `mold`: create transition geometry between bodies, then union and blend.
- `blend`: fillet, chamfer, or transition after union.

## Package Format

Editing should use a normal folder. Sharing should use one compressed package file.

Recommended public extension:

```text
.ecky
```

The extension represents an Ecky package, not only a source file, when used as an archive.

Working folder:

```text
my-bike-kit/
  ecky-package.json
  components/
    bottle-cage/
      source.ecky
      manifest.json
      preview.webp
      preview.stl
    frame-rail/
      source.ecky
      manifest.json
      preview.webp
      preview.stl
  assemblies/
    bottle-holder.assembly.json
  exports/
    bottle-holder.step
    bottle-holder.3mf
  checksums.json
```

Shareable package:

```text
my-bike-kit.ecky
```

Current implementation uses a ZIP outer envelope with:

- `ecky-header.json`: public searchable interface
- `ecky-payload.b64`: base64-encoded inner ZIP payload
- generated `.ecky` source-map snapshot for SketchDocument round-trip extraction

The app shows one file to users. The base64 payload is not security. It is an opaque transport envelope that keeps the top-level package clean and avoids leaking the folder layout during quick inspection. Real private packages still need encryption or server-side authorization later.

### Public Header And Private Payload

The package should expose enough metadata for search and reuse without forcing users to inspect internal structure.

Public header:

- package name
- version
- license
- author
- preview image
- component list
- exposed params
- exposed ports
- supported operations
- required Ecky version
- compatibility tags

Payload:

- source `.ecky`
- manifests
- generated meshes
- STEP/3MF exports
- sketch artifacts
- solver traces
- authoring history

Package visibility modes:

- `source`: editable and inspectable.
- `compiled`: params and ports exposed, source hidden.
- `locked`: preview/export only, no param editing.
- `private`: encrypted payload for authorized users.

Base64 may be used for the payload envelope or text-only transport. It is larger and does not provide secrecy, so it must not be described as protection.

## Standards And Interop

Use existing formats where they fit:

- `.ecky`: Ecky source or package.
- `manifest.json`: semantic layer.
- `.FCStd`: FreeCAD-native editable artifact where applicable.
- `.step`: CAD exchange.
- `.3mf`: print package.
- `.stl`: preview and fallback export only.
- `.glb` or `.gltf`: future rich viewer asset with scene metadata.

No existing standard fully covers Ecky-style parametric components with ports, mates, validations, and fuse/mold rules. STEP AP242 has product structure, but Ecky still needs its own semantic interface.

## Configurable Port And Mate System

Port and mate types should not be permanent hardcoded enums.

Use this structure:

```text
small built-in geometric contracts
  -> data-defined templates
  -> user-created interface types
  -> optional safe plugin solvers later
```

Built-in primitive contracts:

- local frame
- point
- axis
- plane
- profile
- curve
- face patch
- volume
- pattern
- clearance envelope
- keepout volume

Built-in templates:

- `plane`
- `axis`
- `bolt_pattern`
- `dovetail`
- `snap_fit`
- `socket`
- `insert`
- `surface_patch`
- `cut_profile`

These should be implemented as shipped definitions over primitive contracts.

User-created interface types should be data-defined in v1:

```json
{
  "typeId": "user.panel.clip_socket.v1",
  "base": "socket",
  "params": {
    "width": 18,
    "depth": 8,
    "clearance": 0.35
  },
  "constraints": [
    {
      "kind": "min_wall",
      "value": 2.4
    },
    {
      "kind": "insert_axis",
      "axis": "z"
    }
  ],
  "compatibleWith": ["user.panel.clip_plug.v1"]
}
```

Later extension path:

- sandboxed WASM or plugin-defined solvers
- signed packages
- permission gates for untrusted solver execution
- deterministic replay and validation logs

## Assembly Modes

Assembly should support multiple output modes.

### Separate Parts

Use for modular designs, screws, rails, snap-fit assemblies, and kits.

Properties:

- parts remain separate
- mates define placement
- exports can include a multi-part scene
- printing can be per part

### Joined Assembly

Use when parts are positioned as one design but not boolean-fused.

Properties:

- constraints solved
- no destructive boolean operations
- useful for visualization and BOM

### Fused Solid

Use when output should become one printable or manufacturable body.

Properties:

- boolean union
- operation order matters
- validation must check manifold output
- source should preserve original components and operation recipe

### Molded Or Blended Solid

Use when two components should transition seamlessly.

Properties:

- requires explicit mating surfaces or blend zones
- may generate bridge geometry
- may apply fillets/chamfers after union
- needs keepouts and wall thickness validation

Mold/blend should not be attempted blindly from raw STL except as a best-effort preview.

## Fusion And Molding Requirements

Reliable seamless fusion needs metadata.

Each participating component should expose:

- fusion surfaces
- allowed overlap depth
- blend radius limits
- keepout volumes
- wall thickness constraints
- material/print constraints
- operation order hints
- cut/fuse permissions

Example:

```json
{
  "fusionZone": {
    "id": "rear_spine_patch",
    "surface": "rear_spine_outer_face",
    "normal": [0, -1, 0],
    "allowedOps": ["fuse", "blend"],
    "maxBlendRadius": 4,
    "keepouts": ["bottle_clearance_volume", "bolt_holes"]
  }
}
```

Validation must run after every boolean or blend:

- body exists
- part count expected
- manifold or BRep-valid
- no self-intersections
- no blocked keepout
- minimum wall thickness satisfied
- clearance still valid
- export succeeds

## Sketch-First Modeling

Sketch-first modeling should be treated as the intent lens of a larger sketch/draft/exact workflow.

First UI shape:

```text
+----------------+----------------+----------------+----------------+
| Front Sketch   | Side Sketch    | Top Sketch     | Draft View     |
| draw profile   | draw depth     | draw footprint | touch the form |
+----------------+----------------+----------------+----------------+
                         |
                         v
                shared WorkspaceScene
                         |
     +-------------------+-------------------+
     |                                       |
     v                                       v
local sketch/draft validation         exact rebuild candidate
     |                                       |
     +-------------------+-------------------+
                         |
                         v
                 commit exact regions
```

Supported sketch primitives:

- point
- line
- polyline
- arc
- circle
- spline
- closed profile
- construction line
- dimension
- symmetry axis
- tangent constraint
- horizontal/vertical constraint
- equal length/radius constraint

Supported first features:

- extrude
- revolve
- loft
- shell
- cut
- hole
- chamfer
- fillet

The first version should avoid full automatic inference. Users or agent patches should specify intent:

- this sketch is a revolve profile
- this top sketch is a footprint
- extrude this profile by 80 mm
- cut this hole through
- apply shell thickness 3 mm

The workspace should also explain feature math:

- extrude: profile plus direction plus distance
- revolve: profile plus axis plus angle
- loft: ordered profiles plus continuity choice
- cut: subtracting profile or body from target
- shell: offset surfaces plus wall thickness
- fillet/chamfer: edge selection plus radius or distance

The sketch lens should remain editable after draft-form edits and after exact rebuild attempts. Editing sketch intent should invalidate downstream draft/exact state, not replace it.

## Sketch As Source

Sketch intent should remain durable source, not disappear when a draft or exact model exists.

Today the exact-source path is:

```text
SketchDocument
  -> generated `.ecky`
  -> Core IR
  -> backend build
```

That remains current implementation truth. The roadmap extension is not replacing that path. It is adding a first-class draft-form lane and a shared-scene contract around it.

Example target shape:

```lisp
(model
  (params
    (number width 80.0 :min 20.0 :max 200.0 :label "Width")
    (number height 120.0 :min 20.0 :max 240.0 :label "Height")
  )

  (sketch "front_profile"
    (polyline "outer"
      (point (- 0.0 (/ width 2.0)) 0.0)
      (point (- 0.0 (/ width 2.0)) height)
      (point (/ width 2.0) height)
      (point (/ width 2.0) 0.0)
      (close)
    )
    (constraint vertical "outer.left")
    (constraint vertical "outer.right")
    (constraint horizontal "outer.bottom")
  )

  (part "body"
    (extrude (profile "front_profile.outer") 42.0)
  )
)
```

Generated source can choose packaging behavior:

- keep sketches as editable construction history
- compile sketches into profiles for locked packages
- hide sketch internals in compiled packages
- drop sketches only for non-editable export

Default should keep sketches. Users will want to edit the original design intent.

Rendered models without editable sketch history should still produce derived projection sketches. These projections can support measurement, validation, Math Lens explanations, repair suggestions, and later draft reconstruction, but they should not be saved as authoring history unless the user explicitly converts or redraws them.

## Internal Representations And Correspondence

Ecky should use linked representations with clear responsibility.

### SketchIntent

Purpose:

- 2D curves
- dimensions
- constraints
- semantic labels
- view planes
- construction geometry

Current shape today:

- `SketchDocument`
- sketch-source envelope embedded in generated `.ecky`

### MeshDraft

Purpose:

- editable draft form
- stable draft region/entity ids
- immediate visual feedback
- local push/pull/brush/inset/extrude style operations
- silhouette comparison
- staging for exact rebuild

Current shape today:

- preview hull mesh
- backend preview mesh assets
- no first-class editable `MeshDraftDocument` yet

### ExactModel

Purpose:

- BRep feature graph
- params
- ports
- mates
- validations
- exports

Current shape today:

- generated `.ecky`
- Core IR
- exact backend build
- accepted exact validation/repair loop

### CorrespondenceGraph

Purpose:

- sketch primitive -> draft region links
- draft region -> exact feature/entity links
- projection/validation target links
- stale/fresh/rebuildable/failed/committed status propagation

This graph should make the shared object state inspectable for both users and agents.

## Agent Patch Loop And Rebuild Requests

Agents can propose local patches and rebuilds, but they should operate inside the shared-scene solver/validator loop.

Manual sketch edits and agent sketch patches share the same current path: `SketchDocument` primitives become generated `.ecky` source, then the existing preview/build/validation loop handles the result. The next step is to give agents the same shared-scene view over sketch, draft, and exact state.

Workflow:

```text
user intent
  -> user edits sketch or draft
  -> shared WorkspaceScene updates
  -> agent receives AgentScenePacket
  -> agent proposes local sketch patch, draft patch, or exact rebuild
  -> solver/validator checks the patch
  -> preview and projections update
  -> exact backend builds candidate where requested
  -> validator compares exact result back to sketch and draft intent
  -> accepted patch becomes part of scene history
```

This creates a constrained local-edit loop:

- user and agent act on the same selected object state
- sketch solver rejects bad 2D geometry early
- draft-form edits give fast feedback
- exact rebuild happens only where requested and supported
- validator compares exact output back to sketch and draft intent
- repair stays targeted to specific failures and regions

Example agent scene packet:

```json
{
  "selection": {
    "activeLens": "draft",
    "sketchPrimitiveIds": ["front_outer"],
    "meshRegionIds": ["body_shoulder_left"],
    "exactEntityIds": []
  },
  "status": {
    "sketch": "fresh",
    "draft": "fresh",
    "exact": "stale"
  },
  "constraints": {
    "lockedDimensions": ["front_width"],
    "allowedPatchTargets": ["draft", "exactRebuildCandidate"]
  },
  "evidence": {
    "projectionViews": ["front", "side", "top"],
    "sourceFragments": ["SketchDocument", "generatedEcky"],
    "correspondence": ["front_outer -> body_shoulder_left"]
  }
}
```

Agent responses should be local:

- patch selected sketch primitives
- patch selected draft regions
- propose exact rebuild for selected rebuildable region
- explain blocked rebuild or validation failure in terms of visible scene state
- Math Lens explanation of core geometry and feature choices

## Subagent Execution Model

Ecky should treat generation as visible, ledger-backed subagent work, not hidden chat output.

Execution loop:

```text
intent
  -> planner subagent creates small sketch task
  -> tutor subagent explains the relevant modeling concept
  -> sketch subagent emits Sketch IR patch
  -> validator subagent checks constraints
  -> preview subagent updates mesh evidence
  -> projection subagent updates useful 2D views
  -> source subagent emits .ecky patch
  -> build subagent runs backend build
  -> repair subagent consumes raw failures
  -> progress ledger records accepted/rejected step
```

Rules:

- one observable step per ledger entry
- every step has input, output, validation status, and raw error when failed
- failed steps remain visible but do not mutate accepted model state
- accepted steps become source, not chat transcript
- subagents may propose; validators decide
- UI shows concise Ecky bubble status, not terminal log stream
- mentor roles explain and coach, but do not bypass solver or validator authority

## Validation System

Validation is the backbone of the whole feature.

### Component Validation

Checks:

- source parses
- params valid
- geometry builds
- BRep or mesh exists
- preview assets exist
- exported artifacts exist when required
- manifest matches source
- ports reference valid geometry
- keepouts are valid volumes

### Port Validation

Checks:

- frame exists
- frame orientation valid
- referenced geometry exists
- required params present
- compatible interface declared
- allowed operations valid
- no reused param key with changed meaning

### Mate Validation

Checks:

- both ports exist
- type compatibility
- solve succeeds
- transforms deterministic
- clearance applied
- limits respected
- assembly remains valid

### Fusion Validation

Checks:

- boolean operation succeeds
- expected body count
- no non-manifold output
- no self-intersections
- keepouts preserved
- wall thickness valid
- clearance valid
- blend radius feasible
- export succeeds

### Sketch Validation

Checks:

- profiles closed where required
- no self-intersecting profile
- dimensions solvable
- constraints solvable
- generated preview within expected bounds
- generated BRep matches sketches within tolerance
- required features present
- forbidden zones avoided

Spatial sketch validation examples:

- generated model stays inside front silhouette envelope
- model intersects required support region
- hole center lies within tolerance
- bottle clearance volume remains empty
- rail slot aligns with side sketch

### Projection Validation

Checks:

- rendered model has front, side, and top projections
- projection extents match model bounds
- hole and port centers appear in expected views
- derived outlines identify their source as generated projection data
- authoring sketches are editable only when backed by Sketch IR or feature history
- validation overlays distinguish authoring sketches from derived projections

## User Experience

### Workspace Lenses And Selection

Capabilities:

- one object stays visible across sketch, draft, and exact lenses
- users can switch lenses without destroying other representations
- selected sketch primitives, draft regions, and exact entities stay inspectable
- each representation exposes `fresh`, `stale`, `rebuildable`, `failed`, or `committed` status
- exact rebuild can be region-local while the rest of the object remains draft
- projection views stay attached to the current scene state, not a disconnected export

### Learning And Mentor

Capabilities:

- Tutor role explains active modeling concepts in context.
- Builder role proposes the next valid operation or rebuild.
- Coach role suggests constraints, dimensions, region selection, feature order, and simplifications.
- Debugger role explains raw solver/backend/provider failures.
- Math narrator role links coordinates, vectors, projections, functions, transforms, and tolerances to visible geometry.
- Mentor copy appears in Ecky bubble state and relevant panels.
- Interactive agent stdout/stderr remains in the terminal modal.

### Math Lens

Capabilities:

- show coordinate frames, axes, vectors, normals, tangents, and dimensions
- explain extrude/revolve/loft/sweep/shell as profile- or path-driven operations
- explain draft-form edits as visible region changes with local geometric consequences
- explain transforms as visible frame changes
- show slope, curvature, radius, and continuity for curves and blends
- show tolerance, clearance, wall thickness, and fit checks with measured values
- tie highlighted geometry to generated `.ecky` source, draft state, and validation ledger entries

### Model Projections

Capabilities:

- generate front, side, and top projections for every representation
- show section or face projections where useful
- expose derived projection sketches for models without editable source/history
- expose authoring sketches only when backed by `SketchIntent` or exact feature profiles
- compare draft/exact projections against authoring sketches and tolerance overlays
- keep derived projections useful for learning, validation, rebuild guidance, and agent context without claiming they are original intent

### Sketch Workspace

Capabilities:

- draw front/side/top sketches
- inspect and edit one shared object state
- use polylines/splines/arcs
- dimension sketch
- mark construction geometry
- mark holes, axes, ports, keepouts
- see live draft-form feedback while drawing
- switch between sketch, draft, and exact lenses
- generate `.ecky` and exact rebuild candidates
- compare draft and exact result back to sketch
- accept or repair step by step
- switch Math Lens overlays on selected geometry and operations

### Agent Patch Loop

Capabilities:

- send selected scene state to the agent through a shared packet
- let the agent patch sketch, draft, or exact rebuild candidates locally
- show planner/sketch/draft/validator/preview/source/build/repair steps
- record accepted and rejected steps in a visible ledger
- preserve raw backend and validator failures
- keep accepted model state separate from failed proposals
- replay accepted sketch, draft, and source patches deterministically

### Validation And Progress Ledger

Capabilities:

- show current sketch validity
- show current draft validity
- show current exact rebuild status
- show drawn profile evidence
- show live SketchDocument source evidence with primitive ids
- show draft mesh evidence
- show generated exact source state
- show source-backed projection evidence
- show backend build state
- show sketch/draft/exact validation state
- keep raw backend/provider error bodies attached to rejected steps

### Assembly Editor

Capabilities:

- drag components into workspace
- snap compatible ports
- choose mate
- choose output mode
- preview constraints
- show failed mates
- validate assembly

### Fusion/Molding Editor

Capabilities:

- select components
- choose operation
- choose fusion zone
- set overlap/blend radius
- preview generated bridge
- validate output
- preserve original recipe

### Parked Library Browser

Capabilities:

- browse installed packages
- see previews
- filter by tags
- inspect exposed params
- inspect ports
- open package detail
- add component to workspace

### Parked Component Detail

Capabilities:

- preview component
- adjust params
- show ports
- show keepouts
- show compatible components
- export single part
- add to assembly

## Progress Ledger

Status meanings:

- Done: implemented in current work.
- In progress: partial foundation exists; user-visible workflow or full acceptance still pending.
- Next: planned roadmap work not yet implemented.
- Pending: blocked behind earlier sketch/BRep work; not started.
- Parked: intentionally deferred outside the sketch-first loop.

| Status | Area | Implemented Evidence / Remaining Work |
| --- | --- | --- |
| Done | Sketch workspace drawing flow | `SKETCH` floating window supports Front/Top/Side pointer drawing, full-pane cursor-aligned SVG strokes, closed-profile detection, automatic closed-profile preview, closed-profile point handles, drag updates that flow through SketchDocument/source/preview while preserving primitive ids, local open-profile blocking, pending state, preview asset evidence, and raw backend error display. |
| Done | Visible sketch-step ledger | Sketch workspace exposes user-visible step records for draw, preview, draft generation, and SketchDocument source evidence so accepted, pending, and failed sketch work is inspectable instead of hidden in chat. |
| Done | Sketch draft source generator | Closed polyline, spline, and circle draft source generation; Tauri command and TypeScript client wrapper. |
| Done | Sketch draft mesh preview backend | Sketch draft source renders through Ecky mesh backend into `ArtifactBundle` with preview STL and viewer assets. |
| Done | Deterministic sketch suggestions | SketchDocument contracts, deterministic closed-profile feature suggestions, open-profile warnings, accepted-suggestion conversion, Tauri command, TypeScript contracts, visible suggestion UI from the live SketchDocument, and accepted suggestion draft/preview/source evidence landed. UI evidence exposes camelCase SketchDocument data and primitive ids. |
| Done | SketchDocument source evidence | Drawn sketch strokes now become structured SketchDocument / Sketch IR source evidence instead of disposable SVG-only marks. Generated `.ecky` now carries an embedded SketchDocument source-map snapshot, and extraction can round-trip source back into sketch state. UI exposes the live camelCase document, view planes, and primitive ids so source, preview, projection, and later replay/edit flows can reference the same authored primitives. |
| In progress | Modeling playground and AI mentor direction | Product roadmap now positions Ecky as modeling playground plus AI mentor. Tutor, Builder, Coach, Debugger, and Math narrator product roles defined. UI integration and grounded mentor workflows remain. |
| In progress | Math Lens | Coordinates, geometry, functions, transforms, feature math, vectors/normals, curvature/slope, and tolerance concepts defined. Extrude lens UI is live, accepted-CAD auto-repair switches the lens to affine BRep bounds snap / containment expansion math, and topology/concavity redraws explain projection-derived seeds vs original authoring history. Rich selected-geometry overlays, source links, and measured validation explanations remain. |
| In progress | Projection principle | Rule documented that every rendered model should expose useful 2D projections. Editable source authoring sketches remain source/history-backed only; derived projection sketches handle non-authoring model inspection. Projection-loop seed now has deterministic source-backed projections, a UI projection panel, viewport source silhouette overlay, projection validation copy, and BRep-derived projection conversion with explicit `derivedFromBRep` provenance. Closed and multi-loop HLR projections now convert into separate editable primitives before falling back to bounds. Live local projection preview now shows closed profiles as deterministic orthographic projections before backend preview returns and blocks open profiles. Rich arbitrary topology semantics from BRep remain pending. |
| Done | Projection-loop seed | Deterministic projection helper seeds front/top/side projection records from source-authored SketchDocument primitives. UI projection panel exposes those source authoring projections without presenting them as BRep-derived outlines. Later BRep-derived projections remain pending. |
| Done | Source-fit/containment validation seed | Sketch workspace now records a source sketch contract, generated draft source, backend preview artifact evidence, source-backed projections, source silhouette overlay evidence, source-fit/containment validation copy, tolerance/readout ledger evidence, dimensions/constraints readout, and raw backend/provider errors in the visible ledger. This is source-backed UI evidence for the sketch/build loop, not full BRep validation or BRep/silhouette comparison. Full BRep-derived silhouette compare, inside/outside from BRep, tolerance overlays, and targeted repair loop remain next. |
| In progress | Continuous sketch stream | User drawing, automatic closed-profile preview, visible sketch-step ledger, structured SketchDocument source evidence, and source patch ledger for cleanup/repair mutations landed. `.ecky` source with embedded SketchDocument envelope can now import back into editable sketch state. Append-only replay of accepted/rejected source patches, richer source edit round-trip, and richer subagent task boundaries remain. |
| In progress | Subagent execution model | Need planner/sketch/preview/source/build/repair task boundaries, raw failure capture, and ledger-backed UI updates. |
| In progress | Sketch IR | Package-level Sketch IR contracts, live SketchDocument source evidence, camelCase UI document exposure, primitive ids, source generator, editable import, pasteable camelCase JSON, pasteable `.ecky` source-envelope import, local validation, replay back to strokes, preview mesh, editable closed-profile point drag flow, configurable grid snap, selected point coordinate editing, delete point, simple profile dimension editing, exact profile origin placement, profile size/origin grid snaps, source-backed width/height dimension lock constraints, locked-axis point-edit solver seed, dimensions/constraints readout, and source-backed dimension constraint validation landed. Arbitrary BRep reverse projection and full BRep constraint solving remain pending. |
| Done | Point edit handles | Closed-profile point handles are visible only for closed profiles. Dragging a handle updates stroke geometry, SketchDocument/source evidence, suggestions, and preview request while preserving primitive ids. |
| Done | Configurable snap grid | Grid snap is user-changeable in the sketch workspace and applies to both new drawing points and closed-profile point edits. Exact constraint-driven snaps and dimension snaps remain next. |
| Done | Selected point coordinate editor | Numeric coordinate editing for the selected closed-profile point landed with selection binding, x/y entry, source/preview refresh, validation copy, and e2e proof. |
| Done | Simple profile dimension editor | Profile-level width/height editing landed for closed profiles. Apply size scales from the profile min corner, refreshes SketchDocument/source/preview evidence, preserves closed duplicate points, and reports exact local dimension validation errors without backend calls. Full dimensions, exact constraints, and dimension snaps stay next after this. |
| Done | Delete point | Closed-profile point delete landed with UI affordance, source/preview refresh, minimum 3 logical point guard, exact local validation, and e2e proof. |
| Done | Source-backed dimension locks | Sketch workspace can lock/unlock current closed-profile width and height as SketchDocument `dimension` constraints. Constraints preserve primitive ids, stay camelCase in frontend JSON, feed draft preview requests, replay from imported SketchDocument constraints back into editable sketch state, and guard edits that would change locked width or height before any backend call. This remains source evidence and local constraint enforcement, not full geometric solving. |
| Done | Constraint solver seed | Locked-dimension point edits translate the profile on locked axes so edits preserve locked width/height without warping the profile or dead-blocking point movement. Validation ledger shows `CONSTRAINT SOLVER` evidence after a locked-axis solve. Profile width/height size edits remain blocked while that dimension is locked unless explicit unlock/solve behavior lands later. This is a seed, not full constraint solving. |
| Done | Source-backed dimension constraint validation seed | Imported SketchDocument and `.ecky` source-envelope dimension constraints validate against source primitive bounds before preview. The validation ledger shows locked constraint value evidence for current source profiles. Tiny imported dimension mismatches now auto-snap primitive geometry to the explicit dimension constraint before preview with `AUTO SNAP` evidence. Large stale imported constraints still fail locally with exact measured mismatch text and expose explicit repair. This is source-backed validation, not full BRep constraint solving. Package/library work remains parked. |
| Done | Import repair action | Stale imported width/height dimension constraints now expose an explicit `REPAIR IMPORT` action after raw validation fails. Repair updates constraint values to measured source primitive bounds, preserves primitive ids/view/points, refreshes SketchDocument/source evidence, and then previews. This remains explicit source repair after visible failure, not silent validation bypass. |
| Done | Source patch ledger | Cleanup and import-repair mutations now append visible source patch ledger entries with action, primitive id, and evidence text. This is UI-visible mutation history for source-backed sketch edits; patch replay/undo semantics remain later. |
| Done | Multi-view depth constraint seed | Front remains the primary authoring profile for extrude, but matching Top and Side profiles now constrain depth instead of being ignored. Front+Top validates shared width and uses Top height as extrusion depth; Front+Side validates shared height and uses Side width as depth; Top/Side depth mismatches block before backend. Draft mode UI now labels `SINGLE-VIEW EXTRUDE` versus `MULTI-VIEW CONSTRAINED`. This is still a seed, not full 3-view surface reconstruction. |
| Done | Orthographic auto snap seed | Multi-view width/height/depth mismatches now emit typed repair actions and the Sketch Workspace applies safe axis-scale snaps before preview. Shared-axis range mismatches auto-translate Top/Side views to match Front/Top ranges before preview hull generation. Import-time tiny endpoint gaps auto-close into exact duplicate endpoints, and tiny imported dimension mismatches auto-resize primitive geometry to the explicit source dimension. Auto snap preserves primitive ids, records `AUTO SNAP` entries in the source patch ledger and validation ledger, reruns validation, and still leaves non-repairable import constraint failures as exact local errors without backend calls. |
| Done | Orthographic reconstruction research spine | Roadmap now separates live preview hull, exact BRep candidate reconstruction, BRep hidden-line reprojection, and learned proposal generation. Sources reviewed: Wesley/Markowsky projection fleshing, Shum/Lau/Yuen/Yu 2-stage extrusion, Liu/Hu/Chen/Sun curved-solid BRep reconstruction, Laurentini visual hull, Open CASCADE HLR, and PlankAssembly shape programs. |
| Done | Preview hull mesh seed | Front+Top or Front+Side sketches now use a dedicated `generate_sketch_preview_hull` command instead of single-view draft preview. Backend emits `.ecky` mesh source from silhouette-consistent candidate cell search, embeds the full SketchDocument source map, preserves raw validation errors, and warns that the result is a preview hull, not accepted BRep. Front-only sketches still use the single-view draft path. |
| Done | BRep candidate graph seed | Front+Top/Side sketch documents now run `analyze_sketch_brep_candidates` after preview hull generation. Backend builds candidate 3D vertices from matching orthographic endpoints, emits two-view-supported candidate edges, replays candidate edges back into Front/Top/Side source projections, searches silhouette-consistent candidate cells, and reports cell/solution evidence. Multi-cell concave profiles with rectangular Top/Side depth views are promoted to an exact front-profile prism strategy instead of a box-cell union. Sketch workspace shows `BREP CANDIDATE GRAPH` vertex/edge/cell/solution counts, projection replay pass/fail evidence, and raw candidate backend errors without blocking preview mesh display. This is first-pass exact topology selection, not general arbitrary BRep topology. |
| Done | Exact OCCT hidden-line extraction | Backend now uses exact OCCT hidden-line extraction for accepted backend geometry projection evidence. Hidden-line projection accepts FCStd artifacts and direct-OCCT STEP export artifacts, so BRep-derived front/top/side line extraction is no longer FreeCAD-bundle-only. Topology reconstruction remains pending. |
| Done | BRep hidden-line sketch validation seed | Hidden-line extraction now accepts the current SketchDocument and returns backend-authoritative BRep/sketch validation evidence with the OCCT projection response. Sketch workspace sends source sketch context, displays raw validation issues in the OCCT panel, uses backend validation evidence in the ledger with frontend summary as fallback, emits compact repair targets for failed sketch/primitive ids, and validates sampled BRep projection points against closed source profiles so same-bounds/wrong-silhouette cases fail. This is validation seed evidence for exact BRep-derived lines, not full topology validation. |
| Done | Exact topology target surface | FreeCAD reports `Shape.Edges` endpoints and `Shape.Faces` center/normal/area into runner reports; Direct OCCT writes per-part edge endpoint and face center/normal/area topology reports beside STEP/STL exports. Rust turns those into manifest `edge` / `face` selection targets and bundle `edgeTargets` / `faceTargets` with matching ids. This gives the viewer and later assembly/package tools real selectable BRep topology handles, not semantic inference like "hinge" or "tripod". Manifest refresh syncs FreeCAD labels/editability while preserving geometry coordinates. Component ports can preserve validated `targetIds` that reference accepted edge or face targets. |
| Done | Accepted CAD auto-repair seed | Repairable BRep/sketch bounds mismatches now auto-snap matching closed source polylines to exact hidden-line projection bounds, preserve primitive ids and closed duplicate points, append `AUTO SNAP` source patch evidence, rerun preview/hidden-line once, and pass the accepted-CAD gate only after the second exact validation passes. BRep containment mismatches now apply a bounded source-envelope expansion only when projection bounds strictly exceed source bounds and stay under the safety cap. If backend validation omits a primitive id, the workspace infers the target only when the failed sketch has exactly one closed polyline; ambiguous, unsupported, repeated, same-bounds containment, huge expansion, and unknown topology issues stay raw visible failures with no repair loop. |
| Done | BRep hidden-line sketch overlays | Sketch panes now draw OCCT hidden-line projection edges over Front/Top/Side source sketches. Visible and hidden BRep edges stay distinct, and backend validation failures mark the affected overlay as failed so mismatch is visible in the drawing surface, not only the ledger. This is exact projection overlay evidence, not topology repair or editable BRep reverse-sketching. |
| Done | Accepted CAD gate | Sketch workspace now exposes an `ACCEPTED CAD` validation row. Mesh preview and preview hull artifacts stay pending; exact FCStd/STEP hidden-line BRep/sketch validation must pass before the row passes. Raw BRep/sketch mismatch text fails the acceptance row, so disposable preview output cannot be mistaken for manufacturable accepted CAD. |
| Done | Authoring context ownership | Thread authoring context has been removed. Source belongs to model versions and generated artifacts; backend selection belongs to session config and artifact build metadata, not thread records. |
| Done | Exact profile origin placement | Profile size editor exposes `PROFILE X` and `PROFILE Y` as the closed profile's min-corner origin. Applying position translates all profile points, preserves dimensions and locks, updates SketchDocument source JSON, and blocks invalid coordinates before backend calls. |
| Done | Dimension snaps | `SNAP` plus `GRID` applies to exact `PROFILE X/Y` placement and `PROFILE WIDTH/HEIGHT` sizing. Values round to the active grid before source generation; invalid grid values show exact local validation and block backend calls. |
| Done | `.ecky` source-envelope import | Sketch import accepts either camelCase SketchDocument JSON or generated `.ecky` source containing the embedded SketchDocument base64 envelope. Missing envelope marker and invalid JSON errors remain raw and visible; successful import rebuilds editable strokes and queues preview from source-backed primitive ids. |
| Done | Source-backed viewport silhouette overlay | Main viewport can show source-authored sketch silhouettes over sketch preview output. Overlay is UI evidence from SketchDocument/projection records, not a BRep-derived silhouette or geometric compare. |
| Done | Dimensions/constraints readout | Sketch workspace exposes current source-backed dimensions and constraint/readout copy for authored profiles. Readout is inspection evidence, not exact constraint solving. |
| Done | Projection validation copy | Projection UI now labels source-backed projection evidence and clarifies validation scope. Copy does not claim full BRep/silhouette compare, inside/outside validation, or geometric truth beyond authored-source projections. |
| Done | Sketch cleanup UX | Deterministic cleanup of rough closed source profiles into clean editable source geometry landed. `CLEAN UP` converts the latest rough closed source profile into a source-bounds rectangle, preserves primitive id/view, refreshes SketchDocument/source/projection/preview evidence, and reports exact local validation for open profiles without backend calls. This is not BRep reverse engineering, arbitrary model-to-sketch inference, or speculative agent magic. |
| In progress | Arbitrary model-to-sketch projection | Source-backed authoring projections landed. Exact HLR front/top/side projection evidence can now produce derived editable sketch seeds with `derivedFromBRep` provenance and a user-triggered convert path. Closed and multi-loop HLR edge loops are preserved as separate editable primitives when possible; otherwise conversion falls back to a bounded editable seed. This stays separate from authoring history. Rich topology semantics, hole intent, and feature reconstruction remain pending. |
| Done | Live local sketch ghost | Main viewport now shows immediate local sketch ghost feedback while a user draws open profiles and while closed-profile backend preview is queued/generating. It is disposable UI evidence only, distinct from real mesh preview, BRep build, silhouette comparison, and sketch/BRep validation. |
| In progress | Draft Mesh Seed | Backend preview path, UI preview evidence, main viewport sketch preview handoff, generated `.ecky` CODE affordance, compact preview STL/asset evidence, cursor-aligned pane drawing, viewport-local ghost feedback, source-backed silhouette overlay, and ledger-backed preview artifact evidence landed. Real mesh preview remains backend-generated, while BRep validation, BRep-derived silhouette comparison, and inside/outside checks remain next. |
| In progress | Exact Rebuild Seed | Draft source generation, drawn-profile workspace trigger, deterministic suggestion UI from SketchDocument, and accepted-suggestion path through draft source, preview asset evidence, and source output landed. Rich feature generation and backend build validation remain. |
| In progress | Shared Scene / Exact Validation Seed | Exact HLR, BRep-derived sketch overlays, bounds validation, sampled containment validation, projected-loop topology validation, repair-target surfacing, accepted-CAD gating, bounded bounds-mismatch auto-repair, bounded containment envelope expansion, BRep-derived sketch conversion, explicit topology/concavity redraw execution, first-pass arbitrary 3-view candidate cell reconstruction/search, and exact front-profile prism selection for rectangular depth views landed. General exact BRep topology selection remains. |
| Next | Agent Patch Loop | Prompt-to-scene proposal workflow, local agent patch cards, and repair from structured validation state remain later work. Deterministic suggestions are visible now without agent dependency. |
| Done | Package contracts | Component package schema, package visibility modes, params, ports, data-defined port and mate type definitions, mate pair allow-lists, mate compatibility validation, assembly recipes, operation kinds, keepouts, fusion zones, package-level Sketch IR contracts. |
| Done | `.ecky` header and payload archive | ZIP outer envelope with `ecky-header.json` plus `ecky-payload.b64`; header model omits source refs and sketch internals; header can be read without full manifest payload. |
| Done | Local package install/import | Local component library install, header listing, Projects package browser/import, and raw backend error display landed. |
| Parked | Package detail/library UI | Component detail, preview, and exposed param controls deferred until sketch-first loop works. Package/library UI stays parked; source/backend state is owned by model versions, artifacts, and session config. |
| Parked | Assembly/Fuse/Mold UI | Contracts exist. Geometric solvers, boolean execution, molding UI, and validation overlays deferred. |
| Parked | Public Registry | Registry metadata, upload/download, signing, dependencies, version resolution. |

## Implementation Phases

| Phase | Status | Progress |
| --- | --- | --- |
| Phase 0: Sketch Stream Contracts And Ledger | In progress | SketchDocument, suggestion, draft, validation contracts, visible sketch-step ledger, camelCase SketchDocument source evidence, primitive-id evidence, accepted suggestion draft/preview/source evidence, source-backed projection evidence, source silhouette overlay evidence, dimensions/constraints readout evidence, tolerance/readout ledger evidence, projection validation copy, source-fit/containment validation seed evidence, raw backend error evidence, editable import, local validation, and shared manual/agent SketchDocument -> `.ecky` polygon/extrude pipeline direction landed. Append-only accepted/rejected source patch replay remains. |
| Phase 1: Sketch Workspace Continuous Flow | In progress | `SKETCH` window supports real full-pane cursor-aligned drawing into structured SketchDocument state, automatic closed-profile preview, local invalid-state gating, pending UI, editable SketchDocument paste/import, pasteable `.ecky` source-envelope import, replay to strokes, point handles, configurable grid snap for drawing and point edits, selected point coordinate editing, delete point, simple profile dimension editing, exact profile origin placement, profile size/origin grid snaps, source-backed dimension lock/unlock constraints, locked-axis point-edit solver seed, deterministic cleanup of rough closed source profiles into clean editable source geometry, dimensions/constraints readout, preview mesh, main viewport preview handoff, viewport-local ghost feedback, source silhouette overlay, and ledger UI. Richer source edit round-trip, richer overlays, and full constraint solving remain. |
| Phase 2: Learning Mentor And Math Lens | In progress | Product roles and Math Lens scope documented. Extrude math lens is live, and BRep auto-snap / auto-contain repairs now explain the affine source-to-BRep bounds mapping from ledger evidence. Grounded tutor/builder/coach/debugger/math narrator UI, richer overlays, and source/ledger links remain. |
| Phase 3: Projection Principle And Reconstruction Math | In progress | Source authoring sketches vs derived projection sketches rule documented. Orthographic reconstruction spine now separates live preview hull, exact BRep candidate reconstruction, BRep hidden-line reprojection, and learned proposal generation. Deterministic projection helper, UI projection panel, viewport source silhouette overlay, validation-scope copy, exact HLR overlays, and BRep-derived sketch conversion seeded. Live local projection preview now shows closed profiles as deterministic orthographic projections before backend preview returns and blocks open profiles. Rich arbitrary model-to-sketch topology conversion and richer overlays remain pending. |
| Phase 4: Shared Scene And Mentor Grounding | Next | Planner/tutor/sketch/draft/validator/preview/source/build/repair task boundaries and ledger-backed UI remain. This phase introduces `WorkspaceScene`, scene-owned selection, representation status (`fresh` / `stale` / `rebuildable` / `failed` / `committed`), and the first `AgentScenePacket` contract so user and agent act on the same object state. |
| Phase 5: Draft Mesh Seed | In progress | Backend draft preview already renders an Ecky mesh bundle, workspace shows preview asset path, main viewport shows disposable local ghost feedback before backend preview, then switches to the sketch preview with compact preview STL/asset evidence plus generated `.ecky` CODE and source-backed silhouette overlay. Front+Top/Side now route through a dedicated preview-hull command that uses candidate cell search and embeds the full SketchDocument. The next step is to present that lane as the first draft-form seed in the shared scene instead of only a disposable preview artifact. |
| Phase 6: First-Class MeshDraft | Next | `MeshDraftDocument`, editable draft regions, stable draft entity ids, region-local draft edits, and persistence of draft state remain. Existing preview-hull, preview asset, and source-backed silhouette work become the seed for this editable draft lane. |
| Phase 7: Shared Scene And Correspondence | In progress | Candidate graph seed builds 3D vertices/edges from orthographic sketch endpoints, replays them into source projections with visible pass/fail coverage, searches silhouette-consistent candidate cells, and feeds preview source from selected cells. Exact OCCT hidden-line extraction from accepted backend geometry is done for FCStd and STEP artifacts. Hidden-line extraction now returns backend-authoritative BRep/sketch validation evidence from the current SketchDocument; Front/Top/Side panes overlay BRep visible/hidden projection edges with failed-view styling; backend validation checks bounds plus sampled projection containment against closed source profiles; the OCCT panel exposes compact repair targets keyed by sketch/primitive id. BRep projection conversion now creates derived editable sketch seeds without claiming authoring history, including multi-loop projection seeds. FreeCAD exact edge/face topology and Direct OCCT exact edge/face topology now flow into manifest selection targets and bundle edgeTargets/faceTargets for later port/mate labeling. Next step: promote this work into a first-class `CorrespondenceGraph` linking sketch primitives, draft regions, exact targets, and validation/repair evidence. |
| Phase 8: Exact Rebuild | In progress | Draft source generator, deterministic suggestion UI from SketchDocument, accepted-suggestion path through draft source, preview asset evidence, source output, primitive-id traceability, exact OCCT hidden-line validation, bounded bounds-mismatch auto-repair, bounded containment envelope expansion, and topology/concavity redraw actions already seed the exact-rebuild story. Next step: recognizer/compiler for rebuildable exact subsets such as extrude, revolve, sweep, loft, shell, cuts, and holes, with incremental rebuild per region or feature instead of only whole-model acceptance. |
| Phase 9: Agent Patch Loop | Next | Prompt-to-scene proposal workflow, local agent patch cards, scene-backed Math Lens explanations, and repair from structured validation state remain later. Agent patches should target sketch, draft, or exact rebuild locally through `AgentScenePacket` on the same shared scene path. |
| Phase 10: Commit And Package | Next | Accepted exact regions/features become committed model state, component extraction from accepted sketch/draft/exact state remains, and package/archive/install flows build on that committed exact state. External import wrapper doctrine lives in [External Model Import And Component Reuse](./external-model-imports.md). |
| Phase 11+: Parked Package/Library/Assembly/Fusion/Registry | Parked | Package archive/import contracts landed. Rich package detail, assembly editor, fuse/mold UI, public registry, and broader component lifecycle remain deferred until the sketch/draft/exact loop works end-to-end. |

## Landed Foundation Tranche

Implemented 2026-04-28:

- component package schema and validation
- package visibility modes
- component params and ports
- data-defined port type definitions
- data-defined mate type definitions and allowed port type pairs
- mate compatibility validation
- assembly recipes with mates and operations
- operation kinds for place, mate, join, cut, fuse, mold, and blend
- component keepout volumes and fusion zones
- package-level Sketch IR contracts for primitives and constraints
- public package header model that omits source refs and sketch internals
- `.ecky` archive writer with `ecky-header.json` plus base64 inner ZIP payload
- backward-compatible legacy archive read/extract for plain `ecky-package.json`
- archive header read without requiring full manifest payload
- safe archive extraction with traversal rejection
- local component library install and header listing
- Tauri command boundary and generated TypeScript contracts for package read/write/archive/install/list
- Projects window package browser with installed package interface cards
- package archive import from UI with raw backend error display
- sketch draft source generator for closed polylines, splines, and circles
- Tauri command and TypeScript client wrapper for sketch-to-Ecky draft source
- sketch draft preview command that renders through Ecky mesh backend
- Sketch workspace floating window with 3 orthographic panes and raw backend error display

## Landed Sketch Stream Tranche

Implemented 2026-04-29:

- roadmap reorganized around sketch-first modeling and subagent execution
- package detail/library, assembly/fuse/mold UI, and public registry marked parked
- `SKETCH` window pointer drawing for Front/Top/Side panes
- immediate SVG stroke rendering while drawing
- closed-profile detection from endpoint proximity
- open-profile local validation that blocks backend preview
- pending preview UI and raw backend error display
- generated preview request uses user-drawn primitive ids and points instead of seed geometry
- SketchDocument and deterministic sketch feature suggestion contracts
- editable SketchDocument import and paste flow
- local validation for pasted/imported camelCase SketchDocument JSON
- replay imported SketchDocument back to strokes
- generated `.ecky` embeds SketchDocument source-map snapshot
- preview mesh for SketchDocument sketch work
- suggestion command for closed profile detection without LLM dependency
- warning path for open and unsupported primitives
- accepted-suggestion conversion to `.ecky` draft source
- generated TypeScript contracts and client wrapper for sketch suggestions

## Current Source-Fit/Containment Validation Seed Tranche

Status as of 2026-04-30:

- Ecky remains positioned as modeling playground plus AI mentor, not only AI CAD generator.
- Current validation scope is a seed tranche: source sketch contract, source-fit/containment copy, preview artifact evidence, source-backed projections, tolerance/readout evidence, and raw failure evidence recorded in the ledger.
- This tranche proves traceability from authored sketch to generated draft source to preview artifacts, viewport source silhouette overlay, source-fit/containment validation copy, dimensions/constraints readout, tolerance/readout ledger entries, and projection copy.
- This tranche is source-backed UI evidence, not full BRep validation, BRep-derived silhouette compare, inside/outside validation from BRep, or geometric truth beyond the sketch/build evidence it records.
- Drawn sketch marks now become structured SketchDocument / Sketch IR evidence, not only transient SVG strokes.
- UI evidence exposes the live SketchDocument with frontend-idiomatic camelCase fields.
- UI evidence exposes stable sketch primitive ids so source, preview, projection, and future replay/edit can address authored geometry.
- editable SketchDocument IR now accepts pasted camelCase JSON and local validation before replay.
- replay path now restores strokes from imported SketchDocument source.
- generated `.ecky` now carries embedded SketchDocument source-map snapshot, enabling source-to-sketch extraction round trip foundation.
- preview mesh now renders from the sketch workspace for sketch-first feedback.
- Projection loop now starts from source authoring sketches, not inferred BRep history.
- Deterministic projection helper seeds front, side, and top projection records from source-authored SketchDocument primitives.
- UI projection panel exposes source-backed projection evidence beside the sketch workflow.
- Live local projection preview now shows closed profiles as deterministic orthographic projections before backend preview returns.
- Open profiles stay blocked in the local projection preview path.
- Main viewport source silhouette overlay is done for sketch preview context.
- The viewport silhouette overlay is derived from authored SketchDocument/projection records, not from BRep silhouette extraction.
- Dimensions/constraints readout is done as source-backed inspection evidence for current sketch profiles.
- Dimensions/constraints readout does not replace exact constraint solving, dimension snaps, or full constraint validation.
- Source-backed dimension lock/unlock is done for current closed-profile width and height.
- Locked dimensions are emitted as SketchDocument `dimension` constraints, sent in draft preview requests, and replayed from imported SketchDocument JSON.
- Constraint solver seed is done for locked-dimension point edits by locked-axis translation.
- Locked-dimension point edits should solve by translating profile points on locked axes to preserve locked width or height, not warp the profile or dead-block the edit.
- Profile size edits remain blocked while the edited dimension is locked unless explicit unlock/solve behavior lands later.
- Dimension locks are traceable source evidence and local constraint enforcement, not a full geometric solver.
- Projection validation copy is done and labels current projection evidence as source-backed.
- Projection validation copy explicitly avoids claiming BRep/silhouette compare, inside/outside checks, or final CAD validation.
- Closed profiles now trigger automatic preview from the sketch workspace without a separate manual preview action.
- Visible sketch-step ledger now records draw, preview, source generation, and projection evidence for the sketch-first loop.
- Ledger evidence now covers drawn sketch profiles, generated `.ecky` draft source, backend preview assets, source-backed projections, viewport source silhouette overlay, source-fit/containment validation copy, dimensions/constraints readout, tolerance/readout evidence, projection validation copy, and raw backend/provider errors.
- Ledger evidence now represents the seed source-fit/containment validation record: source sketch contract, preview artifact evidence, projection evidence, tolerance/readout evidence, and UI-visible source-backed inspection evidence.
- Deterministic suggestion UI now reads the live SketchDocument and shows feature suggestions without requiring an LLM.
- Accepted deterministic suggestions now follow a deterministic path from selected suggestion to `.ecky` draft source, preview asset evidence, and source output.
- Draft, preview, and source states stay grounded in the live SketchDocument and visible ledger, not chat transcript.
- Saved camelCase IR snapshot now replays back to strokes.
- Primitive ids stay stable across snapshot/replay.
- Sketch panes now map the full visible pane to sketch coordinates, so pointer strokes no longer offset from the cursor in rectangular panes.
- Successful sketch previews now hand off to the main model viewport with compact preview STL and asset-count evidence.
- Viewport CODE opens the generated `.ecky` source for the active sketch preview.
- Live local sketch ghost is done as viewport-local pre-backend feedback while drawing open profiles and while closed-profile preview is queued/generating.
- The ghost is explicitly not real mesh or BRep validation; backend mesh preview, BRep build, silhouette comparison, and sketch/BRep validation remain separate gates.
- Closed-profile point handles are done.
- Handle drag keeps primitive ids and refreshes SketchDocument/source/preview evidence.
- Configurable grid snap is user-changeable and applies to drawing and point edits.
- Configurable snap grid is done for the current precision path.
- Selected point coordinate editor is done.
- Selected point coordinate editor binds to one selected closed-profile point, allows exact x/y entry, refreshes SketchDocument/source/preview evidence, and reports validation failures with raw backend/provider details where backend work is involved.
- Simple profile dimension editor is done for closed-profile width/height scaling.
- Profile size edits preserve primitive ids, scale from the min corner, refresh SketchDocument/source/preview evidence, and show exact local validation for invalid dimensions without backend calls.
- Manual and agent-authored sketches use the same SketchDocument -> `.ecky` polygon/extrude pipeline on the shared scene path.
- Constraint solver seed landed after the simple profile dimension editor, source-backed dimension locks, and readout evidence. Dimension snaps landed for exact size/origin controls. Source-backed dimension constraint validation landed; full BRep constraint validation remains next.
- Import repair action is done for stale source-backed width/height dimension constraints. Tiny mismatches auto-snap geometry to the explicit dimension first; large stale mismatches keep raw mismatch visible, then repair only after user clicks `REPAIR IMPORT`.
- Source patch ledger is done for cleanup and import repair actions. It records action, primitive id, and evidence text; replay/undo semantics remain later.
- Delete point is done with source/preview refresh, minimum-point guard, and e2e proof.
- Sketch cleanup UX is done after point edit mechanics.
- Cleanup scope is deterministic source-profile cleanup: rough closed source profiles become clean editable source geometry with preserved primitive ids, source/projection/preview refresh, and local open-profile validation. It is not BRep reverse engineering, arbitrary model-to-sketch inference, or speculative agent magic.
- Model-to-sketch round trip now has a first accepted-BRep path: exact HLR projections can become derived editable sketch seeds with explicit provenance, preserving closed and multi-loop HLR loops when available and falling back to bounds. Rich topology reconstruction, hole intent, and feature semantics remain.
- Source authoring sketches and derived projection sketches remain distinct in copy and validation language.
- Raw backend/provider errors remain user-visible as failure evidence instead of generic API-key copy.
- BRep sketch validation remains later; current seed tranche records frontend/source/preview/projection/tolerance/readout/overlay evidence before BRep comparison exists.
- BRep validation now includes exact HLR from FCStd/STEP artifacts, BRep-derived sketch overlays, bounds checks, sampled containment checks, projected-loop hole mismatch checks, compact repair targets, bounded bounds-mismatch auto-repair, bounded containment envelope expansion, BRep-derived sketch conversion, topology/concavity redraw execution, first-pass arbitrary 3-view candidate cell reconstruction/search, exact front-profile prism selection for rectangular depth views including source front-profile holes, a backend accepted candidate-cell path with hard STEP gating, UI accept proof for STEP-backed accepted CAD evidence, UI reusable-package proof with explicit accepted-BRep ports, FreeCAD exact BRep edge/face targets, and Direct OCCT exact BRep edge/face targets in manifest/bundle topology surfaces. Richer exact loop/feature topology selection remains next.
- Arbitrary BRep reverse projection has a derived-seed convert path with multi-loop preservation. Rich topology reconstruction remains pending.
- BRep-derived projections for models without editable source/history are visible as derived/non-history seeds.
- Constraint solver seed landed after simple profile scaling, source-backed dimension locks, and readout evidence. Dimension snaps landed for exact size/origin controls. Source-backed dimension constraint validation landed; full BRep constraint validation remains next.
- BRep-derived projection validation seed, projected-loop hole mismatch checks, repair-target surfacing, bounded bounds auto-repair, bounded containment envelope expansion, derived sketch conversion, topology/concavity redraw execution, accepted-CAD repair Math Lens copy, candidate cell reconstruction/search, exact front-profile prism strategy with source front-profile holes, backend accepted candidate-cell STEP proof, candidate accept UI proof, accepted-BRep reusable package UI proof, FreeCAD exact edge/face target manifest/bundle export, and Direct OCCT exact edge/face target manifest/bundle export are done. Tolerance overlays and richer exact accepted-BRep loop/feature topology selection remain pending.
- Agent sketch/rebuild proposals, proposal cards, and repair from structured validation state stay later in Phase 9.
- Package/library work stays parked with assembly/fuse/mold/registry; this tranche stays limited to sketch source/replay, artifact evidence, session-configured backend choice, and simple precision controls.

### Phase 0: RFC And Data Contracts

Goal: lock vocabulary and file contracts before UI.

Deliverables:

- component manifest schema
- package manifest schema
- port schema
- mate schema
- assembly recipe schema
- operation schema
- validation result schema
- compatibility policy

Acceptance:

- sample package can be parsed
- sample component exposes params and ports
- sample assembly recipe can be validated structurally
- schema errors are specific

### Phase 1: Project Folder To Package

Goal: make user project folders shareable.

Deliverables:

- package writer
- package reader
- package header
- compressed payload
- checksum support
- preview support
- install/import into local library

Acceptance:

- a folder can be packed into one `.ecky` package
- package can be imported back
- public metadata can be read without unpacking full payload
- corrupt package gives exact validation error

### Phase 2: Component Manifest And Library Browser

Goal: make reusable components visible and inspectable.

Deliverables:

- component manifest extraction
- local library index
- component browser
- component preview
- exposed param controls
- package detail page

Acceptance:

- user can import package
- user can browse components
- user can see preview and params
- component model version, artifact, and session-configured backend metadata are preserved

### Phase 3: Built-In Ports And Mates

Goal: support first useful assembly workflows.

Deliverables:

- port frames
- plane mate
- axis mate
- bolt pattern mate
- dovetail rail/slot mate
- socket/insert mate
- mate validation
- assembly recipe output

Acceptance:

- bottle cage and rail can mate by dovetail
- bolt pattern can align to mount holes
- invalid port compatibility fails with exact error
- assembly can be saved and reopened

### Phase 4: Configurable User Port Types

Goal: avoid fixed-interface ceiling.

Deliverables:

- data-defined port templates
- data-defined compatibility rules
- user-created port type registry
- template validation
- no arbitrary code execution

Acceptance:

- user can define a custom clip/socket interface
- package can expose custom interface
- another package can declare compatibility
- invalid template fails safely

### Phase 5: Assembly Editor

Goal: make port/mate system usable.

Deliverables:

- assembly workspace
- component placement
- snap candidates
- mate picker
- transform solver
- visible mate errors
- save assembly recipe

Acceptance:

- user can assemble two components without manual coordinate entry
- failed mate shows reason
- saved assembly rebuilds deterministically

### Phase 6: Fuse And Cut

Goal: convert assemblies into production solids.

Deliverables:

- boolean operation recipe
- cut operation
- fuse operation
- operation order
- BRep validation
- export validation

Acceptance:

- two compatible components can fuse into one body
- cut operation can create matching socket or relief
- failed boolean reports raw backend error and context
- original component recipe remains editable

### Phase 7: Mold And Blend

Goal: support seamless generated transitions.

Deliverables:

- fusion zone metadata
- generated bridge solids
- blend radius rules
- keepout preservation
- post-fusion fillet/chamfer support
- validation overlays

Acceptance:

- components with fusion zones can create a smooth transition
- keepout violation blocks output
- blend radius outside feasible range fails clearly
- final export remains valid

### Phase 8: Sketch IR

Goal: represent user sketches as structured source data.

Deliverables:

- sketch schema
- planes/views
- points, lines, polylines, arcs, splines
- dimensions
- basic constraints
- source serialization

Acceptance:

- sketch can be saved in `.ecky`
- sketch can be reloaded and edited
- closed profile validation works
- constraint errors are specific

### Phase 9: Draft Mesh Seed

Goal: give immediate editable draft-form feedback before exact rebuild.

Deliverables:

- sketch to rough mesh path
- draft-form viewport/lens
- extrusion/revolve/loft preview
- ghost rendering
- silhouette overlays
- first region-local draft selection and status

Acceptance:

- user draws closed profile and gets a draft-form seed
- invalid sketch does not generate misleading preview
- preview updates interactively

### Phase 10: Exact Rebuild Seed

Goal: turn accepted sketch and draft steps into exact rebuild candidates.

Deliverables:

- feature generation from sketch and recognized draft regions
- extrude
- revolve
- loft
- shell
- cut
- hole
- chamfer/fillet hooks
- generated source view

Acceptance:

- user can create a simple exact candidate from sketch or recognized draft regions
- generated `.ecky` builds through backend
- source keeps sketch construction history and stays linked to draft state

### Phase 11: Shared Scene / Exact Validation

Goal: make sketch, draft, and exact state visible as one validated object.

Deliverables:

- shared scene contract
- correspondence graph
- exact-to-projection validation
- silhouette comparison
- tolerance settings
- required region checks
- forbidden region checks
- visible validation overlay

Acceptance:

- generated exact model can be checked against front/side/top intent and draft evidence
- failures highlight the offending region
- repair loop can target the failed feature or draft region

### Phase 12: Agent Patch Loop

Goal: let agents propose local scene patches instead of whole-model rewrites.

Deliverables:

- scene packet construction
- local sketch patch proposals
- local draft patch proposals
- exact rebuild proposals for selected regions
- proposal validation
- repair from structured validation state

Acceptance:

- agent can propose a local patch for a selected region
- invalid proposal is rejected by solver/validator
- accepted proposal updates the shared scene without discarding other representations
- repair loop uses structured validation state

### Phase 13: Public Registry

Goal: make packages discoverable and installable.

Deliverables:

- registry metadata
- package upload/download
- license display
- package signing
- compatibility filtering
- dependency support
- version resolution

Acceptance:

- user can publish package
- another user can install it
- dependency version conflicts are reported
- package interface is searchable without unpacking private payload

## Suggested Vertical Slice

Primary vertical slice:

1. Open Sketch workspace.
2. Draw or accept first front-view closed profile.
3. Preview mesh updates while profile is valid.
4. Progress ledger records sketch step as accepted or rejected.
5. Generate `.ecky` draft from accepted sketch.
6. Build through session-configured backend.
7. Validate build against sketch envelope.
8. Failed validation records raw backend/validator error.
9. Accepted result remains editable as sketch plus source.

This proves Ecky's core sketch-first loop.

Parked package slice:

1. Save accepted model as component.
2. Expose params and ports.
3. Pack `.ecky` archive with public header.
4. Import package.
5. Show minimal installed package card.

## Technical Risks

### Boolean Robustness

Booleans can fail with valid-looking geometry.

Mitigation:

- validate operation preconditions
- prefer BRep sources
- preserve operation order
- expose raw backend errors
- keep original components editable

### Topology Stability

Raw face and edge indices are not stable.

Mitigation:

- use named construction geometry
- use port frames and semantic references
- avoid long-term references to triangulated mesh faces

### User-Created Port Types

Arbitrary solver code is unsafe and hard to validate.

Mitigation:

- v1 user types are data-defined
- no arbitrary package code execution
- later plugin/WASM path with permissions and signatures

### Sketch Inference

Automatic inference from drawings is unreliable.

Mitigation:

- require explicit feature intent in v1
- support agent proposals as editable drafts
- validate every sketch and generated source step

### Package Privacy

Users may want one clean file without exposing messy internals.

Mitigation:

- public header exposes interface
- payload can be compiled or locked
- source packages remain available for open sharing
- private/encrypted packages can come later

## Open Questions

1. Should `.ecky` be both source extension and package extension, or should packages use `.eckypkg`?
2. Should compiled packages expose generated `.ecky` interface stubs?
3. Which artifact should be canonical for generated exact output: `.FCStd`, internal ExactModel / Solid IR, or both?
4. How much of STEP AP242 assembly semantics should be imported/exported?
5. Should custom port type definitions live inside packages, global registry, or both?
6. Should sketches remain visible in final public source packages by default?
7. What is the minimum viable sketch constraint solver?
8. Should registry packages require signing from day one?
9. How should package dependencies be versioned and cached?

## Architecture Direction

Keep the UI approachable, but make the scene model boring and inspectable:

```text
workspaceScene
  params
  sketchIntent
  meshDraft
  exactModel
  correspondenceGraph
  selection
  validations
  artifacts
  exports
```

The UI may feel like assisted CAD. Internally every accepted step should become source, correspondence, constraints, and validation evidence. The current `SketchDocument` -> generated `.ecky` -> Core IR path remains the exact-source spine until first-class `MeshDraftDocument` and `WorkspaceScene` contracts land.

This is the durable foundation for public reusable Ecky components.
