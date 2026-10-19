"""Builds the Ukraine theater's own models in Blender and exports them.

    blender -b --factory-startup --python tools/theater_ua/models.py -- <out.glb>

Writes <out.glb> and <out.glb>.json (model numbers, attributes and detail level distances,
merged into assets/models/ua.glb by assemble.py together with Central Europe's models).

- Sea, lake and land overlays per LOD4 cell (128 km): the coasts, rivers and reservoirs of
  geo.py cut into triangles by a constrained Delaunay triangulation, each triangle coloured by
  what lies under it (sea OR-overlay 49, inland water AND-overlay 57, land map overlay 50).
- Forest patches (LOD2), nuclear power plants (LOD2), ruined town blocks, a rubble pile and
  Donbas slag heaps (LOD1).

Model space: x east, y north, z up, in the units of the level the model is placed on.
"""

import json
import math
import os
import random
import sys

import bpy
from mathutils import Vector
from mathutils.geometry import delaunay_2d_cdt

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(ROOT, "tools", "models"))

import geo  # noqa: E402
import mklib  # noqa: E402
from mklib import Geo, Ramp, block, frustum, hexa, tube  # noqa: E402

LEVEL_M = {1: geo.WU, 2: 4 * geo.WU, 3: 16 * geo.WU, 4: 64 * geo.WU}
CELL_M = {lod: 4096 * m for lod, m in LEVEL_M.items()}

SEA, LAKE, LAND = 49, 57, 50

# Model numbers: Central Europe's 0..93, then these. LOD1/LOD2 models stay below 128 (the
# simulation masks tile shapes with 0x7F); the LOD4 water cells follow from 128.
NPP_SHAPES = {"znpp": 94, "rostov": 95, "south": 96, "kursk": 97, "novovoronezh": 98}
RUINS = [99, 100, 101]
RUBBLE = 102
TERRIKON = 103
FORESTS = [104, 105, 106, 107]
WATER_BASE = 128

CONCRETE = Ramp(0xE1, 0xE6)
WHITE = Ramp(0xE0, 0xE2)
GREY = Ramp(0xE3, 0xEA)
DARK = Ramp(0xE9, 0xEF)
BLUEGREY = Ramp(0xF1, 0xF6)
RUBBLE_C = Ramp(0xE6, 0xEE)
BRICK = Ramp(0xD5, 0xDB)
SOOT = 0xEE
STRIPE = 0xD7


def models_root():
    for o in list(bpy.data.objects):
        bpy.data.objects.remove(o, do_unlink=True)


def empty(name, parent=None):
    ob = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(ob)
    ob.parent = parent
    return ob


def mesh_obj(name, geo_, parent):
    ob = bpy.data.objects.new(name, mklib.faces_mesh(name, geo_))
    bpy.context.scene.collection.objects.link(ob)
    ob.parent = parent
    for c, segs in sorted(geo_.lines.items()):
        ln = "%s_lines_pal_%03d" % (name, c)
        lo = bpy.data.objects.new(ln, mklib.lines_mesh(ln, segs, c))
        bpy.context.scene.collection.objects.link(lo)
        lo.parent = ob
    return ob


META = []


def no_floors(g):
    """Drops the faces under buildings (on the ground, facing down): never seen, and the
    scenery would take a long one for a runway."""
    out = Geo()
    out.lines = g.lines
    out.faces = [(p, c) for p, c in g.faces if not (all(abs(v.z) < 1e-3 for v in p) and mklib.normal(p).z < -0.9)]
    return out


