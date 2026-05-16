from build123d import *

__all__ = [
    "_ecky_intersect_x",
    "_ecky_face",
    "_ecky_polygon",
    "_ecky_wire_from_segments",
    "_ecky_face_from_wires",
    "_ecky_face_with_holes",
    "_ecky_apply_transform",
    "_ecky_solid",
    "_ecky_has_solids",
    "_ecky_collect_solids",
    "_ecky_compound",
    "_ecky_difference_solid",
    "_ecky_intersection_solid",
    "_ecky_fuse_many",
    "_ecky_cut_many",
    "_ecky_common_many",
    "_ecky_select_edges",
    "_ecky_select_shell_faces",
    "_ecky_path_frame",
    "_ecky_as_location",
    "_ecky_location",
    "_ecky_place",
    "_ecky_extrude",
    "_ecky_loft",
    "_ecky_clip_box",
    "_ecky_non_uniform_scale",
]

from OCP.BRepBuilderAPI import BRepBuilderAPI_GTransform
from OCP.gp import gp_GTrsf


def _ecky_intersect_x(shape, z):
    try:
        pts = shape.find_intersection_points(Axis(origin=(0, 0, z), direction=(1, 0, 0)))
        if not pts:
            return 0.0
        pt = pts[-1][0] if isinstance(pts[-1], (list, tuple)) else pts[-1]
        return pt.X
    except Exception:
        return 0.0


def _ecky_face(shape):
    try:
        faces = shape.faces()
        if len(faces) == 1:
            return faces[0]
        if len(faces) > 1:
            return shape
    except Exception:
        pass
    try:
        return make_face(Wire(shape.edges()))
    except Exception:
        return shape


def _ecky_polygon(*points):
    if len(points) == 1 and isinstance(points[0], (list, tuple)):
        first = points[0]
        if first and isinstance(first[0], (list, tuple)):
            pts = list(first)
        else:
            pts = list(points)
    else:
        pts = list(points)
    return Polyline(*pts, close=True)


def _ecky_wire_from_segments(*segments):
    edges = []
    for segment in segments:
        try:
            edges.extend(list(segment.edges()))
            continue
        except Exception:
            pass
        try:
            edges.append(segment)
        except Exception:
            pass
    return Wire(edges)


def _ecky_face_from_wires(*wires):
    edges = []
    for wire in wires:
        try:
            edges.extend(list(wire.edges()))
        except Exception:
            pass
    if not edges:
        return Compound(children=[])
    try:
        return make_face(Wire(edges))
    except Exception:
        return _ecky_face(_ecky_compound(*wires))


def _ecky_face_with_holes(outer_wire, *hole_wires):
    return Face(outer_wire, list(hole_wires))


def _ecky_apply_transform(transform, shape):
    try:
        solids = _ecky_collect_solids(shape)
        if len(solids) == 1:
            return transform * solids[0]
        if len(solids) > 1:
            return Compound(children=[transform * solid for solid in solids])
    except Exception:
        pass
    try:
        return transform * shape
    except Exception:
        return Compound(children=[])


def _ecky_non_uniform_scale(shape, sx, sy, sz):
    gtrsf = gp_GTrsf()
    gtrsf.SetValue(1, 1, float(sx))
    gtrsf.SetValue(2, 2, float(sy))
    gtrsf.SetValue(3, 3, float(sz))
    transformed = BRepBuilderAPI_GTransform(shape.wrapped, gtrsf, True).Shape()
    for caster in (Solid.cast, Compound.cast, Face.cast, Wire.cast, Edge.cast, Vertex.cast):
        try:
            wrapped = caster(transformed)
            return _ecky_solid(wrapped) if _ecky_has_solids(wrapped) else wrapped
        except Exception:
            continue
    return shape


def _ecky_solid(shape):
    try:
        solids = list(shape.solids())
        if len(solids) == 1:
            return solids[0]
        if len(solids) > 1:
            return Compound(children=solids)
    except Exception:
        pass
    return shape


def _ecky_has_solids(shape):
    try:
        return len(list(shape.solids())) > 0
    except Exception:
        return False


def _ecky_collect_solids(shape):
    try:
        return list(shape.solids())
    except Exception:
        return []


def _ecky_compound(*shapes):
    solids = []
    for shape in shapes:
        solids.extend(_ecky_collect_solids(shape))
    return Compound(children=solids)


