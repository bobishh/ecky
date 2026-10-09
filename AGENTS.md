# Ecky CAD

Read [README.md](README.md). Global working agreements cover BDD, browser consent, proportional verification, and VCS authorization.

## Contracts and ownership

- Tauri boundary: frontend TypeScript/Svelte uses camelCase; Rust uses snake_case. Boundary structs use `#[serde(rename_all = "camelCase")]`; JS `invoke` arguments use camelCase for Rust snake_case arguments.
- Config lives in `app_config_dir/config.edn`, written through `save_config` / `config_store::save_config`. `config.json` is import-only legacy input. Parse EDN as data; never evaluate it.
- Rust owns generation/exploration orchestration: PLAN -> BUILD -> VERIFY -> DECIDE, provider/render retries, budgets, ASK suspension, cancellation, latest-wins scheduling, restart recovery, and terminal decisions.
- Svelte may submit intent, answer, stop, provide requested viewport evidence, subscribe, and project backend state. Do not add frontend generation/retry/session state machines.
- API, MCP, watchers, and workbench converge on the same Rust controller and immutable version services. Callers must not manufacture lifecycle facts; owning Rust operations perform or validate them.
- Before architecture/lifecycle changes, read the applicable active OpenSpec proposal, design, normative specs, and tasks under `openspec/changes/`; update them with implementation.
- Until archived, `openspec/changes/exploration-build-cycle/` is authoritative for exploration. Read `proposal.md`, `design.md`, `specs/**/*.md`, and `tasks.md` before exploration changes. Superseded four-step, attempt/promotion, frontend-orchestrator, and caller-authored flows are historical.

## UI

- Preserve Tactical Midnight theme: square borders and `--primary` / `--secondary` bronze accents.
- Keep layout contained without hiding required content or controls. Apply overflow handling per pane; preserve scrolling where needed.
- Show actionable backend/provider error details rather than generic "Check API Key" messages. Redact credentials and private payloads before display.
- Agent state belongs in Ecky bubble copy. Interactive agent stdout/stderr belongs in the dedicated terminal modal; do not add a separate agent status bar or dump live terminal output into app logs.

## CAD authoring

- Use `repeat` or `instance` for repeated shelves/ribs/clips/doors/corridors.
- Represent fit-critical relationships with named constraints or bindings, not anonymous geometry offsets.
- Debug overlays are preview-only; never include them in STL/STEP production exports.
- Follow MCP `inspect -> validate -> preview -> verify` before claiming authoring completion. A changed persisted draft is already an immutable version; no promote/commit/finalize step exists.
- Prefer `ecky_ast_*` patches when they can express the requested source change.
- Avoid speculative TMP/throwaway threads or versions. Inspect or fork an existing target; preserve user history when removing task-created temporary artifacts.

## Tooling and proof

- Frontend unit tests: `npm run test:unit` (`src/lib/**/*.test.ts`). Component tests: `npm run test:component`.
- Browser checks: `npm run test:e2e` or `npx playwright test e2e/app.spec.ts`.
- `playwright.config.ts` starts Vite and Node server, not the native Tauri application. Set unused `PLAYWRIGHT_WEB_PORT`; inspect its second listener on 8787 and both `reuseExistingServer` settings before testing.
- Backend: run `cargo test` from `src-tauri/`. Run `cargo check` only when no current compile/runtime proof covers changed Rust code. An open dev process alone is not proof.
- `npm run dev` starts Vite + Node server; `npm run tauri dev` starts the desktop application.

## Documentation sources

- Chapters: `docs/books/ecky-ir/missions/*.md`; reference source: `docs/books/ecky-ir/ecky-ir-corpus.md`. Edit sources rather than generated copies under `public/`.
- `npm run sync:book-source` projects reference content; `npm run check:book-source` verifies synchronization. `npm run build:docs-site` packages web docs/examples; `npm run build:book` builds EPUB.
- Components own shell/navigation/search/loading; substantive docs remain file-backed. Verify content, linked examples, and serving path before adding interactive extras.

## Requested commits

- Follow Conventional Commits: `<type>[optional scope]: <description>`; types `feat`, `fix`, `refactor`, `perf`, `docs`, `test`, `build`, `ci`, `chore`.
- English subject/body; imperative lower-case description without trailing period. One logical change per commit; use relevant module/OpenSpec scope.
- Breaking changes use `!` and a `BREAKING CHANGE:` footer. Release Please consumes this format.
