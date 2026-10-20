# Stealth Mission 89

<img width="1684" height="1204" alt="image" src="https://github.com/user-attachments/assets/c6cc829d-3e1d-46ab-be9e-2e11dbe84a67" />

A Rust/Bevy 0.19 remake of DOS-era 3D flight simulations with VR support

> **📖 [User Manual](docs/manual.md)**

## Running
```sh
cargo run --release
```
Pick a theater, situation, mission type, weather, opponents and aircraft in the menu. The briefing
shows the generated mission.

<img width="1414" height="705" alt="image" src="https://github.com/user-attachments/assets/44d5b0eb-702a-4281-aa8e-86c82fc969e2" />

Command-line options:

| Option | Effect |
|---|---|
| `--mission <seed> [--theater-index N]` | fly a generated mission directly |
| `--theater ce` / `--fly` | fly from the menu's defaults, skipping it |
| `--weather random\|clear\|scattered\|broken\|overcast\|storm` | the mission's weather (default: rolled from the seed, odds by theater) |
| `--cloud-cover <percent>` | coverage of the weather's main cloud layer (default: rolled) |
| `--modern` | modern rendering touches (smooth gradients and haze, subtle shading, MSAA) |
| `--aircraft f117\|f16` | the flown aircraft: the F-117A (default) or the F-16C, which has its own cockpit (with an RWR scope instead of the radar bar graph) and flight model |
| `--classic-avionics` | the original's MFD pages instead of the F-16 style formats (HSD, HARM, targeting pod, map, stores on either MFD, with a slewable cursor) |
| `--classic-hud` | the original low-resolution HUD instead of the green F-16 style one |
| `--classic-cockpit` | the original instrument panel arrangement with 77x60 displays, instead of the modern panel (high-resolution map and displays, labelled MFD bezel buttons); F-117A only |
| `--shadows` / `--local-lights` | sun shadows; local lights (cockpit lamps, explosions, flares, missile motors, runway lights) |
| `--classic-flight` | the original game's flight model instead of the modern one (aerodynamic model with fly-by-wire: the stick commands pitch and roll rate, released the nose stays where it was left and the settled flight path is held); `--script` runs default to it, `--modern-flight` overrides |
| `--no-crashes` | crash protection: automatic pull-up near the ground with the gear up, ground contact and collisions are not fatal |
| `--lockheed` | realistic weapons: the real aircraft's 2 bays, no air-to-air missiles, no carrier operations |
| `--vr` | OpenXR headset (without a runtime it runs on the desktop) |
| `--viewer [--file stflt --model 24]` | browse the converted models |
| `--freecam [--at Gutersloh\|primary] [--pos x,z]` | free debug camera |

<img width="1173" height="727" alt="image" src="https://github.com/user-attachments/assets/affc6127-3d83-49a2-8cde-e128e941fa64" />

## Weather

- **Sky:** a mix of up to three cloud layers, each with its own coverage:
  - cumulus, drawn as ray-marched volumes;
  - a stratus or stratocumulus layer, with gaps of clear sky placed by a noise field
    thresholded to its coverage;
  - cumulonimbus storm towers.
- **Presets:** the menu's Weather choice (`--weather`) picks a realistic mix and rolls the
  coverages. "Random cloud cover" off (`--cloud-cover <percent>`) sets the main layer's
  coverage instead: stratus for overcast and storms, cumulus otherwise. The briefing lists
  every layer.
- **Storms:** towers bring rain, lightning, turbulence and shorter visibility near them,
  whatever the stratus coverage.
- **Clouds block optical lines of sight:**
  - IR and AIM-9 seekers, the Maverick and laser-guided bombs lose their target behind cloud.
  - Ground targets under cloud can't be designated ("Target obscured").
  - Enemy pilots can't visually ID you or use their guns through cloud.
  - IR SAMs and IR air-to-air missiles aren't launched at what their launcher can't see.
- **Radar** sees through cloud.

## Ukraine theater

Eastern Ukraine, Crimea and southern Russia (about 31-42.6 E, 44-52 N), with the front line of 2026
from the Sumy and Kharkiv border areas through Kupiansk, Lyman, Kostiantynivka, Pokrovsk and
Huliaipole to the Dnipro and down the river to Kherson. You fly from Ukrainian bases (Myrhorod,
Poltava, Dnipro, ...) against the occupied areas, Crimea and the Russian bases behind them; the
missions follow Central Europe's rules (`--theater ua`).
- Real towns and cities: Soviet panel-block districts with private houses around them; towns at
  and behind the front lie in ruins, and the Donbas mining towns have their slag heaps.