def _ecky_difference_solid(base, *cuts):
    if not _ecky_has_solids(base):
        return Compound(children=[])
    out = _ecky_solid(base)
    for cut in cuts:
        if not _ecky_has_solids(cut):
            continue
        out = out - _ecky_solid(cut)
    return _ecky_solid(out)


def _ecky_intersection_solid(*shapes):
    non_empty = [shape for shape in shapes if _ecky_has_solids(shape)]
    if not non_empty:
        return Compound(children=[])
    out = _ecky_solid(non_empty[0])
    for shape in non_empty[1:]:
        out = out & _ecky_solid(shape)
        if not _ecky_has_solids(out):
            return Compound(children=[])
    return _ecky_solid(out)


def _ecky_fuse_many(*shapes):
    solids = []
    for shape in shapes:
        solids.extend(_ecky_collect_solids(shape))
    if not solids:
        return Compound(children=[])
    if len(solids) == 1:
        return solids[0]
    return _ecky_solid(solids[0].fuse(*solids[1:]))


def _ecky_cut_many(base, *cuts):
    base_solids = _ecky_collect_solids(base)
    cut_solids = []
    for cut in cuts:
        cut_solids.extend(_ecky_collect_solids(cut))
    if not base_solids:
        return Compound(children=[])
    if not cut_solids:
        return base_solids[0] if len(base_solids) == 1 else Compound(children=base_solids)
    cutter = cut_solids[0] if len(cut_solids) == 1 else Compound(children=cut_solids)
    out = []
    for solid in base_solids:
        out.extend(_ecky_collect_solids(solid - cutter))
    if not out:
        return Compound(children=[])
    return out[0] if len(out) == 1 else Compound(children=out)


def _ecky_common_many(*shapes):
    buckets = [_ecky_collect_solids(shape) for shape in shapes]
    if any(len(bucket) == 0 for bucket in buckets):
        return Compound(children=[])
    current = buckets[0]
    for bucket in buckets[1:]:
        out = []
        for left in current:
            hit = left.intersect(*bucket)
            out.extend(_ecky_collect_solids(hit))
        current = out
        if not current:
            return Compound(children=[])
    return current[0] if len(current) == 1 else Compound(children=current)


_ECKY_EDGE_SELECTOR_HELP = (
    "`all`, `top`, `bottom`, `left`, `right`, `front`, `back`, `vertical`, "
    "`axis-x`, `axis-y`, `axis-z`, `x-min`, `x-max`, `y-min`, `y-max`, `z-min`, `z-max`, "
    "`target-id:<id>`, `target-ids:<id>|<id>`, or `+` intersections such as `x-min+axis-z`."
)

_ECKY_FACE_SELECTOR_HELP = (
    "`all`, `planar`, `normal-x`, `normal-y`, `normal-z`, `area-min`, `area-max`, "
    "`top`, `bottom`, `left`, `right`, `front`, `back`, "
    "`x-min`, `x-max`, `y-min`, `y-max`, `z-min`, `z-max`, "
    "`target-id:<id>`, `target-ids:<id>|<id>`, or `+` intersections such as `planar+normal-z+z-max`."
)


def _ecky_edge_selector_error(selector):
    raise ValueError(f"Unknown edge selector `{selector}`. Use {_ECKY_EDGE_SELECTOR_HELP}")


def _ecky_face_selector_error(selector):
    raise ValueError(f"Unknown face selector `{selector}`. Use {_ECKY_FACE_SELECTOR_HELP}")


def _ecky_selector_kind(selector):
    if isinstance(selector, dict):
        return str(selector.get("kind") or "").strip()
    return ""


def _ecky_selector_target_ids(selector):
    if selector is None:
        return None
    if not isinstance(selector, dict):
        raise ValueError(f"Edge selector `{selector}` requires typed selector payload.")
    if _ecky_selector_kind(selector) != "targetIds":
        return None
    target_ids = []
    for item in selector.get("targetIds") or []:
        text = str(item).strip()
        if text and text not in target_ids:
            target_ids.append(text)
    if not target_ids:
        raise ValueError(f"Edge selector `{selector}` did not include any target ids.")
    return target_ids


def _ecky_face_selector_target_ids(selector):
    if selector is None:
        return None
    if not isinstance(selector, dict):
        raise ValueError(f"Face selector `{selector}` requires typed selector payload.")
    if _ecky_selector_kind(selector) != "targetIds":
        return None
    target_ids = []
    for item in selector.get("targetIds") or []:
        text = str(item).strip()
        if text and ":face:" in text and text not in target_ids:
            target_ids.append(text)
    if not target_ids:
        raise ValueError(f"Face selector `{selector}` did not include any face target ids.")
    return target_ids


