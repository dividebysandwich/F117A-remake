"""The Mi-24D Hind (`drones_102`) with three detail levels: tandem gunner and pilot cockpits
under bubble canopies, chin gun turret, two engines on the roof, stub wings with anhedral, four
rocket pods and the wingtip missile rails, a five-blade main rotor turning clockwise from above
and a three-blade tail rotor on the left of the fin. Called by drones.py, which exports it with
the Gerans into drones.glb.

Proportions are taken from three-view drawings (fuselage 17.5 m, rotor 17.3 m, hub 4.5 m above
the ground, wing span 6.7 m, main wheel track 3 m). The origin is on the ground under the rotor
mast, so the wheels touch at z 0. Two-tone green camouflage over a pale blue-grey underside.
Built in metres and scaled to aircraft units (K per metre). Model space: x right, y forward,
z up.
"""

import math

import bpy
from mathutils import Vector

import heli
import mklib
from mklib import Geo, Ramp, frustum, loft, v3

K = 16.38
# max_dist of each level (the game draws them 4x further): about 600 m and 2.2 km.
LOD_DISTS = [2500, 9000, 32769]

# Faces built in CAMO are repainted by `camouflage` from their position and normal.
CAMO = 0xFF
GREEN_LIGHT = Ramp(0xA4, 0xA9)
GREEN_DARK = Ramp(0xAA, 0xAF)
BELLY = Ramp(0xF1, 0xF4)
GLASS = Ramp(144, 145)
WINDOW = 145
DARK = Ramp(0xEB, 0xEF)
METAL = Ramp(0xE8, 0xEC)
POD = Ramp(0xE6, 0xEB)
BLADE = Ramp(0xEC, 0xEF)
TYRE = 0xEF
SEAM = 0xAF
RED, GREEN = 147, 151

# Fuselage stations (see heli.py): nose glazing, gunner canopy, pilot canopy (raised behind
# it), cabin, then the tail boom rising to the fin.
STATIONS = [
    # y      zb    zl    zu    zs    zt     wb    wl    wu    ws    wt
    (6.25, 1.10, 1.15, 1.38, 1.46, 1.50, 0.10, 0.18, 0.18, 0.14, 0.08),
    (5.90, 0.92, 1.00, 1.50, 1.72, 1.82, 0.32, 0.48, 0.47, 0.38, 0.18),
    (5.30, 0.92, 1.00, 1.58, 1.95, 2.10, 0.45, 0.65, 0.62, 0.48, 0.24),
    (4.45, 0.95, 1.05, 1.65, 2.05, 2.25, 0.50, 0.75, 0.72, 0.55, 0.28),  # gunner canopy top
    (3.95, 0.97, 1.05, 1.70, 2.00, 2.12, 0.55, 0.80, 0.77, 0.60, 0.32),  # gunner canopy rear
    (3.55, 0.98, 1.05, 1.75, 2.25, 2.45, 0.57, 0.83, 0.80, 0.60, 0.30),
    (3.05, 0.98, 1.05, 1.80, 2.50, 2.80, 0.58, 0.85, 0.82, 0.60, 0.30),  # pilot canopy top
    (2.45, 1.00, 1.08, 1.95, 2.45, 2.62, 0.60, 0.88, 0.85, 0.70, 0.45),  # pilot canopy rear
    (1.50, 1.00, 1.10, 2.20, 2.55, 2.68, 0.60, 0.90, 0.88, 0.75, 0.50),
    (-0.50, 1.00, 1.10, 2.20, 2.55, 2.70, 0.60, 0.90, 0.88, 0.75, 0.50),
    (-2.20, 1.05, 1.15, 2.25, 2.58, 2.70, 0.58, 0.88, 0.86, 0.73, 0.48),
    (-3.00, 1.30, 1.45, 2.40, 2.65, 2.75, 0.45, 0.72, 0.70, 0.60, 0.42),
    (-3.85, 1.67, 1.80, 3.10, 3.35, 3.45, 0.32, 0.50, 0.48, 0.40, 0.28),  # tail boom
    (-6.50, 2.20, 2.30, 3.25, 3.40, 3.48, 0.22, 0.34, 0.33, 0.28, 0.18),
    (-9.20, 2.75, 2.82, 3.35, 3.45, 3.50, 0.14, 0.22, 0.21, 0.18, 0.12),
    (-9.80, 2.95, 3.00, 3.30, 3.38, 3.42, 0.10, 0.15, 0.14, 0.12, 0.08),
]
# Glazed lengths (gunner station with the nose, pilot) and the ring quads they glaze.
CANOPIES = [(6.25, 3.95), (3.55, 2.45)]
CANOPY_QUADS = {2, 3, 4, 5, 6}

