"""Aircraft remodels for STFLT.3DX (real proportions in metres, nose along +y).

Each builder returns a `Craft`: the airframe, optional afterburner flames (shown by the
original's `_s13v1` node), an optional rotating part (`_part1`, pivot at the model origin) and
the anchors for the navigation light dots (147 red / left, 151 green / right, 149 white / tail).
"""

from mklib import Geo, Ramp, block, frustum, hexa, loft, wing, ring, tube
import math

from mathutils import Vector

GREY_L = Ramp(224, 234)
GREY = Ramp(226, 236)
GREY_D = Ramp(229, 239)
GREY_B = Ramp(225, 235)
CANOPY = Ramp(144, 145)
DARK = Ramp(235, 239)
RADOME = Ramp(230, 236)
OLIVE = Ramp(165, 173)
FLAME = 13
BLACK = 239


class Craft:
    def __init__(self, body, red, green, white, flames=None, part=None, pivot=False):
        self.body, self.flames, self.part = body, flames, part
        # Light dots sit just outboard of the wingtips so the tip itself doesn't hide them.
        out = Vector((0.25, 0, 0))
        self.lights = {147: [Vector(red) - out], 151: [Vector(green) + out], 149: [Vector(w) + Vector((0, -0.1, 0.1)) for w in white]}
        self.pivot = pivot


def mirror(g):
    return Geo().add(g).add(g.mirrored())


def canopy(y0, y1, z, hw, hh, n=6):
    """Bubble from front windscreen (y0) to aft fairing (y1) sitting at height z."""
    L = y0 - y1
    return loft(
        [(y0, z, 0.02, 0.02), (y0 - 0.25 * L, z + 0.6 * hh, 0.8 * hw, 0.7 * hh), (y0 - 0.55 * L, z + 0.75 * hh, hw, hh),
         (y1, z, 0.5 * hw, 0.2)],
        CANOPY, n=n, power=2.0,
    )


def nozzle(x, y, z, r, length, n=8):
    """Engine nozzle with the hot exit glowing red, like the original models."""
    g = frustum((x, y, z), (x, y - length, z), r * 1.05, r * 0.9, DARK, n, caps=False)
    g.add(frustum((x, y, z), (x, y - length + 0.15, z), r * 0.75, r * 0.75, FLAME, n, caps=True))
    return g


def flame(x, y, z, r, length, n=6):
    """Exhaust glow behind a nozzle (drawn whenever the aircraft is airborne)."""
    return frustum((x, y + 0.3, z), (x, y - length * 0.45, z), r * 0.85, 0.05, FLAME, n, caps=True)


def missile(x, y, z, length=3.0, r=0.08):
    g = frustum((x, y, z), (x, y - length, z), r, r, GREY_L, 4)
    g.add(frustum((x, y + 0.4, z), (x, y, z), 0.01, r, GREY_L, 4))
    return g


def box_intake(x, y0, y1, z, hw, hh, slant=0.6, paint=GREY):
    """Rectangular intake trunk with a raked mouth (top lip ahead by `slant`)."""
    g = hexa([(x - hw, y0 - slant, z - hh), (x + hw, y0 - slant, z - hh), (x + hw, y1, z - hh), (x - hw, y1, z - hh),
              (x - hw, y0, z + hh), (x + hw, y0, z + hh), (x + hw, y1, z + hh), (x - hw, y1, z + hh)], paint)
    # dark mouth just inside the lip
    lip = lambda t: y0 - slant * t + 0.04
    g.face2([(x - hw * 0.85, lip(0.925), z - hh * 0.85), (x + hw * 0.85, lip(0.925), z - hh * 0.85),
             (x + hw * 0.85, lip(0.075), z + hh * 0.85), (x - hw * 0.85, lip(0.075), z + hh * 0.85)], BLACK)
    return g


def nacelle(x, y0, y1, z, r, paint=GREY_L, n=8, inlet=BLACK):
    """Podded engine: lip ring, body, tapered exhaust."""
    L = y0 - y1
    g = loft([(y0, z, r * 0.9, r * 0.9), (y0 - 0.15 * L, z, r, r), (y1 + 0.2 * L, z, r * 0.9, r * 0.9), (y1, z, r * 0.55, r * 0.55)],
             paint, n=n, x=x)
    g.add(frustum((x, y0 + 0.01, z), (x, y0 - 0.05, z), r * 0.75, r * 0.75, inlet, n))
    return g


def pylon(x, y0, y1, z0, z1, w=0.12, paint=GREY):
    return hexa([(x - w, y0, z0), (x + w, y0, z0), (x + w, y1, z0), (x - w, y1, z0),
                 (x - w, y0 - 0.3 * (y0 - y1), z1), (x + w, y0 - 0.3 * (y0 - y1), z1), (x + w, y1, z1), (x - w, y1, z1)], paint)


# ---------------------------------------------------------------------------------------------
# Fighters


def f15():
    g = loft([(9.7, 0.1, 0.0, 0.0), (8.9, 0.1, 0.32, 0.3), (7.6, 0.15, 0.55, 0.55), (6.0, 0.2, 0.65, 0.7), (4.4, 0.25, 0.75, 0.75),
              (2.8, 0.3, 1.1, 0.7), (0.0, 0.3, 1.55, 0.65), (-4.0, 0.25, 1.6, 0.6), (-7.6, 0.2, 1.45, 0.55), (-8.5, 0.15, 1.3, 0.5)],
             GREY, n=8, power=2.6)
    g.add(loft([(9.7, 0.1, 0, 0), (8.9, 0.1, 0.32, 0.3), (8.2, 0.12, 0.42, 0.4)], RADOME, n=8))
    for s in (-1, 1):
        g.add(box_intake(s * 1.75, 4.0, -1.5, -0.05, 0.55, 0.75, slant=0.9))
        g.add(nozzle(s * 0.65, -8.4, 0.1, 0.58, 1.3))
    g.add(canopy(6.8, 3.6, 0.75, 0.42, 0.5))
    g.add(wing((1.5, 1.6, 0.25), 6.6, (6.5, -3.4, 0.15), 1.9, GREY, t=0.05))
    g.add(wing((1.5, -6.3, 0.1), 3.0, (4.3, -8.4, 0.05), 1.3, GREY, t=0.05))
    for s in (-1, 1):
        g.add(wing((s * 1.45, -5.2, 0.75), 3.3, (s * 1.85, -7.4, 3.4), 1.3, GREY, t=0.06, vertical=True, mirror=False))
    fl = Geo().add(flame(0.65, -9.7, 0.1, 0.5, 3.0)).add(flame(-0.65, -9.7, 0.1, 0.5, 3.0))
    return Craft(g, (-6.5, -4.6, 0.15), (6.5, -4.6, 0.15), [(-1.85, -8.6, 3.4), (1.85, -8.6, 3.4)], fl)


