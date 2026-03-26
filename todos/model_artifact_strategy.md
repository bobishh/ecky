# Model Artifact Strategy

## Summary

The current runtime treats `STL` as the practical output of generation.
That is sufficient for preview and export, but insufficient for durable semantics, object-level selection, or imported-model reuse.

This plan replaces the STL-centric mental model with a layered artifact model:

- `FCStd` is the canonical editable source of truth.
- `manifest.json` is the app-owned semantic layer.
- Viewer assets are derived outputs.
- `STEP` remains an optional interoperability export.
- LLMs enrich extracted facts, but do not define truth on their own.

This document locks the target representation and the invariants other plans should assume.

## Problem Statement

`STL` is a triangle mesh.
It is useful for preview and printing, but weak for in-place editing because:

- it does not preserve the FreeCAD object graph
- it does not preserve stable semantic part names
- it does not preserve sketch, constraint, or feature information
- triangle/face indices are unstable across regenerations
- imported models cannot be upgraded into reliable editing semantics from mesh data alone

The app needs a model runtime that supports:

- app-generated designs with durable edit semantics
- `FCStd` import with graceful partial recovery
- object/part-level picking in the viewer
- reusable community entry via FreeCAD-native files
- future semantic enrichment without making the LLM the source of truth

## Canonical Artifact Model

### Delivery Track

Use these artifacts for the first shipped milestone:

- `FCStd`
  - canonical editable source of truth
  - preserves FreeCAD-native document structure
- `manifest.json`
  - canonical app-owned semantic metadata
  - stores stable ids, bindings, warnings, and enrichment state
- per-object mesh assets
  - derived viewer assets for picking and rendering
  - keep the current viewer stack viable with minimal churn
- preview `STL`
  - derived preview/print output
  - not authoritative
- optional `STEP`
  - derived interoperability artifact
  - not authoritative

### North-Star Track

Move viewer assets toward:

- `GLB` or `glTF`
  - preferred rich viewer asset
  - supports node-level metadata, scene graph reuse, and cleaner picking

The north-star does not change the canonical truth decision:

- `FCStd + manifest.json` remains the authoritative pair

## Invariants

- Mesh assets are derived, never authoritative.
- `FCStd` is the only CAD-native source of truth for editable model state.
- `manifest.json` is the only app-owned source of truth for semantic bindings and interaction metadata.
- Imported models may be partially editable.
- Missing semantics for imported models are a valid state, not a failure of persistence.
- Topology-level references such as mesh faces or raw BRep face indices are not stable enough for v1 editing.
- v1 editing targets objects/parts, not faces/edges.
- LLM proposals must consume deterministic extraction output, not raw files alone.
- LLM proposals must include confidence and provenance.
- LLM output may enrich or propose metadata, but never silently replace deterministic facts.
- Existing STL preview behavior may remain during migration, but only as a derived artifact.

## Source Kinds

The runtime should distinguish at least these model source kinds:

- `generated`
  - produced by this app from a macro render
  - expected to have high-quality app-authored semantics
- `imported_fcstd`
  - imported FreeCAD document
  - expected to have deterministic extraction plus partial semantic recovery

Deferred source kinds:

- `imported_macro`
- `imported_step`
- arbitrary CAD interchange beyond `FCStd`

## Planned Public Interfaces

These are the interfaces that later implementation work should standardize.

### `ArtifactBundle`

Purpose:

- return one logical model payload from the backend
- replace the single-path `render_stl -> string` contract

Recommended shape:

```ts
type ArtifactBundle = {
  modelId: string;
  sourceKind: 'generated' | 'imported_fcstd';
  version: number;
  contentHash: string;
  fcstdPath: string;
  manifestPath: string;
  previewStlPath: string | null;
  stepPath: string | null;
  viewerAssets: {
    format: 'stl-parts' | 'glb';
    parts: Array<{
      partId: string;
      path: string;
    }> | null;
    glbPath: string | null;
  };
};
```

Required semantics:

- `fcstdPath` is always present once the bundle exists
- viewer assets are derived and may evolve in format without changing truth
- preview STL may remain nullable during import-only or migration states

### `ModelManifest`

Purpose:

- stable semantic layer owned by the app
- bridge between CAD document facts, viewer nodes, and UI editing

Recommended shape:

