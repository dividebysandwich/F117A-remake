"""Detail for the original buildings, ships and structures, and the STFLT stores remodels.

Run by build.py on the open .blend master. Added geometry goes into `<node>_detail` children;
replaced faces, lines and moved lights keep their original meshes as unexported `orig:<object>`
meshes, so every build starts from the original models again and its output does not change
between runs. Models remodelled by ground.py are left alone, as are hills, terrain cells and
natural features.

Theater and photo models keep their silhouettes (collision hulls and LOD changes still fit) and
get, as their shapes call for:
- windows in rows per storey, a door with a canopy on the widest ground-level wall, sliding
  doors on the end walls of halls, roof seams on large pitched roofs, chimneys on houses;
- parapets and rooftop plant on flat roofs, railings, ladders and bands on tanks and stacks;
- support columns under free-standing canopies, guy ropes and door flaps on tents, a stiffening
  frame on the back of lone panels, lattice towers around vertical masts and pylons, railings
  along raised decks;
- ships: a lofted hull with a raked bow, flare, sheer, boot-top, gunwale rail, hawse pipes and
  portholes in place of the boxy one (superstructure kept above the main deck), windows on the
  superstructure; aircraft carriers keep their flight deck clear of railings.

Lights stay: lit-window dots (colour 148) move onto the new windows, roof lights are lifted above
the parapets. STFLT models are rebuilt by stores.py (the F-117 keeps its original shell).
"""

import math
import os
import random
import sys

import bpy
from mathutils import Vector
from mathutils.geometry import intersect_point_tri

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import ground  # noqa: E402
import mklib  # noqa: E402
from mklib import Geo, Ramp  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))

U = 10.0  # model units per (exaggerated) metre at level 1; set per model by `detail_model`
UP = Vector((0, 0, 1))
GLASS = 154
LINE = 230
WIN_OFF = 0.04 * U


def darker(c, k=3):
    """A darker shade of colour byte `c` along its 16-colour ramp."""
    if c >= 160 or 112 <= c < 128:
        return min(c + k, (c | 15))
    if 224 <= c <= 239:
        return min(c + k, 239)
    return 238


# ---------------------------------------------------------------------------------------------
# Reading meshes into coplanar faces


def key(p):
    return (round(p.x * 20), round(p.y * 20), round(p.z * 20))


class Face:
    def __init__(self, tris, colour):
        self.tris = tris
        self.colour = colour
        a, b, c = tris[0]
        self.n = (b - a).cross(c - a).normalized()
        self.d = self.n.dot(a)
        nz = self.n.z
        self.kind = "wall" if abs(nz) < 0.3 else "roof" if nz > 0.97 else "slope" if nz > 0.15 else "other"
        pts = [p for t in tris for p in t]
        self.zmin = min(p.z for p in pts)
        self.zmax = max(p.z for p in pts)
        self.area = sum((t[1] - t[0]).cross(t[2] - t[0]).length / 2 for t in tris)
        count = {}
        pos = {}
        for t in tris:
            for i in range(3):
                a, b = t[i], t[(i + 1) % 3]
                k = frozenset((key(a), key(b)))
                count[k] = count.get(k, 0) + 1
                pos[k] = (a, b)
        self.boundary = [pos[k] for k, n in count.items() if n == 1]
        self.keys = {key(p) for p in pts}
        self.centroid = sum(pts, Vector()) / len(pts)

    def contains(self, p, tol=None):
        tol = 0.05 * U if tol is None else tol
        if abs(self.n.dot(p) - self.d) > tol:
            return False
        return any(intersect_point_tri(p, *t) is not None for t in self.tris)


