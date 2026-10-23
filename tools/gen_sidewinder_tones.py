#!/usr/bin/env python3
"""Synthesizes the AIM-9 Sidewinder seeker tones (assets/sounds/aim9_growl.flac, aim9_tone.flac).

- Growl: the seeker's search tone, a buzzy 400 Hz carrier chopped by a rough 8-70 Hz
  amplitude flutter (the reticle chopping background heat), with a little hiss. The game plays
  it quietly with no target in the seeker and louder and higher once a heat source is in view.
- Tone: the lock tone, a steady high 1500 Hz whine with a faint warble.

Both loop seamlessly: the carriers make whole cycles per loop and the flutter is built from
whole-period noise (random spectrum on the loop's own frequency bins).
Mono, 44.1 kHz, 16 bit, like the other game sounds.

    python3 tools/gen_sidewinder_tones.py      (needs numpy and ffmpeg)
"""

import os
import subprocess

import numpy as np

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "assets", "sounds")
RATE = 44100

GROWL_SECONDS = 2.0
GROWL_FREQ = 400.0
FLUTTER_BAND = (8.0, 70.0)
TONE_SECONDS = 1.0
TONE_FREQ = 1500.0
WARBLE = (6.0, 0.004)


def periodic_noise(n, seconds, band, rng):
    """Noise that repeats exactly every `n` samples, limited to `band` (Hz), unit peak."""
    spec = np.zeros(n // 2 + 1, complex)
    lo, hi = (int(round(f * seconds)) for f in band)
    k = np.arange(lo, hi + 1)
    spec[k] = rng.normal(size=k.size) + 1j * rng.normal(size=k.size)
    x = np.fft.irfft(spec, n)
    return x / np.max(np.abs(x))


def growl():
    rng = np.random.default_rng(9)
    n = int(GROWL_SECONDS * RATE)
    # Slight pitch wander, periodic so the phase still ends on a whole cycle.
    wander = 1.0 + 0.02 * periodic_noise(n, GROWL_SECONDS, (1.0, 6.0), rng)
    f = GROWL_FREQ * wander
    cycles = np.sum(f) / RATE
    f *= round(cycles) / cycles
    phase = 2 * np.pi * np.cumsum(f) / RATE
    carrier = np.tanh(2.5 * sum(np.sin(k * phase) / k for k in (1, 2, 3, 5)))
    flutter = periodic_noise(n, GROWL_SECONDS, FLUTTER_BAND, rng)
    am = np.clip(0.55 + 0.6 * flutter, 0.0, None)
    hiss = 0.08 * periodic_noise(n, GROWL_SECONDS, (2000.0, 6000.0), rng)
    return carrier * am + hiss * am


def tone():
    n = int(TONE_SECONDS * RATE)
    t = np.arange(n) / RATE
    rate, depth = WARBLE
    f = TONE_FREQ * (1.0 + depth * np.sin(2 * np.pi * rate * t))
    cycles = np.sum(f) / RATE
    f *= round(cycles) / cycles
    phase = 2 * np.pi * np.cumsum(f) / RATE
    return np.sin(phase) + 0.25 * np.sin(2 * phase) + 0.1 * np.sin(3 * phase)


def write(name, x):
    x = x / np.max(np.abs(x)) * 0.89
    pcm = np.round(x * 32767).astype("<i2").tobytes()
    path = os.path.join(OUT, name)
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-f", "s16le", "-ar", str(RATE), "-ac", "1", "-i", "-",
         "-compression_level", "12", path],
        input=pcm, check=True)
    print(path)


if __name__ == "__main__":
    write("aim9_growl.flac", growl())
    write("aim9_tone.flac", tone())
