import json
import os
import sys
import traceback


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


def run_generate(macro_path, stl_path, fcstd_path, parts_dir, report_path, params_dict):
    import Mesh

    active = execute_macro(macro_path, params_dict)
    exportable = collect_exportable(active)
    if not exportable:
        raise RuntimeError("No solid objects found to export.")

    Mesh.export(exportable, stl_path)
    active.saveAs(fcstd_path)

    report = export_report(active, exportable, parts_dir)
    with open(report_path, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2)


def run_import_fcstd(fcstd_path, stl_path, parts_dir, report_path):
    import Mesh

    active = open_fcstd(fcstd_path)
    exportable = collect_exportable(active)
    if not exportable:
        raise RuntimeError("No solid objects found in imported FCStd.")

    Mesh.export(exportable, stl_path)
    report = export_report(active, exportable, parts_dir)
    with open(report_path, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2)


def run_apply_imported_fcstd(fcstd_path, output_fcstd_path, stl_path, parts_dir, report_path, params_dict, bindings):
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
    active.saveAs(output_fcstd_path)
    report = export_report(active, exportable, parts_dir)
    report["warnings"] = warnings
    with open(report_path, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2)


def main():
    mode = os.environ.get("ECKYCAD_MODE", "generate")
    macro_path = os.environ.get("ECKYCAD_MACRO")
    stl_path = os.environ.get("ECKYCAD_STL")
    fcstd_path = os.environ.get("ECKYCAD_FCSTD")
    import_fcstd_path = os.environ.get("ECKYCAD_IMPORT_FCSTD")
    parts_dir = os.environ.get("ECKYCAD_PARTS_DIR")
    report_path = os.environ.get("ECKYCAD_REPORT")
    params_str = os.environ.get("ECKYCAD_PARAMS", "{}")
    bindings_str = os.environ.get("ECKYCAD_BINDINGS", "[]")

    if not stl_path or not parts_dir or not report_path:
        print("Missing one of ECKYCAD_STL, ECKYCAD_PARTS_DIR, or ECKYCAD_REPORT.")
        sys.exit(1)

    try:
        params_dict = json.loads(params_str)
        bindings = json.loads(bindings_str)
        ensure_dir(os.path.dirname(os.path.abspath(stl_path)))
        ensure_dir(os.path.abspath(parts_dir))
        ensure_dir(os.path.dirname(os.path.abspath(report_path)))

        if mode == "generate":
            if not macro_path or not fcstd_path:
                print("Missing ECKYCAD_MACRO or ECKYCAD_FCSTD for generate mode.")
                sys.exit(1)
            ensure_dir(os.path.dirname(os.path.abspath(fcstd_path)))
            run_generate(macro_path, stl_path, fcstd_path, parts_dir, report_path, params_dict)
        elif mode == "import_fcstd":
            if not import_fcstd_path:
                print("Missing ECKYCAD_IMPORT_FCSTD for import_fcstd mode.")
                sys.exit(1)
            run_import_fcstd(import_fcstd_path, stl_path, parts_dir, report_path)
        elif mode == "apply_imported_fcstd":
            if not import_fcstd_path or not fcstd_path:
                print("Missing ECKYCAD_IMPORT_FCSTD or ECKYCAD_FCSTD for apply_imported_fcstd mode.")
                sys.exit(1)
            ensure_dir(os.path.dirname(os.path.abspath(fcstd_path)))
            run_apply_imported_fcstd(
                import_fcstd_path,
                fcstd_path,
                stl_path,
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


if __name__ == "__main__" or __name__ == "freecad_runner":
    main()
