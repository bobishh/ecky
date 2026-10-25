## Why

Ecky's component library blocks browsing on history recovery, stores discovery metadata in per-component files, and falsely rejects valid model-local definitions and loop bindings. Agents lack a reliable design-time path from a reusable model or component to a pinned package dependency. Existing archives, immutable payload storage and model-version locks provide a foundation, but there is no complete remote registry and CLI workflow.

Cairn turns reusable geometry and source into discoverable building blocks. A user or agent can find a candidate, inspect its exact contract, reuse or adapt it, and distribute a component collection through the owner's server.

## What Changes

- Persist compact model/component/package discovery records in SQLite; move history recovery and embedding work outside search requests.
- Correct dependency closure for supported model-local definitions, loop scopes and installed imports; migrate old failure cursors.
- Add bounded catalog retrieval with exact/text search and optional local semantic retrieval, including match reasons and immutable provenance.
- Require a search-and-selection step during PLAN before writing a new reusable geometry unit.
- Extend existing package/lock ownership with registry origin, archive integrity and exact dependency graphs; keep historical locks reproducible.
- Provide Cairn CLI commands for search, detail, package creation, publication, installation, frozen/offline use and explicit updates.
- Deploy a small versioned HTTP registry as a Docker service with persistent metadata/blobs, authenticated publication and immutable releases.

## Capabilities

### New Capabilities

- `component-catalog`: persistent discovery metadata, scope-correct capture, asynchronous recovery and query isolation.
- `design-retrieval`: model/component/package retrieval shared by CLI, MCP and the Rust design controller.
- `cairn-packages`: component collections, exact dependency installation, CLI workflow and canonical lock compatibility.
- `cairn-registry`: remote metadata discovery, immutable artifact publication/download and container operation.

### Modified Capabilities

No existing OpenSpec capabilities are registered. Existing runtime source, package and lock contracts remain authoritative and are explicitly preserved or extended in these new capability specifications.

## Impact

Affected owners: component extraction/compiler binding analysis; local catalog storage and Library projection; MCP discovery/detail tools; exploration PLAN guidance/context; package archive/store/import/lock services; CLI entry points; registry server and deployment configuration. Registry code must build without desktop UI or CAD geometry dependencies.

Research: [Cargo registry metadata](https://doc.rust-lang.org/cargo/reference/registry-index.html), [Bundler lockfiles](https://guides.rubygems.org/gemfile-lock/), [RubyGems compact caching](https://guides.rubygems.org/rubygems-org-compact-index-api/), [SQLite FTS5](https://www.sqlite.org/fts5.html), [hybrid retrieval](https://docs.vespa.ai/en/learn/tutorials/hybrid-search), [OCI distribution](https://github.com/opencontainers/distribution-spec/blob/main/spec.md). The detailed comparative report and execution measurements live outside Git in the configured deliverables directory.
