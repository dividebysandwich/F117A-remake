# stealth-master

The coop server list of Stealth Mission 89. Game servers (dedicated ones and players' games
hosting a campaign) report to it every 30 seconds; games list them in MULTIPLAYER, JOIN A GAME;
a web browser gets the list as a page. A server not heard of for 90 seconds is gone.

| Request | |
|---|---|
| `POST /api/servers` | A game server's report (`stealth_master_proto::ServerReport`, JSON). Its address is where the request came from with the report's UDP port. |
| `DELETE /api/servers/<port>` | The game server at the request's address and that port goes. |
| `GET /api/servers` | The servers, most players first (`Vec<ServerEntry>`). |
| `GET /api/server/<id>` | One server by its `id` (a hash of its name and IP address). |
| `GET /` | The same as a web page: a row per server, opened for its campaign, places held, pilots and live map. |
| `GET /server/<id>` | One server's page with a big live map: its places and the players in the air. |
| `GET /maps/<theater>.png` | A theater's map picture under the live maps. |
| `GET /health` | `{"ok":true}` |
| `GET /relay/<id>` (WebSocket) | The browser build's way to a listed server, on a port of its own (below). |

The server speaks plain HTTP and keeps the list in memory (it fills again within half a
minute after a restart). HTTPS comes from a reverse proxy (Caddy, Cloudflare and cloudflared, ...); a
request from this machine or its LAN is a proxy's, and the client is in its `CF-Connecting-IP`
or else the last internet address in `X-Forwarded-For`.

The pages fetch themselves again every 5 seconds and swap in what changed, so the maps follow
each report (while somebody flies, servers report every 10 seconds). The map pictures in
`maps/` come from the game: `stealth-mission-89 --export-maps <dir>`.

## The relay for browsers

Browsers can't send UDP, so the browser build joins through the master server (docs/web.md).
It opens a WebSocket to `/relay/<id>`. Each binary message goes on as one UDP datagram to that
server, from a socket of its own, and each datagram back as one message. The game server sees
an ordinary client at the master server's address, so game servers need nothing new.

It only relays:

- to servers on the list, and not to ones on this machine or its LAN (`--relay-internal`
  allows them, for testing);
- the game's packets (starting `SM89`, at most 1500 bytes), at most 300 a second;
- 4 browsers per address at once, 512 in all.

A relay closes after 15 seconds without packets either way. Each one is a thread and a UDP
socket. The relay listens on `--relay-listen` (default `127.0.0.1:8090`, `off` for none), and
the proxy sends `/relay/` there (`deploy/Caddyfile`). Opens, refusals and closes are logged.

## Running it on sm-master.playspoon.com

```sh
cargo build --release -p stealth-master
sudo install target/release/stealth-master /usr/local/bin/
sudo cp crates/stealth-master/deploy/stealth-master.service /etc/systemd/system/
sudo systemctl enable --now stealth-master
# Caddy for HTTPS (deploy/Caddyfile into /etc/caddy/Caddyfile, then reload).
sudo systemctl reload caddy
```

`stealth-master --listen <addr:port> --threads <n> --relay-listen <addr:port>|off
--relay-internal` (default `127.0.0.1:8089`, 4, `127.0.0.1:8090`).

Game servers report to `https://sm-master.playspoon.com` unless told otherwise: the dedicated
server with `--master <url>` or `--no-master`, the game with `--master <url>` (for its list and
when it hosts; HOST A GAME has a switch to stay off the list).
