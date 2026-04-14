use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use csgrs::float_types::parry3d::na::{self, Point3, Vector3};
use csgrs::mesh::plane::Plane as IrPlane;
use csgrs::mesh::polygon::Polygon as IrPolygon;
use csgrs::mesh::vertex::Vertex as IrVertex;
use csgrs::mesh::Mesh;
use csgrs::sketch::Sketch;
use csgrs::traits::CSG;
use geo::algorithm::contains::Contains;
use geo::algorithm::orient::{Direction, Orient};
use geo::{
    Coord, Geometry as GeoGeometry, GeometryCollection, LineString, MultiPolygon,
    Polygon as GeoPolygon,
};
use lexpr::parse::{KeywordSyntax, Options};
use lexpr::Value;
use sha2::{Digest, Sha256};

use crate::ecky_ir_patterns::{
    apply_wall_pattern, ContourSweepSlice, WallPatternMode, WallPatternSpec, WallPatternTarget,
};
use crate::models::{
    AppError, AppResult, ArtifactBundle, DesignParams, DocumentMetadata, EngineKind,
    GeometryBackend, ManifestBounds, ModelManifest, ModelSourceKind, ParamValue, ParameterGroup,
    ParsedParamsResult, PartBinding, PathResolver, SelectOption, SelectValue, SourceLanguage,
    UiField, ViewerAsset, ViewerAssetFormat, MODEL_RUNTIME_SCHEMA_VERSION,
};

const MODEL_RUNTIME_ROOT: &str = "model-runtime";
const GENERATED_ARTIFACT_DIR: &str = "generated";
const BUNDLE_FILE_NAME: &str = "bundle.json";
const MANIFEST_FILE_NAME: &str = "manifest.json";
const SOURCE_FILE_NAME: &str = "source.eckyir";
const PREVIEW_STL_FILE_NAME: &str = "preview.stl";
const PARTS_DIR_NAME: &str = "parts";

type IrMesh = Mesh<()>;
type IrSketch = Sketch<()>;

type LoopPoints = Vec<[f64; 2]>;

#[derive(Clone, Debug)]
struct ContourPolygon2d {
    outer: LoopPoints,
    holes: Vec<LoopPoints>,
}

#[derive(Clone, Debug)]
struct ContourSet2d {
    polygons: Vec<ContourPolygon2d>,
}

#[derive(Clone, Debug)]
struct SketchContours {
    outer_loops: Vec<LoopPoints>,
    hole_loops: Vec<LoopPoints>,
}

#[derive(Debug, Clone)]
struct IrModel {
    params: Vec<IrParam>,
    parts: Vec<IrPart>,
}

#[derive(Debug, Clone)]
struct IrParam {
    field: UiField,
    default_value: ParamValue,
}

#[derive(Debug, Clone)]
struct IrPart {
    part_id: String,
    label: String,
    expr: Value,
}

enum Geometry {
    Mesh(IrMesh),
    Sketch(IrSketch),
    Path(Vec<([f64; 3], [f64; 3])>),
}

impl Geometry {
    fn into_mesh(self, node: &str) -> AppResult<IrMesh> {
        match self {
            Self::Mesh(mesh) => Ok(mesh),
            _ => Err(unsupported(format!(
                "Node `{}` expected a 3D solid but received {}.",
                node,
                self.kind_str()
            ))),
        }
    }

    fn into_sketch(self, node: &str) -> AppResult<IrSketch> {
        match self {
            Self::Sketch(sketch) => Ok(sketch),
            _ => Err(unsupported(format!(
                "Node `{}` expected a 2D sketch but received {}.",
                node,
                self.kind_str()
            ))),
        }
    }

    fn into_path(self, node: &str) -> AppResult<Vec<([f64; 3], [f64; 3])>> {
        match self {
            Self::Path(path) => Ok(path),
            _ => Err(unsupported(format!(
                "Node `{}` expected a 3D path but received {}.",
                node,
                self.kind_str()
            ))),
        }
    }

    fn kind_str(&self) -> &'static str {
        match self {
            Self::Mesh(_) => "a 3D solid",
            Self::Sketch(_) => "a 2D sketch",
            Self::Path(_) => "a 3D path",
        }
    }
}

fn unsupported(details: impl Into<String>) -> AppError {
    AppError::with_details(
        crate::models::AppErrorCode::Validation,
        "Unsupported by Ecky IR v0. Switch the thread engine to FreeCAD and rerender.",
        details.into(),
    )
}

fn validation(message: impl Into<String>) -> AppError {
    AppError::validation(message.into())
}

fn list_items(value: &Value, context: &str) -> AppResult<Vec<Value>> {
    value
        .to_vec()
        .ok_or_else(|| validation(format!("Expected a proper list for {}.", context)))
}

fn head_symbol<'a>(items: &'a [Value], context: &str) -> AppResult<&'a str> {
    items
        .first()
        .and_then(Value::as_symbol)
        .ok_or_else(|| validation(format!("Expected a symbolic head for {}.", context)))
}

fn keyword_name(value: &Value) -> Option<&str> {
    value.as_keyword().or_else(|| {
        value
            .as_symbol()
            .and_then(|symbol| symbol.strip_prefix(':'))
    })
}

fn parse_number_value(value: &Value, context: &str) -> AppResult<f64> {
    value
        .as_f64()
        .ok_or_else(|| validation(format!("Expected a number for {}.", context)))
}

fn parse_stringish(value: &Value, context: &str) -> AppResult<String> {
    if let Some(text) = value.as_str() {
        return Ok(text.to_string());
    }
    if let Some(symbol) = value.as_symbol() {
        return Ok(symbol.to_string());
    }
    Err(validation(format!("Expected text for {}.", context)))
}

