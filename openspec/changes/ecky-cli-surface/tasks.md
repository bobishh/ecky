# Tasks: Ecky CLI Surface

## Worker Rules

- Use subagents for disjoint write scopes only.
- No worker may revert unrelated edits.
- Workers must report changed files and tests run.
- Keep raw backend error detail intact.
- Run `cd src-tauri && cargo check` before claiming completion.

## 1. W1 - CLI Parser, Check, Lower

Write scope:

- `src-tauri/src/bin/ecky.rs`
- CLI parser/unit tests colocated with binary if added

Tasks:

- [ ] 1.1 Add `ecky` binary entry.
- [ ] 1.2 Implement usage/help text.
- [ ] 1.3 Implement `check` command with compile diagnostics.
- [ ] 1.4 Implement `lower` command with backend selection.
- [ ] 1.5 Implement `--out` handling and stdout fallback.
- [ ] 1.6 Implement exit-code mapping for usage/check/lower failures.

## 2. W2 - Render Orchestration

Write scope:

- `src-tauri/src/bin/ecky.rs`
- `src-tauri/src/services/render.rs` only if seam required

Tasks:

- [ ] 2.1 Parse `--param key=value` and `--params file.json`.
- [ ] 2.2 Merge parameter sources deterministically.
- [ ] 2.3 Route render to `build123d`.
- [ ] 2.4 Route render to `freecad`.
- [ ] 2.5 Route render to `direct-occt`.
- [ ] 2.6 Copy requested STL/STEP outputs and fail if missing.
- [ ] 2.7 Add optional `--json` render summary.
- [ ] 2.8 Preserve raw backend/runtime errors.

## 3. W3 - Proof, Docs, Smoke

Write scope:

- `README.md`
- CLI integration tests or smoke scripts
- OpenSpec task updates

Tasks:

- [ ] 3.1 Add README CLI examples tied to docs/tutorial flow.
- [ ] 3.2 Add proof for `check` happy/fail path.
- [ ] 3.3 Add proof for `lower` build123d/freecad path.
- [ ] 3.4 Add proof for one render backend with params.
- [ ] 3.5 Run `cd src-tauri && cargo check`.
- [ ] 3.6 Run targeted CLI tests/smokes.

## 4. Main Thread Integration

Tasks:

- [ ] 4.1 Review worker patches for overlap/regression.
- [ ] 4.2 Re-run CLI-targeted tests after integration.
- [ ] 4.3 Update tasks as slices land.
- [ ] 4.4 Leave packaging/global install for later change.
