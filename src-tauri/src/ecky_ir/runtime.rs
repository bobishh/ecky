use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use csgrs::float_types::parry3d::na::Vector3;
use csgrs::traits::CSG;
use sha2::{Digest, Sha256};

use crate::models::{
    AppError, AppResult, ArtifactBundle, DesignParams, DocumentMetadata, EngineKind, FeatureGraph,
    FeatureNode, FeatureOutputRef, GeometryBackend, ManifestBounds, ModelManifest, ModelSourceKind,
    ParamValue, ParameterGroup, ParsedParamsResult, PartBinding, PathResolver, SelectionTarget,
    SourceLanguage, SourceRef, ViewerAsset, ViewerAssetFormat, MODEL_RUNTIME_SCHEMA_VERSION,
};

use super::mesh_ops::eval_geometry_expr;
use super::model::{
    build_param_env, core_program_param_defaults, materialize_selector_nodes, parse_model,
    parsed_params_from_core_program, parsed_params_from_model, IrExpr, IrModel,
};
use super::shared::{unsupported, validation, IrMesh};
use super::syntax::canonicalize;
use crate::ecky_core_ir::{
    CoreArrayOp, CoreBooleanOp, CoreFeatureDecl, CoreFrameOp, CoreLiteral, CoreMetaOp, CoreNode,
    CoreNodeKind, CoreOperation, CorePart, CorePathOp, CorePrimitive, CoreProgram, CoreReference,
    CoreSelectorPayload, CoreSurfaceOp, CoreSymbol, CoreTransformOp, CoreValueKind, SourceSpan,
};
use crate::ecky_ir::edge_ops::{
    edge_selector_spec_from_core_payload, face_selector_spec_from_core_payload,
};

pub(super) const MODEL_RUNTIME_ROOT: &str = "model-runtime";
pub(super) const GENERATED_ARTIFACT_DIR: &str = "generated";
pub(super) const BUNDLE_FILE_NAME: &str = "bundle.json";
pub(super) const MANIFEST_FILE_NAME: &str = "manifest.json";
pub(super) const SOURCE_FILE_NAME: &str = "source.ecky";
pub(super) const PREVIEW_STL_FILE_NAME: &str = "preview.stl";
pub(super) const PARTS_DIR_NAME: &str = "parts";
const CORE_AST_SCHEMA_VERSION: u32 = 1;
pub(super) fn mesh_volume(mesh: &IrMesh) -> Option<f64> {
    let tri_mesh = mesh.triangulate();
    if tri_mesh.polygons.is_empty() {
        return None;
    }
    let mut volume = 0.0f64;
    for poly in &tri_mesh.polygons {
        if poly.vertices.len() != 3 {
            continue;
        }
        let a = &poly.vertices[0].pos;
        let b = &poly.vertices[1].pos;
        let c = &poly.vertices[2].pos;
        // Signed volume of tetrahedron formed with origin
        let cross = Vector3::new(
            b.y * c.z - b.z * c.y,
            b.z * c.x - b.x * c.z,
            b.x * c.y - b.y * c.x,
        );
        volume += a.x * cross.x + a.y * cross.y + a.z * cross.z;
    }
    let vol = (volume / 6.0).abs();
    if vol.is_finite() && vol > 0.0 {
        Some(vol)
    } else {
        None
    }
}

/// Compute the total surface area of a triangulated mesh.
///
/// For each triangle with vertices (a, b, c):
///   area = ||(b - a) × (c - a)|| / 2
pub(super) fn mesh_area(mesh: &IrMesh) -> Option<f64> {
    let tri_mesh = mesh.triangulate();
    if tri_mesh.polygons.is_empty() {
        return None;
    }
    let mut area = 0.0f64;
    for poly in &tri_mesh.polygons {
        if poly.vertices.len() != 3 {
            continue;
        }
        let a = &poly.vertices[0].pos;
        let b = &poly.vertices[1].pos;
        let c = &poly.vertices[2].pos;
        let ab = Vector3::new(b.x - a.x, b.y - a.y, b.z - a.z);
        let ac = Vector3::new(c.x - a.x, c.y - a.y, c.z - a.z);
        let cross = ab.cross(&ac);
        area += cross.norm();
    }
    let result = area / 2.0;
    if result.is_finite() && result > 0.0 {
        Some(result)
    } else {
        None
    }
}

pub(super) fn bounds_from_mesh(mesh: &IrMesh) -> ManifestBounds {
    let bb = mesh.bounding_box();
    ManifestBounds {
        x_min: bb.mins.x,
        y_min: bb.mins.y,
        z_min: bb.mins.z,
        x_max: bb.maxs.x,
        y_max: bb.maxs.y,
        z_max: bb.maxs.z,
    }
}

pub(super) fn runtime_root(app: &dyn PathResolver) -> AppResult<PathBuf> {
    let root = app.app_data_dir().join(MODEL_RUNTIME_ROOT);
    fs::create_dir_all(&root).map_err(|err| AppError::persistence(err.to_string()))?;
    Ok(root)
}

pub(super) fn bundle_dir(app: &dyn PathResolver, model_id: &str) -> AppResult<PathBuf> {
    let path = runtime_root(app)?
        .join(GENERATED_ARTIFACT_DIR)
        .join(model_id);
    fs::create_dir_all(&path).map_err(|err| AppError::persistence(err.to_string()))?;
    Ok(path)
}

pub(super) fn write_bundle(path: &Path, bundle: &ArtifactBundle) -> AppResult<()> {
    let data = serde_json::to_string_pretty(bundle)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    fs::write(path, data).map_err(|err| AppError::persistence(err.to_string()))
}

pub(super) fn write_manifest(path: &Path, manifest: &ModelManifest) -> AppResult<()> {
    let data = serde_json::to_string_pretty(manifest)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    fs::write(path, data).map_err(|err| AppError::persistence(err.to_string()))
}

pub fn derive_controls(source: &str) -> AppResult<ParsedParamsResult> {
    let model = parse_model(source)?;
    derive_controls_from_model(&model)
}

pub(crate) fn derive_controls_from_core_program(
    program: &CoreProgram,
) -> AppResult<ParsedParamsResult> {
    parsed_params_from_core_program(program)
}

pub(crate) fn derive_controls_from_model(model: &IrModel) -> AppResult<ParsedParamsResult> {
    Ok(parsed_params_from_model(model))
}

pub(super) fn load_cached_bundle(bundle_dir: &Path) -> AppResult<Option<ArtifactBundle>> {
    let bundle_path = bundle_dir.join(BUNDLE_FILE_NAME);
    if !bundle_path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&bundle_path)
        .map_err(|e| AppError::persistence(format!("Failed to read bundle: {}", e)))?;
    let bundle: ArtifactBundle = serde_json::from_str(&raw)
        .map_err(|e| AppError::parse(format!("Failed to parse bundle: {}", e)))?;
    if !Path::new(&bundle.manifest_path).exists() || !Path::new(&bundle.preview_stl_path).exists() {
        return Ok(None);
    }
    Ok(Some(bundle))
}

pub fn render_model(
    source: &str,
    parameters: &DesignParams,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let model = parse_model(source)?;
    let canonical_source = canonicalize(source)?;
    render_model_from_model(&model, &canonical_source, parameters, app)
}

#[derive(Clone)]
struct RuntimePart {
    part_id: String,
    label: String,
    expr: IrExpr,
    feature_decl: Option<CoreFeatureDecl>,
    source_ref: Option<SourceRef>,
    dependency_ids: Vec<String>,
}

#[derive(Clone)]
struct CoreAstIdentity {
    core_digest: String,
    ast_schema_version: u32,
}

fn runtime_part_feature_id(part_id: &str) -> String {
    format!("part:{}", part_id)
}