/// Strip `;` line comments, respecting string literals.
fn strip_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| {
            let mut in_string = false;
            let mut cut = line.len();
            let mut chars = line.char_indices().peekable();
            while let Some((i, ch)) = chars.next() {
                match ch {
                    '\\' if in_string => {
                        chars.next();
                    }
                    '"' => in_string = !in_string,
                    ';' if !in_string => {
                        cut = i;
                        break;
                    }
                    _ => {}
                }
            }
            line[..cut].trim_end()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn ir_options() -> Options {
    Options::new().with_keyword_syntax(KeywordSyntax::ColonPrefix)
}

fn ir_parse(source: &str) -> AppResult<Value> {
    lexpr::parse::Parser::from_str_custom(&strip_comments(source), ir_options())
        .expect_value()
        .map_err(|err| validation(format!("Failed to parse Ecky IR v0: {}", err)))
}

fn canonicalize(source: &str) -> AppResult<String> {
    let value = ir_parse(source)?;
    lexpr::to_string(&value)
        .map_err(|err| validation(format!("Failed to canonicalize IR: {}", err)))
}

fn parse_model(source: &str) -> AppResult<IrModel> {
    let value = ir_parse(source)?;
    let forms = list_items(&value, "model")?;
    if head_symbol(&forms, "model root")? != "model" {
        return Err(validation(
            "Ecky IR v0 source must start with `(model ...)`.",
        ));
    }

    let mut params = Vec::new();
    let mut parts = Vec::new();
    for form in forms.into_iter().skip(1) {
        let items = list_items(&form, "model form")?;
        match head_symbol(&items, "model form")? {
            "params" => {
                for decl in items.into_iter().skip(1) {
                    params.push(parse_param_decl(&decl)?);
                }
            }
            "part" => parts.push(parse_part_decl(&items)?),
            "meta" => {}
            other => {
                return Err(unsupported(format!(
                    "Top-level node `{}` is not supported by Ecky IR v0.",
                    other
                )))
            }
        }
    }

    if parts.is_empty() {
        return Err(validation(
            "Ecky IR v0 models need at least one `(part ...)` node.",
        ));
    }

    Ok(IrModel { params, parts })
}

fn parse_param_decl(value: &Value) -> AppResult<IrParam> {
    let items = list_items(value, "param declaration")?;
    let kind = head_symbol(&items, "param declaration")?;
    if items.len() < 3 {
        return Err(validation(format!(
            "Param declaration `{}` needs at least a name and default value.",
            kind
        )));
    }
    let key = parse_stringish(&items[1], "param key")?;
    let mut label = humanize_key(&key);
    let mut min = None;
    let mut max = None;
    let mut step = None;
    let mut options = Vec::new();
    let mut frozen = false;
    let default_atom = &items[2];
    let mut index = 3usize;
    while index + 1 < items.len() {
        let Some(name) = keyword_name(&items[index]) else {
            break;
        };
        let option_value = &items[index + 1];
        match name {
            "label" => label = parse_stringish(option_value, "param label")?,
            "min" => min = Some(parse_number_value(option_value, "param min")?),
            "max" => max = Some(parse_number_value(option_value, "param max")?),
            "step" => step = Some(parse_number_value(option_value, "param step")?),
            "frozen" => frozen = option_value.as_bool().unwrap_or(false),
            "options" => {
                let option_forms = list_items(option_value, "select options")?;
                options = option_forms
                    .iter()
                    .map(parse_select_option)
                    .collect::<AppResult<Vec<_>>>()?;
            }
            _ => {}
        }
        index += 2;
    }

    let (field, default_value) = match kind {
        "number" => (
            UiField::Number {
                key,
                label,
                min,
                max,
                step,
                min_from: None,
                max_from: None,
                frozen,
            },
            ParamValue::Number(parse_number_value(default_atom, "number default")?),
        ),
        "select" => {
            if options.is_empty() {
                return Err(validation("Select params require `:options` entries."));
            }
            let default_value = if let Some(text) = default_atom.as_str() {
                ParamValue::String(text.to_string())
            } else if let Some(number) = default_atom.as_f64() {
                ParamValue::Number(number)
            } else {
                return Err(validation("Select defaults must be string or number."));
            };
            (
                UiField::Select {
                    key,
                    label,
                    options,
                    frozen,
                },
                default_value,
            )
        }
        "toggle" => (
            UiField::Checkbox { key, label, frozen },
            ParamValue::Boolean(default_atom.as_bool().unwrap_or(false)),
        ),
        "image" => (
            UiField::Image { key, label, frozen },
            ParamValue::String(default_atom.as_str().unwrap_or_default().to_string()),
        ),
        other => {
            return Err(unsupported(format!(
                "Param kind `{}` is not supported by Ecky IR v0.",
                other
            )))
        }
    };

    Ok(IrParam {
        field,
        default_value,
    })
}

fn parse_select_option(value: &Value) -> AppResult<SelectOption> {
    let pair = list_items(value, "select option")?;
    if pair.len() != 2 {
        return Err(validation("Select options must be `(label value)` pairs."));
    }
    let label = parse_stringish(&pair[0], "select option label")?;
    let value = if let Some(text) = pair[1].as_str() {
        SelectValue::String(text.to_string())
    } else if let Some(symbol) = pair[1].as_symbol() {
        SelectValue::String(symbol.to_string())
    } else if let Some(number) = pair[1].as_f64() {
        SelectValue::Number(number)
    } else {
        return Err(validation("Select option values must be string or number."));
    };
    Ok(SelectOption { label, value })
}

fn parse_part_decl(items: &[Value]) -> AppResult<IrPart> {
    if items.len() < 3 {
        return Err(validation("Each `(part ...)` needs an id and expression."));
    }
    let part_id = parse_stringish(&items[1], "part id")?;
    let (label, expr) =
        if items.len() >= 4 && (items[2].as_str().is_some() || items[2].as_symbol().is_some()) {
            (parse_stringish(&items[2], "part label")?, items[3].clone())
        } else {
            (humanize_key(&part_id), items[2].clone())
        };
    Ok(IrPart {
        part_id,
        label,
        expr,
    })
}

fn humanize_key(key: &str) -> String {
    key.split(['_', '-', '.'])
        .filter(|token| !token.is_empty())
        .map(|token| {
            let mut chars = token.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_param_env(model: &IrModel, parameters: &DesignParams) -> BTreeMap<String, ParamValue> {
    let mut env = BTreeMap::new();
    for param in &model.params {
        env.insert(param.field.key().to_string(), param.default_value.clone());
    }
    for (key, value) in parameters {
        env.insert(key.clone(), value.clone());
    }
    env
}

fn eval_number(value: &Value, env: &BTreeMap<String, ParamValue>) -> AppResult<f64> {
    if let Some(number) = value.as_f64() {
        return Ok(number);
    }
    if let Some(symbol) = value.as_symbol() {
        return env
            .get(symbol)
            .and_then(|param| match param {
                ParamValue::Number(number) => Some(*number),
                _ => None,
            })
            .ok_or_else(|| validation(format!("Unknown numeric symbol `{}`.", symbol)));
    }

    let items = list_items(value, "numeric expression")?;
    let op = head_symbol(&items, "numeric expression")?;
    let args = &items[1..];
    match op {
        "+" => args
            .iter()
            .try_fold(0.0, |acc, arg| Ok(acc + eval_number(arg, env)?)),
        "-" => {
            if args.is_empty() {
                return Err(validation("`-` expects at least one numeric argument."));
            }
            if args.len() == 1 {
                Ok(-eval_number(&args[0], env)?)
            } else {
                let first = eval_number(&args[0], env)?;
                args[1..]
                    .iter()
                    .try_fold(first, |acc, arg| Ok(acc - eval_number(arg, env)?))
            }
        }
        "*" => args
            .iter()
            .try_fold(1.0, |acc, arg| Ok(acc * eval_number(arg, env)?)),
        "/" => {
            if args.len() != 2 {
                return Err(validation("`/` expects exactly two numeric arguments."));
            }
            Ok(eval_number(&args[0], env)? / eval_number(&args[1], env)?)
        }
        "min" => args.iter().try_fold(
            f64::INFINITY,
            |acc, arg| Ok(acc.min(eval_number(arg, env)?)),
        ),
        "max" => args.iter().try_fold(f64::NEG_INFINITY, |acc, arg| {
            Ok(acc.max(eval_number(arg, env)?))
        }),
        "clamp" => {
            if args.len() != 3 {
                return Err(validation("`clamp` expects value, min, and max."));
            }
            Ok(eval_number(&args[0], env)?
                .max(eval_number(&args[1], env)?)
                .min(eval_number(&args[2], env)?))
        }
        "lerp" => {
            if args.len() != 3 {
                return Err(validation("`lerp` expects start, end, and t."));
            }
            let start = eval_number(&args[0], env)?;
            let end = eval_number(&args[1], env)?;
            let t = eval_number(&args[2], env)?;
            Ok(start + (end - start) * t)
        }
        "smoothstep" => {
            if args.len() != 3 {
                return Err(validation("`smoothstep` expects edge0, edge1, and x."));
            }
            let edge0 = eval_number(&args[0], env)?;
            let edge1 = eval_number(&args[1], env)?;
            if approx_eq(edge0, edge1) {
                return Err(validation("`smoothstep` needs distinct edge values."));
            }
            let x = eval_number(&args[2], env)?;
            let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
            Ok(t * t * (3.0 - 2.0 * t))
        }
        "sin" => unary_op(args, env, f64::sin),
        "cos" => unary_op(args, env, f64::cos),
        "tan" => unary_op(args, env, f64::tan),
        "deg" => unary_op(args, env, |value| value.to_radians()),
        "rad" => unary_op(args, env, |value| value.to_degrees()),
        "abs" => unary_op(args, env, f64::abs),
        other => Err(unsupported(format!(
            "Numeric operator `{}` is not supported by Ecky IR v0.",
            other
        ))),
    }
}

fn unary_op(
    args: &[Value],
    env: &BTreeMap<String, ParamValue>,
    op: impl Fn(f64) -> f64,
) -> AppResult<f64> {
    if args.len() != 1 {
        return Err(validation(
            "Unary numeric operator expects exactly one argument.",
        ));
    }
    Ok(op(eval_number(&args[0], env)?))
}

fn eval_bool(value: &Value, env: &BTreeMap<String, ParamValue>) -> AppResult<bool> {
    if let Some(flag) = value.as_bool() {
        return Ok(flag);
    }
    if let Some(symbol) = value.as_symbol() {
        return env
            .get(symbol)
            .and_then(|param| match param {
                ParamValue::Boolean(flag) => Some(*flag),
                _ => None,
            })
            .ok_or_else(|| validation(format!("Unknown boolean symbol `{}`.", symbol)));
    }

    let items = list_items(value, "boolean expression")?;
    let op = head_symbol(&items, "boolean expression")?;
    let args = &items[1..];
    match op {
        "not" => {
            if args.len() != 1 {
                return Err(validation("`not` expects one argument."));
            }
            Ok(!eval_bool(&args[0], env)?)
        }
        "and" => args
            .iter()
            .try_fold(true, |acc, arg| Ok(acc && eval_bool(arg, env)?)),
        "or" => args
            .iter()
            .try_fold(false, |acc, arg| Ok(acc || eval_bool(arg, env)?)),
        "=" => {
            if args.len() != 2 {
                return Err(validation("`=` expects exactly two arguments."));
            }
            // Support both numeric and string equality.
            if let (Ok(a), Ok(b)) = (eval_stringish(&args[0], env), eval_stringish(&args[1], env)) {
                return Ok(a == b);
            }
            compare_numbers(args, env, |a, b| (a - b).abs() <= f64::EPSILON)
        }
        ">" => compare_numbers(args, env, |a, b| a > b),
        ">=" => compare_numbers(args, env, |a, b| a >= b),
        "<" => compare_numbers(args, env, |a, b| a < b),
        "<=" => compare_numbers(args, env, |a, b| a <= b),
        other => Err(unsupported(format!(
            "Boolean operator `{}` is not supported by Ecky IR v0.",
            other
        ))),
    }
}

fn eval_stringish(value: &Value, env: &BTreeMap<String, ParamValue>) -> AppResult<String> {
    if let Some(text) = value.as_str() {
        return Ok(text.to_string());
    }
    if let Some(kw) = value.as_keyword() {
        return Ok(kw.to_string());
    }
    if let Some(symbol) = value.as_symbol() {
        if let Some(param) = env.get(symbol) {
            return match param {
                ParamValue::String(s) => Ok(s.clone()),
                ParamValue::Number(n) => Ok(n.to_string()),
                ParamValue::Boolean(b) => Ok(b.to_string()),
                ParamValue::Null => Ok("null".to_string()),
            };
        }
        return Ok(symbol.to_string());
    }
    Err(validation("Expected a string value."))
}

fn compare_numbers(
    args: &[Value],
    env: &BTreeMap<String, ParamValue>,
    predicate: impl Fn(f64, f64) -> bool,
) -> AppResult<bool> {
    if args.len() != 2 {
        return Err(validation(
            "Comparison expects exactly two numeric arguments.",
        ));
    }
    Ok(predicate(
        eval_number(&args[0], env)?,
        eval_number(&args[1], env)?,
    ))
}

fn eval_points(value: &Value, env: &BTreeMap<String, ParamValue>) -> AppResult<Vec<[f64; 2]>> {
    let points = list_items(value, "polygon points")?;
    points
        .iter()
        .map(|point| {
            let pair = list_items(point, "polygon point")?;
            if pair.len() != 2 {
                return Err(validation("Polygon points must be `(x y)` pairs."));
            }
            Ok([eval_number(&pair[0], env)?, eval_number(&pair[1], env)?])
        })
        .collect()
}

fn eval_points_3d(value: &Value, env: &BTreeMap<String, ParamValue>) -> AppResult<Vec<[f64; 3]>> {
    let points = list_items(value, "3D point list")?;
    points
        .iter()
        .map(|point| {
            let triple = list_items(point, "3D point")?;
            if triple.len() != 3 {
                return Err(validation("3D points must be `(x y z)` triples."));
            }
            Ok([
                eval_number(&triple[0], env)?,
                eval_number(&triple[1], env)?,
                eval_number(&triple[2], env)?,
            ])
        })
        .collect()
}

fn cubic_bezier(p0: [f64; 3], p1: [f64; 3], p2: [f64; 3], p3: [f64; 3], t: f64) -> [f64; 3] {
    let mt = 1.0 - t;
    let c0 = mt * mt * mt;
    let c1 = 3.0 * mt * mt * t;
    let c2 = 3.0 * mt * t * t;
    let c3 = t * t * t;
    [
        c0 * p0[0] + c1 * p1[0] + c2 * p2[0] + c3 * p3[0],
        c0 * p0[1] + c1 * p1[1] + c2 * p2[1] + c3 * p3[1],
        c0 * p0[2] + c1 * p1[2] + c2 * p2[2] + c3 * p3[2],
    ]
}

fn cubic_bezier_tangent(
    p0: [f64; 3],
    p1: [f64; 3],
    p2: [f64; 3],
    p3: [f64; 3],
    t: f64,
) -> [f64; 3] {
    let mt = 1.0 - t;
    let c0 = 3.0 * mt * mt;
    let c1 = 6.0 * mt * t;
    let c2 = 3.0 * t * t;
    [
        c0 * (p1[0] - p0[0]) + c1 * (p2[0] - p1[0]) + c2 * (p3[0] - p2[0]),
        c0 * (p1[1] - p0[1]) + c1 * (p2[1] - p1[1]) + c2 * (p3[1] - p2[1]),
        c0 * (p1[2] - p0[2]) + c1 * (p2[2] - p1[2]) + c2 * (p3[2] - p2[2]),
    ]
}

fn sample_bezier_path(
    points: &[[f64; 3]],
    segments_per_segment: usize,
) -> AppResult<Vec<([f64; 3], [f64; 3])>> {
    if points.len() < 4 || !(points.len() - 1).is_multiple_of(3) {
        return Err(validation(
            "`bezier-path` expects 4, 7, 10, ... points (3n+1).",
        ));
    }
    let mut sampled = Vec::new();
    for i in (0..points.len() - 1).step_by(3) {
        let p0 = points[i];
        let p1 = points[i + 1];
        let p2 = points[i + 2];
        let p3 = points[i + 3];
        for step in 0..segments_per_segment {
            let t = step as f64 / segments_per_segment as f64;
            sampled.push((
                cubic_bezier(p0, p1, p2, p3, t),
                cubic_bezier_tangent(p0, p1, p2, p3, t),
            ));
        }
    }
    let last_p = *points.last().unwrap();
    let last_p0 = points[points.len() - 4];
    let last_p1 = points[points.len() - 3];
    let last_p2 = points[points.len() - 2];
    let last_p3 = points[points.len() - 1];
    sampled.push((
        last_p,
        cubic_bezier_tangent(last_p0, last_p1, last_p2, last_p3, 1.0),
    ));
    Ok(sampled)
}

fn normalize_loop_points(points: &[[f64; 2]], context: &str) -> AppResult<LoopPoints> {
    let mut normalized = points.to_vec();
    if normalized.len() > 1 {
        let first = normalized[0];
        let last = *normalized.last().expect("checked above");
        if approx_eq(first[0], last[0]) && approx_eq(first[1], last[1]) {
            normalized.pop();
        }
    }
    if normalized.len() < 3 {
        return Err(validation(format!(
            "{} needs at least three unique points.",
            context
        )));
    }
    Ok(normalized)
}

fn normalize_loop_from_coords(coords: &[Coord<f64>], context: &str) -> AppResult<LoopPoints> {
    let points: Vec<[f64; 2]> = coords.iter().map(|coord| [coord.x, coord.y]).collect();
    normalize_loop_points(&points, context)
}

fn loop_centroid(points: &[[f64; 2]]) -> [f64; 2] {
    let mut x = 0.0;
    let mut y = 0.0;
    let len = points.len().max(1) as f64;
    for point in points {
        x += point[0];
        y += point[1];
    }
    [x / len, y / len]
}

fn contour_sort_key(points: &[[f64; 2]]) -> (i64, i64, i64) {
    let centroid = loop_centroid(points);
    let area = signed_area(points).abs();
    (
        (centroid[0] * 1000.0).round() as i64,
        (centroid[1] * 1000.0).round() as i64,
        (area * 1000.0).round() as i64,
    )
}

fn signed_area(points: &[[f64; 2]]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    for (index, point) in points.iter().enumerate() {
        let next = points[(index + 1) % points.len()];
        area += point[0] * next[1] - next[0] * point[1];
    }
    area * 0.5
}

fn build_ring(points: &[[f64; 2]]) -> LineString<f64> {
    let mut coords: Vec<Coord<f64>> = points
        .iter()
        .map(|point| Coord {
            x: point[0],
            y: point[1],
        })
        .collect();
    if let Some(first) = coords.first().copied() {
        if coords.last().copied() != Some(first) {
            coords.push(first);
        }
    }
    LineString::new(coords)
}

fn sketch_contours_from_loops(contours: SketchContours, context: &str) -> AppResult<ContourSet2d> {
    if contours.outer_loops.is_empty() {
        return Err(validation(format!(
            "{} needs at least one outer loop.",
            context
        )));
    }

    let outer_polygons: Vec<GeoPolygon<f64>> = contours
        .outer_loops
        .iter()
        .map(|loop_points| {
            GeoPolygon::new(build_ring(loop_points), vec![]).orient(Direction::Default)
        })
        .collect();

    let mut holes_by_outer: Vec<Vec<LoopPoints>> = vec![Vec::new(); outer_polygons.len()];
    for hole in &contours.hole_loops {
        let probe = Coord {
            x: loop_centroid(hole)[0],
            y: loop_centroid(hole)[1],
        };
        let outer_index = outer_polygons
            .iter()
            .enumerate()
            .find(|(_, polygon)| polygon.contains(&probe))
            .map(|(index, _)| index)
            .ok_or_else(|| {
                validation(format!(
                    "{} contains a hole loop that is not inside any outer loop.",
                    context
                ))
            })?;
        holes_by_outer[outer_index].push(hole.clone());
    }

    let mut polygons = outer_polygons
        .into_iter()
        .enumerate()
        .map(|(index, polygon)| ContourPolygon2d {
            outer: normalize_loop_from_coords(&polygon.exterior().0, context)
                .expect("normalized outer contour"),
            holes: holes_by_outer[index].clone(),
        })
        .collect::<Vec<_>>();

    sort_contour_set(&mut polygons);
    Ok(ContourSet2d { polygons })
}

fn sort_contour_set(polygons: &mut [ContourPolygon2d]) {
    polygons.sort_by_key(|polygon| contour_sort_key(&polygon.outer));
    for polygon in polygons {
        polygon.holes.sort_by_key(|hole| contour_sort_key(hole));
    }
}

fn contours_from_sketch(sketch: &IrSketch, context: &str) -> AppResult<ContourSet2d> {
    let multipolygon = sketch.to_multipolygon().orient(Direction::Default);
    let mut polygons = Vec::new();
    for polygon in multipolygon.0 {
        let outer = normalize_loop_from_coords(&polygon.exterior().0, context)?;
        let mut holes = Vec::new();
        for hole in polygon.interiors() {
            holes.push(normalize_loop_from_coords(&hole.0, context)?);
        }
        polygons.push(ContourPolygon2d { outer, holes });
    }

    if polygons.is_empty() {
        return Err(validation(format!(
            "{} needs at least one closed contour.",
            context
        )));
    }

    sort_contour_set(&mut polygons);
    Ok(ContourSet2d { polygons })
}

fn contour_set_to_sketch(contours: &ContourSet2d) -> IrSketch {
    let multipolygon = MultiPolygon(
        contours
            .polygons
            .iter()
            .map(|polygon| {
                GeoPolygon::new(
                    build_ring(&polygon.outer),
                    polygon.holes.iter().map(|hole| build_ring(hole)).collect(),
                )
                .orient(Direction::Default)
            })
            .collect(),
    )
    .orient(Direction::Default);
    IrSketch::from_geo(
        GeometryCollection::from(vec![GeoGeometry::MultiPolygon(multipolygon)]),
        None,
    )
}

fn eval_sketch_primitive_points(
    value: &Value,
    env: &BTreeMap<String, ParamValue>,
    context: &str,
) -> AppResult<LoopPoints> {
    if let Some(items) = value.to_vec() {
        if let Ok(node) = head_symbol(&items, context) {
            let args = &items[1..];
            match node {
                "circle" => {
                    let radius = eval_number(&args[0], env)?;
                    let segments = args
                        .get(1)
                        .map(|arg| eval_number(arg, env))
                        .transpose()?
                        .unwrap_or(48.0) as usize;
                    return normalize_loop_points(&circle_points(radius, segments.max(3)), context);
                }
                "polygon" => {
                    return normalize_loop_points(&eval_points(&args[0], env)?, context);
                }
                "rounded_rect" | "rounded-rect" => {
                    let width = eval_number(&args[0], env)?;
                    let height = eval_number(&args[1], env)?;
                    let corner_radius = eval_number(&args[2], env)?;
                    let segments = args
                        .get(3)
                        .map(|arg| eval_number(arg, env))
                        .transpose()?
                        .unwrap_or(12.0) as usize;
                    return normalize_loop_points(
                        &rounded_rectangle_points(width, height, corner_radius, segments.max(2)),
                        context,
                    );
                }
                "rounded-polygon" | "rounded_polygon" => {
                    let points = eval_points(&args[0], env)?;
                    let radius = eval_number(&args[1], env)?;
                    let segments = args
                        .get(2)
                        .map(|arg| parse_count(arg, env, "rounded-polygon segments", 2))
                        .transpose()?
                        .unwrap_or(8usize);
                    return normalize_loop_points(
                        &rounded_polygon_points(&points, radius, segments)?,
                        context,
                    );
                }
                "bspline" => {
                    let points = eval_points(&args[0], env)?;
                    let closed = args
                        .get(1)
                        .map(|arg| eval_bool(arg, env))
                        .transpose()?
                        .unwrap_or(true);
                    let samples = args
                        .get(2)
                        .map(|arg| parse_count(arg, env, "bspline samples", 1))
                        .transpose()?
                        .unwrap_or(12usize);
                    return normalize_loop_points(
                        &bspline_points(&points, samples, closed)?,
                        context,
                    );
                }
                _ => {}
            }
        }
    }

    normalize_loop_points(&eval_points(value, env)?, context)
}

fn parse_loop_collection(
    value: &Value,
    env: &BTreeMap<String, ParamValue>,
    context: &str,
) -> AppResult<Vec<LoopPoints>> {
    let items = list_items(value, context)?;
    if items.is_empty() {
        return Ok(Vec::new());
    }

    // If it looks like a single node call (symbol followed by args) or a single point list.
    let is_node = items
        .first()
        .and_then(|v| v.as_symbol())
        .map(|s| !s.starts_with(':'))
        .unwrap_or(false);

    if is_node {
        return Ok(vec![eval_sketch_primitive_points(value, env, context)?]);
    }

    let is_single_loop = items
        .first()
        .and_then(|value| value.to_vec())
        .map(|pair| pair.len() == 2)
        .unwrap_or(false);

    if is_single_loop {
        return Ok(vec![normalize_loop_points(
            &eval_points(value, env)?,
            context,
        )?]);
    }

    items
        .iter()
        .map(|loop_value| eval_sketch_primitive_points(loop_value, env, context))
        .collect()
}

fn parse_profile_sketch(args: &[Value], env: &BTreeMap<String, ParamValue>) -> AppResult<IrSketch> {
    if args.is_empty() {
        return Err(validation("`profile` expects contour data."));
    }

    if args.len() == 1 && keyword_name(&args[0]).is_none() {
        return Ok(IrSketch::polygon(&eval_points(&args[0], env)?, None));
    }

    let mut outer_loops: Vec<LoopPoints> = Vec::new();
    let mut hole_loops: Vec<LoopPoints> = Vec::new();

    for value in args {
        let pair = list_items(value, "profile clause")?;
        if pair.len() != 2 {
            return Err(validation(
                "`profile` clauses must look like `(:outer ...)` or `(:holes ...)`.",
            ));
        }
        let name = keyword_name(&pair[0]).ok_or_else(|| {
            validation("`profile` clauses must use keywords like `:outer` and `:holes`.")
        })?;
        match name {
            "outer" => {
                outer_loops.extend(parse_loop_collection(&pair[1], env, "profile outer loops")?)
            }
            "holes" => {
                hole_loops.extend(parse_loop_collection(&pair[1], env, "profile hole loops")?)
            }
            other => {
                return Err(validation(format!(
                    "`profile` does not recognize clause `:{}`.",
                    other
                )))
            }
        }
    }

    let contours = sketch_contours_from_loops(
        SketchContours {
            outer_loops,
            hole_loops,
        },
        "profile",
    )?;
    Ok(contour_set_to_sketch(&contours))
}

fn distance2(point: [f64; 2], other: [f64; 2]) -> f64 {
    ((other[0] - point[0]).powi(2) + (other[1] - point[1]).powi(2)).sqrt()
}

fn normalize2(point: [f64; 2]) -> [f64; 2] {
    let length = (point[0] * point[0] + point[1] * point[1]).sqrt();
    if length <= f64::EPSILON {
        [0.0, 0.0]
    } else {
        [point[0] / length, point[1] / length]
    }
}

fn circle_points(radius: f64, segments: usize) -> Vec<[f64; 2]> {
    let mut points = Vec::with_capacity(segments);
    for i in 0..segments {
        let angle = 2.0 * std::f64::consts::PI * i as f64 / segments as f64;
        points.push([radius * angle.cos(), radius * angle.sin()]);
    }
    points
}

fn rounded_rectangle_points(
    width: f64,
    height: f64,
    radius: f64,
    segments: usize,
) -> Vec<[f64; 2]> {
    let r = radius.min(width / 2.0).min(height / 2.0);
    let mut points = Vec::new();

    let centers = [
        [width / 2.0 - r, height / 2.0 - r],
        [-width / 2.0 + r, height / 2.0 - r],
        [-width / 2.0 + r, -height / 2.0 + r],
        [width / 2.0 - r, -height / 2.0 + r],
    ];

    let angles = [(0.0, 90.0), (90.0, 180.0), (180.0, 270.0), (270.0, 360.0)];

    for i in 0..4 {
        let center = centers[i];
        let (start, end) = angles[i];
        for j in 0..=segments {
            let a = (start + (end - start) * j as f64 / segments as f64).to_radians();
            points.push([center[0] + r * a.cos(), center[1] + r * a.sin()]);
        }
    }

    points
}

fn rounded_polygon_points(
    points: &[[f64; 2]],
    radius: f64,
    segments: usize,
) -> AppResult<LoopPoints> {
    let base = normalize_loop_points(points, "`rounded-polygon`")?;
    if radius <= 0.0 {
        return Ok(base);
    }

    let segment_count = segments.max(2);
    let mut rounded = Vec::new();
    let polygon_area = signed_area(&base);
    let ccw = polygon_area >= 0.0;

    for index in 0..base.len() {
        let prev = base[(index + base.len() - 1) % base.len()];
        let current = base[index];
        let next = base[(index + 1) % base.len()];

        let incoming = normalize2([prev[0] - current[0], prev[1] - current[1]]);
        let outgoing = normalize2([next[0] - current[0], next[1] - current[1]]);
        let incoming_len = distance2(prev, current);
        let outgoing_len = distance2(current, next);
        if incoming_len <= f64::EPSILON || outgoing_len <= f64::EPSILON {
            continue;
        }

        let dot = ((-incoming[0] * outgoing[0]) + (-incoming[1] * outgoing[1])).clamp(-1.0, 1.0);
        let interior_angle = dot.acos();
        if interior_angle <= 1e-4 || (std::f64::consts::PI - interior_angle).abs() <= 1e-4 {
            rounded.push(current);
            continue;
        }

        let offset = (radius / (interior_angle * 0.5).tan())
            .min(incoming_len * 0.5)
            .min(outgoing_len * 0.5);
        if offset <= f64::EPSILON {
            rounded.push(current);
            continue;
        }

        let start = [
            current[0] + incoming[0] * offset,
            current[1] + incoming[1] * offset,
        ];
        let end = [
            current[0] + outgoing[0] * offset,
            current[1] + outgoing[1] * offset,
        ];
        let bisector = normalize2([(-incoming[0]) + outgoing[0], (-incoming[1]) + outgoing[1]]);
        let center_distance = radius / (interior_angle * 0.5).sin();
        let turn = (outgoing[0] - (-incoming[0])) * (outgoing[1] + incoming[1]);
        let center_sign = if ccw { 1.0 } else { -1.0 };
        let adjusted_bisector = if bisector == [0.0, 0.0] || !turn.is_finite() {
            [-incoming[1] * center_sign, incoming[0] * center_sign]
        } else {
            bisector
        };
        let center = [
            current[0] + adjusted_bisector[0] * center_distance,
            current[1] + adjusted_bisector[1] * center_distance,
        ];

        let start_angle = (start[1] - center[1]).atan2(start[0] - center[0]);
        let end_angle = (end[1] - center[1]).atan2(end[0] - center[0]);
        let mut delta = end_angle - start_angle;
        if ccw && delta <= 0.0 {
            delta += std::f64::consts::TAU;
        } else if !ccw && delta >= 0.0 {
            delta -= std::f64::consts::TAU;
        }

        rounded.push(start);
        for segment in 1..segment_count {
            let t = segment as f64 / segment_count as f64;
            let theta = start_angle + delta * t;
            rounded.push([
                center[0] + radius * theta.cos(),
                center[1] + radius * theta.sin(),
            ]);
        }
        rounded.push(end);
    }

    normalize_loop_points(&rounded, "`rounded-polygon`")
}

fn bspline_points(
    control_points: &[[f64; 2]],
    samples: usize,
    closed: bool,
) -> AppResult<LoopPoints> {
    if control_points.len() < 3 {
        return Err(validation("`bspline` needs at least three control points."));
    }

    let n = control_points.len();
    let mut points = Vec::new();
    let sample_count = samples.max(2);

    if closed {
        let mut cp = control_points.to_vec();
        cp.push(control_points[0]);
        cp.push(control_points[1]);
        cp.push(control_points[2]);

        for i in 0..n {
            let p0 = cp[i];
            let p1 = cp[i + 1];
            let p2 = cp[i + 2];
            let p3 = cp[i + 3];

            for j in 0..sample_count {
                let t = j as f64 / sample_count as f64;
                points.push(sample_cubic_bspline(p0, p1, p2, p3, t));
            }
        }
    } else {
        if n < 4 {
            return Err(validation(
                "Open `bspline` needs at least four control points for cubic interpolation.",
            ));
        }
        for i in 0..n - 3 {
            let p0 = control_points[i];
            let p1 = control_points[i + 1];
            let p2 = control_points[i + 2];
            let p3 = control_points[i + 3];

            for j in 0..sample_count {
                let t = j as f64 / sample_count as f64;
                points.push(sample_cubic_bspline(p0, p1, p2, p3, t));
            }
        }
        points.push(sample_cubic_bspline(
            control_points[n - 4],
            control_points[n - 3],
            control_points[n - 2],
            control_points[n - 1],
            1.0,
        ));
    }

    normalize_loop_points(&points, "`bspline`")
}

fn sample_cubic_bspline(
    p0: [f64; 2],
    p1: [f64; 2],
    p2: [f64; 2],
    p3: [f64; 2],
    t: f64,
) -> [f64; 2] {
    let t2 = t * t;
    let t3 = t2 * t;

    let f1 = (-t3 + 3.0 * t2 - 3.0 * t + 1.0) / 6.0;
    let f2 = (3.0 * t3 - 6.0 * t2 + 4.0) / 6.0;
    let f3 = (-3.0 * t3 + 3.0 * t2 + 3.0 * t + 1.0) / 6.0;
    let f4 = t3 / 6.0;

    [
        f1 * p0[0] + f2 * p1[0] + f3 * p2[0] + f4 * p3[0],
        f1 * p0[1] + f2 * p1[1] + f3 * p2[1] + f4 * p3[1],
    ]
}

fn resample_loop(points: &[[f64; 2]], target_count: usize) -> LoopPoints {
    let n = points.len();
    if n == 0 || target_count == 0 {
        return Vec::new();
    }
    if n == target_count {
        return points.to_vec();
    }

    let mut perimeter = 0.0;
    let mut segment_lengths = Vec::with_capacity(n);
    for i in 0..n {
        let p1 = points[i];
        let p2 = points[(i + 1) % n];
        let d = ((p2[0] - p1[0]).powi(2) + (p2[1] - p1[1]).powi(2)).sqrt();
        segment_lengths.push(d);
        perimeter += d;
    }

    let mut resampled = Vec::with_capacity(target_count);
    for i in 0..target_count {
        let target_d = (i as f64 / target_count as f64) * perimeter;
        let mut current_d = 0.0;
        let mut found = false;
        for j in 0..n {
            if current_d + segment_lengths[j] >= target_d - 1e-7 {
                let t = if segment_lengths[j] > 1e-9 {
                    ((target_d - current_d) / segment_lengths[j]).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let p1 = points[j];
                let p2 = points[(j + 1) % n];
                resampled.push([p1[0] + (p2[0] - p1[0]) * t, p1[1] + (p2[1] - p1[1]) * t]);
                found = true;
                break;
            }
            current_d += segment_lengths[j];
        }
        if !found {
            resampled.push(*points.last().unwrap());
        }
    }
    resampled
}

fn approx_eq(left: f64, right: f64) -> bool {
    (left - right).abs() <= f64::EPSILON
}

fn parse_count(
    value: &Value,
    env: &BTreeMap<String, ParamValue>,
    context: &str,
    minimum: usize,
) -> AppResult<usize> {
    let parsed = eval_number(value, env)?;
    if !parsed.is_finite() {
        return Err(validation(format!(
            "`{}` must be a finite number.",
            context
        )));
    }
    Ok(parsed.round().max(minimum as f64) as usize)
}

fn parse_wall_pattern_spec(
    value: &Value,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<WallPatternSpec> {
    let items = list_items(value, "wall-pattern options")?;
    if items.is_empty() || items.len() % 2 != 0 {
        return Err(validation(
            "`wall-pattern` expects keyword/value pairs like `(:mode ribs :depth 1.2 ...)`.",
        ));
    }

    let mut mode = None;
    let mut depth = None;
    let mut u_freq = 8.0;
    let mut v_freq = 0.0;
    let mut phase = 0.0;
    let mut bias = 0.0;
    let mut duty = 0.5;
    let mut softness = 0.15;
    let mut twist_deg = 0.0;
    let mut seed = 0u64;
    let mut rim_fade = 0.08;

    let mut index = 0usize;
    while index < items.len() {
        let name = keyword_name(&items[index]).ok_or_else(|| {
            validation("`wall-pattern` options must use keywords like `:mode` and `:depth`.")
        })?;
        let option_value = &items[index + 1];
        match name {
            "mode" => {
                let pattern_name = eval_stringish(option_value, env)?;
                mode = Some(match pattern_name.as_str() {
                    "ribs" => WallPatternMode::Ribs,
                    "rings" => WallPatternMode::Rings,
                    "spiral" => WallPatternMode::Spiral,
                    "diamond" => WallPatternMode::Diamond,
                    "hammered" => WallPatternMode::Hammered,
                    other => {
                        return Err(unsupported(format!(
                            "wall-pattern mode `{}` is not supported by Ecky IR v0.",
                            other
                        )))
                    }
                });
            }
            "depth" => depth = Some(eval_number(option_value, env)?),
            "uFreq" | "ufreq" => u_freq = eval_number(option_value, env)?,
            "vFreq" | "vfreq" => v_freq = eval_number(option_value, env)?,
            "phase" => phase = eval_number(option_value, env)?,
            "bias" => bias = eval_number(option_value, env)?,
            "duty" => duty = eval_number(option_value, env)?,
            "softness" => softness = eval_number(option_value, env)?,
            "twistDeg" | "twistdeg" => twist_deg = eval_number(option_value, env)?,
            "seed" => seed = eval_number(option_value, env)?.round().max(0.0) as u64,
            "rimFade" | "rimfade" => rim_fade = eval_number(option_value, env)?,
            other => {
                return Err(validation(format!(
                    "`wall-pattern` does not recognize option `:{}`.",
                    other
                )))
            }
        }
        index += 2;
    }

    Ok(WallPatternSpec {
        mode: mode.ok_or_else(|| validation("`wall-pattern` requires `:mode`."))?,
        depth: depth.ok_or_else(|| validation("`wall-pattern` requires `:depth`."))?,
        u_freq,
        v_freq,
        phase,
        bias,
        duty,
        softness,
        twist_deg,
        seed,
        rim_fade,
    })
}

fn contour_outer_loops(contours: &ContourSet2d) -> Vec<LoopPoints> {
    contours
        .polygons
        .iter()
        .map(|polygon| polygon.outer.clone())
        .collect()
}

fn contour_hole_loops(contours: &ContourSet2d) -> Vec<LoopPoints> {
    contours
        .polygons
        .iter()
        .flat_map(|polygon| polygon.holes.clone())
        .collect()
}

fn contour_all_loops(contours: &ContourSet2d) -> Vec<LoopPoints> {
    let mut loops = contour_outer_loops(contours);
    loops.extend(contour_hole_loops(contours));
    loops
}

fn contour_sweep_slice_from_contours(
    contours: &ContourSet2d,
    blocked_loops: Vec<LoopPoints>,
    z: f64,
) -> ContourSweepSlice {
    ContourSweepSlice {
        z,
        outer_loops: contour_outer_loops(contours),
        blocked_loops,
    }
}

fn build_wall_pattern_target(
    value: &Value,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<(IrMesh, WallPatternTarget)> {
    let items = list_items(value, "wall-pattern target")?;
    let node = head_symbol(&items, "wall-pattern target")?;
    let args = &items[1..];

    match node {
        "extrude" => {
            if args.len() != 2 {
                return Err(validation("`extrude` expects a sketch and height."));
            }
            let sketch = eval_geometry(&args[0], env)?.into_sketch("wall-pattern")?;
            let height = eval_number(&args[1], env)?;
            let contours = contours_from_sketch(&sketch, "wall-pattern")?;
            Ok((
                sketch.extrude(height),
                WallPatternTarget::ContourSweep {
                    slices: vec![
                        contour_sweep_slice_from_contours(
                            &contours,
                            contour_hole_loops(&contours),
                            0.0,
                        ),
                        contour_sweep_slice_from_contours(
                            &contours,
                            contour_hole_loops(&contours),
                            height,
                        ),
                    ],
                },
            ))
        }
        "taper" => {
            if !(args.len() == 3 || args.len() == 4) {
                return Err(validation(
                    "`taper` expects height, scale, sketch or height, scale-x, scale-y, sketch.",
                ));
            }
            let height = eval_number(&args[0], env)?;
            let (scale_x, scale_y, sketch_index) = if args.len() == 3 {
                let scale = eval_number(&args[1], env)?;
                (scale, scale, 2usize)
            } else {
                (
                    eval_number(&args[1], env)?,
                    eval_number(&args[2], env)?,
                    3usize,
                )
            };
            let sketch = eval_geometry(&args[sketch_index], env)?.into_sketch("wall-pattern")?;
            let base_contours = contours_from_sketch(&sketch, "wall-pattern")?;
            let top = sketch.scale(scale_x, scale_y, 1.0);
            let top_contours = contours_from_sketch(&top, "wall-pattern")?;
            Ok((
                taper_mesh(&sketch, height, scale_x, scale_y, "wall-pattern")?,
                WallPatternTarget::ContourSweep {
                    slices: vec![
                        contour_sweep_slice_from_contours(
                            &base_contours,
                            contour_hole_loops(&base_contours),
                            0.0,
                        ),
                        contour_sweep_slice_from_contours(
                            &top_contours,
                            contour_hole_loops(&top_contours),
                            height,
                        ),
                    ],
                },
            ))
        }
        "twist" => {
            if !(args.len() == 3 || args.len() == 4) {
                return Err(validation(
                    "`twist` expects height, angle, sketch or height, angle, segments, sketch.",
                ));
            }
            let height = eval_number(&args[0], env)?;
            let angle_deg = eval_number(&args[1], env)?;
            let (segments, sketch_index) = if args.len() == 3 {
                (12usize, 2usize)
            } else {
                (parse_count(&args[2], env, "twist segments", 1)?, 3usize)
            };
            let sketch = eval_geometry(&args[sketch_index], env)?.into_sketch("wall-pattern")?;
            let mut slices = Vec::with_capacity(segments + 1);
            for index in 0..=segments {
                let t = index as f64 / segments as f64;
                let z = height * t;
                let rotated = sketch.rotate(0.0, 0.0, angle_deg * t);
                let contours = contours_from_sketch(&rotated, "wall-pattern")?;
                let blocked_loops = contour_hole_loops(&contours);
                slices.push(contour_sweep_slice_from_contours(&contours, blocked_loops, z));
            }
            Ok((
                twist_mesh(&sketch, height, angle_deg, segments, "wall-pattern")?,
                WallPatternTarget::ContourSweep { slices },
            ))
        }
        "revolve" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(validation("`revolve` expects a sketch, angle, and optional segments."));
            }
            let sketch = eval_geometry(&args[0], env)?.into_sketch("wall-pattern")?;
            let angle_deg = eval_number(&args[1], env)?;
            let segments = args
                .get(2)
                .map(|arg| parse_count(arg, env, "revolve segments", 12))
                .transpose()?
                .unwrap_or(48usize);
            let contours = contours_from_sketch(&sketch, "wall-pattern")?;
            let outer_loops = contour_outer_loops(&contours);
            let z_min = outer_loops
                .iter()
                .flat_map(|loop_points| loop_points.iter().map(|point| point[1]))
                .fold(f64::INFINITY, f64::min);
            let z_max = outer_loops
                .iter()
                .flat_map(|loop_points| loop_points.iter().map(|point| point[1]))
                .fold(f64::NEG_INFINITY, f64::max);
            Ok((
                revolve_mesh(&sketch, angle_deg, segments, "wall-pattern")?,
                WallPatternTarget::RevolveProfile {
                    angle_deg,
                    z_min,
                    z_max,
                    outer_loops,
                    blocked_loops: contour_hole_loops(&contours),
                },
            ))
        }
        "shell" => {
            if args.len() != 2 {
                return Err(validation("`shell` expects wall thickness and a supported solid node."));
            }
            let wall = eval_number(&args[0], env)?;
            let mesh = eval_shell_geometry(&args[1], wall, env)?;
            let shell_items = list_items(&args[1], "wall-pattern shell target")?;
            let shell_node = head_symbol(&shell_items, "wall-pattern shell target")?;
            let shell_args = &shell_items[1..];
            let target = match shell_node {
                "cylinder" => {
                    if shell_args.len() < 2 || shell_args.len() > 3 {
                        return Err(validation("`shell` cylinder expects radius, height, and optional segments."));
                    }
                    let outer_radius = eval_number(&shell_args[0], env)?;
                    let height = eval_number(&shell_args[1], env)?;
                    WallPatternTarget::ShellCylinder {
                        outer_radius,
                        inner_radius: outer_radius - wall,
                        height,
                    }
                }
                "cone" => {
                    if shell_args.len() < 3 || shell_args.len() > 4 {
                        return Err(validation(
                            "`shell` cone expects bottom radius, top radius, height, and optional segments.",
                        ));
                    }
                    let outer_bottom_radius = eval_number(&shell_args[0], env)?;
                    let outer_top_radius = eval_number(&shell_args[1], env)?;
                    let height = eval_number(&shell_args[2], env)?;
                    WallPatternTarget::ShellCone {
                        outer_bottom_radius,
                        outer_top_radius,
                        inner_bottom_radius: outer_bottom_radius - wall,
                        inner_top_radius: outer_top_radius - wall,
                        height,
                    }
                }
                "sphere" => {
                    if shell_args.is_empty() || shell_args.len() > 3 {
                        return Err(validation("`shell` sphere expects radius and optional slices/stacks."));
                    }
                    let outer_radius = eval_number(&shell_args[0], env)?;
                    WallPatternTarget::ShellSphere {
                        outer_radius,
                        inner_radius: outer_radius - wall,
                    }
                }
                "extrude" => {
                    if shell_args.len() != 2 {
                        return Err(validation("`shell` extrude expects a sketch and height."));
                    }
                    let outer_sketch = eval_geometry(&shell_args[0], env)?.into_sketch("wall-pattern")?;
                    let inner_sketch = offset_sketch(&outer_sketch, -wall, false, "wall-pattern")?;
                    let height = eval_number(&shell_args[1], env)?;
                    let outer_contours = contours_from_sketch(&outer_sketch, "wall-pattern")?;
                    let inner_contours = contours_from_sketch(&inner_sketch, "wall-pattern")?;
                    WallPatternTarget::ContourSweep {
                        slices: vec![
                            contour_sweep_slice_from_contours(
                                &outer_contours,
                                {
                                    let mut blocked = contour_hole_loops(&outer_contours);
                                    blocked.extend(contour_all_loops(&inner_contours));
                                    blocked
                                },
                                0.0,
                            ),
                            contour_sweep_slice_from_contours(
                                &outer_contours,
                                {
                                    let mut blocked = contour_hole_loops(&outer_contours);
                                    blocked.extend(contour_all_loops(&inner_contours));
                                    blocked
                                },
                                height,
                            ),
                        ],
                    }
                }
                "revolve" => {
                    if shell_args.len() < 2 || shell_args.len() > 3 {
                        return Err(validation("`shell` revolve expects a sketch, angle, and optional segments."));
                    }
                    let outer_sketch = eval_geometry(&shell_args[0], env)?.into_sketch("wall-pattern")?;
                    let inner_sketch = offset_sketch(&outer_sketch, -wall, false, "wall-pattern")?;
                    let angle_deg = eval_number(&shell_args[1], env)?;
                    let outer_contours = contours_from_sketch(&outer_sketch, "wall-pattern")?;
                    let inner_contours = contours_from_sketch(&inner_sketch, "wall-pattern")?;
                    let outer_loops = contour_outer_loops(&outer_contours);
                    let z_min = outer_loops
                        .iter()
                        .flat_map(|loop_points| loop_points.iter().map(|point| point[1]))
                        .fold(f64::INFINITY, f64::min);
                    let z_max = outer_loops
                        .iter()
                        .flat_map(|loop_points| loop_points.iter().map(|point| point[1]))
                        .fold(f64::NEG_INFINITY, f64::max);
                    WallPatternTarget::RevolveProfile {
                        angle_deg,
                        z_min,
                        z_max,
                        outer_loops,
                        blocked_loops: {
                            let mut blocked = contour_hole_loops(&outer_contours);
                            blocked.extend(contour_all_loops(&inner_contours));
                            blocked
                        },
                    }
                }
                "loft" => {
                    if shell_args.len() != 3 {
                        return Err(validation("`shell` loft expects height, bottom sketch, and top sketch."));
                    }
                    let height = eval_number(&shell_args[0], env)?;
                    let bottom = eval_geometry(&shell_args[1], env)?.into_sketch("wall-pattern")?;
                    let top = eval_geometry(&shell_args[2], env)?.into_sketch("wall-pattern")?;
                    let inner_bottom = offset_sketch(&bottom, -wall, false, "wall-pattern")?;
                    let inner_top = offset_sketch(&top, -wall, false, "wall-pattern")?;
                    let outer_bottom = contours_from_sketch(&bottom, "wall-pattern")?;
                    let outer_top = contours_from_sketch(&top, "wall-pattern")?;
                    let inner_bottom_contours = contours_from_sketch(&inner_bottom, "wall-pattern")?;
                    let inner_top_contours = contours_from_sketch(&inner_top, "wall-pattern")?;
                    WallPatternTarget::ContourSweep {
                        slices: vec![
                            contour_sweep_slice_from_contours(
                                &outer_bottom,
                                {
                                    let mut blocked = contour_hole_loops(&outer_bottom);
                                    blocked.extend(contour_all_loops(&inner_bottom_contours));
                                    blocked
                                },
                                0.0,
                            ),
                            contour_sweep_slice_from_contours(
                                &outer_top,
                                {
                                    let mut blocked = contour_hole_loops(&outer_top);
                                    blocked.extend(contour_all_loops(&inner_top_contours));
                                    blocked
                                },
                                height,
                            ),
                        ],
                    }
                }
                "taper" => {
                    if !(shell_args.len() == 3 || shell_args.len() == 4) {
                        return Err(validation(
                            "`shell` taper expects height, scale, sketch or height, scale-x, scale-y, sketch.",
                        ));
                    }
                    let height = eval_number(&shell_args[0], env)?;
                    let (scale_x, scale_y, sketch_index) = if shell_args.len() == 3 {
                        let scale = eval_number(&shell_args[1], env)?;
                        (scale, scale, 2usize)
                    } else {
                        (
                            eval_number(&shell_args[1], env)?,
                            eval_number(&shell_args[2], env)?,
                            3usize,
                        )
                    };
                    let base = eval_geometry(&shell_args[sketch_index], env)?.into_sketch("wall-pattern")?;
                    let inner_base = offset_sketch(&base, -wall, false, "wall-pattern")?;
                    let top = base.scale(scale_x, scale_y, 1.0);
                    let inner_top = inner_base.scale(scale_x, scale_y, 1.0);
                    let base_contours = contours_from_sketch(&base, "wall-pattern")?;
                    let top_contours = contours_from_sketch(&top, "wall-pattern")?;
                    let inner_base_contours = contours_from_sketch(&inner_base, "wall-pattern")?;
                    let inner_top_contours = contours_from_sketch(&inner_top, "wall-pattern")?;
                    WallPatternTarget::ContourSweep {
                        slices: vec![
                            contour_sweep_slice_from_contours(
                                &base_contours,
                                {
                                    let mut blocked = contour_hole_loops(&base_contours);
                                    blocked.extend(contour_all_loops(&inner_base_contours));
                                    blocked
                                },
                                0.0,
                            ),
                            contour_sweep_slice_from_contours(
                                &top_contours,
                                {
                                    let mut blocked = contour_hole_loops(&top_contours);
                                    blocked.extend(contour_all_loops(&inner_top_contours));
                                    blocked
                                },
                                height,
                            ),
                        ],
                    }
                }
                "twist" => {
                    if !(shell_args.len() == 3 || shell_args.len() == 4) {
                        return Err(validation(
                            "`shell` twist expects height, angle, sketch or height, angle, segments, sketch.",
                        ));
                    }
                    let height = eval_number(&shell_args[0], env)?;
                    let angle_deg = eval_number(&shell_args[1], env)?;
                    let (segments, sketch_index) = if shell_args.len() == 3 {
                        (12usize, 2usize)
                    } else {
                        (parse_count(&shell_args[2], env, "shell twist segments", 1)?, 3usize)
                    };
                    let base = eval_geometry(&shell_args[sketch_index], env)?.into_sketch("wall-pattern")?;
                    let inner_base = offset_sketch(&base, -wall, false, "wall-pattern")?;
                    let mut slices = Vec::with_capacity(segments + 1);
                    for index in 0..=segments {
                        let t = index as f64 / segments as f64;
                        let z = height * t;
                        let outer_contours =
                            contours_from_sketch(&base.rotate(0.0, 0.0, angle_deg * t), "wall-pattern")?;
                        let inner_contours =
                            contours_from_sketch(&inner_base.rotate(0.0, 0.0, angle_deg * t), "wall-pattern")?;
                        let mut blocked = contour_hole_loops(&outer_contours);
                        blocked.extend(contour_all_loops(&inner_contours));
                        slices.push(contour_sweep_slice_from_contours(&outer_contours, blocked, z));
                    }
                    WallPatternTarget::ContourSweep { slices }
                }
                other => {
                    return Err(unsupported(format!(
                        "Node `wall-pattern` supports `shell` targets for cylinder, cone, sphere, extrude, revolve, loft, taper, and twist. It does not support `{}` yet.",
                        other
                    )))
                }
            };
            Ok((mesh, target))
        }
        other => Err(unsupported(format!(
            "Node `wall-pattern` only supports shell-surface targets (`shell`, `extrude`, `revolve`, `taper`, `twist`). It does not support `{}`.",
            other
        ))),
    }
}

fn align_contour_sets(
    left: &ContourSet2d,
    right: &ContourSet2d,
    node: &str,
) -> AppResult<(ContourSet2d, ContourSet2d)> {
    if left.polygons.len() != right.polygons.len() {
        return Err(validation(format!(
            "Node `{}` needs contour sets with the same number of outer loops.",
            node
        )));
    }

    let mut aligned_left = Vec::with_capacity(left.polygons.len());
    let mut aligned_right = Vec::with_capacity(right.polygons.len());
    for (left_polygon, right_polygon) in left.polygons.iter().zip(&right.polygons) {
        if left_polygon.holes.len() != right_polygon.holes.len() {
            return Err(validation(format!(
                "Node `{}` needs matching hole topology between contours.",
                node
            )));
        }

        let outer_count = left_polygon.outer.len().max(right_polygon.outer.len());
        let mut left_holes = Vec::with_capacity(left_polygon.holes.len());
        let mut right_holes = Vec::with_capacity(right_polygon.holes.len());
        for (left_hole, right_hole) in left_polygon.holes.iter().zip(&right_polygon.holes) {
            let count = left_hole.len().max(right_hole.len());
            left_holes.push(resample_loop(left_hole, count));
            right_holes.push(resample_loop(right_hole, count));
        }

        aligned_left.push(ContourPolygon2d {
            outer: resample_loop(&left_polygon.outer, outer_count),
            holes: left_holes,
        });
        aligned_right.push(ContourPolygon2d {
            outer: resample_loop(&right_polygon.outer, outer_count),
            holes: right_holes,
        });
    }

    Ok((
        ContourSet2d {
            polygons: aligned_left,
        },
        ContourSet2d {
            polygons: aligned_right,
        },
    ))
}

fn append_cap_polygons(
    polygons: &mut Vec<IrPolygon<()>>,
    contours: &ContourSet2d,
    z: f64,
    flip: bool,
) {
    for polygon in &contours.polygons {
        let holes = polygon
            .holes
            .iter()
            .map(|hole| hole.as_slice())
            .collect::<Vec<_>>();
        for triangle in IrSketch::triangulate_2d(&polygon.outer, &holes) {
            let verts = triangle
                .into_iter()
                .map(|point| IrVertex::new(Point3::new(point.x, point.y, z), Vector3::zeros()))
                .collect::<Vec<_>>();
            let mut poly = IrPolygon::new(verts, None);
            if flip {
                poly.flip();
            }
            polygons.push(poly);
        }
    }
}

fn append_loop_side_polygons(
    polygons: &mut Vec<IrPolygon<()>>,
    bottom: &[[f64; 2]],
    bottom_z: f64,
    top: &[[f64; 2]],
    top_z: f64,
    flip: bool,
) {
    for index in 0..bottom.len() {
        let next = (index + 1) % bottom.len();
        let b0 = IrVertex::new(
            Point3::new(bottom[index][0], bottom[index][1], bottom_z),
            Vector3::zeros(),
        );
        let b1 = IrVertex::new(
            Point3::new(bottom[next][0], bottom[next][1], bottom_z),
            Vector3::zeros(),
        );
        let t1 = IrVertex::new(
            Point3::new(top[next][0], top[next][1], top_z),
            Vector3::zeros(),
        );
        let t0 = IrVertex::new(
            Point3::new(top[index][0], top[index][1], top_z),
            Vector3::zeros(),
        );
        let mut poly = IrPolygon::new(vec![b0, b1, t1, t0], None);
        if flip {
            poly.flip();
        }
        polygons.push(poly);
    }
}

fn append_contour_side_polygons(
    polygons: &mut Vec<IrPolygon<()>>,
    bottom: &ContourSet2d,
    bottom_z: f64,
    top: &ContourSet2d,
    top_z: f64,
    flip: bool,
) {
    for (bottom_polygon, top_polygon) in bottom.polygons.iter().zip(&top.polygons) {
        append_loop_side_polygons(
            polygons,
            &bottom_polygon.outer,
            bottom_z,
            &top_polygon.outer,
            top_z,
            flip,
        );
        for (bottom_hole, top_hole) in bottom_polygon.holes.iter().zip(&top_polygon.holes) {
            append_loop_side_polygons(polygons, bottom_hole, bottom_z, top_hole, top_z, !flip);
        }
    }
}

fn loft_between_contour_sets(
    bottom: &ContourSet2d,
    bottom_z: f64,
    top: &ContourSet2d,
    top_z: f64,
    node: &str,
) -> AppResult<IrMesh> {
    let (bottom_aligned, top_aligned) = align_contour_sets(bottom, top, node)?;
    let mut polygons = Vec::new();
    append_cap_polygons(&mut polygons, &bottom_aligned, bottom_z, true);
    append_cap_polygons(&mut polygons, &top_aligned, top_z, false);
    append_contour_side_polygons(
        &mut polygons,
        &bottom_aligned,
        bottom_z,
        &top_aligned,
        top_z,
        false,
    );
    if polygons.is_empty() {
        return Err(validation(format!("`{}` produced no geometry.", node)));
    }
    Ok(IrMesh::from_polygons(&polygons, None))
}

fn loft_between_sketches(
    bottom: &IrSketch,
    bottom_z: f64,
    top: &IrSketch,
    top_z: f64,
    node: &str,
) -> AppResult<IrMesh> {
    let bottom_contours = contours_from_sketch(bottom, node)?;
    let top_contours = contours_from_sketch(top, node)?;
    loft_between_contour_sets(&bottom_contours, bottom_z, &top_contours, top_z, node)
}

fn offset_sketch(
    sketch: &IrSketch,
    distance: f64,
    rounded: bool,
    node: &str,
) -> AppResult<IrSketch> {
    let shifted = if rounded {
        sketch.offset_rounded(distance)
    } else {
        sketch.offset(distance)
    };
    if shifted.to_multipolygon().0.is_empty() {
        return Err(validation(format!(
            "Node `{}` collapsed the sketch at offset distance {}.",
            node, distance
        )));
    }
    Ok(shifted)
}

fn loft_segments(mut slices: Vec<(IrSketch, f64)>, node: &str) -> AppResult<IrMesh> {
    if slices.len() < 2 {
        return Err(validation(format!(
            "Node `{}` needs at least two slices.",
            node
        )));
    }
    let (first_sketch, first_z) = slices.remove(0);
    let mut current_contours = contours_from_sketch(&first_sketch, node)?;
    let mut current_z = first_z;
    let mut polygons = Vec::new();

    for (next_sketch, next_z) in slices {
        let next_contours = contours_from_sketch(&next_sketch, node)?;
        let (aligned_current, aligned_next) =
            align_contour_sets(&current_contours, &next_contours, node)?;
        if polygons.is_empty() {
            append_cap_polygons(&mut polygons, &aligned_current, current_z, true);
        }
        append_contour_side_polygons(
            &mut polygons,
            &aligned_current,
            current_z,
            &aligned_next,
            next_z,
            false,
        );
        current_contours = aligned_next;
        current_z = next_z;
    }
    append_cap_polygons(&mut polygons, &current_contours, current_z, false);

    if polygons.is_empty() {
        return Err(validation(format!("Node `{}` produced no geometry.", node)));
    }
    Ok(IrMesh::from_polygons(&polygons, None))
}

fn contour_difference(
    outer: &ContourSet2d,
    inner: &ContourSet2d,
    node: &str,
) -> AppResult<ContourSet2d> {
    let ring = contour_set_to_sketch(outer).difference(&contour_set_to_sketch(inner));
    contours_from_sketch(&ring, node)
}

fn append_shell_cap_polygons(
    polygons: &mut Vec<IrPolygon<()>>,
    outer: &ContourSet2d,
    inner: &ContourSet2d,
    z: f64,
    flip: bool,
    node: &str,
) -> AppResult<()> {
    let cap = contour_difference(outer, inner, node)?;
    append_cap_polygons(polygons, &cap, z, flip);
    Ok(())
}

fn shell_from_contour_slices(
    mut outer_slices: Vec<(ContourSet2d, f64)>,
    mut inner_slices: Vec<(ContourSet2d, f64)>,
    node: &str,
) -> AppResult<IrMesh> {
    if outer_slices.len() < 2 || inner_slices.len() < 2 || outer_slices.len() != inner_slices.len()
    {
        return Err(validation(format!(
            "Node `{}` needs matching outer/inner slice stacks.",
            node
        )));
    }

    let (first_outer, first_z) = outer_slices.remove(0);
    let (first_inner, inner_first_z) = inner_slices.remove(0);
    if !approx_eq(first_z, inner_first_z) {
        return Err(validation(format!(
            "Node `{}` needs aligned outer/inner slice heights.",
            node
        )));
    }

    let mut current_outer = first_outer;
    let mut current_inner = first_inner;
    let mut current_z = first_z;
    let mut polygons = Vec::new();
    let mut start_outer = None;
    let mut start_inner = None;

    for ((next_outer, next_outer_z), (next_inner, next_inner_z)) in
        outer_slices.into_iter().zip(inner_slices.into_iter())
    {
        if !approx_eq(next_outer_z, next_inner_z) {
            return Err(validation(format!(
                "Node `{}` needs aligned outer/inner slice heights.",
                node
            )));
        }
        let (aligned_outer, aligned_next_outer) =
            align_contour_sets(&current_outer, &next_outer, node)?;
        let (aligned_inner, aligned_next_inner) =
            align_contour_sets(&current_inner, &next_inner, node)?;
        if start_outer.is_none() {
            start_outer = Some(aligned_outer.clone());
            start_inner = Some(aligned_inner.clone());
        }
        append_contour_side_polygons(
            &mut polygons,
            &aligned_outer,
            current_z,
            &aligned_next_outer,
            next_outer_z,
            false,
        );
        append_contour_side_polygons(
            &mut polygons,
            &aligned_inner,
            current_z,
            &aligned_next_inner,
            next_inner_z,
            true,
        );
        current_outer = aligned_next_outer;
        current_inner = aligned_next_inner;
        current_z = next_outer_z;
    }

    let start_outer = start_outer.unwrap_or_else(|| current_outer.clone());
    let start_inner = start_inner.unwrap_or_else(|| current_inner.clone());
    append_shell_cap_polygons(
        &mut polygons,
        &start_outer,
        &start_inner,
        first_z,
        true,
        node,
    )?;
    append_shell_cap_polygons(
        &mut polygons,
        &current_outer,
        &current_inner,
        current_z,
        false,
        node,
    )?;

    if polygons.is_empty() {
        return Err(validation(format!("Node `{}` produced no geometry.", node)));
    }
    Ok(IrMesh::from_polygons(&polygons, None))
}

fn revolve_mesh(sketch: &IrSketch, angle: f64, segments: usize, node: &str) -> AppResult<IrMesh> {
    sketch
        .clone()
        .rotate(90.0, 0.0, 0.0)
        .revolve(angle, segments.max(12))
        .map_err(|err| validation(format!("`{}` failed: {:?}", node, err)))
}

fn taper_mesh(
    sketch: &IrSketch,
    height: f64,
    scale_x: f64,
    scale_y: f64,
    node: &str,
) -> AppResult<IrMesh> {
    let top = sketch.scale(scale_x, scale_y, 1.0);
    loft_between_sketches(sketch, 0.0, &top, height, node)
}

fn twist_mesh(
    sketch: &IrSketch,
    height: f64,
    angle: f64,
    segments: usize,
    node: &str,
) -> AppResult<IrMesh> {
    let mut slices = Vec::with_capacity(segments + 1);
    for index in 0..=segments {
        let t = index as f64 / segments as f64;
        let z = height * t;
        let rotated = sketch.rotate(0.0, 0.0, angle * t);
        slices.push((rotated, z));
    }
    loft_segments(slices, node)
}

fn mirror_plane(axis: &str, offset: f64) -> AppResult<IrPlane> {
    match axis {
        "x" => Ok(IrPlane::from_normal(Vector3::x(), offset)),
        "y" => Ok(IrPlane::from_normal(Vector3::y(), offset)),
        "z" => Ok(IrPlane::from_normal(Vector3::z(), offset)),
        other => Err(validation(format!(
            "Unsupported mirror axis `{}`. Use `x`, `y`, or `z`.",
            other
        ))),
    }
}

fn append_cap_polygons_transformed(
    polygons: &mut Vec<IrPolygon<()>>,
    contours: &ContourSet2d,
    transform: &na::Isometry3<f64>,
    flip: bool,
) {
    for polygon in &contours.polygons {
        let holes = polygon
            .holes
            .iter()
            .map(|hole| hole.as_slice())
            .collect::<Vec<_>>();
        for triangle in IrSketch::triangulate_2d(&polygon.outer, &holes) {
            let verts = triangle
                .into_iter()
                .map(|point| {
                    IrVertex::new(
                        transform * Point3::new(point.x, point.y, 0.0),
                        Vector3::zeros(),
                    )
                })
                .collect::<Vec<_>>();
            let mut poly = IrPolygon::new(verts, None);
            if flip {
                poly.flip();
            }
            polygons.push(poly);
        }
    }
}

fn append_loop_side_polygons_transformed(
    polygons: &mut Vec<IrPolygon<()>>,
    bottom: &[[f64; 2]],
    bottom_transform: &na::Isometry3<f64>,
    top: &[[f64; 2]],
    top_transform: &na::Isometry3<f64>,
    flip: bool,
) {
    for index in 0..bottom.len() {
        let next = (index + 1) % bottom.len();
        let b0 = IrVertex::new(
            bottom_transform * Point3::new(bottom[index][0], bottom[index][1], 0.0),
            Vector3::zeros(),
        );
        let b1 = IrVertex::new(
            bottom_transform * Point3::new(bottom[next][0], bottom[next][1], 0.0),
            Vector3::zeros(),
        );
        let t1 = IrVertex::new(
            top_transform * Point3::new(top[next][0], top[next][1], 0.0),
            Vector3::zeros(),
        );
        let t0 = IrVertex::new(
            top_transform * Point3::new(top[index][0], top[index][1], 0.0),
            Vector3::zeros(),
        );
        let mut poly = IrPolygon::new(vec![b0, b1, t1, t0], None);
        if flip {
            poly.flip();
        }
        polygons.push(poly);
    }
}

fn append_contour_side_polygons_transformed(
    polygons: &mut Vec<IrPolygon<()>>,
    bottom: &ContourSet2d,
    bottom_transform: &na::Isometry3<f64>,
    top: &ContourSet2d,
    top_transform: &na::Isometry3<f64>,
    flip: bool,
) {
    for (bottom_polygon, top_polygon) in bottom.polygons.iter().zip(&top.polygons) {
        append_loop_side_polygons_transformed(
            polygons,
            &bottom_polygon.outer,
            bottom_transform,
            &top_polygon.outer,
            top_transform,
            flip,
        );
        for (bottom_hole, top_hole) in bottom_polygon.holes.iter().zip(&top_polygon.holes) {
            append_loop_side_polygons_transformed(
                polygons,
                bottom_hole,
                bottom_transform,
                top_hole,
                top_transform,
                !flip,
            );
        }
    }
}

fn loft_segments_transformed(
    mut slices: Vec<(ContourSet2d, na::Isometry3<f64>)>,
    node: &str,
) -> AppResult<IrMesh> {
    if slices.len() < 2 {
        return Err(validation(format!(
            "Node `{}` needs at least two slices.",
            node
        )));
    }
    let (first_contours, first_transform) = slices.remove(0);
    let mut current_contours = first_contours;
    let mut current_transform = first_transform;
    let mut polygons = Vec::new();

    for (next_contours, next_transform) in slices {
        let (aligned_current, aligned_next) =
            align_contour_sets(&current_contours, &next_contours, node)?;
        if polygons.is_empty() {
            append_cap_polygons_transformed(
                &mut polygons,
                &aligned_current,
                &current_transform,
                true,
            );
        }
        append_contour_side_polygons_transformed(
            &mut polygons,
            &aligned_current,
            &current_transform,
            &aligned_next,
            &next_transform,
            false,
        );
        current_contours = aligned_next;
        current_transform = next_transform;
    }
    append_cap_polygons_transformed(&mut polygons, &current_contours, &current_transform, false);

    if polygons.is_empty() {
        return Err(validation(format!("Node `{}` produced no geometry.", node)));
    }
    Ok(IrMesh::from_polygons(&polygons, None))
}

fn sweep_mesh(sketch: &IrSketch, path: &[([f64; 3], [f64; 3])], node: &str) -> AppResult<IrMesh> {
    if path.len() < 2 {
        return Err(validation(format!(
            "`{}` expects a path with at least two points.",
            node
        )));
    }
    let contours = contours_from_sketch(sketch, node)?;
    let mut slices = Vec::with_capacity(path.len());

    for (p, t) in path {
        let point = Point3::from(*p);
        let tangent_vec = Vector3::from(*t).normalize();

        let rotation = if tangent_vec.dot(&Vector3::z()).abs() > 0.999 {
            if tangent_vec.z > 0.0 {
                na::Rotation3::identity()
            } else {
                na::Rotation3::from_axis_angle(&Vector3::x_axis(), std::f64::consts::PI)
            }
        } else {
            na::Rotation3::rotation_between(&Vector3::z(), &tangent_vec)
                .unwrap_or_else(na::Rotation3::identity)
        };

        let transform = na::Isometry3::from_parts(point.into(), rotation.into());
        slices.push((contours.clone(), transform));
    }

    loft_segments_transformed(slices, node)
}

fn eval_shell_geometry(
    target: &Value,
    wall: f64,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<IrMesh> {
    if wall <= 0.0 {
        return Err(validation("`shell` expects a positive wall thickness."));
    }
    let items = list_items(target, "shell target")?;
    let node = head_symbol(&items, "shell target")?;
    let args = &items[1..];

    match node {
        "cylinder" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(validation("`shell` cylinder expects radius, height, and optional segments."));
            }
            let radius = eval_number(&args[0], env)?;
            let height = eval_number(&args[1], env)?;
            let inner_radius = radius - wall;
            if inner_radius <= 0.0 {
                return Err(validation("`shell` cylinder needs wall thickness smaller than radius."));
            }
            let segments = args
                .get(2)
                .map(|arg| parse_count(arg, env, "shell cylinder segments", 12))
                .transpose()?
                .unwrap_or(48usize);
            let outer = IrMesh::cylinder(radius, height, segments.max(12), None);
            let inner = IrMesh::cylinder(inner_radius, height, segments.max(12), None);
            Ok(outer.difference(&inner))
        }
        "cone" => {
            if args.len() < 3 || args.len() > 4 {
                return Err(validation(
                    "`shell` cone expects bottom radius, top radius, height, and optional segments.",
                ));
            }
            let bottom_radius = eval_number(&args[0], env)?;
            let top_radius = eval_number(&args[1], env)?;
            let height = eval_number(&args[2], env)?;
            let inner_bottom = bottom_radius - wall;
            let inner_top = top_radius - wall;
            if inner_bottom <= 0.0 || inner_top <= 0.0 {
                return Err(validation("`shell` cone needs wall thickness smaller than both radii."));
            }
            let segments = args
                .get(3)
                .map(|arg| parse_count(arg, env, "shell cone segments", 12))
                .transpose()?
                .unwrap_or(48usize);
            let outer = IrMesh::frustum(bottom_radius, top_radius, height, segments.max(12), None);
            let inner = IrMesh::frustum(inner_bottom, inner_top, height, segments.max(12), None);
            Ok(outer.difference(&inner))
        }
        "sphere" => {
            if args.is_empty() || args.len() > 3 {
                return Err(validation("`shell` sphere expects radius and optional slices/stacks."));
            }
            let radius = eval_number(&args[0], env)?;
            let inner_radius = radius - wall;
            if inner_radius <= 0.0 {
                return Err(validation("`shell` sphere needs wall thickness smaller than radius."));
            }
            let slices = args
                .get(1)
                .map(|arg| parse_count(arg, env, "shell sphere slices", 12))
                .transpose()?
                .unwrap_or(48usize);
            let stacks = args
                .get(2)
                .map(|arg| parse_count(arg, env, "shell sphere stacks", 6))
                .transpose()?
                .unwrap_or(24usize);
            let outer = IrMesh::sphere(radius, slices.max(12), stacks.max(6), None);
            let inner = IrMesh::sphere(inner_radius, slices.max(12), stacks.max(6), None);
            Ok(outer.difference(&inner))
        }
        "extrude" => {
            if args.len() != 2 {
                return Err(validation("`shell` extrude expects a sketch and height."));
            }
            let sketch = eval_geometry(&args[0], env)?.into_sketch("shell")?;
            let height = eval_number(&args[1], env)?;
            let inner_sketch = offset_sketch(&sketch, -wall, false, "shell")?;
            Ok(sketch.extrude(height).difference(&inner_sketch.extrude(height)))
        }
        "revolve" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(validation("`shell` revolve expects a sketch, angle, and optional segments."));
            }
            let sketch = eval_geometry(&args[0], env)?.into_sketch("shell")?;
            let angle = eval_number(&args[1], env)?;
            let segments = args
                .get(2)
                .map(|arg| parse_count(arg, env, "shell revolve segments", 12))
                .transpose()?
                .unwrap_or(48usize);
            let inner_sketch = offset_sketch(&sketch, -wall, false, "shell")?;
            Ok(revolve_mesh(&sketch, angle, segments, "shell")?
                .difference(&revolve_mesh(&inner_sketch, angle, segments, "shell")?))
        }
        "loft" => {
            if args.len() != 3 {
                return Err(validation("`shell` loft expects height, bottom sketch, and top sketch."));
            }
            let height = eval_number(&args[0], env)?;
            let bottom = eval_geometry(&args[1], env)?.into_sketch("shell")?;
            let top = eval_geometry(&args[2], env)?.into_sketch("shell")?;
            let inner_bottom = offset_sketch(&bottom, -wall, false, "shell")?;
            let inner_top = offset_sketch(&top, -wall, false, "shell")?;
            Ok(shell_from_contour_slices(
                vec![
                    (contours_from_sketch(&bottom, "shell")?, 0.0),
                    (contours_from_sketch(&top, "shell")?, height),
                ],
                vec![
                    (contours_from_sketch(&inner_bottom, "shell")?, 0.0),
                    (contours_from_sketch(&inner_top, "shell")?, height),
                ],
                "shell",
            )?)
        }
        "taper" => {
            if !(args.len() == 3 || args.len() == 4) {
                return Err(validation(
                    "`shell` taper expects height, scale, sketch or height, scale-x, scale-y, sketch.",
                ));
            }
            let height = eval_number(&args[0], env)?;
            let (scale_x, scale_y, sketch_index) = if args.len() == 3 {
                let scale = eval_number(&args[1], env)?;
                (scale, scale, 2usize)
            } else {
                (
                    eval_number(&args[1], env)?,
                    eval_number(&args[2], env)?,
                    3usize,
                )
            };
            if scale_x <= 0.0 || scale_y <= 0.0 {
                return Err(validation("`shell` taper requires positive scale factors."));
            }
            let base = eval_geometry(&args[sketch_index], env)?.into_sketch("shell")?;
            let inner_base = offset_sketch(&base, -wall, false, "shell")?;
            let top = base.scale(scale_x, scale_y, 1.0);
            let inner_top = inner_base.scale(scale_x, scale_y, 1.0);
            Ok(shell_from_contour_slices(
                vec![
                    (contours_from_sketch(&base, "shell")?, 0.0),
                    (contours_from_sketch(&top, "shell")?, height),
                ],
                vec![
                    (contours_from_sketch(&inner_base, "shell")?, 0.0),
                    (contours_from_sketch(&inner_top, "shell")?, height),
                ],
                "shell",
            )?)
        }
        "twist" => {
            if !(args.len() == 3 || args.len() == 4) {
                return Err(validation(
                    "`shell` twist expects height, angle, sketch or height, angle, segments, sketch.",
                ));
            }
            let height = eval_number(&args[0], env)?;
            let angle = eval_number(&args[1], env)?;
            let (segments, sketch_index) = if args.len() == 3 {
                (12usize, 2usize)
            } else {
                (parse_count(&args[2], env, "shell twist segments", 1)?, 3usize)
            };
            let base = eval_geometry(&args[sketch_index], env)?.into_sketch("shell")?;
            let inner_base = offset_sketch(&base, -wall, false, "shell")?;
            let mut outer_slices = Vec::with_capacity(segments + 1);
            let mut inner_slices = Vec::with_capacity(segments + 1);
            for index in 0..=segments {
                let t = index as f64 / segments as f64;
                let z = height * t;
                outer_slices.push((
                    contours_from_sketch(&base.rotate(0.0, 0.0, angle * t), "shell")?,
                    z,
                ));
                inner_slices.push((
                    contours_from_sketch(&inner_base.rotate(0.0, 0.0, angle * t), "shell")?,
                    z,
                ));
            }
            Ok(shell_from_contour_slices(
                outer_slices,
                inner_slices,
                "shell",
            )?)
        }
        "sweep" => {
            if args.len() != 2 {
                return Err(validation("`shell` sweep expects a sketch and a path."));
            }
            let sketch = eval_geometry(&args[0], env)?.into_sketch("shell")?;
            let path = eval_geometry(&args[1], env)?.into_path("shell")?;
            let inner_sketch = offset_sketch(&sketch, -wall, false, "shell")?;
            Ok(sweep_mesh(&sketch, &path, "shell")?
                .difference(&sweep_mesh(&inner_sketch, &path, "shell")?))
        }
        other => Err(unsupported(format!(
            "Node `shell` currently supports cylinder, cone, sphere, extrude, revolve, loft, taper, twist, and sweep. It does not support `{}` yet.",
            other
        ))),
    }
}

fn eval_geometry(value: &Value, env: &BTreeMap<String, ParamValue>) -> AppResult<Geometry> {
    let items = list_items(value, "geometry node")?;
    let node = head_symbol(&items, "geometry node")?;
    let args = &items[1..];
    match node {
        "box" => {
            if args.len() != 3 {
                return Err(validation("`box` expects width, depth, and height."));
            }
            let width = eval_number(&args[0], env)?;
            let depth = eval_number(&args[1], env)?;
            let height = eval_number(&args[2], env)?;
            Ok(Geometry::Mesh(
                Sketch::square(1.0, None)
                    .scale(width, depth, 1.0)
                    .extrude(height),
            ))
        }
        "cylinder" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(validation("`cylinder` expects radius, height, and optional segments."));
            }
            let radius = eval_number(&args[0], env)?;
            let height = eval_number(&args[1], env)?;
            let segments = args.get(2).map(|arg| eval_number(arg, env)).transpose()?.unwrap_or(48.0) as usize;
            Ok(Geometry::Mesh(IrMesh::cylinder(radius, height, segments.max(12), None)))
        }
        "cone" => {
            if args.len() < 3 || args.len() > 4 {
                return Err(validation(
                    "`cone` expects bottom radius, top radius, height, and optional segments.",
                ));
            }
            let bottom_radius = eval_number(&args[0], env)?;
            let top_radius = eval_number(&args[1], env)?;
            let height = eval_number(&args[2], env)?;
            let segments = args.get(3).map(|arg| eval_number(arg, env)).transpose()?.unwrap_or(48.0) as usize;
            Ok(Geometry::Mesh(IrMesh::frustum(
                bottom_radius,
                top_radius,
                height,
                segments.max(12),
                None,
            )))
        }
        "sphere" => {
            if args.is_empty() || args.len() > 3 {
                return Err(validation("`sphere` expects radius and optional slices/stacks."));
            }
            let radius = eval_number(&args[0], env)?;
            let slices = args.get(1).map(|arg| eval_number(arg, env)).transpose()?.unwrap_or(48.0) as usize;
            let stacks = args.get(2).map(|arg| eval_number(arg, env)).transpose()?.unwrap_or(24.0) as usize;
            Ok(Geometry::Mesh(IrMesh::sphere(
                radius,
                slices.max(12),
                stacks.max(6),
                None,
            )))
        }
        "circle" => {
            if args.is_empty() || args.len() > 2 {
                return Err(validation("`circle` expects radius and optional segments."));
            }
            let radius = eval_number(&args[0], env)?;
            let segments = args
                .get(1)
                .map(|arg| eval_number(arg, env))
                .transpose()?
                .unwrap_or(48.0) as usize;
            Ok(Geometry::Sketch(IrSketch::polygon(
                &circle_points(radius, segments.max(3)),
                None,
            )))
        }
        "rounded_rect" | "rounded-rect" => {
            if args.len() < 3 || args.len() > 4 {
                return Err(validation(
                    "`rounded_rect` expects width, height, corner radius, and optional segments.",
                ));
            }
            let width = eval_number(&args[0], env)?;
            let height = eval_number(&args[1], env)?;
            let corner_radius = eval_number(&args[2], env)?;
            let segments = args
                .get(3)
                .map(|arg| eval_number(arg, env))
                .transpose()?
                .unwrap_or(12.0) as usize;
            Ok(Geometry::Sketch(IrSketch::polygon(
                &rounded_rectangle_points(width, height, corner_radius, segments.max(2)),
                None,
            )))
        }
        "polygon" => {
            if args.len() != 1 {
                return Err(validation("`polygon` expects a single point list."));
            }
            Ok(Geometry::Sketch(IrSketch::polygon(
                &eval_points(&args[0], env)?,
                None,
            )))
        }
        "profile" => Ok(Geometry::Sketch(parse_profile_sketch(args, env)?)),
        "rounded-polygon" | "rounded_polygon" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(validation(
                    "`rounded-polygon` expects point list, corner radius, and optional segments.",
                ));
            }
            let points = eval_points(&args[0], env)?;
            let radius = eval_number(&args[1], env)?;
            let segments = args
                .get(2)
                .map(|arg| parse_count(arg, env, "rounded-polygon segments", 2))
                .transpose()?
                .unwrap_or(8usize);
            Ok(Geometry::Sketch(IrSketch::polygon(
                &rounded_polygon_points(&points, radius, segments)?,
                None,
            )))
        }
        "bspline" => {
            if args.is_empty() || args.len() > 3 {
                return Err(validation(
                    "`bspline` expects point list, optional closed flag, and optional samples.",
                ));
            }
            let points = eval_points(&args[0], env)?;
            let closed = args
                .get(1)
                .map(|arg| eval_bool(arg, env))
                .transpose()?
                .unwrap_or(true);
            let samples = args
                .get(2)
                .map(|arg| parse_count(arg, env, "bspline samples", 1))
                .transpose()?
                .unwrap_or(12usize);
            Ok(Geometry::Sketch(IrSketch::polygon(
                &bspline_points(&points, samples, closed)?,
                None,
            )))
        }
        "offset" | "offset-rounded" => {
            if args.len() != 2 {
                return Err(validation(format!(
                    "`{}` expects distance and a sketch.",
                    node
                )));
            }
            let distance = eval_number(&args[0], env)?;
            let sketch = eval_geometry(&args[1], env)?.into_sketch(node)?;
            Ok(Geometry::Sketch(offset_sketch(
                &sketch,
                distance,
                node == "offset-rounded",
                node,
            )?))
        }
        "extrude" => {
            if args.len() != 2 {
                return Err(validation("`extrude` expects a sketch and height."));
            }
            let sketch = eval_geometry(&args[0], env)?.into_sketch("extrude")?;
            Ok(Geometry::Mesh(sketch.extrude(eval_number(&args[1], env)?)))
        }
        "revolve" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(validation("`revolve` expects a sketch, angle, and optional segments."));
            }
            let sketch = eval_geometry(&args[0], env)?.into_sketch("revolve")?;
            let angle = eval_number(&args[1], env)?;
            let segments = args
                .get(2)
                .map(|arg| parse_count(arg, env, "revolve segments", 12))
                .transpose()?
                .unwrap_or(48usize);
            Ok(Geometry::Mesh(revolve_mesh(&sketch, angle, segments, "revolve")?))
        }
        "loft" => {
            if args.len() != 3 {
                return Err(validation(
                    "`loft` expects height, bottom sketch, and top sketch.",
                ));
            }
            let height = eval_number(&args[0], env)?;
            let bottom = eval_geometry(&args[1], env)?.into_sketch("loft")?;
            let top = eval_geometry(&args[2], env)?.into_sketch("loft")?;
            Ok(Geometry::Mesh(loft_between_sketches(&bottom, 0.0, &top, height, "loft")?))
        }
        "taper" => {
            if !(args.len() == 3 || args.len() == 4) {
                return Err(validation(
                    "`taper` expects height, scale, sketch or height, scale-x, scale-y, sketch.",
                ));
            }
            let height = eval_number(&args[0], env)?;
            let (scale_x, scale_y, sketch_index) = if args.len() == 3 {
                let scale = eval_number(&args[1], env)?;
                (scale, scale, 2usize)
            } else {
                (
                    eval_number(&args[1], env)?,
                    eval_number(&args[2], env)?,
                    3usize,
                )
            };
            let base = eval_geometry(&args[sketch_index], env)?.into_sketch("taper")?;
            Ok(Geometry::Mesh(taper_mesh(&base, height, scale_x, scale_y, "taper")?))
        }
        "twist" => {
            if !(args.len() == 3 || args.len() == 4) {
                return Err(validation(
                    "`twist` expects height, angle, sketch or height, angle, segments, sketch.",
                ));
            }
            let height = eval_number(&args[0], env)?;
            let angle = eval_number(&args[1], env)?;
            let (segments, sketch_index) = if args.len() == 3 {
                (12usize, 2usize)
            } else {
                (parse_count(&args[2], env, "twist segments", 1)?, 3usize)
            };
            let base = eval_geometry(&args[sketch_index], env)?.into_sketch("twist")?;
            Ok(Geometry::Mesh(twist_mesh(
                &base, height, angle, segments, "twist",
            )?))
        }
        "sweep" => {
            if args.len() != 2 {
                return Err(validation("`sweep` expects a sketch and a path."));
            }
            let sketch = eval_geometry(&args[0], env)?.into_sketch("sweep")?;
            let path = eval_geometry(&args[1], env)?.into_path("sweep")?;
            Ok(Geometry::Mesh(sweep_mesh(&sketch, &path, "sweep")?))
        }
        "path" => {
            let mut points = Vec::with_capacity(args.len());
            for arg in args {
                let triple = list_items(arg, "3D point")?;
                if triple.len() != 3 {
                    return Err(validation("3D points must be `(x y z)` triples."));
                }
                points.push([
                    eval_number(&triple[0], env)?,
                    eval_number(&triple[1], env)?,
                    eval_number(&triple[2], env)?,
                ]);
            }
            if points.len() < 2 {
                return Err(validation("`path` expects at least two points."));
            }
            let mut path = Vec::with_capacity(points.len());
            for i in 0..points.len() {
                let tangent = if i < points.len() - 1 {
                    [
                        points[i + 1][0] - points[i][0],
                        points[i + 1][1] - points[i][1],
                        points[i + 1][2] - points[i][2],
                    ]
                } else {
                    [
                        points[i][0] - points[i - 1][0],
                        points[i][1] - points[i - 1][1],
                        points[i][2] - points[i - 1][2],
                    ]
                };
                path.push((points[i], tangent));
            }
            Ok(Geometry::Path(path))
        }
        "bezier-path" => {
            if args.is_empty() {
                return Err(validation("`bezier-path` expects points and optional segments."));
            }
            let points = eval_points_3d(&args[0], env)?;
            let segments = if args.len() > 1 {
                parse_count(&args[1], env, "bezier-path segments", 1)?
            } else {
                12usize
            };
            Ok(Geometry::Path(sample_bezier_path(&points, segments)?))
        }
        "shell" => {
            if args.len() != 2 {
                return Err(validation("`shell` expects wall thickness and a supported solid node."));
            }
            let wall = eval_number(&args[0], env)?;
            Ok(Geometry::Mesh(eval_shell_geometry(&args[1], wall, env)?))
        }
        "union" => fold_boolean_geometry(
            "union",
            args,
            env,
            |left, right| left.union(&right),
            |left, right| left.union(&right),
        ),
        "difference" => fold_boolean_geometry(
            "difference",
            args,
            env,
            |left, right| left.difference(&right),
            |left, right| left.difference(&right),
        ),
        "intersection" => fold_boolean_geometry(
            "intersection",
            args,
            env,
            |left, right| left.intersection(&right),
            |left, right| left.intersection(&right),
        ),
        "xor" => fold_boolean_geometry(
            "xor",
            args,
            env,
            |left, right| left.xor(&right),
            |left, right| left.xor(&right),
        ),
        "translate" => transform_mesh_or_sketch("translate", args, env, |shape, x, y, z| match shape {
            Geometry::Mesh(mesh) => Geometry::Mesh(mesh.translate(x, y, z)),
            Geometry::Sketch(sketch) => Geometry::Sketch(sketch.translate(x, y, z)),
            Geometry::Path(path) => Geometry::Path(
                path.into_iter()
                    .map(|(p, t)| ([p[0] + x, p[1] + y, p[2] + z], t))
                    .collect(),
            ),
        }),
        "rotate" => transform_mesh_or_sketch("rotate", args, env, |shape, x, y, z| match shape {
            Geometry::Mesh(mesh) => Geometry::Mesh(mesh.rotate(x, y, z)),
            Geometry::Sketch(sketch) => Geometry::Sketch(sketch.rotate(x, y, z)),
            Geometry::Path(path) => {
                let rot =
                    na::Rotation3::from_euler_angles(x.to_radians(), y.to_radians(), z.to_radians());
                Geometry::Path(
                    path.into_iter()
                        .map(|(p, t)| {
                            (
                                (rot * Point3::from(p)).into(),
                                (rot * Vector3::from(t)).into(),
                            )
                        })
                        .collect(),
                )
            }
        }),
        "scale" => transform_mesh_or_sketch("scale", args, env, |shape, x, y, z| match shape {
            Geometry::Mesh(mesh) => Geometry::Mesh(mesh.scale(x, y, z)),
            Geometry::Sketch(sketch) => Geometry::Sketch(sketch.scale(x, y, z)),
            Geometry::Path(path) => Geometry::Path(
                path.into_iter()
                    .map(|(p, t)| {
                        (
                            [p[0] * x, p[1] * y, p[2] * z],
                            [t[0] * x, t[1] * y, t[2] * z],
                        )
                    })
                    .collect(),
            ),
        }),
        "mirror" => {
            if args.len() != 3 {
                return Err(validation("`mirror` expects axis, offset, and a geometry node."));
            }
            let axis = parse_stringish(&args[0], "mirror axis")?;
            let offset = eval_number(&args[1], env)?;
            let plane = mirror_plane(axis.as_str(), offset)?;
            Ok(match eval_geometry(&args[2], env)? {
                Geometry::Mesh(mesh) => Geometry::Mesh(mesh.mirror(plane)),
                Geometry::Sketch(sketch) => Geometry::Sketch(sketch.mirror(plane)),
                Geometry::Path(path) => Geometry::Path(
                    path.into_iter()
                        .map(|(p, t)| {
                            let pt = Point3::from(p);
                            let tv = Vector3::from(t);
                            let normal = plane.normal();
                            let dist = normal.dot(&pt.coords) - plane.offset();
                            let t_dist = normal.dot(&tv);
                            (
                                (pt - 2.0 * dist * normal).into(),
                                (tv - 2.0 * t_dist * normal).into(),
                            )
                        })
                        .collect(),
                ),
            })
        }
        "linear-array" => {
            if args.len() != 5 {
                return Err(validation("`linear-array` expects count, dx, dy, dz, and a mesh."));
            }
            let count = parse_count(&args[0], env, "linear-array count", 1)?;
            let dx = eval_number(&args[1], env)?;
            let dy = eval_number(&args[2], env)?;
            let dz = eval_number(&args[3], env)?;
            let base = eval_geometry(&args[4], env)?.into_mesh("linear-array")?;
            let mut acc = base.clone();
            for index in 1..count {
                acc = acc.union(&base.clone().translate(dx * index as f64, dy * index as f64, dz * index as f64));
            }
            Ok(Geometry::Mesh(acc))
        }
        "grid-array" => {
            if args.len() != 5 {
                return Err(validation(
                    "`grid-array` expects rows, cols, dx, dy, and a mesh.",
                ));
            }
            let rows = parse_count(&args[0], env, "grid-array rows", 1)?;
            let cols = parse_count(&args[1], env, "grid-array cols", 1)?;
            let dx = eval_number(&args[2], env)?;
            let dy = eval_number(&args[3], env)?;
            let base = eval_geometry(&args[4], env)?.into_mesh("grid-array")?;
            Ok(Geometry::Mesh(base.distribute_grid(rows, cols, dx, dy)))
        }
        "radial-array" => {
            if args.len() != 4 {
                return Err(validation(
                    "`radial-array` expects count, step degrees, radius, and a mesh.",
                ));
            }
            let count = parse_count(&args[0], env, "radial-array count", 1)?;
            let step_degrees = eval_number(&args[1], env)?;
            let radius = eval_number(&args[2], env)?;
            let base = eval_geometry(&args[3], env)?.into_mesh("radial-array")?;
            let translated = base.translate(radius, 0.0, 0.0);
            let mut acc = translated.clone();
            for index in 1..count {
                acc = acc.union(&translated.clone().rotate(0.0, 0.0, step_degrees * index as f64));
            }
            Ok(Geometry::Mesh(acc))
        }
        "arc-array" => {
            if args.len() != 5 {
                return Err(validation(
                    "`arc-array` expects count, radius, start degrees, end degrees, and a mesh.",
                ));
            }
            let count = parse_count(&args[0], env, "arc-array count", 1)?;
            let radius = eval_number(&args[1], env)?;
            let start_degrees = eval_number(&args[2], env)?;
            let end_degrees = eval_number(&args[3], env)?;
            let base = eval_geometry(&args[4], env)?.into_mesh("arc-array")?;
            Ok(Geometry::Mesh(
                base.distribute_arc(count, radius, start_degrees, end_degrees),
            ))
        }
        "if" => {
            if args.len() != 3 {
                return Err(validation("`if` expects condition, then-shape, else-shape."));
            }
            if eval_bool(&args[0], env)? {
                eval_geometry(&args[1], env)
            } else {
                eval_geometry(&args[2], env)
            }
        }
        "wall-pattern" | "pattern" => {
            if args.len() != 2 {
                return Err(validation(
                    "`wall-pattern` expects an option list and a supported shell-surface target.",
                ));
            }
            let spec = parse_wall_pattern_spec(&args[0], env)?;
            let (mesh, target) = build_wall_pattern_target(&args[1], env)?;
            Ok(Geometry::Mesh(apply_wall_pattern(&mesh, &target, &spec)?))
        }
        "chamfer" => {
            if args.len() < 2 {
                return Err(validation("`chamfer` expects distance and a geometry node."));
            }
            let distance = eval_number(&args[0], env)?;
            let (selector, body_index) = parse_edge_selector(args, env)?;
            if body_index >= args.len() {
                return Err(validation("`chamfer` is missing the geometry body argument."));
            }
            let mesh = eval_geometry(&args[body_index], env)?.into_mesh("chamfer")?;
            Ok(Geometry::Mesh(chamfer_mesh(&mesh, distance, selector)?))
        }
        "fillet" => {
            if args.len() < 2 {
                return Err(validation("`fillet` expects radius and a geometry node."));
            }
            let radius = eval_number(&args[0], env)?;
            let (selector, body_index) = parse_edge_selector(args, env)?;
            if body_index >= args.len() {
                return Err(validation("`fillet` is missing the geometry body argument."));
            }
            let mesh = eval_geometry(&args[body_index], env)?.into_mesh("fillet")?;
            Ok(Geometry::Mesh(fillet_mesh(&mesh, radius, selector)?))
        }
        "lithophane" => Err(unsupported(
            "Ecky IR v0 does not use a `lithophane` source node. Generate the geometry in IR and drive lithophane through postProcessing.lithophaneAttachments / the LITHO tab instead.",
        )),
        other => Err(unsupported(format!(
            "Node `{}` is not supported by Ecky IR v0.",
            other
        ))),
    }
}

