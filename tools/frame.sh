#!/bin/bash
# Cross-compiles the aarch64 Linux release build on an x86-64 Linux PC, deploys it to a Steam
# Frame over ssh and launches it in VR there.
#
#   tools/frame.sh [options] [ENV=VALUE ...] [-- game args]
#
# Options:
#   --no-build     deploy and launch the last build
#   --no-launch    build and deploy only
#   --time SECS    quit the game after SECS seconds (default: run until Ctrl-C)
#   --detach       leave the game running and return (stop it with `tools/frame.sh --stop`)
#   --stop         stop a running game and exit
# ENV=VALUE pairs are set for the game (e.g. SM89_PROFILE=10s SM89_MSAA=2).
# Game args default to: --vr --mission 1234 --theater-index 3 --weather clear --modern
#
# Ctrl-C stops the game on the Frame. The log is copied to target/frame-logs/, followed by
# SteamVR's measurement of the run (ApplicationTime CPU / GPU per frame, from vrcompositor.txt).
#
# Needs:
# - ssh access to the Frame as host `frame` (an entry in ~/.ssh/config with key login; on the
#   Frame: `passwd`, then `sudo systemctl enable --now sshd`). FRAME_HOST overrides the host.
# - The aarch64-unknown-linux-gnu Rust target (`rustup target add`), aarch64-linux-gnu-gcc,
#   python3 and bsdtar. The first build makes an Ubuntu 24.04 arm64 sysroot (the CI's libraries,
#   glibc 2.39 like SteamOS) in target/sysroot-arm64 with tools/sysroot_arm64.py.
# - SteamVR running on the Frame. The game is copied to ~/sm89-dev there; the OpenXR loader
#   comes from a CI aarch64 AppImage in the Frame's ~/Downloads on the first deploy.

set -euo pipefail
cd "$(dirname "$0")/.."
HOST=${FRAME_HOST:-frame}
DIR=sm89-dev
TARGET=aarch64-unknown-linux-gnu
SR=$PWD/target/sysroot-arm64
BIN=target/$TARGET/release/stealth-mission-89

build=1 launch=1 secs="" detach=0
envs=() args=()
while [ $# -gt 0 ]; do
    case $1 in
        --no-build) build=0 ;;
        --no-launch) launch=0 ;;
        --time) secs=$2; shift ;;
        --detach) detach=1 ;;
        --stop) stop=1 ;;
        --) shift; args=("$@"); break ;;
        *=*) envs+=("$1") ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
    shift
done
[ ${#args[@]} -eq 0 ] && args=(--vr --mission 1234 --theater-index 3 --weather clear --modern)

remote() { ssh -o BatchMode=yes "$HOST" "$@"; }
stop_game() { remote 'pkill -INT -x stealth-mission; sleep 2; pkill -KILL -x stealth-mission' || true; }

if [ "${stop:-0}" = 1 ]; then
    stop_game
    exit 0
fi

if [ $build = 1 ]; then
    if [ ! -d "$SR/usr/lib/aarch64-linux-gnu" ]; then
        echo "making the arm64 sysroot in $SR"
        python3 tools/sysroot_arm64.py "$SR"
    fi
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=--sysroot=$SR -C link-arg=-L$SR/usr/lib/gcc/aarch64-linux-gnu/13"
    export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++
    export CFLAGS_aarch64_unknown_linux_gnu="--sysroot=$SR" CXXFLAGS_aarch64_unknown_linux_gnu="--sysroot=$SR"
    export PKG_CONFIG_ALLOW_CROSS=1 PKG_CONFIG_SYSROOT_DIR=$SR
    export PKG_CONFIG_LIBDIR=$SR/usr/lib/aarch64-linux-gnu/pkgconfig:$SR/usr/share/pkgconfig
    cargo build --release --target $TARGET -p stealth-mission-89 --no-default-features --features stealth-mission-89/xr
    # The Frame gets a stripped copy; the unstripped one stays here for backtraces.
    aarch64-linux-gnu-strip -o "$BIN.stripped" "$BIN"
fi

echo "deploying to $HOST:$DIR"
stop_game
remote "mkdir -p $DIR && cd $DIR && if [ ! -e libopenxr_loader.so ]; then
    a=\$(ls -t ~/Downloads/stealth-mission-89-*-aarch64.AppImage 2>/dev/null | head -1)
    [ -n \"\$a\" ] || { echo 'no OpenXR loader: put a CI aarch64 AppImage in ~/Downloads on the Frame' >&2; exit 1; }
    rm -rf squashfs-root && \"\$a\" --appimage-extract >/dev/null && cp -L squashfs-root/usr/bin/libopenxr_loader.so . && rm -rf squashfs-root
fi"
rsync -az --delete assets "$HOST:$DIR/"
rsync -az "$BIN.stripped" "$HOST:$DIR/stealth-mission-89"
[ $launch = 1 ] || exit 0

# The game runs in the Frame's gamescope session, detached from this ssh connection.
log=run-$(date +%Y%m%d-%H%M%S).log
limit=${secs:+timeout -s INT -k 10 $secs}
remote "cd $DIR && DISPLAY=:0 XDG_RUNTIME_DIR=/run/user/\$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$(id -u)/bus \
    BEVY_ASSET_ROOT=\$PWD ${envs[*]} setsid nohup $limit ./stealth-mission-89 ${args[*]} > $log 2>&1 < /dev/null &"
echo "launched: ${envs[*]} ${args[*]} (log $DIR/$log)"
if [ $detach = 1 ]; then
    exit 0
fi

finish() {
    trap - INT TERM
    stop_game
    mkdir -p target/frame-logs
    if scp -q "$HOST:$DIR/$log" target/frame-logs/; then
        sed -i 's/\x1b\[[0-9;]*m//g' "target/frame-logs/$log"
        echo "log: target/frame-logs/$log"
    fi
    sleep 2
    remote 'grep -E "Game Info|Total\.\.\." ~/.local/share/Steam/logs/vrcompositor.txt | tail -2 | sed "s/^.*- //"' || true
    exit 0
}
trap finish INT TERM
# Follow the log until the game exits (or Ctrl-C).
remote "cd $DIR && tail -n +1 --pid=\$(pgrep -nx stealth-mission || echo 1) -f $log" || true
finish
