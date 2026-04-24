from __future__ import annotations

from pathlib import Path

from freecad_thomas_parity_common import SURFACE_FIXTURES, run_thomas_phase, write_legacy_until_marker


THOMAS_FULL_SURFACE = SURFACE_FIXTURES / "thomas_modular_ramp.ecky"


def prepare_legacy(source: Path, out: Path) -> None:
    write_legacy_until_marker(
        source,
        out,
        "# --- MODULAR SLICING LOGIC ---",
        "track",
    )


def main() -> int:
    return run_thomas_phase(
        surface_source=THOMAS_FULL_SURFACE,
        params={"has_teeth": True, "num_segments": 1, "print_segment": 0},
        prepare_legacy=prepare_legacy,
    )


if __name__ == "__main__":
    raise SystemExit(main())
