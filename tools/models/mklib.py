"""Low-poly modelling helpers for generated models (run inside Blender; used by tools/theater_ua).

Model space is the original's: x right, y forward (nose), z up, in model units. Every face
carries a palette colour byte; a `Ramp` picks a shade along one of the palette's 16-colour ramps
from the face normal, so the flat-coloured classic renderer still shows the form.
"""

import math

import bpy
from mathutils import Vector

LIGHT = Vector((-0.3, 0.2, 1.0)).normalized()


class Ramp:
    """A palette ramp from `lo` (lit) to `hi` (shadowed) colour bytes."""

    def __init__(self, lo, hi):
        self.lo, self.hi = lo, hi

    def pick(self, n):
        t = (1.0 - n.dot(LIGHT)) / 2.0
        t = min(max((t - 0.05) / 0.9, 0.0), 1.0)
        return round(self.lo + t * (self.hi - self.lo))


def colour(paint, n):
    return paint if isinstance(paint, int) else paint.pick(n)


def v3(p):
    return Vector((float(p[0]), float(p[1]), float(p[2])))


class Geo:
    """Polygon soup with per-face palette colour bytes, plus loose line segments."""

    def __init__(self):
        self.faces = []  # (points, colour byte)
        self.lines = {}  # colour byte -> [(a, b)]

    def face(self, pts, paint, centre=None):
        pts = [v3(p) for p in pts]
        n = normal(pts)
        if n.length < 1e-9:
            return
        if centre is not None:
            c = sum(pts, Vector()) / len(pts)
            if n.dot(c - v3(centre)) < 0:
                pts.reverse()
                n = -n
        self.faces.append((pts, colour(paint, n)))

    def face2(self, pts, paint):
        """Thin sheet visible from both sides (the renderer culls back faces)."""
        self.face(pts, paint)
        self.face(list(reversed([v3(p) for p in pts])), paint)

    def line(self, a, b, c):
        self.lines.setdefault(c, []).append((v3(a), v3(b)))

    def add(self, other):
        self.faces += other.faces
        for c, l in other.lines.items():
            self.lines.setdefault(c, []).extend(l)
        return self

    def transformed(self, f):
        g = Geo()
        g.faces = [([f(p) for p in pts], c) for pts, c in self.faces]
        g.lines = {c: [(f(a), f(b)) for a, b in l] for c, l in self.lines.items()}
        return g

    def mirrored(self):
        g = self.transformed(lambda p: Vector((-p.x, p.y, p.z)))
        g.faces = [(list(reversed(pts)), c) for pts, c in g.faces]
        return g

    def scaled(self, s, offset=(0, 0, 0)):
        o = v3(offset)
        return self.transformed(lambda p: p * s + o)

    def rotated_z(self, a, pivot=(0, 0, 0)):
        c, s, o = math.cos(a), math.sin(a), v3(pivot)
        return self.transformed(lambda p: Vector((o.x + (p.x - o.x) * c - (p.y - o.y) * s, o.y + (p.x - o.x) * s + (p.y - o.y) * c, p.z)))

    def rotated_x(self, a, pivot=(0, 0, 0)):
        """Pitch: positive raises +y."""
        c, s, o = math.cos(a), math.sin(a), v3(pivot)
        return self.transformed(lambda p: Vector((p.x, o.y + (p.y - o.y) * c - (p.z - o.z) * s, o.z + (p.y - o.y) * s + (p.z - o.z) * c)))

    def recolour(self, paint):
        """Reshade every face with another paint (keeps geometry)."""
        g = Geo()
        g.faces = [(pts, colour(paint, normal(pts))) for pts, _ in self.faces]
        g.lines = dict(self.lines)
        return g

    def bounds(self):
        pts = [p for f, _ in self.faces for p in f] + [p for l in self.lines.values() for s in l for p in s]
        lo = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
        hi = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
        return lo, hi


def normal(pts):
    n = Vector()
    for i in range(len(pts)):
        a, b = pts[i], pts[(i + 1) % len(pts)]
        n += Vector(((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y)))
    return n.normalized() if n.length > 1e-12 else n


def basis(axis):
    axis = axis.normalized()
    up = Vector((0, 0, 1)) if abs(axis.z) < 0.9 else Vector((1, 0, 0))
    u = axis.cross(up).normalized()
    w = u.cross(axis).normalized()
    return u, w


def tube(rings, paint, cap0=True, cap1=True, paints=None):
    """Closed tube through rings of equal point count (each ring a list of points)."""
    g = Geo()
    m = len(rings[0])
    centres = [sum((v3(p) for p in r), Vector()) / m for r in rings]
    for i in range(len(rings) - 1):
        a, b = rings[i], rings[i + 1]
        p = paints[i] if paints else paint
        mid = (centres[i] + centres[i + 1]) / 2
        for k in range(m):
            k1 = (k + 1) % m
            q = [a[k], a[k1], b[k1], b[k]]
            qv = [v3(x) for x in q]
            # Drop collapsed corners (rings that shrink to a point).
            uniq = []
            for x in qv:
                if not uniq or (x - uniq[-1]).length > 1e-6:
                    uniq.append(x)
            if len(uniq) > 1 and (uniq[0] - uniq[-1]).length < 1e-6:
                uniq.pop()
            if len(uniq) >= 3:
                g.face(uniq, p, mid)
    for cap, r, c, other in ((cap0, rings[0], centres[0], centres[1]), (cap1, rings[-1], centres[-1], centres[-2])):
        if cap and max((v3(x) - c).length for x in r) > 1e-6:
            g.face(r, paints[0] if paints and r is rings[0] else (paints[-1] if paints else paint), other)
    return g


