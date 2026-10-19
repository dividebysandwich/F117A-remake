"""Rebuilds the remodelled vehicles inside a converted .blend and exports it.

    blender -b assets-src/blend/<file>.blend --python tools/models/build.py -- <out.glb> [--save]

Only the models listed in aircraft.BUILDERS (stflt) or ground.MODELS (theaters) are touched;
`merge_glb.py` then splices exactly those into the shipped .glb.
"""

import json
import os
import sys

import bpy
from mathutils import Vector

sys.path.insert(0, os.path.dirname(__file__))

import aircraft  # noqa: E402
import ground  # noqa: E402
import mklib  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))


def subtree(o):
    yield o
    for c in o.children:
        yield from subtree(c)


def extent(ob, skip=("_dots_", "_s1v1", "_s13v1", "_s14v1")):
    pts = [o.matrix_world @ v.co for o in subtree(ob) if o.type == "MESH" and not any(s in o.name for s in skip)
           for v in o.data.vertices]
    lo = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
    hi = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
    return lo, hi


def place_dots(node, anchors):
    """Moves every `_dots_pal_NNN` mesh below `node` onto the nearest anchor of its colour."""
    for o in subtree(node):
        if o.type != "MESH" or "_dots_pal_" not in o.name:
            continue
        c = mklib.index_of(o.name)
        targets = anchors.get(c)
        if not targets:
            continue
        pts = [min(targets, key=lambda a: (a - v.co).length) for v in o.data.vertices]
        mklib.set_points(o, pts)


def apply_aircraft(index, craft):
    root = bpy.data.objects["stflt_%03d" % index]
    lod0 = bpy.data.objects["stflt_%03d_lod0" % index]
    lo, hi = extent(lod0)
    mlo, mhi = craft.body.bounds()
    s = (hi.y - lo.y) / (mhi.y - mlo.y)
    off = Vector((0, 0, 0)) if craft.pivot else Vector((0, lo.y - mlo.y * s, 0))

    def fit(g):
        return g.scaled(s, off)

    # Hot nozzle exits go into their own node, which the game flickers red/yellow.
    body = fit(craft.body)
    hot = mklib.Geo()
    hot.faces = [f for f in body.faces if f[1] == aircraft.FLAME]
    body.faces = [f for f in body.faces if f[1] != aircraft.FLAME]
    mklib.set_geometry(lod0, body)
    exhaust = bpy.data.objects.get(lod0.name + "_exhaust")
    if exhaust is not None:
        mklib.delete_tree(exhaust)
    if hot.faces:
        mklib.link_child(lod0, lod0.name + "_exhaust", mklib.faces_mesh(lod0.name + "_exhaust", hot))
    for c in list(lod0.children):
        if c.name.endswith("_s13v1"):
            if craft.flames is not None:
                mklib.set_geometry(c, fit(craft.flames))
        elif "_part" in c.name and craft.part is not None:
            mklib.set_geometry(c, fit(craft.part), keep=lambda x: "_dots_" in x.name)
    anchors = {k: [p * s + off for p in v] for k, v in craft.lights.items()}
    place_dots(lod0, anchors)
    return root.name


def main():
    argv = sys.argv[sys.argv.index("--") + 1:]
    out = argv[0]
    stem = os.path.splitext(os.path.basename(bpy.data.filepath))[0].split("_")[0]
    theater = stem if stem in ground.THEATERS else "ce"
    mklib.load_palette(os.path.join(ROOT, "assets", "palettes", "tod_%s.ron" % theater))
    changed = []
    if stem == "stflt":
        for index, build in sorted(aircraft.BUILDERS.items()):
            changed.append(apply_aircraft(index, build()))
    else:
        changed += ground.apply(stem)
    if "--save" in argv:
        bpy.context.preferences.filepaths.save_version = 0
        bpy.ops.wm.save_mainfile(compress=True)
    bpy.ops.export_scene.gltf(
        filepath=out, export_format="GLB", export_extras=True, export_yup=True, export_texcoords=False,
        export_normals=False, export_materials="EXPORT", use_visible=False, export_animations=False,
        use_mesh_edges=True, use_mesh_vertices=True,
    )
    with open(out + ".models.json", "w") as f:
        json.dump(changed, f)


main()
