#!/bin/sh
# Rebuilds the Ukraine theater (models, tile layout, world, palette) from geo.py:
#   tools/theater_ua/build.sh
set -e
cd "$(dirname "$0")/../.."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
blender -b --factory-startup --python tools/theater_ua/models.py -- "$tmp/new.glb" >"$tmp/models.log" 2>&1 || {
    cat "$tmp/models.log"
    exit 1
}
python3 tools/theater_ua/assemble.py "$tmp/new.glb"
