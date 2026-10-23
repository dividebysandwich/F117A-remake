"""Rebuilds the UH-60A Black Hawk (`stflt_025`) in stflt.blend with
three detail levels, and saves it:

    blender -b assets-src/blend/stflt.blend --python-exit-code 1 --python tools/models/uh60.py
    tools/models/export.sh stflt

Proportions are taken from a three-view drawing (fuselage 15.3 m, rotor 16.4 m, hub 3.5 m above
the ground, wheel track 2.7 m, wheelbase 8.8 m). The origin is on the ground under the rotor
mast, so the wheels touch at z 0. Each level carries its own rotors (`_part1` main rotor about
the mast, built upside down so it turns anticlockwise, `_part2` tail rotor on the right of the pylon, canted 20 degrees; both on slot 17),
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

import heli  # noqa: E402
import mklib  # noqa: E402
from heli import ring  # noqa: E402
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
    (2.75, 0.58, 0.789, 1.815, 2.142, 2.253, 0.882, 1.189, 1.145, 0.982, 0.464),  # roof glazing ends (on the line between its neighbours)
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
# The roof over the pilots glazed back from the windshield's top, clear of their view up.
WINDSHIELD, ROOF, COCKPIT = 3, 4, 5
GLAZED = {WINDSHIELD: {2, 3, 4, 5, 6}, ROOF: {2, 3, 4, 5, 6}, COCKPIT: {2, 3, 5, 6}}

MAST = (0.0, 0.0, 3.47)
ROTOR_R, ROTOR_CHORD = 8.18, 0.53
TAIL_HUB = (0.42, -9.82, 3.85)
TAIL_R, TAIL_CHORD, TAIL_CANT = 1.68, 0.25, math.radians(20)
MAIN_WHEEL = (1.33, 1.06, 0.34)
TAIL_WHEEL = (0.0, -7.69, 0.21)


def side_x(y, z):
    return heli.side_x(STATIONS, y, z)


def side_panel(y0, y1, z0, z1, paint, g):
    heli.side_panel(STATIONS, y0, y1, z0, z1, paint, g)


def side_outline(pts, g):
    heli.side_outline(STATIONS, pts, SEAM, g)


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
    a, b = ring(STATIONS[WINDSHIELD]), ring(STATIONS[ROOF])
    g.line((0, a[4].y, a[4].z + 0.01), (0, b[4].y, b[4].z + 0.01), SEAM)
    # Wire strike cutter on the cockpit roof.
    g.line((0, 2.6, 2.36), (0, 3.15, 2.62), DARK.hi)


def body(level):
    g = Geo()
    if level == 0:
        g.add(heli.fuselage(STATIONS, BODY, GLASS, True, GLAZED))
        g.add(nacelles(True))
        details(g)
    elif level == 1:
        st = [STATIONS[i] for i in (0, 1, 3, 4, 5, 6, 8, 11, 12, 14, 15)]
        g.add(heli.fuselage(st, BODY, GLASS, True, {2: GLAZED[WINDSHIELD], 3: GLAZED[ROOF], 4: GLAZED[COCKPIT]}))
        g.add(nacelles(False))
    else:
        st = [STATIONS[i] for i in (1, 3, 4, 8, 11, 12, 15)]
        g.add(heli.fuselage(st, BODY, GLASS, False, {1: {1, 2, 3}}))
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


def build():
    root = bpy.data.objects[NAME]
    root["attr"] = 0x83
    levels = []
    for level, dist in enumerate(LOD_DISTS):
        # The main rotor turns anticlockwise seen from above: built upside down (see heli.flipped).
        main = heli.flipped(heli.rotor(level, 4, ROTOR_R, ROTOR_CHORD, BLADE, METAL, clockwise=False, phase=math.pi / 4))
        tail = heli.tail_rotor(level, 4 if level < 2 else 2, TAIL_R, TAIL_CHORD, BLADE, METAL)
        levels.append((dist, body(level), main, tail, (math.pi, 0.0, 0.0)))
    heli.build_levels(root, levels, K, MAST, TAIL_HUB, (0.0, -TAIL_CANT, 0.0), NAV, {RED: BEACONS})


mklib.load_palette(os.path.join(ROOT, "assets", "palettes", "tod_ce.ron"))
build()
bpy.ops.wm.save_mainfile()
