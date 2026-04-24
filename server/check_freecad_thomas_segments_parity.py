from __future__ import annotations

from freecad_thomas_parity_common import SURFACE_FIXTURES, run_thomas_phase, write_legacy_full


THOMAS_FULL_SURFACE = SURFACE_FIXTURES / "thomas_modular_ramp.ecky"


def main() -> int:
    return run_thomas_phase(
        surface_source=THOMAS_FULL_SURFACE,
        params={"has_teeth": False, "num_segments": 3, "print_segment": 1},
        prepare_legacy=write_legacy_full,
    )


if __name__ == "__main__":
    raise SystemExit(main())
