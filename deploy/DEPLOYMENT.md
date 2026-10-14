# Deployment record — 14 October 2026

VPS: `13.203.104.97` / SSH user `ubuntu` / Ubuntu 26.04 LTS, x86-64.

Installed an optimized Linux executable and the `elvnx-bot` systemd service.
The service is enabled for reboot, runs as `elvnx`, and is active. It connected
as **avior's Core** (bot ID `1474265288223621215`) in ELVNX-PVT, with all 100 guild
commands registered. Discord Member and Message Content intents were already
authorized in this application's flags and are enabled in the VPS environment.
No external interactions endpoint is configured, so commands use the gateway.
Automatic registration is disabled after successful initial registration.

Validated:

- 40 unit tests passed on Windows and the VPS, including poll vote closing/fair scheduling, once-only job claims, restore/playlist ownership, repeat behavior, and the existing moderation checks.
- Strict Clippy and formatting checks passed on the VPS.
- Optimized Linux build passed using locked dependencies.
- Database schema initialization passed as the service user.
- PNG rendering passed in a transient systemd service with filesystem restrictions.
- Linux welcome preview visually matches the local doodle design.
- Unit configuration verified, reboot startup enabled, no public bot port opened.
- Font license files installed alongside the executable.

Binary SHA-256:
`ad1d94bb26b497f902f54a8664db9507e426404a0474447d46534974b5e7d1ef`.

The service state directory is private to `elvnx`. No Discord token or SSH private
key was copied into source control or the source archive. Git was not committed
or pushed. The existing website was not deployed or changed.

Token rotation: update the ignored local `.env` and run `deploy/start.ps1`.
This validates membership from the VPS, installs
the secret environment with mode 600, restarts the service, verifies 100 registered
guild commands, and disables registration on subsequent restarts. Enable Message
Content and Server Members privileged intents in the Discord portal before
setting the corresponding environment flags to true.

Staff log update deployed: voice joins/leaves/moves and status changes, cached before/after edits, deleted-message content and attachment links, bulk-deletion transcripts, nickname/role/timeout changes. Destination remains #auto-log (1557909674194501703). Message history is memory-only, bounded to 1,000 observed IDs and 24 hours; copies sent to Discord logs follow Discord channel retention. No historical fetch was made. Previous executable retained for rollback.


Music and daily insight release: 67 commands, including /play controls, /lavlink-status, /count-toggle, /daily-insight. Lavalink 4.2.2 and private yt-cipher bind to loopback only. youtube-source revision 2be8e542d3f6f178e048dca565892684c2e40177 fixed tested playback failures from 1.18.2. Authorized YouTube refresh token and cipher credentials are root-only on the VPS. Decoder tests passed for a YouTube search and direct video; direct stream probe received HTTP 200 and 1024 bytes. Discord audible playback requires /play from a member voice channel; no test audio was played into the server. 20 tests and strict lint/format checks passed locally and on the VPS. Daily reports remain opt-in until a destination is selected. See MUSIC.md and DAILY-INSIGHT.md for controls and retention.

AFK release deployed: /afk-move, 68 registered guild commands, 23 passing tests locally and on the VPS, strict lint/format checks passed. Ten-minute continuous mute/deafen timers reset after voice-channel changes, active status, leaving, gateway reconnect, or bot restart. Live Discord voice state and permissions are rechecked before moves. Feature remains disabled until the administrator selects a DND destination. Existing bot state and music credentials preserved; no Git commit or push.

Music cleanup release: active music panels are deleted on stop, queue exhaustion, or voice disconnect, with bounded background retries for temporary Discord deletion failures. Panel refresh does not recreate stopped controls. 23 tests and strict Clippy/format checks passed locally and on the VPS. Command count remains 68. Existing state and server secrets preserved; no Git commit or push.

Always-react release deployed: 69 registered guild commands. /always-react selects one channel and adds ✅ to new human messages whose Discord role mentions include 1554097852672770118. Bot/webhook messages, other roles, and other channels are ignored. Server settings persist across restarts. Strict formatting/lint checks and the VPS's 23 existing tests passed. Feature remains disabled until a channel is selected. No Git commit or push.

