# Builds the UH-60A cockpit master (assets-src/blend/cockpit_uh60.blend) from the panel layout,
# after the UH-60M's cockpit, with parts of the F-117A's modern cockpit:
#   blender -b assets-src/blend/cockpit_modern.blend --python-exit-code 1 \
#       --python tools/models/build_cockpit_uh60.py -- \
#       assets-src/blend/cockpit_uh60_layout.json assets-src/blend/cockpit_uh60.blend
# then tools/models/export.sh cockpit_uh60. The layout JSON is written by
# `UPDATE_LAYOUT=1 cargo test layout_files` from `PanelLayout::uh60` (src/cockpit3d/layout.rs).
#
# Life size (the F-117A's cockpit is built larger than life: its kept parts are scaled down by
# F117_SCALE about the eye). Kept, with their materials, labels and control properties: the seat
# with the ejection handle, the stick (a centre cyclic on a shaft from the floor), the throttle
# (the collective, its handle on an arm from a pivot behind it left of the seat, swinging up:
# the `throttle_full` and `throttle_pivot` anchors), the gear handle, the five target buttons,
# the HUD glass, the approach indexer and the flood lamps. Built here, low-poly: the instrument
# panel across the cabin under a deep glareshield, the pilot's two MFDs in raised bezels, the
# round instruments in their bezels and cases; the centre console between the seats with the
# CDU (the data entry display and its caps), the caution and countermeasure panels and the
# target buttons; the floor, the pedals and the copilot's seat. The helicopter's own fuselage
# is drawn around the cockpit (src/aircraft.rs), no walls or canopy frame here.
#
# Blender frame: eye at the origin (the pilot in the right seat; the cabin's centre line is
# CABIN_X to the left), x right, y forward, z up, metres. A control's face points along its
# local -Y (towards the pilot), local Z is up the panel.
import json
import math
import sys

import bmesh
import bpy
from mathutils import Euler, Matrix, Quaternion, Vector

layout_path, out = sys.argv[sys.argv.index("--") + 1:][:2]
layout = json.load(open(layout_path))
root = bpy.data.objects["cockpit"]
mats = bpy.data.materials

F117_SCALE = 1.4706
CABIN_X = -0.52
FLOOR = -1.20
# The main panel: its plane leaning back PANEL_TILT, the layout's origin at PANEL_CENTRE; from
# the left wall to the right one (the fuselage hides what is outside), TOP to BOTTOM (mm).
PANEL_TILT = math.radians(15.0)
PANEL_CENTRE = Vector((0.0, 0.66, -0.34))
PANEL_U = (-1620.0, 560.0)
TOP, BOTTOM = -140.0, 205.0
# The glareshield over its top: its lip at HOOD_Y, its top RISE over the panel's top edge,
# forward to GLARE_FRONT. The HUD glass on it, scaled about the eye (the same angular size) to
# stand at HUD_Y.
HOOD_Y = 0.61
RISE = 0.055
GLARE_FRONT = 0.85
HUD_Y = 0.62
# Centre console (layout x >= 1500, centred on 2000): its face sloping down aft from under the
# panel, CONSOLE_V (mm) along it from CONSOLE_FRONT to CONSOLE_REAR; CONSOLE_W wide.
CONSOLE_U = 2000.0
CONSOLE_V = (-150.0, 265.0)
CONSOLE_FRONT = Vector((CABIN_X, 0.59, -0.555))
CONSOLE_REAR = Vector((CABIN_X, 0.19, -0.67))
CONSOLE_W = 0.26
# Seat: its cushion's top (upright, the eye over the hips); the copilot's seat mirrored.
SEAT_TOP = -0.78
# Cyclic: pivot on the floor between the knees, its grip raised on a shaft.
CYCLIC_PIVOT = Vector((0.0, 0.30, FLOOR + 0.01))
CYCLIC_RAISE = 0.48
# Collective: handle (lowered) and pivot left of the seat, swing (degrees).
COLLECTIVE_HANDLE = Vector((-0.27, 0.12, -0.62))
COLLECTIVE_PIVOT = Vector((-0.27, -0.25, -0.70))
COLLECTIVE_SWING = 28.0
# Where the hand holds the collective's grip, in the grip's frame (mm, negated).
GRIP_HAND = Vector((0.0, -50.0, 4.0))
# MFD bezels standing BEZEL out of the panel, their margins round the screen (mm).
BEZEL = 0.03
MFD_SIDE, MFD_TOP = 22.0, 25.0
PLATE = 0.004
HUD_BAR = 0.010
ROUND = 16
KNOB = 8

