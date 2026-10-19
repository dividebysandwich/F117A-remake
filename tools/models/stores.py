"""Remodels of the remaining STFLT.3DX models: detail for the player's F-117A (24, 31, 33, keeping
its original faceted shell), its gear and bay
(34), the drag chute (35), missiles (1, 10, 15), the guided bomb (12), the decoy (2) and the
supply parachute (11).

Built directly in the original model units (x right, y forward, z up; the F-117 is 258 units
long with its belly at z = 0) so that every state node, light and exhaust keeps its place. Each
builder returns {node suffix: Geo}: '' is the LOD 0 node itself, '_s8v1' its child of that name
and so on; colour 13 faces in the F-117 body are the exhaust glow (models.rs flickers them).
"""

import math

from mathutils import Vector

from mklib import Geo, Ramp, frustum, loft, tube

SKIN = Ramp(152, 159)       # F-117 radar-absorbent grey
SKIN_D = Ramp(155, 159)
GLASS = Ramp(144, 145)
BAY = 159
FLAME = 13
WHITE = Ramp(224, 232)
GREY = Ramp(226, 236)
DARK = Ramp(233, 239)
OLIVE = Ramp(165, 173)
YELLOW = 199
RED = Ramp(208, 215)
LINE = 230


def V(x, y, z):
    return Vector((x, y, z))


# ---------------------------------------------------------------------------------------------
# F-117A


def bay(state):
    """Weapon bay doors (state 0 closed, 1 opening, 2 open) and the bay seen through them."""
    g = Geo()
    y0, y1 = -12, 44
    if state == 0:
        for s in (1, -1):
            g.face2([V(s * 0.2, y0, -0.5), V(s * 10, y0, -0.5), V(s * 10, y1, -0.5), V(s * 0.2, y1, -0.5)], SKIN_D)
        g.line(V(0, y0, -0.6), V(0, y1, -0.6), 159)
        return g
    ang = math.radians(45 if state == 1 else 88)
    w = 10
    g.face2([V(-10, y0, -0.3), V(10, y0, -0.3), V(10, y1, -0.3), V(-10, y1, -0.3)], BAY)
    for s in (1, -1):
        hinge = s * 10
        dx, dz = -s * w * math.cos(ang), -w * math.sin(ang)
        # Door edges are sawtoothed like the real ones (front and rear ends).
        pts = [V(hinge, y0 + 2, 0), V(hinge, y1 - 2, 0), V(hinge + dx * 0.6, y1, dz * 0.6), V(hinge + dx, y1 - 2, dz),
               V(hinge + dx, y0 + 2, dz), V(hinge + dx * 0.6, y0, dz * 0.6)]
        g.face2(pts, SKIN_D)
        # actuator
        g.line(V(s * 7, (y0 + y1) / 2, -0.3), V(hinge + dx * 0.5, (y0 + y1) / 2, dz * 0.5), 236)
    if state == 2:
        # GBU-27 on the trapeze, ready to drop
        g.add(gbu27(scale=1.0).transformed(lambda p: V(p.x * 1.0 + 4.5, p.y + 16, p.z - 5)))
        g.line(V(4.5, 16, -0.3), V(4.5, 16, -2.6), 236)
    return g


def outline(g, tris, colour, lift=0.35):
    """Frame lines along the boundary of a group of coplanar triangles, lifted off the face."""
    count = {}
    for t, _ in tris:
        n = (t[1] - t[0]).cross(t[2] - t[0]).normalized()
        for i in range(3):
            a, b = t[i], t[(i + 1) % 3]
            k = frozenset((tuple(round(c, 2) for c in a), tuple(round(c, 2) for c in b)))
            count.setdefault(k, [0, a, b, n])[0] += 1
    for k, (c, a, b, n) in count.items():
        if c == 1:
            g.line(a + n * lift, b + n * lift, colour)


def grid(g, quad, n_u, n_v, colour, lift=0.35):
    """Grid lines over a quad (corners in order), lifted along its normal."""
    a, b, c, d = [V(*p) for p in quad]
    nrm = (b - a).cross(d - a).normalized()
    if nrm.dot((a + b + c + d) / 4 - V(0, (a.y + c.y) / 2, 4)) < 0:
        nrm = -nrm
    for i in range(1, n_u):
        t = i / n_u
        g.line(a.lerp(b, t) + nrm * lift, d.lerp(c, t) + nrm * lift, colour)
    for j in range(1, n_v):
        t = j / n_v
        g.line(a.lerp(d, t) + nrm * lift, b.lerp(c, t) + nrm * lift, colour)


