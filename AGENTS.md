# Agent Protocol

## Mandates
- **Tauri Boundary (Payload Translation):** 
  - **Frontend (Svelte/TS):** Always use idiomatic `camelCase`. Never use `snake_case`.
  - **Backend (Rust):** Always use idiomatic `snake_case`. Never use `camelCase`.
  - **Contract:** The Rust backend is responsible for translation. All boundary structs MUST use `#[serde(rename_all = "camelCase")]`. Tauri `invoke('cmd', { myArg: 1 })` arguments must be `camelCase` in JS to map correctly to `fn cmd(my_arg: i32)` in Rust.
- **NEVER COMMIT OR STAGE ANYTHING UNLESS ASKED FOR.** This includes `jj describe`, `jj commit`, `git add`, `git commit`, or any other source control operations that create a checkpoint or update a description.
- **Always verify Rust code** by running `cd src-tauri && cargo check` before reporting a successful implementation or restart.
- **Strictly adhere to the established UI theme** (Tactical Midnight, square borders, `--primary` / `--secondary` bronze accents).
- **Enforce layout boundaries**: All major layout containers must have `overflow: hidden` to prevent UI jitter and content bleeding.
- **Real Error Reporting**: Never use generic "Check API Key" messages. Always capture and display the raw error body from the backend/provider.
- **Persistence**: Any configuration changes made in the UI must be persisted to `app_config_dir/config.json` via the `save_config` command.
- **Tauri Invoke**: Reminder: Tauri expects `camelCase` in JS arguments, which maps to `snake_case` in Rust.