MAST = (0.0, 0.0, 4.50)
ROTOR_R, ROTOR_CHORD = 8.65, 0.58
TAIL_HUB = (-0.42, -11.10, 4.88)
TAIL_R, TAIL_CHORD = 1.95, 0.30
# Stub wing: root and tip leading edges (x, y, z) and chords.
WING_ROOT, WING_ROOT_CHORD = (0.85, -0.50, 2.45), 1.40
WING_TIP, WING_TIP_CHORD = (3.33, -0.95, 1.85), 0.90
PYLONS = (1.75, 2.55)
NOSE_WHEEL = (0.20, 3.27, 0.28)
MAIN_WHEEL = (1.52, -1.20, 0.40)


def camouflage(g):
    """Repaints the CAMO faces: pale underside, irregular light and dark green patches."""
    faces = []
    for pts, c in g.faces:
        if c == CAMO:
            n = mklib.normal(pts)
            p = sum(pts, Vector()) / len(pts)
            # Below the cabin roof: the engines' undersides stay green.
            if n.z < -0.55 and p.z < 2.6:
                paint = BELLY
            else:
                v = math.sin(1.3 * p.y + 2.1 * p.z) + 0.8 * math.sin(0.7 * p.y - 1.9 * p.z + 2.3 * p.x + 1.0)
                paint = GREEN_LIGHT if v > 0.35 else GREEN_DARK
            c = mklib.colour(paint, n)
        faces.append((pts, c))
    g.faces = faces
    return g


def glazing(stations):
    """Segments of a station list that lie under a canopy -> the quads glazed there."""
    out = {}
    for i, (s0, s1) in enumerate(zip(stations, stations[1:])):
        y = (s0[0] + s1[0]) / 2
        if any(a >= y >= b for a, b in CANOPIES):
            out[i] = CANOPY_QUADS
    return out


def mirrored_pair(g):
    return g.add(g.mirrored())


def wing_z(x):
    """Height and leading edge of the wing's chord line at span position x."""
    t = (x - WING_ROOT[0]) / (WING_TIP[0] - WING_ROOT[0])
    return WING_ROOT[2] + (WING_TIP[2] - WING_ROOT[2]) * t, WING_ROOT[1] + (WING_TIP[1] - WING_ROOT[1]) * t


def wings(level):
    g = mklib.wing(WING_ROOT, WING_ROOT_CHORD, WING_TIP, WING_TIP_CHORD, CAMO, t=0.10 if level < 2 else 0.06)
    if level == 2:
        return g
    stores = Geo()
    x, y, z = WING_TIP
    # Wingtip pylon with the missile launcher rail under it.
    stores.add(mklib.block(x - 0.05, x + 0.07, y - 0.80, y, z - 0.75, z + 0.03, CAMO))
    stores.add(mklib.block(x - 0.11, x + 0.13, y - 1.10, y + 0.55, z - 0.85, z - 0.75, DARK))
    for px in PYLONS:
        zw, le = wing_z(px)
        zc = zw - 0.63
        y0, y1 = le + 0.30, le - 1.80
        if level == 0:
            stores.add(mklib.block(px - 0.05, px + 0.05, le - 0.95, le - 0.20, zw - 0.36, zw - 0.02, DARK))
            st = [(y0, zc, 0.14, 0.14), (y0 - 0.25, zc, 0.28, 0.28), (y1 + 0.30, zc, 0.28, 0.28), (y1, zc, 0.20, 0.20)]
            stores.add(loft(st, POD, n=6, x=px, paints=[DARK, POD, POD]))
        else:
            stores.add(frustum((px, y0, zc), (px, y1, zc), 0.27, 0.27, POD, n=4))
    return g.add(mirrored_pair(stores))


