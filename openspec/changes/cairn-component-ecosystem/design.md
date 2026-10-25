## Context

The existing library captures successful rendered source into immutable component revision files. Browsing currently performs history backfill before scanning headers. The extractor understands top-level definitions and repeat bindings, but valid model-local component definitions and `for-union` indices can appear unresolved. A schema-2 cursor persists these failures.

Package archives already have a compact header and validated payload, a global content-addressed store, immutable coordinate records, native source/STEP imports, canonical model-version dependency locks and explicit GC roots. Cairn extends these contracts; it does not replace them with per-project package trees.

The measured corpus contains 159 visible threads, 2,673 successful model versions and 218 component-header files across revisions. Index latest reusable state and retain exact historical source by reference; do not embed every historical version.

## Goals / Non-Goals

**Goals:**

- Fast, reliable discovery independent of history decoding, source parsing and geometry execution.
- Design-time retrieval that leads to actual inspected reuse or justified adaptation.
- Reproducible component collections installable from the owner's server using CLI and MCP.
- One maintainable registry deployment and compatibility with existing models, archives and locks.

**Non-Goals:**

- A distributed package marketplace, arbitrary build-script execution or a general-purpose programming-language package manager.
- Running Docker on clients or introducing a vector database service.
- Silent package upgrades, remote package lookup during compilation or publishing local models merely because they were indexed.
- Claiming physical fit or mechanical safety from semantic similarity or package publication.

## Decisions

### 1. One package/reuse context with explicit ownership

The maintainer's shared model has six primary concepts:

| Concept | Authoritative owner | Relationship |
| --- | --- | --- |
| Component | Authored source/signature/ports | A package exports components; a captured revision links to a model version. |
| Model revision | Existing immutable version service | Catalog records point to its exact source and evidence. |
| Package release | Package service and immutable payload store | A coordinate identifies one payload; registry metadata cannot redirect an existing coordinate. |
| Dependency lock | Existing model-version lock service | Pins the resolved releases and their digests; the project file is its mirror. |
| Catalog | SQLite projection module | Derives bounded search metadata from exact source/release facts. |
| Registry | Cairn distribution service | Publishes and serves deliberate package releases; does not own local model history. |

A design plan remains owned by the existing Rust controller. Retrieval is a PLAN activity rather than another controller phase or frontend state machine. Normal explanations require at most three relationships: PLAN → catalog hit → exact source; CLI → release → verified store/lock; render → lock → immutable source.

### 2. Persisted catalog with background recovery

Store compact metadata, lexical indexes, capture checkpoints and recovery status in a SQLite catalog beside the component store. Immutable source remains file-backed. A one-time migration imports existing compact headers, then schedules history recovery. Do not import old error cursors as successful current checkpoints.

Capture parses source and writes verified immutable revisions outside the history writer lock. Publication rechecks that the source version is still the latest successful version while holding the short shared lock, then atomically publishes catalog metadata. A stale completion cannot replace a newer revision. Partial extraction failures retain independently usable candidates and report specific diagnostics.

Catalog search reads only SQLite. It does not enumerate history or source/header files, acquire the history writer lock, render, embed or fetch remote packages. The workbench immediately projects existing rows while a background worker reports processed/total counts. Compact invalidation events refresh the projection; retry starts backend-owned recovery. A crash or cancellation leaves the prior published snapshot usable.

The capture schema changes when binding/extraction rules change. Recovery reprocesses versions with obsolete checkpoints. Removed components and deleted projects must not remain falsely discoverable; immutable revisions referenced by old models remain readable.

### 3. Reuse units follow source semantics

Capture direct model-level `define-component` forms in the same supported scope as the compiler, including transitive helper/component dependencies. Treat `for-union` and `for-compound` indices as local to their body; evaluate loop counts in their outer scope. Preserve parallel `let`, sequential `let*`, parameter metadata, ports and verification clauses.

Installed source imports resolve through the existing host package resolver at the version's committed lock before capture. Catalog identity/provenance retains the authored model revision and any package origins. Extraction must not resolve an old model through a newer mutable package coordinate. A captured reusable source closure must compile in a fresh model without relying on the original model's implicit bindings.

### 4. Hybrid retrieval with bounded context

Index one descriptor per current reusable model revision, component revision or package export. Include purpose/description, names, parameters/units, ports, tags, language/backend, license/provenance and exact references. Keep arbitrary conversation text, full source and mesh payloads outside search responses.

Use exact identifier matching plus FTS5 token ranking and substring lookup. Configured semantic retrieval stores normalized vectors keyed by descriptor digest, embedding model identity and dimensions. Initially use a configured local Ollama embedding endpoint; absence or failure returns an honest lexical-only result. Reject non-finite vectors, dimension/model mismatches and stale descriptor embeddings.

At the present catalog size use exact cosine over compact persisted vectors. This is portable and testable without ANN training or extension lifecycle management. Add ANN only after measured corpus/latency demonstrates a need. Fuse lexical and vector lists with reciprocal rank fusion (`k = 60`) and stable identity tie breaks. Apply requested kind, backend and interface constraints before candidate ranking; similarity never overrides them.

Return at most eight default candidates with match reasons and revision references. Fetch source only after selecting a candidate. Descriptor/source text is untrusted evidence, not instructions. Semantic processing stays local unless an explicitly configured connection authorizes otherwise.

### 5. Shared discovery and a reuse-first PLAN contract