def f117(orig=None):
    """The original faceted shell, unchanged, with the detail it lacked: canopy framing, intake
    and exhaust grids, panel lines, antennas, probes and articulated bay doors."""
    g = Geo()
    tris = (orig or {}).get("", [])
    g.faces = [(list(t), c) for t, c in tris]
    # Canopy frames around the glazing
    outline(g, [(t, c) for t, c in tris if c in (144, 145)], 159, 0.3)
    for s in (1, -1):
        # Intake grilles (the dark facets ahead of the wing root)
        grid(g, [(s * 14, 64, 12), (s * 20, 72, 3), (s * 26, 54, 3), (s * 20, 48, 12)], 6, 3, 154)
        # Exhaust slot vanes over the glowing faces
        grid(g, [(s * 6, -70, 0), (s * 31, -40, 0), (s * 30, -42, 6), (s * 6, -70, 6)], 9, 2, 159, 0.3)
        # Ruddervator hinge lines and leading-edge strips
        g.line(V(s * 2.2, -66, 7.6), V(s * 15.5, -106, 27.5), 159)
        g.line(V(s * 4.4, -48.5, 14.6), V(s * 18.3, -104, 30.6), 152)
        # Elevon hinge and tip lights' fairing lines on the wing
        g.line(V(s * 33, -37, 0.55), V(s * 74, -82, 0.55), 159)
        g.line(V(s * 30, -40, 0.55), V(s * 30, 20, 0.55), 157)
        # Inlet/fuselage blend line and canopy rail
        g.line(V(s * 16, 92, 0.6), V(s * 24, -14, 16.4), 157)
    # Spine: refuelling receptacle doors and blade antennas
    g.face2([V(-2, 24, 22.5), V(2, 24, 22.5), V(2, 16, 21.7), V(-2, 16, 21.7)], 159)
    g.face2([V(0, 2, 20.6), V(0, -6, 19.8), V(0, -5, 23.0), V(0, 0, 23.0)], 157)
    g.face2([V(0, -20, -0.4), V(0, -28, -0.4), V(0, -27, -3.0), V(0, -23, -3.0)], 157)
    # Nose air data probe
    g.line(V(0, 136, 0.2), V(0, 144, 0.2), 236)
    return {"": g, "_s8v0": bay(0), "_s8v1": bay(1), "_s8v2": bay(2)}


# ---------------------------------------------------------------------------------------------
# Gear (STFLT 34: the F-117's main and nose gear, shown with the bay doors)


def leg(x, y, z_top, z_axle, r_wheel, width, twin=False):
    g = Geo()
    g.add(frustum((x, y, z_top), (x, y - 1.5, z_axle + r_wheel * 0.4), 1.1, 0.9, GREY, 6))
    g.add(frustum((x, y - 1.5, z_axle + r_wheel * 0.4), (x, y - 2.0, z_axle), 0.7, 0.7, Ramp(224, 230), 6))
    # torque links
    g.line(V(x, y - 1.0, z_axle + r_wheel * 1.2), V(x, y + 1.5, z_axle + r_wheel * 0.8), 236)
    g.line(V(x, y + 1.5, z_axle + r_wheel * 0.8), V(x, y - 1.8, z_axle + r_wheel * 0.2), 236)
    offs = (-width * 0.6, width * 0.6) if twin else (0.0,)
    for o in offs:
        wx = x + o
        g.add(frustum((wx - width / 2, y - 2.0, z_axle), (wx + width / 2, y - 2.0, z_axle), r_wheel, r_wheel, Ramp(237, 239), 10))
        g.add(frustum((wx - width / 2 - 0.05, y - 2.0, z_axle), (wx + width / 2 + 0.05, y - 2.0, z_axle), r_wheel * 0.55,
                      r_wheel * 0.55, Ramp(226, 232), 8))
    return g


