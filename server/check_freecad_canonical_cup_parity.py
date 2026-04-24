#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import subprocess
import shutil
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "src-tauri" / "tests" / "fixtures" / "cad"
SURFACE_FIXTURES = FIXTURES / "surface"
CANONICAL_BUILD123D = SURFACE_FIXTURES / "canonical_cup.build123d.py"
CANONICAL_ECKY = SURFACE_FIXTURES / "canonical_cup.ecky"
FREECAD_RUNNER = ROOT / "server" / "freecad_runner.py"
COMPARE_SCRIPT = ROOT / "server" / "compare_metric.py"
TAURI_MANIFEST = ROOT / "src-tauri" / "Cargo.toml"


def resolve_python() -> str:
    for env_name in ("BUILD123D_PYTHON", "PYTHON_CMD"):
        value = os.environ.get(env_name, "").strip()
        if value:
            return value
    bundled = ROOT / ".dist" / "build123d-runtime" / "bin" / "python3"
    if bundled.exists():
        return str(bundled)
    bundled_fallback = ROOT / ".dist" / "build123d-runtime" / "bin" / "python"
    if bundled_fallback.exists():
        return str(bundled_fallback)
    return shutil.which("python3") or sys.executable or "python3"


def resolve_freecad_cmd() -> str:
    return os.environ.get(
        "FREECAD_CMD",
        "/Applications/FreeCAD.app/Contents/Resources/bin/freecadcmd",
    )


def run(cmd: list[str], *, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=ROOT,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )


def require_ok(result: subprocess.CompletedProcess[str], label: str) -> None:
    if result.returncode == 0:
        return
    raise SystemExit(
        f"{label} failed\n"
        f"cmd: {' '.join(result.args)}\n"
        f"stdout:\n{result.stdout}\n"
        f"stderr:\n{result.stderr}"
    )


def main() -> int:
    python_cmd = resolve_python()
    freecad_cmd = resolve_freecad_cmd()

    with tempfile.TemporaryDirectory(prefix="ecky-freecad-canonical-cup-") as tmp_dir:
        tmp = Path(tmp_dir)
        reference_stl = tmp / "canonical_ref.stl"
        lowered_macro = tmp / "canonical_lowered.FCMacro"
        generated_stl = tmp / "canonical_generated.stl"
        generated_fcstd = tmp / "canonical_generated.FCStd"
        parts_dir = tmp / "parts"
        report_path = tmp / "runner-report.json"

        native_export = run(
            [python_cmd, str(CANONICAL_BUILD123D), "--export-stl", str(reference_stl), "--json"]
        )
        require_ok(native_export, "native canonical export")
        native_metrics = json.loads(native_export.stdout)

        lower = run(
            [
                "cargo",
                "run",
                "--quiet",
                "--manifest-path",
                str(TAURI_MANIFEST),
                "--bin",
                "lower_ecky_ir_to_freecad",
                "--",
                str(CANONICAL_ECKY),
                "--out",
                str(lowered_macro),
            ]
        )
        require_ok(lower, "lower canonical ecky to FreeCAD")

        runner_env = os.environ.copy()
        runner_env.update(
            {
                "ECKYCAD_MODE": "generate",
                "ECKYCAD_MACRO": str(lowered_macro),
                "ECKYCAD_STL": str(generated_stl),
                "ECKYCAD_FCSTD": str(generated_fcstd),
                "ECKYCAD_PARTS_DIR": str(parts_dir),
                "ECKYCAD_REPORT": str(report_path),
                "ECKYCAD_PARAMS": "{}",
            }
        )
        render = run([freecad_cmd, str(FREECAD_RUNNER)], env=runner_env)
        require_ok(render, "render lowered FreeCAD macro")

        compare = run([python_cmd, str(COMPARE_SCRIPT), "--json", str(reference_stl), str(generated_stl)])
        require_ok(compare, "compare canonical meshes")
        comparison = json.loads(compare.stdout)

        print(f"Reference Volume: {native_metrics['volume']:.2f} mm^3")
        print(f"Generated Volume: {comparison['generated_volume']:.2f} mm^3")
        print(f"Volume Difference: {comparison['volume_difference_percent']:.2f}%")
        print(f"Bounding Box Match Error: {comparison['bounding_box_match_error']:.2f} mm")
        print(
            "BBox Axis Deltas: "
            f"dx={comparison['bounding_box_axis_deltas']['x']:.2f} "
            f"dy={comparison['bounding_box_axis_deltas']['y']:.2f} "
            f"dz={comparison['bounding_box_axis_deltas']['z']:.2f}"
        )
        print(f"Status: {comparison['status']}")
        print(f"Reference STL: {reference_stl}")
        print(f"Generated STL: {generated_stl}")

        if comparison["status"] != "EXCELLENT MATCH":
            return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
