import json
import os
import sys
import traceback
import unittest


def ensure_dir(path):
    os.makedirs(path, exist_ok=True)


def sanitize_name(value):
    sanitized = []
    for ch in value:
        if ch.isalnum():
            sanitized.append(ch.lower())
        else:
            sanitized.append("-")
    result = "".join(sanitized)
    while "--" in result:
        result = result.replace("--", "-")
    return result.strip("-") or "part"


def freecad_object_name(value, index):
    sanitized = "".join(ch for ch in str(value) if ch.isalnum() or ch == "_")
    sanitized = sanitized.strip("_")
    if not sanitized:
        sanitized = f"EckyPart{index:03d}"
    elif not sanitized[0].isalpha():
        sanitized = f"EckyPart{index:03d}_{sanitized}"
    return sanitized[:80]


def first_value(entry, *keys):
    for key in keys:
        value = entry.get(key)
        if value is not None:
            return value
    return None


def normalize_part_text(value):
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def normalize_generated_part(entry, index):
    if isinstance(entry, dict):
        shape = first_value(entry, "shape", "part", "solid", "object")
        if shape is None:
            return None
        part_id = first_value(
            entry,
            "part_id",
            "partId",
            "id",
            "key",
            "name",
            "label",
            "object_name",
            "objectName",
            "freecad_object_name",
            "freecadObjectName",
        )
        object_name = first_value(
            entry,
            "object_name",
            "objectName",
            "freecad_object_name",
            "freecadObjectName",
            "name",
            "part_id",
            "partId",
            "id",
            "key",
            "label",
        ) or part_id
        label = first_value(
            entry,
            "label",
            "display_name",
            "displayName",
            "name",
            "object_name",
            "objectName",
            "part_id",
            "partId",
        ) or part_id or object_name
        part_id = normalize_part_text(part_id) or f"part_{index}"
        object_name = normalize_part_text(object_name) or part_id
        label = normalize_part_text(label) or object_name
        return {
            "part_id": part_id,
            "object_name": object_name,
            "label": label,
            "shape": shape,
        }

    if not isinstance(entry, (list, tuple)) or len(entry) != 2:
        return None

    part_id, shape = entry
    if shape is None:
        return None
    part_id = normalize_part_text(part_id) or f"part_{index}"
    return {
        "part_id": part_id,
        "object_name": part_id,
        "label": part_id,
        "shape": shape,
    }


def collect_exportable(doc):
    exportable = []
    for obj in doc.Objects:
        shape = getattr(obj, "Shape", None)
        if shape is None:
            continue
        try:
            if shape.isNull() or shape.Volume <= 0:
                continue
        except Exception:
            continue
        exportable.append(obj)
    return exportable


def shape_bounds(shape):
    bbox = getattr(shape, "BoundBox", None)
    if bbox is None:
        return None
    return {
        "x_min": float(bbox.XMin),
        "y_min": float(bbox.YMin),
        "z_min": float(bbox.ZMin),
        "x_max": float(bbox.XMax),
        "y_max": float(bbox.YMax),
        "z_max": float(bbox.ZMax),
    }


def point_to_xyz(point):
    x = getattr(point, "x", None)
    if x is None:
        x = getattr(point, "X", 0.0)
    if callable(x):
        x = x()
    y = getattr(point, "y", None)
    if y is None:
        y = getattr(point, "Y", 0.0)
    if callable(y):
        y = y()
    z = getattr(point, "z", None)
    if z is None:
        z = getattr(point, "Z", 0.0)
    if callable(z):
        z = z()
    return {"x": float(x), "y": float(y), "z": float(z)}


def edge_endpoints(edge):
    try:
        vertexes = getattr(edge, "Vertexes", []) or []
        if len(vertexes) >= 2:
            return point_to_xyz(vertexes[0].Point), point_to_xyz(vertexes[-1].Point)
    except Exception:
        pass

    try:
        start = edge.valueAt(edge.FirstParameter)
        end = edge.valueAt(edge.LastParameter)
        return point_to_xyz(start), point_to_xyz(end)
    except Exception:
        return None


def edge_point_signature(point):
    return "-".join(
        number_signature(point[axis])
        for axis in ("x", "y", "z")
    )


def number_signature(value):
    return format(float(value), ".3f").rstrip("0").rstrip(".") or "0"


