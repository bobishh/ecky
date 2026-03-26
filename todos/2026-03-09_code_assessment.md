# Drydemacher Code Assessment — 2026-03-09

## Overall Grade: **B-** (72/100)

A genuinely creative and well-conceived product (prompt-driven CAD via LLM + FreeCAD) with solid architecture for a v0.1. The codebase is functional and demonstrates clear thinking about UX flows (multi-microwave concurrency, repair loops, intent classification). However, it suffers from several structural issues that will compound as the project grows.

---

## Scorecard

| Category                      | Score | Weight | Notes |
|-------------------------------|-------|--------|-------|
| **Architecture**              | 7/10  | 20%    | Clean separation: Tauri commands → domain logic → DB. Frontend stores are well-decomposed. |
| **Code Quality**              | 6/10  | 20%    | Inconsistent typing (untyped JS in `.ts` files), some dead code. Good Rust quality. |
| **Correctness / Bugs**        | 6/10  | 15%    | Active bug: `deleteVersion` uses snake_case `message_id` (now fixed). Silent failures possible in several places. |
| **Error Handling**            | 7/10  | 10%    | Raw error bodies surfaced per AGENTS.md mandate. Some `let _ =` swallowing in Rust. |
| **Test Coverage**             | 7/10  | 10%    | 36 Rust unit tests (all passing), 10 E2E spec files. No frontend unit tests. |
| **Security**                  | 5/10  | 10%    | API keys stored in plaintext JSON config. Gemini key passed in URL query param. |
| **Performance**               | 7/10  | 5%     | STL cache with SHA256 dedup is smart. Render lock prevents FreeCAD contention. |
| **DX / Maintainability**      | 6/10  | 5%     | Good README, clear module boundaries. Lack of TypeScript types in stores/controllers. |
| **UI/UX Code**                | 8/10  | 5%     | Polished theme, good component decomposition, accessible keyboard handlers. |

---

## Critical Issues

### 1. **BUG: `deleteVersion` invoke used snake_case key** (FIXED)
- **File:** `src/lib/stores/history.ts:75`
- **Impact:** Delete version button was completely non-functional.
- **Fix:** Changed `{ message_id: messageId }` → `{ messageId }` (Tauri camelCase convention).

### 2. **API Keys in Plaintext + URL Params**
- **Files:** `src-tauri/src/llm.rs:246,345`, `src-tauri/src/commands/config.rs:17`
- **Impact:** Config stored as plaintext JSON. Gemini API key appears in URL query parameters, visible in logs/crash reports.
- **Recommendation:** Use OS keychain (Tauri plugin) for API key storage. For Gemini, use `x-goog-api-key` header instead of query param.

### 3. **`std::sync::Mutex` on DB connection in async context**
- **File:** `src-tauri/src/models.rs:149`
- **Impact:** `state.db.lock().unwrap()` inside `async` commands can block the Tokio runtime. Under concurrent load (4 LLM requests), this creates potential deadlocks or starvation.
- **Recommendation:** Replace `Mutex<Connection>` with `tokio::sync::Mutex<Connection>` (already used for `render_lock`), or better yet, use an `r2d2` connection pool.

---

## High-Priority Improvements

### 4. **Untyped Frontend Stores & Controllers**
- **Files:** `src/lib/stores/history.ts`, `src/lib/stores/workingCopy.ts`, `src/lib/controllers/*.ts`
- All store functions use `any` or omit parameter types entirely (`loadVersion(msg)`, `createNewThread(payload)`).
- Define interfaces for `Thread`, `Message`, `DesignOutput`, `WorkingCopyState` on the frontend.

### 5. **Duplicated LLM Client Code**
- **File:** `src-tauri/src/llm.rs`
- `call_openai_compatible` / `call_openai_json_object` share ~90% of their logic. Same for the two Gemini functions.
- Refactor into a single `call_provider(format: ResponseFormat)` with an enum discriminator.

### 6. **Duplicated "save last design" Pattern**
- **Files:** `commands/generation.rs:204-211`, `commands/design.rs:78-85`, `commands/design.rs:111-118`
- Three places write `last_design.json`. Extract into a shared `persist_last_design(state, app, design, thread_id)` helper.

