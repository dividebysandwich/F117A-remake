# Builds the F-16C cockpit master (assets-src/blend/cockpit_f16.blend) from the F-117A's modern
# cockpit and the F-16 panel layout:
#   blender -b assets-src/blend/cockpit_modern.blend --python-exit-code 1 \
#       --python tools/models/build_cockpit_f16.py -- assets-src/blend/cockpit_f16_layout.json \
#       assets-src/blend/cockpit_f16.blend
# then tools/models/export.sh cockpit_f16.
#
# Kept from the modern cockpit (with their materials, labels and control properties): the seat,
# the ejection and gear handles, five side panel buttons (the target buttons), the flood lamps
# and the HUD glass. Everything else is built here, low-poly: the tub and
# side consoles, the glare shield, the up-front controls' pedestal with its round override
# buttons and keypad, the data entry display, the round instruments in their bezels, the main
# panel with the faces and displays of `PanelLayout::f16` (src/cockpit3d/layout.rs; the layout
# JSON is written by `UPDATE_F16_LAYOUT=1 cargo test f16_layout_file`), the side stick and the
# throttle, the centre pedestal between the legs with the knee wall's air vents, and the side
# consoles' switch panels and yellow handles (dressing, after the F-16's).
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
# The consoles end here; sloped panels rise from their ends to the main panel's lower edge
# (layout x <= -600 left, centred on -700; x >= 1100 right, centred on 1200; y down the slope).
CONSOLE_END = 0.42
# They meet the wings' lower edge (WING_BOTTOM), under the MFDs' bottom bezel buttons.
SLOPE_TOP = (0.6175, -0.4175)  # (y, z)
SLOPE_X = (0.17, 0.37)
# Their outer edges come in this much (mm) at the top.
SLOPE_BEVEL = 40.0
SILL = -0.45
WALL_X = 0.37
# The panel's centre column (|x| <= HINGE) and its wings out to WING either side (flat, in the
# panel's plane).
HINGE = 85.0
WING = 335.0
WING_ANGLE = 0.0
# The up-front controls (layout y < ICP_BOTTOM) stand out of the panel on a pedestal; the
# instruments under them are flush.
ICP_BOTTOM = -46.0
ICP_DEPTH = 0.055
CENTRE_DEPTH = 0.0
# The wings' upper outer corners are bevelled (layout mm along the top and down the side).
BEVEL = 55.0
# The MFDs sit back in wells: their bezels this far behind the panel surface (m).
MFD_WELL = 0.010
# Advisory light panel on the right console ahead of the stick (layout x >= CONSOLE_U, its
# middle at x 700, y 0), tilted towards the pilot.
CONSOLE_U = 600.0
ADVISORY_CENTRE = Vector((0.28, 0.33, -0.585))
ADVISORY_TILT = math.radians(15.0)
TOP = -160.0
WING_BOTTOM = 125.0
CENTRE_BOTTOM = 166.0
# HUD combiner glass: half width, bottom and top (m), at HUD_Y in front of the eye.
HUD_Y = 0.70
HUD_GLASS = (0.1675, -0.12, 0.095)
# Segments of the round parts (bezels, buttons, knobs): low-poly.
ROUND = 16
KNOB = 8

RIGHT = Vector((1.0, 0.0, 0.0))
UP = Vector((0.0, math.sin(PANEL_TILT), math.cos(PANEL_TILT)))
NORMAL = Vector((0.0, -math.cos(PANEL_TILT), math.sin(PANEL_TILT)))


def section(u):
    """-4 / 4 left / right slope, 3 right console, -1 / 1 left / right wing, 0 centre column."""
    if u >= 1100.0:
        return 4
    if u <= -CONSOLE_U:
        return -4
    if u >= CONSOLE_U:
        return 3
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
    if abs(sec) == 4:
        d = Vector((0.0, SLOPE_TOP[0] - CONSOLE_END, SLOPE_TOP[1] - CONSOLE_TOP)).normalized()
        return RIGHT, Vector((0.0, -d.z, d.y)), d
    # Console: facing up, tilted towards the pilot; up the layout = forward.
    up = Vector((0.0, math.cos(ADVISORY_TILT), math.sin(ADVISORY_TILT)))
    normal = Vector((0.0, -math.sin(ADVISORY_TILT), math.cos(ADVISORY_TILT)))
    return RIGHT, normal, up