def f16():
    g = loft([(7.5, 0.25, 0.0, 0.0), (6.7, 0.25, 0.32, 0.32), (5.4, 0.3, 0.52, 0.58), (3.6, 0.35, 0.7, 0.72), (1.0, 0.3, 0.85, 0.7),
              (-2.5, 0.3, 0.85, 0.7), (-5.2, 0.3, 0.7, 0.62), (-6.2, 0.3, 0.6, 0.58)], GREY_B, n=8, power=2.4)
    g.add(loft([(7.5, 0.25, 0, 0), (6.7, 0.25, 0.32, 0.32), (6.1, 0.27, 0.42, 0.45)], RADOME, n=8))
    # chin intake and its trunk
    g.add(hexa([(-0.5, 2.9, -1.25), (0.5, 2.9, -1.25), (0.5, -1.0, -0.75), (-0.5, -1.0, -0.75),
                (-0.5, 3.3, -0.35), (0.5, 3.3, -0.35), (0.5, -1.0, -0.2), (-0.5, -1.0, -0.2)], GREY_B))
    g.face2([(-0.42, 3.0, -1.15), (0.42, 3.0, -1.15), (0.42, 3.25, -0.45), (-0.42, 3.25, -0.45)], BLACK)
    g.add(canopy(5.6, 2.4, 0.85, 0.4, 0.5))
    # strakes + wing
    g.add(wing((0.5, 4.6, 0.25), 4.2, (1.2, 0.9, 0.2), 1.0, GREY_B, t=0.04))
    g.add(wing((0.8, 1.0, 0.2), 4.6, (4.7, -2.4, 0.2), 1.2, GREY_B, t=0.045))
    g.add(wing((0.6, -4.3, 0.15), 2.4, (2.9, -6.0, -0.15), 1.0, GREY_B, t=0.05))
    g.add(wing((0, -2.8, 0.8), 3.7, (0, -5.6, 4.0), 1.2, GREY_B, t=0.05, vertical=True, mirror=False))
    g.add(wing((0.35, -4.2, -0.4), 1.6, (0.55, -4.8, -1.1), 0.9, GREY_B, t=0.06, vertical=True))
    for s in (-1, 1):
        g.add(missile(s * 4.8, -1.0, 0.2, 3.0))
    g.add(nozzle(0, -6.2, 0.3, 0.55, 1.1))
    fl = flame(0, -7.3, 0.3, 0.5, 3.2)
    return Craft(g, (-4.8, -3.6, 0.2), (4.8, -3.6, 0.2), [(0, -6.4, 4.0)], fl)


def twin_nacelle_fighter(L, span, flanker=False, paint=GREY_B):
    """MiG-29 / Su-27 layout: slim forebody, LERX, widely spaced nacelles under the wing, twin fins."""
    k = L / 17.3
    g = loft([(8.6 * k, 0.15, 0, 0), (7.8 * k, 0.15, 0.32, 0.32), (6.4 * k, 0.25, 0.55, 0.6), (4.2 * k, 0.3, 0.65, 0.7),
              (2.0 * k, 0.25, 0.75, 0.55), (-1.5 * k, 0.15, 0.7, 0.4), (-5.0 * k, 0.1, 0.5, 0.35),
              ((-8.7 if flanker else -6.5) * k, 0.1, 0.3, 0.25)], paint, n=8, power=2.4)
    g.add(loft([(8.6 * k, 0.15, 0, 0), (7.8 * k, 0.15, 0.32, 0.32), (7.0 * k, 0.18, 0.45, 0.47)], RADOME, n=8))
    nx = 1.05 * k
    for s in (-1, 1):
        g.add(box_intake(s * nx, 3.2 * k, 1.2 * k, -0.45, 0.42 * k, 0.48, slant=-0.5))
        g.add(loft([(1.2 * k, -0.45, 0.42 * k, 0.48), (-2.0 * k, -0.3, 0.55 * k, 0.55), (-6.6 * k, -0.25, 0.55 * k, 0.5),
                    (-7.6 * k, -0.25, 0.48 * k, 0.45)], paint, n=8, power=3.0, x=s * nx))
        g.add(nozzle(s * nx, -7.6 * k, -0.25, 0.48 * k, 0.8 * k))
    g.add(canopy(5.8 * k, 3.0 * k, 0.85, 0.4, 0.5))
    # LERX and wing
    g.add(wing((0.5, 6.0 * k, 0.15), 5.6 * k, (1.6 * k, 0.8 * k, 0.1), 2.0 * k, paint, t=0.035))
    g.add(wing((1.5 * k, 0.9 * k, 0.1), 4.4 * k, (span / 2, -3.3 * k, 0.05), 1.2 * k, paint, t=0.05))
    g.add(wing((1.6 * k, -5.2 * k, -0.1), 2.6 * k, (3.9 * k, -7.2 * k, -0.15), 1.0 * k, paint, t=0.05))
    for s in (-1, 1):
        g.add(wing((s * 1.55 * k, -3.0 * k, 0.45), 3.2 * k, (s * 1.95 * k, -5.8 * k, 3.4 * k), 1.3 * k, paint, t=0.06,
                   vertical=True, mirror=False))
    if flanker:
        for s in (-1, 1):
            g.add(missile(s * span / 2, -2.6 * k, 0.05, 3.6 * k, 0.1))
    fl = Geo()
    for s in (-1, 1):
        fl.add(flame(s * nx, -8.4 * k, -0.25, 0.45 * k, 3.0))
    tip = (span / 2, -3.9 * k, 0.05)
    return Craft(g, (-tip[0], tip[1], tip[2]), tip, [(-1.95 * k, -6.4 * k, 3.4 * k), (1.95 * k, -6.4 * k, 3.4 * k)], fl)


def mig29():
    return twin_nacelle_fighter(17.3, 11.4)


def su27():
    return twin_nacelle_fighter(21.9, 14.7, flanker=True)


