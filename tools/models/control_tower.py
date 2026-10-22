"""Rebuilds the airfield control tower of every theater master as an octagonal tower: a
concrete shaft, a flared operations floor with a window band and a gallery, a glazed cab with
an overhanging roof, antenna masts and an operations building at the foot.

    blender -b assets-src/blend/<file>.blend --python-exit-code 1 --python tools/models/control_tower.py
    tools/models/export.sh <file>

Runs on the open master and saves it. Each detail level with geometry is rebuilt in place (its
`max_dist`, `radius` and collision `planes` stay as they were); the last level (a dot) is
kept. Structure: unconditional shaft and building, `_s1v0` (intact: operations floor, cab,
roof, masts, red obstacle lights), `_s1v1` (destroyed: broken shaft top) and `_s10v1` (lights
on: white floodlights). Colour 150 is the classic window colour: dark glass by day, amber
at night. Model space: x east, y north, z up, in LOD1 units (WU, about 0.49 m).
"""

import math
import os
import sys

import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, HERE)

import mklib  # noqa: E402
from mklib import Geo, Ramp, block, tube  # noqa: E402

# Tower per theater file: model, shaft centre, shaft circumradius, roof height, operations
# building boxes (x0, x1, y0, y1, height) and an optional lattice mast (x, y, top).
TOWERS = {
    "ce": dict(model=39, c=(-36, 0), r=10, top=80, base=[(-48, 48, -48, 96, 16)]),
    "pg": dict(model=53, c=(36, 0), r=10, top=80, base=[(-48, 48, -96, 48, 16)]),
    "lb": dict(model=50, c=(-24, 0), r=14, top=76, base=[(-48, 48, -48, 96, 16)]),
    "nc": dict(model=37, c=(-24, 0), r=14, top=76, base=[(-16, 48, -48, 96, 16)]),
    "cu": dict(model=39, c=(-24, 4), r=14, top=76, base=[(-48, 0, -48, 24, 14)]),
    "vn": dict(model=39, c=(-24, 4), r=14, top=76, base=[(-48, 0, -48, 24, 14)]),
    "ku": dict(model=53, c=(24, 0), r=16, top=108, base=[(-4, 52, 14, 52, 16)], mast=(24, 44, 128)),
    "me": dict(model=53, c=(24, 0), r=16, top=108, base=[(-4, 52, 14, 52, 16)], mast=(24, 44, 128)),
}

SHAFT = Ramp(0xE2, 0xE9)
BAND = Ramp(0xE5, 0xEC)
SLAB = Ramp(0xE7, 0xEE)
BUILDING = Ramp(0xE3, 0xEA)
ROOF = Ramp(0xE9, 0xED)
CHAR = Ramp(0xEB, 0xEF)
GLASS = 150
SEAM = 0xEB
RAIL = 0xE8
MAST = 0xEE
STRIPE_RED = 0xD9
STRIPE_WHITE = 0xE0
RED_LIGHT = 146
FLOODLIGHT = 148


def octagon(cx, cy, z, r):
    """Octagon with its flat sides facing the axes (circumradius r)."""
    return [(cx + r * math.cos(math.radians(22.5 + 45 * k)), cy + r * math.sin(math.radians(22.5 + 45 * k)), z) for k in range(8)]


def ring_tube(cx, cy, stations, paint, caps=(True, True)):
    """Octagonal body through (z, r) stations."""
    return tube([octagon(cx, cy, z, r) for z, r in stations], paint, *caps)


def ring_lines(g, cx, cy, z, r, c):
    pts = octagon(cx, cy, z, r)
    for k in range(8):
        g.line(pts[k], pts[(k + 1) % 8], c)


def railing(g, cx, cy, z, r, h=2.2):
    """Rail, mid rail and posts along an octagonal edge."""
    ring_lines(g, cx, cy, z + h, r, RAIL)
    ring_lines(g, cx, cy, z + h / 2, r, RAIL)
    pts = octagon(cx, cy, z, r)
    for k in range(8):
        a, b = pts[k], pts[(k + 1) % 8]
        for t in (0.0, 1 / 3, 2 / 3):
            x, y = a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t
            g.line((x, y, z), (x, y, z + h), RAIL)


