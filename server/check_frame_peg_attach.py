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
SOURCE = SURFACE_FIXTURES / "frame_peg_attach.ecky"
ORACLE = SURFACE_FIXTURES / "frame_peg_attach.build123d.py"
BUILD123D_RUNNER = ROOT / "server" / "build123d_runner.py"
COMPARE_SCRIPT = ROOT / "server" / "compare_metric.py"


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


def write_lowering_test(test_path: Path) -> None:
    test_path.write_text(
        """
        use std::env;
        use std::fs;
        use std::path::PathBuf;

        #[test]
        fn lower_frame_peg_attach_fixture() {
            let output_path = PathBuf::from(
                env::var("FRAME_PEG_ATTACH_OUT").expect("FRAME_PEG_ATTACH_OUT"),
            );
            let source = include_str!("fixtures/cad/surface/frame_peg_attach.ecky");
            let lowered = ecky_cad_lib::ecky_ir::lower_to_build123d(source)
                .expect("lower frame peg attach source");
            fs::write(&output_path, lowered).expect("write lowered python");
        }
        """
    )


def main() -> int:
    if not SOURCE.exists():
        raise SystemExit(f"missing source fixture: {SOURCE}")
    if not ORACLE.exists():
        raise SystemExit(f"missing oracle fixture: {ORACLE}")

    python_cmd = resolve_build123d_python()

    with tempfile.TemporaryDirectory(prefix="ecky-frame-peg-attach-out-") as tmp_dir:
        tmp = Path(tmp_dir)
        lowered_python = tmp / "frame_peg_attach_lowered.py"
        generated_stl = tmp / "frame_peg_attach_generated.stl"
        generated_parts = tmp / "generated_parts"
        generated_report = tmp / "generated_report.json"
        reference_stl = tmp / "frame_peg_attach_reference.stl"
        reference_parts = tmp / "reference_parts"
        reference_report = tmp / "reference_report.json"

        test_path = ROOT / "src-tauri" / "tests" / "_frame_peg_attach_parity.rs"
        write_lowering_test(test_path)
        try:
            cargo_env = os.environ.copy()
            cargo_env["FRAME_PEG_ATTACH_OUT"] = str(lowered_python)
            lower = run(
                [
                    "cargo",
                    "test",
                    "--quiet",
                    "--manifest-path",
                    str(ROOT / "src-tauri" / "Cargo.toml"),
                    "--test",
                    "_frame_peg_attach_parity",
                    "--locked",
                    "--",
                    "--nocapture",
                ],
                env=cargo_env,
            )
            require_ok(lower, "lower frame peg attach source")
        finally:
            if test_path.exists():
                test_path.unlink()

        generated_env = os.environ.copy()
        generated_env.update(
            {
                "ECKYCAD_SOURCE": str(lowered_python),
                "ECKYCAD_STL": str(generated_stl),
                "ECKYCAD_PARTS_DIR": str(generated_parts),
                "ECKYCAD_REPORT": str(generated_report),
                "ECKYCAD_PARAMS": "{}",
            }
        )
        render_generated = run([python_cmd, str(BUILD123D_RUNNER)], env=generated_env)
        require_ok(render_generated, "render lowered frame peg attach source")

        reference_env = os.environ.copy()
        reference_env.update(
            {
                "ECKYCAD_SOURCE": str(ORACLE),
                "ECKYCAD_STL": str(reference_stl),
                "ECKYCAD_PARTS_DIR": str(reference_parts),
                "ECKYCAD_REPORT": str(reference_report),
                "ECKYCAD_PARAMS": "{}",
            }
        )
        render_reference = run([python_cmd, str(BUILD123D_RUNNER)], env=reference_env)
        require_ok(render_reference, "render frame peg attach oracle")

        compare = run(
            [python_cmd, str(COMPARE_SCRIPT), "--json", str(reference_stl), str(generated_stl)]
        )
        require_ok(compare, "compare frame peg attach meshes")
        comparison = json.loads(compare.stdout)

        print(f"Source: {SOURCE}")
        print(f"Oracle: {ORACLE}")
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


if __name__ == "__main__":
    raise SystemExit(main())