def f18(two_seat=False):
    g = loft([(8.5, 0.1, 0, 0), (7.6, 0.1, 0.33, 0.33), (6.0, 0.15, 0.55, 0.6), (4.0, 0.2, 0.65, 0.72), (1.5, 0.15, 0.95, 0.65),
              (-2.0, 0.1, 1.05, 0.6), (-5.6, 0.1, 0.95, 0.55), (-6.9, 0.1, 0.85, 0.5)], GREY_B, n=8, power=2.6)
    g.add(loft([(8.5, 0.1, 0, 0), (7.6, 0.1, 0.33, 0.33), (6.9, 0.12, 0.45, 0.47)], RADOME, n=8))
    for s in (-1, 1):
        g.add(box_intake(s * 1.15, 1.6, -1.6, -0.35, 0.38, 0.45, slant=-0.4))
        g.add(nozzle(s * 0.55, -6.8, 0.1, 0.5, 1.0))
    g.add(canopy(6.3, 2.6 if two_seat else 3.6, 0.8, 0.42, 0.5))
    g.add(wing((0.5, 6.0, 0.25), 6.0, (1.3, 0.6, 0.2), 1.0, GREY_B, t=0.03))
    g.add(wing((1.2, 0.6, 0.15), 4.0, (6.1, -2.2, 0.1), 1.5, GREY_B, t=0.05))
    g.add(wing((1.0, -4.9, 0.0), 2.8, (3.4, -6.8, -0.1), 1.3, GREY_B, t=0.05))
    for s in (-1, 1):
        g.add(wing((s * 1.0, -2.2, 0.75), 3.2, (s * 2.1, -4.4, 3.5), 1.4, GREY_B, t=0.05, vertical=True, mirror=False))
        g.add(missile(s * 6.3, -0.6, 0.1, 3.0))
    fl = Geo().add(flame(0.55, -7.8, 0.1, 0.45, 3.0)).add(flame(-0.55, -7.8, 0.1, 0.45, 3.0))
    return Craft(g, (-6.3, -3.0, 0.1), (6.3, -3.0, 0.1), [(-2.1, -5.0, 3.5), (2.1, -5.0, 3.5)], fl)


def mig25():
    g = loft([(9.9, 0.3, 0, 0), (8.6, 0.3, 0.45, 0.45), (6.5, 0.35, 0.7, 0.7), (4.8, 0.4, 0.75, 0.75), (2.0, 0.4, 1.0, 0.6),
              (-4.0, 0.3, 1.0, 0.55), (-8.2, 0.3, 1.0, 0.55)], GREY_L, n=8, power=2.4)
    g.add(loft([(9.9, 0.3, 0, 0), (8.6, 0.3, 0.45, 0.45), (7.8, 0.32, 0.6, 0.6)], RADOME, n=8))
    for s in (-1, 1):
        # huge wedge intakes merging into the engine trunks
        g.add(hexa([(s * 1.0, 4.6, -0.9), (s * 2.3, 3.8, -0.9), (s * 2.3, -6.0, -0.6), (s * 1.0, -6.0, -0.6),
                    (s * 1.0, 4.6, 1.0), (s * 2.3, 4.6, 1.0), (s * 2.0, -6.0, 0.8), (s * 1.0, -6.0, 0.8)], GREY_L))
        g.face2([(s * 1.1, 4.54, -0.8), (s * 2.2, 3.89, -0.8), (s * 2.2, 4.64, 0.9), (s * 1.1, 4.64, 0.9)], BLACK)
        g.add(loft([(-6.0, 0.1, 0.75, 0.75), (-8.6, 0.1, 0.72, 0.72)], GREY_L, n=8, x=s * 0.95))
        g.add(nozzle(s * 0.95, -8.6, 0.1, 0.7, 1.2))
    g.add(canopy(6.6, 4.4, 1.0, 0.4, 0.35))
    g.add(wing((2.3, 0.6, 0.9), 6.2, (7.0, -4.3, 0.6), 2.0, GREY_L, t=0.04))
    g.add(wing((1.6, -5.9, 0.2), 3.2, (4.4, -8.1, 0.1), 1.6, GREY_L, t=0.05))
    for s in (-1, 1):
        g.add(wing((s * 1.6, -3.6, 0.9), 4.4, (s * 2.4, -6.6, 4.2), 1.8, GREY_L, t=0.05, vertical=True, mirror=False))
        g.add(wing((s * 1.7, -5.2, -0.6), 2.4, (s * 2.1, -6.4, -1.6), 1.2, GREY_L, t=0.05, vertical=True, mirror=False))
    fl = Geo().add(flame(0.95, -9.8, 0.1, 0.62, 3.6)).add(flame(-0.95, -9.8, 0.1, 0.62, 3.6))
    return Craft(g, (-7.0, -5.3, 0.6), (7.0, -5.3, 0.6), [(-2.4, -7.6, 4.2), (2.4, -7.6, 4.2)], fl)


def f14():
    g = loft([(9.5, 0.2, 0, 0), (8.5, 0.2, 0.4, 0.4), (6.5, 0.3, 0.65, 0.7), (4.0, 0.35, 0.8, 0.8), (1.5, 0.3, 1.0, 0.6),
              (-3.0, 0.2, 1.0, 0.4), (-7.0, 0.15, 0.8, 0.3)], GREY_L, n=8, power=2.4)
    g.add(loft([(9.5, 0.2, 0, 0), (8.5, 0.2, 0.4, 0.4), (7.6, 0.22, 0.55, 0.55)], RADOME, n=8))
    for s in (-1, 1):
        g.add(box_intake(s * 1.9, 3.0, 0.0, -0.2, 0.55, 0.7, slant=1.0, paint=GREY_L))
        g.add(loft([(0.0, -0.2, 0.55, 0.7), (-5.0, -0.1, 0.65, 0.65), (-8.1, -0.1, 0.62, 0.62)], GREY_L, n=8, power=2.6, x=s * 1.9))
        g.add(nozzle(s * 1.9, -8.1, -0.1, 0.62, 1.1))
        g.add(wing((s * 2.0, -3.6, 0.5), 3.6, (s * 2.6, -6.6, 3.4), 1.4, GREY_L, t=0.05, vertical=True, mirror=False))
    g.add(canopy(6.8, 2.8, 0.9, 0.45, 0.5))
    # glove + half-swept outer wing
    g.add(wing((0.9, 4.6, 0.3), 6.0, (2.6, 0.4, 0.3), 3.2, GREY_L, t=0.04))
    g.add(wing((2.6, 0.4, 0.3), 3.0, (7.6, -3.6, 0.3), 1.6, GREY_L, t=0.05))
    g.add(wing((2.4, -5.8, 0.0), 3.4, (5.0, -8.4, 0.0), 1.4, GREY_L, t=0.04))
    fl = Geo().add(flame(1.9, -9.2, -0.1, 0.55, 3.2)).add(flame(-1.9, -9.2, -0.1, 0.55, 3.2))
    return Craft(g, (-7.6, -4.6, 0.3), (7.6, -4.6, 0.3), [(-2.6, -7.2, 3.4), (2.6, -7.2, 3.4)], fl)