def model(index, lods, attr, child_suffix=""):
    """A root `ua_NNN` with one node per detail level: [(geo, max_dist)]. With a suffix, the
    geometry hangs below each level in a child node of that name (`_s1v0`: a building, `_forest`)."""
    lods = [(no_floors(g), d) for g, d in lods]
    name = "ua_%03d" % index
    root = empty(name)
    info = {"index": index, "name": name, "attr": attr, "lods": []}
    for k, (g, max_dist) in enumerate(lods):
        ln = "%s_lod%d" % (name, k)
        pts = [p for f, _ in g.faces for p in f]
        radius = max((Vector((p.x, p.y, p.z)).length for p in pts), default=0.0)
        if child_suffix:
            node = empty(ln, root)
            mesh_obj(ln + child_suffix, g, node)
        else:
            mesh_obj(ln, g, root)
        info["lods"].append({"name": ln, "max_dist": max_dist, "radius": int(math.ceil(radius))})
    META.append(info)


def flat(pts, c):
    """Upward facing flat polygon (counter-clockwise from above)."""
    pts = [Vector((p[0], p[1], p[2] if len(p) > 2 else 0.0)) for p in pts]
    area = sum(pts[i].x * pts[(i + 1) % len(pts)].y - pts[(i + 1) % len(pts)].x * pts[i].y for i in range(len(pts)))
    if area < 0:
        pts.reverse()
    return pts, c


# ---------------------------------------------------------------------------------------------
# Water and land overlays per LOD4 cell


def clip(a, b, x0, y0, x1, y1):
    """Liang-Barsky: the part of segment a-b inside the rectangle, or None."""
    t0, t1 = 0.0, 1.0
    dx, dy = b[0] - a[0], b[1] - a[1]
    for p, q in ((-dx, a[0] - x0), (dx, x1 - a[0]), (-dy, a[1] - y0), (dy, y1 - a[1])):
        if p == 0:
            if q < 0:
                return None
        else:
            t = q / p
            if p < 0:
                if t > t1:
                    return None
                t0 = max(t0, t)
            else:
                if t < t0:
                    return None
                t1 = min(t1, t)
    return (a[0] + dx * t0, a[1] + dy * t0), (a[0] + dx * t1, a[1] + dy * t1)


def water_cells(water):
    """Overlay geometry of each LOD4 cell (cols/rows -2..5): shape number per cell."""
    cell = CELL_M[4]
    unit = LEVEL_M[4]
    polys = water.polygons()
    shapes = {}
    next_shape = WATER_BASE
    for row in range(-2, 6):
        for col in range(-2, 6):
            x0, y0 = col * cell, row * cell
            x1, y1 = x0 + cell, y0 + cell
            cx, cy = x0 + cell / 2, y0 + cell / 2
            segs = []
            for _, p in polys:
                b = geo.bbox(p)
                if b[2] < x0 or b[0] > x1 or b[3] < y0 or b[1] > y1:
                    continue
                for i in range(len(p)):
                    s = clip(p[i], p[(i + 1) % len(p)], x0, y0, x1, y1)
                    if s and (abs(s[0][0] - s[1][0]) + abs(s[0][1] - s[1][1])) > 1.0:
                        segs.append(s)
            if not segs:
                kind = water.at(cx, cy)
                shapes[(col, row)] = 20 if kind == "sea" else 0
                continue
            loc = lambda q: Vector(((q[0] - cx) / unit, (q[1] - cy) / unit))
            verts, edges, index = [], [], {}

            def vid(q):
                v = loc(q)
                key = (round(v.x, 3), round(v.y, 3))
                if key not in index:
                    index[key] = len(verts)
                    verts.append(v)
                return index[key]

            corners = [vid((x0, y0)), vid((x1, y0)), vid((x1, y1)), vid((x0, y1))]
            for i in range(4):
                edges.append((corners[i], corners[(i + 1) % 4]))
            for a, b in segs:
                ia, ib = vid(a), vid(b)
                if ia != ib:
                    edges.append((ia, ib))
            out = delaunay_2d_cdt(verts, edges, [], 0, 1e-4)
            vco, faces = out[0], out[2]
            g = Geo()
            kinds = set()
            for f in faces:
                pts = [vco[i] for i in f]
                area = (pts[1] - pts[0]).cross(pts[2] - pts[0]) if len(pts) == 3 else 1.0
                if abs(area) < 1e-6:
                    continue
                c = sum(pts, Vector((0, 0))) / len(pts)
                kind = water.at(cx + c.x * unit, cy + c.y * unit)
                kinds.add(kind)
                colour = SEA if kind == "sea" else LAKE if kind == "lake" else LAND
                g.faces.append(flat([(p.x, p.y) for p in pts], colour))
            if kinds == {"sea"}:
                shapes[(col, row)] = 20
                continue
            if kinds == {None}:
                shapes[(col, row)] = 0
                continue
            shape = next_shape
            next_shape += 1
            model(shape, [(merge_flat(g), 32769)], 7)
            shapes[(col, row)] = shape
            print("water cell", col, row, "->", shape, len(g.faces), "triangles", flush=True)
    return shapes


