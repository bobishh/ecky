from __future__ import annotations

import argparse
import json
from pathlib import Path

from build123d import *
from build123d import export_stl


wall_thickness = 3.0
fillet_radius = wall_thickness * 0.49


def build_canonical_cup():
    with BuildPart() as tea_cup:
        with BuildSketch(Plane.XZ):
            with BuildLine():
                side = Spline(
                    (30.0, 10.0),
                    (69.0, 105.0),
                    periodic=False,
                    tangents=[(1.0, 0.5), (0.7, 1.0)],
                    tangent_scalars=[1.75, 1.0],
                )
                Polyline(
                    side @ 0,
                    side @ 0 + (10.0, -10.0),
                    (0.0, 0.0),
                    (0.0, (side @ 1).Y),
                    side @ 1,
                )
            make_face()
        revolve(axis=Axis.Z)
        offset(amount=-wall_thickness, openings=tea_cup.faces().filter_by(GeomType.PLANE))
        with Locations((0.0, 0.0, 10.0)):
            Cylinder(radius=30.0, height=wall_thickness)
        fillet(tea_cup.edges(), radius=fillet_radius)
    return tea_cup.part


def metrics(shape):
    bb = shape.bounding_box()
    return {
        "volume": float(shape.volume),
        "min_bound": [float(bb.min.X), float(bb.min.Y), float(bb.min.Z)],
        "max_bound": [float(bb.max.X), float(bb.max.Y), float(bb.max.Z)],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--export-stl", dest="export_stl_path")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    cup = build_canonical_cup()
    data = metrics(cup)

    if args.export_stl_path:
        export_path = Path(args.export_stl_path)
        export_path.parent.mkdir(parents=True, exist_ok=True)
        export_stl(cup, export_path)

    if args.json:
        print(json.dumps(data))
    else:
        print(f"Volume: {data['volume']:.2f}")
        print(f"BB Min: {tuple(data['min_bound'])}")
        print(f"BB Max: {tuple(data['max_bound'])}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