Appukuttan Legacy release deployed: 70 guild commands registered. /appukuttan-legacy selects a text channel for the exact join message ente appukuttan vanneyyyy, only for user 463980163852664832 entering voice from a disconnected state. Voice moves and mute/deafen changes do not trigger announcements. Destination persists; command remains disabled until configured. Local and VPS checks passed, with 23 existing tests passing. No Git commit or push.

Rules release deployed: /rules channel:YOUR_CHANNEL posts one message containing a 1200x420 dynamically rendered monochrome doodle cover and an 11-section rules embed. Artwork uses the server name, current IST date, and bundled fonts. Manage Server plus channel access are required; bot Embed Links/Attach Files permissions are validated. Local cover visually checked; native VPS PNG render succeeded. Formatting, strict Clippy, and the VPS's 23 existing tests passed. 71 guild commands confirmed. No Git commit or push.

Live status release deployed: 72 guild commands registered. /bot-status-live creates a read-only ELVNX BOT STATUS category and elvnx-status/lavlink-status channels, editing one embed each every 30 seconds. Resource IDs and enabled state persist in SQLite; restart recovery and owned-message reconciliation are implemented. Pause with enabled:false. Setup remains uninvoked until the administrator runs the command. Local/VPS formatting, strict Clippy, and 23 existing tests passed. Live Lavalink stats endpoint confirmed required uptime/player/memory/CPU metrics. Stale timestamps indicate potentially offline bot/updater; no independent external monitor was added. No Git commit or push.

Community release deployed: settings dashboard, approved public role panels, persistent button polls/result cards, IST reminders, confidential staff reports, reviewed settings backup/import/restore, opt-in birthday cards, DJ role/vote skip/shuffle/repeat/private playlists. All 93 guild commands and the playlist save/play/list/delete subcommands verified through Discord's API. 30 tests, strict lint/format, optimized native build, schema initialization, and PNG generation passed. Lavalink connected after service restart. New report/birthday destinations remain unconfigured until selected by staff. See FEATURES.md.

Report and command-registration fix deployed: canonical /setup-report accepts optional channel and enabled options. Without a channel it creates/reuses a private member-reports destination with explicit everyone View Channel denial and staff/bot access; existing broad channels receive actionable errors without changing their permissions. Removed serverinfo/numer-game aliases and all 9 obsolete global registrations after preserving their definitions in a private VPS backup. Discord API confirms 91 guild commands, zero globals/duplicate names, and optional report setup fields. 31 tests, formatting, strict Clippy, native build, and PNG/schema checks passed. Bot and Lavalink connected. These changes remain uncommitted/unpushed pending user permission.

Availability release deployed: /availibilty-panel creates/reuses an overview cover plus Online, Offline, Gaming, and On VC channels. Default audience is the existing Legacy role; private voice activity is filtered to audience-visible channels. Presence intent availability verified before enabling the private server/local flag. Real-time event coalescing, bounded pagination, ownership checks, permission-change redaction, pause/resume, and persistent resource IDs are included; presence history stays in memory. 35 tests, strict Clippy/formatting, native build, schema/PNG checks passed. Discord API confirms 92 guild commands with zero globals or duplicate names. Final restart synchronized the member/presence roster within one second of Ready and Lavalink connected. Panel creation remains administrator-invoked; Git changes remain uncommitted/unpushed. See AVAILABILITY.md.

Single-embed availability update deployed: existing overview channel and message reused; Online, Offline, Gaming, and On VC now share four bounded embed fields plus the doodle cover. Compact non-pinging member references preserve space; overflow is shown within Discord's field limits. Four obsolete empty bot-generated status channels removed after their tracked messages were cleaned; custom content is preserved by the migration checks. Discord API verifies exactly one category child (availability-panel), one tracked message, one embed, four status fields, and a cover image. Retired migration records are cleared. Observed a live voice count changing from zero to one. 35 tests, strict lint/formatting, native build, schema and PNG checks passed. 92 commands remain registered with no duplicates. Git changes remain uncommitted/unpushed.

