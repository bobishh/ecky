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
CANONICAL_BUILD123D = SURFACE_FIXTURES / "canonical_cup.build123d.py"
CANONICAL_ECKY = SURFACE_FIXTURES / "canonical_cup.ecky"
BUILD123D_RUNNER = ROOT / "server" / "build123d_runner.py"
COMPARE_SCRIPT = ROOT / "server" / "compare_metric.py"
TAURI_MANIFEST = ROOT / "src-tauri" / "Cargo.toml"


def resolve_python() -> str:
    for env_name in ("BUILD123D_PYTHON", "PYTHON_CMD"):
        value = os.environ.get(env_name, "").strip()
        if value:
            return value
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


def main() -> int:
    allow_good = False
    args = sys.argv[1:]
    if "--allow-good" in args:
        allow_good = True
        args.remove("--allow-good")
    if args:
        raise SystemExit(f"Usage: {Path(__file__).name} [--allow-good]")

    python_cmd = resolve_python()

    with tempfile.TemporaryDirectory(prefix="ecky-canonical-cup-") as tmp_dir:
        tmp = Path(tmp_dir)
        reference_stl = tmp / "canonical_ref.stl"
        lowered_python = tmp / "canonical_lowered.py"
        generated_stl = tmp / "canonical_generated.stl"
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
                "lower_ecky_ir_to_build123d",
                "--",
                str(CANONICAL_ECKY),
                "--out",
                str(lowered_python),
            ]
        )
        require_ok(lower, "lower canonical ecky")

        runner_env = os.environ.copy()
        runner_env.update(
            {
                "ECKYCAD_SOURCE": str(lowered_python),
                "ECKYCAD_STL": str(generated_stl),
                "ECKYCAD_PARTS_DIR": str(parts_dir),
                "ECKYCAD_REPORT": str(report_path),
                "ECKYCAD_PARAMS": "{}",
            }
        )
        render = run([python_cmd, str(BUILD123D_RUNNER)], env=runner_env)
        require_ok(render, "render lowered python")

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

        accepted = {"EXCELLENT MATCH"}
        if allow_good:
            accepted.add("GOOD MATCH")
        if comparison["status"] not in accepted:
            return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