def _ecky_selector_clauses(selector):
    if selector is None:
        return []
    if not isinstance(selector, dict):
        raise ValueError(f"Edge selector `{selector}` requires typed selector payload.")
    kind = _ecky_selector_kind(selector)
    if kind == "all":
        return []
    if kind != "clauses":
        _ecky_edge_selector_error(selector)
    clauses = []
    for clause in selector.get("clauses") or []:
        if not isinstance(clause, dict):
            _ecky_edge_selector_error(selector)
        clause_kind = str(clause.get("kind") or "").strip()
        if clause_kind == "axis":
            axis = str(clause.get("axis") or "").strip().lower()
            if axis not in ("x", "y", "z"):
                _ecky_edge_selector_error(selector)
            clauses.append(("axis", axis))
            continue
        if clause_kind == "boundary":
            axis = str(clause.get("axis") or "").strip().lower()
            bound = str(clause.get("bound") or "").strip().lower()
            if axis not in ("x", "y", "z") or bound not in ("min", "max"):
                _ecky_edge_selector_error(selector)
            clauses.append(("boundary", axis, bound))
            continue
        _ecky_edge_selector_error(selector)
    return clauses


def _ecky_face_selector_clauses(selector):
    if selector is None:
        return []
    if not isinstance(selector, dict):
        raise ValueError(f"Face selector `{selector}` requires typed selector payload.")
    kind = _ecky_selector_kind(selector)
    if kind == "all":
        return []
    if kind != "clauses":
        _ecky_face_selector_error(selector)
    clauses = []
    for clause in selector.get("clauses") or []:
        if not isinstance(clause, dict):
            _ecky_face_selector_error(selector)
        clause_kind = str(clause.get("kind") or "").strip().lower()
        if clause_kind == "boundary":
            axis = str(clause.get("axis") or "").strip().lower()
            bound = str(clause.get("bound") or "").strip().lower()
            if axis not in ("x", "y", "z") or bound not in ("min", "max"):
                _ecky_face_selector_error(selector)
            clauses.append(("boundary", axis, bound))
            continue
        if clause_kind == "planar":
            clauses.append(("planar",))
            continue
        if clause_kind == "normal":
            axis = str(clause.get("axis") or "").strip().lower()
            if axis not in ("x", "y", "z"):
                _ecky_face_selector_error(selector)
            clauses.append(("normal", axis))
            continue
        if clause_kind == "area":
            rank = str(clause.get("rank") or "").strip().lower()
            if rank not in ("min", "max"):
                _ecky_face_selector_error(selector)
            clauses.append(("area", rank))
            continue
        _ecky_face_selector_error(selector)
    return clauses


def _ecky_axis_value(vec, axis):
    return {"x": float(vec.X), "y": float(vec.Y), "z": float(vec.Z)}[axis]


def _ecky_format_coordinate(value):
    value = float(value)
    if abs(value) < 0.0005:
        return "0"
    text = f"{value:.3f}".rstrip("0").rstrip(".")
    return "0" if text in ("", "-0") else text


def _ecky_point_signature(point):
    return "-".join(
        _ecky_format_coordinate(coord)
        for coord in (point.X, point.Y, point.Z)
    )


def _ecky_edge_target_id(edge, edge_index, object_name):
    start = edge.start_point()
    end = edge.end_point()
    first = _ecky_point_signature(start)
    second = _ecky_point_signature(end)
    if second < first:
        first, second = second, first
    return f"{object_name}:edge:{int(edge_index)}:{first}_{second}"


def _ecky_stable_target_suffix(payload):
    if ":" not in payload:
        return payload
    prefix, suffix = payload.split(":", 1)
    return suffix if prefix.isdigit() else payload


def _ecky_stable_edge_target_id(target_id):
    marker = ":edge:"
    if marker not in target_id:
        return target_id
    prefix, payload = target_id.split(marker, 1)
    return f"{prefix}{marker}{_ecky_stable_target_suffix(payload)}"


def _ecky_face_target_id(face, face_index, object_name):
    center = face.center()
    return (
        f"{object_name}:face:{int(face_index)}:"
        f"{_ecky_point_signature(center)}:{_ecky_format_coordinate(face.area)}"
    )