def merge_flat(g):
    """Drops nothing; keeps the faces (Blender merges nothing either). Hook for tuning."""
    return g


# ---------------------------------------------------------------------------------------------
# Forest patches (LOD2: 1.95 m units, cell +-2048)


def blob(cx, cy, r, rng, n=20):
    pts = []
    phase = [rng.uniform(0, math.tau) for _ in range(3)]
    for i in range(n):
        a = math.tau * i / n
        k = 1.0 + 0.22 * math.sin(2 * a + phase[0]) + 0.14 * math.sin(3 * a + phase[1]) + 0.08 * math.sin(5 * a + phase[2])
        pts.append((cx + math.cos(a) * r * k, cy + math.sin(a) * r * k * rng.uniform(0.85, 1.0)))
    return pts


def forest_patches():
    """Four patch sets of rising cover. The colour mix gives woods some texture from afar."""
    for k, shape in enumerate(FORESTS):
        rng = random.Random(4100 + k)
        g = Geo()
        count = [3, 5, 7, 9][k]
        for i in range(count):
            r = rng.uniform(150, 420) * (1.0 + 0.35 * k)
            cx, cy = rng.uniform(-1700 + r * 0.5, 1700 - r * 0.5), rng.uniform(-1700 + r * 0.5, 1700 - r * 0.5)
            g.faces.append(flat(blob(cx, cy, r, rng), rng.choice([0xAB, 0xAC, 0xAC, 0xAD])))
        model(shape, [(g, 32769)], 6, "_forest")


# ---------------------------------------------------------------------------------------------
# Buildings, in metres (scaled to the level's unit at the end)


def scaled(g, unit):
    return g.scaled(1.0 / unit)


def dome_unit(x, y, g, lod):
    """VVER containment: cylinder with a shallow dome, on its auxiliary building."""
    n = 16 if lod == 0 else 8
    g.add(block(x - 34, x + 34, y - 30, y + 30, 0, 22, GREY))
    g.add(frustum((x, y, 0), (x, y, 46), 23, 23, WHITE, n, caps=False))
    rings = [mklib.ring((x, y, 46 + 14 * math.sin(math.radians(a))), 23 * math.cos(math.radians(a)), 23 * math.cos(math.radians(a)), n,
                        axis=(0, 0, 1)) for a in (0, 30, 60, 85)]
    g.add(tube(rings, WHITE, cap0=False, cap1=True))


def turbine_hall(x0, x1, y0, y1, h, g, paint=BLUEGREY):
    g.add(block(x0, x1, y0, y1, 0, h, paint))
    g.add(block(x0 + 4, x1 - 4, y0 + 4, y1 - 4, h, h + 3, GREY))


def stack(x, y, h, g, r=3.5, striped=True, n=8):
    g.add(frustum((x, y, 0), (x, y, h * 0.8), r * 1.3, r, CONCRETE, n, caps=False))
    g.add(frustum((x, y, h * 0.8), (x, y, h), r, r * 0.9, STRIPE if striped else CONCRETE, n, caps=True))