def edge_signature(start, end):
    first = edge_point_signature(start)
    second = edge_point_signature(end)
    return "_".join(sorted((first, second)))


def face_center(face):
    try:
        return point_to_xyz(face.CenterOfMass)
    except Exception:
        return None


def face_normal(face):
    try:
        parameter_range = getattr(face, "ParameterRange", None)
        if parameter_range and len(parameter_range) == 4:
            u = (float(parameter_range[0]) + float(parameter_range[1])) / 2.0
            v = (float(parameter_range[2]) + float(parameter_range[3])) / 2.0
            return point_to_xyz(face.normalAt(u, v))
    except Exception:
        pass

    try:
        surface = getattr(face, "Surface", None)
        axis = getattr(surface, "Axis", None)
        if axis is not None:
            return point_to_xyz(axis)
    except Exception:
        pass

    return None


def shape_edges(shape, object_name):
    edges = []
    try:
        source_edges = getattr(shape, "Edges", []) or []
    except Exception:
        return edges

    for edge_index, edge in enumerate(source_edges):
        try:
            endpoints = edge_endpoints(edge)
            if endpoints is None:
                continue
            start, end = endpoints
            label = f"{object_name}.Edge{edge_index + 1}"
            edges.append(
                {
                    "target_id": f"{object_name}:edge:{edge_index}:{edge_signature(start, end)}",
                    "edge_index": edge_index,
                    "label": label,
                    "start": start,
                    "end": end,
                }
            )
        except Exception:
            continue

    return edges


def shape_faces(shape, object_name):
    faces = []
    try:
        source_faces = getattr(shape, "Faces", []) or []
    except Exception:
        return faces

    for face_index, face in enumerate(source_faces):
        try:
            center = face_center(face)
            if center is None:
                continue
            area = float(getattr(face, "Area", 0.0))
            label = f"{object_name}.Face{face_index + 1}"
            face_id = (
                f"{object_name}:face:{face_index}:"
                f"{edge_point_signature(center)}:{number_signature(area)}"
            )
            faces.append(
                {
                    "target_id": face_id,
                    "face_index": face_index,
                    "label": label,
                    "center": center,
                    "normal": face_normal(face),
                    "area": area,
                }
            )
        except Exception:
            continue

    return faces


def export_report(doc, exportable, parts_dir):
    import Mesh

    ensure_dir(parts_dir)
    objects = []
    for index, obj in enumerate(exportable):
        file_name = f"{index:03d}-{sanitize_name(getattr(obj, 'Name', 'part'))}.stl"
        export_path = os.path.abspath(os.path.join(parts_dir, file_name))
        Mesh.export([obj], export_path)
        shape = getattr(obj, "Shape", None)
        object_name = getattr(obj, "Name", f"Object{index}")
        part_id = normalize_part_text(getattr(obj, "EckyPartId", None)) or object_name
        objects.append(
            {
                "part_id": part_id,
                "object_name": object_name,
                "label": getattr(obj, "Label", getattr(obj, "Name", f"Object{index}")),
                "type_id": getattr(obj, "TypeId", ""),
                "export_path": export_path,
                "bounds": shape_bounds(shape) if shape is not None else None,
                "volume": float(getattr(shape, "Volume", 0.0)) if shape is not None else None,
                "area": float(getattr(shape, "Area", 0.0)) if shape is not None else None,
                "edges": shape_edges(shape, part_id) if shape is not None else [],
                "faces": shape_faces(shape, part_id) if shape is not None else [],
            }
        )

    return {
        "document_name": getattr(doc, "Name", "EckyCAD"),
        "document_label": getattr(doc, "Label", getattr(doc, "Name", "EckyCAD")),
        "warnings": [],
        "objects": objects,
    }


def export_step(exportable, step_path):
    if not step_path:
        return
    import Import

    ensure_dir(os.path.dirname(os.path.abspath(step_path)))
    Import.export(exportable, step_path)


def infer_scale_factors(bounds, parameter_keys, params_dict):
    width = max(0.0, float(bounds["x_max"]) - float(bounds["x_min"]))
    depth = max(0.0, float(bounds["y_max"]) - float(bounds["y_min"]))
    height = max(0.0, float(bounds["z_max"]) - float(bounds["z_min"]))
    scale_x = 1.0
    scale_y = 1.0
    scale_z = 1.0

    for key in parameter_keys:
        raw_value = params_dict.get(key)
        if raw_value is None:
            continue
        try:
            target = float(raw_value)
        except Exception:
            continue

        if key.endswith("_height") and height > 0.0:
            scale_z = max(0.01, target / height)
        elif key.endswith("_depth") and depth > 0.0:
            scale_y = max(0.01, target / depth)
        elif width > 0.0:
            scale_x = max(0.01, target / width)

    return scale_x, scale_y, scale_z


