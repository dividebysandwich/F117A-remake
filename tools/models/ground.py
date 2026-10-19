"""Ground vehicle and SAM site remodels for the theater .3DX files (metres, front along +y).

Builders return `Parts`: geometry per node suffix ('' = the LOD node itself, '_s1v0' = intact
superstructure hidden when destroyed, '_s2v0' = missiles on the rail, '_part1' = the antenna
turned by state slot 16 around the model origin) and anchors for the original light dots.
"""

import math

import bpy
from mathutils import Vector

import mklib
from mklib import Geo, Ramp, block, extrude, frustum, hexa, loft, side_prism, tube, v3, wing

THEATERS = ["ce", "cu", "ko", "ku", "lb", "me", "nc", "pg", "vn"]
DESERT = {"ku", "lb", "me", "pg"}

OLIVE = Ramp(166, 175)
SAND = Ramp(193, 197)
GREY = Ramp(226, 236)
GREY_L = Ramp(224, 231)
DGREY = Ramp(231, 239)
TRACK = Ramp(236, 239)
GLASS = Ramp(144, 145)
WHITE = Ramp(224, 228)
BLACK = 239
LINE = 230
LINE_D = 236


class Parts(dict):
    def __init__(self, anchors=None, **kw):
        super().__init__(**kw)
        self.anchors = anchors or {}


def rot_x(g, deg, pivot):
    return g.rotated_x(math.radians(deg), pivot)


# ---------------------------------------------------------------------------------------------
# Running gear and trucks


def tracks(L, W, h, lod, tw=0.55):
    g = Geo()
    prof = [(-L / 2 + 0.45, 0.0), (L / 2 - 0.45, 0.0), (L / 2, 0.5 * h), (L / 2 - 0.25, h), (-L / 2 + 0.2, h), (-L / 2, 0.5 * h)]
    for s in (-1, 1):
        x0 = s * W / 2
        g.add(side_prism(prof, x0 - s * tw if s > 0 else x0, x0 if s > 0 else x0 + tw, TRACK))
        if lod == 0:
            n = 6
            for i in range(n):
                y = -L / 2 + 0.75 + (L - 1.5) * i / (n - 1)
                xo = x0 + s * 0.02
                g.add(frustum((xo, y, 0.4), (xo + s * 0.04, y, 0.4), 0.36, 0.36, DGREY, 6))
    return g


def wheels(xs, ys, r, w, lod):
    g = Geo()
    for x in xs:
        for y in ys:
            g.add(frustum((x - w / 2, y, r), (x + w / 2, y, r), r, r, TRACK, 8 if lod == 0 else 6))
    return g


