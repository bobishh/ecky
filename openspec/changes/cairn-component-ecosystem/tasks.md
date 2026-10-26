## 1. Research and contracts

- [x] 1.1 Research Cargo, RubyGems/Bundler, Nix, OCI/Zot, SQLite and hybrid retrieval using primary sources; save the comparative report outside Git.
- [x] 1.2 Inspect existing extraction, package/store/lock, MCP and PLAN ownership and establish representative catalog scale.
- [x] 1.3 Define Cairn proposal, capability scenarios and design decisions before extending implementation.
- [x] 1.4 Validate the OpenSpec change and keep contract changes aligned with the existing authoritative owners.

## 2. Reliable persisted catalog

- [x] 2.1 Add failing outside-in regressions for model-local definitions, loop scope and metadata-only queries.
- [x] 2.2 Correct extraction dependency closure, including locked installed imports.
- [x] 2.3 Persist compact metadata and checkpoints in SQLite with indexed lexical lookup and legacy migration.
- [x] 2.4 Move recovery outside query requests, preserve latest-version publication and expose progress/retry.
- [x] 2.5 Cover stale completion, removed/deleted entries, independent valid parts and exact source retrieval.
- [x] 2.6 Verify real saved dryer/bracket sources without committing private designs; record cold/warm and scaled measurements outside Git.

## 3. Design retrieval and MCP

- [x] 3.1 Add model/package descriptors and a shared bounded catalog search/detail contract while preserving component tools.
- [x] 3.2 Implement exact/text ranking, compatibility filters, match reasons and optional local embeddings cached by descriptor/model identity.
- [x] 3.3 Fuse lexical and semantic candidate lists with stable RRF and verify stale/missing embedding behavior.
- [x] 3.4 Add reuse/adapt/no-match selection to PLAN context and MCP authoring guidance with provenance and bounded candidate evidence.
- [x] 3.5 Verify keyword, exact, semantic-only, compatibility and untrusted-candidate scenarios.

## 4. Cairn packages and CLI

- [x] 4.1 Extend existing package and canonical lock contracts for registry origin, integrity and bounded exact dependency graphs.
- [x] 4.2 Implement shared Cairn/ecky command parsing and search/info/init/pack workflows.
- [x] 4.3 Implement publish/add/install/update with frozen/offline behavior and shared content-store ownership.
- [x] 4.4 Resolve component collections/dependencies and preserve exact import origins, old locks and native STEP behavior.
- [x] 4.5 Verify round trips, cycles/conflicts, corrupt archives, unsafe paths, interrupted installs and cache reuse locally.

## 5. Registry and publication

- [x] 5.1 Implement a desktop-independent versioned registry with SQLite metadata, immutable blobs and ETag caching.
- [x] 5.2 Cover authenticated/idempotent publication, coordinate conflicts, yanking, bounded uploads and crash recovery through real HTTP flows.
- [x] 5.3 Add a pinned Docker/Podman-compatible image, standalone binary usage, non-root volume/backup instructions and isolated container verification.
- [x] 5.4 Deploy through the existing server workflow with the task's infrastructure changes isolated from unrelated work.
- [x] 5.5 Publish a new low-poly stone demonstration collection and verify CLI discovery, frozen install and source compilation from the deployed endpoint.

- [x] 5.6 Serve a branded public Hub with shared landing assets, bounded source inspection and browser happy/failure checks.
- [x] 5.7 Configure DNS, then verify public CLI installation and local catalog discovery.
- [x] 5.8 Activate the Hub in the existing desktop profile and verify configuration and installed exports after restarting Ecky.

## 6. Release

- [x] 6.1 Update maintained English product/CLI/MCP documentation and generated contracts/references.
- [x] 6.2 Run appropriate local Rust, frontend and browser checks; fix failures before the batched release push.
- [x] 6.3 Commit and push verified task changes without rewriting unrelated commits; follow relevant CI and deployment checks to completion using notifications or scheduled checks.
- [x] 6.4 Reconcile final specifications and retain execution evidence outside Git.

## 7. CLI distribution

- [x] 7.1 Package `ecky` and `cairn` as versioned macOS/Linux archives with checksums and any required runtime assets.
- [x] 7.2 Provide a one-command installer that selects the platform, verifies checksums and preserves existing installations on failure.
- [x] 7.3 Verify installation from an isolated release directory and publish through the existing release workflow.