- The steppe is farmland: autumn fields (stubble, ploughland, winter wheat) with shelterbelts.
- The Zaporizhzhia, South Ukraine, Rostov, Kursk and Novovoronezh nuclear power plants.
- Fires and smoke burn along the whole front line and in the ruined towns in every mission.

## Controls (original keyboard layout)

| Action | Key |
|---|---|
| Stick | Arrows / keypad (tap twice for full deflection, Ins: sensitivity) |
| Throttle up / down | `=` / `-` |
| Full / idle throttle | Shift `=` / Shift `-` |
| Select weapon / fire / cannon | Space / Enter / Backspace |
| Flare / chaff / IR jammer / ECM / decoy | 1 / 2 / 3 / 4 / 5 |
| Gear / autopilot / bay doors / flaps / brakes | 6 / 7 / 8 / 9 / 0 |
| Eject | Shift F10 |
| Cockpit / look ahead, rear, left, right | F1 / Shift `/` `.` `,` M |
| External views | Shift F1 .. Shift F6 |
| HUD mode / declutter | F2 / V |
| Night vision goggles (not in the original) | F4 |
| Maps / ordnance / FLIR (tracking camera in black and white thermal: aircraft, vehicles and buildings bright) | F3 / F5 / F6 |
| Waypoints | F7 / F8 / PgUp / PgDn |
| Mission orders | F10 |
| Tracking camera ahead, rear, right, left | `/` / `.` / `,` / M |
| Designate / next target | N / B |
| Nearest enemy target ahead (AIR or GND mode by the selected weapon; not in the original) | T |
| Clear target (no automatic acquisition until the next target or camera key; not in the original) | Shift B |
| Zoom / unzoom / wide angle | Z / X / C |
| Map zoom in / out, north up / heading up (modern panel) | Alt Z / Alt X / Alt F3 |
| Time acceleration / normal | Shift Z / Shift X |
| Pause / detail / sound level | Alt P / Alt D / Alt V |
| Training / resupply / quit | Alt T / Alt R / Alt Q |

The full key list is in [docs/keys.md](docs/keys.md). A gamepad works as stick and throttle as
well (right stick: rudder). The menu's Classic / Modern switches (shaders, shadows, lighting, cockpit, flight model) and
its Aircraft options match the options above. Shaders, shadows, lighting, cockpit and HUD can
also be switched in flight (Esc).

## 3D cockpit (desktop)
* Fully clickable cockpit
* Hold right mouse button to look around
* Esc opens the in-flight settings menu (also in VR: the MENU button on the left side panel)

### Editing the cockpits
The cockpits are Blender models: `assets-src/blend/cockpit_modern.blend` and
`cockpit_classic.blend` are the sources, `tools/models/export.sh cockpit_modern cockpit_classic`
writes `assets/models/cockpit_*.glb`, which the game loads. The game finds its parts by
material name (live displays, HUD glass, instrument atlas faces, palette-coloured solids) and by
object custom properties (buttons, levers, lamps); the conventions are listed in
`src/cockpit3d/model.rs`. A new cockpit starts as a copy of one of the files.

## VR support
* Tested on Steam Frame
* Streaming and standalone supported
* Control your aircraft with keyboard, HOTAS or virtual joysticks using hand controllers
* All buttons operable in VR
* Working eject handle

## Classic modes
* Classic flight model
* Classic cockpit
* Classic HUD
* Classic rendering

<img width="1681" height="1342" alt="image" src="https://github.com/user-attachments/assets/1d783e0c-12fa-4f75-93a8-327b0d952782" />


## Testing
```sh
cargo test --workspace
cargo run -q -- --mission 1234 --script "0:ThrottleMax,7:pitch=0.35,13:pitch=0,14:Gear" \
    --log-flight --shot-frames 600 --screenshot screenshots/test.png
```
Test runs can use a private headless compositor (such as sway) so test windows don't appear on
the desktop. `--script` replays timed actions, `--log-flight` / `--log-sim`
print the state, `--time-scale` speeds runs up, and `--diag --novsync` shows the frame rate.
Script steps `yaw=`, `throttle=` and `press=Gear` drive the rudder, an absolute throttle and the
3D cockpit's controls; `--head-look yaw,pitch` turns the 3D cockpit view (degrees).