```ts
type ModelManifest = {
  schemaVersion: number;
  modelId: string;
  sourceKind: 'generated' | 'imported_fcstd';
  document: {
    title: string;
    sourceName: string | null;
    freecadPath: string;
    units: 'mm';
    hash: string;
  };
  parts: PartBinding[];
  sceneNodes: Array<{
    nodeId: string;
    partId: string;
    label: string;
    assetPath: string | null;
    boundsMm: [number, number, number, number, number, number] | null;
  }>;
  parameterGroups: Array<{
    groupId: string;
    label: string;
    parameterKeys: string[];
    partIds: string[];
  }>;
  selectionTargets: SelectionTarget[];
  warnings: string[];
  enrichment: {
    status: 'none' | 'proposed' | 'confirmed' | 'rejected';
    proposalIds: string[];
  };
};
```

Required semantics:

- stores only stable ids and extracted facts plus accepted enrichments
- does not use mesh face indices as durable identifiers
- allows imports to exist with sparse bindings

### `PartBinding`

Purpose:

- define the stable unit of v1 interaction
- map one semantic part to document objects and viewer nodes

Recommended shape:

```ts
type PartBinding = {
  partId: string;
  freecadObjectName: string;
  freecadLabel: string | null;
  kind: string;
  viewerNodeIds: string[];
  parameterKeys: string[];
  editable: boolean;
  source: 'deterministic' | 'llm-proposed' | 'user-confirmed';
};
```

Required semantics:

- `partId` is stable within the manifest
- `editable` may be false for imported or weakly understood parts
- generated designs should aim for `parameterKeys` completeness

### `SelectionTarget`

Purpose:

- define what the viewer can select in v1

Recommended shape:

```ts
type SelectionTarget = {
  targetId: string;
  kind: 'part';
  partId: string;
  viewerNodeIds: string[];
  focusGroupId: string | null;
};
```

Required semantics:

- v1 only uses `kind: 'part'`
- face/edge targets are intentionally deferred

### `EnrichmentProposal`

Purpose:

- persist LLM-produced suggestions without making them authoritative

Recommended shape:

```ts
type EnrichmentProposal = {
  proposalId: string;
  modelId: string;
  kind: 'label' | 'grouping' | 'binding';
  confidence: number;
  provenance: {
    extractorVersion: string;
    llmModel: string;
    createdAt: string;
  };
  payload: Record<string, unknown>;
  status: 'proposed' | 'accepted' | 'rejected';
};
```

Required semantics:

- proposals are reviewable and attributable
- confidence is advisory, not automatic authority

## Deterministic Extraction Responsibilities

The backend runner and extraction boundary should own:

- opening or saving the `FCStd` document
- enumerating document objects
- reading object names, labels, type ids, visibility, placements
- computing bounds, center, volume, or other cheap geometry summaries when available
- exporting derived viewer assets
- producing stable `modelId` and content hash
- constructing the initial manifest from deterministic facts

The deterministic extractor should not:

- invent semantic labels that are not present
- guess parameter intent from geometry alone
- create face-level editing bindings

## LLM Role

The LLM is assistive only.

It may:

- propose semantic labels for unlabeled parts
- group parts into higher-level controls
- infer likely parameter-to-part relationships from extracted facts
- generate user-facing descriptions and warnings

It may not:

- act as the sole parser of `FCStd`
- override deterministic extraction silently
- define canonical ids or object relationships

All LLM enrichment should be based on extracted structured facts such as:

- object tree
- labels
- type ids
- placements
- bounds
- sketch/constraint metadata if present
- existing knob schema for generated designs

## Editing Boundary for v1

The first in-view editing target is object/part selection.

That means:

- clicking a viewer node highlights a part
- the UI focuses the relevant knob group for that part
- generated designs use app-authored bindings
- imported `FCStd` models may expose inspectable parts with no editable bindings yet

Deferred:

- face selection
- edge selection
- topology naming repair across regenerations
- fully automatic constraint editing for arbitrary imports

## Non-Goals

This plan explicitly does not target:

- arbitrary macro import as a first-class input
- face/edge-level editing in v1
- LLM-only semantic recovery
- `STEP` as the canonical editable artifact
- viewer assets as the canonical source of truth

## Decision Lock

The implementation plans that follow this document should assume:

- first shipped scope includes app-generated designs plus `FCStd` import
- `FCStd + manifest.json` is canonical truth
- per-object mesh exports are the delivery-track viewer asset
- `GLB` or `glTF` is the north-star viewer asset
- object/part selection is the v1 editing boundary
- LLM semantics are proposals layered on deterministic extraction
