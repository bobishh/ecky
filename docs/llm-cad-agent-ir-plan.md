# LLM CAD Agent IR Plan

## Purpose

Make Ecky a structural CAD-agent system, not text regeneration pipeline.

Target:

```text
.ecky source
  -> Core IR
  -> stable edit identity
  -> semantic feature graph
  -> structural transforms
  -> verified render/export
```

This doc tracks work needed for local, low-entropy LLM edits:

- edit exact nodes, not whole source
- preserve topology intent
- transform features by semantic role
- validate constraints before backend execution
- optimize printable geometry without full model regen

## Current Baseline

| Area | Status | Current Evidence | Gap |
| --- | --- | --- | --- |
| Typed Core IR | Done | `CoreProgram`, `CorePart`, `CoreNode`, `CoreNodeKind`, `CoreValueKind` | IR is transient during compile/render, not persisted as design truth. |
| Stable identity | Partial v1 | stable AST node keys exist; Direct OCCT, FreeCAD, and build123d durable topology IDs prefer source-span stable keys | Geometry-signature identity and alias migration limits remain. |
| MCP AST read | Partial v1 | `ecky_ast_get` returns bounded Core nodes, structural paths, digests, spans | Read surface can expose paths that edit surface cannot write; nodes lack addressability metadata. |
| MCP AST edit | Partial v1 | `ecky_ast_replace_and_render` uses `sourceDigest` and `expectedNodeDigest` guards | Edit still slices source, recompiles, renders draft. Core-only and macro-expanded paths reject. |
| Parametric DSL | Partial v1 | `.ecky` params, typed defaults, min/max/step/choices, unit metadata | Unit arithmetic and richer constraint language are missing. |
| Semantic selectors | Partial | edge/face selector payloads and backend topology targets exist | Selectors are topology-id/geometric-clause only; no feature/role/port selectors. |
| Feature graph | Partial v1 | manifest has feature/correspondence graph contracts, part-level provenance, output refs, port contract fields, and component-render port population with provenance/confidence/target role | No operation-level provenance, role selectors, or general authored port/interface graph yet. |
| Supportless FDM optimizer | Partial v1 | `printability_analyze`, `printability_transform_recipes_get`, and content-hash-guarded reorient-only `semantic_transform_preview` exist | No thin-wall check, chamfer/split preview, or constraint-aware apply. |

## Critical Gaps From Audit

| Gap | Why It Blocks CAD-Agent Behavior | First Fix |
| --- | --- | --- |
| `NodeId` unstable | It is allocated by traversal order; insert/reorder can change IDs. Durable topology IDs currently include root `NodeId`. | Add `StableNodeKey`: part key + source path + op/binding name + source slice digest. |
| Read/write AST mismatch | `ecky_ast_get` emits `if`, `range`, `map`, `apply`, `list`, `group` paths that source resolver cannot edit. | Add `sourceAddressable`, `editableOps`, and rejection reason per AST node. |
| Persisted AST identity partial | `.ecky` runtime manifest carries `sourceDigest`, `coreDigest`, `astSchemaVersion`. | Expose identity in target detail flows and non-runtime edit surfaces. |
| No semantic feature graph | Manifest has targets and controls, not feature nodes with source refs and output target IDs. | Add feature/correspondence graph contracts before richer selectors. |
| Face target viewer selection | Viewer accepts face targets, renders overlays, and raycasts before mesh fallback. | Expand selection UX beyond first overlay slice and connect richer face semantics. |
| FDM optimizer lacks full anchors | Printability suggestions get a source anchor when feature graph has one clear feature. | Add per-risk source/face anchors; no auto-edit. |

## Architecture Invariants

- Human source stays `.ecky`.
- Agent edits must be digest guarded.
- Unsupported structural paths must reject before render.
- `NodeId` must not become public stable identity until stability proof exists.
- Source spans may guide edits, but cannot be sole identity contract.
- Backend errors remain raw and specific.
- Render/export claims must be artifact-backed.
- Text edit tools remain escape hatch until structural patches cover common edits.

## Work Tracks

Status meanings: `Done`, `Doing`, `Next`, `Pending`, `Blocked`, `Parked`.

### Track A: Stable Edit Identity

Goal: stable handles for agent edits across small source changes.