// ---------------------------------------------------------------------------
// Chamfer / Fillet
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeSelector {
    All,
    Top,
    Bottom,
    Vertical,
}

/// A feature edge between two triangles, identified by canonical vertex indices.
#[derive(Debug, Clone)]
struct FeatureEdge {
    /// Canonical vertex indices (a < b).
    vi_a: usize,
    vi_b: usize,
    /// Positions of the two endpoints.
    pos_a: Point3<f64>,
    pos_b: Point3<f64>,
    /// Normals of the two adjacent faces.
    normal_left: Vector3<f64>,
    normal_right: Vector3<f64>,
    /// Dihedral angle in radians (0 = coplanar, π = folded back).
    /// Used by fillet (Phase 2) to scale the arc profile.
    #[allow(dead_code)]
    dihedral: f64,
}

const FEATURE_EDGE_DIHEDRAL_THRESHOLD: f64 = 0.35; // ~20 degrees

fn detect_feature_edges(mesh: &IrMesh) -> Vec<FeatureEdge> {
    let tri_mesh = mesh.triangulate();
    let polygons = &tri_mesh.polygons;

    // Build vertex index map for epsilon-based deduplication.
    let mut vertex_map = csgrs::mesh::connectivity::VertexIndexMap::new(1e-9);
    for poly in polygons {
        for v in &poly.vertices {
            vertex_map.get_or_create_index(v.pos);
        }
    }

    // Map each canonical edge to the (up to two) polygon indices sharing it.
    let mut edge_faces: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (poly_idx, poly) in polygons.iter().enumerate() {
        let verts: Vec<usize> = poly
            .vertices
            .iter()
            .map(|v| vertex_map.get_or_create_index(v.pos))
            .collect();
        for i in 0..verts.len() {
            let a = verts[i];
            let b = verts[(i + 1) % verts.len()];
            let key = if a < b { (a, b) } else { (b, a) };
            edge_faces.entry(key).or_default().push(poly_idx);
        }
    }

    let mut result = Vec::new();
    for ((vi_a, vi_b), faces) in &edge_faces {
        if faces.len() != 2 {
            continue; // Non-manifold or boundary edge — skip.
        }
        let n1 = polygons[faces[0]].plane.normal();
        let n2 = polygons[faces[1]].plane.normal();
        let dot = n1.dot(&n2).clamp(-1.0, 1.0);
        let dihedral = dot.acos();
        if dihedral < FEATURE_EDGE_DIHEDRAL_THRESHOLD {
            continue; // Nearly coplanar — not a feature edge.
        }
        let pos_a = vertex_map
            .get_position(*vi_a)
            .expect("vertex index must exist");
        let pos_b = vertex_map
            .get_position(*vi_b)
            .expect("vertex index must exist");
        result.push(FeatureEdge {
            vi_a: *vi_a,
            vi_b: *vi_b,
            pos_a,
            pos_b,
            normal_left: n1,
            normal_right: n2,
            dihedral,
        });
    }
    result
}

