#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BOOK_TARGET_DIR = ROOT / "target" / "book"
sys.path.insert(0, str(ROOT / "scripts"))
sys.path.insert(0, str(ROOT / "server"))

from compare_metric import mesh_metrics  # noqa: E402
from render_book_examples import Example, collect_examples  # noqa: E402

WORK_DIR = BOOK_TARGET_DIR / "example-parity"
PUBLIC_DOCS = ROOT / "public" / "docs" / "ecky-ir.md"
BUILD123D_RUNNER = ROOT / "server" / "build123d_runner.py"
TAURI_MANIFEST = ROOT / "src-tauri" / "Cargo.toml"
ECKY_BIN = ROOT / "src-tauri" / "target" / "debug" / "ecky"
NATIVE_BIN = ROOT / "src-tauri" / "target" / "debug" / "render_ecky_ir_native_occt"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--strict", action="store_true", help="exit nonzero on parity failures")
    parser.add_argument("--book-only", action="store_true", help="skip app/public docs examples")
    parser.add_argument("--filter", default="", help="substring filter for example asset stem")
    parser.add_argument("--limit", type=int, default=0)
    args = parser.parse_args()

    examples = collect_examples()
    if not args.book_only:
        examples.extend(collect_public_docs_examples())
    if args.filter:
        examples = [example for example in examples if args.filter in example.asset_stem]
    if args.limit:
        examples = examples[: args.limit]
    if not examples:
        raise SystemExit("No examples selected.")

    WORK_DIR.mkdir(parents=True, exist_ok=True)
    ensure_bins()
    python_cmd = resolve_build123d_python()

    results = []
    for example in examples:
        result = check_example(example, python_cmd)
        results.append(result)
        print(format_result(result), flush=True)

    summary_path = WORK_DIR / "summary.json"
    summary_path.write_text(json.dumps(results, indent=2), encoding="utf-8")
    print(f"summary={summary_path}")

    failed = [
        result
        for result in results
        if result["status"] not in {"excellent", "good"}
    ]
    print(f"examples={len(results)} failed_or_skipped={len(failed)}")
    return 1 if args.strict and failed else 0


def collect_public_docs_examples() -> list[Example]:
    markdown = PUBLIC_DOCS.read_text(encoding="utf-8")
    examples: list[Example] = []
    local_index = 0
    for match in re.finditer(r"```scheme\n([\s\S]*?)```", markdown):
        code = match.group(1).strip()
        if "(model" not in code:
            continue
        local_index += 1
        if "..." in code:
            continue
        title = f"{nearest_heading(markdown, match.start())}, app docs example {local_index}"
        examples.append(Example(PUBLIC_DOCS, "public-docs", local_index, title, code))
    return examples


def nearest_heading(markdown: str, offset: int) -> str:
    heading = "Ecky app docs"
    for line in markdown[:offset].splitlines():
        if line.startswith("## "):
            heading = line[3:].strip()
        elif line.startswith("### "):
            heading = line[4:].strip()
    return heading


def ensure_bins() -> None:
    run_checked(
        [
            "cargo",
            "build",
            "--quiet",
            "--manifest-path",
            str(TAURI_MANIFEST),
            "--bin",
            "ecky",
            "--bin",
            "render_ecky_ir_native_occt",
        ],
        "build parity binaries",
    )


def resolve_build123d_python() -> str:
    for env_name in ("BUILD123D_PYTHON", "PYTHON_CMD"):
        value = os.environ.get(env_name, "").strip()
        if value:
            return value
    for candidate in [
        ROOT / "src-tauri/target/debug/runtime/build123d/bin/python",
        ROOT / "src-tauri/target/debug/runtime/build123d/bin/python3",
        ROOT / "src-tauri/target/release/runtime/build123d/bin/python",
        ROOT / "src-tauri/target/release/runtime/build123d/bin/python3",
        ROOT / ".dist/runtime/build123d/bin/python",
        ROOT / ".dist/runtime/build123d/bin/python3",
        ROOT / ".dist/build123d-runtime/bin/python3",
        ROOT / ".dist/build123d-runtime/bin/python",
    ]:
        if candidate.exists():
            return str(candidate)
    return sys.executable or "python3"


def check_example(example, python_cmd: str) -> dict:
    example_dir = WORK_DIR / example.asset_stem
    build_dir = example_dir / "build123d"
    native_dir = example_dir / "native"
    build_dir.mkdir(parents=True, exist_ok=True)
    native_dir.mkdir(parents=True, exist_ok=True)
    source_path = example_dir / "source.ecky"
    source_path.write_text(example.code + "\n", encoding="utf-8")

    result = {
        "id": example.asset_stem,
        "title": example.title,
        "source": str(source_path),
        "build123d": {"status": "pending"},
        "native": {"status": "pending"},
        "status": "pending",
    }

    try:
        build_stl, build_elapsed = render_build123d(source_path, build_dir, python_cmd)
        result["build123d"] = metrics_payload(build_stl, build_elapsed)
    except Exception as exc:
        result["build123d"] = {"status": "error", "error": str(exc)}
        result["status"] = "build123d-error"
        return result

    try:
        native_stl, native_elapsed = render_native(source_path, native_dir)
        result["native"] = metrics_payload(native_stl, native_elapsed)
    except Exception as exc:
        result["native"] = {"status": "error", "error": str(exc)}
        result["status"] = "native-error"
        return result

    comparison = compare_metrics(result["build123d"], result["native"])
    result["comparison"] = comparison
    result["status"] = comparison["status"]
    return result