Version release deployed: /version displays package v0.1.0, build 1791990335, localized build time, recent fixes, and the avrxt.dev developer credit in an ephemeral themed embed. The installed executable's --version output matches the release metadata without initializing the database or connecting to Discord. All 93 guild commands verified, with zero globals, duplicate names, or missing features. 35 native tests, formatting, strict Clippy, optimized build, schema and PNG checks passed. Bot, Lavalink, and yt-cipher services are active; the availability roster synchronized after restart. Automatic command registration disabled after verification. Git changes remain uncommitted/unpushed.

Media posting release deployed: /send-attachement accepts one image/video/audio upload and optional caption for a selected text channel. Manage Server and destination access are required; bot Attach Files is validated. Native attachments preserve previews and playback. HTTPS Discord attachment hosts only, no redirects, 45-second download timeout, 8 MiB bounded download, no SVG, sanitized filename, disabled mentions, and private errors. Files remain memory-only; audit stores destination/message IDs. 36 tests and strict format/Clippy passed locally and on VPS, along with optimized build, PNG and schema checks. Discord verifies 94 guild commands, zero globals/duplicates/missing features. Services active; build 1791993836. Changes remain uncommitted/unpushed.

SVG support update deployed: /send-attachement accepts image/svg+xml and .svg filenames with missing, generic binary, or XML content types. Original files are forwarded without parsing, rendering, or executing SVG content. The existing host, size, permissions, timeout, and disabled-mention protections remain. Regression validation covers SVG acceptance and unsafe source/size rejection. All 36 tests and strict format/Clippy passed locally and on VPS; native build, PNG/schema checks passed. Services active, build 1791994264, command schema unchanged at 94. No commit or push.

Attachment validation fix deployed: regular and ephemeral-attachments Discord CDN paths are accepted. Missing/generic MIME metadata uses recognized image/video/audio filename extensions (case insensitive); explicit non-media MIME values remain rejected. Errors distinguish size, source URL, and unsupported format. Regression test covers a signed ephemeral 600 KiB PNG with generic/missing MIME, uppercase extension, and executable/HTML rejection. All 36 tests, strict Clippy/formatting and native build passed. Bot/Lavalink/cipher active, build 1791994782; schema remains 94 commands. No commit or push.

Community toolbox deployed: /media-panel previews up to three uploads and requires initiating-user confirmation; /schedule-post persists bounded media/text with atomic latest-payload claims, edit/cancel, current permission checks and no ambiguous replay; /event creates native RSVP events, reminders and optional permission-matched temporary rooms with empty-room cleanup; /mod-review and new report/appeal buttons implement atomic assignment and assignee-only resolution; /server-health supplies private resource cards and opt-in sustained alerts/recovery; /privacy erases and opts out of activity counters. /help search/check_command provide search and permission diagnostics within Discord's 100-command limit. Native API verifies 100 guild registrations, no globals/duplicates/missing features, file attachment options, report kind and help options. All five new database tables initialize; queue/events/opt-outs/alerts are initially empty. 39 tests, strict Clippy/formatting, optimized build, schema and PNG checks passed locally/on VPS. Event and health cover previews visually reviewed. Services active, presence roster synchronized; build 1791996143. Automatic registration disabled after verification. No Git commit/push. See TOOLBOX.md for configuration and retention.

Release 0.2.0+build.20261014.165200 deployed. Public release IDs follow SemVer MAJOR.MINOR.PATCH with build.YYYYMMDD.HHMMSS UTC metadata. /version and installed --version agree; Discord build timestamp uses the separate epoch. Reproducible SOURCE_DATE_EPOCH values are supported and invalid values fail clearly. A SemVer parser/calendar regression check passes along with the other 39 tests on Windows and Linux. Strict lint/format, locked optimized build, schema and PNG checks passed. Services healthy; 100 commands verified with zero globals/duplicates/missing features. This source release and the prior media/toolbox fixes are included in the user-authorized Git commit/push.
