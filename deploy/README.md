# VPS deployment

See [CURRENT-VM.md](CURRENT-VM.md) for the active Instabox deployment.
The older Ubuntu provisioning notes below are retained for reference.

Target: Ubuntu 26.04 LTS on 15.252.100.197 (2 vCPU, 4 GB RAM), SSH user `ubuntu`. Authentication uses the existing
private key outside this project. No key or Discord token belongs in Git.

The standalone bot maintains an outbound Discord gateway connection. Its optional presence API listens only on `127.0.0.1:4100`, published at
`https://presence.11-x.online` through Cloudflare Tunnel. No inbound application
port is needed; Lavalink and the YouTube helper also listen only on loopback.

## Installed layout

- Binary: `/opt/elvnx-bot/elvnx-bot` (root-owned).
- Service: `/etc/systemd/system/elvnx-bot.service`.
- Secret environment: `/etc/elvnx-bot/bot.env` (root-owned, mode 600).
- Rules: `/etc/elvnx-bot/automod.json` (read-only for the bot).
- SQLite: `/var/lib/elvnx-bot/data/elvnx.db` (private `elvnx` service user).
- Generated preview images: `/var/lib/elvnx-bot/previews/`.
- Build workspace: `/home/ubuntu/elvnx-build` (separate from live state).

The service runs without root privileges, with a read-only filesystem outside its
state directory, no Linux capabilities, a 768 MB memory limit, and rate-limited
automatic restarts. Owner `/bot-restart` exits with code 75; systemd restarts it.
SIGINT allows the gateway to close before service shutdown.

## Build and install

`bootstrap.sh` installs Ubuntu build dependencies, the official Rust toolchain,
Noto fallback fonts, and a 2 GB swap file when provisioning a build server.
The 4 GB instance runs without swap and now has Rust 1.99.0, rustfmt, Clippy,
build-essential and pkg-config installed. Source and native Cargo caches live in
`/home/ubuntu/elvnx-build`; compilation no longer depends on the old EC2 host.
The first native dependency build took about twelve minutes with one Cargo job.
Build tools are not required for normal service operation. It does not modify SSH
or firewall settings. The source archive excludes `.env`, SQLite, and Windows binaries.

`build.sh` tests, lints, and compiles an optimized Linux binary using the locked
dependencies and the lower-memory `vps` profile. `install.sh` installs only after
all checks have passed. It enables startup on reboot but does not start without
the private environment file.

## First connection

Put the dedicated bot's token in the ignored local `.env`. Transfer the required
bot environment through SSH, then install it at the secret path with permissions
600. Keep `BOT_SUPERVISED=true`. Enable privileged intents in the Discord portal
before setting their environment flags. Set `REGISTER_COMMANDS=true` for the first
connection, confirm registration, then turn it off. Invite this bot to ELVNX-PVT
using `bot` and `applications.commands` scopes and the permissions in the main README.

The local `deploy/start.ps1` is for initial setup and reconstructs the bot
environment. Do not run it against the migrated instance to restart or update it;
preserve the complete existing server environment, including presence and music
settings. For initial setup it performs the preflight, private SSH transfer, startup,
and command-registration checks without printing the token. Run it only after
the token is saved and this bot is invited to the configured server.

```bash
sudo systemctl start elvnx-bot
sudo systemctl status elvnx-bot --no-pager
sudo journalctl -u elvnx-bot -n 50 --no-pager
sudo systemctl restart elvnx-bot
sudo systemctl stop elvnx-bot
```

After five failed starts within five minutes, investigate before
`sudo systemctl reset-failed elvnx-bot`. Do not expose the environment in diagnostic output.

For updates, preserve SQLite and the environment, stop the bot before replacing
the binary, then restart. Back up SQLite using its backup API or stop the service
and copy the database with its WAL/SHM files; copying only a live database file can
lose recent writes. Do not run multiple copies against the same gateway/database.

Music setup and operation: deploy/MUSIC.md. Daily reports: deploy/DAILY-INSIGHT.md.

Public lifecycle artwork and scoped Legacy notifications: [ONLINE-INFO.md](ONLINE-INFO.md).
