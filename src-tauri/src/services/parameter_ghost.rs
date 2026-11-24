use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::component_import_runtime::{
    compile_authoring_source, InstalledLibraryComponentResolver, ResolveAuthoringSourceRequest,
};
use crate::contracts::{
    validate_design_params, AppError, AppResult, DesignParams, GeometryBackend, SourceLanguage,
};
use crate::ecky_cad_host::direct_occt::{
    plan_core_program_with_params, plan_core_program_with_params_and_bindings, OcctPlannedProgram,
};
use crate::ecky_cad_host::direct_occt_runner::{run_plan_preview_mesh, with_runner_cancellation};
use crate::ecky_cad_host::direct_occt_runtime::with_direct_occt_text_font_path;
use crate::models::{AppState, PathResolver};
use crate::services::render::{configured_cad_text_font_path, run_direct_occt_with_large_stack};

const RETAINED_PREVIEWS: usize = 8;

#[derive(Debug, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ParameterGhostPreviewInput {
    pub thread_id: String,
    pub target_message_id: String,
    pub parameters: DesignParams,
    pub request_id: String,
}

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ParameterGhostPreview {
    pub model_stl_path: Option<String>,
}

struct ActiveRun {
    request_id: String,
    cancelled: Arc<AtomicBool>,
}

#[derive(Default)]
pub(crate) struct ParameterGhostRuns(Mutex<Option<ActiveRun>>);

struct PreviewRun<'a> {
    runs: &'a ParameterGhostRuns,
    cancelled: Arc<AtomicBool>,
}

impl ParameterGhostRuns {
    fn begin(&self, request_id: String) -> PreviewRun<'_> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut active = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(previous) = active.replace(ActiveRun {
            request_id,
            cancelled: Arc::clone(&cancelled),
        }) {
            previous.cancelled.store(true, Ordering::Release);
        }
        PreviewRun {
            runs: self,
            cancelled,
        }
    }

    pub(crate) fn cancel(&self, request_id: &str) {
        let active = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(run) = active.as_ref().filter(|run| run.request_id == request_id) {
            run.cancelled.store(true, Ordering::Release);
        }
    }
}

impl Drop for PreviewRun<'_> {
    fn drop(&mut self) {
        let mut active = self
            .runs
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if active
            .as_ref()
            .is_some_and(|run| Arc::ptr_eq(&run.cancelled, &self.cancelled))
        {
            *active = None;
        }
    }
}

fn check_current(cancelled: &AtomicBool) -> AppResult<()> {
    if cancelled.load(Ordering::Acquire) {
        Err(AppError::conflict("Parameter preview was superseded."))
    } else {
        Ok(())
    }
}

pub async fn preview_parameter_ghost(
    input: ParameterGhostPreviewInput,
    state: &AppState,
    app: Arc<dyn PathResolver>,
) -> AppResult<ParameterGhostPreview> {
    if input.request_id.trim().is_empty() {
        return Err(AppError::validation(
            "Parameter preview requires a request ID.",
        ));
    }
    let run = state.parameter_ghost_runs.begin(input.request_id);
    let target = {
        let conn = state.db.lock().await;
        crate::services::target::resolve_target(
            &conn,
            app.as_ref(),
            Some(input.thread_id),
            Some(input.target_message_id),
        )?
    };
    let design = target
        .design
        .ok_or_else(|| AppError::validation("The target has no Ecky source."))?;
    if design.source_language != SourceLanguage::EckyIrV0
        || design.geometry_backend != GeometryBackend::EckyRust
        || design.post_processing.is_some()
    {
        return Err(AppError::validation(
            "Parameter preview requires native Ecky source without post-processing.",
        ));
    }
    validate_design_params(&input.parameters, &design.ui_spec)?;
    let expected_lock = target
        .artifact_bundle
        .and_then(|bundle| bundle.component_dependency_lock);
    if expected_lock.is_none()
        && crate::component_import_runtime::source_has_live_component_import(&design.macro_code)
    {
        return Err(AppError::validation(
            "Parameter preview requires the saved component dependency lock.",
        ));
    }

    let font_path = configured_cad_text_font_path(state);
    let guard = state.acquire_geometry_render_silent().await;
    check_current(&run.cancelled)?;
    let cancelled = Arc::clone(&run.cancelled);
    let mesh = tokio::task::spawn_blocking(move || {
        let _guard = guard;
        run_direct_occt_with_large_stack("parameter-preview", move || {
            check_current(&cancelled)?;
            let compilation = compile_authoring_source(
                ResolveAuthoringSourceRequest {
                    authored_source: &design.macro_code,
                    expected_lock: expected_lock.as_ref(),
                },
                &InstalledLibraryComponentResolver { app: app.as_ref() },
            )?;
            let _pins = crate::component_package_runtime::pin_component_store_payloads(
                compilation.dependency_lock.package_digests(),
            );
            let program = crate::topology_target_ids::rebind_program_tagged_selectors(
                &compilation.program,
                target.model_manifest.as_ref(),
            )?;
            with_direct_occt_text_font_path(font_path.as_deref(), || {
                let planned =
                    changed_part_plans(&program, &design.initial_params, &input.parameters)?;
                check_current(&cancelled)?;
                if planned.plan.parts.is_empty() {
                    return Ok(None);
                }
                let root =
                    crate::model_runtime::runtime_root(app.as_ref())?.join("parameter-ghosts");
                let output = root.join(uuid::Uuid::new_v4().to_string());
                let result = with_runner_cancellation(Some(Arc::clone(&cancelled)), || {
                    run_plan_preview_mesh(
                        &planned.plan,
                        &planned.authored_shape_bindings,
                        &output,
                        app.as_ref(),
                    )
                })
                .and_then(|mesh| {
                    check_current(&cancelled)?;
                    Ok(mesh)
                });
                match result {
                    Ok(mesh) => {
                        prune_preview_cache(&root, &output);
                        Ok(Some(mesh))
                    }
                    Err(error) => {
                        let _ = std::fs::remove_dir_all(output);
                        Err(error)
                    }
                }
            })
        })
    })
    .await
    .map_err(|error| AppError::internal(format!("Parameter preview worker failed: {error}")))??;
    check_current(&run.cancelled)?;
    Ok(ParameterGhostPreview {
        model_stl_path: mesh.map(|path| path.to_string_lossy().into_owned()),
    })
}

fn changed_part_plans(
    program: &ecky_render::core_ir::CoreProgram,
    saved: &DesignParams,
    draft: &DesignParams,
) -> AppResult<OcctPlannedProgram> {
    let saved = plan_core_program_with_params(program, saved)?;
    let base_parts: BTreeMap<_, _> = saved
        .parts
        .iter()
        .map(|part| (part.key.as_str(), part))
        .collect();
    let mut draft = plan_core_program_with_params_and_bindings(program, draft)?;
    draft
        .plan
        .parts
        .retain(|part| base_parts.get(part.key.as_str()).copied() != Some(part));
    draft.authored_shape_bindings.retain(|binding| {
        draft
            .plan
            .parts
            .iter()
            .any(|part| part.key == binding.part_key)
    });
    Ok(draft)
}

fn prune_preview_cache(root: &Path, latest: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut older: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.path() != latest
                && uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()).is_ok()
        })
        .filter_map(|entry| {
            let updated = entry.metadata().ok()?.modified().ok()?;
            Some((updated, entry.path()))
        })
        .collect();
    older.sort_unstable_by_key(|(updated, _)| std::cmp::Reverse(*updated));
    for (_, path) in older.into_iter().skip(RETAINED_PREVIEWS - 1) {
        let _ = std::fs::remove_dir_all(path);
    }
}
