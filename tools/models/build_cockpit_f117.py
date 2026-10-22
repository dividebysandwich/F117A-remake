# Rebuilds the F-117A's modern cockpit master (assets-src/blend/cockpit_modern.blend) in place
# from the panel layout, after the F-16's (tools/models/build_cockpit_f16.py) and the real
# F-117A's front panel:
#   blender -b assets-src/blend/cockpit_modern.blend --python-exit-code 1 \
#       --python tools/models/build_cockpit_f117.py -- \
#       assets-src/blend/cockpit_modern_layout.json assets-src/blend/cockpit_modern.blend
# then tools/models/export.sh cockpit_modern. Running it again on its output rebuilds the same.
#
# Kept (with their materials, labels and control properties): the seat and the ejection
# handle, the stick and the throttle with its rails, the gear handle and five side panel
# buttons (the target buttons), the floor, the HUD glass, the approach indexer and the flood
# lamps (the HUD glass gets a new frame along its outline). Everything else is built here, low-poly: the panel
# with the faces and displays of `PanelLayout::modern` (src/cockpit3d/layout.rs; the layout JSON
# is written by `UPDATE_LAYOUT=1 cargo test layout_files`): its wings' upper edges falling
# steeply outwards under the canopy's long front frames, the up-front display standing out
# under the HUD, the large centre display and the side MFDs in raised bezels, the round
# instruments in their bezels and cases; the coaming over it, the consoles with their switch
# panels, the sloped panels from them up to the main panel, the knee wall and the centre
# pedestal between the legs; the walls up to the low canopy sills (level with the panel's outer
# top corners, as the real one's), the canopy bow behind the seat and outside the sills the
# fuselage's faceted shoulders (the exterior model is not drawn from this cockpit).
#
# The F-117A's cockpit is the classic one, larger than life (`AircraftSpec::vr_scale`): the
# new parts are built life size, the F-16's millimetres, and scaled by SCALE about the eye at the
# end. Blender frame: eye at the origin, x right, y forward, z up, metres. A control's face
# points along its local -Y (towards the pilot), local Z is up the panel.
import json
import math
import sys

import bmesh
import bpy
from mathutils import Euler, Matrix, Quaternion, Vector

layout_path, out = sys.argv[sys.argv.index("--") + 1:][:2]
layout = json.load(open(layout_path))
root = bpy.data.objects["cockpit"]

# The F-117A cockpit's size over life size (as tools/models/copy_f16_controls.py's).
SCALE = 1.4706
# Life size from here on (the kept parts' coordinates divided by SCALE).
PANEL_TILT = math.radians(20.0)  # top of the panel leaning away
PANEL_CENTRE = Vector((0.0, 0.50, -0.27))
FLOOR = -0.765
WALL_X = 0.36
# Canopy sills (top), from the rear of the cockpit to the coaming's back.
SILL = -0.24
SILL_REAR = -0.40
# Consoles (the stick and the throttle stand on them): x, y and their top (low enough for the
# VR hands on the stick and throttle to stay clear of the arm rests).
CONSOLE_X = (0.19, 0.354)
CONSOLE_Y = (-0.269, 0.241)
CONSOLE_TOP = -0.646
# The kept stick and throttle with their rails and boot, moved with the consoles' top; the
# stick's pivot stands this far over it (the cockpit's scale).
LEVERS = ("stick", "throttle", "throttle_full", "black_549", "metal_550", "metal_551", "rubber_543")
PIVOT_OVER_CONSOLE = 0.01
# The panel's centre column (|x| <= HINGE) and its wings out to WING either side (flat, in the
# panel's plane), down to WING_BOTTOM and CENTRE_BOTTOM (layout mm).
HINGE = 100.0
WING = 358.0
WING_BOTTOM = 170.0
CENTRE_BOTTOM = 175.0
# The panel's top: level at TOP for |x| <= TOP_FLAT, falling to CORNER at the wings' outer
# edges (the canopy's front frames run down to the sills there).
TOP = -137.0
TOP_FLAT = 80.0
CORNER = -40.0
# The up-front display (|x| <= TOP_FLAT, y < UFC_BOTTOM) stands out of the panel.
UFC_BOTTOM = -55.0
UFC_DEPTH = 0.035
# The consoles' sloped panels rise from their ends to the wings' lower edge (layout x <= -600
# left, centred on -700; x >= 1100 right, centred on 1200; y down the slope).
SLOPE_X = (0.19, 0.358)
SLOPE_BEVEL = 40.0
# The coaming reaches forward to here, level with the top of the up-front display at the back.
COAMING_BACK = 0.714
# The HUD frame's bars (square, the cockpit's scale), round the glass from the coaming up.
HUD_BAR = 0.012
# Segments of the round parts (bezels, buttons, knobs): low-poly.
ROUND = 16
KNOB = 8

