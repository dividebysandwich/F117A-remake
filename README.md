# Stealth Fighter

A Rust/Bevy 0.19 remake of DOS-era 3D flight simulations

## Running
```sh
cargo run --release
```
Pick a theater, situation, mission type, weather, opponents and aircraft in the menu. The briefing
shows the generated mission.

Command-line options:

| Option | Effect |
|---|---|
| `--mission <seed> [--theater-index N]` | fly a generated mission directly |
| `--theater ce` / `--fly` | fly from the menu's defaults, skipping it |
| `--weather random\|clear\|scattered\|broken\|overcast\|storm` | the mission's weather (default: rolled from the seed, odds by theater) |
| `--modern` | modern rendering touches (smooth gradients and haze, subtle shading, MSAA) |
| `--classic-cockpit` | the original instrument panel arrangement with 77x60 displays, instead of the modern panel (high-resolution map and displays, labelled MFD bezel buttons) |
| `--shadows` / `--local-lights` | sun shadows; local lights (cockpit lamps, explosions, flares, missile motors, runway lights) |
| `--vr` | OpenXR headset (without a runtime it runs on the desktop) |
| `--viewer [--file stflt --model 24]` | browse the converted models |
| `--freecam [--at Gutersloh\|primary] [--pos x,z]` | free debug camera |

## Weather

Not part of the original game. The original generator runs unchanged, and the weather is
rolled from its own random stream. A clear sky behaves exactly like the original.
- Scattered and broken cumulus, or an overcast deck. Storms add towering cells, rain,
  lightning, turbulence and shorter visibility.
- Clouds block optical lines of sight:
  - IR and AIM-9 seekers, the Maverick and laser-guided bombs lose their target behind cloud.
  - Ground targets under the deck can't be designated ("Target obscured").
  - Enemy pilots can't visually ID you or use their guns through cloud.
  - IR SAMs and IR air-to-air missiles aren't launched at what their launcher can't see.
- Radar sees through cloud.

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
| HUD mode / declutter / day-night HUD | F2 / V / F4 |
| Maps / ordnance / FLIR | F3 / F5 / F6 |
| Waypoints | F7 / F8 / PgUp / PgDn |
| Mission orders | F10 |
| Tracking camera ahead, rear, right, left | `/` / `.` / `,` / M |
| Designate / next target | N / B |
| Zoom / unzoom / wide angle | Z / X / C |
| Map zoom in / out, north up / heading up (modern panel) | Alt Z / Alt X / Alt F3 |
| Time acceleration / normal | Shift Z / Shift X |
| Pause / detail / sound level | Alt P / Alt D / Alt V |
| Training / resupply / quit | Alt T / Alt R / Alt Q |

The full key list is in [docs/keys.md](docs/keys.md). A gamepad works as stick and throttle as
well (right stick: rudder). The menu's Graphics column has the same toggles as the options above.

**3D cockpit (desktop):** hold the right mouse button to look around, click the panel buttons
(lit panel captions, the MFD bezel buttons, the labelled side buttons, gear lever, ejection
handle) to press them. On the modern panel the bezel buttons select the MFD pages (MAP / TAC,
TC / WAY / WPN / ORD / FLIR), zoom the map (ZM- / ZM+), switch it between north up and heading up
(NUP / HUP) and give page functions (camera direction, waypoints, weapon select). In the
external views the right mouse button orbits the camera around the aircraft.

**VR (`--vr`):** the headset is recentred on entering the flight (left menu button or Home
recentres). Push buttons with your fingertip. Grab the stick (right side) with the grip button:
tilt for pitch and roll, twist for rudder; trigger fires, A gun, B selects the weapon, stick
click designates. Grab the throttle (left side) and slide it forward/back; X flare, Y chaff,
stick click decoy. Grab the yellow handle at the front of the seat and pull up to eject.
On Linux you need the `openxr` loader and a runtime (Monado, WiVRn, SteamVR); build with
`--release`.

## Testing
```sh
cargo test --workspace
tools/headless-run.sh cargo run -q -- --mission 1234 --script "0:ThrottleMax,7:pitch=0.35,13:pitch=0,14:Gear" \
    --log-flight --shot-frames 600 --screenshot screenshots/test.png
```
`tools/headless-run.sh` runs the game in a private headless compositor (sway), so test windows
don't appear on the desktop. `--script` replays timed actions, `--log-flight` / `--log-sim`
print the state, `--time-scale` speeds runs up, and `--diag --novsync` shows the frame rate.
Script steps `yaw=`, `throttle=` and `press=Gear` drive the rudder, an absolute throttle and the
3D cockpit's controls; `--head-look yaw,pitch` turns the 3D cockpit view (degrees).
