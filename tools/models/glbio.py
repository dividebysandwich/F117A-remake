"""Reading and writing binary glTF (.glb) files: (JSON dict, binary chunk)."""

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
