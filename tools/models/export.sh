#!/bin/sh
# Exports the .blend masters to the shipped model files:
#   tools/models/export.sh [stflt ce cu ... cockpit_modern cockpit_classic cockpit_f16]   (default: all files)
# The cockpits (cockpit_*) keep normals, UVs and materials (tools/models/export_cockpit.py);
# cockpit_f16.blend is generated first by tools/models/build_cockpit_f16.py.
# assets/models/ua.glb is assembled from ce.glb by tools/theater_ua/build.sh.
set -e
cd "$(dirname "$0")/../.."
for f in ${@:-stflt ce cu ko ku lb me nc pg vn photo cockpit_modern cockpit_classic cockpit_f16}; do
    blend=assets-src/blend/$f.blend
    script=tools/models/export.py
    case $f in cockpit_*) script=tools/models/export_cockpit.py ;; esac
    log=$(blender -b "$blend" --python-exit-code 1 --python $script -- "assets/models/$f.glb" 2>&1) || {
        echo "$log"
        exit 1
    }
    raw=$(cut -d' ' -f1 "assets-src/blend/$f.stamp" 2>/dev/null || echo -)
    echo "$raw $(sha256sum "$blend" | cut -d' ' -f1)" >"assets-src/blend/$f.stamp"
    echo "$f: exported"
done
