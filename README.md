# Stealth Mission 89

<img width="1988" height="1411" alt="image" src="https://github.com/user-attachments/assets/bb262243-cd4e-427b-8b9a-e59e0863a1af" />

A Rust/Bevy 0.19 remake of DOS-era 3D flight simulations with VR support

> **📖 [User Manual](docs/manual.md)**

<p align="center">
  <a href="https://discord.gg/zjjGeKKpC">
    <img alt="Join us on Discord" src="https://img.shields.io/badge/Join%20us%20on%20Discord-5865F2?style=for-the-badge&logo=discord&logoColor=white" height="48" />
  </a>
  <br />
  <sub>Fly together, report bugs, share missions and follow development on the <a href="https://discord.gg/zjjGeKKpC">Stealth Mission 89 Discord</a>.</sub>
</p>

## Features
* **Play in the browser** right on [stealth-mission.com](https://stealth-mission.com), no install needed
* **Dynamic realtime campaign:** a living war with materiel and dynamic missions, strikes, CSAR, CAP, escort, etc.
* **Career mode:** pilot records, tours of duty, ranks and medals
* **Multiplayer:** coop campaigns for up to 64 players, hosted in-game or on a **dedicated server**
* **Live map** of every public server's campaign on the [server list](https://sm-master.playspoon.com)
* **VR:** great on Steam Frame (streaming and standalone), fully clickable cockpit, virtual joysticks,
  working eject handle and kneeboard
* **HOTAS** support - but also supports keyboard-only flight
* **Head and Eye tracking** via FreeTrack
* **Weather:** layered volumetric clouds and thunderstorms that block seekers and visual ID
* **Theaters** including Ukraine 2026, with drone defense missions against Geran attack drones
* **Tacview** export and real-time telemetry plus ACMI recording
* **F-117A and F-16C**, each with its own cockpit and flight model
* **Ukraine theater** with drone defense missions and famous landmarks like the Zaporizhzhia NPP
* **Classic modes:** DOS era flight model, cockpit, HUD and rendering, switchable on their own

<img width="1842" height="1269" alt="image" src="https://github.com/user-attachments/assets/5c706024-1f8b-46f8-ad07-95373a8a8056" />

See the [User Manual](docs/manual.md) for details.

## Dynamic campaign

The war runs in real time, whether you fly or not. Pick sorties from the Ops screen's mission
board, alone or with up to 64 players in coop.
- **Goal:** the war is won when the enemy holds no airfield, and lost when the friendly side
  holds neither an airfield nor a carrier.
- **Resources:** each side's AI repairs, flies sorties and buys aircraft with the materiel its
  places and cities bring in every hour. Interdiction strikes on enemy cities cut income.
- **Enemy attacks:** helicopter attacks on weak spots along the front, escorted raids on
  airfields, AWACS patrols and, in Ukraine, Geran drone waves. The board posts intercept and
  defend missions for them. A bombed airfield launches no AI aircraft until it is repaired.
- **CSAR:** eject over enemy territory and the board posts a rescue mission.
  Hold the air near the pilot for 2 minutes to get them out, or they may be captured. In the
  UH-60, land within 200 m of the pilot, wait for them to board and land them at a friendly base.
- **Insertions:** order one for the AI helicopter crews, or lead it yourself in the UH-60 Black
  Hawk with AI wingmen: land within 1.5 km of the target and stay down while the troops get out.
- **Points and ranks:** objectives, kills and bringing the aircraft home earn points, ranks and
  medals. Packages (two kinds of objective at one place within 15 minutes) earn a bonus.
- **Weather** changes once or twice a day, from fair skies to thunderstorms.
- **Rules:** configurable by the host, including enemy strength, weather and realism.

<img width="1615" height="938" alt="image" src="https://github.com/user-attachments/assets/731ba65f-3876-4166-9252-40cc4cfa0f4b" />

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
- Fires and smoke burn along the whole front line and in the ruined towns.

## Running
```sh
cargo run --release
```
Pick a theater, situation, mission type, weather, opponents and aircraft in the menu. The briefing
shows the generated mission.

In the browser (WebGPU; coop joins public servers through the master server): `trunk serve --release`, see [docs/web.md](docs/web.md).

### Command-line options:

| Option | Effect |
|---|---|
| `--mission <seed> [--theater-index N]` | fly a generated mission directly |
| `--theater ce` / `--fly` | fly from the menu's defaults, skipping it |
| `--weather random\|clear\|scattered\|broken\|overcast\|storm` | the mission's weather (default: rolled from the seed, odds by theater) |
| `--cloud-cover <percent>` | coverage of the weather's main cloud layer (default: rolled) |
| `--modern` | modern rendering touches (smooth gradients and haze, subtle shading, MSAA) |
| `--aircraft f117\|f16\|uh60` | the aircraft to fly: the F-117A (default), the F-16C, which has its own cockpit (with an RWR scope instead of the radar bar graph) and flight model, or the UH-60A Black Hawk helicopter (cyclic and collective; the throttle controls move the collective; its missions are insertions: land near the targets) |
| `--classic-avionics` | DOS era MFD pages instead of the F-16 style formats (HSD, HARM, targeting pod, map, stores on either MFD, with a slewable cursor) |
| `--classic-hud` | DOS era low-resolution HUD instead of the green F-16 style one |
| `--classic-cockpit` | DOS era panel arrangement with 77x60 displays, instead of the modern panel (high-resolution map and displays, labelled MFD bezel buttons); F-117A only |
| `--shadows` / `--local-lights` | sun shadows; local lights (cockpit lamps, explosions, flares, missile motors, runway lights) |
| `--classic-flight` | DOS era flight model instead of the modern one (aerodynamic model with fly-by-wire: the stick commands pitch and roll rate, released the nose stays where it was left and the settled flight path is held); `--script` runs default to it, `--modern-flight` overrides |
| `--realistic-guidance` | realistic missile guidance: proportional navigation, motors that burn out (the missiles glide on, smoke only while burning), lofted SAM shots, the AIM-120 flying straight for 3 s so it can be lobbed |
| `--realistic-damage` | realistic weapon damage: missile hits can destroy the aircraft, start fires that burn out or blow it up, and bend control surfaces (out of trim, oscillating) |
| `--no-crashes` | crash protection: automatic pull-up near the ground with the gear up, ground contact and collisions are not fatal |
| `--tacview-real-time [--tacview-address <ip[:port]>] [--tacview-password <text>]` | Tacview real-time telemetry: the flight is shown in Tacview (*Record > Real-Time Telemetry*) while it is flown; hosted on 127.0.0.1:42674 by default, `0.0.0.0` lets another computer join |
| `--lockheed` | realistic weapons: the real aircraft's 2 bays, no air-to-air missiles, no carrier operations |
| `--head-tracking` | head/eye tracking: infrared, eye, webcam or phone trackers move the cockpit view through a tracking program such as OpenTrack (FreeTrack output on Windows, or UDP to 127.0.0.1 port 4242); Home recentres |
| `--vr` | OpenXR headset (without a runtime it runs on the desktop); the menu and the debrief are a panel in the headset, operated with a controller |
| `--viewer [--file stflt --model 24]` | browse the 3D models |
| `--freecam [--at Gutersloh\|primary] [--pos x,z]` | free debug camera |

<img width="1173" height="727" alt="image" src="https://github.com/user-attachments/assets/affc6127-3d83-49a2-8cde-e128e941fa64" />


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
| Night vision goggles | F4 |
| Maps / ordnance / FLIR (tracking camera in black and white thermal: aircraft, vehicles and buildings bright) | F3 / F5 / F6 |
| Waypoints | F7 / F8 / PgUp / PgDn |
| Mission orders | F10 |
| Tracking camera ahead, rear, right, left | `/` / `.` / `,` / M |
| Designate / next target | N / B |
| Nearest enemy target ahead (AIR or GND mode by the selected weapon) | T |
| Clear target (no automatic acquisition until the next target or camera key) | Shift B |
| Zoom / unzoom / wide angle | Z / X / C |
| Map zoom in / out, north up / heading up (modern panel) | Alt Z / Alt X / Alt F3 |
| Time acceleration / normal | Shift Z / Shift X |
| Pause / detail / sound level | Alt P / Alt D / Alt V |
| Training / resupply / quit | Alt T / Alt R / Alt Q |

The full key list is in [docs/keys.md](docs/keys.md). A gamepad works as stick and throttle as
well (right stick: rudder). The menu's Classic / Modern switches (shaders, shadows, lighting, cockpit, flight model) and
its Aircraft options match the options above. Shaders, shadows, lighting, cockpit and HUD can
also be switched in flight (Esc).

### Editing the cockpits
The cockpits are Blender models: `assets-src/blend/cockpit_modern.blend` and
`cockpit_classic.blend` are the sources, `tools/models/export.sh cockpit_modern cockpit_classic`
writes `assets/models/cockpit_*.glb`, which the game loads. The game finds its parts by
material name (live displays, HUD glass, instrument atlas faces, palette-coloured solids) and by
object custom properties (buttons, levers, lamps); the conventions are listed in
`src/cockpit3d/model.rs`. A new cockpit starts as a copy of one of the files.

## Server list
Coop servers report to a master server (`crates/stealth-master`, at sm-master.playspoon.com),
which lists running games both on the webpage and in-game. See 
[README](crates/stealth-master/README.md) for the API and deployment.

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

### Testing on Steam Frame
`tools/frame.sh` cross-compiles the aarch64 build on an x86-64 Linux PC, copies it to the Frame
over ssh (host `frame` in `~/.ssh/config`) and starts a VR flight there; Ctrl-C stops it and
copies the log back. See the script for its options and setup. Environment variables for
measuring: `SM89_PROFILE=1` logs frame, GPU pass and per-system CPU times every 10 seconds,
`SM89_MSAA=off|2|4|8` overrides anti-aliasing, `SM89_XR_PIPELINED=0` turns off pipelined
rendering in VR, `SM89_NO_INDIRECT=0|1` overrides direct draws (on in VR) and `SM89_FLAT_SHADER=<bits>`
flattens parts of the palette shader (1 all, 2 background, 4 objects, 8 clouds and smoke).
On the Frame, SteamVR's per-app GPU time (printed at the end of each run) is the reliable GPU
measurement.


## Contributing, LLM Usage

Both local and hosted LLMs (usually advertised as "Generative AI") were used in 
the development of this software. Contributions written using LLMs are ok 
provided the following rules are observed:

* **Read and review** generated code. You should be able to answer questions 
about your contribution.
* **Document and comment** non-trivial parts of the code.
* **Test** your contribution 
* Don't use LLMs for trivial things like changing a constant. This is slow, 
  wasteful and runs the risk of unnecessary modifications elsewhere.
* Use modern, sufficiently sized models with sufficient context size. Running 
  small or outdated models or limiting them to small contexts results in low 
  quality code and damage to existing functionality.
* Usage of locally-hosted LLMs is encouraged, but not required.
* Please keep commits as vendor-neutral as possible, i.e. put skills into the 
  ``docs`` directory and reference them.
