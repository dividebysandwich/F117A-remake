# f117-sim

Pure-Rust port of the VGAME.EXE combat game logic: EMV and radar detection, ground sites,
theater alarm, scrambles and the flight-unit AI, SAM/AAM guidance, jammers and countermeasures,
player weapons and targeting, damage, objectives and mission end. Specs: `docs/re/radar-threats.md`,
`docs/re/combat.md`, `docs/re/weapons.md`.

* Input: an `f117_mission::Mission` (the START hand-off) and `SimData` (tables read from VGAME.EXE
  and the theater .3DG/.3DT/.3DX; `SimData::load(&GameDir)` or `assets/sim/sim_data.bin`,
  written by f117-convert or `cargo run -p f117-sim --example export_sim_data`).
* `Sim::step(&PlayerState, fs) -> Vec<SimEvent>` runs one original frame. All arithmetic is the
  original 16-bit integer / BAM / map-unit maths; the RNG is MSC `rand` seeded from the
  mission's final `holdrand` (or `SimOptions::seed`), so runs are reproducible.

## Deviations from the original

Behind `SimOptions::fix_original_bugs` (default `true`):

* **Supply drop** (weapons.md §5.5.4): the original's airdrop test is unreachable (it tests the
  spec 38 inside a spec 28..30 branch), so mission type 3 can never be completed and a level
  release never comes down. Fixed: special equipment is released like a retarded bomb (pitch
  -90°, speed 1) and on ground contact the intended rule runs ("Equipment Delivered" within
  0x100 map units of a type-3 target before the deadline, else "Bad Drop").
* **AAM repeat hit** (weapons.md §5.5.3): after a hit the original missile keeps 2 s of ttl
  on the wreck, still locked, and repeats the hit (message, sound) every frame. Fixed: the
  missile is spent after the hit.

Always (engine / data limits):

* `posHitsTerrain` / `testWorldPosVisible` (3D containment of AI aircraft and projectiles) is not
  modelled: the world is a flat plane, projectiles impact at `alt < 0`. Player collisions come
  from the renderer via `PlayerState::collided`.
* Gun hit tests do not require the round to project on screen (the original runs them inside
  the HUD overlay), and the gun-vs-ground path uses the tile found by `findObjectAt` instead of
  the stale stack argument the original passes to `tileIsIntact`.
* HUD projection for the lock oval uses the F-15 `projectWorldToHud` formula (focal 256,
  boresight (160,52)); VGAME's B85D is assumed identical.
* "<base> - <type> landing" is printed once per landing (the original repeats it every frame
  while the aircraft is within 0x10 of the touchdown point).
* Base traffic (last 4 flight units) follows the doc summary; parked positions are next to the
  base point (exact offsets not documented).
* Countermeasure slots attract missiles only while their ttl > 0 (ASSUMED).
* The carrier deck-edge auto crash and the "No Crashes" bounce altitude are left to the flight
  model (`SimEvent::CollisionBounce`).
* Not ported: the never-called SA-14 pop-up launcher (V:5616), map plotting, smoke-ring and
  explosion animation timers (only events are emitted), palette / drawing.
* Without theater tile data (`SimData::from_vgame_image` only), `findObjectAt` falls back to
  the nearest world object within 0x100 map units.

## Fixes after integration
- `objectives.rs`: the bad-landing gear test was inverted (`!gear_up`); gear *up* is the bad case (combat.md 6.5.2). Every touchdown ended the mission as "Bad landing".