def point(u, v, offset=0.0, plane=False):
    """A panel point: layout millimetres (u right, v down), raised `offset` m off its surface (the
    panel plane with `plane`, not the pedestal's raised face)."""
    sec = section(u)
    d, n, up = basis(sec)
    s = math.copysign(1, u)
    if sec == 0:
        p = PANEL_CENTRE + RIGHT * (u / 1000) - UP * (v / 1000)
        if not plane:
            p += NORMAL * (ICP_DEPTH if v < ICP_BOTTOM else CENTRE_DEPTH)
    elif abs(sec) == 1:
        hinge = PANEL_CENTRE + RIGHT * (s * HINGE / 1000) - UP * (v / 1000)
        p = hinge + d * ((u - s * HINGE) / 1000)
    elif abs(sec) == 4:
        mid = Vector((s * sum(SLOPE_X) / 2, (CONSOLE_END + SLOPE_TOP[0]) / 2, (CONSOLE_TOP + SLOPE_TOP[1]) / 2))
        p = mid + d * ((u - s * (700.0 if s < 0 else 1200.0)) / 1000) - up * (v / 1000)
    else:
        p = ADVISORY_CENTRE + n * 0.004 + d * ((u - 700.0) / 1000) - up * (v / 1000)
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
    keep = ("control_Action_Eject_", "control_Action_Gear_.001", "button_NEAR", "button_NEXT", "button_NONE", "button_WPN", "button_DTL", "seat_", "cushion_", "lamp", "hud_glass", "metal_98", "metal_99")
    return n.startswith(keep)


def drop(o):
    for c in list(o.children):
        drop(c)
    bpy.data.objects.remove(o, do_unlink=True)


for o in list(root.children):
    if not kept(o):
        drop(o)
old = list(root.children)
# The seat back up by as much as the F-117A's was lowered after the F-16's was set.
F117_SEAT_DROP = 0.22
for o in old:
    if o.name.startswith(("seat_", "cushion_", "metal_98", "metal_99", "control_Action_Eject_")):
        o.location.z += F117_SEAT_DROP
bpy.context.view_layer.update()

# --- materials
mats = bpy.data.materials


def material(name, like):
    m = mats.get(name)
    if m is None:
        m = mats[like].copy()
        m.name = name
    return m


for n, like in (("hsi", "adi"), ("screen_rwr", "adi"), ("ded", "adi"), ("trackcam_left", "trackcam")):
    material(n, like)
# Guard covers and red buttons (not a shared material: the game takes its colour from the model).
red = material("red", "yellow")
red.diffuse_color = (0.55, 0.04, 0.03, 1.0)
for nd in red.node_tree.nodes if red.use_nodes else ():
    if nd.type == "BSDF_PRINCIPLED":
        nd.inputs["Base Color"].default_value = red.diffuse_color


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


def cylinder_mesh(name, r, h, mat, axis="-Y", centre=(0.0, 0.0, 0.0), n=KNOB, r2=None):
    """A closed cylinder (or truncated cone with `r2` at its far end) of radius r and height h
    from `centre` along `axis` (local)."""
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    rot = {"Z": Matrix.Identity(4), "-Y": Matrix.Rotation(math.radians(90.0), 4, "X"), "Y": Matrix.Rotation(math.radians(-90.0), 4, "X"), "X": Matrix.Rotation(math.radians(90.0), 4, "Y"), "-X": Matrix.Rotation(math.radians(-90.0), 4, "Y")}[axis]
    m = Matrix.Translation(centre) @ rot @ Matrix.Translation((0.0, 0.0, h / 2))
    bmesh.ops.create_cone(bm, cap_ends=True, cap_tris=False, segments=n, radius1=r, radius2=r if r2 is None else r2, depth=h, matrix=m)
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


def panel_box(name, mat, r, raise_m, margin=0.0, base=0.0):
    """A plate from `base` to `base + raise_m` off the panel over the rectangle r (mm, within one
    section) grown by `margin` mm."""
    x, y, w, h = r
    c = rect_point(r, base + raise_m / 2)
    return box(name, mat, c, ((w + 2 * margin) / 1000, abs(raise_m), (h + 2 * margin) / 1000), rotation(x + w / 2))


def bezel(r, offset, height=0.008, lip=0.003):
    """A round black bezel standing `height` out of the panel at `offset` round the rectangle r's
    inscribed circle."""
    x, y, w, h = r
    me = cylinder_mesh("black", min(w, h) / 2000 + lip, height, "black", n=ROUND)
    mesh_object("black", me, rect_point(r, offset), rotation(x + w / 2))


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