def side_quads(cx, cy, z0, z1, r0, r1, paint, inset=0.0):
    """The eight side faces between two octagons, each shrunk by `inset` along its edges
    (window panes in front of a wall)."""
    g = Geo()
    a, b = octagon(cx, cy, z0, r0), octagon(cx, cy, z1, r1)
    for k in range(8):
        k1 = (k + 1) % 8
        q = [a[k], a[k1], b[k1], b[k]]
        if inset:
            mid0 = [(a[k][i] + a[k1][i]) / 2 for i in range(3)]
            mid1 = [(b[k][i] + b[k1][i]) / 2 for i in range(3)]
            q = [
                [mid0[i] + (a[k][i] - mid0[i]) * (1 - inset) for i in range(3)],
                [mid0[i] + (a[k1][i] - mid0[i]) * (1 - inset) for i in range(3)],
                [mid1[i] + (b[k1][i] - mid1[i]) * (1 - inset) for i in range(3)],
                [mid1[i] + (b[k][i] - mid1[i]) * (1 - inset) for i in range(3)],
            ]
        g.face(q, paint, (cx, cy, (z0 + z1) / 2))
    return g


class Tower:
    def __init__(self, c, r, top, base, mast=None):
        self.cx, self.cy = c
        self.r, self.top, self.base, self.mast = r, top, base, mast
        # Heights: shaft to zs, flared underside to zu, window band to zb, gallery deck to zd,
        # cab to the roof at `top`.
        self.zs = round(top * 0.70, 1)
        self.zu = self.zs + 4
        self.zb = self.zu + 5
        self.zd = self.zb + 1
        self.r_floor = r * 1.9
        self.r_deck = r * 2.05
        self.r_cab0 = r * 1.5
        self.r_cab1 = r * 1.78
        self.r_roof = r * 1.9

    # --- unconditional: shaft and operations building

    def shaft(self):
        g = ring_tube(self.cx, self.cy, [(0, self.r), (self.zs, self.r)], SHAFT, (False, False))
        for x0, x1, y0, y1, h in self.base:
            g.add(block(x0, x1, y0, y1, 0, h, BUILDING))
        return g

    def shaft_detail(self):
        g = Geo()
        cx, cy, r = self.cx, self.cy, self.r
        # Panel seams: storey rings on the shaft and a lit door at the foot.
        z = 6.0
        while z < self.zs - 2:
            ring_lines(g, cx, cy, z, r + 0.05, SEAM)
            z += 6.0
        h0 = min(b[4] for b in self.base)
        for x0, x1, y0, y1, h in self.base:
            # Parapet, two window bands with mullions on all sides and roof plant.
            for px0, px1, py0, py1 in ((x0, x1, y0, y0 + 0.8), (x0, x1, y1 - 0.8, y1), (x0, x0 + 0.8, y0, y1), (x1 - 0.8, x1, y0, y1)):
                g.add(block(px0, px1, py0, py1, h, h + 1.2, ROOF))
            for z0, z1 in ((h * 0.2, h * 0.42), (h * 0.62, h * 0.84)):
                self.window_band(g, x0, x1, y0, y1, z0, z1)
            w, d = x1 - x0, y1 - y0
            g.add(block(x0 + w * 0.15, x0 + w * 0.35, y0 + d * 0.62, y0 + d * 0.74, h, h + 3, SLAB))
            g.add(block(x0 + w * 0.6, x0 + w * 0.72, y0 + d * 0.2, y0 + d * 0.28, h, h + 2.5, SLAB))
        # Glass entrance at the shaft foot, facing the side away from the building.
        far = min(self.base, key=lambda b: abs((b[0] + b[1]) / 2 - self.cx))
        s = -1 if (far[0] + far[1]) / 2 >= self.cx else 1
        x = self.cx + s * (self.r * math.cos(math.radians(22.5)) + 0.1)
        g.face([(x, self.cy - 2.5, 0), (x, self.cy + 2.5, 0), (x, self.cy + 2.5, min(5.0, h0 - 1)), (x, self.cy - 2.5, min(5.0, h0 - 1))], GLASS, (self.cx, self.cy, 2))
        g.add(block(x + s * 0.1 - 0.1, x + s * 3 + 0.1, self.cy - 4, self.cy + 4, min(6.0, h0), min(6.6, h0 + 0.6), SLAB))
        return g

    def window_band(self, g, x0, x1, y0, y1, z0, z1):
        cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
        e = 0.12
        for (ax, ay), (bx, by) in (((x0, y0), (x1, y0)), ((x1, y0), (x1, y1)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0))):
            length = math.hypot(bx - ax, by - ay)
            ux, uy = (bx - ax) / length, (by - ay) / length
            nx, ny = uy, -ux
            m = 2.5
            p0 = (ax + ux * m + nx * e, ay + uy * m + ny * e)
            p1 = (bx - ux * m + nx * e, by - uy * m + ny * e)
            g.face([(*p0, z0), (*p1, z0), (*p1, z1), (*p0, z1)], GLASS, (cx, cy, (z0 + z1) / 2))
            n = max(1, round((length - 2 * m) / 4.0))
            for i in range(n + 1):
                t = i / n
                x, y = p0[0] + (p1[0] - p0[0]) * t + nx * 0.05, p0[1] + (p1[1] - p0[1]) * t + ny * 0.05
                g.line((x, y, z0), (x, y, z1), SEAM)

    # --- intact: operations floor, gallery, cab, roof, masts

    def head(self, simple=False):
        cx, cy, r = self.cx, self.cy, self.r
        g = Geo()
        # Flared underside, window band, gallery deck slab.
        g.add(ring_tube(cx, cy, [(self.zs, r), (self.zu, self.r_floor)], BAND, (False, False)))
        g.add(ring_tube(cx, cy, [(self.zu, self.r_floor), (self.zb, self.r_floor)], BAND, (False, False)))
        g.add(ring_tube(cx, cy, [(self.zb, self.r_deck), (self.zd, self.r_deck)], SLAB))
        if not simple:
            g.add(side_quads(cx, cy, self.zu + 1, self.zb - 0.8, self.r_floor + 0.1, self.r_floor + 0.1, GLASS, inset=0.08))
        # Cab: glass leaning outwards over a sill, roof slab overhanging it, roof deck.
        zc = self.zd + 1.2
        g.add(ring_tube(cx, cy, [(self.zd, self.r_cab0), (zc, self.r_cab0)], SLAB, (False, False)))
        g.add(ring_tube(cx, cy, [(zc, self.r_cab0), (self.top, self.r_cab1)], GLASS, (False, False)))
        g.add(ring_tube(cx, cy, [(self.top, self.r_roof), (self.top + 1.6, self.r_roof)], SLAB))
        g.add(ring_tube(cx, cy, [(self.top + 1.6, r * 0.6), (self.top + 4.5, r * 0.6)], ROOF, (False, True)))
        return g

    def head_detail(self):
        cx, cy, r = self.cx, self.cy, self.r
        g = Geo()
        railing(g, cx, cy, self.zd, self.r_deck - 0.3)
        railing(g, cx, cy, self.top + 1.6, self.r_roof - 0.3, h=1.8)
        # Cab mullions at the corners and mid-panes, window band mullions.
        zc = self.zd + 1.2
        lo, hi = octagon(cx, cy, zc, self.r_cab0 + 0.05), octagon(cx, cy, self.top, self.r_cab1 + 0.05)
        for k in range(8):
            k1 = (k + 1) % 8
            for t in (0.0, 0.5):
                a = [lo[k][i] + (lo[k1][i] - lo[k][i]) * t for i in range(3)]
                b = [hi[k][i] + (hi[k1][i] - hi[k][i]) * t for i in range(3)]
                g.line(a, b, SEAM)
        lo, hi = octagon(cx, cy, self.zu + 1, self.r_floor + 0.15), octagon(cx, cy, self.zb - 0.8, self.r_floor + 0.15)
        for k in range(8):
            k1 = (k + 1) % 8
            for t in (0.25, 0.5, 0.75):
                a = [lo[k][i] + (lo[k1][i] - lo[k][i]) * t for i in range(3)]
                b = [hi[k][i] + (hi[k1][i] - hi[k][i]) * t for i in range(3)]
                g.line(a, b, SEAM)
        # Antennas on the roof: a striped obstacle mast, whips and a dish yard-arm.
        z0 = self.top + 1.6
        sx, sy = cx + r * 1.2, cy - r * 0.9
        h = self.top * 0.3
        n = 6
        for i in range(n):
            g.line((sx, sy, z0 + h * i / n), (sx, sy, z0 + h * (i + 1) / n), STRIPE_RED if i % 2 == 0 else STRIPE_WHITE)
        for dx, dy, hh in ((-1.1, 0.8, 0.42), (-0.4, -1.3, 0.3), (0.9, 1.1, 0.22), (-1.4, -0.5, 0.18)):
            g.line((cx + r * dx, cy + r * dy, z0), (cx + r * dx, cy + r * dy, z0 + self.top * hh), MAST)
        ax, ay = cx - r * 0.3, cy + r * 0.2
        g.line((ax, ay, self.top + 4.5), (ax, ay, self.top + 10), MAST)
        g.line((ax - r * 0.6, ay, self.top + 9), (ax + r * 0.6, ay, self.top + 9), MAST)
        g.add(block(ax - 1.2, ax + 1.2, ay - 1.2, ay + 1.2, self.top + 4.5, self.top + 6.5, SLAB))
        if self.mast:
            x, y, top = self.mast
            zb = max(b[4] for b in self.base)
            for dx, dy in ((-2, -2), (2, -2), (2, 2), (-2, 2)):
                g.line((x + dx, y + dy, zb), (x + dx * 0.4, y + dy * 0.4, top), MAST)
            for i in range(9):
                z = zb + (top - zb) * i / 9
                s = 2 - 1.2 * i / 9
                ring = [(x - s, y - s), (x + s, y - s), (x + s, y + s), (x - s, y + s)]
                for k in range(4):
                    g.line((*ring[k], z), (*ring[(k + 1) % 4], z), MAST)
        return g

    def red_lights(self):
        cx, cy, r = self.cx, self.cy, self.r
        pts = [p for k, p in enumerate(octagon(cx, cy, self.top + 1.7, self.r_roof)) if k % 2 == 0]
        pts.append((cx + r * 1.2, cy - r * 0.9, self.top + 1.6 + self.top * 0.3))
        pts.append((cx - r * 1.1, cy + r * 0.8, self.top + 1.6 + self.top * 0.42))
        if self.mast:
            pts.append(self.mast)
        return pts

    def floodlights(self):
        pts = []
        for x0, x1, y0, y1, h in self.base:
            pts += [(x0, y0, h + 1.3), (x1, y0, h + 1.3), (x1, y1, h + 1.3), (x0, y1, h + 1.3)]
        pts += [p for k, p in enumerate(octagon(self.cx, self.cy, self.zd + 2.3, self.r_deck - 0.3)) if k % 2 == 1]
        return pts

    # --- destroyed: broken shaft top and rubble on the building

    def wreck(self):
        cx, cy, r = self.cx, self.cy, self.r
        g = Geo()
        jag = octagon(cx, cy, self.zs, r)
        tops = [(p[0] + (cx - p[0]) * 0.15, p[1] + (cy - p[1]) * 0.15, self.zs + (3.5 if k % 3 == 0 else 1.0 if k % 2 else 2.2)) for k, p in enumerate(jag)]
        g.add(tube([jag, tops], CHAR, False, True))
        x0, x1, y0, y1, h = max(self.base, key=lambda b: (b[1] - b[0]) * (b[3] - b[2]))
        for fx, fy, s in ((0.3, 0.4, 5), (0.55, 0.6, 4), (0.4, 0.75, 3)):
            x, y = x0 + (x1 - x0) * fx, y0 + (y1 - y0) * fy
            g.add(block(x - s, x + s, y - s * 0.8, y + s * 0.8, h, h + s * 0.6, CHAR, taper=s * 0.4, taper_y=s * 0.3))
        return g


