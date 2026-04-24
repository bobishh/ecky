#!/usr/bin/env python3
from __future__ import annotations

from check_freecad_surface_parity_common import SURFACE_FIXTURES, check_surface_fixture


def main() -> int:
    return check_surface_fixture(
        "plane_location_place",
        SURFACE_FIXTURES / "plane_location_place.ecky",
        SURFACE_FIXTURES / "plane_location_place.build123d.py",
    )


if __name__ == "__main__":
    raise SystemExit(main())
