from build123d import *


def build_segment_clip():
    shaft = Pos(-20.0, 0.0, 0.0) * (
        Rot(0.0, 90.0, 0.0)
        * Cylinder(
            6.0,
            40.0,
            align=(Align.CENTER, Align.CENTER, Align.MIN),
        )
    )
    clip = Box(
        20.0,
        10.0,
        12.0,
        align=(Align.CENTER, Align.CENTER, Align.CENTER),
    )
    return shaft & clip


_ecky_parts = [("body", build_segment_clip())]