def apply_binding_to_object(doc, binding, params_dict):
    import FreeCAD as App

    object_name = binding.get("object_name")
    parameter_keys = binding.get("parameter_keys") or []
    if not object_name or not parameter_keys:
        return None

    obj = doc.getObject(object_name)
    if obj is None:
        return f"Could not find FreeCAD object '{object_name}' for imported apply."

    shape = getattr(obj, "Shape", None)
    if shape is None:
        return f"Object '{object_name}' does not expose a Shape for imported apply."

    try:
        if shape.isNull():
            return f"Object '{object_name}' has an empty Shape and was skipped."
    except Exception:
        return f"Object '{object_name}' could not be inspected for imported apply."

    bounds = shape_bounds(shape)
    if bounds is None:
        return f"Object '{object_name}' is missing bounds and was skipped."

    scale_x, scale_y, scale_z = infer_scale_factors(bounds, parameter_keys, params_dict)
    if (
        abs(scale_x - 1.0) < 0.0001
        and abs(scale_y - 1.0) < 0.0001
        and abs(scale_z - 1.0) < 0.0001
    ):
        return None

    anchor_x = (float(bounds["x_min"]) + float(bounds["x_max"])) * 0.5
    anchor_y = (float(bounds["y_min"]) + float(bounds["y_max"])) * 0.5
    anchor_z = float(bounds["z_min"])

    matrix = App.Matrix()
    matrix.A11 = scale_x
    matrix.A22 = scale_y
    matrix.A33 = scale_z
    matrix.A14 = anchor_x - (anchor_x * scale_x)
    matrix.A24 = anchor_y - (anchor_y * scale_y)
    matrix.A34 = anchor_z - (anchor_z * scale_z)

    try:
        transformed = shape.copy()
        transformed.transformGeometry(matrix)
        obj.Shape = transformed
        return None
    except Exception as exc:
        return f"Failed to apply imported binding to '{object_name}': {exc}"


def execute_macro(macro_path, params_dict):
    import FreeCAD as App

    doc = App.newDocument("EckyCAD")

    with open(macro_path, "r", encoding="utf-8") as handle:
        code = handle.read()

    macro_dir = os.path.dirname(macro_path)
    if macro_dir and macro_dir not in sys.path:
        sys.path.insert(0, macro_dir)
    sdk_path = os.environ.get("ECKYCAD_SDK_PATH")
    if sdk_path and sdk_path not in sys.path:
        sys.path.insert(0, sdk_path)

    namespace = {"__name__": "__main__", "parameters": params_dict, "params": params_dict}
    for key, value in params_dict.items():
        namespace[key] = value

    exec(compile(code, macro_path, "exec"), namespace, namespace)

    generated_parts = namespace.get("_ecky_parts")
    if isinstance(generated_parts, list):
        for index, entry in enumerate(generated_parts):
            normalized = normalize_generated_part(entry, index)
            if normalized is None:
                continue
            shape = normalized["shape"]
            if shape is None:
                continue
            obj = doc.addObject(
                "Part::Feature",
                freecad_object_name(normalized["object_name"], index),
            )
            obj.Label = str(normalized["label"])
            try:
                obj.addProperty("App::PropertyString", "EckyPartId", "Ecky")
            except Exception:
                pass
            try:
                obj.EckyPartId = str(normalized["part_id"])
            except Exception:
                pass
            obj.Shape = shape

    if App.ActiveDocument is None:
        App.setActiveDocument(doc.Name)

    active = App.ActiveDocument
    active.recompute()
    return active


def open_fcstd(path):
    import FreeCAD as App

    doc = App.openDocument(path)
    App.setActiveDocument(doc.Name)
    doc.recompute()
    return doc


def open_step(path):
    import FreeCAD as App
    import Import

    doc = App.newDocument("EckyStepHiddenLine")
    Import.insert(path, doc.Name)
    App.setActiveDocument(doc.Name)
    doc.recompute()
    return doc