def gear_door(x0, x1, y0, y1, ang, side):
    """Door hinged along x = x0 (rotated down by `ang`), sawtooth ends."""
    g = Geo()
    w = abs(x1 - x0)
    dx, dz = side * w * math.cos(ang), -w * math.sin(ang)
    pts = [V(x0, y0 + 2, 0), V(x0, y1 - 2, 0), V(x0 + dx * 0.5, y1, dz * 0.5), V(x0 + dx, y1 - 2, dz),
           V(x0 + dx, y0 + 2, dz), V(x0 + dx * 0.5, y0, dz * 0.5)]
    g.face2(pts, SKIN_D)
    return g


def gear(orig=None):
    # s5v1: gear doors open; s6v1: gear half down; s6v2: gear down and locked.
    doors = Geo()
    for s in (1, -1):
        doors.add(gear_door(s * 24, s * 18, 6, 42, math.radians(80), -s))
    doors.add(gear_door(4, 0, 60, 88, math.radians(80), -1)).add(gear_door(-4, 0, 60, 88, math.radians(80), 1))
    half = Geo()
    for s in (1, -1):
        half.add(leg(s * 22, 30, 0, -10, 6, 3.5).rotated_x(math.radians(-50), (0, 30, 0)))
    half.add(leg(0, 78, 0, -9, 4.5, 2.4, twin=True).rotated_x(math.radians(50), (0, 78, 0)))
    down = Geo()
    for s in (1, -1):
        down.add(leg(s * 24, 30, 0, -21, 6.5, 4.0))
        # side brace
        down.line(V(s * 24, 28.5, -8), V(s * 14, 24, -0.2), 236)
    down.add(leg(0, 74, 0, -23, 4.8, 2.6, twin=True))
    down.line(V(0, 72.5, -9), V(0, 64, -0.2), 236)
    return {"_s5v1": doors, "_s6v1": half, "_s6v2": down, "_s8v1": bay(1), "_s8v2": bay(2)}


# ---------------------------------------------------------------------------------------------
# Drag chute (STFLT 35, shown while braking): ribbon canopy trailing on its risers


def chute(orig=None):
    g = Geo()
    apex = V(0, -214, 26)
    n = 12
    r, mouth_y = 46, -196
    ring_pts = [V(r * math.cos(2 * math.pi * k / n), mouth_y, 26 + 36 * math.sin(2 * math.pi * k / n)) for k in range(n)]
    mid = [apex + (p - apex) * 0.55 + V(0, 3, 0) for p in ring_pts]
    for k in range(n):
        a, b = ring_pts[k], ring_pts[(k + 1) % n]
        ma, mb = mid[k], mid[(k + 1) % n]
        col = Ramp(224, 230) if k % 2 else RED
        g.face2([a, b, mb, ma], col)
        g.face2([ma, mb, apex], col)
        # ribbon gaps
        g.line((a + ma) / 2 + V(0, 0.4, 0), (b + mb) / 2 + V(0, 0.4, 0), 239)
    conf = V(0, -150, 12)
    for p in ring_pts:
        g.line(p, conf, 237)
    g.line(conf, V(0, -122, 14), 239)
    return {"_s7v1": g}


# ---------------------------------------------------------------------------------------------
# Missiles and bombs (nose along +y, tail at the original's tail)


def fin_set(y0, chord, r0, span, paint, n=4, rot=45.0, sweep=0.5, tip=0.35):
    g = Geo()
    for k in range(n):
        a = math.radians(rot + 360 * k / n)
        d = V(math.cos(a), 0, math.sin(a))
        g.face2([d * r0 + V(0, y0, 0), d * r0 + V(0, y0 + chord, 0), d * (r0 + span) + V(0, y0 + chord * (1 - sweep), 0),
                 d * (r0 + span) + V(0, y0 + chord * (1 - sweep) - chord * tip, 0)], paint)
    return g


