# Agent Protocol

## Mandates
- **Always verify Rust code** by running `cd src-tauri && cargo check` before reporting a successful implementation or restart.
- **Strictly adhere to the established UI theme** (Tactical Midnight, square borders, `--primary` / `--secondary` bronze accents).
- **Enforce layout boundaries**: All major layout containers must have `overflow: hidden` to prevent UI jitter and content bleeding.
- **Real Error Reporting**: Never use generic "Check API Key" messages. Always capture and display the raw error body from the backend/provider.
- **Persistence**: Any configuration changes made in the UI must be persisted to `app_config_dir/config.json` via the `save_config` command.
