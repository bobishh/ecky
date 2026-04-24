#!/usr/bin/env python3
from __future__ import annotations

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent
CHECKS = [
    "check_freecad_canonical_cup_parity.py",
    "check_freecad_compound_boolean.py",
    "check_freecad_segment_clip.py",
    "check_freecad_repeat_segments.py",
    "check_freecad_frame_peg_attach.py",
    "check_freecad_plane_location_place.py",
    "check_freecad_path_frame_pose.py",
    "check_freecad_path_frame_up_pose.py",
    "check_freecad_thomas_body_parity.py",
    "check_freecad_thomas_grooves_parity.py",
    "check_freecad_thomas_teeth_parity.py",
    "check_freecad_thomas_connectors_parity.py",
    "check_freecad_thomas_ramp_parity.py",
]


def main() -> int:
    failed: list[str] = []
    for script_name in CHECKS:
        script_path = ROOT / script_name
        print(f"\n== {script_name} ==")
        result = subprocess.run(
            [sys.executable, str(script_path)],
            cwd=ROOT.parent,
            text=True,
            capture_output=True,
            check=False,
        )
        if result.stdout:
            print(result.stdout, end="")
        if result.stderr:
            print(result.stderr, end="", file=sys.stderr)
        if result.returncode != 0:
            failed.append(script_name)

    if failed:
        print("\nFAILED:")
        for name in failed:
            print(f"- {name}")
        return 1

    print(f"\nAll FreeCAD parity checks passed ({len(CHECKS)} fixtures).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