RIGHT = Vector((1.0, 0.0, 0.0))
UP = Vector((0.0, math.sin(PANEL_TILT), math.cos(PANEL_TILT)))
NORMAL = Vector((0.0, -math.cos(PANEL_TILT), math.sin(PANEL_TILT)))


def top_v(u):
    """The panel's upper edge at layout x u."""
    return TOP + max(0.0, abs(u) - TOP_FLAT) * (CORNER - TOP) / (WING - TOP_FLAT)


def section(u):
    """-4 / 4 left / right slope, -1 / 1 left / right wing, 0 centre column."""
    if u >= 1100.0:
        return 4
    if u <= -600.0:
        return -4
    if abs(u) > HINGE:
        return int(math.copysign(1, u))
    return 0


def slope_top():
    """(y, z) of the wings' lower edge, where the sloped panels end."""
    p = PANEL_CENTRE - UP * (WING_BOTTOM / 1000)
    return p.y, p.z


def basis(sec):
    """(across, normal towards the pilot, up) of a section."""
    if abs(sec) == 4:
        sy, sz = slope_top()
        d = Vector((0.0, sy - CONSOLE_Y[1], sz - CONSOLE_TOP)).normalized()
        return RIGHT, Vector((0.0, -d.z, d.y)), d
    return RIGHT, NORMAL, UP


def point(u, v, offset=0.0, plane=False):
    """A panel point: layout millimetres (u right, v down), raised `offset` m off its surface (the
    panel plane with `plane`, not the up-front display's raised face)."""
    sec = section(u)
    d, n, up = basis(sec)
    if abs(sec) == 4:
        s = math.copysign(1, u)
        sy, sz = slope_top()
        mid = Vector((s * sum(SLOPE_X) / 2, (CONSOLE_Y[1] + sy) / 2, (CONSOLE_TOP + sz) / 2))
        p = mid + d * ((u - s * (700.0 if s < 0 else 1200.0)) / 1000) - up * (v / 1000)
    else:
        p = PANEL_CENTRE + RIGHT * (u / 1000) - UP * (v / 1000)
        if not plane and abs(u) <= TOP_FLAT and v < UFC_BOTTOM:
            p += NORMAL * UFC_DEPTH
    return p + n * offset


def rotation(u):
    d, n, up = basis(section(u))
    return Matrix((d, -n, up)).transposed().to_quaternion()


def rect_point(r, offset=0.0):
    x, y, w, h = r
    return point(x + w / 2.0, y + h / 2.0, offset)


# --- keep, drop
KEEP = (
    "seat_", "cushion_", "metal_98", "metal_99", "control_Action_Eject_", "stick", "throttle",
    "black_549", "metal_550", "metal_551", "rubber_543", "dark_83", "hud_glass", "approach", "lamp", "control_Action_Gear_.001",
    "button_NEAR", "button_NEXT", "button_NONE", "button_WPN", "button_DTL",
)


def kept(o):
    return o.name.startswith(KEEP)


def drop(o):
    for c in list(o.children):
        drop(c)
    bpy.data.objects.remove(o, do_unlink=True)


for o in list(root.children):
    if not kept(o):
        drop(o)
old = set(root.children)

# --- materials
mats = bpy.data.materials


def material(name, like):
    m = mats.get(name)
    if m is None:
        m = mats[like].copy()
        m.name = name
    return m


for n, like in (
    ("ded", "adi"), ("screen_third", "screen_right"), ("trackcam_left", "trackcam"),
    ("trackcam_third", "trackcam"),
):
    material(n, like)
# Guard covers and red buttons (not a shared material: the game takes its colour from the model).
red = material("red", "yellow")
red.diffuse_color = (0.55, 0.04, 0.03, 1.0)
for nd in red.node_tree.nodes if red.use_nodes else ():
    if nd.type == "BSDF_PRINCIPLED":
        nd.inputs["Base Color"].default_value = red.diffuse_color