| Status | Slice | Acceptance Evidence |
| --- | --- | --- |
| Done | Keep AST read paths structural and digest guarded | `ecky_ast_get` emits path + digest; edits require expected digest. |
| Done | Add `StableNodeKey` contract | `ecky_ast_get` nodes include `stableNodeKey`; `NodeId` stays debug-only. |
| Done | Add addressability matrix | AST nodes report `sourceAddressable`, `editableOps`, and rejection reason when not editable. |
| Done | Add persisted source identity per committed version | `.ecky` runtime `ModelManifest` has `sourceDigest`, `coreDigest`, and `astSchemaVersion`. |
| Done | Replace topology durable root `NodeId` dependency | Direct OCCT, FreeCAD, and build123d durable IDs prefer source-span stable keys; old `node:<rootNodeId>` API remains fallback. |
| Next | Add source-addressable CST path layer | `ecky_ast_get` can expose editable Lisp paths for params, parts, build shapes, let bindings, call args, keywords. |
| Next | Add stable declaration IDs for params, parts, feature bindings | Reordering siblings preserves declaration identity where names are unchanged. |
| Pending | Promote `NodeId` only after deterministic recompile proof | Test proves unchanged source nodes keep same ID across unrelated sibling edits. |

First BDD slice:

- Given `.ecky` with one part, one `let`, one `if`, and one call arg
- When AST identity snapshot is requested
- Then every node includes `stableNodeKey`, `sourceAddressable`, and `editableOps`
- And `NodeId` is present only as debug metadata
- And non-editable nodes explain why

### Track B: AST-Native Edit Surface

Goal: move from source slicing toward source/CST/Core structural patch protocol.

| Status | Slice | Acceptance Evidence |
| --- | --- | --- |
| Done | Replace source-addressable node and render | `ecky_ast_replace_and_render` compiles replacement and renders draft. |
| Done | Support insert/delete/rename for common source forms | params, parts, build shapes, let bindings, call args/keywords covered by unit tests. |
| Done | Mark non-editable paths before mutation | `ecky_ast_get` reports non-addressable paths with reason before mutation. |
| Done | Add bounded source slices on demand | `includeSource=true` returns source-addressable node span/text with truncation metadata and never full `macroCode`. |
| Done | Split `validate_patch` from `render_patch` | `ecky_ast_patch_validate` validates replace patches, returns compact diff metadata, and does not render or create a draft. |
| Done | Return structured edit diff | Replace validation includes edited path, old/new digests, `affectedPaths`, `affectedPathDetails`, and no macro source payload. |
| Next | Add CST-backed scoped rename | Let/param/build rename updates only in-scope references, rejects shadowing ambiguity. |
| Pending | Core-only patch protocol | Direct Core patch possible when canonical source regeneration exists. |

First BDD slice:

- Given AST authoring enabled and source containing `if`, `map`, and `list`
- When agent reads AST
- Then unsupported edit paths return `sourceAddressable=false`
- And supported paths return allowed operations before any mutation tool is called

### Track C: Semantic Feature Graph

Goal: model meaning survives lowering and topology export.

| Status | Slice | Acceptance Evidence |
| --- | --- | --- |
| Partial | Export edge/face topology targets from exact backends | artifact bundle contains edge/face targets and selection targets. |
| Done | Add `FeatureGraph` / `CorrespondenceGraph` contracts | Manifest can store feature nodes, dependency edges, source refs, output target IDs. |
| Done | Add viewer face target selection | UI selects `SelectionTargetKind::Face`; Playwright covers face click before mesh fallback. |
| Next | Add explicit source feature declarations | `.ecky` accepts named feature wrapper or build binding metadata with role. |
| Partial | Persist feature provenance in manifest | `.ecky` runtime manifests include part feature graph provenance; read path backfills v0 from parts and selection targets. |
| Next | Add face selector predicates | role, normal, plane, bbox, area, loop, inner/outer, adjacency. |
| Next | Add edge selector predicates | adjacent face role, convexity, angle, direction, length/radius, loop membership. |
| Partial | Add feature graph port contracts | `FeatureNode` has optional ports with target IDs, frames, interfaces, params, provenance/confidence, and target role; manifest validation rejects unknown target IDs. |
| Partial | Populate ports and component interfaces on feature graph | installed component render maps validated `ComponentPort`s to `FeaturePort`s on matching feature nodes. |
| Partial | Add port provenance checks | Installed component ports populate `sourceRef`, `confidence`, and same-kind `targetRole`; mixed-role ports leave role unset. |
| Pending | Populate authored ports and component interfaces | `.ecky`/sketch/import authoring flows emit provenance/confidence/source target role, not only component-package ports. |

First BDD slice:

- Given accepted BRep model with sketch primitive and exported edge/face targets
- When artifact manifest is requested
- Then manifest contains a correspondence graph linking sketch primitive -> exact targets
- And graph omits no target IDs used by selected ports

