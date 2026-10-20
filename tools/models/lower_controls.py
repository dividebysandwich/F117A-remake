# Lowers the stick, the throttle and their arm rests (side consoles) of a cockpit .blend master
# and saves it (the VR hands met them too high):
#   blender -b assets-src/blend/cockpit_modern.blend --python-exit-code 1 --python tools/models/lower_controls.py -- 0.10
# Picks the root children over the side consoles (|x| 0.35..0.45 m, more than 0.35 m under the eye
# and less than 0.4 m ahead of it, which leaves the lower corners of the main panel alone): the
# same objects in both cockpits whatever their numeric name suffixes.
import sys

import bpy

drop = float(sys.argv[sys.argv.index("--") + 1])
root = bpy.data.objects["cockpit"]
moved = []
for o in root.children:
    p = o.matrix_world.translation
    if 0.35 <= abs(p.x) <= 0.45 and p.z < -0.35 and p.y < 0.4:
        o.location.z -= drop
        moved.append(o.name)
for name in ("stick", "throttle", "throttle_full"):
    if name not in moved:
        raise SystemExit(f"{name} not found over the side consoles")
print("lowered", drop, "m:", ", ".join(sorted(moved)))
bpy.ops.wm.save_mainfile()
