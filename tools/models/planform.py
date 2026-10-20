"""Outlines of the flyable aircraft seen from above, for the loadout editor (src/loadout.rs).

Projects every triangle of the exterior model in assets/models/stflt.glb onto the ground plane,
fills them into a fine grid, traces the outline of the filled area and simplifies it. Prints the
outline (from the nose down the right side and back up the left) as Rust constants in metres: x
right of the centreline, y aft of the nose.

    python3 tools/models/planform.py
"""

import os
import struct
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))
import glbio  # noqa: E402

ROOT = os.path.join(os.path.dirname(__file__), "..", "..")
# AIRCRAFT_MODEL_SCALE (src/player.rs): world units / 8, a world unit 512000 / 2^20 m.
SCALE = 512000.0 / 1048576.0 / 8.0
# Grid cell (m) and outline tolerance (m).
CELL = 0.02
TOLERANCE = 0.05
AIRCRAFT = [("F117_OUTLINE", "stflt_024"), ("F16_OUTLINE", "stflt_023")]


def accessor(js, binary, i):
    a = js["accessors"][i]
    v = js["bufferViews"][a["bufferView"]]
    comps = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[a["type"]]
    fmt = {5126: "f", 5125: "I", 5123: "H", 5121: "B"}[a["componentType"]]
    size = struct.calcsize(fmt)
    stride = v.get("byteStride", size * comps)
    off = v.get("byteOffset", 0) + a.get("byteOffset", 0)
    out = [struct.unpack_from("<" + fmt * comps, binary, off + k * stride) for k in range(a["count"])]
    return np.array(out, dtype=float)


def triangles(js, binary, node):
    """Triangles (n, 3, 3) of a node and its children, in model units."""
    n = js["nodes"][node]
    tris = []
    if "mesh" in n:
        for p in js["meshes"][n["mesh"]]["primitives"]:
            if p.get("mode", 4) != 4:
                continue
            pos = accessor(js, binary, p["attributes"]["POSITION"])
            idx = accessor(js, binary, p["indices"]).astype(int).ravel() if "indices" in p else np.arange(len(pos))
            tris.append(pos[idx].reshape(-1, 3, 3))
    for c in n.get("children", []):
        tris.extend(triangles(js, binary, c))
    t = n.get("translation", [0, 0, 0])
    return [x + np.array(t) for x in tris]


def fill(tris):
    """Cells covered by the triangles seen from above: (mask, x0, y0)."""
    pts = tris[:, :, [0, 2]] * SCALE
    lo, hi = pts.reshape(-1, 2).min(0) - 4 * CELL, pts.reshape(-1, 2).max(0) + 4 * CELL
    w, h = np.ceil((hi - lo) / CELL).astype(int) + 1
    mask = np.zeros((h, w), bool)
    gx, gy = np.meshgrid(np.arange(w), np.arange(h))
    for t in pts:
        a, b, c = (t - lo) / CELL
        x0, y0 = np.floor(np.minimum(np.minimum(a, b), c)).astype(int)
        x1, y1 = np.ceil(np.maximum(np.maximum(a, b), c)).astype(int) + 1
        px, py = gx[y0:y1, x0:x1] + 0.5, gy[y0:y1, x0:x1] + 0.5

        def edge(p, q):
            return (q[0] - p[0]) * (py - p[1]) - (q[1] - p[1]) * (px - p[0])

        e = [edge(a, b), edge(b, c), edge(c, a)]
        inside = ((e[0] >= 0) & (e[1] >= 0) & (e[2] >= 0)) | ((e[0] <= 0) & (e[1] <= 0) & (e[2] <= 0))
        mask[y0:y1, x0:x1] |= inside
    return mask, lo


def trace(mask):
    """Boundary of the largest filled region, as grid corner points (Moore neighbourhood)."""
    h, w = mask.shape
    start = next((x, y) for y in range(h) for x in range(w) if mask[y, x])
    dirs = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)]
    path, cur, back = [start], start, 4
    while True:
        for k in range(8):
            d = (back + 1 + k) % 8
            nx, ny = cur[0] + dirs[d][0], cur[1] + dirs[d][1]
            if 0 <= nx < w and 0 <= ny < h and mask[ny, nx]:
                back = (d + 4) % 8
                cur = (nx, ny)
                break
        else:
            break
        if cur == start:
            break
        path.append(cur)
    return np.array(path, dtype=float)


def simplify(pts, tol):
    """Douglas-Peucker on an open polyline."""
    if len(pts) < 3:
        return pts
    a, b = pts[0], pts[-1]
    ab = b - a
    n = np.hypot(*ab)
    d = np.abs(ab[0] * (pts[:, 1] - a[1]) - ab[1] * (pts[:, 0] - a[0])) / n if n > 0 else np.hypot(*(pts - a).T)
    i = int(np.argmax(d))
    if d[i] <= tol:
        return np.array([a, b])
    return np.vstack([simplify(pts[: i + 1], tol)[:-1], simplify(pts[i:], tol)])


def shift_or(m, r):
    """Dilation by a square of radius r cells."""
    out = m.copy()
    for dy in range(-r, r + 1):
        for dx in range(-r, r + 1):
            out |= np.roll(np.roll(m, dy, 0), dx, 1)
    return out


def close(mask, r):
    """Closes gaps up to 2 r cells wide (slits between parts, the wingtip rails) and fills holes."""
    m = ~shift_or(~shift_or(mask, r), r)
    # Holes: what the outside cannot reach.
    outside = np.zeros_like(m)
    outside[0, :] = outside[-1, :] = outside[:, 0] = outside[:, -1] = True
    outside &= ~m
    while True:
        grown = shift_or(outside, 1) & ~m
        if (grown == outside).all():
            return ~outside
        outside = grown


def outline(js, binary, name):
    node = next(i for i, n in enumerate(js["nodes"]) if n.get("name") == name)
    tris = np.concatenate(triangles(js, binary, node))
    mask, lo = fill(tris)
    mask = close(np.pad(mask, 12), 5)[12:-12, 12:-12]
    # Model -Z forward: metres aft of the nose; x right.
    p = trace(mask) * CELL + lo
    p[:, 1] -= p[:, 1].min()
    # Start at the nose, then down the right side.
    p = np.roll(p, -int(np.argmin(p[:, 1])), axis=0)
    if p[len(p) // 8, 0] < 0:
        p = np.roll(p[::-1], 1, axis=0)
    # The right side down to where the outline crosses the centreline at the tail, simplified
    # and mirrored: exactly symmetric.
    centre = lo[0] + round(-lo[0] / CELL) * CELL
    p[:, 0] -= centre
    end = next(i for i in range(len(p) // 8, len(p)) if p[i, 0] < 0)
    right = np.vstack([[0.0, p[0, 1]], np.maximum(p[1:end], [0.0, -np.inf]), [0.0, p[end, 1]]])
    right = simplify(right, TOLERANCE)
    left = right[-2:0:-1] * [-1.0, 1.0]
    p = np.vstack([right, left])
    return p, p[:, 0].max(), p[:, 1].max()


def main():
    js, binary = glbio.read(os.path.join(ROOT, "assets", "models", "stflt.glb"))
    for const, name in AIRCRAFT:
        pts, half_span, length = outline(js, binary, name)
        print(f"/// {name} seen from above: half span {half_span:.2f} m, length {length:.2f} m.")
        body = ", ".join(f"({x:.2f}, {y:.2f})" for x, y in pts)
        print(f"const {const}: &[(f32, f32)] = &[{body}];")


if __name__ == "__main__":
    main()
