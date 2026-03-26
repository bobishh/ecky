# Model Runtime Roadmap

## Summary

This roadmap moves the app from a single-output STL renderer to a model runtime that supports:

- generated designs with durable artifact bundles
- `FCStd` import with partial semantic recovery
- object/part selection in the viewer
- LLM-assisted enrichment over deterministic extraction
- a later migration to `GLB` or `glTF` viewer assets

The roadmap separates delivery-track decisions from north-star ambitions.
It assumes the artifact strategy in `todos/model_artifact_strategy.md`.

## Current-State Baseline

Today the runtime behaves like this:

- backend render returns one STL path
- viewer consumes a single STL asset
- UI knobs come from macro/UI spec only
- no canonical `FCStd` artifact is persisted
- no manifest exists for stable semantic bindings
- no object/part picker exists in the viewer

This is adequate for preview and export, but not for model-runtime semantics.

## Command and Interface Decisions

### Current Contract

```text
render_stl -> string
```

Current meaning:

- backend produces a single STL artifact path
- frontend treats the STL as the model payload

### Target Contract

```text
render_model -> ArtifactBundle
```

Target meaning:

- backend returns one bundle that includes canonical truth and derived assets
- frontend stops treating STL as the only model representation

### Planned Commands

Add these eventual commands:

- `render_model`
- `import_fcstd`
- `get_model_manifest`
- `save_model_manifest`

Compatibility notes:

- `render_stl` may remain during migration as a thin compatibility wrapper
- preview STL may remain a derived field on `ArtifactBundle`
- existing UI can consume preview assets before full picker support lands

## Milestone 0: Contracts and Extraction Boundary

### Goal

Define the model-runtime boundary before changing the viewer or persistence model.

### Changes

- define `ArtifactBundle`
- define `ModelManifest`
- define `PartBinding`
- define `SelectionTarget`
- define `EnrichmentProposal`
- define deterministic extractor responsibilities in the FreeCAD backend/runner
- define the migration policy from `render_stl` to `render_model`

### Success Criteria

- runtime contracts are written and stable enough to implement against
- implementation work no longer assumes STL is canonical
- deterministic extraction responsibilities are clearly separate from LLM enrichment

### Blocked By

- none

### Explicitly Deferred

- viewer interaction changes
- import UX
- `GLB` runtime

## Milestone 1: Generated Designs Become Artifact Bundles

### Goal

Make app-generated designs persist richer artifacts without breaking the current rendering flow.

### Changes

- render macro through FreeCAD as today
- save `FCStd` alongside derived outputs
- export one viewer mesh per FreeCAD object or selected part
- emit `manifest.json`
- keep current knob model as the source of editable parameters for generated designs
- keep preview STL for the existing viewer and export flow

### Success Criteria

- every successful generated design produces:
  - `FCStd`
  - preview `STL`
  - per-object viewer assets
  - `manifest.json`
- generated models have stable `partId` and viewer node mapping
- generated models remain editable through existing knob logic

### Blocked By

- Milestone 0 contracts
- backend ability to save `FCStd` and enumerate document objects

### Explicitly Deferred

- import support
- `GLB` output
- automatic semantic inference beyond generated-model facts

## Milestone 2: `FCStd` Import

### Goal

Allow FreeCAD-native models to enter the runtime as first-class imported documents.

### Changes

- add `import_fcstd`
- extract object tree, placements, bounds, labels, and document metadata
- create a partial manifest for imported models even when no editable bindings are known
- run LLM enrichment on extracted structured facts only
- persist proposals separately from accepted manifest state
- require confirmation or explicit persistence of recovered semantics before treating them as durable bindings

### Success Criteria

- imported `FCStd` produces an `ArtifactBundle`
- imported models appear in the viewer as selectable parts
- imported models can exist with sparse or zero editable bindings
- enrichment proposals are attributable, confidence-scored, and non-authoritative

### Blocked By

- Milestone 1 artifact persistence
- deterministic extraction of object-level facts
- LLM prompt path for manifest enrichment

### Explicitly Deferred

- arbitrary macro import
- automatic acceptance of LLM proposals
- face-level editing

## Milestone 3: In-View Object/Part Editing

### Goal

Turn derived viewer parts into stable part-selection targets for the parameter UI.

### Changes

- viewer loads multi-part assets with stable ids from the manifest
- clicking a part focuses its parameter group
- generated designs route clicks to known knob groups
- imported models degrade gracefully when no editable binding exists
- UI distinguishes between:
  - editable part
  - inspect-only part
  - part with pending enrichment proposal

### Success Criteria

- click selection is object/part-based, not topology-based
- generated designs provide predictable focus behavior
- imported models never pretend to be editable when bindings are missing
- no core editing flow depends on face indices or mesh topology stability

### Blocked By

- Milestone 1 viewer asset mapping
- Milestone 2 partial manifests for imported models

### Explicitly Deferred

- face and edge picking
- stable topology naming repair
- direct sketch editing in the viewer

## Milestone 4: Ambitious Runtime

### Goal

Upgrade the viewer/runtime layer from delivery-track mesh parts to a richer scene-graph model.

### Changes

- introduce `GLB` or `glTF` as the preferred viewer asset
- preserve manifest-backed node ids and part mappings
- attach richer node metadata for selection and highlighting
- reuse scene graph structure across selection, overlays, and future annotations
- expose sketch/constraint-backed editing when present in generated models or imported FreeCAD documents
- keep `STEP` export as optional downstream artifact

### Success Criteria

- viewer no longer depends on a flat STL-only flow
- node metadata and selection are compatible with the manifest model
- richer object hierarchies can be surfaced without changing canonical truth
- `STEP` remains available for interoperability without becoming source of truth

### Blocked By

- mature manifest discipline from earlier milestones
- stable part/node ids in derived assets
- viewer migration work

### Explicitly Deferred

- arbitrary CAD interchange as first-class editable source
- full constraint editing for every imported model
- fully automatic semantic reconstruction from geometry alone

## Delivery Track vs North-Star

### Delivery Track

Ship these first:

- `FCStd + manifest.json` as truth
- preview STL for compatibility
- per-object mesh exports for viewer picking
- object/part-level selection
- LLM enrichment as assistive proposal generation only

Why:

- lowest-risk migration from the current codebase
- preserves current viewer investment
- gives immediate path to generated-design semantics and `FCStd` import

### North-Star

Build toward:

- `GLB` or `glTF` viewer assets
- richer scene graph and metadata
- tighter mapping between viewer nodes, document objects, and UI groups
- sketch/constraint-aware editing where available

Why:

- cleaner long-term viewer architecture
- stronger selection semantics
- better fit for overlays, annotations, and future per-part UX

## Acceptance Checklist

This roadmap is complete only if implementation work that follows it preserves these decisions:

- `FCStd + manifest.json` is the canonical source of truth
- STL is a derived preview artifact during migration
- first shipped scope includes generated designs plus `FCStd` import
- LLM output is enrichment over deterministic extraction, not replacement
- first in-view editing target is object/part selection
- imported models may be partially editable without being invalid
- arbitrary macro import remains deferred