def f4():
    g = loft([(9.6, 0.1, 0, 0), (8.5, 0.1, 0.45, 0.45), (6.5, 0.2, 0.65, 0.7), (4.0, 0.3, 0.75, 0.8), (1.0, 0.2, 1.0, 0.75),
              (-4.0, 0.1, 0.9, 0.7), (-6.5, 0.2, 0.5, 0.55), (-9.6, 0.4, 0.2, 0.25)], GREY_D, n=8, power=2.3)
    g.add(loft([(9.6, 0.1, 0, 0), (8.5, 0.1, 0.45, 0.45), (7.6, 0.12, 0.58, 0.6)], RADOME, n=8))
    for s in (-1, 1):
        g.add(box_intake(s * 1.15, 4.0, 0.0, -0.05, 0.4, 0.7, slant=-0.4, paint=GREY_D))
        g.add(nozzle(s * 0.55, -5.0, -0.45, 0.45, 0.9))
        # outer wing panel with dihedral
        g.add(wing((s * 4.0, -0.9, 0.0), 3.2, (s * 5.85, -2.6, 0.75), 1.6, GREY_D, t=0.05, mirror=False))
    g.add(canopy(6.7, 2.6, 0.85, 0.45, 0.5))
    g.add(wing((0.9, 1.6, -0.2), 5.6, (4.0, -0.9, 0.0), 3.2, GREY_D, t=0.05))
    g.add(wing((0.3, -6.2, 0.4), 2.6, (2.9, -8.0, -0.4), 1.3, GREY_D, t=0.05))
    g.add(wing((0, -4.6, 0.6), 3.8, (0, -7.5, 3.6), 1.5, GREY_D, t=0.05, vertical=True, mirror=False))
    fl = Geo().add(flame(0.55, -5.9, -0.45, 0.42, 2.8)).add(flame(-0.55, -5.9, -0.45, 0.42, 2.8))
    return Craft(g, (-5.85, -3.2, 0.75), (5.85, -3.2, 0.75), [(0, -8.0, 3.6)], fl)


def mig21():
    g = loft([(7.6, 0.0, 0.0, 0.0), (7.0, 0.0, 0.28, 0.28), (6.6, 0.0, 0.5, 0.5), (4.0, 0.05, 0.62, 0.7), (0.0, 0.05, 0.66, 0.68),
              (-4.6, 0.05, 0.6, 0.6), (-6.4, 0.05, 0.5, 0.5)], GREY_L, n=8)
    g.add(frustum((0, 7.0, 0), (0, 7.9, 0), 0.26, 0.02, RADOME, 8))
    g.add(frustum((0, 6.95, 0), (0, 6.9, 0), 0.48, 0.48, BLACK, 8, caps=True))
    g.add(loft([(4.4, 0.55, 0.0, 0.0), (2.0, 0.6, 0.25, 0.25), (-3.6, 0.55, 0.18, 0.18)], GREY_L, n=6))
    g.add(canopy(4.6, 2.4, 0.55, 0.32, 0.38))
    g.add(wing((0.55, 1.6, -0.15), 5.4, (3.6, -3.4, -0.15), 0.6, GREY_L, t=0.04))
    g.add(wing((0.4, -4.6, 0.0), 1.8, (1.9, -5.7, 0.0), 0.8, GREY_L, t=0.05))
    g.add(wing((0, -2.4, 0.6), 3.4, (0, -5.0, 2.9), 1.2, GREY_L, t=0.05, vertical=True, mirror=False))
    g.add(wing((0, -4.6, -0.55), 1.4, (0, -5.4, -1.0), 0.8, GREY_L, t=0.06, vertical=True, mirror=False))
    g.add(nozzle(0, -6.4, 0.05, 0.5, 0.6))
    fl = flame(0, -7.0, 0.05, 0.45, 2.8)
    return Craft(g, (-3.6, -3.7, -0.15), (3.6, -3.7, -0.15), [(0, -5.6, 2.9)], fl)


def mig23():
    g = loft([(8.4, 0.1, 0, 0), (7.4, 0.1, 0.4, 0.4), (5.5, 0.2, 0.6, 0.65), (3.5, 0.3, 0.7, 0.75), (0.0, 0.2, 0.9, 0.7),
              (-5.0, 0.15, 0.75, 0.65), (-7.3, 0.15, 0.6, 0.6)], GREY_D, n=8, power=2.4)
    g.add(loft([(8.4, 0.1, 0, 0), (7.4, 0.1, 0.4, 0.4), (6.8, 0.12, 0.5, 0.5)], RADOME, n=8))
    for s in (-1, 1):
        g.add(box_intake(s * 1.15, 3.6, -1.5, 0.15, 0.45, 0.65, slant=0.3, paint=GREY_D))
    g.add(canopy(5.0, 2.6, 0.75, 0.38, 0.45))
    # fixed glove, outer panels at mid sweep
    g.add(wing((0.9, 1.4, 0.45), 4.4, (2.2, -0.6, 0.45), 2.6, GREY_D, t=0.04))
    g.add(wing((2.2, -0.4, 0.45), 2.6, (6.2, -3.6, 0.45), 1.3, GREY_D, t=0.05))
    g.add(wing((0.7, -5.1, 0.1), 2.6, (3.3, -6.8, -0.2), 1.1, GREY_D, t=0.05))
    g.add(wing((0, -2.4, 0.75), 4.2, (0, -5.8, 3.8), 1.6, GREY_D, t=0.05, vertical=True, mirror=False))
    g.add(wing((0, -5.2, -0.6), 1.8, (0, -6.0, -1.6), 1.1, GREY_D, t=0.06, vertical=True, mirror=False))
    g.add(nozzle(0, -7.3, 0.15, 0.6, 0.8))
    fl = flame(0, -8.1, 0.15, 0.55, 3.0)
    return Craft(g, (-6.2, -4.2, 0.45), (6.2, -4.2, 0.45), [(0, -6.6, 3.8)], fl)