fn runtime_part_source_ref(part_id: &str, span: Option<SourceSpan>) -> Option<SourceRef> {
    if part_id.trim().is_empty() {
        return None;
    }

    Some(SourceRef {
        source_id: None,
        path: Some(format!("/parts/{}/root", part_id)),
        start_byte: span.map(|span| span.start),
        end_byte: span.map(|span| span.end),
    })
}

fn runtime_part_feature_graph(
    parts: &[RuntimePart],
    selection_targets: &[SelectionTarget],
) -> FeatureGraph {
    let nodes = parts
        .iter()
        .map(|part| {
            let fallback_feature_id = runtime_part_feature_id(&part.part_id);
            let feature_id = part
                .feature_decl
                .as_ref()
                .map(|decl| decl.feature_id.trim())
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| fallback_feature_id.clone());
            let kind = part
                .feature_decl
                .as_ref()
                .map(|decl| decl.role.trim())
                .filter(|role| !role.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| "part".to_string());
            let target_ids = selection_targets
                .iter()
                .filter(|target| target.part_id == part.part_id)
                .filter_map(runtime_selection_target_output_id)
                .map(str::to_string)
                .collect::<Vec<_>>();
            let output_refs = if target_ids.is_empty() {
                Vec::new()
            } else {
                vec![FeatureOutputRef {
                    feature_id: feature_id.clone(),
                    output_id: "selectionTargets".to_string(),
                    target_ids,
                }]
            };

            FeatureNode {
                feature_id,
                kind,
                label: if part.label.trim().is_empty() {
                    part.part_id.clone()
                } else {
                    part.label.clone()
                },
                source_ref: part.source_ref.clone(),
                dependency_ids: runtime_feature_dependency_ids(part),
                output_refs,
                ports: Vec::new(),
            }
        })
        .collect();

    FeatureGraph { nodes }
}

fn runtime_selection_target_output_id(target: &SelectionTarget) -> Option<&str> {
    target
        .target_id
        .as_deref()
        .or(target.durable_target_id.as_deref())
        .or(target.canonical_target_id.as_deref())
}

fn runtime_feature_dependency_ids(part: &RuntimePart) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut ids = Vec::new();

    if let Some(feature_decl) = &part.feature_decl {
        for key in &feature_decl.param_keys {
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            let normalized = key.to_string();
            if seen.insert(normalized.clone()) {
                ids.push(normalized);
            }
        }
    }

    for key in &part.dependency_ids {
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let normalized = key.to_string();
        if seen.insert(normalized.clone()) {
            ids.push(normalized);
        }
    }

    ids
}

pub(crate) fn build_core_program_param_env_for_eval(
    program: &CoreProgram,
    parameters: &DesignParams,
) -> AppResult<BTreeMap<String, ParamValue>> {
    let mut env = core_program_param_defaults(program)?;
    for (key, value) in parameters {
        env.insert(key.clone(), value.clone());
    }
    Ok(env)
}