def render_build123d(source_path: Path, out_dir: Path, python_cmd: str) -> tuple[Path, float]:
    lowered_path = out_dir / "source.build123d.py"
    started = time.perf_counter()
    run_checked(
        [
            str(ECKY_BIN),
            "lower",
            "--backend",
            "build123d",
            str(source_path),
            "--out",
            str(lowered_path),
        ],
        f"lower build123d {source_path.name}",
    )
    env = os.environ.copy()
    env.update(
        {
            "ECKYCAD_SOURCE": str(lowered_path),
            "ECKYCAD_STL": str(out_dir / "preview.stl"),
            "ECKYCAD_STEP": str(out_dir / "model.step"),
            "ECKYCAD_PARTS_DIR": str(out_dir / "parts"),
            "ECKYCAD_REPORT": str(out_dir / "runner-report.json"),
            "ECKYCAD_PARAMS": "{}",
        }
    )
    run_checked([python_cmd, str(BUILD123D_RUNNER)], f"render build123d {source_path.name}", env=env)
    return out_dir / "preview.stl", time.perf_counter() - started


def render_native(source_path: Path, out_dir: Path) -> tuple[Path, float]:
    started = time.perf_counter()
    run_checked(
        [str(NATIVE_BIN), str(source_path), "--out-dir", str(out_dir)],
        f"render native {source_path.name}",
    )
    return out_dir / "preview.stl", time.perf_counter() - started


def metrics_payload(stl_path: Path, elapsed: float) -> dict:
    metrics = mesh_metrics(stl_path)
    return {
        "status": "ok",
        "stl": str(stl_path),
        "elapsedSeconds": elapsed,
        "triangleCount": metrics.triangle_count,
        "volume": metrics.volume,
        "surfaceArea": metrics.surface_area,
        "minBound": metrics.min_bound,
        "maxBound": metrics.max_bound,
    }


def compare_metrics(reference: dict, generated: dict) -> dict:
    volume_diff = percent_delta(reference["volume"], generated["volume"])
    area_diff = percent_delta(reference["surfaceArea"], generated["surfaceArea"])
    bbox_delta = sum(
        abs(reference["minBound"][index] - generated["minBound"][index])
        + abs(reference["maxBound"][index] - generated["maxBound"][index])
        for index in range(3)
    )
    bbox_volume = bounding_box_volume(reference)
    volume_reliable = (
        bbox_volume > 0
        and max(reference["volume"], generated["volume"]) / bbox_volume >= 0.01
    )
    triangle_ratio = (
        generated["triangleCount"] / reference["triangleCount"]
        if reference["triangleCount"]
        else 0.0
    )
    if volume_reliable and volume_diff < 2.0 and area_diff < 2.0 and bbox_delta < 0.5:
        status = "excellent"
    elif volume_reliable and volume_diff < 8.0 and area_diff < 8.0 and bbox_delta < 2.0:
        status = "good"
    elif not volume_reliable and area_diff < 2.0 and bbox_delta < 0.5:
        status = "excellent"
    elif not volume_reliable and area_diff < 8.0 and bbox_delta < 2.0:
        status = "good"
    else:
        status = "poor"
    return {
        "status": status,
        "volumeReliable": volume_reliable,
        "volumeDifferencePercent": volume_diff,
        "surfaceAreaDifferencePercent": area_diff,
        "boundingBoxDeltaMm": bbox_delta,
        "triangleRatio": triangle_ratio,
    }


def percent_delta(reference: float, generated: float) -> float:
    if reference == 0:
        return float("inf") if generated != 0 else 0.0
    return abs(reference - generated) / abs(reference) * 100.0


def bounding_box_volume(metrics: dict) -> float:
    dx = abs(metrics["maxBound"][0] - metrics["minBound"][0])
    dy = abs(metrics["maxBound"][1] - metrics["minBound"][1])
    dz = abs(metrics["maxBound"][2] - metrics["minBound"][2])
    return dx * dy * dz


def format_result(result: dict) -> str:
    if result["status"] in {"build123d-error", "native-error"}:
        side = "build123d" if result["status"] == "build123d-error" else "native"
        return f"{result['id']}: {result['status']} :: {result[side]['error'].splitlines()[0]}"
    comparison = result["comparison"]
    volume_label = "vol" if comparison["volumeReliable"] else "vol~"
    return (
        f"{result['id']}: {result['status']} "
        f"{volume_label}={comparison['volumeDifferencePercent']:.2f}% "
        f"area={comparison['surfaceAreaDifferencePercent']:.2f}% "
        f"bbox={comparison['boundingBoxDeltaMm']:.2f}mm "
        f"tris={result['native']['triangleCount']}/{result['build123d']['triangleCount']} "
        f"time={result['native']['elapsedSeconds']:.2f}s/{result['build123d']['elapsedSeconds']:.2f}s"
    )


def run_checked(cmd: list[str], label: str, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(cmd, cwd=ROOT, text=True, capture_output=True, env=env)
    if result.returncode != 0:
        raise RuntimeError(
            f"{label} failed\ncmd: {' '.join(cmd)}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
    return result


if __name__ == "__main__":
    raise SystemExit(main())
