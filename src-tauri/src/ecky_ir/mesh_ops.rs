use std::collections::BTreeMap;
use std::path::Path;

use csgrs::float_types::parry3d::na::{self, Point3, Vector3};
use csgrs::mesh::plane::Plane as IrPlane;
use csgrs::mesh::polygon::Polygon as IrPolygon;
use csgrs::mesh::vertex::Vertex as IrVertex;
use csgrs::traits::CSG;

use crate::contracts::{
    AppResult, ParamValue, RasterTraceCalibration, RasterTraceRequest, SketchView,
};
use crate::ecky_ir_patterns::{
    apply_wall_pattern, WallPatternMode, WallPatternSpec, WallPatternTarget,
};
use crate::image_sampling::{resolve_raster_layout, RasterFitMode, RasterForeground};
use crate::raster_trace::extract_raster_contours;
use crate::surface_trim_cap::SurfaceTrimCapMode;
use crate::surface_trim_external_shapes::SurfaceTrimPathMode;
use crate::surface_trim_runtime::{
    execute_surface_trim, CanonicalSurfaceTrimAnchor, SurfaceTrimRuntimeOutput,
};

use super::edge_ops::{chamfer_mesh, fillet_mesh, parse_edge_selector};
use super::eval_scalar::{
    eval_bool, eval_number, eval_points, eval_points_3d, eval_stringish, parse_count,
};
use super::heightfield::{build_heightfield, build_protrusion};
use super::mesh_literal::{
    build_mesh_literal, MAX_MESH_LITERAL_TRIANGLES, MAX_MESH_LITERAL_VERTICES,
};
use super::model::{
    expr_head_symbol, expr_keyword_name, expr_list_items, expr_parse_stringish, inline_let_expr,
    parse_typed_build_expr, IrExpr,
};
use super::shared::{unsupported, validation, IrMesh, IrSketch};
use super::sketch::{
    bspline_points, circle_points, contour_all_loops, contour_hole_loops, contour_outer_loops,
    contour_set_to_sketch, contour_sweep_slice_from_contours, contours_from_sketch,
    loft_between_sketches, loft_segments, loft_segments_transformed, offset_sketch,
    parse_profile_sketch, rounded_polygon_points, rounded_rectangle_points, sample_bezier_path,
    shell_from_contour_slices, sketch_contours_from_loops, SketchContours,
};

#[derive(Clone, Debug)]
pub(super) struct Frame3d {
    origin: [f64; 3],
    x_axis: [f64; 3],
    y_axis: [f64; 3],
    z_axis: [f64; 3],
}

impl Frame3d {
    fn apply_point(&self, point: [f64; 3]) -> [f64; 3] {
        [
            self.origin[0]
                + point[0] * self.x_axis[0]
                + point[1] * self.y_axis[0]
                + point[2] * self.z_axis[0],
            self.origin[1]
                + point[0] * self.x_axis[1]
                + point[1] * self.y_axis[1]
                + point[2] * self.z_axis[1],
            self.origin[2]
                + point[0] * self.x_axis[2]
                + point[1] * self.y_axis[2]
                + point[2] * self.z_axis[2],
        ]
    }

    fn apply_vector(&self, vector: [f64; 3]) -> [f64; 3] {
        [
            vector[0] * self.x_axis[0] + vector[1] * self.y_axis[0] + vector[2] * self.z_axis[0],
            vector[0] * self.x_axis[1] + vector[1] * self.y_axis[1] + vector[2] * self.z_axis[1],
            vector[0] * self.x_axis[2] + vector[1] * self.y_axis[2] + vector[2] * self.z_axis[2],
        ]
    }
}

#[derive(Clone, Debug)]
pub(super) enum Geometry {
    Mesh(IrMesh),
    Compound(Vec<IrMesh>),
    Sketch(IrSketch),
    Path(Vec<([f64; 3], [f64; 3])>),
    Frame(Frame3d),
}

impl Geometry {
    pub(super) fn into_mesh(self, node: &str) -> AppResult<IrMesh> {
        match self {
            Self::Mesh(mesh) => Ok(mesh),
            Self::Compound(meshes) => Ok(compound_mesh(&meshes)),
            _ => Err(unsupported(format!(
                "Node `{}` expected a 3D solid but received {}.",
                node,
                self.kind_str()
            ))),
        }
    }

    pub(super) fn into_sketch(self, node: &str) -> AppResult<IrSketch> {
        match self {
            Self::Sketch(sketch) => Ok(sketch),
            _ => Err(unsupported(format!(
                "Node `{}` expected a 2D sketch but received {}.",
                node,
                self.kind_str()
            ))),
        }
    }

    pub(super) fn into_path(self, node: &str) -> AppResult<Vec<([f64; 3], [f64; 3])>> {
        match self {
            Self::Path(path) => Ok(path),
            _ => Err(unsupported(format!(
                "Node `{}` expected a 3D path but received {}.",
                node,
                self.kind_str()
            ))),
        }
    }

    pub(super) fn into_solids(self, node: &str) -> AppResult<Vec<IrMesh>> {
        match self {
            Self::Mesh(mesh) => Ok(vec![mesh]),
            Self::Compound(meshes) => Ok(meshes),
            _ => Err(unsupported(format!(
                "Node `{}` expected 3D solids but received {}.",
                node,
                self.kind_str()
            ))),
        }
    }

    pub(super) fn into_frame(self, node: &str) -> AppResult<Frame3d> {
        match self {
            Self::Frame(frame) => Ok(frame),
            _ => Err(unsupported(format!(
                "Node `{}` expected a 3D frame but received {}.",
                node,
                self.kind_str()
            ))),
        }
    }

    fn kind_str(&self) -> &'static str {
        match self {
            Self::Mesh(_) => "a 3D solid",
            Self::Compound(_) => "a 3D compound",
            Self::Sketch(_) => "a 2D sketch",
            Self::Path(_) => "a 3D path",
            Self::Frame(_) => "a 3D frame",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AxisAlign {
    Min,
    Center,
    Max,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Align3d {
    x: AxisAlign,
    y: AxisAlign,
    z: AxisAlign,
}

impl Align3d {
    const fn center_center_min() -> Self {
        Self {
            x: AxisAlign::Center,
            y: AxisAlign::Center,
            z: AxisAlign::Min,
        }
    }

    const fn center_center_center() -> Self {
        Self {
            x: AxisAlign::Center,
            y: AxisAlign::Center,
            z: AxisAlign::Center,
        }
    }
}

/// Pre-triangulate a mesh for STL export, discarding degenerate polygons
/// that would panic `earcutr`.
///
/// After CSG operations on displaced meshes (e.g. `wall-pattern`), some
/// resulting n-gons may have collinear or duplicate vertices that cause
/// `earcutr::add_contour` to panic during triangulation. This function
/// catches those panics via `catch_unwind` and silently drops the
/// offending polygons, producing a clean triangulated mesh where every
/// polygon is a 3-vertex triangle. `csgrs::Mesh::to_stl_binary` has a
/// fast path for triangles that never enters earcutr, so the subsequent
/// export is safe.
pub(super) fn sanitize_mesh_for_export(mesh: &IrMesh) -> IrMesh {
    let mut triangles = Vec::new();
    for poly in &mesh.polygons {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| poly.triangulate()));
        match result {
            Ok(tris) if !tris.is_empty() => {
                for tri in tris {
                    triangles.push(IrPolygon::new(tri.to_vec(), poly.metadata));
                }
            }
            _ => { /* degenerate polygon — skip */ }
        }
    }
    IrMesh::from_polygons(&triangles, mesh.metadata)
}

fn is_empty_mesh(mesh: &IrMesh) -> bool {
    // Check polygon count directly — do NOT call `.triangulate()`.
    // After CSG operations on displaced meshes (e.g. `wall-pattern`),
    // csgrs may produce degenerate polygons (collinear or duplicate
    // vertices) that cause `earcutr` to panic during triangulation.
    // An emptiness check never needs triangulation.
    mesh.polygons.is_empty()
}

fn compound_mesh(meshes: &[IrMesh]) -> IrMesh {
    // Concatenate raw polygons without triangulating. Triangulation is
    // unnecessary for combining meshes and can panic on degenerate
    // polygons produced by CSG ops over displaced (wall-pattern) meshes.
    let mut polygons = Vec::new();
    for mesh in meshes {
        polygons.extend(mesh.polygons.iter().cloned());
    }
    IrMesh::from_polygons(&polygons, None)
}

fn flatten_solids(geometry: Geometry, node: &str) -> AppResult<Vec<IrMesh>> {
    geometry.into_solids(node)
}

fn fuse_solids(mut solids: Vec<IrMesh>) -> Geometry {
    if solids.is_empty() {
        return Geometry::Compound(Vec::new());
    }
    let mut acc = solids.remove(0);
    for next in solids {
        acc = acc.union(&next);
    }
    if is_empty_mesh(&acc) {
        Geometry::Compound(Vec::new())
    } else {
        Geometry::Mesh(acc)
    }
}

fn cut_solids(base: Geometry, cutters: Vec<IrMesh>, node: &str) -> AppResult<Geometry> {
    let mut outputs = Vec::new();
    for mesh in flatten_solids(base, node)? {
        let mut current = mesh;
        for cut in &cutters {
            if is_empty_mesh(cut) {
                continue;
            }
            current = current.difference(cut);
            if is_empty_mesh(&current) {
                break;
            }
        }
        if !is_empty_mesh(&current) {
            outputs.push(current);
        }
    }
    Ok(match outputs.len() {
        0 => Geometry::Compound(Vec::new()),
        1 => Geometry::Mesh(outputs.remove(0)),
        _ => Geometry::Compound(outputs),
    })
}

fn common_solids(
    name: &str,
    args: &[IrExpr],
    env: &BTreeMap<String, ParamValue>,
    bindings: &BTreeMap<String, Geometry>,
) -> AppResult<Geometry> {
    let mut iter = args.iter();
    let first = iter
        .next()
        .ok_or_else(|| validation(format!("`{}` expects at least two operands.", name)))?;
    let current = flatten_solids(eval_geometry_with_bindings(first, env, bindings)?, name)?;
    common_solids_from_first(name, current, iter, env, bindings)
}

fn common_solids_from_first<'a>(
    name: &str,
    mut current: Vec<IrMesh>,
    iter: impl Iterator<Item = &'a IrExpr>,
    env: &BTreeMap<String, ParamValue>,
    bindings: &BTreeMap<String, Geometry>,
) -> AppResult<Geometry> {
    for arg in iter {
        let next = flatten_solids(eval_geometry_with_bindings(arg, env, bindings)?, name)?;
        if current.is_empty() || next.is_empty() {
            return Ok(Geometry::Compound(Vec::new()));
        }
        let mut out = Vec::new();
        for left in &current {
            for right in &next {
                let clipped = left.intersection(right);
                if !is_empty_mesh(&clipped) {
                    out.push(clipped);
                }
            }
        }
        current = out;
        if current.is_empty() {
            return Ok(Geometry::Compound(Vec::new()));
        }
    }
    Ok(match current.len() {
        0 => Geometry::Compound(Vec::new()),
        1 => Geometry::Mesh(current.remove(0)),
        _ => Geometry::Compound(current),
    })
}

