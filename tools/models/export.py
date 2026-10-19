"""Exports an open .blend master as is (the master holds every model as shipped):

    blender -b assets-src/blend/<file>.blend --python-exit-code 1 --python tools/models/export.py -- <out.glb>

Node names carry the game's conventions (`_lod<N>`, `_s<slot>v<value>`, `_part<N>`, `_exhaust`,
`_lines_pal_NNN` loose edges, `_dots_pal_NNN` loose vertices) and the object custom properties
are exported as node extras (attributes, LOD distances, collision planes, conditions).
"""

import sys

import bpy

out = sys.argv[sys.argv.index("--") + 1]
bpy.ops.export_scene.gltf(
    filepath=out, export_format="GLB", export_extras=True, export_yup=True, export_texcoords=False,
    export_normals=False, export_materials="EXPORT", use_visible=False, export_animations=False,
    use_mesh_edges=True, use_mesh_vertices=True,
)
