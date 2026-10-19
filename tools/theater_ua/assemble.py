"""Assembles the Ukraine theater from geo.py and the models built by models.py.

    python3 tools/theater_ua/assemble.py <new.glb>

Writes:
- assets/models/ua.glb: Central Europe's models (as ua_000..ua_093, the theater shares their
  meaning: airbases, SAM sites, depots, ...) followed by the new ones.
- assets/theaters/ua.ron: tile grid and tile objects, the world (WLD) and the extras the remake
  reads (front line, towns, model attributes).
- assets/mission/theaters/ua.json: the same for the mission generator, which runs Central
  Europe's START rules (mission table rows, grid references) on it.
- assets/palettes/tod_ua.ron: Central Europe's palettes with steppe ground and dark soil.
"""

import json
import math
import os
import random
import re
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(ROOT, "tools", "models"))

import geo  # noqa: E402
from merge_glb import read, write  # noqa: E402

UNIT_WU = {1: 1, 2: 4, 3: 16, 4: 64}
DIM = {1: 256, 2: 64, 3: 16}

# Central Europe model numbers used here.
BASE, DEPOT, HQ, TANKFARM, SA_MISSILE, SAM_RADAR, TOWER, HANGAR = 21, 28, 29, 30, 33, 38, 39, 40
BRIDGE, BOAT, BUILDINGS, WINDMILL, OTH, MOBILE_SAM, TANK, AIRPLANE, POW, SS = 49, 53, 62, 65, 68, 76, 77, 84, 85, 86
HOUSES, HOUSES2 = 26, 27

TEMPLATES = {
    # Central Europe's own LOD1 layouts (offsets in world units from the cell centre).
    "base_support": [(1024, -768, HANGAR), (-768, 256, AIRPLANE), (1024, 0, HANGAR), (256, -1024, TOWER), (1024, 384, HANGAR),
                     (1024, 768, HANGAR), (-1280, 1280, TANKFARM), (0, 1792, AIRPLANE), (0, -1792, AIRPLANE), (512, 0, AIRPLANE)],
    "base_support_blue": [(1024, -768, HANGAR), (1024, 0, HANGAR), (256, -1024, TOWER), (1024, 384, HANGAR), (1024, 768, HANGAR),
                          (-1280, 1280, TANKFARM)],
    "sam": [(0, 0, SAM_RADAR), (1024, 0, SA_MISSILE), (512, 768, SA_MISSILE), (512, -768, SA_MISSILE), (-1024, 0, SA_MISSILE),
            (-512, 768, SA_MISSILE), (-512, -768, SA_MISSILE)],
    "msam": [(1024, 0, MOBILE_SAM), (512, 768, MOBILE_SAM), (-512, 768, MOBILE_SAM), (-1024, 0, MOBILE_SAM), (-512, -768, MOBILE_SAM),
             (512, -768, MOBILE_SAM), (0, 0, SAM_RADAR)],
    "hq": [(0, 0, HQ), (-1024, -1024, HOUSES), (1536, 0, HOUSES), (-1536, 1024, HOUSES), (-768, 0, DEPOT), (0, 768, MOBILE_SAM),
           (0, -768, MOBILE_SAM)],
    "depot": [(0, 0, DEPOT), (-256, 256, DEPOT), (-128, -384, HOUSES), (256, 256, TANKFARM), (512, -512, TANKFARM), (-1024, 0, MOBILE_SAM)],
    "tankfarm": [(0, 0, TANKFARM), (0, 512, TANKFARM), (512, 0, TANKFARM), (0, -512, HOUSES), (512, -512, TANKFARM),
                 (-1536, -1024, HOUSES), (-512, 1536, HOUSES)],
    "oth": [(0, 0, OTH), (0, -512, DEPOT), (512, -512, HOUSES)],
    "armor": [(0, 0, MOBILE_SAM), (-1536, 512, TANK), (-1792, 0, TANK), (-1024, 1024, TANK), (-1024, 0, TANK), (-1536, -512, TANK),
              (-1024, -1024, TANK), (1536, 512, HOUSES)],
    "ss": [(0, 0, SS)],
    "pow": [(0, 0, POW), (-640, 0, HOUSES)],
    "farm_a": [(256, 1280, HOUSES), (-256, 512, HOUSES), (128, -256, HOUSES), (0, 0, WINDMILL)],
    "farm_b": [(-1280, 1536, HOUSES), (1280, 768, HOUSES), (256, -256, HOUSES2)],
    "farm_c": [(0, 0, HOUSES2), (-512, -768, HOUSES)],
}