def engines(level):
    g = Geo()
    if level == 2:
        return mklib.block(-0.80, 0.80, -3.40, 3.00, 2.65, 3.70, CAMO, taper=0.25, taper_y=0.4)
    for sx in (-1, 1):
        if level == 0:
            st = [(3.05, 3.00, 0.30, 0.30), (2.50, 3.10, 0.42, 0.45), (0.50, 3.25, 0.45, 0.55), (-2.00, 3.25, 0.45, 0.55), (-3.30, 3.20, 0.36, 0.40), (-3.60, 3.15, 0.25, 0.28)]
            g.add(loft(st, CAMO, n=8, x=sx * 0.42, paints=[DARK, CAMO, CAMO, CAMO, DARK]))
            # Exhaust pipe angled out and down behind the engine.
            g.add(frustum((sx * 0.78, -2.50, 3.10), (sx * 1.15, -3.20, 2.85), 0.20, 0.22, DARK, n=6))
            # Oil cooler fan intake on the outside of the engine cowling.
            ring = [(sx * 0.885, 0.70 + 0.24 * math.cos(a), 3.25 + 0.24 * math.sin(a)) for a in (math.pi * k / 4 for k in range(8))]
            g.face(ring, DARK, (0, 0.70, 3.25))
        else:
            st = [(3.00, 3.05, 0.38, 0.40), (0.50, 3.25, 0.45, 0.55), (-3.50, 3.18, 0.28, 0.30)]
            g.add(loft(st, CAMO, n=6, x=sx * 0.42, paints=[DARK, CAMO]))
    return g


def upper_deck(level):
    """Main gearbox fairing between the engines and the rotor mast."""
    g = mklib.block(-0.35, 0.35, -1.60, 1.00, 3.55, 3.95, CAMO, taper=0.10, taper_y=0.20)
    mx, my, mz = MAST
    g.add(frustum((mx, my, 3.90), (mx, my, mz - 0.10), 0.20, 0.16, METAL, n=6 if level == 0 else 4))
    return g


def fin(level):
    """Swept fin with the tail rotor gearbox fairing on its left, horizontal stabilisers."""

    def section(le, te, t):
        le, te = v3(le), v3(te)
        a, b = le + (te - le) * 0.25, le + (te - le) * 0.7
        return [le, a + Vector((t, 0, 0)), b + Vector((t * 0.7, 0, 0)), te, b - Vector((t * 0.7, 0, 0)), a - Vector((t, 0, 0))]

    g = mklib.tube([section((0, -8.60, 3.45), (0, -9.85, 2.95), 0.16), section((0, -10.65, 5.10), (0, -11.65, 4.95), 0.12)], CAMO)
    hx, hy, hz = TAIL_HUB
    g.add(mklib.block(hx + 0.12, 0.08, hy - 0.40, hy + 0.40, hz - 0.30, hz + 0.20, CAMO, taper=0.03 if level == 0 else 0.0))
    g.add(mklib.wing((0, -8.85, 3.05), 0.75, (1.70, -8.95, 3.05), 0.55, CAMO, t=0.10 if level < 2 else 0.06))
    if level == 0:
        # Tail skid under the fin.
        g.add(frustum((0, -9.50, 2.85), (0, -10.10, 1.95), 0.05, 0.05, DARK, n=4))
    return g


def gear(level):
    g = Geo()
    n = 8 if level == 0 else 6
    x, y, z = NOSE_WHEEL
    for sx in (-1, 1):
        g.add(mklib.wheel((sx * x, y, z), z, 0.16, TYRE, n=n))
    x, y, z = MAIN_WHEEL
    for sx in (-1, 1):
        g.add(mklib.wheel((sx * x, y, z), z, 0.26, TYRE, n=n))
    if level == 0:
        g.add(frustum((0, 3.32, 0.30), (0, 3.45, 1.00), 0.06, 0.07, DARK, n=4))
        for sx in (-1, 1):
            g.add(frustum((sx * (x - 0.08), y - 0.05, z + 0.05), (sx * 0.95, -1.40, 1.35), 0.08, 0.10, DARK, n=4))
            g.add(frustum((sx * (x - 0.08), y + 0.05, z + 0.03), (sx * 0.88, -0.20, 1.15), 0.05, 0.05, DARK, n=4))
    return g


