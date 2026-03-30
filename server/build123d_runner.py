#!/usr/bin/env python3
"""
Ecky CAD build123d runner.

Executes a generated build123d Python source file and exports:
  - Per-part STL files to ECKYCAD_PARTS_DIR
  - Merged preview STL to ECKYCAD_STL
  - JSON runner report to ECKYCAD_REPORT

The source file must define:
  _ecky_parts = [("part_id", build123d_shape), ...]

Environment variables:
  ECKYCAD_SOURCE    path to the Python source file to execute
  ECKYCAD_STL       path to write the merged preview STL
  ECKYCAD_PARTS_DIR directory to write per-part STL files
  ECKYCAD_REPORT    path to write the runner report JSON
  ECKYCAD_PARAMS    JSON-encoded parameters dict (injected as `params`)
"""

import json
import os
import sys
import traceback


def sanitize_name(value):
    sanitized = []
    for ch in str(value):
        if ch.isalnum():
            sanitized.append(ch.lower())
        else:
            sanitized.append("-")
    result = "".join(sanitized)
    while "--" in result:
        result = result.replace("--", "-")
    return result.strip("-") or "part"


def shape_bounds(shape):
    try:
        bb = shape.bounding_box()
        return {
            "x_min": float(bb.min.X),
            "y_min": float(bb.min.Y),
            "z_min": float(bb.min.Z),
            "x_max": float(bb.max.X),
            "y_max": float(bb.max.Y),
            "z_max": float(bb.max.Z),
        }
    except Exception:
        return None


def export_stl(shape, path):
    from build123d import exporters
    exporters.export(shape, path, exporters.ExportTypes.STL)


def merge_shapes(shapes):
    from build123d import Compound
    if len(shapes) == 1:
        return shapes[0]
    return Compound(children=shapes)


def main():
    source_path = os.environ["ECKYCAD_SOURCE"]
    stl_path = os.environ["ECKYCAD_STL"]
    parts_dir = os.environ["ECKYCAD_PARTS_DIR"]
    report_path = os.environ["ECKYCAD_REPORT"]
    params_json = os.environ.get("ECKYCAD_PARAMS", "{}")

    params = json.loads(params_json)
    os.makedirs(parts_dir, exist_ok=True)

    with open(source_path) as f:
        source_code = f.read()

    namespace = {"params": params}
    try:
        exec(compile(source_code, source_path, "exec"), namespace)
    except Exception as exc:
        print(f"ERROR: build123d source execution failed: {exc}", file=sys.stderr)
        traceback.print_exc()
        sys.exit(1)

    ecky_parts = namespace.get("_ecky_parts")
    if not ecky_parts:
        print("ERROR: source did not define _ecky_parts or it is empty.", file=sys.stderr)
        sys.exit(1)

    objects = []
    all_shapes = []

    for index, (part_id, shape) in enumerate(ecky_parts):
        file_name = f"{index:03d}-{sanitize_name(part_id)}.stl"
        export_path = os.path.abspath(os.path.join(parts_dir, file_name))
        try:
            export_stl(shape, export_path)
        except Exception as exc:
            print(f"ERROR: failed to export part '{part_id}': {exc}", file=sys.stderr)
            traceback.print_exc()
            sys.exit(1)
        all_shapes.append(shape)

        bounds = shape_bounds(shape)
        try:
            volume = float(shape.volume)
        except Exception:
            volume = None
        try:
            area = float(shape.area)
        except Exception:
            area = None

        objects.append({
            "object_name": part_id,
            "label": part_id,
            "type_id": "build123d",
            "export_path": export_path,
            "bounds": bounds,
            "volume": volume,
            "area": area,
        })

    try:
        preview_shape = merge_shapes(all_shapes)
        export_stl(preview_shape, stl_path)
    except Exception as exc:
        print(f"ERROR: failed to export preview STL: {exc}", file=sys.stderr)
        traceback.print_exc()
        sys.exit(1)

    report = {
        "document_name": "EckyCAD",
        "document_label": "EckyCAD",
        "warnings": [],
        "objects": objects,
    }

    with open(report_path, "w") as f:
        json.dump(report, f, indent=2)


if __name__ == "__main__":
    try:
        main()
    except KeyError as exc:
        print(f"ERROR: missing required environment variable: {exc}", file=sys.stderr)
        sys.exit(1)
    except Exception:
        traceback.print_exc()
        sys.exit(1)