# Unit (radar) types of the SAM sites, as Central Europe uses them.
SAM_TYPES = [4, 5, 6, 7]
DONBAS = {"Donetsk", "Makiivka", "Horlivka", "Alchevsk", "Luhansk", "Toretsk", "Pokrovsk", "Myrnohrad", "Shakhty", "Debaltseve",
          "Avdiivka", "Novocherkassk", "Popasna", "Bakhmut"}
CITY_SAMS = ["Sevastopol", "Simferopol", "Donetsk", "Luhansk", "Rostov", "Belgorod", "Melitopol", "Novorossiysk"]


def wu(x_m):
    return x_m / geo.WU


class Layout:
    def __init__(self, meta):
        self.meta = meta
        self.water = geo.Water()
        self.sides = geo.Sides()
        self.front = geo.front_m()
        self.objs = {1: {}, 2: {}, 3: {}}
        self.rng = random.Random(2026)

    # --- placement -----------------------------------------------------------------------

    def cell(self, lod, x_m, y_m):
        size = 4096 * UNIT_WU[lod]
        return int(wu(x_m) // size), int(wu(y_m) // size)

    def put(self, lod, x_m, y_m, shape):
        """A tile object at a point; returns its (lod, col, row, ox, oy)."""
        u = UNIT_WU[lod]
        c, r = self.cell(lod, x_m, y_m)
        ox = int(round((wu(x_m) - (c * 4096 + 2048) * u) / u))
        oy = int(round((wu(y_m) - (r * 4096 + 2048) * u) / u))
        self.objs[lod].setdefault((c, r), []).append((ox, oy, shape))
        return (lod, c, r, ox, oy)

    def put_at_cell(self, lod, c, r, ox, oy, shape):
        self.objs[lod].setdefault((c, r), []).append((ox, oy, shape))
        return (lod, c, r, ox, oy)

    @staticmethod
    def map_pos(t):
        lod, c, r, ox, oy = t
        u = UNIT_WU[lod]
        east = (c * 4096 + 2048 + ox) * u
        north = (r * 4096 + 2048 + oy) * u
        return east >> 5, 0x8000 - (north >> 5)

    @staticmethod
    def centre_m(lod, c, r):
        u = UNIT_WU[lod]
        return (c * 4096 + 2048) * u * geo.WU, (r * 4096 + 2048) * u * geo.WU

    def template(self, name, c1, r1):
        """A LOD1 template in cell (c1, r1); returns the placements by shape."""
        out = []
        for ox, oy, shape in TEMPLATES[name]:
            out.append((shape, self.put_at_cell(1, c1, r1, ox, oy, shape)))
        return out

    def land(self, x, y):
        return self.water.at(x, y) is None

    def land_cell(self, lod, c, r, n=5, margin=0.0):
        size = 4096 * UNIT_WU[lod] * geo.WU
        for i in range(n):
            for j in range(n):
                x = (c + (i + 0.5) / n) * size
                y = (r + (j + 0.5) / n) * size
                if not self.land(x, y):
                    return False
        return True

    def red(self, x, y):
        return not self.sides.blue_at(x, y)

    # --- content -------------------------------------------------------------------------

    def build(self):
        self.ground_units = []  # (name, placement, unit_type, flags)
        self.red_bases = []
        self.blue_bases = []
        self.cities = []  # (name, placement or point, red, shape)
        self.airbases()
        self.npps()
        self.towns()
        self.sites()
        self.city_sams()
        self.boats()
        self.mountains()
        self.generic()

    def airbases(self):
        for name, lon, lat, friendly in geo.AIRBASES:
            x, y = geo.ll(lon, lat)
            c1, r1 = self.cell(1, x, y)
            c2, r2 = c1 >> 2, r1 >> 2
            # Runway centred on the LOD1 cell, the model on the LOD2 level.
            cx, cy = (c1 * 4096 + 2048), (r1 * 4096 + 2048)
            ox2 = (cx - (c2 * 4096 + 2048) * 4) // 4
            oy2 = (cy - (r2 * 4096 + 2048) * 4) // 4
            t = self.put_at_cell(2, c2, r2, ox2, oy2, BASE)
            self.template("base_support_blue" if friendly else "base_support", c1, r1)
            if friendly:
                self.blue_bases.append((name, t))
            else:
                self.red_bases.append((name, t))
                # SAM site two cells to the side, inside the same LOD2 cell.
                sc = c1 - 2 if c1 & 3 >= 2 else c1 + 2
                radar = self.template("sam", sc, r1)[0][1]
                self.ground_units.append((name, radar, self.rng.choice(SAM_TYPES), 0))

    def npps(self):
        for name, lon, lat, kind in geo.NPPS:
            x, y = geo.ll(lon, lat)
            shape = {"znpp": 94, "rostov": 95, "south": 96, "kursk": 97, "novovoronezh": 98}[kind]
            self.put(2, x, y, shape)

    def towns(self):
        """Tile objects of the towns: central buildings (or ruins), slag heaps in the Donbas.
        Only the named towns and the ruined ones, within the central LOD1 cell (the scenery
        builds the towns themselves)."""
        self.town_sites = []
        for name, lon, lat, kind, r, ruin, wld in geo.TOWNS:
            x, y = geo.ll(lon, lat)
            self.town_sites.append((name, x, y, r, kind, ruin))
            red = self.red(x, y)
            if not (wld or ruin >= 0.5) or not self.land(x, y):
                continue
            rng = random.Random(name)
            c1, r1 = self.cell(1, x, y)
            cx, cy = self.centre_m(1, c1, r1)
            ruined = lambda: rng.random() < ruin
            central = rng.choice([99, 100, 101]) if ruin >= 0.5 else BUILDINGS
            t = self.put(1, x, y, central)
            self.cities.append((name, t, red, central))
            extra = {2: 3, 1: 2, 0: 1}[kind]
            for k in range(extra):
                a = rng.uniform(0, math.tau)
                d = rng.uniform(250, 700)
                px, py = x + math.cos(a) * d, y + math.sin(a) * d
                if self.cell(1, px, py) != (c1, r1) or not self.land(px, py):
                    continue
                shape = rng.choice([99, 100, 101]) if ruined() else rng.choice([BUILDINGS, HOUSES, HOUSES2])
                self.put(1, px, py, shape)
            if name in DONBAS:
                # Slag heaps at the edge of the mining towns.
                for k in range(3):
                    a = rng.uniform(0, math.tau)
                    d = r + rng.uniform(250, 1200)
                    px, py = x + math.cos(a) * d, y + math.sin(a) * d
                    if self.land(px, py):
                        self.put(1, px, py, 103)

    def site_cell(self, x, y):
        """LOD1 cell for a site: the one under it, or the nearest one whose centre is on land."""
        c1, r1 = self.cell(1, x, y)
        best = None
        for dc in range(-3, 4):
            for dr in range(-3, 4):
                cx, cy = self.centre_m(1, c1 + dc, r1 + dr)
                if self.land_cell(1, c1 + dc, r1 + dr, 3) and (c1 + dc, r1 + dr) not in self.objs[1]:
                    d = dc * dc + dr * dr
                    if best is None or d < best[0]:
                        best = (d, c1 + dc, r1 + dr)
        return best[1:] if best else None

    def sites(self):
        for kind, lon, lat, name in geo.SITES:
            x, y = geo.ll(lon, lat)
            if kind == "bridge":
                bx, by = self.on_river(x, y)
                self.put(1, bx, by, BRIDGE)
                continue
            cell = self.site_cell(x, y)
            if cell is None:
                print("no room for", kind, name)
                continue
            placed = self.template(kind, *cell)
            for shape, t in placed:
                if shape == SAM_RADAR:
                    self.ground_units.append((name, t, self.rng.choice([5, 7]), 0))
                if shape == OTH:
                    self.ground_units.append((name, t, 21, 0))

    def city_sams(self):
        for name in CITY_SAMS:
            t = next(t for t in geo.TOWNS if t[0] == name)
            x, y = geo.ll(t[1], t[2])
            for a in range(8):
                ang = a * math.tau / 8 + 0.4
                px, py = x + math.cos(ang) * (t[4] + 4000), y + math.sin(ang) * (t[4] + 4000)
                cell = self.site_cell(px, py)
                if cell:
                    radar = self.template("sam", *cell)[0][1]
                    self.ground_units.append((name, radar, self.rng.choice(SAM_TYPES), 0))
                    break

    def on_river(self, x, y):
        best = None
        for r in geo.RIVERS:
            pts = [geo.ll(lon, lat) for lon, lat, _ in r]
            for a, b in zip(pts, pts[1:]):
                dx, dy = b[0] - a[0], b[1] - a[1]
                t = max(0.0, min(1.0, ((x - a[0]) * dx + (y - a[1]) * dy) / (dx * dx + dy * dy)))
                p = (a[0] + dx * t, a[1] + dy * t)
                d = math.hypot(p[0] - x, p[1] - y)
                if best is None or d < best[0]:
                    best = (d, p)
        return best[1]

    def boats(self):
        self.boat_units = []
        for k, (lon, lat) in enumerate(geo.BOATS):
            x, y = geo.ll(lon, lat)
            if self.water.at(x, y) != "sea":
                print("boat on land at", lon, lat)
                continue
            t = self.put(1, x, y, BOAT)
            self.boat_units.append((t, [11, 9][k % 2]))

    def mountains(self):
        for shape, lon, lat in geo.MOUNTAINS:
            x, y = geo.ll(lon, lat)
            self.put(3, x, y, shape)

    def forest_density(self, x, y):
        d = 0.0
        for lon, lat, rad, dens in geo.FORESTS:
            fx, fy = geo.ll(lon, lat)
            k = 1.0 - math.hypot(x - fx, y - fy) / (rad * 1000.0)
            d = max(d, dens * k)
        return d

    def generic(self):
        """Forest patches and farmsteads in the LOD2 cells without anything else, from a few
        shared patterns (each distinct cell content costs one of the 256 tile types)."""
        towns = [(x, y, r) for _, x, y, r, _, _ in self.town_sites]
        rng = random.Random(7)
        busy2 = set(self.objs[2])
        busy2 |= {(c >> 2, r >> 2) for c, r in self.objs[1]}
        patterns = [
            {},
            {(1, 2): "farm_a"},
            {(2, 1): "farm_b", (0, 3): "farm_c"},
            {(3, 0): "farm_c"},
            {(1, 1): "farm_b", (3, 2): "farm_a"},
        ]
        self.pattern_cells = {}
        for r2 in range(64):
            for c2 in range(64):
                if (c2, r2) in busy2:
                    continue
                if not self.land_cell(2, c2, r2, 5):
                    continue
                cx, cy = self.centre_m(2, c2, r2)
                if any(math.hypot(cx - x, cy - y) < r + 4500 for x, y, r in towns):
                    continue
                d = self.forest_density(cx, cy) + rng.uniform(-0.12, 0.12)
                forest = None
                if d > 0.55:
                    forest = 107
                elif d > 0.4:
                    forest = 106
                elif d > 0.25:
                    forest = 105
                elif d > 0.12 or rng.random() < 0.03:
                    forest = 104
                if forest is not None:
                    self.objs[2].setdefault((c2, r2), []).append((0, 0, forest))
                p = patterns[rng.randrange(len(patterns))] if forest is None else patterns[0]
                for (sc, sr), name in p.items():
                    for ox, oy, shape in TEMPLATES[name]:
                        self.put_at_cell(1, c2 * 4 + sc, r2 * 4 + sr, ox, oy, shape)

    # --- grids ---------------------------------------------------------------------------

    def grids(self):
        """Tile types per level, deduplicated (types are bytes)."""
        lod1_types = {(): 0}
        lod1_list = [[]]

        def t1(c, r):
            objs = tuple(self.objs[1].get((c, r), []))
            if objs not in lod1_types:
                lod1_types[objs] = len(lod1_list)
                lod1_list.append(list(objs))
            return lod1_types[objs]

        lod2_types = {((), (0,) * 16): 0}
        lod2_list = [([], [0] * 16)]

        def t2(c, r):
            subs = tuple(t1(c * 4 + (k & 3), r * 4 + (k >> 2)) for k in range(16))
            key = (tuple(self.objs[2].get((c, r), [])), subs)
            if key not in lod2_types:
                lod2_types[key] = len(lod2_list)
                lod2_list.append((list(key[0]), list(subs)))
            return lod2_types[key]

        lod3_list = []
        lod3_types = {}
        lod3 = []
        for r in range(16):
            for c in range(16):
                subs = tuple(t2(c * 4 + (k & 3), r * 4 + (k >> 2)) for k in range(16))
                key = (tuple(self.objs[3].get((c, r), [])), subs)
                if key not in lod3_types:
                    lod3_types[key] = len(lod3_list)
                    lod3_list.append((list(key[0]), list(subs)))
                lod3.append(lod3_types[key])
        lod4_types = {}
        lod4_list = []
        lod4 = []
        for r in range(-2, 6):
            for c in range(-2, 6):
                shape = self.meta["lod4"]["%d,%d" % (c, r)]
                if shape not in lod4_types:
                    lod4_types[shape] = len(lod4_list)
                    lod4_list.append([(0, 0, shape)])
                lod4.append(lod4_types[shape])
        print("tile types: lod1 %d, lod2 %d, lod3 %d, lod4 %d" % (len(lod1_list), len(lod2_list), len(lod3_list), len(lod4_list)))
        assert max(len(lod1_list), len(lod2_list), len(lod3_list), len(lod4_list)) <= 256, "too many tile types"
        g2 = [t for _, subs in lod3_list for t in subs]
        g1 = [t for _, subs in lod2_list for t in subs]
        tiles = [[[]], lod1_list, [o for o, _ in lod2_list], [o for o, _ in lod3_list], lod4_list]
        inner4 = [lod4[(c + 2) + (r + 2) * 8] for r in range(4) for c in range(4)]
        return {"lod4": lod4, "lod3": lod3, "lod2": g2, "lod1": g1, "inner4": inner4, "tiles": tiles}

    def territory(self):
        """16x16 grid, row 0 north: side (3 friendly, 0 enemy), terrain class (8 land, 4 water)
        and 0x10 for enemy forces massed near the front."""
        grid = [0] * 256
        size = 32_000.0
        for row in range(16):
            for col in range(16):
                land = blue = 0
                n = 8
                for i in range(n):
                    for j in range(n):
                        x, y = (col + (i + 0.5) / n) * size, (row + (j + 0.5) / n) * size
                        if self.land(x, y):
                            land += 1
                            blue += self.sides.blue_at(x, y)
                friendly = blue * 2 > land if land else self.sides.blue_at((col + 0.5) * size, (row + 0.5) * size)
                v = (3 if friendly else 0) | (8 if land * 3 >= n * n else 4)
                cx, cy = (col + 0.5) * size, (row + 0.5) * size
                if not friendly and land and geo.dist_to_front(cx, cy, self.front) < 40_000:
                    v |= 0x10
                grid[(15 - row) * 16 + col] = v
        return grid


# ---------------------------------------------------------------------------------------------
# World (WLD)


def ce_data():
    return json.load(open(os.path.join(ROOT, "assets", "mission", "theaters", "ce.json")))


def build_world(lay, ce):
    w = ce["world"]
    fixed = {i: n for i, n in enumerate(w["names"]) if w["object_types"][i] or w["buf7"][i] or i in (92, 93)}
    names = [""] * 128
    for i, n in fixed.items():
        names[i] = n
    free = [i for i in range(1, 128) if i not in fixed]
    index = {}

    def name_id(n):
        if n not in index:
            index[n] = free.pop(0)
            names[index[n]] = n
        return index[n]

    objects = [dict(name=0, x=0, y=0, unit_type=0, flags=0, occupant_type=0, patrol_count=0, object_idx=0) for _ in range(3)]

    def obj(name, t, unit_type, flags, shape):
        x, y = Layout.map_pos(t)
        return dict(name=name_id(name) if name else 0, x=x, y=y, unit_type=unit_type, flags=flags, occupant_type=0, patrol_count=0,
                    object_idx=shape)

    for name, t, ut, fl in lay.ground_units:
        objects.append(obj(name, t, ut, fl, OTH if ut == 21 else SAM_RADAR))
    for t, ut in lay.boat_units:
        objects.append(obj("", t, ut, 8, BOAT))
    guc = len(objects)
    for name, t in lay.red_bases:
        objects.append(obj(name, t, 0, 1, BASE))
    # Towns by size while the name table has room (750 bytes); red ones before the world
    # object count, friendly ones after it.
    cities = sorted(lay.cities, key=lambda c: -next(t[4] for t in geo.TOWNS if t[0] == c[0]))
    size = lambda: 1 + sum(len(n) + 1 for n in names[1:])
    kept = []
    for c in cities:
        if c[0] in index or size() + len(c[0]) <= 750:
            name_id(c[0])
            kept.append(c)
    for name, t, red, shape in kept:
        if red:
            objects.append(obj(name, t, 0, 0, shape))
    woc = len(objects)
    for name, t, red, shape in kept:
        if not red:
            objects.append(obj(name, t, 0, 0x400, shape))
    for name, t in lay.blue_bases:
        objects.append(obj(name, t, 0, 0x401, BASE))
    assert len(objects) <= 128, len(objects)
    raw = b"\0" + b"".join(n.encode("ascii") + b"\0" for n in names[1:])
    raw = raw.rstrip(b"\0") + b"\0"
    assert len(raw) <= 750, len(raw)
    print("world: %d objects (%d ground units, world object count %d), names %d bytes" % (len(objects), guc, woc, len(raw)))
    return dict(header=(91 << 8) | 102, ground_unit_count=guc, world_object_count=woc, objects=objects, flights=w["flights"],
                buf7=w["buf7"], buf8=w["buf8"], object_types=w["object_types"], names_raw=list(raw), names=names)


def flight_bytes(f):
    return list(struct.pack("<hhhhiihhhhhhhhhh", f["waypoint"], f["x"], f["y"], f["alt"], f["xp"], f["yp"], f["heading"], f["pitch"],
                            f["roll"], f["plane_type"], f["flags"], f["speed"], f["fuel"], *f["reserved"]))


# ---------------------------------------------------------------------------------------------
# Models


def merge_models(new_glb, meta):
    a, abin = read(os.path.join(ROOT, "assets", "models", "ce.glb"))
    b, bbin = read(new_glb)
    attrs = {}
    for n in a["nodes"]:
        n["name"] = re.sub(r"^ce_", "ua_", n["name"])
    for m in a["meshes"]:
        m["name"] = re.sub(r"^ce_", "ua_", m.get("name", ""))
    child = {c for n in a["nodes"] for c in n.get("children", [])}
    for i, n in enumerate(a["nodes"]):
        if i not in child:
            attrs[int(n["name"][3:6])] = n.get("extras", {}).get("attr", 0)
    info = {m["name"]: m for m in meta["models"]}
    lods = {l["name"]: l for m in meta["models"] for l in m["lods"]}
    for n in b["nodes"]:
        if n["name"] in info:
            m = info[n["name"]]
            n["extras"] = {"attr": m["attr"], "lod_dists": [l["max_dist"] for l in m["lods"]]}
            attrs[m["index"]] = m["attr"]
        elif n["name"] in lods:
            l = lods[n["name"]]
            n["extras"] = {"max_dist": l["max_dist"], "planes": [[0, 16384, 0, 0]], "radius": l["radius"]}
        else:
            n.pop("extras", None)
    out, blob = append(a, abin, b, bbin)
    write(os.path.join(ROOT, "assets", "models", "ua.glb"), out, blob)
    count = max(attrs) + 1
    return count, [attrs.get(i, 0) for i in range(count)]


def append(a, abin, b, bbin):
    """All of A's nodes, then all of B's (root order kept), with one buffer."""
    out = {k: v for k, v in a.items() if k not in ("nodes", "meshes", "accessors", "bufferViews", "buffers", "materials", "scenes")}
    nodes, meshes, accessors, views, materials = [], [], [], [], []
    blob = bytearray()
    mat_index = {}
    for src, sbin in ((a, abin), (b, bbin)):
        vmap, amap, mmap = {}, {}, {}
        for i, v in enumerate(src["bufferViews"]):
            v = dict(v)
            o = v.get("byteOffset", 0)
            data = sbin[o:o + v["byteLength"]]
            blob.extend(b"\0" * (-len(blob) % 4))
            v["byteOffset"] = len(blob)
            v["buffer"] = 0
            blob.extend(data)
            vmap[i] = len(views)
            views.append(v)
        for i, acc in enumerate(src["accessors"]):
            acc = dict(acc)
            if "bufferView" in acc:
                acc["bufferView"] = vmap[acc["bufferView"]]
            amap[i] = len(accessors)
            accessors.append(acc)
        for i, m in enumerate(src.get("materials", [])):
            if m["name"] not in mat_index:
                mat_index[m["name"]] = len(materials)
                materials.append(m)
        for i, m in enumerate(src["meshes"]):
            m = dict(m)
            prims = []
            for p in m["primitives"]:
                p = dict(p)
                p["attributes"] = {k: amap[v] for k, v in p["attributes"].items()}
                if "indices" in p:
                    p["indices"] = amap[p["indices"]]
                if "material" in p:
                    p["material"] = mat_index[src["materials"][p["material"]]["name"]]
                prims.append(p)
            m["primitives"] = prims
            mmap[i] = len(meshes)
            meshes.append(m)
        base = len(nodes)
        for n in src["nodes"]:
            n = dict(n)
            if "children" in n:
                n["children"] = [c + base for c in n["children"]]
            if "mesh" in n:
                n["mesh"] = mmap[n["mesh"]]
            nodes.append(n)
    child = {c for n in nodes for c in n.get("children", [])}
    out["scene"] = 0
    out["scenes"] = [{"name": "Scene", "nodes": [i for i in range(len(nodes)) if i not in child]}]
    out["nodes"], out["meshes"], out["accessors"], out["bufferViews"], out["materials"] = nodes, meshes, accessors, views, materials
    out["buffers"] = [{"byteLength": len(blob) + (-len(blob) % 4)}]
    if "KHR_materials_unlit" not in out.get("extensionsUsed", []):
        out.setdefault("extensionsUsed", []).append("KHR_materials_unlit")
    return out, bytes(blob)


# ---------------------------------------------------------------------------------------------
# Palette: Central Europe's, with a drier, yellower ground gradient and dark soil browns.


def palette():
    src = open(os.path.join(ROOT, "assets", "palettes", "tod_ce.ron")).read()
    colour = re.compile(r"\((\d+), (\d+), (\d+)\)")
    starts = [src.index(k) for k in ("day:", "dusk:", "night:")] + [len(src)]
    blocks = [src[starts[k]:starts[k + 1]] for k in range(3)]
    day = [tuple(int(v) for v in c) for c in colour.findall(blocks[0])]
    soil = [(31, 22, 13), (26, 18, 10), (22, 15, 8), (18, 12, 6)]  # 0xCC..0xCF by day (6-bit)

    def fix(block):
        cols = [tuple(int(v) for v in c) for c in colour.findall(block)]
        out = list(cols)
        for i in range(0x70, 0x80):
            r, g, b = cols[i]
            # Steppe: the green of the ground ramp turns a little olive (its greys stay).
            out[i] = (min(63, round(r + max(0, g - r) * 0.25)), g, round(b * 0.85))
        for k, i in enumerate(range(0xCC, 0xD0)):
            # Each palette keeps its brightness relative to the day palette.
            scale = sum(cols[i]) / max(1, sum(day[i]))
            out[i] = tuple(min(63, round(v * scale)) for v in soil[k])
        it = iter(out)
        return colour.sub(lambda m: "(%d, %d, %d)" % next(it), block)

    text = src[:starts[0]] + "".join(fix(b) for b in blocks)
    open(os.path.join(ROOT, "assets", "palettes", "tod_ua.ron"), "w").write(text)


# ---------------------------------------------------------------------------------------------
# Output


def ron_list(v):
    return "[" + ", ".join(str(x) for x in v) + "]"


def ron_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def write_ron(lay, grids, world, model_count, model_attr):
    tiles = grids["tiles"]
    tl = lambda objs: "[" + ", ".join("(x: %d, y: %d, z: 0, shape: %d)" % o for o in objs) + "]"
    level = lambda lv: "[" + ", ".join(tl(t) for t in lv) + "]"
    objs = ",\n            ".join(
        "(name: %d, x: %d, y: %d, unit_type: %d, target_flags: %d, occupant_type: %d, patrol_count: %d, object_idx: %d)"
        % (o["name"], o["x"], o["y"], o["unit_type"], o["flags"], o["occupant_type"], o["patrol_count"], o["object_idx"])
        for o in world["objects"])
    front = ", ".join("(%.1f, %.1f, %.2f)" % p for p in geo.front_m())
    towns = ",\n        ".join(
        "(name: %s, x: %.1f, y: %.1f, r: %.1f, kind: %d, ruined: %.2f)" % (ron_str(n), x, y, r, k, ru)
        for n, x, y, r, k, ru in lay.town_sites)
    text = f"""(
    id: "ua",
    index: 9,
    grid: (
        lod4: {ron_list(grids['lod4'])},
        lod3: {ron_list(grids['lod3'])},
        lod2: {ron_list(grids['lod2'])},
        lod1: {ron_list(grids['lod1'])},
    ),
    tiles: (
        tiles: [{", ".join(level(lv) for lv in tiles)}],
    ),
    world: (
        header: {world['header']},
        ground_unit_count: {world['ground_unit_count']},
        world_object_count: {world['world_object_count']},
        objects: [
            {objs}
        ],
        flights: [{", ".join(ron_list(flight_bytes(f)) for f in world['flights'])}],
        table7: {ron_list(world['buf7'])},
        table8: {ron_list(world['buf8'])},
        object_types: {ron_list([v & 0xFF for v in world['object_types']])},
        terrain: {ron_list(world['grid'])},
        names: [{", ".join(ron_str(n) for n in world['names'])}],
    ),
    model_count: {model_count},
    model_attr: {ron_list(model_attr)},
    front: [{front}],
    towns: [
        {towns}
    ],
)
"""
    open(os.path.join(ROOT, "assets", "theaters", "ua.ron"), "w").write(text)


def write_json(grids, world):
    td = {
        "index": 3,
        "date": [10, 1, 2026],
        "world": world,
        "g": [[0] * 512, grids["lod1"], grids["lod2"], grids["lod3"], grids["inner4"]],
        "tiles": [[[list(o) for o in t] for t in lv] for lv in grids["tiles"]],
        "grid_size": [1024, 256, 64, 16, 4],
    }
    with open(os.path.join(ROOT, "assets", "mission", "theaters", "ua.json"), "w") as f:
        json.dump(td, f, separators=(",", ":"))


def main():
    new_glb = sys.argv[1]
    meta = json.load(open(new_glb + ".json"))
    lay = Layout(meta)
    lay.build()
    grids = lay.grids()
    ce = ce_data()
    world = build_world(lay, ce)
    world["grid"] = lay.territory()
    model_count, model_attr = merge_models(new_glb, meta)
    write_ron(lay, grids, world, model_count, model_attr)
    write_json(grids, world)
    palette()
    print("ua: %d models" % model_count)


main()
