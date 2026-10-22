"""Geography of the Ukraine theater: projection, coasts, rivers, the front line and the places.

The theater squeezes 31.0-42.6 E, 44.1-52.0 N (about 860 km square) into the 512 km of a
theater, like the other compressed theaters. Coordinates here are (lon, lat) degrees;
`ll` turns them into theater metres (x east, y north, 0..512000).

Front line: roughly the line of contact in 2026, from the Sumy and Kharkiv border areas through
Kupiansk, Lyman, Kostiantynivka, Pokrovsk and Huliaipole to the Dnipro below Zaporizhzhia, then
down the river to Kherson. Ukrainian-held ground is blue (friendly), occupied Ukraine, Crimea and
Russia red (enemy).
"""

import math

THEATER_M = 512_000.0
WU = 512_000.0 / 1_048_576.0  # metres per world unit
LON0, LAT0 = 31.0, 44.1
SQUEEZE = 1.72  # real km per theater km
KX = 111.32 * math.cos(math.radians(48.0)) / SQUEEZE  # theater km per degree
KY = 111.2 / SQUEEZE


def ll(lon, lat):
    """(lon, lat) -> theater metres (x east, y north)."""
    return ((lon - LON0) * KX * 1000.0, (lat - LAT0) * KY * 1000.0)


def lls(pts):
    return [ll(p[0], p[1]) for p in pts]


# ---------------------------------------------------------------------------------------------
# Water

# The Black Sea and the Sea of Azov as one outline (the Kerch Strait joins them), coarse
# outside the theater (it is seen from the edges).
SEA = [
    (29.05, 41.20), (28.10, 41.62), (27.98, 41.87), (27.70, 42.42), (27.47, 42.50), (27.90, 42.90),
    (27.92, 43.20), (28.58, 43.40), (28.60, 43.80), (28.65, 44.17), (28.80, 44.60), (29.10, 44.80),
    (29.65, 45.05), (29.75, 45.30), (29.70, 45.55), (30.10, 45.85), (30.25, 46.05), (30.55, 46.30),
    (30.75, 46.48), (30.80, 46.57), (31.00, 46.60), (31.20, 46.63), (31.45, 46.60), (31.53, 46.62),
    # Southern Buh estuary up to Mykolaiv
    (31.62, 46.68), (31.80, 46.75), (31.92, 46.85), (31.96, 46.95), (32.03, 46.94), (32.02, 46.82),
    (31.95, 46.69),
    # Dnipro estuary to Kherson and back along the Kinburn shore
    (32.05, 46.625), (32.25, 46.605), (32.45, 46.605), (32.56, 46.625), (32.58, 46.565), (32.30, 46.515),
    (32.00, 46.505), (31.75, 46.525), (31.55, 46.555), (31.68, 46.49), (31.80, 46.42),
    # Tendra and Dzharylhach bays, Karkinit Bay to Perekop
    (31.95, 46.28), (32.15, 46.16), (32.55, 46.08), (32.90, 46.11), (33.20, 46.14), (33.50, 46.13),
    (33.66, 46.12),
    # Crimea: west coast, Tarkhankut, Yevpatoria, Sevastopol, the south coast
    (33.58, 46.02), (33.40, 45.95), (33.05, 45.90), (32.80, 45.78), (32.70, 45.51), (32.48, 45.35),
    (32.75, 45.33), (33.10, 45.26), (33.37, 45.19), (33.55, 45.10), (33.58, 44.95), (33.53, 44.78),
    (33.48, 44.62), (33.37, 44.58), (33.55, 44.48), (33.73, 44.39), (34.10, 44.43), (34.17, 44.49),
    (34.35, 44.56), (34.41, 44.67), (34.70, 44.80), (34.97, 44.84), (35.20, 44.92), (35.40, 45.02),
    (35.65, 45.08), (35.84, 45.00), (36.20, 45.02), (36.40, 45.10), (36.45, 45.22), (36.47, 45.35),
    (36.58, 45.40), (36.62, 45.47),
    # Azov: Kerch peninsula, Arabat Spit, the north coast, Taganrog Bay
    (36.30, 45.47), (35.95, 45.45), (35.85, 45.48), (35.55, 45.40), (35.38, 45.32), (35.25, 45.50),
    (35.10, 45.70), (34.95, 45.92), (34.84, 46.12), (34.95, 46.25), (35.20, 46.38), (35.40, 46.40),
    (35.90, 46.60), (36.35, 46.73), (36.60, 46.75), (36.75, 46.62), (36.80, 46.74), (37.10, 46.90),
    (37.35, 47.03), (37.55, 47.08), (37.80, 47.08), (38.08, 47.10), (38.40, 47.15), (38.75, 47.18),
    (38.92, 47.20), (39.10, 47.22), (39.25, 47.25), (39.15, 47.08), (38.90, 46.98), (38.55, 46.82),
    (38.30, 46.72), (38.05, 46.68), (37.95, 46.60), (38.10, 46.45), (38.17, 46.05), (38.05, 45.85),
    (37.75, 45.65), (37.60, 45.45), (37.35, 45.32), (37.05, 45.38), (36.85, 45.42), (36.68, 45.37),
    # Kerch Strait east shore, Taman, the Caucasus coast, Turkey
    (36.72, 45.25), (36.80, 45.10), (37.05, 45.00), (37.32, 44.88), (37.45, 44.75), (37.77, 44.70),
    (38.06, 44.55), (38.50, 44.32), (39.07, 44.08), (39.73, 43.58), (40.27, 43.32), (41.00, 43.00),
    (41.67, 42.15), (41.64, 41.62), (40.52, 41.03), (39.72, 41.00), (38.39, 40.92), (37.88, 41.00),
    (36.70, 41.35), (36.33, 41.29), (35.95, 41.73), (35.15, 42.03), (34.20, 41.98), (33.76, 41.98),
    (32.39, 41.75), (31.79, 41.46), (31.42, 41.28), (30.60, 41.15), (29.60, 41.18),
]

