#!/usr/bin/env python3
import argparse
import math
import os
import re
import struct
import subprocess
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from mpl_toolkits.mplot3d.art3d import Poly3DCollection


ROOT = Path(__file__).resolve().parents[1]
CHAPTER_DIR = ROOT / "docs" / "books" / "ecky-ir" / "chapters"
BOOK_TARGET_DIR = ROOT / "target" / "book"
ASSET_DIR = BOOK_TARGET_DIR / "docs" / "books" / "ecky-ir" / "assets"
PUBLIC_ASSET_DIR = BOOK_TARGET_DIR / "public" / "docs" / "assets"
WORK_DIR = BOOK_TARGET_DIR / "example-renders"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    examples = collect_examples()
    if not examples:
        raise SystemExit("No renderable examples found.")

    if args.check:
        missing = [
            example.asset_name
            for example in examples
            if not (ASSET_DIR / example.asset_name).is_file()
            or not (PUBLIC_ASSET_DIR / example.asset_name).is_file()
        ]
        if missing:
            raise SystemExit("Missing render assets: " + ", ".join(missing))
        print(f"render assets present: {len(examples)}")
        return 0

    ASSET_DIR.mkdir(parents=True, exist_ok=True)
    PUBLIC_ASSET_DIR.mkdir(parents=True, exist_ok=True)
    WORK_DIR.mkdir(parents=True, exist_ok=True)

    rendered = 0
    skipped = 0
    for example in examples:
        ecky_path = WORK_DIR / f"{example.asset_stem}.ecky"
        render_root = WORK_DIR / example.asset_stem
        ecky_path.write_text(example.code + "\n", encoding="utf-8")
        render_root.mkdir(parents=True, exist_ok=True)
        try:
            stl_path = render_preview_stl(ecky_path, render_root)
            asset_path = ASSET_DIR / example.asset_name
            render_stl_png(stl_path, asset_path, example.title)
            (PUBLIC_ASSET_DIR / example.asset_name).write_bytes(asset_path.read_bytes())
            ensure_chapter_image(example)
            rendered += 1
        except Exception as exc:
            # Some book examples are deliberately backend-specific (e.g. the
            # native-only `:created-by` selector, which build123d rejects). Skip
            # those instead of aborting the whole book render, but surface them.
            first_line = str(exc).strip().splitlines()[0] if str(exc).strip() else repr(exc)
            print(f"SKIPPED {example.asset_name}: {first_line}", flush=True)
            skipped += 1

    print(f"rendered examples: {rendered} (skipped: {skipped})")
    return 0


class Example:
    def __init__(
        self,
        chapter_path: Path,
        chapter_slug: str,
        index: int,
        title: str,
        code: str,
        render_source_marker: str | None = None,
    ):
        self.chapter_path = chapter_path
        self.chapter_slug = chapter_slug
        self.index = index
        self.title = title
        self.code = code
        self.render_source_marker = render_source_marker
        self.asset_stem = f"{chapter_slug}-{index:02d}"
        self.asset_name = f"{self.asset_stem}.png"


def collect_examples() -> list[Example]:
    examples: list[Example] = []
    for chapter_path in sorted(CHAPTER_DIR.glob("*.md")):
        markdown = chapter_path.read_text(encoding="utf-8")
        chapter_slug = chapter_path.stem
        heading = first_heading(markdown)
        local_index = 0
        for match in re.finditer(r"```scheme\n([\s\S]*?)```", markdown):
            code = match.group(1).strip()
            if not code.startswith("(model"):
                continue
            local_index += 1
            title = f"{heading}, example {local_index}"
            examples.append(Example(chapter_path, chapter_slug, local_index, title, code))
        for match in re.finditer(r"<!--\s*render-source:\s*([^>]+?)\s*-->", markdown):
            source_rel = match.group(1).strip()
            source_path = (chapter_path.parent / source_rel).resolve()
            if not source_path.is_file():
                raise RuntimeError(f"Render source not found: {source_path}")
            code = source_path.read_text(encoding="utf-8").strip()
            if not code.startswith("(model"):
                raise RuntimeError(f"Render source must start with `(model`: {source_path}")
            local_index += 1
            title = f"{heading}, example {local_index}"
            examples.append(
                Example(chapter_path, chapter_slug, local_index, title, code, match.group(0))
            )
    return examples


def first_heading(markdown: str) -> str:
    for line in markdown.splitlines():
        if line.startswith("## "):
            return line[3:].strip()
    return "Ecky example"