def frame_list(frame, *keys):
    values = first_value(frame, *keys)
    if not isinstance(values, (list, tuple)) or len(values) != 3:
        return None
    try:
        return [float(values[0]), float(values[1]), float(values[2])]
    except Exception:
        return None


def frame_matrix(frame):
    import FreeCAD as App

    origin = frame_list(frame, "origin")
    x_axis = frame_list(frame, "x_axis", "xAxis")
    y_axis = frame_list(frame, "y_axis", "yAxis")
    z_axis = frame_list(frame, "z_axis", "zAxis")
    if origin is None or x_axis is None or y_axis is None or z_axis is None:
        raise RuntimeError(f"Invalid assembly placement frame: {frame}")

    matrix = App.Matrix()
    matrix.A11 = x_axis[0]
    matrix.A12 = y_axis[0]
    matrix.A13 = z_axis[0]
    matrix.A14 = origin[0]
    matrix.A21 = x_axis[1]
    matrix.A22 = y_axis[1]
    matrix.A23 = z_axis[1]
    matrix.A24 = origin[1]
    matrix.A31 = x_axis[2]
    matrix.A32 = y_axis[2]
    matrix.A33 = z_axis[2]
    matrix.A34 = origin[2]
    return matrix


def transform_shape_copy(shape, placement_frame):
    transformed = shape.copy()
    if not placement_frame:
        return transformed
    transformed.transformGeometry(frame_matrix(placement_frame))
    return transformed


def load_step_exportable_shapes(step_path):
    import FreeCAD as App

    temp_doc = open_step(step_path)
    try:
        exportable = collect_exportable(temp_doc)
        if not exportable:
            raise RuntimeError(f"No solid objects found in assembly STEP part '{step_path}'.")

        loaded = []
        for index, obj in enumerate(exportable):
            shape = getattr(obj, "Shape", None)
            if shape is None:
                continue
            loaded.append(
                {
                    "object_name": getattr(obj, "Name", f"StepPart{index}"),
                    "label": getattr(obj, "Label", getattr(obj, "Name", f"StepPart{index}")),
                    "shape": shape.copy(),
                }
            )
        return loaded
    finally:
        App.closeDocument(temp_doc.Name)


def run_generate(macro_path, stl_path, fcstd_path, step_path, parts_dir, report_path, params_dict):
    import Mesh

    active = execute_macro(macro_path, params_dict)
    exportable = collect_exportable(active)
    if not exportable:
        raise RuntimeError("No solid objects found to export.")

    Mesh.export(exportable, stl_path)
    export_step(exportable, step_path)
    active.saveAs(fcstd_path)

    report = export_report(active, exportable, parts_dir)
    with open(report_path, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2)


def run_import_fcstd(fcstd_path, stl_path, step_path, parts_dir, report_path):
    import Mesh

    active = open_fcstd(fcstd_path)
    exportable = collect_exportable(active)
    if not exportable:
        raise RuntimeError("No solid objects found in imported FCStd.")

    Mesh.export(exportable, stl_path)
    export_step(exportable, step_path)
    report = export_report(active, exportable, parts_dir)
    with open(report_path, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2)


def run_import_step(step_source_path, output_fcstd_path, stl_path, step_path, parts_dir, report_path):
    import Mesh

    active = open_step(step_source_path)
    exportable = collect_exportable(active)
    if not exportable:
        raise RuntimeError("No solid objects found in imported STEP.")

    Mesh.export(exportable, stl_path)
    export_step(exportable, step_path)
    active.saveAs(output_fcstd_path)
    report = export_report(active, exportable, parts_dir)
    with open(report_path, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2)


