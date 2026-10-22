# The browser build

Single missions, careers and the dynamic campaign run in the browser, built to WebAssembly with
[Trunk](https://trunkrs.dev).

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
`.github/workflows/build.yml` and uploads it as an artifact. Nothing is deployed automatically.

**Browser.** The game needs WebGPU: a current Chrome or Edge, Firefox 141 or later, or Safari 26,
with hardware acceleration on. WebGL2 can't run it, because the palette, particle, trail and
cloud shaders read storage buffers. Without WebGPU, the page says so.

**Saves.** Settings, the input mapping, pilots and campaign saves are kept in the browser's
localStorage under `sm89/…` (`crates/stealth-host/src/store.rs`). They belong to that browser
and site. Clearing the site's data deletes them.

**Not in the browser build:**

- VR
- multiplayer (see below)
- Tacview export and real-time telemetry
- head tracking
- the command line options
- the `SM89_*` environment variables

The main menu shows DOWNLOAD instead of MULTIPLAYER and QUIT. It opens `web/download.html`,
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
- **Native-only code.** `stealth-net`'s console, dedicated server and master server HTTP client
  are left out of the wasm build, along with `--dedicated`. So are the head-tracking and Tacview
  real-time systems.

## Multiplayer in the browser: feasibility

It is feasible, but not without new infrastructure. Today's protocol can't reach a browser.

### Why it doesn't work as it is

- **No UDP.** The coop transport (`crates/stealth-net/src/transport.rs`) is a reliability layer
  over a raw UDP socket. Browsers can't open UDP sockets.
- **Certificates.** A page served over HTTPS may only connect with `wss://` (WebSocket over TLS),
  WebTransport (HTTP/3) or WebRTC. The first two need a certificate the browser trusts.
  Servers are hosted by players, by IP address, without certificates.
- **No hosting.** A browser can't listen for connections, so it could only join. It could not
  host a listen server.

The rest is ready:

- The sim, the campaign host and the protocol (CBOR plus lz4) build for wasm.
- The master server sends `Access-Control-Allow-Origin: *`, so a browser can `fetch` the server
  list directly.

### Recommended: a WebSocket-to-UDP relay on the master server

The master server (sm-master.playspoon.com) already has TLS through Caddy. A relay there would
work like this:

1. The browser opens `wss://sm-master.playspoon.com/relay/<server id>` (through Caddy).
2. The relay opens one UDP socket per browser and passes each WebSocket message on as one UDP
   datagram to the game server listed under that id, and back.
3. The game server sees an ordinary UDP client at the relay's address. **No change on game
   servers, and old servers work too.**

Work needed:

- **stealth-net.**
  - A datagram trait under `Endpoint`. The socket is only touched in `bind`, `poll`
    (`recv_from`), `transmit` (`send_to`) and `local_addr`. A UDP implementation and a
    WebSocket implementation (`web-sys` `WebSocket`, binary messages) would sit under it.
  - `Instant` replaced by `web-time` in `transport.rs`, `server.rs` and `client.rs`.
  - The connect salt seeded without `SystemTime`.
- **stealth-master.** The relay endpoint: a WebSocket server (for example `tungstenite`) next to
  `tiny_http`, limited to listed servers and with per-IP caps, so it can't be used to send UDP
  anywhere.
- **The game.**
  - The multiplayer menu back in the browser build: join only, the list fetched with `fetch`.
  - A "join by address" field that only offers listed servers.

Costs:

- **Latency.** Every packet makes an extra hop through the relay.
- **TCP head-of-line blocking.** A WebSocket is TCP. A lost packet delays the unreliable world
  updates behind it instead of being skipped. The world is still sent whole every time (and
  interpolated), so this shows up as stutter on bad connections, not as errors.
- **Relay bandwidth.** The server console shows each player's kB/s, and that much goes through the
  relay twice per browser player.

Effort: a few days for the transport trait, the relay and the menu, plus testing with the
dedicated server and two browser clients.

### Alternatives

- **WebTransport on the relay** (`wtransport` or `quinn`). Unreliable datagrams, so no
  head-of-line blocking and less latency. Caddy can't pass it through, so the relay needs the
  certificate itself. A later step for the same relay.
- **WebSocket or WebTransport on every game server.** Needs a trusted certificate per server.
  WebTransport's `serverCertificateHashes` works with self-signed certificates, but only those
  valid for at most 14 days, whose hash the master server would have to publish. Too much for
  player-hosted servers.
- **WebRTC data channels.** Unreliable and peer-to-peer, but every server needs a WebRTC stack
  (ICE, DTLS, SCTP), plus a signalling service. The most work, for little gain over the relay.
