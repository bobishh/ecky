use crate::contracts::infer_macro_dialect_from_code;
use crate::freecad;
use crate::models::{
    AppError, AppResult, AppState, ArtifactBundle, DesignParams, DiagnosticContext,
    DiagnosticParamValue, GeometryBackend, MacroDialect, ModelManifest, PathResolver,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const ECKY_LOWERING_STACK_SIZE: usize = 32 * 1024 * 1024;
const ECKY_DIRECT_OCCT_DEFAULT_STACK_SIZE: usize = 64 * 1024 * 1024;
const ECKY_DIRECT_OCCT_STACK_MB_ENV: &str = "ECKY_DIRECT_OCCT_STACK_MB";
const DIRECT_OCCT_RESOURCE_SNAPSHOT_PATHS: &[&str] = &[
    "runtime/occt",
    "runtime/build123d",
    "runtime/build123d/bin/python3",
    "runtime/build123d/bin/python",
    "runtime/occt/bin/direct-occt-runner",
    "bin/direct-occt-runner",
];

fn source_line_for_offset(source: &str, offset: usize) -> Option<usize> {
    if offset > source.len() {
        return None;
    }
    Some(
        source.as_bytes()[..offset]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1,
    )
}

fn parse_byte_offset_from_message(message: &str) -> Option<usize> {
    let marker = "byte ";
    let idx = message.find(marker)?;
    let digits = message[idx + marker.len()..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    (!digits.is_empty())
        .then(|| digits.parse::<usize>().ok())
        .flatten()
}

fn source_line_range_for_span(
    source: &str,
    span: crate::ecky_core_ir::SourceSpan,
) -> Option<(usize, usize)> {
    let start = span.start as usize;
    let end = span.end as usize;
    if start >= end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return None;
    }
    let start_line = source_line_for_offset(source, start)?;
    let inclusive_end = end.saturating_sub(1);
    let end_line = source_line_for_offset(source, inclusive_end)?;
    Some((start_line, end_line.max(start_line)))
}

fn stable_node_key_for_span(source: &str, span: crate::ecky_core_ir::SourceSpan) -> Option<String> {
    let start = span.start as usize;
    let end = span.end as usize;
    if start >= end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return None;
    }
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"ecky-diagnostic-span|");
    hasher.update(&source.as_bytes()[start..end]);
    Some(format!("sha256:{:x}", hasher.finalize()))
}

fn core_operation_name(op: &crate::ecky_core_ir::CoreOperation) -> String {
    use crate::ecky_core_ir::{
        CoreArrayOp, CoreBooleanOp, CoreFrameOp, CoreMetaOp, CoreOperation, CorePathOp,
        CorePrimitive, CoreSurfaceOp, CoreTransformOp,
    };

    match op {
        CoreOperation::Primitive(CorePrimitive::Box) => "box".to_string(),
        CoreOperation::Primitive(CorePrimitive::Sphere) => "sphere".to_string(),
        CoreOperation::Primitive(CorePrimitive::Cylinder) => "cylinder".to_string(),
        CoreOperation::Primitive(CorePrimitive::Cone) => "cone".to_string(),
        CoreOperation::Primitive(CorePrimitive::Torus) => "torus".to_string(),
        CoreOperation::Primitive(CorePrimitive::Wedge) => "wedge".to_string(),
        CoreOperation::Primitive(CorePrimitive::Ellipse) => "ellipse".to_string(),
        CoreOperation::Primitive(CorePrimitive::Slot) => "slot-overall".to_string(),
        CoreOperation::Primitive(CorePrimitive::SlotArc) => "slot-arc".to_string(),
        CoreOperation::Primitive(CorePrimitive::Circle) => "circle".to_string(),
        CoreOperation::Primitive(CorePrimitive::Rectangle) => "rectangle".to_string(),
        CoreOperation::Primitive(CorePrimitive::RoundedRectangle) => "rounded-rect".to_string(),
        CoreOperation::Primitive(CorePrimitive::RoundedPolygon) => "rounded-polygon".to_string(),
        CoreOperation::Primitive(CorePrimitive::Polygon) => "polygon".to_string(),
        CoreOperation::Primitive(CorePrimitive::Profile) => "profile".to_string(),
        CoreOperation::Primitive(CorePrimitive::MakeFace) => "make-face".to_string(),
        CoreOperation::Primitive(CorePrimitive::Text) => "text".to_string(),
        CoreOperation::Primitive(CorePrimitive::Svg) => "svg".to_string(),
        CoreOperation::Primitive(CorePrimitive::Stl) => "import-stl".to_string(),
        CoreOperation::Boolean(CoreBooleanOp::Union) => "union".to_string(),
        CoreOperation::Boolean(CoreBooleanOp::Difference) => "difference".to_string(),
        CoreOperation::Boolean(CoreBooleanOp::Intersection) => "intersection".to_string(),
        CoreOperation::Boolean(CoreBooleanOp::Xor) => "xor".to_string(),
        CoreOperation::Transform(CoreTransformOp::Translate) => "translate".to_string(),
        CoreOperation::Transform(CoreTransformOp::Rotate) => "rotate".to_string(),
        CoreOperation::Transform(CoreTransformOp::Scale) => "scale".to_string(),
        CoreOperation::Transform(CoreTransformOp::Mirror) => "mirror".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Extrude) => "extrude".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Revolve) => "revolve".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Loft) => "loft".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Sweep) => "sweep".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Shell) => "shell".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Offset) => "offset".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::OffsetRounded) => "offset-rounded".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Fillet) => "fillet".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Chamfer) => "chamfer".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Taper) => "taper".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Twist) => "twist".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Draft) => "draft".to_string(),
        CoreOperation::Path(CorePathOp::Polyline) => "path".to_string(),
        CoreOperation::Path(CorePathOp::BezierPath) => "bezier-path".to_string(),
        CoreOperation::Path(CorePathOp::Bspline) => "bspline".to_string(),
        CoreOperation::Array(CoreArrayOp::LinearArray) => "linear-array".to_string(),
        CoreOperation::Array(CoreArrayOp::RadialArray) => "radial-array".to_string(),
        CoreOperation::Array(CoreArrayOp::GridArray) => "grid-array".to_string(),
        CoreOperation::Array(CoreArrayOp::ArcArray) => "arc-array".to_string(),
        CoreOperation::Array(CoreArrayOp::Repeat) => "repeat".to_string(),
        CoreOperation::Array(CoreArrayOp::RepeatUnion) => "repeat-union".to_string(),
        CoreOperation::Array(CoreArrayOp::RepeatCompound) => "repeat-compound".to_string(),
        CoreOperation::Array(CoreArrayOp::RepeatPick) => "repeat-pick".to_string(),
        CoreOperation::Frame(CoreFrameOp::Plane) => "plane".to_string(),
        CoreOperation::Frame(CoreFrameOp::Location) => "location".to_string(),
        CoreOperation::Frame(CoreFrameOp::PathFrame) => "path-frame".to_string(),
        CoreOperation::Frame(CoreFrameOp::Place) => "place".to_string(),
        CoreOperation::Frame(CoreFrameOp::ClipBox) => "clip-box".to_string(),
        CoreOperation::Meta(CoreMetaOp::Group) => "compound".to_string(),
        CoreOperation::Meta(CoreMetaOp::Comment) => "meta".to_string(),
        CoreOperation::Meta(CoreMetaOp::Annotate) => "build".to_string(),
        CoreOperation::Custom(name) => name.clone(),
    }
}

fn diagnostic_param_values(parameters: &DesignParams) -> Vec<DiagnosticParamValue> {
    parameters
        .iter()
        .map(|(key, value)| DiagnosticParamValue {
            key: key.clone(),
            value: value.clone(),
        })
        .collect()
}

fn best_matching_node_context(
    node: &crate::ecky_core_ir::CoreNode,
    part_key: &str,
    start_line: usize,
    end_line: usize,
    best: &mut Option<(usize, String, String)>,
    source: &str,
) {
    let Some(span) = node.span else {
        return;
    };
    let Some((node_start, node_end)) = source_line_range_for_span(source, span) else {
        return;
    };
    if node_start > start_line || node_end < end_line {
        return;
    }
    let score = span.end.saturating_sub(span.start) as usize;
    let op_name = match &node.kind {
        crate::ecky_core_ir::CoreNodeKind::Call { op, .. } => Some(core_operation_name(op)),
        _ => None,
    };
    if let Some(op_name) = op_name {
        let replace = best
            .as_ref()
            .map(|(best_score, _, _)| score < *best_score)
            .unwrap_or(true);
        if replace {
            *best = Some((score, part_key.to_string(), op_name));
        }
    }
    if let crate::ecky_core_ir::CoreNodeKind::Call { args, .. } = &node.kind {
        for arg in args {
            best_matching_node_context(arg, part_key, start_line, end_line, best, source);
        }
    }
}

fn diagnostic_context_from_source(
    source: &str,
    parameters: &DesignParams,
    start_line: Option<usize>,
    end_line: Option<usize>,
    fallback_op_name: Option<&str>,
) -> Option<DiagnosticContext> {
    let resolved_params = diagnostic_param_values(parameters);
    let mut part_key = None;
    let mut op_name = fallback_op_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    if let (Some(start_line), Some(end_line)) = (start_line, end_line) {
        if let Ok(program) = crate::ecky_scheme::compile_to_core_program(source) {
            let mut best = None;
            for part in &program.parts {
                best_matching_node_context(
                    &part.root, &part.key, start_line, end_line, &mut best, source,
                );
            }
            if let Some((_, resolved_part_key, resolved_op_name)) = best {
                part_key = Some(resolved_part_key);
                if op_name.is_none() {
                    op_name = Some(resolved_op_name);
                }
            }
        }
    }

    if part_key.is_none() && op_name.is_none() && resolved_params.is_empty() {
        return None;
    }

    Some(DiagnosticContext {
        part_key,
        op_name,
        start_line,
        end_line,
        resolved_params,
    })
}

fn attach_diagnostic_context(
    mut error: AppError,
    source: Option<&str>,
    parameters: &DesignParams,
    default_operation: Option<&str>,
) -> AppError {
    if error.operation.is_none() {
        if let Some(operation) = default_operation
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            error = error.with_operation(operation.to_string());
        }
    }
    if error.diagnostic_context.is_none() {
        let context = source.and_then(|source| {
            diagnostic_context_from_source(
                source,
                parameters,
                error.start_line,
                error.end_line,
                error.operation.as_deref(),
            )
        });
        if let Some(context) = context {
            error = error.with_diagnostic_context(context);
        }
    }
    error
}

fn annotate_lowering_error(
    mut error: AppError,
    source: &str,
    operation: &str,
    parameters: &DesignParams,
) -> AppError {
    if let Some(kind) = classify_lowering_diagnostic_kind(&error.message, error.details.as_deref())
    {
        error.message = format!("lowering_diagnostic[{kind}] {}", error.message);
    }
    error = error.with_operation(operation.to_string());
    if let Err(compile_error) = crate::ecky_scheme::compile_to_core_program(source) {
        if let Some(span) = compile_error.primary_span {
            if let Some((start_line, end_line)) = source_line_range_for_span(source, span) {
                error = error.with_line_range(start_line, end_line);
            }
            if let Some(stable_node_key) = stable_node_key_for_span(source, span) {
                error = error.with_stable_node_key(stable_node_key);
            }
        } else if let Some(byte_offset) = parse_byte_offset_from_message(&compile_error.message) {
            if let Some(line) = source_line_for_offset(source, byte_offset.min(source.len())) {
                error = error.with_line_range(line, line);
            }
        }
    }
    attach_diagnostic_context(error, Some(source), parameters, Some(operation))
}

fn classify_lowering_diagnostic_kind(message: &str, details: Option<&str>) -> Option<&'static str> {
    let mut combined =
        String::with_capacity(message.len() + details.map(str::len).unwrap_or(0) + 1);
    combined.push_str(&message.to_ascii_lowercase());
    if let Some(details) = details {
        if !details.is_empty() {
            combined.push(' ');
            combined.push_str(&details.to_ascii_lowercase());
        }
    }

    if combined.contains("unsupported") && combined.contains("backend") {
        return Some("unsupported_backend");
    }
    if combined.contains("null topods_shape")
        || (combined.contains("null") && combined.contains("boolean"))
    {
        return Some("null_boolean");
    }
    if combined.contains("non-manifold") {
        return Some("non_manifold_output");
    }
    if combined.contains("empty part")
        || combined.contains("no solids")
        || combined.contains("contains no solids")
    {
        return Some("empty_part");
    }
    if combined.contains("invalid parameter")
        || combined.contains("requires `:")
        || combined.contains("must be positive")
        || combined.contains("expects keyword")
    {
        return Some("invalid_parameter");
    }
    None
}