def control(key, r, offset, effect, kind, round=False):
    """A push button: origin on the surface, its face 6 mm out; a dark cap body under the face
    and a highlight frame. Round ones are cylinders (the up-front override buttons)."""
    x, y, w, h = r
    hw, hh = w / 2000, h / 2000
    height, travel = 0.006, 0.004
    name = "control_" + "".join(ch if ch.isalnum() else "_" for ch in effect)
    c = empty(name, rect_point(r, offset), rotation(x + w / 2), props={"control": effect, "kind": kind, "half": [hw, hh], "height": height, "travel": travel})
    moving = empty("moving", (0, 0, 0), parent=c, props={"moving": True})
    if round:
        mesh_object("atlas", disc_mesh("atlas", min(hw, hh), "atlas", -height), (0, 0, 0), parent=moving, props={"atlas_face": key, "atlas_uv": [0.0, 0.0, 1.0, 1.0], "no_shadow": True})
        mesh_object("dark", cylinder_mesh("dark", min(hw, hh), height - 0.0004, "dark", n=ROUND), (0, 0, 0), parent=moving)
    else:
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


def plate(name, mat, pieces, depth):
    """A panel plate `depth` m thick behind the panel plane, from convex outlines (layout mm, one
    section) that together make its shape."""
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    n = basis(section(pieces[0][0][0]))[1]
    for loop in pieces:
        front = [bm.verts.new(point(u, v, 0.0, True)) for u, v in loop]
        back = [bm.verts.new(point(u, v, 0.0, True) - n * depth) for u, v in loop]
        bm.faces.new(front)
        bm.faces.new(back[::-1])
        for i in range(len(loop)):
            j = (i + 1) % len(loop)
            bm.faces.new((front[i], back[i], back[j], front[j]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.to_mesh(me)
    bm.free()
    me.materials.append(mats[mat])
    return mesh_object(name, me, (0, 0, 0))


def plane_box(name, mat, r, d0, d1):
    """A box over the rectangle r (layout mm) from d0 to d1 m off the panel plane."""
    x, y, w, h = r
    c = point(x + w / 2, y + h / 2, (d0 + d1) / 2, True)
    return box(name, mat, c, (w / 1000, abs(d1 - d0), h / 1000), rotation(x + w / 2))


def knob(u, v, r=0.006, h=0.012, mat="metal", offset=0.0):
    """A round knob standing out of the panel at layout (u, v)."""
    mesh_object(mat, cylinder_mesh(mat, r, h, mat, n=KNOB), point(u, v, offset), rotation(u))


def toggle(u, v, offset=0.0, up=True):
    """A toggle switch: a round base and a lever leaning up or down the panel."""
    mesh_object("dark", cylinder_mesh("dark", 0.005, 0.004, "dark", n=KNOB), point(u, v, offset), rotation(u))
    lean = Matrix.Rotation(math.radians(-35.0 if up else 35.0), 3, "X")
    me = box_mesh("metal", (0.004, 0.016, 0.004), "metal", (0.0, -0.011, 0.0), lean.to_quaternion())
    mesh_object("metal", me, point(u, v, offset + 0.003), rotation(u))


def wheel(u, v, offset=0.0):
    """A thumb wheel (the up-front controls' brightness and reticle wheels): a ridged drum
    half sunk in the panel, its axis across."""
    mesh_object("metal", cylinder_mesh("metal", 0.009, 0.006, "metal", axis="X", centre=(-0.003, 0.0, 0.0), n=12), point(u, v, offset + 0.001), rotation(u))


def glare_shield(depth=0.28, thickness=0.016, lip=0.01, slope=8.0):
    """The glare shield: one piece over the top of the panel, its front edge along the panel's
    top (the bevelled wing corners, the wings, the up-front controls' pedestal standing out in
    the middle), reaching forward `depth` m to a straight back edge and sloping down `slope`
    degrees; `lip` m overhangs the panel towards the pilot."""
    edge = [
        point(-WING, TOP + BEVEL), point(-(WING - BEVEL), TOP), point(-HINGE, TOP, 0.0, True),
        point(-HINGE, TOP, ICP_DEPTH, True), point(HINGE, TOP, ICP_DEPTH, True),
        point(HINGE, TOP, 0.0, True), point(WING - BEVEL, TOP), point(WING, TOP + BEVEL),
    ]
    a = math.radians(slope)
    fwd = Vector((0.0, math.cos(a), -math.sin(a)))
    up = Vector((0.0, math.sin(a), math.cos(a)))
    front = [e - fwd * lip + up * thickness for e in edge]
    y_back = max(f.y for f in front) + depth
    back = [f + fwd * ((y_back - f.y) / fwd.y) for f in front]
    me = bpy.data.meshes.new("black")
    bm = bmesh.new()
    top_f = [bm.verts.new(v) for v in front]
    top_b = [bm.verts.new(v) for v in back]
    bot_f = [bm.verts.new(v - up * thickness) for v in front]
    bot_b = [bm.verts.new(v - up * thickness) for v in back]
    n = len(front)
    for i in range(n - 1):
        j = i + 1
        bm.faces.new((top_f[i], top_f[j], top_b[j], top_b[i]))
        bm.faces.new((bot_f[j], bot_f[i], bot_b[i], bot_b[j]))
        bm.faces.new((bot_f[i], bot_f[j], top_f[j], top_f[i]))
        bm.faces.new((top_b[i], top_b[j], bot_b[j], bot_b[i]))
    for i in (0, n - 1):
        bm.faces.new((top_f[i], top_b[i], bot_b[i], bot_f[i]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    uv = bm.loops.layers.uv.new()
    for f in bm.faces:
        for l in f.loops:
            l[uv].uv = (0.0, 0.0)
    bm.to_mesh(me)
    bm.free()
    me.materials.append(mats["black"])
    mesh_object("black", me, (0.0, 0.0, 0.0))


# --- tub, consoles, glare shield
box("panel_grey", "panel_grey", (0.0, 0.05, -1.0), (2 * WALL_X, 1.4, 0.02))  # floor
for s in (-1, 1):
    box("panel_grey", "panel_grey", (s * WALL_X, 0.05, (SILL - 1.0) / 2), (0.02, 1.4, 1.0 + SILL))  # walls
    box("frame", "frame", (s * (WALL_X - 0.01), 0.05, SILL), (0.04, 1.4, 0.02))  # canopy sills
    cx = s * sum(CONSOLE_X) / 2
    y0, y1 = -0.31, CONSOLE_END
    box("panel_grey", "panel_grey", (cx, (y0 + y1) / 2, (CONSOLE_TOP - 1.0) / 2), (CONSOLE_X[1] - CONSOLE_X[0], y1 - y0, 1.0 + CONSOLE_TOP))
    box("dark", "dark", (cx, (y0 + y1) / 2, CONSOLE_TOP + 0.002), (CONSOLE_X[1] - CONSOLE_X[0] - 0.01, y1 - y0 - 0.02, 0.004))
    # Sloped panel from the console's end to the main panel (the side buttons and gear on it).
    u_mid = -700.0 if s < 0 else 1200.0
    length = (Vector(SLOPE_TOP) - Vector((CONSOLE_END, CONSOLE_TOP))).length * 1000 + 30.0
    # Its outer edge bevelled in towards the top, where the fuselage narrows.
    half = (SLOPE_X[1] - SLOPE_X[0]) * 500
    inner, outer = u_mid - s * half, u_mid + s * half
    top, bottom = -length / 2, length / 2
    shape = [(inner, top), (outer - s * SLOPE_BEVEL, top), (outer, bottom), (inner, bottom)]
    if s > 0:
        shape.reverse()
    plate("dark", "dark", [shape], 0.03)
box("panel_grey", "panel_grey", (0.0, -0.68, -0.55), (2 * WALL_X, 0.03, 0.9))  # rear bulkhead
# Panel: the wings (bevelled upper outer corners, wells for the MFDs), the centre column with
# the up-front controls' pedestal standing out of it, the knee well under it.
scr = layout["screens"]
wells = {}
for key in ("left", "right"):
    x, y, w, h = scr[key]
    wells[key] = (x - 19.0, y - 25.0, w + 38.0, h + 50.0)
for s_, key in ((-1, "left"), (1, "right")):
    x, y, w, h = wells[key]
    # In the right wing's terms (the left one mirrored): above, below, inside and outside the well.
    x0, x1 = (x, x + w) if s_ > 0 else (-(x + w), -x)
    y0, y1 = y, y + h
    pieces = [
        [(HINGE, TOP), (WING - BEVEL, TOP), (WING, TOP + BEVEL), (WING, y0), (HINGE, y0)],
        [(HINGE, y1), (WING, y1), (WING, WING_BOTTOM), (HINGE, WING_BOTTOM)],
        [(HINGE, y0), (x0, y0), (x0, y1), (HINGE, y1)],
        [(x1, y0), (WING, y0), (WING, y1), (x1, y1)],
    ]
    plate("panel_grey", "panel_grey", [[(s_ * u, v) for u, v in piece] for piece in pieces], 0.03)
plane_box("panel_grey", "panel_grey", (-HINGE, ICP_BOTTOM, 2 * HINGE, CENTRE_BOTTOM - ICP_BOTTOM), -0.03, CENTRE_DEPTH)
plane_box("dark", "dark", (-HINGE, TOP, 2 * HINGE, ICP_BOTTOM - TOP), -0.03, ICP_DEPTH)
glare_shield()
low = point(0, WING_BOTTOM, 0.0, True)
# Knee well wall under the panel, with an opening in the middle for the pilot's legs.
LEGS = (0.20, -0.50)  # half width, top (m)
for s_ in (-1, 1):
    box("panel_grey", "panel_grey", (s_ * (LEGS[0] + WALL_X) / 2, low.y + 0.08, (low.z - 1.0) / 2), (WALL_X - LEGS[0], 0.02, 1.0 + low.z))
box("panel_grey", "panel_grey", (0.0, low.y + 0.08, (low.z + LEGS[1]) / 2), (2 * LEGS[0], 0.02, low.z - LEGS[1]))
# The footwell behind the opening: side walls, roof, front wall and floor.
WELL_DEPTH = 0.35
y0 = low.y + 0.08
yc = y0 + WELL_DEPTH / 2
for s_ in (-1, 1):
    box("dark", "dark", (s_ * (LEGS[0] + 0.01), yc, (LEGS[1] - 1.0) / 2), (0.02, WELL_DEPTH, LEGS[1] + 1.0))
box("dark", "dark", (0.0, yc, LEGS[1] + 0.01), (2 * LEGS[0] + 0.04, WELL_DEPTH, 0.02))
box("dark", "dark", (0.0, y0 + WELL_DEPTH, (LEGS[1] - 1.0) / 2), (2 * LEGS[0] + 0.04, 0.02, LEGS[1] + 1.0))
box("dark", "dark", (0.0, yc, -1.0), (2 * LEGS[0] + 0.04, WELL_DEPTH, 0.02))
# Advisory light panel: a raised box on the right console.
panel_box("black", "black", (628.0, -48.0, 136.0, 65.0), 0.012, 0.0, -0.016)
# HUD frame: posts either side of the combiner glass, a bar on top of them (the glass fills the
# frame's inside: HUD_GLASS; AircraftSpec::hud_scale fits the symbology into it).
post = 0.015
for s_ in (-1, 1):
    x = s_ * (HUD_GLASS[0] + post / 2)
    box("frame", "frame", (x, HUD_Y, (HUD_GLASS[1] - 0.02 + HUD_GLASS[2]) / 2), (post, 0.025, HUD_GLASS[2] - HUD_GLASS[1] + 0.02))
box("frame", "frame", (0.0, HUD_Y, HUD_GLASS[2] + post / 2), (2 * (HUD_GLASS[0] + post), 0.025, post))
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
plates = [
    ("black", (-80.0, -159.0, 160.0, 111.0)),  # up-front controls
    ("dark", (-320.0, -106.0, 48.0, 56.0)),  # aircraft systems
    ("dark", (-266.0, -154.0, 32.0, 78.0)),  # threat lights
    ("dark", (-238.0, -154.0, 54.0, 46.0)),  # master caution, stall
    ("dark", (-750.0, -108.0, 48.0, 40.0)),  # gear lights (left slope)
    ("black", (90.0, -157.0, 130.0, 60.0)),  # data entry display
    ("dark", (1146.0, -74.0, 80.0, 75.0)),  # countermeasures (right slope)
    ("black", (1146.0, 24.0, 110.0, 70.0)),  # switches under them
    ("black", (-326.0, 14.0, 52.0, 110.0)),  # switches low on the left wing
]
for mat, r in plates:
    panel_box(mat, mat, r, PLATE)
# Up-front controls' thumb wheels (symbology and brightness left, reticle and contrast right),
# the drift cut-out switch, the data control switch and the two knobs right of the keypad.
for u in (-77.0, 77.0):
    wheel(u, -118.0, PLATE)
    wheel(u, -84.0, PLATE)
toggle(52.0, -124.0, PLATE)
knob(52.0, -100.0, 0.006, 0.010, offset=PLATE)
knob(52.0, -66.0, 0.008, 0.014, "dark", offset=PLATE)
# A few switches on the right slope's lower part and low on the left of the panel.
for v in (40.0, 60.0, 80.0):
    toggle(1160.0, v, PLATE, up=(v != 60.0))
    toggle(1200.0, v, PLATE, up=(v == 60.0))
knob(1240.0, 60.0, 0.008, 0.012, offset=PLATE)
for v in (30.0, 56.0, 82.0):
    toggle(-312.0, v, PLATE, up=(v != 56.0))
    toggle(-288.0, v, PLATE, up=(v == 56.0))
knob(-300.0, 108.0, 0.007, 0.012, offset=PLATE)
# The master arm switch's red guard over the top left one, hinged at its top.
mesh_object("red", box_mesh("red", (0.014, 0.012, 0.022), "red", (0.0, -0.006, -0.002)), point(-312.0, 30.0, PLATE), rotation(-312.0))
# MFDs in their wells, the bezel's face MFD_WELL behind the panel surface, with fixed side
# bezel buttons (the game uses the top and bottom rows). The bezel plate overlaps the well's
# walls, so nothing shows through round it.
for key in ("left", "right"):
    x, y, w, h = scr[key]
    wx, wy, ww, wh = wells[key]
    panel_box("dark", "dark", (wx - 2.0, wy - 2.0, ww + 4.0, wh + 4.0), 0.008, 0.0, -MFD_WELL - 0.008)
    for k in range(5):
        for bx in (x - 9.0, x + w + 9.0):
            by = y + h * (k + 0.5) / 5
            panel_box("dark", "dark", (bx - 5.0, by - 4.0, 10.0, 8.0), 0.005, 0.0, -MFD_WELL)
screen("screen_left", scr["left"], -MFD_WELL + 0.0035, "trackcam_left")
screen("screen_right", scr["right"], -MFD_WELL + 0.0035, "trackcam")
screen("adi", scr["adi"], 0.0, round=True)
screen("hsi", scr["hsi"], 0.0, round=True)
screen("screen_rwr", scr["rwr"], 0.0, round=True)
screen("ded", scr["ded"], PLATE + 0.0005)
for f in layout["faces"]:
    if f["control"] is None:
        face(f["key"], f["rect"], PLATE + 0.0005, f["round"])
    elif f["kind"] == "osb":
        control(f["key"], f["rect"], -MFD_WELL, f["control"], "osb")
    else:
        control(f["key"], f["rect"], PLATE, f["control"], f["kind"], f["round"])


# --- side console panels: black plates on the consoles' tops round the throttle and the stick,
# with knobs, toggles, red guards and buttons (dressing; none of them is a control)
CONSOLE_FACE = CONSOLE_TOP + 0.004


def fitting(c, p):
    """A console fitting at p (on a panel's top): k / K small / large knob, t / T toggle leaning
    forward / back, g red guard, r red button."""
    if c in "kK":
        r, h = (0.006, 0.010) if c == "k" else (0.009, 0.013)
        mesh_object("dark", cylinder_mesh("dark", r + 0.003, 0.002, "dark", axis="Z", n=KNOB), p)
        mesh_object("metal", cylinder_mesh("metal", r, h, "metal", axis="Z", n=KNOB), p + Vector((0.0, 0.0, 0.002)))
    elif c in "tT":
        mesh_object("dark", cylinder_mesh("dark", 0.005, 0.004, "dark", axis="Z", n=KNOB), p)
        lean = Euler((math.radians(-30.0 if c == "t" else 30.0), 0.0, 0.0)).to_quaternion()
        mesh_object("metal", box_mesh("metal", (0.004, 0.004, 0.016), "metal", (0.0, 0.0, 0.008), lean), p + Vector((0.0, 0.0, 0.003)))
    elif c == "g":
        mesh_object("red", box_mesh("red", (0.013, 0.022, 0.012), "red", (0.0, 0.0, 0.006)), p)
    elif c == "r":
        mesh_object("red", cylinder_mesh("red", 0.005, 0.006, "red", axis="Z", n=KNOB), p)


def console_panel(x0, x1, y0, y1, rows, h=0.006):
    """A plate over x0..x1, y0..y1 (m) on a console, its fittings in rows from the front."""
    box("black", "black", ((x0 + x1) / 2, (y0 + y1) / 2, CONSOLE_FACE + h / 2), (x1 - x0, y1 - y0, h))
    for i, row in enumerate(rows):
        y = y1 - (y1 - y0) * (i + 0.5) / len(rows)
        for j, c in enumerate(row):
            fitting(c, Vector((x0 + (x1 - x0) * (j + 0.5) / len(row), y, CONSOLE_FACE + h)))


for s in (-1, 1):
    def side(x0, x1, y0, y1, rows):
        """A panel given for the right console (x outwards), mirrored on the left."""
        console_panel(*sorted((s * x0, s * x1)), y0, y1, [r if s > 0 else r[::-1] for r in rows])
    # Behind the throttle and the arm rest, the width of the console.
    side(0.208, 0.352, -0.300, -0.212, ["kKk", "tgt"])
    side(0.208, 0.352, -0.204, -0.110 if s > 0 else -0.080, ["tttt", "kTk"] if s > 0 else ["tttt", "rgrg", "kk"])
for rows, rect in (
    (["kk", "TT", "kr"], (-0.352, -0.208, -0.070, 0.062)),  # ahead of those, behind the throttle
    (["k", "t", "r", "k"], (-0.354, -0.312, 0.080, 0.360)),  # outboard of the throttle rails
    (["t", "k", "t"], (-0.250, -0.210, 0.190, 0.360)),  # inboard of them
    (["kk", "tt", "rr", "Kk"], (0.208, 0.260, -0.095, 0.150)),  # inboard of the arm rest
):
    console_panel(*rect, rows)
# Yellow handles on the consoles' inner faces by the pilot's knees (canopy jettison left,
# its emergency release right).
for s, y in ((-1, 0.30), (1, 0.12)):
    x = s * (CONSOLE_X[0] - 0.022)
    box("yellow", "yellow", (x, y, -0.66), (0.012, 0.012, 0.08))
    for dz in (-0.03, 0.03):
        box("yellow", "yellow", (s * (CONSOLE_X[0] - 0.011), y, -0.66 + dz), (0.022, 0.01, 0.01))

# --- centre pedestal between the legs, under the panel's centre column, and air vents in the
# knee wall either side of it
col = point(0.0, CENTRE_BOTTOM, -0.03, True)
PEDESTAL = 0.06  # half width
front = col.y - 0.004
wall = low.y + 0.08
box("panel_grey", "panel_grey", (0.0, (front + wall + WELL_DEPTH) / 2, (col.z - 1.0) / 2), (2 * PEDESTAL, wall + WELL_DEPTH - front, col.z + 1.0))
# A recessed panel down its face with two knobs and a toggle.
box("dark", "dark", (0.0, front - 0.002, col.z - 0.10), (0.09, 0.004, 0.12))
for z in (-0.06, -0.10):
    mesh_object("metal", cylinder_mesh("metal", 0.008, 0.012, "metal", n=KNOB), (0.0, front - 0.004, col.z + z))
mesh_object("dark", cylinder_mesh("dark", 0.005, 0.004, "dark", n=KNOB), (0.0, front - 0.004, col.z - 0.14))
mesh_object("metal", box_mesh("metal", (0.004, 0.016, 0.004), "metal", (0.0, -0.008, 0.0), Euler((math.radians(-30.0), 0, 0)).to_quaternion()), (0.0, front - 0.007, col.z - 0.14))
for s in (-1, 1):
    x, z = s * (PEDESTAL + LEGS[0]) / 2, (low.z + LEGS[1]) / 2
    box("black", "black", (x, wall - 0.012, z), (0.09, 0.006, 0.05))
    for k in range(3):
        box("dark", "dark", (x, wall - 0.016, z - 0.015 + 0.015 * k), (0.08, 0.004, 0.006))

# --- side stick (right console) and throttle (left console), after the F-16's
# Stick: a boot on the console, the pivot above it; a short grip leaning forward with the
# trigger guard in front, the pinky lever behind and the weapon release and trim hat on top.
STICK = Vector((0.28, 0.22, CONSOLE_TOP))
box("rubber", "rubber", STICK + Vector((0.0, 0.0, 0.013)), (0.09, 0.09, 0.026))
stick = empty("stick", STICK + Vector((0.0, 0.0, 0.025)), props={"stick": True})
mesh_object("metal", cylinder_mesh("metal", 0.013, 0.055, "metal", axis="Z"), (0, 0, 0), parent=stick)
lean = Euler((math.radians(-12.0), 0.0, 0.0))
# The grip: a narrower lower part, the wider head leaning forward over it with a dished top.
mesh_object("rubber", box_mesh("rubber", (0.036, 0.042, 0.065), "rubber", (0.0, 0.008, 0.085), lean.to_quaternion()), (0, 0, 0), parent=stick)
mesh_object("rubber", box_mesh("rubber", (0.046, 0.058, 0.055), "rubber", (0.0, 0.02, 0.14), lean.to_quaternion()), (0, 0, 0), parent=stick)
mesh_object("metal", box_mesh("metal", (0.012, 0.028, 0.024), "metal", (0.0, 0.04, 0.088)), (0, 0, 0), parent=stick)
mesh_object("dark", box_mesh("dark", (0.01, 0.012, 0.03), "dark", (0.0, -0.016, 0.065)), (0, 0, 0), parent=stick)
mesh_object("metal", box_mesh("metal", (0.013, 0.013, 0.008), "metal", (-0.011, 0.03, 0.17)), (0, 0, 0), parent=stick)
mesh_object("dark", cylinder_mesh("dark", 0.0055, 0.009, "dark", axis="Z", centre=(0.012, 0.026, 0.167)), (0, 0, 0), parent=stick)
mesh_object("dark", cylinder_mesh("dark", 0.004, 0.007, "dark", axis="Z", centre=(0.0, 0.006, 0.167)), (0, 0, 0), parent=stick)
# Arm rest behind the stick.
box("cushion", "cushion", (0.31, 0.03, CONSOLE_TOP + 0.015), (0.09, 0.26, 0.03))
# Throttle: two rails along the left console; the handle slides on them from idle (its origin)
# to full: a slider block, the post and the grip across the console sloping up inboard, with the
# cut-off release at its inboard end, the radar cursor on top and the antenna elevation knob in
# front.
for x in (-0.294, -0.266):
    box("metal", "metal", (x, 0.21, CONSOLE_TOP + 0.007), (0.005, 0.25, 0.006))
throttle = empty("throttle", (-0.28, 0.125, -0.50), props={"throttle": True})
empty("throttle_full", (-0.28, 0.30, -0.50), props={"anchor": "throttle_full"})
mesh_object("dark", box_mesh("dark", (0.05, 0.075, 0.03), "dark", (0.0, 0.0, -0.07)), (0, 0, 0), parent=throttle)
mesh_object("metal", box_mesh("metal", (0.024, 0.03, 0.065), "metal", (0.0, 0.0, -0.035)), (0, 0, 0), parent=throttle)
slope = Euler((math.radians(12.0), 0.0, 0.0))
grip = [
    mesh_object("rubber", box_mesh("rubber", (0.045, 0.12, 0.04), "rubber", (0.0, 0.01, 0.0), slope.to_quaternion()), (0, 0, 0), parent=throttle),
    mesh_object("metal", box_mesh("metal", (0.018, 0.022, 0.014), "metal", (0.0, 0.056, 0.028)), (0, 0, 0), parent=throttle),
    mesh_object("dark", cylinder_mesh("dark", 0.007, 0.012, "dark", axis="Z", centre=(-0.014, 0.012, 0.02)), (0, 0, 0), parent=throttle),
    mesh_object("metal", cylinder_mesh("metal", 0.006, 0.01, "metal", axis="-X", centre=(-0.0225, -0.03, 0.0)), (0, 0, 0), parent=throttle),
]
# The grip and its switches turned 90 degrees clockwise seen from above (its long side across
# the console); the slider block and the post stay along the rails.
for o in grip:
    o.data.transform(Matrix.Rotation(math.radians(-90.0), 4, "Z"))

# --- the kept parts, moved into the F-16's cockpit
objs = {o.name: o for o in root.children}
# Seat back, head rest and rails reclined 15 degrees more (the F-16's 30 degree seat).
hip = Vector((0.0, -0.30, -0.78))
recline = Matrix.Translation(hip) @ Matrix.Rotation(math.radians(15.0), 4, "X") @ Matrix.Translation(-hip)
for o in root.children:
    if o.name.startswith(("seat_95", "cushion_96", "cushion_97", "metal_98", "metal_99")):
        o.matrix_world = recline @ o.matrix_world
# Target buttons on the left slope, ahead of the throttle, faces up towards the pilot.
for i, n in enumerate(["button_NEAR", "button_NEXT", "button_NONE", "button_WPN", "button_DTL"]):
    o = objs[n]
    col, row = i // 3, i % 3
    # Two columns of three on the lower part of the slope (layout y grows down it).
    u = -700.0 - (-25.0 + 45.0 * col)
    o.location = point(u, 15.0 + 34.0 * row, 0.004)
    o.rotation_quaternion = rotation(u)
# Gear handle on the left slope, right of the gear lights, above the target buttons.
g = objs["control_Action_Gear_.001"]
g.location = point(-672.0, -80.0, 0.002)
g.rotation_quaternion = rotation(-672.0)
# HUD glass: a smaller combiner over the up-front controls (its vertices are absolute).
hud = next(o for o in root.children if o.name.startswith("hud_glass"))
lo = Vector((min(v.co.x for v in hud.data.vertices), 0.0, min(v.co.z for v in hud.data.vertices)))
hi = Vector((max(v.co.x for v in hud.data.vertices), 0.0, max(v.co.z for v in hud.data.vertices)))
for v in hud.data.vertices:
    tx, tz = (v.co.x - lo.x) / (hi.x - lo.x), (v.co.z - lo.z) / (hi.z - lo.z)
    v.co = Vector((-HUD_GLASS[0] + 2 * HUD_GLASS[0] * tx, HUD_Y, HUD_GLASS[1] + (HUD_GLASS[2] - HUD_GLASS[1]) * tz))
# Approach (AOA) indexer left of the HUD: a small live canvas in a dark housing; the
# refuelling / nose wheel steering indexer right of the HUD; flood lamps over the consoles.
mesh_object("approach", quad_mesh("approach", 0.02, 0.056, "approach"), (-0.15, 0.66, -0.09), props={"no_shadow": True})
box("dark", "dark", (-0.15, 0.668, -0.09), (0.028, 0.014, 0.066))
box("dark", "dark", (0.15, 0.665, -0.09), (0.028, 0.014, 0.066))
for z in (-0.073, -0.107):
    mesh_object("black", cylinder_mesh("black", 0.009, 0.003, "black", n=ROUND), (0.15, 0.658, z))
for o in root.children:
    if o.name.startswith("lamp") and abs(o.location.x) > 0.3:
        o.location.x = math.copysign(0.30, o.location.x)

bpy.data.orphans_purge(do_recursive=True)
bpy.ops.wm.save_as_mainfile(filepath=out)
print("built", out, "objects", len(bpy.data.objects))
