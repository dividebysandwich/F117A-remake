"""Builds the AH-1Z Viper (`stflt_043`) in stflt.blend with three detail levels, and saves it:

    blender -b assets-src/blend/stflt.blend --python-exit-code 1 --python tools/models/ah1z.py
    tools/models/export.sh stflt

A narrow tandem fuselage: the gunner in front, the pilot behind and higher under a stepped
canopy whose side panels reach down to the elbows, a sensor turret on the nose and the three-
barrel gun under the chin, two engines on the shoulders with upturned exhausts, stub wings
with two pylons each and an AIM-9 rail on the tip, skids, and a tail boom ending in a swept fin.
Proportions are taken from a three-view drawing (fuselage 14.6 m, rotor 14.6 m, hub 3.8 m above
the ground, wing span 4.7 m, skid track 2.3 m). The origin is on the ground under the rotor
mast, so the skids touch at z 0. Each level carries its own rotors (`_part1` four-blade main
rotor about the mast, built upside down so it turns anticlockwise, `_part2` four-blade tail
rotor on the left of the fin; both on slot 17), navigation lights (`_s12v1`) and red beacons
(`_s20v1`). Marine light grey. Built in metres and scaled to aircraft units (K per metre).
Model space: x right, y forward, z up.
"""

import math
import os
import sys

import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, HERE)

import heli  # noqa: E402
import mklib  # noqa: E402
from heli import ring  # noqa: E402
from mklib import Geo, Ramp, frustum, loft  # noqa: E402

NAME = "stflt_043"
K = 16.38
# max_dist of each level (the game draws them 4x further): about 600 m and 2.2 km.
LOD_DISTS = [2500, 9000, 32769]

BODY = Ramp(0xE1, 0xE7)
DECK = Ramp(0xE2, 0xE8)
GLASS = Ramp(144, 145)
DARK = Ramp(0xEB, 0xEF)
METAL = Ramp(0xE8, 0xEC)
BLADE = Ramp(0xEC, 0xEF)
SENSOR = Ramp(0xE9, 0xEE)
SEAM = 0xE9
RED, GREEN = 147, 151

# Fuselage stations from the nose: y, then the heights of the belly, the lower and upper edge
# of the flat side, the roof shoulder and the dorsal line, then the half widths at those heights.
# Over the cockpits the flat side ends at the canopy sill, low, so the side panels (quads 2/6)
# run from about the pilots' elbows up to the roof.
STATIONS = [
    # y      zb    zl    zu    zs    zt     wb    wl    wu    ws    wt
    (5.05, 1.16, 1.22, 1.40, 1.48, 1.52, 0.06, 0.10, 0.10, 0.08, 0.05),
    (4.80, 0.94, 1.02, 1.52, 1.64, 1.70, 0.24, 0.36, 0.34, 0.26, 0.14),
    (4.35, 0.82, 0.92, 1.56, 1.78, 1.86, 0.34, 0.45, 0.43, 0.32, 0.17),
    (4.02, 0.79, 0.90, 1.40, 1.70, 1.76, 0.37, 0.47, 0.46, 0.33, 0.16),  # windshield base
    (3.50, 0.77, 0.88, 1.34, 1.94, 2.02, 0.40, 0.49, 0.48, 0.37, 0.19),  # windshield top
    (2.70, 0.76, 0.88, 1.38, 1.98, 2.06, 0.42, 0.50, 0.49, 0.38, 0.20),  # gunner canopy aft
    (2.30, 0.76, 0.88, 1.46, 2.40, 2.50, 0.43, 0.51, 0.50, 0.38, 0.20),  # pilot windshield top
    (1.10, 0.76, 0.88, 1.52, 2.42, 2.52, 0.45, 0.53, 0.52, 0.39, 0.21),  # pilot canopy aft
    (0.60, 0.76, 0.88, 1.60, 2.32, 2.62, 0.48, 0.56, 0.54, 0.44, 0.30),
    (-0.80, 0.78, 0.90, 1.62, 2.30, 2.66, 0.50, 0.57, 0.55, 0.46, 0.32),
    (-2.50, 0.86, 0.96, 1.58, 2.14, 2.40, 0.42, 0.49, 0.47, 0.38, 0.25),
    (-3.40, 1.08, 1.16, 1.66, 1.96, 2.06, 0.22, 0.28, 0.27, 0.22, 0.14),  # tail boom
    (-6.50, 1.36, 1.42, 1.76, 1.88, 1.93, 0.14, 0.18, 0.17, 0.14, 0.08),
    (-8.60, 1.52, 1.56, 1.80, 1.88, 1.91, 0.10, 0.13, 0.12, 0.10, 0.06),
]
# Glazed segments (between station i and i+1): the gunner's windshield and canopy, the
# pilot's windshield (the step) and canopy. Quads 2..6: side panels, roof slopes, top.
FRONT_SHIELD, FRONT, STEP, REAR = 3, 4, 5, 6
CANOPY = {2, 3, 4, 5, 6}
GLAZED = {FRONT_SHIELD: CANOPY, FRONT: CANOPY, STEP: CANOPY, REAR: CANOPY}

