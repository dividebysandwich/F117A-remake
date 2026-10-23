# Builds the UH-60A cockpit master (assets-src/blend/cockpit_uh60.blend) from the F-117A's modern
# cockpit:
#   blender -b assets-src/blend/cockpit_modern.blend --python-exit-code 1 \
#       --python tools/models/build_cockpit_uh60.py -- assets-src/blend/cockpit_uh60.blend
# then tools/models/export.sh cockpit_uh60.
#
# Kept: the front panel with its instruments, displays and controls, the HUD glass and its
# frame, the seat, the side consoles. Removed: the canopy frame and sills and the cockpit tub's
# walls (the helicopter's own fuselage is drawn around the cockpit, src/aircraft.rs). The side
# stick becomes a centre cyclic on a shaft from the floor; the throttle becomes the collective,
# its handle on an arm from a pivot behind it on the left console, swinging up 28 degrees (the
# `throttle_full` anchor, the `throttle_pivot` anchor at the pivot). A floor plate under it all.
#
# Blender frame: eye at the origin, x right, y forward, z up, metres.
import math
import sys

import bmesh
import bpy
from mathutils import Matrix, Vector

out = sys.argv[sys.argv.index("--") + 1]
root = bpy.data.objects["cockpit"]
collection = root.users_collection[0]
mats = bpy.data.materials

# The canopy sills and aft bow, the F-117A's fuselage shoulders, the tub's side walls.
REMOVE = ("frame", "frame.001", "frame.002", "frame.003", "dark.008", "dark.009",
          "panel_grey.007", "panel_grey.009")
FLOOR_Z = -1.14
# Cyclic: pivot on the floor between the knees; its grip raised this much on a shaft.
CYCLIC_PIVOT = Vector((0.0, 0.36, FLOOR_Z + 0.01))
CYCLIC_RAISE = 0.25
# Collective: handle (lowered) and pivot on the left console, swing (degrees).
COLLECTIVE_HANDLE = Vector((-0.40, 0.14, -0.80))
COLLECTIVE_PIVOT = Vector((-0.40, -0.36, -0.86))
COLLECTIVE_SWING = 28.0


def box(name, lo, hi, material, parent=root):
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    mesh = bpy.data.meshes.new(name)
    bm.to_mesh(mesh)
    bm.free()
    lo, hi = Vector(lo), Vector(hi)
    mesh.transform(Matrix.Translation((lo + hi) / 2) @ Matrix.Diagonal((*(hi - lo), 1.0)))
    mesh.materials.append(mats[material])
    o = bpy.data.objects.new(name, mesh)
    collection.objects.link(o)
    o.parent = parent
    o.matrix_parent_inverse = Matrix.Identity(4)
    return o


def rod(name, a, b, radius, material, parent, n=8):
    """A cylinder from a to b (parent frame)."""
    a, b = Vector(a), Vector(b)
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=n, radius1=radius, radius2=radius,
                          depth=(b - a).length)
    mesh = bpy.data.meshes.new(name)
    bm.to_mesh(mesh)
    bm.free()
    rot = Vector((0, 0, 1)).rotation_difference((b - a).normalized()).to_matrix().to_4x4()
    mesh.transform(Matrix.Translation((a + b) / 2) @ rot)
    for p in mesh.polygons:
        p.use_smooth = True
    mesh.materials.append(mats[material])
    o = bpy.data.objects.new(name, mesh)
    collection.objects.link(o)
    o.parent = parent
    o.matrix_parent_inverse = Matrix.Identity(4)
    return o


def empty(name, at, **props):
    o = bpy.data.objects.new(name, None)
    collection.objects.link(o)
    o.parent = root
    o.location = at
    for k, v in props.items():
        o[k] = v
    return o


for name in REMOVE:
    o = bpy.data.objects.get(name)
    if o is None:
        raise SystemExit(f"{name} not found")
    bpy.data.objects.remove(o, do_unlink=True)

# --- the floor
box("floor", (-0.62, -0.70, FLOOR_Z - 0.02), (0.72, 1.25, FLOOR_Z), "dark")

# --- centre cyclic
stick = bpy.data.objects["stick"]
stick.location = CYCLIC_PIVOT
for c in stick.children:
    c.location.z += CYCLIC_RAISE
stick["grip"] = float(stick["grip"]) + CYCLIC_RAISE
rod("stick_shaft", (0, 0, 0), (0, 0, CYCLIC_RAISE + 0.02), 0.014, "metal", stick)
# The boot at its foot (does not move with it).
rod("cyclic_boot", CYCLIC_PIVOT, CYCLIC_PIVOT + Vector((0, 0, 0.05)), 0.045, "rubber", root)

# --- collective
throttle = bpy.data.objects["throttle"]
throttle.location = COLLECTIVE_HANDLE
arm = COLLECTIVE_PIVOT - COLLECTIVE_HANDLE
rod("throttle_arm", (0, 0, -0.04), (arm.x, arm.y, arm.z), 0.018, "metal", throttle)
box("collective_pivot", COLLECTIVE_PIVOT - Vector((0.05, 0.05, 0.08)),
    COLLECTIVE_PIVOT + Vector((0.05, 0.05, 0.03)), "dark")
full = COLLECTIVE_PIVOT + Matrix.Rotation(math.radians(COLLECTIVE_SWING), 3, "X") @ -arm
bpy.data.objects["throttle_full"].location = full
empty("throttle_pivot", COLLECTIVE_PIVOT, anchor="throttle_pivot")

bpy.context.view_layer.update()
print(f"cyclic grip {stick['grip']:.3f} m; collective {COLLECTIVE_HANDLE} -> {full} "
      f"({(full - COLLECTIVE_HANDLE).length:.3f} m)")
bpy.ops.wm.save_as_mainfile(filepath=out)
