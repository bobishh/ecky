use crate::contracts::{
    AppError, AppResult, ArtifactBundle, ComponentInterfaceValue, ComponentMateNormalMode,
    ComponentMateStatus, ComponentMirrorAxis, ComponentPlacementEvidence, DesignParams,
    DiagnosticContext, DiagnosticParamValue, GeometryBackend, GeometryProvenance,
    GeometryRepresentation, MacroDialect, ModelManifest, ParamValue, PortFrame, PortReference,
};
use crate::freecad;
use crate::models::{AppState, PathResolver};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock, Weak};

const ECKY_LOWERING_STACK_SIZE: usize = 32 * 1024 * 1024;
const ECKY_DIRECT_OCCT_DEFAULT_STACK_SIZE: usize = 64 * 1024 * 1024;
const ECKY_DIRECT_OCCT_STACK_MB_ENV: &str = "ECKY_DIRECT_OCCT_STACK_MB";
const DIRECT_OCCT_RESOURCE_SNAPSHOT_PATHS: &[&str] = &[
    "runtime/occt",
    "runtime/occt/bin/direct-occt-runner",
    "bin/direct-occt-runner",
];

fn source_render_cancellations() -> &'static Mutex<HashMap<String, Weak<AtomicBool>>> {
    static CANCELLATIONS: OnceLock<Mutex<HashMap<String, Weak<AtomicBool>>>> = OnceLock::new();
    CANCELLATIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) struct SourceRenderCancellationGuard {
    digest: String,
    cancellation: Arc<AtomicBool>,
}

impl Drop for SourceRenderCancellationGuard {
    fn drop(&mut self) {
        let mut cancellations = source_render_cancellations()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let matches_guard = cancellations
            .get(&self.digest)
            .and_then(Weak::upgrade)
            .is_some_and(|active| Arc::ptr_eq(&active, &self.cancellation));
        if matches_guard {
            cancellations.remove(&self.digest);
        }
    }
}

pub(crate) fn register_source_render_cancellation(
    source: &str,
    cancellation: Arc<AtomicBool>,
) -> SourceRenderCancellationGuard {
    let digest = crate::project_mirror::source_digest(source);
    source_render_cancellations()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(digest.clone(), Arc::downgrade(&cancellation));
    SourceRenderCancellationGuard {
        digest,
        cancellation,
    }
}

fn source_render_cancellation(source: &str) -> Option<Arc<AtomicBool>> {
    let digest = crate::project_mirror::source_digest(source);
    source_render_cancellations()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&digest)
        .and_then(Weak::upgrade)
}

struct RenderFlight {
    result: std::sync::Mutex<Option<AppResult<ArtifactBundle>>>,
    notify: tokio::sync::Notify,
}

#[derive(Clone)]
struct RenderConfigSnapshot {
    default_source_language: crate::contracts::SourceLanguage,
    default_geometry_backend: GeometryBackend,
    freecad_cmd: String,
    cad_text_font_path: String,
}

impl RenderConfigSnapshot {
    fn from_state(state: &AppState) -> Self {
        let config = state
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self {
            default_source_language: config.default_source_language,
            default_geometry_backend: config.default_geometry_backend,
            freecad_cmd: config.freecad_cmd.clone(),
            cad_text_font_path: config.cad_text_font_path.clone(),
        }
    }

    fn freecad_cmd(&self) -> Option<&str> {
        (!self.freecad_cmd.trim().is_empty()).then_some(self.freecad_cmd.trim())
    }

    fn cad_text_font_path(&self) -> Option<&str> {
        (!self.cad_text_font_path.trim().is_empty()).then_some(self.cad_text_font_path.trim())
    }
}

impl RenderFlight {
    fn pending() -> Self {
        Self {
            result: std::sync::Mutex::new(None),
            notify: tokio::sync::Notify::new(),
        }
    }
}

struct RenderFlightOwner {
    key: String,
    flight: std::sync::Arc<RenderFlight>,
    completed: bool,
}

impl RenderFlightOwner {
    fn complete(mut self, result: AppResult<ArtifactBundle>) -> AppResult<ArtifactBundle> {
        *self
            .flight
            .result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(result.clone());
        remove_render_flight(&self.key, &self.flight);
        self.completed = true;
        self.flight.notify.notify_waiters();
        result
    }
}

impl Drop for RenderFlightOwner {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let error = AppError::render("Render owner cancelled before completion.");
        *self
            .flight
            .result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Err(error));
        remove_render_flight(&self.key, &self.flight);
        self.flight.notify.notify_waiters();
    }
}

enum RenderFlightRole {
    Owner(RenderFlightOwner),
    Waiter(std::sync::Arc<RenderFlight>),
}

