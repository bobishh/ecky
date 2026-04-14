from pathlib import Path
import importlib.util
import sys


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "src-tauri" / "tests" / "fixtures" / "cad"
SURFACE_FIXTURES = FIXTURES / "surface"
CANONICAL_BUILD123D = SURFACE_FIXTURES / "canonical_cup.build123d.py"


def load_fixture_module():
    spec = importlib.util.spec_from_file_location("canonical_cup_build123d", CANONICAL_BUILD123D)
    module = importlib.util.module_from_spec(spec)
    assert spec is not None and spec.loader is not None
    spec.loader.exec_module(module)
    return module


def main():
    canonical_source = CANONICAL_BUILD123D.read_text()
    fixture = load_fixture_module()

    assert "tangent_scalars=[1.75, 1.0]" in canonical_source
    assert (
        "offset(amount=-wall_thickness, openings=tea_cup.faces().filter_by(GeomType.PLANE))"
        in canonical_source
    )
    assert "Rot(90" not in canonical_source

    cup = fixture.build_canonical_cup()
    bb = cup.bounding_box()

    assert cup.volume > 100000.0
    assert bb.max.Z > 100.0

    print(f"Volume: {cup.volume:.2f}")
    print(f"BB: {bb.min} to {bb.max}")
    print("Canonical cup fixture OK")


if __name__ == "__main__":
    main()