def f5():
    g = loft([(7.2, 0.0, 0, 0), (6.0, 0.05, 0.3, 0.3), (4.4, 0.15, 0.5, 0.55), (2.5, 0.2, 0.6, 0.65), (0.0, 0.1, 0.75, 0.6),
              (-5.0, 0.05, 0.75, 0.55), (-6.6, 0.05, 0.7, 0.5)], GREY_B, n=8, power=2.4)
    for s in (-1, 1):
        g.add(box_intake(s * 0.85, 1.6, -1.2, 0.05, 0.3, 0.4, slant=-0.2, paint=GREY_B))
        g.add(nozzle(s * 0.38, -6.6, 0.05, 0.34, 0.6))
        g.add(missile(s * 4.1, -0.6, 0.0, 2.8))
    g.add(canopy(4.6, 2.2, 0.62, 0.36, 0.42))
    g.add(wing((0.3, 2.6, 0.0), 2.0, (0.7, 0.8, 0.0), 1.0, GREY_B, t=0.03))
    g.add(wing((0.7, 0.9, 0.0), 3.6, (4.05, -1.6, 0.0), 1.2, GREY_B, t=0.045))
    g.add(wing((0.6, -5.0, 0.15), 1.8, (2.4, -6.4, 0.15), 0.8, GREY_B, t=0.05))
    g.add(wing((0, -3.4, 0.6), 3.0, (0, -6.0, 3.1), 1.1, GREY_B, t=0.05, vertical=True, mirror=False))
    fl = Geo().add(flame(0.38, -7.2, 0.05, 0.3, 2.4)).add(flame(-0.38, -7.2, 0.05, 0.3, 2.4))
    return Craft(g, (-4.1, -2.4, 0.0), (4.1, -2.4, 0.0), [(0, -6.6, 3.1)], fl)


def mirage_f1():
    g = loft([(7.6, 0.1, 0, 0), (6.6, 0.1, 0.35, 0.35), (4.8, 0.2, 0.55, 0.6), (2.6, 0.25, 0.65, 0.7), (0.0, 0.2, 0.75, 0.7),
              (-5.0, 0.15, 0.7, 0.65), (-6.8, 0.15, 0.6, 0.6)], GREY_B, n=8, power=2.3)
    g.add(frustum((0, 7.6, 0.1), (0, 8.4, 0.1), 0.04, 0.02, BLACK, 4))
    for s in (-1, 1):
        g.add(loft([(3.0, 0.25, 0.0, 0.0), (2.2, 0.25, 0.4, 0.5), (-1.0, 0.25, 0.45, 0.55)], GREY_B, n=6, x=s * 0.75))
    g.add(canopy(4.8, 2.4, 0.72, 0.36, 0.42))
    g.add(wing((0.6, 0.0, 0.62), 3.8, (4.2, -2.8, 0.45), 1.6, GREY_B, t=0.04))
    g.add(wing((0.6, -4.8, 0.0), 2.2, (2.6, -6.4, 0.0), 1.0, GREY_B, t=0.05))
    g.add(wing((0, -2.6, 0.7), 3.6, (0, -5.6, 3.4), 1.3, GREY_B, t=0.05, vertical=True, mirror=False))
    g.add(nozzle(0, -6.8, 0.15, 0.6, 0.7))
    fl = flame(0, -7.5, 0.15, 0.55, 3.0)
    return Craft(g, (-4.2, -3.6, 0.45), (4.2, -3.6, 0.45), [(0, -6.2, 3.4)], fl)


def mig17():
    g = loft([(5.6, 0.0, 0.42, 0.45), (4.6, 0.05, 0.6, 0.65), (2.0, 0.1, 0.68, 0.72), (-1.5, 0.1, 0.6, 0.65), (-4.6, 0.15, 0.4, 0.45),
              (-5.6, 0.15, 0.36, 0.4)], GREY_L, n=8)
    g.add(frustum((0, 5.62, 0), (0, 5.55, 0), 0.36, 0.36, BLACK, 8))
    g.add(canopy(3.9, 2.0, 0.55, 0.32, 0.38))
    g.add(wing((0.6, 1.6, -0.1), 3.4, (4.8, -2.6, -0.3), 1.5, GREY_L, t=0.06))
    for s in (-1, 1):
        for fx in (2.0, 3.4):
            f = fx * s
            g.add(hexa([(f - 0.02, 1.6 - (fx - 0.6), -0.05), (f + 0.02, 1.6 - (fx - 0.6), -0.05), (f + 0.02, -0.9 - (fx - 0.6) * 0.75, -0.05),
                        (f - 0.02, -0.9 - (fx - 0.6) * 0.75, -0.05), (f - 0.02, 1.4 - (fx - 0.6), 0.2), (f + 0.02, 1.4 - (fx - 0.6), 0.2),
                        (f + 0.02, -0.8 - (fx - 0.6) * 0.75, 0.2), (f - 0.02, -0.8 - (fx - 0.6) * 0.75, 0.2)], GREY_D))
    g.add(wing((0, -2.4, 0.4), 3.4, (0, -5.0, 3.0), 1.6, GREY_L, t=0.06, vertical=True, mirror=False))
    g.add(wing((0.1, -3.9, 1.8), 1.9, (1.8, -5.0, 1.7), 0.9, GREY_L, t=0.05))
    g.add(nozzle(0, -5.6, 0.15, 0.36, 0.4))
    fl = flame(0, -6.0, 0.15, 0.32, 2.2)
    return Craft(g, (-4.8, -3.4, -0.3), (4.8, -3.4, -0.3), [(0, -5.8, 3.0)], fl)


def yak38():
    g = loft([(8.2, 0.1, 0, 0), (7.2, 0.1, 0.35, 0.35), (5.2, 0.2, 0.6, 0.65), (3.0, 0.3, 0.7, 0.8), (0.0, 0.2, 0.95, 0.85),
              (-5.0, 0.2, 0.75, 0.7), (-8.0, 0.3, 0.35, 0.4)], GREY_D, n=8, power=2.3)
    for s in (-1, 1):
        g.add(box_intake(s * 1.05, 2.4, -1.4, 0.2, 0.42, 0.65, slant=0.3, paint=GREY_D))
        g.add(frustum((s * 1.15, -1.0, -0.2), (s * 1.15, -1.9, -0.3), 0.45, 0.4, DARK, 8))
    g.add(canopy(5.6, 3.4, 0.75, 0.38, 0.45))
    g.add(wing((0.9, 0.0, 0.7), 3.6, (3.65, -2.0, 0.45), 1.6, GREY_D, t=0.05))
    g.add(wing((0.4, -5.2, 0.4), 1.9, (2.0, -6.4, 0.4), 0.9, GREY_D, t=0.05))
    g.add(wing((0, -3.6, 0.9), 3.4, (0, -6.6, 3.2), 1.4, GREY_D, t=0.05, vertical=True, mirror=False))
    return Craft(g, (-3.65, -2.8, 0.45), (3.65, -2.8, 0.45), [(0, -7.2, 3.2)])


