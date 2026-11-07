use tauri::State;

use crate::contracts::{AppError, AppResult, ProviderWriterActivationInput};
use crate::models::AppState;

#[tauri::command]
#[specta::specta]
pub async fn activate_provider_writer(
    input: ProviderWriterActivationInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    match input.provider.as_str() {
        "codex" => {
            crate::commands::codex_takeover::activate_bound_writer(&state, &input.ecky_thread_id)
                .await
        }
        "agy" => {
            crate::commands::agy_provider::activate_bound_writer(&state, &input.ecky_thread_id)
                .await
        }
        "grok" => {
            crate::commands::grok_provider::activate_bound_writer(&state, &input.ecky_thread_id)
                .await
        }
        provider => Err(AppError::validation(format!(
            "Unsupported provider writer '{provider}'."
        ))),
    }
}
