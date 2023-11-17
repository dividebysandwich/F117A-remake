# F117A-remake
 A spiritual successor to an old DOS game, written in Rust / Bevy

![image](https://github.com/dividebysandwich/F117A-remake/assets/23048489/5f71d059-25f8-4a62-8f81-a4f6d1b91ffa)

![image](https://github.com/dividebysandwich/F117A-remake/assets/23048489/603ae486-8727-4d48-8e6e-066c4a736f89)

![image](https://i.imgur.com/8nikN4e.gif)


### Todo:

- [X] Basic scene setup
- [X] Basic controls
- [X] Basic flight physics
- [X] Basic Camera system
- [X] DOS style point light sources
- [X] Missiles with proportional navigation
- [X] Target destruction
- [-] IN PROGRESS: HUD
- [-] IN PROGRESS: Targeting MFD
- [-] IN PROGRESS: Arcade targeting
- [ ] Pulse / doppler radar gameplay
- [ ] SAM sites attacking player
- [ ] Radar countermeasures
- [ ] HSI MFD
- [ ] Sound
- [ ] Player damage modeling
- [ ] Advanced targeting (LANTIRN)
- [ ] Advanced flight physics
- [ ] Bombs
- [ ] Scenery detail generation: Roads, cities, fields, mountains
- [ ] Mission system
- [ ] AI aircraft
- [ ] IR AAM missiles
- [ ] IR countermeasures
- [ ] Menu


Non-goals:
Multiplayer (unless done by contributors), study-level realism, realistic graphics, VR

### Keys:

- [W] Throttle up
- [S] Throttle down
- [A] Rudder left
- [D] Rudder right
- [Cursor keys] Elevator & Ailerons
- [Space] Weapon release
- [F1] Cockpit view
- [F2] Follow cam, press repeatedly to cycle through view targets
- [N] Next target (Arcade targeting)
- [M] Previous target (Arcade targeting)
- [T] Lock target at crosshair (Arcade targeting)
- [Backspace] Clear target lock (Arcade targeting)

### Compiling:

At the moment, a special Bevy 0.12.0 compatible version of bevy_mod_billboard is needed, which has to be fetched from https://github.com/robtfm/bevy_mod_billboard/tree/bevy12
Simply compile this locally with ```cargo build``` and then put the relative (or absolute) path to that directory on your disk into the appropriate line in cargo.toml


Old images:

![image](https://github.com/dividebysandwich/F117A-remake/assets/23048489/225bec29-d680-49af-a29c-38eb084c2901)