def a6():
    g = loft([(8.4, -0.1, 0, 0), (7.6, -0.1, 0.6, 0.55), (6.0, 0.0, 1.0, 0.95), (3.5, 0.0, 1.05, 0.95), (0.0, 0.0, 0.95, 0.75),
              (-4.0, 0.2, 0.55, 0.5), (-8.2, 0.4, 0.25, 0.3)], GREY_L, n=8, power=2.2)
    g.add(loft([(6.2, 0.7, 0.05, 0.05), (5.4, 0.95, 0.8, 0.5), (4.0, 1.05, 0.85, 0.5), (3.0, 0.8, 0.5, 0.2)], CANOPY, n=6))
    for s in (-1, 1):
        g.add(nacelle(s * 1.1, 3.0, -2.2, -0.5, 0.48, GREY_L))
    g.add(frustum((0, 6.0, 0.9), (0, 7.4, 1.1), 0.05, 0.03, BLACK, 4))
    g.add(wing((0.9, 1.4, 0.2), 4.4, (8.1, -2.4, 0.2), 1.8, GREY_L, t=0.06))
    g.add(wing((0.3, -6.0, 0.6), 2.4, (3.4, -7.8, 0.6), 1.2, GREY_L, t=0.05))
    g.add(wing((0, -4.6, 0.6), 3.2, (0, -7.4, 4.0), 1.7, GREY_L, t=0.05, vertical=True, mirror=False))
    return Craft(g, (-8.1, -3.3, 0.2), (8.1, -3.3, 0.2), [(0, -8.2, 4.0)])


def ef111():
    g = loft([(11.6, 0.0, 0, 0), (10.4, 0.0, 0.45, 0.45), (8.6, 0.1, 0.85, 0.75), (6.4, 0.2, 1.1, 0.9), (2.0, 0.2, 1.5, 0.75),
              (-6.0, 0.2, 1.6, 0.7), (-9.6, 0.2, 1.4, 0.6), (-10.6, 0.25, 0.9, 0.5)], GREY_B, n=8, power=2.6)
    g.add(loft([(7.8, 0.8, 0.05, 0.05), (7.2, 1.05, 0.85, 0.4), (5.6, 1.15, 0.85, 0.4), (4.4, 0.9, 0.5, 0.2)], CANOPY, n=6))
    for s in (-1, 1):
        g.add(nozzle(s * 0.75, -10.2, 0.15, 0.62, 1.2))
        g.add(box_intake(s * 1.65, 2.6, -1.2, 0.1, 0.35, 0.6, slant=0.3, paint=GREY_B))
    g.add(wing((1.0, 3.4, 0.6), 5.4, (2.6, 2.4, 0.6), 4.0, GREY_B, t=0.04))
    g.add(wing((2.4, 2.2, 0.55), 3.4, (7.0, -3.3, 0.55), 1.5, GREY_B, t=0.05))
    g.add(wing((1.2, -7.6, 0.0), 3.6, (4.4, -10.6, 0.0), 1.5, GREY_B, t=0.05))
    g.add(wing((0, -5.0, 0.8), 4.8, (0, -9.6, 5.0), 2.4, GREY_B, t=0.05, vertical=True, mirror=False))
    g.add(loft([(-8.0, 5.2, 0.0, 0.0), (-8.6, 5.25, 0.32, 0.38), (-11.8, 5.25, 0.32, 0.38), (-12.3, 5.25, 0.1, 0.1)], GREY_B, n=6))
    fl = Geo().add(flame(0.75, -11.4, 0.15, 0.55, 3.4)).add(flame(-0.75, -11.4, 0.15, 0.55, 3.4))
    return Craft(g, (-7.0, -4.0, 0.55), (7.0, -4.0, 0.55), [(0, -12.3, 5.25)], fl)


def a10():
    p = GREY_D
    g = loft([(8.0, -0.2, 0, 0), (7.6, -0.2, 0.35, 0.35), (6.4, 0.0, 0.7, 0.7), (4.0, 0.15, 0.9, 0.95), (0.0, 0.15, 0.9, 0.9),
              (-4.5, 0.4, 0.55, 0.55), (-7.8, 0.6, 0.3, 0.35)], p, n=8, power=2.3)
    g.add(frustum((0, 8.0, -0.35), (0, 8.6, -0.35), 0.12, 0.12, BLACK, 6))
    g.add(canopy(6.2, 4.0, 0.8, 0.42, 0.55))
    g.add(wing((0.8, 1.4, -0.4), 3.2, (8.8, 0.5, -0.1), 1.8, p, t=0.12, peak=0.3))
    for s in (-1, 1):
        g.add(nacelle(s * 1.55, -2.0, -5.6, 1.35, 0.62, p))
        g.add(pylon(s * 1.0, -2.6, -4.4, 0.8, 1.0, 0.2, p))
        g.add(wing((s * 3.2, -6.4, 0.9), 2.4, (s * 3.25, -7.0, 3.2), 1.6, p, t=0.06, vertical=True, mirror=False))
        g.add(wing((s * 3.2, -6.6, 0.9), 1.6, (s * 3.25, -6.9, -0.4), 1.2, p, t=0.06, vertical=True, mirror=False))
    g.add(wing((0.3, -6.0, 0.8), 2.2, (3.3, -6.4, 0.9), 1.6, p, t=0.06))
    return Craft(g, (-8.8, -0.4, -0.1), (8.8, -0.4, -0.1), [(-3.25, -8.4, 3.2), (3.25, -8.4, 3.2)])


# ---------------------------------------------------------------------------------------------
# Transports, bombers, helicopter