def nose(level):
    """Chin gun turret with its barrels, sensor pods either side of the chin, pitot boom."""
    g = frustum((0, 5.75, 0.95), (0, 5.75, 0.62), 0.25, 0.18, DARK, n=6 if level == 0 else 4)
    if level == 0:
        g.add(frustum((0, 5.80, 0.70), (0, 6.65, 0.68), 0.05, 0.04, DARK, n=4))
        for sx in (-1, 1):
            g.add(frustum((sx * 0.45, 5.70, 0.98), (sx * 0.45, 5.25, 0.98), 0.12, 0.13, DARK, n=6))
        g.line((-0.30, 6.00, 1.75), (-0.30, 7.20, 1.75), DARK.hi)
    return g


def details(g):
    """Cabin windows, door outline and canopy frames (level 0 only)."""
    for y0, y1 in ((1.60, 1.27), (0.94, 0.60), (0.18, -0.15)):
        heli.side_panel(STATIONS, y0, y1, 1.80, 2.10, WINDOW, g)
    # Clamshell cabin door: upper half hinged up, lower half down.
    heli.side_outline(STATIONS, [(1.85, 1.15), (0.40, 1.15), (0.40, 2.15), (1.85, 2.15)], SEAM, g)
    for sx in (-1, 1):
        g.line((sx * (heli.side_x(STATIONS, 1.85, 1.70) + 0.01), 1.85, 1.70), (sx * (heli.side_x(STATIONS, 0.40, 1.70) + 0.01), 0.40, 1.70), SEAM)
    # Canopy frames across the glazing.
    for i in (2, 6):
        r = heli.ring(STATIONS[i])
        zc = (r[0].z + r[4].z) / 2
        r = [Vector((p.x * 1.02, p.y, zc + (p.z - zc) * 1.02)) for p in r]
        for a, b in zip(r[2:7], r[3:8]):
            g.line(a, b, SEAM)


def body(level):
    g = Geo()
    if level == 0:
        st = STATIONS
    elif level == 1:
        st = [STATIONS[i] for i in (0, 1, 3, 4, 6, 7, 9, 11, 12, 14, 15)]
    else:
        st = [STATIONS[i] for i in (1, 3, 6, 9, 11, 12, 15)]
    glazed = glazing(st) if level < 2 else {i: {1, 2, 3} for i in glazing(st)}
    g.add(heli.fuselage(st, CAMO, GLASS, level < 2, glazed))
    g.add(engines(level))
    g.add(upper_deck(level))
    g.add(wings(level))
    g.add(fin(level))
    if level < 2:
        g.add(gear(level))
        g.add(nose(level))
    if level == 0:
        details(g)
    return camouflage(g)


# Lights in metres: navigation lights on the outside of the wingtip pylons, beacons on the tail
# boom and under the belly.
NAV = {RED: [(-3.42, -1.00, 1.80)], GREEN: [(3.42, -1.00, 1.80)]}
BEACONS = [(0.0, -4.30, 3.58), (0.0, -0.80, 0.92)]


def build(name):
    root = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(root)
    root["attr"] = 0x83
    levels = []
    for level, dist in enumerate(LOD_DISTS):
        main = heli.rotor(level, 5, ROTOR_R, ROTOR_CHORD, BLADE, METAL, clockwise=True)
        tail = heli.tail_rotor(level, 3, TAIL_R, TAIL_CHORD, BLADE, METAL)
        levels.append((dist, body(level), main, tail, (0.0, 0.0, 0.0)))
    heli.build_levels(root, levels, K, MAST, TAIL_HUB, (0.0, 0.0, 0.0), NAV, {RED: BEACONS})
    return root