pub(crate) fn eval_core_number_with_locals(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<f64> {
    let expr = core_node_to_eval_ir_expr(node, param_names, env)?;
    super::eval_scalar::eval_number(&expr, env)
}

pub(crate) fn eval_core_bool_with_locals(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<bool> {
    let expr = core_node_to_eval_ir_expr(node, param_names, env)?;
    super::eval_scalar::eval_bool(&expr, env)
}

pub(crate) fn eval_core_stringish_with_locals(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<String> {
    let expr = core_node_to_eval_ir_expr(node, param_names, env)?;
    super::eval_scalar::eval_stringish(&expr, env)
}

fn core_node_to_eval_ir_expr(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<IrExpr> {
    let mut used_local_names = BTreeMap::new();
    let locals = env
        .keys()
        .map(|key| (key.clone(), key.clone()))
        .collect::<BTreeMap<_, _>>();
    runtime_core_node_to_ir_expr(
        node,
        param_names,
        &BTreeMap::new(),
        &locals,
        &mut used_local_names,
    )
}

fn runtime_core_part_to_runtime_part(
    part: &CorePart,
    param_names: &BTreeMap<u64, String>,
    feature_decls: &BTreeMap<String, CoreFeatureDecl>,
) -> AppResult<RuntimePart> {
    let mut used_local_names = BTreeMap::new();
    Ok(RuntimePart {
        part_id: part.key.clone(),
        label: part.label.clone(),
        expr: materialize_selector_nodes(runtime_core_node_to_ir_expr(
            &part.root,
            param_names,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &mut used_local_names,
        )?)?,
        feature_decl: feature_decls.get(&part.key).cloned(),
        source_ref: runtime_part_source_ref(&part.key, part.root.span),
        dependency_ids: core_node_parameter_dependencies(&part.root, param_names),
    })
}

fn core_node_parameter_dependencies(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
) -> Vec<String> {
    let mut keys = BTreeSet::new();
    collect_core_node_parameter_dependencies(node, param_names, &mut keys);
    keys.into_iter().collect()
}

fn collect_core_node_parameter_dependencies(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    keys: &mut BTreeSet<String>,
) {
    match &node.kind {
        CoreNodeKind::Literal(_) => {}
        CoreNodeKind::Reference(CoreReference::Parameter(param_id)) => {
            if let Some(key) = param_names.get(&param_id.raw()) {
                keys.insert(key.clone());
            }
        }
        CoreNodeKind::Reference(_) => {}
        CoreNodeKind::Build { bindings, result } => {
            for binding in bindings {
                collect_core_node_parameter_dependencies(&binding.value, param_names, keys);
            }
            collect_core_node_parameter_dependencies(result, param_names, keys);
        }
        CoreNodeKind::Let { bindings, body } => {
            for binding in bindings {
                collect_core_node_parameter_dependencies(&binding.value, param_names, keys);
            }
            collect_core_node_parameter_dependencies(body, param_names, keys);
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_core_node_parameter_dependencies(condition, param_names, keys);
            collect_core_node_parameter_dependencies(then_branch, param_names, keys);
            collect_core_node_parameter_dependencies(else_branch, param_names, keys);
        }
        CoreNodeKind::Call { args, keywords, .. } => {
            for arg in args {
                collect_core_node_parameter_dependencies(arg, param_names, keys);
            }
            for keyword in keywords {
                collect_core_node_parameter_dependencies(keyword.source_node(), param_names, keys);
            }
        }
        CoreNodeKind::Range { start, end } => {
            collect_core_node_parameter_dependencies(start, param_names, keys);
            collect_core_node_parameter_dependencies(end, param_names, keys);
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for source in sources {
                collect_core_node_parameter_dependencies(source, param_names, keys);
            }
            collect_core_node_parameter_dependencies(body, param_names, keys);
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for arg in args {
                collect_core_node_parameter_dependencies(arg, param_names, keys);
            }
            collect_core_node_parameter_dependencies(list, param_names, keys);
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for item in items {
                collect_core_node_parameter_dependencies(item, param_names, keys);
            }
        }
    }
}

fn ir_expr_parameter_dependencies(expr: &IrExpr, parameter_keys: &[String]) -> Vec<String> {
    let parameter_key_set = parameter_keys.iter().cloned().collect::<BTreeSet<_>>();
    let mut used = BTreeSet::new();
    collect_ir_expr_parameter_dependencies(expr, &parameter_key_set, &mut used);
    used.into_iter().collect()
}

fn collect_ir_expr_parameter_dependencies(
    expr: &IrExpr,
    parameter_keys: &BTreeSet<String>,
    used: &mut BTreeSet<String>,
) {
    match expr {
        IrExpr::Symbol(symbol) => {
            if parameter_keys.contains(symbol) {
                used.insert(symbol.clone());
            }
        }
        IrExpr::List(items) => {
            for (index, item) in items.iter().enumerate() {
                if index == 0 && matches!(item, IrExpr::Symbol(_)) {
                    continue;
                }
                collect_ir_expr_parameter_dependencies(item, parameter_keys, used);
            }
        }
        IrExpr::Number(_)
        | IrExpr::Boolean(_)
        | IrExpr::String(_)
        | IrExpr::Keyword(_)
        | IrExpr::Selector(_) => {}
    }
}

fn runtime_ir_expr_from_core_selector_payload(payload: &CoreSelectorPayload) -> AppResult<IrExpr> {
    match payload {
        CoreSelectorPayload::EdgeAll
        | CoreSelectorPayload::EdgeClauses(_)
        | CoreSelectorPayload::EdgeTargetIds(_) => Ok(IrExpr::Selector(
            crate::ecky_ir::model::IrSelectorExpr::Edge(edge_selector_spec_from_core_payload(
                payload,
            )?),
        )),
        CoreSelectorPayload::FaceClauses(_) | CoreSelectorPayload::FaceTargetIds(_) => Ok(
            IrExpr::Selector(crate::ecky_ir::model::IrSelectorExpr::Face(
                face_selector_spec_from_core_payload(payload)?,
            )),
        ),
    }
}

fn runtime_core_node_to_ir_expr(
    node: &CoreNode,
    param_names: &BTreeMap<u64, String>,
    refs: &BTreeMap<u64, String>,
    locals: &BTreeMap<String, String>,
    used_local_names: &mut BTreeMap<String, usize>,
) -> AppResult<IrExpr> {
    match &node.kind {
        CoreNodeKind::Literal(CoreLiteral::Number(n)) => Ok(IrExpr::number(*n)),
        CoreNodeKind::Literal(CoreLiteral::Boolean(flag)) => Ok(IrExpr::boolean(*flag)),
        CoreNodeKind::Literal(CoreLiteral::Text(text)) => Ok(IrExpr::string(text.clone())),
        CoreNodeKind::Literal(CoreLiteral::Symbol(symbol)) => {
            Ok(IrExpr::symbol(runtime_core_symbol_name(symbol)))
        }
        CoreNodeKind::Literal(CoreLiteral::Point2([x, y])) => {
            Ok(IrExpr::list(vec![IrExpr::number(*x), IrExpr::number(*y)]))
        }
        CoreNodeKind::Literal(CoreLiteral::Point3([x, y, z])) => Ok(IrExpr::list(vec![
            IrExpr::number(*x),
            IrExpr::number(*y),
            IrExpr::number(*z),
        ])),
        CoreNodeKind::Reference(CoreReference::Local(name)) => Ok(IrExpr::symbol(
            locals.get(name).cloned().unwrap_or_else(|| name.clone()),
        )),
        CoreNodeKind::Reference(CoreReference::Node(id)) => refs
            .get(&id.raw())
            .map(|name| IrExpr::symbol(name.clone()))
            .ok_or_else(|| unsupported(format!("Unsupported Core node reference {:?}.", id))),
        CoreNodeKind::Reference(CoreReference::Parameter(id)) => param_names
            .get(&id.raw())
            .map(|name| IrExpr::symbol(name.clone()))
            .ok_or_else(|| unsupported(format!("Unsupported Core parameter reference {:?}.", id))),
        CoreNodeKind::Reference(other) => Err(unsupported(format!(
            "Unsupported Core IR reference in runtime bridge: {:?}.",
            other
        ))),
        CoreNodeKind::Build { bindings, result } => {
            let mut items = vec![IrExpr::symbol("build")];
            let mut nested = refs.clone();
            let mut nested_locals = locals.clone();
            for binding in bindings {
                let ir_name = runtime_allocate_local_name(&binding.name, used_local_names);
                let mut shape_items = vec![
                    IrExpr::symbol("shape"),
                    IrExpr::symbol(ir_name.clone()),
                    runtime_core_node_to_ir_expr(
                        &binding.value,
                        param_names,
                        &nested,
                        &nested_locals,
                        used_local_names,
                    )?,
                ];
                if binding.value.value_kind != CoreValueKind::Any {
                    shape_items.push(IrExpr::keyword("value-kind"));
                    shape_items.push(IrExpr::symbol(runtime_core_value_kind_tag(
                        binding.value.value_kind,
                    )));
                }
                items.push(IrExpr::list(shape_items));
                nested.insert(binding.value.id.raw(), ir_name.clone());
                nested_locals.insert(binding.name.clone(), ir_name);
            }
            items.push(IrExpr::list(vec![
                IrExpr::symbol("result"),
                runtime_core_node_to_ir_expr(
                    result,
                    param_names,
                    &nested,
                    &nested_locals,
                    used_local_names,
                )?,
            ]));
            Ok(IrExpr::list(items))
        }
        CoreNodeKind::Let { bindings, body } => {
            let mut nested_refs = refs.clone();
            let mut nested_locals = locals.clone();
            let ir_binding_names = bindings
                .iter()
                .map(|binding| {
                    (
                        binding.name.clone(),
                        runtime_allocate_local_name(&binding.name, used_local_names),
                        binding.value.id.raw(),
                    )
                })
                .collect::<Vec<_>>();
            let binding_values = bindings
                .iter()
                .zip(ir_binding_names.iter())
                .map(|(binding, (_, ir_name, node_id))| {
                    nested_refs.insert(*node_id, ir_name.clone());
                    let mut pair = vec![
                        IrExpr::symbol(ir_name.clone()),
                        runtime_core_node_to_ir_expr(
                            &binding.value,
                            param_names,
                            refs,
                            locals,
                            used_local_names,
                        )?,
                    ];
                    if binding.value.value_kind != CoreValueKind::Any {
                        pair.push(IrExpr::keyword("value-kind"));
                        pair.push(IrExpr::symbol(runtime_core_value_kind_tag(
                            binding.value.value_kind,
                        )));
                    }
                    Ok(IrExpr::list(pair))
                })
                .collect::<AppResult<Vec<_>>>()?;
            for (original_name, ir_name, _) in ir_binding_names {
                nested_locals.insert(original_name, ir_name);
            }
            Ok(IrExpr::list(vec![
                IrExpr::symbol("let"),
                IrExpr::list(binding_values),
                runtime_core_node_to_ir_expr(
                    body,
                    param_names,
                    &nested_refs,
                    &nested_locals,
                    used_local_names,
                )?,
            ]))
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => Ok(IrExpr::list(vec![
            IrExpr::symbol("if"),
            runtime_core_node_to_ir_expr(condition, param_names, refs, locals, used_local_names)?,
            runtime_core_node_to_ir_expr(then_branch, param_names, refs, locals, used_local_names)?,
            runtime_core_node_to_ir_expr(else_branch, param_names, refs, locals, used_local_names)?,
        ])),
        CoreNodeKind::Call { op, args, keywords } => {
            let mut items = vec![IrExpr::symbol(runtime_core_operation_name(op))];
            for arg in args {
                items.push(runtime_core_node_to_ir_expr(
                    arg,
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                )?);
            }
            for keyword in keywords {
                items.push(IrExpr::keyword(keyword.name.clone()));
                items.push(match (keyword.name.as_str(), keyword.selector_payload()) {
                    ("edges", None) => {
                        return Err(validation(
                            "CoreProgram `:edges` keyword requires selector payload.",
                        ))
                    }
                    ("faces", None) => {
                        return Err(validation(
                            "CoreProgram `:faces` keyword requires selector payload.",
                        ))
                    }
                    (
                        "edges",
                        Some(
                            crate::ecky_core_ir::CoreSelectorPayload::FaceClauses(_)
                            | crate::ecky_core_ir::CoreSelectorPayload::FaceTargetIds(_),
                        ),
                    ) => {
                        return Err(validation(
                            "CoreProgram `:edges` keyword requires edge selector payload.",
                        ))
                    }
                    (
                        "faces",
                        Some(
                            crate::ecky_core_ir::CoreSelectorPayload::EdgeAll
                            | crate::ecky_core_ir::CoreSelectorPayload::EdgeClauses(_)
                            | crate::ecky_core_ir::CoreSelectorPayload::EdgeTargetIds(_),
                        ),
                    ) => {
                        return Err(validation(
                            "CoreProgram `:faces` keyword requires face selector payload.",
                        ))
                    }
                    (_, Some(selector)) => runtime_ir_expr_from_core_selector_payload(selector)?,
                    (_, None) => runtime_core_node_to_ir_expr(
                        keyword.source_node(),
                        param_names,
                        refs,
                        locals,
                        used_local_names,
                    )?,
                });
            }
            Ok(IrExpr::list(items))
        }
        CoreNodeKind::Range { start, end } => Ok(IrExpr::list(vec![
            IrExpr::symbol("range"),
            runtime_core_node_to_ir_expr(start, param_names, refs, locals, used_local_names)?,
            runtime_core_node_to_ir_expr(end, param_names, refs, locals, used_local_names)?,
        ])),
        CoreNodeKind::Map {
            params,
            sources,
            body,
        } => {
            let mut nested_locals = locals.clone();
            let mut ir_params = Vec::new();
            for param in params {
                let ir_name = runtime_allocate_local_name(param, used_local_names);
                nested_locals.insert(param.clone(), ir_name.clone());
                ir_params.push(IrExpr::symbol(ir_name));
            }
            let mut items = vec![
                IrExpr::symbol("map"),
                IrExpr::list(vec![
                    IrExpr::symbol("lambda"),
                    IrExpr::list(ir_params),
                    runtime_core_node_to_ir_expr(
                        body,
                        param_names,
                        refs,
                        &nested_locals,
                        used_local_names,
                    )?,
                ]),
            ];
            for source in sources {
                items.push(runtime_core_node_to_ir_expr(
                    source,
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                )?);
            }
            Ok(IrExpr::list(items))
        }
        CoreNodeKind::Apply { op, args, list } => {
            let mut items = vec![
                IrExpr::symbol("apply"),
                IrExpr::symbol(runtime_core_operation_name(op)),
            ];
            for arg in args {
                items.push(runtime_core_node_to_ir_expr(
                    arg,
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                )?);
            }
            items.push(runtime_core_node_to_ir_expr(
                list,
                param_names,
                refs,
                locals,
                used_local_names,
            )?);
            Ok(IrExpr::list(items))
        }
        CoreNodeKind::List(items) => Ok(IrExpr::list(
            items
                .iter()
                .map(|item| {
                    runtime_core_node_to_ir_expr(item, param_names, refs, locals, used_local_names)
                })
                .collect::<AppResult<Vec<_>>>()?,
        )),
        CoreNodeKind::Group(items) => Ok(IrExpr::list(
            items
                .iter()
                .map(|item| {
                    runtime_core_node_to_ir_expr(item, param_names, refs, locals, used_local_names)
                })
                .collect::<AppResult<Vec<_>>>()?,
        )),
    }
}

fn runtime_allocate_local_name(name: &str, used: &mut BTreeMap<String, usize>) -> String {
    let mut base = name.trim_start_matches('#').trim().replace('#', "");
    if base.is_empty() {
        base = "value".to_string();
    }
    let mut normalized = String::with_capacity(base.len());
    for ch in base.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => normalized.push(ch),
            _ => normalized.push('_'),
        }
    }
    if normalized.is_empty() {
        normalized.push_str("value");
    }
    if normalized
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit())
    {
        normalized.insert_str(0, "v_");
    }

    let slot = used.entry(normalized.clone()).or_insert(0);
    *slot += 1;
    if *slot == 1 {
        normalized
    } else {
        format!("{}_{}", normalized, *slot)
    }
}

fn runtime_core_symbol_name(symbol: &CoreSymbol) -> &'static str {
    match symbol {
        CoreSymbol::Start => "start",
        CoreSymbol::End => "end",
        CoreSymbol::Xy => "xy",
        CoreSymbol::Yz => "yz",
        CoreSymbol::Xz => "xz",
        CoreSymbol::Min => "min",
        CoreSymbol::Center => "center",
        CoreSymbol::Max => "max",
    }
}

fn runtime_core_value_kind_tag(kind: CoreValueKind) -> &'static str {
    match kind {
        CoreValueKind::Any => "any",
        CoreValueKind::Number => "number",
        CoreValueKind::Boolean => "boolean",
        CoreValueKind::Text => "text",
        CoreValueKind::List => "list",
        CoreValueKind::Point2 => "point2",
        CoreValueKind::Point3 => "point3",
        CoreValueKind::Sketch => "sketch",
        CoreValueKind::Path => "path",
        CoreValueKind::Frame => "frame",
        CoreValueKind::Compound => "compound",
        CoreValueKind::Solid => "solid",
    }
}