fn filter_edges(edges: &[FeatureEdge], selector: EdgeSelector) -> Vec<&FeatureEdge> {
    match selector {
        EdgeSelector::All => edges.iter().collect(),
        EdgeSelector::Top => {
            let max_z = edges
                .iter()
                .map(|e| e.pos_a.z.max(e.pos_b.z))
                .fold(f64::NEG_INFINITY, f64::max);
            let threshold = max_z - 1e-6;
            edges
                .iter()
                .filter(|e| {
                    let mid_z = (e.pos_a.z + e.pos_b.z) * 0.5;
                    mid_z >= threshold
                })
                .collect()
        }
        EdgeSelector::Bottom => {
            let min_z = edges
                .iter()
                .map(|e| e.pos_a.z.min(e.pos_b.z))
                .fold(f64::INFINITY, f64::min);
            let threshold = min_z + 1e-6;
            edges
                .iter()
                .filter(|e| {
                    let mid_z = (e.pos_a.z + e.pos_b.z) * 0.5;
                    mid_z <= threshold
                })
                .collect()
        }
        EdgeSelector::Vertical => edges
            .iter()
            .filter(|e| {
                let dir = (e.pos_b - e.pos_a).normalize();
                dir.z.abs() > 0.95
            })
            .collect(),
    }
}

