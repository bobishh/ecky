#!/usr/bin/env python3
from __future__ import annotations

from check_primitive_parity_common import SURFACE_FIXTURES, check_surface_fixture


def main() -> int:
    return check_surface_fixture(
        "segment_clip",
        SURFACE_FIXTURES / "segment_clip.ecky",
        SURFACE_FIXTURES / "segment_clip.build123d.py",
    )


if __name__ == "__main__":
    raise SystemExit(main())