def ring(centre, hw, hh, n=8, power=2.0, axis=(0, 1, 0), rot=None):
    """Superellipse cross-section around `centre` perpendicular to `axis` (default fore-aft)."""
    c = v3(centre)
    axis = v3(axis)
    if axis.normalized() == Vector((0, 1, 0)):
        u, w = Vector((1, 0, 0)), Vector((0, 0, 1))
    else:
        u, w = basis(axis)
    off = math.pi / n if rot is None else rot
    pts = []
    for k in range(n):
        a = 2 * math.pi * k / n + off
        ca, sa = math.cos(a), math.sin(a)
        x = math.copysign(abs(ca) ** (2 / power), ca) * hw
        z = math.copysign(abs(sa) ** (2 / power), sa) * hh
        pts.append(c + u * x + w * z)
    return pts


def loft(stations, paint, n=8, power=2.0, x=0.0, paints=None, rot=None):
    """Fuselage-style body: stations (y, z centre, half width, half height)."""
    rings = [ring((x, s[0], s[1]), s[2], s[3], n, power, rot=rot) for s in stations]
    return tube(rings, paint, paints=paints)


def frustum(p0, p1, r0, r1, paint, n=8, caps=True, rot=None, r0b=None, r1b=None):
    """Cylinder / cone between two points (radii may be elliptical with r0b/r1b)."""
    p0, p1 = v3(p0), v3(p1)
    axis = p1 - p0
    a = ring(p0, r0, r0 if r0b is None else r0b, n, axis=axis, rot=rot)
    b = ring(p1, r1, r1 if r1b is None else r1b, n, axis=axis, rot=rot)
    return tube([a, b], paint, caps, caps)


def hexa(pts, paint):
    """Six-sided solid from 8 corners: bottom quad then top quad, same winding."""
    p = [v3(x) for x in pts]
    c = sum(p, Vector()) / 8
    g = Geo()
    quads = [(0, 1, 2, 3), (4, 5, 6, 7), (0, 1, 5, 4), (1, 2, 6, 5), (2, 3, 7, 6), (3, 0, 4, 7)]
    for q in quads:
        pts4 = [p[i] for i in q]
        uniq = []
        for v in pts4:
            if all((v - u).length > 1e-6 for u in uniq):
                uniq.append(v)
        if len(uniq) >= 3:
            g.face(uniq, paint, c)
    return g


def block(x0, x1, y0, y1, z0, z1, paint, taper=0.0, taper_y=0.0):
    """Axis-aligned box; `taper`/`taper_y` shrink the top in x/y (cab roofs, turrets)."""
    tx, ty = taper, taper_y
    return hexa(
        [(x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0),
         (x0 + tx, y0 + ty, z1), (x1 - tx, y0 + ty, z1), (x1 - tx, y1 - ty, z1), (x0 + tx, y1 - ty, z1)],
        paint,
    )


def wing(root, root_chord, tip, tip_chord, paint, t=0.06, vertical=False, mirror=True, peak=0.35, t_tip=None):
    """Airfoil panel from a root and a tip leading-edge point (chord runs aft along -y)."""
    t_tip = t if t_tip is None else t_tip

    def section(le, c, th):
        le = v3(le)
        d = Vector((th * c / 2, 0, 0)) if vertical else Vector((0, 0, th * c / 2))
        mid = le - Vector((0, peak * c, 0))
        return [le, mid + d, le - Vector((0, c, 0)), mid - d]

    g = tube([section(root, root_chord, t), section(tip, tip_chord, t_tip)], paint)
    if mirror:
        g.add(g.mirrored())
    return g


def extrude(poly, d, paint):
    """Prism: a planar polygon swept along vector d (side faces + both caps)."""
    a = [v3(p) for p in poly]
    d = v3(d)
    b = [p + d for p in a]
    return tube([a, b], paint)


def side_prism(profile, x0, x1, paint):
    """Prism from a (y, z) side profile spanning x0..x1 (hulls, tracks, cabs)."""
    return extrude([(x0, y, z) for y, z in profile], (x1 - x0, 0, 0), paint)


def wheel(centre, r, width, paint, n=8, hub=None):
    c = v3(centre)
    g = frustum(c - Vector((width / 2, 0, 0)), c + Vector((width / 2, 0, 0)), r, r, paint, n)
    return g