def run_assemble_step_parts(assembly_parts_path, output_fcstd_path, stl_path, step_path, parts_dir, report_path):
    import FreeCAD as App
    import Mesh

    with open(assembly_parts_path, "r", encoding="utf-8") as handle:
        assembly_parts = json.load(handle)
    if not isinstance(assembly_parts, list) or not assembly_parts:
        raise RuntimeError("Assembly STEP input must contain at least one part.")

    active = App.newDocument("EckyAssembly")
    active.Label = "Joined Assembly"
    global_index = 0
    fused_groups = {}
    fused_group_labels = {}
    cut_groups = {}
    cut_group_labels = {}

    for part_index, part in enumerate(assembly_parts):
        if not isinstance(part, dict):
            raise RuntimeError(f"Assembly STEP part #{part_index + 1} is not an object.")
        step_path_value = first_value(part, "step_path", "stepPath")
        if not step_path_value:
            raise RuntimeError(f"Assembly STEP part #{part_index + 1} is missing stepPath.")
        instance_id = normalize_part_text(first_value(part, "instance_id", "instanceId")) or f"instance_{part_index}"
        instance_label = normalize_part_text(part.get("label")) or instance_id
        fuse_group_id = normalize_part_text(first_value(part, "fuse_group_id", "fuseGroupId"))
        cut_group_id = normalize_part_text(first_value(part, "cut_group_id", "cutGroupId"))
        cut_role = normalize_part_text(first_value(part, "cut_role", "cutRole"))
        placement_frame = first_value(part, "placement_frame", "placementFrame")
        source_shapes = load_step_exportable_shapes(step_path_value)
        if fuse_group_id and cut_group_id:
            raise RuntimeError(
                f"Assembly STEP part #{part_index + 1} cannot use both fuseGroupId and cutGroupId."
            )

        combined_transformed = None
        for source_shape in source_shapes:
            transformed = transform_shape_copy(source_shape["shape"], placement_frame)
            combined_transformed = (
                transformed
                if combined_transformed is None
                else combined_transformed.fuse(transformed)
            )

        if cut_group_id:
            if cut_role not in ("base", "tool"):
                raise RuntimeError(
                    f"Assembly STEP part #{part_index + 1} cutGroupId requires cutRole=base|tool."
                )
            group = cut_groups.setdefault(cut_group_id, {"base": None, "tools": []})
            labels = cut_group_labels.setdefault(cut_group_id, {"base": None, "tools": []})
            if cut_role == "base":
                if group["base"] is not None:
                    raise RuntimeError(f"Cut group '{cut_group_id}' has multiple base parts.")
                group["base"] = combined_transformed
                labels["base"] = instance_label
            else:
                group["tools"].append(combined_transformed)
                labels["tools"].append(instance_label)
            continue

        for source_shape in source_shapes:
            transformed = transform_shape_copy(source_shape["shape"], placement_frame)
            if fuse_group_id:
                fused_groups[fuse_group_id] = (
                    transformed
                    if fuse_group_id not in fused_groups
                    else fused_groups[fuse_group_id].fuse(transformed)
                )
                fused_group_labels.setdefault(fuse_group_id, []).append(instance_label)
                continue
            object_name = freecad_object_name(
                f"{instance_id}_{source_shape['object_name']}", global_index
            )
            label = instance_label
            if len(source_shapes) > 1:
                label = f"{instance_label} / {source_shape['label']}"
            obj = active.addObject("Part::Feature", object_name)
            obj.Label = label
            obj.Shape = transformed
            global_index += 1

    for fuse_group_id in sorted(fused_groups.keys()):
        object_name = freecad_object_name(fuse_group_id, global_index)
        unique_labels = []
        for label in fused_group_labels.get(fuse_group_id, []):
            if label not in unique_labels:
                unique_labels.append(label)
        label = " + ".join(unique_labels) if unique_labels else fuse_group_id
        obj = active.addObject("Part::Feature", object_name)
        obj.Label = label
        obj.Shape = fused_groups[fuse_group_id]
        global_index += 1

    for cut_group_id in sorted(cut_groups.keys()):
        group = cut_groups[cut_group_id]
        if group["base"] is None:
            raise RuntimeError(f"Cut group '{cut_group_id}' is missing a base part.")
        if not group["tools"]:
            raise RuntimeError(f"Cut group '{cut_group_id}' is missing tool parts.")
        cut_shape = group["base"]
        for tool_shape in group["tools"]:
            cut_shape = cut_shape.cut(tool_shape)
        object_name = freecad_object_name(cut_group_id, global_index)
        labels = cut_group_labels.get(cut_group_id, {})
        base_label = labels.get("base") or cut_group_id
        tool_labels = []
        for label in labels.get("tools", []):
            if label not in tool_labels:
                tool_labels.append(label)
        label = f"{base_label} - {' - '.join(tool_labels)}" if tool_labels else base_label
        obj = active.addObject("Part::Feature", object_name)
        obj.Label = label
        obj.Shape = cut_shape
        global_index += 1

    active.recompute()
    exportable = collect_exportable(active)
    if not exportable:
        raise RuntimeError("No solid objects found in assembled STEP output.")

    Mesh.export(exportable, stl_path)
    export_step(exportable, step_path)
    active.saveAs(output_fcstd_path)
    report = export_report(active, exportable, parts_dir)
    with open(report_path, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2)


