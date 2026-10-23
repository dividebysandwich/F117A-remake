"""Rebuilds the UH-60A Black Hawk (`stflt_025`, also drawn for the Hinds) in stflt.blend with
three detail levels, and saves it:

    blender -b assets-src/blend/stflt.blend --python-exit-code 1 --python tools/models/uh60.py
    tools/models/export.sh stflt

Proportions are taken from a three-view drawing (fuselage 15.3 m, rotor 16.4 m, hub 3.5 m above
the ground, wheel track 2.7 m, wheelbase 8.8 m). The origin is on the ground under the rotor
mast, so the wheels touch at z 0. Each level carries its own rotors (`_part1` main rotor about
the mast, `_part2` tail rotor on the right of the pylon, canted 20 degrees; both on slot 17),
navigation lights (`_s12v1`) and red beacons (`_s20v1`, top and bottom; anticollision.py
leaves this model alone). Built in metres and scaled to aircraft units (K per metre).
Model space: x right, y forward, z up.
"""

import math
import os
import sys

import bpy
from mathutils import Vector

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, HERE)

import mklib  # noqa: E402
from mklib import Geo, Ramp, frustum, loft, v3  # noqa: E402

NAME = "stflt_025"
K = 16.38
# max_dist of each level (the game draws them 4x further): about 600 m and 2.2 km.
LOD_DISTS = [2500, 9000, 32769]

BODY = Ramp(0xA7, 0xAE)
DECK = Ramp(0xA8, 0xAF)
GLASS = Ramp(144, 145)
WINDOW = 145
DARK = Ramp(0xEB, 0xEF)
METAL = Ramp(0xE8, 0xEC)
BLADE = Ramp(0xEC, 0xEF)
TYRE = 0xEF
SEAM = 0xAF
RED, GREEN = 147, 151

# Fuselage stations from the nose: y, then the heights of the belly, the lower and upper edge
# of the flat side, the roof shoulder and the dorsal line, then the half widths at those heights.
# Rings run belly corner, side (lower, upper), shoulder, top on each side.
STATIONS = [
    # y      zb    zl    zu    zs    zt     wb    wl    wu    ws    wt
    (4.52, 0.98, 1.00, 1.06, 1.08, 1.10, 0.05, 0.10, 0.10, 0.08, 0.05),
    (4.30, 0.78, 0.86, 1.22, 1.34, 1.40, 0.30, 0.55, 0.55, 0.45, 0.25),
    (3.95, 0.66, 0.76, 1.32, 1.46, 1.50, 0.55, 0.92, 0.92, 0.75, 0.35),
    (3.62, 0.61, 0.74, 1.40, 1.56, 1.60, 0.70, 1.08, 1.06, 0.88, 0.40),  # windshield base
    (3.05, 0.58, 0.78, 1.70, 2.05, 2.17, 0.85, 1.18, 1.14, 0.95, 0.45),  # windshield top
    (2.40, 0.58, 0.80, 1.95, 2.25, 2.35, 0.92, 1.20, 1.15, 1.02, 0.48),  # cockpit door post
    (1.70, 0.58, 0.80, 1.95, 2.25, 2.58, 0.92, 1.20, 1.15, 1.02, 0.50),
    (0.42, 0.58, 0.80, 1.95, 2.25, 2.87, 0.92, 1.20, 1.15, 1.02, 0.55),
    (-0.84, 0.58, 0.80, 1.95, 2.25, 2.92, 0.92, 1.20, 1.15, 1.02, 0.55),
    (-1.95, 0.58, 0.80, 1.95, 2.25, 2.84, 0.92, 1.20, 1.15, 1.02, 0.52),
    (-2.85, 0.58, 0.80, 1.95, 2.22, 2.62, 0.92, 1.20, 1.15, 1.00, 0.45),  # end of the cabin
    (-3.65, 0.63, 0.85, 1.90, 2.20, 2.46, 0.30, 0.41, 0.40, 0.32, 0.18),  # tail cone
    (-5.50, 0.78, 0.95, 1.75, 1.98, 2.11, 0.24, 0.32, 0.31, 0.25, 0.13),
    (-7.80, 0.98, 1.08, 1.52, 1.63, 1.69, 0.15, 0.21, 0.20, 0.16, 0.09),
    (-8.75, 1.05, 1.12, 1.40, 1.46, 1.50, 0.12, 0.17, 0.16, 0.13, 0.07),
]
# Segments (between station i and i+1) and ring quads that are glazed. Quads per ring:
# 0/8 belly chamfer, 1/7 side, 2/6 upper side, 3/5 roof slope, 4 top strip, 9 belly.
WINDSHIELD, COCKPIT = 3, 4
GLAZED = {WINDSHIELD: {2, 3, 4, 5, 6}, COCKPIT: {2, 3, 5, 6}}

