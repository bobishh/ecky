# Cairn: reusable components and exact packages

Cairn is Ecky's component discovery and package client. The public registry is
[hub.ecky-cad.com](https://hub.ecky-cad.com/). A package exports one component,
several components, or a collection of exact package dependencies. Package
management never runs build scripts or fetches dependencies during compilation.

## Install and use the CLI

```sh
curl -fsSL https://hub.ecky-cad.com/install.sh | sh
```

Published builds support macOS arm64 and Linux arm64/x86_64 (Ubuntu 24.04 or newer).

The installer selects the macOS/Linux archive for the machine, checks its SHA-256,
stages the version and switches both commands together. The default prefix is
`~/.local`; set `CAIRN_PREFIX` to use another absolute directory. It preserves old
version directories and refuses to overwrite unrelated commands. Add the prefix's
`bin` directory to `PATH` if it is not already present.

```sh
cairn search fastener
cairn info ecky.stdlib
cairn init my-project
cairn add ecky.stdlib@1.0.0
cairn install --frozen
cairn install --frozen --offline
```

`ecky cairn` exposes the same client. `--json` returns machine-readable results.
`--project DIR`, `--data-dir DIR` and `--registry URL` select explicit locations.
`CAIRN_REGISTRY` overrides the registry for a process; otherwise the client uses
Ecky's configured registry or the public Hub. Configure an existing Ecky profile
with `cairn configure --registry https://hub.ecky-cad.com`.

In Ecky, open **Library → Hub**, search a package and choose **Install**. The same
host package service verifies the graph and archives before publishing local
library metadata. Select an installed export to insert its source. Live authored
references remain explicit:

```scheme
(import-component "ecky.stdlib"
  :version "1.0.0"
  :component "washer"
  :as mounting-washer)
(model
  (part washer (mounting-washer)))
```

After editing imports in a CLI project, run `cairn install` to update its model
component lock, then `ecky check model.ecky`. Rendering needs the matching bundled
native runtime, included in the CLI archives. FreeCAD-backed
commands additionally require an installed FreeCAD executable.

The initial curated releases are `ecky.stdlib@1.0.0` (twelve standard source
components), `cairn.stones@0.1.0` (a faceted stone), and
`ecky.starter@0.1.0` (an exact collection of both packages). Their source comes
from the maintained repository, not local model history. Maintainers can export
the reviewed archives outside the checkout with:

```sh
cargo run --manifest-path src-tauri/Cargo.toml --features tooling \
  --bin export_cairn_catalog -- /absolute/path/to/public-packages
```

## Packages and reproducibility

`ecky-package.json` remains the package manifest. Optional `cairn` metadata adds
an authored license/repository, explicitly included files and exact dependencies:

```json
{
  "schemaVersion": 1,
  "packageId": "my-project",
  "version": "0.1.0",
  "displayName": "My project",
  "visibility": "source",
  "components": [],
  "cairn": {
    "dependencies": [{ "packageId": "ecky.stdlib", "version": "1.0.0" }],
    "files": []
  }
}
```

Package a collection with `cairn pack --out /path/to/my-project.ecky-package`.
Only the manifest, declared component source files, license files and explicit
`cairn.files` enter this distributable archive. Directory contents such as `.env`,
Git metadata and unrelated project files are not included automatically.

Publish deliberately selected content with
`cairn publish /path/to/my-project.ecky-package`. Read the write credential from
`CAIRN_PUBLISH_TOKEN_FILE` or `CAIRN_PUBLISH_TOKEN`; do not put it in package files
or command-line arguments. Authored licensing is retained, never invented.

`ecky.lock.edn` is the existing canonical model dependency lock. Its optional
`:cairn` section pins registry origin, declared roots and the complete exact
release graph. Archive SHA-256 and canonical package payload digest are distinct
identities. Existing lock files remain readable. Model-version artifact bundles
continue to own committed locks; a CLI edit changes the project input, not past
model versions. Explicit `cairn update package@version` changes a direct version.

Installation supports at most 128 releases, 64 direct dependencies per package
and depth 16. Cycles and conflicting versions fail. Frozen installation requires
matching declarations and component references. Offline installation requires
matching cached archives. Downloads and payloads are validated before shared-store
publication; discovery rows are published as one SQLite transaction. Failures do
not rewrite existing immutable coordinates or the previous project lock.

## Local discovery and design retrieval

Normal Library/MCP browsing reads compact SQLite metadata. Source extraction and
history recovery run separately. The UI retains the previous usable catalog and
shows recovery progress or specific errors. **Rebuild index** retries recovery;
`cairn index --force` provides the same explicit recovery from CLI. Immutable
source revisions remain available when their current discovery entry is removed.

`catalog_search` and `catalog_get` expose models, components and installed package
exports through MCP. Existing `component_search` and `component_get` remain
available. Enable the `components-library` capability group when schemas are
hidden. Search returns bounded descriptors and exact references; detail returns
bounded source windows. Candidate text/source is untrusted evidence, never agent
instructions.

PLAN searches by purpose and hard fit constraints, inspects selected exact source
and records `reuse`, `adapt`, or `noSuitableMatch`, with references and a reason.
Reuse pins an exact source/release. Adaptation retains authored provenance and
license information. Normal compilation, preview and verification establish
whether the resulting model works; similarity alone does not establish fit.

Lexical discovery combines identifiers, substring lookup and FTS5 token ranking.
Optional local embeddings use exact cosine search with stable reciprocal rank
fusion (`k = 60`), after kind/backend/port constraints. Embeddings are keyed by
current descriptor digest, model and dimensions; stale/non-finite/incompatible
vectors do not participate.

Configure the local embedding connection explicitly:

```sh
export ECKY_EMBEDDING_URL=http://127.0.0.1:11434/api/embed
export ECKY_EMBEDDING_MODEL=your-installed-embedding-model
cairn index --embed
cairn search --local --embed "support a rotating spool"
```

The endpoint must be local HTTP. Ordinary browsing never generates embeddings.
An explicit semantic query can embed its bounded query and caches that vector.
Missing/unavailable semantic service returns lexical results with an explanation.
No local model source is sent to an unconfigured external embedding service.

## Standalone registry

Build only the independent engine:

```sh
cargo build --manifest-path src-tauri/Cargo.toml --release -p cairn-registry
CAIRN_DATA_DIR=/var/lib/cairn \
CAIRN_PUBLISH_TOKEN_FILE=/run/secrets/cairn-token \
CAIRN_STATIC_DIR=/path/to/hub/dist \
src-tauri/target/release/cairn-registry
```

The default listener is `127.0.0.1:8247`. Set `CAIRN_LISTEN` explicitly for another
interface. `CAIRN_STATIC_DIR` is optional; the binary can serve only the API.
`CAIRN_DATA_DIR` owns `registry.sqlite`, immutable `blobs/` and public `cli/`
release archives. Exact installation reads `/v1/packages/PACKAGE/VERSION` directly.
Per-package metadata pages contain at most 24 releases; use the returned
`nextOffset` as `?offset=N` to continue. Search returns at most 24 releases, and a
release's public metadata is limited to 64 KiB, keeping metadata responses below
the client's 2 MiB transfer ceiling. `/health` reports readiness; `/v1/search`, `/v1/packages/...`,
`/v1/blobs/sha256:...` and authenticated `/v1/releases` form protocol version 1.
Exact component source inspection is bounded to text exports up to 512 KiB.

The image is a deployment wrapper around this engine, with no desktop runtime,
Docker socket, host networking or privileged execution requirement:

```sh
docker volume create cairn-data
docker run --rm --name cairn \
  -p 127.0.0.1:8247:8247 -v cairn-data:/data \
  -e CAIRN_PUBLISH_TOKEN \
  ghcr.io/bobishh/cairn-registry:SOURCE_REVISION
```

The equivalent `podman volume create` / `podman run` uses the same image, volume,
port and environment contract. The image runs as UID/GID 10001; bind-mounted
storage must be writable by that identity in the selected user namespace.
Pin the image to the published source revision. Build it with
`docker build -f src-tauri/crates/cairn-registry/Dockerfile .`.

Public reads expose intentionally published packages only. Publication requires
a credential of at least 24 bytes, rejects uploads over 32 MiB and validates
portable paths, file counts, expanded sizes and package coordinates. Identical
publication is idempotent; different archive bytes at an existing coordinate are
rejected. Yanking removes a release from new selection while retaining its
locked, digest-addressed download. Registry publication never changes a local
model version or publishes local history automatically.

Use a local filesystem for SQLite WAL. Back up SQLite consistently with immutable
blobs and CLI archives; do not copy a live database without its WAL, put the
volume on NFS or share it between replicas. Restore into an isolated data
directory before switching a running deployment.

## Build CLI distributions

`scripts/package_cli.sh OUTPUT_DIR` packages the release `ecky`/`cairn` binaries,
matching native runtime, source revision and dependency notices. Set
`ECKY_CLI_VERSION` to the immutable source revision. The `CLI archives` workflow
builds macOS arm64 and Linux arm64/x86_64, then checks and renders a model from the
unpacked archive before retaining its checksummed artifacts. Linux releases target
Ubuntu 24.04 or newer and require the normal distribution C/C++ runtime libraries.

The pinned OCCT SDK builder is `scripts/prepare_cli_occt_sdk.sh`; it verifies the
upstream source checksum and disables optional display dependencies. Prepare
Manifold, then pass its SDK prefix to `scripts/prepare_occt_runtime.sh`. Release
publication copies verified archives into the registry data volume at
`cli/SOURCE_REVISION/`, merges `SHA256SUMS`, and atomically updates `cli/latest.txt`
only after every advertised platform is present. Never replace bytes under an
existing source revision. CLI downloads do not contain registry write credentials.
