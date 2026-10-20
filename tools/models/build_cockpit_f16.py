# Builds the F-16 cockpit master (assets-src/blend/cockpit_f16.blend) from the F-117A's modern
# cockpit and the F-16 panel layout:
#   blender -b assets-src/blend/cockpit_modern.blend --python-exit-code 1 \
#       --python tools/models/build_cockpit_f16.py -- assets-src/blend/cockpit_f16_layout.json \
#       assets-src/blend/cockpit_f16.blend
# then tools/models/export.sh cockpit_f16.
#
# Kept from the modern cockpit (with their materials, labels and control properties): the stick,
# the throttle and its rail, the seat, the ejection and gear handles, the side panel buttons,
# the flood lamps, the HUD glass and the approach indexer. Everything else is built here: the tub
# and side consoles, the glare shield and up-front controls, the main panel with the faces and
# displays of `PanelLayout::f16` (src/cockpit3d/layout.rs; the layout JSON is written by
# `UPDATE_F16_LAYOUT=1 cargo test f16_layout_file`).
#
# Blender frame: eye at the origin, x right, y forward, z up, metres. A control's face points
# along its local -Y (towards the pilot), local Z is up the panel.
import json
import math
import sys

import bmesh
import bpy
from mathutils import Euler, Matrix, Quaternion, Vector

layout_path, out = sys.argv[sys.argv.index("--") + 1:][:2]
layout = json.load(open(layout_path))
root = bpy.data.objects["cockpit"]

# The canopy is narrow (+-0.37 m at its sill): the cockpit stays inside that.
PANEL_TILT = math.radians(20.0)  # top of the panel leaning away
PANEL_CENTRE = Vector((0.0, 0.66, -0.30))
CONSOLE_TOP = -0.585
CONSOLE_X = (0.20, 0.36)
SILL = -0.45
WALL_X = 0.37
# The panel's wings turn towards the pilot from the centre column's edges.
HINGE = 75.0
WING_ANGLE = math.radians(25.0)
# The up-front controls stand out of the centre column (layout y < ICP_BOTTOM).
ICP_BOTTOM = -80.0
ICP_DEPTH = 0.04
# Eyebrows: upright light boxes on the glare shield either side of the HUD (layout |x| >= 318).
EYEBROW = 318.0
EYEBROW_X = 0.235
EYEBROW_TILT = math.radians(10.0)
TOP = -160.0

RIGHT = Vector((1.0, 0.0, 0.0))
UP = Vector((0.0, math.sin(PANEL_TILT), math.cos(PANEL_TILT)))
NORMAL = Vector((0.0, -math.cos(PANEL_TILT), math.sin(PANEL_TILT)))


def section(u):
    """-2 / 2 left / right eyebrow, -1 / 1 left / right wing, 0 centre column."""
    if abs(u) >= EYEBROW:
        return int(math.copysign(2, u))
    if abs(u) > HINGE:
        return int(math.copysign(1, u))
    return 0


def basis(sec):
    """(across, normal towards the pilot, up) of a section."""
    if sec == 0:
        return RIGHT, NORMAL, UP
    s = math.copysign(1, sec)
    if abs(sec) == 1:
        c, n = math.cos(WING_ANGLE), math.sin(WING_ANGLE)
        return RIGHT * c + NORMAL * n * s, NORMAL * c - RIGHT * n * s, UP
    up = Vector((0.0, math.sin(EYEBROW_TILT), math.cos(EYEBROW_TILT)))
    normal = Vector((0.0, -math.cos(EYEBROW_TILT), math.sin(EYEBROW_TILT)))
    return RIGHT, normal, up