def run_apply_imported_fcstd(fcstd_path, output_fcstd_path, stl_path, step_path, parts_dir, report_path, params_dict, bindings):
    import Mesh

    active = open_fcstd(fcstd_path)
    warnings = []

    for binding in bindings:
        warning = apply_binding_to_object(active, binding, params_dict)
        if warning:
            warnings.append(warning)

    active.recompute()
    exportable = collect_exportable(active)
    if not exportable:
        raise RuntimeError("No solid objects found after imported apply.")

    Mesh.export(exportable, stl_path)
    export_step(exportable, step_path)
    active.saveAs(output_fcstd_path)
    report = export_report(active, exportable, parts_dir)
    report["warnings"] = warnings
    with open(report_path, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2)


def hidden_line_view_specs(requested_views):
    specs = {
        "front": {"view": "front", "direction": (0.0, -1.0, 0.0)},
        "top": {"view": "top", "direction": (0.0, 0.0, -1.0)},
        "side": {"view": "side", "direction": (-1.0, 0.0, 0.0)},
    }
    views = requested_views or ["front", "top", "side"]
    normalized = []
    for view in views:
        key = str(view).strip().lower()
        if key in specs and key not in [item["view"] for item in normalized]:
            normalized.append(specs[key])
    return normalized or [specs["front"], specs["top"], specs["side"]]


def parse_hidden_line_views(raw):
    if not raw:
        return []
    try:
        parsed = json.loads(raw)
    except Exception:
        return []
    if not isinstance(parsed, list):
        return []
    return parsed


def vector_coord(point, lower_name, upper_name):
    value = getattr(point, lower_name, None)
    if value is None:
        value = getattr(point, upper_name, 0.0)
    return float(value)


def point_to_view_pair(view, point):
    x = vector_coord(point, "x", "X")
    y = vector_coord(point, "y", "Y")
    z = vector_coord(point, "z", "Z")
    if view == "front":
        return [x, z]
    if view == "top":
        return [x, y]
    if view == "side":
        return [y, z]
    return [x, z]


def edge_sample_points(edge):
    attempts = [
        lambda: edge.discretize(Number=24),
        lambda: edge.discretize(24),
        lambda: [vertex.Point for vertex in getattr(edge, "Vertexes", [])],
    ]
    for attempt in attempts:
        try:
            points = attempt()
            if points and len(points) >= 2:
                return points
        except Exception:
            continue
    return []


def projection_edges(shape, view, source_class, prefix):
    edges = []
    for index, edge in enumerate(getattr(shape, "Edges", []) or []):
        points = [point_to_view_pair(view, point) for point in edge_sample_points(edge)]
        if len(points) < 2:
            continue
        edges.append(
            {
                "edgeId": f"{prefix}-{source_class.lower()}-{index}",
                "points": points,
                "sourceClass": source_class,
            }
        )
    return edges


def project_shape_hidden_lines(shape, view, direction_tuple):
    import FreeCAD as App

    try:
        import Drawing as ProjectionModule
    except Exception:
        import TechDraw as ProjectionModule

    direction = App.Vector(*direction_tuple)
    projected = ProjectionModule.projectEx(shape, direction)
    if projected is None or len(projected) < 10:
        raise RuntimeError("Drawing.projectEx returned incomplete hidden-line projection.")

    visible_specs = [("V", 0), ("V1", 1), ("VN", 2), ("VO", 3), ("VI", 4)]
    hidden_specs = [("H", 5), ("H1", 6), ("HN", 7), ("HO", 8), ("HI", 9)]
    visible_edges = []
    hidden_edges = []
    for source_class, index in visible_specs:
        visible_edges.extend(projection_edges(projected[index], view, source_class, view))
    for source_class, index in hidden_specs:
        hidden_edges.extend(projection_edges(projected[index], view, source_class, view))
    return visible_edges, hidden_edges