RIGHT = Vector((1.0, 0.0, 0.0))
UP = Vector((0.0, math.sin(PANEL_TILT), math.cos(PANEL_TILT)))
NORMAL = Vector((0.0, -math.cos(PANEL_TILT), math.sin(PANEL_TILT)))
SLOPE = (CONSOLE_REAR - CONSOLE_FRONT).normalized()  # down the console
C_NORMAL = Vector((0.0, SLOPE.z, -SLOPE.y))  # up out of it


def console(u):
    return u >= 1500.0


def basis(u):
    """(across, normal towards the pilot, up) of the panel or the console."""
    return (RIGHT, C_NORMAL, -SLOPE) if console(u) else (RIGHT, NORMAL, UP)


def point(u, v, offset=0.0):
    """A point of the panel or the console: layout millimetres (u right, v down), `offset` m
    off its surface."""
    if console(u):
        p = CONSOLE_FRONT + RIGHT * ((u - CONSOLE_U) / 1000) + SLOPE * ((v - CONSOLE_V[0]) / 1000)
        return p + C_NORMAL * offset
    return PANEL_CENTRE + RIGHT * (u / 1000) - UP * (v / 1000) + NORMAL * offset


def rotation(u):
    d, n, up = basis(u)
    return Matrix((d, -n, up)).transposed().to_quaternion()


def rect_point(r, offset=0.0):
    x, y, w, h = r
    return point(x + w / 2.0, y + h / 2.0, offset)


# --- keep, drop
KEEP = (
    "seat_", "cushion_", "metal_98", "metal_99", "control_Action_Eject_", "stick", "throttle",
    "hud_glass", "approach", "lamp", "control_Action_Gear_.001",
    "button_NEAR", "button_NEXT", "button_NONE", "button_WPN", "button_DTL",
)
SEAT = ("seat_", "cushion_", "metal_98", "metal_99", "control_Action_Eject_")


def drop(o):
    for c in list(o.children):
        drop(c)
    bpy.data.objects.remove(o, do_unlink=True)


for o in list(root.children):
    if not o.name.startswith(KEEP):
        drop(o)
old = set(root.children)

# The kept parts to life size about the eye: positions and meshes, the controls' sizes.
SIZE_PROPS = ("height", "travel", "grip")
scaled = set()


def scale(o, k):
    o.location = o.location * k
    if o.type == "MESH" and o.data.name not in scaled:
        o.data.transform(Matrix.Scale(k, 4))
        scaled.add(o.data.name)
    for p in SIZE_PROPS:
        if p in o.keys():
            o[p] = o[p] * k
    if "half" in o.keys():
        o["half"] = [h * k for h in o["half"]]
    for c in o.children:
        scale(c, k)


for o in old:
    scale(o, 1.0 / F117_SCALE)
objs = {o.name: o for o in root.children}


# --- building blocks (as the F-117A's)
def mesh_object(name, mesh, loc, rot=Euler((0, 0, 0)), parent=root, props=None):
    o = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(o)
    o.parent = parent
    o.rotation_mode = "QUATERNION"
    o.location = loc
    o.rotation_quaternion = rot if isinstance(rot, Quaternion) else rot.to_quaternion()
    for k, v in (props or {}).items():
        o[k] = v
    return o


def empty(name, loc, rot=Euler((0, 0, 0)), parent=root, props=None):
    o = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(o)
    o.parent = parent
    o.rotation_mode = "QUATERNION"
    o.location = loc
    o.rotation_quaternion = rot if isinstance(rot, Quaternion) else rot.to_quaternion()
    for k, v in (props or {}).items():
        o[k] = v
    return o


def box_mesh(name, size, mat, centre=(0.0, 0.0, 0.0), rot=None):
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    m = Matrix.Translation(centre) @ (rot.to_matrix().to_4x4() if rot is not None else Matrix.Identity(4)) @ Matrix.Diagonal((*size, 1.0))
    bmesh.ops.create_cube(bm, size=1.0, matrix=m)
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


def disc_mesh(name, r, mat, y=0.0, n=ROUND):
    """A ROUND-gon in the local XZ plane at local y (faces -Y); its UVs map the 0..1 square's
    inscribed circle onto it, so a square texture face shows round."""
    me = bpy.data.meshes.new(name)
    v, uvs = [], []
    for i in range(n):
        a = 2 * math.pi * i / n
        v.append((r * math.cos(a), y, r * math.sin(a)))
        uvs.append((0.5 + 0.5 * math.cos(a), 0.5 + 0.5 * math.sin(a)))
    me.from_pydata(v, [], [tuple(range(n))])
    uv = me.uv_layers.new()
    for i, d in enumerate(uv.data):
        d.uv = uvs[i]
    me.materials.append(mats[mat])
    return me


