import FreeCAD as App
import Part
import math

# Parameters
gauge = params.get("gauge", 26.0)
width = params.get("width", 40.0)
height = params.get("height", 12.0)
groove_width = params.get("groove_width", 6.0)
groove_depth = params.get("groove_depth", 3.0)

# Ramp parameters
duplo_height_blocks = params.get("duplo_height_blocks", 5)
flat_start = params.get("flat_start", 48.0)
ramp_length = params.get("ramp_length", 192.0)
flat_end = params.get("flat_end", 48.0)
has_teeth = params.get("has_teeth", True)
teeth_size = params.get("teeth_size", 5.0)

# Modular parameters
num_segments = int(params.get("num_segments", 3))
print_segment = int(params.get("print_segment", 0))

duplo_block_h = 19.2
dz = duplo_height_blocks * duplo_block_h
L = flat_start + ramp_length + flat_end


def make_connector_cutout():
    cutout_radius = 6.0
    neck_width_cutout = 7.0
    neck_length_cutout = 8.5
    c1 = Part.makeCylinder(cutout_radius, height)
    c1.translate(App.Vector(neck_length_cutout, 0, 0))
    box = Part.makeBox(neck_length_cutout, neck_width_cutout, height)
    box.translate(App.Vector(0, -neck_width_cutout / 2, 0))
    return c1.fuse(box)


def make_connector_peg():
    peg_radius = 5.5
    neck_width_peg = 6.0
    neck_length_peg = 8.0
    c1 = Part.makeCylinder(peg_radius, height)
    c1.translate(App.Vector(neck_length_peg, 0, 0))
    box = Part.makeBox(neck_length_peg, neck_width_peg, height)
    box.translate(App.Vector(0, -neck_width_peg / 2, 0))
    return c1.fuse(box)


def z_at(x):
    if x <= flat_start:
        return 0.0
    if x >= flat_start + ramp_length:
        return dz
    t = (x - flat_start) / ramp_length
    return dz * (0.5 - 0.5 * math.cos(math.pi * t))


def dz_dx_at(x):
    if x <= flat_start:
        return 0.0
    if x >= flat_start + ramp_length:
        return 0.0
    t = (x - flat_start) / ramp_length
    return (dz * 0.5 * math.pi / ramp_length) * math.sin(math.pi * t)


N_ramp = 100
xs = [0.0, flat_start]
for i in range(1, N_ramp):
    xs.append(flat_start + i * ramp_length / N_ramp)
xs.append(flat_start + ramp_length)
xs.append(L)

# Restored Original Body Generation (12mm thick track that follows the curve)
pts_body = []
for x in xs:
    pts_body.append(App.Vector(x, 0, z_at(x) + height))
for x in reversed(xs):
    pts_body.append(App.Vector(x, 0, z_at(x)))
pts_body.append(pts_body[0])

poly_body = Part.makePolygon(pts_body)
face_body = Part.Face(poly_body)
body = face_body.extrude(App.Vector(0, width, 0))
body.translate(App.Vector(0, -width / 2, 0))

# Grooves - using sweep for 45-deg sloped walls (support-free when sideways!)
try:
    top_w = groove_width + 2.0
    bot_w = groove_width - 2.0
    d = groove_depth
    pts_prof = [
        App.Vector(0, -bot_w / 2, height - d),
        App.Vector(0, bot_w / 2, height - d),
        App.Vector(0, top_w / 2, height + 2),
        App.Vector(0, -top_w / 2, height + 2),
    ]
    profile = Part.Wire(Part.makePolygon(pts_prof + [pts_prof[0]]))
    pts_spine = [App.Vector(x, 0, z_at(x)) for x in xs]
    spine = Part.Wire(
        [Part.makeLine(pts_spine[j], pts_spine[j + 1]) for j in range(len(pts_spine) - 1)]
    )

    g1 = spine.makePipeShell([profile], True, False)
    g1.translate(App.Vector(0, gauge / 2, 0))

    g2 = spine.makePipeShell([profile], True, False)
    g2.translate(App.Vector(0, -gauge / 2, 0))

    track = body.cut(g1).cut(g2)
except:
    # Fallback to original square grooves if sweep fails
    pts_groove = []
    for x in xs:
        pts_groove.append(App.Vector(x, 0, z_at(x) + height + 5))
    for x in reversed(xs):
        pts_groove.append(App.Vector(x, 0, z_at(x) + height - groove_depth))
    pts_groove.append(pts_groove[0])

    poly_groove = Part.makePolygon(pts_groove)
    face_groove = Part.Face(poly_groove)

    g1 = face_groove.extrude(App.Vector(0, groove_width, 0))
    g1.translate(App.Vector(0, gauge / 2 - groove_width / 2, 0))

    g2 = face_groove.extrude(App.Vector(0, groove_width, 0))
    g2.translate(App.Vector(0, -gauge / 2 - groove_width / 2, 0))

    track = body.cut(g1).cut(g2)

