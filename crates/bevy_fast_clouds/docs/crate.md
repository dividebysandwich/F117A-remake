Hierarchical trigonometric wavelet volumes for real-time clouds in [Bevy](https://bevyengine.org) 0.19.

![planet_approach example](https://raw.githubusercontent.com/steven-moss-notebook/bevy_fast_clouds/main/docs/planet_approach.gif)

A cloud is a box of overlapping blobs. For each pixel whose ray hits that box, the shader walks a **fixed number of steps** along the ray and adds density. The walk is the cost. This crate spends less on that walk by skipping work that cannot show up in the pixel.

```text
cargo run --example display_case
cargo run --example planet_approach
```

# Why the walk is cheap

**1. Miss the box, skip the pixel.**
The GPU draws the cloud’s axis-aligned bounding box, the same way Bevy draws a fog volume: rasterize the crate around the puff, then march only those fragments. A fullscreen pass would walk empty sky.

**2. Stripes that line up with the ray cancel.**
Inside the box, density is not a 3D texture. It is a few families of **stripes** (sine waves) aimed along a fixed list of directions \(n\). A sine wave over a whole number of periods averages to zero — the same fact as \(\int_0^{2\pi}\sin\theta\,d\theta = 0\) in precalculus.

If \(n\) is almost parallel to the ray, the walk crosses many full periods of that stripe. The plus and minus parts cancel, so the shader **does not evaluate that direction**. If \(n\) is across the ray, the stripe looks almost constant along the walk, and it is worth paying for.

**3. Far away, keep the high-level shape.**
Each direction is a little octave tree. Coarse scale \(j=0\) is the overall puff; \(j=3\) is small details. When the box is small on screen those details are smaller than a pixel, so \(j_\max\) and the number of sine harmonics \(H\) go down. Empty space is never split further.

Work per pixel is then roughly

\[
(\text{directions still on}) \times (\text{scales still on}) \times 2 \text{ overlapping windows} \times H \times (\text{march steps}).
\]

Each remaining term is a short sine dot product, not a special function.

# Types

- [`FastCloudsPlugin`], [`CloudVolume`], [`LodPolicy`], [`CloudRenderSettings`]
- [`WaveletVolume::from_spheres`] — occupancy from a sphere union; no fitted dictionary

# The walk, as a loop

Only the box is walked. Waves along the ray cancel. Far away, small details drop and the high-level shape stays.