def cooling_tower(x, y, h, g, lod, r=None):
    """Hyperboloid shell, open at the top, its inside showing from above."""
    r = r or h * 0.4
    n = 16 if lod == 0 else 8
    prof = [(0.0, 1.0), (0.25, 0.86), (0.55, 0.68), (0.8, 0.62), (1.0, 0.66)]
    rings = [mklib.ring((x, y, h * t), r * s, r * s, n, axis=(0, 0, 1)) for t, s in prof]
    shell = tube(rings, Ramp(0xE1, 0xE7), cap0=False, cap1=False)
    g.add(shell)
    inner = Geo()
    inner.faces = [(list(reversed(p)), 0xE9) for p, _ in shell.faces]
    g.add(inner)
    # Water basin at the foot, seen through the opening.
    g.faces.append(flat(mklib.ring((x, y, h * 0.3), r * 0.7, r * 0.7, n, axis=(0, 0, 1)), 0xEB))


def pad(x0, y0, x1, y1, g, c=0xE4):
    g.faces.append(flat([(x0, y0, 0.05), (x1, y0, 0.05), (x1, y1, 0.05), (x0, y1, 0.05)], c))


def pond(pts, g):
    g.faces.append(flat([(p[0], p[1], 0.06) for p in pts], LAKE))


