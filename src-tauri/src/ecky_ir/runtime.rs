use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use csgrs::float_types::parry3d::na::Vector3;
use csgrs::traits::CSG;
use sha2::{Digest, Sha256};

use crate::models::{
    AppError, AppResult, ArtifactBundle, DesignParams, DocumentMetadata, EngineKind,
    GeometryBackend, ManifestBounds, ModelManifest, ModelSourceKind, ParamValue, ParameterGroup,
    ParsedParamsResult, PartBinding, PathResolver, SourceLanguage, ViewerAsset, ViewerAssetFormat,
    MODEL_RUNTIME_SCHEMA_VERSION,
};

use super::mesh_ops::eval_geometry_expr;
use super::model::{
    build_param_env, core_program_param_defaults, parse_model, parsed_params_from_core_program,
    parsed_params_from_model, IrExpr, IrModel,
};
use super::shared::{unsupported, validation, IrMesh};
use super::syntax::canonicalize;
use crate::ecky_core_ir::{
    CoreArrayOp, CoreBooleanOp, CoreFrameOp, CoreKeywordArg, CoreLiteral, CoreMetaOp, CoreNode,
    CoreNodeKind, CoreOperation, CorePart, CorePathOp, CorePrimitive, CoreProgram, CoreReference,
    CoreSurfaceOp, CoreSymbol, CoreTransformOp, CoreValueKind,
};

pub(super) const MODEL_RUNTIME_ROOT: &str = "model-runtime";
pub(super) const GENERATED_ARTIFACT_DIR: &str = "generated";
pub(super) const BUNDLE_FILE_NAME: &str = "bundle.json";
pub(super) const MANIFEST_FILE_NAME: &str = "manifest.json";
pub(super) const SOURCE_FILE_NAME: &str = "source.ecky";
pub(super) const PREVIEW_STL_FILE_NAME: &str = "preview.stl";
pub(super) const PARTS_DIR_NAME: &str = "parts";
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
}

#[allow(dead_code)]
fn build_core_program_param_env(
    program: &CoreProgram,
    parameters: &DesignParams,
) -> AppResult<BTreeMap<String, ParamValue>> {
    let mut env = core_program_param_defaults(program)?;
    for (key, value) in parameters {
        env.insert(key.clone(), value.clone());
    }
    Ok(env)
}

fn runtime_core_part_to_runtime_part(
    part: &CorePart,
    param_names: &BTreeMap<u64, String>,
) -> AppResult<RuntimePart> {
    let mut used_local_names = BTreeMap::new();
    Ok(RuntimePart {
        part_id: part.key.clone(),
        label: part.label.clone(),
        expr: runtime_core_node_to_ir_expr(
            &part.root,
            param_names,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &mut used_local_names,
        )?,
    })
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
            for CoreKeywordArg { name, value } in keywords {
                items.push(IrExpr::keyword(name.clone()));
                items.push(runtime_core_node_to_ir_expr(
                    value,
                    param_names,
                    refs,
                    locals,
                    used_local_names,
                )?);
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
        CoreOperation::Surface(CoreSurfaceOp::Fillet) => "fillet".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Chamfer) => "chamfer".to_string(),
        CoreOperation::Surface(CoreSurfaceOp::Twist) => "twist".to_string(),
        CoreOperation::Path(CorePathOp::Polyline) => "path".to_string(),
        CoreOperation::Path(CorePathOp::BezierPath) => "bezier-path".to_string(),
        CoreOperation::Path(CorePathOp::Bspline) => "bspline".to_string(),
        CoreOperation::Array(CoreArrayOp::LinearArray) => "linear-array".to_string(),
        CoreOperation::Array(CoreArrayOp::RadialArray) => "radial-array".to_string(),
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

fn render_prepared_parts(
    parts: &[RuntimePart],
    parameter_keys: &[String],
    source_identity: &str,
    parameters: &DesignParams,
    env: &BTreeMap<String, ParamValue>,
    app: &dyn PathResolver,
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
    let model_id = format!("generated-ir-{}", &hash[..12]);
    let dir = bundle_dir(app, &model_id)?;

    if let Some(cached) = load_cached_bundle(&dir)? {
        return Ok(cached);
    }

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

    let manifest = ModelManifest {
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id: model_id.clone(),
        source_kind: ModelSourceKind::Generated,
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
        selection_targets: Vec::new(),
        measurement_annotations: Vec::new(),
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
        })
        .collect::<Vec<_>>();
    render_prepared_parts(
        &parts,
        &parameter_keys,
        source_identity,
        parameters,
        &env,
        app,
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
        .map(|part| runtime_core_part_to_runtime_part(part, &param_names))
        .collect::<AppResult<Vec<_>>>()?;
    let parameter_keys = program
        .parameters
        .iter()
        .map(|param| param.key.clone())
        .collect::<Vec<_>>();
    let env = build_core_program_param_env(program, parameters)?;
    render_prepared_parts(
        &parts,
        &parameter_keys,
        source_identity,
        parameters,
        &env,
        app,
    )
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::ecky_core_ir::{
        CoreNode, CoreNodeKind, CoreOperation, CoreParameter, CoreParameterConstraints,
        CoreParameterKind, CoreParameterValue, CorePart, CorePrimitive, CoreProgram, CoreValueKind,
        NodeId, ParamId, PartId, ProgramId,
    };
    use crate::ecky_ir::model::{core_part_to_ir_part, core_program_to_model, parse_model};
    use crate::models::ModelManifest;

    fn render_root() -> PathBuf {
        std::env::temp_dir().join(format!("ecky-ir-runtime-test-{}", uuid::Uuid::new_v4()))
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
        let runtime =
            runtime_core_part_to_runtime_part(&program.parts[0], &param_names).expect("runtime");

        assert_eq!(runtime.part_id, legacy.part_id);
        assert_eq!(runtime.label, legacy.label);
        assert_eq!(runtime.expr, legacy.expr);
    }
}