### Track D: Constraint And Dependency Graph

Goal: evaluate intent before backend execution.

| Status | Slice | Acceptance Evidence |
| --- | --- | --- |
| Partial | Param constraints exist | min/max/step/choices on Core params. |
| Partial | Build Core dependency graph | `ecky_dependency_get` supports `/params/{key}` and returns Core reference paths. |
| Done | Add constraint result surface | `ecky_constraints_validate` returns pass/fail rows with param paths, raw values, and messages. |
| Partial | Add units | Param `:unit` metadata supports length, angle, ratio, count, text; no arithmetic type system yet. |
| Done | Add port provenance checks | Installed component ports carry provenance/confidence/source target role when target resolution is explicit and unambiguous. |
| Pending | Constraint-level sketch language | dimensions/locks become solved or rejected before preview. |

First BDD slice:

- Given param `fit_clearance` used by a snap feature
- When dependency graph is requested for `/params/fit_clearance`
- Then response lists dependent source paths and feature ids

### Track E: Supportless FDM Transform Planner

Goal: semantic transform `optimize for supportless FDM printing`.

Non-goal v1: global optimal manufacturing solver.

| Status | Slice | Acceptance Evidence |
| --- | --- | --- |
| Partial | Structural verifier has mesh metrics | STL triangle/connectivity/non-manifold/overhang estimate exists. |
| Done | Add read-only printability core | Reuses STL parser; returns overhang count/ratio, bbox, components, non-manifold facts. |
| Done | Add orientation scoring | Samples fixed rotations, recomputes overhang/footprint/height scores, returns ranked hints only. |
| Done | Add advisory transform suggestions | Printability analysis returns orientation, chamfer, and split suggestions as data only. |
| Partial | Add suggestion anchors | `printability_analyze` fills source anchors when feature graph has one clear feature. |
| Done | Add read-only transform recipes | `printability_transform_recipes_get` returns digest-guarded supportless-FDM recipe candidates with pending preview and unsupported apply status. |
| Done | Add preview-only reorient transform | `semantic_transform_preview` wraps simple `.ecky` part roots, renders a draft, and leaves chamfer/split unsupported. |
| Done | Harden transform guards | artifact digest and `semantic_transform_preview.expectedArtifact` require `contentHash` in addition to model and preview STL path. |
| Next | Broaden transform source support | reorient preview supports richer `.ecky` part forms; chamfer/split remain unsupported until source/feature anchors improve. |
| Pending | Constraint-aware optimization | optimizer respects locked dimensions, clearances, ports, and assembly mates. |

First BDD slice:

- Given model with unsupported 90-degree ledge
- When printability analysis runs
- Then report identifies overhang risk using current STL metrics
- And returns advisory orientation/chamfer/split candidates without editing source

## MCP Surface Target

Current tools stay:

- `ecky_ast_get`
- `ecky_ast_replace_and_render`

Add tools in order:

| Status | Tool | Purpose |
| --- | --- | --- |
| Done | `ecky_ast_patch_validate` | Validate replace patches, return source/core digest and diff metadata, no render. |
| Done | `ecky_ast_get(includeSource=true)` | Return exact bounded source slice for source-addressable nodes. |
| Partial | `ecky_dependency_get` | Return dependency paths for `.ecky` params. |
| Done | `ecky_constraints_validate` | Validate `.ecky` param constraints against provided/current/default values. |
| Done | `artifact_feature_graph_get` | Return model manifest feature/correspondence graphs plus artifact digest. |
| Done | `printability_analyze` | Return preview STL printability analysis and artifact digest. |
| Done | `printability_transform_recipes_get` | Return digest-guarded supportless-FDM recipe candidates without editing source or rendering. |
| Partial | `semantic_transform_preview` | Preview reorient supportless-FDM recipes for simple `.ecky` parts with required content-hash artifact guard; chamfer/split unsupported. |

## Milestones

### M1: Reliable Local AST Editing

Exit criteria:

- persisted `.ecky` runtime identity has source/core digest and AST schema version
- AST get exposes stable keys, addressability, and editable CST paths for common source declarations
- patch validate exists and returns structured diff
- rename has scoped reference evidence
- topology durable IDs no longer depend on traversal-order root `NodeId`
- cargo tests cover stale source digest, stale node digest, unsupported path, invalid replacement

### M2: Feature-Aware Manifest

Exit criteria:

- feature/correspondence graph contracts exist
- named feature declaration supported
- manifest carries feature provenance
- exact topology targets link to source features where backend exposes enough data
- viewer/context selection can surface feature id and face target selection
- tests cover one primitive feature and one boolean-derived feature