# Lagoons and lakes drawn like the sea.
SIVASH = [
    (33.80, 46.10), (34.10, 46.18), (34.40, 46.15), (34.55, 46.05), (34.75, 46.12), (34.85, 46.10),
    (35.05, 45.75), (35.30, 45.36), (35.10, 45.33), (34.80, 45.45), (34.45, 45.70), (34.10, 45.90),
    (33.90, 46.00),
]
MOLOCHNYI = [(35.25, 46.45), (35.35, 46.47), (35.42, 46.62), (35.33, 46.70), (35.22, 46.56)]
CASPIAN = [(46.9, 44.8), (47.4, 45.6), (48.0, 46.0), (49.0, 46.4), (50.5, 46.8), (51.5, 45.0), (50.0, 42.0),
           (48.0, 42.5), (47.4, 43.6)]
SEAS = [SEA, SIVASH, CASPIAN]
LAKES = [MOLOCHNYI]

# Rivers and reservoirs: polylines of (lon, lat, width in theater km). The wide ones become
# water polygons, the others thin ones (still wide enough to read from the air).
RIVERS = [
    # Dnipro: the lower river, the drained Kakhovka reservoir bed, Zaporizhzhia to Dnipro, the
    # Kamianske and Kremenchuk reservoirs.
    [(32.50, 46.60, 0.9), (32.70, 46.66, 0.6), (32.85, 46.70, 0.5), (33.10, 46.74, 0.45), (33.37, 46.77, 0.4),
     (33.75, 46.97, 0.35), (34.12, 47.30, 0.35), (34.45, 47.50, 0.4), (34.80, 47.56, 0.4), (35.10, 47.65, 0.4),
     (35.13, 47.80, 0.5), (35.10, 47.95, 0.6), (35.15, 48.10, 0.7), (35.12, 48.30, 0.8), (35.05, 48.45, 0.9),
     (34.85, 48.50, 1.0), (34.62, 48.53, 1.2), (34.35, 48.62, 2.0), (34.05, 48.75, 2.4), (33.75, 48.92, 2.4),
     (33.47, 49.06, 1.2), (33.20, 49.15, 4.0), (32.90, 49.25, 6.5), (32.60, 49.33, 7.0), (32.30, 49.40, 5.5),
     (32.05, 49.48, 2.0), (31.80, 49.65, 1.4), (31.50, 49.78, 1.6), (31.10, 49.92, 1.5), (30.60, 50.10, 1.5),
     (30.50, 50.50, 1.5)],
    # Siverskyi Donets
    [(36.60, 50.55, 0.12), (36.75, 50.35, 0.12), (36.88, 50.15, 0.6), (36.87, 49.95, 0.6), (36.70, 49.82, 0.14),
     (36.85, 49.60, 0.14), (36.95, 49.45, 0.14), (37.25, 49.22, 0.16), (37.55, 49.05, 0.16), (37.80, 48.95, 0.16),
     (38.10, 48.88, 0.16), (38.40, 48.93, 0.16), (38.75, 48.75, 0.16), (39.10, 48.65, 0.16), (39.50, 48.55, 0.16),
     (39.80, 48.45, 0.16), (40.25, 48.32, 0.18), (40.60, 48.10, 0.18), (40.85, 47.85, 0.2), (40.90, 47.62, 0.25)],
    # Don, upper course past Voronezh (its reservoir) and Liski
    [(39.25, 52.30, 0.14), (39.22, 51.80, 1.0), (39.22, 51.62, 1.0), (39.28, 51.50, 0.16), (39.30, 51.30, 0.16),
     (39.50, 50.98, 0.18), (39.90, 50.65, 0.18), (40.10, 50.42, 0.2), (40.50, 50.20, 0.2), (40.95, 49.95, 0.2),
     (41.30, 49.80, 0.2), (41.70, 49.60, 0.2), (42.20, 49.50, 0.2), (42.80, 49.30, 0.2), (43.50, 49.10, 0.2)],
    # Don, lower course from the Tsimlyansk reservoir past Rostov to Taganrog Bay
    [(43.40, 48.70, 2.5), (43.00, 48.30, 3.5), (42.70, 48.05, 3.5), (42.45, 47.85, 2.8), (42.25, 47.72, 2.0),
     (42.10, 47.64, 0.3), (41.70, 47.60, 0.3), (41.10, 47.58, 0.3), (40.90, 47.62, 0.3), (40.50, 47.52, 0.3),
     (40.10, 47.37, 0.3), (39.90, 47.25, 0.3), (39.72, 47.21, 0.3), (39.50, 47.15, 0.35), (39.42, 47.11, 0.4),
     (39.25, 47.22, 0.6)],
    # Southern Buh
    [(31.96, 46.96, 0.4), (31.85, 47.20, 0.2), (31.60, 47.40, 0.15), (31.33, 47.57, 0.15), (31.15, 47.80, 0.14),
     (30.90, 48.00, 0.14), (30.50, 48.30, 0.14)],
    # Inhulets through Kryvyi Rih
    [(32.68, 46.66, 0.1), (32.80, 46.95, 0.1), (32.85, 47.20, 0.1), (33.05, 47.50, 0.1), (33.25, 47.75, 0.1),
     (33.39, 47.92, 0.12), (33.30, 48.20, 0.1), (33.10, 48.50, 0.1)],
    # Vorskla through Poltava
    [(34.05, 48.82, 0.1), (34.30, 49.15, 0.1), (34.55, 49.50, 0.1), (34.75, 49.85, 0.1), (35.00, 50.15, 0.1),
     (35.30, 50.45, 0.1), (35.60, 50.70, 0.1)],
    # Psel
    [(33.55, 49.08, 0.1), (33.75, 49.40, 0.1), (34.05, 49.80, 0.1), (34.40, 50.15, 0.1), (34.70, 50.50, 0.1),
     (35.10, 50.85, 0.1), (35.60, 51.20, 0.1)],
    # Samara past Pavlohrad, and the Vovcha
    [(35.12, 48.47, 0.12), (35.40, 48.60, 0.12), (35.85, 48.55, 0.1), (36.30, 48.50, 0.08), (36.80, 48.45, 0.08)],
    [(35.85, 48.55, 0.08), (36.20, 48.30, 0.08), (36.60, 48.10, 0.08), (37.00, 47.95, 0.08)],
    # Oskil with its reservoir, past Kupiansk
    [(37.85, 51.30, 0.08), (37.88, 50.90, 0.08), (38.08, 50.25, 0.1), (37.85, 49.95, 0.1), (37.65, 49.72, 0.12),
     (37.66, 49.50, 0.8), (37.64, 49.30, 1.1), (37.58, 49.14, 0.5), (37.42, 49.10, 0.12)],
    # Aidar past Starobilsk
    [(38.75, 50.10, 0.08), (38.80, 49.70, 0.08), (38.90, 49.30, 0.08), (39.05, 48.90, 0.08), (39.10, 48.68, 0.08)],
    # Kalmius (Donetsk to Mariupol), Mius, Molochna
    [(37.80, 48.10, 0.08), (37.70, 47.80, 0.08), (37.60, 47.40, 0.08), (37.55, 47.09, 0.1)],
    [(38.60, 48.00, 0.08), (38.70, 47.70, 0.08), (38.85, 47.40, 0.1), (38.88, 47.22, 0.2)],
    [(35.33, 46.64, 0.06), (35.40, 46.85, 0.06), (35.60, 47.10, 0.06), (35.75, 47.30, 0.06)],
    # Kuban past Krasnodar (its reservoir) to Temryuk
    [(41.10, 44.95, 0.14), (40.60, 45.40, 0.14), (40.10, 45.20, 0.14), (39.55, 45.08, 1.4), (39.20, 45.05, 1.8),
     (38.98, 45.02, 0.15), (38.20, 45.12, 0.15), (37.80, 45.20, 0.15), (37.40, 45.31, 0.2)],
    # Seym past Kursk
    [(36.40, 51.80, 0.08), (36.20, 51.70, 0.1), (35.70, 51.55, 0.1), (35.30, 51.40, 0.1), (34.80, 51.30, 0.1),
     (34.20, 51.25, 0.1), (33.50, 51.20, 0.1), (32.80, 51.40, 0.1)],
]


