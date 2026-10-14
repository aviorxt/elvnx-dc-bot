# Private music services

The Rust bot uses Lavalink 4.2.2 with youtube-source upstream revision `2be8e542d3f6f178e048dca565892684c2e40177`. Both Lavalink
(127.0.0.1:2333) and its self-hosted yt-cipher helper (127.0.0.1:8001) bind
only to localhost. No inbound music port is required on AWS.

`/play` accepts a search phrase or supported HTTPS media URL and defaults to
YouTube search. Join a normal voice channel first. The bot needs View Channel,
Connect, Speak, and View/Send/Embed/Attach permissions for the music panel.
Stage channels are unsupported. Same-voice-channel membership is checked on
every playback control. Queues hold at most 50 tracks; volume is capped at 100.

`/lavlink-status` shows node connection/version/latency and queue health. Voice
channel status shows the song title, changes when paused, and clears on stop;
grant Set Voice Channel Status to the bot.

The doodle PNG panel provides pause/resume, skip, stop/leave, volume adjustment,
and queue buttons. It refreshes periodically without posting new messages.
The bot leaves an empty human voice channel on its next 15-second check and
leaves after 180 seconds with an empty queue. Queues are memory-only; a node
disconnect or bot restart clears them. The bot is deafened and does not record
voice audio. Source availability depends on the upstream provider.

Services: `elvnx-bot`, `lavalink`, `yt-cipher`; all are enabled for reboot.
Use `sudo systemctl status SERVICE` for health. Secrets remain in mode-600
server environment files; never print them into logs or copy them into source.
Bot environment rotation with `deploy/start.ps1` preserves Lavalink credentials.

YouTube authorization uses the plugin's official device OAuth flow, completed
by the account owner. Refresh token: `/etc/lavalink/youtube.env` (root-only).
HTTP debug logging is disabled because the plugin can otherwise log tokens.
Cipher password: `/etc/lavalink/cipher.env` (root-only), copied by
`cipher-secrets.py` into the Lavalink environment without displaying it.

Cipher source: https://github.com/kikkia/yt-cipher
Pinned revision: `1e1fd8e2f34ca90cf23545be72e46307bd3d3d2a`.
Deno 2.9.7; EJS revision `cd4e87f52e87ab6d8b318fd3a817adda6fafa8dc`, patched
with the upstream script. A single worker and five processed player scripts
limit memory. Runtime imports are cached; the service uses `--cached-only`.
The private service account can write only its cache directory.

`music-probe.py` tests source decoder startup without joining Discord voice.
It does not verify audible Discord playback; test that with `/play` from a
voice channel. It deletes its temporary Lavalink player before exiting.

References: https://github.com/lavalink-devs/youtube-source and
https://lavalink.dev/api/rest.html .

Music panel cleanup: /stop, the Stop/leave button, queue exhaustion, and voice disconnects delete the tracked control-panel message and clear its stored channel/message IDs. /nowplaying does not recreate a stopped panel. Temporary Discord deletion failures are retried in bounded batches every 15 seconds, using only IDs recorded for this bot's panels. Pending cleanup is memory-only and does not survive a process restart.