def make(name, parent, geo=None, conds=None, dots=None):
    if geo is not None and geo.faces:
        ob = mklib.link_child(parent, name, mklib.faces_mesh(name, geo))
    elif dots is not None:
        ob = mklib.link_child(parent, name, mklib.points_mesh(name, dots[1], dots[0]))
    else:
        ob = mklib.link_child(parent, name, None)
    if geo is not None:
        for c, segs in sorted(geo.lines.items()):
            n = "%s_lines_pal_%03d" % (name, c)
            mklib.link_child(ob, n, mklib.lines_mesh(n, segs, c))
    if conds:
        ob["conds"] = conds
    return ob


def rebuild_level(lod, t, level):
    """level 0: full detail; 1: no fine detail; 2: simple massing only."""
    name = lod.name
    for c in list(lod.children):
        mklib.delete_tree(c)
    body = t.shaft()
    if level == 2:
        body.add(t.head(simple=True))
    mklib.set_geometry(lod, body, keep=lambda c: False)
    if level == 2:
        return
    if level == 0:
        make(name + "_detail", lod, t.shaft_detail())
    intact = make(name + "_s1v0", lod, t.head(), conds=[[1, 0]])
    if level == 0:
        make(name + "_s1v0_detail", intact, t.head_detail())
    make("%s_s1v0_dots_pal_%03d" % (name, RED_LIGHT), intact, dots=(RED_LIGHT, t.red_lights()))
    make(name + "_s1v1", lod, t.wreck(), conds=[[1, 1]])
    lights = make(name + "_s10v1", lod, conds=[[10, 1]])
    make("%s_s10v1_dots_pal_%03d" % (name, FLOODLIGHT), lights, dots=(FLOODLIGHT, t.floodlights()))


def main():
    stem = os.path.splitext(os.path.basename(bpy.data.filepath))[0]
    spec = TOWERS[stem]
    mklib.load_palette(os.path.join(ROOT, "assets", "palettes", "tod_%s.ron" % stem))
    root = bpy.data.objects["%s_%03d" % (stem, spec["model"])]
    t = Tower(spec["c"], spec["r"], spec["top"], spec["base"], spec.get("mast"))
    lods = sorted((c for c in root.children if "_lod" in c.name), key=lambda c: int(c.name.rsplit("_lod", 1)[1]))
    for k, lod in enumerate(lods):
        if lod.get("radius", 0) == 0:
            continue  # the last level's dot
        rebuild_level(lod, t, min(k, 2))
    bpy.ops.wm.save_mainfile()
    print("control tower rebuilt:", root.name)


main()