fn transform_mesh_with_frame(mesh: &IrMesh, frame: &Frame3d) -> IrMesh {
    let tri_mesh = mesh.triangulate();
    let polygons = tri_mesh
        .polygons
        .iter()
        .map(|poly| {
            let verts = poly
                .vertices
                .iter()
                .map(|vertex| {
                    let pos = frame.apply_point([vertex.pos.x, vertex.pos.y, vertex.pos.z]);
                    let normal =
                        frame.apply_vector([vertex.normal.x, vertex.normal.y, vertex.normal.z]);
                    IrVertex::new(
                        Point3::new(pos[0], pos[1], pos[2]),
                        Vector3::new(normal[0], normal[1], normal[2]),
                    )
                })
                .collect::<Vec<_>>();
            IrPolygon::new(verts, None)
        })
        .collect::<Vec<_>>();
    IrMesh::from_polygons(&polygons, None)
}

fn build_clip_box(x: (f64, f64), y: (f64, f64), z: (f64, f64)) -> IrMesh {
    let width = (x.1 - x.0).max(0.0);
    let depth = (y.1 - y.0).max(0.0);
    let height = (z.1 - z.0).max(0.0);
    let center_x = (x.0 + x.1) * 0.5;
    let center_y = (y.0 + y.1) * 0.5;
    let center_z = (z.0 + z.1) * 0.5;
    IrMesh::cuboid(width, depth, height, None).translate(center_x, center_y, center_z)
}

fn anchor_shift(min: f64, max: f64, align: AxisAlign) -> f64 {
    match align {
        AxisAlign::Min => -min,
        AxisAlign::Center => -((min + max) * 0.5),
        AxisAlign::Max => -max,
    }
}

fn align_mesh_to_origin(mesh: IrMesh, align: Align3d) -> IrMesh {
    let bb = mesh.bounding_box();
    let tx = anchor_shift(bb.mins.x, bb.maxs.x, align.x);
    let ty = anchor_shift(bb.mins.y, bb.maxs.y, align.y);
    let tz = anchor_shift(bb.mins.z, bb.maxs.z, align.z);
    mesh.translate(tx, ty, tz)
}

fn boxes_overlap(mesh: &IrMesh, x: (f64, f64), y: (f64, f64), z: (f64, f64)) -> bool {
    let bb = mesh.bounding_box();
    bb.maxs.x >= x.0
        && bb.mins.x <= x.1
        && bb.maxs.y >= y.0
        && bb.mins.y <= y.1
        && bb.maxs.z >= z.0
        && bb.mins.z <= z.1
}

fn parse_axis_range(
    value: Option<&IrExpr>,
    env: &BTreeMap<String, ParamValue>,
    axis: &str,
) -> AppResult<(f64, f64)> {
    let pair = value.ok_or_else(|| validation(format!("`clip-box` requires `:{}`.", axis)))?;
    let items = expr_list_items(pair, "clip-box axis range")?;
    if items.len() != 2 {
        return Err(validation(format!(
            "`clip-box` axis `:{}` expects `(min max)`.",
            axis
        )));
    }
    let start = eval_number(&items[0], env)?;
    let end = eval_number(&items[1], env)?;
    Ok(if start <= end {
        (start, end)
    } else {
        (end, start)
    })
}

fn split_call_args<'a>(
    node: &str,
    args: &'a [IrExpr],
    allowed_keywords: &[&str],
) -> AppResult<(Vec<&'a IrExpr>, BTreeMap<String, &'a IrExpr>)> {
    let allowed = allowed_keywords
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let mut positional = Vec::new();
    let mut keywords = BTreeMap::new();
    let mut index = 0usize;

    while index < args.len() {
        if let Some(name) = expr_keyword_name(&args[index]) {
            if !allowed.contains(name) {
                return Err(validation(format!(
                    "`{}` does not recognize option `:{}`.",
                    node, name
                )));
            }
            if index + 1 >= args.len() {
                return Err(validation(format!("Keyword `:{}` needs a value.", name)));
            }
            if keywords
                .insert(name.to_string(), &args[index + 1])
                .is_some()
            {
                return Err(validation(format!(
                    "`{}` received duplicate `:{}`.",
                    node, name
                )));
            }
            index += 2;
            continue;
        }
        positional.push(&args[index]);
        index += 1;
    }

    Ok((positional, keywords))
}

fn eval_mesh_vertices(
    operation: &str,
    value: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<Vec<[f64; 3]>> {
    let points = expr_list_items(value, "mesh vertex list")?;
    if points.len() > MAX_MESH_LITERAL_VERTICES {
        return Err(validation(format!(
            "`{operation}` vertices count {} exceeds allowed count {}.",
            points.len(),
            MAX_MESH_LITERAL_VERTICES
        )));
    }
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let point = inline_let_expr(point)?;
            let triple = expr_list_items(&point, "mesh vertex")?;
            if triple.len() != 3 {
                return Err(validation(format!(
                    "`{operation}` vertex {index} expects `(x y z)`."
                )));
            }
            Ok([
                eval_number(&triple[0], env)?,
                eval_number(&triple[1], env)?,
                eval_number(&triple[2], env)?,
            ])
        })
        .collect()
}

