"""Builds the remake's low-level air defences (ZSU-23-4 Shilka, S-60 gun, MANPADS team) in
Blender, for the air defence option (crates/stealth-sim/src/aaa.rs):

    blender -b --factory-startup --python-exit-code 1 --python tools/models/air_defence.py -- assets/models/aaa.glb

Nodes follow stflt.glb's conventions: a root `aaa_NNN` with `attr` and `_lodN` levels. In the
detailed level the intact model is `_s1v0` and the wreck `_s1v1` (state slot 1, the destroyed
variant); the intact model's moving parts are `_part1` (turret or crew, yaw, state slot 21),
`_part2` (guns or launcher, elevation, slot 22, a child of part 1) and on the Shilka `_part3`
(the Gun Dish radar, turning with the global radar angle, slot 16). Model space: x right,
y forward, z up, aircraft model units (WU / 8 m); built in metres and scaled.
"""

import math
import os
import sys

import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, HERE)

import mklib  # noqa: E402
from mklib import Geo, Ramp, block, frustum, wheel  # noqa: E402

# Model numbers (after the drones' 100 and 101).
ZSU = 110
S60 = 111
MANPADS = 112

# Model units per metre (a unit is WU / 8 = 0.061 m).
K = 8.0 * 1_048_576 / 512_000

OLIVE = Ramp(0xA6, 0xAF)
OLIVE_DARK = Ramp(0xAA, 0xAF)
KHAKI = Ramp(0xA1, 0xA5)
STEEL = Ramp(0xEB, 0xEF)
RUBBER = Ramp(0xED, 0xEF)
BURNT = Ramp(0xEC, 0xEF)
SKIN = Ramp(0xC2, 0xC5)
GLASS = 0x0B

TURRET_SLOT, GUN_SLOT, RADAR_SLOT = 21, 22, 16


def empty(name, parent=None, loc=(0, 0, 0)):
    ob = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(ob)
    ob.parent = parent
    ob.location = tuple(c * K for c in loc)
    return ob


def mesh_obj(name, g, parent, loc=(0, 0, 0)):
    ob = bpy.data.objects.new(name, mklib.faces_mesh(name, g.scaled(K)))
    bpy.context.scene.collection.objects.link(ob)
    ob.parent = parent
    ob.location = tuple(c * K for c in loc)
    return ob


def part(name, g, parent, loc, slot, axis):
    ob = mesh_obj(name, g, parent, loc)
    ob["state_slot"] = slot
    ob["axis"] = axis
    return ob


def barrel(y0, y1, r, x=0.0, z=0.0, paint=STEEL, n=6):
    return frustum((x, y0, z), (x, y1, z), r, r, paint, n=n)


def sandbag_ring(r, h, n, gap=None, z0=0.0):
    """A ring of sandbags (a low wall), open over the arc `gap` (radians, around +y)."""
    g = Geo()
    for k in range(n):
        a0, a1 = 2 * math.pi * k / n, 2 * math.pi * (k + 1) / n
        mid = (a0 + a1) / 2
        if gap is not None and abs(math.atan2(math.sin(mid), math.cos(mid))) < gap / 2:
            continue
        inner, outer = r - 0.25, r + 0.25
        pts = lambda rr, a: (rr * math.sin(a), rr * math.cos(a))  # noqa: E731
        (x0, y0), (x1, y1) = pts(inner, a0), pts(inner, a1)
        (x2, y2), (x3, y3) = pts(outer, a1), pts(outer, a0)
        g.add(mklib.hexa(
            [(x0, y0, z0), (x1, y1, z0), (x2, y2, z0), (x3, y3, z0),
             (x0 * 0.98, y0 * 0.98, z0 + h), (x1 * 0.98, y1 * 0.98, z0 + h), (x2 * 0.96, y2 * 0.96, z0 + h * 0.9), (x3 * 0.96, y3 * 0.96, z0 + h * 0.9)],
            KHAKI,
        ))
    return g


# ------------------------------------------------------------------------------------ ZSU-23-4


