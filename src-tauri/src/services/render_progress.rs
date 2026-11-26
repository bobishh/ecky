use std::future::Future;
use std::sync::Arc;

use serde::Serialize;
use tauri::Emitter;

use crate::contracts::AppResult;
use crate::ecky_cad_host::direct_occt_runner::{NativeRenderProgress, NativeRenderReporter};
use crate::models::AppState;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RenderProgressTarget {
    render_id: String,
    thread_id: String,
    base_message_id: Option<String>,
    message_id: Option<String>,
}

#[derive(Clone)]
struct RenderProgressScope {
    app: tauri::AppHandle,
    target: RenderProgressTarget,
}

tokio::task_local! {
    static RENDER_PROGRESS: RenderProgressScope;
}

pub(crate) async fn observe<T>(
    state: &AppState,
    thread_id: &str,
    base_message_id: Option<&str>,
    message_id: Option<&str>,
    render: impl Future<Output = AppResult<T>>,
) -> AppResult<T> {
    let Some(app) = state.render_progress_app_handle() else {
        return render.await;
    };
    let scope = RenderProgressScope {
        app,
        target: RenderProgressTarget {
            render_id: uuid::Uuid::new_v4().to_string(),
            thread_id: thread_id.to_owned(),
            base_message_id: base_message_id.map(str::to_owned),
            message_id: message_id.map(str::to_owned),
        },
    };
    RENDER_PROGRESS
        .scope(scope, async {
            let result = render.await;
            if result.is_err() {
                RENDER_PROGRESS.with(|scope| {
                    emit(scope, NativeRenderProgress::Failed);
                });
            }
            result
        })
        .await
}

pub(crate) fn reporter() -> Option<NativeRenderReporter> {
    RENDER_PROGRESS
        .try_with(|scope| {
            let scope = scope.clone();
            Arc::new(move |progress| emit(&scope, progress)) as NativeRenderReporter
        })
        .ok()
}

fn emit(scope: &RenderProgressScope, progress: NativeRenderProgress) {
    #[derive(Clone, Serialize)]
    struct Event<'a> {
        #[serde(flatten)]
        target: &'a RenderProgressTarget,
        #[serde(flatten)]
        progress: NativeRenderProgress,
    }

    let _ = scope.app.emit(
        "native-render-progress",
        Event {
            target: &scope.target,
            progress,
        },
    );
}