### 7. **Silent Error Swallowing in Rust**
- Multiple `let _ = ...` statements that discard errors:
  - `lib.rs:387` — `migrate_legacy_references` failure silently ignored on boot.
  - `commands/generation.rs:195` — `persist_thread_summary` failure ignored.
  - `commands/design.rs:52` — same.
- At minimum, log these with `eprintln!` or `tracing::warn!`.

---

## Medium-Priority Improvements

### 8. **No Frontend Unit Tests**
- 10 E2E Playwright specs cover UI flows, but zero unit tests for stores (`requestQueue`, `workingCopy`, `sessionStore`) or controllers (`requestOrchestrator`, `manualController`).
- The `GenerationPipeline` class especially needs unit tests — it has complex state transitions and cancellation logic.

### 9. **`commit_generated_version` is Dead Code**
- **File:** `src-tauri/src/commands/generation.rs:220-290`
- Not registered in `invoke_handler` in `lib.rs`. The flow now uses `init_generation_attempt` + `finalize_generation_attempt`. Remove it.

### 10. **Viewer Doesn't Clean Up `resize` Listener**
- **File:** `src/lib/Viewer.svelte:76`
- `window.addEventListener('resize', onResize)` is never removed in `onDestroy`. Memory leak on component remount.

### 11. **`answer_question_light` is Dead Code**
- **File:** `src-tauri/src/commands/generation.rs:347-408`
- Not registered in `invoke_handler`. Remove or register it.

### 12. **Hardcoded Mac FreeCAD Path**
- **File:** `src-tauri/src/freecad.rs:108`
- Only checks macOS path. Needs Windows/Linux fallback paths.

### 13. **Missing `overflow: hidden` on Some Containers**
- Per AGENTS.md mandate, all major layout containers must have `overflow: hidden`.
- `.dialogue-content` (`App.svelte:439`) has no overflow constraint — long dialogue trails can cause jitter.

---

## Low-Priority / Polish

### 14. **`reqwest::Client` Created Per-Request**
- **File:** `src-tauri/src/llm.rs:21,34,109`
- Each LLM call creates a new `reqwest::Client`. Store a shared client in `AppState` for connection pooling.

### 15. **`chrono` Dependency Unused**
- **File:** `Cargo.toml:21`
- All timestamps use `std::time::SystemTime`. Remove `chrono` from deps.

### 16. **`url` Dependency Unused**
- **File:** `Cargo.toml:22`
- Not imported anywhere. Remove.

### 17. **`MAX_CONCURRENT_LLM` Not Enforced**
- **File:** `src/lib/stores/requestQueue.ts:49`
- `MAX_CONCURRENT_LLM = 4` is declared but never checked. The orchestrator fires pipelines without throttling.

### 18. **Image MIME Sniffing Too Simplistic**
- **File:** `src-tauri/src/llm.rs:262-276`
- Only handles `data:image/jpeg` and `data:image/png` prefixes. WebP, GIF, TIFF silently dropped.

### 19. **`setMuted` Function Incomplete**
- **File:** `src/lib/audio/microwave.ts:337-340`
- Comment says "Actually stopMicrowaveAudio closes context by default" — the function doesn't work as intended for mute toggle. It kills the context instead of pausing.

---

## Strengths Worth Preserving

1. **STL cache with SHA256 content-addressing** — elegant, avoids re-rendering identical code+params.
2. **Concurrent request "cafeteria" metaphor** — delightful UX for multi-request workflows.
3. **Intent classification pipeline** — smart light-model routing before committing to heavy generation.
4. **Thread summary / pinned references context system** — keeps LLM prompts focused without overwhelming token budgets.
5. **Comprehensive E2E test suite** — 10 spec files covering core flows.
6. **36 Rust unit tests all passing** — good coverage of parsing, context building, and DB operations.
7. **Repair loop with retry** — automatic FreeCAD error recovery up to 3 attempts.

---

## Recommended Next Actions (Priority Order)

1. ~~Fix `deleteVersion` camelCase bug~~ ✅ Done
2. Move API keys to OS keychain
3. Replace `std::sync::Mutex<Connection>` with `tokio::sync::Mutex` or connection pool
4. Add TypeScript interfaces for domain types on frontend
5. Deduplicate LLM provider call functions
6. Remove dead code (`commit_generated_version`, `answer_question_light`, unused deps)
7. Add frontend unit tests for stores/controllers
8. Fix Viewer resize listener leak
