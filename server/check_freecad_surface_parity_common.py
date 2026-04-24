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
BUILD123D_RUNNER = ROOT / "server" / "build123d_runner.py"
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


def lower_to_freecad(source: Path, out_path: Path) -> None:
    cmd = [
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str(TAURI_MANIFEST),
        "--bin",
        "lower_ecky_ir_to_freecad",
        "--",
        str(source),
        "--out",
        str(out_path),
    ]
    require_ok(run(cmd), f"lower {source.name} to FreeCAD")


def render_freecad_macro(
    freecad_cmd: str,
    macro_path: Path,
    stl_path: Path,
    fcstd_path: Path,
    parts_dir: Path,
    report_path: Path,
    *,
    params: dict[str, object],
    label: str,
) -> None:
    env = os.environ.copy()
    env.update(
        {
            "ECKYCAD_MODE": "generate",
            "ECKYCAD_MACRO": str(macro_path),
            "ECKYCAD_STL": str(stl_path),
            "ECKYCAD_FCSTD": str(fcstd_path),
            "ECKYCAD_PARTS_DIR": str(parts_dir),
            "ECKYCAD_REPORT": str(report_path),
            "ECKYCAD_PARAMS": json.dumps(params),
        }
    )
    require_ok(run([freecad_cmd, str(FREECAD_RUNNER)], env=env), label)


def render_build123d_oracle(
    python_cmd: str,
    oracle_path: Path,
    stl_path: Path,
    parts_dir: Path,
    report_path: Path,
    *,
    params: dict[str, object],
    label: str,
) -> None:
    env = os.environ.copy()
    env.update(
        {
            "ECKYCAD_SOURCE": str(oracle_path),
            "ECKYCAD_STL": str(stl_path),
            "ECKYCAD_PARTS_DIR": str(parts_dir),
            "ECKYCAD_REPORT": str(report_path),
            "ECKYCAD_PARAMS": json.dumps(params),
        }
    )
    require_ok(run([python_cmd, str(BUILD123D_RUNNER)], env=env), label)


def check_surface_fixture(name: str, source: Path, oracle: Path, *, params: dict[str, object] | None = None) -> int:
    if not source.exists():
        raise SystemExit(f"missing source fixture: {source}")
    if not oracle.exists():
        raise SystemExit(f"missing oracle fixture: {oracle}")

    freecad_cmd = resolve_freecad_cmd()
    python_cmd = resolve_python()
    params = params or {}

    with tempfile.TemporaryDirectory(prefix=f"ecky-freecad-{name}-") as tmp_dir:
        tmp = Path(tmp_dir)
        lowered_macro = tmp / f"{name}_lowered.FCMacro"
        generated_stl = tmp / f"{name}_generated.stl"
        generated_fcstd = tmp / f"{name}_generated.FCStd"
        generated_parts = tmp / "generated_parts"
        generated_report = tmp / "generated_report.json"
        reference_stl = tmp / f"{name}_reference.stl"
        reference_parts = tmp / "reference_parts"
        reference_report = tmp / "reference_report.json"

        lower_to_freecad(source, lowered_macro)

        render_freecad_macro(
            freecad_cmd,
            lowered_macro,
            generated_stl,
            generated_fcstd,
            generated_parts,
            generated_report,
            params=params,
            label=f"render lowered {name} source via FreeCAD",
        )
        render_build123d_oracle(
            python_cmd,
            oracle,
            reference_stl,
            reference_parts,
            reference_report,
            params=params,
            label=f"render {name} oracle",
        )

        compare = run(
            [python_cmd, str(COMPARE_SCRIPT), "--json", str(reference_stl), str(generated_stl)]
        )
        require_ok(compare, f"compare {name} meshes")
        comparison = json.loads(compare.stdout)

        print(f"Source: {source}")
        print(f"Oracle: {oracle}")
        print(f"Reference STL: {reference_stl}")
        print(f"Generated STL: {generated_stl}")
        print(f"Volume Difference: {comparison['volume_difference_percent']:.2f}%")
        print(f"Bounding Box Match Error: {comparison['bounding_box_match_error']:.2f} mm")
        print(
            "BBox Axis Deltas: "
            f"dx={comparison['bounding_box_axis_deltas']['x']:.2f} "
            f"dy={comparison['bounding_box_axis_deltas']['y']:.2f} "
            f"dz={comparison['bounding_box_axis_deltas']['z']:.2f}"
        )
        print(f"Status: {comparison['status']}")

        if comparison["status"] != "EXCELLENT MATCH":
            return 1

    return 0
