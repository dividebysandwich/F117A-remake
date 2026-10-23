"""Helicopter modelling helpers shared by uh60.py and mi24.py (run inside Blender).

Fuselages are lofted through stations: y, then the heights of the belly, the lower and upper
edge of the flat side, the roof shoulder and the top, then the half widths at those heights.
Rings run belly corner, side (lower, upper), shoulder, top on each side, so a full ring has 10
quads: 0/8 belly chamfer, 1/7 side, 2/6 upper side, 3/5 roof slope, 4 top strip, 9 belly.
Built in metres, scaled to model units by the caller. Model space: x right, y forward, z up.
"""

import math

import bpy
from mathutils import Vector

import mklib
from mklib import Geo, v3


def ring(s, full=True):
    """Cross-section points of a station: 10 (full) or 6 (coarse: belly corner, side, top)."""
    y, zb, zl, zu, zs, zt, wb, wl, wu, ws, wt = s
    if full:
        right = [(wb, zb), (wl, zl), (wu, zu), (ws, zs), (wt, zt)]
    else:
        right = [(wb, zb), ((wl + wu) / 2, (zl + zu) / 2), ((ws + wt) / 2, zt)]
    pts = right + [(-x, z) for x, z in reversed(right)]
    return [Vector((x, y, z)) for x, z in pts]


