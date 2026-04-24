from build123d import *


def build_path_frame_up_pose():
    rail = Polyline((0.0, 0.0, 0.0), (20.0, 0.0, 10.0), (20.0, 10.0, 10.0))
    frame = rail.location_at(
        1.0,
        position_mode=PositionMode.PARAMETER,
        frame_method=FrameMethod.FRENET,
        x_dir=Vector(1.0, 0.0, 0.0),
    )
    pose = frame * Pos(1.0, 2.0, 3.0) * Rot(10.0, 20.0, 30.0)
    return pose * Box(4.0, 2.0, 6.0, align=(Align.MIN, Align.MIN, Align.MIN))


_ecky_parts = [("body", build_path_frame_up_pose())]