def buffer_polyline(pts):
    """Outline of a river polyline (theater metres, width per point in km)."""
    left, right = [], []
    n = len(pts)
    for i in range(n):
        x, y, w = pts[i]
        if i == 0:
            dx, dy = pts[1][0] - x, pts[1][1] - y
        elif i == n - 1:
            dx, dy = x - pts[i - 1][0], y - pts[i - 1][1]
        else:
            ax, ay = x - pts[i - 1][0], y - pts[i - 1][1]
            bx, by = pts[i + 1][0] - x, pts[i + 1][1] - y
            la, lb = math.hypot(ax, ay), math.hypot(bx, by)
            dx, dy = ax / la + bx / lb, ay / la + by / lb
        l = math.hypot(dx, dy)
        nx, ny = -dy / l, dx / l
        h = w * 500.0
        left.append((x + nx * h, y + ny * h))
        right.append((x - nx * h, y - ny * h))
    return left + right[::-1]


def river_polygons():
    out = []
    for r in RIVERS:
        pts = [ll(lon, lat) + (w,) for lon, lat, w in r]
        out.append(buffer_polyline(pts))
    return out


def point_in(poly, x, y):
    inside = False
    n = len(poly)
    j = n - 1
    for i in range(n):
        xi, yi = poly[i]
        xj, yj = poly[j]
        if (yi > y) != (yj > y) and x < (xj - xi) * (y - yi) / (yj - yi) + xi:
            inside = not inside
        j = i
    return inside