### M3: Dependency And Constraints

Exit criteria:

- dependency graph query exists
- param -> Core path dependencies visible through MCP
- constraint validation surface reports raw values and source paths
- param unit metadata available; arithmetic unit system still pending

### M4: Supportless FDM Preview

Exit criteria:

- printability analyzer produces source/feature-anchored risks
- transform suggestions are preview-only
- at least three advisory recipes exist: underside chamfer, orientation hint, part split suggestion
- before/after structural verification comparison returned

## Test Strategy

- Outer BDD first for user-visible MCP/UI flows.
- Rust unit tests for compiler, identity, patch validation, feature graph.
- Playwright only where Settings/UI/MCP interaction or viewer selection changes.
- Existing render tests prove backend artifacts.
- Every new MCP response gets compact-payload test: no full source unless explicitly requested.

## Open Decisions

| Decision | Options | Default |
| --- | --- | --- |
| Stable edit ID form | structural path + name, explicit source UUID, generated digest path | structural path + declaration name first |
| Feature syntax | `(feature id ...)`, build binding metadata, annotations | build binding metadata first, feature wrapper later |
| Core persistence | store full Core JSON, store digest + source, store compact sidecar | digest + schema first |
| Units v1 | parse suffix literals, param-only units, metadata-only units | param-only unit metadata landed |
| FDM transform apply | patch suggestions only, auto-apply preview, auto-commit | preview only |
| Topology durable ID | root `NodeId`, stable node key, feature graph ID | stable node key, then feature graph ID |

## Subagent Audit Log

| Agent | Scope | Status | Summary |
| --- | --- | --- | --- |
| Wegener | stable identity | Done | `NodeId` traversal-order unstable; topology durable IDs currently include root `NodeId`; add `StableNodeKey`, addressability matrix, topology aliases. |
| Pauli | AST persistence/edit surface | Done | AST tools gated and compact; source slicing exists; read surface emits non-editable paths; persist `sourceDigest`/`coreDigest`; add sourceAddressable metadata. |
| Curie | semantic feature graph | Done | Manifest lacks feature graph; face targets backend-exported but viewer selection incomplete; selectors lack feature/role/port semantics. |
| Meitner | supportless FDM optimizer | Done | Overhang metric exists and is non-blocking; v1 should be read-only printability analyzer plus orientation hints and patch recipes, no auto-edit. |
| Popper | topology durable IDs | Done | FreeCAD/build123d now prefer source-span stable node keys for durable edge/face target IDs with numeric root fallback. |
| Hume | supportless FDM recipes | Done | Added read-only MCP recipe tool returning artifact-digest-guarded reorient/chamfer/split candidates; no source edit or render apply. |
| Bacon | feature ports | Done | Recommended additive `FeatureNode.ports` contract with target IDs, frame, interfaces, params, source refs, confidence, and validation. |
| Tesla | AST source slices | Done | Added `ecky_ast_get(includeSource=true)` bounded source slices with UTF-8 safe truncation and non-addressable omission. |
| Leibniz | transform preview | Done | Audited safe v1: `.ecky` reorient-only source patch preview; artifact-only STL rotation unsafe without non-committable draft provenance. |
| Euler | semantic transform preview | Done | Added guarded reorient-only `semantic_transform_preview` that renders draft previews and rejects non-Ecky/chamfer/split. |
| Volta | feature graph ports | Done | Identified component render path as first low-risk port population surface; component ports now map into `FeatureNode.ports`. |
| Boyle | patch validate | Done | Audited replace-only `ecky_ast_patch_validate` plan: source/core diff without render or draft. |
| Ptolemy | AST patch validate | Done | Added replace-only `ecky_ast_patch_validate` with source/node digest guards, source-addressable splice, compile validation, and compact diff response. |
| Aristotle | transform guard | Done | Required `contentHash` in artifact digest and `semantic_transform_preview` guard; stale/missing hash rejects. |
| Poincare | port provenance | Done | Enriched component-render `FeaturePort`s with sourceRef, confidence, and same-kind targetRole. |
| Bohr | AST diff detail | Done | Recommended structured affected path details before exposing insert/delete/rename validate operations. |
| Schrodinger | AST diff detail | Done | Added `affectedPathDetails` to `ecky_ast_patch_validate` replace responses. |
| Hypatia | declaration identity | Done | Audited optional `declarationId` and recommended leaving `stableNodeKey` untouched; implementation deferred pending need. |
