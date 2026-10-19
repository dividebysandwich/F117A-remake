#!/bin/sh
# Rebuilds the remodelled vehicles in the .blend masters and splices them into assets/models.
#   tools/models/build.sh [stflt ce cu ...]   (default: all files)
# The other models' detail is added afterwards by tools/models/detail.py (see there).
set -e
cd "$(dirname "$0")/../.."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for f in ${@:-stflt ce cu ko ku lb me nc pg vn}; do
    blend=assets-src/blend/$f.blend
    blender -b "$blend" --python tools/models/build.py -- "$tmp/$f.glb" --save >"$tmp/$f.log" 2>&1 || {
        cat "$tmp/$f.log"
        exit 1
    }
    python3 tools/models/merge_glb.py "assets/models/$f.glb" "$tmp/$f.glb" "$tmp/$f.glb.models.json"
    raw=$(cut -d' ' -f1 "assets-src/blend/$f.stamp")
    echo "$raw $(sha256sum "$blend" | cut -d' ' -f1)" >"assets-src/blend/$f.stamp"
    echo "$f: $(cat "$tmp/$f.glb.models.json")"
done