class Water:
    """Point classification: 'sea', 'lake' (rivers, reservoirs, lakes) or None (land)."""

    def __init__(self):
        self.seas = [lls(p) for p in SEAS]
        self.lakes = [lls(p) for p in LAKES] + river_polygons()
        self.boxes = [(p, bbox(p)) for p in self.seas + self.lakes]

    def at(self, x, y):
        for p in self.seas:
            if inside_box(bbox_cached(p, self.boxes), x, y) and point_in(p, x, y):
                return "sea"
        for p in self.lakes:
            if inside_box(bbox_cached(p, self.boxes), x, y) and point_in(p, x, y):
                return "lake"
        return None

    def polygons(self):
        return [("sea", p) for p in self.seas] + [("lake", p) for p in self.lakes]


def bbox(p):
    xs = [q[0] for q in p]
    ys = [q[1] for q in p]
    return (min(xs), min(ys), max(xs), max(ys))


def bbox_cached(p, boxes):
    for q, b in boxes:
        if q is p:
            return b
    return bbox(p)


def inside_box(b, x, y):
    return b[0] <= x <= b[2] and b[1] <= y <= b[3]


# ---------------------------------------------------------------------------------------------
# Sides

# Line of contact, north to south: (lon, lat, intensity). The border stretch of Sumy and
# Kharkiv sees drones and shelling, the Dnipro stretch artillery across the river.
FRONT = [
    (34.25, 51.30, 0.25), (34.70, 51.18, 0.25), (35.10, 51.12, 0.3), (35.30, 51.05, 0.35), (35.45, 50.80, 0.3),
    (35.60, 50.47, 0.3), (36.00, 50.38, 0.35), (36.20, 50.38, 0.6),
    (36.35, 50.22, 1.0), (36.65, 50.24, 0.9), (36.95, 50.27, 1.0), (37.20, 50.17, 0.9), (37.45, 50.02, 0.9),
    (37.60, 49.86, 1.0), (37.62, 49.72, 1.0), (37.70, 49.55, 0.9), (37.68, 49.38, 0.9), (37.80, 49.18, 1.0),
    (37.80, 49.00, 1.0), (37.98, 48.90, 1.0), (37.92, 48.74, 1.0), (37.82, 48.60, 1.0), (37.70, 48.52, 1.0),
    (37.55, 48.42, 1.0), (37.35, 48.33, 1.0), (37.15, 48.28, 1.0), (36.95, 48.20, 1.0), (36.75, 48.05, 1.0),
    (36.55, 47.88, 1.0), (36.35, 47.72, 1.0), (36.22, 47.65, 1.0), (35.98, 47.60, 1.0), (35.78, 47.57, 1.0),
    (35.55, 47.52, 1.0), (35.35, 47.53, 0.9), (35.18, 47.62, 0.7),
    (35.10, 47.65, 0.45), (34.80, 47.56, 0.45), (34.45, 47.50, 0.45), (34.12, 47.30, 0.4), (33.75, 46.97, 0.4),
    (33.37, 46.77, 0.45), (33.10, 46.74, 0.45), (32.85, 46.70, 0.5), (32.70, 46.66, 0.5), (32.55, 46.61, 0.4),
]

# Ukrainian-held ground (the rest of the theater is occupied or Russia). West and north are
# beyond the theater edges; the north border runs past Sumy and Kharkiv to the front.
BLUE = (
    [(30.0, 46.40), (30.0, 52.60), (33.60, 52.60), (34.15, 52.10), (34.40, 51.72), (34.25, 51.40)]
    + [(lon, lat) for lon, lat, _ in FRONT]
    + [(32.30, 46.55), (31.80, 46.55), (31.40, 46.59)]
)


