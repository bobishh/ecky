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


def export_report(doc, exportable, parts_dir):
    import Mesh

    ensure_dir(parts_dir)
    objects = []
    for index, obj in enumerate(exportable):
        file_name = f"{index:03d}-{sanitize_name(getattr(obj, 'Name', 'part'))}.stl"
        export_path = os.path.abspath(os.path.join(parts_dir, file_name))
        Mesh.export([obj], export_path)
        shape = getattr(obj, "Shape", None)
        objects.append(
            {
                "object_name": getattr(obj, "Name", f"Object{index}"),
                "label": getattr(obj, "Label", getattr(obj, "Name", f"Object{index}")),
                "type_id": getattr(obj, "TypeId", ""),
                "export_path": export_path,
                "bounds": shape_bounds(shape) if shape is not None else None,
                "volume": float(getattr(shape, "Volume", 0.0)) if shape is not None else None,
                "area": float(getattr(shape, "Area", 0.0)) if shape is not None else None,
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


def run_hidden_line_projection(fcstd_path, report_path, views, tolerance):
    import Part

    active = open_fcstd(fcstd_path)
    exportable = collect_exportable(active)
    if not exportable:
        raise RuntimeError("No solid objects found in FCStd for hidden-line projection.")

    shapes = [obj.Shape.copy() for obj in exportable]
    compound = shapes[0] if len(shapes) == 1 else Part.makeCompound(shapes)
    warnings = []
    projection_views = []
    for spec in hidden_line_view_specs(views):
        visible_edges, hidden_edges = project_shape_hidden_lines(
            compound,
            spec["view"],
            spec["direction"],
        )
        if not visible_edges and not hidden_edges:
            warnings.append(f"{spec['view']} projection produced no edges.")
        projection_views.append(
            {
                "view": spec["view"],
                "direction": list(spec["direction"]),
                "visibleEdges": visible_edges,
                "hiddenEdges": hidden_edges,
            }
        )

    report = {
        "sourceArtifactPath": os.path.abspath(fcstd_path),
        "views": projection_views,
        "warnings": warnings,
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
            if not import_fcstd_path or not report_path:
                print("Missing ECKYCAD_IMPORT_FCSTD or ECKYCAD_REPORT for hidden_line_projection mode.")
                sys.exit(1)
            ensure_dir(os.path.dirname(os.path.abspath(report_path)))
            run_hidden_line_projection(
                import_fcstd_path,
                report_path,
                parse_hidden_line_views(projection_views_str),
                projection_tolerance,
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
