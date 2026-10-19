"""Replaces whole models (root node subtrees) of a shipped .glb with those of a fresh export.

    python3 tools/models/merge_glb.py <shipped.glb> <export.glb> <models.json> [-o out.glb]

The shipped files carry fixes made after the .blend conversion, so everything not listed in
models.json stays byte-for-byte the same data; replaced models keep their place in the node
order (the model viewer lists them in file order). Unused meshes, accessors and buffer views
are dropped and the binary chunk is rebuilt.
"""

import argparse
import json
import struct


def read(path):
    d = open(path, "rb").read()
    assert d[:4] == b"glTF"
    off, js, binary = 12, None, b""
    while off < len(d):
        n, kind = struct.unpack_from("<II", d, off)
        chunk = d[off + 8:off + 8 + n]
        if kind == 0x4E4F534A:
            js = json.loads(chunk)
        elif kind == 0x004E4942:
            binary = chunk
        off += 8 + n
    return js, binary


def write(path, js, binary):
    j = json.dumps(js, separators=(",", ":")).encode()
    j += b" " * (-len(j) % 4)
    binary += b"\0" * (-len(binary) % 4)
    total = 12 + 8 + len(j) + 8 + len(binary)
    with open(path, "wb") as f:
        f.write(struct.pack("<III", 0x46546C67, 2, total))
        f.write(struct.pack("<II", len(j), 0x4E4F534A) + j)
        f.write(struct.pack("<II", len(binary), 0x004E4942) + binary)


def subtree(js, i):
    out = [i]
    for c in js["nodes"][i].get("children", []):
        out += subtree(js, c)
    return out


def roots(js):
    child = {c for n in js["nodes"] for c in n.get("children", [])}
    return [i for i in range(len(js["nodes"])) if i not in child]


def merge(a, abin, b, bbin, names):
    names = set(names)
    a_root = {a["nodes"][i]["name"]: i for i in roots(a)}
    b_root = {b["nodes"][i]["name"]: i for i in roots(b)}
    missing = names - set(a_root) | names - set(b_root)
    assert not missing, f"models not in both files: {sorted(missing)}"

    out = {k: v for k, v in a.items() if k not in ("nodes", "meshes", "accessors", "bufferViews", "buffers", "materials", "scenes")}
    nodes, meshes, accessors, views, materials = [], [], [], [], []
    blob = bytearray()
    mat_index = {}
    cache = {}

    def material(src, i):
        m = src["materials"][i]
        if m["name"] not in mat_index:
            mat_index[m["name"]] = len(materials)
            materials.append(m)
        return mat_index[m["name"]]

    def view(src, sbin, i):
        key = (id(src), "v", i)
        if key not in cache:
            v = dict(src["bufferViews"][i])
            o = v.get("byteOffset", 0)
            data = sbin[o:o + v["byteLength"]]
            blob.extend(b"\0" * (-len(blob) % 4))
            v["byteOffset"] = len(blob)
            v["buffer"] = 0
            blob.extend(data)
            cache[key] = len(views)
            views.append(v)
        return cache[key]

    def accessor(src, sbin, i):
        key = (id(src), "a", i)
        if key not in cache:
            acc = dict(src["accessors"][i])
            if "bufferView" in acc:
                acc["bufferView"] = view(src, sbin, acc["bufferView"])
            cache[key] = len(accessors)
            accessors.append(acc)
        return cache[key]

    def mesh(src, sbin, i):
        key = (id(src), "m", i)
        if key not in cache:
            m = dict(src["meshes"][i])
            prims = []
            for p in m["primitives"]:
                p = dict(p)
                p["attributes"] = {k: accessor(src, sbin, v) for k, v in p["attributes"].items()}
                if "indices" in p:
                    p["indices"] = accessor(src, sbin, p["indices"])
                if "material" in p:
                    p["material"] = material(src, p["material"])
                prims.append(p)
            m["primitives"] = prims
            cache[key] = len(meshes)
            meshes.append(m)
        return cache[key]

    # Node order: A's order, with each replaced model's subtree (in B's order) in its place.
    order = []
    replaced = set()
    for name in names:
        replaced.update(subtree(a, a_root[name]))
    b_nodes = {name: sorted(subtree(b, b_root[name])) for name in names}
    a_name_of_root = {i: n for n, i in a_root.items()}
    for i in range(len(a["nodes"])):
        if i in replaced:
            if a_name_of_root.get(i) in names:
                order += [(b, bbin, j) for j in b_nodes[a_name_of_root[i]]]
            continue
        order.append((a, abin, i))
    index = {(id(s), j): k for k, (s, _, j) in enumerate(order)}
    for src, sbin, j in order:
        n = dict(src["nodes"][j])
        if "children" in n:
            n["children"] = [index[(id(src), c)] for c in n["children"]]
        if "mesh" in n:
            n["mesh"] = mesh(src, sbin, n["mesh"])
        nodes.append(n)
    child = {c for n in nodes for c in n.get("children", [])}
    out["scene"] = 0
    out["scenes"] = [{"name": a["scenes"][a.get("scene", 0)].get("name", "Scene"), "nodes": [i for i in range(len(nodes)) if i not in child]}]
    out["nodes"], out["meshes"], out["accessors"], out["bufferViews"], out["materials"] = nodes, meshes, accessors, views, materials
    out["buffers"] = [{"byteLength": len(blob) + (-len(blob) % 4)}]
    return out, bytes(blob)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("shipped")
    ap.add_argument("export")
    ap.add_argument("models")
    ap.add_argument("-o", "--out")
    args = ap.parse_args()
    a, abin = read(args.shipped)
    b, bbin = read(args.export)
    names = json.load(open(args.models))
    js, binary = merge(a, abin, b, bbin, names)
    write(args.out or args.shipped, js, binary)


if __name__ == "__main__":
    main()