class Sides:
    def __init__(self):
        self.blue = lls(BLUE)

    def blue_at(self, x, y):
        return point_in(self.blue, x, y)


def front_m():
    """The front line in theater metres with its intensity: [(x, y, w)]."""
    return [ll(lon, lat) + (w,) for lon, lat, w in FRONT]


def dist_to_front(x, y, front=None):
    front = front or front_m()
    best = 1e18
    for a, b in zip(front, front[1:]):
        ax, ay, bx, by = a[0], a[1], b[0], b[1]
        dx, dy = bx - ax, by - ay
        t = max(0.0, min(1.0, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)))
        best = min(best, math.hypot(x - ax - dx * t, y - ay - dy * t))
    return best


# ---------------------------------------------------------------------------------------------
# Places

# Towns: (name, lon, lat, kind 0 village / 1 town / 2 city, radius m, ruined share, in the WLD
# name table). Ruined: how much of the town lies in ruins (front-line towns, occupied towns
# taken by storm, towns under constant shelling).
TOWNS = [
    # Ukrainian-held
    ("Kharkiv", 36.23, 49.99, 2, 3000, 0.08, True),
    ("Dnipro", 35.05, 48.46, 2, 2800, 0.02, True),
    ("Zaporizhzhia", 35.17, 47.84, 2, 2600, 0.08, True),
    ("Kryvyi Rih", 33.39, 47.91, 2, 2200, 0.03, True),
    ("Poltava", 34.55, 49.59, 2, 1800, 0.01, True),
    ("Sumy", 34.80, 50.91, 2, 1800, 0.08, True),
    ("Mykolaiv", 32.00, 47.02, 2, 2000, 0.05, True),
    ("Kherson", 32.62, 46.67, 2, 1800, 0.35, True),
    ("Kremenchuk", 33.42, 49.10, 2, 1600, 0.02, True),
    ("Kamianske", 34.61, 48.49, 2, 1500, 0.01, False),
    ("Kropyvnytskyi", 32.26, 48.51, 2, 1800, 0.0, False),
    ("Cherkasy", 32.06, 49.44, 2, 1700, 0.0, False),
    ("Pavlohrad", 35.87, 48.53, 1, 1100, 0.04, True),
    ("Kramatorsk", 37.56, 48.72, 2, 1600, 0.3, True),
    ("Sloviansk", 37.61, 48.85, 2, 1400, 0.25, True),
    ("Druzhkivka", 37.53, 48.62, 1, 1000, 0.4, False),
    ("Kostiantynivka", 37.71, 48.53, 1, 1100, 0.8, False),
    ("Izium", 37.27, 49.21, 1, 1000, 0.55, True),
    ("Kupiansk", 37.61, 49.71, 1, 1000, 0.9, True),
    ("Chuhuiv", 36.68, 49.84, 1, 900, 0.2, False),
    ("Balakliia", 36.84, 49.46, 0, 700, 0.3, False),
    ("Nikopol", 34.39, 47.59, 1, 1100, 0.35, True),
    ("Marhanets", 34.63, 47.65, 0, 800, 0.3, False),
    ("Orikhiv", 35.79, 47.57, 0, 800, 0.95, False),
    ("Huliaipole", 36.26, 47.66, 0, 800, 0.95, False),
    ("Myrhorod", 33.61, 49.97, 1, 900, 0.0, False),
    ("Lozova", 36.32, 48.89, 1, 900, 0.03, False),
    ("Samar", 35.26, 48.63, 1, 900, 0.0, False),
    ("Synelnykove", 35.52, 48.32, 0, 700, 0.0, False),
    ("Okhtyrka", 34.90, 50.31, 1, 800, 0.15, False),
    ("Konotop", 33.20, 51.24, 1, 900, 0.0, False),
    ("Romny", 33.49, 50.75, 0, 700, 0.0, False),
    ("Lebedyn", 34.48, 50.59, 0, 700, 0.0, False),
    ("Shostka", 33.48, 51.86, 0, 700, 0.05, False),
    ("Hlukhiv", 33.92, 51.68, 0, 600, 0.15, False),
    ("Bohodukhiv", 35.53, 50.16, 0, 600, 0.05, False),
    ("Zolochiv", 35.98, 50.28, 0, 600, 0.4, False),
    ("Krasnohrad", 35.45, 49.37, 0, 600, 0.0, False),
    ("Lubny", 33.00, 50.01, 0, 700, 0.0, False),
    ("Pryluky", 32.39, 50.59, 1, 800, 0.0, False),
    ("Oleksandriia", 33.10, 48.67, 1, 900, 0.0, False),
    ("Voznesensk", 31.33, 47.57, 0, 700, 0.0, False),
    ("Yuzhnoukrainsk", 31.18, 47.82, 0, 600, 0.0, False),
    ("Bashtanka", 32.44, 47.40, 0, 500, 0.0, False),
    ("Apostolove", 33.71, 47.66, 0, 500, 0.0, False),
    ("Pokrov", 34.08, 47.65, 0, 600, 0.05, False),
    # Front-line ruins
    ("Bakhmut", 37.99, 48.59, 1, 1100, 1.0, True),
    ("Chasiv Yar", 37.84, 48.59, 0, 600, 1.0, False),
    ("Lyman", 37.80, 48.99, 0, 800, 0.9, False),
    ("Siversk", 38.09, 48.87, 0, 600, 1.0, False),
    ("Pokrovsk", 37.18, 48.28, 1, 1100, 0.95, True),
    ("Myrnohrad", 37.26, 48.30, 1, 800, 0.95, False),
    ("Avdiivka", 37.75, 48.14, 0, 800, 1.0, False),
    ("Toretsk", 37.85, 48.39, 0, 700, 1.0, False),
    ("Vuhledar", 37.25, 47.78, 0, 600, 1.0, False),
    ("Velyka Novosilka", 36.84, 47.84, 0, 500, 1.0, False),
    ("Vovchansk", 36.94, 50.29, 0, 700, 1.0, False),
    ("Popasna", 38.38, 48.63, 0, 600, 1.0, False),
    ("Rubizhne", 38.38, 49.01, 0, 800, 0.8, False),
    ("Marinka", 37.50, 47.94, 0, 500, 1.0, False),
    # Occupied
    ("Mariupol", 37.55, 47.13, 2, 2200, 0.6, True),
    ("Sievierodonetsk", 38.47, 48.95, 1, 1200, 0.65, False),
    ("Lysychansk", 38.43, 48.90, 1, 1200, 0.55, False),
    ("Volnovakha", 37.50, 47.60, 0, 700, 0.8, False),
    ("Donetsk", 37.80, 48.00, 2, 2800, 0.12, True),
    ("Horlivka", 38.05, 48.30, 2, 1500, 0.25, False),
    ("Makiivka", 37.97, 48.05, 2, 1500, 0.05, False),
    ("Luhansk", 39.31, 48.57, 2, 2000, 0.02, True),
    ("Alchevsk", 38.80, 48.47, 1, 1100, 0.02, False),
    ("Debaltseve", 38.40, 48.34, 0, 700, 0.4, False),
    ("Starobilsk", 38.91, 49.28, 0, 700, 0.0, False),
    ("Svatove", 38.16, 49.41, 0, 600, 0.25, False),
    ("Melitopol", 35.37, 46.85, 2, 1500, 0.03, True),
    ("Berdiansk", 36.79, 46.76, 1, 1200, 0.03, True),
    ("Enerhodar", 34.66, 47.50, 1, 900, 0.1, True),
    ("Tokmak", 35.71, 47.25, 0, 800, 0.1, False),
    ("Nova Kakhovka", 33.36, 46.75, 1, 900, 0.5, False),
    ("Oleshky", 32.72, 46.62, 0, 700, 0.5, False),
    ("Hola Prystan", 32.52, 46.53, 0, 600, 0.4, False),
    ("Henichesk", 34.82, 46.17, 0, 700, 0.0, False),
    ("Skadovsk", 32.91, 46.12, 0, 600, 0.0, False),
    ("Simferopol", 34.10, 44.95, 2, 2000, 0.0, True),
    ("Sevastopol", 33.53, 44.58, 2, 2000, 0.02, True),
    ("Kerch", 36.45, 45.35, 1, 1300, 0.0, True),
    ("Yevpatoria", 33.37, 45.21, 1, 1200, 0.0, False),
    ("Feodosia", 35.38, 45.05, 1, 1100, 0.0, True),
    ("Yalta", 34.16, 44.51, 1, 900, 0.0, False),
    ("Dzhankoi", 34.40, 45.71, 1, 800, 0.0, True),
    ("Alushta", 34.40, 44.68, 0, 700, 0.0, False),
    ("Armiansk", 33.69, 46.08, 0, 600, 0.0, False),
    ("Krasnoperekopsk", 33.79, 45.95, 0, 600, 0.0, False),
    ("Saky", 33.62, 45.14, 0, 600, 0.0, False),
    ("Bakhchysarai", 33.86, 44.75, 0, 600, 0.0, False),
    # Russia
    ("Belgorod", 36.59, 50.60, 2, 2000, 0.05, True),
    ("Kursk", 36.19, 51.73, 2, 2000, 0.0, True),
    ("Voronezh", 39.20, 51.66, 2, 2600, 0.0, True),
    ("Stary Oskol", 37.84, 51.30, 2, 1500, 0.0, False),
    ("Gubkin", 37.55, 51.28, 1, 900, 0.0, False),
    ("Valuyki", 38.10, 50.21, 1, 800, 0.0, True),
    ("Shebekino", 36.90, 50.41, 1, 800, 0.3, False),
    ("Rostov", 39.72, 47.23, 2, 2800, 0.0, True),
    ("Taganrog", 38.90, 47.22, 2, 1600, 0.0, True),
    ("Novocherkassk", 40.10, 47.42, 2, 1400, 0.0, False),
    ("Shakhty", 40.21, 47.71, 2, 1400, 0.0, False),
    ("Kamensk", 40.27, 48.32, 1, 900, 0.0, False),
    ("Millerovo", 40.40, 48.92, 1, 800, 0.0, True),
    ("Volgodonsk", 42.15, 47.51, 1, 1200, 0.0, False),
    ("Krasnodar", 38.98, 45.04, 2, 2600, 0.0, True),
    ("Novorossiysk", 37.77, 44.73, 2, 1600, 0.0, True),
    ("Anapa", 37.32, 44.89, 1, 1100, 0.0, False),
    ("Yeysk", 38.27, 46.71, 1, 1000, 0.0, True),
    ("Azov", 39.42, 47.11, 1, 900, 0.0, False),
    ("Krymsk", 38.00, 44.93, 0, 700, 0.0, False),
    ("Temryuk", 37.38, 45.27, 0, 700, 0.0, False),
    ("Tikhoretsk", 40.13, 45.85, 1, 900, 0.0, False),
    ("Kropotkin", 40.58, 45.44, 1, 900, 0.0, False),
    ("Armavir", 41.12, 44.99, 1, 1000, 0.0, False),
    ("Salsk", 41.54, 46.47, 1, 900, 0.0, False),
    ("Rossosh", 39.57, 50.19, 1, 900, 0.0, False),
    ("Liski", 39.50, 50.98, 1, 800, 0.0, False),
    ("Kurchatov", 35.65, 51.66, 0, 800, 0.0, False),
    ("Novovoronezh", 39.20, 51.31, 0, 700, 0.0, False),
    ("Belaya Kalitva", 40.80, 48.18, 0, 700, 0.0, False),
    ("Bataysk", 39.74, 47.14, 1, 900, 0.0, False),
    ("Primorsko-Akhtarsk", 38.17, 46.05, 0, 600, 0.0, False),
    ("Timashevsk", 38.94, 45.62, 0, 700, 0.0, False),
    ("Slavyansk", 38.12, 45.26, 0, 700, 0.0, False),
    ("Morozovsk", 41.82, 48.35, 0, 700, 0.0, False),
    ("Boguchar", 40.55, 49.93, 0, 600, 0.0, False),
    ("Pavlovsk", 40.10, 50.45, 0, 600, 0.0, False),
]