MAST = (0.0, 0.0, 3.47)
ROTOR_R, ROTOR_CHORD = 8.18, 0.53
TAIL_HUB = (0.42, -9.82, 3.85)
TAIL_R, TAIL_CHORD, TAIL_CANT = 1.68, 0.25, math.radians(20)
MAIN_WHEEL = (1.33, 1.06, 0.34)
TAIL_WHEEL = (0.0, -7.69, 0.21)


def ring(s, full=True):
    """Cross-section points of a station: 10 (full) or 6 (coarse: belly corner, side, top)."""
    y, zb, zl, zu, zs, zt, wb, wl, wu, ws, wt = s
    if full:
        right = [(wb, zb), (wl, zl), (wu, zu), (ws, zs), (wt, zt)]
    else:
        right = [(wb, zb), ((wl + wu) / 2, (zl + zu) / 2), ((ws + wt) / 2, zt)]
    pts = right + [(-x, z) for x, z in reversed(right)]
    return [Vector((x, y, z)) for x, z in pts]


def fuselage(stations, full=True, glazed=None):
    """Loft through the stations; `glazed` maps segment index -> glazed quad indices."""
    g = Geo()
    rings = [ring(s, full) for s in stations]
    m = len(rings[0])
    for i in range(len(rings) - 1):
        a, b = rings[i], rings[i + 1]
        mid = Vector((0, (stations[i][0] + stations[i + 1][0]) / 2, (a[0].z + a[m // 2 - 1].z + b[0].z + b[m // 2 - 1].z) / 4))
        for k in range(m):
            k1 = (k + 1) % m
            paint = GLASS if glazed and k in glazed.get(i, ()) else BODY
            g.face([a[k], a[k1], b[k1], b[k]], paint, mid)
    for r, other in ((rings[0], rings[1]), (rings[-1], rings[-2])):
        g.face(r, BODY, sum(other, Vector()) / m)
    return g


def side_x(y, z):
    """Half width of the fuselage's flat side at (y, z), interpolated between the stations."""
    for s0, s1 in zip(STATIONS, STATIONS[1:]):
        if s1[0] <= y <= s0[0]:
            t = (s0[0] - y) / (s0[0] - s1[0])
            s = [a + (b - a) * t for a, b in zip(s0, s1)]
            _, _, zl, zu, _, _, _, wl, wu, _, _ = s
            return wl + (wu - wl) * min(max((z - zl) / (zu - zl), 0.0), 1.0)
    raise ValueError(y)


def side_panel(y0, y1, z0, z1, paint, g, lift=0.012):
    """A window on both flat sides of the cabin (a face just outside the skin)."""
    for sx in (-1, 1):
        pts = [(sx * (side_x(y, z) + lift), y, z) for y, z in ((y0, z0), (y1, z0), (y1, z1), (y0, z1))]
        g.face(pts, paint, (0, (y0 + y1) / 2, (z0 + z1) / 2))


def side_outline(pts, g, lift=0.01):
    """Panel seam lines on both sides through (y, z) points (closed)."""
    for sx in (-1, 1):
        p = [(sx * (side_x(y, z) + lift), y, z) for y, z in pts]
        for a, b in zip(p, p[1:] + p[:1]):
            g.line(a, b, SEAM)


def nacelles(full=True):
    g = Geo()
    if full:
        st = [(0.45, 2.52, 0.18, 0.20), (0.30, 2.52, 0.30, 0.31), (-1.60, 2.55, 0.32, 0.33), (-2.60, 2.60, 0.27, 0.27), (-3.00, 2.62, 0.17, 0.17)]
        paints = [DARK, DECK, DECK, DARK]
        n = 8
    else:
        st = [(0.35, 2.52, 0.28, 0.30), (-1.60, 2.55, 0.32, 0.33), (-3.00, 2.62, 0.18, 0.18)]
        paints = [DECK, DECK]
        n = 6
    for sx in (-1, 1):
        g.add(loft(st, DECK, n=n, x=sx * 0.92, paints=paints))
    return g


def pylon():
    """Swept tail pylon: hexagonal airfoil from the tail cone up to the tail rotor gearbox."""

    def section(le, te, t):
        le, te = v3(le), v3(te)
        a, b = le + (te - le) * 0.25, le + (te - le) * 0.7
        return [le, a + Vector((t, 0, 0)), b + Vector((t * 0.7, 0, 0)), te, b - Vector((t * 0.7, 0, 0)), a - Vector((t, 0, 0))]

    return mklib.tube([section((0, -7.55, 1.62), (0, -8.75, 1.00), 0.17), section((0, -9.62, 3.92), (0, -10.58, 3.70), 0.13)], BODY)


def stabilator(t=0.08):
    return mklib.wing((0, -8.80, 1.72), 1.0, (2.16, -8.86, 1.72), 0.92, BODY, t=t)


def blade_rings(d, c, up, stations):
    """Diamond sections (leading edge, top, trailing edge, bottom) of a blade along direction
    `d`; `c` points to the leading edge. Stations: (radius, chord, sweep aft, rise, thickness)."""
    rings = []
    for r, chord, sweep, rise, th in stations:
        p = d * r + up * rise - c * sweep
        le, te = p + c * chord * 0.25, p - c * chord * 0.75
        mid = p - c * chord * 0.05
        rings.append([le, mid + up * th / 2, te, mid - up * th / 2])
    return rings


def main_rotor(level):
    """Main rotor about its hub (the part origin): four blades, turning anticlockwise from above."""
    g = Geo()
    up = Vector((0, 0, 1))
    cone = math.tan(math.radians(1.5))
    if level == 0:
        g.add(frustum((0, 0, -0.10), (0, 0, 0.10), 0.36, 0.30, METAL, n=8))
        g.add(frustum((0, 0, 0.10), (0, 0, 0.24), 0.12, 0.08, METAL, n=6))
    elif level == 1:
        g.add(frustum((0, 0, -0.08), (0, 0, 0.08), 0.34, 0.30, METAL, n=6))
    for k in range(4):
        a = math.radians(45 + 90 * k)
        d = Vector((math.cos(a), math.sin(a), 0))
        c = Vector((-math.sin(a), math.cos(a), 0))
        if level == 2:
            # One flat double-sided sheet per blade.
            p0, p1 = d * 0.3, d * ROTOR_R + up * ROTOR_R * cone
            g.face2([p0 + c * 0.13, p1 + c * 0.13, p1 - c * 0.4, p0 - c * 0.4], BLADE)
            continue
        if level == 0:
            # Hub arm (spindle and damper) out to the blade cuff.
            g.add(frustum(d * 0.30, d * 0.85, 0.09, 0.07, METAL, n=4))
            st = [(0.85, 0.30, 0.0, 0.0, 0.10), (1.6, ROTOR_CHORD, 0.0, 0.02, 0.07), (ROTOR_R - 0.55, ROTOR_CHORD, 0.0, (ROTOR_R - 0.55) * cone, 0.06),
                  (ROTOR_R, 0.24, 0.20, ROTOR_R * cone, 0.03)]
        else:
            st = [(0.35, 0.35, 0.0, 0.0, 0.08), (ROTOR_R - 0.5, ROTOR_CHORD, 0.0, (ROTOR_R - 0.5) * cone, 0.06), (ROTOR_R, 0.25, 0.2, ROTOR_R * cone, 0.03)]
        g.add(mklib.tube(blade_rings(d, c, up, st), BLADE))
    return g


def tail_rotor(level):
    """Tail rotor in its own frame (spins about local x): four blades in the y-z plane."""
    g = Geo()
    if level == 0:
        g.add(frustum((-0.05, 0, 0), (0.16, 0, 0), 0.12, 0.08, METAL, n=6))
    side = Vector((1, 0, 0))
    for k in range(4 if level < 2 else 2):
        a = math.radians(45 + 90 * k)
        d = Vector((0, math.cos(a), math.sin(a)))
        c = Vector((0, -math.sin(a), math.cos(a)))
        if level == 0:
            st = [(0.15, 0.18, 0.0, 0.0, 0.06), (TAIL_R - 0.1, TAIL_CHORD, 0.0, 0.0, 0.04), (TAIL_R, TAIL_CHORD * 0.8, 0.0, 0.0, 0.03)]
            g.add(mklib.tube(blade_rings(d, c, side, st), BLADE))
        else:
            # A flat double-sided sheet per blade (a pair per sheet on the coarsest level).
            r0 = 0.1 if level == 1 else -TAIL_R
            p0, p1 = d * r0, d * TAIL_R
            g.face2([p0 + c * 0.08, p1 + c * 0.08, p1 - c * 0.17, p0 - c * 0.17], BLADE)
    return g


def gear(full=True):
    g = Geo()
    n = 8 if full else 6
    x, y, z = MAIN_WHEEL
    for sx in (-1, 1):
        g.add(mklib.wheel((sx * x, y, z), z, 0.22, TYRE, n=n))
        if full:
            g.add(frustum((sx * 1.15, 2.63, 0.80), (sx * (x - 0.05), y + 0.08, z + 0.03), 0.06, 0.07, DARK, n=4))
            g.add(frustum((sx * (x - 0.05), y - 0.02, z + 0.05), (sx * 1.10, 0.92, 0.86), 0.07, 0.06, DARK, n=4))
    x, y, z = TAIL_WHEEL
    g.add(mklib.wheel(TAIL_WHEEL, z, 0.12, TYRE, n=n))
    if full:
        g.add(frustum((0, y + 0.02, z + 0.05), (0, -7.10, 0.96), 0.05, 0.06, DARK, n=4))
    return g


def upper_deck(full=True):
    """Rotor mast, transmission fairing hump and the APU exhaust behind it."""
    g = Geo()
    mx, my, mz = MAST
    g.add(frustum((mx, my, 2.80), (mx, my, 3.22), 0.46, 0.32, METAL if full else DECK, n=8 if full else 6))
    if full:
        g.add(frustum((mx, my, 3.22), (mx, my, mz - 0.08), 0.15, 0.15, METAL, n=6))
        g.add(mklib.block(-0.22, 0.22, -1.78, -1.25, 2.70, 3.08, DECK, taper=0.04, taper_y=0.06))
    return g


def tail_gearbox(full=True):
    """Fairing over the tail rotor gearbox, on the right at the top of the pylon."""
    hx, hy, hz = TAIL_HUB
    return mklib.block(-0.10, 0.28, hy - 0.45, hy + 0.30, hz - 0.30, hz + 0.12, BODY, taper=0.04 if full else 0.0)


def details(g):
    """Cabin windows, door outlines and windshield frames (level 0 only)."""
    # Gunner windows, the two windows of the sliding cabin door.
    for y0, y1 in ((1.85, 1.61), (1.52, 1.25), (0.68, 0.08), (-0.13, -0.78)):
        side_panel(y0, y1, 1.22, 1.88, WINDOW, g)
    # Cockpit door, gunner door, cabin door with its rail.
    side_outline([(3.05, 0.78), (2.40, 0.70), (2.40, 2.20), (3.05, 1.98)], g)
    side_outline([(2.35, 0.70), (0.95, 0.70), (0.95, 2.05), (2.35, 2.05)], g)
    side_outline([(0.82, 0.66), (-0.95, 0.66), (-0.95, 2.02), (0.82, 2.02)], g)
    for sx in (-1, 1):
        g.line((sx * (side_x(0.9, 2.08) + 0.01), 0.9, 2.08), (sx * (side_x(-2.6, 2.08) + 0.01), -2.6, 2.08), SEAM)
    # Windshield centre post and the frame between windshield and side window.
    a, b = ring(STATIONS[WINDSHIELD]), ring(STATIONS[COCKPIT])
    g.line((0, a[4].y, a[4].z + 0.01), (0, b[4].y, b[4].z + 0.01), SEAM)
    # Wire strike cutter on the cockpit roof.
    g.line((0, 2.6, 2.36), (0, 3.15, 2.62), DARK.hi)


def body(level):
    g = Geo()
    if level == 0:
        g.add(fuselage(STATIONS, True, GLAZED))
        g.add(nacelles(True))
        details(g)
    elif level == 1:
        st = [STATIONS[i] for i in (0, 1, 3, 4, 5, 7, 10, 11, 13, 14)]
        g.add(fuselage(st, True, {2: GLAZED[WINDSHIELD], 3: GLAZED[COCKPIT]}))
        g.add(nacelles(False))
    else:
        st = [STATIONS[i] for i in (1, 3, 4, 7, 10, 11, 14)]
        g.add(fuselage(st, False, {1: {1, 2, 3}}))
    g.add(pylon())
    g.add(stabilator(0.08 if level < 2 else 0.05))
    g.add(upper_deck(level == 0))
    g.add(tail_gearbox(level == 0))
    if level < 2:
        g.add(gear(level == 0))
    return g


# Lights in metres: navigation lights on the aft roof corners of the cabin, beacons on the
# transmission fairing and under the belly.
NAV = {RED: [(-1.10, -2.70, 2.32)], GREEN: [(1.10, -2.70, 2.32)]}
BEACONS = [(0.0, -1.55, 3.16), (0.0, -1.20, 0.49)]


def link(parent, name, data):
    ob = mklib.link_child(parent, name, data)
    return ob


def lights(lod, slot, groups):
    group = bpy.data.objects.new(f"{lod.name}_s{slot}v1", None)
    group["conds"] = [[slot, 1]]
    for col in lod.users_collection:
        col.objects.link(group)
    group.parent = lod
    for c, pts in groups.items():
        name = f"{group.name}_dots_pal_{c:03}"
        link(group, name, mklib.points_mesh(name, [v3(p) * K for p in pts], c))


def mesh_with_lines(parent, name, geo, loc=(0, 0, 0)):
    ob = link(parent, name, mklib.faces_mesh(name, geo))
    ob.location = loc
    for c, segs in sorted(geo.lines.items()):
        ln = f"{name}_lines_pal_{c:03}"
        link(ob, ln, mklib.lines_mesh(ln, segs, c))
    return ob


def radius(*geos):
    return math.ceil(max(p.length for g in geos for f, _ in g.faces for p in f))


def build():
    root = bpy.data.objects[NAME]
    for c in list(root.children):
        mklib.delete_tree(c)
    root["attr"] = 0x83
    root["lod_dists"] = LOD_DISTS
    for level, dist in enumerate(LOD_DISTS):
        geo = body(level).scaled(K)
        lod = mesh_with_lines(root, f"{NAME}_lod{level}", geo)
        lod["max_dist"] = dist
        hub = v3(MAST) * K
        rotor = main_rotor(level).scaled(K)
        part = mesh_with_lines(lod, f"{lod.name}_part1", rotor, hub)
        part["state_slot"] = 17
        part["axis"] = 0
        tail = tail_rotor(level).scaled(K)
        part = mesh_with_lines(lod, f"{lod.name}_part2", tail, v3(TAIL_HUB) * K)
        part.rotation_euler = (0.0, -TAIL_CANT, 0.0)
        part["state_slot"] = 17
        part["axis"] = 1
        rotor_world = rotor.transformed(lambda p: p + hub)
        lod["radius"] = radius(geo, rotor_world)
        lights(lod, 12, NAV)
        lights(lod, 20, {RED: BEACONS})
        faces = len(geo.faces) + len(rotor.faces) + len(tail.faces)
        print(f"{lod.name}: {faces} faces, radius {lod['radius']}")


mklib.load_palette(os.path.join(ROOT, "assets", "palettes", "tod_ce.ron"))
build()
bpy.ops.wm.save_mainfile()
