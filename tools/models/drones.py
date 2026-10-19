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


def delta_wing(span, root_le, root_chord, tip_le_y, tip_chord, z, t=0.05):
    """Mirrored cropped-delta wing: root leading edge at (0, root_le), tip at +-span/2."""
    return mklib.wing((0, root_le, z), root_chord, (span / 2, tip_le_y, z), tip_chord, WING, t=t, t_tip=0.03)


def winglets(span, y, chord, h, z):
    g = Geo()
    for sx in (-1, 1):
        x = sx * span / 2
        g.face2([(x, y, z), (x, y - chord, z), (x, y - chord, z + h), (x, y - chord * 0.35, z + h)], WING)
    return g


def geran2():
    """Shahed-136 / Geran-2: cropped delta, short fuselage, pusher propeller, tip fins."""
    g = Geo()
    g.add(loft([(36, 0, 0.5, 0.5), (32, 0, 4, 4), (20, 0, 6.5, 6), (-10, 0, 6.5, 6), (-26, 0, 5, 4.5), (-30, 0, 3, 3)], BODY, n=8))
    g.add(delta_wing(56, 24, 52, -22, 12, 0))
    g.add(winglets(56, -22, 12, 9, 0))
    g.add(winglets(56, -22, 12, -7, 0).recolour(WING))
    # Engine nacelle behind the wing root.
    g.add(frustum((0, -26, 0), (0, -32, 0), 3.2, 2.2, BELLY, n=8))
    g.line((-28, -28, 0), (-28, -12, 0), RED)
    g.line((28, -28, 0), (28, -12, 0), GREEN)
    prop = Geo()
    # Two-bladed pusher propeller around its hub (the part origin).
    for a in (0, math.pi):
        c, s = math.cos(a), math.sin(a)
        prop.face2([(c * 1.2, 0, s * 1.2), (c * 14, 0.5, s * 14 + 1.8 * c), (c * 14, -0.5, s * 14 - 1.8 * c), (c * 1.2, 0.4, -s * 1.2)], PROP)
    return g, prop, (0, -33, 0)


def geran3():
    """Geran-3: the jet version, a longer fuselage with a turbojet on the spine."""
    g = Geo()
    g.add(loft([(40, 0, 0.5, 0.5), (36, 0, 4.5, 4.5), (22, 0, 7, 6.5), (-14, 0, 7, 6.5), (-30, 0, 5, 4.5), (-34, 0, 2.5, 2.5)], BODY, n=8))
    g.add(delta_wing(54, 22, 50, -24, 12, -1))
    g.add(winglets(54, -24, 12, 9, -1))
    # Turbojet pod on the spine.
    g.add(loft([(4, 9, 0.5, 0.5), (0, 9, 3.5, 3.5), (-22, 9, 3.8, 3.8), (-34, 9, 3, 3)], NOZZLE, n=8))
    g.add(frustum((0, -10, 5), (0, -10, 7), 2, 2, BODY, n=6))
    g.line((-27, -30, -1), (-27, -14, -1), RED)
    g.line((27, -30, -1), (27, -14, -1), GREEN)
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
