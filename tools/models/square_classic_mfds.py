# Makes the classic cockpit's MFDs rectangular and saves the .blend master:
#   blender -b assets-src/blend/cockpit_classic.blend --python-exit-code 1 \
#       --python tools/models/square_classic_mfds.py
# The panel was built on the classic screen's pixels projected onto the tilted panel, so each screen
# came out a trapezoid: its inner edge upright, its outer edge leaning in towards the bottom.
# Each MFD is narrowed to its bottom width: the screen (with the camera picture and the rim)
# squeezed towards its inner edge, the bezel, its buttons and the panel's edge along it moved in
# with the outer edge. Small parts (lamps, caps and the controls) move whole, larger ones
# (screens, bezel pieces, panel plates) have their vertices moved; nothing outside a bezel moves.
import bpy
from mathutils import Vector

root = bpy.data.objects["cockpit"]
# The bezel's width outside the screen's outer edge, and how far past it vertices still go with
# it (the panel's edge along it).
BEZEL = 0.028
SLACK = 0.004
# Parts smaller than this (m) move whole.
SMALL = 0.10
# The panel: more than PANEL_Y ahead, from REACH[0] above the screens (the strip over them, not
# the windscreen's frame) to REACH[1] under them.
PANEL_Y = 0.55
REACH = (0.028, 0.05)


def screen(mat):
    """(inner x, outward sign, (outer x, z) at the top and the bottom) of a screen."""
    o = next(o for o in root.children_recursive if o.type == "MESH" and any(m and m.name == mat for m in o.data.materials))
    pts = [o.matrix_world @ v.co for v in o.data.vertices]
    top, bottom = max(p.z for p in pts), min(p.z for p in pts)
    centre = sum(p.x for p in pts) / len(pts)
    s = 1.0 if centre > 0 else -1.0
    inner = min((p.x for p in pts), key=lambda x: s * x)
    edge = lambda z: max((p for p in pts if abs(p.z - z) < 0.002), key=lambda p: s * p.x).x  # noqa: E731
    return inner, s, (edge(top), top), (edge(bottom), bottom)


mfds = [screen("screen_left"), screen("screen_right")]


def warp(p):
    """Where a point goes (only x changes); nothing off the panel."""
    for inner, s, (xt, zt), (xb, zb) in mfds:
        if p.y < PANEL_Y or not zb - REACH[1] <= p.z <= zt + REACH[0]:
            continue
        z = min(max(p.z, zb), zt)
        outer = xb + (xt - xb) * (z - zb) / (zt - zb)
        d, width, target = s * (p.x - inner), s * (outer - inner), s * (xb - inner)
        if -0.0005 <= d <= width:
            return Vector((inner + s * d * target / width, p.y, p.z))
        if width < d <= width + BEZEL + SLACK:
            return Vector((p.x + s * (target - width), p.y, p.z))
    return p


def meshes(o):
    return [c for c in [o, *o.children_recursive] if c.type == "MESH"]


moved, warped = 0, 0
for o in root.children:
    ms = meshes(o)
    if not ms:
        continue
    pts = [m.matrix_world @ v.co for m in ms for v in m.data.vertices]
    lo = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
    hi = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
    if max(hi - lo) < SMALL:
        c = (lo + hi) / 2
        dx = warp(c).x - c.x
        if abs(dx) > 1e-6:
            o.location.x += dx
            moved += 1
        continue
    for m in ms:
        inv = m.matrix_world.inverted()
        new = {}
        for v in m.data.vertices:
            w = m.matrix_world @ v.co
            n = warp(w)
            if abs(n.x - w.x) > 1e-6:
                new[v.index] = inv @ n
        if not new:
            continue
        if m.data.users > 1:
            raise SystemExit(f"{m.name}: shared mesh")
        for i, co in new.items():
            m.data.vertices[i].co = co
        warped += 1
for inner, s, (xt, zt), (xb, zb) in mfds:
    print(f"screen at {inner:.3f}: outer edge {xt:.3f} (top) -> {xb:.3f}")
print("moved", moved, "parts, reshaped", warped, "meshes")
bpy.ops.wm.save_mainfile()