def _ecky_stable_face_target_id(target_id):
    marker = ":face:"
    if marker not in target_id:
        return target_id
    prefix, payload = target_id.split(marker, 1)
    return f"{prefix}{marker}{_ecky_stable_target_suffix(payload)}"


def _ecky_edge_axis_span(edge):
    box = edge.bounding_box()
    return float(box.size.X), float(box.size.Y), float(box.size.Z)


def _ecky_edge_matches_clause(edge, shape_box, clause, tol):
    edge_box = edge.bounding_box()
    if clause[0] == "axis":
        x_span, y_span, z_span = _ecky_edge_axis_span(edge)
        if clause[1] == "x":
            return x_span > tol and y_span <= tol and z_span <= tol
        if clause[1] == "y":
            return y_span > tol and x_span <= tol and z_span <= tol
        return z_span > tol and x_span <= tol and y_span <= tol
    _, axis, bound = clause
    target = _ecky_axis_value(shape_box.min if bound == "min" else shape_box.max, axis)
    return (
        abs(_ecky_axis_value(edge_box.min, axis) - target) <= tol
        and abs(_ecky_axis_value(edge_box.max, axis) - target) <= tol
    )


def _ecky_face_matches_clause(face, shape_box, clause, tol):
    face_box = face.bounding_box()
    if clause[0] == "planar":
        return getattr(face, "geom_type", None) == GeomType.PLANE
    if clause[0] == "normal":
        if getattr(face, "geom_type", None) != GeomType.PLANE:
            return False
        axis = clause[1]
        return float(_ecky_axis_value(face_box.size, axis)) <= tol
    _, axis, bound = clause
    target = _ecky_axis_value(shape_box.min if bound == "min" else shape_box.max, axis)
    return (
        abs(_ecky_axis_value(face_box.min, axis) - target) <= tol
        and abs(_ecky_axis_value(face_box.max, axis) - target) <= tol
    )


def _ecky_filter_faces_by_area(faces, rank):
    if not faces:
        return []
    areas = [float(face.area) for face in faces]
    target = min(areas) if rank == "min" else max(areas)
    tol = max(abs(float(target)), 1.0) * 1e-6
    return [face for face, area in zip(faces, areas) if abs(float(area) - target) <= tol]


def _ecky_select_edges(shape, selector=None, object_name=None):
    edges = list(shape.edges())
    if not edges:
        raise ValueError("Shape has no edges for fillet/chamfer.")
    target_ids = _ecky_selector_target_ids(selector)
    if target_ids is not None:
        if not object_name:
            raise ValueError(
                f"Edge selector `{selector}` requires an object name for exact target-id matching."
            )
        edge_records = []
        stable_counts = {}
        for edge_index, edge in enumerate(edges):
            target_id = _ecky_edge_target_id(edge, edge_index, object_name)
            stable_id = _ecky_stable_edge_target_id(target_id)
            edge_records.append((edge, target_id, stable_id))
            stable_counts[stable_id] = stable_counts.get(stable_id, 0) + 1
        selected = []
        matched = set()
        for requested_target_id in target_ids:
            exact = next(
                (record for record in edge_records if record[1] == requested_target_id),
                None,
            )
            if exact is not None:
                if exact[1] not in matched:
                    selected.append(exact[0])
                    matched.add(exact[1])
                continue
            stable_requested = _ecky_stable_edge_target_id(requested_target_id)
            candidates = [
                record for record in edge_records if record[2] == stable_requested
            ]
            if not candidates:
                raise ValueError(
                    f"Edge selector `{selector}` did not match target ids: {[requested_target_id]}"
                )
            if len(candidates) > 1 or stable_counts.get(stable_requested, 0) > 1:
                raise ValueError(
                    f"Edge selector `{selector}` ambiguously matched stable edge target `{requested_target_id}`."
                )
            candidate = candidates[0]
            if candidate[1] not in matched:
                selected.append(candidate[0])
                matched.add(candidate[1])
        return selected
    clauses = _ecky_selector_clauses(selector)
    if not clauses:
        return edges
    shape_box = shape.bounding_box()
    tol = max(abs(float(shape_box.size.X)), abs(float(shape_box.size.Y)), abs(float(shape_box.size.Z)), 1.0) * 1e-6
    selected = [edge for edge in edges if all(_ecky_edge_matches_clause(edge, shape_box, clause, tol) for clause in clauses)]
    if not selected:
        raise ValueError(f"Edge selector `{selector}` matched no edges.")
    return selected


