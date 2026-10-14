# VPS deployment

Target: Ubuntu on 13.203.104.97, SSH user `ubuntu`. Authentication uses the existing
private key outside this project. No key or Discord token belongs in Git.

The standalone bot maintains an outbound Discord gateway connection. It does not
run a public HTTP server and needs no additional inbound port or domain.

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
Noto fallback fonts, and a 2 GB swap file on this 2 GB VPS. It does not modify SSH
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

The local `deploy/start.ps1` performs the preflight, private SSH transfer, startup,
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