A common catalog search/detail interface serves the workbench, CLI and MCP. Preserve `component_search`/`component_get` compatibility; add a general search/detail surface for models, components and package exports. Results expose whether the catalog is rebuilding and whether retrieval used lexical or hybrid mode.

Before implementing a new geometry unit, PLAN identifies purpose and hard fit constraints, searches the catalog, inspects selected contracts/source, and records `reuse`, `adapt` or `noSuitableMatch` with references and a short reason. The controller supplies bounded candidate context to providers that do not call MCP directly. MCP agents receive the same workflow instruction and tools. Do not repeatedly search unchanged intent/candidate state during a repair loop.

Reuse imports a pinned component/release. Adaptation vendors explicitly chosen logic and retains provenance plus authored license/attribution. BUILD still appends an immutable changed version; normal compile, preview and verification establish whether the adapted result works.

### 6. Cargo/Bundler experience on existing package contracts

The CLI brand is Cairn. Provide `cairn` and `ecky cairn` entry points sharing one implementation: `search`, `info`, `init`, `pack`, `publish`, `add`, `install`, `update`, and catalog recovery/indexing commands. Machine-readable output is available through `--json`.

Use the existing package manifest/archive as the canonical published component collection. Extend it compatibly for exact package dependencies and provenance; it may export one component or many. Require explicit versions in dependency declarations initially. Installing a package resolves its complete bounded exact graph, rejects cycles/conflicting coordinates and verifies all bytes before discovery publication. Named explicit update is the only operation selecting changed versions.

Keep `ecky.lock.edn` as the canonical mirrored dependency lock. Extend its schema compatibly to record registry origin, archive integrity and graph roots/edges as needed. Existing version locks remain readable and are never mutated by install, preview, render or reopen. A frozen install verifies the existing lock and fails if declarations differ; an offline install succeeds only from matching cached payloads. Transport archive SHA-256 and canonical package payload digest are distinct fields and must never be interchanged.

Network operations occur in explicit package management, not compilation. Bundle/reexport resolution must preserve exact package origins; source materialization uses the existing AST-aware resolver and deterministic namespacing. No `node_modules`-style project copies. GC roots include installed coordinates, committed model locks and in-flight installation/render operations.

### 7. A small versioned HTTP registry, containerized once

Use one independently deployable Cairn registry with no desktop/geometry runtime dependency. Store metadata in SQLite WAL and immutable archives by SHA-256 on a local persistent volume. A small versioned JSON protocol is sufficient for the present single-maintainer catalog; OCI/Zot would still require a CAD metadata adapter. Preserve digest-oriented transport boundaries for future storage adapters.

The registry supports health, bounded search, per-package release metadata, digest-addressed download, authenticated publish and soft yank. Metadata supports ETag/If-None-Match. Package downloads are immutable and cacheable. Public reads serve intentionally published releases; bearer credentials authorize writes and are never returned or logged. The server rejects oversized/invalid uploads and different bytes for an existing coordinate. Identical publication is idempotent.

Stage and verify bytes before transactional metadata publication. A crash cannot advertise a missing/incomplete blob. Yank excludes a release from new selection without rewriting or deleting bytes required by locks. A consistent SQLite backup and immutable blob retention define recovery; multiple replicas must not share a SQLite file through a network filesystem.

The registry engine is an ordinary server binary with CLI/env/file configuration. Its container image is only a portable deployment wrapper: support Docker and Podman, non-root execution, a configurable unprivileged HTTP port and one externally mounted data directory. Require neither a container-engine socket nor engine-specific APIs, host networking or privileged mode. Document both container commands and direct binary startup.

Deploy through the existing Kamal infrastructure on the owner's server under a dedicated Ecky subdomain. Pin the built image to the source revision, persist the data volume, check health and verify a real CLI install from the public endpoint. Use new demonstration source for the initial published package; local private models are not bulk-published.

## Risks / Trade-offs

- Exact versions simplify reproducibility but require explicit upgrades. This matches current CAD contracts and avoids introducing a solver without a concrete need.
- A purpose-built small registry owns its HTTP contract. Keep it narrow, versioned and covered by real-process lifecycle tests rather than cloning an entire package ecosystem.
- Descriptor quality limits semantic retrieval. Preserve lexical retrieval and evaluate a labeled query set; do not call an embedding path effective merely because vectors exist.
- Two SQLite databases (history and derived catalog) are not one atomic transaction. Immutable source writes plus latest-version rechecks and recoverable catalog rebuilds explicitly bridge that seam.
- Existing archives can contain large geometry. Enforce bounded transfer, entry counts and expanded sizes; clients verify the full existing package contract before install.

## Validation

Use outside-in scenarios before behavior changes. Cover exact saved-source regressions without committing private designs, catalog query isolation under a held history writer, stale completion, recovery/repair, source closure compilation, MCP parity, lexical/hybrid ranking, package/lock compatibility and real HTTP publication/install failure cases. Measure cold/warm browsing and a 10,000-descriptor catalog outside Git. Locally verify the batched release before pushing; inspect only relevant CI failures and follow CI/deployment through completion with notifications or scheduled checks.

### 8. Versioned client archives

Distribute the two clients together as one versioned CLI archive for supported macOS/Linux architectures. The CLI distribution is independent of the registry server image. Package required native runtime assets for commands that render geometry; discovery/install/check commands must not require a desktop application to be installed. Publish checksums alongside archives. The installer selects the platform and architecture, downloads into a temporary directory, verifies integrity and stages the new version before switching the executable entry points. Preserve the previous installed version when any download, validation or staging operation fails.
