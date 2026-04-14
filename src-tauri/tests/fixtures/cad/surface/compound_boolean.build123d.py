from build123d import *


def build_compound_boolean():
    left_base = Pos(-18.0, 0.0, 0.0) * Box(
        18.0,
        18.0,
        12.0,
        align=(Align.CENTER, Align.CENTER, Align.MIN),
    )
    left_cut = Pos(-18.0, 0.0, 0.0) * Cylinder(
        4.0,
        12.0,
        align=(Align.CENTER, Align.CENTER, Align.MIN),
    )
    left_result = left_base - left_cut

    right_base = Pos(18.0, 0.0, 0.0) * Box(
        18.0,
        18.0,
        12.0,
        align=(Align.CENTER, Align.CENTER, Align.MIN),
    )
    right_pin = Pos(18.0, 0.0, 0.0) * Cylinder(
        5.0,
        12.0,
        align=(Align.CENTER, Align.CENTER, Align.MIN),
    )
    right_cap = Pos(18.0, 0.0, 6.0) * Sphere(7.0)
    right_result = (right_base + right_pin) & right_cap

    return Compound(children=[left_result, right_result])


_ecky_parts = [("body", build_compound_boolean())]