def aim9(orig=None):
    """AIM-9 style air-to-air missile (STFLT 1): seeker dome, canards, tail fins with rollerons."""
    L0, L1, r = -22, 22, 2.2
    g = loft([(L0, 0, r, r), (L1 - 7, 0, r, r), (L1 - 2, 0, 1.4, 1.4), (L1, 0, 0.3, 0.3)], WHITE, n=8)
    g.add(loft([(L1 - 2.6, 0, 1.5, 1.5), (L1, 0, 0.6, 0.6)], GLASS, n=8))
    g.add(fin_set(L1 - 13, 4.5, r, 3.6, WHITE, sweep=0.9, tip=0.1))
    g.add(fin_set(L0 + 0.5, 7.5, r, 3.8, WHITE, sweep=0.6, tip=0.4))
    for k in range(4):
        a = math.radians(45 + 90 * k)
        d = V(math.cos(a), 0, math.sin(a)) * (r + 3.6)
        g.add(frustum(d + V(0, L0 + 2.0, 0) - V(0, 0.4, 0), d + V(0, L0 + 2.0, 0) + V(0, 0.4, 0), 0.9, 0.9, GREY, 6))
    g.add(frustum((0, L0, 0), (0, L0 - 0.8, 0), r * 0.8, r * 0.6, DARK, 8))
    # yellow warhead band
    g.add(frustum((0, L1 - 9, 0), (0, L1 - 8, 0), r + 0.05, r + 0.05, YELLOW, 8, caps=False))
    return {"": g, "_s14v1": motor_flame(L0 - 0.8, 1.6, 44)}


def maverick(orig=None):
    """AGM-65 (STFLT 10): fat body, glass seeker dome, long-chord wings, small tail fins."""
    L0, L1, r = -18, 18, 3.0
    g = loft([(L0, 0, r, r), (L1 - 4, 0, r, r), (L1 - 1.2, 0, 2.6, 2.6), (L1, 0, 1.2, 1.2)], OLIVE, n=10)
    g.add(loft([(L1 - 1.6, 0, 2.7, 2.7), (L1 + 0.6, 0, 0.8, 0.8)], GLASS, n=10))
    g.add(fin_set(L0 + 3, 17, r, 3.0, OLIVE, sweep=0.95, tip=0.05))
    g.add(fin_set(L0, 3.5, r, 2.6, OLIVE, sweep=0.3, tip=0.6))
    g.add(frustum((0, L1 - 7, 0), (0, L1 - 6, 0), r + 0.05, r + 0.05, YELLOW, 10, caps=False))
    g.add(frustum((0, L0, 0), (0, L0 - 0.8, 0), r * 0.7, r * 0.6, DARK, 8))
    return {"": g, "_s14v1": motor_flame(L0 - 0.8, 2.2, 46, vertical=True)}


def sam(orig=None):
    """Big surface-to-air / long-range missile (STFLT 15): wings mid-body, tail control fins."""
    L0, L1, r = -32, 32, 3.0
    g = loft([(L0, 0, r, r), (L1 - 12, 0, r, r), (L1 - 4, 0, 1.8, 1.8), (L1, 0, 0.2, 0.2)], WHITE, n=10)
    g.add(fin_set(L1 - 30, 18, r, 2.6, WHITE, sweep=0.85, tip=0.1, rot=0.0))
    g.add(fin_set(L0, 9, r, 3.0, WHITE, sweep=0.5, tip=0.5, rot=0.0))
    g.add(fin_set(L1 - 10, 4, r, 1.6, WHITE, sweep=0.7, tip=0.2, rot=0.0))
    g.add(frustum((0, L1 - 14, 0), (0, L1 - 13, 0), r + 0.05, r + 0.05, RED, 10, caps=False))
    g.add(frustum((0, L0, 0), (0, L0 - 1.0, 0), r * 0.8, r * 0.7, DARK, 10))
    return {"": g, "_s14v1": motor_flame(L0 - 1.0, 2.6, 62, wide=6.0)}


def motor_flame(y, r, length, vertical=False, wide=None):
    """Exhaust plume behind the nozzle: a bright core in a softer cone."""
    g = frustum((0, y + 0.5, 0), (0, y - length * 0.35, 0), r, r * 0.5, FLAME, 6)
    g.add(frustum((0, y - length * 0.35, 0), (0, y - length, 0), r * 0.5, 0.05, FLAME, 6))
    if wide:
        g.add(frustum((0, y, 0), (0, y - length * 0.5, 0), wide * 0.5, 0.2, 17, 6))
    return g