def cylinder_mesh(name, r, h, mat, axis="-Y", n=KNOB):
    """A closed cylinder of radius r and height h from the origin along `axis` (local)."""
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    rot = {"Z": Matrix.Identity(4), "-Y": Matrix.Rotation(math.radians(90.0), 4, "X")}[axis]
    m = rot @ Matrix.Translation((0.0, 0.0, h / 2))
    bmesh.ops.create_cone(bm, cap_ends=True, cap_tris=False, segments=n, radius1=r, radius2=r, depth=h, matrix=m)
    uv = bm.loops.layers.uv.new()
    for f in bm.faces:
        for l in f.loops:
            l[uv].uv = (0.0, 0.0)
    bm.to_mesh(me)
    bm.free()
    me.materials.append(mats[mat])
    return me


def box(name, mat, centre, size, rot=Euler((0, 0, 0)), parent=root):
    return mesh_object(name, box_mesh(name, size, mat), centre, rot, parent=parent)


def rod(name, a, b, radius, mat, parent=root, n=8):
    """A cylinder from a to b (parent frame)."""
    a, b = Vector(a), Vector(b)
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=n, radius1=radius, radius2=radius, depth=(b - a).length)
    bm.to_mesh(me)
    bm.free()
    rot = Vector((0, 0, 1)).rotation_difference((b - a).normalized()).to_matrix().to_4x4()
    me.transform(Matrix.Translation((a + b) / 2) @ rot)
    for p in me.polygons:
        p.use_smooth = True
    me.materials.append(mats[mat])
    return mesh_object(name, me, (0, 0, 0), parent=parent)