# --- building blocks (as the F-16's)
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


def cylinder_mesh(name, r, h, mat, axis="-Y", centre=(0.0, 0.0, 0.0), n=KNOB):
    """A closed cylinder of radius r and height h from `centre` along `axis` (local)."""
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    rot = {"Z": Matrix.Identity(4), "-Y": Matrix.Rotation(math.radians(90.0), 4, "X")}[axis]
    m = Matrix.Translation(centre) @ rot @ Matrix.Translation((0.0, 0.0, h / 2))
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


def panel_box(name, mat, r, raise_m, margin=0.0, base=0.0):
    """A plate from `base` to `base + raise_m` off the panel over the rectangle r (mm, within one
    section) grown by `margin` mm."""
    x, y, w, h = r
    c = rect_point(r, base + raise_m / 2)
    return box(name, mat, c, ((w + 2 * margin) / 1000, abs(raise_m), (h + 2 * margin) / 1000), rotation(x + w / 2))


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


def frame(mat, outer, inner, d0, d1):
    """A rectangular frame (layout mm, outer and inner rectangles) from d0 to d1 m off the
    panel plane."""
    ox, oy, ow, oh = outer
    ix, iy, iw, ih = inner
    for r in ((ox, oy, ow, iy - oy), (ox, iy + ih, ow, oy + oh - iy - ih), (ox, iy, ix - ox, ih), (ix + iw, iy, ox + ow - ix - iw, ih)):
        plane_box(mat, mat, r, d0, d1)


def knob(u, v, r=0.006, h=0.012, mat="metal", offset=0.0):
    """A round knob standing out of the panel at layout (u, v)."""
    mesh_object(mat, cylinder_mesh(mat, r, h, mat, n=KNOB), point(u, v, offset), rotation(u))


def toggle(u, v, offset=0.0, up=True):
    """A toggle switch: a round base and a lever leaning up or down the panel."""
    mesh_object("dark", cylinder_mesh("dark", 0.005, 0.004, "dark", n=KNOB), point(u, v, offset), rotation(u))
    lean = Matrix.Rotation(math.radians(-35.0 if up else 35.0), 3, "X")
    me = box_mesh("metal", (0.004, 0.016, 0.004), "metal", (0.0, -0.011, 0.0), lean.to_quaternion())
    mesh_object("metal", me, point(u, v, offset + 0.003), rotation(u))


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