fn lower_ecky_with_large_stack(
    label: &'static str,
    macro_code: &str,
    parameters: &DesignParams,
    lower: impl FnOnce(&str) -> AppResult<String> + Send + 'static,
) -> AppResult<String> {
    let source = macro_code.to_string();
    let source_for_diagnostics = source.clone();
    let lowered = std::thread::Builder::new()
        .name(format!("ecky-{label}-lower"))
        .stack_size(ECKY_LOWERING_STACK_SIZE)
        .spawn(move || lower(&source))
        .map_err(|err| AppError::internal(format!("Failed to spawn Ecky {label} lowerer: {err}")))?
        .join()
        .map_err(|_| AppError::internal(format!("Ecky {label} lowerer panicked.")))?;
    lowered.map_err(|err| {
        annotate_lowering_error(
            err,
            &source_for_diagnostics,
            &format!("lower:{label}"),
            parameters,
        )
    })
}

fn run_direct_occt_with_large_stack<T: Send + 'static>(
    label: &'static str,
    task: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    std::thread::Builder::new()
        .name(format!("ecky-direct-occt-{label}"))
        .stack_size(direct_occt_stack_size())
        .spawn(task)
        .map_err(|err| {
            AppError::internal(format!("Failed to spawn Direct OCCT {label} worker: {err}"))
        })?
        .join()
        .map_err(|_| AppError::internal(format!("Direct OCCT {label} worker panicked.")))?
}

fn direct_occt_stack_size() -> usize {
    match std::env::var(ECKY_DIRECT_OCCT_STACK_MB_ENV) {
        Ok(raw) => direct_occt_stack_size_from_mb(raw.trim())
            .unwrap_or(ECKY_DIRECT_OCCT_DEFAULT_STACK_SIZE),
        Err(_) => ECKY_DIRECT_OCCT_DEFAULT_STACK_SIZE,
    }
}

fn direct_occt_stack_size_from_mb(raw: &str) -> Option<usize> {
    let mb = raw.parse::<usize>().ok()?;
    if mb == 0 {
        return None;
    }
    mb.checked_mul(1024)?.checked_mul(1024)
}

#[derive(Clone)]
struct DirectOcctThreadResolver {
    config_dir: PathBuf,
    data_dir: PathBuf,
    resources: BTreeMap<String, PathBuf>,
}

impl DirectOcctThreadResolver {
    fn from_resolver(app: &dyn PathResolver) -> Self {
        Self {
            config_dir: app.app_config_dir(),
            data_dir: app.app_data_dir(),
            resources: DIRECT_OCCT_RESOURCE_SNAPSHOT_PATHS
                .iter()
                .filter_map(|path| {
                    app.resource_path(path)
                        .map(|resolved| ((*path).to_string(), resolved))
                })
                .collect(),
        }
    }
}

impl PathResolver for DirectOcctThreadResolver {
    fn app_config_dir(&self) -> PathBuf {
        self.config_dir.clone()
    }

    fn app_data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }

    fn resource_path(&self, path: &str) -> Option<PathBuf> {
        self.resources.get(path).cloned()
    }
}

fn load_manifest_for_bundle(bundle: &ArtifactBundle) -> AppResult<Option<ModelManifest>> {
    let path = bundle.manifest_path.trim();
    if path.is_empty() {
        return Ok(None);
    }
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(AppError::internal(format!(
                "Failed to read model manifest '{}': {}",
                path, err
            )));
        }
    };
    let parsed: ModelManifest = serde_json::from_str(&raw).map_err(|e| {
        AppError::internal(format!("Failed to parse model manifest '{}': {}", path, e))
    })?;
    Ok(Some(parsed))
}

fn update_content_hash_and_exports(
    preview_stl_path: &str,
    bundle: &mut ArtifactBundle,
) -> AppResult<()> {
    let stl_path = Path::new(preview_stl_path);
    if let Ok(bytes) = std::fs::read(stl_path) {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        bundle.content_hash = format!("{:x}", hasher.finalize());
    }
    Ok(())
}

fn apply_requested_post_processing(
    bundle: &mut ArtifactBundle,
    parameters: &DesignParams,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
) -> AppResult<()> {
    let Some(post_proc) =
        crate::contracts::normalize_post_processing_spec(post_processing.cloned())
    else {
        return Ok(());
    };
    let has_explicit_attachment_path = post_processing
        .map(|post| !post.lithophane_attachments.is_empty())
        .unwrap_or(false);

    let stl_path = Path::new(&bundle.preview_stl_path);

    if has_explicit_attachment_path && !post_proc.lithophane_attachments.is_empty() {
        let resolved_attachments =
            resolve_lithophane_attachments(bundle, parameters, &post_proc.lithophane_attachments)?;

        if !resolved_attachments.is_empty() {
            let export_dir = crate::lithophane::export_dir_for_preview(stl_path);
            bundle.export_artifacts.clear();
            bundle.export_artifacts = crate::lithophane::apply_lithophane_attachments(
                stl_path,
                &resolved_attachments,
                stl_path,
                &export_dir,
            )?;
            let preview_path = bundle.preview_stl_path.clone();
            update_content_hash_and_exports(&preview_path, bundle)?;
            return Ok(());
        }
    }

    if let Some(disp) = &post_proc.displacement {
        let Some(crate::models::ParamValue::String(image_path)) = parameters.get(&disp.image_param)
        else {
            return Ok(());
        };
        if image_path.trim().is_empty() {
            return Ok(());
        }
        crate::displacement::apply(stl_path, image_path, disp, stl_path)?;
        bundle.export_artifacts.clear();
        let preview_path = bundle.preview_stl_path.clone();
        update_content_hash_and_exports(&preview_path, bundle)?;
    }

    Ok(())
}

fn resolve_lithophane_attachments(
    bundle: &ArtifactBundle,
    parameters: &DesignParams,
    attachments: &[crate::contracts::LithophaneAttachment],
) -> AppResult<Vec<crate::lithophane::ResolvedLithophaneAttachment>> {
    let manifest = load_manifest_for_bundle(bundle)?;
    let mut resolved = Vec::new();

    for attachment in attachments.iter().filter(|attachment| attachment.enabled) {
        let Some(image_path) = crate::lithophane::resolve_image_path(attachment, parameters) else {
            continue;
        };

        let target_part_id = attachment.target_part_id.trim();
        let target_bounds = if target_part_id.is_empty() {
            None
        } else {
            let loaded_manifest = manifest.as_ref().ok_or_else(|| {
                AppError::validation(format!(
                    "Lithophane attachment '{}' references targetPartId '{}' but the model manifest is missing.",
                    attachment.id, target_part_id
                ))
            })?;
            let target_part = loaded_manifest
                .parts
                .iter()
                .find(|part| part.part_id == target_part_id)
                .ok_or_else(|| {
                    AppError::validation(format!(
                        "Lithophane attachment '{}' references missing targetPartId '{}'.",
                        attachment.id, target_part_id
                    ))
                })?;
            Some(target_part.bounds.clone().ok_or_else(|| {
                AppError::validation(format!(
                    "Lithophane attachment '{}' targetPartId '{}' has no bounds in the model manifest.",
                    attachment.id, target_part_id
                ))
            })?)
        };

        resolved.push(crate::lithophane::ResolvedLithophaneAttachment {
            id: attachment.id.clone(),
            image_path,
            target_bounds,
            placement: attachment.placement.clone(),
            relief: attachment.relief.clone(),
            color_mode: attachment.color.mode,
            channel_thickness_mm: attachment.color.channel_thickness_mm,
        });
    }

    Ok(resolved)
}

pub fn configured_freecad_cmd(state: &AppState) -> Option<String> {
    let config = state.config.lock().unwrap();
    let cmd = config.freecad_cmd.trim();
    if cmd.is_empty() {
        None
    } else {
        Some(cmd.to_string())
    }
}