def zsu_hull(paint=OLIVE, wheels=RUBBER):
    """Hull 6.5 x 3.1 m on six road wheels a side, track skirts, the sloped glacis in front."""
    g = Geo()
    g.add(mklib.side_prism([(-3.2, 0.45), (2.9, 0.45), (3.25, 0.95), (3.0, 1.45), (-3.25, 1.45)], -1.3, 1.3, paint))
    for sx in (-1, 1):
        x = sx * 1.4
        for k in range(6):
            g.add(wheel((x, -2.35 + k * 0.94, 0.34), 0.34, 0.3, wheels, n=8))
        # Drive sprocket and idler, the track's upper run and the skirt over it.
        g.add(wheel((x, 2.8, 0.55), 0.3, 0.3, wheels, n=8))
        g.add(wheel((x, -2.85, 0.5), 0.28, 0.3, wheels, n=8))
        g.add(block(x - 0.18, x + 0.18, -3.0, 2.95, 0.66, 0.74, wheels))
        g.add(block(x + sx * 0.15 - 0.03, x + sx * 0.15 + 0.03, -3.05, 3.0, 0.75, 1.15, paint))
    # Engine deck louvres at the rear, a hatch and the driver's vision block in front.
    g.add(block(-1.0, 1.0, -3.0, -1.6, 1.45, 1.5, OLIVE_DARK))
    g.add(block(-1.05, -0.45, 2.15, 2.6, 1.45, 1.55, paint))
    g.add(block(-0.95, -0.55, 2.55, 2.62, 1.47, 1.53, GLASS))
    return g


def zsu_turret(paint=OLIVE):
    """Flat, wide turret around its pivot (the turret ring's centre)."""
    g = Geo()
    g.add(block(-1.45, 1.45, -1.3, 1.5, 0.0, 0.72, paint, taper=0.18, taper_y=0.12))
    # Gun housings either side, commander's hatch and periscopes.
    for sx in (-1, 1):
        g.add(block(sx * 1.45 - 0.2, sx * 1.45 + 0.2, -0.2, 1.55, 0.12, 0.6, paint))
    g.add(frustum((0.6, -0.4, 0.72), (0.6, -0.4, 0.84), 0.32, 0.28, OLIVE_DARK, n=8))
    g.add(block(-0.3, 0.1, 1.2, 1.45, 0.72, 0.82, GLASS))
    return g


def zsu_guns():
    """Four 23 mm guns in two pairs around the trunnions (the part origin)."""
    g = Geo()
    for sx in (-1, 1):
        for dz in (-0.09, 0.09):
            g.add(barrel(0.0, 2.2, 0.035, x=sx * 1.45, z=dz))
            g.add(barrel(1.7, 2.25, 0.06, x=sx * 1.45, z=dz))
        g.add(block(sx * 1.45 - 0.12, sx * 1.45 + 0.12, -0.3, 0.35, -0.18, 0.18, OLIVE_DARK))
    return g


def zsu_radar():
    """The Gun Dish: a dish on a short mast (part origin at the mast foot)."""
    g = Geo()
    g.add(frustum((0, 0, 0), (0, 0, 0.35), 0.08, 0.08, STEEL, n=6))
    g.add(frustum((0, 0.05, 0.75), (0, 0.22, 0.75), 0.62, 0.42, OLIVE_DARK, n=10, rot=0.0))
    g.add(block(-0.08, 0.08, -0.05, 0.1, 0.35, 0.8, STEEL))
    return g


def zsu():
    root, lod0, lod1 = levels(ZSU, 16000)
    intact = mesh_obj("aaa_110_lod0_s1v0", zsu_hull(), lod0)
    turret = part("aaa_110_lod0_s1v0_part1", zsu_turret(), intact, (0, 0.3, 1.45), TURRET_SLOT, 0)
    part("aaa_110_lod0_s1v0_part2", zsu_guns(), turret, (0, 0.9, 0.36), GUN_SLOT, 1)
    part("aaa_110_lod0_s1v0_part3", zsu_radar(), turret, (0, -1.0, 0.72), RADAR_SLOT, 0)
    # The wreck: burnt hull, the turret knocked askew, the guns drooping.
    w = zsu_hull(BURNT, BURNT)
    w.add(zsu_turret(BURNT).rotated_z(0.45).transformed(lambda p: p + mklib.v3((0.2, 0.3, 1.4))))
    guns = zsu_guns().recolour(BURNT).rotated_x(-0.25)
    w.add(guns.rotated_z(0.45).transformed(lambda p: p + mklib.v3((0.0, 1.3, 1.65))))
    mesh_obj("aaa_110_lod0_s1v1", w, lod0)
    far(lod1, "aaa_110", [(-1.5, 1.5, -3.2, 3.2, 0.2, 1.45), (-1.45, 1.45, -1.0, 1.8, 1.45, 2.15)])


# ----------------------------------------------------------------------------------------- S-60