def point(u, v, offset=0.0):
    """A panel point: layout millimetres (u right, v down), raised `offset` m off its surface."""
    sec = section(u)
    d, n, up = basis(sec)
    s = math.copysign(1, u)
    if sec == 0:
        p = PANEL_CENTRE + RIGHT * (u / 1000) - UP * (v / 1000)
        if v < ICP_BOTTOM:
            p += NORMAL * ICP_DEPTH
    elif abs(sec) == 1:
        hinge = PANEL_CENTRE + RIGHT * (s * HINGE / 1000) - UP * (v / 1000)
        p = hinge + d * ((u - s * HINGE) / 1000)
    else:
        # Eyebrow: centred at x = +-EYEBROW_X on the glare shield, layout x 360 at its middle.
        base = point(0.0, TOP) + NORMAL * 0.005
        p = Vector((s * EYEBROW_X, base.y + 0.03, base.z + 0.012)) + d * ((u - s * 360.0) / 1000) + up * ((TOP - v) / 1000)
    return p + n * offset


def rotation(u):
    d, n, up = basis(section(u))
    return Matrix((d, -n, up)).transposed().to_quaternion()


def rect_point(r, offset=0.0):
    x, y, w, h = r
    return point(x + w / 2.0, y + h / 2.0, offset)


# --- keep, drop
def kept(o):
    n = o.name
    keep = ("stick", "throttle", "control_Action_Eject_", "control_Action_Gear_.001", "button_", "seat_", "cushion_", "lamp", "hud_glass", "approach", "metal_98", "metal_99")
    if n.startswith(keep):
        return True
    # The stick boot and the throttle rail.
    p = o.matrix_world.translation
    return abs(abs(p.x) - 0.4) < 0.03 and 0.15 < p.y < 0.3 and p.z < -0.45 and o.type == "MESH"


def drop(o):
    for c in list(o.children):
        drop(c)
    bpy.data.objects.remove(o, do_unlink=True)


for o in list(root.children):
    if not kept(o):
        drop(o)
old = list(root.children)

# --- materials
mats = bpy.data.materials


def material(name, like):
    m = mats.get(name)
    if m is None:
        m = mats[like].copy()
        m.name = name
    return m


for n, like in (("hsi", "adi"), ("screen_rwr", "adi"), ("trackcam_left", "trackcam")):
    material(n, like)


# --- building blocks
def quat(rot):
    return rot if isinstance(rot, Quaternion) else rot.to_quaternion()


def mesh_object(name, mesh, loc, rot=Euler((0, 0, 0)), parent=root, props=None):
    o = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(o)
    o.parent = parent
    o.rotation_mode = "QUATERNION"
    o.location = loc
    o.rotation_quaternion = quat(rot)
    for k, v in (props or {}).items():
        o[k] = v
    return o


def empty(name, loc, rot=Euler((0, 0, 0)), parent=root, props=None):
    o = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(o)
    o.parent = parent
    o.rotation_mode = "QUATERNION"
    o.location = loc
    o.rotation_quaternion = quat(rot)
    for k, v in (props or {}).items():
        o[k] = v
    return o


def box_mesh(name, size, mat, centre=(0.0, 0.0, 0.0)):
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0, matrix=Matrix.Translation(centre) @ Matrix.Diagonal((*size, 1.0)))
    uv = bm.loops.layers.uv.new()
    for f in bm.faces:
        for i, l in enumerate(f.loops):
            l[uv].uv = ((0, 0), (1, 0), (1, 1), (0, 1))[i % 4]
    bm.to_mesh(me)
    bm.free()
    me.materials.append(mats[mat])
    return me


def quad_mesh(name, w, h, mat, y=0.0):
    """A quad in the local XZ plane at local y (faces -Y), UVs 0..1 with (0, 0) bottom left."""
    me = bpy.data.meshes.new(name)
    v = [(-w / 2, y, -h / 2), (w / 2, y, -h / 2), (w / 2, y, h / 2), (-w / 2, y, h / 2)]
    me.from_pydata(v, [], [(0, 1, 2, 3)])
    uv = me.uv_layers.new()
    for i, d in enumerate(uv.data):
        d.uv = ((0, 0), (1, 0), (1, 1), (0, 1))[i]
    me.materials.append(mats[mat])
    return me


def box(name, mat, centre, size, rot=Euler((0, 0, 0))):
    return mesh_object(name, box_mesh(name, size, mat), centre, rot)