fn chamfer_mesh(mesh: &IrMesh, distance: f64, selector: EdgeSelector) -> AppResult<IrMesh> {
    if distance.abs() < 1e-9 {
        return Ok(mesh.clone());
    }
    let tri_mesh = mesh.triangulate();
    let all_edges = detect_feature_edges(&tri_mesh);
    let selected = filter_edges(&all_edges, selector);
    if selected.is_empty() {
        return Ok(mesh.clone());
    }

    // Build vertex index map matching the one used in detect_feature_edges.
    let polygons = &tri_mesh.polygons;
    let mut vertex_map = csgrs::mesh::connectivity::VertexIndexMap::new(1e-9);
    for poly in polygons {
        for v in &poly.vertices {
            vertex_map.get_or_create_index(v.pos);
        }
    }

    // Collect the set of selected edge keys.
    let selected_keys: std::collections::HashSet<(usize, usize)> =
        selected.iter().map(|e| (e.vi_a, e.vi_b)).collect();

    // For each selected edge, build a cutting plane that bevels the edge.
    // Strategy: for each triangle, if it has an edge in the selected set,
    // inset the edge by `distance` along the face plane and produce a chamfer
    // strip connecting the two faces.

    // Build edge → face normal pairs for selected edges.
    let mut edge_normals: HashMap<(usize, usize), (Vector3<f64>, Vector3<f64>)> = HashMap::new();
    for e in &selected {
        edge_normals.insert((e.vi_a, e.vi_b), (e.normal_left, e.normal_right));
    }

    // For each polygon, find which of its edges are selected and split accordingly.
    let mut out_polygons: Vec<IrPolygon<()>> = Vec::new();
    let mut chamfer_strips: Vec<[Point3<f64>; 4]> = Vec::new();

    // Track which polygon index was "left" or "right" for each edge so we
    // can assign inset directions consistently.
    let mut edge_face_sides: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (poly_idx, poly) in polygons.iter().enumerate() {
        let verts: Vec<usize> = poly
            .vertices
            .iter()
            .map(|v| vertex_map.get_or_create_index(v.pos))
            .collect();
        for i in 0..verts.len() {
            let a = verts[i];
            let b = verts[(i + 1) % verts.len()];
            let key = if a < b { (a, b) } else { (b, a) };
            if selected_keys.contains(&key) {
                edge_face_sides.entry(key).or_default().push(poly_idx);
            }
        }
    }

    // For each polygon, check if any of its edges are chamfered.
    // If so, inset those edge vertices along the face plane.
    for (poly_idx, poly) in polygons.iter().enumerate() {
        let face_normal = poly.plane.normal();
        let verts: Vec<usize> = poly
            .vertices
            .iter()
            .map(|v| vertex_map.get_or_create_index(v.pos))
            .collect();

        let mut has_chamfer_edge = false;
        let mut new_vertices: Vec<IrVertex> = Vec::new();

        for i in 0..verts.len() {
            let a = verts[i];
            let b = verts[(i + 1) % verts.len()];
            let key = if a < b { (a, b) } else { (b, a) };

            new_vertices.push(poly.vertices[i].clone());

            if selected_keys.contains(&key) {
                has_chamfer_edge = true;

                let pos_a = poly.vertices[i].pos;
                let pos_b = poly.vertices[(i + 1) % verts.len()].pos;
                let edge_dir = (pos_b - pos_a).normalize();

                // Inset direction: perpendicular to the edge within the face plane.
                let inset_dir = face_normal.cross(&edge_dir).normalize();
                // Ensure inset points inward (toward face interior).
                let face_center = Point3::from(
                    poly.vertices
                        .iter()
                        .fold(Vector3::zeros(), |acc, v| acc + v.pos.coords)
                        / poly.vertices.len() as f64,
                );
                let test_point = pos_a + inset_dir * 0.001;
                let inset_dir = if (test_point - face_center).norm() < (pos_a - face_center).norm()
                {
                    inset_dir
                } else {
                    -inset_dir
                };

                let inset_a = pos_a + inset_dir * distance;
                let inset_b = pos_b + inset_dir * distance;

                // Replace the original edge endpoints with inset versions.
                // We add the inset points and will later form the chamfer strip.
                let last_idx = new_vertices.len() - 1;
                new_vertices[last_idx] = IrVertex::new(inset_a, face_normal);
                new_vertices.push(IrVertex::new(inset_b, face_normal));

                // Record the chamfer strip quad: original → inset on both sides.
                // We only record from one side; the strip connects when both faces
                // have been processed. Use edge_face_sides to determine if this is
                // the first or second face.
                let sides = edge_face_sides.get(&key).unwrap();
                if sides.len() == 2 && sides[0] == poly_idx {
                    // First face records the strip — we'll get the other side's
                    // inset points from the second face processing below.
                    chamfer_strips.push([pos_a, pos_b, inset_b, inset_a]);
                }
            }
        }

        if has_chamfer_edge && new_vertices.len() >= 3 {
            out_polygons.push(IrPolygon::new(new_vertices, None));
        } else {
            out_polygons.push(poly.clone());
        }
    }

    // Now build the chamfer strip polygons connecting the two inset faces.
    // For each selected edge, we need to form a quad from the inset points
    // on both adjacent faces.
    for e in &selected {
        let key = (e.vi_a, e.vi_b);
        let sides = match edge_face_sides.get(&key) {
            Some(s) if s.len() == 2 => s,
            _ => continue,
        };

        // Get inset positions from both faces.
        let poly_l = &polygons[sides[0]];
        let poly_r = &polygons[sides[1]];
        let nl = poly_l.plane.normal();
        let nr = poly_r.plane.normal();

        let mut compute_inset =
            |poly: &IrPolygon<()>, normal: Vector3<f64>| -> (Point3<f64>, Point3<f64>) {
                let verts: Vec<usize> = poly
                    .vertices
                    .iter()
                    .map(|v| vertex_map.get_or_create_index(v.pos))
                    .collect();
                // Find the edge in this polygon.
                for i in 0..verts.len() {
                    let a = verts[i];
                    let b = verts[(i + 1) % verts.len()];
                    let k = if a < b { (a, b) } else { (b, a) };
                    if k == key {
                        let pos_a = poly.vertices[i].pos;
                        let pos_b = poly.vertices[(i + 1) % verts.len()].pos;
                        let edge_dir = (pos_b - pos_a).normalize();
                        let mut inset_dir = normal.cross(&edge_dir).normalize();
                        let face_center = Point3::from(
                            poly.vertices
                                .iter()
                                .fold(Vector3::zeros(), |acc, v| acc + v.pos.coords)
                                / poly.vertices.len() as f64,
                        );
                        let test_point = pos_a + inset_dir * 0.001;
                        if (test_point - face_center).norm() > (pos_a - face_center).norm() {
                            inset_dir = -inset_dir;
                        }
                        return (pos_a + inset_dir * distance, pos_b + inset_dir * distance);
                    }
                }
                (e.pos_a, e.pos_b) // fallback — shouldn't happen
            };

        let (inset_la, inset_lb) = compute_inset(poly_l, nl);
        let (inset_ra, inset_rb) = compute_inset(poly_r, nr);

        // The chamfer strip quad connects:
        //   inset_la — inset_lb — inset_rb — inset_ra
        // But we need to be careful about winding order for correct normals.
        let edge_vec: Vector3<f64> = inset_lb - inset_la;
        let side_vec: Vector3<f64> = inset_ra - inset_la;
        let chamfer_normal = edge_vec.cross(&side_vec).normalize();
        // Check if normal points outward (same hemisphere as average of face normals).
        let avg_outward = (nl + nr).normalize();
        let strip_verts = if chamfer_normal.dot(&avg_outward) > 0.0 {
            vec![
                IrVertex::new(inset_la, chamfer_normal),
                IrVertex::new(inset_lb, chamfer_normal),
                IrVertex::new(inset_rb, chamfer_normal),
                IrVertex::new(inset_ra, chamfer_normal),
            ]
        } else {
            let flipped = -chamfer_normal;
            vec![
                IrVertex::new(inset_ra, flipped),
                IrVertex::new(inset_rb, flipped),
                IrVertex::new(inset_lb, flipped),
                IrVertex::new(inset_la, flipped),
            ]
        };
        out_polygons.push(IrPolygon::new(strip_verts, None));
    }

    Ok(IrMesh::from_polygons(&out_polygons, None))
}

fn polygon_inset_for_edge(
    poly: &IrPolygon<()>,
    key: (usize, usize),
    vertex_map: &mut csgrs::mesh::connectivity::VertexIndexMap,
    distance: f64,
) -> Option<(Point3<f64>, Point3<f64>, Vector3<f64>)> {
    let verts: Vec<usize> = poly
        .vertices
        .iter()
        .map(|v| vertex_map.get_or_create_index(v.pos))
        .collect();
    for i in 0..verts.len() {
        let a = verts[i];
        let b = verts[(i + 1) % verts.len()];
        let k = if a < b { (a, b) } else { (b, a) };
        if k != key {
            continue;
        }
        let pos_a = poly.vertices[i].pos;
        let pos_b = poly.vertices[(i + 1) % verts.len()].pos;
        let edge_dir = (pos_b - pos_a).normalize();
        let face_normal = poly.plane.normal();
        let mut inset_dir = face_normal.cross(&edge_dir).normalize();
        let face_center = Point3::from(
            poly.vertices
                .iter()
                .fold(Vector3::zeros(), |acc, v| acc + v.pos.coords)
                / poly.vertices.len() as f64,
        );
        let test_point = pos_a + inset_dir * 0.001;
        if (test_point - face_center).norm() > (pos_a - face_center).norm() {
            inset_dir = -inset_dir;
        }
        return Some((
            pos_a + inset_dir * distance,
            pos_b + inset_dir * distance,
            inset_dir,
        ));
    }
    None
}

fn fillet_inset_distance(radius: f64, dihedral: f64) -> f64 {
    let half = (dihedral * 0.5).clamp(1e-4, std::f64::consts::FRAC_PI_2 - 1e-4);
    radius / half.tan()
}

fn fillet_segment_count(radius: f64, dihedral: f64) -> usize {
    let density = (radius.abs() * dihedral.max(0.5)).ceil() as usize;
    density.clamp(4, 10)
}

fn rotate_around_axis(v: Vector3<f64>, axis: Vector3<f64>, angle: f64) -> Vector3<f64> {
    let unit_axis = axis.normalize();
    let cos = angle.cos();
    let sin = angle.sin();
    v * cos + unit_axis.cross(&v) * sin + unit_axis * unit_axis.dot(&v) * (1.0 - cos)
}

fn fillet_arc_points(
    corner: Point3<f64>,
    start_dir: Vector3<f64>,
    end_dir: Vector3<f64>,
    axis: Vector3<f64>,
    radius: f64,
    dihedral: f64,
    segments: usize,
) -> Option<Vec<(Point3<f64>, Vector3<f64>)>> {
    let bisector = (start_dir + end_dir).try_normalize(1e-9)?;
    let center = corner + bisector * (radius / (dihedral * 0.5).sin());
    let tangent_distance = fillet_inset_distance(radius, dihedral);
    let tangent_start = corner + start_dir * tangent_distance;
    let tangent_end = corner + end_dir * tangent_distance;
    let radial_start = tangent_start - center;
    let radial_end = tangent_end - center;
    let axis = axis.try_normalize(1e-9)?;
    let mut angle = radial_start
        .normalize()
        .dot(&radial_end.normalize())
        .clamp(-1.0, 1.0)
        .acos();
    if radial_start.cross(&radial_end).dot(&axis) < 0.0 {
        angle = -angle;
    }
    let mut points = Vec::with_capacity(segments + 1);
    for step in 0..=segments {
        let t = step as f64 / segments as f64;
        let radial = rotate_around_axis(radial_start, axis, angle * t);
        let point = center + radial;
        points.push((point, radial.normalize()));
    }
    Some(points)
}