def s60_carriage(paint=OLIVE, wheels=RUBBER):
    """Four-wheeled carriage on its jacks, outriggers out, inside a sandbag ring."""
    g = Geo()
    g.add(block(-0.75, 0.75, -2.0, 2.0, 0.55, 0.85, paint))
    for sx in (-1, 1):
        for y in (-1.3, 1.3):
            g.add(wheel((sx * 1.0, y, 0.48), 0.45, 0.3, wheels, n=8))
    # Outriggers to the ground, each with a jack pad.
    for a in (45, 135, 225, 315):
        t = math.radians(a)
        dx, dy = math.sin(t), math.cos(t)
        ex, ey = dx * 2.6, dy * 2.6
        g.add(frustum((dx * 0.6, dy * 0.6, 0.7), (ex, ey, 0.15), 0.08, 0.08, paint, n=4))
        g.add(block(ex - 0.2, ex + 0.2, ey - 0.2, ey + 0.2, 0.0, 0.12, STEEL))
    # Towbar and the ready ammunition crate beside the gun.
    g.add(frustum((0, 2.0, 0.65), (0, 3.0, 0.35), 0.05, 0.05, paint, n=4))
    g.add(block(1.4, 2.0, -0.6, 0.2, 0.0, 0.45, OLIVE_DARK))
    g.add(sandbag_ring(4.0, 0.7, 14, gap=0.9))
    return g


def s60_mount(paint=OLIVE):
    """Cradle and the shield around the traverse pivot."""
    g = Geo()
    g.add(frustum((0, 0, 0), (0, 0, 0.25), 0.55, 0.5, paint, n=8))
    for sx in (-1, 1):
        g.add(block(sx * 0.25 - 0.08, sx * 0.25 + 0.08, -0.4, 0.4, 0.2, 0.75, paint))
        # Gunner's seats and sights.
        g.add(block(sx * 0.75 - 0.18, sx * 0.75 + 0.18, -0.5, -0.1, 0.35, 0.5, OLIVE_DARK))
    # Shield: a front plate with angled wings.
    g.face2([(-0.6, 0.55, 0.35), (0.6, 0.55, 0.35), (0.6, 0.55, 1.25), (-0.6, 0.55, 1.25)], paint)
    for sx in (-1, 1):
        g.face2([(sx * 0.6, 0.55, 0.35), (sx * 1.0, 0.25, 0.35), (sx * 1.0, 0.25, 1.1), (sx * 0.6, 0.55, 1.25)], paint)
    return g


def s60_gun():
    """The 57 mm barrel with its muzzle brake and recoil cylinders (pivot at the trunnions)."""
    g = Geo()
    g.add(block(-0.16, 0.16, -0.9, 0.5, -0.14, 0.14, OLIVE_DARK))
    g.add(barrel(0.5, 4.2, 0.055))
    g.add(barrel(4.1, 4.45, 0.09, n=6))
    for sx in (-1, 1):
        g.add(barrel(0.0, 1.6, 0.04, x=sx * 0.12, z=0.17, paint=OLIVE_DARK))
    return g


def s60():
    root, lod0, lod1 = levels(S60, 16000)
    intact = mesh_obj("aaa_111_lod0_s1v0", s60_carriage(), lod0)
    mount = part("aaa_111_lod0_s1v0_part1", s60_mount(), intact, (0, 0, 0.85), TURRET_SLOT, 0)
    part("aaa_111_lod0_s1v0_part2", s60_gun(), mount, (0, 0.1, 0.8), GUN_SLOT, 1)
    w = s60_carriage(BURNT, BURNT).rotated_x(0.05)
    w.add(s60_mount(BURNT).rotated_z(-0.6).transformed(lambda p: p + mklib.v3((0, 0, 0.8))))
    w.add(s60_gun().recolour(BURNT).rotated_x(-0.12).rotated_z(-0.6).transformed(lambda p: p + mklib.v3((0, 0, 1.55))))
    mesh_obj("aaa_111_lod0_s1v1", w, lod0)
    far(lod1, "aaa_111", [(-1.2, 1.2, -2.0, 2.0, 0.1, 0.9), (-0.1, 0.1, 0.0, 4.4, 1.5, 1.8)])


# -------------------------------------------------------------------------------------- MANPADS