def render_preview_stl(ecky_path: Path, render_root: Path) -> Path:
    lowered_path = render_root / "source.build123d.py"
    lower = subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "--bin",
            "ecky",
            "--",
            "lower",
            "--backend",
            "build123d",
            str(ecky_path),
            "--out",
            str(lowered_path),
        ],
        cwd=ROOT / "src-tauri",
        text=True,
        capture_output=True,
    )
    if lower.returncode != 0:
        raise RuntimeError(
            f"Lowering failed for {ecky_path.name}\nstdout:\n{lower.stdout}\nstderr:\n{lower.stderr}"
        )

    stl_path = render_root / "preview.stl"
    step_path = render_root / "model.step"
    parts_dir = render_root / "parts"
    report_path = render_root / "runner-report.json"
    parts_dir.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env.update(
        {
            "ECKYCAD_SOURCE": str(lowered_path),
            "ECKYCAD_STL": str(stl_path),
            "ECKYCAD_STEP": str(step_path),
            "ECKYCAD_PARTS_DIR": str(parts_dir),
            "ECKYCAD_REPORT": str(report_path),
            "ECKYCAD_PARAMS": "{}",
        }
    )
    run = subprocess.run(
        [str(ROOT / "src-tauri/target/release/runtime/build123d/bin/python"), str(ROOT / "server/build123d_runner.py")],
        cwd=ROOT,
        text=True,
        capture_output=True,
        env=env,
    )
    if run.returncode != 0:
        raise RuntimeError(
            f"Render failed for {ecky_path.name}\nstdout:\n{run.stdout}\nstderr:\n{run.stderr}"
        )
    if not stl_path.is_file():
        raise RuntimeError(f"Render did not produce STL: {stl_path}")
    return stl_path


def render_stl_png(stl_path: Path, png_path: Path, title: str) -> None:
    triangles = read_binary_stl(stl_path)
    if triangles.size == 0:
        raise RuntimeError(f"No triangles in {stl_path}")

    fig = plt.figure(figsize=(7.2, 5.2), dpi=160)
    ax = fig.add_subplot(111, projection="3d")
    ax.set_facecolor("#f8f2e8")
    fig.patch.set_facecolor("#f8f2e8")

    mesh = Poly3DCollection(triangles, linewidths=0.12, edgecolors=(0.18, 0.16, 0.14, 0.24))
    mesh.set_facecolor((0.72, 0.58, 0.34, 0.92))
    ax.add_collection3d(mesh)

    points = triangles.reshape(-1, 3)
    mins = points.min(axis=0)
    maxs = points.max(axis=0)
    center = (mins + maxs) / 2.0
    span = max(maxs - mins)
    radius = max(span * 0.62, 1.0)
    ax.set_xlim(center[0] - radius, center[0] + radius)
    ax.set_ylim(center[1] - radius, center[1] + radius)
    ax.set_zlim(center[2] - radius, center[2] + radius)
    ax.set_box_aspect((1, 1, 0.72))
    ax.view_init(elev=24, azim=-42)
    ax.set_axis_off()
    ax.set_title(title, fontsize=10, color="#2b241c", pad=8)
    fig.tight_layout(pad=0.2)
    fig.savefig(png_path, bbox_inches="tight", pad_inches=0.05)
    plt.close(fig)


def read_binary_stl(path: Path) -> np.ndarray:
    data = path.read_bytes()
    if len(data) < 84:
        raise RuntimeError(f"Invalid STL: {path}")
    tri_count = struct.unpack_from("<I", data, 80)[0]
    expected = 84 + tri_count * 50
    if expected > len(data):
        raise RuntimeError(f"Truncated STL: {path}")

    triangles = []
    offset = 84
    for _ in range(tri_count):
        offset += 12
        coords = struct.unpack_from("<9f", data, offset)
        triangles.append([(coords[0], coords[1], coords[2]), (coords[3], coords[4], coords[5]), (coords[6], coords[7], coords[8])])
        offset += 38
    return np.asarray(triangles, dtype=float)


def ensure_chapter_image(example: Example) -> None:
    markdown = example.chapter_path.read_text(encoding="utf-8")
    marker = f"![Rendered output for {example.title}](assets/{example.asset_name})"
    if marker in markdown:
        return

    if example.render_source_marker:
        next_markdown = markdown.replace(example.render_source_marker, f"{example.render_source_marker}\n\n{marker}", 1)
        example.chapter_path.write_text(next_markdown, encoding="utf-8")
        return

    pattern = re.compile(r"```scheme\n([\s\S]*?)```")
    seen = 0

    def replace(match: re.Match[str]) -> str:
        nonlocal seen
        code = match.group(1).strip()
        if not code.startswith("(model"):
            return match.group(0)
        seen += 1
        if seen != example.index:
            return match.group(0)
        return f"{match.group(0)}\n\n{marker}"

    next_markdown = pattern.sub(replace, markdown, count=0)
    example.chapter_path.write_text(next_markdown, encoding="utf-8")


if __name__ == "__main__":
    raise SystemExit(main())