fn runtime_core_operation_name(op: &CoreOperation) -> String {
    match op {
        CoreOperation::Primitive(CorePrimitive::Box) => "box".to_string(),
        CoreOperation::Primitive(CorePrimitive::Sphere) => "sphere".to_string(),
        CoreOperation::Primitive(CorePrimitive::Cylinder) => "cylinder".to_string(),
        CoreOperation::Primitive(CorePrimitive::Cone) => "cone".to_string(),
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

fn core_ast_identity(program: &CoreProgram) -> CoreAstIdentity {
    let mut canonical = program.clone();
    clear_core_program_spans(&mut canonical);

    let mut hasher = Sha256::new();
    hasher.update(b"ecky-core-ast");
    hasher.update(CORE_AST_SCHEMA_VERSION.to_string().as_bytes());
    hasher.update(format!("{canonical:#?}").as_bytes());

    CoreAstIdentity {
        core_digest: format!("sha256:{:x}", hasher.finalize()),
        ast_schema_version: CORE_AST_SCHEMA_VERSION,
    }
}

fn clear_core_program_spans(program: &mut CoreProgram) {
    for part in &mut program.parts {
        clear_core_node_spans(&mut part.root);
    }
}

fn clear_core_node_spans(node: &mut CoreNode) {
    node.span = None;
    match &mut node.kind {
        CoreNodeKind::Literal(_) | CoreNodeKind::Reference(_) => {}
        CoreNodeKind::Build { bindings, result } => {
            for binding in bindings {
                clear_core_node_spans(&mut binding.value);
            }
            clear_core_node_spans(result);
        }
        CoreNodeKind::Let { bindings, body } => {
            for binding in bindings {
                clear_core_node_spans(&mut binding.value);
            }
            clear_core_node_spans(body);
        }
        CoreNodeKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            clear_core_node_spans(condition);
            clear_core_node_spans(then_branch);
            clear_core_node_spans(else_branch);
        }
        CoreNodeKind::Call { args, keywords, .. } => {
            for arg in args {
                clear_core_node_spans(arg);
            }
            for keyword in keywords {
                clear_core_node_spans(keyword.source_node_mut());
            }
        }
        CoreNodeKind::Range { start, end } => {
            clear_core_node_spans(start);
            clear_core_node_spans(end);
        }
        CoreNodeKind::Map { sources, body, .. } => {
            for source in sources {
                clear_core_node_spans(source);
            }
            clear_core_node_spans(body);
        }
        CoreNodeKind::Apply { args, list, .. } => {
            for arg in args {
                clear_core_node_spans(arg);
            }
            clear_core_node_spans(list);
        }
        CoreNodeKind::List(items) | CoreNodeKind::Group(items) => {
            for item in items {
                clear_core_node_spans(item);
            }
        }
    }
}

fn cached_bundle_satisfies_manifest_identity(
    bundle: &ArtifactBundle,
    source_digest: &str,
    ast_identity: Option<&CoreAstIdentity>,
) -> bool {
    let Ok(raw) = fs::read_to_string(&bundle.manifest_path) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_str::<ModelManifest>(&raw) else {
        return false;
    };
    if manifest.source_digest.as_deref() != Some(source_digest) {
        return false;
    }
    if manifest.feature_graph.is_none() {
        return false;
    }

    match ast_identity {
        Some(identity) => {
            manifest.core_digest.as_deref() == Some(identity.core_digest.as_str())
                && manifest.ast_schema_version == Some(identity.ast_schema_version)
        }
        None => manifest.core_digest.is_none() && manifest.ast_schema_version.is_none(),
    }
}

fn render_prepared_parts(
    parts: &[RuntimePart],
    parameter_keys: &[String],
    source_identity: &str,
    parameters: &DesignParams,
    env: &BTreeMap<String, ParamValue>,
    app: &dyn PathResolver,
    ast_identity: Option<CoreAstIdentity>,
) -> AppResult<ArtifactBundle> {
    let part_ids = parts
        .iter()
        .map(|part| part.part_id.clone())
        .collect::<Vec<_>>();
    let params_json = serde_json::to_string(parameters).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(source_identity.as_bytes());
    hasher.update(b"|");
    hasher.update(params_json.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let mut source_hasher = Sha256::new();
    source_hasher.update(source_identity.as_bytes());
    let source_digest = format!("sha256:{:x}", source_hasher.finalize());
    let model_id = format!("generated-ir-{}", &hash[..12]);
    let dir = bundle_dir(app, &model_id)?;

    if let Some(cached) = load_cached_bundle(&dir)? {
        if cached_bundle_satisfies_manifest_identity(&cached, &source_digest, ast_identity.as_ref())
        {
            return Ok(cached);
        }
    }

    let core_digest = ast_identity
        .as_ref()
        .map(|identity| identity.core_digest.clone());
    let ast_schema_version = ast_identity
        .as_ref()
        .map(|identity| identity.ast_schema_version);

    let parts_dir = dir.join(PARTS_DIR_NAME);
    fs::create_dir_all(&parts_dir).map_err(|err| AppError::persistence(err.to_string()))?;

    let mut part_bindings = Vec::new();
    let mut viewer_assets = Vec::new();
    let mut preview_mesh: Option<IrMesh> = None;

    for (index, part) in parts.iter().enumerate() {
        let mesh = eval_geometry_expr(&part.expr, env)?.into_mesh("part")?;
        let part_path = parts_dir.join(format!("{}-{}.stl", index + 1, part.part_id));
        fs::write(
            &part_path,
            mesh.to_stl_binary(&part.part_id)
                .map_err(|err| AppError::persistence(format!("Failed to encode STL: {}", err)))?,
        )
        .map_err(|err| AppError::persistence(err.to_string()))?;

        preview_mesh = Some(match preview_mesh.take() {
            Some(existing) => existing.union(&mesh),
            None => mesh.clone(),
        });

        let asset_path = part_path.to_string_lossy().to_string();
        viewer_assets.push(ViewerAsset {
            part_id: part.part_id.clone(),
            node_id: part.part_id.clone(),
            object_name: part.part_id.clone(),
            label: part.label.clone(),
            path: asset_path.clone(),
            format: ViewerAssetFormat::Stl,
        });
        part_bindings.push(PartBinding {
            part_id: part.part_id.clone(),
            freecad_object_name: part.part_id.clone(),
            label: part.label.clone(),
            kind: "solid".to_string(),
            semantic_role: Some("generated".to_string()),
            viewer_asset_path: Some(asset_path),
            viewer_node_ids: vec![part.part_id.clone()],
            parameter_keys: parameter_keys.to_vec(),
            editable: true,
            bounds: Some(bounds_from_mesh(&mesh)),
            volume: mesh_volume(&mesh),
            area: mesh_area(&mesh),
        });
    }

    let preview_mesh =
        preview_mesh.ok_or_else(|| validation("`.ecky` model produced no printable parts."))?;
    let preview_path = dir.join(PREVIEW_STL_FILE_NAME);
    fs::write(
        &preview_path,
        preview_mesh.to_stl_binary("preview").map_err(|err| {
            AppError::persistence(format!("Failed to encode preview STL: {}", err))
        })?,
    )
    .map_err(|err| AppError::persistence(err.to_string()))?;

    let macro_path = dir.join(SOURCE_FILE_NAME);
    fs::write(&macro_path, source_identity.as_bytes())
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let selection_targets = Vec::new();
    let feature_graph = runtime_part_feature_graph(parts, &selection_targets);
    let manifest = ModelManifest {
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id: model_id.clone(),
        source_kind: ModelSourceKind::Generated,
        source_digest: Some(source_digest),
        core_digest,
        ast_schema_version,
        engine_kind: EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        document: DocumentMetadata {
            document_name: "Ecky".to_string(),
            document_label: "Ecky".to_string(),
            source_path: Some(macro_path.to_string_lossy().to_string()),
            object_count: part_bindings.len(),
            warnings: Vec::new(),
        },
        parts: part_bindings,
        parameter_groups: vec![ParameterGroup {
            group_id: "core".to_string(),
            label: "Core".to_string(),
            parameter_keys: parameter_keys.to_vec(),
            part_ids,
            editable: true,
            presentation: Some("primary".to_string()),
            order: Some(0),
        }],
        control_primitives: Vec::new(),
        control_relations: Vec::new(),
        control_views: Vec::new(),
        advisories: Vec::new(),
        selection_targets,
        measurement_annotations: Vec::new(),
        feature_graph: Some(feature_graph),
        correspondence_graph: None,
        warnings: Vec::new(),
        enrichment_state: crate::models::ManifestEnrichmentState {
            status: crate::models::EnrichmentStatus::None,
            proposals: Vec::new(),
        },
    };

    let manifest_path = dir.join(MANIFEST_FILE_NAME);
    write_manifest(&manifest_path, &manifest)?;

    let bundle = ArtifactBundle {
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id,
        source_kind: ModelSourceKind::Generated,
        engine_kind: EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        content_hash: hash,
        artifact_version: 1,
        fcstd_path: String::new(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        macro_path: Some(macro_path.to_string_lossy().to_string()),
        preview_stl_path: preview_path.to_string_lossy().to_string(),
        viewer_assets,
        edge_targets: Vec::new(),
        face_targets: Vec::new(),
        callout_anchors: Vec::new(),
        measurement_guides: Vec::new(),
        export_artifacts: Vec::new(),
    };
    write_bundle(&dir.join(BUNDLE_FILE_NAME), &bundle)?;
    Ok(bundle)
}

pub(crate) fn render_model_from_model(
    model: &IrModel,
    source_identity: &str,
    parameters: &DesignParams,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let env = build_param_env(model, parameters);
    let parameter_keys = model
        .params
        .iter()
        .map(|param| param.field.key().to_string())
        .collect::<Vec<_>>();
    let parts = model
        .parts
        .iter()
        .map(|part| RuntimePart {
            part_id: part.part_id.clone(),
            label: part.label.clone(),
            expr: part.expr.clone(),
            feature_decl: None,
            source_ref: runtime_part_source_ref(&part.part_id, None),
            dependency_ids: ir_expr_parameter_dependencies(&part.expr, &parameter_keys),
        })
        .collect::<Vec<_>>();
    render_prepared_parts(
        &parts,
        &parameter_keys,
        source_identity,
        parameters,
        &env,
        app,
        None,
    )
}

#[allow(dead_code)]
pub(crate) fn render_core_program(
    program: &CoreProgram,
    source_identity: &str,
    parameters: &DesignParams,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let param_names = program
        .parameters
        .iter()
        .map(|param| (param.id.raw(), param.key.clone()))
        .collect::<BTreeMap<_, _>>();
    let parts = program
        .parts
        .iter()
        .map(|part| runtime_core_part_to_runtime_part(part, &param_names, &program.feature_decls))
        .collect::<AppResult<Vec<_>>>()?;
    let parameter_keys = program
        .parameters
        .iter()
        .map(|param| param.key.clone())
        .collect::<Vec<_>>();
    let env = build_core_program_param_env_for_eval(program, parameters)?;
    render_prepared_parts(
        &parts,
        &parameter_keys,
        source_identity,
        parameters,
        &env,
        app,
        Some(core_ast_identity(program)),
    )
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::ecky_core_ir::{
        CoreNode, CoreNodeKind, CoreOperation, CoreParameter, CoreParameterConstraints,
        CoreParameterKind, CoreParameterValue, CorePart, CorePrimitive, CoreProgram, CoreValueKind,
        NodeId, ParamId, PartId, ProgramId, SourceFileId, SourceSpan,
    };
    use crate::ecky_ir::model::{core_part_to_ir_part, core_program_to_model, parse_model};
    use crate::models::ModelManifest;

    fn render_root() -> PathBuf {
        std::env::temp_dir().join(format!("ecky-ir-runtime-test-{}", uuid::Uuid::new_v4()))
    }

    fn example_fixture(name: &str) -> String {
        let path = format!(
            "{}/../model-runtime/examples/{}",
            env!("CARGO_MANIFEST_DIR"),
            name
        );
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
    }

    #[derive(Clone)]
    struct TestResolver {
        root: PathBuf,
    }

    impl crate::models::PathResolver for TestResolver {
        fn app_config_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn app_data_dir(&self) -> PathBuf {
            self.root.clone()
        }

        fn resource_path(&self, _path: &str) -> Option<PathBuf> {
            None
        }
    }

    fn read_manifest(bundle: &ArtifactBundle) -> ModelManifest {
        serde_json::from_str(
            &std::fs::read_to_string(&bundle.manifest_path).expect("read manifest file"),
        )
        .expect("parse manifest")
    }

    fn contains_edge_selector(expr: &IrExpr) -> bool {
        match expr {
            IrExpr::Selector(crate::ecky_ir::model::IrSelectorExpr::Edge(_)) => true,
            IrExpr::List(items) => items.iter().any(contains_edge_selector),
            _ => false,
        }
    }

    fn contains_face_selector(expr: &IrExpr) -> bool {
        match expr {
            IrExpr::Selector(crate::ecky_ir::model::IrSelectorExpr::Face(_)) => true,
            IrExpr::List(items) => items.iter().any(contains_face_selector),
            _ => false,
        }
    }

    #[test]
    fn render_model_from_model_renders_typed_build_expr() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };
        let source = r#"(model
            (part body
              (build
                (shape base (box 20 20 20))
                (shape cut (translate 0 0 10 (cylinder 4 12 24)))
                (result (difference base cut)))))"#;
        let model = parse_model(source).expect("model");

        let bundle = render_model_from_model(&model, source, &DesignParams::new(), &resolver)
            .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 1);
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }

    #[test]
    fn render_model_from_model_film_gap_coupon_fixture_has_stable_parts_and_export_readiness() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };
        let source = example_fixture("film-adapter-film-gap-coupon.ecky");
        let model = parse_model(&source).expect("model");

        let bundle = render_model_from_model(&model, &source, &DesignParams::new(), &resolver)
            .expect("render");
        let manifest = read_manifest(&bundle);
        let part_ids = manifest
            .parts
            .iter()
            .map(|part| part.part_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(bundle.viewer_assets.len(), 2);
        assert_eq!(manifest.document.object_count, 2);
        assert_eq!(manifest.parts.len(), 2);
        assert_eq!(part_ids, vec!["film_gate", "lens_adapter"]);
        assert!(bundle.export_artifacts.is_empty());
    }

    #[test]
    fn render_model_from_model_film_adapter_golden_closest_fixture_keeps_deterministic_count_and_step_readiness_signal(
    ) {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };
        let source = example_fixture("film-adapter-film-gap-coupon.ecky");
        let model = parse_model(&source).expect("model");
        let model_part_ids = model
            .parts
            .iter()
            .map(|part| part.part_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            model_part_ids,
            vec!["film_gate", "lens_adapter"],
            "film adapter golden closest fixture has deterministic runtime part ids/count=2 (not trench-doc 6 for integrated helicoid path)"
        );

        let bundle = render_model_from_model(&model, &source, &DesignParams::new(), &resolver)
            .expect("render");
        let manifest = read_manifest(&bundle);
        let manifest_part_ids = manifest
            .parts
            .iter()
            .map(|part| part.part_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            manifest_part_ids,
            vec!["film_gate", "lens_adapter"],
            "manifest part ids must stay deterministic for film adapter golden closest runtime render"
        );
        assert_eq!(
            manifest.parts.len(),
            2,
            "manifest deterministic part count for film adapter golden closest fixture is 2 on this backend path"
        );
        assert_eq!(
            manifest.document.object_count, 2,
            "document object count stays aligned with deterministic manifest part count"
        );
        assert!(bundle.export_artifacts.is_empty());
        assert!(
            matches!(
                manifest.enrichment_state.status,
                crate::contracts::EnrichmentStatus::None
            ),
            "manifest enrichment state stays none on EckyRust backend path (STEP export not materialized)"
        );
    }

    #[test]
    fn render_model_from_model_film_path_gap_coupon_fixture_has_stable_parts_and_count() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };
        let source = example_fixture("film-path-gap-coupon.ecky");
        let model = parse_model(&source).expect("model");

        let model_part_ids = model
            .parts
            .iter()
            .map(|part| part.part_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            model_part_ids,
            vec![
                "film_path_lower_035",
                "film_path_upper_clamp_035",
                "film_path_lower_045",
                "film_path_upper_clamp_045",
                "film_path_lower_055",
                "film_path_upper_clamp_055"
            ]
        );

        let bundle = render_model_from_model(&model, &source, &DesignParams::new(), &resolver)
            .expect("render");
        let manifest = read_manifest(&bundle);
        let manifest_part_ids = manifest
            .parts
            .iter()
            .map(|part| part.part_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(bundle.viewer_assets.len(), 6);
        assert_eq!(manifest.document.object_count, 6);
        assert_eq!(manifest.parts.len(), 6);
        assert_eq!(
            manifest_part_ids,
            vec![
                "film_path_lower_035",
                "film_path_upper_clamp_035",
                "film_path_lower_045",
                "film_path_upper_clamp_045",
                "film_path_lower_055",
                "film_path_upper_clamp_055"
            ]
        );
        assert!(bundle.export_artifacts.is_empty());
    }

    #[test]
    fn render_model_from_model_helicoid_thread_coupon_fixture_keeps_clearance_variants() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };
        let source = example_fixture("helicoid-thread-coupon.ecky");
        let model = parse_model(&source).expect("model");
        let part_ids = model
            .parts
            .iter()
            .map(|part| part.part_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            part_ids,
            vec![
                "coupon_male_020",
                "coupon_female_020",
                "coupon_male_025",
                "coupon_female_025",
                "coupon_male_030",
                "coupon_female_030",
                "coupon_male_035",
                "coupon_female_035"
            ]
        );
        let err = render_model_from_model(&model, &source, &DesignParams::new(), &resolver)
            .expect_err("ecky runtime should reject helical-ridge");
        assert!(
            err.message
                .contains("Unsupported on current geometry backend"),
            "{err:?}"
        );
        assert!(
            err.details
                .as_deref()
                .unwrap_or_default()
                .contains("helical-ridge"),
            "{err:?}"
        );
    }

    #[test]
    fn render_model_from_model_magnet_clamp_coupon_fixture_has_stable_parts_and_count() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };
        let source = example_fixture("magnet-clamp-coupon.ecky");
        let model = parse_model(&source).expect("model");

        let model_part_ids = model
            .parts
            .iter()
            .map(|part| part.part_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            model_part_ids,
            vec![
                "magnet_clamp_base_n",
                "magnet_clamp_base_s",
                "magnet_polarity_mask_n",
                "magnet_polarity_mask_s"
            ]
        );

        let bundle = render_model_from_model(&model, &source, &DesignParams::new(), &resolver)
            .expect("render");
        let manifest = read_manifest(&bundle);
        let manifest_part_ids = manifest
            .parts
            .iter()
            .map(|part| part.part_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(bundle.viewer_assets.len(), 4);
        assert_eq!(manifest.document.object_count, 4);
        assert_eq!(manifest.parts.len(), 4);
        assert_eq!(
            manifest_part_ids,
            vec![
                "magnet_clamp_base_n",
                "magnet_clamp_base_s",
                "magnet_polarity_mask_n",
                "magnet_polarity_mask_s"
            ]
        );
        assert!(bundle.export_artifacts.is_empty());
    }

    #[test]
    fn render_core_program_matches_public_render_entrypoint() {
        let source = r#"
            (define base-radius 14)
            (model
              (params
                (number radius base-radius :label "Radius")
                (toggle vents true :label "Vents"))
              (part body
                (difference
                  (extrude (circle radius) 20)
                  (translate 0 0 2 (extrude (circle (- radius 2)) 18)))))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let mut params = DesignParams::new();
        params.insert("radius".into(), ParamValue::Number(16.0));

        let direct_root = render_root();
        std::fs::create_dir_all(&direct_root).expect("direct root");
        let direct = render_core_program(
            &program,
            source,
            &params,
            &TestResolver { root: direct_root },
        )
        .expect("direct render");

        let public_root = render_root();
        std::fs::create_dir_all(&public_root).expect("public root");
        let public =
            crate::ecky_ir::render_model(source, &params, &TestResolver { root: public_root })
                .expect("public render");

        let direct_manifest = read_manifest(&direct);
        let public_manifest = read_manifest(&public);

        assert_eq!(direct.content_hash, public.content_hash);
        assert_eq!(direct.viewer_assets.len(), public.viewer_assets.len());
        assert_eq!(
            direct_manifest.parameter_groups,
            public_manifest.parameter_groups
        );
        assert_eq!(direct_manifest.parts.len(), public_manifest.parts.len());
        assert_eq!(
            direct_manifest.parts[0].bounds,
            public_manifest.parts[0].bounds
        );
        assert_eq!(
            direct_manifest.parts[0].volume,
            public_manifest.parts[0].volume
        );
        assert_eq!(direct_manifest.parts[0].area, public_manifest.parts[0].area);
    }

    #[test]
    fn render_core_program_manifest_includes_ast_identity() {
        let source = r#"
            (model
              (params
                (number width 10 :label "Width"))
              (part body (box width 8 6)))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let bundle = render_core_program(
            &program,
            source,
            &DesignParams::new(),
            &TestResolver { root },
        )
        .expect("render");

        let manifest = read_manifest(&bundle);

        assert!(manifest.source_digest.is_some());
        assert!(manifest.core_digest.is_some());
        assert_eq!(manifest.ast_schema_version, Some(1));
    }

    #[test]
    fn render_core_program_manifest_includes_part_feature_graph_provenance() {
        let source = r#"
            (model
              (params
                (number width 10 :label "Width"))
              (part body (box width 8 6)))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let bundle = render_core_program(
            &program,
            source,
            &DesignParams::new(),
            &TestResolver { root },
        )
        .expect("render");

        let manifest = read_manifest(&bundle);
        let feature_graph = manifest.feature_graph.expect("feature graph");

        assert_eq!(feature_graph.nodes.len(), 1);
        assert_eq!(feature_graph.nodes[0].feature_id, "part:body");
        assert_eq!(feature_graph.nodes[0].kind, "part");
        assert_eq!(feature_graph.nodes[0].label, "Body");
        assert_eq!(
            feature_graph.nodes[0]
                .source_ref
                .as_ref()
                .and_then(|source_ref| source_ref.path.as_deref()),
            Some("/parts/body/root")
        );
        assert_eq!(feature_graph.nodes[0].dependency_ids, vec!["width"]);
    }

    #[test]
    fn render_core_program_manifest_uses_feature_metadata_for_feature_graph_nodes() {
        let source = r#"
            (model
              (params
                (number width 10 :label "Width"))
              (feature shell-cutout :role subtraction :params (width gap) (box width 8 6)))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let bundle = render_core_program(
            &program,
            source,
            &DesignParams::new(),
            &TestResolver { root },
        )
        .expect("render");

        let manifest = read_manifest(&bundle);
        let feature_graph = manifest.feature_graph.expect("feature graph");

        assert_eq!(feature_graph.nodes.len(), 1);
        assert_eq!(feature_graph.nodes[0].feature_id, "shell-cutout");
        assert_eq!(feature_graph.nodes[0].kind, "subtraction");
        assert_eq!(feature_graph.nodes[0].dependency_ids, vec!["width", "gap"]);
    }

    #[test]
    fn runtime_part_feature_graph_links_selection_target_outputs() {
        let parts = vec![RuntimePart {
            part_id: "body".to_string(),
            label: "Body".to_string(),
            expr: IrExpr::symbol("body"),
            feature_decl: None,
            source_ref: runtime_part_source_ref("body", None),
            dependency_ids: vec!["width".to_string()],
        }];
        let selection_targets = vec![crate::models::SelectionTarget {
            target_id: Some("target-body".to_string()),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "body".to_string(),
            label: "Body".to_string(),
            kind: crate::models::SelectionTargetKind::Object,
            editable: true,
            parameter_keys: vec!["width".to_string()],
            primitive_ids: Vec::new(),
            view_ids: Vec::new(),
        }];

        let graph = runtime_part_feature_graph(&parts, &selection_targets);

        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].output_refs.len(), 1);
        assert_eq!(graph.nodes[0].output_refs[0].feature_id, "part:body");
        assert_eq!(
            graph.nodes[0].output_refs[0].target_ids,
            vec!["target-body"]
        );
    }

    #[test]
    fn core_ast_identity_is_deterministic_and_ignores_spans() {
        let source = r#"(model (part body (box 1 2 3)))"#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let repeated = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let changed =
            crate::ecky_scheme::compile_to_core_program(r#"(model (part body (box 1 2 4)))"#)
                .expect("changed program");
        let mut with_span = program.clone();
        with_span.parts[0].root.span = Some(SourceSpan::new(Some(SourceFileId::new(9)), 12, 34));

        let identity = core_ast_identity(&program);

        assert_eq!(
            identity.core_digest,
            core_ast_identity(&repeated).core_digest
        );
        assert_eq!(
            identity.core_digest,
            core_ast_identity(&with_span).core_digest
        );
        assert_ne!(
            identity.core_digest,
            core_ast_identity(&changed).core_digest
        );
    }

    #[test]
    fn render_model_from_model_manifest_keeps_ast_identity_empty() {
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };
        let source = r#"(model (part body (box 10 8 6)))"#;
        let model = parse_model(source).expect("model");

        let bundle = render_model_from_model(&model, source, &DesignParams::new(), &resolver)
            .expect("render");
        let manifest = read_manifest(&bundle);

        assert!(manifest.source_digest.is_some());
        assert_eq!(manifest.core_digest, None);
        assert_eq!(manifest.ast_schema_version, None);
        assert_eq!(
            manifest
                .feature_graph
                .as_ref()
                .and_then(|graph| graph.nodes.first())
                .and_then(|node| node.source_ref.as_ref())
                .and_then(|source_ref| source_ref.path.as_deref()),
            Some("/parts/body/root")
        );
    }

    #[test]
    fn render_core_program_builds_param_env_from_core_program_defaults_and_overrides() {
        let source = r#"
            (model
              (params
                (number width 10 :label "Width"))
              (part body (box width 10 10)))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let resolver = TestResolver { root };

        let default_bundle = render_core_program(&program, source, &DesignParams::new(), &resolver)
            .expect("default render");
        let default_manifest = read_manifest(&default_bundle);
        let default_volume = default_manifest.parts[0].volume.expect("default volume");

        let mut override_params = DesignParams::new();
        override_params.insert("width".into(), ParamValue::Number(20.0));
        let override_bundle = render_core_program(&program, source, &override_params, &resolver)
            .expect("override render");
        let override_manifest = read_manifest(&override_bundle);
        let override_volume = override_manifest.parts[0].volume.expect("override volume");

        assert!(
            (default_volume - 1000.0).abs() < 1e-6,
            "default volume {default_volume}"
        );
        assert!(
            (override_volume - 2000.0).abs() < 1e-6,
            "override volume {override_volume}"
        );
        assert_eq!(
            override_manifest.parameter_groups[0].parameter_keys,
            vec!["width".to_string()]
        );
    }

    #[test]
    fn render_core_program_bypasses_full_model_bridge_for_text_params() {
        fn num(id: u64, value: f64) -> CoreNode {
            CoreNode::new(
                NodeId::new(id),
                CoreNodeKind::Literal(crate::ecky_core_ir::CoreLiteral::Number(value)),
                CoreValueKind::Number,
            )
        }

        let source = "(model (params (text-param label \"hello\" :label \"Label\")) (part body (box 10 10 10)))";
        let program = CoreProgram::new(
            ProgramId::new(1),
            vec![CoreParameter {
                id: ParamId::new(2),
                key: "label".into(),
                label: "Label".into(),
                kind: CoreParameterKind::Text,
                default_value: CoreParameterValue::Text("hello".into()),
                frozen: false,
                constraints: CoreParameterConstraints::default(),
            }],
            vec![CorePart {
                id: PartId::new(3),
                key: "body".into(),
                label: "Body".into(),
                root: CoreNode::new(
                    NodeId::new(4),
                    CoreNodeKind::Call {
                        op: CoreOperation::Primitive(CorePrimitive::Box),
                        args: vec![num(5, 10.0), num(6, 10.0), num(7, 10.0)],
                        keywords: vec![],
                    },
                    CoreValueKind::Solid,
                ),
            }],
        );

        let bridge_err = match core_program_to_model(&program) {
            Ok(_) => panic!("legacy bridge should fail"),
            Err(err) => err,
        };
        assert!(
            bridge_err
                .details
                .as_deref()
                .unwrap_or("")
                .contains("Text params are not yet supported by the legacy IR bridge."),
            "unexpected bridge error: {}",
            bridge_err.message
        );

        let root = render_root();
        std::fs::create_dir_all(&root).expect("root");
        let bundle = render_core_program(
            &program,
            source,
            &DesignParams::new(),
            &TestResolver { root },
        )
        .expect("direct render");

        assert_eq!(bundle.viewer_assets.len(), 1);
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }

    #[test]
    fn runtime_core_part_conversion_matches_legacy_bridge() {
        let source = r#"
            (define base-radius 14)
            (model
              (params
                (number radius base-radius :label "Radius")
                (toggle vents true :label "Vents"))
              (part body
                (build
                  (shape outer (extrude (circle radius) 20))
                  (shape inner (translate 0 0 2 (extrude (circle (- radius 2)) 18)))
                  (result
                    (if vents
                      (difference outer inner)
                      outer)))))
        "#;
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let param_names = program
            .parameters
            .iter()
            .map(|param| (param.id.raw(), param.key.clone()))
            .collect::<BTreeMap<_, _>>();

        let legacy = core_part_to_ir_part(&program.parts[0], &param_names).expect("legacy");
        let runtime = runtime_core_part_to_runtime_part(
            &program.parts[0],
            &param_names,
            &program.feature_decls,
        )
        .expect("runtime");

        assert_eq!(runtime.part_id, legacy.part_id);
        assert_eq!(runtime.label, legacy.label);
        assert_eq!(runtime.expr, legacy.expr);
    }

    #[test]
    fn runtime_core_part_conversion_materializes_selector_nodes() {
        let source =
            "(model (part body (fillet 1 :edges \"target-id:body:edge:0:0-0-0_1-0-0\" (box 1 1 1))))";
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let param_names = program
            .parameters
            .iter()
            .map(|param| (param.id.raw(), param.key.clone()))
            .collect::<BTreeMap<_, _>>();

        let runtime = runtime_core_part_to_runtime_part(
            &program.parts[0],
            &param_names,
            &program.feature_decls,
        )
        .expect("runtime");
        assert!(
            contains_edge_selector(&runtime.expr),
            "expected typed selector in {:?}",
            runtime.expr
        );
    }

    #[test]
    fn runtime_core_part_conversion_materializes_face_selector_nodes() {
        let source =
            "(model (part body (shell 1 :faces \"target-id:body:face:0:0-0-1:1\" (box 1 1 1))))";
        let program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let param_names = program
            .parameters
            .iter()
            .map(|param| (param.id.raw(), param.key.clone()))
            .collect::<BTreeMap<_, _>>();

        let runtime = runtime_core_part_to_runtime_part(
            &program.parts[0],
            &param_names,
            &program.feature_decls,
        )
        .expect("runtime");
        assert!(
            contains_face_selector(&runtime.expr),
            "expected typed face selector in {:?}",
            runtime.expr
        );
    }

    #[test]
    fn runtime_core_part_conversion_rejects_missing_selector_payload_on_edges_keyword() {
        let source = "(model (part body (fillet 1 :edges \"left+vertical\" (box 1 1 1))))";
        let mut program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let crate::ecky_core_ir::CoreNodeKind::Call { keywords, .. } =
            &mut program.parts[0].root.kind
        else {
            panic!("expected call");
        };
        keywords[0].set_selector_payload(None);
        let param_names = program
            .parameters
            .iter()
            .map(|param| (param.id.raw(), param.key.clone()))
            .collect::<BTreeMap<_, _>>();

        let err = match runtime_core_part_to_runtime_part(
            &program.parts[0],
            &param_names,
            &program.feature_decls,
        ) {
            Ok(_) => panic!("missing selector payload should fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("CoreProgram `:edges` keyword requires selector payload"),
            "{err}"
        );
    }

    #[test]
    fn runtime_core_part_conversion_rejects_wrong_kind_selector_payload_on_edges_keyword() {
        let source = "(model (part body (fillet 1 :edges \"left+vertical\" (box 1 1 1))))";
        let mut program = crate::ecky_scheme::compile_to_core_program(source).expect("program");
        let crate::ecky_core_ir::CoreNodeKind::Call { keywords, .. } =
            &mut program.parts[0].root.kind
        else {
            panic!("expected call");
        };
        keywords[0].set_selector_payload(Some(
            crate::ecky_core_ir::CoreSelectorPayload::FaceTargetIds(vec![
                "body:face:0:0-0-1:1".into()
            ]),
        ));
        let param_names = program
            .parameters
            .iter()
            .map(|param| (param.id.raw(), param.key.clone()))
            .collect::<BTreeMap<_, _>>();

        let err = match runtime_core_part_to_runtime_part(
            &program.parts[0],
            &param_names,
            &program.feature_decls,
        ) {
            Ok(_) => panic!("wrong-kind selector payload should fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("CoreProgram `:edges` keyword requires edge selector payload"),
            "{err}"
        );
    }

    #[test]
    fn ecky_lowering_failure_exposes_operation_stable_node_key_and_line_range() {
        let source = r#"(model (part body (wall-pattern (:mode ribs :depth 1) (shell 2 (cylinder 10 20)))))"#;
        let err = crate::ecky_ir::lower_to_build123d(source)
            .expect_err("wall-pattern should fail on build123d")
            .with_operation("lower:build123d")
            .with_line_range(1, 1)
            .with_stable_node_key("sha256:test-lowering-span");

        assert_eq!(err.operation.as_deref(), Some("lower:build123d"));
        assert_eq!(
            err.stable_node_key.as_deref(),
            Some("sha256:test-lowering-span")
        );
        assert_eq!(err.start_line, Some(1));
        assert_eq!(err.end_line, Some(1));
        assert!(err.start_line.unwrap() <= err.end_line.unwrap(), "{err:?}");
    }
}
