#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "src-tauri" / "tests" / "fixtures" / "cad"
SURFACE_FIXTURES = FIXTURES / "surface"
BUILD123D_RUNNER = ROOT / "server" / "build123d_runner.py"
COMPARE_SCRIPT = ROOT / "server" / "compare_metric.py"
TAURI_MANIFEST = ROOT / "src-tauri" / "Cargo.toml"
LOWER_BIN = ROOT / "src-tauri" / "target" / "debug" / "lower_ecky_ir_to_build123d"


def resolve_build123d_python() -> str:
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
    return sys.executable or "python3"


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


def render_python_fixture(
    python_cmd: str,
    source_path: Path,
    stl_path: Path,
    parts_dir: Path,
    report_path: Path,
    *,
    label: str,
) -> None:
    render_env = os.environ.copy()
    render_env.update(
        {
            "ECKYCAD_SOURCE": str(source_path),
            "ECKYCAD_STL": str(stl_path),
            "ECKYCAD_PARTS_DIR": str(parts_dir),
            "ECKYCAD_REPORT": str(report_path),
            "ECKYCAD_PARAMS": "{}",
        }
    )
    render = run([python_cmd, str(BUILD123D_RUNNER)], env=render_env)
    require_ok(render, label)


def check_surface_fixture(name: str, source: Path, oracle: Path) -> int:
    if not source.exists():
        raise SystemExit(f"missing source fixture: {source}")
    if not oracle.exists():
        raise SystemExit(f"missing oracle fixture: {oracle}")

    python_cmd = resolve_build123d_python()

    with tempfile.TemporaryDirectory(prefix=f"ecky-{name}-") as tmp_dir:
        tmp = Path(tmp_dir)
        lowered_python = tmp / f"{name}_lowered.py"
        generated_stl = tmp / f"{name}_generated.stl"
        generated_parts = tmp / "generated_parts"
        generated_report = tmp / "generated_report.json"
        reference_stl = tmp / f"{name}_reference.stl"
        reference_parts = tmp / "reference_parts"
        reference_report = tmp / "reference_report.json"

        if LOWER_BIN.exists():
            lower_cmd = [str(LOWER_BIN), str(source), "--out", str(lowered_python)]
        else:
            lower_cmd = [
                "cargo",
                "run",
                "--quiet",
                "--manifest-path",
                str(TAURI_MANIFEST),
                "--bin",
                "lower_ecky_ir_to_build123d",
                "--",
                str(source),
                "--out",
                str(lowered_python),
            ]
        lower = run(lower_cmd)
        require_ok(lower, f"lower {name} source")

        render_python_fixture(
            python_cmd,
            lowered_python,
            generated_stl,
            generated_parts,
            generated_report,
            label=f"render lowered {name} source",
        )
        render_python_fixture(
            python_cmd,
            oracle,
            reference_stl,
            reference_parts,
            reference_report,
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
