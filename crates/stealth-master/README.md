# stealth-master

The coop server list of Stealth Mission 89. Game servers (dedicated ones and players' games
hosting a campaign) report to it every 30 seconds; games list them in MULTIPLAYER, JOIN A GAME;
a web browser gets the list as a page. A server not heard of for 90 seconds is gone.

| Request | |
|---|---|
| `POST /api/servers` | A game server's report (`stealth_master_proto::ServerReport`, JSON). Its address is where the request came from with the report's UDP port. |
| `DELETE /api/servers/<port>` | The game server at the request's address and that port goes. |
| `GET /api/servers` | The servers, most players first (`Vec<ServerEntry>`). |
| `GET /` | The same as a web page: a row per server, opened for its campaign, places held and pilots. |
| `GET /health` | `{"ok":true}` |

The server speaks plain HTTP and keeps the list in memory (it fills again within half a
minute after a restart). HTTPS comes from a reverse proxy on the same machine; requests from
there carry the client's address in `X-Forwarded-For`.

## Running it on sm-master.playspoon.com

```sh
cargo build --release -p stealth-master
sudo install target/release/stealth-master /usr/local/bin/
sudo cp crates/stealth-master/deploy/stealth-master.service /etc/systemd/system/
sudo systemctl enable --now stealth-master
# Caddy for HTTPS (deploy/Caddyfile into /etc/caddy/Caddyfile, then reload).
sudo systemctl reload caddy
```

`stealth-master --listen <addr:port> --threads <n>` (default `127.0.0.1:8089`, 4).

Game servers report to `https://sm-master.playspoon.com` unless told otherwise: the dedicated
server with `--master <url>` or `--no-master`, the game with `--master <url>` (for its list and
when it hosts; HOST A GAME has a switch to stay off the list).