fn eval_mesh_triangles(
    operation: &str,
    value: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<Vec<[usize; 3]>> {
    let triangles = expr_list_items(value, "mesh triangle list")?;
    if triangles.len() > MAX_MESH_LITERAL_TRIANGLES {
        return Err(validation(format!(
            "`{operation}` triangles count {} exceeds allowed count {}.",
            triangles.len(),
            MAX_MESH_LITERAL_TRIANGLES
        )));
    }
    triangles
        .iter()
        .enumerate()
        .map(|(triangle_index, triangle)| {
            let triangle = inline_let_expr(triangle)?;
            let triple = expr_list_items(&triangle, "mesh triangle")?;
            if triple.len() != 3 {
                return Err(validation(format!(
                    "`{operation}` triangle {triangle_index} expects three vertex indices."
                )));
            }
            let mut indices = [0usize; 3];
            for (index_index, value) in triple.iter().enumerate() {
                let parsed = eval_number(value, env)?;
                if !parsed.is_finite()
                    || parsed < 0.0
                    || parsed.fract() != 0.0
                    || parsed > usize::MAX as f64
                {
                    return Err(validation(format!(
                        "`{operation}` triangle {triangle_index} index {index_index} must be a non-negative integer, got {parsed}."
                    )));
                }
                indices[index_index] = parsed as usize;
            }
            Ok(indices)
        })
        .collect()
}

fn parse_align_axis(value: &IrExpr, node: &str) -> AppResult<AxisAlign> {
    match value.as_symbol().or_else(|| value.as_str()) {
        Some("min") => Ok(AxisAlign::Min),
        Some("center") => Ok(AxisAlign::Center),
        Some("max") => Ok(AxisAlign::Max),
        Some(other) => Err(validation(format!(
            "`{} :align` expects `min`, `center`, or `max`, got `{}`.",
            node, other
        ))),
        None => Err(validation(format!(
            "`{} :align` expects axis symbols `min`, `center`, or `max`.",
            node
        ))),
    }
}

fn parse_align_3d(value: Option<&IrExpr>, default: Align3d, node: &str) -> AppResult<Align3d> {
    let Some(value) = value else {
        return Ok(default);
    };
    let items = if let Ok(items) = expr_list_items(value, "align tuple") {
        if items.len() == 2 && items.first().and_then(IrExpr::as_symbol) == Some("quote") {
            expr_list_items(&items[1], "align tuple")?
        } else {
            items
        }
    } else {
        expr_list_items(value, "align tuple")?
    };
    if items.len() != 3 {
        return Err(validation(format!("`{} :align` expects `(x y z)`.", node)));
    }
    Ok(Align3d {
        x: parse_align_axis(&items[0], node)?,
        y: parse_align_axis(&items[1], node)?,
        z: parse_align_axis(&items[2], node)?,
    })
}

fn parse_vec3_expr(
    value: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
    context: &str,
) -> AppResult<[f64; 3]> {
    let triple = expr_list_items(value, context)?;
    if triple.len() != 3 {
        return Err(validation(format!("{} expects `(x y z)`.", context)));
    }
    Ok([
        eval_number(&triple[0], env)?,
        eval_number(&triple[1], env)?,
        eval_number(&triple[2], env)?,
    ])
}

fn compose_frame(base_frame: &Frame3d, offset: [f64; 3], rotate: [f64; 3]) -> Frame3d {
    let local_rot = na::Rotation3::from_euler_angles(
        rotate[0].to_radians(),
        rotate[1].to_radians(),
        rotate[2].to_radians(),
    );
    Frame3d {
        origin: base_frame.apply_point(offset),
        x_axis: base_frame.apply_vector((local_rot * Vector3::x()).into()),
        y_axis: base_frame.apply_vector((local_rot * Vector3::y()).into()),
        z_axis: base_frame.apply_vector((local_rot * Vector3::z()).into()),
    }
}

fn extrude_sketch(sketch: &IrSketch, height: f64, symmetric: bool) -> IrMesh {
    let mesh = sketch.extrude(height);
    if symmetric {
        mesh.translate(0.0, 0.0, -height * 0.5)
    } else {
        mesh
    }
}

fn extrude_raster_sketch(sketch: &IrSketch, height: f64, symmetric: bool) -> AppResult<IrMesh> {
    let bottom_z = if symmetric { -height * 0.5 } else { 0.0 };
    let top_z = bottom_z + height;
    loft_between_sketches(sketch, bottom_z, sketch, top_z, "raster extrude")
}

fn raster_foreground(
    value: Option<&&IrExpr>,
    env: &BTreeMap<String, ParamValue>,
    operation: &str,
) -> AppResult<RasterForeground> {
    let Some(value) = value else {
        return Ok(RasterForeground::Dark);
    };
    match eval_stringish(value, env)?.as_str() {
        "dark" => Ok(RasterForeground::Dark),
        "light" => Ok(RasterForeground::Light),
        other => Err(validation(format!(
            "`{operation}` does not recognize `:foreground {other}`. Use `dark` or `light`."
        ))),
    }
}

fn raster_fit(
    value: Option<&&IrExpr>,
    env: &BTreeMap<String, ParamValue>,
    operation: &str,
) -> AppResult<RasterFitMode> {
    let Some(value) = value else {
        return Ok(RasterFitMode::Contain);
    };
    match eval_stringish(value, env)?.as_str() {
        "contain" => Ok(RasterFitMode::Contain),
        "stretch" => Ok(RasterFitMode::Stretch),
        other => Err(validation(format!(
            "`{operation}` does not recognize `:fit {other}`. Use `contain` or `stretch`."
        ))),
    }
}

fn is_raster_source(value: &IrExpr, env: &BTreeMap<String, ParamValue>) -> bool {
    value.as_str().is_some()
        || value
            .as_symbol()
            .and_then(|symbol| env.get(symbol))
            .is_some_and(|value| matches!(value, ParamValue::String(_)))
}

fn raster_sketch(
    image_path: String,
    width: Option<f64>,
    depth: Option<f64>,
    threshold: f64,
    foreground: RasterForeground,
    fit: RasterFitMode,
) -> AppResult<IrSketch> {
    if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) {
        return Err(validation(
            "`extrude` raster `:threshold` must be within 0..=1.",
        ));
    }
    let path = Path::new(&image_path);
    let (source_width, source_height) = image::image_dimensions(path).map_err(|error| {
        validation(format!(
            "`extrude` failed to inspect raster '{}': {error}",
            path.display()
        ))
    })?;
    let layout = resolve_raster_layout(source_width, source_height, width, depth, fit)
        .map_err(|details| validation(format!("raster `extrude` {details}.")))?;
    let response = extract_raster_contours(RasterTraceRequest {
        image_path,
        view: SketchView::Top,
        calibration: RasterTraceCalibration {
            physical_width: layout.width,
            physical_height: layout.depth,
        },
        threshold: (threshold * 255.0).round() as u8,
        invert: foreground == RasterForeground::Light,
    })?;
    let mut contours = SketchContours {
        outer_loops: Vec::new(),
        hole_loops: Vec::new(),
    };
    for contour in response.contours {
        if contour.signed_area < 0.0 {
            contours.outer_loops.push(contour.points);
        } else {
            contours.hole_loops.push(contour.points);
        }
    }
    let contours = sketch_contours_from_loops(contours, "raster extrude")?;
    Ok(contour_set_to_sketch(&contours).translate(layout.offset_x, layout.offset_y, 0.0))
}