fn fillet_mesh(mesh: &IrMesh, radius: f64, selector: EdgeSelector) -> AppResult<IrMesh> {
    if radius.abs() < 1e-9 {
        return Ok(mesh.clone());
    }
    let tri_mesh = mesh.triangulate();
    let all_edges = detect_feature_edges(&tri_mesh);
    let selected = filter_edges(&all_edges, selector);
    if selected.is_empty() {
        return Ok(mesh.clone());
    }

    let polygons = &tri_mesh.polygons;
    let mut vertex_map = csgrs::mesh::connectivity::VertexIndexMap::new(1e-9);
    for poly in polygons {
        for v in &poly.vertices {
            vertex_map.get_or_create_index(v.pos);
        }
    }

    let edge_distances: HashMap<(usize, usize), f64> = selected
        .iter()
        .map(|edge| {
            (
                (edge.vi_a, edge.vi_b),
                fillet_inset_distance(radius, edge.dihedral),
            )
        })
        .collect();
    let selected_keys: std::collections::HashSet<(usize, usize)> =
        edge_distances.keys().copied().collect();

    let mut edge_face_sides: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (poly_idx, poly) in polygons.iter().enumerate() {
        let verts: Vec<usize> = poly
            .vertices
            .iter()
            .map(|v| vertex_map.get_or_create_index(v.pos))
            .collect();
        for i in 0..verts.len() {
            let a = verts[i];
            let b = verts[(i + 1) % verts.len()];
            let key = if a < b { (a, b) } else { (b, a) };
            if selected_keys.contains(&key) {
                edge_face_sides.entry(key).or_default().push(poly_idx);
            }
        }
    }

    let mut out_polygons: Vec<IrPolygon<()>> = Vec::new();
    for poly in polygons {
        let verts: Vec<usize> = poly
            .vertices
            .iter()
            .map(|v| vertex_map.get_or_create_index(v.pos))
            .collect();
        let face_normal = poly.plane.normal();
        let mut modified = false;
        let mut new_vertices = Vec::new();

        for i in 0..verts.len() {
            let a = verts[i];
            let b = verts[(i + 1) % verts.len()];
            let key = if a < b { (a, b) } else { (b, a) };
            new_vertices.push(poly.vertices[i].clone());
            let Some(distance) = edge_distances.get(&key).copied() else {
                continue;
            };
            modified = true;
            let Some((inset_a, inset_b, _)) =
                polygon_inset_for_edge(poly, key, &mut vertex_map, distance)
            else {
                continue;
            };
            let last_idx = new_vertices.len() - 1;
            new_vertices[last_idx] = IrVertex::new(inset_a, face_normal);
            new_vertices.push(IrVertex::new(inset_b, face_normal));
        }

        if modified && new_vertices.len() >= 3 {
            out_polygons.push(IrPolygon::new(new_vertices, None));
        } else {
            out_polygons.push(poly.clone());
        }
    }

    for edge in selected {
        let key = (edge.vi_a, edge.vi_b);
        let Some(sides) = edge_face_sides.get(&key) else {
            continue;
        };
        if sides.len() != 2 {
            continue;
        }
        let Some(distance) = edge_distances.get(&key).copied() else {
            continue;
        };
        let poly_l = &polygons[sides[0]];
        let poly_r = &polygons[sides[1]];
        let Some((_inset_la, _inset_lb, dir_l)) =
            polygon_inset_for_edge(poly_l, key, &mut vertex_map, distance)
        else {
            continue;
        };
        let Some((_inset_ra, _inset_rb, dir_r)) =
            polygon_inset_for_edge(poly_r, key, &mut vertex_map, distance)
        else {
            continue;
        };
        let edge_axis = edge.pos_b - edge.pos_a;
        let segments = fillet_segment_count(radius, edge.dihedral);
        let Some(arc_a) = fillet_arc_points(
            edge.pos_a,
            dir_l,
            dir_r,
            edge_axis,
            radius,
            edge.dihedral,
            segments,
        ) else {
            continue;
        };
        let Some(arc_b) = fillet_arc_points(
            edge.pos_b,
            dir_l,
            dir_r,
            edge_axis,
            radius,
            edge.dihedral,
            segments,
        ) else {
            continue;
        };

        let avg_outward = (edge.normal_left + edge.normal_right)
            .try_normalize(1e-9)
            .unwrap_or(edge.normal_left);
        for segment in 0..segments {
            let (a0, n_a0) = arc_a[segment];
            let (a1, n_a1) = arc_a[segment + 1];
            let (b0, n_b0) = arc_b[segment];
            let (b1, n_b1) = arc_b[segment + 1];
            let mut poly = IrPolygon::new(
                vec![
                    IrVertex::new(a0, n_a0),
                    IrVertex::new(b0, n_b0),
                    IrVertex::new(b1, n_b1),
                    IrVertex::new(a1, n_a1),
                ],
                None,
            );
            if poly.plane.normal().dot(&avg_outward) < 0.0 {
                poly.flip();
            }
            out_polygons.push(poly);
        }
    }

    Ok(IrMesh::from_polygons(&out_polygons, None))
}