# Airbases: (name, lon, lat, friendly). The runway model runs north-south.
AIRBASES = [
    ("Myrhorod", 33.64, 49.93, True),
    ("Poltava", 34.40, 49.63, True),
    ("Kanatove", 32.28, 48.56, True),
    ("Dnipro", 35.10, 48.36, True),
    ("Kryvyi Rih", 33.21, 48.04, True),
    ("Mykolaiv", 32.10, 46.93, True),
    ("Kremenchuk", 33.30, 49.22, True),
    ("Belbek", 33.58, 44.69, False),
    ("Saky", 33.60, 45.09, False),
    ("Hvardiiske", 33.98, 45.11, False),
    ("Dzhankoi", 34.41, 45.70, False),
    ("Kirovske", 35.18, 45.16, False),
    ("Melitopol", 35.31, 46.88, False),
    ("Berdiansk", 36.76, 46.80, False),
    ("Taganrog", 38.85, 47.25, False),
    ("Millerovo", 40.37, 48.95, False),
    ("Morozovsk", 41.79, 48.31, False),
    ("Yeysk", 38.23, 46.68, False),
    ("Krymsk", 38.00, 44.97, False),
    ("Krasnodar", 39.17, 45.03, False),
    ("Belgorod", 36.65, 50.65, False),
    ("Kursk", 36.30, 51.75, False),
    ("Voronezh", 39.17, 51.62, False),
    ("Luhansk", 39.37, 48.42, False),
]

