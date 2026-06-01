use crate::mcp::contracts::{
    AgentUiDispatchEvent, HealthCheckResponse, UiDispatchRequest, UiDispatchResponse,
};
use crate::models::{AppError, AppResult, AppState, PathResolver};
use crate::services::render;
use tauri::Emitter;

pub async fn handle_health_check(
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<HealthCheckResponse> {
    let db_ready = state
        .db
        .lock()
        .await
        .query_row("SELECT 1", [], |_row| Ok(()))
        .is_ok();
    let runtime_capabilities = crate::runtime_capabilities::collect_runtime_capabilities(
        render::configured_freecad_cmd(state).as_deref(),
        app,
    );
    let freecad_configured = runtime_capabilities.freecad.available;
    let config_dir = app.app_config_dir();
    let db_path = config_dir
        .join("history.sqlite")
        .to_string_lossy()
        .to_string();

    Ok(HealthCheckResponse {
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        db_path,
        freecad_configured,
        db_ready,
        runtime_capabilities,
    })
}

pub async fn handle_ui_dispatch(
    app: &tauri::AppHandle,
    params: UiDispatchRequest,
) -> AppResult<UiDispatchResponse> {
    app.emit(
        "mcp://ui-dispatch",
        AgentUiDispatchEvent {
            action: params.action,
            target: params.target,
            value: params.value,
        },
    )
    .map_err(|e| AppError::internal(format!("Failed to emit UI dispatch event: {}", e)))?;

    Ok(UiDispatchResponse { success: true })
}
