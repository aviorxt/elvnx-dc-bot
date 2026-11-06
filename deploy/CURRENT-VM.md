# Current VM deployment

The active bot runs on the Instabox VM `elvnx-svr` (Debian 13, x86_64).
The former EC2 services are stopped and disabled; files remain for rollback.

Services enabled at boot:

- `elvnx-bot.service`: Discord bot and presence API on loopback port 4100.
- `lavalink.service`: private audio node on port 2333, Java 21.
- `yt-cipher.service`: private YouTube helper on port 8001, Deno.
- `cloudflared-presence.service`: cloudflared 2026.10.7, outbound tunnel.
- `elvnx-keepalive.timer`: local health and Discord gateway HTTP checks every five minutes.

Public hostname: `https://presence.11-x.online`.

Source: `/home/ubuntu/elvnx-build`. Build artifacts: `/data/workspace/elvnx-target`.

```sh
cd /home/ubuntu/elvnx-build
CARGO_TARGET_DIR=/data/workspace/elvnx-target cargo build --locked --profile vps -j 3
CARGO_TARGET_DIR=/data/workspace/elvnx-target cargo test --locked --profile vps -j 2 presence::tests
```

The binary lives at `/opt/elvnx-bot/elvnx-bot`. A systemd drop-in uses a private
loader/library bundle under `/opt/elvnx-runtime` for compatibility with the binary
originally built on newer Ubuntu. Debian's system libraries are unchanged.

Secrets stay in `/etc/elvnx-bot`, `/etc/lavalink` and `/etc/cloudflared`.
Databases/caches stay in `/var/lib/elvnx-bot`, `/var/lib/lavalink` and
`/var/lib/yt-cipher`. Root-only migration archives live under
`/data/elvnx-migration-backup`. None belong in Git.

```sh
sudo systemctl restart elvnx-bot
sudo systemctl status elvnx-bot lavalink yt-cipher cloudflared-presence
curl -fsS https://presence.11-x.online/health
curl -fsS http://127.0.0.1:20241/ready
```

The keepalive cannot guarantee prevention of free-plan idle pauses or override
the maximum runtime. Services resume or start with the VM.
