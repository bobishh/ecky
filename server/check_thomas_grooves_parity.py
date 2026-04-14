from __future__ import annotations

from pathlib import Path

from thomas_parity_common import SURFACE_FIXTURES, run_thomas_phase, write_legacy_until_marker


THOMAS_GROOVES_SURFACE = SURFACE_FIXTURES / "thomas_modular_ramp_grooves.ecky"


def prepare_legacy(source: Path, out: Path) -> None:
    write_legacy_until_marker(source, out, "# Teeth - Trapezoidal Profile!", "track")


def main() -> int:
    return run_thomas_phase(
        surface_source=THOMAS_GROOVES_SURFACE,
        params={"has_teeth": False, "num_segments": 1, "print_segment": 0},
        prepare_legacy=prepare_legacy,
    )


if __name__ == "__main__":
    raise SystemExit(main())
