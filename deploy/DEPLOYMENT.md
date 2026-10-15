# Live update — opt-in presence API and member URL command

Deployed **0.9.0+build.20261015.141003** on 15 October 2026 at approximately **7:46 PM IST**.
Installed SHA-256: `3551fec6c7c2bd6cb10e9201559a8194201dd8e0e918993427f6890ade6fc9ed`.

All **86 tests**, formatting, strict all-target Clippy, native configuration/schema and preview rendering passed. A private SQLite backup, root-only environment backup and previous executable are available for rollback. No Git commit or push was performed.

`/presence` returns the caller's public URL; optional `member` selects another consenting guild member. `action` defaults to Url. Enable, Disable and Info are self-service only and reject the member selector. Enable requires explicit confirmation after a disclosure of public profile, device, status and activity sharing. The command replies privately. Deployment did not opt anyone in; the live consent table contained zero rows at verification.

Cloudflare account AXT hosts the dedicated healthy `elvnx-presence` tunnel. DNS and its public route publish `https://presence.11x.dpdns.org` to `http://127.0.0.1:4100`. Origin binding was verified as loopback-only, with no new inbound VPS port. The tunnel token is root-owned mode 600 outside the repository. HTTPS certificate validation succeeded. The health endpoint returned 200; an unconsented member's REST request and WebSocket upgrade both returned 404 with no-store and read-only CORS headers. Cloudflare reported dynamic responses.

Discord API verification confirmed 100 guild commands and exactly two guild-install-only globals (24-7 and presence), no duplicate names, and the optional member/action/confirm schema. Automatic registration was reset to false afterward. Bot, tunnel, Lavalink and yt-cipher services are active. Existing music boards, two favorites, recap configuration and the currently configured persistent voice target were preserved; one idle Lavalink player remained connected and self-deafened. No playback was initiated.

Real member opt-in, live activity delivery and live revocation were not exercised by deployment because those require a member's voluntary consent. Unit checks cover consent persistence/revocation failures and activity redaction. Streams check every two seconds; the shared Tunnel peer has an aggregate 120-request/minute limit. See [PRESENCE.md](PRESENCE.md) for the API protocol, privacy limits and usage.

---

# Live update — direct /24-7 command

Deployed **0.8.1+build.20261015.122048** on 15 October 2026 at approximately **5:57 PM IST**.
Installed SHA-256: `b4a73a6662a439c8dde419efbfeb9b0104d70ed2d344cfe9bffc30c33674a678`.

All **81 tests**, strict all-target Clippy, native configuration check and preview rendering passed locally/on the VPS as applicable. A fresh private SQLite snapshot and previous binary are available for rollback. No song was playing before restart.

The direct `/24-7` command is registered with required Enable/Disable choices and an optional normal-voice-channel selector (required by the handler when enabling). Manage Server is required both in the command default and runtime checks. It shares the existing persistent-mode handler. The old /playlist always-on entry remains available.

Discord API verification confirmed 100 guild commands plus exactly one distinct global alias named 24-7, no duplicate names, Guild-only interaction context and Guild installation context. The framework retains its configured ELVNX-PVT guild check; DMs and user-installed invocation are unavailable. Automatic command registration was reset to false after verification.

After restart, Discord confirmed the bot connected to Kona and self-deafened; Lavalink reported one idle player and no playing players. Existing music request/favorites board IDs and covers were preserved, as were the saved favorites and recap setting. Bot, Lavalink and yt-cipher services are active. Human invocation of Enable/Disable was not simulated; registration schema, shared handler policies and live persisted presence were verified. No Git commit or push was performed.

---

# Live update — community music and 24/7 presence

Deployed **0.8.0+build.20261015.120218** on 15 October 2026 at approximately **5:40 PM IST**.
Installed SHA-256: `42bb992ccf18e3517d03822b64175a464b0b71a9d74ff5b5029795ea48a83af4`.

All **80 tests** and strict all-target Clippy passed on Windows and the native optimized VPS build. Native configuration/schema and preview checks passed. A fresh private SQLite snapshot was taken before the stopped-service configuration update; the v0.7.0 executable remains as the previous binary.

Created the user-approved #music-requests (`1560260576263929906`) and #playlist-fav (`1560260578457690163`) under Legacy (`1554097393463599115`), copying category permission overwrites and applying five-second slowmode. Both bot-owned permanent boards were verified live with illustrated covers and their expected buttons. Request board message: `1560263551900459140`. Favorites board message: `1560263569206153246`. No sample bookmarks were inserted by deployment; a subsequent read-only check observed one saved favorite from live server activity.

Persisted the approved 24/7 target Kona (`1559607582090461326`). After process start, the bot automatically joined Kona, and Discord confirmed that it remained connected and self-deafened. Lavalink showed one idle player and zero playing players. No user was moved and no playback was initiated by deployment. Recaps are enabled. The AFK system's existing bot exclusions were checked.