def run_hidden_line_projection(artifact_path, report_path, views, tolerance, artifact_kind="fcstd"):
    import Part

    active = open_step(artifact_path) if artifact_kind == "step" else open_fcstd(artifact_path)
    exportable = collect_exportable(active)
    if not exportable:
        raise RuntimeError(f"No solid objects found in {artifact_kind.upper()} for hidden-line projection.")

    shapes = [obj.Shape.copy() for obj in exportable]
    compound = shapes[0] if len(shapes) == 1 else Part.makeCompound(shapes)
    warning_entries = []
    projection_views = []
    for spec in hidden_line_view_specs(views):
        visible_edges, hidden_edges = project_shape_hidden_lines(
            compound,
            spec["view"],
            spec["direction"],
        )
        if not visible_edges and not hidden_edges:
            warning_entries.append(
                {
                    "kind": "projectionNoEdges",
                    "view": spec["view"],
                    "message": "projection produced no edges.",
                }
            )
        projection_views.append(
            {
                "view": spec["view"],
                "direction": list(spec["direction"]),
                "visibleEdges": visible_edges,
                "hiddenEdges": hidden_edges,
            }
        )

    report = {
        "sourceArtifactPath": os.path.abspath(artifact_path),
        "views": projection_views,
        "warningEntries": warning_entries,
        "tolerance": tolerance,
    }
    with open(report_path, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2)


