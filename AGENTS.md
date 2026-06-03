# Agent Protocol

## Mandates
- **Tauri Boundary (Payload Translation):** 
  - **Frontend (Svelte/TS):** Always use idiomatic `camelCase`. Never use `snake_case`.
  - **Backend (Rust):** Always use idiomatic `snake_case`. Never use `camelCase`.
  - **Contract:** The Rust backend is responsible for translation. All boundary structs MUST use `#[serde(rename_all = "camelCase")]`. Tauri `invoke('cmd', { myArg: 1 })` arguments must be `camelCase` in JS to map correctly to `fn cmd(my_arg: i32)` in Rust.
- **NEVER COMMIT OR STAGE ANYTHING UNLESS ASKED FOR.** This includes `jj describe`, `jj commit`, `git add`, `git commit`, or any other source control operations that create a checkpoint or update a description.
- **Conventional Commits (when a commit IS requested).** Every commit message MUST follow the Conventional Commits spec (conventionalcommits.org): `<type>[optional scope]: <description>`. Types: `feat`, `fix`, `refactor`, `perf`, `docs`, `test`, `build`, `ci`, `chore`. Scope is the OpenSpec change or module when it helps (`feat(component-unification): ...`). A breaking change uses `!` after the type/scope (`refactor!: ...`) and a `BREAKING CHANGE:` footer explaining the migration. One commit = one logical change, mapped to the ticket being worked. Description is imperative, lower-case, no trailing period. Commit messages are always in English (subject and body), regardless of the chat language. Never add `Co-Authored-By: Claude` or any AI co-author line. Release Please consumes these messages to compute versions and the changelog, so they are a machine contract, not just prose.
- **Always verify Rust code** by running `cd src-tauri && cargo check` before reporting a successful implementation or restart.
- **Strictly adhere to the established UI theme** (Tactical Midnight, square borders, `--primary` / `--secondary` bronze accents).
- **Enforce layout boundaries**: All major layout containers must have `overflow: hidden` to prevent UI jitter and content bleeding.
- **Real Error Reporting**: Never use generic "Check API Key" messages. Always capture and display the raw error body from the backend/provider.
- **Persistence**: Any configuration changes made in the UI must be persisted to `app_config_dir/config.json` via the `save_config` command.
- **Tauri Invoke**: Reminder: Tauri expects `camelCase` in JS arguments, which maps to `snake_case` in Rust.
- **Agent UX**: Do not introduce a separate agent status bar or dump live auto-agent terminal output into app logs. Agent state belongs in Ecky bubble copy, and interactive agent stdout/stderr belongs in the dedicated terminal modal.
- **Repeated CAD Structures**: New repeated shelves/ribs/clips/doors/corridors must be authored with `repeat` or `instance`, not copy-paste shape blocks.
- **Physical Fit Relations**: Any new physical fit relation must be represented by a named constraint or named binding. No anonymous geometry offsets for fit-critical dimensions.
- **Debug Overlay Boundary**: Debug overlays are preview-only diagnostics. Never emit debug overlay primitives into production export geometry (STL/STEP).
- **MCP-First Authoring Order**: Follow `inspect -> validate -> preview -> commit` through MCP tools before claiming completion.
- **No SQLite Writes**: Never write SQLite files directly (`history.sqlite` or any app DB). All updates must flow through MCP commands.
- **AST Patch Preference**: Prefer AST patch operations over full macro rewrites when an `ecky_ast_*` patch can express the change.
- **No Junk Threads**: Do not create speculative `TMP`/throwaway threads or versions for debugging when an existing target thread can be inspected or forked. If the agent creates temporary history noise during a failed attempt, it must clean that noise up before stopping.

## Development Approach

Follow BDD dual-loop TDD. Every feature increment starts from a failing integration
test and is driven inward through unit-level red-green-refactor cycles.

### Outer loop (integration)

1. **Red (integration)** — Write one integration/acceptance test that describes the
next observable behavior from the outside in. Run it. Confirm it fails for the
reason you expect. Do not proceed until the failure message matches your intent.
2. **Inner loop (unit)** — repeat until the integration test can pass:
   - **Red** — Write the smallest unit test that expresses the next missing piece of
   implementation the integration test needs.
   - **Green** — Write the minimum production code to make that unit test pass.
   Run it in isolation and confirm. No speculative code.
   - **Refactor** — Clean up the code you just wrote (duplication, naming, structure)
   while all unit tests stay green. Only touch code covered by passing tests.
