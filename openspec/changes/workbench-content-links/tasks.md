## Implementation

- [x] Trace documentation, provider references, native exports, and voice binding ownership before edits.
- [x] Author outer scenarios and confirm missing download semantics with a failing focused check.
- [x] Unify content link routing and bundled documentation export.
- [x] Prove native save success/cancellation/failure and external Codex link routing.
- [x] Prove Code references in Chromium and WebKit, plus required structural/type/Rust checks.

## Evidence — 2026-10-18

- 15 focused scenarios per browser engine: Chromium and WebKit. Native IPC and audio
  transport are mocked; no live microphone or system browser is launched by tests.
- 42 focused frontend unit checks; 4 Rust unit checks and 25 persistence/architecture checks.
- Typecheck, production frontend/book build, strict specs, and book-source synchronization pass.
- All 15 published source-download paths contain exact public bytes in the production bundle.
