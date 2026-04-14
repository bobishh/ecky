from build123d import *


def build_frame_peg_attach():
    with BuildPart() as frame_peg_attach:
        with Locations((20.0, 0.0, 0.0)):
            Box(4.0, 4.0, 4.0)
    return frame_peg_attach.part


_ecky_parts = [("body", build_frame_peg_attach())]