def cab(x0, x1, y0, y1, z0, z1, cam, lod, nose=0.5):
    """Truck cab: box with a raked windscreen facing +y."""
    g = hexa([(x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
              (x0, y0, z1), (x1, y0, z1), (x1, y1 - nose, z1), (x0, y1 - nose, z1)], cam)
    if lod == 0:
        zm = z0 + 0.55 * (z1 - z0)
        t = (zm - z0) / (z1 - z0)
        g.face2([(x0 + 0.15, y1 - nose * t + 0.02, zm), (x1 - 0.15, y1 - nose * t + 0.02, zm),
                (x1 - 0.15, y1 - nose + 0.1, z1 - 0.12), (x0 + 0.15, y1 - nose + 0.1, z1 - 0.12)], GLASS)
    return g


def ural(cam, lod, y0=-3.6, body=True):
    """Ural-375 style 6x6: cab and bonnet at the front, chassis, optional box body."""
    g = Geo()
    g.add(block(-1.0, 1.0, y0, y0 + 7.3, 0.8, 1.15, DGREY))
    g.add(block(-0.75, 0.75, y0 + 6.0, y0 + 7.4, 1.15, 2.0, cam, taper=0.1))
    g.add(cab(-1.2, 1.2, y0 + 4.5, y0 + 6.0, 1.15, 2.6, cam, lod, nose=0.25))
    if body:
        g.add(block(-1.25, 1.25, y0, y0 + 4.4, 1.15, 3.3, cam))
    g.add(wheels((-1.0, 1.0), (y0 + 0.6, y0 + 1.95, y0 + 6.4), 0.55, 0.4, lod))
    return g


def trailer(L, W, z, cam, lod, axles=(-0.3,), y=0.0):
    g = block(-W / 2, W / 2, y - L / 2, y + L / 2, z - 0.3, z, cam)
    g.add(block(-0.15, 0.15, y + L / 2, y + L / 2 + 1.6, z - 0.25, z - 0.1, DGREY))
    for a in axles:
        g.add(wheels((-W / 2 + 0.2, W / 2 - 0.2), (y + a * L,), 0.5, 0.35, lod))
    return g


def outriggers(r, z, cam):
    g = Geo()
    for a in (45, 135, 225, 315):
        c, s = math.cos(math.radians(a)), math.sin(math.radians(a))
        g.add(hexa([(c * 0.6 - s * 0.15, s * 0.6 + c * 0.15, z - 0.3), (c * r - s * 0.12, s * r + c * 0.12, 0.15),
                    (c * r + s * 0.12, s * r - c * 0.12, 0.15), (c * 0.6 + s * 0.15, s * 0.6 - c * 0.15, z - 0.3),
                    (c * 0.6 - s * 0.15, s * 0.6 + c * 0.15, z), (c * r - s * 0.12, s * r + c * 0.12, 0.35),
                    (c * r + s * 0.12, s * r - c * 0.12, 0.35), (c * 0.6 + s * 0.15, s * 0.6 - c * 0.15, z)], cam))
        g.add(block(c * r - 0.35, c * r + 0.35, s * r - 0.35, s * r + 0.35, 0.0, 0.15, DGREY))
    return g


# ---------------------------------------------------------------------------------------------
# Missiles


def missile(L, r, paint=WHITE, booster=0.0, rb=None, wing_span=0.0, fin_span=0.0, n=8, nose=0.18):
    """Missile along +y with its tail at y=0 (optional booster stage behind)."""
    g = Geo()
    rb = rb or r * 1.3
    y0 = booster
    g.add(loft([(y0, 0, r, r), (y0 + L * (1 - nose), 0, r, r), (y0 + L, 0, 0.0, 0.0)], paint, n=n))
    if booster:
        g.add(frustum((0, 0, 0), (0, booster, 0), rb, rb, paint, n))
        if fin_span:
            for a in range(4):
                g.add(fin(a * 90 + 45, rb, rb + fin_span, 0.1, booster * 0.55, paint))
    elif fin_span:
        for a in range(4):
            g.add(fin(a * 90 + 45, r, r + fin_span, 0.0, L * 0.12, paint))
    if wing_span:
        for a in range(4):
            g.add(fin(a * 90 + 45, r, r + wing_span, y0 + L * 0.35, L * 0.22, paint))
    return g


def fin(angle, r0, r1, y, chord, paint):
    a = math.radians(angle)
    d = Vector((math.cos(a), 0, math.sin(a)))
    return Geo().add(tube([[d * r0 + Vector((0, y, 0)), d * r0 + Vector((0, y + chord, 0))],
                           [d * r1 + Vector((0, y, 0)), d * r1 + Vector((0, y + chord * 0.45, 0))]], paint, False, False))


def placed(g, x, y, z, pitch):
    """Missile geometry pitched up by `pitch` degrees and moved to (x, y, z)."""
    return rot_x(g, pitch, (0, 0, 0)).scaled(1.0, (x, y, z))


# ---------------------------------------------------------------------------------------------
# Vehicles


def t72(cam, lod):
    base = tracks(6.9, 3.5, 0.95, lod)
    base.add(side_prism([(-3.35, 0.45), (3.0, 0.45), (3.5, 0.85), (2.6, 1.45), (-3.35, 1.5)], -1.2, 1.2, cam))
    base.add(block(-1.78, 1.78, -3.2, 3.15, 0.95, 1.05, cam))
    up = Geo()
    up.add(frustum((0, -0.4, 1.45), (0, -0.4, 2.1), 1.2, 0.8, cam, 8 if lod == 0 else 6, r0b=1.05, r1b=0.7))
    up.add(frustum((0, 0.55, 1.85), (0, 6.3, 1.9), 0.14, 0.1, DGREY, 6 if lod == 0 else 4))
    if lod == 0:
        base.add(frustum((-1.0, -3.55, 1.3), (1.0, -3.55, 1.3), 0.28, 0.28, DGREY, 6))
        up.add(block(-0.38, 0.38, 0.4, 0.95, 1.55, 2.05, cam, taper=0.05))
        up.add(frustum((0.45, -0.75, 2.05), (0.45, -0.75, 2.32), 0.32, 0.26, cam, 6))
        up.add(frustum((-0.5, -0.4, 2.0), (-0.5, -0.4, 2.15), 0.25, 0.2, cam, 6))
        up.line((0.45, -0.75, 2.4), (0.45, 0.4, 2.48), BLACK)
        up.add(block(-0.95, 0.95, -1.75, -1.3, 1.6, 1.9, DGREY))
    return Parts(**{"": base, "_s1v0": up})


def sa6(cam, lod):
    base = tracks(7.0, 3.2, 0.9, lod)
    base.add(side_prism([(-3.6, 0.45), (3.0, 0.45), (3.7, 0.8), (3.1, 1.5), (-3.6, 1.55)], -1.35, 1.35, cam))
    up = Geo()
    up.add(frustum((0, -0.8, 1.5), (0, -0.8, 1.85), 1.15, 1.05, cam, 8 if lod == 0 else 6))
    up.add(block(-1.2, 1.2, -1.8, 0.3, 1.85, 2.2, cam))
    m = missile(5.8, 0.17, WHITE, wing_span=0.28 if lod == 0 else 0.0, fin_span=0.22 if lod == 0 else 0.0, n=6 if lod == 0 else 4)
    for x in (-0.8, 0.0, 0.8):
        up.add(placed(m, x, -2.6, 2.45, 12))
        if lod == 0:
            up.add(placed(Geo().add(block(-0.05, 0.05, 0.0, 4.2, -0.3, -0.18, DGREY)), x, -2.4, 2.45, 12))
    return Parts(**{"": base, "_s1v0": up})


def scud(cam, lod):
    """MAZ-543 with an R-17 missile raised on its erector."""
    g = Geo()
    g.add(block(-1.2, 1.2, -6.4, 5.4, 1.0, 1.5, DGREY))
    for s in (-1, 1):
        g.add(cab(s * 0.55 if s > 0 else -1.5, 1.5 if s > 0 else -0.55, 4.2, 6.6, 1.0, 2.9, cam, lod, nose=0.3))
    g.add(block(-0.5, 0.5, 4.6, 6.4, 1.0, 2.2, cam))
    g.add(block(-1.4, 1.4, -6.6, 3.8, 1.5, 2.0, cam))
    g.add(wheels((-1.25, 1.25), (-5.2, -3.4, 1.6, 3.4), 0.78, 0.5, lod))
    up = Geo()
    up.add(placed(missile(11.2, 0.44, Ramp(168, 174) if cam is OLIVE else Ramp(194, 197), wing_span=0.0, fin_span=0.55,
                          n=8 if lod == 0 else 6), 0, -6.2, 2.4, 88))
    up.add(hexa([(-0.5, -6.6, 2.0), (0.5, -6.6, 2.0), (0.5, -5.6, 2.0), (-0.5, -5.6, 2.0),
                 (-0.3, -6.5, 10.0), (0.3, -6.5, 10.0), (0.3, -6.1, 10.0), (-0.3, -6.1, 10.0)], DGREY))
    up.add(block(-1.4, 1.4, -7.8, -6.6, 0.0, 2.4, DGREY))
    return Parts(**{"": g, "_s1v0": up})


def frog(cam, lod):
    """9P113 (ZiL-135 8x8) with a FROG-7 rocket on its rail."""
    g = Geo()
    g.add(block(-1.2, 1.2, -4.8, 4.4, 0.9, 1.4, DGREY))
    g.add(cab(-1.35, 1.35, 3.2, 5.4, 0.9, 2.6, cam, lod, nose=0.4))
    g.add(block(-1.35, 1.35, -5.0, 3.2, 1.4, 1.9, cam))
    g.add(wheels((-1.15, 1.15), (-4.0, -2.4, 0.6, 2.2), 0.6, 0.45, lod))
    up = Geo()
    up.add(block(-0.25, 0.25, -5.0, 3.0, 1.9, 2.3, DGREY))
    up.add(placed(missile(9.1, 0.27, WHITE, fin_span=0.35, n=6), 0, -5.2, 2.6, 4))
    return Parts(**{"": g, "_s1v0": up})


# ---------------------------------------------------------------------------------------------
# SAM launchers (the original keeps the missiles in `_s2v0`)


def berm(r0, r1, h, cam, gap=1):
    g = Geo()
    n = 8
    for i in range(gap, n):
        a0, a1 = 2 * math.pi * i / n, 2 * math.pi * (i + 1) / n
        p = lambda r, a, z: (r * math.sin(a), -r * math.cos(a), z)
        g.add(hexa([p(r0 - 0.6, a0, 0), p(r1 + 0.6, a0, 0), p(r1 + 0.6, a1, 0), p(r0 - 0.6, a1, 0),
                    p(r0, a0, h), p(r1, a0, h), p(r1, a1, h), p(r0, a1, h)], cam))
    return g


def sa2(cam, lod):
    base = Geo()
    base.add(berm(5.4, 6.6, 1.1, cam, gap=1) if lod == 0 else Geo())
    base.add(outriggers(3.4, 0.9, DGREY))
    base.add(frustum((0, 0, 0.6), (0, 0, 1.2), 1.4, 1.2, cam, 8 if lod == 0 else 6))
    arm = Geo()
    arm.add(block(-0.45, 0.45, -1.6, 7.4, -0.55, -0.25, cam))
    arm.add(block(-0.6, 0.6, -1.9, -0.6, -0.9, 0.0, DGREY))
    base.add(rot_x(arm, 35, (0, 0, 0)).scaled(1.0, (0, -1.0, 2.0)))
    base.add(hexa([(-0.7, -1.6, 1.2), (0.7, -1.6, 1.2), (0.7, 0.4, 1.2), (-0.7, 0.4, 1.2),
                   (-0.4, -1.2, 2.2), (0.4, -1.2, 2.2), (0.4, -0.6, 2.2), (-0.4, -0.6, 2.2)], cam))
    m = missile(8.0, 0.25, WHITE, booster=2.6, rb=0.33, fin_span=1.0 if lod == 0 else 0.6, wing_span=0.5 if lod == 0 else 0.0,
                n=8 if lod == 0 else 4)
    miss = rot_x(m.scaled(1.0, (0, -2.4, 0)), 35, (0, 0, 0)).scaled(1.0, (0, -1.0, 2.15))
    return Parts(**{"": base, "_s2v0": miss})


def sa3(cam, lod):
    base = Geo()
    base.add(outriggers(2.6, 0.8, DGREY))
    base.add(frustum((0, 0, 0.5), (0, 0, 1.1), 1.3, 1.1, cam, 8 if lod == 0 else 6))
    frame = Geo()
    for x in (-0.95, 0.95):
        frame.add(block(x - 0.1, x + 0.1, -1.2, 4.4, 0.5, 0.75, cam))
    frame.add(block(-1.05, 1.05, -0.6, 0.0, 0.45, 0.8, cam))
    frame.add(block(-0.3, 0.3, -0.6, 0.0, -0.4, 0.5, DGREY))
    base.add(rot_x(frame, 25, (0, 0, 0)).scaled(1.0, (0, -0.4, 1.8)))
    m = missile(4.0, 0.19, WHITE, booster=2.1, rb=0.28, fin_span=0.45 if lod == 0 else 0.3, wing_span=0.3 if lod == 0 else 0.0,
                n=6 if lod == 0 else 4)
    miss = Geo()
    for x in (-0.95, 0.95):
        for z in (0.15, 1.15):
            miss.add(rot_x(m.scaled(1.0, (x, -1.6, z)), 25, (0, 0, 0)).scaled(1.0, (0, -0.4, 1.8)))
    return Parts(**{"": base, "_s2v0": miss})


def hawk(cam, lod):
    base = Geo()
    base.add(block(-1.3, 1.3, -2.6, 2.0, 0.6, 1.0, cam))
    base.add(block(-0.12, 0.12, 2.0, 3.6, 0.6, 0.8, DGREY))
    base.add(wheels((-1.3, 1.3), (-1.2,), 0.5, 0.35, lod))
    base.add(outriggers(2.4, 0.7, DGREY))
    base.add(frustum((0, 0, 1.0), (0, 0, 1.6), 0.9, 0.8, cam, 8 if lod == 0 else 6))
    beam = Geo().add(block(-1.6, 1.6, -0.8, 0.8, -0.2, 0.25, cam)).add(block(-0.3, 0.3, -0.5, 0.5, -0.8, -0.2, DGREY))
    base.add(rot_x(beam, 30, (0, 0, 0)).scaled(1.0, (0, 0, 2.1)))
    m = missile(5.1, 0.18, WHITE, wing_span=0.5 if lod == 0 else 0.3, fin_span=0.3 if lod == 0 else 0.0, n=6 if lod == 0 else 4)
    # Hawk wings are long-chord strakes
    miss = Geo()
    for x in (-1.2, 0.0, 1.2):
        miss.add(rot_x(m.scaled(1.0, (x, -2.0, 0.55)), 30, (0, 0, 0)).scaled(1.0, (0, 0, 2.1)))
    return Parts(**{"": base, "_s2v0": miss})


# ---------------------------------------------------------------------------------------------
# SAM site radars (antenna in `_part1`, turned around the model origin)


def yagi(g, x, y0, y1, z, span, n, c=LINE, vertical=False):
    g.line((x, y0, z), (x, y1, z), c)
    for i in range(n):
        y = y0 + (y1 - y0) * (i + 0.5) / n
        h = span * (1.0 - 0.08 * i) / 2
        if vertical:
            g.line((x, y, z - h), (x, y, z + h), c)
        else:
            g.line((x - h, y, z), (x + h, y, z), c)


def beam(a, b, w, paint):
    """Square bar between two points."""
    return frustum(a, b, w, w, paint, 4, rot=math.pi / 4)


def nebo_u(cam, lod):
    """55Zh6U Nebo-U: VHF antenna post with a wide array of Yagi rows on a folding mast."""
    base = Geo()
    base.add(trailer(11.0, 2.6, 1.4, cam, lod, axles=(-0.42, -0.31, -0.2)))
    base.add(cab(-1.25, 1.25, 7.4, 9.6, 0.9, 3.0, cam, lod, nose=0.3))
    base.add(block(-0.9, 0.9, 9.6, 10.6, 0.9, 2.0, cam, taper=0.1))
    base.add(wheels((-1.1, 1.1), (6.6, 9.8), 0.6, 0.45, lod))
    base.add(block(-1.0, 1.0, 5.2, 7.2, 1.1, 1.5, DGREY))
    if lod == 0:
        for sx in (-1, 1):
            for sy in (-4.5, 4.0):
                base.add(block(sx * 1.3 - 0.15 + sx * 1.0, sx * 1.3 + 0.15 + sx * 1.0, sy - 0.15, sy + 0.15, 0.0, 1.2, DGREY))
                base.add(block(min(sx * 1.3, sx * 2.45), max(sx * 1.3, sx * 2.45), sy - 0.1, sy + 0.1, 1.0, 1.2, DGREY))
    part = Geo()
    part.add(frustum((0, 0, 1.4), (0, 0, 2.0), 1.2, 1.1, cam, 8))
    part.add(block(-0.9, 0.9, -1.5, 1.2, 2.0, 2.8, cam))
    part.add(beam((0, -0.3, 2.8), (0, 0.0, 8.0), 0.35, GREY))
    W, z0, z1, y = 9.0, 5.4, 10.6, 0.5
    for z in (z0, z1):
        part.add(beam((-W, y, z), (W, y, z), 0.18, GREY))
    for i in range(7):
        x = -W + 2 * W * i / 6
        part.add(beam((x, y, z0), (x, y, z1), 0.12, GREY))
    for s in (-1, 1):
        part.add(beam((0, -0.3, 3.2), (s * W * 0.7, y - 0.05, z1 - 0.2), 0.1, DGREY))
        part.add(beam((0, -0.3, 3.0), (s * W, y - 0.05, z0 + 0.1), 0.1, DGREY))
    if lod == 0:
        for j in range(4):
            z = z0 + (z1 - z0) * (j + 0.5) / 4
            for i in range(12):
                x = -W + 2 * W * (i + 0.5) / 12
                part.add(frustum((x, y + 0.1, z), (x, y + 2.4, z), 0.05, 0.05, GREY_L, 3))
                yagi(part, x, y + 0.1, y + 2.4, z, 0.9, 4)
            part.add(beam((-W, y + 0.15, z), (W, y + 0.15, z), 0.06, DGREY))
        for j in range(9):
            z = z0 + (z1 - z0) * j / 8
            part.line((-W, y - 0.1, z), (W, y - 0.1, z), LINE_D)
    else:
        part.face2([(-W, y + 0.6, z0), (W, y + 0.6, z0), (W, y + 0.6, z1), (-W, y + 0.6, z1)], GREY)
    return Parts({146: [(-W, y, z1 + 0.15), (W, y, z1 + 0.15), (0, y, z1 + 0.15)], 148: [(0, y, z1 + 0.2)]},
                 **{"": base, "_part1": part})


def reflector(g, x0, x1, z0, z1, y, depth, tilt, paint, cols=4, grid=True, c=LINE_D):
    """Cylindrical open-mesh reflector: concave towards +y, tilted back by `tilt` degrees."""
    pts = []
    for i in range(cols + 1):
        t = i / cols
        x = x0 + (x1 - x0) * t
        dy = -depth * (1 - (2 * t - 1) ** 2)
        pts.append((x, dy))
    k = math.tan(math.radians(tilt))
    for i in range(cols):
        (xa, ya), (xb, yb) = pts[i], pts[i + 1]
        g.face2([(xa, y + ya, z0), (xb, y + yb, z0), (xb, y + yb - k * (z1 - z0), z1), (xa, y + ya - k * (z1 - z0), z1)], paint)
    if grid:
        rows = 4
        for j in range(rows + 1):
            z = z0 + (z1 - z0) * j / rows
            for i in range(cols):
                (xa, ya), (xb, yb) = pts[i], pts[i + 1]
                g.line((xa, y + ya - k * (z - z0) + 0.05, z), (xb, y + yb - k * (z - z0) + 0.05, z), c)


def p37(cam, lod):
    """P-37 Bar Lock: rotating cabin carrying two stacked wide reflectors."""
    base = Geo()
    base.add(block(-3.5, 3.5, -3.0, 3.0, 0.0, 1.0, GREY_L))
    base.add(block(-1.2, 1.2, -9.5, -4.5, 0.8, 3.0, cam))
    base.add(wheels((-1.1, 1.1), (-8.6, -5.4), 0.5, 0.35, lod))
    part = Geo()
    part.add(frustum((0, 0, 1.0), (0, 0, 1.4), 2.0, 2.0, DGREY, 8))
    part.add(block(-2.2, 2.2, -2.0, 1.2, 1.4, 4.4, cam))
    for z0, z1 in ((4.4, 7.4), (7.8, 10.8)):
        reflector(part, -5.4, 5.4, z0, z1, 1.6, 0.9, 12, GREY, grid=lod == 0)
        part.add(beam((-4.6, 3.6, z0 + 0.4), (4.6, 3.6, z0 + 0.4), 0.12, DGREY))
        if lod == 0:
            for x in (-4.0, -2.0, 0.0, 2.0, 4.0):
                part.add(block(x - 0.25, x + 0.25, 3.3, 3.9, z0 + 0.3, z0 + 0.8, DGREY))
            for s in (-1, 1):
                part.line((s * 5.0, 1.4, z0 + 0.1), (s * 4.6, 3.6, z0 + 0.4), LINE_D)
    part.add(beam((-1.5, -0.5, 4.4), (-1.5, 0.3, 10.8), 0.15, DGREY))
    part.add(beam((1.5, -0.5, 4.4), (1.5, 0.3, 10.8), 0.15, DGREY))
    return Parts({146: [(-5.4, 0.7, 10.9), (5.4, 0.7, 10.9), (0, 0.7, 10.9)], 148: [(0, 0.5, 11.0)]}, **{"": base, "_part1": part})


def p18(cam, lod):
    """P-18 Spoon Rest D: Ural-375 shelter truck with a mast carrying two rows of eight Yagis."""
    base = ural(cam, lod, y0=-3.0)
    part = Geo()
    part.add(frustum((0, 0, 3.3), (0, 0, 3.6), 0.6, 0.5, DGREY, 6))
    part.add(beam((0, 0, 3.6), (0, 0, 7.6), 0.18, GREY))
    W = 5.2
    for z in (6.2, 7.6):
        part.add(beam((-W, 0, z), (W, 0, z), 0.1, GREY))
    for x in (-W, W):
        part.add(beam((x, 0, 5.9), (x, 0, 7.9), 0.08, GREY))
    part.add(beam((0, 0, 4.4), (-W * 0.6, 0, 6.2), 0.06, DGREY))
    part.add(beam((0, 0, 4.4), (W * 0.6, 0, 6.2), 0.06, DGREY))
    if lod == 0:
        for z in (6.55, 7.25):
            for i in range(8):
                x = -W + 2 * W * (i + 0.5) / 8
                part.add(frustum((x, -0.1, z), (x, 2.9, z), 0.05, 0.05, GREY_L, 3))
                yagi(part, x, -0.1, 2.9, z, 1.1, 5)
    else:
        part.face2([(-W, 1.2, 6.2), (W, 1.2, 6.2), (W, 1.2, 7.6), (-W, 1.2, 7.6)], DGREY)
    return Parts({146: [(-W, 0, 8.0), (W, 0, 8.0), (0, 0, 8.0)], 148: [(0, 0, 8.0)]}, **{"": base, "_part1": part})


def p19(cam, lod):
    """P-19 Flat Face: ZiL-131 shelter truck with two stacked elliptical reflectors on the roof."""
    base = ural(cam, lod, y0=-3.2)
    part = Geo()
    part.add(frustum((0, 0, 3.3), (0, 0, 3.7), 0.8, 0.7, DGREY, 6))
    part.add(beam((0, -0.4, 3.7), (0, -0.2, 7.6), 0.15, GREY))
    for z0, z1 in ((4.0, 5.6), (5.9, 7.5)):
        reflector(part, -3.6, 3.6, z0, z1, 0.4, 0.8, 8, GREY, grid=lod == 0)
        part.add(beam((0, -0.2, (z0 + z1) / 2), (0, 2.6, (z0 + z1) / 2 - 0.2), 0.06, DGREY))
        part.add(block(-0.3, 0.3, 2.5, 2.9, (z0 + z1) / 2 - 0.5, (z0 + z1) / 2 + 0.1, DGREY))
    return Parts({146: [(-3.6, 0.4, 7.6), (3.6, 0.4, 7.6), (0, 0.4, 7.6)], 148: [(0, 0.2, 7.7)]}, **{"": base, "_part1": part})


def trough(g, a, b, depth, width, paint, axis_up=False):
    """V-trough antenna along a->b opening towards +y."""
    a, b = v3(a), v3(b)
    off = Vector((width / 2, 0, 0)) if axis_up else Vector((0, 0, width / 2))
    back = Vector((0, -depth, 0))
    g.face2([a + off, b + off, b + back, a + back], paint)
    g.face2([a - off, b - off, b + back, a + back], paint)
    g.face2([a + off, a - off, a + back], paint)
    g.face2([b + off, b - off, b + back], paint)


def fan_song(cam, lod):
    """SNR-75 Fan Song: cabin trailer with the horizontal and vertical trough antennas and dishes."""
    base = trailer(6.4, 2.5, 1.1, cam, lod, axles=(-0.3, 0.3))
    base.add(block(-1.25, 1.25, -3.2, 3.2, 1.1, 3.4, cam))
    part = Geo()
    part.add(frustum((0, 0, 3.4), (0, 0, 3.8), 1.0, 0.9, DGREY, 8))
    part.add(block(-1.6, 1.6, -1.0, 0.8, 3.8, 5.0, cam))
    trough(part, (-4.6, 1.0, 5.6), (2.6, 1.0, 5.6), 1.0, 1.6, GREY)
    trough(part, (3.4, 1.0, 4.6), (3.4, 1.0, 10.2), 1.0, 1.4, GREY, axis_up=True)
    part.add(beam((-4.0, 0.0, 5.0), (2.4, 0.0, 5.0), 0.12, DGREY))
    part.add(beam((3.4, 0.0, 5.0), (3.4, 0.0, 9.8), 0.12, DGREY))
    for x, z in ((-1.2, 7.4), (0.6, 7.4), (-0.3, 8.8)):
        part.add(frustum((x, 0.6, z), (x, 1.0, z), 0.15, 0.65, GREY_L, 8))
        part.add(beam((x, 0.0, 6.4), (x, 0.6, z), 0.06, DGREY))
    if lod == 0:
        for x in (-4.0, -2.0, 0.0, 2.0):
            part.line((x, 1.8, 5.6), (x, 0.4, 5.6), LINE_D)
        for z in (5.4, 7.4, 9.4):
            part.line((3.4, 1.8, z), (3.4, 0.4, z), LINE_D)
    return Parts({146: [(3.4, 1.0, 10.3), (-4.6, 1.0, 6.5)], 148: [(3.4, 0.8, 10.4)]}, **{"": base, "_part1": part})


def low_blow(cam, lod):
    """SNR-125 Low Blow: antenna post with two crossed tilted array antennas and a dish."""
    base = trailer(7.0, 2.6, 1.2, cam, lod, axles=(-0.32, 0.25))
    base.add(block(-1.3, 1.3, -3.4, -0.6, 1.2, 3.2, cam))
    part = Geo()
    part.add(frustum((0, 1.4, 1.2), (0, 1.4, 2.4), 1.4, 1.2, cam, 8))
    part.add(block(-1.4, 1.4, 0.4, 2.6, 2.4, 3.6, cam))
    big = Geo().add(block(-3.0, 3.0, -0.25, 0.25, -0.9, 0.9, GREY))
    if lod == 0:
        for i in range(7):
            x = -3.0 + i
            big.line((x, 0.27, -0.9), (x, 0.27, 0.9), LINE_D)
    part.add(rot_x(big, 50, (0, 0, 0)).scaled(1.0, (0, 1.8, 5.6)))
    small = Geo().add(block(-0.9, 0.9, -0.2, 0.2, -2.6, 2.6, GREY))
    part.add(rot_x(small, 40, (0, 0, 0)).scaled(1.0, (2.4, 1.8, 5.0)))
    part.add(frustum((-2.3, 2.2, 4.4), (-2.3, 2.8, 4.6), 0.2, 0.9, GREY_L, 8))
    part.add(beam((0, 1.4, 3.6), (0, 1.6, 5.0), 0.25, DGREY))
    part.add(beam((1.2, 1.4, 3.6), (2.4, 1.6, 4.6), 0.12, DGREY))
    part.add(beam((-1.2, 1.4, 3.6), (-2.3, 2.0, 4.4), 0.12, DGREY))
    return Parts({146: [(-3.0, 1.3, 6.4), (3.0, 1.3, 6.4), (2.4, 1.4, 7.3)], 148: [(0, 1.3, 6.5)]}, **{"": base, "_part1": part})


def straight_flush(cam, lod):
    """1S91 Straight Flush: tracked chassis with a turret, search antenna on top, tracking dish below."""
    base = tracks(7.0, 3.2, 0.9, lod)
    base.add(side_prism([(-3.6, 0.45), (3.0, 0.45), (3.7, 0.8), (3.1, 1.5), (-3.6, 1.55)], -1.35, 1.35, cam))
    part = Geo()
    part.add(frustum((0, 0, 1.55), (0, 0, 1.9), 1.3, 1.25, cam, 8))
    part.add(block(-1.2, 1.2, -1.4, 1.2, 1.9, 3.4, cam, taper=0.15))
    part.add(beam((0, 0, 3.4), (0, 0, 4.4), 0.25, DGREY))
    reflector(part, -2.3, 2.3, 4.4, 5.5, 0.6, 0.5, 15, GREY, cols=4, grid=lod == 0)
    part.add(beam((0, 0.4, 4.95), (0, 2.0, 4.8), 0.06, DGREY))
    part.add(frustum((0, 1.25, 2.6), (0, 1.75, 2.6), 0.3, 1.05, GREY_L, 10 if lod == 0 else 8))
    part.add(frustum((0, 1.75, 2.6), (0, 2.6, 2.6), 0.08, 0.05, DGREY, 4))
    return Parts({146: [(-2.3, 0.6, 5.6), (2.3, 0.6, 5.6)], 148: [(0, 0.4, 5.7)]}, **{"": base, "_part1": part})


def hawk_par(cam, lod):
    """AN/MPQ-50 Pulse Acquisition Radar (I-Hawk): trailer with a wide tilted reflector and IFF bar."""
    base = trailer(5.4, 2.4, 1.2, cam, lod, axles=(0.05,))
    base.add(block(-1.1, 1.1, -2.6, -0.8, 1.2, 2.6, cam))
    for sx in (-1, 1):
        for sy in (-2.3, 2.3):
            base.add(block(sx * 1.6 - 0.12, sx * 1.6 + 0.12, sy - 0.12, sy + 0.12, 0.0, 1.0, DGREY))
    part = Geo()
    part.add(frustum((0, 0.5, 1.2), (0, 0.5, 1.8), 0.7, 0.6, DGREY, 8))
    part.add(beam((0, 0.5, 1.8), (0, 0.2, 3.0), 0.2, DGREY))
    reflector(part, -3.1, 3.1, 2.6, 4.4, 0.6, 0.6, 20, GREY_L, cols=6, grid=lod == 0)
    part.add(block(-3.0, 3.0, -0.1, 0.2, 4.6, 4.9, GREY_L))
    part.add(beam((0, 0.3, 2.7), (0, 2.6, 2.6), 0.07, DGREY))
    part.add(block(-0.25, 0.25, 2.4, 2.9, 2.35, 2.85, DGREY))
    for s in (-1, 1):
        part.add(beam((s * 2.6, 0.2, 2.7), (s * 0.2, 2.5, 2.6), 0.04, DGREY))
    return Parts({146: [(-3.0, 0.0, 5.0), (3.0, 0.0, 5.0)], 148: [(0, 0.0, 5.0)]}, **{"": base, "_part1": part})


# ---------------------------------------------------------------------------------------------
# Which original models get which remodel. Kinds: 'vehicle' (turned to the original's long axis),
# 'site' (turned towards the original's offset), 'radar'.

RADARS = {"ce": nebo_u, "nc": p37, "cu": p18, "lb": p18, "ko": p19, "vn": fan_song, "ku": low_blow, "me": straight_flush,
          "pg": hawk_par}
LAUNCHERS = {"ce": sa2, "cu": sa2, "ko": sa2, "nc": sa2, "vn": sa2, "ku": sa3, "lb": sa3, "me": sa3, "pg": hawk}

MODELS = {
    "ce": {"ce_038": "radar", "ce_033": "launcher", "ce_076": sa6, "ce_080": sa6, "ce_077": t72, "ce_093": t72},
    "cu": {"cu_038": "radar", "cu_033": "launcher"},
    "ko": {"ko_038": "radar", "ko_033": "launcher", "ko_098": t72, "ko_100": t72},
    "ku": {"ku_052": "radar", "ku_047": "launcher", "ku_020": scud, "ku_102": scud, "ku_096": t72, "ku_100": t72},
    "lb": {"lb_049": "radar", "lb_044": "launcher", "lb_075": frog, "lb_097": frog, "lb_076": sa6, "lb_079": sa6,
           "lb_077": t72, "lb_078": t72},
    "me": {"me_052": "radar", "me_047": "launcher"},
    "nc": {"nc_036": "radar", "nc_031": "launcher"},
    "pg": {"pg_052": "radar", "pg_047": "launcher"},
    "vn": {"vn_038": "radar", "vn_033": "launcher"},
}


def subtree(o):
    yield o
    for c in o.children:
        yield from subtree(c)


def original_shape(lod0):
    """Bounding radius and horizontal extent of the original LOD 0 (without lights and fires)."""
    pts = [o.matrix_world @ v.co for o in subtree(lod0) if o.type == "MESH" and "_dots_" not in o.name and "_s1v1" not in o.name
           for v in o.data.vertices]
    lo = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
    hi = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
    return max(p.length for p in pts), lo, hi


def heading(lo, hi):
    """Rotation (radians around z) taking +y onto the original's long axis, towards its offset."""
    c = (lo + hi) / 2
    if hi.x - lo.x > hi.y - lo.y:
        h = Vector((1 if c.x >= 0 else -1, 0, 0))
    else:
        h = Vector((0, 1 if c.y >= 0 else -1, 0))
    return math.atan2(-h.x, h.y)


def radius(parts):
    return max(p.length for g in parts.values() for f, _ in g.faces for p in f)


LOW = {"ce_080", "ce_093", "ko_100", "ku_100", "ku_102", "lb_078", "lb_079", "lb_097"}


def apply(stem):
    cam = SAND if stem in DESERT else OLIVE
    done = []
    for name, kind in MODELS.get(stem, {}).items():
        build = RADARS[stem] if kind == "radar" else LAUNCHERS[stem] if kind == "launcher" else kind
        r, lo, hi = original_shape(bpy.data.objects[name + "_lod0"])
        detailed = build(cam, 0)
        s = r / radius(detailed)
        a = 0.0 if kind == "radar" else heading(lo, hi)
        lods = sorted((o for o in bpy.data.objects[name].children if "_lod" in o.name), key=lambda o: o.name)
        for k, node in enumerate(lods):
            if not any(o.type == "MESH" and "_dots_" not in o.name for o in subtree(node)):
                continue  # far LOD: a single dot
            parts = detailed if k == 0 and name not in LOW else build(cam, 1)
            geos = {key: geo.rotated_z(a).scaled(s) for key, geo in parts.items()}
            # Parts without a node at this LOD join the LOD node's own geometry.
            own = geos.pop("", Geo())
            for key, geo in geos.items():
                target = bpy.data.objects.get(node.name + key)
                if target is None:
                    own.add(geo)
                else:
                    mklib.set_geometry(target, geo)
            mklib.set_geometry(node, own)
            place_dots(node, {c: [rotz(p, a) * s for p in pts] for c, pts in parts.anchors.items()})
        done.append(name)
    return done


def rotz(p, a):
    p = v3(p)
    c, s = math.cos(a), math.sin(a)
    return Vector((p.x * c - p.y * s, p.x * s + p.y * c, p.z))


def place_dots(node, anchors):
    for o in subtree(node):
        if o.type != "MESH" or "_dots_pal_" not in o.name:
            continue
        targets = anchors.get(mklib.index_of(o.name))
        if targets:
            mklib.set_points(o, [min(targets, key=lambda t: (t - v.co).length) for v in o.data.vertices])