def _ecky_select_shell_faces(shape, selector, object_name=None):
    faces = list(shape.faces())
    if not faces:
        raise ValueError("Shape has no faces for shell openings.")
    target_ids = _ecky_face_selector_target_ids(selector)
    if target_ids is not None:
        if not object_name:
            raise ValueError(
                f"Shell face selector `{selector}` requires an object name for exact target-id matching."
            )
        face_records = []
        stable_counts = {}
        for face_index, face in enumerate(faces):
            target_id = _ecky_face_target_id(face, face_index, object_name)
            stable_id = _ecky_stable_face_target_id(target_id)
            face_records.append((face, target_id, stable_id))
            stable_counts[stable_id] = stable_counts.get(stable_id, 0) + 1
        selected = []
        matched = set()
        for requested_target_id in target_ids:
            exact = next(
                (record for record in face_records if record[1] == requested_target_id),
                None,
            )
            if exact is not None:
                if exact[1] not in matched:
                    selected.append(exact[0])
                    matched.add(exact[1])
                continue
            stable_requested = _ecky_stable_face_target_id(requested_target_id)
            candidates = [
                record for record in face_records if record[2] == stable_requested
            ]
            if not candidates:
                raise ValueError(
                    f"Shell face selector `{selector}` did not match target ids: {[requested_target_id]}"
                )
            if len(candidates) > 1 or stable_counts.get(stable_requested, 0) > 1:
                raise ValueError(
                    f"Shell face selector `{selector}` ambiguously matched stable face target `{requested_target_id}`."
                )
            candidate = candidates[0]
            if candidate[1] not in matched:
                selected.append(candidate[0])
                matched.add(candidate[1])
        return selected
    clauses = _ecky_face_selector_clauses(selector)
    if selector is None:
        return []
    if not clauses:
        return faces
    shape_box = shape.bounding_box()
    tol = max(
        abs(float(shape_box.size.X)),
        abs(float(shape_box.size.Y)),
        abs(float(shape_box.size.Z)),
        1.0,
    ) * 1e-6
    selected = list(faces)
    for clause in clauses:
        if clause[0] == "area":
            selected = _ecky_filter_faces_by_area(selected, clause[1])
            continue
        selected = [
            face
            for face in selected
            if _ecky_face_matches_clause(face, shape_box, clause, tol)
        ]
    if not selected:
        raise ValueError(f"Shell face selector `{selector}` matched no faces.")
    return selected


def _ecky_path_frame(path, at="end", up=None):
    if at == "start":
        position = 0.0
    elif at == "end":
        position = 1.0
    else:
        position = float(at)
    kwargs = {"position_mode": PositionMode.PARAMETER, "frame_method": FrameMethod.FRENET}
    if up is not None:
        kwargs["x_dir"] = Vector(*up)
    return path.location_at(position, **kwargs)


def _ecky_as_location(frame):
    return frame if isinstance(frame, Location) else Location(frame)


def _ecky_location(frame, offset=(0, 0, 0), rotate=(0, 0, 0)):
    ox, oy, oz = offset
    rx, ry, rz = rotate
    return _ecky_as_location(frame) * Pos(ox, oy, oz) * Rot(rx, ry, rz)


def _ecky_place(frame, shape, offset=(0, 0, 0), rotate=(0, 0, 0)):
    return _ecky_location(frame, offset, rotate) * shape


def _ecky_extrude(sketch, amount, symmetric=False):
    out = extrude(_ecky_face(sketch), amount)
    if symmetric:
        return Pos(0, 0, -amount / 2.0) * out
    return out


def _ecky_loft(sections):
    faces = [_ecky_face(section) for section in sections]
    return _ecky_solid(loft(faces))


def _ecky_clip_box(shape, xmin, xmax, ymin, ymax, zmin, zmax):
    solids = _ecky_collect_solids(shape)
    if not solids:
        return Compound(children=[])
    xmin, xmax = min(xmin, xmax), max(xmin, xmax)
    ymin, ymax = min(ymin, ymax), max(ymin, ymax)
    zmin, zmax = min(zmin, zmax), max(zmin, zmax)
    clip = Pos((xmin + xmax) / 2.0, (ymin + ymax) / 2.0, (zmin + zmax) / 2.0) * Box(
        xmax - xmin,
        ymax - ymin,
        zmax - zmin,
        align=(Align.CENTER, Align.CENTER, Align.CENTER),
    )
    return _ecky_common_many(shape, clip)