def spray_pools(x, y, g, n=4):
    for i in range(n):
        px = x + (i % 2) * 140
        py = y + (i // 2) * 110
        g.faces.append(flat([(px, py, 0.08), (px + 120, py, 0.08), (px + 120, py + 90, 0.08), (px, py + 90, 0.08)], LAKE))


def npp(kind, lod):
    g = Geo()
    if kind == "znpp":
        # Six VVER-1000 units in a row along the turbine hall, two cooling towers, spray
        # pools, and the cooling pond reaching to the old reservoir bed.
        pad(-560, -260, 560, 260, g)
        for i in range(6):
            x = -450 + i * 180
            dome_unit(x, 70, g, lod)
            stack(x + 45, 40, 100, g, striped=False)
        turbine_hall(-520, 520, -110, -30, 34, g)
        cooling_tower(-380, -420, 150, g, lod)
        cooling_tower(-140, -420, 150, g, lod)
        if lod == 0:
            spray_pools(120, -480, g, 6)
            block_row(g, -520, 210, 8, 60, 30, 14)
        pond([(-1500, 300), (-600, 380), (-600, 1700), (-1500, 1500)], g)
    elif kind == "rostov":
        pad(-420, -240, 420, 240, g)
        for i in range(4):
            x = -300 + i * 200
            dome_unit(x, 60, g, lod)
            stack(x + 45, 30, 100, g, striped=False)
        turbine_hall(-380, 380, -110, -30, 32, g)
        for i in range(2):
            cooling_tower(-200 + i * 400, -420, 170, g, lod)
        pond([(-1400, 400), (1400, 450), (1400, 1600), (-1400, 1600)], g)
    elif kind == "south":
        pad(-360, -220, 360, 220, g)
        for i in range(3):
            x = -220 + i * 220
            dome_unit(x, 60, g, lod)
            stack(x + 45, 30, 100, g, striped=False)
        turbine_hall(-320, 320, -110, -30, 32, g)
        cooling_tower(-120, -420, 150, g, lod)
        cooling_tower(160, -420, 150, g, lod)
        if lod == 0:
            spray_pools(-420, -480, g, 4)
        pond([(500, -700), (1600, -600), (1700, 400), (520, 300)], g)
    elif kind == "kursk":
        # RBMK: four boxy reactor buildings in two pairs, striped stacks, a long turbine
        # hall, the cooling pond south.
        pad(-640, -240, 640, 260, g)
        for i in range(4):
            x = -480 + i * 320
            g.add(block(x - 55, x + 55, 30, 140, 0, 72, GREY))
            g.add(block(x - 40, x + 40, 50, 120, 72, 82, CONCRETE))
            if i % 2 == 0:
                stack(x + 160, 190, 150, g, r=4.5)
        turbine_hall(-600, 600, -100, 10, 40, g, GREY)
        pond([(-1800, -500), (1800, -500), (1800, -1800), (-1800, -1800)], g)
    elif kind == "novovoronezh":
        # Two VVER-1200 with their tall cooling towers, the older blocks with smaller ones.
        pad(-560, -300, 560, 280, g)
        for i in range(2):
            x = 260 + i * 200
            dome_unit(x, 80, g, lod)
        turbine_hall(180, 540, -110, -20, 34, g)
        cooling_tower(260, -460, 171, g, lod)
        cooling_tower(500, -460, 171, g, lod)
        for i in range(3):
            x = -440 + i * 170
            g.add(block(x - 50, x + 50, 20, 120, 0, 48, GREY))
            g.add(block(x - 36, x + 36, 36, 104, 48, 60, CONCRETE))
        turbine_hall(-520, 0, -110, -10, 30, g)
        for i in range(4):
            cooling_tower(-480 + i * 140, -380, 95, g, lod)
        stack(-120, 160, 120, g)
    return g


def block_row(g, x0, y, n, w, d, h):
    for i in range(n):
        x = x0 + i * (w + 20)
        g.add(block(x, x + w, y, y + d, 0, h, GREY))


# ---------------------------------------------------------------------------------------------
# Ruins (LOD1: 0.49 m units)


def ruined_block(g, cx, cy, L, W, H, rng, lod, angle=0.0):
    """A gutted apartment block: broken wall panels with jagged tops and gaps, slabs hanging
    from the floors that still stand, a debris spill along the walls, soot over the windows."""
    ca, sa = math.cos(angle), math.sin(angle)
    P = lambda x, y, z: (cx + x * ca - y * sa, cy + x * sa + y * ca, z)
    t = 0.5
    floors = max(1, int(H / 3.0))
    sides = [((-L / 2, -W / 2), (L / 2, -W / 2)), ((L / 2, -W / 2), (L / 2, W / 2)), ((L / 2, W / 2), (-L / 2, W / 2)),
             ((-L / 2, W / 2), (-L / 2, -W / 2))]
    # Collapse profile along the block: one end may stand, the middle often fell in.
    standing = rng.uniform(0.25, 1.0)
    for (ax, ay), (bx, by) in sides:
        length = math.hypot(bx - ax, by - ay)
        n = max(1, int(length / (4.0 if lod == 0 else 9.0)))
        dx, dy = (bx - ax) / length, (by - ay) / length
        nx, ny = dy, -dx
        for i in range(n):
            if rng.random() < 0.18:
                continue  # blown out
            s0, s1 = i * length / n, (i + 1) * length / n
            mid = (ax + dx * (s0 + s1) / 2) / max(L, 1) + 0.5
            hmax = H * (0.25 + 0.75 * standing * (1.0 - 0.6 * math.sin(mid * math.pi) * rng.random()))
            h0 = hmax * rng.uniform(0.35, 1.0)
            h1 = hmax * rng.uniform(0.35, 1.0)
            p0 = (ax + dx * s0, ay + dy * s0)
            p1 = (ax + dx * s1, ay + dy * s1)
            wall_c = rng.choice([0xE3, 0xE4, 0xE5, 0xE6]) if rng.random() < 0.8 else SOOT
            pts = [P(p0[0], p0[1], 0), P(p1[0], p1[1], 0), P(p1[0], p1[1], h1), P(p0[0], p0[1], h0)]
            g.face(pts, wall_c, P(-nx * 5 + (p0[0] + p1[0]) / 2, -ny * 5 + (p0[1] + p1[1]) / 2, 0))
            inside = [P(p0[0] - nx * t, p0[1] - ny * t, 0), P(p1[0] - nx * t, p1[1] - ny * t, 0),
                      P(p1[0] - nx * t, p1[1] - ny * t, h1), P(p0[0] - nx * t, p0[1] - ny * t, h0)]
            g.face(inside, 0xE9, P(nx * 5 + (p0[0] + p1[0]) / 2, ny * 5 + (p0[1] + p1[1]) / 2, 0))
            # Soot over the windows.
            if lod == 0 and rng.random() < 0.5 and min(h0, h1) > 3.5:
                z = rng.uniform(1.5, min(h0, h1) - 2.0)
                sx0, sx1 = 0.2, 0.8
                q = lambda f, zz: P(p0[0] + (p1[0] - p0[0]) * f + nx * 0.05, p0[1] + (p1[1] - p0[1]) * f + ny * 0.05, zz)
                g.face([q(sx0, z), q(sx1, z), q(sx1, z + 1.8), q(sx0, z + 1.8)], SOOT,
                       P(-nx * 5 + (p0[0] + p1[0]) / 2, -ny * 5 + (p0[1] + p1[1]) / 2, z))
    # Floor slabs: some still level, some hanging down at an angle.
    if lod == 0:
        for f in range(1, floors):
            z = f * 3.0
            if z > H * standing:
                break
            if rng.random() < 0.55:
                x0 = rng.uniform(-L / 2, 0)
                x1 = x0 + rng.uniform(L * 0.2, L * 0.5)
                drop = rng.uniform(0, 2.5) if rng.random() < 0.5 else 0
                slab = [P(x0, -W / 2 + 0.5, z), P(x1, -W / 2 + 0.5, z - drop), P(x1, W / 2 - 0.5, z - drop * 0.3),
                        P(x0, W / 2 - 0.5, z)]
                g.face(slab, 0xE7, P((x0 + x1) / 2, 0, z + 10))
                g.face(list(reversed(slab)), 0xEA, P((x0 + x1) / 2, 0, z - 10))
    debris(g, P, L, W, H * (1.0 - standing) * 0.35 + 1.2, rng, lod)


def debris(g, P, L, W, h, rng, lod):
    """Rubble mounds inside and around a footprint."""
    n = 5 if lod == 0 else 2
    for _ in range(n):
        x, y = rng.uniform(-L / 2, L / 2), rng.uniform(-W / 2 - 3, W / 2 + 3)
        r = rng.uniform(3, 7) * (1.4 if lod else 1.0)
        hh = h * rng.uniform(0.5, 1.2)
        ring = [P(x + math.cos(a) * r * rng.uniform(0.7, 1.2), y + math.sin(a) * r * rng.uniform(0.7, 1.2), 0)
                for a in [k * math.tau / 6 for k in range(6)]]
        top = P(x + rng.uniform(-1, 1), y + rng.uniform(-1, 1), hh)
        for k in range(6):
            g.face([ring[k], ring[(k + 1) % 6], top], RUBBLE_C, P(x, y, -5))


def ruined_house(g, cx, cy, rng, lod):
    L, W = rng.uniform(8, 12), rng.uniform(7, 9)
    a = rng.uniform(0, math.pi)
    ruined_block(g, cx, cy, L, W, rng.uniform(2.5, 5.5), rng, lod, a)
    # The roof came down inside.
    ca, sa = math.cos(a), math.sin(a)
    P = lambda x, y, z: (cx + x * ca - y * sa, cy + x * sa + y * ca, z)
    roof = [P(-L / 2 + 1, -W / 2 + 1, 0.4), P(L / 2 - 1, -W / 2 + 1, 2.2), P(L / 2 - 1, W / 2 - 1, 1.2), P(-L / 2 + 1, W / 2 - 1, 0.3)]
    g.face(roof, rng.choice([0xD8, 0xD9, 0xE8]), P(0, 0, 20))


def wrecked_vehicle(g, cx, cy, rng):
    a = rng.uniform(0, math.pi)
    body = block(-3.2, 3.2, -1.5, 1.5, 0.3, 1.9, Ramp(0xEB, 0xEF)).rotated_z(a)
    g.add(body.scaled(1.0, (cx, cy, 0)))


def ruin_cluster(seed, lod):
    """A town quarter in ruins, about 300 m across."""
    rng = random.Random(seed)
    g = Geo()
    blocks = [(-70, 40, 0.0), (45, 60, math.pi / 2), (-20, -60, 0.1), (80, -40, 0.0)]
    for k, (x, y, a) in enumerate(blocks):
        if seed % 3 == 2 and k % 2:
            continue
        ruined_block(g, x + rng.uniform(-10, 10), y + rng.uniform(-10, 10), rng.uniform(40, 70), rng.uniform(12, 15),
                     rng.uniform(12, 27), rng, lod, a + rng.uniform(-0.1, 0.1))
    houses = 6 if lod == 0 else 3
    for _ in range(houses):
        ruined_house(g, rng.uniform(-140, 140), rng.uniform(-140, 140), rng, lod)
    if lod == 0:
        for _ in range(3):
            wrecked_vehicle(g, rng.uniform(-120, 120), rng.uniform(-120, 120), rng)
        # Scorched ground and craters.
        for _ in range(6):
            x, y, r = rng.uniform(-150, 150), rng.uniform(-150, 150), rng.uniform(4, 9)
            g.faces.append(flat([(x + math.cos(a) * r, y + math.sin(a) * r, 0.1) for a in [k * math.tau / 7 for k in range(7)]],
                                rng.choice([0xEC, 0xED, 0xCF])))
    return scaled(g, LEVEL_M[1])


def rubble_pile(lod):
    rng = random.Random(77)
    g = Geo()
    P = lambda x, y, z: (x, y, z)
    debris(g, P, 30, 22, 4.0, rng, lod)
    debris(g, P, 18, 14, 6.0, rng, lod)
    for _ in range(3):
        a = rng.uniform(0, math.pi)
        slab = block(-6, 6, -0.3, 0.3, 0, 4, GREY).rotated_x(rng.uniform(0.3, 1.0)).rotated_z(a)
        g.add(slab.scaled(1.0, (rng.uniform(-8, 8), rng.uniform(-8, 8), 0)))
    return scaled(g, LEVEL_M[1])


def terrikon(lod):
    """Slag heap of the Donbas mines: a dark cone, the top a little flattened."""
    rng = random.Random(12)
    n = 12 if lod == 0 else 7
    h, r = 60.0, 140.0
    rings = []
    for t, s in ((0.0, 1.0), (0.45, 0.62), (0.85, 0.22), (1.0, 0.1)):
        rings.append([(math.cos(a) * r * s * rng.uniform(0.92, 1.06), math.sin(a) * r * s * rng.uniform(0.92, 1.06), h * t)
                      for a in [k * math.tau / n for k in range(n)]])
    g = tube(rings, Ramp(0xEC, 0xEF), cap0=False, cap1=True)
    return scaled(g, LEVEL_M[1])


def main():
    out = sys.argv[sys.argv.index("--") + 1]
    mklib.load_palette(os.path.join(ROOT, "assets", "palettes", "tod_ce.ron"))
    models_root()
    forest_patches()
    for kind, shape in NPP_SHAPES.items():
        unit = LEVEL_M[2]
        model(shape, [(scaled(npp(kind, 0), unit), 6000), (scaled(npp(kind, 1), unit), 32769)], 6, "_s1v0")
    for k, shape in enumerate(RUINS):
        model(shape, [(ruin_cluster(500 + k, 0), 2500), (ruin_cluster(500 + k, 1), 32769)], 5, "_s1v0")
    model(RUBBLE, [(rubble_pile(0), 2048), (rubble_pile(1), 32769)], 3, "_s1v0")
    model(TERRIKON, [(terrikon(0), 6000), (terrikon(1), 32769)], 6, "_s1v0")
    shapes = water_cells(geo.Water())
    bpy.ops.export_scene.gltf(
        filepath=out, export_format="GLB", export_extras=True, export_yup=True, export_texcoords=False,
        export_normals=False, export_materials="EXPORT", use_visible=False, export_animations=False,
        use_mesh_edges=True, use_mesh_vertices=True,
    )
    with open(out + ".json", "w") as f:
        json.dump({"models": META, "lod4": {"%d,%d" % k: v for k, v in shapes.items()}}, f, indent=1)


main()
