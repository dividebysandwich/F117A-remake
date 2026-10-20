# Lowers the seat (pan, back, cushions, head rest, rails and the ejection handle) of a cockpit
# .blend master and saves it (in VR the seat met the pilot too high):
#   blender -b assets-src/blend/cockpit_modern.blend --python-exit-code 1 --python tools/models/lower_seat.py -- 0.22
# Picks the root children by name prefix (seat_, cushion_, the ejection handle) and the two
# rails beside the seat back (metal objects at |x| 0.2..0.3 m, 0.3..0.6 m behind the eye): the
# same objects in both cockpits whatever their numeric name suffixes.
import sys

import bpy

drop = float(sys.argv[sys.argv.index("--") + 1])
root = bpy.data.objects["cockpit"]
moved = []
for o in root.children:
    p = o.matrix_world.translation
    rail = o.name.startswith("metal_") and 0.2 <= abs(p.x) <= 0.3 and -0.6 <= p.y <= -0.3
    if o.name.startswith(("seat_", "cushion_", "control_Action_Eject_")) or rail:
        o.location.z -= drop
        moved.append(o.name)
if not any(n.startswith("control_Action_Eject_") for n in moved) or sum(n.startswith("metal_") for n in moved) != 2:
    raise SystemExit(f"seat parts not found: {moved}")
print("lowered", drop, "m:", ", ".join(sorted(moved)))
bpy.ops.wm.save_mainfile()