def panel_box(name, mat, r, raise_m, margin=0.0, base=0.0):
    """A plate from `base` to `base + raise_m` off the panel over the rectangle r (mm, within one
    section) grown by `margin` mm."""
    x, y, w, h = r
    c = rect_point(r, base + raise_m / 2)
    return box(name, mat, c, ((w + 2 * margin) / 1000, abs(raise_m), (h + 2 * margin) / 1000), rotation(x + w / 2))


def face(key, r, offset):
    x, y, w, h = r
    me = quad_mesh("atlas", w / 1000, h / 1000, "atlas")
    mesh_object("atlas", me, rect_point(r, offset), rotation(x + w / 2), props={"atlas_face": key, "atlas_uv": [0.0, 0.0, 1.0, 1.0], "no_shadow": True})


def control(key, r, offset, effect, kind):
    """A push button: origin on the surface, its face 6 mm out; a dark cap body under the face
    and a highlight frame."""
    x, y, w, h = r
    hw, hh = w / 2000, h / 2000
    height, travel = 0.006, 0.004
    name = "control_" + "".join(ch if ch.isalnum() else "_" for ch in effect)
    c = empty(name, rect_point(r, offset), rotation(x + w / 2), props={"control": effect, "kind": kind, "half": [hw, hh], "height": height, "travel": travel})
    moving = empty("moving", (0, 0, 0), parent=c, props={"moving": True})
    mesh_object("atlas", quad_mesh("atlas", 2 * hw, 2 * hh, "atlas", -height), (0, 0, 0), parent=moving, props={"atlas_face": key, "atlas_uv": [0.0, 0.0, 1.0, 1.0], "no_shadow": True})
    mesh_object("dark", box_mesh("dark", (2 * hw, height - 0.0004, 2 * hh), "dark", (0, -(height - 0.0004) / 2, 0)), (0, 0, 0), parent=moving)
    hl = empty("highlight", (0, 0, 0), parent=c, props={"highlight": True})
    t = 0.002
    for (cx, cz, sx, sz) in ((0, hh + t, 2 * hw + 2 * t, t), (0, -hh - t, 2 * hw + 2 * t, t), (hw + t, 0, t, 2 * hh), (-hw - t, 0, t, 2 * hh)):
        mesh_object("highlight", box_mesh("highlight", (sx, t, sz), "highlight"), (cx, -height, cz), parent=hl, props={"no_shadow": True})


def screen(mat, r, offset, behind=None):
    x, y, w, h = r
    rot = rotation(x + w / 2)
    mesh_object(mat, quad_mesh(mat, w / 1000, h / 1000, mat), rect_point(r, offset), rot, props={"no_shadow": True})
    if behind:
        mesh_object(behind, quad_mesh(behind, w / 1000, h / 1000, behind), rect_point(r, offset - 0.003), rot, props={"no_shadow": True})


# --- tub, consoles, glare shield
box("panel_grey", "panel_grey", (0.0, 0.05, -1.0), (2 * WALL_X, 1.4, 0.02))  # floor
for s in (-1, 1):
    box("panel_grey", "panel_grey", (s * WALL_X, 0.05, (SILL - 1.0) / 2), (0.02, 1.4, 1.0 + SILL))  # walls
    box("frame", "frame", (s * (WALL_X - 0.01), 0.05, SILL), (0.04, 1.4, 0.02))  # canopy sills
    cx = s * sum(CONSOLE_X) / 2
    box("panel_grey", "panel_grey", (cx, 0.12, (CONSOLE_TOP - 1.0) / 2), (CONSOLE_X[1] - CONSOLE_X[0], 0.86, 1.0 + CONSOLE_TOP))
    box("dark", "dark", (cx, 0.12, CONSOLE_TOP + 0.002), (CONSOLE_X[1] - CONSOLE_X[0] - 0.01, 0.84, 0.004))
