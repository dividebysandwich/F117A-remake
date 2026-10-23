//! `--help` / `-h`: the command line options (the dedicated server has its own, see
//! `dedicated.rs`).

const USAGE: &str = "\
stealth-mission-89 [options]

Without options the game starts in the menu.

Starting
  --mission <seed>             fly a generated mission directly
    --theater-index <n>        its theater by index (default: --theater's, else 3)
    --kind <0..4>              air, strike, air training, strike training, drone defence
                               (default 1)
    --roe <0..2>               cold war .. conventional war (default 1)
    --opponents <0..3>         enemy skill (default 1)
    --start-hour <0..23>       takeoff time
    --moon <0..7>              moon phase in eighths (0 new, 4 full; default: the date's)
    --start-base <name>        take off from another airfield
    --career <pilot>           a tour sortie of that pilot
    --route <file>             a route planned in the menu (JSON)
    --loadout <file>           stores chosen in the menu (JSON)
    --print-mission            print the generated mission's summary
  --theater <code>             fly from the menu's defaults in this theater, skipping the menu:
                               ce, cu, ko, ku, lb, me, nc, pg, ua, vn
  --fly                        fly from the menu's defaults, skipping the menu
    --base <name>              the airfield to start from
    --night                    a night flight
  --campaign <name>            straight into a saved campaign, or a new one under that name
                               (--theater, --roe, --opponents, --seed, --sky-ratio, --start-hour)
  --vr                         OpenXR headset (without a runtime it runs on the desktop)
  --viewer                     browse the game's 3D models
    --file <name> --model <n>  the model shown first
    --pitch <deg> --yaw <deg> --distance <m>
                               the view of it
    --lights                   with its lights
    --no-label                 without the model label
  --freecam                    free debug camera
    --at <place>               at a place by name, or primary / secondary (the targets)
    --pos <x,z>                at a ground position in metres
    --alt <m>                  height (default 300)
    --yaw <deg> --look-pitch <deg>
                               view direction (default 0, -15)

Aircraft and rules
  --aircraft f117|f16          the flown aircraft (default f117)
  --lockheed                   realistic weapons: the real F-117A's 2 bays, no air-to-air
                               missiles, no carrier operations
  --realistic-guidance         realistic missile guidance
  --realistic-damage           realistic weapon damage
  --no-crashes                 crash protection: collisions and ground contact are not fatal
  --continuous-time            moving sun and moon; moonlight sets the enemy's visual range
  --sun-moon                   without --continuous-time: sun by day, moon and stars by night
  --air-defence                low-level air defences
  --weather <sky>              random, clear, scattered, broken, overcast, storm
                               (default: rolled from the seed)
  --cloud-cover <percent>      coverage of the main cloud layer (default: rolled)

Cockpit and graphics
  --classic-cockpit            DOS era instrument panel (F-117A only)
  --classic-hud                DOS era low-resolution HUD
  --classic-avionics           DOS era MFD pages
  --classic-flight             DOS era flight model (default for --script runs)
  --modern-flight              the modern flight model also in --script runs
  --modern                     modern rendering (smooth gradients and haze, shading, MSAA)
  --shadows                    sun shadows
  --local-lights               cockpit lamps, explosions, flares, missile motors, runway lights
  --head-tracking              head/eye tracking through OpenTrack (FreeTrack or UDP 4242)
  --vr-cockpit-scale <percent> cockpit size in VR
  --window-size <WxH>          window size
  --novsync                    uncapped frame rate
  --ui-zoom <f>                interface scale

Tacview
  --tacview                    save the flight as a Tacview ACMI file when it ends
  --tacview-real-time          send the flight to Tacview while it is flown
  --tacview-address <ip[:port]|port>
                               where it is hosted (default 127.0.0.1:42674; 0.0.0.0 lets
                               another computer join)
  --tacview-password <text>    the password Tacview must give to join

Multiplayer
  --connect <host[:port]>      join a server
  --host                       serve a new campaign (in --theater)
  --name <pilot>               the pilot name (default: the saved one)
  --master <url>               the server list (default https://sm-master.playspoon.com)
  --dedicated                  run a coop server without a window
                               (--dedicated --help lists its options)

Testing and screenshots
  --script <steps>             replay timed actions, e.g. \"0:ThrottleMax,7:pitch=0.35,14:Gear\"
  --log-flight / --log-sim     print the flight / simulation state
  --readout                    on-screen text readout of the flight state
  --time-scale <x>             run the game time x times faster
  --diag                       frame time and entity count diagnostics in the log
  --screenshot <file.png>      save a screenshot and exit
    --shot-frames <n>          after n frames (default 90)
    --shot-time <s>            after s seconds of flight
    --shots <s,s,..>           several, at these flight times
    --shot-debrief             frames counted from the debrief on
  --head-look <yaw,pitch>      initial cockpit view direction (degrees)
  --start-final <km[,ft[,kt]]> start on final approach (default 3 km, 500 ft, 170 kt)
  --start-target <km[,ft[,kt]]> start in the air south of the primary target, heading at it
                               (default 8 km, 2000 ft, 350 kt)
  --start-chase <k[,m[,kt]]>   start in the air behind drone k (default 300 m, 250 kt)
  --stage-cam <mode[,dist,height]>
                               staged camera: front, behind, side, high, missile, drone
  --damage <bits,hits>         start damaged
  --strike <ground|cloud>      a frozen lightning strike early in the flight
  --tod-seed <n>               the palette's time of day fade (0..15)
  --sky-golden <base[,intensity[,warmth]]>, --sky-hours <h>, --sky-phase <0..1>
                               sky test looks
  --debrief-map                open the debrief's flight map at mid-flight
  --export-maps <dir>          write every theater's map picture and exit
  --open-mission, --open-planner, --open-loadout, --open-career, --open-campaign,
  --open-multiplayer, --open-keyboard
                               start with that screen or window open
  --campaign-tab <tab>         board, map, players, debrief, log
  --campaign-take <n>          take the n-th mission of the board
  --campaign-accept map|loadout
                               open the first mission's briefing on that page
  --campaign-insert            order an insertion against the nearest enemy place
  --campaign-watch             watch the first friendly aircraft

  -h, --help                   this list
";

/// Prints the options and exits when the command line asks for them.
pub fn run_if_asked() {
    if std::env::args().skip(1).any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        std::process::exit(0);
    }
}