def facets(name, mat, tris, thickness=0.02):
    """Triangles (m) as sheets `thickness` thick under them (the fuselage's facets)."""
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    for t in tris:
        t = [Vector(v) for v in t]
        n = (t[1] - t[0]).cross(t[2] - t[0]).normalized()
        if n.z < 0:
            n = -n
        top = [bm.verts.new(v) for v in t]
        bot = [bm.verts.new(v - n * thickness) for v in t]
        bm.faces.new(top)
        bm.faces.new(bot[::-1])
        for i in range(3):
            j = (i + 1) % 3
            bm.faces.new((top[i], bot[i], bot[j], top[j]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.to_mesh(me)
    bm.free()
    me.materials.append(mats[mat])
    return mesh_object(name, me, (0, 0, 0))


def solid(name, mat, front, back, thickness, up):
    """A sheet `thickness` m thick under the surface between the polylines front and back (the
    coaming)."""
    me = bpy.data.meshes.new(name)
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
    me.materials.append(mats[mat])
    return mesh_object(name, me, (0.0, 0.0, 0.0))


# --- coaming: over the panel's top edge (the up-front display standing out in the middle),
# forward to COAMING_BACK; level behind the up-front display, rising from the falling wing
# edges to that height (the real one's anti-glare flaps, not modelled, hinge along those).
THICK = 0.016
LIP = 0.008
edge = [
    point(-WING, CORNER), point(-TOP_FLAT, TOP, 0.0, True), point(-TOP_FLAT, TOP, UFC_DEPTH, True),
    point(TOP_FLAT, TOP, UFC_DEPTH, True), point(TOP_FLAT, TOP, 0.0, True), point(WING, CORNER),
]
front = [e + UP * THICK + NORMAL * LIP for e in edge]
z_back = max(f.z for f in front)
back = [Vector((f.x, COAMING_BACK, z_back)) for f in front]
solid("black", "black", front, back, THICK, Vector((0.0, 0.0, 1.0)))
# Out to the walls past the wings' outer edges.
for s in (-1, 1):
    f = front[0 if s < 0 else -1]
    box("black", "black", (s * (WING / 1000 + WALL_X) / 2, (f.y + COAMING_BACK) / 2, f.z - THICK / 2), (WALL_X - WING / 1000, COAMING_BACK - f.y, THICK))
# A bulkhead under its back edge.
box("dark", "dark", (0.0, COAMING_BACK + 0.01, (FLOOR + z_back) / 2), (2 * WALL_X, 0.02, z_back - FLOOR))


def coaming_z(x, y):
    """The coaming's top at (x, y) (between its front edge and the back)."""
    for a, b in zip(front, front[1:]):
        if a.x <= x <= b.x and b.x > a.x:
            t = (x - a.x) / (b.x - a.x)
            f = a.lerp(b, t)
            return f.z + (z_back - f.z) * (y - f.y) / (COAMING_BACK - f.y)
    return z_back


# --- panel: the wings round the side MFDs' wells, the centre column round the centre
# display's, the up-front display standing out at its top
scr = layout["screens"]
faces = {f["key"]: f["rect"] for f in layout["faces"]}


def well(r, side=19.0):
    x, y, w, h = r
    return (x - side, y - 25.0, w + 2 * side, h + 50.0)


wells = {"left": well(scr["left"]), "third": well(scr["third"]), "right": well(scr["right"], 8.0)}
for key in ("left", "third"):
    x, y, w, h = wells[key]
    s_ = -1 if x < 0 else 1
    # In the right wing's terms (the left one mirrored): above, outside, below and inside the well.
    x0, x1 = (x, x + w) if s_ > 0 else (-(x + w), -x)
    y0, y1 = y, y + h
    pieces = [
        [(HINGE, top_v(HINGE)), (x1, top_v(x1)), (x1, y0), (HINGE, y0)],
        [(x1, top_v(x1)), (WING, CORNER), (WING, WING_BOTTOM), (x1, WING_BOTTOM)],
        [(HINGE, y1), (x1, y1), (x1, WING_BOTTOM), (HINGE, WING_BOTTOM)],
        [(HINGE, y0), (x0, y0), (x0, y1), (HINGE, y1)],
    ]
    plate("panel_grey", "panel_grey", [[(s_ * u, v) for u, v in piece] for piece in pieces], 0.03)
x, y, w, h = wells["right"]
plate("panel_grey", "panel_grey", [
    [(-HINGE, top_v(HINGE)), (-TOP_FLAT, TOP), (TOP_FLAT, TOP), (HINGE, top_v(HINGE)), (HINGE, y), (-HINGE, y)],
    [(-HINGE, y + h), (HINGE, y + h), (HINGE, CENTRE_BOTTOM), (-HINGE, CENTRE_BOTTOM)],
    [(-HINGE, y), (x, y), (x, y + h), (-HINGE, y + h)],
    [(x + w, y), (HINGE, y), (HINGE, y + h), (x + w, y + h)],
], 0.03)
plane_box("dark", "dark", (-TOP_FLAT, TOP, 2 * TOP_FLAT, UFC_BOTTOM - TOP), -0.03, UFC_DEPTH)
# Rims standing out along the wings' lower edges.
for s_ in (-1, 1):
    panel_box("dark", "dark", (HINGE if s_ > 0 else -WING, WING_BOTTOM - 6.0, WING - HINGE, 6.0), 0.007)

# --- tub: consoles with sloped panels up to the main panel, knee wall with the footwell,
# centre pedestal
for s in (-1, 1):
    cx = s * sum(CONSOLE_X) / 2
    y0, y1 = CONSOLE_Y
    box("panel_grey", "panel_grey", (cx, (y0 + y1) / 2, (CONSOLE_TOP + FLOOR) / 2), (CONSOLE_X[1] - CONSOLE_X[0], y1 - y0, CONSOLE_TOP - FLOOR))
    box("dark", "dark", (cx, (y0 + y1) / 2, CONSOLE_TOP + 0.002), (CONSOLE_X[1] - CONSOLE_X[0] - 0.01, y1 - y0 - 0.02, 0.004))
    u_mid = -700.0 if s < 0 else 1200.0
    sy, sz = slope_top()
    length = (Vector((sy, sz)) - Vector((CONSOLE_Y[1], CONSOLE_TOP))).length * 1000 + 30.0
    # Its outer edge bevelled in towards the top.
    half = (SLOPE_X[1] - SLOPE_X[0]) * 500
    inner, outer = u_mid - s * half, u_mid + s * half
    top, bottom = -length / 2, length / 2
    shape = [(inner, top), (outer - s * SLOPE_BEVEL, top), (outer, bottom), (inner, bottom)]
    if s > 0:
        shape.reverse()
    plate("dark", "dark", [shape], 0.03)
    # Filled under it to the floor: a wedge under the slope.
    yz = [(CONSOLE_Y[1], CONSOLE_TOP - 0.03), (sy, sz - 0.03), (sy, FLOOR), (CONSOLE_Y[1], FLOOR)]
    prism("panel_grey", "panel_grey", yz, *sorted(s * x for x in CONSOLE_X))
# Walls up to the sills, the sill rails on them; between the sills' front and the coaming's
# rising outer ends, the walls filled up to the coaming.
length = COAMING_BACK - SILL_REAR
for s in (-1, 1):
    box("panel_grey", "panel_grey", (s * (WALL_X + 0.01), (SILL_REAR + COAMING_BACK) / 2, (SILL + FLOOR) / 2), (0.02, length, SILL - FLOOR))
    box("frame", "frame", (s * (WALL_X + 0.008), (SILL_REAR + COAMING_BACK) / 2, SILL - 0.012), (0.034, length, 0.025))
    f = front[0 if s < 0 else -1]
    yz = [(f.y, SILL), (COAMING_BACK, SILL), (COAMING_BACK, z_back), (f.y, f.z)]
    prism("panel_grey", "panel_grey", yz, *sorted((s * WALL_X, s * (WALL_X + 0.02))))
# Canopy bow behind the seat: two bars from the sills up to its apex (the faceted canopy's).
APEX = Vector((0.0, -0.34, 0.28))
for s in (-1, 1):
    p0 = Vector((s * (WALL_X + 0.008), APEX.y, SILL))
    d = APEX - p0
    rot = Vector((1, 0, 0)).rotation_difference(d.normalized()).to_euler()
    box("frame", "frame", (p0 + APEX) / 2, (d.length + 0.02, 0.03, 0.03), rot)
# The fuselage outside the sills: a facet falling outwards along each, narrowing to the front,
# and one from its front up to the coaming's back corner.
for s in (-1, 1):
    rail_rear = Vector((s * (WALL_X + 0.025), SILL_REAR - 0.05, SILL))
    rail_front = Vector((s * (WALL_X + 0.025), COAMING_BACK, SILL))
    out_rear = Vector((s * 1.40, SILL_REAR - 0.05, SILL - 0.45))
    out_front = Vector((s * 0.85, COAMING_BACK + 0.50, SILL - 0.30))
    corner_ = Vector((s * (WALL_X + 0.02), COAMING_BACK + 0.02, z_back))
    facets("dark", "dark", [(rail_rear, rail_front, out_front), (rail_rear, out_front, out_rear), (rail_front, corner_, out_front)])
low = point(0, WING_BOTTOM, 0.0, True)
LEGS = (0.20, -0.48)  # opening's half width, top (m)
wall = low.y + 0.08
for s_ in (-1, 1):
    box("panel_grey", "panel_grey", (s_ * (LEGS[0] + WALL_X) / 2, wall, (low.z + FLOOR) / 2), (WALL_X - LEGS[0], 0.02, low.z - FLOOR))
box("panel_grey", "panel_grey", (0.0, wall, (low.z + LEGS[1]) / 2), (2 * LEGS[0], 0.02, low.z - LEGS[1]))
# The footwell behind the opening: side walls, roof and front wall.
WELL_DEPTH = 0.30
yc = wall + WELL_DEPTH / 2
for s_ in (-1, 1):
    box("dark", "dark", (s_ * (LEGS[0] + 0.01), yc, (LEGS[1] + FLOOR) / 2), (0.02, WELL_DEPTH, LEGS[1] - FLOOR))
box("dark", "dark", (0.0, yc, LEGS[1] + 0.01), (2 * LEGS[0] + 0.04, WELL_DEPTH, 0.02))
box("dark", "dark", (0.0, wall + WELL_DEPTH, (LEGS[1] + FLOOR) / 2), (2 * LEGS[0] + 0.04, 0.02, LEGS[1] - FLOOR))
# Centre pedestal between the legs under the centre column: two panels of knobs down its face
# (the real one's radio and navigation controls).
col = point(0.0, CENTRE_BOTTOM, -0.03, True)
PEDESTAL = 0.07  # half width
pfront = col.y - 0.004
box("panel_grey", "panel_grey", (0.0, (pfront + wall + WELL_DEPTH) / 2, (col.z + FLOOR) / 2), (2 * PEDESTAL, wall + WELL_DEPTH - pfront, col.z - FLOOR))
for k, (z0, rows) in enumerate(((0.015, 3), (0.135, 3))):
    zt = col.z - z0
    box("black", "black", (0.0, pfront - 0.002, zt - 0.05), (0.12, 0.004, 0.10))
    for r in range(rows):
        for c in range(3):
            p = Vector(((c - 1) * 0.034, pfront - 0.004, zt - 0.018 - r * 0.032))
            if (r + c + k) % 3 == 2:
                mesh_object("dark", cylinder_mesh("dark", 0.005, 0.004, "dark", n=KNOB), p)
                lean = Euler((math.radians(-30.0 if r % 2 else 30.0), 0, 0)).to_quaternion()
                mesh_object("metal", box_mesh("metal", (0.004, 0.016, 0.004), "metal", (0.0, -0.008, 0.0), lean), p - Vector((0, 0.003, 0)))
            else:
                mesh_object("metal", cylinder_mesh("metal", 0.009 if r == 0 else 0.007, 0.012, "metal", n=KNOB), p)

# --- plates, bezels, faces and displays
PLATE = 0.004
plates = [
    ("black", (-77.0, -136.0, 154.0, 80.0)),  # up-front display
    ("black", (-62.0, -53.0, 124.0, 14.0)),  # its knobs under it
    ("black", (-72.0, 156.0, 144.0, 16.0)),  # the centre display's knobs
    ("dark", (-171.0, -99.0, 72.0, 32.0)),  # left warning lights
    ("dark", (99.0, -99.0, 72.0, 32.0)),  # right warning lights
    ("dark", (-326.0, -40.0, 52.0, 200.0)),  # flight instruments
    ("dark", (274.0, -40.0, 52.0, 188.0)),  # engine instruments
    ("black", (-166.0, 88.0, 52.0, 64.0)),  # master arm panel
    ("dark", (106.0, 84.0, 156.0, 72.0)),  # fuel, engine readouts, EMV
    ("dark", (-736.0, -114.0, 48.0, 40.0)),  # gear lights (left slope)
    ("dark", (-735.0, -72.0, 46.0, 54.0)),  # flaps, brakes and autopilot under them
    ("dark", (1128.0, -122.0, 108.0, 40.0)),  # caution lights (right slope)
    ("dark", (1146.0, -74.0, 80.0, 75.0)),  # countermeasures
    ("black", (1146.0, 24.0, 110.0, 70.0)),  # switches under them
]
for mat, r in plates:
    panel_box(mat, mat, r, PLATE)
for u in (-36.0, -6.0, 24.0):
    knob(u, -46.0, 0.006, 0.010, offset=PLATE)
toggle(48.0, -46.0, PLATE)
for u in (-50.0, -18.0, 18.0, 50.0):
    knob(u, 164.0, 0.006, 0.010, offset=PLATE)
# Master arm panel: master arm and bay door switches, the jettison button under a red guard.
for u in (-154.0, -138.0):
    toggle(u, 124.0, PLATE, up=(u == -154.0))
mesh_object("red", cylinder_mesh("red", 0.005, 0.005, "red", n=KNOB), point(-124.0, 124.0, PLATE), rotation(-124.0))
mesh_object("red", box_mesh("red", (0.014, 0.012, 0.022), "red", (0.0, -0.006, -0.002)), point(-124.0, 140.0, PLATE), rotation(-124.0))
for v in (40.0, 60.0, 80.0):
    toggle(1160.0, v, PLATE, up=(v != 60.0))
    toggle(1200.0, v, PLATE, up=(v == 60.0))
knob(1240.0, 60.0, 0.008, 0.012, offset=PLATE)
# MFD bezels: a housing standing out of the well to the bezel's face (BEZEL off the panel), the
# screen back in a black recess. The side MFDs as the F-16's: twenty push buttons round the
# screen with ridges between them and rocker switches at the corners (the game uses the top and
# bottom rows). The centre display: its top and bottom rows only.
BEZEL = 0.006
for key in ("left", "third", "right"):
    x, y, w, h = scr[key]
    wx, wy, ww, wh = wells[key]
    hole = (x - 3.0, y - 3.0, w + 6.0, h + 6.0)
    frame("dark", (wx - 3.0, wy - 3.0, ww + 6.0, wh + 6.0), hole, -0.03, BEZEL - 0.002)
    frame("black", (wx + 1.0, wy + 1.0, ww - 2.0, wh - 2.0), hole, BEZEL - 0.002, BEZEL)
    frame("black", hole, (x - 0.5, y - 0.5, w + 1.0, h + 1.0), -0.006, BEZEL - 0.001)
    for k in range(1, 5):
        for by in (y - 16.5, y + h + 6.5):
            panel_box("black", "black", (x + w * k / 5 - 1.0, by - 4.0, 2.0, 8.0), 0.003, 0.0, BEZEL)
    if key == "right":
        continue
    for k in range(5):
        by = y + h * (k + 0.5) / 5
        for bx in (x - 9.0, x + w + 9.0):
            panel_box("dark", "dark", (bx - 5.0, by - 6.0, 10.0, 12.0), 0.005, 0.0, BEZEL)
    for k in range(1, 5):
        for bx in (x - 9.0, x + w + 9.0):
            panel_box("black", "black", (bx - 4.0, y + h * k / 5 - 1.0, 8.0, 2.0), 0.003, 0.0, BEZEL)
    for bx in (x - 9.0, x + w + 9.0):
        for by in (y - 16.0, y + h + 16.0):
            panel_box("dark", "dark", (bx - 4.5, by - 6.5, 9.0, 13.0), 0.003, 0.0, BEZEL)
            panel_box("black", "black", (bx - 1.5, by - 5.0, 3.0, 10.0), 0.003, 0.0, BEZEL + 0.003)
# Setting knobs at instrument corners: airspeed and altimeter, the attitude indicator and the
# clock.


def corner(r, sx, sy, inset=3.0):
    x, y, w, h = r
    return (x + w - inset if sx > 0 else x + inset, y + h - inset if sy > 0 else y + inset)


for r, sx, sy in (
    (faces["Item(Dial(Airspeed))"], 1, 1),
    (faces["Item(Dial(Altimeter))"], 1, 1),
    (scr["adi"], 1, 1),
    (faces["Item(Dial(Clock))"], 1, 1),
):
    u, v = corner(r, sx, sy)
    knob(u, v, 0.0045, 0.009, "dark", offset=PLATE)
screen("screen_left", scr["left"], -0.004, "trackcam_left")
screen("screen_right", scr["right"], -0.004, "trackcam")
screen("screen_third", scr["third"], -0.004, "trackcam_third")
screen("adi", scr["adi"], 0.0, round=True)
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

# --- side console panels round the throttle and the stick (dressing, as the F-16's)
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
    side(0.198, 0.346, -0.262, -0.176, ["kKk", "tgt"])
    side(0.198, 0.346, -0.168, -0.112, ["tttt", "kTk"] if s > 0 else ["tttt", "rgrg"])
for rows, rect in (
    (["kk", "TT", "kr"], (-0.346, -0.198, -0.100, 0.040)),  # ahead of those, behind the throttle
    (["k", "t", "r", "k"], (-0.350, -0.300, 0.060, 0.232)),  # outboard of the throttle rails
    (["t", "k", "t"], (-0.246, -0.198, 0.060, 0.232)),  # inboard of them
    (["kk", "tt", "rr", "Kk"], (0.198, 0.240, -0.100, 0.120)),  # inboard of the arm rest
):
    console_panel(*rect, rows)
# Arm rest behind the stick.
box("cushion", "cushion", (0.295, 0.01, CONSOLE_TOP + 0.015), (0.09, 0.22, 0.03))
# Yellow handles on the consoles' inner faces by the pilot's knees (canopy jettison left,
# its emergency release right).
for s, y in ((-1, 0.20), (1, 0.06)):
    box("yellow", "yellow", (s * (CONSOLE_X[0] - 0.022), y, CONSOLE_TOP - 0.07), (0.012, 0.012, 0.08))
    for dz in (-0.03, 0.03):
        box("yellow", "yellow", (s * (CONSOLE_X[0] - 0.011), y, CONSOLE_TOP - 0.07 + dz), (0.022, 0.01, 0.01))


# --- the new parts to the F-117A cockpit's size: positions and meshes scaled about the eye,
# the controls' sizes with them
SIZE_PROPS = ("height", "travel")


def scale(o, k):
    o.location = o.location * k
    if o.type == "MESH":
        o.data.transform(Matrix.Scale(k, 4))
    for p in SIZE_PROPS:
        if p in o.keys():
            o[p] = o[p] * k
    if "half" in o.keys():
        o["half"] = [h * k for h in o["half"]]
    for c in o.children:
        scale(c, k)


for o in list(root.children):
    if o not in old:
        scale(o, SCALE)

# --- the kept parts
objs = {o.name: o for o in root.children}
rise = CONSOLE_TOP * SCALE + PIVOT_OVER_CONSOLE - objs["stick"].location.z
for n in LEVERS:
    objs[n].location.z += rise
# Target buttons on the left slope, ahead of the throttle, faces up towards the pilot; the gear
# handle right of the gear lights above them.
for i, n in enumerate(["button_NEAR", "button_NEXT", "button_NONE", "button_WPN", "button_DTL"]):
    o = objs[n]
    col_, row = i // 3, i % 3
    u = -700.0 - (-25.0 + 45.0 * col_)
    o.location = point(u, 15.0 + 34.0 * row, 0.004) * SCALE
    o.rotation_mode = "QUATERNION"
    o.rotation_quaternion = rotation(u)
g = objs["control_Action_Gear_.001"]
g.location = point(-668.0, -94.0, 0.002) * SCALE
g.rotation_mode = "QUATERNION"
g.rotation_quaternion = rotation(-668.0)
# HUD frame (the cockpit's scale): bars along the glass's outline (its posts, chamfered top
# corners and top), outside it by half a bar, the posts standing on the coaming; each bar
# overlaps the next by half a bar so the joints close.
hud = next(o for o in root.children if o.name.startswith("hud_glass"))
glass = [hud.matrix_world @ v.co for v in hud.data.vertices]
hud_y = sum(v.y for v in glass) / len(glass)
half_x = max(abs(v.x) for v in glass)
top = max(v.z for v in glass)
# The chamfers: from the posts' tops (the highest glass corner at the side) to the top's ends.
shoulder = max(v.z for v in glass if abs(abs(v.x) - half_x) < 0.01)
top_x = min(abs(v.x) for v in glass if abs(v.z - top) < 0.002)
post_x = half_x + HUD_BAR / 2
bottom = [coaming_z(s * post_x / SCALE, hud_y / SCALE) * SCALE for s in (-1, 1)]
outline = [
    Vector((-post_x, bottom[0])), Vector((-post_x, shoulder)),
    Vector((-top_x, top + HUD_BAR / 2)), Vector((top_x, top + HUD_BAR / 2)),
    Vector((post_x, shoulder)), Vector((post_x, bottom[1])),
]
for a, b in zip(outline, outline[1:]):
    d = b - a
    centre = (a + b) / 2
    rot = Euler((0.0, -math.atan2(d.y, d.x), 0.0))
    box("frame", "frame", (centre.x, hud_y, centre.y), (d.length + HUD_BAR, HUD_BAR, HUD_BAR), rot)
# The approach indexer just outside the left post, in a housing behind it.
ind = next(o for o in root.children if o.name.startswith("approach"))
ind.location.x = -(post_x + HUD_BAR / 2 + 0.018)
box("dark", "dark", (ind.location.x, hud_y + 0.009, ind.location.z), (0.040, 0.014, 0.097))

bpy.data.orphans_purge(do_recursive=True)
bpy.ops.wm.save_as_mainfile(filepath=out)
print("built", out, "objects", len(bpy.data.objects))