# Teeth - Trapezoidal Profile!
if has_teeth:

    def make_inverse_tooth(l, w, h):
        top_w = 3.5
        bot_w = 1.5
        pts = [
            App.Vector(-top_w / 2, 0, 10),
            App.Vector(-top_w / 2, 0, 0),
            App.Vector(-bot_w / 2, 0, -h),
            App.Vector(bot_w / 2, 0, -h),
            App.Vector(top_w / 2, 0, 0),
            App.Vector(top_w / 2, 0, 10),
        ]
        poly = Part.makePolygon(pts + [pts[0]])
        face = Part.Face(poly)
        tooth = face.extrude(App.Vector(0, w, 0))
        tooth.translate(App.Vector(0, -w / 2, 0))
        return tooth

    num_teeth = int(L // teeth_size)
    valleys = []

    for i in range(num_teeth):
        x_pos = (i + 0.5) * teeth_size
        z_pos = z_at(x_pos) + height - groove_depth
        slope = dz_dx_at(x_pos)
        angle_rad = math.atan(slope)
        angle_deg = math.degrees(angle_rad)

        actual_pitch = teeth_size / math.cos(angle_rad)
        v_unit = make_inverse_tooth(actual_pitch, groove_width, 2.5)  # 2.5mm depth

        v1 = v_unit.copy()
        v1.rotate(App.Vector(0, 0, 0), App.Vector(0, 1, 0), -angle_deg)
        v1.translate(App.Vector(x_pos, gauge / 2, z_pos))

        v2 = v_unit.copy()
        v2.rotate(App.Vector(0, 0, 0), App.Vector(0, 1, 0), -angle_deg)
        v2.translate(App.Vector(x_pos, -gauge / 2, z_pos))

        valleys.extend([v1, v2])

    if valleys:
        track = track.cut(Part.makeCompound(valleys))

# Duplo bottom connectors
duplo_r = 4.8
duplo_depth = 5.0
duplo_pitch = 16.0

holes = []
x_holes = int(L // duplo_pitch)
for i in range(1, x_holes - 1):
    cx = duplo_pitch / 2 + i * duplo_pitch

    is_start_flat = cx <= flat_start
    is_end_flat = cx >= flat_start + ramp_length

    if is_start_flat:
        cz = 0.0
    elif is_end_flat:
        cz = dz
    else:
        continue

    for cy in [-duplo_pitch / 2, duplo_pitch / 2]:
        cyl = Part.makeCylinder(duplo_r, duplo_depth)
        cyl.translate(App.Vector(cx, cy, cz))
        holes.append(cyl)

if holes:
    track = track.cut(Part.makeCompound(holes))

# End connectors (standard Thomas)
thomas_cutout_tool = make_connector_cutout()
thomas_peg_tool = make_connector_peg()
thomas_peg_tool.translate(App.Vector(L, 0, dz))

track = track.cut(thomas_cutout_tool)
track = track.fuse(thomas_peg_tool)

track_original = track.copy()

# --- MODULAR SLICING LOGIC ---
segments = []
if num_segments < 1:
    num_segments = 1
seg_len = L / num_segments


def get_puzzle_tool(x_pos):
    c1 = Part.makeCylinder(4.5, dz + height + 100)
    c1.translate(App.Vector(4.5, 0, -50))
    box = Part.makeBox(4.5, 6.0, dz + height + 100)
    box.translate(App.Vector(0, -3.0, -50))
    tool = c1.fuse(box)
    tool.translate(App.Vector(x_pos, 0, 0))
    return tool


for i in range(num_segments):
    X0 = i * seg_len
    X1 = (i + 1) * seg_len

    box_X0 = X0
    box_X1 = X1
    if i == 0:
        box_X0 -= 20
    if i == num_segments - 1:
        box_X1 += 20

    bbox = Part.makeBox(box_X1 - box_X0, width + 50, dz + height + 50)
    bbox.translate(App.Vector(box_X0, -(width + 50) / 2, -20))

    seg = track_original.common(bbox)

    if i > 0:
        tool_X0 = get_puzzle_tool(X0)
        seg = seg.cut(tool_X0)

    if i < num_segments - 1:
        tool_X1 = get_puzzle_tool(X1)
        peg_solid = tool_X1.common(track_original)
        seg = seg.fuse(peg_solid)

    segments.append(seg)

if print_segment == 0:
    for i in range(num_segments):
        offset_y = (i - (num_segments - 1) / 2.0) * 15
        segments[i].translate(App.Vector(0, offset_y, 0))
    track = Part.makeCompound(segments)
else:
    seg_idx = print_segment - 1
    if seg_idx < 0:
        seg_idx = 0
    if seg_idx >= num_segments:
        seg_idx = num_segments - 1

    seg = segments[seg_idx]
    # RESTORED ROTATION: This lays the segment on its side!
    seg.rotate(App.Vector(0, 0, 0), App.Vector(1, 0, 0), 90)
    bb = seg.BoundBox
    seg.translate(App.Vector(0, 0, -bb.ZMin))

    X0 = seg_idx * seg_len
    X1 = (seg_idx + 1) * seg_len
    seg.translate(App.Vector(-(X0 + X1) / 2, 0, 0))
    track = seg

try:
    track = track.removeSplitter()
except:
    pass

Part.show(track, "Thomas_Modular_Ramp")
