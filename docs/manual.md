# Stealth Mission 89 – User Manual

This manual describes the basic operation of the game: the main menu, the input mapping, the
cockpit, the displays, targeting, radar detection and the aircraft, air defences and ships in the
theaters. A complete list of keys is in [keys.md](keys.md).

<img src="images/units/ac_f117.png" width="240" alt="F-117A">

The player flies an F-117A or an F-16C (chosen in the menu's *Aircraft* section). Both have four
weapon bays (two with *Realistic weapons*), a 20 mm gun with 650 rounds, chaff, flares, decoys,
an ECM jammer and an IR jammer. The F-16C is faster (Mach 2 class at altitude), pulls 9 g and
rolls about twice as fast, but it has no weapon bay: its stores hang outside, so radars see it as
an F-117A with its bay doors always open (+24 EMV), and the bay door key does nothing. The F-117A
has a modern and a classic cockpit, the F-16C its own.

## Contents

1. [Main menu](#1-main-menu)
2. [Controls menu](#2-controls-menu)
3. [The cockpit](#3-the-cockpit)
4. [MFD pages](#4-mfd-pages)
5. [Targeting and weapons](#5-targeting-and-weapons)
6. [Radar, detection and evasion](#6-radar-detection-and-evasion)
7. [Keyboard shortcuts](#7-keyboard-shortcuts)
8. [Aircraft, air defences and ships](#8-aircraft-air-defences-and-ships)
9. [Mission planner](#9-mission-planner)
10. [Debrief and career](#10-debrief-and-career)
11. [Drone defense (Ukraine)](#11-drone-defense-ukraine)
12. [Ejection and rescue](#12-ejection-and-rescue)
13. [Multiplayer](#13-multiplayer)

---

## 1. Main menu

The main menu chooses what to play; each choice replaces it with its own screen, and BACK (or Esc)
returns to it.

| Button | Function |
|---|---|
| SINGLE MISSION | One mission with the options below. |
| CAREER | Pilot records and the tour of duty, with the tour's options (chapter 10). |
| DYNAMIC CAMPAIGN | Start or continue a dynamic campaign, with the theater, situation, opponents, aircraft, realism and classic / modern options. **Weather**: *changing* (the default) or *clear sky*. Changing weather turns once or twice a day of the campaign's sky, mostly fair (clear, scattered or broken cumulus, sometimes an overcast) and now and then thunderstorms; each change takes a few minutes, the clouds building up or thinning out and the light following. The Ops screen shows the weather and the next change next to the clock, the briefing the cloud layers. |
| MULTIPLAYER | Coop in the dynamic campaign: join a server or host a campaign and fly in it (chapter 13). |
| CONTROLS | The input mapping (chapter 2). |
| QUIT | Exit to the desktop. |

### Single mission

![Single mission](images/menu.png)

The single mission screen generates a mission from the selected options. Every change creates a new
mission, and the summary on the right shows it. Hovering over an option shows a short description
(in the picture, the *Cockpit* switch).

| Section | Options |
|---|---|
| **Theater** | Libya (1986), Persian Gulf (1984), North Cape, Central Europe, Vietnam, Middle East, Desert Storm, Cuba, Korea, Ukraine (2026). |
| **Opponents** | Green, Regular, Veteran, Elite. Sets the skill of enemy pilots and SAM crews; Veteran and Elite add ships and patrols. Higher levels also increase radar sensitivity. |
| **Start time** | The takeoff hour; night from 18:00 to 06:00. *Random*: rolled with each new mission, 06:00-08:00 by day or 18:00-20:00 by night. |
| **Situation** | *Cold War*: covert peacetime operation, fewer enemy aircraft. *Limited War*: regional conflict, air defences on alert. *Conventional War*: full-scale war, all enemy forces active. The briefing states the exact rules of engagement. |
| **Mission** | *Strike*: ground target (destroy, photograph or reach), with a primary and a secondary objective. *Air to air*: intercept an enemy aircraft. *Strike training* and *Air training*: practice missions. *Drone defense* (Ukraine only): stop a wave of attack drones, then bomb their launch site (chapter 11). |
| **Aircraft** | *F-117A* or *F-16C*: the flown aircraft. *Realistic weapons* (F-117A only): two bays instead of four, no AIM-9/AIM-120, no carrier operations. *Crash protection*: automatic pull-up near the ground with the gear up; ground contact and collisions are not fatal. *Tacview export*: each flight is saved as a Tacview recording (chapter 10). *Tacview real-time telemetry*: the flight is shown in Tacview while you fly it (chapter 10). |
| **Realism** | *Damage*: *Game* features simplified weapon damage (hits add up, damaging systems, but never bring the aircraft down). *Real*: a missile hit can destroy the aircraft outright (the engine dies, it burns and breaks up within seconds: eject), otherwise it damages several systems at once and often starts a fire, which either burns out or blows the aircraft up after a while. Hits to the flight controls bend control surfaces: the aircraft rolls or pitches by itself (hold the stick against it) or wobbles. *Guidance*: *Classic* keeps the DOS era missiles. *Real*: missiles fly proportional navigation to the collision point; their motors burn for the real missile's burn time (the smoke trail ends with it) and then they glide, losing speed with every turn and turning ever more slowly as they slow down, especially up high. SAMs climb at a loft angle against distant targets and dive on them from above. The AIM-120 flies straight for 3 s after launch: pull up before firing to lob it for more range. *Low level*: *Real* adds anti-aircraft guns and MANPADS (chapter 8.3). |
| **Weather** | Random (based on the theater's climate), Clear, Scattered, Broken, Overcast, Storm. *Random cover* off: a slider sets the coverage of the main cloud layer. Clouds block optical seekers and visual identification for both sides; radar sees through clouds. |
| **Head/eye tracking** | *Head/Eye Tracking* (desktop): your head turns and moves the view in the cockpit (chapter 3). Below it the menu shows whether a tracker is heard. |
| **Classic / Modern** | *Shaders*: flat DOS era shading or anti-aliasing, shading and haze. *Shadows*: sun shadows. *Lighting*: cockpit lamps, explosions, flares and runway lights light the scene. *Cockpit*: DOS era panel or modern panel (this manual describes the modern panel). *HUD*: the DOS era HUD, or a high-resolution F-16 style HUD in green. *Time of day*: simplified slow change of colours, or the sun and the moon moving with the mission clock: the sun is drawn in the sky (with the modern shaders it glares when you look towards it), the sky turns golden or orange around sunrise and sunset (each dawn and dusk a little different), purple and dark blue about an hour after sunset, stars come out, the moon shows the phase of the briefing date, and moonlight sets how far enemy pilots see you at night (about 2 km on a moonless night, 4 km under a full moon high in the sky, 8 km by day). The sun keeps to the equinox, rising at 06:00 and setting at 18:00, so with a random start time day missions start at dawn and night missions at dusk. *Flight model*: DOS era flight model, or an aerodynamic model with fly-by-wire: the stick commands pitch rate (up to 7 g) and roll rate; released, the nose stays where it was left and, once the flight path has caught up with it, the path is held (also in turns up to 60° of bank). Angle of attack and g limiters only act near the limits. New players start with modern shaders, time of day, cockpit, HUD, avionics and flight model, and classic shadows and lighting. |

The **mission summary** lists theater, situation, mission type, opponents, time of day and
departure time, weather, visibility and fuel. The **briefing** below it lists the objectives, the
departure and landing bases, the weather, the armament and intelligence notes.

The menu remembers its choices and switches for the next start.

| Button | Function |
|---|---|
| BACK | Back to the main menu. |
| NEW MISSION | Generate different objectives with the same options and start time. |
| PLAN ROUTE | Below the mission summary: the mission planner (chapter 9). |
| LOADOUT | Below the mission summary: the stores on each hardpoint, seen from above. Click a bay or pylon and pick its load; *Same on both sides* loads the matching hardpoint on the other wing too. The F-117A has its two bays, and without *Realistic weapons* two more inside the wing roots (amber, not on the real aircraft); the F-16C has stations 1 (left wingtip) to 9 (right wingtip): air-to-air missiles on 1-3 and 7-9, bombs and missiles on 3-4 and 6-7, a fuel tank on 4, 5 or 6, the camera or special equipment on the centreline. Weapons out of stock for the mission are greyed out. BRIEFED restores the briefed stores, CLEAR empties every hardpoint. The briefing's armament line shows the loadout. |
| FLY IN VR | Restart the game in the VR headset with this mission (requires an OpenXR runtime). After the mission the debrief and the menu stay in the headset, on a panel in front of you: point a controller at it, pull the trigger to click, push the thumbstick to scroll; the left menu button puts the panel in front of you again. Starting the game with `--vr` opens the menu in the headset too. |
| FLY | Start the mission. |

## 2. Controls menu

![Input mapping](images/controls.png)

The *Controls* button opens the input mapping. The mapping is saved automatically and applies
to all later flights.

- **Device list**: connected joysticks, throttles and gamepads.
- **Standard gamepad layout**: left stick for pitch and roll, right stick for rudder, triggers
  for throttle, face buttons for fire, gun, weapon select and bay doors.
- **Preview** (top right): an F-117 outline that moves with the mapped axes, with the throttle
  shown as the exhaust colour. If the aircraft moves the wrong way, enable *Invert* for that axis.
- **Axes**: Roll, Pitch, Rudder, Throttle, Sensor/cursor slew X and Y (modern avionics). Click **BIND**, then move the axis. *Invert* reverses
  the direction, *dead zone* sets the neutral range around the centre, and the bar shows the
  current value. **CLEAR** removes the binding.
- **Buttons**: every game action, with its keyboard key for reference. Click **BIND**, then press
  a button. A hat switch or an axis pushed past halfway can also be bound. **Esc** cancels the
  capture.
- **CLEAR ALL** removes all bindings; **DONE** closes the dialog.

![VR controller mapping](images/controls_vr.png)

**VR controllers**: the last section assigns an action to each VR controller input (trigger, A/X,
B/Y, stick click, stick directions) per hand. **DEFAULTS** restores the default assignment. A
trigger only counts while that hand holds the throttle (left) or the stick (right). By default
the right trigger fires, the left trigger holds the wheel brakes (on while pulled, also with the
hand off the throttle unless its fingertip is on a control; any VR button set to *Brakes* works
this way) and the right thumbstick pushed left opens and closes the bay
doors; the left thumbstick slews the displays' cursor.

## 3. The cockpit

![Modern cockpit](images/cockpit.png)

The view shows the head-up display (HUD) at the top and the instrument panel below it. Hold the
**right mouse button** to look around; turn the **mouse wheel** while holding it to zoom in. **Click** a button on the panel to press it. Every button
has a keyboard equivalent (chapter 7).

**Head/eye tracking.** With *Head/Eye Tracking* on in the menu, the cockpit view follows your head
movements, picked up by an infrared tracker (TrackIR, DelanClip, TrackHat, Grass Monkey
Simulations, ...), an eye tracker (Tobii Eye Tracker, Beam Eye Tracker, ...), a webcam or a smartphone. A tracking program such as OpenTrack sends them to the game (FreeTrack output, or UDP on port 4242).
**Home** recentres the view.

For comparison, the **classic cockpit** (menu: *Cockpit: Classic*) uses a DOS era panel with
the 77×60-pixel displays and lit lamps instead of labelled bezel buttons:

![Classic cockpit](images/cockpit_classic.png)

The rest of this manual describes the modern cockpit.

### 3.1 HUD and HUD mode buttons

The modern HUD is drawn in green in the style of the F-16's (in VR the F-117A's symbology is
drawn at half the size, about the F-16C's, so it stays in the middle of the large combiner):

![Modern HUD](images/hud_modern.png)

* **Top:** heading tape (tens of degrees) with the caret of the current steerpoint.
* **Top left:** g load, Mach number and the highest g of the flight.
* **Left / right:** airspeed scale (knots) and altitude scale ("1,5" = 1,500 ft) with the boxed
  readouts; climb rate in feet per minute above the altitude box, radar altitude ("R") below it
  under 5,000 ft.
* **Centre:** gun cross at the boresight and the flight path marker (the circle with wings: where
  the aircraft is going). The pitch ladder is centred on the flight path marker: a rung every
  5°, solid above the horizon, dashed and bent below it. With the gear down, the bracket left of
  the marker shows the approach angle of attack: marker in the middle of the bracket = on speed,
  high = fast, low = slow.
* **Bottom:** bank angle scale; mode, selected weapon and gun rounds (left); steerpoint number,
  distance and time to go (right). Messages such as "*Dessau detected*" appear at the top.

The modes add their own symbols:

* **NAV:** a diamond on the steerpoint on the ground.
* **AA:** the target designator box with the missile diamond once locked, the range below it;
  with an air-to-air missile selected the steering circle and dot, *SHOOT* in range, and the
  launch zone scale (maximum and minimum range, target range caret).
* **AG:** the target designator box (*IN RNG* in range); for guided bombs the azimuth steering
  line with the release cue that slides down to the flight path marker. With a free-fall or
  retarded bomb the bombsight replaces *AG* at the bottom left with its mode, **CCIP** or
  **CCRP** (chapter 5, *Bombing*), and the bottom right shows the slant range in nautical miles
  (*F004.2*) to the impact point (CCIP) or the target (CCRP), the time to release (CCRP,
  *REL 0:12*) and the bomb's time of fall (*TOF*):
  * **CCIP:** the bomb fall line runs down from the flight path marker to the pipper, a small
    circle with a dot where a bomb released now would land. While that point is below the HUD
    the pipper stays at the HUD's edge and the time delay cue crosses the line: dive steeper or
    wait.
  * **CCRP:** the azimuth steering line runs through the target designator box; the solution
    cue (a short bar on the line) slides down towards the flight path marker as the release
    point comes up and reaches it at release.
* **Gun** selected (in every mode): the gun pipper, where the target has to be for the rounds
  fired now to hit it. It allows for the rounds' time of flight to the locked target's range,
  the target's motion relative to the rounds and their drop. Unless an aircraft is locked, when
  the rounds would hit the ground within 4 km the pipper shows where they hit it (put it on the
  ground target), with the range to that point; otherwise without a lock it is set for a
  target 1000 m ahead flying along with you. With a lock, an arc around it unwinds with the
  range (a full circle is 12,000 ft), the range in km is under it and *SHOOT* flashes within
  2 km.

The **classic HUD** (menu: *HUD: Classic*) is a DOS era low-resolution HUD, as in the
classic cockpit picture above.

The HUD mode caps are the first four of the up-front display's top row (chapter 3.2):

| Button | Function |
|---|---|
| NAV | Navigation mode: steering to the current waypoint. |
| AIR | Air-to-air mode: aircraft targets. |
| GND | Air-to-ground mode: ground targets. |
| LOCK | Lamp: blinks while a target is designated, steady when the target is within weapon range. Pressing it designates a target (as N). |

F2 cycles through the modes; V declutters the HUD.

### 3.2 The F-117A's panel

The panel is laid out like the real F-117A's, its upper corners falling steeply outwards under
the canopy's front frames. Under the HUD the **up-front display** stands out of the panel: the
**DED** (data entry display, as the F-16's: the radio channels, the current steerpoint and the
target, the clock, the bearing, distance and ETA to the steerpoint and the fuel) between two
rows of caps. Under it the large **centre display** shows the right MFD's pages (where the real
one shows the infrared picture; chapter 4), between the **left MFD** and the **third MFD**,
right of it, which starts on the **HSI** page (chapter 4.2).
The warning lights are over the side MFDs' inner corners. The F-16's round instruments are down
the outer edges: the airspeed indicator, the altimeter, the AOA tape (the approach band green)
and the vertical velocity tape and the clock (local time) on the left; RPM, FTIT, oil pressure
and nozzle position on the right. Under the left MFD the **attitude indicator** (a blue and
brown ball with a pitch ladder, a bank scale and a small fixed aircraft symbol) and the master
arm panel with the **BAY** cap; under the right MFD the fuel quantity, the fuel and thrust
readouts and the radar bar graph. As in the F-16, sloped panels rise from the consoles to the
main panel: the gear handle and the aircraft systems on the left one, the countermeasures on
the right one. The side stick is on the right console, the throttle on the left one.

| Up-front display, top row | Key | Function |
|---|---|---|
| NAV, AIR, GND, LOCK | F2, N | HUD modes and designation (chapter 3.1). |
| DCLT | V | HUD declutter. |
| NVG | F4 | Night vision goggles. |
| ILS | F9 | Instrument landing system: the landing guidance on the centre display and, approaching a runway, the F-16-style ILS steering bars on the HUD (*ILS* at the bottom left): the localizer bar (upright) and glideslope bar (level) are referenced to the flight path marker; fly the marker to where they cross to be on the centreline and the 3° glide path. Each bar stops at full scale (2.5° off the centreline, 0.7° off the glide path). |

| Up-front display, bottom row | Key | Function |
|---|---|---|
| MENU | Esc | Open or close the in-flight settings menu (chapter 3.8). |
| VOL | Alt V | Sound level. |
| TIME | Shift Z / Shift X | Time acceleration on/off. Acceleration ends automatically when a radar detects the aircraft or a weapon is released. |
| STPT | F7 | Waypoints. |
| ORD | F10 | Mission orders. |
| COM, IFF | – | Not used. |

### 3.3 Warning lights

The lights are dark when inactive. Left: **MASTER CAUTION** (missile warning), **STALL**
(airspeed below stall speed, blinks), **TRACK** (an enemy radar tracks the aircraft), **IR**
(an infrared missile is in flight) and **LAUNCH** (a radar-guided missile is in flight).
Right: **ENG FIRE** (fire on board), **ENGINE** (engine damage), **FLCS** (fire control
damage), **JAM** (jammer damage: ECM and IRJ fail) and **AV** (avionics damage). The caution
lights at the top of the right slope: **FUEL** (fuel system damage), **BD** (bay door damage:
the doors stay open); the others are not used.

A fire on board also sounds the fire warning siren until the fire is out (with realistic damage
a fire burns out or blows the aircraft up; with game damage it burns on, leaking fuel). A damaged aircraft
trails smoke (grey, black when badly hit), and flames while it burns.

### 3.4 Gear, aircraft systems and targeting (left slope)

The gear handle (its arm shows the gear position: down = gear down) with the gear lights left
of it (an aircraft outline with a green lamp at each wheel while the gear is down), the caps
under them, and below them ahead of the throttle the target buttons. The **BAY** cap is under
the left MFD.

| Button | Key | Function |
|---|---|---|
| Gear handle | 6 | Landing gear. |
| FLAPS | 9 | Flaps: extend over about 3 s; more lift and a higher angle of attack limit, about 20 % lower stall speed. |
| BRAKE | 0 | Wheel brakes on the ground, speed brake in the air. |
| AUTO | 7 | Autopilot: flies to the current waypoint. |
| BAY | 8 | Weapon bay doors open/close. Weapons can only be released with open bays. |
| NEAR | T | Lock the nearest target ahead. AIR or GND mode depends on the selected weapon. |
| NEXT | B | Select the next target. |
| NONE | Shift B | Clear the target. No automatic acquisition until the next target or camera key. |
| WPN | Space | Select the next weapon, after the last one the gun. |
| DTL | Alt D | Detail level. |

The selected weapon is released with Enter, the gamepad's A button or the right VR trigger while
the hand holds the stick.

### 3.5 Countermeasures (right slope)

| Control | Key | Function |
|---|---|---|
| CHAFF | 2 | Release chaff. The counter shows the remaining stock; the lamp on the right lights on release. |
| FLARE | 1 | Release a flare. |
| DECOY | 5 | Release a decoy. |
| ECM | 4 | Radar jammer on/off (lit when on). |
| IRJ | 3 | IR jammer on/off (lit when on). Switches itself off after 20 s of continuous use and must cool down. |

### 3.6 Fuel, thrust and the radar bar graph

![Bottom strip](images/panel_bottom.png)

Under the right MFD, right of the fuel quantity: **FUEL** (fuel in lbs divided by 10) with the
**LOW FUEL** lamp (blinks below 1500 lbs of fuel) and **THRUST** (in percent); under them the
**EGR/EAR radar bar graph** (described in chapter 6.2) with the **EMV** warning lamp (lit for
2 s after each detection).

### 3.7 The F-16 cockpit

The F-16C has its own cockpit, laid out like the real Block 50's. Under the HUD the **up-front
controls** stand out of the panel: the round override buttons along the top (**COM1** is the
sound volume, **LIST** the settings menu, **A-A** and **A-G** the master modes; pressing A-A again
returns to NAV, pressing A-G again switches the bombsight between CCIP and CCRP) and the keypad with the real page labels, of which **1 T-ILS** (ILS),
**4 STPT** (the WAY page), **6 TIME** (time acceleration) and **7 MARK** (designate) work, with
**DCLT** (HUD declutter) and **NVG** right of them. The **DED** (data entry display) right of the
up-front controls shows a CNI-style page in amber: the radio channels, the current steerpoint and
the target, the clock, the bearing, distance and ETA to the steerpoint, and the time on target
(or the fuel). Under the up-front controls the flight instruments, as in the real one: the
airspeed indicator and the altimeter at the upper corners of the **attitude indicator** (a blue
and brown ball with a pitch ladder, a bank scale and a small fixed aircraft symbol), the AOA
tape (the approach band green) left of it and the vertical velocity tape right of it, the
**HSI** (heading, bearing pointer and distance to the current waypoint) under it. The MFDs are
left and right of them, each in a raised bezel with twenty buttons (the top and bottom rows
work) and rocker switches at its corners. Top left the master caution light under the glare
shield with the stall light and the game's TRACK (a radar tracks the aircraft) and IR (an
infrared guided missile is in the air) lights under it, the threat warning prime buttons, whose
**LAUNCH** light comes on when a radar guided missile is in the air, the **RWR** scope next to
the up-front controls, and the miscellaneous panel's switches. Top right
the warning lights (ENG FIRE, ENGINE, FLCS) and down the right edge the engine instruments (oil
pressure, nozzle position, RPM, FTIT) and the fuel quantity. The other warning lights are on the
caution light panel on the right console, ahead of the stick. Sloped panels rise from the
consoles to the main panel: the clock (local time) and the countermeasure panel (chaff, flare
and decoy with their counts, ECM and IRJ) are on the right one; on the left one the gear handle
with the gear lights left of it (an aircraft outline with a green lamp at each wheel while the
gear is down), the **FLAPS**, **BRAKE** and **AUTO** (autopilot) caps under them and, below
them ahead of the throttle, the target buttons (NEAR, NEXT, NONE, WPN, DTL). The side stick is
on the right console, the throttle on the left one.

The **AOA indexer** left of the HUD lights its symbols for the angle of attack, as the real one:
the red top chevron when the aircraft is slow (two or more degrees above the approach angle of
attack, the middle of the HUD's bracket), the green circle on speed, the amber bottom chevron
when fast; the circle and a chevron together in between.

It has no radar bar graph. The **RWR** shows every radar that can see the aircraft, at its bearing
relative to the nose (up = ahead). Like a real RWR it does not show range: the closer a symbol is to
the centre, the more dangerous the radar (a launching or tracking SAM innermost, a radar only
searching near the rim). Symbols: the SAM type (`2` SA-2, `10` SA-10, `N6` SA-N-6, `HK` Hawk,
`RA` Rapier, ...), aircraft radars with a hat over their type (`29` MiG-29, `AW` AWACS); dim =
searching, bright = tracking, a diamond marks the most dangerous one, a flashing circle a missile in
the air.

### 3.8 In-flight settings menu

**Esc** (also Pause, Alt P, the gamepad's Start button, the **MENU** cap of the F-117A's up-front
display or the F-16C's **LIST** key) opens the settings menu and pauses the flight: in VR a screen in front of the pilot, on
the desktop over the middle of the window. In VR, touch an
entry with a fingertip and pull the trigger, or use a thumbstick and A / X. On the desktop, click
an entry or use the arrow keys and Enter (gamepad: d-pad and A, B closes).

| Entry | Function |
|---|---|
| Shaders | Modern (anti-aliasing, shading, haze) or classic flat shading. |
| Shadows | Sun shadows on / off. |
| Point lights | Cockpit lamps, explosions, flares and runway lights lighting the scene. |
| Time of day | Modern (the sun and the moon move with the mission clock) or classic (chapter 1). |
| Cockpit | Modern or classic panel (F-117A only). |
| HUD | Modern (F-16 style) or classic HUD. |
| Avionics | Modern (F-16 style MFD formats, chapter 4.1) or classic MFD pages. |
| Cockpit size | VR only: size of the cockpit in the headset (70–150 %; at the default 100 % the F-117A's cockpit looks 60 % and the F-16C's 87 % of life size, which felt right); left / right or the < > halves. The world looks smaller by as much, so in the F-117A the cockpit is raised in VR until the runway looks as far below as from the real one (2.7 m). |
| Resume | Closes the menu (as does Esc again). |
| End mission | Ends the flight and goes to the debrief. |

Changes apply at once and are saved as the defaults for the next flight.

### 3.9 Kneeboard

A kneeboard lies on your left thigh: look down to read it. Its tabs choose the page: **BRIEF**
(the orders, the objectives, departure and landing, the weather, the moon's phase, the armament
and the rules of engagement), **ROUTE** (each leg's bearing, distance and the time at 480 kt, the
current waypoint highlighted), **MAP** (the route on the theater map with the radar sites' missile
ranges and your position) and **TARGET** (a close-up of the primary target's surroundings with a
2 km scale and its grid reference, and the intelligence summary). In VR touch a tab with a
fingertip; on the desktop click one. **K** shows the kneeboard large over the view on the desktop
and turns its pages, the last one hides it again; in VR the *Kneeboard* action (bind it to a
controller button in the input mapping) hides or shows the board. In VR you can also squeeze the
grip with a hand on the board to pick it up and put it anywhere in the cockpit: it stays floating
where you let go (until the next flight).

## 4. MFD pages

The panel has two multi-function displays (MFDs), the F-117A's three: its right MFD is the large
centre display, its third MFD right of that shows the HSI (chapter 4.2; with the modern avionics
any format). With the **classic avionics** (menu: Classic /
Modern, Avionics) they show the low resolution pages described here; the **modern avionics** are in
chapter 4.1. The labelled buttons above and below each screen select the pages and their
functions. The button of the active page is lit.

| Left MFD buttons | Function |
|---|---|
| MAP / TAC | Select the page (key F3 toggles). |
| NUP / HUP | Map orientation: north up or heading up (Alt F3). |
| ZM- / ZM+ | Zoom out / in (Alt X / Alt Z). |

| Right MFD buttons | Function |
|---|---|
| TC | Tracking camera. |
| WAY | Waypoints (F7). |
| WPN | Weapons (F5). |
| ORD | Mission orders (F10). |
| FLIR | Thermal camera on/off (F6). |
| Lower row | Page functions, see below. |

<table>
<tr>
<td width="50%"><img src="images/mfd_map.png" alt="MAP"><br>
<b>MAP</b> – Top-down map of the theater with the own aircraft (triangle), the route and
waypoints, airbases and ground objects. A scale bar is shown at the bottom left. During a radar
sweep, the map shows the radar's coverage (chapter 6.3).</td>
<td width="50%"><img src="images/mfd_tac.png" alt="TAC"><br>
<b>TAC</b> – Heading-up tactical display with a grid (spacing shown at the bottom). Own aircraft
in blue. Other aircraft are shown as triangles pointing in their direction of flight: red at
the same altitude (±1000 ft), dark red below, yellow above. Missiles are shown as short lines;
radar sites, airbases and targets as icons.</td>
</tr>
<tr>
<td><img src="images/mfd_tc.png" alt="TC"><br>
<b>TC</b> – Tracking camera. Shows the current target with its name, range, bearing and
zoom factor. AHD / RGT / REAR / LFT (keys / , . M) point the camera ahead, right, rear or left;
each press also picks the nearest object in that direction. The last bottom button sets the zoom
(AUTO by range, then 1X..6X; also `[` / `]`).</td>
<td><img src="images/mfd_way.png" alt="WAY"><br>
<b>WAY</b> – Mission time and waypoint table: distance (km), flight time (minutes) and ETA for
each waypoint; fuel bar at the bottom. WP- / WP+ (PgUp / PgDn) select the waypoint, RST
(Shift F8) resets it to its original position.</td>
</tr>
<tr>
<td><img src="images/mfd_wpn.png" alt="WPN"><br>
<b>WPN</b> – The weapons carried and how many are left; the selected one is highlighted.
SEL (Space) selects the next weapon, after the last one the gun; the button next to it shows
the bombsight mode, CCIP or CCRP, and switches it (Alt F2).</td>
<td><img src="images/mfd_ord.png" alt="ORD"><br>
<b>ORD</b> – Mission orders: primary objective (yellow), secondary objective (red), landing
base (green).</td>
</tr>
<tr>
<td><img src="images/mfd_flir.png" alt="FLIR"><br>
<b>FLIR</b> – The tracking camera as a black-and-white thermal picture: aircraft, vehicles and
buildings appear bright.</td>
<td></td>
</tr>
</table>

### 4.1 Modern avionics

With the modern avionics each MFD can show any of five F-16 style formats, chosen with the
bottom row of bezel buttons: **HSD**, **HARM**, **TGP**, **MAP** and **SMS**; the F-117A's
third MFD offers **HSI** in place of TGP. The buttons' functions are written on the display next
to them (lit = selected); the bezel caps are blank. Choosing a format another MFD shows swaps
the two. F3 cycles the left MFD through HSD, MAP
and HARM; F5 / F10 bring up SMS, F6 and the camera keys the TGP, F7 the waypoint table.

| Format | Top row buttons | Shows |
|---|---|---|
| HSD | DEP / CEN, RNG-, range, RNG+, SOI | Horizontal situation display, heading up: range rings (the range in km is the outer ring), the route with the current steerpoint ringed, SAM sites with their missile's reach as red threat rings, hostile (red) and friendly (green) aircraft, ships (hull symbols, red or green; armed ones with their threat ring), missiles, the mission targets (cyan diamonds); the locked target is boxed. |
| HARM | ALL / SAM / AIR, SOI | The radars painting the aircraft, like the RWR: at their bearing, the most dangerous nearest the centre (chapter 3.7). |
| TGP | FLIR, ZM-, ZM+, CZ, SOI | The targeting pod (tracking camera), see below. The pod's mode (SLV / AREA / PNT) is shown bottom left, the zoom bottom right (AUTO: by range). |
| MAP | NUP / HUP, ZM-, ZM+, WAY | The moving map in muted colours; WAY switches to the waypoint table (WP-, WP+, RST). |
| SMS | WPN, ORD, SEL, CCIP / CCRP | Stores (WPN) and mission orders (ORD); on WPN the bombsight mode. |
| HSI | WP-, WP+ | Horizontal situation indicator (chapter 4.2). |

**Sensor of interest (SOI).** The HSD, the HARM display or the TGP is the SOI (white frame; the top right
SOI button, or Alt S to move it to the next MFD). Its cursor, two short vertical lines, moves with
the **sensor/cursor slew** controls: the keypad (8 / 2 / 4 / 6 and diagonals), the left VR
thumbstick, or two joystick axes bound in the CONTROLS dialog ("Sensor/cursor slew X/Y"). **Lock
cursor target** (keypad Enter, left thumbstick click, or a bound button) locks what is under the
cursor: an aircraft on the HSD (the HUD goes to A-A), a ground object, ship or SAM site (A-G), or a
radar on the HARM display. Held down on the HSD or HARM display, it opens a window magnified four
times around the cursor, in the corner away from it, and the cursor slews four times slower; letting
go locks what is right under the cursor in the window. The lock holds until the next target key (B, T, N, Shift B or the
camera keys). Z / X change the HSD range while it is the SOI. The HSD cursor stays on its ground
point as the aircraft flies, turns or the range changes, and the HARM cursor turns with the radar
symbols, so a turn doesn't take the cursor off its target.

**Targeting pod.** The TGP follows the locked target (SLV); without one it looks ahead and down,
or where the camera keys turn it. Made the SOI, the slew controls turn the pod itself (AREA, slower
when zoomed in); a moment after the slew stops it holds the ground point it looks at (PNT), wherever
the aircraft flies. **Lock cursor target** then locks what is at that point (the ground itself if
nothing is there) and the pod follows the lock again. CZ returns it to SLV looking ahead (until
something new is locked, so the next slew starts from straight ahead) and the zoom to AUTO.
ZM- / ZM+ (also `[` / `]`, and Z / X while the pod is the SOI) zoom it from 1X to 6X. The HSD shows where
the pod looks: a green diamond on its ground point with a dashed line from the aircraft.

### 4.2 The HSI page (F-117A third MFD)

A heading-up compass rose with the heading in a box over it. The green **course arrow** lies
along the leg to the current steerpoint (from the previous one; on the way to the first one,
the direct course); its middle part, the **deviation bar**, moves off the arrow towards the
course when the aircraft is beside it: one dot 2.5 km, two dots (full scale) 5 km. The small
green triangle beside the centre points along the arrow while the steerpoint is ahead (TO), the
other way once it is passed (FROM). The cyan **bearing pointer** points at the steerpoint.
Left: the steerpoint number, its distance (km) and the ETA; right: its bearing, the course and
the ground speed (knots). WP- / WP+ select the previous / next steerpoint.

## 5. Targeting and weapons

![Ground target locked](images/targeting.png)

The picture shows a ground target locked in GND mode: the HUD box marks the target, the TC page
shows the target (here *Gutersloh Airbase*, 0.9 km), and the GND lamp is lit.

**Procedure**

1. Select the weapon with **WPN** (Space). The HUD shows it at the bottom left.
2. Lock a target: **NEAR** (T) for the nearest target ahead, **DESG** (N) to designate,
   **NEXT** (B) to step through targets. The camera keys (/ . , M) also select targets in
   their direction.
3. Open the bay doors (**BAY**, 8).
4. Release with Enter (gamepad A, VR: right trigger) when **LOCK** is steady. Releases fail with *Bay closed*,
   with a bank angle above about 67°, or on the ground (*Insufficient Altitude*).
5. Close the bay doors. Open doors increase the radar signature (chapter 6.1).

**Bombing:** free-fall and retarded bombs have two release modes; CCIP is the default. Switch
with Alt F2, the F-16's **A-G** button pressed again in AG mode, or the CCIP / CCRP button on the
weapons page. The HUD shows the mode at the bottom left (in AG mode).

* **CCIP** (continuously computed impact point): put the pipper at the end of the bomb fall line
  on the target and release with Enter. Free-fall bombs need a dive or some height to bring the
  pipper into the HUD; low and level, retarded bombs do.
* **CCRP** (continuously computed release point): lock the target (NEAR, DESG, NEXT or a display
  lock), steer the flight path marker onto the azimuth steering line and hold Enter (or the
  trigger): the bomb is released by itself when the solution cue reaches the flight path marker,
  i.e. when the target passes the release point. Pressing Enter does not release the bomb in
  CCRP; without a target the HUD says *No target designated*. Let go of the button to hold
  the release.

**Gun:** select it with **WPN** (Space) after the last weapon (the HUD shows *GUN* and the
rounds left) and hold the fire button (Enter, gamepad A, VR: right trigger): put the target in
the gun pipper. Backspace fires the gun whatever is selected. The gun fires for as long as the
trigger is held (until the rounds run out). The rounds leave along the nose at 1050 m/s plus
your airspeed and drop under gravity; they show as bright yellow tracer lines (enemy rounds in
red), and where they hit the ground small yellow flashes and sparks. The real F-117A has no gun: with
*Realistic weapons* there is none.

**Weapon view:** Shift F4 follows your newest missile or bomb in flight from behind (above a
falling bomb); pressed again it switches to your next weapon in flight. After the impact the camera
stays on the explosion and slowly pulls back for 10 seconds, then returns to the view you came
from. The right mouse button orbits around the weapon and the mouse wheel zooms.

Ground targets under cloud cannot be designated (*Target obscured*). IR and optical seekers
(AIM-9, Maverick, laser-guided bombs) need a clear line of sight.

**Player weapons** (lock range in km):

| Weapon | Type | Range |
|---|---|---|
| AIM-9M Sidewinder | IR air-to-air missile | 17 |
| AIM-120A AMRAAM | Radar air-to-air missile | 32 |
| AGM-88A HARM | Anti-radiation missile, only against radar sites | 20 |
| Penguin 3 / AGM-84A Harpoon | Anti-ship missiles, only against ships | 32 / 60 |
| AGM-65D Maverick | Optical air-to-ground missile | 32 |
| GBU-12 Paveway, Mk 20 Rockeye II, CBU-72 FAE | Guided bombs | 10 |
| Durandal, Mk 82-1 Snakeye, Mk 20 Rockeye, Mk 35, Minelets | Retarded / cluster bombs | – |
| Mk 82-0 Slick, Mk 122 Fireye | Free-fall bombs | – |
| 135 mm camera | Reconnaissance photos of the target | – |
| Extra fuel | 1900 lbs, transferred on release | – |

**Unguided bombs** leave with the aircraft's speed and direction and fall under gravity and
air drag. Free-fall bombs keep most of their speed and land far ahead of the release point
(about 4,000 ft ahead from 500 ft at 450 knots, 6 seconds later). Retarded and cluster bombs
open fins or a chute: they lose their speed within seconds and fall on a short, steep arc
(about 2,500 ft ahead, 7 seconds), landing well behind the aircraft, which is clear of the
blast by then. Throwing a bomb in a
climb (loft) makes it fly farther. The CCIP pipper always shows where the selected bomb would
land if released now.

## 6. Radar, detection and evasion

### 6.1 How detection works

Each radar sweep produces a **signal** against the aircraft. The aircraft itself has an
electromagnetic visibility (**EMV**). The aircraft is detected when **signal + EMV exceeds 100**.

**EMV** increases with:
- altitude (higher = larger),
- speed (500 kt adds about 13),
- bank angle (90° bank adds 32),
- open bay doors (+24),
- ECM switched on (+16).

**Radar signal** depends on:
- **Distance**: the signal decreases linearly with distance and reaches zero at the radar's
  maximum range.
- **Terrain under the aircraft**: open sea gives the strongest signal, normal land less, hills
  half of that. Over **mountains** no radar can detect the aircraft.
- **Height above the radar**: an aircraft high above a nearby radar is in the blind cone above
  it. Example: at 10,000 ft above the site, radars closer than about 10 km cannot see the aircraft.
- **Aspect and radar type** (see 6.4).
- **Difficulty**: higher opponent levels make radars more sensitive.

**Consequences of detection**: the site's alert level rises, the RWR tone sounds, the EMV lamp
lights and time acceleration stops. A site that stays alerted raises a theater-wide alarm, starts
tracking (**TRAK** lamp) and launches missiles when the aircraft is within its missile's lock
range. After repeated detections the HUD reports "*Radar ID*". Signals below the threshold only
produce a short tone, and the site's alert level decreases with each sweep that does not detect
the aircraft.

Enemy fighters that detect the aircraft by radar can identify it visually within about 8 km by
day and 4 km by night (not through clouds).

### 6.2 The radar bar graph (F-117A)

![Radar bar graph](images/panel_bottom.png)

The bar graph has two strips of LED segments:
- **EGR** (upper strip): signal of the last **ground** radar sweep.
- **EAR** (lower strip): signal of the last **airborne** radar sweep.

The radar signal lights segments from the **left in yellow**. The own **EMV** lights segments from
the **right in green**. Segments lit by both turn **red**: the two values add up to more than 100
and the **aircraft is detected**. A gap between the yellow and the green segments is the remaining
margin.

In the picture, an airborne radar (lower strip, yellow) is sweeping the aircraft; the EMV (green)
is low and the bars do not meet, so the aircraft is not detected.

The classic cockpit shows the same values in a single gauge: the EMV as a red bar from the right
(the blue remainder is the margin), with the EGR and EAR signal bars growing from the left over it.

### 6.3 Radar coverage on the map

![Radar coverage on the MAP page](images/radar_map.png)

During a sweep, the MAP page draws the radar's coverage as a circle around the radar. Its
radius is the distance at which that radar detects the aircraft **with its current EMV**. If
the own aircraft symbol is inside the circle, the aircraft is detected. A searching radar
shows a full circle; an alerted ground site shows a 90° sector pointing towards the aircraft.

Colours: ground radars dark red (weak), red (strong), yellow (detected); airborne radars cyan
(not detected) or white (detected). The picture shows an airborne radar (cyan circle); the
aircraft (white triangle, left) is outside the circle and not detected.

### 6.4 Radar types

| Type | Users | Strongest signal | Weakest signal |
|---|---|---|---|
| **Pulse radar** | SA-2, SA-5, SA-8B, SA-11, SA-13, SA-N-5, SA-N-7, Rapier, Tiger, Seacat; MiG-17, MiG-21, MiG-23, MiG-25, F-4E, F-5, Yak-38 | Aircraft seen from the side (beam aspect) | Aircraft flying directly towards or away from the radar |
| **Doppler radar** | SA-10, SA-12, SA-N-4, SA-N-6, Hawk, OTH radar; MiG-29, MiG-31, Su-27, F-14, Mirage F-1, IL-76 | Aircraft flying directly towards or away from the radar, at high speed | Aircraft flying at right angles to the radar (beam), at low speed |

Ground radars sweep all around. Fighter radars only see within about 67° either side of the
fighter's nose. The IL-76 airborne radar sees all around.

### 6.5 Evading radar

- **Keep EMV low**: fly low, at moderate speed and with small bank angles. Keep the bay doors
  closed until just before release. Switch ECM on only against missiles in flight, because it
  raises EMV.
- **Use the terrain**: fly over mountains and hills. Avoid flying over open sea near radars.
- **Fly the right aspect**: point the nose towards or away from **pulse** radars. Fly at right
  angles to **Doppler** radars and reduce speed.
- **Watch the bar graph**: if the yellow and green bars approach each other, change heading,
  descend or slow down.
- **Watch the map**: keep the aircraft symbol outside the coverage circles. Stay out of the
  forward sector of enemy fighters.
- **Keep distance**: the signal drops with range. A site out of missile lock range can detect
  but not engage.

### 6.6 Defeating missiles

The RAD and IR lamps show which kind of missile is in flight; MW warns of a missile.

| Missile guidance | Examples | Countermeasures |
|---|---|---|
| Radar command guidance | SA-2, SA-5, Tiger, Seacat | Chaff; ECM (always effective). |
| Semi-active radar | SA-8B, SA-11, SA-N-4, SA-N-7, Hawk, Rapier, AA-6, AA-7, AA-9, AIM-7 | Chaff; ECM beyond a minimum distance. |
| Active radar | SA-10, SA-12, SA-N-6, AA-10 (MiG-29/IL-76 in Central Europe, North Cape, Ukraine) | ECM beyond a minimum distance; chaff only while flying at right angles to the missile. |
| Infrared, all-aspect | AA-2, AA-6 (IR), SA-N-5 | Flares; IR jammer (always effective). |
| Infrared, rear-aspect (can only lock on from behind the aircraft) | SA-13, AA-8, AA-10 (IR), AIM-9W | Flares; IR jammer beyond a minimum distance. |

The minimum distance for ECM and IR jammer is about 6 km (Green), 10 km (Regular), 14 km
(Veteran) and 18 km (Elite). A **decoy** draws missiles and the alarm position away from the
aircraft while it is active. A radar-guided missile more than about 8 km from the aircraft loses
guidance when its launcher stops tracking the aircraft. Bringing the signal + EMV below 100
therefore also defeats missiles at long range.

With realistic guidance a missile's energy is the other way out: once its smoke trail ends the
motor has burnt out, and a hard turn away (or a dive into thicker air while it is high up) makes
it bleed speed until it can no longer follow.

## 7. Keyboard shortcuts

The input mapping (chapter 2) shows all keys and can bind them to joystick buttons.

| Group | Action | Key |
|---|---|---|
| **Flight** | Stick (pitch / roll) | Arrow keys (rebindable); double-tap for full deflection |
| | Rudder (yaw) left / right | Unbound by default; bind keys in the input mapping |
| | Stick sensitivity | Ins |
| | Throttle up / down | `=` / `-` |
| | Full / idle throttle | Shift `=` / Shift `-` |
| | Gear / autopilot / bay doors / flaps / brakes | 6 / 7 / 8 / 9 / 0 |
| | Eject | Shift F10 |
| **Weapons** | Select weapon / fire / gun | Space / Enter / Backspace |
| | Flare / chaff / IR jammer / ECM / decoy | 1 / 2 / 3 / 4 / 5 |
| **Targeting** | Nearest target ahead | T |
| | Slew the SOI's cursor / lock the target under it / SOI to the other MFD (modern avionics) | keypad 1–9 / keypad Enter / Alt S |
| | Designate new target / next target / clear target | N / B / Shift B |
| | Tracking camera ahead / rear / right / left | `/` / `.` / `,` / M |
| | Tracking camera zoom in / out | `]` / `[` |
| **Displays** | HUD mode / declutter | F2 / V |
| | Bombsight CCIP / CCRP | Alt F2 |
| | MAP / TAC (left MFD) | F3 |
| | Map zoom in / out, north up / heading up | Alt Z / Alt X / Alt F3 |
| | Night vision goggles | F4 |
| | Weapons page / FLIR / ILS / mission orders | F5 / F6 / F9 / F10 |
| **Navigation** | Waypoint page / change waypoint / reset waypoint | F7 / F8 / Shift F8 |
| | Kneeboard: show, next page, hide (chapter 3.9) | K |
| | Previous / next waypoint | PgUp / PgDn |
| **Views** | Cockpit | F1 |
| | Look ahead / rear / left / right | Shift `/` / Shift `.` / Shift `,` / Shift M |
| | Slot / chase / side / weapon / tactical / inverse tactical | Shift F1 … Shift F6 |
| | Zoom / unzoom / wide angle | Z / X / C |
| | Look around (cockpit) / orbit (external views) | Right mouse button |
| | Zoom in and out (cockpit: up to 6x; external views: the camera distance) | Mouse wheel with the right mouse button held |
| | Recentre head tracking (chapter 3) | Home |
| **Time** | Time acceleration / normal | Shift Z / Shift X |
| **Game** | Settings menu (pauses) / detail / sound level | Esc, Pause, Alt P, gamepad Start / Alt D / Alt V |
| | Training / resupply / quit | Alt T / Alt R / Alt Q |

## 8. Aircraft, air defences and ships

Ranges are the values the game uses, in km. **Radar range** is the distance at which the radar
signal reaches zero; **lock range** is the distance within which a missile is launched.
Theaters: LB Libya, PG Persian Gulf, NC North Cape, CE Central Europe, VN Vietnam, ME Middle
East, DS Desert Storm, CU Cuba, KO Korea, UA Ukraine. Enemy fighters also use their guns at
close range.

### 8.1 Aircraft

| | Aircraft | Description |
|---|---|---|
| <img src="images/units/ac_mig17.png" width="200"> | **MiG-17 Fresco** | Older fighter. Pulse radar, 50 km. Armament: AA-2 (IR, 14 km) in DS; guns only in CU and KO. |
| <img src="images/units/ac_mig21.png" width="200"> | **MiG-21 Fishbed** | Light fighter. Pulse radar, 40 km. Armament: AA-2 (IR, 14 km); AA-6 (IR, 50 km) in DS. Theaters: CU, KO, DS. |
| <img src="images/units/ac_mig23.png" width="200"> | **MiG-23 Flogger** | Fighter, the most common type. Pulse radar, 50 km. Armament: AA-2 (IR, 14 km) in CU, KO, DS, LB, ME; AA-8 (IR rear-aspect, 12 km) in CE; AA-6 (radar or IR, 50 km) in VN. |
| <img src="images/units/ac_mig25.png" width="200"> | **MiG-25 Foxbat / MiG-31 Foxhound** | Interceptors (same model). MiG-25: pulse radar 70 km, AA-6 (semi-active radar or IR, 50 km); CE, LB, NC. MiG-31: Doppler radar 100 km, AA-9 (semi-active radar, 82 km) in CE, NC, UA; AA-6 in ME, UA. |
| <img src="images/units/ac_mig29.png" width="200"> | **MiG-29 Fulcrum** | Fighter. Doppler radar, 80 km. Armament: AA-10 (active radar, 64 km) in CE, NC; AA-10 (IR rear-aspect, 64 km) in CU, KO, DS, LB, ME. |
| <img src="images/units/ac_su27.png" width="200"> | **Su-27 Flanker** | Fighter. Doppler radar, 80 km. Armament: AA-10 (IR rear-aspect, 64 km). Theaters: CE, KO, NC, UA. |
| <img src="images/units/ac_su27.png" width="200"> | **Su-34 Fullback / Su-35 Flanker-E** | Ukraine's strike fighter and fighter (drawn with the Su-27 model). Su-34: Doppler radar 80 km, AA-8 (IR rear-aspect, 12 km). Su-35: Doppler radar 100 km, AA-10 (active radar, 64 km). Theater: UA. |
| <img src="images/units/ac_yak38.png" width="200"> | **Yak-38 Forger** | Carrier-based fighter. Pulse radar, 50 km. Armament: AA-2 (IR, 14 km) in VN; AA-8 (IR rear-aspect, 12 km) in NC. |
| <img src="images/units/ac_mirage_f1.png" width="200"> | **Mirage F-1** | Fighter (Iraq). Doppler radar, 100 km. Armament: AA-6 (semi-active radar, 50 km). Theater: DS. |
| <img src="images/units/ac_f5.png" width="200"> | **F-5 Tiger** | Light fighter. Pulse radar, 40 km. Armament: AA-2 (IR, 14 km) in ME; AIM-7 (semi-active radar, 44 km) in PG. |
| <img src="images/units/ac_f4e.png" width="200"> | **F-4E Phantom** | Fighter. Enemy in PG: pulse radar 70 km, AIM-9 (IR rear-aspect, 12 km). Friendly and unarmed in CE, CU, KO, NC, UA, VN. |
| <img src="images/units/ac_f14.png" width="200"> | **F-14 Tomcat** | Fighter (Iran). Doppler radar, 80 km. Armament: AIM-9 (IR rear-aspect, 12 km). Theater: PG. |
| <img src="images/units/ac_il76.png" width="200"> | **IL-76 Mainstay** | Airborne early warning aircraft. Doppler radar, 200 km, sees all around. Armament listed: AA-10 (active radar, 64 km). Theaters: CE, NC, UA. |
| <img src="images/units/ac_an72.png" width="200"> | **An-72 Coaler** | Transport. No radar, unarmed. All theaters; a typical air-to-air mission target. |
| <img src="images/units/ac_uh60.png" width="200"> | **Mi-24 Hind** | Attack helicopter (drawn with the UH-60 model). Flies low, hovers, guns a low and slow player at close range; hunts a downed pilot (chapter 12). |
| <img src="images/units/ac_uh60.png" width="200"> | **UH-60 Black Hawk** | Friendly rescue helicopter (chapter 12). |
| <img src="images/units/ac_geran2.png" width="200"> | **Geran-2** (Shahed-136) | Attack drone (Ukraine). Piston engine and pusher propeller, about 185 km/h, low. No radar, unarmed: it flies straight at its target and dives into it. Shown on the TAC display as an open delta. |
| <img src="images/units/ac_geran3.png" width="200"> | **Geran-3** (Shahed-238) | The jet version: about 600 km/h, flies higher. Otherwise like the Geran-2. |
| <img src="images/units/ac_f18.png" width="200"> | **F/A-18 Hornet** | Friendly fighter. No threat. LB, ME, NC, PG, CE, UA. |
| <img src="images/units/ac_f15.png" width="200"> | **F-15 Eagle** | Friendly fighter. No threat. DS. |
| <img src="images/units/ac_a10.png" width="200"> | **A-10 Thunderbolt II** | Friendly ground attack aircraft. No threat. DS. |

### 8.2 Air defences

SAM sites appear on the map as radar sites. The site model differs by theater; the SAM type
determines radar and missile:

| CE / UA | LB | PG | KO | VN | DS |
|---|---|---|---|---|---|
| <img src="images/units/sam_ce.png" width="150"> | <img src="images/units/sam_lb.png" width="150"> | <img src="images/units/sam_pg.png" width="150"> | <img src="images/units/sam_ko.png" width="150"> | <img src="images/units/sam_vn.png" width="150"> | <img src="images/units/sam_ku.png" width="150"> |

| SAM | Radar | Radar range | Missile guidance | Lock range | Theaters |
|---|---|---|---|---|---|
| **SA-2** | Pulse | 200 | Radar command | 125 | CU, KO, DS, LB, ME, VN |
| **SA-5** | Pulse | 350 | Radar command | 150 | CE, CU, KO, DS, LB, ME, NC, VN |
| **SA-8B** | Pulse | 125 | Semi-active radar | 65 | KO |
| **SA-10** | Doppler | 320 | Active radar | 125 | CE, KO, NC, UA |
| **SA-11** | Pulse | 200 | Semi-active radar | 100 | CE |
| **SA-12** | Doppler | 250 | Active radar | 125 | CE, KO |
| **SA-13** | Pulse | 125 | IR (rear-aspect) | 65 | CE |
| **SA-21** (S-400) | Doppler | 400 | Active radar | 200 | UA |
| **SA-17** (Buk-M2) | Doppler | 250 | Semi-active radar | 110 | UA |
| **SA-22** (Pantsir) | Doppler | 130 | Semi-active radar | 60 | UA |
| **SA-15** (Tor-M2) | Doppler | 125 | Semi-active radar | 50 | UA |
| **Hawk** | Doppler | 175 | Semi-active radar | 125 | PG |
| **Rapier** | Pulse | 75 | Semi-active radar | 65 | PG |
| **Tiger** | Pulse | 65 | Radar command | 30 | PG |

| | Radar | Description |
|---|---|---|
| <img src="images/units/oth_ce.png" width="200"> | **OTH / LPAR radar** | Over-the-horizon and large phased-array early warning radars. Doppler, 500 km. No missiles; a detection raises the theater-wide alarm. CE, UA (OTH), NC (LPAR). |

### 8.3 Low-level air defences

With *Low level* set to *Real* in the menu's Realism section (on for new players), guns and
shoulder-launched missiles defend the enemy's airfields, the targets and the towns.

| Unit | Where | Sees you | Reach |
|---|---|---|---|
| **ZSU-23-4 Shilka** | 1-3 around each enemy airfield and target | Its Gun Dish radar (on the RWR as **A**: your EMV counts as against a SAM), or by eye | 2.5 km, below 2000 m. Fires bursts of 23 mm tracers |
| **S-60** | Batteries of 2-4 guns at airfields and targets (not in a cold war) | By eye only | 6 km, below 4500 m. Its 57 mm shells burst in black puffs around you |
| **MANPADS team** | In and around the towns | By eye only | 5 km, below 3500 m. Fires an SA-14 (IR) at most once a minute; no radar warning |

Seeing by eye takes daylight or moonlight (about 8 km by day, 4 km under a full moon, 2 km on a
moonless night with the *Time of day* switch on) and a clear line of sight through the clouds.
Bombs, missiles and the gun destroy the guns and the teams; a HARM fired at a Shilka's radar on
the HARM page flies at the spot where it stands.

### 8.4 Ships

Ships stand out on the open sea: they are drawn from about 25 km away and keep their detailed
models further out than other objects. Every ship shows on the HSD, where the cursor can lock it.

| | Ship | Description |
|---|---|---|
| <img src="images/units/ship_missileboat.png" width="200"> | **Missile boat / Patrol boat** | Armed with ship-based SAMs. Missile boat: SA-N-4 (Doppler 200 km, semi-active radar, 30 km), SA-N-5 (pulse 150 km, IR, 30 km) in LB, CU, KO; SA-N-6 (Doppler 320 km, active radar, 125 km) in KO; Seacat (pulse 200 km, radar command, 30 km) in PG. Patrol boat: SA-N-5, SA-N-7 (pulse 200 km, semi-active radar, 100 km) in CE, UA. |
| <img src="images/units/ship_destroyer.png" width="200"> | **Destroyer** | Warship. LB, PG, NC, VN, ME, DS, CU, KO. |
| <img src="images/units/ship_krivak.png" width="200"> | **Krivak** | Soviet frigate. NC. |
| <img src="images/units/ship_aegis.png" width="200"> | **Aegis cruiser** | Cruiser. PG. |
| <img src="images/units/ship_typhoon.png" width="200"> | **Typhoon** | Soviet ballistic missile submarine (surfaced). NC. |
| <img src="images/units/ship_tanker.png" width="200"> | **Oil tanker** | Tanker. LB, PG, CU. |
| <img src="images/units/ship_cargo.png" width="200"> | **Cargo ship** | Freighter. NC; merchant ships also appear in CU, ME, DS and KO. |
| <img src="images/units/ship_trawler.png" width="200"> | **Trawler** | Intelligence trawler. CU. |
| <img src="images/units/ship_fishing.png" width="200"> | **Armed fishing boat** | KO. |
| <img src="images/units/ship_carrier.png" width="200"> | **Aircraft carrier** | US carriers (USS America, Nimitz, Kennedy, ...). Friendly; takeoff and landing base in carrier missions (not with *Realistic weapons*). |

Only the missile boats and patrol boats carry air defences. The other ships have no air defences in
the game.

## 9. Mission planner

![Mission planner](images/planner.png)

*Plan route* in the menu opens the theater map with the mission's route. The targets and the
landing base stay where they are; between them you add, move and remove navigation points:

| Action | |
|---|---|
| Click a leg | Add a point there (up to 16 points). |
| Drag a point | Move it. |
| Right click a point | Remove it. |
| Drag the map, mouse wheel | Pan, zoom. |
| RESET | Back to the briefed route. |

Each SAM site shows its missile range as a dotted red circle and, filled in orange, where its radar would
detect you, flown at the selected profile (*Low* 500 ft, *Medium* 5000 ft or *High* 20,000 ft, at
480 kt with the bay doors shut and wings level; banking, opening the doors, the ECM jammer and
speed make the circles larger). Sites that are off the air are drawn faint. The panel lists the
legs with distance and time and how many km of the route a radar would see. In drone defense
missions the drones' starting points are marked.

In flight the INS and the autopilot follow the planned route. When a target is done, the next
waypoint is the point after it, so the planned way home is flown as well. The planned route also
goes along with *Fly in VR*.

## 10. Debrief and career

![Debrief](images/debrief.png)

After every flight the **debrief** shows the result, the statistics, the points and a rating, and
a timeline of the mission (kills, radar and visual identifications, objectives). Points: primary
objective 100, secondary 50, each aircraft or ground target 10 (+5 for an aircraft shot down
before it saw you), each drone 15; −5 for each identification, −10 for each missile hit; landing
+25, crash −50, captured −100. The sum is multiplied by the difficulty (opponents and situation;
training missions count half).

**Flight map.** *FLIGHT MAP* shows the flight on the theater map, with the track coloured by how
close the radars came to detecting you: green out of reach, amber close (within 30 points of
detection), red detectable. The radars that tracked you are drawn with their missile range, and
symbols mark where a site started tracking you (amber diamond), missiles were fired at you (red
triangle), you were hit (red cross) and you scored a kill (green cross). Drag the slider below
the map, or press *PLAY*, to replay the flight: the aircraft symbol moves along the track, a red
line joins it to every radar tracking it at that moment, and the panel on the right gives the
altitude, speed, EMV and detection margin (EMV plus the strongest radar's score, minus 100) at that
time. *Radar exposure* sums up the flight: the time any radar tracked you, the closest you came to
detection and the radar that tracked you longest. Drag the map to pan it, turn the wheel (in VR,
push the thumbstick) to zoom. *RESULTS* goes back to the statistics.

**Tacview.** With *Tacview export* checked in the menu, every flight is also saved as a Tacview
recording (`.acmi`) in the game's data folder under `tacview` (on Linux
`~/.local/share/stealth-mission-89/tacview`, on Windows `%APPDATA%\stealth-mission-89\tacview`):
your aircraft, the enemy aircraft and missiles, the radar sites and airfields. The theater is laid
on the globe around one of its real airbases at the game's scale, so the rest of it lies only
roughly where the real places are.

**Tacview real-time telemetry.** With *Tacview real-time telemetry* checked in the menu, the
game hosts a Tacview session and sends it the same data while you fly. In Tacview, choose
*Record > Real-Time Telemetry* and connect to address `127.0.0.1`, port `42674`, with an empty
password. Tacview can join from the menu, before the flight, or during it: it then first
receives the flight so far. The session lasts one flight (in the campaign, one sortie): when it ends, Tacview is disconnected
(it keeps what it received) and has to connect again for the next one. To watch from another
computer, start the game with `--tacview-address 0.0.0.0` (or `<ip>:<port>`) and let it through
the firewall; `--tacview-password <text>` makes Tacview give that password to join.

**Career.** *Career* in the main menu keeps pilot records: enlist a pilot by name, then start a
**tour of duty**, 12 sorties in the theater, situation and opponents chosen beside the records
(fixed while the selected pilot is on a tour). The aircraft, realism, weather and classic / modern
options there apply to every sortie. *Start tour* or *Fly next sortie* shows the sortie's briefing
with PLAN ROUTE, LOADOUT and FLY; NEW SORTIE picks another one, BACK returns to the records. A
sortie starts at its rolled time. In a tour:

- What you destroy stays destroyed for the rest of the tour, and new targets are never ones you
  already hit.
- Points add up to promotions: First Lieutenant 300, Captain 800, Major 1800, Lieutenant Colonel
  3500, Colonel 6000. A finished tour earns 200 extra points.
- A mission can earn the Air Medal (150 points), Distinguished Flying Cross (300), Silver Star
  (500), Air Force Cross (800) or Medal of Honor (1200); hits taken earn the Purple Heart.
- A crash or failed ejection ends the career (killed in action); so does being captured after an
  ejection (prisoner of war).

![Pilot records](images/career.png)

Single missions from the menu do not count for any pilot.

## 11. Drone defense (Ukraine)

![Closing in on a Geran-3](images/geran_chase.png)

Russian Geran attack drones are on their way to a Ukrainian city or the South Ukraine nuclear power
plant. They start already in the air and arrive one after another, about 9 to 25 minutes after
take-off. You take off from the nearest airfield with air-to-air missiles, the gun and two
Paveways (with *Realistic weapons*: the Paveways and extra fuel).

- The **primary objective** is met when every drone is down or through and no more than the
  briefed number got through. A drone that hits the nuclear power plant fails the mission.
- The **secondary objective** is the launch site the drones came from.
- The early warning network shows every drone on the TAC display (open deltas) and on the map.
  They have no radar and do not fight back, but the Geran-2 is small, slow and low: slow down
  behind it and use the gun or a Sidewinder.
- Opponents and situation set the size of the wave (4 to 12) and the share of the faster jet
  Geran-3.

<table>
<tr>
<td width="40%"><img src="images/mfd_map_drones.png" alt="Drones on the MAP page"><br>
The drones (crosses) on the MAP page, on their way to Poltava.</td>
<td width="60%"><img src="images/geran_missile.png" alt="AMRAAM hit"><br>
An AMRAAM fired from 120 m behind hits the Geran-3.</td>
</tr>
</table>

In Ukraine strike missions a few drones may also cross the theater; shooting them down earns points.

## 12. Ejection and rescue

![Waiting for the rescue](images/csar.png)

After a good ejection over enemy territory (Shift-F10, or the ejection handle in the cockpit) the
mission goes on from the ground. The view follows the pilot under the parachute (look up, with the
headset or the right mouse button, to see the canopy; it is packed away on landing) and on the
ground, and a panel shows the rescue's progress, with buttons for the signal flare and time
acceleration (click them with the mouse; in the headset the panel floats at chest height: point at
a button with either controller and pull the trigger). The aircraft falls silent once you are out;
on the ground your emergency beacon chirps every few seconds.

- A **UH-60** comes from the nearest friendly airfield's alert area. Close in it cannot find you
  without a **signal flare** (the flare key, 1): fire one when it is near (you have three).
- **Mi-24 Hinds** from the nearest enemy airfield search the area. A flare also shows them where you
  are, and a Hind passing close may spot you. If one gets to you first you are captured.
- Wait too long (15 minutes against green opponents, 9 against elite ones) and you are captured.
- Time acceleration (Shift-Z) runs four times faster while you wait.

Picked up: the mission ends like a parachute landing. Over friendly territory or the sea the
mission ends when the parachute lands, as before.

## 13. Multiplayer

Up to 64 players fly the dynamic campaign together (coop: everybody is on the friendly side).
One computer runs the war, the **server**: either a player's game (HOST A GAME) or a dedicated
server without a game of its own. The others join it over UDP.

### Your name

The MULTIPLAYER screen asks for your name first; the other players know you by it, and the
campaign keeps your sorties under it: join again under the same name and they are yours again. In
the headset, click a text field with the laser pointer and a keyboard opens under it; press its
keys with the trigger and DONE when finished. Keys type at the blinking cursor: click in the text
to place it, or move it with ← and →. The career's pilot name uses the same field.

### Joining

JOIN A GAME lists the **public servers** at the top: each one's name, the pilots in it out of
how many it takes, the theater, the situation and the campaign's clock. JOIN on a row connects
to it (REFRESH fetches the list again; a server of another game version is greyed out). The
same list is on the web at https://sm-master.playspoon.com, where a server opens up for its
settings, the places each side holds and who flies what.

Or type the server's IP address or host name, with the port after a colon if it is not the
default 28989 (`192.168.1.20`, `sm89.example.org:29000`, `[2001:db8::1]:28989`), and CONNECT.
Either way you come to the campaign's Ops screen; your aircraft waits at a friendly base.

### Hosting

HOST A GAME serves a new campaign (theater, opponents, situation) or one of your saved ones, and
you fly in it. Give the server a name (the others see it when they join; without one it is
"<your name>'s campaign"), choose the UDP port and how many players may join, and whether the
game is on the public server list (on by default); to play over the internet,
forward that UDP port to your computer in your router. The campaign is saved on your computer as
when you play alone.

### Who decides what

The server decides how the war is fought, for everybody: realistic damage, realistic guidance, no
crashes, time of day (moonlit nights shorten the enemy's visual range), realistic F-117A
weapons and friendly fire, and of course the campaign's own options. The weather is the same for
everybody: the same clouds in the same places, and the same lightning strikes. Each player keeps his own cockpit (classic or
modern), flight model, avionics, HUD, shaders, shadows and lights, and picks his aircraft (F-117A
or F-16) for each sortie on the briefing's LOADOUT page.

### On the Ops screen

- **PLAYERS** lists everybody in the campaign: who is on the mission board, who flies what, the
  sorties flown and objectives achieved, and each player's ping.
- **SAVE & EXIT** (top right, under the clock) leaves the campaign; as the host it also ends the
  game for the others. On a server that is not yours the button reads **DISCONNECT**: the war goes
  on on the server, and nothing is saved on your computer.
- Every player takes his own missions from the board and may order insertions. Kills and
  objectives count for the player whose weapon it was. Players taking off from the same base line
  up in a staggered formation on the runway: the first in the middle, far enough down the runway
  for the others, then rows of two behind him.

In the air the other players are friendly contacts on the MFD (blue on the classic map and TAC
pages, a green ring on the modern MFD); their weapons in flight show as missiles. Their cannon
fire is drawn as tracers like your own. Picked on the MFD, another player can be locked like an
enemy aircraft (an enemy under the cursor is picked first), and an air-to-air missile fired with
that lock goes for him: he gets the missile warning, and his decoy can lure it away. Your cannon
rounds hit him too. Whether that hurts is the server's **friendly fire** rule: with it, missile
and cannon hits damage him like the enemy's; without it a missile bursts on him harmlessly and
the rounds just spark.

Your cannon fires at once on your computer, and what its rounds hit on your screen counts: your
game tells the server, which checks that you were firing and that the target really was there
at that moment (it remembers the last second of the war), then it is your kill. Missiles and
bombs are flown by the server.

If your connection drops during a sortie, your aircraft waits for you for 30 seconds: join again
under the same name and fly on. After that the sortie ends as aborted.

### Dedicated server

The game's executable runs a server without a window with `--dedicated`:

```
stealth-mission-89 --dedicated --theater ua --name "Friday night ops" --port 28989 --realistic-guidance
```

| Switch | |
|---|---|
| `--name <text>` | Server name, shown to the players. |
| `--port <n>` | UDP port (default 28989). |
| `--max-players <n>` | 1 to 64 (default 64). |
| `--theater <code>` | ce, cu, ko, ku, lb, me, nc, pg, ua, vn. Default: the saved campaign's, else ce. |
| `--roe <0..2>`, `--opponents <0..3>`, `--seed <n>`, `--start-hour <h>` | A new campaign's options. |
| `--sky-ratio <x>` | The sky runs x times faster than real time (default 24: a day per hour). |
| `--clear-sky` | A new campaign without weather (else it changes once or twice a day). |
| `--realistic-damage`, `--realistic-guidance`, `--no-crashes`, `--continuous-time`, `--realistic-weapons`, `--friendly-fire` | The rules. |
| `--save <file>` | The campaign file (default `campaigns/dedicated.json` in the game's data directory). |
| `--send-rate <hz>` | How often the moving world goes to the players, 1 to 15 times a second (default 7.5). More is smoother; less leaves bandwidth for more players. |
| `--master <url>` | The server list to be on (default `https://sm-master.playspoon.com`; reported every 30 seconds). |
| `--no-master` | Not on the server list: players join by address. |
| `--plain` | Log lines instead of the console screen. |

The server saves the campaign (which side holds which place, what is destroyed, the players'
records) every minute and when it stops, and goes on from the save when it starts again. Started
with another `--theater` than the saved campaign's, it begins a new campaign and keeps the old
save next to it (`dedicated.json.<theater>.bak.json`).

In a terminal the server shows a console screen: its uptime, the places and airfields each side
holds, the players and what they do, and the recent events with their times (UTC). **q**, Esc or
Ctrl-C stops it (it saves first). Without a terminal (a service, `--plain`) it writes the events
as log lines; SIGTERM or Ctrl-C stops it the same way.