def prism(name, mat, yz, x0, x1):
    """A convex outline in the y-z plane (m) from x0 to x1."""
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    sides = [[bm.verts.new((x, y, z)) for y, z in yz] for x in (x0, x1)]
    bm.faces.new(sides[0])
    bm.faces.new(sides[1][::-1])
    n = len(yz)
    for i in range(n):
        j = (i + 1) % n
        bm.faces.new((sides[0][i], sides[1][i], sides[1][j], sides[0][j]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.to_mesh(me)
    bm.free()
    me.materials.append(mats[mat])
    return mesh_object(name, me, (0, 0, 0))


def panel_box(name, mat, r, raise_m, margin=0.0, base=0.0):
    """A plate from `base` to `base + raise_m` off the panel over the rectangle r (mm) grown by
    `margin` mm."""
    x, y, w, h = r
    c = rect_point(r, base + raise_m / 2)
    return box(name, mat, c, ((w + 2 * margin) / 1000, abs(raise_m), (h + 2 * margin) / 1000), rotation(x + w / 2))


def frame(mat, outer, inner, d0, d1):
    """A rectangular frame (mm, outer and inner rectangles) from d0 to d1 m off the panel."""
    ox, oy, ow, oh = outer
    ix, iy, iw, ih = inner
    for r in ((ox, oy, ow, iy - oy), (ox, iy + ih, ow, oy + oh - iy - ih), (ox, iy, ix - ox, ih), (ix + iw, iy, ox + ow - ix - iw, ih)):
        panel_box(mat, mat, r, d1 - d0, base=d0)


def knob(u, v, r=0.006, h=0.012, mat="metal", offset=0.0):
    mesh_object(mat, cylinder_mesh(mat, r, h, mat), point(u, v, offset), rotation(u))


def toggle(u, v, offset=0.0, up=True):
    mesh_object("dark", cylinder_mesh("dark", 0.005, 0.004, "dark"), point(u, v, offset), rotation(u))
    lean = Matrix.Rotation(math.radians(-35.0 if up else 35.0), 3, "X")
    me = box_mesh("metal", (0.004, 0.016, 0.004), "metal", (0.0, -0.011, 0.0), lean.to_quaternion())
    mesh_object("metal", me, point(u, v, offset + 0.003), rotation(u))


def bezel(r, offset, height=0.008, lip=0.003):
    """A round black bezel standing `height` out of the panel at `offset` round the rectangle r's
    inscribed circle, the instrument's octagonal case behind it."""
    x, y, w, h = r
    me = cylinder_mesh("black", min(w, h) / 2000 + lip, height, "black", n=ROUND)
    mesh_object("black", me, rect_point(r, offset), rotation(x + w / 2))
    flat = min(w, h) / 2000 + lip + 0.002
    me = cylinder_mesh("dark", flat / math.cos(math.pi / 8), offset + 0.003, "dark", n=8)
    me.transform(Matrix.Rotation(math.pi / 8, 4, "Y"))
    mesh_object("dark", me, rect_point(r, 0.0), rotation(x + w / 2))


def face(key, r, offset, round=False):
    """An atlas face over the rectangle r; round ones are discs in a bezel (the instruments)."""
    x, y, w, h = r
    if round:
        bezel(r, offset)
        me = disc_mesh("atlas", min(w, h) / 2000, "atlas")
        offset += 0.0085
    else:
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


def screen(mat, r, offset, behind=None, round=False):
    """A display over the rectangle r: a quad, or a disc in a bezel (the round instruments)."""
    x, y, w, h = r
    rot = rotation(x + w / 2)
    if round:
        bezel(r, offset, 0.010, 0.004)
        mesh_object(mat, disc_mesh(mat, min(w, h) / 2000, mat), rect_point(r, offset + 0.0105), rot, props={"no_shadow": True})
        return
    mesh_object(mat, quad_mesh(mat, w / 1000, h / 1000, mat), rect_point(r, offset), rot, props={"no_shadow": True})
    if behind:
        mesh_object(behind, quad_mesh(behind, w / 1000, h / 1000, behind), rect_point(r, offset - 0.003), rot, props={"no_shadow": True})


# --- the instrument panel, 3 cm thick, across the cabin
u0, u1 = PANEL_U
panel_box("panel_grey", "panel_grey", (u0, TOP, u1 - u0, BOTTOM - TOP), -0.03)
# A rim along its bottom edge.
panel_box("dark", "dark", (u0, BOTTOM - 6.0, u1 - u0, 6.0), 0.008)

# --- glareshield: a deep hood over the panel's top edge, its lip reaching back towards the
# pilot, forward to the windshield
t = point(0.0, TOP)
glare_top = t.z + RISE
yz = [(t.y + 0.01, t.z - 0.01), (HOOD_Y, glare_top - 0.02), (HOOD_Y, glare_top - 0.008),
      (HOOD_Y + 0.015, glare_top), (GLARE_FRONT, glare_top + 0.01), (GLARE_FRONT, t.z - 0.01)]
prism("black", "black", yz, u0 / 1000, u1 / 1000)

# --- MFDs: a housing standing BEZEL out of the panel round each screen, the screen recessed in
# it; ten labelled push buttons above and below, five dummy ones each side with ridges between
# them and rocker switches at the corners (as the F-16's).
scr = layout["screens"]
faces = {f["key"]: f["rect"] for f in layout["faces"]}
for key in ("left", "right"):
    x, y, w, h = scr[key]
    outer = (x - MFD_SIDE, y - MFD_TOP, w + 2 * MFD_SIDE, h + 2 * MFD_TOP)
    hole = (x - 3.0, y - 3.0, w + 6.0, h + 6.0)
    frame("dark", outer, hole, 0.0, BEZEL - 0.002)
    frame("black", (outer[0] + 1.0, outer[1] + 1.0, outer[2] - 2.0, outer[3] - 2.0), hole, BEZEL - 0.002, BEZEL)
    # The recess's back behind the targeting pod picture (3 mm behind the screen).
    frame("black", hole, (x - 0.5, y - 0.5, w + 1.0, h + 1.0), BEZEL - 0.016, BEZEL - 0.001)
    panel_box("black", "black", hole, 0.002, base=BEZEL - 0.016)
    for k in range(1, 5):
        for by in (y - 16.5, y + h + 6.5):
            panel_box("black", "black", (x + w * k / 5 - 1.0, by - 4.0, 2.0, 8.0), 0.003, base=BEZEL)
    for k in range(5):
        by = y + h * (k + 0.5) / 5
        for bx in (x - 10.0, x + w + 10.0):
            panel_box("dark", "dark", (bx - 5.0, by - 6.0, 10.0, 12.0), 0.005, base=BEZEL)
    for bx in (x - 10.0, x + w + 10.0):
        for by in (y - 16.0, y + h + 16.0):
            panel_box("dark", "dark", (bx - 4.5, by - 6.5, 9.0, 13.0), 0.003, base=BEZEL)
            panel_box("black", "black", (bx - 1.5, by - 5.0, 3.0, 10.0), 0.003, base=BEZEL + 0.003)
screen("screen_left", scr["left"], BEZEL - 0.008, "trackcam_left")
screen("screen_right", scr["right"], BEZEL - 0.008, "trackcam")

# --- plates under the instrument groups
plates = [
    ("dark", (-585.0, -135.0, 125.0, 195.0)),  # standby instruments between the seats
    ("dark", (-425.0, -55.0, 112.0, 112.0)),  # engine gauges
    ("dark", (-183.0, -130.0, 76.0, 37.0)),  # warning lights over the left MFD
    ("dark", (107.0, -130.0, 76.0, 37.0)),  # and over the right one
    ("dark", (-248.0, 95.0, 113.0, 102.0)),  # gear lights, FLAPS, BRAKE, AUTO, BAY
    ("dark", (62.0, 100.0, 164.0, 78.0)),  # fuel, engine readouts, EMV
    ("black", (1920.0, -142.0, 160.0, 104.0)),  # CDU
    ("dark", (1940.0, 25.0, 120.0, 128.0)),  # cautions and countermeasures
]
for mat, r in plates:
    panel_box(mat, mat, r, PLATE)
# The CDU's knobs under its display; switches on the countermeasure panel's side.
for u in (1964.0, 1994.0, 2024.0):
    knob(u, -46.0, 0.006, 0.010, offset=PLATE)
toggle(2048.0, -46.0, PLATE)
for v in (85.0, 105.0, 125.0):
    toggle(2050.0, v, PLATE, up=(v != 105.0))


# Setting knobs at instrument corners.
def corner(r, inset=3.0):
    x, y, w, h = r
    return x + w - inset, y + h - inset


for r in (faces["Item(Dial(Airspeed))"], faces["Item(Dial(Altimeter))"], scr["adi"], faces["Item(Dial(Clock))"]):
    knob(*corner(r), 0.0045, 0.009, "dark", offset=PLATE)

screen("adi", scr["adi"], PLATE, round=True)
screen("ded", scr["ded"], PLATE + 0.0005)
for f in layout["faces"]:
    if f["control"] is None:
        face(f["key"], f["rect"], PLATE + 0.0005, f["round"])
        if f["key"].startswith("Item(Dial(") and not f["round"]:
            # The tapes' black window frames.
            x, y, w, h = f["rect"]
            frame("black", (x - 3.0, y - 3.0, w + 6.0, h + 6.0), (x, y, w, h), 0.0, PLATE + 0.005)
    elif f["kind"] == "osb":
        control(f["key"], f["rect"], BEZEL, f["control"], "osb")
    else:
        control(f["key"], f["rect"], PLATE, f["control"], f["kind"])

# --- centre console between the seats: its face from under the panel down aft, filled to the
# floor
half = CONSOLE_W / 2
f0 = CONSOLE_FRONT
f1 = CONSOLE_FRONT + SLOPE * ((CONSOLE_V[1] - CONSOLE_V[0]) / 1000)
yz = [(f0.y + 0.02, f0.z), (f1.y, f1.z), (f1.y, FLOOR), (f0.y + 0.02, FLOOR)]
prism("panel_grey", "panel_grey", [(y, z - 0.002) for y, z in yz], CABIN_X - half, CABIN_X + half)
# Up to the panel's bottom edge in front of it.
b = point(CABIN_X * 1000, BOTTOM)
prism("panel_grey", "panel_grey", [(f0.y + 0.02, f0.z - 0.002), (b.y, b.z), (b.y, FLOOR), (f0.y + 0.02, FLOOR)], CABIN_X - half, CABIN_X + half)
# Target buttons behind the cautions, two columns, faces up the console.
for i, n in enumerate(["button_NEAR", "button_NEXT", "button_NONE", "button_WPN", "button_DTL"]):
    o = objs[n]
    u = CONSOLE_U - 30.0 + 60.0 * (i // 3)
    v = 180.0 + 32.0 * (i % 3)
    o.location = point(u, v, 0.004)
    o.rotation_mode = "QUATERNION"
    o.rotation_quaternion = rotation(u)
panel_box("dark", "dark", (CONSOLE_U - 70.0, 160.0, 140.0, 100.0), 0.004)
# The gear handle left of the gear lights.
g = objs["control_Action_Gear_.001"]
g.location = point(-280.0, 120.0, 0.002)
g.rotation_mode = "QUATERNION"
g.rotation_quaternion = rotation(-280.0)

# --- floor, pedals
box("dark", "dark", ((PANEL_U[0] / 1000 + 0.62) / 2, 0.15, FLOOR - 0.01), (0.62 - PANEL_U[0] / 1000, 1.7, 0.02))
for s in (-1, 1):
    for x0 in (0.0, 2 * CABIN_X):
        p = Vector((x0 + s * 0.13, 0.80, FLOOR + 0.12))
        box("dark", "dark", p, (0.07, 0.025, 0.20), Euler((math.radians(-25.0), 0.0, 0.0)))
        rod("metal", p + Vector((0, 0.03, -0.08)), p + Vector((0, 0.10, -0.12)), 0.008, "metal")

# --- seat: upright, its cushion at SEAT_TOP, on a pedestal from the floor; the copilot's
# beside it on the cabin's other side (without the handle)
seat = [o for o in old if o.name.startswith(SEAT)]
bpy.context.view_layer.update()
cushion = min((o for o in seat if o.name.startswith("cushion_")), key=lambda o: o.location.z)
cushion_top = max((cushion.matrix_world @ v.co).z for v in cushion.data.vertices)
for o in seat:
    o.location.z += SEAT_TOP - cushion_top
bpy.context.view_layer.update()
bottom = min((o.matrix_world @ v.co).z for o in seat if o.type == "MESH" for v in o.data.vertices)
box("dark", "dark", (0.0, -0.08, (bottom + FLOOR) / 2), (0.36, 0.40, bottom - FLOOR))
for o in seat:
    if o.type == "MESH":
        c = o.copy()
        bpy.context.scene.collection.objects.link(c)
        c.location.x += 2 * CABIN_X
box("dark", "dark", (2 * CABIN_X, -0.08, (bottom + FLOOR) / 2), (0.36, 0.40, bottom - FLOOR))

# --- centre cyclic
stick = objs["stick"]
stick.location = CYCLIC_PIVOT
for c in stick.children:
    c.location.z += CYCLIC_RAISE
stick["grip"] = float(stick["grip"]) + CYCLIC_RAISE
rod("stick_shaft", (0, 0, 0), (0, 0, CYCLIC_RAISE + 0.02), 0.012, "metal", stick)
# The boot at its foot (does not move with it).
rod("cyclic_boot", CYCLIC_PIVOT, CYCLIC_PIVOT + Vector((0, 0, 0.05)), 0.04, "rubber")

# --- collective: the arm from the pivot up to the grip, the UH-60's grip on it (after its
# drawings, mm): a knurled ring on the arm's end, the hand grip, the switch box ahead of it
# with its face up (the hat, the rocker, the release button, the dimmer, the servo toggle, the
# landing light hat). Built in its own frame (ring's front at the origin, y along the grip, z
# up), then turned onto the arm with the hand at the lever's origin.
throttle = objs["throttle"]
for c in list(throttle.children):
    drop(c)
throttle.location = COLLECTIVE_HANDLE
arm = COLLECTIVE_PIVOT - COLLECTIVE_HANDLE
GRIP = Matrix.Rotation(math.atan2(-arm.z, -arm.y), 4, "X") @ Matrix.Scale(0.001, 4) @ Matrix.Translation(GRIP_HAND)


def grip_mat(name, rgba):
    m = mats.get(name) or mats.new(name)
    m.diffuse_color = rgba
    m.use_nodes = True
    bsdf = m.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = rgba
    bsdf.inputs["Roughness"].default_value = 0.4
    return name


def grip(name, me, m=Matrix.Identity(4), smooth=False):
    """A grip part from its mesh in the grip frame (mm), moved by m first."""
    me.transform(GRIP @ m)
    for p in me.polygons:
        p.use_smooth = smooth
    return mesh_object(name, me, (0, 0, 0), parent=throttle)


def loft(name, mat, sections, n=16, rib=1.0):
    """Closed tube through elliptical sections (y, centre z, half width, half height); every
    other vertex at `rib` times the radius (knurling)."""
    me = bpy.data.meshes.new(name)
    v = []
    for y, z, hw, hh in sections:
        for i in range(n):
            a = 2 * math.pi * i / n
            k = rib if i % 2 else 1.0
            v.append((hw * k * math.cos(a), y, z + hh * k * math.sin(a)))
    f = [tuple(range(n))[::-1], tuple(range(n * (len(sections) - 1), n * len(sections)))]
    for s in range(len(sections) - 1):
        for i in range(n):
            j = (i + 1) % n
            f.append((s * n + i, s * n + j, (s + 1) * n + j, (s + 1) * n + i))
    me.from_pydata(v, [], f)
    outward(me)
    me.materials.append(mats[mat])
    return me


def outward(me):
    bm = bmesh.new()
    bm.from_mesh(me)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.to_mesh(me)
    bm.free()


def slab(name, mat, top, z0, z1, shrink=1.0):
    """A plate under the outline `top` [(x, y)] from z1 down to z0, its bottom outline shrunk
    by `shrink` about its centre."""
    cx = sum(p[0] for p in top) / len(top)
    cy = sum(p[1] for p in top) / len(top)
    bottom = [(cx + (x - cx) * shrink, cy + (y - cy) * shrink, z0) for x, y in top]
    me = bpy.data.meshes.new(name)
    n = len(top)
    v = [(x, y, z1) for x, y in top] + bottom
    f = [tuple(range(n)), tuple(range(n, 2 * n))[::-1]]
    f += [(i, n + i, n + (i + 1) % n, (i + 1) % n) for i in range(n)]
    me.from_pydata(v, [], f)
    outward(me)
    me.materials.append(mats[mat])
    return me


def at(x, y, z, rz=0.0):
    return Matrix.Translation((x, y, z)) @ Matrix.Rotation(math.radians(rz), 4, "Z")


def post(name, mat, x, y, z, r, h, n=KNOB):
    """A cylinder up from (x, y, z) mm."""
    grip(name, cylinder_mesh(name, r, h, mat, axis="Z", n=n), at(x, y, z))


red = grip_mat("grip_red", (0.75, 0.02, 0.01, 1.0))
blue = grip_mat("grip_blue", (0.05, 0.45, 0.9, 1.0))
# The arm: from the ring's back to the pivot.
back = GRIP @ Vector((0.0, -19.0, 0.0))
rod("throttle_arm", back, arm, 0.016, "metal", throttle)
# Knurled ring, the hand grip narrowing to a waist and dropping onto the switch box's hub.
grip("throttle_ring", loft("throttle_ring", "rubber", [(-19.0, 0.0, 21.5, 21.5), (0.0, 0.0, 21.5, 21.5)], n=32, rib=0.94))
grip("throttle_grip", loft("throttle_grip", "dark", [
    (0.0, 0.0, 19.0, 19.0), (30.0, -2.0, 17.0, 19.0), (65.0, -6.0, 13.5, 16.0),
    (100.0, -11.0, 15.0, 15.0), (128.0, -14.0, 15.0, 10.0),
]), smooth=True)
# The two buttons on top and on the outboard side near the ring.
for y in (8.0, 38.0):
    grip("throttle_button", cylinder_mesh("throttle_button", 3.5, 3.0, "metal", axis="Z"), at(0.0, y, 18.0 - y / 15))
    grip("throttle_button", cylinder_mesh("throttle_button", 3.5, 3.0, "metal", axis="Z"),
         at(-18.0 + y / 15, y, -2.0) @ Matrix.Rotation(math.radians(-90.0), 4, "Y"))
# The web under the grip down to the box's back, the finger lever on it.
me = bpy.data.meshes.new("throttle_web")
web = [(55.0, -14.0), (98.0, -22.0), (98.0, -56.0), (86.0, -56.0)]
me.from_pydata([(x, y, z) for x in (-7.0, 7.0) for y, z in web], [], [
    (0, 1, 2, 3), (7, 6, 5, 4), (0, 4, 5, 1), (1, 5, 6, 2), (2, 6, 7, 3), (3, 7, 4, 0)])
outward(me)
me.materials.append(mats["dark"])
grip("throttle_web", me)
grip("throttle_lever", box_mesh("throttle_lever", (7.0, 6.0, 22.0), "black"),
     Matrix.Translation((9.0, 72.0, -40.0)) @ Matrix.Rotation(math.radians(-25.0), 4, "Y"))
# The switch box: its top outline (x across, inboard +, y along), the black face on it.
face_top = [(-44.0, 125.0), (-44.0, 178.0), (-38.0, 182.0), (6.0, 182.0), (53.0, 151.0), (53.0, 102.0), (35.0, 97.0)]
FACE = -15.0
grip("throttle_box", slab("throttle_box", "dark", face_top, -60.0, FACE - 2.0, 0.9))
inset = [(x * 0.97 + 0.5, (y - 140.0) * 0.97 + 140.0) for x, y in face_top]
grip("throttle_face", slab("throttle_face", "black", inset, FACE - 2.0, FACE))
for x, y in [(-39.0, 130.0), (-39.0, 176.0), (5.0, 177.0), (46.0, 151.0), (47.0, 106.0), (20.0, 104.0)]:
    post("throttle_screw", "metal", x, y, FACE, 1.6, 0.8, n=6)
# The hat on its collar where the grip meets the box.
post("throttle_collar", "metal", 0.0, 136.0, FACE, 15.0, 8.0, n=ROUND)
post("throttle_hat", "black", 0.0, 136.0, FACE + 8.0, 9.0, 8.0, n=ROUND)
post("throttle_hat", "black", 0.0, 136.0, FACE + 16.0, 4.0, 3.0)
# The ribbed rocker in its housing (outboard, aft).
grip("throttle_rocker", box_mesh("throttle_rocker", (16.0, 18.0, 4.0), "dark", (-28.0, 140.0, FACE + 2.0)))
grip("throttle_rocker", loft("throttle_rocker", "black", [(-6.0, 0.0, 7.0, 7.0), (6.0, 0.0, 7.0, 7.0)], n=16, rib=0.88),
     Matrix.Translation((-28.0, 140.0, FACE + 3.0)))
# The release button: red, in its blue ring (outboard, ahead).
post("throttle_ring_lit", blue, -22.0, 165.0, FACE, 12.0, 1.5, n=ROUND)
post("throttle_release", red, -22.0, 165.0, FACE + 1.5, 8.5, 6.0, n=ROUND)
# The dimmer knob, star cut.
post("throttle_dimmer", "dark", 12.0, 167.0, FACE, 9.5, 3.0, n=ROUND)
grip("throttle_dimmer", loft("throttle_dimmer", "black", [(0.0, 0.0, 8.0, 8.0), (8.0, 0.0, 8.0, 8.0)], n=16, rib=0.75),
     Matrix.Translation((12.0, 167.0, FACE + 3.0)) @ Matrix.Rotation(math.radians(90.0), 4, "X"))
# The servo toggle in its slanted slot.
grip("throttle_slot", box_mesh("throttle_slot", (9.0, 26.0, 4.0), "dark", (30.0, 147.0, FACE + 2.0), Euler((0.0, 0.0, math.radians(-35.0)))))
grip("throttle_toggle", box_mesh("throttle_toggle", (3.5, 4.0, 12.0), "metal", (30.0, 147.0, FACE + 8.0), Euler((0.0, math.radians(15.0), 0.0))))
# The landing light hat (inboard, aft).
post("throttle_ldg", "dark", 39.0, 118.0, FACE, 9.5, 4.0, n=ROUND)
post("throttle_ldg", "black", 39.0, 118.0, FACE + 4.0, 6.5, 6.0, n=ROUND)
box("dark", "dark", COLLECTIVE_PIVOT + Vector((0, 0, -0.03)), (0.07, 0.07, 0.09))
box("dark", "dark", COLLECTIVE_PIVOT + Vector((0, 0, (FLOOR - COLLECTIVE_PIVOT.z) / 2 - 0.04)), (0.05, 0.05, COLLECTIVE_PIVOT.z - FLOOR - 0.08))
full = COLLECTIVE_PIVOT + Matrix.Rotation(math.radians(COLLECTIVE_SWING), 3, "X") @ -arm
objs["throttle_full"].location = full
empty("throttle_pivot", COLLECTIVE_PIVOT, props={"anchor": "throttle_pivot"})

# --- HUD: the glass on the glareshield in front of the pilot, a frame of bars along its outline
# (posts down to the glareshield, chamfered top corners, top) outside it by half a bar.
hud = next(o for o in old if o.name.startswith("hud_glass"))
bpy.context.view_layer.update()
glass = [hud.matrix_world @ v.co for v in hud.data.vertices]
k = HUD_Y / (sum(v.y for v in glass) / len(glass))
hud.location *= k
hud.data.transform(Matrix.Scale(k, 4))
bpy.context.view_layer.update()
glass = [hud.matrix_world @ v.co for v in hud.data.vertices]
hud_y = sum(v.y for v in glass) / len(glass)
half_x = max(abs(v.x) for v in glass)
top = max(v.z for v in glass)
shoulder = max(v.z for v in glass if abs(abs(v.x) - half_x) < 0.01)
top_x = min(abs(v.x) for v in glass if abs(v.z - top) < 0.002)
post_x = half_x + HUD_BAR / 2
outline = [
    Vector((-post_x, glare_top)), Vector((-post_x, shoulder)),
    Vector((-top_x, top + HUD_BAR / 2)), Vector((top_x, top + HUD_BAR / 2)),
    Vector((post_x, shoulder)), Vector((post_x, glare_top)),
]
for a, b in zip(outline, outline[1:]):
    d = b - a
    centre = (a + b) / 2
    rot = Euler((0.0, -math.atan2(d.y, d.x), 0.0))
    box("frame", "frame", (centre.x, hud_y, centre.y), (d.length + HUD_BAR, HUD_BAR, HUD_BAR), rot)
# The approach indexer just outside the left post, in a housing behind it.
ind = next(o for o in old if o.name.startswith("approach"))
ind.location.x = -(post_x + HUD_BAR / 2 + 0.014)
ind.location.y = hud_y
box("dark", "dark", (ind.location.x, hud_y + 0.007, ind.location.z), (0.028, 0.010, 0.068))
box("dark", "dark", (ind.location.x, hud_y + 0.007, (ind.location.z - 0.034 + glare_top) / 2), (0.010, 0.010, ind.location.z - 0.034 - glare_top))

# --- flood lamps: two behind the pilot's head, one under the glareshield
objs["lamp.002"].location = Vector((0.0, HOOD_Y + 0.04, t.z))

bpy.data.orphans_purge(do_recursive=True)
bpy.context.view_layer.update()
print(f"cyclic grip {stick['grip']:.3f} m; collective {COLLECTIVE_HANDLE} -> {full} "
      f"({(full - COLLECTIVE_HANDLE).length:.3f} m); glareshield top {glare_top:.3f}; seat bottom {bottom:.3f}")
bpy.ops.wm.save_as_mainfile(filepath=out)
print("built", out, "objects", len(bpy.data.objects))