fn render_flights(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<RenderFlight>>> {
    static FLIGHTS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<RenderFlight>>>,
    > = std::sync::OnceLock::new();
    FLIGHTS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn acquire_render_flight(key: &str) -> RenderFlightRole {
    let mut flights = render_flights()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(flight) = flights.get(key) {
        return RenderFlightRole::Waiter(flight.clone());
    }
    let flight = std::sync::Arc::new(RenderFlight::pending());
    flights.insert(key.to_string(), flight.clone());
    RenderFlightRole::Owner(RenderFlightOwner {
        key: key.to_string(),
        flight,
        completed: false,
    })
}

fn remove_render_flight(key: &str, flight: &std::sync::Arc<RenderFlight>) {
    let mut flights = render_flights()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if flights
        .get(key)
        .is_some_and(|current| std::sync::Arc::ptr_eq(current, flight))
    {
        flights.remove(key);
    }
}

#[cfg(test)]
fn render_flight_strong_count(key: &str) -> Option<usize> {
    render_flights()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(key)
        .map(std::sync::Arc::strong_count)
}

#[cfg(test)]
fn render_flight_keys() -> Vec<String> {
    render_flights()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .keys()
        .cloned()
        .collect()
}

async fn wait_for_render_flight(flight: std::sync::Arc<RenderFlight>) -> AppResult<ArtifactBundle> {
    loop {
        let notified = flight.notify.notified();
        if let Some(result) = flight
            .result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
        {
            return result;
        }
        notified.await;
    }
}

fn render_dependency_identities(
    macro_code: &str,
    parameters: &DesignParams,
    app: &dyn PathResolver,
) -> Vec<(String, String)> {
    use crate::ecky_cad_host::direct_occt::{OcctArg, OcctOp};
    use sha2::{Digest, Sha256};

    let mut identities = Vec::new();
    if let Some(Ok(program)) = crate::ecky_scheme::try_compile_to_core_program(macro_code) {
        let parameters = parameters.clone();
        if let Ok(plan) = run_direct_occt_with_large_stack("dependency-plan", move || {
            crate::ecky_cad_host::direct_occt::plan_core_program_with_params(&program, &parameters)
                .map_err(AppError::from)
        }) {
            for command in plan.parts.iter().flat_map(|part| part.commands.iter()) {
                if command.op != OcctOp::ImportStl {
                    continue;
                }
                let Some(path) = command.args.first().and_then(|arg| match arg {
                    OcctArg::Text(path) | OcctArg::Symbol(path) => Some(path.as_str()),
                    _ => None,
                }) else {
                    continue;
                };
                let digest = fs::read(path)
                    .map(|bytes| format!("sha256:{:x}", Sha256::digest(bytes)))
                    .unwrap_or_else(|err| format!("unreadable:{err}"));
                identities.push((format!("import-stl:{path}"), digest));
            }
        }
    }

    if let Some(runner) =
        crate::ecky_cad_host::direct_occt_runner::discover_direct_occt_runner_with_mode(app, true)
    {
        let digest = fs::read(&runner)
            .map(|bytes| format!("sha256:{:x}", Sha256::digest(bytes)))
            .unwrap_or_else(|err| format!("unreadable:{err}"));
        identities.push((format!("direct-occt-runner:{}", runner.display()), digest));
        if let Some(runtime_root) = runner.parent().and_then(Path::parent) {
            let manifest = runtime_root.join("manifest.json");
            if manifest.is_file() {
                let digest = fs::read(&manifest)
                    .map(|bytes| format!("sha256:{:x}", Sha256::digest(bytes)))
                    .unwrap_or_else(|err| format!("unreadable:{err}"));
                identities.push((format!("runtime-manifest:{}", manifest.display()), digest));
            }
        }
    }
    identities.sort();
    identities.dedup();
    identities
}

fn post_processing_asset_identities(
    parameters: &DesignParams,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
) -> Vec<(String, String)> {
    use sha2::{Digest, Sha256};

    if crate::contracts::normalize_post_processing_spec(post_processing.cloned()).is_none() {
        return Vec::new();
    }
    let mut identities = parameters
        .iter()
        .filter_map(|(key, value)| match value {
            crate::contracts::ParamValue::String(path) if Path::new(path).is_file() => {
                let digest = fs::read(path)
                    .map(|bytes| format!("sha256:{:x}", Sha256::digest(bytes)))
                    .unwrap_or_else(|error| format!("unreadable:{error}"));
                Some((format!("parameter-file:{key}:{path}"), digest))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    identities.sort();
    identities
}

fn render_flight_key(
    macro_code: &str,
    parameters: &DesignParams,
    macro_dialect: Option<&MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    previous_manifest: Option<&ModelManifest>,
    config: &RenderConfigSnapshot,
    app: &dyn PathResolver,
) -> AppResult<String> {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct PreviousManifestFlightIdentity<'a> {
        schema_version: u32,
        model_id: &'a str,
        source_digest: Option<&'a str>,
        core_digest: Option<&'a str>,
        ast_schema_version: Option<u32>,
        tagged_anchors:
            &'a std::collections::BTreeMap<String, crate::contracts::TaggedAnchorBinding>,
    }

    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct RenderFlightIdentity<'a> {
        schema_version: u32,
        macro_code: &'a str,
        parameters: &'a DesignParams,
        macro_dialect: Option<&'a MacroDialect>,
        geometry_backend: Option<GeometryBackend>,
        post_processing: Option<&'a crate::contracts::PostProcessingSpec>,
        previous_manifest: Option<PreviousManifestFlightIdentity<'a>>,
        default_source_language: crate::contracts::SourceLanguage,
        default_geometry_backend: GeometryBackend,
        freecad_cmd: &'a str,
        cad_text_font_path: &'a str,
        dependency_identities: Vec<(String, String)>,
        post_processing_asset_identities: Vec<(String, String)>,
        app_config_dir: PathBuf,
        app_data_dir: PathBuf,
    }

    let previous_manifest = previous_manifest.map(|manifest| PreviousManifestFlightIdentity {
        schema_version: manifest.schema_version,
        model_id: &manifest.model_id,
        source_digest: manifest.source_digest.as_deref(),
        core_digest: manifest.core_digest.as_deref(),
        ast_schema_version: manifest.ast_schema_version,
        tagged_anchors: &manifest.tagged_anchors,
    });
    let dependency_identities = render_dependency_identities(macro_code, parameters, app);
    let post_processing_asset_identities =
        post_processing_asset_identities(parameters, post_processing);
    let identity = RenderFlightIdentity {
        schema_version: 1,
        macro_code,
        parameters,
        macro_dialect,
        geometry_backend,
        post_processing,
        previous_manifest,
        default_source_language: config.default_source_language,
        default_geometry_backend: config.default_geometry_backend,
        freecad_cmd: &config.freecad_cmd,
        cad_text_font_path: &config.cad_text_font_path,
        dependency_identities,
        post_processing_asset_identities,
        app_config_dir: app.app_config_dir(),
        app_data_dir: app.app_data_dir(),
    };
    let encoded = serde_json::to_vec(&identity).map_err(|err| {
        AppError::internal(format!(
            "Failed to encode render singleflight identity: {err}"
        ))
    })?;
    use sha2::{Digest, Sha256};
    Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
}

fn cache_salted_render_source(
    source: &str,
    dialect: &MacroDialect,
    render_input_digest: &str,
) -> String {
    let comment = match dialect {
        MacroDialect::EckyIrV0 | MacroDialect::Build123d => ";",
        MacroDialect::CadFrameworkV1 | MacroDialect::Legacy => "#",
    };
    format!(
        "{}\n{} eckyRenderCacheIdentity {}\n",
        source.trim_end(),
        comment,
        render_input_digest
    )
}

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
        CoreOperation::Frame(CoreFrameOp::ClipPlane) => "clip-plane".to_string(),
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
    model_stl_path: &str,
    bundle: &mut ArtifactBundle,
) -> AppResult<()> {
    let stl_path = Path::new(model_stl_path);
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

    let stl_path = Path::new(&bundle.model_stl_path);

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
            let preview_path = bundle.model_stl_path.clone();
            update_content_hash_and_exports(&preview_path, bundle)?;
            return Ok(());
        }
    }

    if let Some(disp) = &post_proc.displacement {
        let Some(crate::contracts::ParamValue::String(image_path)) =
            parameters.get(&disp.image_param)
        else {
            return Ok(());
        };
        if image_path.trim().is_empty() {
            return Ok(());
        }
        crate::displacement::apply(stl_path, image_path, disp, stl_path)?;
        bundle.export_artifacts.clear();
        let preview_path = bundle.model_stl_path.clone();
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
    let Some(input_digest) = post_processing_input_digest(parameters, post_processing)? else {
        evict_cache_preserving_bundle(app, &bundle)?;
        return Ok(bundle);
    };
    if post_processing_marker_matches(&bundle, &input_digest) {
        evict_cache_preserving_bundle(app, &bundle)?;
        return Ok(bundle);
    }
    apply_requested_post_processing(&mut bundle, parameters, post_processing).map_err(|err| {
        attach_diagnostic_context(err, None, parameters, Some("export:post-processing"))
    })?;
    write_post_processing_marker(&bundle, &input_digest)?;
    evict_cache_preserving_bundle(app, &bundle)?;
    Ok(bundle)
}

fn evict_cache_preserving_bundle(app: &dyn PathResolver, bundle: &ArtifactBundle) -> AppResult<()> {
    let runtime_cache_dir = freecad::runtime_cache_dir(app)?;
    if let Some(bundle_dir) = Path::new(&bundle.model_stl_path).parent() {
        freecad::evict_cache_if_needed_except(&runtime_cache_dir, bundle_dir);
    } else {
        freecad::evict_cache_if_needed(&runtime_cache_dir);
    }
    Ok(())
}

const POST_PROCESSING_MARKER_FILE: &str = "post-processing-cache.json";

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PostProcessingCacheMarker {
    schema_version: u32,
    input_digest: String,
    preview_digest: String,
}

fn post_processing_input_digest(
    parameters: &DesignParams,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
) -> AppResult<Option<String>> {
    let Some(normalized) =
        crate::contracts::normalize_post_processing_spec(post_processing.cloned())
    else {
        return Ok(None);
    };
    let encoded = serde_json::to_vec(&(
        "post-processing-v1",
        parameters,
        normalized,
        post_processing_asset_identities(parameters, post_processing),
    ))
    .map_err(|error| {
        AppError::validation(format!("Cannot digest post-processing inputs: {error}"))
    })?;
    use sha2::{Digest, Sha256};
    Ok(Some(format!("sha256:{:x}", Sha256::digest(encoded))))
}

fn preview_file_digest(bundle: &ArtifactBundle) -> Option<String> {
    use sha2::{Digest, Sha256};
    fs::read(&bundle.model_stl_path)
        .ok()
        .map(|bytes| format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn post_processing_marker_path(bundle: &ArtifactBundle) -> Option<PathBuf> {
    Path::new(&bundle.model_stl_path)
        .parent()
        .map(|directory| directory.join(POST_PROCESSING_MARKER_FILE))
}

fn post_processing_marker_matches(bundle: &ArtifactBundle, input_digest: &str) -> bool {
    let Some(path) = post_processing_marker_path(bundle) else {
        return false;
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(marker) = serde_json::from_str::<PostProcessingCacheMarker>(&raw) else {
        return false;
    };
    marker.schema_version == 1
        && marker.input_digest == input_digest
        && preview_file_digest(bundle).as_deref() == Some(marker.preview_digest.as_str())
}

fn write_post_processing_marker(bundle: &ArtifactBundle, input_digest: &str) -> AppResult<()> {
    let path = post_processing_marker_path(bundle).ok_or_else(|| {
        AppError::persistence("Cannot locate runtime directory for post-processing cache marker.")
    })?;
    let preview_digest = preview_file_digest(bundle).ok_or_else(|| {
        AppError::persistence(format!(
            "Cannot digest post-processed preview '{}'.",
            bundle.model_stl_path
        ))
    })?;
    let marker = PostProcessingCacheMarker {
        schema_version: 1,
        input_digest: input_digest.to_string(),
        preview_digest,
    };
    let encoded = serde_json::to_vec_pretty(&marker)
        .map_err(|error| AppError::persistence(error.to_string()))?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, encoded).map_err(|error| {
        AppError::persistence(format!(
            "Failed to write post-processing cache marker '{}': {}",
            temporary.display(),
            error
        ))
    })?;
    fs::rename(&temporary, &path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        AppError::persistence(format!(
            "Failed to publish post-processing cache marker '{}': {}",
            path.display(),
            error
        ))
    })?;
    Ok(())
}

fn persist_authored_source_digest(
    mut bundle: ArtifactBundle,
    authored_source: &str,
    parameters: &DesignParams,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    if let Some(path) = bundle.macro_path.as_deref() {
        fs::write(path, authored_source).map_err(|error| {
            AppError::persistence(format!(
                "Failed to persist exact authored source '{}': {}",
                path, error
            ))
        })?;
    }
    let mut manifest = crate::model_runtime::read_model_manifest(app, &bundle.model_id)?;
    manifest.source_digest = Some(crate::services::render_snapshot::canonical_source_digest(
        authored_source,
    ));
    let placement_evidence = if bundle.source_language == crate::contracts::SourceLanguage::EckyIrV0
    {
        component_placement_evidence_from_source(authored_source, parameters)?
    } else {
        Vec::new()
    };
    bundle.component_placement_evidence = placement_evidence.clone();
    manifest.component_placement_evidence = placement_evidence;
    crate::model_runtime::write_runtime_bundle(app, &bundle.model_id, &bundle, &manifest)
        .map(|(stored_bundle, _)| stored_bundle)
}

fn component_placement_evidence_from_source(
    authored_source: &str,
    parameters: &DesignParams,
) -> AppResult<Vec<ComponentPlacementEvidence>> {
    let numeric_parameters = parameters
        .iter()
        .filter_map(|(key, value)| match value {
            ParamValue::Number(value) => Some((key.clone(), *value)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    Ok(
        crate::ecky_scheme::compiler::inspect_component_placement_evidence(
            authored_source,
            &numeric_parameters,
        )
        .map_err(crate::ecky_scheme::core_err_to_app)?
        .into_iter()
        .map(|evidence| ComponentPlacementEvidence {
            instance_id: evidence.instance_id,
            component_id: evidence.component_id,
            source_port_ref: PortReference {
                instance_id: evidence.source_instance_id,
                port_id: evidence.source_port_id,
            },
            target_port_ref: PortReference {
                instance_id: evidence.target_instance_id,
                port_id: evidence.target_port_id,
            },
            placement_frame: PortFrame {
                origin: evidence.placement_frame.origin,
                x_axis: evidence.placement_frame.x_axis,
                y_axis: evidence.placement_frame.y_axis,
                z_axis: evidence.placement_frame.z_axis,
            },
            normal_mode: match evidence.normal_mode {
                ecky_render::component_placement::MateNormalMode::Aligned => {
                    ComponentMateNormalMode::Aligned
                }
                ecky_render::component_placement::MateNormalMode::Opposed => {
                    ComponentMateNormalMode::Opposed
                }
            },
            roll_degrees: evidence.roll_degrees,
            offset: evidence.offset,
            mirror_axis: evidence.mirror_axis.map(|axis| match axis {
                ecky_render::component_placement::MirrorAxis::X => ComponentMirrorAxis::X,
                ecky_render::component_placement::MirrorAxis::Y => ComponentMirrorAxis::Y,
            }),
            mate_status: ComponentMateStatus::Solved,
            resolved_fit_values: evidence
                .resolved_fit_values
                .into_iter()
                .map(|(key, value)| (key, ComponentInterfaceValue::Number(value)))
                .collect(),
            diagnostics: Vec::new(),
            source_start: evidence.source_start,
            source_end: evidence.source_end,
        })
        .collect::<Vec<_>>(),
    )
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
    config: &RenderConfigSnapshot,
    app: &dyn PathResolver,
    cancellation: Option<Arc<AtomicBool>>,
) -> AppResult<Option<ArtifactBundle>> {
    if *effective_dialect != MacroDialect::EckyIrV0 {
        return Ok(None);
    }
    let macro_code = macro_code.to_string();
    let parameters = parameters.clone();
    let previous_manifest = previous_manifest.cloned();
    let app = DirectOcctThreadResolver::from_resolver(app);
    let cad_text_font_path = config.cad_text_font_path().map(str::to_string);
    run_direct_occt_with_large_stack("render", move || {
        crate::ecky_cad_host::direct_occt_runner::with_runner_cancellation(cancellation, || {
            let program = match crate::ecky_scheme::compile_to_core_program(&macro_code) {
                Ok(program) => program,
                Err(_) => return Ok(None),
            };
            let program = crate::topology_target_ids::rebind_program_tagged_selectors(
                &program,
                previous_manifest.as_ref(),
            )?;
            let runtime_root =
                match crate::runtime_capabilities::resolve_direct_occt_runtime_root(&app) {
                    Ok(runtime_root) => runtime_root,
                    Err(_) => return Ok(None),
                };
            let layout = crate::ecky_cad_host::direct_occt_sdk::inspect_occt_runtime(&runtime_root);
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
    })
}

/// Attempt poly BRep bridge rendering when one OCCT program contains mesh
/// geometry. Phase 1 renders Hybrid mesh islands and complete PureMesh parts.
/// Phase 2 feeds their STL into OCCT as `solidify(import-stl(...))`, alongside
/// unchanged PureOcct geometry and Hybrid post-boundary operations.
///
/// Returns `Ok(None)` when no mesh geometry must cross into OCCT.
fn try_render_hybrid_poly_brep(
    macro_code: &str,
    parameters: &DesignParams,
    config: &RenderConfigSnapshot,
    app: &dyn PathResolver,
) -> AppResult<Option<ArtifactBundle>> {
    let macro_code_owned = macro_code.to_string();
    let parameters_owned = parameters.clone();
    let app_clone = DirectOcctThreadResolver::from_resolver(app);
    let state_font_path = config.cad_text_font_path().map(str::to_string);

    // Compile + partition analysis needs the large stack (deep trees).
    run_direct_occt_with_large_stack("hybrid-partition", move || {
        let program = match crate::ecky_scheme::try_compile_to_core_program(&macro_code_owned) {
            Some(Ok(program)) => program,
            _ => return Ok(None),
        };

        let partitions = crate::ecky_ir::poly_partition::analyze_program(&program);
        if !crate::ecky_ir::poly_partition::requires_poly_brep_bridge(&partitions) {
            return Ok(None);
        }
        let surface_op_admission_issues =
            crate::ecky_ir::poly_partition::mesh_origin_surface_op_admission_issues(&program);
        if let Some(issue) = surface_op_admission_issues.first() {
            return Err(AppError::validation(format!(
                "Mesh-origin faceted BRep `{}` rejected before OCCT kernel execution: selector `{}` at Core node {} in part {}. {}.",
                issue.operation,
                issue.selector,
                issue.node_id.raw(),
                issue.part_index,
                issue.reason
            )));
        }
        let runtime_root =
            crate::runtime_capabilities::resolve_direct_occt_runtime_root(&app_clone)?;
        let layout = crate::ecky_cad_host::direct_occt_sdk::inspect_occt_runtime(&runtime_root);

        // Phase 1: resolve each mesh island to the engine-independent
        // MeshAsset contract. Today EckyRust is one producer; imported or
        // generated STL producers enter through the same contract.
        let mut mesh_assets = std::collections::HashMap::new();
        let mut source_mesh_digests = Vec::new();
        let mut source_mesh_boundary_or_non_manifold_edges = 0_u64;
        let mut manifold_route_notes = Vec::new();
        for (part_index, partition) in partitions.iter().enumerate() {
            let part = &program.parts[part_index];
            let output_node_ids = crate::ecky_ir::poly_partition::mesh_bridge_output_node_ids(
                &program, part_index, partition,
            );
            for output_node_id in output_node_ids {
                let mut mesh_program =
                    crate::ecky_ir::poly_partition::clone_program_for_mesh_output(
                        &program,
                        part_index,
                        output_node_id,
                    )
                    .ok_or_else(|| {
                        AppError::internal(format!(
                            "Poly BRep bridge mesh slice missing for part '{}' node {}.",
                            part.key,
                            output_node_id.raw()
                        ))
                    })?;
                let preludes =
                    crate::ecky_ir::poly_partition::exact_mesh_prelude_node_ids(&mesh_program);
                for (prelude_part_index, prelude_node_id) in preludes {
                    let prelude_program =
                        crate::ecky_ir::poly_partition::clone_program_for_exact_mesh_prelude(
                            &mesh_program,
                            prelude_part_index,
                            prelude_node_id,
                        )
                        .ok_or_else(|| {
                            AppError::internal(format!(
                                "Hybrid exact mesh prelude missing for part '{}' node {}.",
                                part.key,
                                prelude_node_id.raw()
                            ))
                        })?;
                    let (prelude_bundle, _) = crate::ecky_cad_host::direct_occt_runtime::
                        render_core_program_runtime_bundle_with_font_path(
                            &prelude_program,
                            &format!(
                                "{macro_code_owned}\n;; hybrid-exact-prelude:{}:{}",
                                part.key,
                                prelude_node_id.raw()
                            ),
                            &parameters_owned,
                            &layout,
                            &app_clone,
                            state_font_path.as_deref(),
                        )
                        .map_err(|err| {
                            AppError::render(format!(
                                "Hybrid OCCT pre-mesh phase failed for part '{}' node {}: {}",
                                part.key,
                                prelude_node_id.raw(),
                                format_nested_app_error(&err)
                            ))
                        })?;
                    crate::ecky_ir::poly_partition::replace_node_with_mesh_asset(
                        &mut mesh_program,
                        prelude_node_id,
                        &prelude_bundle.model_stl_path,
                    );
                }
                let mesh_bundle = crate::ecky_ir::render_core_program(
                    &mesh_program,
                    &format!(
                        "{macro_code_owned}\n;; hybrid-mesh-phase:{}:{}",
                        part.key,
                        output_node_id.raw()
                    ),
                    &parameters_owned,
                    &app_clone,
                )
                .map_err(|err| {
                    AppError::render(format!(
                        "Hybrid mesh phase failed for part '{}' node {}: {}",
                        part.key,
                        output_node_id.raw(),
                        format_nested_app_error(&err)
                    ))
                })?;
                let mesh_part_path = match mesh_bundle.viewer_assets.as_slice() {
                    [asset] => std::path::PathBuf::from(&asset.path),
                    assets => {
                        return Err(AppError::internal(format!(
                            "Hybrid mesh slice for part '{}' node {} produced {} viewer assets; expected exactly one indexed mesh island.",
                            part.key,
                            output_node_id.raw(),
                            assets.len()
                        )));
                    }
                };
                let indexed_asset = crate::ecky_ir::mesh_asset::IndexedMeshAsset::read_cache(
                    &mesh_part_path.with_extension("indexed-mesh.json"),
                )?;
                if let Err(error) = indexed_asset.validate_for_boolean() {
                    manifold_route_notes.push(format!(
                        "Manifold route skipped for part '{}' node {} during admission: {}. Explicit OCCT solidify route retained.",
                        part.key,
                        output_node_id.raw(),
                        error
                    ));
                }
                let asset = crate::ecky_ir::mesh_asset::MeshAsset::ecky_mesh_phase(
                    part.key.clone(),
                    output_node_id,
                    &mesh_part_path,
                )?;
                let topology = indexed_asset.topology();
                let mesh_non_manifold = topology.boundary_edge_count
                    + topology.non_manifold_edge_count
                    + topology.winding_mismatch_count;
                if crate::ecky_ir::poly_partition::mesh_output_contains_open_mesh(
                    &program,
                    part_index,
                    output_node_id,
                ) && mesh_non_manifold > 0
                {
                    let consumer =
                        crate::ecky_ir::poly_partition::open_mesh_brep_consumer_operation(
                            &program, part_index,
                        )
                        .unwrap_or("unknown BRep operation");
                    return Err(AppError::render(format!(
                        "Hybrid mesh asset for part '{}' node {} cannot enter solidification: open `mesh` has {mesh_non_manifold} boundary/non-manifold edges before consumer `{consumer}`. Use `polyhedron` with closed topology before the BRep consumer.",
                        part.key, output_node_id.raw()
                    )));
                }
                source_mesh_digests.push(indexed_asset.content_digest().to_string());
                source_mesh_boundary_or_non_manifold_edges += mesh_non_manifold as u64;
                mesh_assets.insert(output_node_id, asset);
            }
        }

        // Phase 2: OCCT — replace wall-pattern with solidify(import-stl(stl)).
        // Use a high starting ID to avoid collisions with existing node IDs.
        let next_node_id = crate::ecky_core_ir::NodeId::new(1_000_000);
        let occt_program =
            crate::ecky_ir::poly_partition::clone_program_for_occt_phase_with_mesh_assets(
                &program,
                &partitions,
                &mesh_assets,
                next_node_id,
            )?;

        let (mut bundle, mut manifest) =
            crate::ecky_cad_host::direct_occt_runtime::render_core_program_runtime_bundle_with_font_path(
                &occt_program,
                &macro_code_owned,
                &parameters_owned,
                &layout,
                &app_clone,
                state_font_path.as_deref(),
            )
            .map_err(|err| {
                AppError::render(format!(
                    "Hybrid OCCT phase failed after mesh solidification: {}",
                    format_nested_app_error(&err)
                ))
            })?;

        for asset in &bundle.viewer_assets {
            let non_manifold =
                crate::services::structural_verification::model_stl_non_manifold_edge_count(
                    std::path::Path::new(&asset.path),
                )?;
            if non_manifold >= 100 {
                return Err(AppError::render(format!(
                    "Hybrid OCCT part '{}' has {non_manifold} non-manifold edges (limit < 100). Increase mesh tessellation density or simplify the displacement pattern.",
                    asset.part_id
                )));
            }
        }
        let non_manifold =
            crate::services::structural_verification::model_stl_non_manifold_edge_count(
                std::path::Path::new(&bundle.model_stl_path),
            )?;
        if non_manifold >= 100 {
            return Err(AppError::render(format!(
                "Hybrid OCCT assembly has {non_manifold} non-manifold edges (limit < 100). Increase mesh tessellation density or simplify the displacement pattern."
            )));
        }

        let used_manifold_route = !bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format.eq_ignore_ascii_case("step"));
        let geometry_provenance = GeometryProvenance {
            representation: if used_manifold_route {
                GeometryRepresentation::MeshNative
            } else {
                GeometryRepresentation::FacetedPolyBrep
            },
            source_mesh_digests,
            closed: Some(source_mesh_boundary_or_non_manifold_edges == 0),
            boundary_or_non_manifold_edge_count: Some(source_mesh_boundary_or_non_manifold_edges),
        };
        bundle.geometry_provenance = Some(geometry_provenance.clone());
        for artifact in &mut bundle.export_artifacts {
            if artifact.format.eq_ignore_ascii_case("step") {
                artifact.geometry_provenance = Some(geometry_provenance.clone());
            }
        }
        manifest.geometry_provenance = Some(geometry_provenance);

        // Tag stored artifact truth so UI/MCP consumers know the bridge
        // produced faceted STEP rather than analytic source CAD.
        let bridged_count = partitions
            .iter()
            .filter(|partition| {
                partition.strategy != crate::ecky_ir::poly_partition::PartRenderStrategy::PureOcct
            })
            .count();
        let warning = if used_manifold_route {
            format!(
                "Mesh Boolean route: {bridged_count} part(s) rendered from validated indexed mesh + in-memory OCCT tessellation through Manifold; STL retained as mesh-native output and no STEP fallback was fabricated."
            )
        } else {
            format!(
                "Poly BRep bridge: {bridged_count} part(s) rendered through mesh generation + OCCT solidify (mesh → import-stl → solidify); STEP representation is faceted poly-BRep, not analytic source CAD."
            )
        };
        if !manifest.warnings.iter().any(|w| w == &warning) {
            manifest.warnings.push(warning);
        }
        for note in manifold_route_notes {
            if !manifest.warnings.iter().any(|warning| warning == &note) {
                manifest.warnings.push(note);
            }
        }
        manifest.source_digest = Some(crate::services::render_snapshot::canonical_source_digest(
            &macro_code_owned,
        ));
        let model_id = bundle.model_id.clone();
        let (bundle, _) =
            crate::model_runtime::write_runtime_bundle(&app_clone, &model_id, &bundle, &manifest)?;

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
    let program = match ecky_render::scheme::try_compile_to_core_program(macro_code) {
        Some(Ok(program)) => program,
        Some(Err(err)) => {
            return Err(format!(
                "Ecky compiler rejected model.\n{}",
                err.render_with_source(macro_code)
            ));
        }
        None => return Err("Ecky compiler did not recognize source as Ecky Core IR.".to_string()),
    };
    let parameters = parameters.clone();
    run_direct_occt_with_large_stack("plan", move || {
        crate::ecky_cad_host::direct_occt::plan_core_program_with_params(&program, &parameters)
            .map(|_| ())
            .map_err(AppError::from)
    })
    .map_err(|err| {
        let message = format_nested_app_error(&err);
        format!("Direct OCCT planner rejected model. {}", message)
    })
}

fn unsupported_required_direct_occt_error(details: String) -> AppError {
    AppError::with_details(
        crate::contracts::AppErrorCode::Validation,
        "Direct OCCT required for this Ecky Native model. Native render unavailable.",
        details,
    )
}

fn blocked_direct_occt_native_error(details: String) -> AppError {
    AppError::with_details(
        crate::contracts::AppErrorCode::Validation,
        "Ecky Native direct OCCT render failed.",
        details,
    )
}

/// Synchronous CLI seam. The CLI supplies an Ecky source and an explicit backend;
/// this keeps argument and output ownership outside of the render service.
pub fn render_cli_ecky(
    source: &str,
    parameters: &DesignParams,
    geometry_backend: GeometryBackend,
    configured_freecad_cmd: Option<&str>,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let config = RenderConfigSnapshot {
        default_source_language: crate::contracts::SourceLanguage::EckyIrV0,
        default_geometry_backend: geometry_backend,
        freecad_cmd: configured_freecad_cmd.unwrap_or_default().to_string(),
        cad_text_font_path: String::new(),
    };
    render_model_unlocked(
        source,
        parameters,
        Some(MacroDialect::EckyIrV0),
        Some(geometry_backend),
        None,
        None,
        &config,
        app,
        None,
    )
}

pub async fn render_stl(
    macro_code: &str,
    parameters: &DesignParams,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<String> {
    let _guard = state.acquire_geometry_render().await;
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
    render_model_with_previous_manifest_internal(
        macro_code,
        parameters,
        macro_dialect,
        geometry_backend,
        post_processing,
        previous_manifest,
        false,
        state,
        app,
    )
    .await
}

pub async fn render_model_with_previous_manifest_silent(
    macro_code: &str,
    parameters: &DesignParams,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    previous_manifest: Option<&ModelManifest>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    render_model_with_previous_manifest_internal(
        macro_code,
        parameters,
        macro_dialect,
        geometry_backend,
        post_processing,
        previous_manifest,
        true,
        state,
        app,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn render_model_with_previous_manifest_internal(
    macro_code: &str,
    parameters: &DesignParams,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    previous_manifest: Option<&ModelManifest>,
    silent: bool,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    if crate::component_import_runtime::source_has_live_component_import(macro_code) {
        let previous_bundle = previous_manifest
            .map(|manifest| crate::model_runtime::read_artifact_bundle(app, &manifest.model_id))
            .transpose()?;
        return render_model_with_component_lock_internal(
            macro_code,
            parameters,
            macro_dialect,
            geometry_backend,
            post_processing,
            previous_manifest,
            previous_bundle
                .as_ref()
                .and_then(|bundle| bundle.component_dependency_lock.as_ref()),
            silent,
            state,
            app,
        )
        .await;
    }
    render_model_with_previous_manifest_resolved(
        macro_code,
        parameters,
        macro_dialect,
        geometry_backend,
        post_processing,
        previous_manifest,
        silent,
        state,
        app,
    )
    .await
}

/// Host package-aware render seam. Project apply and historical re-render pass
/// their committed lock explicitly; unlocked first preview receives a
/// candidate lock in the returned ArtifactBundle.
#[allow(clippy::too_many_arguments)]
pub async fn render_model_with_component_lock(
    authored_source: &str,
    parameters: &DesignParams,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    previous_manifest: Option<&ModelManifest>,
    expected_lock: Option<&crate::contracts::ComponentDependencyLock>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    render_model_with_component_lock_internal(
        authored_source,
        parameters,
        macro_dialect,
        geometry_backend,
        post_processing,
        previous_manifest,
        expected_lock,
        false,
        state,
        app,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn render_model_with_component_lock_internal(
    authored_source: &str,
    parameters: &DesignParams,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    previous_manifest: Option<&ModelManifest>,
    expected_lock: Option<&crate::contracts::ComponentDependencyLock>,
    silent: bool,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let resolver = crate::component_import_runtime::InstalledLibraryComponentResolver { app };
    let compilation = crate::component_import_runtime::compile_authoring_source(
        crate::component_import_runtime::ResolveAuthoringSourceRequest {
            authored_source,
            expected_lock,
        },
        &resolver,
    )?;
    let _component_payload_pins = crate::component_package_runtime::pin_component_store_payloads(
        compilation
            .dependency_lock
            .dependencies
            .iter()
            .map(|dependency| dependency.package_digest.clone()),
    );
    let lock_digest = crate::services::render_snapshot::component_dependency_lock_digest(
        &compilation.dependency_lock,
    )?;
    // Runtime model ids and backend caches hash source identity. The lock salt
    // prevents byte-identical package exports at different immutable
    // coordinates from sharing one artifact directory.
    let render_source = format!(
        "; componentDependencyLockDigest {lock_digest}\n{}",
        compilation.compiler_source
    );
    let mut bundle = render_model_with_previous_manifest_resolved(
        &render_source,
        parameters,
        macro_dialect,
        geometry_backend,
        post_processing,
        previous_manifest,
        silent,
        state,
        app,
    )
    .await?;
    if let Some(path) = bundle.macro_path.as_deref() {
        fs::write(path, authored_source).map_err(|error| {
            AppError::persistence(format!(
                "Failed to persist authored live-reference source '{}': {}",
                path, error
            ))
        })?;
    }
    let mut manifest = crate::model_runtime::read_model_manifest(app, &bundle.model_id)?;
    manifest.source_digest = Some(crate::services::render_snapshot::canonical_source_digest(
        authored_source,
    ));
    crate::component_import_runtime::attach_resolved_component_evidence(
        &mut bundle,
        &mut manifest,
        &compilation,
    )?;
    crate::model_runtime::write_runtime_bundle(app, &bundle.model_id, &bundle, &manifest)
        .map(|(stored_bundle, _)| stored_bundle)
}

/// Explicit dependency-upgrade preview. Unlike ordinary historical re-render,
/// this deliberately resolves the authored exact coordinates without the
/// previous version's lock. The returned bundle owns a candidate new lock;
/// callers commit it as a new message version. The prior bundle is never
/// mutated.
#[allow(clippy::too_many_arguments)]
pub async fn render_model_with_dependency_upgrade(
    authored_source: &str,
    parameters: &DesignParams,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    previous_manifest: Option<&ModelManifest>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    render_model_with_component_lock(
        authored_source,
        parameters,
        macro_dialect,
        geometry_backend,
        post_processing,
        previous_manifest,
        None,
        state,
        app,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn render_model_with_previous_manifest_resolved(
    macro_code: &str,
    parameters: &DesignParams,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    previous_manifest: Option<&ModelManifest>,
    silent: bool,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let config = RenderConfigSnapshot::from_state(state);
    let flight_key = render_flight_key(
        macro_code,
        parameters,
        macro_dialect.as_ref(),
        geometry_backend,
        post_processing,
        previous_manifest,
        &config,
        app,
    )?;
    let owner = match acquire_render_flight(&flight_key) {
        RenderFlightRole::Waiter(flight) => return wait_for_render_flight(flight).await,
        RenderFlightRole::Owner(owner) => owner,
    };

    let effective_dialect = macro_dialect
        .clone()
        .unwrap_or(match config.default_source_language {
            crate::contracts::SourceLanguage::EckyIrV0 => MacroDialect::EckyIrV0,
            crate::contracts::SourceLanguage::Build123d => MacroDialect::Build123d,
            crate::contracts::SourceLanguage::LegacyPython => MacroDialect::Legacy,
        });
    let cache_source = cache_salted_render_source(macro_code, &effective_dialect, &flight_key);
    let source_cancellation = source_render_cancellation(macro_code);

    let _guard = if silent {
        state.acquire_geometry_render_silent().await
    } else {
        state.acquire_geometry_render().await
    };
    let first_attempt = render_model_unlocked(
        &cache_source,
        parameters,
        macro_dialect.clone(),
        geometry_backend,
        post_processing,
        previous_manifest,
        &config,
        app,
        source_cancellation.clone(),
    );
    let result = match first_attempt {
        Ok(bundle) => Ok(bundle),
        Err(err)
            if previous_manifest.is_some()
                && source_has_selector_tags(macro_code)
                && is_tagged_selector_mismatch_error(&err) =>
        {
            let bundle = render_model_unlocked(
                &cache_source,
                parameters,
                macro_dialect,
                geometry_backend,
                post_processing,
                None,
                &config,
                app,
                source_cancellation,
            )?;
            append_tagged_selector_rebind_warning(app, &bundle);
            Ok(bundle)
        }
        Err(err) => Err(err),
    };
    owner.complete(
        result
            .and_then(|bundle| persist_authored_source_digest(bundle, macro_code, parameters, app)),
    )
}

fn render_model_unlocked(
    macro_code: &str,
    parameters: &DesignParams,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    previous_manifest: Option<&ModelManifest>,
    config: &RenderConfigSnapshot,
    app: &dyn PathResolver,
    source_cancellation: Option<Arc<AtomicBool>>,
) -> AppResult<ArtifactBundle> {
    let configured_dialect = match config.default_source_language {
        crate::contracts::SourceLanguage::EckyIrV0 => MacroDialect::EckyIrV0,
        crate::contracts::SourceLanguage::Build123d => MacroDialect::Build123d,
        crate::contracts::SourceLanguage::LegacyPython => MacroDialect::Legacy,
    };
    let effective_dialect = macro_dialect.unwrap_or(configured_dialect);
    if effective_dialect == MacroDialect::Build123d {
        return Err(AppError::validation(
            "build123d source/runtime was removed; migrate this model to `.ecky`.",
        ));
    }
    let config_default_backend = config.default_geometry_backend;
    let resolved_backend =
        resolve_geometry_backend(&effective_dialect, geometry_backend, config_default_backend);
    let dispatch_backend =
        resolve_dispatch_backend(macro_code, &effective_dialect, resolved_backend)?;
    crate::runtime_capabilities::ensure_backend_available(
        dispatch_backend,
        config.freecad_cmd(),
        app,
    )?;
    // Lower Ecky IR to the target backend before dispatch.
    // Legacy Python and Build123d sources stay as-is.
    let lowered = match (dispatch_backend, effective_dialect.clone()) {
        (GeometryBackend::Build123d, MacroDialect::EckyIrV0) => {
            return Err(AppError::validation(
                "build123d backend was removed; render `.ecky` through Ecky Native.",
            ));
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

    // === Hybrid poly BRep bridge ===
    // If the source uses mesh-only ops (wall-pattern) AND BRep-required ops
    // (difference/chamfer/fillet), the normal dispatch fails: OCCT can't plan
    // wall-pattern, and the mesh renderer produces garbage on CSG over
    // displaced meshes. The hybrid bridge splits the part at the mesh
    // boundary: mesh renderer does displacement, OCCT does the booleans on
    // the solidified poly BRep.
    if dispatch_backend == GeometryBackend::EckyRust && effective_dialect == MacroDialect::EckyIrV0
    {
        if let Some(hybrid_bundle) =
            try_render_hybrid_poly_brep(macro_code, parameters, config, app)?
        {
            return finalize_render_bundle(hybrid_bundle, parameters, post_processing, app)
                .map_err(|err| {
                    attach_diagnostic_context(
                        err,
                        Some(macro_code),
                        parameters,
                        Some("render:hybrid"),
                    )
                });
        }
    }
    let result = match dispatch_backend {
        GeometryBackend::EckyRust => {
            // Direct OCCT handles BRep ops; the Rust mesh renderer handles
            // mesh-only ops like `wall-pattern`. When a source uses mesh-only
            // ops, the mesh renderer is the designated handler regardless of
            // which config backend was selected.
            let uses_mesh_only_ops = effective_dialect == MacroDialect::EckyIrV0
                && crate::ecky_ir::source_uses_ecky_rust_only_cad_ops(macro_code);
            let pure_mesh_source = effective_dialect == MacroDialect::EckyIrV0
                && source_partitions_are_all_pure_mesh(macro_code);
            let mesh_only_redirect =
                resolved_backend != GeometryBackend::EckyRust || uses_mesh_only_ops;
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
            let direct_attempt = if pure_mesh_source {
                Ok(None)
            } else if direct_occt_capability
                .as_ref()
                .is_some_and(|capability| capability.available)
            {
                try_render_direct_occt_ecky_ir(
                    macro_code,
                    parameters,
                    &effective_dialect,
                    previous_manifest,
                    config,
                    app,
                    source_cancellation.clone(),
                )
            } else {
                Ok(None)
            };
            match direct_attempt {
                Ok(Some(bundle)) => Ok(bundle),
                Ok(None) => {
                    if pure_mesh_source {
                        crate::ecky_ir::render_model_with_previous_manifest(
                            macro_code,
                            parameters,
                            previous_manifest,
                            app,
                        )
                    } else if uses_direct_occt_required {
                        Err(attach_diagnostic_context(
                            unsupported_required_direct_occt_error(
                                "Direct OCCT did not produce a native bundle for native-required CAD ops like `chamfer`, `fillet`, `text`, `svg`, `import-stl`, `import-step`, or `helical-ridge`.".to_string()
                            ),
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
                        let planner_detail = direct_occt_plan_detail
                            .as_deref()
                            .unwrap_or("Direct OCCT planner reason unavailable.");
                        Err(attach_diagnostic_context(
                            blocked_direct_occt_native_error(planner_detail.to_string()),
                            Some(macro_code),
                            parameters,
                            Some("plan:direct-occt"),
                        ))
                    }
                }
                Err(err) => {
                    if uses_direct_occt_required {
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
                            String::from("Direct OCCT native render failed.")
                        } else {
                            direct_occt_plan_detail
                                .as_deref()
                                .unwrap_or("Direct OCCT render failed.")
                                .to_string()
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
        GeometryBackend::Build123d => Err(AppError::validation(
            "build123d backend was removed; render `.ecky` through Ecky Native.",
        )),
        GeometryBackend::Freecad => {
            let source_language = if effective_dialect == MacroDialect::EckyIrV0 {
                crate::contracts::SourceLanguage::EckyIrV0
            } else {
                crate::contracts::SourceLanguage::LegacyPython
            };
            freecad::render_model_with_sources_and_font_path(
                dispatch_source,
                if effective_dialect == MacroDialect::EckyIrV0 {
                    Some(macro_code)
                } else {
                    None
                },
                parameters,
                config.freecad_cmd(),
                config.cad_text_font_path(),
                app,
                source_language,
            )
        }
    };
    result
        .map_err(|err| attach_diagnostic_context(err, Some(macro_code), parameters, Some("render")))
        .and_then(|bundle| persist_authored_source_digest(bundle, macro_code, parameters, app))
        .and_then(|bundle| finalize_render_bundle(bundle, parameters, post_processing, app))
}

fn source_partitions_are_all_pure_mesh(source: &str) -> bool {
    let Some(result) = crate::ecky_scheme::try_compile_to_core_program(source) else {
        return false;
    };
    let Ok(program) = result else {
        return false;
    };
    !program.parts.is_empty()
        && crate::ecky_ir::poly_partition::analyze_program(&program)
            .iter()
            .all(|partition| {
                partition.strategy == crate::ecky_ir::poly_partition::PartRenderStrategy::PureMesh
            })
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
    source_language: Option<crate::contracts::SourceLanguage>,
    macro_dialect: Option<MacroDialect>,
    geometry_backend: Option<GeometryBackend>,
    parameters: &DesignParams,
    post_processing: Option<&crate::contracts::PostProcessingSpec>,
    state: &AppState,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let _guard = state.acquire_geometry_render().await;
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
    source_language: Option<crate::contracts::SourceLanguage>,
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
            let config = RenderConfigSnapshot::from_state(state);
            let resolved_dialect = resolve_source_macro_dialect(
                source_language,
                macro_dialect,
                config.default_source_language,
            );
            return render_model_unlocked(
                &macro_code,
                parameters,
                Some(resolved_dialect),
                geometry_backend,
                post_processing,
                None,
                &config,
                app,
                None,
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
    source_language: Option<crate::contracts::SourceLanguage>,
    macro_dialect: Option<MacroDialect>,
    configured_source_language: crate::contracts::SourceLanguage,
) -> MacroDialect {
    if let Some(explicit) = macro_dialect {
        return explicit;
    }
    match source_language.unwrap_or(configured_source_language) {
        crate::contracts::SourceLanguage::LegacyPython => MacroDialect::Legacy,
        crate::contracts::SourceLanguage::EckyIrV0 => MacroDialect::EckyIrV0,
        crate::contracts::SourceLanguage::Build123d => MacroDialect::Build123d,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        acquire_render_flight, annotate_lowering_error, apply_requested_post_processing,
        cache_salted_render_source, component_placement_evidence_from_source,
        direct_occt_plan_diagnostic, is_tagged_selector_mismatch_error, load_manifest_for_bundle,
        persist_authored_source_digest, post_processing_marker_matches, render_flight_key,
        render_flight_keys, render_flight_strong_count, render_model,
        render_model_with_dependency_upgrade, render_model_with_previous_manifest,
        resolve_dispatch_backend, resolve_geometry_backend, resolve_source_macro_dialect,
        wait_for_render_flight, write_post_processing_marker, RenderConfigSnapshot,
        RenderFlightRole,
    };
    use crate::contracts::{
        AppError, ComponentDefinition, ComponentPackage, DesignParams, GeometryBackend,
        GeometryRepresentation, PackageVisibility, ParamValue, SourceLanguage, UiSpec,
    };
    use crate::contracts::{
        Config, DisplacementSpec, LithophaneAttachment, LithophaneAttachmentSource,
        LithophaneColor, LithophaneColorMode, LithophanePlacement, LithophanePlacementMode,
        LithophaneRelief, LithophaneSide, MacroDialect, McpConfig, OverflowMode,
        PostProcessingSpec, ProjectionType,
    };
    use crate::models::{AppState, PathResolver};
    use std::path::PathBuf;

    #[derive(Clone)]
    struct TestResolver {
        root: PathBuf,
    }

    #[test]
    fn cache_salt_is_semantically_comment_only_and_changes_backend_identity() {
        let source = "(model (part body (box 1 2 3)))";
        let first = cache_salted_render_source(source, &MacroDialect::EckyIrV0, "sha256:first");
        let second = cache_salted_render_source(source, &MacroDialect::EckyIrV0, "sha256:second");

        assert!(first.starts_with(source));
        assert!(first.ends_with("; eckyRenderCacheIdentity sha256:first\n"));
        assert_ne!(first, second);
    }

    #[test]
    fn placement_evidence_maps_named_fit_and_orthogonal_frame_for_runtime_contracts() {
        let source = r#"
            (define-component latch ((number clearance 0.3))
              (ports (port mount :type "mount.v1" :params ((clearance clearance))
                :frame (frame :origin '(0 0 0) :x-axis '(1 0 0) :z-axis '(0 0 1))))
              (box 20 4 2))
            (model
              (part enclosure
                (ports (port side :type "mount.v1"
                  :frame (frame :origin '(50 0 15) :x-axis '(0 1 0) :z-axis '(1 0 0))))
                (box 100 50 30))
              (part side-latch
                (place-component (latch :clearance 0.45) :from mount
                  :to (port-ref enclosure side) :normal opposed)))
        "#;

        let evidence = component_placement_evidence_from_source(source, &DesignParams::new())
            .expect("runtime evidence");
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].instance_id, "side-latch");
        assert_eq!(evidence[0].target_port_ref.port_id, "side");
        assert_eq!(evidence[0].placement_frame.z_axis, [-1.0, 0.0, 0.0]);
        assert_eq!(
            evidence[0].resolved_fit_values.get("clearance"),
            Some(&crate::contracts::ComponentInterfaceValue::Number(0.45))
        );
    }

    #[test]
    fn normal_app_mesh_runtime_persists_component_placement_evidence() {
        let source =
            include_str!("../../tests/fixtures/component-placement/dryer-latch-front-side.ecky");
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let root = std::env::temp_dir().join(format!(
            "ecky-placement-app-runtime-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root: root.clone() };
        let bundle =
            crate::ecky_ir::render_core_program(&program, source, &DesignParams::new(), &resolver)
                .expect("normal mesh runtime");
        let bundle =
            persist_authored_source_digest(bundle, source, &DesignParams::new(), &resolver)
                .expect("persist authored evidence");
        let manifest = crate::model_runtime::read_model_manifest(&resolver, &bundle.model_id)
            .expect("manifest");

        assert_eq!(bundle.component_placement_evidence.len(), 3);
        assert_eq!(
            manifest.component_placement_evidence,
            bundle.component_placement_evidence
        );
        assert_eq!(
            bundle.component_placement_evidence[1]
                .target_port_ref
                .port_id,
            "side-left"
        );
        assert_eq!(
            bundle.component_placement_evidence[1]
                .placement_frame
                .z_axis,
            [-1.0, 0.0, 0.0]
        );
        for asset in &bundle.viewer_assets {
            let indexed = crate::ecky_ir::mesh_asset::IndexedMeshAsset::read_cache(
                &std::path::Path::new(&asset.path).with_extension("indexed-mesh.json"),
            )
            .expect("indexed mesh");
            assert!(indexed.topology().closed, "{}", asset.part_id);
            assert_eq!(indexed.topology().non_manifold_edge_count, 0);
        }

        let invalid = source.replace(
            "(port-ref enclosure side-left)",
            "(port-ref enclosure missing-side)",
        );
        let error = crate::ecky_scheme::compile_to_core_program(&invalid)
            .expect_err("invalid mate fails before render");
        assert!(error.message.contains("side-latch"), "{error}");
        assert!(error.message.contains("enclosure.missing-side"), "{error}");
        assert!(error.primary_span.is_some(), "{error:?}");

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn post_processing_marker_rejects_mutated_preview_bytes() {
        let root = temp_root("post-processing-marker");
        let preview = root.join("model.stl");
        std::fs::write(&preview, b"processed-a").expect("preview");
        let bundle = crate::contracts::ArtifactBundle {
            geometry_provenance: None,
            component_dependency_lock: None,
            component_dependency_lock_digest: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
            schema_version: 1,
            model_id: "generated-marker".to_string(),
            source_kind: crate::contracts::ModelSourceKind::Generated,
            engine_kind: crate::contracts::EngineKind::EckyIrV0,
            source_language: crate::contracts::SourceLanguage::EckyIrV0,
            geometry_backend: crate::contracts::GeometryBackend::EckyRust,
            content_hash: "sha256:preview".to_string(),
            artifact_version: 1,
            fcstd_path: String::new(),
            manifest_path: root.join("manifest.json").to_string_lossy().to_string(),
            macro_path: None,
            model_stl_path: preview.to_string_lossy().to_string(),
            viewer_assets: vec![],
            edge_targets: vec![],
            face_targets: vec![],
            callout_anchors: vec![],
            measurement_guides: vec![],
            export_artifacts: vec![],
        };

        write_post_processing_marker(&bundle, "sha256:input").expect("marker");
        assert!(post_processing_marker_matches(&bundle, "sha256:input"));

        std::fs::write(&preview, b"unprocessed-b").expect("mutate preview");
        assert!(!post_processing_marker_matches(&bundle, "sha256:input"));
        std::fs::remove_dir_all(root).expect("cleanup");
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

    #[test]
    fn source_dialect_uses_saved_language_without_content_inference() {
        assert_eq!(
            resolve_source_macro_dialect(
                Some(SourceLanguage::LegacyPython),
                None,
                SourceLanguage::EckyIrV0,
            ),
            MacroDialect::Legacy
        );
    }

    #[test]
    fn source_dialect_uses_global_config_when_saved_language_is_absent() {
        assert_eq!(
            resolve_source_macro_dialect(None, None, SourceLanguage::Build123d),
            MacroDialect::Build123d
        );
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

    fn write_ascii_cube_stl_fixture(path: &std::path::Path) {
        let vertices = [
            [-1.0, -1.0, -1.0],
            [1.0, -1.0, -1.0],
            [1.0, 1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, -1.0, 1.0],
            [1.0, -1.0, 1.0],
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0],
        ];
        let triangles = [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        let mut stl = String::from("solid cube\n");
        for triangle in triangles {
            let [a, b, c] = triangle.map(|index| vertices[index]);
            stl.push_str(&format!(
                "facet normal 0 0 0\n  outer loop\n    vertex {} {} {}\n    vertex {} {} {}\n    vertex {} {} {}\n  endloop\nendfacet\n",
                a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2],
            ));
        }
        stl.push_str("endsolid cube\n");
        std::fs::write(path, stl).expect("write cube STL fixture");
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
            voice: crate::contracts::VoiceConfig::default(),
            mcp: McpConfig::default(),
            fem_compute: crate::contracts::FemComputeConfig::default(),
            has_seen_onboarding: true,
            connection_type: None,
            provider_models: crate::contracts::ProviderModels::default(),
            default_engine_kind: crate::contracts::EngineKind::Freecad,
            default_source_language: crate::contracts::SourceLanguage::LegacyPython,
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

    fn install_live_source_fixture(
        resolver: &TestResolver,
        root: &std::path::Path,
        version: &str,
    ) -> crate::component_package_runtime::InstalledStorePackage {
        let project = root.join(format!("live-source-package-{version}"));
        let source_path = project.join("components/cage.ecky");
        std::fs::create_dir_all(source_path.parent().expect("component parent"))
            .expect("package source dir");
        std::fs::write(
            &source_path,
            r#"(define-component cage ()
          (verify
    (tag package_wall)
    (metric min_wall_thickness "body")
    (expect (>= value 1)))
  (box 5 6 7))"#,
        )
        .expect("package source");
        crate::component_package_runtime::write_component_package_manifest(
            &project,
            &ComponentPackage {
                schema_version: crate::contracts::COMPONENT_PACKAGE_SCHEMA_VERSION,
                package_id: "fixture.live".to_string(),
                version: version.to_string(),
                display_name: "Live fixture".to_string(),
                visibility: PackageVisibility::Source,
                tags: Vec::new(),
                port_types: Vec::new(),
                mate_types: Vec::new(),
                components: vec![ComponentDefinition {
                    component_id: "cage".to_string(),
                    version: version.to_string(),
                    display_name: "Cage".to_string(),
                    source_ref: Some("components/cage.ecky".to_string()),
                    entry_symbol: Some("cage".to_string()),
                    source_language: Some(SourceLanguage::EckyIrV0),
                    geometry_backend: Some(GeometryBackend::EckyRust),
                    macro_dialect: Some(MacroDialect::EckyIrV0),
                    geometry_provenance: None,
                    sketches: Vec::new(),
                    keepouts: Vec::new(),
                    fusion_zones: Vec::new(),
                    params: Vec::new(),
                    ui_spec: UiSpec::default(),
                    initial_params: DesignParams::new(),
                    ports: Vec::new(),
                }],
                assemblies: Vec::new(),
            },
        )
        .expect("package manifest");
        let archive = root.join(format!("fixture-live-{version}.eckypkg"));
        crate::component_package_runtime::write_component_package_archive(&project, &archive)
            .expect("package archive");
        crate::component_package_runtime::install_component_package_to_store(resolver, &archive)
            .expect("install package")
    }

    #[tokio::test]
    async fn installed_live_source_component_travels_verify_and_renders_with_lock_evidence() {
        let root = temp_root("live-source-outer");
        let resolver = TestResolver { root: root.clone() };
        let state = test_state(&root);
        let installed = install_live_source_fixture(&resolver, &root, "1.0.0");
        let authored = r#"
          (import-component "fixture.live" :version "1.0.0" :component "cage" :as holder)
          (model
            (part body (holder))
            (part mesh_route
              (translate 12 0 0
                (wall-pattern (:mode ribs :depth 0.2 :uFreq 4)
                  (extrude (circle 5) 8)))))
        "#;

        let resolved = crate::component_import_runtime::resolve_authoring_source(
            crate::component_import_runtime::ResolveAuthoringSourceRequest {
                authored_source: authored,
                expected_lock: None,
            },
            &crate::component_import_runtime::InstalledLibraryComponentResolver { app: &resolver },
        )
        .expect("host pre-resolution");
        let compiled = crate::component_import_runtime::compile_authoring_source(
            crate::component_import_runtime::ResolveAuthoringSourceRequest {
                authored_source: authored,
                expected_lock: None,
            },
            &crate::component_import_runtime::InstalledLibraryComponentResolver { app: &resolver },
        )
        .unwrap_or_else(|error| {
            panic!(
                "host pre-resolution compile: {error}; source:\n{}",
                resolved.compiler_source
            )
        });
        assert!(compiled
            .program
            .constraints
            .verify_clauses
            .iter()
            .any(|clause| matches!(
                clause.tag.items.first(),
                Some(crate::ecky_core_ir::CoreVerifyValue::Symbol(tag))
                    if tag == "body/package_wall"
            )));

        let bundle = render_model_with_previous_manifest(
            authored,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            None,
            &state,
            &resolver,
        )
        .await
        .expect("normal render service");
        let lock = bundle
            .component_dependency_lock
            .as_ref()
            .expect("dependency lock");
        assert_eq!(
            lock.dependencies[0].package_digest,
            installed.package_digest
        );
        assert!(bundle.component_dependency_lock_digest.is_some());
        assert_eq!(bundle.component_import_origins.len(), 1);
        assert_eq!(bundle.component_import_origins[0].alias, "holder");
        let manifest = crate::model_runtime::read_model_manifest(&resolver, &bundle.model_id)
            .expect("manifest");
        let expected_source_digest =
            crate::services::render_snapshot::canonical_source_digest(authored);
        assert_eq!(
            manifest.source_digest.as_deref(),
            Some(expected_source_digest.as_str())
        );
        assert_eq!(
            bundle.component_import_origins,
            manifest.component_import_origins
        );
        let (restored_bundle, restored_manifest) =
            crate::model_runtime::read_runtime_bundle(&resolver, &bundle.model_id)
                .expect("restore validates lock/origin evidence");
        assert_eq!(
            restored_bundle.component_dependency_lock,
            bundle.component_dependency_lock
        );
        assert_eq!(
            restored_manifest.component_import_origins,
            manifest.component_import_origins
        );
        let persisted_source =
            std::fs::read_to_string(bundle.macro_path.as_ref().expect("macro path"))
                .expect("persisted authored source");
        assert!(persisted_source.contains("(import-component \"fixture.live\""));
        assert!(!persisted_source.contains("(define-component cage"));

        let installed_v2 = install_live_source_fixture(&resolver, &root, "2.0.0");
        let upgraded_source = authored.replace("1.0.0", "2.0.0");
        let locked_error = render_model_with_previous_manifest(
            &upgraded_source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            Some(&manifest),
            &state,
            &resolver,
        )
        .await
        .expect_err("ordinary re-render cannot rewrite committed lock");
        assert!(
            locked_error
                .message
                .contains("Expected dependency lock does not contain exact package coordinate"),
            "{locked_error}"
        );
        let upgraded_bundle = render_model_with_dependency_upgrade(
            &upgraded_source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            Some(&manifest),
            &state,
            &resolver,
        )
        .await
        .expect("explicit upgrade preview");
        assert_eq!(
            upgraded_bundle
                .component_dependency_lock
                .as_ref()
                .expect("upgraded lock")
                .dependencies[0]
                .package_digest,
            installed_v2.package_digest
        );
        assert_eq!(
            bundle
                .component_dependency_lock
                .as_ref()
                .expect("prior version lock")
                .dependencies[0]
                .package_digest,
            installed.package_digest,
            "candidate upgrade must not mutate the prior committed bundle"
        );
        assert_ne!(
            upgraded_bundle.component_dependency_lock_digest,
            bundle.component_dependency_lock_digest
        );
        assert_ne!(
            upgraded_bundle.model_id, bundle.model_id,
            "different dependency locks must never reuse one cached artifact"
        );

        let missing = authored.replace("1.0.0", "9.9.9");
        let error = render_model_with_previous_manifest(
            &missing,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            None,
            &state,
            &resolver,
        )
        .await
        .expect_err("missing exact version");
        assert!(error.message.contains("fixture.live@9.9.9"), "{error}");
        assert!(error.message.contains("not indexed"), "{error}");

        let vendored = r#"
          (define-component holder () (box 5 6 7))
          (model
            (part body (holder))
            (part mesh_route
              (translate 12 0 0
                (wall-pattern (:mode ribs :depth 0.2 :uFreq 4)
                  (extrude (circle 5) 8)))))
        "#;
        let vendored_bundle = render_model_with_previous_manifest(
            vendored,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            None,
            &state,
            &resolver,
        )
        .await
        .expect("copy-inline render");
        assert!(vendored_bundle.component_dependency_lock.is_none());
        assert!(vendored_bundle.component_import_origins.is_empty());

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn identical_render_requests_share_failure_and_retry_starts_new_owner() {
        let key = format!("render-flight-test-{}", uuid::Uuid::new_v4());
        let owner = match acquire_render_flight(&key) {
            RenderFlightRole::Owner(owner) => owner,
            RenderFlightRole::Waiter(_) => panic!("first request must own the render"),
        };
        let waiter = match acquire_render_flight(&key) {
            RenderFlightRole::Waiter(waiter) => waiter,
            RenderFlightRole::Owner(_) => panic!("identical overlapping request must join"),
        };

        let raw_failure = AppError::render("native kernel raw failure");
        let owner_result = owner.complete(Err(raw_failure.clone()));
        let waiter_result = wait_for_render_flight(waiter).await;

        assert_eq!(owner_result, Err(raw_failure.clone()));
        assert_eq!(waiter_result, Err(raw_failure));
        assert!(matches!(
            acquire_render_flight(&key),
            RenderFlightRole::Owner(_)
        ));
    }

    #[tokio::test]
    async fn owner_abort_before_kernel_lock_notifies_waiter_and_allows_retry() {
        let key = format!("render-flight-abort-{}", uuid::Uuid::new_v4());
        let owner = match acquire_render_flight(&key) {
            RenderFlightRole::Owner(owner) => owner,
            RenderFlightRole::Waiter(_) => panic!("first request must own the render"),
        };
        let waiter = match acquire_render_flight(&key) {
            RenderFlightRole::Waiter(waiter) => waiter,
            RenderFlightRole::Owner(_) => panic!("overlapping request must join"),
        };

        drop(owner);

        let error = wait_for_render_flight(waiter)
            .await
            .expect_err("waiter must receive owner cancellation");
        assert!(error.to_string().contains("cancelled before completion"));
        assert_eq!(render_flight_strong_count(&key), None);
        assert!(matches!(
            acquire_render_flight(&key),
            RenderFlightRole::Owner(_)
        ));
    }

    #[test]
    fn render_flight_identity_changes_when_imported_stl_bytes_change() {
        let root = temp_root("singleflight-stl-digest");
        let state = test_state(&root);
        let resolver = TestResolver { root: root.clone() };
        let stl_path = root.join("asset.stl");
        write_ascii_stl_fixture(&stl_path);
        let source = format!(
            "(model (part imported (solidify (import-stl {:?}))))",
            stl_path.to_string_lossy()
        );
        let first = render_flight_key(
            &source,
            &DesignParams::new(),
            Some(&MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            None,
            &RenderConfigSnapshot::from_state(&state),
            &resolver,
        )
        .expect("first identity");

        let mut changed = std::fs::read(&stl_path).expect("fixture bytes");
        changed.extend_from_slice(b"\n");
        std::fs::write(&stl_path, changed).expect("change fixture bytes");
        let second = render_flight_key(
            &source,
            &DesignParams::new(),
            Some(&MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            None,
            &RenderConfigSnapshot::from_state(&state),
            &resolver,
        )
        .expect("second identity");

        assert_ne!(first, second);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn render_cache_identity_changes_with_post_processing_inputs() {
        let root = temp_root("singleflight-post-processing");
        let state = test_state(&root);
        let resolver = TestResolver { root: root.clone() };
        let source = "(model (part body (box 1 2 3)))";
        let post_processing = |depth_mm| PostProcessingSpec {
            displacement: Some(DisplacementSpec {
                image_param: "image".to_string(),
                projection: ProjectionType::Planar,
                depth_mm,
                invert: false,
            }),
            lithophane_attachments: vec![],
        };

        let first = render_flight_key(
            source,
            &DesignParams::new(),
            Some(&MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            Some(&post_processing(1.0)),
            None,
            &RenderConfigSnapshot::from_state(&state),
            &resolver,
        )
        .expect("first identity");
        let second = render_flight_key(
            source,
            &DesignParams::new(),
            Some(&MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            Some(&post_processing(2.0)),
            None,
            &RenderConfigSnapshot::from_state(&state),
            &resolver,
        )
        .expect("second identity");

        assert_ne!(first, second);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn overlapping_public_render_requests_join_before_kernel_lock() {
        let root = temp_root("singleflight-public");
        let state = std::sync::Arc::new(test_state(&root));
        let resolver = std::sync::Arc::new(TestResolver { root: root.clone() });
        let source = "(model (part broken (definitely-not-an-op 1)))".to_string();
        let dialect = Some(MacroDialect::EckyIrV0);
        let backend = Some(GeometryBackend::EckyRust);
        let key = render_flight_key(
            &source,
            &DesignParams::new(),
            dialect.as_ref(),
            backend,
            None,
            None,
            &RenderConfigSnapshot::from_state(state.as_ref()),
            resolver.as_ref(),
        )
        .expect("render identity");

        let kernel_gate_state = state.clone();
        let kernel_gate = kernel_gate_state.render_lock.lock().await;
        let spawn_request = |state: std::sync::Arc<AppState>,
                             resolver: std::sync::Arc<TestResolver>,
                             source: String| {
            tokio::spawn(async move {
                render_model_with_previous_manifest(
                    &source,
                    &DesignParams::new(),
                    Some(MacroDialect::EckyIrV0),
                    backend,
                    None,
                    None,
                    state.as_ref(),
                    resolver.as_ref(),
                )
                .await
            })
        };

        let first = spawn_request(state.clone(), resolver.clone(), source.clone());
        for _ in 0..500 {
            if render_flight_strong_count(&key) == Some(2) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(
            render_flight_strong_count(&key),
            Some(2),
            "active keys: {:?}",
            render_flight_keys()
        );

        let second = spawn_request(state.clone(), resolver.clone(), source.clone());
        for _ in 0..500 {
            if render_flight_strong_count(&key) == Some(3) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(render_flight_strong_count(&key), Some(3));

        drop(kernel_gate);
        let first_error = first.await.expect("first task").expect_err("first error");
        let second_error = second
            .await
            .expect("second task")
            .expect_err("second error");
        assert_eq!(first_error, second_error);
        assert_eq!(render_flight_strong_count(&key), None);

        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn render_owner_uses_config_snapshot_taken_before_kernel_gate() {
        let root = temp_root("config-snapshot");
        let state = std::sync::Arc::new(test_state(&root));
        let resolver = std::sync::Arc::new(TestResolver { root: root.clone() });
        {
            let mut config = state.config.lock().expect("config");
            config.default_source_language = SourceLanguage::EckyIrV0;
            config.default_geometry_backend = GeometryBackend::EckyRust;
            config.freecad_cmd = "/before-freecad".to_string();
            config.cad_text_font_path = "/before-font".to_string();
        }
        let kernel_gate = state.render_lock.lock().await;
        let source = "(model (part broken (definitely-not-an-op 1)))".to_string();
        let key = render_flight_key(
            &source,
            &DesignParams::new(),
            None,
            None,
            None,
            None,
            &RenderConfigSnapshot::from_state(state.as_ref()),
            resolver.as_ref(),
        )
        .expect("render identity");
        let request = tokio::spawn({
            let state = state.clone();
            let resolver = resolver.clone();
            let source = source.clone();
            async move {
                render_model_with_previous_manifest(
                    &source,
                    &DesignParams::new(),
                    None,
                    None,
                    None,
                    None,
                    state.as_ref(),
                    resolver.as_ref(),
                )
                .await
            }
        });

        for _ in 0..500 {
            if render_flight_strong_count(&key) == Some(2) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let waiter = tokio::spawn({
            let state = state.clone();
            let resolver = resolver.clone();
            let source = source.clone();
            async move {
                render_model_with_previous_manifest(
                    &source,
                    &DesignParams::new(),
                    None,
                    None,
                    None,
                    None,
                    state.as_ref(),
                    resolver.as_ref(),
                )
                .await
            }
        });
        for _ in 0..500 {
            if render_flight_strong_count(&key) == Some(3) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        {
            let mut config = state.config.lock().expect("config");
            config.default_source_language = SourceLanguage::LegacyPython;
            config.default_geometry_backend = GeometryBackend::Freecad;
            config.freecad_cmd = "/after-freecad".to_string();
            config.cad_text_font_path = "/after-font".to_string();
        }
        drop(kernel_gate);

        let error = request
            .await
            .expect("request task")
            .expect_err("invalid Ecky source must fail");
        assert!(
            error.to_string().contains("definitely-not-an-op"),
            "render used post-gate config: {error}"
        );
        assert_eq!(
            waiter
                .await
                .expect("waiter task")
                .expect_err("waiter must receive owner result"),
            error
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    fn example_fixture_source(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../model-runtime/examples")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read fixture {}: {err}", path.display()))
    }

    #[test]
    fn apply_requested_displacement_surfaces_raw_displacement_errors() {
        let params = DesignParams::from([(
            "image".to_string(),
            crate::contracts::ParamValue::String("/definitely/missing/lithophane.png".to_string()),
        )]);
        let mut bundle = crate::contracts::ArtifactBundle {
            geometry_provenance: None,
            component_dependency_lock: None,
            component_dependency_lock_digest: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
            schema_version: 1,
            model_id: "model".to_string(),
            source_kind: crate::contracts::ModelSourceKind::Generated,
            engine_kind: crate::contracts::EngineKind::Freecad,
            source_language: crate::contracts::SourceLanguage::LegacyPython,
            geometry_backend: crate::contracts::GeometryBackend::Freecad,
            content_hash: "unchanged".to_string(),
            artifact_version: 1,
            fcstd_path: "/tmp/model.FCStd".to_string(),
            manifest_path: "/tmp/missing-manifest.json".to_string(),
            macro_path: None,
            model_stl_path: "/tmp/nonexistent-model.stl".to_string(),
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
    fn direct_occt_diagnostic_names_compiler_failures_without_blame_on_planner() {
        let error = direct_occt_plan_diagnostic("(model", &Default::default())
            .expect_err("malformed source must fail compilation");

        assert!(
            error.starts_with("Ecky compiler rejected model."),
            "{error}"
        );
        assert!(!error.contains("Direct OCCT planner rejected"), "{error}");
    }

    #[test]
    fn direct_occt_compiler_diagnostic_shows_source_line_and_fix() {
        let source = r#"(model
  (meta :title "Pasta Curl")
  (part body
    (for-union (range 6) (lambda (i) (box 1 1 1)))))"#;

        let error = direct_occt_plan_diagnostic(source, &Default::default())
            .expect_err("legacy for-union spelling must fail");

        assert!(error.contains("--> line 4, column 5"), "{error}");
        assert!(error.contains("4 |     (for-union (range 6)"), "{error}");
        assert!(
            error.contains("help: use `(for-union (i 6) body)`"),
            "{error}"
        );
    }

    #[test]
    fn post_processing_noop_preserves_existing_step_export_artifacts() {
        let params = DesignParams::new();
        let mut bundle = crate::contracts::ArtifactBundle {
            geometry_provenance: None,
            component_dependency_lock: None,
            component_dependency_lock_digest: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
            schema_version: 1,
            model_id: "model".to_string(),
            source_kind: crate::contracts::ModelSourceKind::Generated,
            engine_kind: crate::contracts::EngineKind::EckyIrV0,
            source_language: crate::contracts::SourceLanguage::EckyIrV0,
            geometry_backend: crate::contracts::GeometryBackend::EckyRust,
            content_hash: "unchanged".to_string(),
            artifact_version: 1,
            fcstd_path: String::new(),
            manifest_path: "/tmp/missing-manifest.json".to_string(),
            macro_path: None,
            model_stl_path: "/tmp/nonexistent-model.stl".to_string(),
            viewer_assets: vec![],
            edge_targets: vec![],
            face_targets: vec![],
            callout_anchors: vec![],
            measurement_guides: vec![],
            export_artifacts: vec![crate::contracts::ExportArtifact {
                geometry_provenance: None,
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
        let model_stl_path = root.join("model.stl");
        std::fs::write(
            &model_stl_path,
            [&[0u8; 80][..], &0u32.to_le_bytes()[..]].concat(),
        )
        .unwrap();

        let params = DesignParams::new();
        let mut bundle = crate::contracts::ArtifactBundle {
            geometry_provenance: None,
            component_dependency_lock: None,
            component_dependency_lock_digest: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
            schema_version: 1,
            model_id: "model".to_string(),
            source_kind: crate::contracts::ModelSourceKind::Generated,
            engine_kind: crate::contracts::EngineKind::Freecad,
            source_language: crate::contracts::SourceLanguage::LegacyPython,
            geometry_backend: crate::contracts::GeometryBackend::Freecad,
            content_hash: "unchanged".to_string(),
            artifact_version: 1,
            fcstd_path: "/tmp/model.FCStd".to_string(),
            manifest_path: "/tmp/missing-manifest.json".to_string(),
            macro_path: None,
            model_stl_path: model_stl_path.to_string_lossy().to_string(),
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
        let model_stl_path = root.join("model.stl");
        std::fs::write(
            &model_stl_path,
            [&[0u8; 80][..], &0u32.to_le_bytes()[..]].concat(),
        )
        .unwrap();
        let manifest_path = root.join("manifest.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&crate::contracts::ModelManifest {
                geometry_provenance: None,
                component_import_origins: Vec::new(),
                component_placement_evidence: Vec::new(),
                schema_version: 1,
                model_id: "model".to_string(),
                source_kind: crate::contracts::ModelSourceKind::Generated,
                source_digest: None,
                core_digest: None,
                ast_schema_version: None,
                engine_kind: crate::contracts::EngineKind::EckyIrV0,
                source_language: crate::contracts::SourceLanguage::EckyIrV0,
                geometry_backend: crate::contracts::GeometryBackend::EckyRust,
                document: crate::contracts::DocumentMetadata {
                    document_name: "doc".to_string(),
                    document_label: "doc".to_string(),
                    source_path: None,
                    object_count: 1,
                    warnings: vec![],
                },
                parts: vec![crate::contracts::PartBinding {
                    part_id: "body".to_string(),
                    freecad_object_name: "body".to_string(),
                    label: "Body".to_string(),
                    kind: "solid".to_string(),
                    semantic_role: None,
                    viewer_asset_path: None,
                    viewer_node_ids: vec![],
                    parameter_keys: vec![],
                    editable: true,
                    bounds: Some(crate::contracts::ManifestBounds {
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
                analysis_declarations: Vec::new(),
                warnings: vec![],
                enrichment_state: crate::contracts::ManifestEnrichmentState {
                    status: crate::contracts::EnrichmentStatus::None,
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
        let mut bundle = crate::contracts::ArtifactBundle {
            geometry_provenance: None,
            component_dependency_lock: None,
            component_dependency_lock_digest: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
            schema_version: 1,
            model_id: "model".to_string(),
            source_kind: crate::contracts::ModelSourceKind::Generated,
            engine_kind: crate::contracts::EngineKind::EckyIrV0,
            source_language: crate::contracts::SourceLanguage::EckyIrV0,
            geometry_backend: crate::contracts::GeometryBackend::EckyRust,
            content_hash: "unchanged".to_string(),
            artifact_version: 1,
            fcstd_path: String::new(),
            manifest_path: manifest_path.to_string_lossy().to_string(),
            macro_path: None,
            model_stl_path: model_stl_path.to_string_lossy().to_string(),
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

        assert!(std::path::Path::new(&bundle.model_stl_path).exists());
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
            crate::contracts::AppErrorCode::Render,
            "build123d runner failed.",
            "stderr:\nValueError: Edge selector `{'kind': 'targetIds'}` did not match target ids: ['body:edge:old']",
        );
        assert!(is_tagged_selector_mismatch_error(&err));

        let direct_occt = AppError::with_details(
            crate::contracts::AppErrorCode::Render,
            "Direct OCCT native shim probe failed.",
            "stderr:\nDirect OCCT edge selector target ids did not match current topology for part `body`. requested=body:edge:old",
        );
        assert!(is_tagged_selector_mismatch_error(&direct_occt));

        let unrelated = AppError::validation("shell expects positive wall thickness");
        assert!(!is_tagged_selector_mismatch_error(&unrelated));
    }

    #[test]
    fn ecky_rust_request_keeps_sampled_radial_loft_source_on_ecky_rust_for_direct_probe() {
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
    fn mixed_mesh_and_sampled_radial_loft_dispatches_to_ecky_rust() {
        // sampled-radial-loft is a portable op now; a mesh-only mix is no
        // longer rejected up front — it dispatches to EckyRust where the
        // renderers produce their own deterministic diagnostics.
        let backend = resolve_dispatch_backend(
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
            GeometryBackend::Build123d,
        )
        .expect("mixed source must dispatch, not reject");

        assert_eq!(backend, GeometryBackend::EckyRust);
    }

    #[tokio::test]
    async fn ecky_rust_request_routes_wall_pattern_to_mesh_renderer() {
        let root = temp_root("direct-fallback");
        let resolver = TestResolver { root: root.clone() };
        let state = test_state(&root);

        let bundle = render_model(
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
        .expect("native backend should route wall-pattern to mesh renderer");

        assert!(
            !bundle.model_stl_path.is_empty(),
            "mesh renderer must produce a model STL: {bundle:?}"
        );
        assert!(
            !bundle.viewer_assets.is_empty(),
            "mesh renderer must produce viewer assets: {bundle:?}"
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
            diagnostic.contains("Direct OCCT"),
            "unexpected error: {err:?}"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ecky_rust_request_renders_hull_natively() {
        let root = temp_root("eckyrust-hull-native");
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
                  (hull
                    (sphere 6)
                    (translate 30 0 0 (sphere 6)))))"#,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &state,
            &resolver,
        )
        .await
        .expect("EckyRust must render hull through Direct OCCT");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-direct-occt-"));

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
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
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
    async fn plain_import_stl_renders_mesh_native_without_direct_occt() {
        let root = temp_root("eckyrust-import-stl-mesh-native");
        let resolver = TestResolver { root: root.clone() };
        let direct_capability = crate::runtime_capabilities::probe_direct_occt_runtime(&resolver);
        if direct_capability.available {
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
        .expect("plain import-stl must use the mesh-native preview path");

        assert_eq!(
            bundle
                .geometry_provenance
                .as_ref()
                .map(|item| &item.representation),
            Some(&GeometryRepresentation::MeshNative)
        );
        assert!(bundle.edge_targets.is_empty());
        assert!(bundle.face_targets.is_empty());
        assert!(!bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"));

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
    async fn plain_import_stl_stays_mesh_native_when_direct_occt_is_available() {
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
        .expect("plain import-stl must render through the mesh-native preview path");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(bundle.model_id.starts_with("generated-ir-"));
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
        assert_eq!(
            bundle
                .geometry_provenance
                .as_ref()
                .map(|item| &item.representation),
            Some(&GeometryRepresentation::MeshNative)
        );
        assert!(bundle.edge_targets.is_empty());
        assert!(bundle.face_targets.is_empty());
        assert!(!bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"));

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
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
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
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format == "step"
                && std::path::Path::new(&artifact.path).is_file()));

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
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
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
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
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
            diagnostic.contains("Direct OCCT"),
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
            diagnostic.contains("Direct OCCT"),
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
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
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
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
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
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
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
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
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
            .find(|target| target.kind == crate::contracts::SelectionTargetKind::Edge)
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
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
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
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
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
            .find(|target| target.kind == crate::contracts::SelectionTargetKind::Face)
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
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
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
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
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
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
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
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
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

    /// Regression: iPhone 17e case — a 3-part Ecky source that combines
    /// `wall-pattern` (cellular displacement) on the TPU back panel, then
    /// runs CSG `difference` / `chamfer` / `union` over the displaced mesh.
    ///
    /// Previously this panicked inside `earcutr` (triangulation of a
    /// degenerate polygon produced by the CSG library after vertex
    /// displacement). The renderer must either produce valid geometry or
    /// return a clean `AppError` — it must never panic the worker thread.
    #[tokio::test]
    async fn ecky_rust_rejects_iphone_case_dense_broad_chamfer_without_panic() {
        let root = temp_root("iphone-case-wall-pattern");
        let resolver = TestResolver { root: root.clone() };
        let source = example_fixture_source("iphone-17e-case-multipart.ecky");

        let result = render_model(
            &source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &test_state(&root),
            &resolver,
        )
        .await;

        let err = result.expect_err("dense broad faceted chamfer must be rejected");
        assert_eq!(err.code, crate::contracts::AppErrorCode::Validation);
        assert!(err.message.contains("Mesh-origin faceted BRep `chamfer`"));
        assert!(err.message.contains("selector `edge-clauses`"));
        assert!(err
            .message
            .contains("rejected before OCCT kernel execution"));

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
            &crate::contracts::DesignParams::new(),
            &resolver,
        )
        .expect("IR render");

        // Override the geometry_backend field to simulate a Build123d bundle.
        // This is the core of the Phase 7 invariant: post-processing must not
        // branch on the backend.
        bundle.geometry_backend = crate::contracts::GeometryBackend::Build123d;

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
            &crate::contracts::DesignParams::new(),
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
            crate::contracts::GeometryBackend::Build123d,
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

    /// Hybrid poly BRep bridge: wall-pattern + difference must route through
    /// the two-phase pipeline (mesh renderer → solidify → OCCT boolean) and
    /// produce a manifold result, not the 30k+ non-manifold garbage the mesh
    /// renderer produces on CSG over displaced meshes.
    #[tokio::test]
    async fn hybrid_poly_brep_exports_both_stl_and_step() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root =
            crate::ecky_cad_host::direct_occt_sdk::bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let root = temp_root("hybrid-poly-brep-export");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"(model
  (part body
    (difference
      (wall-pattern (:mode ribs :depth 0.4 :uFreq 8)
        (extrude (circle 10) 20))
      (cylinder 3 30))))"#;

        let bundle = render_model(
            source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &test_state(&root),
            &resolver,
        )
        .await
        .expect("hybrid render");

        // STL must exist.
        assert!(
            std::path::Path::new(&bundle.model_stl_path).is_file(),
            "STL must exist: {}",
            bundle.model_stl_path
        );

        // STEP must exist — this is the key benefit of the hybrid bridge over
        // pure mesh rendering (which cannot produce STEP).
        let has_step = bundle
            .export_artifacts
            .iter()
            .any(|a| a.format.eq_ignore_ascii_case("step"));
        assert!(
            has_step,
            "STEP export must exist for hybrid parts. export_artifacts: {:?}",
            bundle
                .export_artifacts
                .iter()
                .map(|a| &a.format)
                .collect::<Vec<_>>()
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    /// A dense raster relief is a mesh Boolean operand, even when authored as
    /// the second input to a multi-operand union. It must stay in the in-memory
    /// Manifold domain instead of taking the mesh -> STL -> OCCT solidify path.
    #[tokio::test]
    async fn dense_raster_tool_uses_manifold_batch_union_without_non_manifold_edges() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root =
            crate::ecky_cad_host::direct_occt_sdk::bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let root = temp_root("dense-raster-manifold-union");
        let resolver = TestResolver { root: root.clone() };
        let image_path = root.join("dense-raster.png");
        let mut image = image::GrayImage::from_pixel(160, 120, image::Luma([255]));
        for row in 0..8 {
            for column in 0..12 {
                let origin_x = 5 + column * 13;
                let origin_y = 5 + row * 14;
                for y in origin_y..origin_y + 5 {
                    for x in origin_x..origin_x + 5 {
                        image.put_pixel(x, y, image::Luma([0]));
                    }
                }
            }
        }
        image.save(&image_path).expect("dense raster fixture");
        let source = format!(
            r#"(model
  (part cliche
    (union
      (translate 20 15 0
        (extrude (rounded-rect 40 30 3) 2.4))
      (translate 0 0 2.2
        (extrude "{}" 1.6
          :width 40
          :depth 30
          :threshold 0.5
          :foreground dark)))))"#,
            image_path.display()
        );

        let bundle = render_model(
            &source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &test_state(&root),
            &resolver,
        )
        .await
        .expect("dense raster tool must render through Manifold");

        assert_eq!(
            bundle
                .geometry_provenance
                .as_ref()
                .map(|provenance| &provenance.representation),
            Some(&GeometryRepresentation::MeshNative)
        );
        assert_eq!(
            crate::services::structural_verification::model_stl_non_manifold_edge_count(
                std::path::Path::new(&bundle.model_stl_path),
            )
            .expect("dense raster topology"),
            0
        );
        assert!(
            !bundle
                .export_artifacts
                .iter()
                .any(|artifact| artifact.format.eq_ignore_ascii_case("step")),
            "single mesh-domain part must not fabricate STEP"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn stamp_dense_raster_tpu_part_has_zero_non_manifold_edges() {
        use base64::Engine as _;

        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root =
            crate::ecky_cad_host::direct_occt_sdk::bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }

        let root = temp_root("stamp-dense-raster-tpu");
        let resolver = TestResolver { root: root.clone() };
        let image_path = root.join("stamp-artwork-gimp-levels-nozzle-06.png");
        let encoded = include_str!(
            "../../tests/fixtures/cad/raster/stamp-artwork-gimp-levels-nozzle-06.png.base64"
        )
        .split_whitespace()
        .collect::<String>();
        let image_bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("exact stamp raster fixture");
        std::fs::write(&image_path, image_bytes).expect("write exact stamp raster fixture");
        let source = format!(
            r#"(model
  (part tpu_cliche
    (union
      (translate 30 22.5 0
        (extrude (rounded-rect 60 45 5) 2.4))
      (translate 0 0 2.2
        (extrude "{}" 1.6
          :width 60
          :depth 45
          :threshold 0.5
          :foreground dark)))))"#,
            image_path.display()
        );

        let bundle = render_model(
            &source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &test_state(&root),
            &resolver,
        )
        .await
        .expect("exact TPU raster part must render through Manifold");
        assert_eq!(
            crate::services::structural_verification::model_stl_non_manifold_edge_count(
                std::path::Path::new(&bundle.model_stl_path),
            )
            .expect("exact TPU topology"),
            0
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    /// Regression for the production stamp project that exposed 4,508
    /// non-manifold edges after the raster relief crossed the hybrid bridge.
    #[tokio::test]
    async fn stamp_dense_raster_project_has_zero_non_manifold_edges() {
        use base64::Engine as _;

        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root =
            crate::ecky_cad_host::direct_occt_sdk::bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }

        let root = temp_root("stamp-dense-raster-project");
        let resolver = TestResolver { root: root.clone() };
        let image_path = root.join("stamp-artwork-gimp-levels-nozzle-06.png");
        let encoded = include_str!(
            "../../tests/fixtures/cad/raster/stamp-artwork-gimp-levels-nozzle-06.png.base64"
        )
        .split_whitespace()
        .collect::<String>();
        let image_bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("exact stamp raster fixture");
        std::fs::write(&image_path, image_bytes).expect("write exact stamp raster fixture");
        let source = include_str!("../../tests/fixtures/cad/raster/stamp_dense_raster.ecky")
            .replace("__STAMP_RASTER_PATH__", &image_path.to_string_lossy());

        let program = crate::ecky_scheme::compile_to_core_program(&source)
            .expect("compile exact stamp raster fixture");
        let partitions = crate::ecky_ir::poly_partition::analyze_program(&program);
        assert_eq!(
            partitions[0].strategy,
            crate::ecky_ir::poly_partition::PartRenderStrategy::Hybrid,
            "TPU raster relief must remain a mesh island below its union"
        );

        let bundle = render_model(
            &source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &test_state(&root),
            &resolver,
        )
        .await
        .expect("exact stamp raster project must render through Manifold");

        for asset in &bundle.viewer_assets {
            assert_eq!(
                crate::services::structural_verification::model_stl_non_manifold_edge_count(
                    std::path::Path::new(&asset.path),
                )
                .expect("exact stamp part topology"),
                0,
                "part '{}' must be manifold",
                asset.part_id
            );
        }

        assert_eq!(
            crate::services::structural_verification::model_stl_non_manifold_edge_count(
                std::path::Path::new(&bundle.model_stl_path),
            )
            .expect("exact stamp topology"),
            0
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn multipart_heightfield_and_thread_bridge_mesh_part_into_occt() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root =
            crate::ecky_cad_host::direct_occt_sdk::bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let root = temp_root("multipart-heightfield-thread");
        let resolver = TestResolver { root: root.clone() };
        let image_path = root.join("heightmap.png");
        image::GrayImage::from_fn(12, 8, |x, y| {
            if (x / 3 + y / 2) % 2 == 0 {
                image::Luma([0])
            } else {
                image::Luma([255])
            }
        })
        .save(&image_path)
        .expect("heightmap fixture");
        let source = format!(
            r#"(model
  (part relief
    (heightfield "{}"
      :width 12
      :depth 8
      :relief-height 2
      :base-thickness 1
      :invert #t))
  (part handle
    (translate 24 0 0
      (union
        (cylinder 4 6)
        (helical-ridge
          :radius 3.8
          :pitch 2
          :height 6
          :depth 0.8
          :base-width 1.6
          :crest-width 0.4)))))"#,
            image_path.display()
        );

        let bundle = render_model(
            &source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &test_state(&root),
            &resolver,
        )
        .await
        .expect("multipart mesh + OCCT render");

        assert_eq!(
            bundle
                .viewer_assets
                .iter()
                .map(|asset| asset.part_id.as_str())
                .collect::<Vec<_>>(),
            vec!["relief", "handle"]
        );
        assert!(
            bundle
                .export_artifacts
                .iter()
                .any(|artifact| artifact.format.eq_ignore_ascii_case("step")),
            "mixed multipart render must preserve STEP export"
        );
        assert_eq!(
            bundle
                .geometry_provenance
                .as_ref()
                .map(|provenance| &provenance.representation),
            Some(&crate::contracts::GeometryRepresentation::FacetedPolyBrep)
        );
        assert_eq!(
            crate::services::structural_verification::model_stl_non_manifold_edge_count(
                std::path::Path::new(&bundle.model_stl_path),
            )
            .expect("preview topology"),
            0
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn fem_density_surface_solid_exports_faceted_step() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root =
            crate::ecky_cad_host::direct_occt_sdk::bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let root = temp_root("fem-density-surface-solid");
        let resolver = TestResolver { root: root.clone() };
        let surface = ecky_fem::FemDensitySurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [10.0, 0.0, 0.0],
                [0.0, 10.0, 0.0],
                [0.0, 0.0, 10.0],
            ],
            triangles: vec![[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]],
            connected_anchor_ids: vec!["mount".into()],
            discarded_cell_indices: vec![],
            discarded_active_volume_fraction: 0.0,
            boundary_edge_count: 0,
            non_manifold_edge_count: 0,
            connected_component_count: 1,
            signed_volume_mm3: 1000.0 / 6.0,
        };
        let expression =
            crate::fem_topology_reconstruction::density_surface_solid_expression(&surface)
                .expect("solid expression");
        let source = format!("(model (part optimized {expression}))");

        let bundle = render_model(
            &source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &test_state(&root),
            &resolver,
        )
        .await
        .expect("density surface hybrid render");

        assert!(bundle.export_artifacts.iter().any(|artifact| {
            artifact.format.eq_ignore_ascii_case("step")
                && std::path::Path::new(&artifact.path).is_file()
        }));
        assert_eq!(
            bundle
                .geometry_provenance
                .as_ref()
                .map(|provenance| &provenance.representation),
            Some(&crate::contracts::GeometryRepresentation::FacetedPolyBrep)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn closed_surface_trim_solidifies_before_later_difference() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root =
            crate::ecky_cad_host::direct_occt_sdk::bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let root = temp_root("surface-trim-solidify-difference");
        let resolver = TestResolver { root: root.clone() };
        let stl_path = root.join("cube.stl");
        write_ascii_cube_stl_fixture(&stl_path);
        let indexed = crate::ecky_ir::mesh_asset::IndexedMeshAsset::from_stl(
            crate::ecky_ir::mesh_asset::MeshAssetSource::Imported,
            &stl_path,
        )
        .expect("indexed cube");
        let source = format!(
            r#"(model
  (part scanned
    (difference
      (solidify
        (surface-trim
          (import-stl {:?})
          :schema-version 1
          :source-digest {:?}
          :loop
            ((mesh-anchor 4 0.25 0.25 0.5)
             (mesh-anchor 7 0.5 0.25 0.25)
             (mesh-anchor 6 0.25 0.25 0.5)
             (mesh-anchor 9 0.5 0.25 0.25)
             (mesh-anchor 8 0.25 0.25 0.5)
             (mesh-anchor 11 0.5 0.25 0.25)
             (mesh-anchor 10 0.25 0.25 0.5)
             (mesh-anchor 5 0.5 0.25 0.25))
          :keep-seed (mesh-anchor 2 0.333333333 0.333333333 0.333333334)
          :path-mode "shortest"
          :cap "flat"))
      (translate 0 0 -0.25 (cylinder 0.25 1.5)))))"#,
            stl_path.to_string_lossy(),
            indexed.content_digest(),
        );

        let bundle = render_model(
            &source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &test_state(&root),
            &resolver,
        )
        .await
        .expect("closed trim must feed later difference");

        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
        assert_eq!(
            crate::services::structural_verification::model_stl_non_manifold_edge_count(
                std::path::Path::new(&bundle.model_stl_path),
            )
            .expect("trimmed Boolean STL topology"),
            0
        );
        let provenance = bundle
            .geometry_provenance
            .as_ref()
            .expect("hybrid provenance");
        assert_eq!(provenance.closed, Some(true));
        assert_eq!(provenance.boundary_or_non_manifold_edge_count, Some(0));
        assert_eq!(provenance.source_mesh_digests.len(), 1);
        assert_ne!(provenance.source_mesh_digests[0], indexed.content_digest());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn pure_mesh_literal_dispatches_to_rust_mesh_without_step() {
        let root = temp_root("pure-mesh-literal-dispatch");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"(model
  (part surface
    (mesh
      :vertices ((0 0 0) (20 0 0) (0 20 0))
      :triangles ((0 1 2)))))"#;

        let bundle = render_model(
            source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::Freecad),
            None,
            &test_state(&root),
            &resolver,
        )
        .await
        .expect("mesh literal must redirect to Rust mesh runtime");

        assert_eq!(bundle.geometry_backend, GeometryBackend::EckyRust);
        assert!(std::path::Path::new(&bundle.model_stl_path).is_file());
        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format.eq_ignore_ascii_case("stl")));
        assert!(!bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format.eq_ignore_ascii_case("step")));
        let provenance = bundle
            .geometry_provenance
            .as_ref()
            .expect("pure mesh provenance");
        assert_eq!(
            provenance.representation,
            GeometryRepresentation::MeshNative
        );
        assert_eq!(provenance.closed, Some(false));
        assert_eq!(provenance.boundary_or_non_manifold_edge_count, Some(3));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn open_mesh_rejects_before_brep_consumer_execution() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root =
            crate::ecky_cad_host::direct_occt_sdk::bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let root = temp_root("open-mesh-brep-rejection");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"(model
  (part surface
    (difference
      (mesh
        :vertices ((0 0 0) (20 0 0) (0 20 0))
        :triangles ((0 1 2)))
      (sphere 2))))"#;

        let error = render_model(
            source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &test_state(&root),
            &resolver,
        )
        .await
        .expect_err("open mesh must not enter OCCT solidification");
        let diagnostic = format!("{} {}", error, error.details.as_deref().unwrap_or(""));
        assert!(diagnostic.contains("open `mesh`"), "{diagnostic}");
        assert!(
            diagnostic.contains("3 boundary/non-manifold edges"),
            "{diagnostic}"
        );
        assert!(diagnostic.contains("consumer `difference`"), "{diagnostic}");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn broad_mesh_origin_chamfer_rejects_before_kernel_execution() {
        let root = temp_root("broad-mesh-origin-chamfer-rejection");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"(model
  (part body
    (chamfer 0.5 :edges "all"
      (wall-pattern (:mode ribs :depth 0.4 :uFreq 8)
        (extrude (circle 10) 20)))))"#;

        let error = render_model(
            source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &test_state(&root),
            &resolver,
        )
        .await
        .expect_err("broad mesh-origin chamfer must be rejected before kernel");
        let diagnostic = format!("{} {}", error, error.details.as_deref().unwrap_or(""));
        assert!(
            diagnostic.contains("Mesh-origin faceted BRep `chamfer`"),
            "{diagnostic}"
        );
        assert!(diagnostic.contains("selector `all`"), "{diagnostic}");
        assert!(
            diagnostic.contains("rejected before OCCT kernel execution"),
            "{diagnostic}"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn hybrid_poly_brep_accepts_llm_generated_polyhedron_asset() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root =
            crate::ecky_cad_host::direct_occt_sdk::bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let root = temp_root("hybrid-poly-brep-generated-mesh");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"(model
  (part generated-body
    (difference
      (polyhedron
        :vertices
          ((-10 -10 0) (10 -10 0) (-10 10 0) (10 10 0)
           (-10 -10 20) (10 -10 20) (-10 10 20) (10 10 20))
        :triangles
          ((0 4 6) (0 6 2) (1 3 7) (1 7 5)
           (0 1 5) (0 5 4) (2 6 7) (2 7 3)
           (0 2 3) (0 3 1) (4 5 7) (4 7 6)))
      (translate 0 0 -1 (cylinder 3 22)))))"#;

        let bundle = render_model(
            source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &test_state(&root),
            &resolver,
        )
        .await
        .expect("LLM-generated polyhedron should enter the hybrid bridge");

        assert!(!bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format.eq_ignore_ascii_case("step")));
        assert_eq!(
            crate::services::structural_verification::model_stl_non_manifold_edge_count(
                std::path::Path::new(&bundle.model_stl_path),
            )
            .expect("hybrid STL topology"),
            0
        );
        let provenance = bundle
            .geometry_provenance
            .as_ref()
            .expect("mesh-native provenance");
        assert_eq!(
            provenance.representation,
            GeometryRepresentation::MeshNative
        );
        assert_eq!(provenance.closed, Some(true));
        assert!(!provenance.source_mesh_digests.is_empty());
        let manifest = crate::model_runtime::read_model_manifest(&resolver, &bundle.model_id)
            .expect("stored manifest");
        assert_eq!(manifest.geometry_provenance.as_ref(), Some(provenance));
        assert_eq!(
            manifest.source_digest.as_deref(),
            Some(crate::services::render_snapshot::canonical_source_digest(source).as_str())
        );
        assert!(manifest
            .warnings
            .iter()
            .any(|warning| warning.contains("Hybrid mesh Boolean route")));
        let bundle_dir = crate::model_runtime::runtime_bundle_dir(&resolver, &bundle.model_id)
            .expect("bundle dir");
        let plan = std::fs::read_to_string(bundle_dir.join("plan.json")).expect("runner plan");
        assert!(plan.contains("import-indexed-mesh"));
        assert!(!bundle_dir.join("model.step").exists());

        std::fs::remove_dir_all(root).unwrap();
    }

    /// Regression guard: a pure OCCT model (no mesh ops) must NOT use the
    /// hybrid bridge. It routes through the normal Direct OCCT path.
    #[tokio::test]
    async fn pure_occt_model_does_not_use_hybrid_bridge() {
        let root = temp_root("pure-occt-no-hybrid");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"(model (part body (difference (box 20 20 20) (cylinder 5 30))))"#;

        let bundle = render_model(
            source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &test_state(&root),
            &resolver,
        )
        .await
        .expect("pure OCCT render");

        let manifest = crate::model_runtime::read_model_manifest(&resolver, &bundle.model_id)
            .expect("stored manifest");
        assert_eq!(
            manifest.source_digest.as_deref(),
            Some(crate::services::render_snapshot::canonical_source_digest(source).as_str())
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn analytic_chamfer_routes_to_direct_occt_with_analytic_provenance() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root =
            crate::ecky_cad_host::direct_occt_sdk::bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let root = temp_root("analytic-chamfer-direct-occt");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"(model
  (part body
    (chamfer 1 :edges "top" (box 20 20 10))))"#;

        let bundle = render_model(
            source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &test_state(&root),
            &resolver,
        )
        .await
        .expect("analytic chamfer render");

        assert!(bundle
            .export_artifacts
            .iter()
            .any(|artifact| artifact.format.eq_ignore_ascii_case("step")));
        assert_eq!(
            bundle
                .geometry_provenance
                .as_ref()
                .map(|provenance| &provenance.representation),
            Some(&GeometryRepresentation::AnalyticBrep)
        );
        let manifest = crate::model_runtime::read_model_manifest(&resolver, &bundle.model_id)
            .expect("stored manifest");
        assert_eq!(
            manifest
                .geometry_provenance
                .as_ref()
                .map(|provenance| &provenance.representation),
            Some(&GeometryRepresentation::AnalyticBrep)
        );
        assert!(
            !manifest
                .warnings
                .iter()
                .any(|warning| warning.contains("Hybrid")),
            "analytic chamfer must not use hybrid bridge: {:?}",
            manifest.warnings
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    /// Regression guard: a pure mesh model (wall-pattern with no post-boundary
    /// BRep ops) must NOT use the hybrid bridge. It routes through the mesh
    /// renderer directly.
    #[tokio::test]
    async fn pure_mesh_model_does_not_use_hybrid_bridge() {
        let root = temp_root("pure-mesh-no-hybrid");
        let resolver = TestResolver { root: root.clone() };
        let source = r#"(model
  (part body
    (wall-pattern (:mode ribs :depth 0.4 :uFreq 8)
      (extrude (circle 10) 20))))"#;

        let bundle = render_model(
            source,
            &DesignParams::new(),
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &test_state(&root),
            &resolver,
        )
        .await
        .expect("pure mesh render");

        assert!(
            std::path::Path::new(&bundle.model_stl_path).is_file(),
            "STL must exist"
        );
        // Pure mesh: no STEP (mesh renderer can't produce STEP).
        let has_step = bundle
            .export_artifacts
            .iter()
            .any(|a| a.format.eq_ignore_ascii_case("step"));
        assert!(
            !has_step,
            "pure mesh model must not produce STEP (no OCCT involvement)"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// PG2: The iPhone 17e case fixture uses wall-pattern (cellular) followed
    /// by chamfer (BRep-required) followed by difference/union (BRep-required).
    /// This is the real-world Hybrid case.
    ///
    /// KNOWN LIMITATION: chamfer on solidified poly BRep fails — OCCT's
    /// `BRepFilletAPI_MakeChamfer` cannot find meaningful edges on the
    /// thousands of tiny planar facets produced by `solidify(import-stl)`.
    /// The bridge routing is correct (classification, slicing, dispatch all
    /// work), but chamfer/fillet on poly edges is best-effort per design.md.
    #[tokio::test]
    async fn hybrid_poly_brep_iphone_case_rejects_dense_broad_chamfer_before_kernel() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let runtime_root =
            crate::ecky_cad_host::direct_occt_sdk::bundled_occt_runtime_root_from_repo(repo_root);
        if !runtime_root.exists() {
            return;
        }
        let root = temp_root("hybrid-poly-brep-iphone-case");
        let resolver = TestResolver { root: root.clone() };
        let source = example_fixture_source("iphone-17e-case-multipart.ecky");
        // Exact values from the original failing `Iphone 17e case - fucked`
        // thread. Source defaults render successfully, but this saved design
        // previously aborted the native shim with `StdFail_NotDone`.
        let params = DesignParams::from([
            ("back-pattern-depth".into(), ParamValue::Number(0.45)),
            ("back-pattern-phase".into(), ParamValue::Number(0.18)),
            ("back-pattern-u-frequency".into(), ParamValue::Number(7.0)),
            ("back-pattern-v-frequency".into(), ParamValue::Number(13.0)),
            (
                "bottom-port-gap-from-usb-edge".into(),
                ParamValue::Number(1.2),
            ),
            ("bottom-port-hole-diameter".into(), ParamValue::Number(1.7)),
            ("bottom-port-hole-pitch".into(), ParamValue::Number(4.0)),
            (
                "camera-frame-seat-clearance".into(),
                ParamValue::Number(0.2),
            ),
            (
                "camera-island-corner-radius".into(),
                ParamValue::Number(7.0),
            ),
            ("camera-island-length".into(), ParamValue::Number(29.0)),
            ("camera-island-width".into(), ParamValue::Number(31.0)),
            (
                "camera-lock-hole-clearance".into(),
                ParamValue::Number(0.12),
            ),
            (
                "camera-post-bead-extra-radius".into(),
                ParamValue::Number(0.28),
            ),
            (
                "camera-post-hole-clearance".into(),
                ParamValue::Number(0.18),
            ),
            ("camera-post-length".into(), ParamValue::Number(4.2)),
            ("camera-post-radius".into(), ParamValue::Number(1.25)),
            ("camera-seat-depth".into(), ParamValue::Number(0.55)),
            ("front-lip-height".into(), ParamValue::Number(1.4)),
            ("front-rim-bottom-overlap".into(), ParamValue::Number(1.05)),
            ("front-rim-top-overlap".into(), ParamValue::Number(0.25)),
            ("outer-bottom-edge-chamfer".into(), ParamValue::Number(0.6)),
            ("outer-top-edge-chamfer".into(), ParamValue::Number(0.8)),
            ("phone-pocket-clearance".into(), ParamValue::Number(0.35)),
            ("rear-camera-hole-diameter".into(), ParamValue::Number(15.8)),
            ("rear-flash-hole-diameter".into(), ParamValue::Number(5.0)),
            ("rear-mic-hole-diameter".into(), ParamValue::Number(2.0)),
            ("rear-panel-thickness".into(), ParamValue::Number(1.6)),
            (
                "side-button-inner-relief-extra-length".into(),
                ParamValue::Number(1.5),
            ),
            (
                "side-button-inner-relief-width".into(),
                ParamValue::Number(4.2),
            ),
            (
                "side-button-membrane-thickness".into(),
                ParamValue::Number(0.45),
            ),
            ("side-button-pad-raise".into(), ParamValue::Number(0.8)),
            ("side-button-pad-width".into(), ParamValue::Number(4.6)),
            ("side-wall-thickness".into(), ParamValue::Number(2.5)),
            ("usb-c-port-opening-height".into(), ParamValue::Number(5.2)),
            ("usb-c-port-opening-width".into(), ParamValue::Number(13.5)),
            (
                "use-back-cellular-pattern".into(),
                ParamValue::Boolean(true),
            ),
        ]);
        let result = render_model(
            &source,
            &params,
            Some(MacroDialect::EckyIrV0),
            Some(GeometryBackend::EckyRust),
            None,
            &test_state(&root),
            &resolver,
        )
        .await;

        let err = result.expect_err("dense broad faceted chamfer must be rejected");
        assert_eq!(err.code, crate::contracts::AppErrorCode::Validation);
        assert!(err.message.contains("Mesh-origin faceted BRep `chamfer`"));
        assert!(err.message.contains("selector `edge-clauses`"));
        assert!(err
            .message
            .contains("rejected before OCCT kernel execution"));

        std::fs::remove_dir_all(root).unwrap();
    }
}
