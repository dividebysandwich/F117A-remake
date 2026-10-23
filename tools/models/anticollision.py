# Adds anti-collision lights (dots shown while a state slot is 1) to the aircraft models of
# stflt.blend and saves it; run again after changing the table (it replaces its own nodes):
#   blender -b assets-src/blend/stflt.blend --python-exit-code 1 --python tools/models/anticollision.py
#
# Slots (driven by sim_render.rs): 18 = white strobe, single flash (fighters' fin tips, the
# A-10's wingtips), 19 = white strobe, double flash (transports' wingtips), 20 = red beacon.
# Lights sit on the skin: a ray down or up the centreline at a station (`top`, `bottom`), the
# front of a fin tip (`fin`), or just aft of the wingtip navigation lights (`wingtips`).
# Model units, Blender axes: +x right, +y forward, +z up.
# The UH-60 (25) gets its beacons on every detail level from tools/models/uh60.py.
# Also removes the stock models' steady white tail lights (colour 149 on the navigation-light
# slot 12), leaving the red/green wingtip lights and the flashing anti-collision lights.
import bmesh
import bpy
from mathutils import Vector
from mathutils.bvhtree import BVHTree

WHITE, RED = 15, 147
SINGLE, DOUBLE, BEACON = 18, 19, 20
# Clearance off the skin, and the strobes' distance aft of the wingtip navigation lights.
LIFT = 1.5
AFT = 4.0

FIGHTER_RED = [(BEACON, "fin"), (BEACON, "bottom", 0)]
FIGHTER_WHITE = [(SINGLE, "fin"), (BEACON, "bottom", 0)]
TWIN_RED = [(BEACON, "top", -32), (BEACON, "bottom", 0)]
LIGHTS = {
    0: TWIN_RED,  # MiG-25
    3: [(SINGLE, "fin")],  # F/A-18: strobes on both fin tips
    5: [(BEACON, "top", 0), (BEACON, "bottom", 0)],  # F-14
    6: [(BEACON, "top", -64), (BEACON, "bottom", 0), (DOUBLE, "wingtips")],  # An-72
    7: FIGHTER_RED,  # MiG-23
    8: FIGHTER_RED,  # F-4E
    9: [(BEACON, "top", -64), (BEACON, "bottom", 0), (DOUBLE, "wingtips")],  # Il-76 / A-50
    13: FIGHTER_RED,  # Yak-38
    14: [(BEACON, "top", 0), (BEACON, "bottom", 0), (DOUBLE, "wingtips")],  # Tu-95
    16: TWIN_RED,  # MiG-29
    18: FIGHTER_RED,  # MiG-17
    19: FIGHTER_WHITE,  # Mirage F1
    20: FIGHTER_WHITE,  # F-5
    21: [(SINGLE, "fin")],  # F-15
    22: TWIN_RED,  # Su-27
    23: [(SINGLE, "fin")],  # F-16
    24: [(BEACON, "top", 0), (BEACON, "bottom", 16)],  # F-117A
    26: [(SINGLE, "fin")],  # F/A-18
    27: [(BEACON, "top", -32), (BEACON, "bottom", 0)],  # A-6
    28: FIGHTER_RED,  # MiG-21
    29: [(BEACON, "top", -48), (BEACON, "bottom", 0)],  # EF-111
    30: [(BEACON, "top", -32), (BEACON, "bottom", 0), (DOUBLE, "wingtips")],  # C-5
    31: [(BEACON, "top", 0), (BEACON, "bottom", 16)],  # F-117A
    32: [(SINGLE, "wingtips")],  # A-10
    33: [(BEACON, "top", 0), (BEACON, "bottom", 16)],  # F-117A
}


def body(lod0):
    """BVH of the model's surfaces (no lights, lines, flames or alternative states)."""
    bm = bmesh.new()

    def walk(o):
        tail = o.name[len(lod0.name):]
        alt = "_s" in tail and not tail.startswith("_s12v1")
        if o.type == "MESH" and not alt and "_dots_" not in o.name and "_lines_" not in o.name:
            part = bmesh.new()
            part.from_mesh(o.data)
            part.transform(o.matrix_world)
            me = bpy.data.meshes.new("tmp")
            part.to_mesh(me)
            bm.from_mesh(me)
            bpy.data.meshes.remove(me)
        for c in o.children:
            walk(c)

    walk(lod0)
    return bm


def points(lod0, bm, bvh, spec):
    kind = spec[1]
    if kind in ("top", "bottom"):
        down = kind == "top"
        hit = bvh.ray_cast(Vector((0, spec[2], 500 if down else -500)), Vector((0, 0, -1 if down else 1)))[0]
        if hit is None:
            raise SystemExit(f"{lod0.name}: no {kind} surface at y {spec[2]}")
        return [hit + Vector((0, 0, LIFT if down else -LIFT))]
    if kind == "fin":
        # The highest vertices are the fin tips (one, or two either side of the centreline).
        top = max(v.co.z for v in bm.verts)
        tip = [v.co for v in bm.verts if v.co.z > top - 3]
        sides = [s for s in (-1, 0, 1) if any((v.x > 5) - (v.x < -5) == s for v in tip)]
        out = []
        for s in sides:
            vs = [v for v in tip if (v.x > 5) - (v.x < -5) == s]
            front = max(vs, key=lambda v: v.y)
            out.append(Vector((front.x, front.y - 3, top + LIFT)))
        return out
    if kind == "wingtips":
        nav = [o for o in lod0.children_recursive if "_s12v1_dots_pal_147" in o.name or "_s12v1_dots_pal_151" in o.name]
        tips = {tuple(round(c, 3) for c in o.matrix_world @ v.co) for o in nav for v in o.data.vertices}
        return [Vector(t) - Vector((0, AFT, 0)) for t in sorted(tips)]
    raise SystemExit(f"unknown light position {kind}")


def remove(o):
    """Removes an object and its mesh (a leftover mesh would take the name of its replacement)."""
    data = o.data
    bpy.data.objects.remove(o)
    if data is not None and data.users == 0:
        bpy.data.meshes.remove(data)


for o in [o for o in bpy.data.objects if "_s12v1_dots_pal_149" in o.name]:
    remove(o)

for model, specs in LIGHTS.items():
    name = f"stflt_{model:03}_lod0"
    lod0 = bpy.data.objects[name]
    for o in [o for o in lod0.children_recursive if any(f"_s{s}v1" in o.name for s in (SINGLE, DOUBLE, BEACON))]:
        remove(o)
    bm = body(lod0)
    bvh = BVHTree.FromBMesh(bm)
    lights = {}
    for spec in specs:
        lights.setdefault(spec[0], []).extend(points(lod0, bm, bvh, spec))
    for slot, ps in lights.items():
        group = bpy.data.objects.new(f"{name}_s{slot}v1", None)
        group["conds"] = [[slot, 1]]
        group.parent = lod0
        colour = RED if slot == BEACON else WHITE
        mesh = bpy.data.meshes.new(f"{name}_s{slot}v1_dots_pal_{colour:03}")
        mesh.from_pydata([tuple(p) for p in ps], [], [])
        dots = bpy.data.objects.new(mesh.name, mesh)
        dots.parent = group
        for o in (group, dots):
            lod0.users_collection[0].objects.link(o)
        print(name, slot, [tuple(round(c, 1) for c in p) for p in ps])
bpy.ops.wm.save_mainfile()