MAST = (0.0, 0.0, 3.80)
ROTOR_R, ROTOR_CHORD = 7.32, 0.58
TAIL_HUB = (-0.32, -9.10, 2.66)
TAIL_R, TAIL_CHORD = 1.45, 0.25
# Skids: half track, height of the tube, fore and aft ends.
SKID_X, SKID_Z, SKID_Y = 1.15, 0.06, (2.40, -2.30)
# Stub wings: root and tip leading edges, chords; the tip rail; the pylons' spans out.
WING_ROOT, WING_ROOT_CHORD = (0.50, 0.45, 1.38), 1.50
WING_TIP, WING_TIP_CHORD = (2.30, 0.30, 1.38), 1.20
PYLONS = (1.05, 1.75)
# Turret, gun muzzle, sensor ball.
TURRET = (0.0, 3.90, 0.62)
MUZZLE = (0.0, 5.30, 0.58)
SENSOR_BALL = (0.0, 5.20, 1.30)


def side_x(y, z):
    return heli.side_x(STATIONS, y, z)


def side_outline(pts, g):
    heli.side_outline(STATIONS, pts, SEAM, g)


def nacelles(level):
    """Engines on the shoulders, their upturned exhaust ducts and the intakes."""
    g = Geo()
    n = 8 if level == 0 else 6
    st = [(0.45, 2.20, 0.22, 0.22), (0.25, 2.20, 0.36, 0.37), (-1.80, 2.20, 0.38, 0.39), (-2.50, 2.24, 0.32, 0.32)]
    if level == 2:
        st = [st[1], st[3]]
    for sx in (-1, 1):
        g.add(loft(st, DECK, n=n, x=sx * 0.66, paints=[DARK] + [DECK] * (len(st) - 2) if level == 0 else None))
        if level < 2:
            # Exhaust: turned up and out, its mouth dark.
            g.add(frustum((sx * 0.68, -2.45, 2.24), (sx * 0.90, -3.20, 2.62), 0.31, 0.28, DECK, n=n))
            if level == 0:
                g.add(frustum((sx * 0.90, -3.20, 2.62), (sx * 0.92, -3.24, 2.64), 0.25, 0.25, DARK, n=n))
    return g


def upper_deck(level):
    """Transmission fairing, the tall mast and the hub cap."""
    g = Geo()
    mx, my, mz = MAST
    n = 8 if level == 0 else 6
    g.add(frustum((mx, my, 2.55), (mx, my, 3.05), 0.42, 0.26, DECK, n=n))
    if level < 2:
        g.add(frustum((mx, my, 3.05), (mx, my, mz - 0.10), 0.13, 0.13, METAL, n=6))
    if level == 0:
        g.add(mklib.block(-0.18, 0.18, -1.30, -0.60, 2.55, 2.86, DECK, taper=0.04, taper_y=0.06))
    return g


