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
    "_ecky_path_frame",
    "_ecky_as_location",
    "_ecky_location",
    "_ecky_place",
    "_ecky_extrude",
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