pub fn configured_cad_text_font_path(state: &AppState) -> Option<String> {
    let config = state.config.lock().unwrap();
    let path = config.cad_text_font_path.trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

pub fn is_freecad_available(state: &AppState) -> bool {
    freecad::resolve_freecad_path(configured_freecad_cmd(state).as_deref()).is_ok()
}

fn finalize_render_bundle(
    mut bundle: ArtifactBundle,
    parameters: &DesignParams,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    apply_requested_post_processing(&mut bundle, parameters, post_processing).map_err(|err| {
        attach_diagnostic_context(err, None, parameters, Some("export:post-processing"))
    })?;
    let runtime_cache_dir = freecad::runtime_cache_dir(app)?;
    freecad::evict_cache_if_needed(&runtime_cache_dir);
    Ok(bundle)
}

fn resolve_geometry_backend(
    effective_dialect: &MacroDialect,
    requested_backend: Option<GeometryBackend>,
    config_default_backend: GeometryBackend,
) -> GeometryBackend {
    requested_backend.unwrap_or(match effective_dialect {
        MacroDialect::EckyIrV0 => config_default_backend,
        MacroDialect::Build123d => GeometryBackend::Build123d,
        MacroDialect::CadFrameworkV1 => GeometryBackend::Freecad,
        MacroDialect::Legacy => GeometryBackend::Freecad,
    })
}

fn resolve_dispatch_backend(
    macro_code: &str,
    effective_dialect: &MacroDialect,
    requested_backend: GeometryBackend,
) -> AppResult<GeometryBackend> {
    if *effective_dialect != MacroDialect::EckyIrV0 {
        return Ok(requested_backend);
    }

    let uses_mesh_only = crate::ecky_ir::source_uses_ecky_rust_only_cad_ops(macro_code);
    let uses_exact_only = crate::ecky_ir::source_uses_exact_backend_only_cad_ops(macro_code);

    if uses_mesh_only && uses_exact_only {
        return Err(AppError::validation(
            "Mesh-only ops like `wall-pattern` cannot mix with exact-only ops like `sampled-radial-loft` in one `.ecky` model.",
        ));
    }

    if matches!(
        requested_backend,
        GeometryBackend::Build123d | GeometryBackend::Freecad
    ) && uses_mesh_only
    {
        return Ok(GeometryBackend::EckyRust);
    }

    Ok(requested_backend)
}

fn try_render_direct_occt_ecky_ir(
    macro_code: &str,
    parameters: &DesignParams,
    effective_dialect: &MacroDialect,
    previous_manifest: Option<&ModelManifest>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<Option<ArtifactBundle>> {
    if *effective_dialect != MacroDialect::EckyIrV0 {
        return Ok(None);
    }
    let macro_code = macro_code.to_string();
    let parameters = parameters.clone();
    let previous_manifest = previous_manifest.cloned();
    let app = DirectOcctThreadResolver::from_resolver(app);
    let cad_text_font_path = configured_cad_text_font_path(state);
    run_direct_occt_with_large_stack("render", move || {
        let program = match crate::ecky_scheme::compile_to_core_program(&macro_code) {
            Ok(program) => program,
            Err(_) => return Ok(None),
        };
        let program = crate::topology_target_ids::rebind_program_tagged_selectors(
            &program,
            previous_manifest.as_ref(),
        )?;
        let runtime_root = match crate::runtime_capabilities::resolve_direct_occt_runtime_root(&app)
        {
            Ok(runtime_root) => runtime_root,
            Err(_) => return Ok(None),
        };
        let layout =
            crate::ecky_cad_host::direct_occt_sdk::inspect_build123d_ocp_runtime(&runtime_root);
        let (bundle, _manifest) =
            crate::ecky_cad_host::direct_occt_runtime::render_core_program_runtime_bundle_with_font_path(
                &program,
                &macro_code,
                &parameters,
                &layout,
                &app,
                cad_text_font_path.as_deref(),
            )?;
        Ok(Some(bundle))
    })
}

fn format_nested_app_error(err: &AppError) -> String {
    let mut text = err.to_string();
    if let Some(extra) = err.details.as_deref() {
        let extra = extra.trim();
        if !extra.is_empty() && extra != text {
            text.push(' ');
            text.push_str(extra);
        }
    }
    text
}

fn direct_occt_plan_diagnostic(macro_code: &str, parameters: &DesignParams) -> Result<(), String> {
    let macro_code = macro_code.to_string();
    let parameters = parameters.clone();
    run_direct_occt_with_large_stack("plan", move || {
        let Some(program) = crate::ecky_scheme::try_compile_to_core_program(&macro_code) else {
            return Err(AppError::validation(
                "Source did not compile to Core IR before Direct OCCT planning.",
            ));
        };
        let program = program.map_err(|err| {
            AppError::validation(format!(
                "Core IR compile failed before Direct OCCT planning. {}",
                format_nested_app_error(&err)
            ))
        })?;
        crate::ecky_cad_host::direct_occt::plan_core_program_with_params(&program, &parameters)
            .map(|_| ())
    })
    .map_err(|err| {
        let message = format_nested_app_error(&err);
        if message.starts_with("Source did not compile")
            || message.starts_with("Core IR compile failed")
        {
            message
        } else {
            format!("Direct OCCT planner rejected model. {}", message)
        }
    })
}

fn unsupported_exact_only_direct_occt_error(details: String) -> AppError {
    AppError::with_details(
        crate::models::AppErrorCode::Validation,
        "Unsupported on current geometry backend. Switch backend and rerender.",
        details,
    )
}

fn unsupported_required_direct_occt_error(details: String) -> AppError {
    AppError::with_details(
        crate::models::AppErrorCode::Validation,
        "Direct OCCT required for this Ecky Native model. Native render unavailable.",
        details,
    )
}

fn blocked_direct_occt_native_error(details: String) -> AppError {
    AppError::with_details(
        crate::models::AppErrorCode::Validation,
        "Ecky Native direct OCCT render failed.",
        details,
    )
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
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    render_model_with_previous_manifest(
        macro_code,
        parameters,
        macro_dialect,
        geometry_backend,
        post_processing,
        None,
        state,
        app,
    )
    .await
}

pub async fn render_model_with_previous_manifest(
    macro_code: &str,
    parameters: &DesignParams,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    previous_manifest: Option<&ModelManifest>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let _guard = state.render_lock.lock().await;
    let first_attempt = render_model_unlocked(
        macro_code,
        parameters,
        macro_dialect.clone(),
        geometry_backend,
        post_processing,
        previous_manifest,
        state,
        app,
    );
    match first_attempt {
        Ok(bundle) => Ok(bundle),
        Err(err)
            if previous_manifest.is_some()
                && source_has_selector_tags(macro_code)
                && is_tagged_selector_mismatch_error(&err) =>
        {
            let bundle = render_model_unlocked(
                macro_code,
                parameters,
                macro_dialect,
                geometry_backend,
                post_processing,
                None,
                state,
                app,
            )?;
            append_tagged_selector_rebind_warning(app, &bundle);
            Ok(bundle)
        }
        Err(err) => Err(err),
    }
}

fn render_model_unlocked(
    macro_code: &str,
    parameters: &DesignParams,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    previous_manifest: Option<&ModelManifest>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let effective_dialect =
        macro_dialect.unwrap_or_else(|| infer_macro_dialect_from_code(macro_code));
    let config_default_backend = state.config.lock().unwrap().default_geometry_backend;
    let resolved_backend =
        resolve_geometry_backend(&effective_dialect, geometry_backend, config_default_backend);
    let dispatch_backend =
        resolve_dispatch_backend(macro_code, &effective_dialect, resolved_backend)?;
    crate::runtime_capabilities::ensure_backend_available(
        dispatch_backend,
        configured_freecad_cmd(state).as_deref(),
        app,
    )?;
    // Lower Ecky IR to the target backend before dispatch.
    // Legacy Python and Build123d sources stay as-is.
    let lowered = match (dispatch_backend, effective_dialect.clone()) {
        (GeometryBackend::Build123d, MacroDialect::EckyIrV0) => {
            lower_ecky_with_large_stack("build123d", macro_code, parameters, {
                let previous_manifest = previous_manifest.cloned();
                move |source| {
                    crate::ecky_ir::lower_to_build123d_with_previous_manifest(
                        source,
                        previous_manifest.as_ref(),
                    )
                }
            })
            .map(Some)
            .map_err(|err| {
                attach_diagnostic_context(
                    err,
                    Some(macro_code),
                    parameters,
                    Some("lower:build123d"),
                )
            })?
        }
        (GeometryBackend::Freecad, MacroDialect::EckyIrV0) => {
            lower_ecky_with_large_stack("freecad", macro_code, parameters, {
                let previous_manifest = previous_manifest.cloned();
                move |source| {
                    crate::ecky_ir::lower_to_freecad_with_previous_manifest(
                        source,
                        previous_manifest.as_ref(),
                    )
                }
            })
            .map(Some)
            .map_err(|err| {
                attach_diagnostic_context(err, Some(macro_code), parameters, Some("lower:freecad"))
            })?
        }
        _ => None,
    };
    let dispatch_source = lowered.as_deref().unwrap_or(macro_code);
    let direct_occt_capability = if dispatch_backend == GeometryBackend::EckyRust
        && effective_dialect == MacroDialect::EckyIrV0
    {
        Some(crate::runtime_capabilities::probe_direct_occt_runtime(app))
    } else {
        None
    };
    let result = match dispatch_backend {
        GeometryBackend::EckyRust => {
            // Explicit native requests are Direct OCCT only: real OCCT success
            // or the real OCCT error, never a silently degraded mesh artifact.
            // The one sanctioned mesh path is a redirect: a Build123d/FreeCAD
            // request whose source uses mesh-only ops (`wall-pattern`) lands
            // here because the Rust mesh renderer is the designated handler
            // for those ops.
            let mesh_only_redirect = resolved_backend != GeometryBackend::EckyRust;
            let uses_exact_only = effective_dialect == MacroDialect::EckyIrV0
                && crate::ecky_ir::source_uses_exact_backend_only_cad_ops(macro_code);
            let uses_direct_occt_required = effective_dialect == MacroDialect::EckyIrV0
                && crate::ecky_ir::source_uses_direct_occt_required_cad_ops(macro_code);
            let direct_occt_plan_detail = if effective_dialect == MacroDialect::EckyIrV0 {
                direct_occt_plan_diagnostic(macro_code, parameters).err()
            } else {
                Some("Direct OCCT planner runs only for `.ecky` source.".to_string())
            };
            let direct_occt_plannable = direct_occt_plan_detail.is_none();
            let direct_occt_ready = direct_occt_capability
                .as_ref()
                .is_some_and(|capability| capability.available);
            let direct_attempt = if direct_occt_capability
                .as_ref()
                .is_some_and(|capability| capability.available)
            {
                try_render_direct_occt_ecky_ir(
                    macro_code,
                    parameters,
                    &effective_dialect,
                    previous_manifest,
                    state,
                    app,
                )
            } else {
                Ok(None)
            };
            match direct_attempt {
                Ok(Some(bundle)) => Ok(bundle),
                Ok(None) => {
                    if uses_exact_only {
                        Err(attach_diagnostic_context(
                            unsupported_exact_only_direct_occt_error(
                            "EckyRust/direct OCCT did not produce a native bundle for exact-backend-only CAD ops."
                                .to_string(),
                            ),
                            Some(macro_code),
                            parameters,
                            Some("export:direct-occt"),
                        ))
                    } else if uses_direct_occt_required {
                        Err(attach_diagnostic_context(
                            unsupported_required_direct_occt_error(format!(
                            "EckyRust/direct OCCT did not produce a native bundle for native-required CAD ops like `text`, `svg`, `import-stl`, or `helical-ridge`. {}",
                            direct_occt_capability
                                .as_ref()
                                .map(|capability| capability.detail.as_str())
                                .unwrap_or("Direct OCCT availability not probed.")
                            )),
                            Some(macro_code),
                            parameters,
                            Some("export:direct-occt"),
                        ))
                    } else if mesh_only_redirect {
                        crate::ecky_ir::render_model_with_previous_manifest(
                            macro_code,
                            parameters,
                            previous_manifest,
                            app,
                        )
                    } else if direct_occt_ready && direct_occt_plannable {
                        Err(attach_diagnostic_context(
                            blocked_direct_occt_native_error(format!(
                            "Direct OCCT runtime reported ready and planned this model, but native export returned no bundle. {}",
                            direct_occt_capability
                                .as_ref()
                                .map(|capability| capability.detail.as_str())
                                .unwrap_or("Direct OCCT availability not probed.")
                            )),
                            Some(macro_code),
                            parameters,
                            Some("export:direct-occt"),
                        ))
                    } else {
                        let planner_detail = direct_occt_plan_detail
                            .as_deref()
                            .unwrap_or("Direct OCCT planner reason unavailable.");
                        Err(attach_diagnostic_context(
                            blocked_direct_occt_native_error(format!(
                            "Native backend requires Direct OCCT. No mesh fallback is used. ready={direct_occt_ready}; plannable={direct_occt_plannable}. Planner blocker: {planner_detail} Next step: switch backend away from native for unsupported ops or rewrite source to a Direct OCCT-supported shape plan. {}",
                            direct_occt_capability
                                .as_ref()
                                .map(|capability| capability.detail.as_str())
                                .unwrap_or("Direct OCCT availability not probed.")
                            )),
                            Some(macro_code),
                            parameters,
                            Some("plan:direct-occt"),
                        ))
                    }
                }
                Err(err) => {
                    if uses_exact_only {
                        let mut details = String::from(
                            "EckyRust/direct OCCT failed on exact-backend-only CAD ops.",
                        );
                        details.push(' ');
                        details.push_str(&err.to_string());
                        if let Some(extra) = err.details.as_deref() {
                            if !extra.is_empty() {
                                details.push(' ');
                                details.push_str(extra);
                            }
                        }
                        Err(attach_diagnostic_context(
                            unsupported_exact_only_direct_occt_error(details),
                            Some(macro_code),
                            parameters,
                            Some("export:direct-occt"),
                        ))
                    } else if uses_direct_occt_required {
                        let mut details =
                            String::from("EckyRust/direct OCCT failed on native-required CAD ops.");
                        details.push(' ');
                        details.push_str(&err.to_string());
                        if let Some(extra) = err.details.as_deref() {
                            if !extra.is_empty() {
                                details.push(' ');
                                details.push_str(extra);
                            }
                        }
                        Err(attach_diagnostic_context(
                            unsupported_required_direct_occt_error(details),
                            Some(macro_code),
                            parameters,
                            Some("export:direct-occt"),
                        ))
                    } else if mesh_only_redirect {
                        crate::ecky_ir::render_model_with_previous_manifest(
                            macro_code,
                            parameters,
                            previous_manifest,
                            app,
                        )
                    } else {
                        let mut details = if direct_occt_ready && direct_occt_plannable {
                            String::from(
                                "Direct OCCT runtime reported ready and planned this model, but native export failed.",
                            )
                        } else {
                            let planner_detail = direct_occt_plan_detail
                                .as_deref()
                                .unwrap_or("Direct OCCT planner reason unavailable.");
                            format!(
                                "Native backend requires Direct OCCT. No mesh fallback is used. ready={direct_occt_ready}; plannable={direct_occt_plannable}. Planner blocker: {planner_detail} Next step: switch backend away from native for unsupported ops or rewrite source to a Direct OCCT-supported shape plan."
                            )
                        };
                        details.push(' ');
                        details.push_str(&err.to_string());
                        if let Some(extra) = err.details.as_deref() {
                            if !extra.is_empty() {
                                details.push(' ');
                                details.push_str(extra);
                            }
                        }
                        Err(attach_diagnostic_context(
                            blocked_direct_occt_native_error(details),
                            Some(macro_code),
                            parameters,
                            Some("export:direct-occt"),
                        ))
                    }
                }
            }
        }
        GeometryBackend::Build123d => {
            let source_language = if effective_dialect == MacroDialect::EckyIrV0 {
                crate::models::SourceLanguage::EckyIrV0
            } else {
                crate::models::SourceLanguage::Build123d
            };
            crate::build123d::render_model_with_sources(
                dispatch_source,
                if effective_dialect == MacroDialect::EckyIrV0 {
                    Some(macro_code)
                } else {
                    None
                },
                parameters,
                app,
                source_language,
            )
        }
        GeometryBackend::Freecad => {
            let source_language = if effective_dialect == MacroDialect::EckyIrV0 {
                crate::models::SourceLanguage::EckyIrV0
            } else {
                crate::models::SourceLanguage::LegacyPython
            };
            freecad::render_model_with_sources_and_font_path(
                dispatch_source,
                if effective_dialect == MacroDialect::EckyIrV0 {
                    Some(macro_code)
                } else {
                    None
                },
                parameters,
                configured_freecad_cmd(state).as_deref(),
                configured_cad_text_font_path(state).as_deref(),
                app,
                source_language,
            )
        }
    };
    result
        .map_err(|err| attach_diagnostic_context(err, Some(macro_code), parameters, Some("render")))
        .and_then(|bundle| finalize_render_bundle(bundle, parameters, post_processing, app))
}

fn source_has_selector_tags(source: &str) -> bool {
    let Some(program) = crate::ecky_scheme::try_compile_to_core_program(source) else {
        return false;
    };
    program
        .map(|program| !program.selector_tags.is_empty())
        .unwrap_or(false)
}

fn is_tagged_selector_mismatch_error(err: &AppError) -> bool {
    let mut combined = err.message.to_ascii_lowercase();
    if let Some(details) = err.details.as_deref() {
        if !details.is_empty() {
            combined.push(' ');
            combined.push_str(&details.to_ascii_lowercase());
        }
    }
    [
        "did not match target ids",
        "ambiguously matched stable face target",
        "ambiguously matched stable edge target",
        "direct occt edge selector target ids did not match current topology",
        "direct occt edge selector stable target id ambiguously matched current topology",
        "direct occt shell face selector target ids did not match current topology",
        "direct occt shell face selector stable target id ambiguously matched current topology",
        "matched no shell opening faces",
        "matched no edges",
    ]
    .iter()
    .any(|needle| combined.contains(needle))
}

fn append_tagged_selector_rebind_warning(app: &dyn PathResolver, bundle: &ArtifactBundle) {
    let Ok(mut manifest) = crate::model_runtime::read_model_manifest(app, &bundle.model_id) else {
        return;
    };
    let warning =
        "Tagged selector recorded ids no longer matched current topology; rerender fell back to authored selector declarations.".to_string();
    if manifest
        .warnings
        .iter()
        .any(|existing| existing == &warning)
    {
        return;
    }
    manifest.warnings.push(warning);
    let _ = crate::model_runtime::write_model_manifest(app, &bundle.model_id, &manifest);
}

pub async fn render_model_source(
    source_path: &Path,
    source_language: Option<crate::models::SourceLanguage>,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    parameters: &DesignParams,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let _guard = state.render_lock.lock().await;
    render_model_source_unlocked(
        source_path,
        source_language,
        macro_dialect,
        geometry_backend,
        parameters,
        post_processing,
        state,
        app,
    )
}

fn render_model_source_unlocked(
    source_path: &Path,
    source_language: Option<crate::models::SourceLanguage>,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    parameters: &DesignParams,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    let source_path_text = source_path
        .to_str()
        .ok_or_else(|| AppError::internal("Invalid component source path."))?;

    let bundle = match extension.as_deref() {
        Some("fcstd") => freecad::import_fcstd(
            source_path_text,
            configured_freecad_cmd(state).as_deref(),
            app,
        )?,
        Some("step") | Some("stp") => freecad::import_step(
            source_path_text,
            configured_freecad_cmd(state).as_deref(),
            app,
        )?,
        Some("ecky") | Some("py") | Some("fcmacro") | None => {
            let macro_code = fs::read_to_string(source_path).map_err(|err| {
                AppError::persistence(format!(
                    "Failed to read component source '{}': {}",
                    source_path.display(),
                    err
                ))
            })?;
            let resolved_dialect = resolve_source_macro_dialect(
                source_path,
                source_language,
                macro_dialect,
                &macro_code,
            );
            return render_model_unlocked(
                &macro_code,
                parameters,
                Some(resolved_dialect),
                geometry_backend,
                post_processing,
                None,
                state,
                app,
            );
        }
        Some(other) => {
            return Err(AppError::validation(format!(
                "Unsupported component source '{}' with extension '.{}'. Expected .ecky, .py, .FCMacro, .FCStd, or .step.",
                source_path.display(),
                other
            )));
        }
    };

    finalize_render_bundle(bundle, parameters, post_processing, app)
}

fn resolve_source_macro_dialect(
    source_path: &Path,
    source_language: Option<crate::models::SourceLanguage>,
    macro_dialect: Option<MacroDialect>,
    macro_code: &str,
) -> MacroDialect {
    if let Some(explicit) = macro_dialect {
        return explicit;
    }
    if let Some(language) = source_language {
        return match language {
            crate::models::SourceLanguage::LegacyPython => {
                infer_macro_dialect_from_code(macro_code)
            }
            crate::models::SourceLanguage::EckyIrV0 => MacroDialect::EckyIrV0,
            crate::models::SourceLanguage::Build123d => MacroDialect::Build123d,
        };
    }
    match source_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("ecky") => MacroDialect::EckyIrV0,
        _ => infer_macro_dialect_from_code(macro_code),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        annotate_lowering_error, apply_requested_post_processing,
        is_tagged_selector_mismatch_error, load_manifest_for_bundle, render_model,
        render_model_with_previous_manifest, resolve_dispatch_backend, resolve_geometry_backend,
    };
    use crate::contracts::{
        Config, DisplacementSpec, LithophaneAttachment, LithophaneAttachmentSource,
        LithophaneColor, LithophaneColorMode, LithophanePlacement, LithophanePlacementMode,
        LithophaneRelief, LithophaneSide, MacroDialect, McpConfig, OverflowMode,
        PostProcessingSpec, ProjectionType,
    };
    use crate::models::{
        AppError, AppState, DesignParams, GeometryBackend, ParamValue, PathResolver,
    };
    use std::path::PathBuf;

    #[derive(Clone)]
    struct TestResolver {
        root: PathBuf,
    }

    impl PathResolver for TestResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.join("config")
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.join("data")
        }

        fn resource_path(&self, path: &str) -> Option<PathBuf> {
            Some(self.root.join("resources").join(path))
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("ecky-render-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }

    fn write_ascii_stl_fixture(path: &std::path::Path) {
        let stl = r#"solid sample
facet normal 0 0 1
  outer loop
    vertex 0 0 0
    vertex 1 0 0
    vertex 0 1 0
  endloop
endfacet
endsolid sample
"#;
        std::fs::write(path, stl).expect("write stl fixture");
    }

    fn create_direct_occt_runtime_layout(root: &std::path::Path) {
        let ocp_root = root
            .join("resources")
            .join("runtime")
            .join("occt")
            .join("lib")
            .join("python3.12")
            .join("site-packages")
            .join("OCP");
        let include_dir = ocp_root.join("include").join("opencascade");
        let dylib_dir = ocp_root.join(".dylibs");
        std::fs::create_dir_all(&include_dir).expect("create include dir");
        std::fs::create_dir_all(&dylib_dir).expect("create dylib dir");
        for header in crate::ecky_cad_host::direct_occt_sdk::REQUIRED_OCCT_HEADERS {
            std::fs::write(include_dir.join(header), "// header\n").expect("write header");
        }
        for lib in crate::ecky_cad_host::direct_occt_sdk::REQUIRED_OCCT_LIBS {
            let filename = if cfg!(target_os = "macos") {
                format!("lib{lib}.dylib")
            } else if cfg!(target_os = "windows") {
                format!("{lib}.dll")
            } else {
                format!("lib{lib}.so")
            };
            std::fs::write(dylib_dir.join(filename), "").expect("write dylib");
        }
    }

    #[cfg(unix)]
    fn write_executable(path: &std::path::Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create executable dir");
        }
        std::fs::write(path, body).expect("write executable");
        let mut perms = std::fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).expect("chmod executable");
    }

    fn text_font_fixture() -> Option<&'static str> {
        [
            "/System/Library/Fonts/Supplemental/Arial Black.ttf",
            "/System/Library/Fonts/Supplemental/Impact.ttf",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/Library/Fonts/Arial.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
            "C:/Windows/Fonts/arial.ttf",
        ]
        .into_iter()
        .find(|path| std::path::Path::new(path).is_file())
    }

    fn test_config() -> Config {
        Config {
            engines: Vec::new(),
            selected_engine_id: String::new(),
            freecad_cmd: String::new(),
            cad_text_font_path: String::new(),
            freecad_library_roots: Vec::new(),
            assets: Vec::new(),
            microwave: None,
            voice: crate::models::VoiceConfig::default(),
            mcp: McpConfig::default(),
            has_seen_onboarding: true,
            connection_type: None,
            default_engine_kind: crate::models::EngineKind::Freecad,
            default_source_language: crate::models::SourceLanguage::LegacyPython,
            default_geometry_backend: GeometryBackend::Freecad,
            max_generation_attempts: 3,
            max_verify_attempts: 0,
            projects_root: None,
        }
    }

    fn test_state(root: &std::path::Path) -> AppState {
        let conn = crate::db::init_db(&root.join("test.db")).expect("test db");
        AppState::new(test_config(), None, conn)
    }

    fn example_fixture_source(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../model-runtime/examples")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read fixture {}: {err}", path.display()))
    }

    fn film_adapter_golden_six_part_fixture_source() -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../model-runtime/examples/film-adapter-golden-6part.ecky");
        std::fs::read_to_string(&path).unwrap_or_else(|_| {
            r#"(model
  (part film_gate_lower_035 (box 40 16 3))
  (part film_gate_upper_035 (translate 0 0 3 (box 40 8 2)))
  (part film_gate_lower_045 (translate 44 0 0 (box 40 16 3)))
  (part film_gate_upper_045 (translate 44 0 3 (box 40 8 2)))
  (part film_gate_lower_055 (translate 88 0 0 (box 40 16 3)))
  (part film_gate_upper_055 (translate 88 0 3 (box 40 8 2))))"#
                .to_string()
        })
    }

    fn fixture_part_ids(source: &str) -> Vec<String> {
        source
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                let rest = trimmed.strip_prefix("(part ")?;
                rest.split_whitespace()
                    .next()
                    .map(|token| token.trim_end_matches(')').to_string())
            })
            .collect()
    }

    #[test]
    fn apply_requested_displacement_surfaces_raw_displacement_errors() {
        let params = DesignParams::from([(
            "image".to_string(),
            crate::models::ParamValue::String("/definitely/missing/lithophane.png".to_string()),
        )]);
        let mut bundle = crate::models::ArtifactBundle {
            schema_version: 1,
            model_id: "model".to_string(),
            source_kind: crate::models::ModelSourceKind::Generated,
            engine_kind: crate::models::EngineKind::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            content_hash: "unchanged".to_string(),
            artifact_version: 1,
            fcstd_path: "/tmp/model.FCStd".to_string(),
            manifest_path: "/tmp/missing-manifest.json".to_string(),
            macro_path: None,
            preview_stl_path: "/tmp/nonexistent-preview.stl".to_string(),
            viewer_assets: vec![],
            edge_targets: vec![],
            face_targets: vec![],
            callout_anchors: vec![],
            measurement_guides: vec![],
            export_artifacts: vec![],
        };

        let error = apply_requested_post_processing(
            &mut bundle,
            &params,
            Some(&PostProcessingSpec {
                displacement: Some(DisplacementSpec {
                    image_param: "image".to_string(),
                    projection: ProjectionType::Planar,
                    depth_mm: 1.0,
                    invert: false,
                }),
                lithophane_attachments: vec![],
            }),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Failed to open image for displacement"),
            "unexpected error: {}",
            error
        );
        assert_eq!(bundle.content_hash, "unchanged");
    }

    #[test]
    fn ecky_lowering_annotation_adds_operation_and_source_lines_for_span_errors() {
        let source = "(model\n  (part body (box 1 2 3))))";
        let error = annotate_lowering_error(
            AppError::validation("compile failed"),
            source,
            "lower:build123d",
            &DesignParams::new(),
        );

        assert_eq!(error.operation.as_deref(), Some("lower:build123d"));
        assert!(error.start_line.is_some());
        assert!(error.end_line.is_some());
        assert!(error.start_line.unwrap() <= error.end_line.unwrap());
    }

    #[test]
    fn ecky_lowering_annotation_tags_known_lowering_diagnostic_kind() {
        let source = "(model\n  (part body (box 1 2 3))))";
        let error = annotate_lowering_error(
            AppError::validation("Null TopoDS_Shape while resolving boolean difference"),
            source,
            "lower:build123d",
            &DesignParams::new(),
        );

        assert!(
            error
                .message
                .starts_with("lowering_diagnostic[null_boolean] "),
            "unexpected message: {}",
            error.message
        );
    }

    #[test]
    fn lowering_diagnostic_kind_classifier_detects_required_categories() {
        assert_eq!(
            super::classify_lowering_diagnostic_kind(
                "Unsupported backend for op helical-ridge",
                None
            ),
            Some("unsupported_backend")
        );
        assert_eq!(
            super::classify_lowering_diagnostic_kind("invalid parameter for :pitch", None),
            Some("invalid_parameter")
        );
        assert_eq!(
            super::classify_lowering_diagnostic_kind("Null TopoDS_Shape", None),
            Some("null_boolean")
        );
        assert_eq!(
            super::classify_lowering_diagnostic_kind("mesh became non-manifold after fuse", None),
            Some("non_manifold_output")
        );
        assert_eq!(
            super::classify_lowering_diagnostic_kind("part contains no solids after shell", None),
            Some("empty_part")
        );
    }

    #[test]
    fn attach_diagnostic_context_maps_part_op_and_resolved_params_from_lines() {
        let source = "(model\n  (part body\n    (fillet 1 (box width 2 3))))";
        let params =
            std::collections::BTreeMap::from([("width".to_string(), ParamValue::Number(12.0))]);
        let error = super::attach_diagnostic_context(
            AppError::validation("fillet failed").with_line_range(3, 3),
            Some(source),
            &params,
            Some("render"),
        );

        let context = error
            .diagnostic_context
            .as_ref()
            .expect("diagnostic context");
        assert_eq!(context.part_key.as_deref(), Some("body"));
        assert_eq!(context.op_name.as_deref(), Some("render"));
        assert_eq!(context.start_line, Some(3));
        assert_eq!(context.end_line, Some(3));
        assert_eq!(context.resolved_params.len(), 1);
        assert_eq!(context.resolved_params[0].key, "width");
        assert_eq!(context.resolved_params[0].value, ParamValue::Number(12.0));
    }

    #[test]
    fn direct_occt_stack_size_defaults_to_64_mb() {
        assert_eq!(super::ECKY_DIRECT_OCCT_DEFAULT_STACK_SIZE, 64 * 1024 * 1024);
    }

    #[test]
    fn direct_occt_stack_size_parses_env_mb() {
        assert_eq!(
            super::direct_occt_stack_size_from_mb("128"),
            Some(128 * 1024 * 1024)
        );
        assert_eq!(super::direct_occt_stack_size_from_mb("0"), None);
        assert_eq!(super::direct_occt_stack_size_from_mb("nope"), None);
    }

    #[test]
    fn broken_helical_ridge_lowering_surfaces_operation_and_diagnostic_kind() {
        let source = r#"(model
  (part body
    (wall-pattern
      (:mode ribs :depth 1.0)
      (shell 2 (cylinder 10 20)))))"#;

        let err = super::lower_ecky_with_large_stack(
            "build123d",
            source,
            &DesignParams::new(),
            crate::ecky_ir::lower_to_build123d,
        )
        .expect_err("wall-pattern should fail for build123d lowering");

        assert_eq!(err.operation.as_deref(), Some("lower:build123d"));
        assert!(
            err.message
                .starts_with("lowering_diagnostic[unsupported_backend] "),
            "{err:?}"
        );
        assert!(
            err.details
                .as_deref()
                .is_some_and(|text| text.contains("wall-pattern")),
            "{err:?}"
        );
    }

    #[test]
    fn post_processing_noop_preserves_existing_step_export_artifacts() {
        let params = DesignParams::new();
        let mut bundle = crate::models::ArtifactBundle {
            schema_version: 1,
            model_id: "model".to_string(),
            source_kind: crate::models::ModelSourceKind::Generated,
            engine_kind: crate::models::EngineKind::EckyIrV0,
            source_language: crate::models::SourceLanguage::EckyIrV0,
            geometry_backend: crate::models::GeometryBackend::EckyRust,
            content_hash: "unchanged".to_string(),
            artifact_version: 1,
            fcstd_path: String::new(),
            manifest_path: "/tmp/missing-manifest.json".to_string(),
            macro_path: None,
            preview_stl_path: "/tmp/nonexistent-preview.stl".to_string(),
            viewer_assets: vec![],
            edge_targets: vec![],
            face_targets: vec![],
            callout_anchors: vec![],
            measurement_guides: vec![],
            export_artifacts: vec![crate::models::ExportArtifact {
                label: "STEP".to_string(),
                format: "step".to_string(),
                path: "/tmp/model.step".to_string(),
                role: "primary".to_string(),
            }],
        };

        apply_requested_post_processing(
            &mut bundle,
            &params,
            Some(&PostProcessingSpec {
                displacement: Some(DisplacementSpec {
                    image_param: "missing_image".to_string(),
                    projection: ProjectionType::Planar,
                    depth_mm: 1.0,
                    invert: false,
                }),
                lithophane_attachments: vec![],
            }),
        )
        .expect("post-processing no-op");

        assert_eq!(bundle.export_artifacts.len(), 1);
        assert_eq!(bundle.export_artifacts[0].format, "step");
        assert_eq!(bundle.export_artifacts[0].path, "/tmp/model.step");
    }

    #[test]
    fn planar_cmyk_requires_attachment_render_path_not_legacy_displacement() {
        let root = std::env::temp_dir().join(format!("ecky-litho-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let preview_stl_path = root.join("preview.stl");
        std::fs::write(
            &preview_stl_path,
            [&[0u8; 80][..], &0u32.to_le_bytes()[..]].concat(),
        )
        .unwrap();

        let params = DesignParams::new();
        let mut bundle = crate::models::ArtifactBundle {
            schema_version: 1,
            model_id: "model".to_string(),
            source_kind: crate::models::ModelSourceKind::Generated,
            engine_kind: crate::models::EngineKind::Freecad,
            source_language: crate::models::SourceLanguage::LegacyPython,
            geometry_backend: crate::models::GeometryBackend::Freecad,
            content_hash: "unchanged".to_string(),
            artifact_version: 1,
            fcstd_path: "/tmp/model.FCStd".to_string(),
            manifest_path: "/tmp/missing-manifest.json".to_string(),
            macro_path: None,
            preview_stl_path: preview_stl_path.to_string_lossy().to_string(),
            viewer_assets: vec![],
            edge_targets: vec![],
            face_targets: vec![],
            callout_anchors: vec![],
            measurement_guides: vec![],
            export_artifacts: vec![],
        };

        let error = apply_requested_post_processing(
            &mut bundle,
            &params,
            Some(&PostProcessingSpec {
                displacement: None,
                lithophane_attachments: vec![LithophaneAttachment {
                    id: "panel".to_string(),
                    enabled: true,
                    source: LithophaneAttachmentSource::File {
                        image_path: "/definitely/missing/lithophane.png".to_string(),
                    },
                    target_part_id: String::new(),
                    placement: LithophanePlacement {
                        mode: LithophanePlacementMode::PartSidePatch,
                        side: LithophaneSide::Front,
                        projection: ProjectionType::Planar,
                        width_mm: 40.0,
                        height_mm: 40.0,
                        offset_x_mm: 0.0,
                        offset_y_mm: 0.0,
                        rotation_deg: 0.0,
                        overflow_mode: OverflowMode::Contain,
                        bleed_margin_mm: 0.0,
                    },
                    relief: LithophaneRelief {
                        depth_mm: 1.0,
                        invert: false,
                    },
                    color: LithophaneColor {
                        mode: LithophaneColorMode::Cmyk,
                        channel_thickness_mm: 0.4,
                    },
                }],
            }),
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("Failed to open image for lithophane attachment"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lithophane_attachment_target_part_id_must_exist_in_manifest() {
        let root =
            std::env::temp_dir().join(format!("ecky-litho-target-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let preview_stl_path = root.join("preview.stl");
        std::fs::write(
            &preview_stl_path,
            [&[0u8; 80][..], &0u32.to_le_bytes()[..]].concat(),
        )
        .unwrap();
        let manifest_path = root.join("manifest.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&crate::models::ModelManifest {
                schema_version: 1,
                model_id: "model".to_string(),
                source_kind: crate::models::ModelSourceKind::Generated,
                source_digest: None,
                core_digest: None,
                ast_schema_version: None,
                engine_kind: crate::models::EngineKind::EckyIrV0,
                source_language: crate::models::SourceLanguage::EckyIrV0,
                geometry_backend: crate::models::GeometryBackend::EckyRust,
                document: crate::models::DocumentMetadata {
                    document_name: "doc".to_string(),
                    document_label: "doc".to_string(),
                    source_path: None,
                    object_count: 1,
                    warnings: vec![],
                },
                parts: vec![crate::models::PartBinding {
                    part_id: "body".to_string(),
                    freecad_object_name: "body".to_string(),
                    label: "Body".to_string(),
                    kind: "solid".to_string(),
                    semantic_role: None,
                    viewer_asset_path: None,
                    viewer_node_ids: vec![],
                    parameter_keys: vec![],
                    editable: true,
                    bounds: Some(crate::models::ManifestBounds {
                        x_min: -10.0,
                        y_min: -10.0,
                        z_min: 0.0,
                        x_max: 10.0,
                        y_max: 10.0,
                        z_max: 20.0,
                    }),
                    volume: None,
                    area: None,
                }],
                parameter_groups: vec![],
                control_primitives: vec![],
                control_relations: vec![],
                control_views: vec![],
                preview_views: vec![],
                advisories: vec![],
                selection_targets: vec![],
                measurement_annotations: vec![],
                tagged_anchors: std::collections::BTreeMap::new(),
                feature_graph: None,
                correspondence_graph: None,
                warnings: vec![],
                enrichment_state: crate::models::ManifestEnrichmentState {
                    status: crate::models::EnrichmentStatus::None,
                    proposals: vec![],
                },
            })
            .unwrap(),
        )
        .unwrap();
        let image_path = root.join("image.png");
        image::RgbImage::from_fn(2, 2, |_x, _y| image::Rgb([255, 255, 255]))
            .save(&image_path)
            .unwrap();

        let params = DesignParams::new();
        let mut bundle = crate::models::ArtifactBundle {
            schema_version: 1,
            model_id: "model".to_string(),
            source_kind: crate::models::ModelSourceKind::Generated,
            engine_kind: crate::models::EngineKind::EckyIrV0,
            source_language: crate::models::SourceLanguage::EckyIrV0,
            geometry_backend: crate::models::GeometryBackend::EckyRust,
            content_hash: "unchanged".to_string(),
            artifact_version: 1,
            fcstd_path: String::new(),
            manifest_path: manifest_path.to_string_lossy().to_string(),
            macro_path: None,
            preview_stl_path: preview_stl_path.to_string_lossy().to_string(),
            viewer_assets: vec![],
            edge_targets: vec![],
            face_targets: vec![],
            callout_anchors: vec![],
            measurement_guides: vec![],
            export_artifacts: vec![],
        };

        let error = apply_requested_post_processing(
            &mut bundle,
            &params,
            Some(&PostProcessingSpec {
                displacement: None,
                lithophane_attachments: vec![LithophaneAttachment {
                    id: "panel".to_string(),
                    enabled: true,
                    source: LithophaneAttachmentSource::File {
                        image_path: image_path.to_string_lossy().to_string(),
                    },
                    target_part_id: "missing".to_string(),
                    placement: LithophanePlacement {
                        mode: LithophanePlacementMode::PartSidePatch,
                        side: LithophaneSide::Front,
                        projection: ProjectionType::Planar,
                        width_mm: 20.0,
                        height_mm: 20.0,
                        offset_x_mm: 0.0,
                        offset_y_mm: 0.0,
                        rotation_deg: 0.0,
                        overflow_mode: OverflowMode::Contain,
                        bleed_margin_mm: 0.0,
                    },
                    relief: LithophaneRelief {
                        depth_mm: 1.0,
                        invert: false,
                    },
                    color: LithophaneColor {
                        mode: LithophaneColorMode::Mono,
                        channel_thickness_mm: 0.4,
                    },
                }],
            }),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("references missing targetPartId 'missing'"),
            "unexpected error: {}",
            error
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ir_generated_bundle_supports_attachment_based_planar_cmyk_lithophane() {
        #[derive(Clone)]
        struct TestResolver {
            root: std::path::PathBuf,
        }

        impl crate::models::PathResolver for TestResolver {
            fn app_config_dir(&self) -> std::path::PathBuf {
                self.root.clone()
            }

            fn app_data_dir(&self) -> std::path::PathBuf {
                self.root.clone()
            }

            fn resource_path(&self, _path: &str) -> Option<std::path::PathBuf> {
                None
            }
        }

        let root =
            std::env::temp_dir().join(format!("ecky-ir-litho-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root: root.clone() };
        let mut bundle = crate::ecky_ir::render_model(
            r#"(model
                (part body
                  (extrude
                    (rounded_rect 32 32 4 12)
                    10)))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("ir render");

        let image_path = root.join("panel.png");
        image::RgbImage::from_fn(3, 3, |x, y| {
            if (x + y) % 2 == 0 {
                image::Rgb([255, 255, 255])
            } else {
                image::Rgb([32, 64, 255])
            }
        })
        .save(&image_path)
        .unwrap();

        apply_requested_post_processing(
            &mut bundle,
            &DesignParams::new(),
            Some(&PostProcessingSpec {
                displacement: None,
                lithophane_attachments: vec![LithophaneAttachment {
                    id: "panel".to_string(),
                    enabled: true,
                    source: LithophaneAttachmentSource::File {
                        image_path: image_path.to_string_lossy().to_string(),
                    },
                    target_part_id: "body".to_string(),
                    placement: LithophanePlacement {
                        mode: LithophanePlacementMode::PartSidePatch,
                        side: LithophaneSide::Front,
                        projection: ProjectionType::Planar,
                        width_mm: 24.0,
                        height_mm: 24.0,
                        offset_x_mm: 0.0,
                        offset_y_mm: 0.0,
                        rotation_deg: 0.0,
                        overflow_mode: OverflowMode::Contain,
                        bleed_margin_mm: 0.0,
                    },
                    relief: LithophaneRelief {
                        depth_mm: 1.0,
                        invert: false,
                    },
                    color: LithophaneColor {
                        mode: LithophaneColorMode::Cmyk,
                        channel_thickness_mm: 0.4,
                    },
                }],
            }),
        )
        .expect("post processing");

        assert!(std::path::Path::new(&bundle.preview_stl_path).exists());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "3mf" && artifact.role == "primary"));
        std::fs::remove_dir_all(root).unwrap();
    }

    // ------------------------------------------------------------------
    // Phase 6 / 7 verification tests
    // ------------------------------------------------------------------

    /// Generic Ecky source uses config backend when request omits backend.
    #[test]
    fn ecky_source_uses_configured_backend_when_request_omits_backend() {
        assert_eq!(
            resolve_geometry_backend(&MacroDialect::EckyIrV0, None, GeometryBackend::Build123d),
            GeometryBackend::Build123d
        );
        assert_eq!(
            resolve_geometry_backend(&MacroDialect::EckyIrV0, None, GeometryBackend::Freecad),
            GeometryBackend::Freecad
        );
        assert_eq!(
            resolve_geometry_backend(
                &MacroDialect::EckyIrV0,
                Some(GeometryBackend::EckyRust),
                GeometryBackend::Build123d
            ),
            GeometryBackend::EckyRust,
        );
    }

    #[test]
    fn legacy_python_and_build123d_sources_keep_backend_defaults() {
        assert_eq!(
            resolve_geometry_backend(&MacroDialect::Build123d, None, GeometryBackend::Freecad),
            GeometryBackend::Build123d
        );
        assert_eq!(
            resolve_geometry_backend(
                &MacroDialect::CadFrameworkV1,
                None,
                GeometryBackend::Build123d
            ),
            GeometryBackend::Freecad
        );
    }

    #[test]
    fn tagged_selector_mismatch_detector_matches_runner_target_id_errors() {
        let err = AppError::with_details(
            crate::models::AppErrorCode::Render,
            "build123d runner failed.",
            "stderr:\nValueError: Edge selector `{'kind': 'targetIds'}` did not match target ids: ['body:edge:old']",
        );
        assert!(is_tagged_selector_mismatch_error(&err));

        let direct_occt = AppError::with_details(
            crate::models::AppErrorCode::Render,
            "Direct OCCT native shim probe failed.",
            "stderr:\nDirect OCCT edge selector target ids did not match current topology for part `body`. requested=body:edge:old",
        );
        assert!(is_tagged_selector_mismatch_error(&direct_occt));

        let unrelated = AppError::validation("shell expects positive wall thickness");
        assert!(!is_tagged_selector_mismatch_error(&unrelated));
    }

    #[test]
    fn ecky_rust_request_keeps_exact_only_source_on_ecky_rust_for_direct_probe() {
        let backend = resolve_dispatch_backend(
            r#"(model
                (part body
                  (sampled-radial-loft
                    (theta z fz)
                    :height 40
                    :z-steps 6
                    :theta-steps 24
                    :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793))))))))"#,
            &MacroDialect::EckyIrV0,
            GeometryBackend::EckyRust,
        )
        .expect("dispatch backend");

        assert_eq!(backend, GeometryBackend::EckyRust);
    }

    #[test]
    fn mixed_mesh_and_exact_only_ops_are_rejected_at_dispatch() {
        let err = resolve_dispatch_backend(
            r#"(model
                (part body
                  (union
                    (wall-pattern (:mode ribs :depth 0.4 :uFreq 8)
                      (extrude (circle 5) 18))
                    (sampled-radial-loft
                      (theta z fz)
                      :height 40
                      :z-steps 6
                      :theta-steps 24
                      :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))))))"#,
            &MacroDialect::EckyIrV0,
            GeometryBackend::EckyRust,
        )
        .expect_err("mixed backend-exclusive ops must reject");

        assert!(err
            .to_string()
            .contains("cannot mix with exact-only ops like `sampled-radial-loft`"));
    }

    #[tokio::test]
    async fn ecky_rust_request_fails_closed_when_direct_occt_cannot_export_operation() {
        let root = temp_root("direct-fallback");
        let resolver = TestResolver { root: root.clone() };
        let state = test_state(&root);

        let err = render_model(
            r#"(model
                (part body
                  (wall-pattern (:mode ribs :depth 0.4 :uFreq 8)
                    (extrude (circle 5) 18))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect_err("native backend should fail closed without Direct OCCT support");

        let diagnostic = format!("{} {}", err, err.details.as_deref().unwrap_or(""));
        assert!(
            diagnostic.contains("Direct OCCT")
                || diagnostic.contains("Native backend requires Direct OCCT"),
            "unexpected error: {err:?}"
        );
        assert!(
            diagnostic.contains("wall-pattern")
                || diagnostic.contains("Planner blocker")
                || diagnostic.contains("Direct OCCT adapter first surface does not support"),
            "planner reason must be surfaced: {err:?}"
        );
        assert!(
            !diagnostic.contains("not supported by current `.ecky` runtime"),
            "must not fall through to mesh runtime: {err:?}"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ecky_rust_request_does_not_silently_mesh_fallback_when_direct_occt_ready_but_export_fails(
    ) {
        let root = temp_root("eckyrust-direct-occt-fail-closed");
        let resolver = TestResolver { root: root.clone() };
        create_direct_occt_runtime_layout(&root);
        let runner = root
            .join("resources")
            .join("runtime")
            .join("occt")
            .join("bin")
            .join("direct-occt-runner");
        write_executable(
            &runner,
            r#"#!/bin/sh
if [ "${1:-}" = "--version" ]; then
  echo "direct-occt-runner 0.1.0"
  exit 0
fi
echo '{"class":"runtime_error","code":"runner_failed","message":"forced test failure","details":"boom"}' >&2
exit 5
"#,
        );

        let direct_capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        assert!(
            direct_capability.available,
            "expected fake Direct OCCT runtime ready, got {:?}",
            direct_capability
        );

        let state = test_state(&root);
        let err = render_model(
            r#"(model (part body (box 10 20 30)))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect_err("covered direct OCCT source must not silently fall back to mesh");

        let diagnostic = format!("{} {}", err, err.details.as_deref().unwrap_or(""));
        assert!(
            diagnostic.contains("Direct OCCT runner failed")
                || diagnostic.contains("forced test failure"),
            "unexpected error: {err:?}"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_request_does_not_silently_build123d_fallback_for_sampled_radial_loft() {
        let root = temp_root("eckyrust-sampled-radial-loft");
        let resolver = TestResolver { root: root.clone() };
        let direct_capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if direct_capability.available {
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        let state = test_state(&root);

        let err = render_model(
            r#"(model
                (part body
                  (sampled-radial-loft
                    (theta z fz)
                    :height 40
                    :z-steps 6
                    :theta-steps 24
                    :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                    :z-map (+ z (* fz 2)))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect_err("EckyRust must not silently build123d fallback");

        assert_ne!(err.operation.as_deref(), Some("lower:build123d"));
        let diagnostic = format!("{} {}", err, err.details.as_deref().unwrap_or(""));
        assert!(
            diagnostic.contains("sampled-radial-loft")
                || diagnostic.contains("exact-backend-only CAD ops"),
            "unexpected error: {err:?}"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_request_renders_helical_ridge_without_build123d_fallback() {
        let root = temp_root("eckyrust-helical-ridge-no-build123d-fallback");
        let resolver = TestResolver { root: root.clone() };
        let direct_capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !direct_capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }
        let state = test_state(&root);

        let bundle = render_model(
            r#"(model
                (part body
                  (helical-ridge
                    :radius 20
                    :pitch 6
                    :height 30
                    :base-width 2
                    :crest-width 1
                    :depth 1.5)))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("EckyRust must render helical-ridge through Direct OCCT");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_request_fails_closed_for_helical_ridge_when_direct_occt_unavailable() {
        let root = temp_root("eckyrust-helical-ridge-direct-occt-required");
        let resolver = TestResolver { root: root.clone() };
        let direct_capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if direct_capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }
        let state = test_state(&root);

        let err = render_model(
            r#"(model
                (part body
                  (helical-ridge
                    :radius 20
                    :pitch 6
                    :height 30
                    :base-width 2
                    :crest-width 1
                    :depth 1.5)))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect_err("helical-ridge must fail closed without direct OCCT");

        let diagnostic = format!("{} {}", err, err.details.as_deref().unwrap_or(""));
        assert!(
            diagnostic.contains("Direct OCCT required")
                || diagnostic.contains("native-required CAD ops"),
            "unexpected error: {err:?}"
        );
        assert!(
            !diagnostic.contains("not supported by current `.ecky` runtime"),
            "must not fall through to mesh runtime: {err:?}"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_request_fails_closed_for_text_when_direct_occt_unavailable() {
        let root = temp_root("eckyrust-text-direct-occt-required");
        let resolver = TestResolver { root: root.clone() };
        let direct_capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if direct_capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }
        let state = test_state(&root);

        let err = render_model(
            r#"(model (part body (extrude (text "A" 12) 2)))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect_err("text must fail closed without direct OCCT");

        let diagnostic = format!("{} {}", err, err.details.as_deref().unwrap_or(""));
        assert!(
            diagnostic.contains("Direct OCCT required")
                || diagnostic.contains("native-required CAD ops"),
            "unexpected error: {err:?}"
        );
        assert!(
            !diagnostic.contains("Switch to FreeCAD or build123d"),
            "must not fall through to mesh runtime: {err:?}"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_request_fails_closed_for_import_stl_when_direct_occt_unavailable() {
        let root = temp_root("eckyrust-import-stl-direct-occt-required");
        let resolver = TestResolver { root: root.clone() };
        let direct_capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if direct_capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }
        let state = test_state(&root);
        let stl_path = root.join("fixture.stl");
        write_ascii_stl_fixture(&stl_path);

        let err = render_model(
            &format!(
                r#"(model (part body (import-stl {:?})))"#,
                stl_path.to_string_lossy()
            ),
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect_err("import-stl must fail closed without direct OCCT");

        let diagnostic = format!("{} {}", err, err.details.as_deref().unwrap_or(""));
        assert!(
            diagnostic.contains("Direct OCCT required")
                || diagnostic.contains("native-required CAD ops"),
            "unexpected error: {err:?}"
        );
        assert!(
            !diagnostic.contains("Switch to FreeCAD or build123d"),
            "must not fall through to mesh runtime: {err:?}"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_request_fails_closed_for_svg_when_direct_occt_unavailable() {
        let root = temp_root("eckyrust-svg-direct-occt-required");
        let resolver = TestResolver { root: root.clone() };
        let direct_capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if direct_capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }
        let state = test_state(&root);

        let err = render_model(
            r#"(model (part body (extrude (svg "/tmp/sample.svg") 2)))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect_err("svg must fail closed without direct OCCT");

        let diagnostic = format!("{} {}", err, err.details.as_deref().unwrap_or(""));
        assert!(
            diagnostic.contains("Direct OCCT required")
                || diagnostic.contains("native-required CAD ops"),
            "unexpected error: {err:?}"
        );
        assert!(
            !diagnostic.contains("Switch to FreeCAD or build123d"),
            "must not fall through to mesh runtime: {err:?}"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_request_renders_import_stl_without_build123d_fallback() {
        let root = temp_root("eckyrust-import-stl-no-build123d-fallback");
        let resolver = TestResolver { root: root.clone() };
        let direct_capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !direct_capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }
        let state = test_state(&root);
        let stl_path = root.join("fixture.stl");
        write_ascii_stl_fixture(&stl_path);

        let bundle = render_model(
            &format!(
                r#"(model (part body (import-stl {:?})))"#,
                stl_path.to_string_lossy()
            ),
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("EckyRust must render import-stl through Direct OCCT");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_request_renders_text_without_build123d_fallback() {
        let root = temp_root("eckyrust-text-no-build123d-fallback");
        let resolver = TestResolver { root: root.clone() };
        let direct_capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !direct_capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }
        let Some(font_path) = text_font_fixture() else {
            let _ = std::fs::remove_dir_all(root);
            return;
        };
        let state = test_state(&root);
        {
            let mut config = state.config.lock().unwrap();
            config.cad_text_font_path = font_path.to_string();
        }

        let bundle = render_model(
            r#"(model (part body (extrude (text "II" 12) 4)))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("EckyRust must render text through Direct OCCT");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_request_renders_svg_without_build123d_fallback() {
        let root = temp_root("eckyrust-svg-no-build123d-fallback");
        let resolver = TestResolver { root: root.clone() };
        let direct_capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !direct_capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }
        let state = test_state(&root);
        let svg_path = root.join("fixture.svg");
        std::fs::write(
            &svg_path,
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><path fill="#000" d="M 1 1 L 9 1 L 9 9 L 1 9 Z"/></svg>"##,
        )
        .expect("write svg");

        let bundle = render_model(
            &format!(
                r#"(model (part body (extrude (svg "{}" 10 10 "contain") 4)))"#,
                svg_path.display()
            ),
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("EckyRust must render svg through Direct OCCT");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn film_scanning_adapter_helicoid_fixture_non_eckyrust_path_keeps_parts_and_step_readiness(
    ) {
        let root = temp_root("film-scanning-adapter-helicoid-golden");
        let resolver = TestResolver { root: root.clone() };
        let state = test_state(&root);
        let source = example_fixture_source("film-scanning-adapter-helicoid.ecky");

        assert!(
            source.contains("(helical-ridge"),
            "fixture must contain helicoid ops"
        );

        let build123d = crate::runtime_capabilities::probe_build123d_runtime(&resolver);
        let freecad =
            crate::runtime_capabilities::probe_freecad_runtime(Some("FreeCADCmd"), &resolver);

        if build123d.available || freecad.available {
            let backend = if build123d.available {
                GeometryBackend::Build123d
            } else {
                GeometryBackend::Freecad
            };

            let bundle = render_model(
                &source,
                &DesignParams::new(),
                Some(MacroDialect::EckyIrV0),
                Some(backend),
                None,
                &state,
                &resolver,
            )
            .await
            .expect("non-eckyrust render");

            assert_eq!(bundle.geometry_backend, backend);
            assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
            assert!(
                bundle
                    .export_artifacts
                    .iter()
                    .any(|artifact| artifact.format == "step"
                        && std::path::Path::new(&artifact.path).is_file()),
                "step artifact must exist on non-EckyRust render path"
            );

            let manifest = load_manifest_for_bundle(&bundle)
                .expect("load manifest")
                .expect("runtime manifest");
            assert_eq!(manifest.document.object_count, 2);
            assert_eq!(
                manifest
                    .parts
                    .iter()
                    .map(|part| part.part_id.as_str())
                    .collect::<Vec<_>>(),
                vec!["top_cover_integrated_helicoid", "moving_lens_carrier"]
            );
        } else {
            let dispatch = resolve_dispatch_backend(
                &source,
                &MacroDialect::EckyIrV0,
                GeometryBackend::Build123d,
            )
            .expect("dispatch backend");
            assert_eq!(dispatch, GeometryBackend::Build123d);

            let lowered = super::lower_ecky_with_large_stack(
                "build123d",
                &source,
                &DesignParams::new(),
                crate::ecky_ir::lower_to_build123d,
            )
            .expect("build123d lower");

            assert!(lowered.contains("_ecky_helical_ridge("), "{}", lowered);
            assert!(lowered.contains("Edge.make_helix("), "{}", lowered);
            assert!(
                lowered.contains(r#"("top_cover_integrated_helicoid","#)
                    && lowered.contains(r#"("moving_lens_carrier","#),
                "{}",
                lowered
            );
            assert!(
                lowered.contains("_ecky_parts"),
                "fallback readiness signal must preserve deterministic part tuple"
            );
        }

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn film_adapter_golden_six_part_fixture_non_eckyrust_path_keeps_six_parts_and_step_readiness(
    ) {
        let root = temp_root("film-adapter-golden-6part");
        let resolver = TestResolver { root: root.clone() };
        let state = test_state(&root);
        let source = film_adapter_golden_six_part_fixture_source();
        let expected_part_ids = fixture_part_ids(&source);
        assert_eq!(
            expected_part_ids.len(),
            6,
            "fixture must declare exactly 6 parts"
        );

        let build123d = crate::runtime_capabilities::probe_build123d_runtime(&resolver);
        let freecad =
            crate::runtime_capabilities::probe_freecad_runtime(Some("FreeCADCmd"), &resolver);

        if build123d.available || freecad.available {
            let backend = if build123d.available {
                GeometryBackend::Build123d
            } else {
                GeometryBackend::Freecad
            };
            let bundle = render_model(
                &source,
                &DesignParams::new(),
                Some(MacroDialect::EckyIrV0),
                Some(backend),
                None,
                &state,
                &resolver,
            )
            .await
            .expect("non-eckyrust render");

            assert_eq!(bundle.geometry_backend, backend);
            assert!(
                bundle
                    .export_artifacts
                    .iter()
                    .any(|artifact| artifact.format == "step"
                        && std::path::Path::new(&artifact.path).is_file()),
                "step artifact must exist on non-EckyRust render path"
            );

            let manifest = load_manifest_for_bundle(&bundle)
                .expect("load manifest")
                .expect("runtime manifest");
            assert_eq!(manifest.parts.len(), 6);
            assert_eq!(manifest.document.object_count, 6);
            let manifest_ids = manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>();
            let expected_ids = expected_part_ids
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>();
            assert_eq!(manifest_ids, expected_ids);
        } else {
            let dispatch = resolve_dispatch_backend(
                &source,
                &MacroDialect::EckyIrV0,
                GeometryBackend::Build123d,
            )
            .expect("dispatch backend");
            assert_eq!(dispatch, GeometryBackend::Build123d);

            let lowered = super::lower_ecky_with_large_stack(
                "build123d",
                &source,
                &DesignParams::new(),
                crate::ecky_ir::lower_to_build123d,
            )
            .expect("build123d lower");

            assert!(
                lowered.contains("_ecky_parts"),
                "fallback readiness signal must preserve deterministic part tuple"
            );
            for part_id in expected_part_ids {
                assert!(
                    lowered.contains(&format!(r#"("{part_id}","#)),
                    "missing tuple entry for part id {part_id}"
                );
            }
        }

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn build123d_request_falls_back_to_mesh_for_wall_pattern_source() {
        let root = temp_root("build123d-wall-pattern");
        let resolver = TestResolver { root: root.clone() };
        let state = test_state(&root);

        let bundle = render_model(
            r#"(model
                (part body
                  (wall-pattern (:mode ribs :depth 0.4 :uFreq 8)
                    (extrude (circle 5) 18))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::Build123d),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("build123d wall-pattern fallback render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-ir-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(!bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn freecad_request_falls_back_to_mesh_for_wall_pattern_source() {
        let root = temp_root("freecad-wall-pattern");
        let resolver = TestResolver { root: root.clone() };
        let state = test_state(&root);

        let bundle = render_model(
            r#"(model
                (part body
                  (wall-pattern (:mode ribs :depth 0.4 :uFreq 8)
                    (extrude (circle 5) 18))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::Freecad),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("freecad wall-pattern fallback render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-ir-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(!bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_request_does_not_silently_build123d_fallback_for_shell_sampled_radial_loft()
    {
        let root = temp_root("eckyrust-shell-sampled-radial-loft");
        let resolver = TestResolver { root: root.clone() };
        let direct_capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if direct_capability.available {
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        let state = test_state(&root);

        let err = render_model(
            r#"(model
                (part body
                  (shell 2
                    (sampled-radial-loft
                      (theta z fz)
                      :height 40
                      :z-steps 6
                      :theta-steps 24
                      :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                      :z-map (+ z (* fz 2))))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect_err("EckyRust must not silently build123d fallback");

        assert_ne!(err.operation.as_deref(), Some("lower:build123d"));
        let diagnostic = format!("{} {}", err, err.details.as_deref().unwrap_or(""));
        assert!(
            diagnostic.contains("sampled-radial-loft")
                || diagnostic.contains("exact-backend-only CAD ops"),
            "unexpected error: {err:?}"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_request_does_not_silently_build123d_fallback_for_dome_style_exact_stack() {
        let root = temp_root("eckyrust-dome-style-radial-loft");
        let resolver = TestResolver { root: root.clone() };
        let direct_capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if direct_capability.available {
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        let state = test_state(&root);

        let err = render_model(
            r#"(model
                (part body
                  (translate 0 0 8
                    (difference
                      (shell 2
                        (sampled-radial-loft
                          (theta z fz)
                          :height 40
                          :z-steps 8
                          :theta-steps 32
                          :radius (+ 18 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                          :z-map (+ z (* fz 2))))
                      (translate 0 0 28 (cylinder 4 18 32))))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect_err("EckyRust must not silently build123d fallback");

        assert_ne!(err.operation.as_deref(), Some("lower:build123d"));
        let diagnostic = format!("{} {}", err, err.details.as_deref().unwrap_or(""));
        assert!(
            diagnostic.contains("sampled-radial-loft")
                || diagnostic.contains("exact-backend-only CAD ops"),
            "unexpected error: {err:?}"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_uses_direct_occt_for_sampled_radial_loft_when_sdk_ready() {
        let root = temp_root("direct-sampled-radial-loft");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let bundle = render_model(
            r#"(model
                (part body
                  (sampled-radial-loft
                    (theta z fz)
                    :height 40
                    :z-steps 6
                    :theta-steps 24
                    :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                    :z-map (+ z (* fz 2)))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("direct OCCT sampled radial loft render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));
        assert!(!bundle.edge_targets.is_empty());
        assert!(!bundle.face_targets.is_empty());

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["body"]
        );
        assert!(!manifest.selection_targets.is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_uses_direct_occt_for_helical_ridge_when_sdk_ready() {
        let root = temp_root("direct-helical-ridge-reject");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let bundle = render_model(
            r#"(model
                (part body
                  (helical-ridge
                    :radius 18
                    :pitch 3
                    :height 24
                    :base-width 1.2
                    :crest-width 0.35
                    :depth 0.6)))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("helical-ridge should route through Direct OCCT");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_uses_direct_occt_for_shell_sampled_radial_loft_when_sdk_ready() {
        let root = temp_root("direct-shell-sampled-radial-loft");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let bundle = render_model(
            r#"(model
                (part body
                  (shell 2
                    (sampled-radial-loft
                      (theta z fz)
                      :height 40
                      :z-steps 6
                      :theta-steps 24
                      :radius (+ 20 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                      :z-map (+ z (* fz 2))))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("direct OCCT sampled radial shell render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));
        assert!(!bundle.edge_targets.is_empty());
        assert!(!bundle.face_targets.is_empty());

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["body"]
        );
        assert!(!manifest.selection_targets.is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_applies_exact_edge_target_id_when_sdk_ready() {
        let root = temp_root("direct-exact-edge-target-id");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let base_bundle = render_model(
            r#"(model
                (part body (box 20 20 10)))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("base direct OCCT render");
        let edge_target_id = base_bundle
            .edge_targets
            .first()
            .and_then(|target| target.canonical_target_id.clone())
            .expect("box edge target");
        let drifted_edge_target_id = edge_target_id.replacen(":edge:0:", ":edge:999:", 1);
        assert_ne!(drifted_edge_target_id, edge_target_id);

        let exact_source = format!(
            r#"(model
                (part body
                  (fillet 1.5 :edges "target-id:{drifted_edge_target_id}" (box 20 20 10))))"#
        );
        let bundle = render_model(
            &exact_source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("exact edge target-id direct OCCT render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));
        assert!(
            edge_target_id.starts_with("body:edge:"),
            "unexpected edge target id: {edge_target_id}"
        );

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["body"]
        );
        assert!(!manifest.selection_targets.is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_applies_exact_edge_alias_target_id_when_sdk_ready() {
        let root = temp_root("direct-exact-edge-alias-target-id");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let base_bundle = render_model(
            r#"(model
                (part body (box 20 20 10)))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("base direct OCCT render");
        let base_manifest = load_manifest_for_bundle(&base_bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        let edge_alias_target_id = base_manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == crate::models::SelectionTargetKind::Edge)
            .and_then(|target| target.canonical_target_id.clone())
            .expect("box edge alias target");

        let exact_source = format!(
            r#"(model
                (part body
                  (fillet 1.5 :edges "target-id:{edge_alias_target_id}" (box 20 20 10))))"#
        );
        let bundle = render_model(
            &exact_source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("exact edge alias direct OCCT render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert!(!manifest.selection_targets.is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_applies_exact_face_target_id_for_shell_when_sdk_ready() {
        let root = temp_root("direct-exact-face-target-id");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let base_bundle = render_model(
            r#"(model
                (part body (box 20 20 10)))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("base direct OCCT render");
        let face_target_id = base_bundle
            .face_targets
            .first()
            .and_then(|target| target.canonical_target_id.clone())
            .expect("box face target");
        let drifted_face_target_id = face_target_id.replacen(":face:0:", ":face:999:", 1);
        assert_ne!(drifted_face_target_id, face_target_id);

        let exact_source = format!(
            r#"(model
                (part body
                  (shell 1.5 :faces "target-id:{drifted_face_target_id}" (box 20 20 10))))"#
        );
        let bundle = render_model(
            &exact_source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("exact face target-id direct OCCT shell render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));
        assert!(
            face_target_id.starts_with("body:face:"),
            "unexpected face target id: {face_target_id}"
        );

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["body"]
        );
        assert!(!manifest.selection_targets.is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_applies_exact_face_alias_target_id_for_shell_when_sdk_ready() {
        let root = temp_root("direct-exact-face-alias-target-id");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let base_bundle = render_model(
            r#"(model
                (part body (box 20 20 10)))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("base direct OCCT render");
        let base_manifest = load_manifest_for_bundle(&base_bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        let face_alias_target_id = base_manifest
            .selection_targets
            .iter()
            .find(|target| target.kind == crate::models::SelectionTargetKind::Face)
            .and_then(|target| target.canonical_target_id.clone())
            .expect("box face alias target");

        let exact_source = format!(
            r#"(model
                (part body
                  (shell 1.5 :faces "target-id:{face_alias_target_id}" (box 20 20 10))))"#
        );
        let bundle = render_model(
            &exact_source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("exact face alias direct OCCT shell render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert!(!manifest.selection_targets.is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn tagged_face_selector_survives_parameter_sweep_across_manifest_rebind() {
        let root = temp_root("tagged-face-parameter-sweep");
        let resolver = TestResolver { root: root.clone() };
        let state = test_state(&root);
        let build123d = crate::runtime_capabilities::probe_build123d_runtime(&resolver);
        let freecad =
            crate::runtime_capabilities::probe_freecad_runtime(Some("FreeCADCmd"), &resolver);
        let backend = if build123d.available {
            Some(GeometryBackend::Build123d)
        } else if freecad.available {
            Some(GeometryBackend::Freecad)
        } else {
            None
        };
        let Some(backend) = backend else {
            let _ = std::fs::remove_dir_all(root);
            return;
        };

        let source = r#"(model
            (params (number pedestal 2 :min 2 :max 8))
            (tag-face mounting_top :faces "top" body)
            (part body
              (fillet 1.5
                :faces (tag mounting_top)
                (union
                  (box 20 20 20)
                  (translate 0 0 (- pedestal) (box 8 8 pedestal))))))"#;

        let base_bundle = render_model(
            source,
            &DesignParams::from([("pedestal".to_string(), ParamValue::Number(2.0))]),
            Some(MacroDialect::EckyIrV0),
            Some(backend),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("base tagged render");
        let base_manifest = load_manifest_for_bundle(&base_bundle)
            .expect("load base manifest")
            .expect("base manifest");
        let base_anchor = base_manifest
            .tagged_anchors
            .get("mounting_top")
            .expect("base anchor");
        assert!(!base_anchor.target_ids.is_empty());

        let next_bundle = render_model_with_previous_manifest(
            source,
            &DesignParams::from([("pedestal".to_string(), ParamValue::Number(8.0))]),
            Some(MacroDialect::EckyIrV0),
            Some(backend),
            None,
            Some(&base_manifest),
            &state,
            &resolver,
        )
        .await
        .expect("rerender with previous manifest");
        let next_manifest = load_manifest_for_bundle(&next_bundle)
            .expect("load next manifest")
            .expect("next manifest");
        let next_anchor = next_manifest
            .tagged_anchors
            .get("mounting_top")
            .expect("next anchor");

        assert!(!next_anchor.target_ids.is_empty());
        assert_eq!(base_anchor.target_ids, next_anchor.target_ids);
        assert_ne!(
            base_anchor.canonical_target_ids, next_anchor.canonical_target_ids,
            "parameter sweep should reindex canonical face ids"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_renders_dome_style_exact_stack_via_direct_occt_when_sdk_ready() {
        let root = temp_root("direct-dome-style-radial-loft");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let bundle = render_model(
            r#"(model
                (part body
                  (translate 0 0 8
                    (difference
                      (shell 2
                        (sampled-radial-loft
                          (theta z fz)
                          :height 40
                          :z-steps 8
                          :theta-steps 32
                          :radius (+ 18 (* 2 (sin (+ (* theta 6) (* fz 3.141592653589793)))))
                          :z-map (+ z (* fz 2))))
                      (translate 0 0 28 (cylinder 4 18 32))))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("direct OCCT dome-style exact render");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));
        assert!(!bundle.edge_targets.is_empty());
        assert!(!bundle.face_targets.is_empty());

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["body"]
        );
        assert!(!manifest.selection_targets.is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_uses_direct_occt_step_when_sdk_ready() {
        let root = temp_root("direct-success");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let params = DesignParams::from([("width".to_string(), ParamValue::Number(24.0))]);
        let bundle = render_model(
            r#"(model
                (params (number width 10))
                (part body (extrude (rounded_rect width 12 2) 14)))"#,
            &params,
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("direct OCCT render");

        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_dispatch_uses_direct_occt_step_for_advanced_multi_part_when_sdk_ready() {
        let root = temp_root("direct-advanced");
        let resolver = TestResolver { root: root.clone() };
        let capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if !capability.available {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        let state = test_state(&root);
        let bundle = render_model(
            r#"(model
                (part base (fillet 0.6 (box 18 14 4)))
                (part shell (translate 28 0 0 (shell 0.8 (box 10 10 10))))
                (part lofted (translate -28 0 0 (loft 18 (circle 5) (rounded-rect 12 8 2))))
                (part pins (translate 0 -24 0 (grid-array 2 2 8 8 (cylinder 1.5 5)))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("advanced direct OCCT render");

        assert!(bundle.model_id.starts_with("generated-direct-occt-"));
        assert!(std::path::Path::new(&bundle.preview_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));

        let manifest = load_manifest_for_bundle(&bundle)
            .expect("load manifest")
            .expect("runtime manifest");
        assert_eq!(manifest.document.object_count, 4);
        assert_eq!(
            manifest
                .parts
                .iter()
                .map(|part| part.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["base", "shell", "lofted", "pins"]
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    /// Phase 7: post-processing is backend-agnostic.
    ///
    /// Render a model via the EckyRust backend, then override the bundle's
    /// `geometry_backend` to `Build123d` before running post-processing.
    /// The lithophane pipeline must produce the same 3MF output regardless of
    /// which backend generated the underlying geometry.
    #[test]
    fn post_processing_is_backend_agnostic_for_build123d_bundle() {
        #[derive(Clone)]
        struct TestResolver {
            root: std::path::PathBuf,
        }
        impl crate::models::PathResolver for TestResolver {
            fn app_config_dir(&self) -> std::path::PathBuf {
                self.root.clone()
            }
            fn app_data_dir(&self) -> std::path::PathBuf {
                self.root.clone()
            }
            fn resource_path(&self, _: &str) -> Option<std::path::PathBuf> {
                None
            }
        }

        let root = std::env::temp_dir().join(format!("ecky-phase7-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root: root.clone() };

        // Render via EckyRust to get a real bundle with actual geometry.
        let mut bundle = crate::ecky_ir::render_model(
            r#"(model (part body (extrude (rounded_rect 32 32 4 12) 10)))"#,
            &crate::models::DesignParams::new(),
            &resolver,
        )
        .expect("IR render");

        // Override the geometry_backend field to simulate a Build123d bundle.
        // This is the core of the Phase 7 invariant: post-processing must not
        // branch on the backend.
        bundle.geometry_backend = crate::models::GeometryBackend::Build123d;

        let image_path = root.join("panel.png");
        image::RgbImage::from_fn(3, 3, |x, y| {
            if (x + y) % 2 == 0 {
                image::Rgb([255u8, 255, 255])
            } else {
                image::Rgb([32, 64, 200])
            }
        })
        .save(&image_path)
        .unwrap();

        apply_requested_post_processing(
            &mut bundle,
            &crate::models::DesignParams::new(),
            Some(&PostProcessingSpec {
                displacement: None,
                lithophane_attachments: vec![LithophaneAttachment {
                    id: "panel".to_string(),
                    enabled: true,
                    source: LithophaneAttachmentSource::File {
                        image_path: image_path.to_string_lossy().to_string(),
                    },
                    target_part_id: "body".to_string(),
                    placement: LithophanePlacement {
                        mode: LithophanePlacementMode::PartSidePatch,
                        side: LithophaneSide::Front,
                        projection: ProjectionType::Planar,
                        width_mm: 24.0,
                        height_mm: 24.0,
                        offset_x_mm: 0.0,
                        offset_y_mm: 0.0,
                        rotation_deg: 0.0,
                        overflow_mode: OverflowMode::Contain,
                        bleed_margin_mm: 0.0,
                    },
                    relief: LithophaneRelief {
                        depth_mm: 1.0,
                        invert: false,
                    },
                    color: LithophaneColor {
                        mode: LithophaneColorMode::Cmyk,
                        channel_thickness_mm: 0.4,
                    },
                }],
            }),
        )
        .expect("post-processing must succeed on a Build123d-tagged bundle (Phase 7 invariant)");

        assert_eq!(
            bundle.geometry_backend,
            crate::models::GeometryBackend::Build123d,
            "geometry_backend must not be mutated by post-processing"
        );
        assert!(
            bundle
                .export_artifacts
                .iter()
                .any(|a| a.format == "3mf" && a.role == "primary"),
            "post-processing must produce a 3MF for a Build123d-tagged bundle"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