def nose(level):
    """The sensor turret on the nose and the chin gun turret with its three barrels."""
    g = Geo()
    n = 8 if level == 0 else 6
    x, y, z = SENSOR_BALL
    g.add(frustum((x, y - 0.25, z), (x, y + 0.05, z), 0.24, 0.34, SENSOR, n=n))
    g.add(frustum((x, y + 0.05, z), (x, y + 0.24, z), 0.34, 0.22, SENSOR, n=n))
    if level == 0:
        g.add(frustum((x, y + 0.24, z), (x, y + 0.26, z), 0.16, 0.16, DARK, n=6))
    x, y, z = TURRET
    g.add(frustum((x, y, z + 0.18), (x, y, z - 0.10), 0.28, 0.22, METAL, n=n))
    mx, my, mz = MUZZLE
    if level == 0:
        for k in range(3):
            a = math.pi / 2 + 2 * math.pi * k / 3
            dx, dz = 0.05 * math.cos(a), 0.05 * math.sin(a)
            g.add(frustum((x + dx, y + 0.15, z + dz), (mx + dx, my, mz + dz), 0.025, 0.025, DARK, n=4))
        g.add(frustum((x, y + 0.15, z), (x, y + 0.80, z - 0.03), 0.12, 0.10, METAL, n=6))
    else:
        g.add(frustum((x, y + 0.15, z), (mx, my, mz), 0.08, 0.06, DARK, n=4))
    return g


def wings(level):
    """Stub wings with two pylons each, the tip rails for the Sidewinders."""
    g = Geo()
    g.add(mklib.wing(WING_ROOT, WING_ROOT_CHORD, WING_TIP, WING_TIP_CHORD, BODY, t=0.14 if level < 2 else 0.10))
    if level == 2:
        return g
    ty = WING_TIP[1]
    for sx in (-1, 1):
        # Tip plate and rail.
        g.add(mklib.block(sx * 2.30 - 0.04, sx * 2.30 + 0.04, ty - 1.15, ty - 0.05, 1.10, 1.46, BODY))
        g.add(mklib.block(sx * 2.30 - 0.03, sx * 2.30 + 0.03, ty - 1.30, ty + 0.35, 1.04, 1.12, METAL))
        if level == 0:
            for px in PYLONS:
                t = (px - WING_ROOT[0]) / (WING_TIP[0] - WING_ROOT[0])
                le = WING_ROOT[1] + (WING_TIP[1] - WING_ROOT[1]) * t
                g.add(mklib.block(sx * px - 0.05, sx * px + 0.05, le - 1.05, le - 0.15, 1.10, 1.32, METAL, taper_y=0.1))
    return g


def gear(level):
    """Skids on two arched cross tubes."""
    g = Geo()
    n = 6 if level == 0 else 4
    y0, y1 = SKID_Y
    for sx in (-1, 1):
        x = sx * SKID_X
        g.add(frustum((x, y1, SKID_Z), (x, y0, SKID_Z), 0.05, 0.05, METAL, n=n))
        if level == 0:
            g.add(frustum((x, y0, SKID_Z), (x, y0 + 0.35, SKID_Z + 0.22), 0.05, 0.05, METAL, n=n))
        for y in (1.30, -1.10):
            g.add(frustum((sx * 0.40, y, 0.82), (sx * 0.95, y, 0.40), 0.05, 0.05, METAL, n=n))
            g.add(frustum((sx * 0.95, y, 0.40), (x, y, SKID_Z), 0.05, 0.05, METAL, n=n))
    return g


def fin(level):
    """Swept fin at the end of the boom with its tail skid, the gearbox on the left at the top
    and the horizontal stabiliser with end plates."""

    def section(le, te, t):
        le, te = mklib.v3(le), mklib.v3(te)
        a, b = le + (te - le) * 0.25, le + (te - le) * 0.7
        return [le, a + mklib.v3((t, 0, 0)), b + mklib.v3((t * 0.7, 0, 0)), te, b - mklib.v3((t * 0.7, 0, 0)), a - mklib.v3((t, 0, 0))]

    g = Geo()
    g.add(mklib.tube([section((0, -8.10, 1.62), (0, -9.25, 1.40), 0.13), section((0, -9.00, 3.30), (0, -9.75, 3.20), 0.09)], BODY))
    if level < 2:
        g.add(mklib.tube([section((0, -8.55, 1.42), (0, -9.25, 1.40), 0.08), section((0, -8.95, 0.95), (0, -9.25, 0.95), 0.05)], BODY))
        hx, hy, hz = TAIL_HUB
        g.add(mklib.block(hx, -0.06, hy - 0.30, hy + 0.25, hz - 0.22, hz + 0.18, BODY, taper=0.03 if level == 0 else 0.0))
    g.add(mklib.wing((0.15, -6.40, 1.80), 0.90, (1.60, -6.55, 1.80), 0.80, BODY, t=0.10))
    if level == 0:
        for sx in (-1, 1):
            g.add(mklib.block(sx * 1.60 - 0.03, sx * 1.60 + 0.03, -7.35, -6.60, 1.62, 2.10, BODY))
    return g


