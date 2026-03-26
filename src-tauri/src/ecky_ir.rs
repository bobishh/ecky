use std::collections::BTreeMap;
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
    ManifestBounds, ModelManifest, ModelSourceKind, ParamValue, ParameterGroup, ParsedParamsResult,
    PartBinding, PathResolver, SelectOption, SelectValue, UiField, ViewerAsset, ViewerAssetFormat,
    MODEL_RUNTIME_SCHEMA_VERSION,
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
        "lithophane" => Err(unsupported(
            "Ecky IR v0 does not use a `lithophane` source node. Generate the geometry in IR and drive lithophane through postProcessing.lithophaneAttachments / the LITHO tab instead.",
        )),
        other => Err(unsupported(format!(
            "Node `{}` is not supported by Ecky IR v0.",
            other
        ))),
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
            volume: None,
            area: None,
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
}
