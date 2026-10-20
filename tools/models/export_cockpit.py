# Exports a cockpit .blend master (assets-src/blend/cockpit_*.blend) to the shipped .glb:
#   blender -b assets-src/blend/cockpit_modern.blend --python tools/models/export_cockpit.py -- assets/models/cockpit_modern.glb
# Unlike the world models, the cockpit keeps its normals, UVs and materials: the game binds
# behaviour by material name and object custom properties (see src/cockpit3d/model.rs).
import sys

import bpy

out = sys.argv[sys.argv.index("--") + 1]
bpy.ops.export_scene.gltf(
    filepath=out,
    export_format="GLB",
    export_extras=True,
    export_yup=True,
    export_texcoords=True,
    export_normals=True,
    export_materials="EXPORT",
    export_image_format="AUTO",
    use_visible=False,
    export_animations=False,
    export_apply=False,
)
