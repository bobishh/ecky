#!/usr/bin/env python3
from __future__ import annotations

from check_freecad_surface_parity_common import SURFACE_FIXTURES, check_surface_fixture


def main() -> int:
    return check_surface_fixture(
        "frame_peg_attach",
        SURFACE_FIXTURES / "frame_peg_attach.ecky",
        SURFACE_FIXTURES / "frame_peg_attach.build123d.py",
    )


if __name__ == "__main__":
    raise SystemExit(main())
