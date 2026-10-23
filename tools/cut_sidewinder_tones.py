#!/usr/bin/env python3
"""Cuts the AIM-9 Sidewinder seeker tones out of a recording of the real ones
(assets/sounds/aim9_growl.flac, aim9_seen.flac, aim9_tone.flac).

The recording ("AIM-9 Sidewinder missile sound effect", YouTube zJvVsHkSs_g, 16 s) has the
low search growl (0.15-3.1 s, after a moment of silence), the higher growl with a heat source in the seeker (3.1-6.65 s,
then a splice into a quieter take that isn't used) and the steady lock tone (12.3-16 s).

Each part becomes a seamless loop: the loop length is chosen near the part's end where the
sound lines up best with its start (autocorrelation), and the seam is crossfaded. All three
get the same gain, so they keep the recording's relative loudness.
Mono, 44.1 kHz, 16 bit, like the other game sounds.

    python3 tools/cut_sidewinder_tones.py <recording.mp3>      (needs numpy and ffmpeg)
"""

import os
import subprocess
import sys

import numpy as np

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "assets", "sounds")
RATE = 44100

# (file, start s, end s): the end is the latest the loop may run to.
PARTS = [
    ("aim9_growl.flac", 0.2, 3.08),
    ("aim9_seen.flac", 3.14, 6.62),
    ("aim9_tone.flac", 12.32, 15.9),
]
# Crossfade at the seam (s) and how far back from the end the loop point may move (s).
FADE = 0.12
SEARCH = 0.4


def decode(path):
    pcm = subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-i", path, "-ac", "1", "-ar", str(RATE), "-f", "s16le", "-"],
        capture_output=True, check=True).stdout
    return np.frombuffer(pcm, "<i2").astype(float) / 32768


def loop(x):
    """A seamless loop from `x`: cut where the end best matches the start, crossfade the seam."""
    fade = int(FADE * RATE)
    head = x[:fade]
    best, best_len = -2.0, len(x) - fade
    for n in range(len(x) - fade - int(SEARCH * RATE), len(x) - fade):
        tail = x[n:n + fade]
        c = np.dot(head, tail) / (np.linalg.norm(head) * np.linalg.norm(tail) + 1e-12)
        if c > best:
            best, best_len = c, n
    out = x[:best_len].copy()
    ramp = np.linspace(0.0, 1.0, fade)
    # A steady tone lines up and takes a linear crossfade; the growl is noisy and doesn't, so
    # it takes an equal-power one to keep its loudness through the seam.
    if best > 0.8:
        fade_in, fade_out = ramp, 1 - ramp
    else:
        fade_in, fade_out = np.sin(ramp * np.pi / 2), np.cos(ramp * np.pi / 2)
    # The loop starts with the material that followed the cut, fading into the start.
    out[:fade] = x[best_len:best_len + fade] * fade_out + head * fade_in
    print(f"  loop {best_len / RATE:.3f} s, seam correlation {best:.2f}")
    return out


def write(name, x):
    pcm = np.round(np.clip(x, -1, 1) * 32767).astype("<i2").tobytes()
    path = os.path.join(OUT, name)
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-f", "s16le", "-ar", str(RATE), "-ac", "1", "-i", "-",
         "-compression_level", "12", path],
        input=pcm, check=True)
    print(path)


if __name__ == "__main__":
    src = decode(sys.argv[1])
    loops = [(name, loop(src[int(a * RATE):int(b * RATE)])) for name, a, b in PARTS]
    gain = 0.89 / max(np.max(np.abs(x)) for _, x in loops)
    for name, x in loops:
        write(name, x * gain)
