use crate::freecad;
use crate::models::{AppResult, AppState, ArtifactBundle, DesignParams, PathResolver};

pub fn configured_freecad_cmd(state: &AppState) -> Option<String> {
    let config = state.config.lock().unwrap();
    let cmd = config.freecad_cmd.trim();
    if cmd.is_empty() {
        None
    } else {
        Some(cmd.to_string())
    }
}

pub fn is_freecad_available(state: &AppState) -> bool {
    freecad::resolve_freecad_path(configured_freecad_cmd(state).as_deref()).is_ok()
}

pub async fn render_stl(
    macro_code: &str,
    parameters: &DesignParams,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<String> {
    let _guard = state.render_lock.lock().await;
    let result = freecad::render(
        macro_code,
        parameters,
        configured_freecad_cmd(state).as_deref(),
        app,
    );
    if result.is_ok() {
        let runtime_cache_dir = freecad::runtime_cache_dir(app)?;
        freecad::evict_cache_if_needed(&runtime_cache_dir);
    }
    result
}

pub async fn render_model(
    macro_code: &str,
    parameters: &DesignParams,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let _guard = state.render_lock.lock().await;
    let mut result = freecad::render_model(
        macro_code,
        parameters,
        configured_freecad_cmd(state).as_deref(),
        app,
    );
    if let Ok(ref mut bundle) = result {
        if let Some(post_proc) = post_processing {
            if let Some(disp) = &post_proc.displacement {
                if let Some(crate::models::ParamValue::String(image_path)) =
                    parameters.get(&disp.image_param)
                {
                    if !image_path.is_empty() {
                        let stl_path = std::path::Path::new(&bundle.preview_stl_path);
                        if let Err(e) =
                            crate::displacement::apply(stl_path, image_path, disp, stl_path)
                        {
                            println!("Warning: Displacement failed: {}", e);
                        } else {
                            // Update content hash since file changed
                            if let Ok(bytes) = std::fs::read(stl_path) {
                                use sha2::{Digest, Sha256};
                                let mut hasher = Sha256::new();
                                hasher.update(&bytes);
                                bundle.content_hash = format!("{:x}", hasher.finalize());
                            }
                        }
                    }
                }
            }
        }
        let runtime_cache_dir = freecad::runtime_cache_dir(app)?;
        freecad::evict_cache_if_needed(&runtime_cache_dir);
    }
    result
}