def high_wing_transport(L, span, engines, t_tail=True, paint=GREY_L, z_wing=2.0):
    k = L / 46.6
    r = 2.0 * k * 1.15
    g = loft([(L / 2, -0.2 * k, 0, 0), (L / 2 - 1.0 * k, -0.1, r * 0.6, r * 0.6), (L / 2 - 4 * k, 0, r, r * 1.05),
              (-L * 0.2, 0, r, r * 1.05), (-L * 0.38, 0.6 * k, r * 0.6, r * 0.7), (-L / 2 + 2 * k, 1.4 * k, r * 0.25, r * 0.35)],
             paint, n=10, power=2.2)
    g.add(loft([(L / 2 - 2.0 * k, r * 0.55, 0.05, 0.05), (L / 2 - 2.8 * k, r * 0.78, r * 0.7, 0.4 * k),
                (L / 2 - 4.5 * k, r * 0.9, r * 0.75, 0.4 * k)], CANOPY, n=6))
    zw = z_wing * k * 1.1
    g.add(wing((r * 0.9, 2.0 * k, zw), 8.0 * k, (span / 2, -6.0 * k, zw - 1.0 * k), 2.6 * k, paint, t=0.1))
    for ex, ey in engines:
        for s in (-1, 1):
            x = s * ex * k
            g.add(nacelle(x, ey * k + 2.5 * k, ey * k - 3.5 * k, zw - 1.6 * k, 0.85 * k, paint))
            g.add(pylon(x, ey * k + 2.0 * k, ey * k - 1.5 * k, zw - 1.0 * k, zw, 0.15 * k, paint))
    if t_tail:
        g.add(wing((0, -L * 0.36, r * 0.9), 6.0 * k, (0, -L / 2 + 2.5 * k, r + 7.0 * k), 3.6 * k, paint, t=0.08, vertical=True,
                   mirror=False))
        g.add(wing((0.2, -L / 2 + 2.2 * k, r + 7.0 * k), 4.2 * k, (8.0 * k, -L / 2 - 1.0 * k, r + 6.6 * k), 2.0 * k, paint, t=0.06))
        tail = (0, -L / 2 + 1.0 * k, r + 7.2 * k)
    else:
        g.add(wing((0, -L * 0.36, r * 0.9), 6.0 * k, (0, -L / 2 + 2.0 * k, r + 6.0 * k), 3.0 * k, paint, t=0.08, vertical=True,
                   mirror=False))
        g.add(wing((0.5, -L * 0.4, r * 0.4), 5.0 * k, (8.0 * k, -L / 2, r * 0.6), 2.0 * k, paint, t=0.06))
        tail = (0, -L / 2, r + 6.0 * k)
    # main gear fairings
    for s in (-1, 1):
        g.add(loft([(3.0 * k, -r * 0.6, 0.0, 0.0), (1.0 * k, -r * 0.7, 0.6 * k, 0.6 * k), (-6.0 * k, -r * 0.7, 0.6 * k, 0.6 * k),
                    (-8.0 * k, -r * 0.6, 0.0, 0.0)], paint, n=6, x=s * r * 0.95))
    tip = (span / 2, -7.3 * k, zw - 1.0 * k)
    return g, tip, tail


def a50():
    g, tip, tail = high_wing_transport(46.6, 50.5, [(8.8, 0.2), (15.5, -4.3)])
    for y in (2.0, -2.0):
        g.add(pylon(0, y + 0.8, y - 0.8, 2.4, 5.0, 0.2))
    part = Geo().add(frustum((0, 0, 5.0), (0, 0, 5.8), 5.3, 5.3, Ramp(232, 238), 12, caps=True)).add(
        frustum((0, 0, 5.8), (0, 0, 6.6), 5.3, 5.3, GREY_L, 12, caps=True))
    return Craft(g, (-tip[0], tip[1], tip[2]), tip, [tail], part=part, pivot=True)


def an72():
    L, span = 28.1, 31.9
    k = 1.0
    r = 1.6
    g = loft([(L / 2, -0.2, 0, 0), (L / 2 - 0.8, -0.1, r * 0.6, r * 0.6), (L / 2 - 3, 0, r, r * 1.05), (-4.0, 0, r, r * 1.05),
              (-9.0, 0.7, r * 0.55, r * 0.6), (-L / 2 + 0.5, 1.6, r * 0.25, r * 0.3)], GREY_L, n=10, power=2.2)
    g.add(loft([(L / 2 - 1.6, r * 0.55, 0.05, 0.05), (L / 2 - 2.2, r * 0.8, r * 0.7, 0.35), (L / 2 - 3.6, r * 0.9, r * 0.75, 0.35)],
               CANOPY, n=6))
    g.add(wing((r * 0.9, 2.4, r + 0.4), 4.2, (span / 2, -0.6, r + 0.1), 1.8, GREY_L, t=0.1))
    for s in (-1, 1):
        # engines above and ahead of the wing (Coanda layout)
        g.add(nacelle(s * 3.2, 6.0, -1.5, r + 1.3, 0.85, GREY_L))
        g.add(loft([(-6.0, -r * 0.6, 0.0, 0.0), (-4.0, -r * 0.7, 0.5, 0.6), (2.0, -r * 0.7, 0.5, 0.6), (3.0, -r * 0.6, 0, 0)],
                   GREY_L, n=6, x=s * r * 0.95))
    g.add(wing((0, -7.0, r * 0.9), 5.0, (0, -11.5, 8.2), 3.0, GREY_L, t=0.08, vertical=True, mirror=False))
    g.add(wing((0.2, -11.3, 8.0), 3.4, (5.2, -13.6, 7.8), 1.6, GREY_L, t=0.06))
    return Craft(g, (-span / 2, -1.5, r + 0.1), (span / 2, -1.5, r + 0.1), [(-5.2, -14.6, 7.8), (5.2, -14.6, 7.8)])


def c5():
    g, tip, tail = high_wing_transport(75.3, 67.9, [(9.5, 1.5), (16.5, -4.0)], paint=GREY_B)
    return Craft(g, (-tip[0], tip[1], tip[2]), tip, [tail])


