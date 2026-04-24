from build123d import *


def build_plane_location_place():
    plane = Plane(origin=(10.0, 20.0, 30.0), x_dir=(0.0, 1.0, 0.0), z_dir=(0.0, 0.0, 1.0))
    pose = Location(plane) * Pos(5.0, 0.0, 0.0) * Rot(0.0, 90.0, 0.0)
    return pose * Box(4.0, 6.0, 2.0, align=(Align.MIN, Align.MIN, Align.MIN))


_ecky_parts = [("body", build_plane_location_place())]
