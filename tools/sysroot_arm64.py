#!/usr/bin/env python3
"""Builds an Ubuntu 24.04 arm64 sysroot for cross-compiling the Linux aarch64 build (the Steam
Frame) on an x86-64 PC: the packages the CI installs (.github/workflows/build.yml) and their
dependencies, unpacked into one folder. glibc 2.39, as on the CI runner and SteamOS.

    tools/sysroot_arm64.py [target/sysroot-arm64]

Needs bsdtar. tools/frame.sh runs it when the sysroot is missing and builds against it.
"""

import gzip
import os
import subprocess
import sys
import urllib.request

MIRROR = "https://ports.ubuntu.com/ubuntu-ports"
SUITES = ["noble", "noble-updates"]
COMPONENTS = ["main", "universe"]
ROOTS = ["libc6-dev", "libgcc-13-dev", "libasound2-dev", "libudev-dev", "libwayland-dev", "libxkbcommon-dev", "libx11-dev"]
# Never needed for linking (and drag in half a system).
SKIP = {"debconf", "dpkg", "perl", "perl-base", "debianutils", "libc-dev-bin", "rpcsvc-proto", "libwayland-bin", "gcc-13-base", "gcc-14-base", "install-info", "xkb-data", "pkgconf", "pkg-config"}


def fetch(url):
    with urllib.request.urlopen(url) as r:
        return r.read()


def packages():
    """name -> fields (the newest suite wins), and virtual name -> providing package."""
    pkgs, provides = {}, {}
    for suite in SUITES:
        for comp in COMPONENTS:
            text = gzip.decompress(fetch(f"{MIRROR}/dists/{suite}/{comp}/binary-arm64/Packages.gz")).decode()
            for block in text.split("\n\n"):
                f = {}
                key = None
                for line in block.splitlines():
                    if line.startswith(" ") and key:
                        f[key] += "\n" + line
                    elif ":" in line:
                        key, v = line.split(":", 1)
                        f[key] = v.strip()
                if "Package" in f:
                    pkgs[f["Package"]] = f
                    for p in f.get("Provides", "").split(","):
                        p = p.split("(")[0].strip()
                        if p:
                            provides.setdefault(p, f["Package"])
    return pkgs, provides


def deps(f):
    for field in ("Pre-Depends", "Depends"):
        for alt in f.get(field, "").split(","):
            names = [a.split("(")[0].split(":")[0].strip() for a in alt.split("|")]
            names = [n for n in names if n]
            if names:
                yield names


def main():
    default = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "target", "sysroot-arm64")
    out = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else default)
    cache = os.path.join(out, ".debs")
    os.makedirs(cache, exist_ok=True)
    pkgs, provides = packages()
    want, todo = set(), list(ROOTS)
    while todo:
        n = todo.pop()
        n = n if n in pkgs else provides.get(n, n)
        if n in want or n in SKIP:
            continue
        if n not in pkgs:
            print(f"missing: {n}", file=sys.stderr)
            continue
        want.add(n)
        for alts in deps(pkgs[n]):
            pick = next((a for a in alts if a in pkgs or a in provides), alts[0])
            todo.append(pick)
    for n in sorted(want):
        fn = pkgs[n]["Filename"]
        deb = os.path.join(cache, os.path.basename(fn))
        if not os.path.exists(deb):
            print(f"fetch {n}")
            with open(deb, "wb") as w:
                w.write(fetch(f"{MIRROR}/{fn}"))
        data = subprocess.run(["bsdtar", "-xOf", deb, "data.tar.*"], check=True, capture_output=True).stdout
        subprocess.run(["bsdtar", "-xf", "-", "-C", out], input=data, check=True)
    # Merged /usr: some packages still use /lib.
    for d in ("lib", "lib64"):
        p = os.path.join(out, d)
        if not os.path.lexists(p):
            os.symlink("usr/" + d if d == "lib" else "usr/lib", p)
    # Absolute symlinks (e.g. libfoo.so -> /lib/...) point into the host: make them relative.
    for root, _, files in os.walk(out):
        for name in files:
            p = os.path.join(root, name)
            if os.path.islink(p) and os.readlink(p).startswith("/"):
                target = out + os.readlink(p)
                os.unlink(p)
                os.symlink(os.path.relpath(target, root), p)
    print(f"{len(want)} packages in {out}")


main()