def fuselage(stations, paint, glass, full=True, glazed=None):
    """Loft through the stations; `glazed` maps segment index -> glazed quad indices."""
    g = Geo()
    rings = [ring(s, full) for s in stations]
    m = len(rings[0])
    for i in range(len(rings) - 1):
        a, b = rings[i], rings[i + 1]
        mid = Vector((0, (stations[i][0] + stations[i + 1][0]) / 2, (a[0].z + a[m // 2 - 1].z + b[0].z + b[m // 2 - 1].z) / 4))
        for k in range(m):
            k1 = (k + 1) % m
            g.face([a[k], a[k1], b[k1], b[k]], glass if glazed and k in glazed.get(i, ()) else paint, mid)
    for r, other in ((rings[0], rings[1]), (rings[-1], rings[-2])):
        g.face(r, paint, sum(other, Vector()) / m)
    return g


def side_x(stations, y, z):
    """Half width of the fuselage's flat side at (y, z), interpolated between the stations."""
    for s0, s1 in zip(stations, stations[1:]):
        if s1[0] <= y <= s0[0]:
            t = (s0[0] - y) / (s0[0] - s1[0])
            s = [a + (b - a) * t for a, b in zip(s0, s1)]
            _, _, zl, zu, _, _, _, wl, wu, _, _ = s
            return wl + (wu - wl) * min(max((z - zl) / (zu - zl), 0.0), 1.0)
    raise ValueError(y)


def side_panel(stations, y0, y1, z0, z1, paint, g, lift=0.012):
    """A window on both flat sides (a face just outside the skin)."""
    for sx in (-1, 1):
        pts = [(sx * (side_x(stations, y, z) + lift), y, z) for y, z in ((y0, z0), (y1, z0), (y1, z1), (y0, z1))]
        g.face(pts, paint, (0, (y0 + y1) / 2, (z0 + z1) / 2))


def side_outline(stations, pts, colour, g, lift=0.01):
    """Panel seam lines on both sides through (y, z) points (closed)."""
    for sx in (-1, 1):
        p = [(sx * (side_x(stations, y, z) + lift), y, z) for y, z in pts]
        for a, b in zip(p, p[1:] + p[:1]):
            g.line(a, b, colour)


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


def rotor(level, blades, radius, chord, paint, hub_paint, clockwise, phase=0.0, cone_deg=1.5):
    """Main rotor about its hub (the part origin). `clockwise` as seen from above sets which
    edge of the blades leads. Level 0: hub, arms, swept tips; 1: plain blades; 2: flat sheets."""
    g = Geo()
    up = Vector((0, 0, 1))
    cone = math.tan(math.radians(cone_deg))
    if level == 0:
        g.add(mklib.frustum((0, 0, -0.10), (0, 0, 0.10), 0.36, 0.30, hub_paint, n=8))
        g.add(mklib.frustum((0, 0, 0.10), (0, 0, 0.24), 0.12, 0.08, hub_paint, n=6))
    elif level == 1:
        g.add(mklib.frustum((0, 0, -0.08), (0, 0, 0.08), 0.34, 0.30, hub_paint, n=6))
    for k in range(blades):
        a = phase + 2 * math.pi * k / blades
        d = Vector((math.cos(a), math.sin(a), 0))
        c = up.cross(d) * (-1 if clockwise else 1)
        if level == 2:
            p0, p1 = d * 0.3, d * radius + up * radius * cone
            g.face2([p0 + c * chord * 0.25, p1 + c * chord * 0.25, p1 - c * chord * 0.75, p0 - c * chord * 0.75], paint)
            continue
        if level == 0:
            g.add(mklib.frustum(d * 0.30, d * 0.85, 0.09, 0.07, hub_paint, n=4))
            st = [(0.85, chord * 0.6, 0.0, 0.0, 0.10), (1.6, chord, 0.0, 0.02, 0.07), (radius - 0.55, chord, 0.0, (radius - 0.55) * cone, 0.06),
                  (radius, chord * 0.45, 0.20, radius * cone, 0.03)]
        else:
            st = [(0.35, chord * 0.65, 0.0, 0.0, 0.08), (radius - 0.5, chord, 0.0, (radius - 0.5) * cone, 0.06), (radius, chord * 0.5, 0.2, radius * cone, 0.03)]
        g.add(mklib.tube(blade_rings(d, c, up, st), paint))
    return g


def tail_rotor(level, blades, radius, chord, paint, hub_paint):
    """Tail rotor in its own frame (spins about local x): blades in the y-z plane."""
    g = Geo()
    if level == 0:
        g.add(mklib.frustum((-0.08, 0, 0), (0.08, 0, 0), 0.12, 0.10, hub_paint, n=6))
    side = Vector((1, 0, 0))
    for k in range(blades):
        a = math.pi / 4 + 2 * math.pi * k / blades
        d = Vector((0, math.cos(a), math.sin(a)))
        c = Vector((0, -math.sin(a), math.cos(a)))
        if level == 0:
            st = [(0.15, chord * 0.7, 0.0, 0.0, 0.06), (radius - 0.1, chord, 0.0, 0.0, 0.04), (radius, chord * 0.8, 0.0, 0.0, 0.03)]
            g.add(mklib.tube(blade_rings(d, c, side, st), paint))
        else:
            p0, p1 = d * 0.1, d * radius
            g.face2([p0 + c * chord * 0.3, p1 + c * chord * 0.3, p1 - c * chord * 0.7, p0 - c * chord * 0.7], paint)
    return g


def flipped(geo):
    """Geometry for a rotor part turned upside down (rest pose rotated 180 degrees about x):
    the game spins slot 17 clockwise seen along the part's local +z, so a part whose local +z
    points down turns anticlockwise seen from above."""
    return geo.transformed(lambda p: Vector((p.x, -p.y, -p.z)))


def link(parent, name, data):
    return mklib.link_child(parent, name, data)


def mesh_with_lines(parent, name, geo, loc=(0, 0, 0)):
    ob = link(parent, name, mklib.faces_mesh(name, geo))
    ob.location = loc
    for c, segs in sorted(geo.lines.items()):
        ln = f"{name}_lines_pal_{c:03}"
        link(ob, ln, mklib.lines_mesh(ln, segs, c))
    return ob


def lights(lod, slot, groups):
    """Dot lights shown while state slot `slot` is 1: {colour: [points in model units]}."""
    group = bpy.data.objects.new(f"{lod.name}_s{slot}v1", None)
    group["conds"] = [[slot, 1]]
    for col in lod.users_collection:
        col.objects.link(group)
    group.parent = lod
    for c, pts in groups.items():
        name = f"{group.name}_dots_pal_{c:03}"
        link(group, name, mklib.points_mesh(name, [v3(p) for p in pts], c))


def radius(*geos):
    return math.ceil(max(p.length for g in geos for f, _ in g.faces for p in f))


def build_levels(root, levels, k, hub, tail_hub, tail_rot, nav, beacons):
    """Rebuilds the detail levels under `root`: `levels` is a list of (max_dist, body Geo, main
    rotor Geo, tail rotor Geo, main rotor rest rotation) in metres; scaled by `k`."""
    for c in list(root.children):
        mklib.delete_tree(c)
    root["lod_dists"] = [lv[0] for lv in levels]
    hub, tail_hub = v3(hub) * k, v3(tail_hub) * k
    for level, (dist, body, main, tail, main_rot) in enumerate(levels):
        geo = body.scaled(k)
        lod = mesh_with_lines(root, f"{root.name}_lod{level}", geo)
        lod["max_dist"] = dist
        main = main.scaled(k)
        part = mesh_with_lines(lod, f"{lod.name}_part1", main, hub)
        part.rotation_euler = main_rot
        part["state_slot"] = 17
        part["axis"] = 0
        tail = tail.scaled(k)
        part = mesh_with_lines(lod, f"{lod.name}_part2", tail, tail_hub)
        part.rotation_euler = tail_rot
        part["state_slot"] = 17
        part["axis"] = 1
        lod["radius"] = radius(geo, main.transformed(lambda p: p + hub))
        lights(lod, 12, {c: [v3(p) * k for p in pts] for c, pts in nav.items()})
        lights(lod, 20, {c: [v3(p) * k for p in pts] for c, pts in beacons.items()})
        print(f"{lod.name}: {len(geo.faces) + len(main.faces) + len(tail.faces)} faces, radius {lod['radius']}")