# Nuclear power plants: (name, lon, lat, model kind).
NPPS = [
    ("Zaporizhzhia NPP", 34.59, 47.51, "znpp"),
    ("Rostov NPP", 42.37, 47.60, "rostov"),
    ("South Ukraine NPP", 31.22, 47.81, "south"),
    ("Kursk NPP", 35.60, 51.67, "kursk"),
    ("Novovoronezh NPP", 39.20, 51.27, "novovoronezh"),
]

# Attack drone (Geran) launch sites: (name, lon, lat).
LAUNCH_SITES = [
    ("Primorsko-Akhtarsk", 38.17, 46.05),
    ("Chauda", 35.84, 45.00),
    ("Donetsk", 37.74, 48.07),
    ("Kursk", 36.30, 51.75),
    ("Millerovo", 40.32, 48.95),
]

# Enemy target sites behind the lines: (template, lon, lat, name).
SITES = [
    ("hq", 37.82, 47.98, "Donetsk"), ("hq", 39.34, 48.55, "Luhansk"), ("hq", 35.40, 46.83, "Melitopol"),
    ("hq", 39.70, 47.26, "Rostov"), ("hq", 33.55, 44.62, "Sevastopol"), ("hq", 36.62, 50.62, "Belgorod"),
    ("depot", 35.70, 47.22, "Tokmak"), ("depot", 34.42, 45.74, "Dzhankoi"), ("depot", 38.12, 50.18, "Valuyki"),
    ("depot", 38.88, 49.27, "Starobilsk"), ("depot", 37.48, 47.58, "Volnovakha"), ("depot", 38.14, 49.38, "Svatove"),
    ("depot", 36.75, 46.74, "Berdiansk"), ("depot", 40.40, 48.88, "Millerovo"), ("depot", 33.10, 46.30, "Skadovsk"),
    ("tankfarm", 35.40, 45.06, "Feodosia"), ("tankfarm", 37.80, 44.70, "Novorossiysk"), ("tankfarm", 39.80, 47.20, "Rostov"),
    ("tankfarm", 36.55, 50.55, "Belgorod"), ("tankfarm", 38.98, 45.08, "Krasnodar"), ("tankfarm", 37.40, 45.25, "Temryuk"),
    ("armor", 37.95, 48.70, "Bakhmut"), ("armor", 37.30, 48.15, "Pokrovsk"), ("armor", 36.40, 47.55, "Huliaipole"),
    ("armor", 35.85, 47.45, "Orikhiv"), ("armor", 37.95, 49.25, "Svatove"), ("armor", 37.25, 50.35, "Vovchansk"),
    ("armor", 33.30, 46.65, "Nova Kakhovka"), ("armor", 38.05, 49.60, "Kupiansk"),
    ("ss", 33.95, 45.40, "Simferopol"), ("ss", 36.95, 50.80, "Belgorod"), ("ss", 39.10, 47.40, "Taganrog"),
    ("ss", 36.10, 51.45, "Kursk"),
    ("pow", 38.55, 48.20, "Alchevsk"), ("pow", 34.30, 46.95, "Melitopol"),
    ("oth", 41.12, 45.10, "Armavir"),
    ("bridge", 39.72, 47.21, "Rostov"), ("bridge", 40.10, 47.37, "Novocherkassk"), ("bridge", 38.42, 48.915, "Lysychansk"),
    ("bridge", 40.27, 48.32, "Kamensk"), ("bridge", 39.42, 47.11, "Azov"),
    ("msam", 35.10, 46.70, "Melitopol"), ("msam", 37.10, 47.70, "Volnovakha"), ("msam", 38.60, 49.00, "Lysychansk"),
    ("msam", 34.70, 45.30, "Simferopol"), ("msam", 36.30, 50.75, "Belgorod"),
]

