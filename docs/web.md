# The browser build

Single missions, careers, the dynamic campaign and joining coop servers run in the browser,
built to WebAssembly with [Trunk](https://trunkrs.dev).

## Building and playing

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
trunk serve --release        # http://127.0.0.1:8080
trunk build --release        # dist/web: serve the folder as it is
```

`Trunk.toml` and `web/index.html` hold the setup. The `wasm-release` profile in `Cargo.toml`
builds quickly for now (about 2.5 minutes, a 79 MB wasm). Its comment lists the settings for a
smaller download (42 MB, 13 MB gzipped, about 15 minutes). Any static web server works, as long
as it serves `.wasm` as `application/wasm`. CI builds the folder in the `web` job of
`.github/workflows/build.yml` and uploads it as one zip (`stealth-mission-89-<ref>-web.zip`).
Nothing is deployed automatically.

The build also writes gzipped copies (`*.wasm.gz`, `*.js.gz`, from the hook in `Trunk.toml`). With
nginx, `gzip_static on;` sends them to browsers that take gzip, without compressing the 79 MB wasm
on every request (about 17 MB on the wire). The hashed wasm and script names can be cached forever;
`index.html` should not be.

**Browser.** The game needs WebGPU: a current Chrome or Edge, Firefox 141 or later, or Safari 26,
with hardware acceleration on. WebGL2 can't run it, because the palette, particle, trail and
cloud shaders read storage buffers. Without WebGPU, the page says so.

**Saves.** Settings, the input mapping, pilots and campaign saves are kept in the browser's
localStorage under `sm89/…` (`crates/stealth-host/src/store.rs`). They belong to that browser
and site. Clearing the site's data deletes them.

**Not in the browser build:**

- VR
- hosting a coop game, and joining a server that isn't on the public list (see below)
- Tacview export and real-time telemetry
- head tracking
- the command line options
- the `SM89_*` environment variables

The main menu shows DOWNLOAD instead of QUIT. It opens `web/download.html`,
which offers the desktop releases. The page links to the GitHub release of the version in
`Cargo.toml`, which a Trunk hook writes to `version.js` at build time. So a build of a version
that has no release yet links to files that don't exist yet.

## How it differs from the desktop build

- **Data files.** Data read directly (sim tables, mission tables, theaters, palettes, sprites)
  goes through `stealth_host::assets::read`. In the browser these files are compiled into the
  wasm by `crates/stealth-host/build.rs`, about 1 MB. Models and sounds are loaded by Bevy's
  asset server from `assets/` beside the page.
- **User files.** These go through `stealth_host::store`: keys such as `config/prefs.json`, which
  are files in the config and data directories on the desktop and localStorage in the browser.
- **Time.** `Instant` is `bevy::platform::time::Instant` and `SystemTime` is `web_time`'s. Both
  of std's panic in the browser.
- **Shaders.** Browsers check WGSL's uniformity rules, which naga doesn't. `dpdx`, `dpdy`,
  `fwidth` and `textureSample` are only allowed where control flow hasn't yet branched or
  returned on a per-pixel value. `palette.wgsl` takes its derivatives at the top of `fragment`
  (`Deriv`) for this reason. A mistake shows as an invalid render pipeline in the browser's
  console, and the game stops.
- **Native-only code.** `stealth-net`'s console, dedicated server and master server reports
  are left out of the wasm build, along with `--dedicated`. So are the head-tracking and Tacview
  real-time systems.
- **Networking.** The coop transport sends through `stealth_net::transport::Datagrams`: a UDP
  socket on the desktop, a WebSocket to the relay in the browser (`stealth-net/src/websocket.rs`).
  The server list comes from the browser's `fetch`. `Instant` in `stealth-net` is `web_time`'s.

## Multiplayer in the browser

Browsers can't open UDP sockets, and a page served over HTTPS may only connect with `wss://`
(WebSocket over TLS), WebTransport or WebRTC, all with a certificate the browser trusts. Game
servers are hosted by players, by IP address, without certificates. So the browser joins
through a relay on the master server, which has a certificate:

1. MULTIPLAYER lists the public servers (`GET /api/servers`, the master server answers with
   `Access-Control-Allow-Origin: *`).
2. JOIN, or CONNECT with a listed server's address or name, opens
   `wss://sm-master.playspoon.com/relay/<server id>` (`master::relay_url`).
3. The relay (`crates/stealth-master/src/relay.rs`) opens a UDP socket for this browser and
   passes each binary WebSocket message on as one datagram to the server listed under that id,
   and each datagram back as one message.
4. The game server sees an ordinary UDP client at the master server's address. Game servers
   need nothing new, and older ones work too.

The relay only reaches servers on the list, passes only the game's packets, and takes a few
browsers per address (limits in `crates/stealth-master/README.md`). So:

- **Join only.** A browser can't listen for connections, so HOST A GAME is left out.
- **Listed servers only.** A server off the list (HOST A GAME's switch, `--no-master`, or on a
  LAN) can't be joined from the browser.

Costs:

- **Latency.** Every packet makes an extra hop through the relay.
- **TCP head-of-line blocking.** A WebSocket is TCP. A lost packet delays the unreliable world
  updates behind it instead of being skipped. The world is still sent whole every time (and
  interpolated), so this shows up as stutter on bad connections, not as errors.
- **Relay bandwidth.** The server console shows each player's kB/s, and that much goes through the
  relay twice per browser player.

**Testing locally.** Run the master server with `--relay-internal` (it relays to this machine),
a dedicated server with `--master http://127.0.0.1:8089`, and `trunk serve`. Then open
`http://127.0.0.1:8080/?master=http://127.0.0.1:8089&relay=ws://127.0.0.1:8090`. The page's
`master` parameter replaces the public master server. `relay` is where the relay listens, since
without a proxy in front it has a port of its own. It defaults to the master server's address.

### Alternatives for later

- **WebTransport on the relay** (`wtransport` or `quinn`). Unreliable datagrams, so no
  head-of-line blocking and less latency. Caddy can't pass it through, so the relay needs the
  certificate itself.
- **WebSocket or WebTransport on every game server.** Needs a trusted certificate per server.
  WebTransport's `serverCertificateHashes` works with self-signed certificates, but only those
  valid for at most 14 days, whose hash the master server would have to publish. Too much for
  player-hosted servers.
- **WebRTC data channels.** Unreliable and peer-to-peer, but every server needs a WebRTC stack
  (ICE, DTLS, SCTP), plus a signalling service. The most work, for little gain over the relay.