3. **Green (integration)** — When enough unit-level pieces exist, re-run the
integration test. If it still fails, diagnose which piece is missing and drop back
into the inner loop. Do not add code without a failing test driving it.
4. **Refactor (integration)** — With the integration test green, refactor across
module boundaries if needed. All tests — unit and integration — must stay green.
5. Repeat from step 1 with the next slice of behavior until the task is complete.

### Discipline rules

- Never skip the red step. If you cannot articulate why a test fails, you do not yet
understand the requirement.
- One logical change per cycle. If you are changing more than one behavior at a time,
split it.
- Run only the relevant test after each green step, then the full suite before each
commit-worthy checkpoint.
- If a refactor breaks a test, revert the refactor — do not fix forward.
- Treat a surprise failure (wrong message, wrong location) as information: re-read it,
adjust your understanding, then proceed.

## Tooling

### Svelte / Vite (frontend unit)
- **Run unit tests:** `npm run test:unit`
- Tests live under `src/lib/` alongside the code they test (`*.test.ts`).

### Playwright (e2e / integration)
- **Run all e2e tests:** `npm run test:e2e`
- **Run a single spec:** `npx playwright test e2e/my-spec.ts`
- **Run with UI:** `npx playwright test --ui`
- E2e tests live under `e2e/`. They spin up the full Tauri app.

### Rust / Tauri (backend)
- **Run Rust unit tests:** `cargo check` then `cargo test` (from `src-tauri/`)
- Integration tests in `src-tauri/tests/`.
- Use `#[cfg(test)]` modules for unit tests co-located with source.

### Dev server (needed for Playwright)
- `npm run dev` starts Vite + the Node server concurrently.
- Playwright is configured to start the app automatically via `playwright.config.ts`.

## Clarity

Work with persistence, clarity, and evidence.

## Documentation Hard Gate

If user asks for documentation, language reference, tutorial, guide, docs site, or
teaching material:

1. Before any implementation, agent MUST state:
   - content source format
   - file locations
   - serving path
   - frontend shell responsibility
   - test/proof path

2. Agent MUST NOT place substantive documentation content inside Svelte/React/Vue
components. Components may contain shell, nav, index, search, article loader, and
interaction wiring only.

3. Agent MUST build docs in this order:
   - information architecture
   - content storage format
   - file layout
   - serving path
   - index/manifest
   - frontend shell
   - interactive extras

4. Agent MUST treat mockup-shaped docs as failure unless user explicitly asked for a
mockup.

5. Agent MUST prefer file-backed docs such as `docs/**/*.html`, markdown, or JSON
content plus a manifest/index file.

6. If agent starts from layout before content model, that is a protocol violation.

## Variable Dump Rule

If task has more than 3 architectural or product-shape variables, agent MUST enumerate
them before coding.

Required format:

- Goal
- Artifact model
- Variables
- Decision
- Rejected paths
- Proof plan

`Variables` MUST list major moving parts explicitly, such as content format, storage
location, routing path, backend ownership, frontend ownership, editing model, testing
surface, export format, and runtime constraints.

Agent MUST NOT hide these decisions inside implementation.

## No Invisible Product Decisions

If agent is making product-shape decisions not explicitly requested, agent MUST list
them first.

Never silently invent:

- UI-first architecture
- mockups
- embedded content blobs
- temporary formats
- fake interaction

### Rules

1. Don't give up early.
Exhaust reasonable approaches before concluding you are blocked.

2. Ask only when asking is cheaper than checking.
If tools, docs, code, logs, or a quick experiment can answer it faster, do that first.
Ask the user only for preferences, missing external context, or decisions only they can make.

3. Verify assumptions.
Do not guess about behavior, versions, paths, configs, or API support. Check.

4. Don't loop.
If multiple attempts share the same core idea, stop and switch approaches.

5. Prove completion.
After changes, run the relevant test, build, request, or command. Evidence beats claims.

6. Finish the surrounding work.
Check for similar issues, regressions, dependencies, and edge cases before stopping.

7. If blocked, hand off responsibly.
State what you tried, what you ruled out, what remains unclear, and the best next step.

### Reset Checklist

When stuck, ask:

- What have I actually tried?
- What assumptions have I not verified?
- What source or docs have I not read directly?
- Am I repeating the same idea?
- What is the simplest different approach?
- Do I already have enough to act without asking?
- What evidence do I have?
