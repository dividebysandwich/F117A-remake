"""Builds the remake's own aircraft models (Geran-2 / Geran-3 attack drones) in Blender:

    blender -b --factory-startup --python-exit-code 1 --python tools/models/drones.py -- assets/models/drones.glb

Nodes follow stflt.glb's conventions: a root `drones_NNN` with `attr`, a `_lod0` mesh and a
spinning `_part1` (state slot 17; axis 2 = fore-aft, the Geran-2's pusher propeller). Model
space: x right, y forward, z up, aircraft model units (WU / 8 m).
"""

import math
import os
import sys

import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, HERE)

import mklib  # noqa: E402
from mklib import Geo, Ramp, frustum, loft  # noqa: E402

# Model numbers (kept clear of STFLT's 0..34).
GERAN2 = 100
GERAN3 = 101

# Dark grey/black paint of the recent Gerans, a lighter grey belly.
BODY = Ramp(0xE9, 0xEF)
WING = Ramp(0xE8, 0xEE)
BELLY = Ramp(0xE4, 0xEA)
PROP = 0xEF
NOZZLE = Ramp(0xEB, 0xEF)
# Nav-light colours used by stflt (red / green / white).
RED, GREEN, WHITE = 147, 149, 151


def empty(name, parent=None, loc=(0, 0, 0)):
    ob = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(ob)
    ob.parent = parent
    ob.location = loc
    return ob


def mesh_obj(name, g, parent, loc=(0, 0, 0)):
    ob = bpy.data.objects.new(name, mklib.faces_mesh(name, g))
    bpy.context.scene.collection.objects.link(ob)
    ob.parent = parent
    ob.location = loc
    return ob


def delta_wing(span, root_le, root_chord, tip_le_y, tip_chord, z, t):
    """Mirrored cropped-delta wing: root leading edge at (0, root_le), tip at +-span/2."""
    return mklib.wing((0, root_le, z), root_chord, (span / 2, tip_le_y, z), tip_chord, WING, t=t, t_tip=0.05, peak=0.4)


def tip_fins(span, y, chord, up, down, z):
    """Fins at the wing tips, reaching above and below the wing."""
    g = Geo()
    for sx in (-1, 1):
        x = sx * span / 2
        g.face2([(x, y - chord * 0.1, z), (x, y - chord, z), (x, y - chord, z + up), (x, y - chord * 0.45, z + up)], WING)
        g.face2([(x, y - chord * 0.3, z), (x, y - chord, z), (x, y - chord, z - down), (x, y - chord * 0.6, z - down)], WING)
    return g


# Proportions from photos of the Shahed-136 (3.5 m long, 2.5 m span, about 0.45 m across the
# nose): a round nose ahead of the wing apex, then a blended wing-body as thick as the wing root.
LENGTH_NOSE, TAIL = 36, -30
SPAN = 48
APEX = 19
TIP_LE, TIP_CHORD = -19, 10


def airframe(nose=LENGTH_NOSE):
    g = Geo()
    # Nose and the low spine over the wing (the same height as the wing root behind the apex).
    g.add(loft([(nose, 0, 0.6, 0.6), (nose - 3, 0, 3.0, 3.0), (nose - 9, 0, 4.2, 4.2), (APEX + 4, 0, 4.2, 3.8),
                (APEX - 6, 0, 3.2, 2.8), (-12, 0, 2.6, 2.4), (TAIL + 2, 0, 2.2, 2.0), (TAIL, 0, 1.6, 1.4)], BODY, n=8))
    g.add(delta_wing(SPAN, APEX, APEX - TAIL, TIP_LE, TIP_CHORD, 0, t=0.1))
    g.add(tip_fins(SPAN, TIP_LE, TIP_CHORD, 8, 4.5, 0))
    g.line((-SPAN / 2, TIP_LE - TIP_CHORD, 0), (-SPAN / 2, TIP_LE, 0), RED)
    g.line((SPAN / 2, TIP_LE - TIP_CHORD, 0), (SPAN / 2, TIP_LE, 0), GREEN)
    return g


def geran2():
    """Shahed-136 / Geran-2: piston engine in the tail with a pusher propeller."""
    g = airframe()
    g.add(frustum((0, TAIL + 1, 0), (0, TAIL - 3, 0), 2.2, 1.6, BELLY, n=8))
    prop = Geo()
    # Two-bladed pusher propeller around its hub (the part origin), about 0.8 m across.
    for a in (0, math.pi):
        c, s = math.cos(a), math.sin(a)
        prop.face2([(c * 1.0, 0, s * 1.0), (c * 7, 0.4, s * 7 + 1.4 * c), (c * 7, -0.4, s * 7 - 1.4 * c), (c * 1.0, 0.3, -s * 1.0)], PROP)
    return g, prop, (0, TAIL - 4, 0)


def geran3():
    """Shahed-238 / Geran-3: the same airframe with a small turbojet on the back of the tail."""
    g = airframe(LENGTH_NOSE + 2)
    g.add(loft([(-8, 2.2, 0.4, 0.4), (-10, 2.2, 1.9, 1.9), (-26, 2.2, 2.1, 2.1), (TAIL - 3, 2.2, 1.6, 1.6)], NOZZLE, n=8))
    return g, None, None


def model(index, geo, prop=None, prop_at=None):
    name = "drones_%03d" % index
    root = empty(name)
    root["attr"] = 0x81
    root["lod_dists"] = [32769]
    lod = mesh_obj(name + "_lod0", geo, root)
    pts = [p for f, _ in geo.faces for p in f]
    lod["max_dist"] = 32769
    lod["radius"] = int(math.ceil(max(p.length for p in pts)))
    for c, segs in sorted(geo.lines.items()):
        ln = "%s_lod0_lines_pal_%03d" % (name, c)
        lo = bpy.data.objects.new(ln, mklib.lines_mesh(ln, segs, c))
        bpy.context.scene.collection.objects.link(lo)
        lo.parent = lod
    if prop is not None:
        part = mesh_obj(name + "_lod0_part1", prop, lod, prop_at)
        part["state_slot"] = 17
        part["axis"] = 2


def main():
    out = sys.argv[sys.argv.index("--") + 1]
    mklib.load_palette(os.path.join(ROOT, "assets", "palettes", "tod_ce.ron"))
    for o in list(bpy.data.objects):
        bpy.data.objects.remove(o, do_unlink=True)
    model(GERAN2, *geran2())
    g3, _, _ = geran3()
    model(GERAN3, g3)
    bpy.ops.export_scene.gltf(
        filepath=out, export_format="GLB", export_extras=True, export_yup=True, export_texcoords=False,
        export_normals=False, export_materials="EXPORT", use_visible=False, export_animations=False,
        use_mesh_edges=True, use_mesh_vertices=True,
    )


main()
