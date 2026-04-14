#!/usr/bin/env python3
from __future__ import annotations

from check_primitive_parity_common import SURFACE_FIXTURES, check_surface_fixture


def main() -> int:
    return check_surface_fixture(
        "compound_boolean",
        SURFACE_FIXTURES / "compound_boolean.ecky",
        SURFACE_FIXTURES / "compound_boolean.build123d.py",
    )


if __name__ == "__main__":
    raise SystemExit(main())
