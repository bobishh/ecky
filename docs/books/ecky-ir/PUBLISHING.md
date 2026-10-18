# Book sources, builds, and publishing

## Source ownership

The illustrated **Worm Farm** book starts at `introduction.md` and the six ordered
`missions/*.md` files: rake, house, garden, tractor, worm, farm. Stable filenames
retain their original numbers; `missionFiles` in `scripts/ecky_ir_content.ts`
defines reading order. Each chapter names its
complete example with an `<!-- example: relative/path.ecky -->` marker. Builders
insert those exact bytes into a code fence; edit the `.ecky` file once, rather
than maintaining two copies. Chapter image links resolve under `assets/`.

`examples/worm-farm/` holds six chapter models, two fit samples, and an assembled
tractor illustration source. `scripts/worm_farm_sources.ts` composes samples,
assembled tractor, and final farm from canonical chapter models; edit the chapter
models rather than generated copies. Synchronization checks catch drift. `assets/worm-farm/` holds
native CAD renders and source-hash manifests. Re-render after changing geometry.
The source links are packaged under content-hashed `/docs/examples/` filenames.
No online CAD service or AI image generator is needed to read or build the book.

`ecky-ir-corpus.md` remains the source for the human reference, bounded agent
reference, and extra engineering exercises. Its older six campaign levels are
supplementary practice. `levels/*.md` are generated projections. The older
`missions/level-*.md` files and `missions/manifest.edn` remain the application's
advanced guided workshop; their stable step identities preserve existing runs.
They are distinct from the new book in the app's Documentation → Chapters reader.

## Build graph

1. `npm run sync:book-source` projects corpus references and extra exercises,
   splits legacy levels, regenerates composed model sources, and assembles `public/tutorials/ecky-book.md` from the
   same source chapters the website uses. `check:book-source` detects drift.
2. `npm run build:book` performs synchronization, builds offline HTML/EPUB under
   `target/book/dist/books/`, and publishes EPUB, images and exact source downloads
   under `public/docs/`. The EPUB embeds the complete worked sources and images;
   extension download links point to the public website.
3. `npm run build:docs-site` builds reference pages and the six chapter pages
   under `target/book/dist/docs-site/`, including chapter images and downloads.
4. `npm run build` bundles the application. Its prebuild refreshes the book;
   `ChaptersReader.svelte` imports the generated book Markdown. Vite copies
   `public/` into the application output. Tauri additionally bundles `docs/` for
   the separate advanced guided workshop.
5. `npm run generate:docs` also refreshes Rust-generated agent prompts and the
   MCP tool reference. These require the Rust toolchain; EPUB/web builds do not.

To refresh illustrations, first build the current CLI (and prepare native OCCT
as described in the repository README):

```sh
cargo build --manifest-path src-tauri/Cargo.toml --features cli --bin ecky
npm run build:farm-assets
npm run generate:docs
npm run check:book-source
npm run check:mission-assets
npm run test:unit
npm run typecheck
cargo test --manifest-path src-tauri/Cargo.toml --test worm_farm_mechanics -- --ignored
cargo test --manifest-path src-tauri/Cargo.toml --lib worm_farm_render_executes_its_authored_verification -- --ignored
```

Browser proof uses the isolated docs server, never an existing browser session:

```sh
DOCS_E2E_PORT=43459 npx playwright test --config=sites/docs/playwright.config.ts
```

Choose an unused port. Server reuse is disabled; the suite starts and stops its
own server. HTML, image loads, source downloads, mobile reading, missing assets,
and EPUB contents are covered. CAD `check` validates compilation only; execute
native renders and runtime verification separately. Final analytic parts now
pass BRep validation before native exports; an invalid boolean result fails
with the part name, rather than producing a successful bundle. Generated
boolean edge inconsistencies can receive bounded repair at 1e-5 mm; existing
hybrid mesh/STEP output is covered by compatibility tests.

## Deployment verified 2026-10-13

Ecky has **not** been included in the neighboring Cloudflare frontend migration.
The infrastructure repository at `../hetzner_playground/cloudflare/README.md`
explicitly excludes Ecky. Its `config/deploy.ecky.yml` still builds this
repository's `Dockerfile`, deploys an nginx image through Kamal, and routes the
Ecky domain to port 80. Public `ecky-cad.com` resolved to `46.224.124.116` and
returned `server: nginx`; `/docs/` was HTTP 200. Those observations support the
existing origin deployment, not a Worker publishing path.

The Dockerfile builds the landing site, builds EPUB and docs from Markdown,
then copies the outputs to `/usr/share/nginx/html/docs/`. `nginx.conf` serves
chapter directories, exact source downloads, raw reference Markdown and EPUB.
No Cloudflare config should be invented merely because other sites migrated.

After reviewing and committing changes, publish only when deployment is
explicitly requested:

```sh
cd ../hetzner_playground
bin/kamal ecky deploy
```

This command changes production. A Git push alone does not deploy Ecky: the
Ecky repository's GitHub workflow runs checks and builds, with no release job.
After deployment, verify chapter pages, source/image responses and the EPUB on
the public domain. An HTTP header alone cannot prove which container digest is
running; inspect the deployment owner when an exact release identity is needed.