Registered exactly 100 guild commands and zero globals, with no duplicate names. /playlist now includes favorites and always-on alongside save/play/list/delete. Automatic command registration was reset to false. Bot, Lavalink and yt-cipher services are active. New schema holds only board IDs, bounded request receipts/throttle metadata, shared favorite URLs/ownership and persistent channel configuration; recap totals remain in memory.

Unit checks cover pause-aware timing, track-start/completed/skip/failure separation, request deduplication, shared search throttling, favorites URL normalization/scoping/capacity, compatibility defaults and staff permission requirements. Live modal submission, favorites playback, real audio and automatic recap delivery have not been exercised by the agent against members. Shared favorite saving was observed through the live count. No Git commit or push was performed.

Details: [MUSIC-COMMUNITY.md](MUSIC-COMMUNITY.md).

---

# Live update — premium music controls

Deployed **0.7.0+build.20261015.111643** on 15 October 2026 at approximately **4:50 PM IST**.
Installed SHA-256: `bd430bfea0223929e435501cdc8e448e688fa97a8110123200100f8eaf8cb808`.

All **72 tests** and strict all-target Clippy passed locally and on the native optimized VPS build. Configuration/schema check and native image previews passed; the Listening Room preview was visually inspected. A fresh root-only SQLite snapshot was taken before deployment and the v0.6.0 executable remains available as the previous binary.

The dedicated Listening Room card, animated controls, progress/queue preview, seek validation, six volume presets, full 50-entry queue viewer and specific action confirmations are installed. The new panel appears on the next /play session. No active player existed before restart, and deployment did not start playback or move/message test members. Live playback, actual seek transitions, DJ interactions and audio still need member testing.

Bot, Lavalink and yt-cipher services are active; the bot connected to Lavalink after restart. A read-only authenticated loopback health check confirmed reachability and zero active/playing players. Existing voice panel and private activity header retain their IDs, covers and animated icons. The activity header version refreshed in place to 0.7.0. No slash command names/options were added; automatic registration remains disabled. No Git commit or push was performed.

Details: [MUSIC-PANEL.md](MUSIC-PANEL.md).

---

# Live update — voice lounge and activity desk

Deployed **0.6.0+build.20261015.104451** on 15 October 2026 at approximately 10:48 UTC.
Installed SHA-256: `6c26a2ad21c17a40f40aaf0c752f8ecd180f7e4449bcb7f9bebd06ab13efc24e`.

All **66 tests** and strict all-target Clippy passed on Windows and the native optimized VPS build. Native configuration check and generated-card preview passed. A root-only SQLite snapshot was taken before deployment; the previous binary remains available for rollback.

The original voice control message `1559873065247182890` in channel `1559872940084957184` was upgraded in place: VOICE LOUNGE title, animated author icon, illustrated cover and 15 controls. Lobby/category inspection confirmed the original lobby's Speak/Voice Activity/Stream denies; new rooms now use category permissions. No temporary rooms existed at deployment, so no live room repair or member movement was performed.

The existing private activity channel `1557909674194501703` now contains the versioned ACTIVITY DESK header `1560242851370238036`, with its illustrated cover and animated icon. Explicit @everyone View Channel denial and the bot's audit permission were verified. Expanded gateway logging and structured audit records are enabled; legacy duplicate server notifications were removed. Human message edits/deletions, microphone audio, invitation delivery/acceptance and ownership interactions were not exercised against real members during deployment.

Discord registration verified 100 guild commands, zero global commands, no duplicates and no missing features. Automatic registration was reset to false afterward. Bot, Lavalink and yt-cipher services are active, and the bot reconnected to Lavalink. The 40 animated and 40 static custom emoji and the honeypot warning remain present. No Git commit or push was performed for this release.

See [VOICE-LOUNGE.md](VOICE-LOUNGE.md) and [LOGGING.md](LOGGING.md) for coverage, permission policy and practical limits.

---

# Deployment record — 15 October 2026

VPS: `13.203.104.97` / SSH user `ubuntu` / Ubuntu 26.04 LTS, x86-64.

Installed an optimized Linux executable and the `elvnx-bot` systemd service.
The service is enabled for reboot, runs as `elvnx`, and is active. It connected
as **avior's Core** (bot ID `1474265288223621215`) in ELVNX-PVT, with all 100 guild
commands registered. Discord Member and Message Content intents were already
authorized in this application's flags and are enabled in the VPS environment.
No external interactions endpoint is configured, so commands use the gateway.
Automatic registration is disabled after successful initial registration.

Validated:

- 58 unit tests passed on Windows and the VPS, including poll vote closing/fair scheduling, once-only job claims, restore/playlist ownership, repeat behavior, and the existing moderation checks.
- Strict Clippy and formatting checks passed on the VPS.
- Optimized Linux build passed using locked dependencies.
- Database schema initialization passed as the service user.
- PNG rendering passed in a transient systemd service with filesystem restrictions.
- Linux welcome preview visually matches the local doodle design.
- Unit configuration verified, reboot startup enabled, no public bot port opened.
- Font license files installed alongside the executable.

Binary SHA-256:
`f18c4e1c58561953145a05aacf22bed444924a44a356c95b8e618c48ae51028b`.

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

