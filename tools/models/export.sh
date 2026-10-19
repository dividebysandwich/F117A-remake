#!/bin/sh
# Exports the .blend masters to the shipped model files:
#   tools/models/export.sh [stflt ce cu ...]   (default: all files)
# assets/models/ua.glb is assembled from ce.glb by tools/theater_ua/build.sh.
set -e
cd "$(dirname "$0")/../.."
for f in ${@:-stflt ce cu ko ku lb me nc pg vn photo}; do
    blend=assets-src/blend/$f.blend
    log=$(blender -b "$blend" --python-exit-code 1 --python tools/models/export.py -- "assets/models/$f.glb" 2>&1) || {
        echo "$log"
        exit 1
    }
    raw=$(cut -d' ' -f1 "assets-src/blend/$f.stamp")
    echo "$raw $(sha256sum "$blend" | cut -d' ' -f1)" >"assets-src/blend/$f.stamp"
    echo "$f: exported"
done
