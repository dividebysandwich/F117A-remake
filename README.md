# Stealth Fighter

A Rust/Bevy 0.19 remake of DOS-era 3D flight simulations

## Running
```sh
cargo run --release
```
Pick a theater, situation, mission type, opponents and aircraft in the menu. The briefing shows
the generated mission.

Command-line options:

| Option | Effect |
|---|---|
| `--mission <seed> [--theater-index N]` | fly a generated mission directly |
| `--theater ce` / `--fly` | fly from the menu's defaults, skipping it |
| `--modern` | modern rendering touches (smooth gradients and haze, subtle shading, MSAA) |
| `--cockpit3d` | modelled 3D cockpit instead of the original cockpit pictures |
| `--shadows` / `--local-lights` | sun shadows; local lights (cockpit lamps, explosions, flares, missile motors, runway lights) |
| `--vr` | OpenXR headset (implies the 3D cockpit; without a runtime it runs on the desktop) |
| `--viewer [--file stflt --model 24]` | browse the converted models |
| `--freecam [--at Gutersloh\|primary] [--pos x,z]` | free debug camera |

## Controls (original keyboard layout)
Arrows / keypad: stick (tap twice for full deflection, Ins: sensitivity) · `=` `-` throttle,
Shift `=` full, Shift `-` idle · Space: select weapon · Enter: fire · Backspace: cannon ·
1 flare · 2 chaff · 3 IR jammer · 4 ECM · 5 decoy · 6 gear · 7 autopilot · 8 bay doors · 9 flaps ·
0 brakes · Shift F10 eject · F1 cockpit · Shift `/` `.` `,` M look ahead/rear/left/right ·
Shift F1..F6 external views · F2 HUD mode · V declutter · F3 maps · F4 day/night HUD ·
F5 ordnance · F6 FLIR · F7/F8/PgUp/PgDn waypoints · F10 orders · `/` `.` `,` M tracking camera ·
N designate · B next target · Z/X zoom · C wide angle · Shift Z/X time acceleration ·
Alt P pause · Alt D detail · Alt V sound level · Alt T training · Alt R resupply · Alt Q quit.
The full table is in `docs/re/keys.md`. A gamepad works as stick and throttle as well (right
stick: rudder). The menu's Graphics column has the same toggles as the options above.

**3D cockpit (desktop):** hold the right mouse button to look around, click the panel buttons
(lit panel captions, the labelled side buttons, gear lever, ejection handle) to press them.

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