def soldier(kneel, paint=OLIVE):
    """A low-poly soldier facing +y, standing or kneeling, around his feet."""
    g = Geo()
    hip = 0.55 if kneel else 0.95
    if kneel:
        # Kneeling: one shin on the ground, the other leg up.
        g.add(block(-0.22, -0.04, -0.45, 0.05, 0.0, 0.16, paint))
        g.add(block(-0.22, -0.04, -0.05, 0.12, 0.0, hip, paint))
        g.add(block(0.04, 0.22, 0.15, 0.45, 0.0, 0.5, paint))
        g.add(block(0.04, 0.22, -0.05, 0.45, 0.42, hip, paint))
    else:
        for sx in (-1, 1):
            g.add(block(sx * 0.13 - 0.08, sx * 0.13 + 0.08, -0.08, 0.1, 0.0, hip, paint))
    g.add(block(-0.22, 0.22, -0.13, 0.13, hip, hip + 0.6, paint, taper=0.03))
    g.add(frustum((0, 0, hip + 0.6), (0, 0, hip + 0.86), 0.1, 0.11, SKIN, n=6))
    # Helmet.
    g.add(frustum((0, 0, hip + 0.86), (0, 0, hip + 0.98), 0.15, 0.1, OLIVE_DARK, n=6))
    return g


def launcher():
    """SA-14 tube with its grip stock and sight, the shooter's shoulder at the origin."""
    g = Geo()
    g.add(barrel(-0.45, 1.05, 0.045, paint=OLIVE_DARK))
    g.add(frustum((0, 1.05, 0), (0, 1.12, 0), 0.06, 0.05, OLIVE_DARK, n=6))
    g.add(block(-0.04, 0.04, 0.05, 0.3, -0.25, -0.04, STEEL))
    g.add(block(-0.12, -0.04, 0.3, 0.42, 0.03, 0.12, STEEL))
    return g


def manpads_position(burnt=False):
    g = sandbag_ring(1.6, 0.5, 9, gap=2.2)
    if burnt:
        g = g.recolour(BURNT)
    return g


def manpads():
    root, lod0, _ = levels(MANPADS, 32769, far_level=False)
    intact = mesh_obj("aaa_112_lod0_s1v0", manpads_position().add(
        soldier(False).rotated_z(2.6).transformed(lambda p: p + mklib.v3((-0.9, -0.6, 0)))
        .add(barrel(-0.5, 0.5, 0.045, x=0.0, z=0.0, paint=OLIVE_DARK).rotated_x(1.2).transformed(lambda p: p + mklib.v3((-0.95, -0.45, 1.2))))
    ), lod0)
    shooter = part("aaa_112_lod0_s1v0_part1", soldier(True), intact, (0.3, 0.2, 0), TURRET_SLOT, 0)
    part("aaa_112_lod0_s1v0_part2", launcher(), shooter, (0.16, -0.05, 1.32), GUN_SLOT, 1)
    mesh_obj("aaa_112_lod0_s1v1", manpads_position(True), lod0)


# -------------------------------------------------------------------------------------- common


def levels(index, near, far_level=True):
    """The root, its detailed level (within `near` model units) and a far level (boxes)."""
    name = "aaa_%03d" % index
    root = empty(name)
    root["attr"] = 0x01
    dists = [near, 32769] if far_level else [near]
    root["lod_dists"] = dists
    lod0 = empty(name + "_lod0", root)
    lod0["max_dist"] = near
    lod0["radius"] = int(8 * K)
    lod1 = None
    if far_level:
        lod1 = empty(name + "_lod1", root)
        lod1["max_dist"] = 32769
        lod1["radius"] = int(8 * K)
    return root, lod0, lod1


def far(lod, name, boxes):
    """Far level: a few boxes, olive, and black once destroyed."""
    for variant, paint in (("s1v0", OLIVE), ("s1v1", BURNT)):
        g = Geo()
        for b in boxes:
            g.add(block(*b, paint))
        mesh_obj("%s_lod1_%s" % (name, variant), g, lod)


def main():
    out = sys.argv[sys.argv.index("--") + 1]
    mklib.load_palette(os.path.join(ROOT, "assets", "palettes", "tod_ce.ron"))
    for o in list(bpy.data.objects):
        bpy.data.objects.remove(o, do_unlink=True)
    zsu()
    s60()
    manpads()
    bpy.ops.export_scene.gltf(
        filepath=out, export_format="GLB", export_extras=True, export_yup=True, export_texcoords=False,
        export_normals=False, export_materials="EXPORT", use_visible=False, export_animations=False,
        use_mesh_edges=True, use_mesh_vertices=True,
    )


main()