# Patrol boats: (lon, lat).
BOATS = [(33.42, 44.66), (37.82, 44.66), (36.95, 46.55), (37.50, 46.95), (36.55, 45.15), (33.20, 45.15)]

# Crimean mountains: the main ridge along the south coast, as raised models at LOD3
# (Central Europe's mountain shapes): (shape, lon, lat).
MOUNTAINS = [
    (4, 33.78, 44.55), (3, 34.02, 44.62), (89, 34.28, 44.70), (18, 34.48, 44.76), (90, 34.70, 44.85),
    (19, 34.95, 44.93), (4, 35.18, 44.99),
    # Caucasus foothills behind Novorossiysk
    (3, 38.30, 44.62), (90, 38.62, 44.48),
]

# Forest regions: (lon, lat, radius theater km, density 0..1).
FORESTS = [
    (33.30, 51.60, 45, 0.45), (34.20, 51.30, 30, 0.3), (35.70, 50.30, 25, 0.4), (36.40, 50.95, 35, 0.35),
    (37.60, 49.00, 18, 0.7), (37.90, 49.05, 12, 0.6), (38.50, 50.70, 25, 0.25), (39.55, 51.85, 18, 0.55),
    (32.90, 46.55, 12, 0.5), (34.30, 44.75, 22, 0.7), (35.60, 48.70, 10, 0.4), (38.20, 44.75, 25, 0.6),
    (32.10, 49.25, 15, 0.35), (36.70, 49.80, 12, 0.4),
]