def read_faces(tris):
    """Coplanar same-coloured faces from a list of ((a, b, c), colour byte) triangles."""
    tris = [(t, c) for t, c in tris if (t[1] - t[0]).cross(t[2] - t[0]).length > 1e-6]
    # Union adjacent coplanar triangles of one colour.
    parent = list(range(len(tris)))

    def find(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    normals = []
    for t, _ in tris:
        n = (t[1] - t[0]).cross(t[2] - t[0]).normalized()
        normals.append((n, n.dot(t[0])))
    edges = {}
    for i, (t, _) in enumerate(tris):
        for k in range(3):
            edges.setdefault(frozenset((key(t[k]), key(t[(k + 1) % 3]))), []).append(i)
    for idx in edges.values():
        for a in idx:
            for b in idx:
                if a < b and tris[a][1] == tris[b][1]:
                    (na, da), (nb, db) = normals[a], normals[b]
                    if na.dot(nb) > 0.999 and abs(da - db) < 0.05 * U:
                        parent[find(a)] = find(b)
    groups = {}
    for i in range(len(tris)):
        groups.setdefault(find(i), []).append(i)
    return [Face([tris[i][0] for i in g], tris[g[0]][1]) for g in groups.values()]


def components(faces):
    parent = list(range(len(faces)))

    def find(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    owner = {}
    for i, f in enumerate(faces):
        for k in f.keys:
            if k in owner:
                parent[find(i)] = find(owner[k])
            else:
                owner[k] = i
    out = {}
    for i in range(len(faces)):
        out.setdefault(find(i), []).append(faces[i])
    return list(out.values())


# ---------------------------------------------------------------------------------------------
# Detail generators


class Detail:
    def __init__(self, rng, ship=False):
        self.geo = Geo()
        self.rng = rng
        self.ship = ship
        self.windows = []  # (centre, normal)
        self.parapets = []  # (roof face, height)

    # -- walls

    def frame(self, f):
        nh = Vector((f.n.x, f.n.y, 0)).normalized()
        t = Vector((-nh.y, nh.x, 0))
        ss = [p.dot(t) for tr in f.tris for p in tr]
        return t, min(ss), max(ss)

    def at(self, f, t, s, z, off=None):
        off = WIN_OFF if off is None else off
        nh = Vector((f.n.x, f.n.y, 0)).normalized()
        p = t * s + UP * z
        # Slide along the horizontal normal onto the face plane.
        k = (f.d - f.n.dot(p)) / max(f.n.dot(nh), 1e-6)
        return p + nh * k + f.n * off

    def fits(self, f, t, s0, s1, z0, z1, margin=None):
        margin = 0.25 * U if margin is None else margin
        for s in (s0 - margin, s1 + margin):
            for z in (z0 - margin, z1 + margin):
                if not f.contains(self.at(f, t, s, z, 0.0)):
                    return False
        return True

    def rect(self, f, t, s0, s1, z0, z1, colour, off=None):
        off = WIN_OFF if off is None else off
        pts = [self.at(f, t, s0, z0, off), self.at(f, t, s1, z0, off), self.at(f, t, s1, z1, off), self.at(f, t, s0, z1, off)]
        self.geo.face(pts, colour, f.centroid - f.n * U)

    def windows_on(self, f, comp_z0, skip=(), decals=()):
        t, s0, s1 = self.frame(f)
        w, h = s1 - s0, f.zmax - f.zmin
        if h < 2.0 * U or w < 1.6 * U:
            return
        ground = f.zmin - comp_z0 < 0.3 * U
        if self.ship and ground:
            # Hull: one row of portholes.
            z = f.zmin + 0.62 * h
            r = min(0.25 * U, 0.12 * h)
            n = int((w - 2 * U) / (2.2 * U))
            for i in range(n):
                s = s0 + (w - (n - 1) * 2.2 * U) / 2 + i * 2.2 * U
                if self.fits(f, t, s - r, s + r, z - r, z + r, 0.1 * U):
                    self.rect(f, t, s - r, s + r, z - r, z + r, GLASS)
            return
        if h < 4.5 * U:
            wh = min(1.5 * U, 0.4 * h)
            rows = [f.zmin + 0.55 * h - wh / 2]
        else:
            wh = 1.5 * U
            rows = []
            z = f.zmin + 1.0 * U
            while z + wh + 0.5 * U <= f.zmax:
                rows.append(z)
                z += 3.2 * U
        pitch, ww = 2.8 * U, 1.2 * U
        if w > 60 * U:
            pitch = w / 22
        n = max(1, int((w - 0.8 * U) / pitch))
        start = s0 + (w - (n - 1) * pitch) / 2
        for z in rows:
            for i in range(n):
                s = start + i * pitch
                a, b = s - ww / 2, s + ww / 2
                if any(a < k1 + 0.3 * U and b > k0 - 0.3 * U and z < kz for k0, k1, kz in skip):
                    continue
                if self.fits(f, t, a, b, z, z + wh) and not any(
                        o.contains(self.at(f, t, x, y, 0.0), 0.1 * U) for o in decals for x in (a, b) for y in (z, z + wh)):
                    self.rect(f, t, a, b, z, z + wh, GLASS)
                    if not self.ship and wh > U:
                        # Mullion
                        self.geo.line(self.at(f, t, s, z, 2 * WIN_OFF), self.at(f, t, s, z + wh, 2 * WIN_OFF), darker(f.colour, 4))
                    self.windows.append((self.at(f, t, s, z + wh / 2, 3 * WIN_OFF), f.n))

    def door(self, f, big=False):
        t, s0, s1 = self.frame(f)
        w, h = s1 - s0, f.zmax - f.zmin
        sc = (s0 + s1) / 2
        dark = darker(f.colour, 6)
        if big:
            for k in (0.8, 0.7, 0.6, 0.5, 0.4):
                dw, dh = k * w / 2, min(0.85 * k * h / 0.8, h - 0.6 * U)
                if self.fits(f, t, sc - dw, sc + dw, f.zmin, f.zmin + dh, 0.1 * U) or self.fits(f, t, sc - dw, sc + dw, f.zmin + 0.15 * U, f.zmin + dh, 0.1 * U):
                    self.rect(f, t, sc - dw, sc + dw, f.zmin, f.zmin + dh, dark)
                    n = max(2, int(2 * dw / (2.5 * U)))
                    for i in range(1, n):
                        s = sc - dw + 2 * dw * i / n
                        self.geo.line(self.at(f, t, s, f.zmin, 2 * WIN_OFF), self.at(f, t, s, f.zmin + dh, 2 * WIN_OFF), darker(dark, 3))
                    self.geo.line(self.at(f, t, sc - dw, f.zmin + dh, 2 * WIN_OFF), self.at(f, t, sc + dw, f.zmin + dh, 2 * WIN_OFF), darker(dark, 3))
                    return (sc - dw, sc + dw, f.zmin + dh)
            return None
        dw = 1.3 * U if w > 12 * U else 0.6 * U
        dh = min(2.3 * U, 0.7 * h)
        if self.fits(f, t, sc - dw, sc + dw, f.zmin + 0.3 * U, f.zmin + dh, 0.1 * U):
            self.rect(f, t, sc - dw, sc + dw, f.zmin, f.zmin + dh, dark)
            # Canopy over the door.
            a, b = self.at(f, t, sc - dw - 0.3 * U, f.zmin + dh + 0.2 * U, 0), self.at(f, t, sc + dw + 0.3 * U, f.zmin + dh + 0.2 * U, 0)
            out = Vector((f.n.x, f.n.y, 0)).normalized() * 0.8 * U
            self.geo.face2([a, b, b + out, a + out], darker(f.colour, 2))
            return (sc - dw, sc + dw, f.zmin + dh)
        return None

    # -- roofs

    def parapet(self, f, h, colour):
        th = 0.25 * U
        c = f.centroid
        for a, b in f.boundary:
            e = b - a
            if e.length < 0.05 * U:
                continue
            o = e.cross(UP).normalized()
            mid = (a + b) / 2
            if f.contains(mid + o * 0.1 * U):
                o = -o
            u = e.normalized() * th
            ia, ib = a - o * th - u, b - o * th + u
            hz = UP * h
            g = self.geo
            g.face([a, b, b + hz, a + hz], colour, mid - o * U)
            g.face([ib, ia, ia + hz, ib + hz], darker(colour, 2), mid + o * U)
            g.face([a + hz, b + hz, ib + hz, ia + hz], colour, mid + hz - UP * U)
        self.parapets.append((f, h))

    def plant(self, f, n):
        """Rooftop boxes (air handling, stair housings) inside a flat roof."""
        pts = [p for t in f.tris for p in t]
        x0, x1 = min(p.x for p in pts), max(p.x for p in pts)
        y0, y1 = min(p.y for p in pts), max(p.y for p in pts)
        placed = []
        z = f.zmax
        for _ in range(n * 8):
            if len(placed) >= n:
                break
            sx = self.rng.uniform(0.8, 2.2) * U
            sy = self.rng.uniform(0.8, 2.2) * U
            sx, sy = min(sx, (x1 - x0) / 3), min(sy, (y1 - y0) / 3)
            if sx < 0.4 * U or sy < 0.4 * U:
                return
            cx = self.rng.uniform(x0 + sx, x1 - sx)
            cy = self.rng.uniform(y0 + sy, y1 - sy)
            m = 0.6 * U
            corners = [Vector((cx + dx * (sx / 2 + m), cy + dy * (sy / 2 + m), z)) for dx in (-1, 1) for dy in (-1, 1)]
            if not all(f.contains(p) for p in corners):
                continue
            if any(abs(cx - px) < (sx + qx) / 2 + 0.4 * U and abs(cy - py) < (sy + qy) / 2 + 0.4 * U for px, py, qx, qy in placed):
                continue
            placed.append((cx, cy, sx, sy))
            h = self.rng.uniform(0.6, 1.4) * U
            self.geo.add(mklib.block(cx - sx / 2, cx + sx / 2, cy - sy / 2, cy + sy / 2, z, z + h, Ramp(225, 234)))
            if self.rng.random() < 0.5:
                # Vent pipe
                r = 0.15 * U
                self.geo.add(mklib.frustum((cx, cy, z + h), (cx, cy, z + h + 0.6 * U), r, r, 233, 4))

    def railing(self, f, h, colour, posts, edges=None):
        for a, b in (f.boundary if edges is None else edges):
            e = b - a
            if e.length < 0.1 * U:
                continue
            hz = UP * h
            self.geo.line(a + hz, b + hz, colour)
            self.geo.line(a + hz * 0.5, b + hz * 0.5, colour)
            n = max(1, int(e.length / posts))
            for i in range(n + 1):
                p = a + e * (i / n)
                self.geo.line(p, p + hz, colour)

    def frame_back(self, f):
        """Stiffening grid and a strut on the back of a lone panel."""
        if f.area < (2 * U) ** 2:
            return
        t, s0, s1 = self.frame(f)
        z0, z1 = f.zmin, f.zmax
        c = darker(f.colour, 5)
        back = -0.3 * U
        nx = max(1, int((s1 - s0) / (2.5 * U)))
        nz = max(1, int((z1 - z0) / (2.5 * U)))
        for i in range(nx + 1):
            s = s0 + (s1 - s0) * i / nx
            pts = [self.at(f, t, s, z0 + (z1 - z0) * j / 8, back) for j in range(9)]
            inside = [p for p in pts if f.contains(p - f.n * back, 0.2 * U)]
            if len(inside) >= 2:
                self.geo.line(inside[0], inside[-1], c)
        for j in range(nz + 1):
            z = z0 + (z1 - z0) * j / nz
            pts = [self.at(f, t, s0 + (s1 - s0) * i / 8, z, back) for i in range(9)]
            inside = [p for p in pts if f.contains(p - f.n * back, 0.2 * U)]
            if len(inside) >= 2:
                self.geo.line(inside[0], inside[-1], c)

    def ladder(self, w, z0, z1):
        t, s0, s1 = self.frame(w)
        s = (s0 + s1) / 2
        for k in (-0.3 * U, 0.3 * U):
            self.geo.line(self.at(w, t, s + k, z0, 0.3 * U), self.at(w, t, s + k, z1, 0.3 * U), LINE)
        for k in range(int((z1 - z0) / (0.6 * U))):
            z = z0 + 0.3 * U + k * 0.6 * U
            self.geo.line(self.at(w, t, s - 0.3 * U, z, 0.3 * U), self.at(w, t, s + 0.3 * U, z, 0.3 * U), LINE)

    def stack(self, comp, z0, z1):
        """Dark bands near the top of a slim stack, and a ladder up one side."""
        sides = [f for f in comp if abs(f.n.z) < 0.6 and f.zmax - f.zmin > 0.5 * (z1 - z0)]
        for f in sides:
            t, s0, s1 = self.frame(f)
            for zb in (z1 - 0.12 * (z1 - z0), z1 - 0.3 * (z1 - z0)):
                band = 0.04 * (z1 - z0)
                # The face narrows towards the top: find its extent at this height.
                ss = []
                for tr in f.tris:
                    for k in range(3):
                        a, b = tr[k], tr[(k + 1) % 3]
                        for z in (zb, zb + band):
                            if (a.z - z) * (b.z - z) <= 0 and abs(a.z - b.z) > 1e-6:
                                ss.append((a + (b - a) * ((z - a.z) / (b.z - a.z))).dot(t))
                if len(ss) < 2:
                    continue
                a, b = min(ss), max(ss)
                if b - a < 1e-3:
                    continue
                pts = [self.at(f, t, a, zb, 0.05 * U), self.at(f, t, b, zb, 0.05 * U),
                       self.at(f, t, b, zb + band, 0.05 * U), self.at(f, t, a, zb + band, 0.05 * U)]
                # Pull the corners onto the face (it may lean).
                self.geo.face(pts, darker(f.colour, 5) if f.colour not in (13, 146, 147) else 239, f.centroid - f.n * U)
        if sides:
            w = min(sides, key=lambda f: f.n.y)
            self.ladder(w, w.zmin + 0.5 * U, w.zmin + 0.85 * (w.zmax - w.zmin))

    def columns(self, roof):
        """Posts under a roof that floats on nothing (canopies, shelters)."""
        corners = []
        for a, _ in roof.boundary:
            if all((a - c).length > 2 * U for c in corners):
                corners.append(a)
        if len(corners) > 12:
            corners = corners[::(len(corners) + 11) // 12]
        c = roof.centroid
        r = 0.22 * U
        for p in corners:
            q = p + (c - p).normalized() * 0.5 * U
            self.geo.add(mklib.block(q.x - r, q.x + r, q.y - r, q.y + r, 0.0, roof.zmin - 0.05 * U, Ramp(226, 235)))
        # Fascia under the roof edge.
        for a, b in roof.boundary:
            o = (b - a).cross(UP).normalized()
            if roof.contains((a + b) / 2 + o * 0.1 * U):
                o = -o
            h = UP * 0.5 * U
            self.geo.face([a - h, b - h, b, a], darker(roof.colour, 3), (a + b) / 2 - o * U)

    def tent(self, comp):
        """Guy ropes from the canvas out to stakes, and a door flap."""
        pts = [p for f in comp for t in f.tris for p in t]
        z1 = max(p.z for p in pts)
        c = sum(pts, Vector()) / len(pts)
        base = []
        for p in pts:
            if p.z < 0.1 * U and all((p - q).length > 0.5 * U for q in base):
                base.append(p)
        tops = [p for p in pts if p.z > z1 - 0.1 * U]
        colour = 236
        for p in base:
            top = min(tops, key=lambda q: (q - p).length)
            a = p + (top - p) * 0.75
            out = Vector((p.x - c.x, p.y - c.y, 0))
            if out.length < 1e-6:
                continue
            stake = p + out.normalized() * max(1.2 * U, 0.35 * (top.z - p.z) + 0.6 * U)
            self.geo.line(a, stake, colour)
        f = max((f for f in comp if f.kind in ("slope", "wall")), key=lambda f: -f.n.y, default=None)
        if f is not None and f.area > (1.5 * U) ** 2:
            t, s0, s1 = self.frame(f)
            sc = (s0 + s1) / 2
            h = 0.55 * (f.zmax - f.zmin)
            w = 0.18 * (s1 - s0)
            tri = [self.at(f, t, sc - w, f.zmin + 0.02 * U, 0.05 * U), self.at(f, t, sc + w, f.zmin + 0.02 * U, 0.05 * U),
                   self.at(f, t, sc, f.zmin + h, 0.05 * U)]
            self.geo.face(tri, darker(f.colour, 6), f.centroid - f.n * U)

    def seams(self, f, step):
        """Ribs across a large roof slope, running down the fall line."""
        dn = Vector((f.n.x, f.n.y, 0))
        if dn.length < 1e-6:
            return
        dn.normalize()
        r = Vector((-dn.y, dn.x, 0))
        ss = [p.dot(r) for t in f.tris for p in t]
        s0, s1 = min(ss), max(ss)
        n = int((s1 - s0) / step)
        c = darker(f.colour, 3)
        for i in range(1, n + 1):
            s = s0 + (s1 - s0) * i / (n + 1)
            hits = []
            for t in f.tris:
                for k in range(3):
                    a, b = t[k], t[(k + 1) % 3]
                    da, db = a.dot(r) - s, b.dot(r) - s
                    if da * db <= 0 and abs(da - db) > 1e-6:
                        hits.append(a + (b - a) * (da / (da - db)))
            if len(hits) >= 2:
                a = min(hits, key=lambda p: p.z)
                b = max(hits, key=lambda p: p.z)
                if (b - a).length > 0.5 * U:
                    self.geo.line(a + f.n * 0.08 * U, b + f.n * 0.08 * U, c)

    def chimney(self, comp, slope):
        ridge = max(f.zmax for f in comp if f.kind == "slope")
        c = slope.centroid
        top = max((p for t in slope.tris for p in t), key=lambda p: p.z)
        q = c + (top - c) * 0.45
        # Surface height under q on the slope plane.
        n = slope.n
        z = (slope.d - n.x * q.x - n.y * q.y) / n.z
        r = 0.4 * U
        z0, z1 = z - 0.6 * U, max(ridge + 0.7 * U, z + 1.3 * U)
        g = mklib.block(q.x - r, q.x + r, q.y - r, q.y + r, z0, z1, Ramp(darker(comp_wall_colour(comp), 1), darker(comp_wall_colour(comp), 6)))
        g.add(mklib.block(q.x - r - 0.08 * U, q.x + r + 0.08 * U, q.y - r - 0.08 * U, q.y + r + 0.08 * U, z1, z1 + 0.15 * U, 236))
        self.geo.add(g)

    def truss(self, comp):
        """Bracing lines over the faces of a steep spire or derrick."""
        z0 = min(f.zmin for f in comp)
        z1 = max(f.zmax for f in comp)
        levels = max(3, min(12, int((z1 - z0) / (2.5 * U))))
        for f in comp:
            if f.kind not in ("slope", "other", "wall") or f.area < (U * U):
                continue
            segs = []
            for i in range(levels + 1):
                z = z0 + (z1 - z0) * i / levels
                pts = []
                for t in f.tris:
                    for k in range(3):
                        a, b = t[k], t[(k + 1) % 3]
                        if (a.z - z) * (b.z - z) <= 0 and abs(a.z - b.z) > 1e-6:
                            pts.append(a + (b - a) * ((z - a.z) / (b.z - a.z)))
                if len(pts) >= 2:
                    ax = Vector((-f.n.y, f.n.x, 0))
                    if ax.length < 1e-6:
                        ax = Vector((1, 0, 0))
                    pa = min(pts, key=lambda p: p.dot(ax))
                    pb = max(pts, key=lambda p: p.dot(ax))
                    off = f.n * 0.08 * U
                    segs.append((pa + off, pb + off))
            c = darker(f.colour, 5)
            for i, (a, b) in enumerate(segs):
                if (a - b).length > 0.2 * U:
                    self.geo.line(a, b, c)
                if i:
                    pa, pb = segs[i - 1]
                    self.geo.line(pa, b, c)
                    self.geo.line(pb, a, c)


def deck(d, r):
    """Railings along the long edges of a raised deck (bridges, gantries, platforms)."""
    pts = [p for t in r.tris for p in t]
    sx = max(p.x for p in pts) - min(p.x for p in pts)
    sy = max(p.y for p in pts) - min(p.y for p in pts)
    if min(sx, sy) < 0.8 * U or max(sx, sy) < 4 * min(sx, sy) or max(sx, sy) < 20 * U:
        return
    longest = max((b - a).length for a, b in r.boundary)
    edges = [(a, b) for a, b in r.boundary if (b - a).length > 0.3 * longest]
    d.railing(r, 0.9 * U, LINE, 2.5 * U, edges)


def comp_wall_colour(comp):
    walls = [f for f in comp if f.kind == "wall"]
    if not walls:
        return 230
    return max(walls, key=lambda f: f.area).colour


def supported(roof, walls):
    """Share of the roof's outline that sits on top of a wall."""
    total = backed = 0.0
    for a, b in roof.boundary:
        L = (b - a).length
        total += L
        mid = (a + b) / 2 - UP * 0.1 * U
        if any(w.zmax >= roof.zmax - 0.1 * U and w.contains(mid, 0.1 * U) for w in walls):
            backed += L
    return backed / total if total else 0.0


def is_tank(comp, walls):
    if len(walls) < 6:
        return False
    pts = [p for f in comp for t in f.tris for p in t]
    cx = sum(p.x for p in pts) / len(pts)
    cy = sum(p.y for p in pts) / len(pts)
    rs = [math.hypot(p.x - cx, p.y - cy) for p in pts if p.z < min(f.zmin for f in comp) + 0.1 * U]
    if not rs:
        return False
    return min(rs) > 0.75 * max(rs)


def detail_mesh(faces, rng, ship=False, windows=True, carrier=False):
    d = Detail(rng, ship)
    for comp in components(faces):
        walls = [f for f in comp if f.kind == "wall" and f.zmax - f.zmin > 0.5 * U]
        roofs = [f for f in comp if f.kind == "roof"]
        slopes = [f for f in comp if f.kind == "slope"]
        z0 = min(f.zmin for f in comp)
        z1 = max(f.zmax for f in comp)
        pts = [p for f in comp for t in f.tris for p in t]
        sx = max(p.x for p in pts) - min(p.x for p in pts)
        sy = max(p.y for p in pts) - min(p.y for p in pts)
        height = z1 - z0
        if height > 6 * U and height > 3 * max(sx, sy) and not ship:
            # Chimney stack, minaret or mast: bands near the top and a ladder.
            d.stack(comp, z0, z1)
            continue
        if not ship and not slopes:
            for r in roofs:
                if r.zmin > 1.0 * U and (not walls or supported(r, walls) < 0.3):
                    deck(d, r)
        ground_flat = all(f.zmax < 0.3 * U for f in roofs)
        if slopes and ground_flat and z0 < 0.2 * U and min(f.zmin for f in slopes) < 0.3 * U and height < 8 * U \
                and max(sx, sy) < 30 * U and not ship:
            # Tents and huts whose canvas or roof comes down to the ground.
            d.tent(comp)
            continue
        if not walls:
            steep = [f for f in comp if f.kind in ("slope", "other") and abs(f.n.z) < 0.6]
            if steep and height > 6 * U and height > 1.4 * max(sx, sy):
                d.truss(comp)
            elif roofs and not slopes and not ship and z0 > 1.5 * U and max(sx, sy) < 4 * min(sx, sy) and \
                    max(f.area for f in roofs) > 0.6 * sx * sy:
                roof = max(roofs, key=lambda f: f.area)
                under = [f for f in faces if f not in comp and 0.5 * U < f.zmax < roof.zmin
                         and roof.contains(Vector((f.centroid.x, f.centroid.y, roof.zmax)), 0.2 * U)]
                if not under:
                    d.columns(roof)
            continue
        vertical = [f for f in walls if abs(f.n.z) < 0.1]
        if not roofs and not slopes and all(abs(f.n.dot(walls[0].n)) > 0.9 for f in walls):
            # A lone panel (fence, screen, antenna reflector): a frame on its back.
            if not ship:
                d.frame_back(max(walls, key=lambda f: f.area))
            continue
        tank = is_tank(comp, walls) and not ship
        if tank:
            for r in roofs:
                if r.zmax > z0 + U and supported(r, walls) > 0.6:
                    d.railing(r, 0.9 * U, LINE, 3 * U)
            # Ladder up the side facing -y.
            w = min(walls, key=lambda f: f.n.y)
            d.ladder(w, w.zmin, w.zmax + 0.9 * U)
            # Band strips around the shell.
            for w in walls:
                t, s0, s1 = d.frame(w)
                for z in (w.zmin + 0.33 * (w.zmax - w.zmin), w.zmin + 0.66 * (w.zmax - w.zmin)):
                    d.geo.line(d.at(w, t, s0, z, 0.1 * U), d.at(w, t, s1, z, 0.1 * U), darker(w.colour, 4))
            continue
        hall = max(sx, sy) > 14 * U and height < 0.6 * min(sx, sy) and not roofs
        skip = []
        door_wall = None
        ground = [w for w in walls if w.zmin - z0 < 0.3 * U]
        if not ship and ground:
            if hall:
                # Sliding doors on the end walls (the ones across the long axis).
                long_axis = Vector((1, 0, 0)) if sx > sy else Vector((0, 1, 0))
                ends = [w for w in ground if abs(w.n.dot(long_axis)) > 0.9]
                for w in ends:
                    d.door(w, big=True)
                ground = [w for w in ground if w not in ends]
                walls = [w for w in walls if w not in ends]
            if ground and max(sx, sy) < 120 * U:
                widest = max(ground, key=lambda f: d.frame(f)[2] - d.frame(f)[1])
                k = d.door(widest)
                if k:
                    skip = [(k[0], k[1], k[2])]
                    door_wall = widest
        if windows:
            for w in walls:
                decals = [o for o in walls if o is not w and o.n.dot(w.n) > 0.999 and abs(o.d - w.d) < 0.1 * U]
                d.windows_on(w, z0, skip if w is door_wall else (), decals)
        for r in roofs:
            if r.zmax - z0 < 1.5 * U or r.area < (1.5 * U) ** 2:
                continue
            share = supported(r, walls)
            if ship:
                # Flight decks stay clear for landing aircraft.
                if share > 0.3 and not carrier:
                    d.railing(r, 0.9 * U, LINE, 1.6 * U)
                continue
            if share < 0.6:
                continue
            ph = min(max(0.06 * (r.zmax - z0), 0.4 * U), 0.9 * U)
            d.parapet(r, ph, comp_wall_colour(comp))
            if r.area > (5 * U) ** 2:
                d.plant(r, min(4, 1 + int(r.area / (14 * U) ** 2)))
        if slopes and not ship and len(vertical) >= 2:
            if max(sx, sy) > 12 * U:
                for f in slopes:
                    d.seams(f, 2.5 * U)
            elif len(vertical) >= 3 and height > 3 * U and min(f.zmin for f in slopes) > z0 + 0.4 * height:
                d.chimney(comp, max(slopes, key=lambda f: f.area))
    return d


# ---------------------------------------------------------------------------------------------
# Ship hulls


def clip_above(tri, zc):
    """Part of a triangle at or above z = zc (as a polygon), or None."""
    out = []
    for i in range(3):
        a, b = tri[i], tri[(i + 1) % 3]
        ia, ib = a.z >= zc - 1e-4, b.z >= zc - 1e-4
        if ia:
            out.append(a)
        if ia != ib:
            out.append(a + (b - a) * ((zc - a.z) / (b.z - a.z)))
    return out if len(out) >= 3 else None


def convex_hull(pts):
    pts = sorted(set(pts))
    if len(pts) < 3:
        return pts

    def half(seq):
        h = []
        for p in seq:
            while len(h) >= 2 and (h[-1][0] - h[-2][0]) * (p[1] - h[-2][1]) - (h[-1][1] - h[-2][1]) * (p[0] - h[-2][0]) <= 0:
                h.pop()
            h.append(p)
        return h

    lo, hi = half(pts), half(reversed(pts))
    return lo[:-1] + hi[:-1]


def span_at(poly, u):
    """(min, max) lateral coordinate of a convex (u, v) polygon at u."""
    vs = []
    for i in range(len(poly)):
        (u0, v0), (u1, v1) = poly[i], poly[(i + 1) % len(poly)]
        if (u0 - u) * (u1 - u) <= 0 and abs(u1 - u0) > 1e-9:
            vs.append(v0 + (v1 - v0) * (u - u0) / (u1 - u0))
        elif abs(u0 - u) < 1e-9:
            vs.append(v0)
    return (min(vs), max(vs)) if vs else None


def ship_hull(faces, wake, carrier=False):
    """Replaces the boxy hull of a ship by a lofted one with a raked bow, flare, sheer, boot-top
    and gunwale, fitted to the original's deck outline. Returns (kept faces, hull Geo): the
    original faces above the main deck (superstructure, raised decks) stay, clipped at it."""
    comps = components([f for f in faces if f.colour != 57])
    if not comps:
        return None

    def plan(c):
        pts = [p for f in c for t in f.tris for p in t]
        return (max(p.x for p in pts) - min(p.x for p in pts)) * (max(p.y for p in pts) - min(p.y for p in pts))

    # The hull: every piece that comes down to the waterline, if the biggest of them stands up
    # from the water (flat hulls of small craft stay as they are).
    low = [c for c in comps if min(f.zmin for f in c) < 0.3 * U]
    if not low:
        return None
    biggest = max(comps, key=plan)
    if biggest not in low or max(f.zmax for f in biggest) < 0.5 * U:
        return None
    hull = [f for c in low for f in c]
    pts = [p for f in hull for t in f.tris for p in t]
    sx = max(p.x for p in pts) - min(p.x for p in pts)
    sy = max(p.y for p in pts) - min(p.y for p in pts)
    a = Vector((1, 0, 0)) if sx >= sy else Vector((0, 1, 0))
    b = Vector((-a.y, a.x, 0))
    centre = sum(pts, Vector()) / len(pts)
    poly = convex_hull([(round(p.dot(a), 3), round(p.dot(b), 3)) for p in pts])
    us = [q[0] for q in poly]
    u0, u1 = min(us), max(us)
    L = u1 - u0
    if L < 10 * U:
        return None
    # Bow towards +a unless the wake says otherwise, or the narrower end is at -a.
    if wake is not None:
        sgn = -1.0 if (wake - centre).dot(a) > 0 else 1.0
    else:
        def w_at(u):
            s = span_at(poly, u)
            return s[1] - s[0] if s else 0.0
        sgn = 1.0 if w_at(u0 + 0.1 * L) >= w_at(u1 - 0.1 * L) else -1.0
    # Main deck: the roof level with the most area (else the top of the waterline walls).
    levels_ = {}
    for f in hull:
        if f.kind == "roof" and f.zmax > 0.3 * U:
            levels_[round(f.zmax, 1)] = levels_.get(round(f.zmax, 1), 0.0) + f.area
    if levels_:
        zd = max(levels_, key=levels_.get)
    else:
        zd = max((f.zmax for f in hull if f.kind == "wall" and f.zmin < 0.3 * U), default=0.0)
    if zd < 0.8 * U:
        return None
    hull_wall = [f for f in hull if f.kind == "wall" and f.zmin < 0.3 * U]
    by_colour = {}
    for f in hull_wall:
        by_colour[f.colour] = by_colour.get(f.colour, 0.0) + f.area
    side_c = max(by_colour, key=by_colour.get) if by_colour else 228
    decks = [f for f in hull if f.kind == "roof" and abs(f.zmax - zd) < 0.05 * U]
    deck_c = max(decks, key=lambda f: f.area).colour if decks else darker(side_c, 4)

    def to3(u, v, z):
        uu = u if sgn > 0 else (u0 + u1) - u
        return a * uu + b * v + Vector((0, 0, z))

    # Stations from the stern (t=0) to the bow (t=1) in the bow-forward frame.
    def span(t):
        u = u0 + t * L if sgn > 0 else u1 - t * L
        s = span_at(poly, u)
        return s if s else (0.0, 0.0)

    n = 18
    ts = [i / n for i in range(n + 1)]
    spans = [span(t) for t in ts]
    mids = [(lo + hi) / 2 for lo, hi in spans]
    halves = [(hi - lo) / 2 for lo, hi in spans]
    # Fuller, rounded bow shoulders over the forward third.
    k30 = int(0.7 * n)
    w30 = halves[k30]
    for i in range(k30, n + 1):
        d = (1 - ts[i]) / 0.3
        halves[i] = max(halves[i], w30 * (1 - (1 - min(d, 1.0)) ** 1.6)) if i < n else 0.0
    rake, transom = 0.7 * zd, 0.25 * zd

    def deck_z(t):
        return zd + 0.18 * zd * max(0.0, (t - 0.75) / 0.25) ** 2

    def ring(i, h):
        """Port and starboard hull points of station i at height fraction h (0 waterline, 1 deck)."""
        t = ts[i]
        ud = u0 + t * L
        uw = u0 + transom + t * (L - transom - rake)
        # Flare: narrower at the waterline, nearly full width by the knuckle.
        f = 0.86 + 0.14 * min(1.0, h / 0.6) ** 0.7
        u = uw + (ud - uw) * h
        w = halves[i] * f
        z = deck_z(t) * h
        return to3(u, mids[i] + w, z), to3(u, mids[i] - w, z)

    g = Geo()
    bands = [(0.0, 0.14, 239), (0.14, 0.6, side_c), (0.6, 1.0, side_c)]
    for i in range(n):
        for h0, h1, c in bands:
            p0, s0 = ring(i, h0)
            p1, s1 = ring(i + 1, h0)
            p2, s2 = ring(i + 1, h1)
            p3, s3 = ring(i, h1)
            mid = (p0 + s2) / 2
            for quad in ([p0, p1, p2, p3], [s3, s2, s1, s0]):
                q = []
                for v in quad:
                    if not q or (v - q[-1]).length > 1e-5:
                        q.append(v)
                if len(q) > 3 and (q[0] - q[-1]).length < 1e-5:
                    q.pop()
                if len(q) >= 3:
                    g.face(q, c, mid)
        # Deck strip
        p3, s3 = ring(i, 1.0)
        p2, s2 = ring(i + 1, 1.0)
        q = [p3, p2, s2, s3] if (p2 - s2).length > 1e-5 else [p3, p2, s3]
        g.face(q, deck_c, (p3 + s2) / 2 - Vector((0, 0, U)))
    # Transom
    stern = []
    for h in (0.0, 0.14, 0.6, 1.0):
        stern.append(ring(0, h))
    poly3 = [p for p, _ in stern] + [s for _, s in reversed(stern)]
    g.face(poly3, side_c, to3(u0 + L / 2, mids[0], zd / 2))
    # Gunwale rail (not around a flight deck), hawse pipes and portholes
    for i in range(n if not carrier else 0):
        for side in (0, 1):
            pa, pb = ring(i, 1.0)[side], ring(i + 1, 1.0)[side]
            hz = Vector((0, 0, 0.9 * U))
            g.line(pa + hz, pb + hz, LINE)
            g.line(pa, pa + hz, LINE)
    for side in (0, 1):
        i = int(0.86 * n)
        p, q = ring(i, 0.72)[side], ring(i + 1, 0.72)[side]
        out = (p - to3(u0 + ts[i] * L, mids[i], p.z)).normalized() * 0.05 * U
        c = (p + q) / 2 + out
        e = (q - p).normalized() * 0.35 * U
        g.face([c - e - Vector((0, 0, 0.3 * U)), c + e - Vector((0, 0, 0.3 * U)), c + e + Vector((0, 0, 0.3 * U)),
                c - e + Vector((0, 0, 0.3 * U))], 239, c - out * 20)
    if zd > 2.5 * U:
        for i in range(2, int(0.8 * n)):
            for side in (0, 1):
                p = ring(i, 0.8)[side]
                q = ring(i + 1, 0.8)[side]
                axis = to3(u0 + ts[i] * L, mids[i], p.z)
                out = (p - axis)
                out.z = 0
                out = out.normalized() * 0.06 * U
                e = (q - p)
                for k in (0.25, 0.75):
                    c = p + e * k + out
                    r = 0.18 * U
                    ed = e.normalized() * r
                    g.face([c - ed - Vector((0, 0, r)), c + ed - Vector((0, 0, r)), c + ed + Vector((0, 0, r)),
                            c - ed + Vector((0, 0, r))], GLASS, c - out * 20)
    # Keep the original above the main deck.
    kept_tris = []
    for f in faces:
        if f.colour == 57 or f not in hull:
            kept_tris += [(t, f.colour) for t in f.tris]
            continue
        for t in f.tris:
            if min(p.z for p in t) >= zd - 1e-4:
                if not (f.kind == "roof" and abs(f.zmax - zd) < 0.05 * U):
                    kept_tris.append((t, f.colour))
            elif max(p.z for p in t) > zd + 0.05 * U:
                q = clip_above(t, zd)
                if q:
                    for k in range(1, len(q) - 1):
                        kept_tris.append(((q[0], q[k], q[k + 1]), f.colour))
    return read_faces(kept_tris), g


def levels(stem):
    """World units per model unit for each model of a theater: the finest terrain level its
    tile lists place it on (models placed elsewhere count as level 1)."""
    import re

    path = os.path.join(ROOT, "assets", "theaters", stem + ".ron")
    if not os.path.exists(path):
        return {}
    t = open(path).read()
    t = t[t.index("    tiles:"):t.index("    world:")]
    unit = [0.5, 1.0, 4.0, 16.0, 64.0]
    depth, level, out = 0, -1, {}
    for m in re.finditer(r"\[|\]|shape: (\d+)", t):
        if m.group(0) == "[":
            depth += 1
            if depth == 2:
                level += 1
        elif m.group(0) == "]":
            depth -= 1
        else:
            k = int(m.group(1))
            out[k] = min(out.get(k, 99.0), unit[level])
    return {k: max(v, 1.0) for k, v in out.items()}




# ---------------------------------------------------------------------------------------------
# Blender side


class Blend:
    """The models of the open .blend as a node list (the glTF view of them: names, children,
    whether a node carries a mesh, root attributes), with helpers to read and change them.
    Replaced meshes keep their originals as unexported `orig:<object>` meshes."""

    def __init__(self, stem):
        objs = sorted((o for o in bpy.data.objects if o.name.startswith(stem + "_")), key=lambda o: o.name)
        self.load(objs)

    def load(self, objs):
        self.objs = list(objs)
        index = {o.name: i for i, o in enumerate(self.objs)}
        self.js = {"nodes": []}
        for o in self.objs:
            n = {"name": o.name, "children": [index[c.name] for c in sorted(o.children, key=lambda c: c.name) if c.name in index]}
            if o.type == "MESH":
                n["mesh"] = o.data.name
            if "attr" in o:
                n["extras"] = {"attr": int(o["attr"])}
            self.js["nodes"].append(n)

    def roots(self):
        return [i for i, o in enumerate(self.objs) if o.parent is None]

    def triangles(self, k):
        """((a, b, c), colour) triangles of a node, in model space (z up)."""
        o = self.objs[k]
        if o.type != "MESH":
            return []
        me = o.data
        cols = [mklib.index_of(m.name) if m is not None and "pal_" in m.name else 239 for m in me.materials]
        out = []
        for p in me.polygons:
            vs = [me.vertices[i].co.copy() for i in p.vertices]
            c = cols[p.material_index] if p.material_index < len(cols) else 239
            for i in range(1, len(vs) - 1):
                out.append(((vs[0], vs[i], vs[i + 1]), c))
        return out

    def points(self, k):
        """Line end points in pairs (lines nodes) or the vertices (dots nodes)."""
        me = self.objs[k].data
        if me.edges:
            return [me.vertices[i].co.copy() for e in me.edges for i in e.vertices]
        return [v.co.copy() for v in me.vertices]

    def mesh(self, name, prims):
        """New mesh from [(colour, mode, points)] (mode 4 triangle list, 1 line list, 0 points)."""
        verts, edges, faces, mats, slots, fmat = [], [], [], [], {}, []
        for c, mode, pts in prims:
            if c not in slots:
                slots[c] = len(mats)
                mats.append(c)
            base = len(verts)
            verts += [tuple(p) for p in pts]
            if mode == 4:
                for i in range(0, len(pts) - 2, 3):
                    faces.append((base + i, base + i + 1, base + i + 2))
                    fmat.append(slots[c])
            elif mode == 1:
                edges += [(base + i, base + i + 1) for i in range(0, len(pts) - 1, 2)]
        me = bpy.data.meshes.new(name)
        me.from_pydata(verts, edges, faces)
        for c in mats:
            me.materials.append(mklib.material(c))
        for poly, m in zip(me.polygons, fmat):
            poly.material_index = m
        me.update()
        return me

    def add_node(self, parent, name, mesh):
        p = self.objs[parent]
        ob = bpy.data.objects.new(name, mesh)
        for col in p.users_collection:
            col.objects.link(ob)
        ob.parent = p
        self.objs.append(ob)
        n = {"name": ob.name, "children": []}
        if mesh is not None:
            n["mesh"] = mesh.name
        self.js["nodes"].append(n)
        self.js["nodes"][parent]["children"].append(len(self.objs) - 1)
        return len(self.objs) - 1

    def keep_original(self, k):
        o = self.objs[k]
        if bpy.data.meshes.get("orig:" + o.name) is None:
            m = o.data.copy()
            m.name = "orig:" + o.name
            m.use_fake_user = True

    def set_data(self, k, me):
        o = self.objs[k]
        prev = o.data
        o.data = me
        if prev.users == 0 and not prev.use_fake_user:
            bpy.data.meshes.remove(prev)
        me.name = o.name

    def replace_faces(self, k, faces):
        """Gives a node new faces [(points, colour)], keeping its original mesh."""
        self.keep_original(k)
        prims = {}
        for pts, c in faces:
            for i in range(1, len(pts) - 1):
                prims.setdefault(c, []).extend([pts[0], pts[i], pts[i + 1]])
        self.set_data(k, self.mesh(self.objs[k].name, [(c, 4, p) for c, p in sorted(prims.items())]))

    def set_points(self, k, pts, mode=0):
        """New dots (mode 0) or line segments (mode 1) for a dots/lines node."""
        self.keep_original(k)
        self.set_data(k, self.mesh(self.objs[k].name, [(mklib.index_of(self.objs[k].name), mode, pts)]))

    def strip(self):
        """Removes earlier detail objects and puts the original meshes back."""
        names = [o.name for o in self.objs]
        for name in names:
            o = bpy.data.objects.get(name)
            if o is not None and "_detail" in name:
                mklib.delete_tree(o)
        objs = [bpy.data.objects[n] for n in names if n in bpy.data.objects]
        for o in objs:
            orig = bpy.data.meshes.get("orig:" + o.name)
            if orig is not None:
                prev = o.data
                o.data = orig
                orig.use_fake_user = False
                if prev.users == 0:
                    bpy.data.meshes.remove(prev)
                orig.name = o.name
        self.load(objs)


def subtree_ids(js, i):
    out = [i]
    for c in js["nodes"][i].get("children", []):
        out += subtree_ids(js, c)
    return out


def detail_root(glb, ri, level):
    """Adds detail to LOD 0 of a model; returns True if anything was added."""
    global U, WIN_OFF
    U = 10.0 / level
    WIN_OFF = 0.04 * U
    js = glb.js
    root = js["nodes"][ri]
    lods = sorted((c for c in root.get("children", []) if "_lod" in js["nodes"][c]["name"]), key=lambda c: js["nodes"][c]["name"])
    if not lods:
        return False
    lod0 = lods[0]
    nodes = subtree_ids(js, lod0)
    ship = False
    for k in nodes:
        n = js["nodes"][k]
        if "_lines_pal_057" in n["name"] or any(c == 57 for _, c in glb.triangles(k)):
            ship = True
    if ship:
        # Ships are drawn closer to true scale than buildings: size the fittings by the hull.
        hull = [p for k in nodes for t, c in glb.triangles(k) if c != 57 for p in t]
        if hull:
            length = max(max(p.x for p in hull) - min(p.x for p in hull), max(p.y for p in hull) - min(p.y for p in hull))
            U = min(U, length / 40)
            WIN_OFF = 0.04 * U
    rng = random.Random(root["name"])
    hull_node, wake = None, None
    if ship:
        # The hull is in the mesh with the biggest plan; the wake trails behind the stern.
        best = 0.0
        wk = []
        for k in nodes:
            n = js["nodes"][k]
            if "mesh" not in n or "_dots_" in n["name"] or "_lines_" in n["name"] or "_s1v1" in n["name"]:
                continue
            tris = glb.triangles(k)
            wk += [p for t, c in tris if c == 57 for p in t]
            area = sum((t[1] - t[0]).cross(t[2] - t[0]).length for t, c in tris if c != 57 and max(p.z for p in t) > 0.1)
            if area > best:
                best, hull_node = area, k
        for k in nodes:
            if "_lines_pal_057" in js["nodes"][k]["name"]:
                wk += glb.points(k)
        wake = sum(wk, Vector()) / len(wk) if wk else None
    # Aircraft carriers: a flat deck covering most of the ship's plan, with little built on it.
    carrier = False
    if ship:
        tris = [(t, c) for k in nodes if "mesh" in js["nodes"][k] and "_dots_" not in js["nodes"][k]["name"]
                and "_lines_" not in js["nodes"][k]["name"] and "_s1v1" not in js["nodes"][k]["name"]
                for t, c in glb.triangles(k) if c != 57]
        pts = [p for t, _ in tris for p in t]
        decks = [f for f in read_faces(tris) if f.kind == "roof" and f.zmax > 0.5 * U]
        if decks:
            plan = (max(p.x for p in pts) - min(p.x for p in pts)) * (max(p.y for p in pts) - min(p.y for p in pts))
            zd = max(decks, key=lambda f: f.area).zmax
            deck = sum(f.area for f in decks if abs(f.zmax - zd) < 0.1 * U)
            above = sum(f.area for f in decks if f.zmax > zd + 0.5 * U)
            carrier = deck > 0.4 * plan and above < 0.3 * deck
    windows, lifts, all_faces = [], [], []
    touched = False
    for k in nodes:
        n = js["nodes"][k]
        name = n["name"]
        if "mesh" not in n or "_dots_" in name or "_lines_" in name or "_s1v1" in name:
            continue
        faces = read_faces(glb.triangles(k))
        if not faces:
            continue
        hull = None
        if ship and k == hull_node:
            hull = ship_hull(faces, wake, carrier)
        if hull is not None:
            kept, hull_geo = hull
            glb.replace_faces(k, [(list(t), f.colour) for f in kept for t in f.tris] + hull_geo.faces)
            faces = kept
            touched = True
            if hull_geo.lines:
                for c, segs in sorted(hull_geo.lines.items()):
                    ln = "%s_hull_detail_lines_pal_%03d" % (name, c)
                    glb.add_node(k, ln, glb.mesh(ln, [(c, 1, [p for sg in segs for p in sg])]))
        moved = any("_part" in js["nodes"][j]["name"] for j in nodes if k in subtree_ids(js, j))
        if not moved:
            all_faces += faces
        d = detail_mesh(faces, rng, ship, carrier=carrier)
        if not d.geo.faces and not d.geo.lines:
            continue
        touched = True
        prims = {}
        for pts, c in d.geo.faces:
            for i in range(1, len(pts) - 1):
                prims.setdefault(c, []).extend([pts[0], pts[i], pts[i + 1]])
        dn = glb.add_node(k, name + "_detail", glb.mesh(name + "_detail", [(c, 4, p) for c, p in sorted(prims.items())]) if prims else None)
        for c, segs in sorted(d.geo.lines.items()):
            ln = "%s_detail_lines_pal_%03d" % (name, c)
            glb.add_node(dn, ln, glb.mesh(ln, [(c, 1, [p for s in segs for p in s])]))
        if not moved:
            windows += d.windows
            lifts += d.parapets
    for k in nodes:
        n = js["nodes"][k]
        name = n["name"]
        if "mesh" not in n or "_lines_pal_" not in name or "_detail" in name:
            continue
        pts = glb.points(k)
        segs = [(pts[i], pts[i + 1]) for i in range(0, len(pts) - 1, 2)]
        g = Geo()
        for a, b in segs:
            if a.z > b.z:
                a, b = b, a
            e = b - a
            if e.length > 8 * U and e.normalized().z > 0.95:
                lattice(g, a, b, mklib.index_of(name))
        for c, ls in g.lines.items():
            parent = next(j for j in nodes if k in js["nodes"][j].get("children", []))
            glb.add_node(parent, name + "_detail", glb.mesh(name + "_detail", [(c, 1, [p for sg in ls for p in sg])]))
            touched = True
    if touched:
        place_lights(glb, nodes, all_faces, windows, lifts)
    return touched


def lattice(g, a, b, c):
    """Four-legged tapering lattice around the vertical line a-b (masts, pylons)."""
    L = (b - a).length
    w0 = min(max(0.06 * L, 0.4 * U), 3 * U)
    w1 = 0.3 * w0
    n = max(2, min(14, int(L / (2.2 * w0))))
    rings = []
    for i in range(n + 1):
        t = i / n
        w = w0 + (w1 - w0) * t
        z = a + (b - a) * t
        rings.append([z + Vector((dx * w, dy * w, 0)) for dx, dy in ((-1, -1), (1, -1), (1, 1), (-1, 1))])
    for k in range(4):
        g.line(rings[0][k], rings[-1][k], c)
    for i, r in enumerate(rings):
        for k in range(4):
            if i:
                g.line(r[k], r[(k + 1) % 4], c)
                p = rings[i - 1]
                g.line(p[k], r[(k + 1) % 4], c)
                g.line(p[(k + 1) % 4], r[k], c)


def place_lights(glb, nodes, faces, windows, lifts):
    js = glb.js
    walls = [f for f in faces if f.kind == "wall"]
    free = list(windows)
    for k in nodes:
        n = js["nodes"][k]
        if "mesh" not in n or "_dots_pal_" not in n["name"] or "_part" in n["name"]:
            continue
        pts = glb.points(k)
        colour = mklib.index_of(n["name"])
        out = []
        for p in pts:
            q = p
            if colour == 148 and free and any(w.contains(p, 0.4 * U) for w in walls):
                best = min(free, key=lambda w: (w[0] - p).length)
                if (best[0] - p).length < 12 * U:
                    q = best[0]
                    free.remove(best)
            for r, h in lifts:
                flat = Vector((p.x, p.y, r.zmax))
                if abs(p.z - r.zmax) < 0.4 * U and (r.contains(flat, 0.4 * U) or any((flat - a).length < 1.5 * U for a, _ in r.boundary)):
                    q = Vector((q.x, q.y, r.zmax + h + 0.3 * U))
            out.append(q)
        if any((a - b).length > 1e-4 for a, b in zip(pts, out)):
            glb.set_points(k, out)


# Natural features (rocks, ice) that the building rules would mistake for structures.
NATURAL = {"nc_070", "nc_089", "nc_090", "nc_091", "nc_092"}


def rebuild(glb, ri, parts):
    """Replaces the faces (and lines of the same colours) of a model's LOD 0 nodes by those of a
    remodel: {node suffix: Geo}. Nodes the remodel leaves out keep their original geometry."""
    js = glb.js
    lod0 = next(c for c in js["nodes"][ri]["children"] if js["nodes"][c]["name"].endswith("_lod0"))
    base = js["nodes"][lod0]["name"]
    for k in subtree_ids(js, lod0):
        n = js["nodes"][k]
        suffix = n["name"][len(base):]
        if suffix not in parts:
            continue
        g = parts[suffix]
        if "mesh" in n:
            glb.replace_faces(k, g.faces)
        elif g.faces:
            prims = {}
            for pts, c in g.faces:
                for i in range(1, len(pts) - 1):
                    prims.setdefault(c, []).extend([pts[0], pts[i], pts[i + 1]])
            glb.add_node(k, n["name"] + "_detail", glb.mesh(n["name"] + "_detail", [(c, 4, p) for c, p in sorted(prims.items())]))
        for c, segs in sorted(g.lines.items()):
            pts = [p for sg in segs for p in sg]
            name = "%s_lines_pal_%03d" % (n["name"], c)
            old = next((j for j in n.get("children", []) if js["nodes"][j]["name"] == name), None)
            if old is not None:
                glb.set_points(old, pts, 1)
            else:
                ln = "%s_detail_lines_pal_%03d" % (n["name"], c)
                glb.add_node(k, ln, glb.mesh(ln, [(c, 1, pts)]))


def apply(stem, skip=()):
    """Details every model of the open .blend (theater or photo file) except those in `skip`,
    hills (attr 147..149), terrain cells and natural features; for STFLT, rebuilds the stores
    (stores.py). Returns the names of the models changed."""
    glb = Blend(stem)
    glb.strip()
    done = []
    if stem == "stflt":
        import stores

        for ri in glb.roots():
            name = glb.js["nodes"][ri]["name"]
            if name in stores.BUILDS:
                lod0 = next(c for c in glb.js["nodes"][ri]["children"] if glb.js["nodes"][c]["name"].endswith("_lod0"))
                base = glb.js["nodes"][lod0]["name"]
                orig = {glb.js["nodes"][k]["name"][len(base):]: glb.triangles(k) for k in subtree_ids(glb.js, lod0)}
                rebuild(glb, ri, stores.BUILDS[name](orig))
                done.append(name)
        return done
    lv = levels(stem)
    for ri in glb.roots():
        name = glb.js["nodes"][ri]["name"]
        attr = (glb.js["nodes"][ri].get("extras") or {}).get("attr", 0)
        if name in skip or name in NATURAL or 147 <= attr <= 149:
            continue
        level = lv.get(int(name.split("_")[-1]), 1.0)
        if level >= 16:
            continue  # terrain cells
        if detail_root(glb, ri, level):
            done.append(name)
    return done