15 October 2026: error-handling patch 0.2.1+build.20261015.081310 deployed. All commands use a shared private error handler for validation, permissions, cooldowns, parsing, Discord HTTP/model errors, database/network/storage failures and unexpected faults. Application validation is explicitly typed; arbitrary technical error text is not echoed into Discord. Expected errors explain next steps; support references correlate with private logs. Music voice preconditions run before audio-service checks. Forms and controls use the same safe mapper, including media, reporting/review, verification, voice and music. 47 tests and strict format/Clippy passed on Windows/VPS; native optimized build, schema and PNG checks passed. Bot/Lavalink/cipher active, 100 commands still registered with zero globals/duplicates/missing features; presence roster synchronized. Matching service unit hash verified before routine daemon-reload to clear stale systemd metadata. No Git commit/push for this patch. See ERROR-HANDLING.md.

15 October 2026: messaging feature release 0.3.0+build.20261015.082507 deployed. /say and existing /dm now accept optional Boolean embed: true sends the styled ELVNX message; false sends plain content. Default true preserves previous behavior. Both retain Manage Server checks, disabled mentions and 2000 UTF-16 text bounds. /say checks Embed Links only when embedding, allowing plain sends without that permission. /dm retains current-human-member validation, private moderator identity, closed-DM guidance, and message-ID deletion tracking. Serializer regression verifies mutually exclusive raw/embed bodies and disabled mentions, blank/oversized rejection, and boundary acceptance. 48 tests and strict lint/format passed on Windows/VPS; locked native build, schema/PNG checks passed. Discord API verifies optional Boolean embed fields on both commands, 100 total commands, no globals/duplicates/missing features. Services active; registration disabled after verification. No Git commit/push.

15 October 2026: messaging option-order correction 0.3.1+build.20261015.083621 deployed. /say registers embed (Boolean, required), message, channel; /dm registers embed (Boolean, required), message, member. Explicit true/false is required to keep the choice before required message text under Discord's option ordering rules. Message rendering, permissions, mention suppression and DM tracking are unchanged. 48 tests, strict Clippy/formatting, native build, schema and PNG checks passed. Discord API verified exact option sequences/types/required flags, 100 commands with no globals/duplicates/missing features. Services active; registration disabled after verification. No Git commit/push.


Verification/custom emoji release deployed: `0.4.0+build.20261015.092816`. Existing #verify-here panel 1559876275370205246 in channel 1559876138967498793 upgraded in place and bound to its policy. Reuses zero-permission Legacy role 1554098593382531112 and private staff report channel 1559929296062255157. Seven-day account-age review threshold, ten-minute join wait, expiring one-time challenge, accessible manual review, native membership-screening checks, atomic approvals and self-role/autorole bypass prevention enabled. Existing member assignments preserved. Root-only database snapshot saved before policy migration. SQLite cleanup runs every minute; see VERIFICATION.md for retention and the limits of alt-risk screening.

All 24 original application emoji uploaded and confirmed. Custom check reaction tested against the existing panel (HTTP 204 add/remove), then removed. Buttons, message embeds, music controls and automatic reactions use the pack. Updated panel has Verify access and Staff help buttons plus a matching generated cover. 53 tests, formatting, strict Clippy, optimized native build, schema check and PNG rendering passed on the VPS; the same 53 tests passed locally. Discord confirms 100 guild commands, zero globals/duplicate names, with extended /verify options. Bot and Lavalink are active; REGISTER_COMMANDS=false after registration. No member role was granted or removed during validation; a complete human/new-member interaction test has not been performed. Git remains uncommitted and unpushed.


Premium motion/approval/honeypot release deployed: `0.5.0+build.20261015.100526`. 40 original animated glyph emoji and 40 static variants confirmed through Discord's application API. Glyph artwork moves while badge edges remain still; GIF frame changes, stationary bounds and upload size limits validated locally. Context-specific icons, footers and timestamps now distinguish music, messages, help, moderation, setup and activity. The active verification panel refreshed in place with animated buttons and author icon, preserving its policy and existing member roles.

Successful staff approvals now send one matching member DM with a generated card and server link. Blocked DMs do not reverse role access; staff replies and the review embed report delivery status. Staff notes and screening flags remain private. The existing #honeypot (1557907883797254204) warning 1560230550802006037 was upgraded in place with an animated warning icon and matching cover. Fresh warning/permission/hierarchy checks, durable duplicate suppression, truthful notices, private staff/owner alerts and bounded matching-reason recovery are active. Log destination remains 1557909674194501703 and explicitly denies everyone View Channel. No test member was banned or sent a test approval DM.

58 unit tests, strict Clippy, formatting, optimized Linux build, schema initialization and native PNG previews passed locally/on the VPS. Both services active; 100 commands remain registered with registration disabled. Private SQLite snapshot saved before deployment. New action-table retention is documented in HONEYPOT.md; approval DM receipts follow outgoing_dms handling. A full human approval/softban test has not been performed. All Git changes remain uncommitted and unpushed.