fn pick_path_sample(
    path: &[([f64; 3], [f64; 3])],
    at: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<([f64; 3], [f64; 3])> {
    if let Some(symbol) = at.as_symbol() {
        return match symbol {
            "start" => Ok(path[0]),
            "end" => Ok(path[path.len() - 1]),
            other => Err(validation(format!(
                "`path-frame` does not recognize `:at {}`. Use `start`, `end`, or a numeric parameter.",
                other
            ))),
        };
    }

    let t = eval_number(at, env)?.clamp(0.0, 1.0);
    if path.len() == 1 {
        return Ok(path[0]);
    }
    let scaled = t * (path.len() - 1) as f64;
    let index = scaled.floor() as usize;
    if index >= path.len() - 1 {
        return Ok(path[path.len() - 1]);
    }
    let local_t = scaled - index as f64;
    let start = path[index];
    let end = path[index + 1];
    let point = [
        start.0[0] + (end.0[0] - start.0[0]) * local_t,
        start.0[1] + (end.0[1] - start.0[1]) * local_t,
        start.0[2] + (end.0[2] - start.0[2]) * local_t,
    ];
    let tangent = [
        start.1[0] + (end.1[0] - start.1[0]) * local_t,
        start.1[1] + (end.1[1] - start.1[1]) * local_t,
        start.1[2] + (end.1[2] - start.1[2]) * local_t,
    ];
    Ok((point, tangent))
}

fn normalize_vec(vector: [f64; 3], label: &str) -> AppResult<Vector3<f64>> {
    let vec = Vector3::new(vector[0], vector[1], vector[2]);
    let norm = vec.norm();
    if norm <= 1e-9 {
        return Err(validation(format!("{} must not be zero-length.", label)));
    }
    Ok(vec / norm)
}

fn build_frame(origin: [f64; 3], tangent: [f64; 3], up: [f64; 3]) -> AppResult<Frame3d> {
    let x_axis = normalize_vec(tangent, "`path-frame` tangent")?;
    let mut up_vec = normalize_vec(up, "`path-frame :up`")?;
    if x_axis.cross(&up_vec).norm() <= 1e-6 {
        up_vec = if x_axis.z.abs() < 0.99 {
            Vector3::z()
        } else {
            Vector3::y()
        };
    }
    let z_axis = x_axis.cross(&up_vec).normalize().cross(&x_axis).normalize();
    let y_axis = z_axis.cross(&x_axis).normalize();
    Ok(Frame3d {
        origin,
        x_axis: [x_axis.x, x_axis.y, x_axis.z],
        y_axis: [y_axis.x, y_axis.y, y_axis.z],
        z_axis: [z_axis.x, z_axis.y, z_axis.z],
    })
}
pub(super) fn parse_wall_pattern_spec(
    value: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<WallPatternSpec> {
    let items = expr_list_items(value, "wall-pattern options")?;
    parse_wall_pattern_spec_items(items, env)
}

fn parse_wall_pattern_spec_items(
    items: &[IrExpr],
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<WallPatternSpec> {
    if items.is_empty() || !items.len().is_multiple_of(2) {
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
        let name = expr_keyword_name(&items[index]).ok_or_else(|| {
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
                    "fourier" => WallPatternMode::Fourier,
                    "cellular" => WallPatternMode::Cellular,
                    "fbm" => WallPatternMode::Fbm,
                    "gyroid" => WallPatternMode::Gyroid,
                    "schwarz-p" => WallPatternMode::SchwarzP,
                    "diamond-field" | "schwarz-d" => WallPatternMode::SchwarzD,
                    "neovius" => WallPatternMode::Neovius,
                    "attractor-field" => WallPatternMode::AttractorField,
                    other => {
                        return Err(unsupported(format!(
                            "wall-pattern mode `{}` is not supported by current `.ecky` runtime.",
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
pub(super) fn build_wall_pattern_target(
    value: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
    bindings: &BTreeMap<String, Geometry>,
) -> AppResult<(IrMesh, WallPatternTarget)> {
    let items = expr_list_items(value, "wall-pattern target")?;
    let node = expr_head_symbol(items, "wall-pattern target")?;
    let args = &items[1..];

    match node {
        "extrude" => {
            if args.len() != 2 {
                return Err(validation("`extrude` expects a sketch and height."));
            }
            let sketch = eval_geometry_with_bindings(&args[0], env, bindings)?
                .into_sketch("wall-pattern")?;
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
            let sketch = eval_geometry_with_bindings(&args[sketch_index], env, bindings)?
                .into_sketch("wall-pattern")?;
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
            let sketch = eval_geometry_with_bindings(&args[sketch_index], env, bindings)?
                .into_sketch("wall-pattern")?;
            let mut slices = Vec::with_capacity(segments + 1);
            for index in 0..=segments {
                let t = index as f64 / segments as f64;
                let z = height * t;
                let rotated = sketch.rotate(0.0, 0.0, angle_deg * t);
                let contours = contours_from_sketch(&rotated, "wall-pattern")?;
                let blocked_loops = contour_hole_loops(&contours);
                slices.push(contour_sweep_slice_from_contours(
                    &contours,
                    blocked_loops,
                    z,
                ));
            }
            Ok((
                twist_mesh(&sketch, height, angle_deg, segments, "wall-pattern")?,
                WallPatternTarget::ContourSweep { slices },
            ))
        }
        "revolve" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(validation(
                    "`revolve` expects a sketch, angle, and optional segments.",
                ));
            }
            let sketch = eval_geometry_with_bindings(&args[0], env, bindings)?
                .into_sketch("wall-pattern")?;
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
                return Err(validation(
                    "`shell` expects wall thickness and a supported solid node.",
                ));
            }
            let wall = eval_number(&args[0], env)?;
            let mesh = eval_shell_geometry(&args[1], wall, env, bindings)?;
            let shell_items = expr_list_items(&args[1], "wall-pattern shell target")?;
            let shell_node = expr_head_symbol(shell_items, "wall-pattern shell target")?;
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
                    let outer_sketch = eval_geometry_with_bindings(&shell_args[0], env, bindings)?.into_sketch("wall-pattern")?;
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
                    let outer_sketch = eval_geometry_with_bindings(&shell_args[0], env, bindings)?.into_sketch("wall-pattern")?;
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
                    let bottom = eval_geometry_with_bindings(&shell_args[1], env, bindings)?.into_sketch("wall-pattern")?;
                    let top = eval_geometry_with_bindings(&shell_args[2], env, bindings)?.into_sketch("wall-pattern")?;
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
                    let base = eval_geometry_with_bindings(&shell_args[sketch_index], env, bindings)?.into_sketch("wall-pattern")?;
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
                    let base = eval_geometry_with_bindings(&shell_args[sketch_index], env, bindings)?.into_sketch("wall-pattern")?;
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
        _ => {
            let mesh =
                eval_geometry_with_bindings(value, env, bindings)?.into_mesh("wall-pattern")?;
            let bounds = mesh.bounding_box();
            let mins = [bounds.mins.x, bounds.mins.y, bounds.mins.z];
            let maxs = [bounds.maxs.x, bounds.maxs.y, bounds.maxs.z];
            let spans = [
                (maxs[0] - mins[0]).abs(),
                (maxs[1] - mins[1]).abs(),
                (maxs[2] - mins[2]).abs(),
            ];
            let normal_axis = spans
                .iter()
                .enumerate()
                .min_by(|(_, left), (_, right)| left.total_cmp(right))
                .map(|(axis, _)| axis)
                .unwrap_or(2);
            Ok((
                mesh,
                WallPatternTarget::MeshBounds {
                    mins,
                    maxs,
                    normal_axis,
                },
            ))
        }
    }
}

pub(super) fn revolve_mesh(
    sketch: &IrSketch,
    angle: f64,
    segments: usize,
    node: &str,
) -> AppResult<IrMesh> {
    sketch
        .clone()
        .rotate(90.0, 0.0, 0.0)
        .revolve(angle, segments.max(12))
        .map_err(|err| validation(format!("`{}` failed: {:?}", node, err)))
}

pub(super) fn taper_mesh(
    sketch: &IrSketch,
    height: f64,
    scale_x: f64,
    scale_y: f64,
    node: &str,
) -> AppResult<IrMesh> {
    let top = sketch.scale(scale_x, scale_y, 1.0);
    loft_between_sketches(sketch, 0.0, &top, height, node)
}

pub(super) fn twist_mesh(
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

pub(super) fn mirror_plane(axis: &str, offset: f64) -> AppResult<IrPlane> {
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
pub(super) fn sweep_mesh(
    sketch: &IrSketch,
    path: &[([f64; 3], [f64; 3])],
    node: &str,
) -> AppResult<IrMesh> {
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

pub(super) fn eval_shell_geometry(
    target: &IrExpr,
    wall: f64,
    env: &BTreeMap<String, ParamValue>,
    bindings: &BTreeMap<String, Geometry>,
) -> AppResult<IrMesh> {
    if wall <= 0.0 {
        return Err(validation("`shell` expects a positive wall thickness."));
    }
    let items = expr_list_items(target, "shell target")?;
    let node = expr_head_symbol(items, "shell target")?;
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
            let sketch = eval_geometry_with_bindings(&args[0], env, bindings)?.into_sketch("shell")?;
            let height = eval_number(&args[1], env)?;
            let inner_sketch = offset_sketch(&sketch, -wall, false, "shell")?;
            Ok(sketch.extrude(height).difference(&inner_sketch.extrude(height)))
        }
        "revolve" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(validation("`shell` revolve expects a sketch, angle, and optional segments."));
            }
            let sketch = eval_geometry_with_bindings(&args[0], env, bindings)?.into_sketch("shell")?;
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
            let bottom = eval_geometry_with_bindings(&args[1], env, bindings)?.into_sketch("shell")?;
            let top = eval_geometry_with_bindings(&args[2], env, bindings)?.into_sketch("shell")?;
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
            let base = eval_geometry_with_bindings(&args[sketch_index], env, bindings)?.into_sketch("shell")?;
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
            let base = eval_geometry_with_bindings(&args[sketch_index], env, bindings)?.into_sketch("shell")?;
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
            let sketch = eval_geometry_with_bindings(&args[0], env, bindings)?.into_sketch("shell")?;
            let path = eval_geometry_with_bindings(&args[1], env, bindings)?.into_path("shell")?;
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

pub(super) fn eval_geometry_expr(
    value: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<Geometry> {
    let bindings = BTreeMap::new();
    eval_geometry_with_bindings(value, env, &bindings)
}

pub(super) fn eval_geometry_with_bindings(
    value: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
    bindings: &BTreeMap<String, Geometry>,
) -> AppResult<Geometry> {
    let value = inline_let_expr(value)?;
    if let Some(symbol) = value.as_symbol() {
        return bindings
            .get(symbol)
            .cloned()
            .ok_or_else(|| validation(format!("Unknown symbol `{}`.", symbol)));
    }
    let items = expr_list_items(&value, "geometry node")?;
    let node = expr_head_symbol(items, "geometry node")?;
    let args = &items[1..];
    match node {
        "build" => {
            let build = parse_typed_build_expr(&value)?;
            let mut local_bindings = bindings.clone();
            let mut local_env = env.clone();
            for binding in build.bindings {
                if let Ok(number) = eval_number(&binding.expr, &local_env) {
                    local_env.insert(binding.name, ParamValue::Number(number));
                    continue;
                }
                if let Ok(flag) = eval_bool(&binding.expr, &local_env) {
                    local_env.insert(binding.name, ParamValue::Boolean(flag));
                    continue;
                }
                let geometry =
                    eval_geometry_with_bindings(&binding.expr, &local_env, &local_bindings)?;
                local_bindings.insert(binding.name, geometry);
            }
            eval_geometry_with_bindings(&build.result, &local_env, &local_bindings)
        }
        "compound" => {
            if args.is_empty() {
                return Err(validation("`compound` expects at least one solid operand."));
            }
            let mut solids = Vec::new();
            for arg in args {
                solids.extend(flatten_solids(
                    eval_geometry_with_bindings(arg, env, bindings)?,
                    "compound",
                )?);
            }
            Ok(Geometry::Compound(solids))
        }
        "fuse" => {
            if args.len() < 2 {
                return Err(validation("`fuse` expects at least two solid operands."));
            }
            let mut solids = Vec::new();
            for arg in args {
                solids.extend(flatten_solids(
                    eval_geometry_with_bindings(arg, env, bindings)?,
                    "fuse",
                )?);
            }
            Ok(fuse_solids(solids))
        }
        "cut" => {
            if args.len() < 2 {
                return Err(validation("`cut` expects a target and at least one cutter."));
            }
            let base = eval_geometry_with_bindings(&args[0], env, bindings)?;
            let mut cutters = Vec::new();
            for arg in &args[1..] {
                cutters.extend(flatten_solids(
                    eval_geometry_with_bindings(arg, env, bindings)?,
                    "cut",
                )?);
            }
            cut_solids(base, cutters, "cut")
        }
        "common" => common_solids("common", args, env, bindings),
        "heightfield" => {
            let (positional, keywords) = split_call_args(
                "heightfield",
                args,
                &["width", "depth", "relief-height", "base-thickness", "invert"],
            )?;
            if positional.len() != 1 {
                return Err(validation(
                    "`heightfield` expects one image path plus dimension keywords.",
                ));
            }
            let dimension = |name: &str| -> AppResult<f64> {
                keywords
                    .get(name)
                    .copied()
                    .ok_or_else(|| validation(format!("`heightfield` requires `:{name}`.")))
                    .and_then(|value| eval_number(value, env))
            };
            Ok(Geometry::Mesh(build_heightfield(
                &eval_stringish(positional[0], env)?,
                dimension("width")?,
                dimension("depth")?,
                dimension("relief-height")?,
                dimension("base-thickness")?,
                keywords
                    .get("invert")
                    .map(|value| eval_bool(value, env))
                    .transpose()?
                    .unwrap_or(false),
            )?))
        }
        "mesh" | "polyhedron" => {
            let (positional, keywords) =
                split_call_args(node, args, &["vertices", "triangles"])?;
            if !positional.is_empty() {
                return Err(validation(format!(
                    "`{node}` accepts only `:vertices` and `:triangles`."
                )));
            }
            let vertices = keywords.get("vertices").copied().ok_or_else(|| {
                validation(format!("`{node}` requires `:vertices`."))
            })?;
            let triangles = keywords.get("triangles").copied().ok_or_else(|| {
                validation(format!("`{node}` requires `:triangles`."))
            })?;
            Ok(Geometry::Mesh(build_mesh_literal(
                node,
                eval_mesh_vertices(node, vertices, env)?,
                eval_mesh_triangles(node, triangles, env)?,
                node == "polyhedron",
            )?))
        }
        "box" => {
            let (positional, keywords) = split_call_args("box", args, &["align"])?;
            if positional.len() != 3 {
                return Err(validation("`box` expects width, depth, and height."));
            }
            let width = eval_number(positional[0], env)?;
            let depth = eval_number(positional[1], env)?;
            let height = eval_number(positional[2], env)?;
            let align = parse_align_3d(keywords.get("align").copied(), Align3d::center_center_min(), "box")?;
            Ok(Geometry::Mesh(align_mesh_to_origin(
                IrMesh::cuboid(width, depth, height, None),
                align,
            )))
        }
        "cylinder" => {
            let (positional, keywords) = split_call_args("cylinder", args, &["align"])?;
            if positional.len() < 2 || positional.len() > 3 {
                return Err(validation("`cylinder` expects radius, height, and optional segments."));
            }
            let radius = eval_number(positional[0], env)?;
            let height = eval_number(positional[1], env)?;
            let segments = positional
                .get(2)
                .map(|arg| eval_number(arg, env))
                .transpose()?
                .unwrap_or(48.0) as usize;
            let align = parse_align_3d(
                keywords.get("align").copied(),
                Align3d::center_center_min(),
                "cylinder",
            )?;
            Ok(Geometry::Mesh(align_mesh_to_origin(
                IrMesh::cylinder(radius, height, segments.max(12), None),
                align,
            )))
        }
        "cone" => {
            let (positional, keywords) = split_call_args("cone", args, &["align"])?;
            if positional.len() < 3 || positional.len() > 4 {
                return Err(validation(
                    "`cone` expects bottom radius, top radius, height, and optional segments.",
                ));
            }
            let bottom_radius = eval_number(positional[0], env)?;
            let top_radius = eval_number(positional[1], env)?;
            let height = eval_number(positional[2], env)?;
            let segments = positional
                .get(3)
                .map(|arg| eval_number(arg, env))
                .transpose()?
                .unwrap_or(48.0) as usize;
            let align = parse_align_3d(keywords.get("align").copied(), Align3d::center_center_min(), "cone")?;
            Ok(Geometry::Mesh(align_mesh_to_origin(
                IrMesh::frustum(bottom_radius, top_radius, height, segments.max(12), None),
                align,
            )))
        }
        "sphere" => {
            let (positional, keywords) = split_call_args("sphere", args, &["align"])?;
            if positional.is_empty() || positional.len() > 3 {
                return Err(validation("`sphere` expects radius and optional slices/stacks."));
            }
            let radius = eval_number(positional[0], env)?;
            let slices = positional
                .get(1)
                .map(|arg| eval_number(arg, env))
                .transpose()?
                .unwrap_or(48.0) as usize;
            let stacks = positional
                .get(2)
                .map(|arg| eval_number(arg, env))
                .transpose()?
                .unwrap_or(24.0) as usize;
            let align = parse_align_3d(
                keywords.get("align").copied(),
                Align3d::center_center_center(),
                "sphere",
            )?;
            Ok(Geometry::Mesh(align_mesh_to_origin(
                IrMesh::sphere(radius, slices.max(12), stacks.max(6), None),
                align,
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
            let sketch = eval_geometry_with_bindings(&args[1], env, bindings)?.into_sketch(node)?;
            Ok(Geometry::Sketch(offset_sketch(
                &sketch,
                distance,
                node == "offset-rounded",
                node,
            )?))
        }
        "extrude" => {
            let (positional, keywords) = split_call_args(
                "extrude",
                args,
                &[
                    "symmetric",
                    "width",
                    "depth",
                    "threshold",
                    "foreground",
                    "fit",
                ],
            )?;
            if positional.len() != 2 {
                return Err(validation("`extrude` expects a sketch or image path and height."));
            }
            let height = eval_number(positional[1], env)?;
            let symmetric = keywords
                .get("symmetric")
                .map(|value| eval_bool(value, env))
                .transpose()?
                .unwrap_or(false);
            let raster_source = is_raster_source(positional[0], env);
            let sketch = if raster_source {
                let dimension = |name: &str| -> AppResult<Option<f64>> {
                    keywords
                        .get(name)
                        .copied()
                        .map(|value| eval_number(value, env))
                        .transpose()
                };
                raster_sketch(
                    eval_stringish(positional[0], env)?,
                    dimension("width")?,
                    dimension("depth")?,
                    keywords
                        .get("threshold")
                        .map(|value| eval_number(value, env))
                        .transpose()?
                        .unwrap_or(0.5),
                    raster_foreground(keywords.get("foreground"), env, "extrude")?,
                    raster_fit(keywords.get("fit"), env, "extrude")?,
                )?
            } else {
                for keyword in ["width", "depth", "threshold", "foreground", "fit"] {
                    if keywords.contains_key(keyword) {
                        return Err(validation(format!(
                            "`extrude` accepts `:{keyword}` only when its first operand is an image path."
                        )));
                    }
                }
                eval_geometry_with_bindings(positional[0], env, bindings)?
                    .into_sketch("extrude")?
            };
            let mesh = if raster_source {
                // csgrs Sketch::extrude can stitch separate raster islands
                // through shared cap vertices. Build caps and side walls from
                // the classified contour set so every island/hole stays closed.
                extrude_raster_sketch(&sketch, height, symmetric)?
            } else {
                extrude_sketch(&sketch, height, symmetric)
            };
            Ok(Geometry::Mesh(mesh))
        }
        "protrude" => {
            let (positional, keywords) =
                split_call_args("protrude", args, &["width", "depth", "foreground", "fit"])?;
            if positional.len() != 2 {
                return Err(validation("`protrude` expects an image path and height."));
            }
            let dimension = |name: &str| -> AppResult<Option<f64>> {
                keywords
                    .get(name)
                    .copied()
                    .map(|value| eval_number(value, env))
                    .transpose()
            };
            Ok(Geometry::Mesh(build_protrusion(
                &eval_stringish(positional[0], env)?,
                dimension("width")?,
                dimension("depth")?,
                eval_number(positional[1], env)?,
                raster_foreground(keywords.get("foreground"), env, "protrude")?,
                raster_fit(keywords.get("fit"), env, "protrude")?,
            )?))
        }
        "revolve" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(validation("`revolve` expects a sketch, angle, and optional segments."));
            }
            let sketch = eval_geometry_with_bindings(&args[0], env, bindings)?.into_sketch("revolve")?;
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
            let bottom = eval_geometry_with_bindings(&args[1], env, bindings)?.into_sketch("loft")?;
            let top = eval_geometry_with_bindings(&args[2], env, bindings)?.into_sketch("loft")?;
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
            let base = eval_geometry_with_bindings(&args[sketch_index], env, bindings)?.into_sketch("taper")?;
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
            let base = eval_geometry_with_bindings(&args[sketch_index], env, bindings)?.into_sketch("twist")?;
            Ok(Geometry::Mesh(twist_mesh(
                &base, height, angle, segments, "twist",
            )?))
        }
        "sweep" => {
            if args.len() != 2 {
                return Err(validation("`sweep` expects a sketch and a path."));
            }
            let sketch = eval_geometry_with_bindings(&args[0], env, bindings)?.into_sketch("sweep")?;
            let path = eval_geometry_with_bindings(&args[1], env, bindings)?.into_path("sweep")?;
            Ok(Geometry::Mesh(sweep_mesh(&sketch, &path, "sweep")?))
        }
        "path" => {
            let mut points = Vec::with_capacity(args.len());
            for arg in args {
                let triple = expr_list_items(arg, "3D point")?;
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
        "plane" => {
            let (positional, keywords) = split_call_args("plane", args, &["origin", "x", "normal"])?;
            if !positional.is_empty() {
                return Err(validation(
                    "`plane` expects only keyword options `:origin`, `:x`, and `:normal`.",
                ));
            }
            let origin = match keywords.get("origin") {
                Some(value) => parse_vec3_expr(value, env, "`plane :origin`")?,
                None => [0.0, 0.0, 0.0],
            };
            let x_axis = match keywords.get("x") {
                Some(value) => parse_vec3_expr(value, env, "`plane :x`")?,
                None => [1.0, 0.0, 0.0],
            };
            let normal = match keywords.get("normal") {
                Some(value) => parse_vec3_expr(value, env, "`plane :normal`")?,
                None => [0.0, 0.0, 1.0],
            };
            Ok(Geometry::Frame(build_frame(origin, x_axis, normal)?))
        }
        "location" => {
            let (positional, keywords) = split_call_args("location", args, &["offset", "rotate"])?;
            if positional.len() != 1 {
                return Err(validation(
                    "`location` expects a plane/frame and optional `:offset` / `:rotate`.",
                ));
            }
            let base_frame =
                eval_geometry_with_bindings(positional[0], env, bindings)?.into_frame("location")?;
            let offset = match keywords.get("offset") {
                Some(value) => parse_vec3_expr(value, env, "`location :offset`")?,
                None => [0.0, 0.0, 0.0],
            };
            let rotate = match keywords.get("rotate") {
                Some(value) => parse_vec3_expr(value, env, "`location :rotate`")?,
                None => [0.0, 0.0, 0.0],
            };
            Ok(Geometry::Frame(compose_frame(&base_frame, offset, rotate)))
        }
        "path-frame" => {
            let (positional, keywords) = split_call_args("path-frame", args, &["at", "up"])?;
            if positional.len() != 1 {
                return Err(validation("`path-frame` expects a path."));
            }
            let path =
                eval_geometry_with_bindings(positional[0], env, bindings)?.into_path("path-frame")?;
            let at_value = keywords
                .get("at")
                .map(|value| value.dup())
                .unwrap_or_else(|| IrExpr::symbol("end"));
            let up = match keywords.get("up") {
                Some(value) => parse_vec3_expr(value, env, "`path-frame :up`")?,
                None => [0.0, 0.0, 1.0],
            };
            let (origin, tangent) = pick_path_sample(&path, &at_value, env)?;
            Ok(Geometry::Frame(build_frame(origin, tangent, up)?))
        }
        "place" => {
            let (positional, keywords) = split_call_args("place", args, &["offset", "rotate"])?;
            if positional.len() != 2 {
                return Err(validation(
                    "`place` expects a frame, geometry, and optional `:offset` / `:rotate`.",
                ));
            }
            let base_frame =
                eval_geometry_with_bindings(positional[0], env, bindings)?.into_frame("place")?;
            let geometry = eval_geometry_with_bindings(positional[1], env, bindings)?;
            let offset = match keywords.get("offset") {
                Some(value) => parse_vec3_expr(value, env, "`place :offset`")?,
                None => [0.0, 0.0, 0.0],
            };
            let rotate = match keywords.get("rotate") {
                Some(value) => parse_vec3_expr(value, env, "`place :rotate`")?,
                None => [0.0, 0.0, 0.0],
            };
            let placed_frame = compose_frame(&base_frame, offset, rotate);
            Ok(match geometry {
                Geometry::Mesh(mesh) => Geometry::Mesh(transform_mesh_with_frame(&mesh, &placed_frame)),
                Geometry::Compound(meshes) => Geometry::Compound(
                    meshes
                        .iter()
                        .map(|mesh| transform_mesh_with_frame(mesh, &placed_frame))
                        .collect(),
                ),
                Geometry::Path(path) => Geometry::Path(
                    path.into_iter()
                        .map(|(point, tangent)| {
                            (
                                placed_frame.apply_point(point),
                                placed_frame.apply_vector(tangent),
                            )
                        })
                        .collect(),
                ),
                Geometry::Frame(frame) => Geometry::Frame(Frame3d {
                    origin: placed_frame.apply_point(frame.origin),
                    x_axis: placed_frame.apply_vector(frame.x_axis),
                    y_axis: placed_frame.apply_vector(frame.y_axis),
                    z_axis: placed_frame.apply_vector(frame.z_axis),
                }),
                Geometry::Sketch(_) => {
                    return Err(unsupported(
                        "Node `place` does not yet support 2D sketches in the EckyRust backend.",
                    ))
                }
            })
        }
        "clip-box" => {
            let (positional, keywords) = split_call_args("clip-box", args, &["x", "y", "z"])?;
            if positional.len() != 1 {
                return Err(validation("`clip-box` expects a shape and axis ranges."));
            }
            let shape = eval_geometry_with_bindings(positional[0], env, bindings)?;
            let x = parse_axis_range(keywords.get("x").copied(), env, "x")?;
            let y = parse_axis_range(keywords.get("y").copied(), env, "y")?;
            let z = parse_axis_range(keywords.get("z").copied(), env, "z")?;
            let clipper = build_clip_box(x, y, z);
            let mut outputs = Vec::new();
            for mesh in flatten_solids(shape, "clip-box")? {
                if !boxes_overlap(&mesh, x, y, z) {
                    continue;
                }
                let clipped = mesh.intersection(&clipper);
                if !is_empty_mesh(&clipped) {
                    outputs.push(clipped);
                }
            }
            Ok(match outputs.len() {
                0 => Geometry::Compound(Vec::new()),
                1 => Geometry::Mesh(outputs.remove(0)),
                _ => Geometry::Compound(outputs),
            })
        }
        "shell" => {
            if args.len() != 2 {
                return Err(validation("`shell` expects wall thickness and a supported solid node."));
            }
            let wall = eval_number(&args[0], env)?;
            Ok(Geometry::Mesh(eval_shell_geometry(&args[1], wall, env, bindings)?))
        }
        "union" => fold_boolean_geometry(
            "union",
            args,
            env,
            bindings,
            |left, right| left.union(&right),
            |left, right| left.union(&right),
        ),
        "difference" => fold_boolean_geometry(
            "difference",
            args,
            env,
            bindings,
            |left, right| left.difference(&right),
            |left, right| left.difference(&right),
        ),
        "intersection" => fold_boolean_geometry(
            "intersection",
            args,
            env,
            bindings,
            |left, right| left.intersection(&right),
            |left, right| left.intersection(&right),
        ),
        "xor" => fold_boolean_geometry(
            "xor",
            args,
            env,
            bindings,
            |left, right| left.xor(&right),
            |left, right| left.xor(&right),
        ),
        "translate" => transform_mesh_or_sketch("translate", args, env, bindings, |shape, x, y, z| match shape {
            Geometry::Mesh(mesh) => Geometry::Mesh(mesh.translate(x, y, z)),
            Geometry::Compound(meshes) => Geometry::Compound(
                meshes
                    .into_iter()
                    .map(|mesh| mesh.translate(x, y, z))
                    .collect(),
            ),
            Geometry::Sketch(sketch) => Geometry::Sketch(sketch.translate(x, y, z)),
            Geometry::Path(path) => Geometry::Path(
                path.into_iter()
                    .map(|(p, t)| ([p[0] + x, p[1] + y, p[2] + z], t))
                    .collect(),
            ),
            Geometry::Frame(frame) => Geometry::Frame(Frame3d {
                origin: [frame.origin[0] + x, frame.origin[1] + y, frame.origin[2] + z],
                ..frame
            }),
        }),
        "rotate" => transform_mesh_or_sketch("rotate", args, env, bindings, |shape, x, y, z| match shape {
            Geometry::Mesh(mesh) => Geometry::Mesh(mesh.rotate(x, y, z)),
            Geometry::Compound(meshes) => Geometry::Compound(
                meshes
                    .into_iter()
                    .map(|mesh| mesh.rotate(x, y, z))
                    .collect(),
            ),
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
            Geometry::Frame(frame) => {
                let rot =
                    na::Rotation3::from_euler_angles(x.to_radians(), y.to_radians(), z.to_radians());
                Geometry::Frame(Frame3d {
                    origin: (rot * Point3::from(frame.origin)).into(),
                    x_axis: (rot * Vector3::from(frame.x_axis)).into(),
                    y_axis: (rot * Vector3::from(frame.y_axis)).into(),
                    z_axis: (rot * Vector3::from(frame.z_axis)).into(),
                })
            }
        }),
        "scale" => transform_mesh_or_sketch("scale", args, env, bindings, |shape, x, y, z| match shape {
            Geometry::Mesh(mesh) => Geometry::Mesh(mesh.scale(x, y, z)),
            Geometry::Compound(meshes) => Geometry::Compound(
                meshes
                    .into_iter()
                    .map(|mesh| mesh.scale(x, y, z))
                    .collect(),
            ),
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
            Geometry::Frame(frame) => Geometry::Frame(Frame3d {
                origin: [frame.origin[0] * x, frame.origin[1] * y, frame.origin[2] * z],
                x_axis: [frame.x_axis[0] * x, frame.x_axis[1] * y, frame.x_axis[2] * z],
                y_axis: [frame.y_axis[0] * x, frame.y_axis[1] * y, frame.y_axis[2] * z],
                z_axis: [frame.z_axis[0] * x, frame.z_axis[1] * y, frame.z_axis[2] * z],
            }),
        }),
        "mirror" => {
            if args.len() != 3 {
                return Err(validation("`mirror` expects axis, offset, and a geometry node."));
            }
            let axis = expr_parse_stringish(&args[0], "mirror axis")?;
            let offset = eval_number(&args[1], env)?;
            let plane = mirror_plane(axis.as_str(), offset)?;
            Ok(match eval_geometry_with_bindings(&args[2], env, bindings)? {
                Geometry::Mesh(mesh) => Geometry::Mesh(mesh.mirror(plane)),
                Geometry::Compound(meshes) => Geometry::Compound(
                    meshes
                        .into_iter()
                        .map(|mesh| mesh.mirror(plane.clone()))
                        .collect(),
                ),
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
                Geometry::Frame(frame) => {
                    let normal = plane.normal();
                    let reflect_point = |point: [f64; 3]| {
                        let pt = Point3::from(point);
                        let dist = normal.dot(&pt.coords) - plane.offset();
                        let reflected: Point3<f64> = pt - 2.0 * dist * normal;
                        [reflected.x, reflected.y, reflected.z]
                    };
                    let reflect_vector = |vector: [f64; 3]| {
                        let vec = Vector3::from(vector);
                        let dist = normal.dot(&vec);
                        let reflected: Vector3<f64> = vec - 2.0 * dist * normal;
                        [reflected.x, reflected.y, reflected.z]
                    };
                    Geometry::Frame(Frame3d {
                        origin: reflect_point(frame.origin),
                        x_axis: reflect_vector(frame.x_axis),
                        y_axis: reflect_vector(frame.y_axis),
                        z_axis: reflect_vector(frame.z_axis),
                    })
                }
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
            let base = eval_geometry_with_bindings(&args[4], env, bindings)?.into_mesh("linear-array")?;
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
            let base = eval_geometry_with_bindings(&args[4], env, bindings)?.into_mesh("grid-array")?;
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
            let base = eval_geometry_with_bindings(&args[3], env, bindings)?.into_mesh("radial-array")?;
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
            let base = eval_geometry_with_bindings(&args[4], env, bindings)?.into_mesh("arc-array")?;
            Ok(Geometry::Mesh(
                base.distribute_arc(count, radius, start_degrees, end_degrees),
            ))
        }
        "if" => {
            if args.len() != 3 {
                return Err(validation("`if` expects condition, then-shape, else-shape."));
            }
            if eval_bool(&args[0], env)? {
                eval_geometry_with_bindings(&args[1], env, bindings)
            } else {
                eval_geometry_with_bindings(&args[2], env, bindings)
            }
        }
        "wall-pattern" | "pattern" => {
            let (spec, target_expr) = if args.len() == 2 && expr_keyword_name(&args[1]).is_none() {
                (parse_wall_pattern_spec(&args[0], env)?, &args[1])
            } else if args.len() >= 3 && expr_keyword_name(&args[1]).is_some() {
                (parse_wall_pattern_spec_items(&args[1..], env)?, &args[0])
            } else {
                return Err(validation(
                    "`wall-pattern` expects an option list and a supported shell-surface target.",
                ));
            };
            let (mesh, target) = build_wall_pattern_target(target_expr, env, bindings)?;
            Ok(Geometry::Mesh(apply_wall_pattern(&mesh, &target, &spec)?))
        }
        "surface-trim" => {
            let (source_path, source_digest, loop_anchors, keep_seed, path_mode, cap_mode) =
                parse_surface_trim_call(args, env)?;
            let output = execute_surface_trim(
                Path::new(&source_path),
                &source_digest,
                &loop_anchors,
                &keep_seed,
                path_mode,
                cap_mode,
            )?;
            Ok(Geometry::Mesh(surface_trim_output_to_mesh(output)?))
        }
        "chamfer" => Err(validation(
            "`chamfer` is a CAD BRep surface operation and must run on the Direct OCCT backend. Use `mesh-chamfer` explicitly for mesh-native geometry.",
        )),
        "mesh-chamfer" => {
            if args.len() < 2 {
                return Err(validation("`mesh-chamfer` expects distance and a geometry node."));
            }
            let distance = eval_number(&args[0], env)?;
            let (selector, body_index) = parse_edge_selector(args, env)?;
            if body_index >= args.len() {
                return Err(validation(
                    "`mesh-chamfer` is missing the geometry body argument.",
                ));
            }
            let mesh = eval_geometry_with_bindings(&args[body_index], env, bindings)?
                .into_mesh("mesh-chamfer")?;
            Ok(Geometry::Mesh(chamfer_mesh(&mesh, distance, selector)?))
        }
        "fillet" => Err(validation(
            "`fillet` is a CAD BRep surface operation and must run on the Direct OCCT backend. Use `mesh-fillet` explicitly for mesh-native geometry.",
        )),
        "mesh-fillet" => {
            if args.len() < 2 {
                return Err(validation("`mesh-fillet` expects radius and a geometry node."));
            }
            let radius = eval_number(&args[0], env)?;
            let (selector, body_index) = parse_edge_selector(args, env)?;
            if body_index >= args.len() {
                return Err(validation(
                    "`mesh-fillet` is missing the geometry body argument.",
                ));
            }
            let mesh = eval_geometry_with_bindings(&args[body_index], env, bindings)?
                .into_mesh("mesh-fillet")?;
            Ok(Geometry::Mesh(fillet_mesh(&mesh, radius, selector)?))
        }
        "import-stl" => {
            if args.len() != 1 {
                return Err(validation("`import-stl` expects one STL file path."));
            }
            let path = eval_stringish(&args[0], env)?;
            Ok(Geometry::Mesh(read_stl_mesh(Path::new(&path))?))
        }
        "mesh-bridge-overlap" => {
            if args.len() != 1 {
                return Err(validation("`mesh-bridge-overlap` expects one mesh."));
            }
            let mesh = eval_geometry_with_bindings(&args[0], env, bindings)?
                .into_mesh("mesh-bridge-overlap")?;
            Ok(Geometry::Mesh(add_mesh_bridge_overlap(&mesh)))
        }
        "text" | "svg" => Err(unsupported(format!(
            "Node `{}` is not supported by the EckyRust backend. Switch to FreeCAD or build123d.",
            node
        ))),
        "lithophane" => Err(unsupported(
            "`.ecky` does not use a `lithophane` source node. Generate the geometry in `.ecky` and drive lithophane through postProcessing.lithophaneAttachments / the LITHO tab instead.",
        )),
        other => Err(unsupported(format!(
            "Node `{}` is not supported by current `.ecky` runtime.",
            other
        ))),
    }
}

fn parse_surface_trim_call(
    args: &[IrExpr],
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<(
    String,
    String,
    Vec<CanonicalSurfaceTrimAnchor>,
    CanonicalSurfaceTrimAnchor,
    SurfaceTrimPathMode,
    SurfaceTrimCapMode,
)> {
    let (positional, keywords) = split_call_args(
        "surface-trim",
        args,
        &[
            "schema-version",
            "source-digest",
            "loop",
            "keep-seed",
            "path-mode",
            "cap",
        ],
    )?;
    if positional.len() != 1 {
        return Err(validation(
            "`surface-trim` expects exactly one imported mesh shape.",
        ));
    }
    let source_path = parse_surface_trim_import_path(positional[0], env)?;
    let schema_version = eval_number(
        required_surface_trim_keyword(&keywords, "schema-version")?,
        env,
    )?;
    if !schema_version.is_finite()
        || schema_version.fract().abs() > f64::EPSILON
        || schema_version != crate::surface_trim_external_shapes::SURFACE_TRIM_SCHEMA_VERSION as f64
    {
        return Err(validation(format!(
            "Unsupported `surface-trim` schema version {}; expected {}.",
            schema_version,
            crate::surface_trim_external_shapes::SURFACE_TRIM_SCHEMA_VERSION
        )));
    }
    let source_digest = eval_stringish(
        required_surface_trim_keyword(&keywords, "source-digest")?,
        env,
    )?;
    let loop_items = expr_list_items(
        required_surface_trim_keyword(&keywords, "loop")?,
        "surface-trim loop",
    )?;
    if loop_items.len() < 3 {
        return Err(validation(
            "`surface-trim` loop requires at least three mesh anchors.",
        ));
    }
    let loop_anchors = loop_items
        .iter()
        .map(|value| parse_surface_trim_anchor(value, env))
        .collect::<AppResult<Vec<_>>>()?;
    let keep_seed =
        parse_surface_trim_anchor(required_surface_trim_keyword(&keywords, "keep-seed")?, env)?;
    let path_mode =
        match eval_stringish(required_surface_trim_keyword(&keywords, "path-mode")?, env)?.as_str()
        {
            "shortest" => SurfaceTrimPathMode::Shortest,
            "feature" => SurfaceTrimPathMode::Feature,
            other => {
                return Err(validation(format!(
                    "`surface-trim` path mode '{}' is invalid; expected shortest or feature.",
                    other
                )))
            }
        };
    let cap_mode =
        match eval_stringish(required_surface_trim_keyword(&keywords, "cap")?, env)?.as_str() {
            "open" => SurfaceTrimCapMode::Open,
            "flat" => SurfaceTrimCapMode::Flat,
            "surface-fill" => SurfaceTrimCapMode::SurfaceFill,
            other => {
                return Err(validation(format!(
                "`surface-trim` cap mode '{}' is invalid; expected open, flat, or surface-fill.",
                other
            )))
            }
        };
    Ok((
        source_path,
        source_digest,
        loop_anchors,
        keep_seed,
        path_mode,
        cap_mode,
    ))
}

fn required_surface_trim_keyword<'a>(
    keywords: &'a BTreeMap<String, &'a IrExpr>,
    name: &str,
) -> AppResult<&'a IrExpr> {
    keywords
        .get(name)
        .copied()
        .ok_or_else(|| validation(format!("`surface-trim` requires `:{}`.", name)))
}

fn parse_surface_trim_import_path(
    value: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<String> {
    let items = expr_list_items(value, "surface-trim source")?;
    if expr_head_symbol(items, "surface-trim source")? != "import-stl" || items.len() != 2 {
        return Err(validation(
            "`surface-trim` source must be one direct `(import-stl path)` node so mesh-anchor indices resolve against immutable source triangles.",
        ));
    }
    eval_stringish(&items[1], env)
}

fn parse_surface_trim_anchor(
    value: &IrExpr,
    env: &BTreeMap<String, ParamValue>,
) -> AppResult<CanonicalSurfaceTrimAnchor> {
    let items = expr_list_items(value, "surface-trim mesh anchor")?;
    if expr_head_symbol(items, "surface-trim mesh anchor")? != "mesh-anchor" || items.len() != 5 {
        return Err(validation(
            "A `surface-trim` anchor must be `(mesh-anchor triangle-index b0 b1 b2)`.",
        ));
    }
    let raw_triangle_index = eval_number(&items[1], env)?;
    if !raw_triangle_index.is_finite()
        || raw_triangle_index < 0.0
        || raw_triangle_index.fract().abs() > f64::EPSILON
        || raw_triangle_index > u64::MAX as f64
    {
        return Err(validation(
            "`mesh-anchor` triangle index must be a non-negative integer.",
        ));
    }
    Ok(CanonicalSurfaceTrimAnchor {
        triangle_index: raw_triangle_index as u64,
        barycentric: [
            eval_number(&items[2], env)?,
            eval_number(&items[3], env)?,
            eval_number(&items[4], env)?,
        ],
    })
}

fn surface_trim_output_to_mesh(output: SurfaceTrimRuntimeOutput) -> AppResult<IrMesh> {
    let mut polygons = Vec::with_capacity(output.triangles.len());
    for triangle in output.triangles {
        let indices = triangle.map(|index| index as usize);
        if indices.iter().any(|index| *index >= output.vertices.len()) {
            return Err(validation(
                "`surface-trim` runtime emitted a triangle index outside its vertex buffer.",
            ));
        }
        let points = indices.map(|index| {
            let point = output.vertices[index];
            Point3::new(point[0], point[1], point[2])
        });
        let cross = (points[1] - points[0]).cross(&(points[2] - points[0]));
        if !cross.norm().is_finite() || cross.norm() <= 1.0e-12 {
            return Err(validation(
                "`surface-trim` runtime emitted a degenerate triangle.",
            ));
        }
        let normal = cross.normalize();
        polygons.push(IrPolygon::new(
            points
                .into_iter()
                .map(|point| IrVertex::new(point, normal))
                .collect(),
            None,
        ));
    }
    if polygons.is_empty() {
        return Err(validation("`surface-trim` removed every source triangle."));
    }
    Ok(IrMesh::from_polygons(&polygons, None))
}

fn add_mesh_bridge_overlap(mesh: &IrMesh) -> IrMesh {
    const OVERLAP_MM: f64 = 0.4;
    const TAPER_INSET_MM: f64 = 0.2;
    let bounds = mesh.bounding_box();
    let mins = [bounds.mins.x, bounds.mins.y, bounds.mins.z];
    let maxs = [bounds.maxs.x, bounds.maxs.y, bounds.maxs.z];
    let spans = [
        (maxs[0] - mins[0]).abs(),
        (maxs[1] - mins[1]).abs(),
        (maxs[2] - mins[2]).abs(),
    ];
    let axis = spans
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(axis, _)| axis)
        .unwrap_or(2);
    let tolerance = spans[axis].max(1.0) * 1.0e-6;
    let centers = [
        (mins[0] + maxs[0]) * 0.5,
        (mins[1] + maxs[1]) * 0.5,
        (mins[2] + maxs[2]) * 0.5,
    ];
    let mut overlapped = mesh.clone();
    for polygon in &mut overlapped.polygons {
        for vertex in &mut polygon.vertices {
            if vertex.pos[axis] >= maxs[axis] - tolerance {
                vertex.pos[axis] += OVERLAP_MM;
                for orthogonal_axis in 0..3 {
                    if orthogonal_axis == axis || spans[orthogonal_axis] <= tolerance {
                        continue;
                    }
                    let inset = TAPER_INSET_MM.min(spans[orthogonal_axis] * 0.1);
                    let scale = (spans[orthogonal_axis] - 2.0 * inset) / spans[orthogonal_axis];
                    vertex.pos[orthogonal_axis] = centers[orthogonal_axis]
                        + (vertex.pos[orthogonal_axis] - centers[orthogonal_axis]) * scale;
                }
            }
        }
        polygon.plane = IrPlane::from_vertices(polygon.vertices.clone());
        polygon.bounding_box = std::sync::OnceLock::new();
    }
    overlapped.bounding_box = std::sync::OnceLock::new();
    overlapped
}

pub(crate) fn read_stl_mesh(path: &Path) -> AppResult<IrMesh> {
    let bytes = std::fs::read(path)
        .map_err(|err| validation(format!("Failed to read STL '{}': {err}", path.display())))?;
    let triangles = if bytes.len() >= 84 {
        let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        if 84usize.checked_add(count.saturating_mul(50)) == Some(bytes.len()) {
            let mut triangles = Vec::with_capacity(count);
            for index in 0..count {
                let mut offset = 84 + index * 50 + 12;
                let mut triangle = [[0.0; 3]; 3];
                for vertex in &mut triangle {
                    for coordinate in vertex {
                        *coordinate = f32::from_le_bytes(
                            bytes[offset..offset + 4]
                                .try_into()
                                .expect("checked binary STL coordinate"),
                        ) as f64;
                        offset += 4;
                    }
                }
                triangles.push(triangle);
            }
            triangles
        } else {
            parse_ascii_stl_mesh(&bytes, path)?
        }
    } else {
        parse_ascii_stl_mesh(&bytes, path)?
    };
    if triangles.is_empty() {
        return Err(validation(format!(
            "STL '{}' contains no triangles.",
            path.display()
        )));
    }
    let polygons = triangles
        .into_iter()
        .filter_map(|triangle| {
            let points = triangle.map(|point| Point3::new(point[0], point[1], point[2]));
            let cross = (points[1] - points[0]).cross(&(points[2] - points[0]));
            if cross.norm() <= 1.0e-12 {
                return None;
            }
            let normal = cross.normalize();
            Some(IrPolygon::new(
                points
                    .into_iter()
                    .map(|point| IrVertex::new(point, normal))
                    .collect(),
                None,
            ))
        })
        .collect::<Vec<_>>();
    if polygons.is_empty() {
        return Err(validation(format!(
            "STL '{}' contains only degenerate triangles.",
            path.display()
        )));
    }
    Ok(IrMesh::from_polygons(&polygons, None))
}

fn parse_ascii_stl_mesh(bytes: &[u8], path: &Path) -> AppResult<Vec<[[f64; 3]; 3]>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|err| validation(format!("STL '{}' is invalid: {err}", path.display())))?;
    let mut vertices = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        if !parts
            .next()
            .is_some_and(|token| token.eq_ignore_ascii_case("vertex"))
        {
            continue;
        }
        let point = [
            parts.next().and_then(|value| value.parse().ok()),
            parts.next().and_then(|value| value.parse().ok()),
            parts.next().and_then(|value| value.parse().ok()),
        ];
        let [Some(x), Some(y), Some(z)] = point else {
            return Err(validation(format!(
                "STL '{}' contains an invalid vertex.",
                path.display()
            )));
        };
        vertices.push([x, y, z]);
    }
    if !vertices.len().is_multiple_of(3) {
        return Err(validation(format!(
            "STL '{}' contains an incomplete facet.",
            path.display()
        )));
    }
    Ok(vertices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|chunk| [chunk[0], chunk[1], chunk[2]])
        .collect())
}
pub(super) fn fold_boolean_geometry(
    name: &str,
    args: &[IrExpr],
    env: &BTreeMap<String, ParamValue>,
    bindings: &BTreeMap<String, Geometry>,
    _combine_mesh: impl Fn(IrMesh, IrMesh) -> IrMesh,
    combine_sketch: impl Fn(IrSketch, IrSketch) -> IrSketch,
) -> AppResult<Geometry> {
    if args.len() < 2 {
        return Err(validation(format!(
            "`{}` expects at least two geometry operands.",
            name
        )));
    }
    let mut iter = args.iter();
    let first_geom = eval_geometry_with_bindings(iter.next().expect("checked"), env, bindings)?;
    match first_geom {
        Geometry::Mesh(_) | Geometry::Compound(_) => {
            let first = flatten_solids(first_geom, name)?;
            match name {
                "union" => {
                    let mut solids = first;
                    for arg in iter {
                        solids.extend(flatten_solids(
                            eval_geometry_with_bindings(arg, env, bindings)?,
                            name,
                        )?);
                    }
                    Ok(fuse_solids(solids))
                }
                "difference" => {
                    let mut cuts = Vec::new();
                    for arg in iter {
                        cuts.extend(flatten_solids(
                            eval_geometry_with_bindings(arg, env, bindings)?,
                            name,
                        )?);
                    }
                    let base = match first.len() {
                        0 => Geometry::Compound(Vec::new()),
                        1 => Geometry::Mesh(first.into_iter().next().unwrap()),
                        _ => Geometry::Compound(first),
                    };
                    cut_solids(base, cuts, name)
                }
                "intersection" => common_solids_from_first(name, first, iter, env, bindings),
                "xor" => {
                    if args.len() != 2 {
                        return Err(validation("`xor` expects exactly two solid operands."));
                    }
                    let left = first.clone();
                    let right = flatten_solids(
                        eval_geometry_with_bindings(&args[1], env, bindings)?,
                        name,
                    )?;
                    let union =
                        fuse_solids(left.iter().cloned().chain(right.iter().cloned()).collect())
                            .into_mesh("xor")?;
                    let inter =
                        common_solids_from_first(name, first, [&args[1]].into_iter(), env, bindings)?
                            .into_mesh("xor")?;
                    if is_empty_mesh(&inter) {
                        Ok(Geometry::Mesh(union))
                    } else {
                        Ok(Geometry::Mesh(union.difference(&inter)))
                    }
                }
                _ => unreachable!(),
            }
        }
        Geometry::Sketch(first) => {
            let sketch = iter.try_fold(first, |acc, arg| {
                match eval_geometry_with_bindings(arg, env, bindings)? {
                    Geometry::Sketch(next) => Ok(combine_sketch(acc, next)),
                    other => Err(unsupported(format!(
                        "Node `{}` cannot mix 2D sketches and {} in one boolean expression.",
                        name,
                        other.kind_str()
                    ))),
                }
            })?;
            Ok(Geometry::Sketch(sketch))
        }
        Geometry::Path(_) | Geometry::Frame(_) => Err(unsupported(format!(
            "Node `{}` does not support boolean operations on 3D paths.",
            name
        ))),
    }
}

pub(super) fn transform_mesh_or_sketch(
    name: &str,
    args: &[IrExpr],
    env: &BTreeMap<String, ParamValue>,
    bindings: &BTreeMap<String, Geometry>,
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
    Ok(transform(
        eval_geometry_with_bindings(&args[3], env, bindings)?,
        x,
        y,
        z,
    ))
}
