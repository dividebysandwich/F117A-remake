#!/usr/bin/env python3
"""Synthesizes the loops of other aircraft heard by the downed pilot (assets/sounds/*.flac).

- jet.flac: a jet's roar (broadband noise peaking at a few hundred Hz) with a slow turbulent
  swell and a faint turbine whine.
- helicopter.flac: main rotor blade slap (4 blades at 258 rpm, 17.2 Hz), a tail rotor buzz
  and the turbine whine over a low rumble.

Both loop seamlessly: the noise is synthesized in the frequency domain (periodic in the loop
length) and every tone and pulse train repeats a whole number of times per loop.
Mono, 44.1 kHz, 16 bit, like the other game sounds.

    python3 tools/gen_aircraft_sounds.py      (needs numpy and ffmpeg)
"""

import os
import subprocess

import numpy as np

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "assets", "sounds")
RATE = 44100

rng = np.random.default_rng(89)


def periodic_noise(seconds, shape):
    """Noise of `seconds` that loops seamlessly, amplitude spectrum `shape(freq_hz)`."""
    n = int(seconds * RATE)
    f = np.fft.rfftfreq(n, 1.0 / RATE)
    spec = shape(np.maximum(f, 1e-3)) * np.exp(2j * np.pi * rng.random(len(f)))
    spec[0] = 0.0
    x = np.fft.irfft(spec, n)
    return x / np.sqrt(np.mean(x * x))


def band(f, lo, hi, order=2.0):
    """Smooth band-pass magnitude between `lo` and `hi` Hz."""
    return 1.0 / np.sqrt(1.0 + (lo / f) ** (2 * order)) / np.sqrt(1.0 + (f / hi) ** (2 * order))


def tone(seconds, hz, phase=0.0):
    """A sine with a whole number of cycles in the loop (frequency rounded to fit)."""
    n = int(seconds * RATE)
    cycles = max(1, round(hz * seconds))
    return np.sin(2 * np.pi * cycles * np.arange(n) / n + phase)


def swell(seconds, max_hz, depth):
    """Slow periodic amplitude fluctuation 1 +- depth."""
    m = periodic_noise(seconds, lambda f: (f <= max_hz) * 1.0)
    return 1.0 + depth * m / np.max(np.abs(m))


def write(name, x):
    x = x / np.max(np.abs(x)) * 0.89
    pcm = np.round(x * 32767).astype("<i2").tobytes()
    path = os.path.join(OUT, name)
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-f", "s16le", "-ar", str(RATE), "-ac", "1", "-i", "-",
         "-compression_level", "12", path],
        input=pcm, check=True)
    print(path)


def jet():
    t = 4.0
    # Roar: peak around 100-400 Hz, rumble below, falling off steeply above (the air absorbs
    # the highs of a distant jet).
    roar = periodic_noise(t, lambda f: band(f, 45, 400, 1.2) + 0.12 * band(f, 400, 2500, 1.5) + 0.5 * band(f, 22, 90, 2.0))
    # Crackle of the exhaust: brighter noise in short random bursts.
    hiss = periodic_noise(t, lambda f: band(f, 1500, 6000, 2.0))
    gate = periodic_noise(t, lambda f: (f < 30) * 1.0)
    crackle = hiss * np.clip(gate / np.max(np.abs(gate)) * 2.0 - 0.6, 0.0, None)
    whine = 0.6 * tone(t, 2650) + 0.3 * tone(t, 2650 * 1.5, 1.0) + 0.2 * tone(t, 5300, 2.0)
    whine *= swell(t, 1.5, 0.4)
    return (roar + 0.06 * crackle) * swell(t, 3.0, 0.25) + 0.015 * whine


def helicopter():
    t = 3.0
    n = int(t * RATE)
    # Main rotor: 4 blades at 258 rpm (51 blade passes in the loop), each a sharp slap (filtered
    # noise burst) over a softer thump, the blades a little uneven.
    passes = round(4 * 258 / 60 * t)
    slap_noise = periodic_noise(t, lambda f: band(f, 250, 1800, 2.0))
    thump = np.zeros(n)
    slap = np.zeros(n)
    k = np.arange(int(0.06 * RATE))
    thump_pulse = np.sin(2 * np.pi * 70 * k / RATE) * np.exp(-k / (0.018 * RATE)) * (1 - np.exp(-k / (0.002 * RATE)))
    slap_env = np.exp(-k / (0.007 * RATE)) * (1 - np.exp(-k / (0.0008 * RATE)))
    unevenness = [1.0, 0.82, 0.93, 0.78]
    for i in range(passes):
        start = int(round(i * n / passes))
        idx = (start + k) % n
        g = unevenness[i % 4]
        thump[idx] += g * thump_pulse
        slap[idx] += g * slap_env
    slap *= slap_noise
    # Tail rotor buzz (about 4.6x the main rotor) with harmonics.
    tail_hz = 79.0
    tail = sum(tone(t, tail_hz * h, h) / h for h in range(1, 7))
    tail *= 1.0 + 0.3 * tone(t, 4 * 258 / 60)
    whine = tone(t, 3100) + 0.4 * tone(t, 6200, 1.0)
    rumble = periodic_noise(t, lambda f: band(f, 30, 400, 1.5))
    return 1.0 * thump / np.max(np.abs(thump)) + 0.9 * slap / np.max(np.abs(slap)) + 0.12 * tail / np.max(np.abs(tail)) + 0.025 * whine + 0.18 * rumble / np.max(np.abs(rumble))


if __name__ == "__main__":
    write("jet.flac", jet())
    write("helicopter.flac", helicopter())