def main():
    mode = os.environ.get("ECKYCAD_MODE", "generate")
    macro_path = os.environ.get("ECKYCAD_MACRO")
    stl_path = os.environ.get("ECKYCAD_STL")
    step_path = os.environ.get("ECKYCAD_STEP")
    fcstd_path = os.environ.get("ECKYCAD_FCSTD")
    import_fcstd_path = os.environ.get("ECKYCAD_IMPORT_FCSTD")
    import_step_path = os.environ.get("ECKYCAD_IMPORT_STEP")
    assembly_parts_path = os.environ.get("ECKYCAD_ASSEMBLY_PARTS")
    parts_dir = os.environ.get("ECKYCAD_PARTS_DIR")
    report_path = os.environ.get("ECKYCAD_REPORT") or os.environ.get("ECKYCAD_PROJECTION_REPORT")
    params_str = os.environ.get("ECKYCAD_PARAMS", "{}")
    bindings_str = os.environ.get("ECKYCAD_BINDINGS", "[]")
    projection_views_str = os.environ.get("ECKYCAD_PROJECTION_VIEWS", "[]")
    projection_tolerance = float(os.environ.get("ECKYCAD_PROJECTION_TOLERANCE", "0.1"))

    try:
        params_dict = json.loads(params_str)
        bindings = json.loads(bindings_str)

        if mode == "hidden_line_projection":
            projection_artifact_path = import_step_path or import_fcstd_path
            projection_artifact_kind = "step" if import_step_path else "fcstd"
            if not projection_artifact_path or not report_path:
                print("Missing ECKYCAD_IMPORT_FCSTD/ECKYCAD_IMPORT_STEP or ECKYCAD_REPORT for hidden_line_projection mode.")
                sys.exit(1)
            ensure_dir(os.path.dirname(os.path.abspath(report_path)))
            run_hidden_line_projection(
                projection_artifact_path,
                report_path,
                parse_hidden_line_views(projection_views_str),
                projection_tolerance,
                projection_artifact_kind,
            )
        elif mode == "generate":
            if not stl_path or not parts_dir or not report_path:
                print("Missing one of ECKYCAD_STL, ECKYCAD_PARTS_DIR, or ECKYCAD_REPORT.")
                sys.exit(1)
            ensure_dir(os.path.dirname(os.path.abspath(stl_path)))
            ensure_dir(os.path.abspath(parts_dir))
            ensure_dir(os.path.dirname(os.path.abspath(report_path)))
            if not macro_path or not fcstd_path:
                print("Missing ECKYCAD_MACRO or ECKYCAD_FCSTD for generate mode.")
                sys.exit(1)
            ensure_dir(os.path.dirname(os.path.abspath(fcstd_path)))
            run_generate(macro_path, stl_path, fcstd_path, step_path, parts_dir, report_path, params_dict)
        elif mode == "import_fcstd":
            if not stl_path or not parts_dir or not report_path:
                print("Missing one of ECKYCAD_STL, ECKYCAD_PARTS_DIR, or ECKYCAD_REPORT.")
                sys.exit(1)
            ensure_dir(os.path.dirname(os.path.abspath(stl_path)))
            ensure_dir(os.path.abspath(parts_dir))
            ensure_dir(os.path.dirname(os.path.abspath(report_path)))
            if not import_fcstd_path:
                print("Missing ECKYCAD_IMPORT_FCSTD for import_fcstd mode.")
                sys.exit(1)
            run_import_fcstd(import_fcstd_path, stl_path, step_path, parts_dir, report_path)
        elif mode == "import_step":
            if not stl_path or not parts_dir or not report_path:
                print("Missing one of ECKYCAD_STL, ECKYCAD_PARTS_DIR, or ECKYCAD_REPORT.")
                sys.exit(1)
            ensure_dir(os.path.dirname(os.path.abspath(stl_path)))
            ensure_dir(os.path.abspath(parts_dir))
            ensure_dir(os.path.dirname(os.path.abspath(report_path)))
            if not import_step_path or not fcstd_path:
                print("Missing ECKYCAD_IMPORT_STEP or ECKYCAD_FCSTD for import_step mode.")
                sys.exit(1)
            ensure_dir(os.path.dirname(os.path.abspath(fcstd_path)))
            run_import_step(import_step_path, fcstd_path, stl_path, step_path, parts_dir, report_path)
        elif mode == "apply_imported_fcstd":
            if not stl_path or not parts_dir or not report_path:
                print("Missing one of ECKYCAD_STL, ECKYCAD_PARTS_DIR, or ECKYCAD_REPORT.")
                sys.exit(1)
            ensure_dir(os.path.dirname(os.path.abspath(stl_path)))
            ensure_dir(os.path.abspath(parts_dir))
            ensure_dir(os.path.dirname(os.path.abspath(report_path)))
            if not import_fcstd_path or not fcstd_path:
                print("Missing ECKYCAD_IMPORT_FCSTD or ECKYCAD_FCSTD for apply_imported_fcstd mode.")
                sys.exit(1)
            ensure_dir(os.path.dirname(os.path.abspath(fcstd_path)))
            run_apply_imported_fcstd(
                import_fcstd_path,
                fcstd_path,
                stl_path,
                step_path,
                parts_dir,
                report_path,
                params_dict,
                bindings,
            )
        elif mode == "assemble_step_parts":
            if not stl_path or not parts_dir or not report_path:
                print("Missing one of ECKYCAD_STL, ECKYCAD_PARTS_DIR, or ECKYCAD_REPORT.")
                sys.exit(1)
            ensure_dir(os.path.dirname(os.path.abspath(stl_path)))
            ensure_dir(os.path.abspath(parts_dir))
            ensure_dir(os.path.dirname(os.path.abspath(report_path)))
            if not assembly_parts_path or not fcstd_path:
                print("Missing ECKYCAD_ASSEMBLY_PARTS or ECKYCAD_FCSTD for assemble_step_parts mode.")
                sys.exit(1)
            ensure_dir(os.path.dirname(os.path.abspath(fcstd_path)))
            run_assemble_step_parts(
                assembly_parts_path,
                fcstd_path,
                stl_path,
                step_path,
                parts_dir,
                report_path,
            )
        else:
            print(f"Unsupported ECKYCAD_MODE: {mode}")
            sys.exit(1)
    except Exception as exc:
        print(f"FATAL ERROR: {exc}")
        traceback.print_exc()
        sys.exit(1)


class NormalizeGeneratedPartTests(unittest.TestCase):
    def test_prefers_stringified_ids_and_trims_blank_fields(self):
        shape = object()
        normalized = normalize_generated_part(
            {
                "shape": shape,
                "partId": "  lid-1  ",
                "objectName": "  ",
                "label": "  Lid  ",
            },
            3,
        )

        self.assertEqual(
            normalized,
            {
                "part_id": "lid-1",
                "object_name": "lid-1",
                "label": "Lid",
                "shape": shape,
            },
        )

    def test_stringifies_non_string_metadata(self):
        shape = object()
        normalized = normalize_generated_part(
            {"shape": shape, "id": 42, "object_name": 99, "label": 123}, 0
        )

        self.assertEqual(normalized["part_id"], "42")
        self.assertEqual(normalized["object_name"], "99")
        self.assertEqual(normalized["label"], "123")

    def test_tuple_entries_fallback_to_indexed_part_id(self):
        shape = object()
        normalized = normalize_generated_part(("   ", shape), 7)

        self.assertEqual(
            normalized,
            {
                "part_id": "part_7",
                "object_name": "part_7",
                "label": "part_7",
                "shape": shape,
            },
        )


if __name__ == "__main__" or __name__ == "freecad_runner":
    main()