def details(g):
    """Canopy frames, access panel seams and the wire cutters (level 0 only)."""
    # The frame between the gunner's and the pilot's canopy, and the canopy centre line.
    for i in (FRONT_SHIELD, STEP, REAR + 1):
        r = ring(STATIONS[i])
        for a, b in zip(r[2:7], r[3:8]):
            g.line(a, b, DARK.hi)
    a, b = ring(STATIONS[FRONT_SHIELD]), ring(STATIONS[STEP])
    g.line((0, a[4].y, a[4].z + 0.01), (0, b[4].y, b[4].z + 0.01), DARK.hi)
    # Avionics bays under the canopy and the engine access panels.
    side_outline([(3.95, 0.95), (2.80, 0.95), (2.80, 1.30), (3.95, 1.30)], g)
    side_outline([(2.10, 0.95), (1.20, 0.95), (1.20, 1.40), (2.10, 1.40)], g)
    side_outline([(0.40, 1.00), (-1.60, 1.00), (-1.60, 1.50), (0.40, 1.50)], g)
    # Wire strike cutters above and below the nose.
    g.line((0, 4.30, 1.88), (0, 4.70, 2.05), DARK.hi)
    g.line((0, 4.60, 0.88), (0, 4.95, 0.75), DARK.hi)


def body(level):
    g = Geo()
    if level == 0:
        g.add(heli.fuselage(STATIONS, BODY, GLASS, True, GLAZED))
        details(g)
    elif level == 1:
        st = [STATIONS[i] for i in (0, 1, 3, 4, 5, 6, 7, 8, 10, 11, 13)]
        g.add(heli.fuselage(st, BODY, GLASS, True, {2: CANOPY, 3: CANOPY, 4: CANOPY, 5: CANOPY}))
    else:
        st = [STATIONS[i] for i in (1, 3, 5, 7, 9, 11, 13)]
        g.add(heli.fuselage(st, BODY, GLASS, False, {1: {1, 2, 3}, 2: {1, 2, 3}}))
    g.add(nacelles(level))
    g.add(upper_deck(level))
    g.add(nose(level))
    g.add(wings(level))
    g.add(fin(level))
    if level < 2:
        g.add(gear(level))
    return g


# Lights in metres: navigation lights on the wingtip plates, beacons on the fairing behind the
# mast and under the belly.
NAV = {RED: [(-2.36, -0.40, 1.46)], GREEN: [(2.36, -0.40, 1.46)]}
BEACONS = [(0.0, -1.35, 2.88), (0.0, -0.60, 0.74)]


def root_object():
    root = bpy.data.objects.get(NAME)
    if root is None:
        root = bpy.data.objects.new(NAME, None)
        for col in bpy.data.objects["stflt_025"].users_collection:
            col.objects.link(root)
    return root


def build():
    root = root_object()
    root["attr"] = 0x83
    levels = []
    for level, dist in enumerate(LOD_DISTS):
        # The main rotor turns anticlockwise seen from above: built upside down (see heli.flipped).
        main = heli.flipped(heli.rotor(level, 4, ROTOR_R, ROTOR_CHORD, BLADE, METAL, clockwise=False, phase=math.pi / 4))
        tail = heli.tail_rotor(level, 4 if level < 2 else 2, TAIL_R, TAIL_CHORD, BLADE, METAL)
        levels.append((dist, body(level), main, tail, (math.pi, 0.0, 0.0)))
    heli.build_levels(root, levels, K, MAST, TAIL_HUB, (0.0, 0.0, 0.0), NAV, {RED: BEACONS})


mklib.load_palette(os.path.join(ROOT, "assets", "palettes", "tod_ce.ron"))
build()
bpy.ops.wm.save_mainfile()