def gbu27(scale=1.0):
    """Paveway III style laser-guided bomb: seeker, canards, body and folding tail fins."""
    s = scale
    g = Geo()
    g.add(loft([(-12 * s, 0, 2.6 * s, 2.6 * s), (6 * s, 0, 3.0 * s, 3.0 * s), (11 * s, 0, 2.2 * s, 2.2 * s), (13 * s, 0, 1.1 * s, 1.1 * s)],
               OLIVE, n=10))
    g.add(loft([(13 * s, 0, 1.1 * s, 1.1 * s), (17 * s, 0, 0.9 * s, 0.9 * s), (18 * s, 0, 0.3 * s, 0.3 * s)], GREY, n=8))
    g.add(loft([(17.6 * s, 0, 0.7 * s, 0.7 * s), (18.3 * s, 0, 0.1, 0.1)], GLASS, n=8))
    g.add(fin_set(13.5 * s, 2.5 * s, 1.0 * s, 1.6 * s, GREY, sweep=0.6, tip=0.4))
    g.add(loft([(-18 * s, 0, 1.8 * s, 1.8 * s), (-12 * s, 0, 2.6 * s, 2.6 * s)], GREY, n=10))
    g.add(fin_set(-18 * s, 6 * s, 1.8 * s, 2.0 * s, GREY, sweep=0.3, tip=0.7))
    for y in (8 * s, 0.0):
        g.add(frustum((0, y, 0), (0, y + 0.8 * s, 0), 3.0 * s + 0.03, 3.0 * s + 0.03 if y else 2.95 * s, YELLOW, 10, caps=False))
    return g


def bomb(orig=None):
    """STFLT 12: the laser-guided bomb (fits the original 8 x 36 units)."""
    return {"": gbu27(scale=1.0).scaled(1.0)}


def decoy(orig=None):
    """STFLT 2: air-launched decoy glider (TALD style): box body, folding wings, tail fins."""
    g = Geo()
    g.add(loft([(-32, 4, 3.2, 3.0), (18, 4, 3.6, 3.4), (28, 4, 2.6, 2.6), (32, 4, 0.4, 0.4)], GREY, n=8, power=3.0))
    from mklib import wing
    g.add(wing((3.4, 2, 5.5), 10, (22, -10, 5.0), 6, Ramp(224, 232), t=0.06))
    g.add(wing((2.0, -24, 6), 8, (8.5, -31, 7.5), 4, Ramp(224, 232), t=0.08))
    g.add(wing((0, -22, 7), 9, (0, -31, 8), 4.5, Ramp(224, 232), t=0.08, vertical=True, mirror=False))
    g.add(frustum((0, 28, 4), (0, 33, 4), 0.5, 0.1, RED, 6))
    return {"": g}


def parachute(orig=None):
    """STFLT 11: supply canopy in red/white gores on its rigging, with the load below."""
    g = Geo()
    n, r = 16, 48
    rows = [(r, 0.0), (r * 0.92, 14), (r * 0.7, 26), (r * 0.38, 34), (0.0, 36)]
    rings = [[V(rr * math.cos(2 * math.pi * k / n), rr * math.sin(2 * math.pi * k / n), z) for k in range(n)] for rr, z in rows]
    for j in range(len(rings) - 1):
        for k in range(n):
            a, b = rings[j][k], rings[j][(k + 1) % n]
            c, d = rings[j + 1][(k + 1) % n], rings[j + 1][k]
            col = RED if (k // 2) % 2 else Ramp(224, 229)
            quad = [a, b, c, d] if (c - d).length > 1e-6 else [a, b, c]
            g.face2(quad, col)
    # skirt hem
    for k in range(n):
        g.line(rings[0][k] - V(0, 0, 0.3), rings[0][(k + 1) % n] - V(0, 0, 0.3), 236)
    lines = Geo()
    conf = V(0, 0, -60)
    for k in range(0, n, 2):
        lines.line(rings[0][k], conf, 224)
    lines.line(conf, V(0, 0, -64), 224)
    # Crate with straps
    box = Geo()
    from mklib import block
    box.add(block(-6, 6, -6, 6, -72, -64, Ramp(166, 173)))
    for x in (-3, 3):
        box.line(V(x, -6.05, -72), V(x, -6.05, -64), 239)
        box.line(V(x, 6.05, -72), V(x, 6.05, -64), 239)
    g.add(box)
    g.lines.setdefault(224, []).extend(lines.lines[224])
    return {"": g}


BUILDS = {
    "stflt_001": aim9,
    "stflt_002": decoy,
    "stflt_010": maverick,
    "stflt_011": parachute,
    "stflt_012": bomb,
    "stflt_015": sam,
    "stflt_024": f117,
    "stflt_031": f117,
    "stflt_033": f117,
    "stflt_034": gear,
    "stflt_035": chute,
}
