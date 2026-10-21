#!/usr/bin/env python3
"""Synthesizes the cockpit fire warning siren (assets/sounds/fire_siren.flac).

A wailing horn: a tone sweeping up from 650 Hz to 1150 Hz and back twice a second, with the
odd harmonics of a buzzy cockpit horn. It loops seamlessly: the sweep repeats a whole number
of times per loop and the frequency is scaled so the tone ends on a whole cycle.
Mono, 44.1 kHz, 16 bit, like the other game sounds.

    python3 tools/gen_fire_siren.py      (needs numpy and ffmpeg)
"""

import os
import subprocess

import numpy as np

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "assets", "sounds")
RATE = 44100

SECONDS = 2.0
SWEEPS = 4
LOW, HIGH = 650.0, 1150.0


def siren():
    n = int(SECONDS * RATE)
    t = np.arange(n) / RATE
    # Smooth up and down sweep, SWEEPS per loop.
    s = 0.5 - 0.5 * np.cos(2 * np.pi * SWEEPS * t / SECONDS)
    f = LOW + (HIGH - LOW) * s
    # A whole number of cycles per loop.
    cycles = np.sum(f) / RATE
    f *= round(cycles) / cycles
    phase = 2 * np.pi * np.cumsum(f) / RATE
    x = sum(np.sin(k * phase) / k for k in (1, 3, 5, 7))
    return np.tanh(1.5 * x)


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
    write("fire_siren.flac", siren())
