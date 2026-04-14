from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Callable


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "src-tauri" / "tests" / "fixtures" / "cad"
SURFACE_FIXTURES = FIXTURES / "surface"
REFERENCE_FIXTURES = FIXTURES / "reference"
LEGACY_MACRO = REFERENCE_FIXTURES / "thomas_modular_ramp_legacy.py"
TAURI_MANIFEST = ROOT / "src-tauri" / "Cargo.toml"
BUILD123D_RUNNER = ROOT / "server" / "build123d_runner.py"
FREECAD_RUNNER = ROOT / "server" / "freecad_runner.py"
COMPARE_SCRIPT = ROOT / "server" / "compare_metric.py"


def resolve_build123d_python() -> str:
    for env_key in ("BUILD123D_PYTHON", "PYTHON_CMD"):
        value = os.environ.get(env_key)
        if value:
            return value
    bundled = ROOT / ".dist" / "build123d-runtime" / "bin" / "python3"
    if bundled.exists():
        return str(bundled)
    bundled_fallback = ROOT / ".dist" / "build123d-runtime" / "bin" / "python"
    if bundled_fallback.exists():
        return str(bundled_fallback)
    return sys.executable


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


def write_legacy_until_marker(source: Path, out: Path, marker: str, show_expr: str) -> None:
    text = source.read_text()
    if marker not in text:
        raise SystemExit(f"legacy Thomas macro missing marker: {marker}")
    head = text.split(marker, 1)[0].rstrip()
    out.write_text(head + f'\n\nPart.show({show_expr}, "Thomas_Modular_Ramp")\n')


def write_legacy_full(source: Path, out: Path) -> None:
    out.write_text(source.read_text())


def run_thomas_phase(
    *,
    surface_source: Path,
    params: dict[str, object],
    prepare_legacy: Callable[[Path, Path], None],
) -> int:
    with tempfile.TemporaryDirectory(prefix="ecky-thomas-phase-") as tmp_dir:
        tmp = Path(tmp_dir)
        legacy_macro = tmp / "thomas_phase.py"
        lowered_python = tmp / "thomas_lowered.py"
        ref_stl = tmp / "thomas_ref.stl"
        ref_fcstd = tmp / "thomas_ref.FCStd"
        ref_parts = tmp / "ref_parts"
        ref_report = tmp / "ref_report.json"
        generated_stl = tmp / "thomas_generated.stl"
        generated_parts = tmp / "gen_parts"
        generated_report = tmp / "gen_report.json"

        prepare_legacy(LEGACY_MACRO, legacy_macro)

        lower = run(
            [
                "cargo",
                "run",
                "--quiet",
                "--manifest-path",
                str(TAURI_MANIFEST),
                "--bin",
                "lower_ecky_ir_to_build123d",
                "--",
                str(surface_source),
                "--out",
                str(lowered_python),
            ]
        )
        require_ok(lower, "lower authored Thomas source")

        freecad_env = os.environ.copy()
        freecad_env.update(
            {
                "ECKYCAD_MODE": "generate",
                "ECKYCAD_MACRO": str(legacy_macro),
                "ECKYCAD_STL": str(ref_stl),
                "ECKYCAD_FCSTD": str(ref_fcstd),
                "ECKYCAD_PARTS_DIR": str(ref_parts),
                "ECKYCAD_REPORT": str(ref_report),
                "ECKYCAD_PARAMS": json.dumps(params),
            }
        )
        freecad_cmd = os.environ.get(
            "FREECAD_CMD", "/Applications/FreeCAD.app/Contents/Resources/bin/freecadcmd"
        )
        freecad_render = run([freecad_cmd, str(FREECAD_RUNNER)], env=freecad_env)
        require_ok(freecad_render, "render FreeCAD reference")

        build123d_env = os.environ.copy()
        build123d_env.update(
            {
                "ECKYCAD_SOURCE": str(lowered_python),
                "ECKYCAD_STL": str(generated_stl),
                "ECKYCAD_PARTS_DIR": str(generated_parts),
                "ECKYCAD_REPORT": str(generated_report),
                "ECKYCAD_PARAMS": json.dumps(params),
            }
        )
        python_cmd = resolve_build123d_python()
        build123d_render = run([python_cmd, str(BUILD123D_RUNNER)], env=build123d_env)
        require_ok(build123d_render, "render build123d model")

        compare = run([python_cmd, str(COMPARE_SCRIPT), "--json", str(ref_stl), str(generated_stl)])
        require_ok(compare, "compare models")
        comparison = json.loads(compare.stdout)

        print(f"Reference STL: {ref_stl}")
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
