# Gemini Instructions & History

## Core Mandates
- **Payload Translation:** Frontend must use `camelCase`. Backend must use `snake_case`. Rust is responsible for translation via `#[serde(rename_all = "camelCase")]`.
- **Parsing:** Python macro parameters are parsed by the Rust backend using `rustpython-parser` AST.

## Recent Changes (2026-03-09)
- **Rust-powered AST Parsing:** Removed fragile JS regex parsing. Implemented `parse_macro_params` command in Rust that detects `params.get("key", default)` and explicit assignments.
- **Payload Normalization:** Applied `#[serde(rename_all = "camelCase")]` to all communication structs in `src-tauri/src/models.rs`.
- **UI Persistence Fix:** Reverted `invoke` calls to use standard `camelCase` arguments, fixing "missing key" errors in parameter updates and UI spec saving.
- **ParamPanel Improvements:** 
  - Added visual feedback (`APPLYING...`) and button locking during render.
  - Fixed parameter ghosting by replacing `localParams` instead of merging.
  - Implemented automatic state reset when switching design versions.