def tu95():
    L, span = 46.2, 50.1
    r = 1.45
    p = GREY_L
    g = loft([(L / 2, 0.0, 0, 0), (L / 2 - 1.2, 0.0, 0.8, 0.8), (L / 2 - 4, 0.1, r, r), (-L * 0.3, 0.1, r, r),
              (-L / 2 + 3.0, 0.4, r * 0.55, r * 0.55), (-L / 2, 0.6, 0.3, 0.35)], p, n=8)
    g.add(loft([(L / 2 - 2.4, r * 0.55, 0.05, 0.05), (L / 2 - 3.2, r * 0.85, r * 0.7, 0.35), (L / 2 - 5.0, r * 0.95, r * 0.7, 0.35)],
               CANOPY, n=6))
    g.add(frustum((0, L / 2 - 0.2, 0.3), (0, L / 2 + 1.4, 0.3), 0.06, 0.04, BLACK, 4))
    g.add(wing((r * 0.9, 1.5, -0.2), 7.0, (span / 2, -14.0, 1.4), 2.4, p, t=0.1))

    def le(x):
        t = (x - r * 0.9) / (span / 2 - r * 0.9)
        return 1.5 + t * (-14.0 - 1.5), -0.2 + t * 1.6

    for ex in (6.8, 13.6):
        for s in (-1, 1):
            y, z = le(ex)
            x = s * ex
            g.add(loft([(y + 1.6, z - 0.1, 0.25, 0.25), (y + 1.0, z - 0.1, 0.75, 0.75), (y - 3.0, z, 0.75, 0.8),
                        (y - 6.0, z + 0.1, 0.25, 0.3)], p, n=8, x=x))
            g.add(frustum((x, y + 1.6, z - 0.1), (x, y + 2.6, z - 0.1), 0.25, 0.02, GREY_D, 6))
            # contra-rotating propellers: two four-blade discs
            for py, rot in ((y + 1.45, 0.0), (y + 1.05, 0.785)):
                for b in range(4):
                    a = rot + b * 1.5708
                    dx, dz = math.cos(a), math.sin(a)
                    g.face2([(x + dx * 0.25 - dz * 0.12, py, z - 0.1 + dz * 0.25 + dx * 0.12),
                            (x + dx * 2.8 - dz * 0.15, py, z - 0.1 + dz * 2.8 + dx * 0.15),
                            (x + dx * 2.8 + dz * 0.15, py, z - 0.1 + dz * 2.8 - dx * 0.15),
                            (x + dx * 0.25 + dz * 0.12, py, z - 0.1 + dz * 0.25 - dx * 0.12)], GREY_D)
    g.add(wing((0, -L / 2 + 9.0, r * 0.9), 7.0, (0, -L / 2 + 1.5, r + 8.0), 3.0, p, t=0.08, vertical=True, mirror=False))
    g.add(wing((0.4, -L / 2 + 7.0, 0.6), 5.4, (7.2, -L / 2 + 0.8, 0.6), 2.2, p, t=0.07))
    return Craft(g, (-span / 2, -15.3, 1.4), (span / 2, -15.3, 1.4), [(0, -L / 2 + 0.3, r + 8.0)])


def uh60():
    p = OLIVE
    g = loft([(4.6, 0.3, 0.0, 0.0), (4.3, 0.2, 0.7, 0.8), (3.2, 0.3, 1.1, 1.15), (0.0, 0.3, 1.15, 1.2), (-2.4, 0.35, 1.0, 1.1),
              (-3.8, 0.7, 0.45, 0.55), (-10.5, 1.1, 0.25, 0.3), (-11.2, 1.2, 0.2, 0.25)], p, n=8, power=2.6)
    g.add(loft([(4.5, 0.4, 0.05, 0.05), (4.2, 0.9, 0.85, 0.5), (3.0, 1.15, 0.95, 0.45), (2.2, 1.3, 0.8, 0.2)], CANOPY, n=6))
    # engine housing / hub fairing
    g.add(block(-0.7, 0.7, -1.8, 1.4, 1.4, 2.0, p, taper=0.15, taper_y=0.3))
    g.add(frustum((0, 0, 2.0), (0, 0, 2.5), 0.3, 0.2, Ramp(236, 239), 6))
    for s in (-1, 1):
        g.add(frustum((s * 1.05, 0.2, 1.5), (s * 1.05, -1.6, 1.5), 0.32, 0.25, p, 6))
        g.add(wing((s * 0.9, 1.4, -0.4), 1.2, (s * 1.8, 1.4, -0.4), 0.8, p, t=0.15, mirror=False))
        g.add(frustum((s * 1.8, 1.0, -0.9), (s * 1.6, 1.0, -0.45), 0.35, 0.1, Ramp(236, 239), 6))
    # tail pylon, rotor and stabilator
    g.add(wing((0, -10.0, 1.1), 1.4, (0, -11.6, 3.1), 1.0, p, t=0.12, vertical=True, mirror=False))
    g.add(wing((0, -10.6, 1.4), 1.1, (2.2, -10.8, 1.5), 0.9, p, t=0.08))
    for b in range(4):
        a = b * math.pi / 2 + 0.4
        g.face2([(0.25, -11.0, 2.6), (0.25, -11.0 + math.cos(a) * 1.65, 2.6 + math.sin(a) * 1.65),
                (0.25, -11.0 + math.cos(a + 0.12) * 1.65, 2.6 + math.sin(a + 0.12) * 1.65)], Ramp(236, 239))
    part = Geo()
    for b in range(4):
        a = b * math.pi / 2
        dx, dy = math.cos(a), math.sin(a)
        nx, ny = -dy, dx
        part.face2([(dx * 0.3 + nx * 0.27, dy * 0.3 + ny * 0.27, 2.45), (dx * 8.2 + nx * 0.27, dy * 8.2 + ny * 0.27, 2.45),
                   (dx * 8.2 - nx * 0.27, dy * 8.2 - ny * 0.27, 2.45), (dx * 0.3 - nx * 0.27, dy * 0.3 - ny * 0.27, 2.45)], Ramp(235, 239))
    part.add(frustum((0, 0, 2.3), (0, 0, 2.6), 0.4, 0.4, Ramp(236, 239), 6))
    return Craft(g, (-1.1, -1.0, -0.2), (1.1, -1.0, -0.2), [(0, -11.8, 1.5)], part=part, pivot=True)


# STFLT model index -> builder. Missiles, bombs, chutes, shadows and the F-117 keep the
# original shapes.
BUILDERS = {
    0: mig25,
    3: lambda: f18(two_seat=True),
    5: f14,
    6: an72,
    7: mig23,
    8: f4,
    9: a50,
    13: yak38,
    14: tu95,
    16: mig29,
    18: mig17,
    19: mirage_f1,
    20: f5,
    21: f15,
    22: su27,
    23: f16,
    25: uh60,
    26: f18,
    27: a6,
    28: mig21,
    29: ef111,
    30: c5,
    32: a10,
}