box("panel_grey", "panel_grey", (0.0, -0.68, -0.55), (2 * WALL_X, 0.03, 0.9))  # rear bulkhead
# Panel: the centre column and the wings (their faces on the panel surfaces), the knee well.
for r in ((-HINGE, TOP, 2 * HINGE, 270.0), (-305.0, TOP, 305.0 - HINGE, 260.0), (HINGE, TOP, 305.0 - HINGE, 260.0)):
    panel_box("panel_grey", "panel_grey", r, 0.03, 0.0, -0.03)
low = point(0, 112)
box("panel_grey", "panel_grey", (0.0, low.y + 0.08, (low.z - 1.0) / 2), (0.62, 0.02, 1.0 + low.z))
# Up-front controls' pedestal, the hood over the attitude indicator and HSI.
panel_box("dark", "dark", (-HINGE, TOP, 2 * HINGE, ICP_BOTTOM - TOP - 2.0), ICP_DEPTH + 0.03, 0.0, -0.03 - ICP_DEPTH)
d, n, up = basis(0)
lid = Matrix((d, -n, up)).transposed() @ Matrix.Rotation(math.radians(15.0), 3, "X")
mesh_object("black", box_mesh("black", (2 * HINGE / 1000 + 0.01, 0.06, 0.008), "black"), point(0.0, -72.0, 0.03), lid.to_quaternion())
# Glare shield: hoods over the wings and the pedestal, sloping down and away.
for (u0, u1) in ((-305.0, -HINGE), (HINGE, 305.0), (-HINGE, HINGE)):
    um = (u0 + u1) / 2
    d, n, up = basis(section(um))
    depth = ICP_DEPTH if section(um) == 0 else 0.0
    edge = point(um, TOP, depth)
    w = abs(u1 - u0) / 1000 + 0.01
    m = Matrix((d, -n, up)).transposed() @ Matrix.Rotation(math.radians(-8.0), 3, "X")
    c = edge + m @ Vector((0.0, 0.14, 0.008))
    mesh_object("black", box_mesh("black", (w, 0.30, 0.016), "black"), c, m.to_quaternion())
# Eyebrows: light boxes on the glare shield either side of the HUD.
for s in (-1, 1):
    r = (s * 360.0 - 38.0, -252.0, 76.0, 96.0)
    panel_box("black", "black", r, 0.05, 2.0, -0.05)
# HUD frame: posts either side of the combiner, a bar over it.
hud_y, hud_lo, hud_hi = 0.70, -0.125, 0.075
for s in (-1, 1):
    box("frame", "frame", (s * 0.175, hud_y, (hud_lo + hud_hi) / 2), (0.015, 0.025, hud_hi - hud_lo))
box("frame", "frame", (0.0, hud_y + 0.02, hud_hi), (0.365, 0.04, 0.015))
# Canopy bow behind the seat.
for k in range(9):
    a0, a1 = math.pi * k / 9, math.pi * (k + 1) / 9
    r = 0.36
    p0 = Vector((r * math.cos(a0), -0.62, SILL + 0.62 * math.sin(a0)))
    p1 = Vector((r * math.cos(a1), -0.62, SILL + 0.62 * math.sin(a1)))
    d = p1 - p0
    rot = Vector((1, 0, 0)).rotation_difference(d.normalized()).to_euler()
    box("frame", "frame", (p0 + p1) / 2, (d.length + 0.01, 0.03, 0.03), rot)

# --- panel plates, instrument bezels, faces and displays
PLATE = 0.004
scr = layout["screens"]
plates = [
    (-72.0, -156.0, 144.0, 72.0),  # up-front controls
    (114.0, -150.0, 84.0, 74.0),  # countermeasures
    (-300.0, -28.0, 48.0, 94.0),  # aircraft systems
    (250.0, -44.0, 48.0, 84.0),  # fuel, thrust
]
for r in plates:
    panel_box("dark", "dark", r, PLATE)
for key in ("adi", "hsi", "rwr"):
    panel_box("black", "black", scr[key], 0.012, 5.0)
