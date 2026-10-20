# Replaces the stick and throttle meshes of an F-117A cockpit .blend master with the F-16C's
# (from its generated master), scaled so they look as large in VR as the F-16's do (the ratio of
# the two cockpits' VR rig scales, src/aircraft.rs), and saves it:
#   blender -b assets-src/blend/cockpit_modern.blend --python-exit-code 1 \
#       --python tools/models/copy_f16_controls.py -- assets-src/blend/cockpit_f16.blend 1.4706
# The F-117A's lever objects (`stick`, `throttle` with their properties) stay where they are;
# the throttle (and its `throttle_full` anchor) rises until the larger grip's base sits where
# the old one did. The stick gets its grip height (`grip`) for the VR grab.
import sys

import bpy
from mathutils import Matrix, Vector

f16_path, k = sys.argv[sys.argv.index("--") + 1:][:2]
k = float(k)
# The F-16's grip centre above its stick pivot (src/cockpit3d/levers.rs GRIP_HEIGHT).
F16_GRIP = 0.11

with bpy.data.libraries.load(f16_path) as (src, dst):
    dst.objects = list(src.objects)
loaded = {name: o for name, o in zip(src.objects, dst.objects) if o is not None}

collection = bpy.data.objects["cockpit"].users_collection[0]
materials = bpy.data.materials


def base_name(name):
    head, _, tail = name.rpartition(".")
    return head if tail.isdigit() and head else name


def lowest(objs):
    return min((o.matrix_world @ Vector(c)).z for o in objs for c in o.bound_box)


def replace(lever):
    ours = bpy.data.objects[lever]
    theirs = loaded[lever]
    old = [c for c in ours.children if c.type == "MESH"]
    # Where the old meshes' base was, relative to the lever's origin.
    base = lowest(old) - ours.matrix_world.translation.z
    for c in old:
        mesh = c.data
        bpy.data.objects.remove(c, do_unlink=True)
        if mesh.users == 0:
            bpy.data.meshes.remove(mesh)
    new = []
    for c in theirs.children:
        if c.type != "MESH":
            continue
        loc, rot, sca = c.matrix_basis.decompose()
        o = bpy.data.objects.new(f"{lever}_{base_name(c.name)}", c.data.copy())
        # Rotation and scale into the mesh, the offset scaled: the object keeps unit scale.
        o.data.transform(Matrix.Scale(k, 4) @ Matrix.LocRotScale(None, rot, sca))
        o.data.materials.clear()
        for m in c.data.materials:
            o.data.materials.append(materials.get(base_name(m.name)) or m)
        collection.objects.link(o)
        o.parent = ours
        o.matrix_parent_inverse = Matrix.Identity(4)
        o.location = loc * k
        new.append(o)
    bpy.context.view_layer.update()
    # The new base where the old one was.
    rise = base - (lowest(new) - ours.matrix_world.translation.z)
    return ours, rise, len(new)


stick, rise, n = replace("stick")
if abs(rise) > 1e-4:
    stick.location.z += rise
stick["grip"] = F16_GRIP * k
throttle, trise, m = replace("throttle")
throttle.location.z += trise
bpy.data.objects["throttle_full"].location.z += trise

# Drop everything else loaded from the F-16 master.
for o in loaded.values():
    if not o.users_collection:
        bpy.data.objects.remove(o, do_unlink=True)
bpy.ops.outliner.orphans_purge(do_local_ids=True, do_linked_ids=True, do_recursive=True)
print(f"stick: {n} meshes, raised {rise:.3f} m; throttle: {m} meshes, raised {trise:.3f} m; scale {k}")
bpy.ops.wm.save_mainfile()
