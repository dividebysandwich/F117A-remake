#!/bin/sh
# Builds the model files from their .blend masters (remodels and detail, see build.py) and saves
# the masters with the result:
#   tools/models/build.sh [stflt ce cu ...]   (default: all files)
# assets/models/ua.glb is assembled from ce.glb by tools/theater_ua/build.sh.
set -e
cd "$(dirname "$0")/../.."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for f in ${@:-stflt ce cu ko ku lb me nc pg vn photo}; do
    blend=assets-src/blend/$f.blend
    blender -b "$blend" --python-exit-code 1 --python tools/models/build.py -- "assets/models/$f.glb" --save >"$tmp/$f.log" 2>&1 || {
        cat "$tmp/$f.log"
        exit 1
    }
    raw=$(cut -d' ' -f1 "assets-src/blend/$f.stamp")
    echo "$raw $(sha256sum "$blend" | cut -d' ' -f1)" >"assets-src/blend/$f.stamp"
    echo "$f: built"
done