for key in ("left", "right"):
    panel_box("black", "black", scr[key], 0.014, 0.0)
    x, y, w, h = scr[key]
    panel_box("dark", "dark", (x - 16.0, y - 22.0, w + 32.0, h + 44.0), 0.012)
    # Side bezel buttons (fixed: the game uses the top and bottom rows).
    for k in range(5):
        for bx in (x - 9.0, x + w + 9.0):
            by = y + h * (k + 0.5) / 5
            panel_box("dark", "dark", (bx - 5.0, by - 4.0, 10.0, 8.0), 0.005, 0.0, 0.012)
screen("screen_left", scr["left"], 0.0145, "trackcam_left")
screen("screen_right", scr["right"], 0.0145, "trackcam")
screen("adi", scr["adi"], 0.0125)
screen("hsi", scr["hsi"], 0.0125)
screen("screen_rwr", scr["rwr"], 0.0125)
for f in layout["faces"]:
    if f["control"] is None:
        face(f["key"], f["rect"], PLATE + 0.0005)
    elif f["kind"] == "osb":
        control(f["key"], f["rect"], 0.012, f["control"], "osb")
    else:
        control(f["key"], f["rect"], PLATE, f["control"], f["kind"])

# --- the kept parts, moved into the F-16's cockpit
objs = {o.name: o for o in root.children}


def move(o, dx=0.0, dy=0.0, dz=0.0):
    o.location += Vector((dx, dy, dz))


# Stick on the right console, throttle on the left one (both 0.12 m further in).
for o in old:
    p = o.location
    if o.name.startswith(("stick", "rubber")) or (o.type == "MESH" and abs(p.x - 0.4) < 0.03 and p.z < -0.45):
        move(o, dx=-0.12)
    elif o.name.startswith("throttle") or (o.type == "MESH" and abs(p.x + 0.4) < 0.03 and p.z < -0.45):
        move(o, dx=0.12)
# Seat back, head rest and rails reclined 15 degrees more (the F-16's 30 degree seat).
hip = Vector((0.0, -0.30, -0.78))
recline = Matrix.Translation(hip) @ Matrix.Rotation(math.radians(15.0), 4, "X") @ Matrix.Translation(-hip)
for o in root.children:
    if o.name.startswith(("seat_95", "cushion_96", "cushion_97", "metal_98", "metal_99")):
        o.matrix_world = recline @ o.matrix_world
# Side panel buttons on the console fronts, two columns of three, faces up towards the pilot.
left = ["button_NEAR", "button_DESG", "button_NEXT", "button_NONE", "button_WPN", "button_MENU"]
right = ["button_NVG", "button_ILS", "button_TIME", "button_DCLT", "button_VOL", "button_DTL"]
for names, s in ((left, -1), (right, 1)):
    for i, n in enumerate(names):
        o = objs[n]
        col, row = i % 2, i // 2
        o.location = (s * (0.26 + 0.045 * col), 0.46 - 0.034 * row, CONSOLE_TOP + 0.004)
        o.rotation_quaternion = Euler((math.radians(-55.0), 0.0, 0.0)).to_quaternion()
# Gear handle low on the left of the panel.
g = objs["control_Action_Gear_.001"]
g.location = point(-276.0, 84.0, 0.0)
g.rotation_quaternion = rotation(-276.0)
# HUD glass: a smaller combiner over the up-front controls (its vertices are absolute).
hud = next(o for o in root.children if o.name.startswith("hud_glass"))
for v in hud.data.vertices:
    v.co = Vector((v.co.x * 0.45, 0.68 + (v.co.y - 0.8), (v.co.z - 0.025) * 0.62))
# Approach indexer left of the HUD, flood lamps over the consoles.
appr = next(o for o in root.children if o.name.startswith("approach"))
appr.location = (-0.15, 0.66, -0.09)
for o in root.children:
    if o.name.startswith("lamp") and abs(o.location.x) > 0.3:
        o.location.x = math.copysign(0.30, o.location.x)

bpy.data.orphans_purge(do_recursive=True)
bpy.ops.wm.save_as_mainfile(filepath=out)
print("built", out, "objects", len(bpy.data.objects))