fn parse_edge_selector(
    args: &[Value],
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<(EdgeSelector, usize)> {
    // Check for :edges "selector" keyword pair after the distance argument.
    if args.len() >= 3
        && keyword_name(&args[1])
            .map(|k| k == "edges")
            .unwrap_or(false)
    {
        let selector_str = eval_stringish(&args[2], env)?;
        let selector = match selector_str.as_str() {
            "all" => EdgeSelector::All,
            "top" => EdgeSelector::Top,
            "bottom" => EdgeSelector::Bottom,
            "vertical" => EdgeSelector::Vertical,
            other => {
                return Err(validation(format!(
                    "Unknown edge selector `{}`. Use `all`, `top`, `bottom`, or `vertical`.",
                    other
                )));
            }
        };
        Ok((selector, 3))
    } else {
        Ok((EdgeSelector::All, 1))
    }
}

fn fold_boolean_geometry(
    name: &str,
    args: &[Value],
    env: &BTreeMap<String, ParamValue>,
    combine_mesh: impl Fn(IrMesh, IrMesh) -> IrMesh,
    combine_sketch: impl Fn(IrSketch, IrSketch) -> IrSketch,
) -> AppResult<Geometry> {
    if args.len() < 2 {
        return Err(validation(format!(
            "`{}` expects at least two geometry operands.",
            name
        )));
    }
    let mut iter = args.iter();
    match eval_geometry(iter.next().expect("checked"), env)? {
        Geometry::Mesh(first) => {
            let mesh = iter.try_fold(first, |acc, arg| match eval_geometry(arg, env)? {
                Geometry::Mesh(next) => Ok(combine_mesh(acc, next)),
                other => Err(unsupported(format!(
                    "Node `{}` cannot mix 3D solids and {} in one boolean expression.",
                    name,
                    other.kind_str()
                ))),
            })?;
            Ok(Geometry::Mesh(mesh))
        }
        Geometry::Sketch(first) => {
            let sketch = iter.try_fold(first, |acc, arg| match eval_geometry(arg, env)? {
                Geometry::Sketch(next) => Ok(combine_sketch(acc, next)),
                other => Err(unsupported(format!(
                    "Node `{}` cannot mix 2D sketches and {} in one boolean expression.",
                    name,
                    other.kind_str()
                ))),
            })?;
            Ok(Geometry::Sketch(sketch))
        }
        Geometry::Path(_) => Err(unsupported(format!(
            "Node `{}` does not support boolean operations on 3D paths.",
            name
        ))),
    }
}

fn transform_mesh_or_sketch(
    name: &str,
    args: &[Value],
    env: &BTreeMap<String, ParamValue>,
    transform: impl Fn(Geometry, f64, f64, f64) -> Geometry,
) -> AppResult<Geometry> {
    if args.len() != 4 {
        return Err(validation(format!(
            "`{}` expects x, y, z, and a geometry node.",
            name
        )));
    }
    let x = eval_number(&args[0], env)?;
    let y = eval_number(&args[1], env)?;
    let z = eval_number(&args[2], env)?;
    Ok(transform(eval_geometry(&args[3], env)?, x, y, z))
}

/// Compute the signed volume of a closed triangulated mesh using the divergence theorem.
///
/// For each triangle with vertices (a, b, c), the signed tetrahedron volume
/// (with the origin as the fourth vertex) is:
///   V_tet = a · (b × c) / 6
///
/// Summing over all triangles of a closed mesh gives the enclosed volume.
/// The result is the absolute value, which is correct for consistently-wound meshes.
fn mesh_volume(mesh: &IrMesh) -> Option<f64> {
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
fn mesh_area(mesh: &IrMesh) -> Option<f64> {
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

fn bounds_from_mesh(mesh: &IrMesh) -> ManifestBounds {
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

fn runtime_root(app: &dyn PathResolver) -> AppResult<PathBuf> {
    let root = app.app_data_dir().join(MODEL_RUNTIME_ROOT);
    fs::create_dir_all(&root).map_err(|err| AppError::persistence(err.to_string()))?;
    Ok(root)
}

fn bundle_dir(app: &dyn PathResolver, model_id: &str) -> AppResult<PathBuf> {
    let path = runtime_root(app)?
        .join(GENERATED_ARTIFACT_DIR)
        .join(model_id);
    fs::create_dir_all(&path).map_err(|err| AppError::persistence(err.to_string()))?;
    Ok(path)
}

fn write_bundle(path: &Path, bundle: &ArtifactBundle) -> AppResult<()> {
    let data = serde_json::to_string_pretty(bundle)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    fs::write(path, data).map_err(|err| AppError::persistence(err.to_string()))
}

fn write_manifest(path: &Path, manifest: &ModelManifest) -> AppResult<()> {
    let data = serde_json::to_string_pretty(manifest)
        .map_err(|err| AppError::persistence(err.to_string()))?;
    fs::write(path, data).map_err(|err| AppError::persistence(err.to_string()))
}

pub fn derive_controls(source: &str) -> AppResult<ParsedParamsResult> {
    let model = parse_model(source)?;
    Ok(ParsedParamsResult {
        fields: model
            .params
            .iter()
            .map(|param| param.field.clone())
            .collect(),
        params: model
            .params
            .iter()
            .map(|param| (param.field.key().to_string(), param.default_value.clone()))
            .collect(),
    })
}

pub fn render_model(
    source: &str,
    parameters: &DesignParams,
    app: &dyn PathResolver,
) -> AppResult<ArtifactBundle> {
    let model = parse_model(source)?;
    let canonical_source = canonicalize(source)?;
    let env = build_param_env(&model, parameters);
    let params_json = serde_json::to_string(parameters).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(canonical_source.as_bytes());
    hasher.update(params_json.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let model_id = format!("generated-ir-{}", &hash[..12]);
    let dir = bundle_dir(app, &model_id)?;
    let parts_dir = dir.join(PARTS_DIR_NAME);
    fs::create_dir_all(&parts_dir).map_err(|err| AppError::persistence(err.to_string()))?;

    let mut part_bindings = Vec::new();
    let mut viewer_assets = Vec::new();
    let mut preview_mesh: Option<IrMesh> = None;

    for (index, part) in model.parts.iter().enumerate() {
        let mesh = eval_geometry(&part.expr, &env)?.into_mesh("part")?;
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
            parameter_keys: model
                .params
                .iter()
                .map(|param| param.field.key().to_string())
                .collect(),
            editable: true,
            bounds: Some(bounds_from_mesh(&mesh)),
            volume: mesh_volume(&mesh),
            area: mesh_area(&mesh),
        });
    }

    let preview_mesh =
        preview_mesh.ok_or_else(|| validation("Ecky IR v0 model produced no printable parts."))?;
    let preview_path = dir.join(PREVIEW_STL_FILE_NAME);
    fs::write(
        &preview_path,
        preview_mesh.to_stl_binary("preview").map_err(|err| {
            AppError::persistence(format!("Failed to encode preview STL: {}", err))
        })?,
    )
    .map_err(|err| AppError::persistence(err.to_string()))?;

    let macro_path = dir.join(SOURCE_FILE_NAME);
    fs::write(&macro_path, canonical_source.as_bytes())
        .map_err(|err| AppError::persistence(err.to_string()))?;

    let manifest = ModelManifest {
        schema_version: MODEL_RUNTIME_SCHEMA_VERSION,
        model_id: model_id.clone(),
        source_kind: ModelSourceKind::Generated,
        engine_kind: EngineKind::EckyIrV0,
        source_language: SourceLanguage::EckyIrV0,
        geometry_backend: GeometryBackend::EckyRust,
        document: DocumentMetadata {
            document_name: "Ecky IR v0".to_string(),
            document_label: "Ecky IR v0".to_string(),
            source_path: Some(macro_path.to_string_lossy().to_string()),
            object_count: part_bindings.len(),
            warnings: Vec::new(),
        },
        parts: part_bindings,
        parameter_groups: vec![ParameterGroup {
            group_id: "core".to_string(),
            label: "Core".to_string(),
            parameter_keys: model
                .params
                .iter()
                .map(|param| param.field.key().to_string())
                .collect(),
            part_ids: model
                .parts
                .iter()
                .map(|part| part.part_id.clone())
                .collect(),
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

// ===========================================================================
// Build123d lowering — Ecky IR AST → build123d Python source
// ===========================================================================

/// Lower an Ecky IR v0 source string into build123d Python code.
///
/// The returned string is a self-contained Python script that:
/// - imports build123d
/// - references a `params` dict injected by the caller (the build123d runner)
/// - assigns `_ecky_parts = [("part_id", shape), ...]`
///
/// Unsupported nodes produce an explicit error rather than silently falling back.
pub fn lower_to_build123d(source: &str) -> AppResult<String> {
    let model = parse_model(source)?;
    let mut lowerer = B123dLowerer::new(&model);
    lowerer.lower_model()
}

struct B123dLowerer<'a> {
    model: &'a IrModel,
    lines: Vec<String>,
    counter: usize,
}

impl<'a> B123dLowerer<'a> {
    fn new(model: &'a IrModel) -> Self {
        Self {
            model,
            lines: Vec::new(),
            counter: 0,
        }
    }

    fn next_var(&mut self) -> String {
        let v = format!("_v{}", self.counter);
        self.counter += 1;
        v
    }

    fn emit(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    fn param_defaults(&self) -> BTreeMap<String, ParamValue> {
        self.model
            .params
            .iter()
            .map(|p| (p.field.key().to_string(), p.default_value.clone()))
            .collect()
    }

    fn lower_model(&mut self) -> AppResult<String> {
        self.emit("from build123d import *");
        self.emit("from build123d import exporters");
        self.emit("import math");
        self.emit("");

        let defaults = self.param_defaults();
        let parts: Vec<(String, Value)> = self
            .model
            .parts
            .iter()
            .map(|p| (p.part_id.clone(), p.expr.clone()))
            .collect();
        let mut part_entries: Vec<String> = Vec::new();
        for (part_id, expr) in &parts {
            let var = self.lower_geometry(expr, &defaults)?;
            part_entries.push(format!("({:?}, {})", part_id, var));
        }

        self.emit("");
        self.emit(format!("_ecky_parts = [{}]", part_entries.join(", ")));
        Ok(self.lines.join("\n"))
    }

    fn lower_geometry_list(
        &mut self,
        args: &[Value],
        defaults: &BTreeMap<String, ParamValue>,
    ) -> AppResult<Vec<String>> {
        args.iter()
            .map(|arg| self.lower_geometry(arg, defaults))
            .collect()
    }

    fn lower_points_2d_args(
        &self,
        value: &Value,
        defaults: &BTreeMap<String, ParamValue>,
    ) -> AppResult<String> {
        let points = list_items(value, "point list")?;
        let mut entries = Vec::new();
        for point in &points {
            let pair = list_items(point, "point")?;
            if pair.len() != 2 {
                return Err(validation("Points must be (x y) pairs."));
            }
            let x = lower_num(&pair[0], defaults)?;
            let y = lower_num(&pair[1], defaults)?;
            entries.push(format!("({x}, {y})"));
        }
        Ok(entries.join(", "))
    }

    fn lower_points_3d_args(
        &self,
        value: &Value,
        defaults: &BTreeMap<String, ParamValue>,
    ) -> AppResult<String> {
        let points = list_items(value, "3D point list")?;
        let mut entries = Vec::new();
        for point in &points {
            let triple = list_items(point, "3D point")?;
            if triple.len() != 3 {
                return Err(validation("3D points must be (x y z) triples."));
            }
            let x = lower_num(&triple[0], defaults)?;
            let y = lower_num(&triple[1], defaults)?;
            let z = lower_num(&triple[2], defaults)?;
            entries.push(format!("({x}, {y}, {z})"));
        }
        Ok(entries.join(", "))
    }

    fn lower_sketch_operand(
        &mut self,
        value: &Value,
        defaults: &BTreeMap<String, ParamValue>,
    ) -> AppResult<String> {
        if let Ok(items) = list_items(value, "sketch operand") {
            if let Ok("circle" | "rounded_rect" | "rounded-rect" | "polygon") =
                head_symbol(&items, "sketch operand")
            {
                return self.lower_geometry(value, defaults);
            }
        }
        let points = self.lower_points_2d_args(value, defaults)?;
        let var = self.next_var();
        self.emit(format!("{var} = Polygon({points})"));
        Ok(var)
    }

    fn lower_loop_collection(
        &mut self,
        value: &Value,
        defaults: &BTreeMap<String, ParamValue>,
    ) -> AppResult<Vec<String>> {
        let items = list_items(value, "loop collection")?;
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let is_node = items
            .first()
            .and_then(|v| v.as_symbol())
            .map(|s| !s.starts_with(':'))
            .unwrap_or(false);
        if is_node {
            return Ok(vec![self.lower_sketch_operand(value, defaults)?]);
        }
        let is_single_loop = items
            .first()
            .and_then(|v| v.to_vec())
            .map(|pair| pair.len() == 2)
            .unwrap_or(false);
        if is_single_loop {
            return Ok(vec![self.lower_sketch_operand(value, defaults)?]);
        }
        items
            .iter()
            .map(|item| self.lower_sketch_operand(item, defaults))
            .collect()
    }

    fn lower_count(
        &self,
        value: &Value,
        defaults: &BTreeMap<String, ParamValue>,
    ) -> AppResult<String> {
        if let Some(n) = value.as_f64() {
            return Ok(format!("{}", n.round().max(1.0) as usize));
        }
        let expr = lower_num(value, defaults)?;
        Ok(format!("int({})", expr))
    }

    fn lower_geometry(
        &mut self,
        value: &Value,
        defaults: &BTreeMap<String, ParamValue>,
    ) -> AppResult<String> {
        let items = list_items(value, "geometry node")?;
        let node = head_symbol(&items, "geometry node")?;
        let args = &items[1..];
        let var = self.next_var();

        match node {
            "box" => {
                if args.len() != 3 {
                    return Err(validation("`box` expects width, depth, and height."));
                }
                let w = lower_num(&args[0], defaults)?;
                let d = lower_num(&args[1], defaults)?;
                let h = lower_num(&args[2], defaults)?;
                self.emit(format!(
                    "{var} = Box({w}, {d}, {h}, align=(Align.CENTER, Align.CENTER, Align.MIN))"
                ));
            }
            "cylinder" => {
                if args.len() < 2 || args.len() > 3 {
                    return Err(validation("`cylinder` expects radius and height."));
                }
                let r = lower_num(&args[0], defaults)?;
                let h = lower_num(&args[1], defaults)?;
                self.emit(format!(
                    "{var} = Cylinder({r}, {h}, align=(Align.CENTER, Align.CENTER, Align.MIN))"
                ));
            }
            "sphere" => {
                if args.is_empty() || args.len() > 3 {
                    return Err(validation("`sphere` expects radius."));
                }
                let r = lower_num(&args[0], defaults)?;
                self.emit(format!("{var} = Sphere({r})"));
            }
            "cone" => {
                if args.len() < 3 || args.len() > 4 {
                    return Err(validation(
                        "`cone` expects bottom radius, top radius, and height.",
                    ));
                }
                let br = lower_num(&args[0], defaults)?;
                let tr = lower_num(&args[1], defaults)?;
                let h = lower_num(&args[2], defaults)?;
                self.emit(format!(
                    "{var} = Cone({br}, {tr}, {h}, align=(Align.CENTER, Align.CENTER, Align.MIN))"
                ));
            }
            "circle" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(validation("`circle` expects radius."));
                }
                let r = lower_num(&args[0], defaults)?;
                self.emit(format!("{var} = Circle({r})"));
            }
            "rounded_rect" | "rounded-rect" => {
                if args.len() < 3 || args.len() > 4 {
                    return Err(validation(
                        "`rounded_rect` expects width, height, and corner radius.",
                    ));
                }
                let w = lower_num(&args[0], defaults)?;
                let h = lower_num(&args[1], defaults)?;
                let r = lower_num(&args[2], defaults)?;
                self.emit(format!("{var} = RectangleRounded({w}, {h}, {r})"));
            }
            "extrude" => {
                if args.len() != 2 {
                    return Err(validation("`extrude` expects a sketch and height."));
                }
                let sketch_var = self.lower_geometry(&args[0], defaults)?;
                let h = lower_num(&args[1], defaults)?;
                self.emit(format!("{var} = extrude({sketch_var}, {h})"));
            }
            "union" => {
                if args.len() < 2 {
                    return Err(validation("`union` expects at least two operands."));
                }
                let operand_vars = self.lower_geometry_list(args, defaults)?;
                self.emit(format!("{var} = {}", operand_vars.join(" + ")));
            }
            "difference" => {
                if args.len() < 2 {
                    return Err(validation("`difference` expects at least two operands."));
                }
                let operand_vars = self.lower_geometry_list(args, defaults)?;
                self.emit(format!("{var} = {}", operand_vars.join(" - ")));
            }
            "intersection" => {
                if args.len() < 2 {
                    return Err(validation("`intersection` expects at least two operands."));
                }
                let operand_vars = self.lower_geometry_list(args, defaults)?;
                self.emit(format!("{var} = {}", operand_vars.join(" & ")));
            }
            "translate" => {
                if args.len() != 4 {
                    return Err(validation(
                        "`translate` expects x, y, z, and a geometry node.",
                    ));
                }
                let x = lower_num(&args[0], defaults)?;
                let y = lower_num(&args[1], defaults)?;
                let z = lower_num(&args[2], defaults)?;
                let inner = self.lower_geometry(&args[3], defaults)?;
                self.emit(format!("{var} = Pos({x}, {y}, {z}) * {inner}"));
            }
            "rotate" => {
                if args.len() != 4 {
                    return Err(validation("`rotate` expects x, y, z, and a geometry node."));
                }
                let rx = lower_num(&args[0], defaults)?;
                let ry = lower_num(&args[1], defaults)?;
                let rz = lower_num(&args[2], defaults)?;
                let inner = self.lower_geometry(&args[3], defaults)?;
                self.emit(format!("{var} = Rot({rx}, {ry}, {rz}) * {inner}"));
            }
            "scale" => {
                if args.len() != 4 {
                    return Err(validation("`scale` expects x, y, z, and a geometry node."));
                }
                let sx = lower_num(&args[0], defaults)?;
                let sy = lower_num(&args[1], defaults)?;
                let sz = lower_num(&args[2], defaults)?;
                let inner = self.lower_geometry(&args[3], defaults)?;
                // build123d only supports uniform scale; emit a runtime guard.
                self.emit(format!("_sx, _sy, _sz = {sx}, {sy}, {sz}"));
                self.emit(
                    "if not (abs(_sx - _sy) < 1e-9 and abs(_sy - _sz) < 1e-9): \
                     raise ValueError(f'build123d lowerer: non-uniform scale not supported \
                     ({{_sx}}, {{_sy}}, {{_sz}}).')"
                        .to_string(),
                );
                self.emit(format!("{var} = {inner}.scale(_sx)"));
            }
            "polygon" => {
                if args.len() != 1 {
                    return Err(validation("`polygon` expects a single point list."));
                }
                let points = self.lower_points_2d_args(&args[0], defaults)?;
                self.emit(format!("{var} = Polygon({points})"));
            }
            "profile" => {
                let mut outer_vars: Vec<String> = Vec::new();
                let mut hole_vars: Vec<String> = Vec::new();
                for form in args {
                    let pair = list_items(form, "profile clause")?;
                    if pair.len() != 2 {
                        return Err(validation(
                            "`profile` clauses must look like `(:outer ...)` or `(:holes ...)`.",
                        ));
                    }
                    let name = keyword_name(&pair[0]).ok_or_else(|| {
                        validation(
                            "`profile` clauses must use keywords like `:outer` and `:holes`.",
                        )
                    })?;
                    match name {
                        "outer" => {
                            outer_vars.extend(self.lower_loop_collection(&pair[1], defaults)?);
                        }
                        "holes" => {
                            hole_vars.extend(self.lower_loop_collection(&pair[1], defaults)?);
                        }
                        other => {
                            return Err(validation(format!(
                                "`profile` does not recognize clause `:{}`.",
                                other
                            )))
                        }
                    }
                }
                if outer_vars.is_empty() {
                    return Err(validation("`profile` needs at least one outer loop."));
                }
                let mut result = outer_vars[0].clone();
                for v in &outer_vars[1..] {
                    let u = self.next_var();
                    self.emit(format!("{u} = {result} + {v}"));
                    result = u;
                }
                for v in &hole_vars {
                    let d = self.next_var();
                    self.emit(format!("{d} = {result} - {v}"));
                    result = d;
                }
                self.emit(format!("{var} = {result}"));
            }
            "offset" | "offset-rounded" => {
                if args.len() != 2 {
                    return Err(validation(format!(
                        "`{}` expects distance and a sketch.",
                        node
                    )));
                }
                let distance = lower_num(&args[0], defaults)?;
                let sketch_var = self.lower_geometry(&args[1], defaults)?;
                self.emit(format!("{var} = offset({sketch_var}, amount={distance})"));
            }
            "revolve" => {
                if args.len() < 2 || args.len() > 3 {
                    return Err(validation(
                        "`revolve` expects a sketch, angle, and optional segments.",
                    ));
                }
                let sketch_var = self.lower_geometry(&args[0], defaults)?;
                let angle = lower_num(&args[1], defaults)?;
                let positioned = self.next_var();
                self.emit(format!("{positioned} = Rot(90, 0, 0) * {sketch_var}"));
                self.emit(format!(
                    "{var} = revolve({positioned}, axis=Axis.Z, revolution_arc={angle})"
                ));
            }
            "loft" => {
                if args.len() != 3 {
                    return Err(validation(
                        "`loft` expects height, bottom sketch, and top sketch.",
                    ));
                }
                let height = lower_num(&args[0], defaults)?;
                let bottom = self.lower_geometry(&args[1], defaults)?;
                let top = self.lower_geometry(&args[2], defaults)?;
                let top_pos = self.next_var();
                self.emit(format!("{top_pos} = Pos(0, 0, {height}) * {top}"));
                self.emit(format!("{var} = loft([{bottom}, {top_pos}])"));
            }
            "taper" => {
                if !(args.len() == 3 || args.len() == 4) {
                    return Err(validation(
                        "`taper` expects height, scale, sketch or height, scale-x, scale-y, sketch.",
                    ));
                }
                let height = lower_num(&args[0], defaults)?;
                let (scale_x, scale_y, sketch_index) = if args.len() == 3 {
                    let s = lower_num(&args[1], defaults)?;
                    (s.clone(), s, 2usize)
                } else {
                    (
                        lower_num(&args[1], defaults)?,
                        lower_num(&args[2], defaults)?,
                        3usize,
                    )
                };
                let sketch_var = self.lower_geometry(&args[sketch_index], defaults)?;
                let bottom = self.next_var();
                self.emit(format!("{bottom} = {sketch_var}"));
                let scaled = self.next_var();
                self.emit(format!("_tsx, _tsy = {scale_x}, {scale_y}"));
                self.emit(format!(
                    "if abs(_tsx - _tsy) < 1e-9: {scaled} = Pos(0, 0, {height}) * {sketch_var}.scale(_tsx)"
                ));
                self.emit(
                    "else: raise ValueError('build123d lowerer: non-uniform taper scale not supported')".to_string()
                );
                self.emit(format!("{var} = loft([{bottom}, {scaled}])"));
            }
            "twist" => {
                if !(args.len() == 3 || args.len() == 4) {
                    return Err(validation(
                        "`twist` expects height, angle, sketch or height, angle, segments, sketch.",
                    ));
                }
                let height = lower_num(&args[0], defaults)?;
                let angle = lower_num(&args[1], defaults)?;
                let (segments, sketch_index) = if args.len() == 3 {
                    ("12".to_string(), 2usize)
                } else {
                    (self.lower_count(&args[2], defaults)?, 3usize)
                };
                let sketch_var = self.lower_geometry(&args[sketch_index], defaults)?;
                let sections = self.next_var();
                self.emit(format!(
                    "{sections} = [Pos(0, 0, {height} * _ti / {segments}) * Rot(0, 0, {angle} * _ti / {segments}) * {sketch_var} for _ti in range({segments} + 1)]"
                ));
                self.emit(format!("{var} = loft({sections})"));
            }
            "path" => {
                let mut point_strs = Vec::new();
                for arg in args {
                    let triple = list_items(arg, "3D point")?;
                    if triple.len() != 3 {
                        return Err(validation("3D points must be (x y z) triples."));
                    }
                    let x = lower_num(&triple[0], defaults)?;
                    let y = lower_num(&triple[1], defaults)?;
                    let z = lower_num(&triple[2], defaults)?;
                    point_strs.push(format!("({x}, {y}, {z})"));
                }
                if point_strs.len() < 2 {
                    return Err(validation("`path` expects at least two points."));
                }
                self.emit(format!("{var} = Polyline({})", point_strs.join(", ")));
            }
            "bezier-path" => {
                if args.is_empty() {
                    return Err(validation(
                        "`bezier-path` expects points and optional segments.",
                    ));
                }
                let points_str = self.lower_points_3d_args(&args[0], defaults)?;
                let pts_var = self.next_var();
                self.emit(format!("{pts_var} = [{points_str}]"));
                self.emit(format!(
                    "{var} = Bezier({pts_var}[0], {pts_var}[1], {pts_var}[2], {pts_var}[3])"
                ));
                self.emit(format!("for _bi in range(3, len({pts_var})-1, 3):"));
                self.emit(format!(
                    "    {var} = {var} + Bezier({pts_var}[_bi], {pts_var}[_bi+1], {pts_var}[_bi+2], {pts_var}[_bi+3])"
                ));
            }
            "sweep" => {
                if args.len() != 2 {
                    return Err(validation("`sweep` expects a sketch and a path."));
                }
                let section = self.lower_geometry(&args[0], defaults)?;
                let path_var = self.lower_geometry(&args[1], defaults)?;
                self.emit(format!("{var} = sweep({section}, path={path_var})"));
            }
            "shell" => {
                if args.len() != 2 {
                    return Err(validation(
                        "`shell` expects wall thickness and a geometry node.",
                    ));
                }
                let wall = lower_num(&args[0], defaults)?;
                let target_items = list_items(&args[1], "shell target")?;
                let target_node = head_symbol(&target_items, "shell target")?;
                let target_args = &target_items[1..];

                match target_node {
                    "cylinder" => {
                        if target_args.len() < 2 || target_args.len() > 3 {
                            return Err(validation(
                                "`shell` cylinder expects radius, height, and optional segments.",
                            ));
                        }
                        let r = lower_num(&target_args[0], defaults)?;
                        let h = lower_num(&target_args[1], defaults)?;
                        let outer = self.next_var();
                        let inner = self.next_var();
                        self.emit(format!(
                            "{outer} = Cylinder({r}, {h}, align=(Align.CENTER, Align.CENTER, Align.MIN))"
                        ));
                        self.emit(format!(
                            "{inner} = Cylinder(({r}) - ({wall}), {h}, align=(Align.CENTER, Align.CENTER, Align.MIN))"
                        ));
                        self.emit(format!("{var} = {outer} - {inner}"));
                    }
                    "cone" => {
                        if target_args.len() < 3 || target_args.len() > 4 {
                            return Err(validation(
                                "`shell` cone expects bottom radius, top radius, height.",
                            ));
                        }
                        let br = lower_num(&target_args[0], defaults)?;
                        let tr = lower_num(&target_args[1], defaults)?;
                        let h = lower_num(&target_args[2], defaults)?;
                        let outer = self.next_var();
                        let inner = self.next_var();
                        self.emit(format!(
                            "{outer} = Cone({br}, {tr}, {h}, align=(Align.CENTER, Align.CENTER, Align.MIN))"
                        ));
                        self.emit(format!(
                            "{inner} = Cone(({br}) - ({wall}), ({tr}) - ({wall}), {h}, align=(Align.CENTER, Align.CENTER, Align.MIN))"
                        ));
                        self.emit(format!("{var} = {outer} - {inner}"));
                    }
                    "sphere" => {
                        if target_args.is_empty() || target_args.len() > 3 {
                            return Err(validation("`shell` sphere expects radius."));
                        }
                        let r = lower_num(&target_args[0], defaults)?;
                        let outer = self.next_var();
                        let inner = self.next_var();
                        self.emit(format!("{outer} = Sphere({r})"));
                        self.emit(format!("{inner} = Sphere(({r}) - ({wall}))"));
                        self.emit(format!("{var} = {outer} - {inner}"));
                    }
                    "extrude" => {
                        if target_args.len() != 2 {
                            return Err(validation("`shell` extrude expects a sketch and height."));
                        }
                        let sketch_var = self.lower_geometry(&target_args[0], defaults)?;
                        let h = lower_num(&target_args[1], defaults)?;
                        let inner_sketch = self.next_var();
                        self.emit(format!(
                            "{inner_sketch} = offset({sketch_var}, amount=-({wall}))"
                        ));
                        let outer = self.next_var();
                        let inner = self.next_var();
                        self.emit(format!("{outer} = extrude({sketch_var}, {h})"));
                        self.emit(format!("{inner} = extrude({inner_sketch}, {h})"));
                        self.emit(format!("{var} = {outer} - {inner}"));
                    }
                    "revolve" => {
                        if target_args.len() < 2 || target_args.len() > 3 {
                            return Err(validation(
                                "`shell` revolve expects a sketch, angle, and optional segments.",
                            ));
                        }
                        let sketch_var = self.lower_geometry(&target_args[0], defaults)?;
                        let angle = lower_num(&target_args[1], defaults)?;
                        let inner_sketch = self.next_var();
                        self.emit(format!(
                            "{inner_sketch} = offset({sketch_var}, amount=-({wall}))"
                        ));
                        let outer_pos = self.next_var();
                        let inner_pos = self.next_var();
                        self.emit(format!("{outer_pos} = Rot(90, 0, 0) * {sketch_var}"));
                        self.emit(format!("{inner_pos} = Rot(90, 0, 0) * {inner_sketch}"));
                        let outer = self.next_var();
                        let inner = self.next_var();
                        self.emit(format!(
                            "{outer} = revolve({outer_pos}, axis=Axis.Z, revolution_arc={angle})"
                        ));
                        self.emit(format!(
                            "{inner} = revolve({inner_pos}, axis=Axis.Z, revolution_arc={angle})"
                        ));
                        self.emit(format!("{var} = {outer} - {inner}"));
                    }
                    "sweep" => {
                        if target_args.len() != 2 {
                            return Err(validation("`shell` sweep expects a sketch and a path."));
                        }
                        let sketch_var = self.lower_geometry(&target_args[0], defaults)?;
                        let path_var = self.lower_geometry(&target_args[1], defaults)?;
                        let inner_sketch = self.next_var();
                        self.emit(format!(
                            "{inner_sketch} = offset({sketch_var}, amount=-({wall}))"
                        ));
                        let outer = self.next_var();
                        let inner = self.next_var();
                        self.emit(format!("{outer} = sweep({sketch_var}, path={path_var})"));
                        self.emit(format!("{inner} = sweep({inner_sketch}, path={path_var})"));
                        self.emit(format!("{var} = {outer} - {inner}"));
                    }
                    "loft" => {
                        if target_args.len() != 3 {
                            return Err(validation(
                                "`shell` loft expects height, bottom sketch, and top sketch.",
                            ));
                        }
                        let h = lower_num(&target_args[0], defaults)?;
                        let bottom = self.lower_geometry(&target_args[1], defaults)?;
                        let top = self.lower_geometry(&target_args[2], defaults)?;
                        let inner_bottom = self.next_var();
                        let inner_top = self.next_var();
                        self.emit(format!(
                            "{inner_bottom} = offset({bottom}, amount=-({wall}))"
                        ));
                        self.emit(format!("{inner_top} = offset({top}, amount=-({wall}))"));
                        let top_pos = self.next_var();
                        let inner_top_pos = self.next_var();
                        self.emit(format!("{top_pos} = Pos(0, 0, {h}) * {top}"));
                        self.emit(format!("{inner_top_pos} = Pos(0, 0, {h}) * {inner_top}"));
                        let outer = self.next_var();
                        let inner = self.next_var();
                        self.emit(format!("{outer} = loft([{bottom}, {top_pos}])"));
                        self.emit(format!("{inner} = loft([{inner_bottom}, {inner_top_pos}])"));
                        self.emit(format!("{var} = {outer} - {inner}"));
                    }
                    other => {
                        return Err(unsupported(format!(
                            "Node `shell` with target `{}` is not yet supported by the build123d lowerer. \
                             Use the EckyRust backend for this model.",
                            other
                        )));
                    }
                }
            }
            "mirror" => {
                if args.len() != 3 {
                    return Err(validation(
                        "`mirror` expects axis, offset, and a geometry node.",
                    ));
                }
                let axis = parse_stringish(&args[0], "mirror axis")?;
                let offset = lower_num(&args[1], defaults)?;
                let inner = self.lower_geometry(&args[2], defaults)?;
                let plane = match axis.as_str() {
                    "x" => "Plane.YZ",
                    "y" => "Plane.XZ",
                    "z" => "Plane.XY",
                    other => {
                        return Err(validation(format!(
                            "Unsupported mirror axis `{}`. Use `x`, `y`, or `z`.",
                            other
                        )));
                    }
                };
                self.emit(format!(
                    "{var} = mirror({inner}, about={plane}.offset({offset}))"
                ));
            }
            "xor" => {
                if args.len() < 2 {
                    return Err(validation("`xor` expects at least two operands."));
                }
                let operand_vars = self.lower_geometry_list(args, defaults)?;
                let sum = self.next_var();
                self.emit(format!("{sum} = {}", operand_vars.join(" + ")));
                let inter = self.next_var();
                self.emit(format!("{inter} = {}", operand_vars.join(" & ")));
                self.emit(format!("{var} = {sum} - {inter}"));
            }
            "linear-array" => {
                if args.len() != 5 {
                    return Err(validation(
                        "`linear-array` expects count, dx, dy, dz, and a mesh.",
                    ));
                }
                let count = self.lower_count(&args[0], defaults)?;
                let dx = lower_num(&args[1], defaults)?;
                let dy = lower_num(&args[2], defaults)?;
                let dz = lower_num(&args[3], defaults)?;
                let base = self.lower_geometry(&args[4], defaults)?;
                self.emit(format!("{var} = {base}"));
                self.emit(format!("for _li in range(1, {count}):"));
                self.emit(format!(
                    "    {var} = {var} + Pos({dx} * _li, {dy} * _li, {dz} * _li) * {base}"
                ));
            }
            "radial-array" => {
                if args.len() != 4 {
                    return Err(validation(
                        "`radial-array` expects count, step degrees, radius, and a mesh.",
                    ));
                }
                let count = self.lower_count(&args[0], defaults)?;
                let step_deg = lower_num(&args[1], defaults)?;
                let radius = lower_num(&args[2], defaults)?;
                let base = self.lower_geometry(&args[3], defaults)?;
                let translated = self.next_var();
                self.emit(format!("{translated} = Pos({radius}, 0, 0) * {base}"));
                self.emit(format!("{var} = {translated}"));
                self.emit(format!("for _ri in range(1, {count}):"));
                self.emit(format!(
                    "    {var} = {var} + Rot(0, 0, {step_deg} * _ri) * {translated}"
                ));
            }
            "grid-array" => {
                if args.len() != 5 {
                    return Err(validation(
                        "`grid-array` expects rows, cols, dx, dy, and a mesh.",
                    ));
                }
                let rows = self.lower_count(&args[0], defaults)?;
                let cols = self.lower_count(&args[1], defaults)?;
                let dx = lower_num(&args[2], defaults)?;
                let dy = lower_num(&args[3], defaults)?;
                let base = self.lower_geometry(&args[4], defaults)?;
                self.emit(format!("{var} = {base}"));
                self.emit(format!("for _gr in range({rows}):"));
                self.emit(format!("    for _gc in range({cols}):"));
                self.emit(format!(
                    "        if _gr != 0 or _gc != 0: {var} = {var} + Pos({dx} * _gc, {dy} * _gr, 0) * {base}"
                ));
            }
            "arc-array" => {
                if args.len() != 5 {
                    return Err(validation(
                        "`arc-array` expects count, radius, start degrees, end degrees, and a mesh.",
                    ));
                }
                let count = self.lower_count(&args[0], defaults)?;
                let radius = lower_num(&args[1], defaults)?;
                let start_deg = lower_num(&args[2], defaults)?;
                let end_deg = lower_num(&args[3], defaults)?;
                let base = self.lower_geometry(&args[4], defaults)?;
                self.emit(format!(
                    "_arc_step = (({end_deg}) - ({start_deg})) / max(1, {count} - 1)"
                ));
                let first = self.next_var();
                self.emit(format!(
                    "{first} = Rot(0, 0, {start_deg}) * Pos({radius}, 0, 0) * {base}"
                ));
                self.emit(format!("{var} = {first}"));
                self.emit(format!("for _ai in range(1, {count}):"));
                self.emit(format!(
                    "    {var} = {var} + Rot(0, 0, ({start_deg}) + _arc_step * _ai) * Pos({radius}, 0, 0) * {base}"
                ));
            }
            "if" => {
                if args.len() != 3 {
                    return Err(validation(
                        "`if` expects condition, then-shape, else-shape.",
                    ));
                }
                let cond = lower_bool(&args[0], defaults)?;
                let then_var = self.lower_geometry(&args[1], defaults)?;
                let else_var = self.lower_geometry(&args[2], defaults)?;
                self.emit(format!("{var} = {then_var} if {cond} else {else_var}"));
            }
            "fillet" | "chamfer" => {
                if args.len() < 2 {
                    return Err(validation(format!(
                        "`{}` expects radius and a geometry node.",
                        node
                    )));
                }
                let radius = lower_num(&args[0], defaults)?;
                let (edge_select, body_index) = if args.len() >= 4
                    && keyword_name(&args[1])
                        .map(|k| k == "edges")
                        .unwrap_or(false)
                {
                    (parse_stringish(&args[2], "edge selection")?, 3usize)
                } else {
                    ("all".to_string(), 1usize)
                };
                if body_index >= args.len() {
                    return Err(validation(format!(
                        "`{}` is missing the geometry body argument.",
                        node
                    )));
                }
                let body = self.lower_geometry(&args[body_index], defaults)?;
                let edges_expr = match edge_select.as_str() {
                    "all" => format!("{body}.edges()"),
                    "top" => format!("{body}.edges().group_by(Axis.Z)[-1]"),
                    "bottom" => format!("{body}.edges().group_by(Axis.Z)[0]"),
                    "vertical" => format!("{body}.edges().filter_by(Axis.Z)"),
                    other => {
                        return Err(validation(format!(
                            "Unknown edge selector `{}`. Use `all`, `top`, `bottom`, or `vertical`.",
                            other
                        )));
                    }
                };
                self.emit(format!("{var} = {node}({edges_expr}, {radius})"));
            }
            "wall-pattern" | "pattern" => {
                return Err(unsupported(
                    "Node `wall-pattern` is not supported by the build123d lowerer. \
                     Use the EckyRust backend for this model.",
                ));
            }
            "rounded-polygon" | "rounded_polygon" | "bspline" => {
                return Err(unsupported(format!(
                    "Node `{}` is not yet supported by the build123d lowerer. \
                     Use the EckyRust backend for this model.",
                    node
                )));
            }
            other => {
                return Err(unsupported(format!(
                    "Node `{}` is not yet supported by the build123d lowerer. \
                     Use the EckyRust backend for this model.",
                    other
                )));
            }
        }

        Ok(var)
    }
}

fn lower_num(value: &Value, defaults: &BTreeMap<String, ParamValue>) -> AppResult<String> {
    if let Some(n) = value.as_f64() {
        return Ok(fmt_f64(n));
    }
    if let Some(sym) = value.as_symbol() {
        return match defaults.get(sym) {
            Some(ParamValue::Number(d)) => {
                Ok(format!("float(params.get({:?}, {}))", sym, fmt_f64(*d)))
            }
            Some(_) => Err(unsupported(format!(
                "Symbol `{}` is not a numeric parameter.",
                sym
            ))),
            None => Err(validation(format!("Unknown symbol `{}`.", sym))),
        };
    }
    let items = list_items(value, "numeric expression")?;
    let op = head_symbol(&items, "numeric expression")?;
    let args = &items[1..];
    match op {
        "+" => {
            if args.is_empty() {
                return Ok("0.0".to_string());
            }
            let parts = lower_num_list(args, defaults)?;
            Ok(format!("({})", parts.join(" + ")))
        }
        "-" => {
            if args.is_empty() {
                return Err(validation("`-` expects at least one argument."));
            }
            if args.len() == 1 {
                return Ok(format!("(-{})", lower_num(&args[0], defaults)?));
            }
            let first = lower_num(&args[0], defaults)?;
            let rest = lower_num_list(&args[1..], defaults)?;
            Ok(format!("({} - {})", first, rest.join(" - ")))
        }
        "*" => {
            if args.is_empty() {
                return Ok("1.0".to_string());
            }
            let parts = lower_num_list(args, defaults)?;
            Ok(format!("({})", parts.join(" * ")))
        }
        "/" => {
            if args.len() != 2 {
                return Err(validation("`/` expects exactly two arguments."));
            }
            let a = lower_num(&args[0], defaults)?;
            let b = lower_num(&args[1], defaults)?;
            Ok(format!("({a} / {b})"))
        }
        "min" => {
            let parts = lower_num_list(args, defaults)?;
            Ok(format!("min({})", parts.join(", ")))
        }
        "max" => {
            let parts = lower_num_list(args, defaults)?;
            Ok(format!("max({})", parts.join(", ")))
        }
        "clamp" => {
            if args.len() != 3 {
                return Err(validation("`clamp` expects value, min, max."));
            }
            let v = lower_num(&args[0], defaults)?;
            let lo = lower_num(&args[1], defaults)?;
            let hi = lower_num(&args[2], defaults)?;
            Ok(format!("max({lo}, min({hi}, {v}))"))
        }
        "lerp" => {
            if args.len() != 3 {
                return Err(validation("`lerp` expects start, end, t."));
            }
            let s = lower_num(&args[0], defaults)?;
            let e = lower_num(&args[1], defaults)?;
            let t = lower_num(&args[2], defaults)?;
            Ok(format!("(({s}) + (({e}) - ({s})) * ({t}))"))
        }
        "smoothstep" => {
            if args.len() != 3 {
                return Err(validation("`smoothstep` expects edge0, edge1, x."));
            }
            let e0 = lower_num(&args[0], defaults)?;
            let e1 = lower_num(&args[1], defaults)?;
            let x = lower_num(&args[2], defaults)?;
            Ok(format!(
                "(lambda _t: _t*_t*(3.0-2.0*_t))\
                 (max(0.0, min(1.0, ({x} - {e0}) / ({e1} - {e0}))))"
            ))
        }
        "sin" => {
            if args.len() != 1 {
                return Err(validation("`sin` expects one argument."));
            }
            Ok(format!("math.sin({})", lower_num(&args[0], defaults)?))
        }
        "cos" => {
            if args.len() != 1 {
                return Err(validation("`cos` expects one argument."));
            }
            Ok(format!("math.cos({})", lower_num(&args[0], defaults)?))
        }
        "tan" => {
            if args.len() != 1 {
                return Err(validation("`tan` expects one argument."));
            }
            Ok(format!("math.tan({})", lower_num(&args[0], defaults)?))
        }
        "abs" => {
            if args.len() != 1 {
                return Err(validation("`abs` expects one argument."));
            }
            Ok(format!("abs({})", lower_num(&args[0], defaults)?))
        }
        "deg" => {
            if args.len() != 1 {
                return Err(validation("`deg` expects one argument."));
            }
            Ok(format!("math.radians({})", lower_num(&args[0], defaults)?))
        }
        "rad" => {
            if args.len() != 1 {
                return Err(validation("`rad` expects one argument."));
            }
            Ok(format!("math.degrees({})", lower_num(&args[0], defaults)?))
        }
        other => Err(unsupported(format!(
            "Numeric expression `{}` is not supported by the build123d lowerer.",
            other
        ))),
    }
}

fn lower_num_list(
    args: &[Value],
    defaults: &BTreeMap<String, ParamValue>,
) -> AppResult<Vec<String>> {
    args.iter().map(|a| lower_num(a, defaults)).collect()
}

fn fmt_f64(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}.0", n as i64)
    } else {
        // Use Rust's default Display which gives enough precision
        format!("{}", n)
    }
}

fn lower_bool(value: &Value, defaults: &BTreeMap<String, ParamValue>) -> AppResult<String> {
    if let Some(b) = value.as_bool() {
        return Ok(if b { "True".into() } else { "False".into() });
    }
    if let Some(sym) = value.as_symbol() {
        return match defaults.get(sym) {
            Some(ParamValue::Boolean(b)) => Ok(format!(
                "bool(params.get({:?}, {}))",
                sym,
                if *b { "True" } else { "False" }
            )),
            Some(_) => Err(unsupported(format!(
                "Symbol `{}` is not a boolean parameter.",
                sym
            ))),
            None => Err(validation(format!("Unknown symbol `{}`.", sym))),
        };
    }
    let items = list_items(value, "boolean expression")?;
    let op = head_symbol(&items, "boolean expression")?;
    let args = &items[1..];
    match op {
        "not" => {
            if args.len() != 1 {
                return Err(validation("`not` expects one argument."));
            }
            Ok(format!("(not {})", lower_bool(&args[0], defaults)?))
        }
        "and" => {
            let parts = args
                .iter()
                .map(|a| lower_bool(a, defaults))
                .collect::<AppResult<Vec<_>>>()?;
            Ok(format!("({})", parts.join(" and ")))
        }
        "or" => {
            let parts = args
                .iter()
                .map(|a| lower_bool(a, defaults))
                .collect::<AppResult<Vec<_>>>()?;
            Ok(format!("({})", parts.join(" or ")))
        }
        "=" => {
            if args.len() != 2 {
                return Err(validation("`=` expects exactly two arguments."));
            }
            if let (Ok(a), Ok(b)) = (lower_num(&args[0], defaults), lower_num(&args[1], defaults)) {
                return Ok(format!("({a} == {b})"));
            }
            let a = lower_stringish(&args[0], defaults)?;
            let b = lower_stringish(&args[1], defaults)?;
            Ok(format!("({a} == {b})"))
        }
        ">" | ">=" | "<" | "<=" => {
            if args.len() != 2 {
                return Err(validation(format!(
                    "`{}` expects exactly two arguments.",
                    op
                )));
            }
            let a = lower_num(&args[0], defaults)?;
            let b = lower_num(&args[1], defaults)?;
            Ok(format!("({a} {op} {b})"))
        }
        other => Err(unsupported(format!(
            "Boolean operator `{}` is not supported by the build123d lowerer.",
            other
        ))),
    }
}

fn lower_stringish(value: &Value, defaults: &BTreeMap<String, ParamValue>) -> AppResult<String> {
    if let Some(text) = value.as_str() {
        return Ok(format!("{:?}", text));
    }
    if let Some(sym) = value.as_symbol() {
        return match defaults.get(sym) {
            Some(ParamValue::String(s)) => Ok(format!("str(params.get({:?}, {:?}))", sym, s)),
            Some(ParamValue::Number(n)) => {
                Ok(format!("str(params.get({:?}, {}))", sym, fmt_f64(*n)))
            }
            _ => Ok(format!("{:?}", sym)),
        };
    }
    Err(validation("Expected a string-like value."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ecky-ir-test-{}", uuid::Uuid::new_v4()))
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

    #[test]
    fn derive_controls_round_trips_basic_params() {
        let parsed = derive_controls(
            r#"(model
                (params
                  (number width 120 :min 20 :max 300 :step 1 :label "Width")
                  (toggle vents #t :label "Vents")
                  (image litho "" :label "Litho"))
                (part body (cylinder 20 80 32)))"#,
        )
        .expect("controls");
        assert_eq!(parsed.fields.len(), 3);
        assert_eq!(parsed.params.get("width"), Some(&ParamValue::Number(120.0)));
        assert_eq!(parsed.params.get("vents"), Some(&ParamValue::Boolean(true)));
    }

    #[test]
    fn render_model_supports_boolean_mesh_pipeline() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (params
                  (number radius 24)
                  (number wall 2)
                  (number height 80))
                (part body
                  (difference
                    (cylinder radius height 48)
                    (translate 0 0 wall
                      (cylinder (- radius wall) height 48)))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");
        assert_eq!(bundle.engine_kind, EngineKind::EckyIrV0);
        assert!(Path::new(&bundle.preview_stl_path).exists());
        assert!(!bundle.viewer_assets.is_empty());
    }

    #[test]
    fn render_model_reports_unsupported_nodes_explicitly() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let err = render_model(
            r#"(model
                (part body
                  (lithophane "todo")))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect_err("unsupported");
        assert!(
            err.message.contains("Unsupported by Ecky IR v0"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn render_model_supports_loft_taper_and_twist_nodes() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part lofted
                  (translate -50 0 0
                    (loft 28
                      (rounded_rect 24 18 4 12)
                      (scale 0.55 0.75 1 (rounded_rect 24 18 4 12)))))
                (part tapered
                  (taper 32 0.45 0.7
                    (circle 12 40)))
                (part twisted
                  (translate 50 0 0
                    (twist 36 120 10
                      (rounded_rect 12 8 2 8)))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 3);
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }

    #[test]
    fn render_model_supports_mirror_grid_arc_and_xor_nodes() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part body
                  (union
                    (arc-array 5 26 -45 45
                      (box 4 4 12))
                    (grid-array 2 3 14 10
                      (mirror x 0
                        (xor
                          (translate 0 0 2 (cylinder 8 16 36))
                          (box 10 10 10)))))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 1);
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }

    #[test]
    fn render_model_supports_offset_and_shell_nodes() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part ring
                  (extrude
                    (difference
                      (offset-rounded 4 (circle 10 32))
                      (circle 10 32))
                    8))
                (part shell-a
                  (translate 32 0 0
                    (shell 2
                      (cylinder 14 28 48))))
                (part shell-b
                  (translate -32 0 0
                    (shell 1.5
                      (extrude
                        (rounded_rect 18 12 3 10)
                        26)))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 3);
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }

    #[test]
    fn render_model_supports_wall_pattern_modes() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part ribs
                  (wall-pattern
                    (:mode ribs :depth 1.2 :uFreq 14 :softness 0.12)
                    (shell 1.2 (cylinder 18 42 48))))
                (part rings
                  (translate 45 0 0
                    (wall-pattern
                      (:mode rings :depth 1.0 :vFreq 10 :rimFade 0.14)
                      (extrude (rounded_rect 20 14 3 12) 36))))
                (part spiral
                  (translate -45 0 0
                    (wall-pattern
                      (:mode spiral :depth 1.1 :uFreq 11 :twistDeg 180)
                      (revolve
                        (polygon ((10 0) (14 0) (14 28) (10 28)))
                        360 48))))
                (part diamond
                  (translate 0 48 0
                    (wall-pattern
                      (:mode diamond :depth 0.8 :uFreq 12 :vFreq 8)
                      (taper 30 0.6 0.8 (rounded_rect 18 12 2 10)))))
                (part hammered
                  (translate 0 -48 0
                    (wall-pattern
                      (:mode hammered :depth 0.7 :uFreq 9 :vFreq 9 :seed 4)
                      (twist 32 120 10 (rounded_rect 14 10 2 8))))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 5);
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }

    #[test]
    fn wall_pattern_rejects_non_shell_surface_targets() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let err = render_model(
            r#"(model
                (part body
                  (wall-pattern
                    (:mode ribs :depth 1)
                    (box 20 20 20))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect_err("unsupported");

        assert!(
            err.to_string().contains("wall-pattern"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn render_model_supports_hole_aware_sweeps_and_new_primitives() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part complex-profile
                  (extrude
                    (profile
                      (:outer ((0 20) (19 6) (12 -16) (-12 -16) (-19 6)))
                      (:holes ((0 0) (5 0) (5 5) (0 5))))
                    10))
                (part rounded-bspline
                  (translate 50 0 0
                    (loft 20
                      (rounded-polygon ((0 10) (10 0) (0 -10) (-10 0)) 2 8)
                      (bspline ((0 5) (5 0) (0 -5) (-5 0)) #t 12))))
                (part twisted-hollow
                  (translate -50 0 0
                    (shell 2
                      (twist 40 90 12
                        (profile
                          (:outer ((0 15) (15 0) (0 -15) (-15 0)))
                          (:holes ((0 0) (5 0) (5 5) (0 5))))))))
                (part tapered-hollow
                  (translate 0 50 0
                    (shell 1.5
                      (taper 30 0.5 0.5
                        (profile
                          (:outer (circle 15 32))
                          (:holes (circle 8 16))))))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 4);
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }

    #[test]
    fn render_model_supports_wall_pattern_on_complex_shell_sweeps() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model
                (part vase
                  (wall-pattern (:mode ribs :depth 1.5 :uFreq 12)
                    (shell 2
                      (twist 60 45 12
                        (profile
                          (:outer (rounded_rect 30 30 5 12))
                          (:holes (circle 10 32))))))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        assert_eq!(bundle.viewer_assets.len(), 1);
        assert!(Path::new(&bundle.preview_stl_path).exists());
    }

    // ------------------------------------------------------------------
    // Build123d lowering tests
    // ------------------------------------------------------------------

    #[test]
    fn lower_to_build123d_minimal_extrude() {
        let src = r#"(model (part body (extrude (rounded_rect 30 20 4) 10)))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("from build123d import *"), "missing import");
        assert!(
            code.contains("RectangleRounded(30.0, 20.0, 4.0)"),
            "rounded_rect"
        );
        assert!(code.contains("extrude("), "extrude call");
        assert!(code.contains(r#"("body","#), "part entry");
        assert!(code.contains("_ecky_parts"), "_ecky_parts assignment");
    }

    #[test]
    fn lower_to_build123d_difference() {
        let src = r#"(model (part shell (difference (cylinder 10 20) (cylinder 8 20))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Cylinder(10.0, 20.0,"), "outer cylinder");
        assert!(code.contains("Cylinder(8.0, 20.0,"), "inner cylinder");
        assert!(code.contains(" - "), "difference operator");
        assert!(code.contains(r#"("shell","#), "part entry");
    }

    #[test]
    fn lower_to_build123d_param_refs() {
        let src = r#"(model (params (number width 30) (number height 20)) (part body (extrude (rounded_rect width height 4) 10)))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(
            code.contains(r#"float(params.get("width", 30.0))"#),
            "width param: {}",
            code
        );
        assert!(
            code.contains(r#"float(params.get("height", 20.0))"#),
            "height param: {}",
            code
        );
    }

    #[test]
    fn lower_to_build123d_numeric_expressions() {
        let src = r#"(model (params (number w 40)) (part body (extrude (circle (/ w 2)) 5)))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(
            code.contains(r#"float(params.get("w", 40.0)) / 2.0"#),
            "division: {}",
            code
        );
    }

    #[test]
    fn lower_to_build123d_translate_rotate() {
        let src = r#"(model (part body (translate 5 0 0 (rotate 0 0 45 (box 10 10 10)))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Box(10.0, 10.0, 10.0,"), "box");
        assert!(code.contains("Rot(0.0, 0.0, 45.0)"), "rotate");
        assert!(code.contains("Pos(5.0, 0.0, 0.0)"), "translate");
    }

    #[test]
    fn lower_to_build123d_unsupported_node_returns_error() {
        let src = r#"(model (part body (wall-pattern (:mode ribs :depth 1) (shell 2 (cylinder 10 20)))))"#;
        let err = lower_to_build123d(src).unwrap_err();
        assert!(
            err.to_string()
                .contains("not supported by the build123d lowerer"),
            "unexpected: {}",
            err
        );
    }

    #[test]
    fn lower_to_build123d_union_three_parts() {
        let src = r#"(model (part compound (union (sphere 5) (cylinder 3 10) (box 4 4 4))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Sphere(5.0)"), "sphere");
        assert!(code.contains("Cylinder(3.0, 10.0,"), "cylinder");
        assert!(code.contains("Box(4.0, 4.0, 4.0,"), "box");
        let plus_count = code.matches(" + ").count();
        assert_eq!(plus_count, 2, "expected two + for three operands: {}", code);
    }

    #[test]
    fn lower_to_build123d_shell_cylinder() {
        let src = r#"(model (part body (shell 2 (cylinder 10 20))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Cylinder(10.0, 20.0,"), "outer cylinder");
        assert!(code.contains("(10.0) - (2.0)"), "inner radius");
        assert!(code.contains(" - "), "difference");
    }

    #[test]
    fn lower_to_build123d_shell_extrude() {
        let src = r#"(model (part body (shell 1.5 (extrude (circle 12) 20))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Circle(12.0)"), "circle");
        assert!(code.contains("offset("), "offset for inner sketch");
        assert!(code.contains("extrude("), "extrude");
        assert!(code.contains(" - "), "difference");
    }

    #[test]
    fn lower_to_build123d_revolve() {
        let src = r#"(model (part body (revolve (polygon ((10 0) (14 0) (14 20) (10 20))) 360)))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Polygon("), "polygon");
        assert!(code.contains("Rot(90, 0, 0)"), "rotation to XZ");
        assert!(code.contains("revolve("), "revolve call");
        assert!(code.contains("revolution_arc=360.0"), "full revolution");
    }

    #[test]
    fn lower_to_build123d_loft() {
        let src = r#"(model (part body (loft 30 (circle 20) (circle 10))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Circle(20.0)"), "bottom");
        assert!(code.contains("Circle(10.0)"), "top");
        assert!(code.contains("Pos(0, 0, 30.0)"), "height positioning");
        assert!(code.contains("loft("), "loft call");
    }

    #[test]
    fn lower_to_build123d_sweep() {
        let src = r#"(model (part body (sweep (circle 5) (path (0 0 0) (0 0 30)))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Circle(5.0)"), "section");
        assert!(code.contains("Polyline("), "path");
        assert!(code.contains("sweep("), "sweep call");
    }

    #[test]
    fn lower_to_build123d_mirror() {
        let src = r#"(model (part body (mirror x 0 (box 10 10 10))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("mirror("), "mirror call");
        assert!(code.contains("Plane.YZ"), "YZ plane for x-axis mirror");
    }

    #[test]
    fn lower_to_build123d_if_conditional() {
        let src =
            r#"(model (params (toggle cap #t)) (part body (if cap (sphere 10) (cylinder 10 20))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Sphere(10.0)"), "then branch");
        assert!(code.contains("Cylinder(10.0, 20.0,"), "else branch");
        assert!(code.contains("if "), "conditional");
        assert!(code.contains("else"), "else");
        assert!(code.contains("params.get(\"cap\""), "param ref");
    }

    #[test]
    fn lower_to_build123d_linear_array() {
        let src = r#"(model (part body (linear-array 4 10 0 0 (box 5 5 5))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Box(5.0, 5.0, 5.0,"), "base box");
        assert!(code.contains("for _li in range(1, 4)"), "loop");
        assert!(code.contains("Pos(10.0 * _li"), "positioning");
    }

    #[test]
    fn lower_to_build123d_profile_with_holes() {
        let src = r#"(model (part body (extrude (profile (:outer (circle 20)) (:holes (circle 10))) 10)))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Circle(20.0)"), "outer circle");
        assert!(code.contains("Circle(10.0)"), "hole circle");
        assert!(code.contains(" - "), "hole subtraction");
        assert!(code.contains("extrude("), "extrude");
    }

    #[test]
    fn lower_to_build123d_polygon() {
        let src = r#"(model (part body (extrude (polygon ((0 0) (10 0) (10 10) (0 10))) 5)))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Polygon("), "polygon");
        assert!(code.contains("(0.0, 0.0)"), "point");
        assert!(code.contains("extrude("), "extrude");
    }

    #[test]
    fn lower_to_build123d_xor() {
        let src = r#"(model (part body (xor (box 10 10 10) (translate 5 5 0 (box 10 10 10)))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains(" + "), "union for xor");
        assert!(code.contains(" & "), "intersection for xor");
        assert!(code.contains(" - "), "difference for xor");
    }

    #[test]
    fn lower_to_build123d_twist() {
        let src = r#"(model (part body (twist 40 90 (circle 10))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Circle(10.0)"), "sketch");
        assert!(code.contains("Pos(0, 0,"), "height positioning");
        assert!(code.contains("Rot(0, 0,"), "rotation");
        assert!(code.contains("loft("), "loft from sections");
    }

    #[test]
    fn lower_to_build123d_trig_functions() {
        let src = r#"(model (part body (cylinder (sin (deg 45)) 10)))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("math.sin("), "sin");
        assert!(code.contains("math.radians("), "deg → radians");
        assert!(code.contains("import math"), "math import");
    }

    #[test]
    fn lower_to_build123d_bezier_path() {
        let src = r#"(model (part body (sweep (circle 3) (bezier-path ((0 0 0) (5 10 20) (10 30 40) (8 50 50))))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Bezier("), "bezier");
        assert!(code.contains("sweep("), "sweep");
    }

    #[test]
    fn lower_to_build123d_offset() {
        let src = r#"(model (part body (extrude (offset 3 (circle 10)) 5)))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("offset("), "offset call");
        assert!(code.contains("amount=3.0"), "offset amount");
    }

    #[test]
    fn lower_to_build123d_radial_array() {
        let src = r#"(model (part body (radial-array 6 60 20 (cylinder 3 10))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Cylinder(3.0, 10.0,"), "base");
        assert!(code.contains("Pos(20.0, 0, 0)"), "radius offset");
        assert!(code.contains("for _ri in range(1, 6)"), "loop");
        assert!(code.contains("Rot(0, 0, 60.0 * _ri)"), "rotation");
    }

    #[test]
    fn lower_to_build123d_shell_revolve() {
        let src = r#"(model (part body (shell 2 (revolve (polygon ((10 0) (14 0) (14 20) (10 20))) 360))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Polygon("), "polygon");
        assert!(code.contains("offset("), "inner offset");
        assert!(code.contains("revolve("), "revolve call");
        assert!(code.contains(" - "), "difference");
    }

    #[test]
    fn lower_to_build123d_fillet_all_edges() {
        let src = r#"(model (part body (fillet 2 (box 20 20 10))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(code.contains("Box(20.0, 20.0, 10.0,"), "box");
        assert!(code.contains(".edges()"), "edge selection");
        assert!(code.contains("fillet("), "fillet call");
        assert!(code.contains(", 2.0)"), "radius");
    }

    #[test]
    fn lower_to_build123d_fillet_top_edges() {
        let src = r#"(model (part body (fillet 1.5 :edges top (box 20 20 10))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(
            code.contains(".edges().group_by(Axis.Z)[-1]"),
            "top edge selection: {}",
            code
        );
        assert!(code.contains("fillet("), "fillet call");
    }

    #[test]
    fn lower_to_build123d_chamfer_bottom_edges() {
        let src = r#"(model (part body (chamfer 1 :edges bottom (cylinder 10 20))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(
            code.contains(".edges().group_by(Axis.Z)[0]"),
            "bottom edge selection: {}",
            code
        );
        assert!(code.contains("chamfer("), "chamfer call");
    }

    #[test]
    fn lower_to_build123d_fillet_vertical_edges() {
        let src = r#"(model (part body (fillet 3 :edges vertical (box 30 30 20))))"#;
        let code = lower_to_build123d(src).expect("lower");
        assert!(
            code.contains(".edges().filter_by(Axis.Z)"),
            "vertical edge selection: {}",
            code
        );
    }

    #[test]
    fn fillet_box_all_edges() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root };
        let bundle = render_model(
            r#"(model (part body (fillet 2 (box 20 20 10))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("fillet box should render");
        assert!(
            !bundle.viewer_assets.is_empty(),
            "should produce viewer assets"
        );
    }

    #[test]
    fn fillet_box_top_edges() {
        let root = render_root();
        let resolver = TestResolver { root };
        render_model(
            r#"(model (part body (fillet 1.5 :edges "top" (box 20 20 10))))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("fillet box top edges should render");
    }

    #[test]
    fn chamfer_box_all_edges() {
        let root = render_root();
        let resolver = TestResolver { root };
        let src = r#"(model (part body (chamfer 2 (box 20 20 10))))"#;
        let bundle =
            render_model(src, &DesignParams::new(), &resolver).expect("chamfer box should render");
        assert!(
            !bundle.viewer_assets.is_empty(),
            "should produce viewer assets"
        );
    }

    #[test]
    fn chamfer_box_top_edges() {
        let root = render_root();
        let resolver = TestResolver { root };
        let src = r#"(model (part body (chamfer 2 :edges "top" (box 20 20 10))))"#;
        render_model(src, &DesignParams::new(), &resolver)
            .expect("chamfer box top edges should render");
    }

    #[test]
    fn chamfer_cylinder() {
        let root = render_root();
        let resolver = TestResolver { root };
        let src = r#"(model (part body (chamfer 1 (cylinder 10 20))))"#;
        render_model(src, &DesignParams::new(), &resolver).expect("chamfer cylinder should render");
    }

    #[test]
    fn detect_feature_edges_box() {
        let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
        let edges = detect_feature_edges(&mesh);
        assert_eq!(edges.len(), 12, "a box has 12 feature edges");
    }

    #[test]
    fn detect_feature_edges_cylinder() {
        let mesh = IrMesh::cylinder(10.0, 20.0, 32, None);
        let edges = detect_feature_edges(&mesh);
        assert!(
            edges.len() >= 32,
            "cylinder should have at least top rim edges, got {}",
            edges.len()
        );
    }

    #[test]
    fn edge_selector_top_box() {
        let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
        let edges = detect_feature_edges(&mesh);
        let top = filter_edges(&edges, EdgeSelector::Top);
        assert_eq!(top.len(), 4, "box top face has 4 edges");
    }

    #[test]
    fn edge_selector_bottom_box() {
        let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
        let edges = detect_feature_edges(&mesh);
        let bottom = filter_edges(&edges, EdgeSelector::Bottom);
        assert_eq!(bottom.len(), 4, "box bottom face has 4 edges");
    }

    #[test]
    fn edge_selector_vertical_box() {
        let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
        let edges = detect_feature_edges(&mesh);
        let vertical = filter_edges(&edges, EdgeSelector::Vertical);
        assert_eq!(vertical.len(), 4, "box has 4 vertical edges");
    }

    #[test]
    fn chamfer_zero_distance_noop() {
        let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
        let chamfered = chamfer_mesh(&mesh, 0.0, EdgeSelector::All).expect("zero chamfer");
        assert_eq!(
            chamfered.polygons.len(),
            mesh.polygons.len(),
            "zero distance should not modify polygon count"
        );
    }

    #[test]
    fn chamfer_increases_polygon_count() {
        let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
        let original_count = mesh.triangulate().polygons.len();
        let chamfered = chamfer_mesh(&mesh, 1.0, EdgeSelector::All).expect("chamfer all");
        assert!(
            chamfered.polygons.len() > original_count,
            "chamfer should add polygons: {} vs {}",
            chamfered.polygons.len(),
            original_count
        );
    }

    #[test]
    fn fillet_increases_polygon_count() {
        let mesh = IrMesh::cuboid(10.0, 10.0, 10.0, None);
        let original_count = mesh.triangulate().polygons.len();
        let filleted = fillet_mesh(&mesh, 1.0, EdgeSelector::All).expect("fillet all");
        let filleted_count = filleted.triangulate().polygons.len();
        assert!(
            filleted_count > original_count,
            "fillet should add polygons: {} vs {}",
            filleted_count,
            original_count
        );
    }

    #[test]
    fn mesh_volume_unit_cube() {
        // A 10x10x10 cube has volume 1000
        let cube = IrMesh::cuboid(10.0, 10.0, 10.0, None);
        let vol = mesh_volume(&cube).expect("volume should be finite and positive");
        assert!((vol - 1000.0).abs() < 1.0, "expected ~1000, got {}", vol);
    }

    #[test]
    fn mesh_area_unit_cube() {
        // A 10x10x10 cube has surface area 6 * 100 = 600
        let cube = IrMesh::cuboid(10.0, 10.0, 10.0, None);
        let area = mesh_area(&cube).expect("area should be finite and positive");
        assert!((area - 600.0).abs() < 1.0, "expected ~600, got {}", area);
    }

    #[test]
    fn mesh_volume_empty_returns_none() {
        let empty = IrMesh::from_polygons(&[], None);
        assert_eq!(mesh_volume(&empty), None);
    }

    #[test]
    fn mesh_area_empty_returns_none() {
        let empty = IrMesh::from_polygons(&[], None);
        assert_eq!(mesh_area(&empty), None);
    }

    #[test]
    fn render_model_produces_volume_and_area_in_manifest() {
        let root = render_root();
        std::fs::create_dir_all(&root).unwrap();
        let resolver = TestResolver { root: root.clone() };
        let bundle = render_model(
            r#"(model
                (params (number size 10))
                (part body (box size size size)))"#,
            &DesignParams::new(),
            &resolver,
        )
        .expect("render");

        let manifest_str = std::fs::read_to_string(&bundle.manifest_path).unwrap();
        let manifest: ModelManifest = serde_json::from_str(&manifest_str).unwrap();
        assert_eq!(manifest.parts.len(), 1);
        let part = &manifest.parts[0];
        assert!(
            part.volume.is_some(),
            "volume should be computed for IR parts"
        );
        assert!(part.area.is_some(), "area should be computed for IR parts");
        assert!(part.volume.unwrap() > 0.0);
        assert!(part.area.unwrap() > 0.0);
    }
}
