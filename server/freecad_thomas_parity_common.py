from __future__ import annotations

import json
import os
import subprocess
import shutil
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
FREECAD_RUNNER = ROOT / "server" / "freecad_runner.py"
COMPARE_SCRIPT = ROOT / "server" / "compare_metric.py"


def resolve_python() -> str:
    for env_name in ("PYTHON_CMD", "BUILD123D_PYTHON"):
        value = os.environ.get(env_name, "").strip()
        if value:
            return value
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


def write_legacy_until_marker(source: Path, out: Path, marker: str, show_expr: str) -> None:
    text = source.read_text()
    if marker not in text:
        raise SystemExit(f"legacy Thomas macro missing marker: {marker}")
    head = text.split(marker, 1)[0].rstrip()
    out.write_text(head + f'\n\nPart.show({show_expr}, "Thomas_Modular_Ramp")\n')


def write_legacy_full(source: Path, out: Path) -> None:
    out.write_text(source.read_text())


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


def run_thomas_phase(
    *,
    surface_source: Path,
    params: dict[str, object],
    prepare_legacy: Callable[[Path, Path], None],
) -> int:
    with tempfile.TemporaryDirectory(prefix="ecky-freecad-thomas-phase-") as tmp_dir:
        tmp = Path(tmp_dir)
        legacy_macro = tmp / "thomas_phase.py"
        lowered_macro = tmp / "thomas_lowered.FCMacro"
        ref_stl = tmp / "thomas_ref.stl"
        ref_fcstd = tmp / "thomas_ref.FCStd"
        ref_parts = tmp / "ref_parts"
        ref_report = tmp / "ref_report.json"
        generated_stl = tmp / "thomas_generated.stl"
        generated_fcstd = tmp / "thomas_generated.FCStd"
        generated_parts = tmp / "gen_parts"
        generated_report = tmp / "gen_report.json"

        prepare_legacy(LEGACY_MACRO, legacy_macro)
        lower_to_freecad(surface_source, lowered_macro)

        freecad_cmd = resolve_freecad_cmd()
        render_freecad_macro(
            freecad_cmd,
            legacy_macro,
            ref_stl,
            ref_fcstd,
            ref_parts,
            ref_report,
            params=params,
            label="render FreeCAD reference",
        )
        render_freecad_macro(
            freecad_cmd,
            lowered_macro,
            generated_stl,
            generated_fcstd,
            generated_parts,
            generated_report,
            params=params,
            label="render lowered FreeCAD model",
        )

        compare = run(
            [resolve_python(), str(COMPARE_SCRIPT), "--json", str(ref_stl), str(generated_stl)]
        )
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