def lattice(x0, x1, z0, z1, y, nx, nz, c):
    """Grid of line segments in the plane y = const (antenna arrays, frames)."""
    g = Geo()
    for i in range(nx + 1):
        x = x0 + (x1 - x0) * i / nx
        g.line((x, y, z0), (x, y, z1), c)
    for j in range(nz + 1):
        z = z0 + (z1 - z0) * j / nz
        g.line((x0, y, z), (x1, y, z), c)
    return g


# ---------------------------------------------------------------------------------------------
# Blender side

PALETTE = {}


def load_palette(path):
    """Day colours of a theater palette file (RON), 6-bit VGA -> sRGB floats."""
    import re

    t = open(path).read()
    day = t[t.index("day"):t.index("dusk")]
    cols = re.findall(r"\((\d+), (\d+), (\d+)\)", day)
    for i, c in enumerate(cols[:256]):
        PALETTE[i] = tuple(int(x) * 4 / 255 for x in c)


def material(index):
    name = "pal_%03d" % index
    m = bpy.data.materials.get(name)
    if m:
        return m
    base = next((x for x in bpy.data.materials if x.name.startswith("pal_")), None)
    m = base.copy() if base else bpy.data.materials.new(name)
    m.name = name
    srgb = PALETTE.get(index, (1, 0, 1))
    lin = tuple(c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4 for c in srgb)
    m.diffuse_color = (*lin, 1.0)
    if m.node_tree:
        for node in m.node_tree.nodes:
            for key in ("Color", "Base Color"):
                if key in node.inputs and hasattr(node.inputs[key], "default_value"):
                    try:
                        node.inputs[key].default_value = (*lin, 1.0)
                    except (TypeError, ValueError):
                        pass
    return m


def faces_mesh(name, geo):
    """Blender mesh of a Geo's faces (one material slot per colour byte)."""
    me = bpy.data.meshes.new(name)
    verts, polys, mats = [], [], []
    slots = {}
    for pts, c in geo.faces:
        i0 = len(verts)
        verts += [tuple(p) for p in pts]
        polys.append(list(range(i0, i0 + len(pts))))
        if c not in slots:
            slots[c] = len(slots)
            me.materials.append(material(c))
        mats.append(slots[c])
    me.from_pydata(verts, [], polys)
    for p, mi in zip(me.polygons, mats):
        p.material_index = mi
    me.update()
    return me


def lines_mesh(name, segs, c):
    me = bpy.data.meshes.new(name)
    verts, edges = [], []
    for a, b in segs:
        edges.append((len(verts), len(verts) + 1))
        verts += [tuple(a), tuple(b)]
    me.from_pydata(verts, edges, [])
    me.materials.append(material(c))
    me.update()
    return me


def points_mesh(name, pts, c):
    me = bpy.data.meshes.new(name)
    me.from_pydata([tuple(v3(p)) for p in pts], [], [])
    me.materials.append(material(c))
    me.update()
    return me


def link_child(parent, name, data):
    ob = bpy.data.objects.new(name, data)
    for col in parent.users_collection:
        col.objects.link(ob)
    ob.parent = parent
    return ob


def delete_tree(ob):
    for c in list(ob.children):
        delete_tree(c)
    data = ob.data
    bpy.data.objects.remove(ob, do_unlink=True)
    if data is not None and data.users == 0 and isinstance(data, bpy.types.Mesh):
        bpy.data.meshes.remove(data)


def set_geometry(ob, geo, keep=lambda child: True):
    """Gives `ob` (a mesh or empty node) the faces of `geo` and its lines as `_lines_pal_NNN`
    children. Existing line children are replaced; other children are kept if `keep` says so.
    Returns the object now carrying the faces (an empty gets a `<name>_body` mesh child)."""
    for c in list(ob.children):
        if "_lines_pal_" in c.name and c.name.startswith(ob.name + "_lines"):
            delete_tree(c)
        elif not keep(c):
            delete_tree(c)
    target = ob
    if ob.type == "MESH":
        old = ob.data
        ob.data = faces_mesh(ob.name, geo)
        if old.users == 0:
            bpy.data.meshes.remove(old)
        ob.data.name = ob.name
    else:
        body = bpy.data.objects.get(ob.name + "_body")
        if body is not None:
            delete_tree(body)
        if geo.faces:
            target = link_child(ob, ob.name + "_body", faces_mesh(ob.name + "_body", geo))
    for c, segs in sorted(geo.lines.items()):
        link_child(ob, "%s_lines_pal_%03d" % (ob.name, c), lines_mesh("%s_lines_pal_%03d" % (ob.name, c), segs, c))
    return target


def set_points(ob, pts):
    """Moves the vertices of a dots/lines mesh (keeps their count when possible)."""
    me = ob.data
    if len(me.vertices) == len(pts):
        for v, p in zip(me.vertices, pts):
            v.co = v3(p)
        me.update()
    else:
        c = index_of(ob.name)
        ob.data = points_mesh(ob.name, pts, c)
        if me.users == 0:
            bpy.data.meshes.remove(me)
        ob.data.name = ob.name


def index_of(name):
    i = name.rfind("pal_")
    return int(name[i + 4:i + 7])

