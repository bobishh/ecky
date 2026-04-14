from build123d import *


def build_repeat_segments():
    ribs = Compound(
        children=[
            Pos(float(index) * 10.0, 0.0, 0.0)
            * Box(
                4.0,
                8.0,
                6.0,
                align=(Align.CENTER, Align.CENTER, Align.MIN),
            )
            for index in range(4)
        ]
    )

    rollers = Compound(
        children=[
            Pos(float(index) * 10.0 + 5.0, 0.0, 0.0)
            * Cylinder(
                2.0,
                6.0,
                align=(Align.CENTER, Align.CENTER, Align.MIN),
            )
            for index in range(4)
        ]
    )

    marker = None
    for index in range(4):
        if float(index) == 3.0:
            marker = Pos(float(index) * 10.0 + 5.0, 0.0, 12.0) * Sphere(3.0)

    if marker is None:
        raise ValueError("repeat-pick found no matching geometry")

    return Compound(children=[ribs, rollers, marker])


_ecky_parts = [("body", build_repeat_segments())]
