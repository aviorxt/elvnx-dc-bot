# ELVNX / 11X Discord bot

A standalone Rust moderation and music bot for ELVNX-PVT, using Poise/Serenity, SQLite, and a private Lavalink node. The AVRXT website is a separate project. This version has 93 top-level slash commands. See deploy/README.md for the Ubuntu VPS setup. A dedicated Discord token and server invitation are required for live operation.

## Commands

| Area | Commands |
| --- | --- |
| Information | `/help`, `/ping`, `/version`, `/userinfo`, `/server-info`, `/avrxt-dev`, `/email` |
| Moderation | `/warn`, `/warnings`, `/delwarn`, `/case`, `/timeout`, `/untimeout`, `/kick`, `/ban`, `/unban`, `/purge` |
| Channel administration | `/slowmode`, `/lock`, `/unlock`, `/channel-management`, `/server-lockdown`, `/log` |
| Roles | `/role_add`, `/role_remove`, `/selfrole_allow`, `/selfrole_remove`, `/selfrole` |
| Setup | `/configure`, `/automod` |
| Support | `/ticket`, `/ticket_close` |
| Messaging | `/say`, `/dm`, `/dm-clear`, `/sticky`, `/sticky_remove` |
| Community | `/number-game`, `/count-toggle`, `/join-to-create`, `/afk-move`, `/always-react`, `/appukuttan-legacy` |
| Server owner | `/bot-restart` |
| Presence | `/status`, `/bot-status`, `/uptime`, `/availibilty-panel` |
| Quotes | `/quote`, `/quote-daily`, `/daily-insight` |
| Membership | `/verify`, `/welcome-setup`, `/rolelist`, `/member-list`, `/bot-list` |
| Activity | `/activity`, `/insight` |
| Protection | `/antinuke`, `/honeypot`, `/automod-reload` |
| Music | `/play`, `/pause`, `/resume`, `/skip`, `/stop`, `/queue`, `/volume`, `/nowplaying`, `/lavlink-status` |

See [FEATURES.md](FEATURES.md) for settings panels, public role panels, polls, reminders, confidential reports, reviewed backups, birthdays, and DJ/playlist music controls.

## Expanded features

`/status` sets Playing, Watching, Listening, or Competing with custom text. `/bot-status` selects Active, Dnd, Sleep (Discord Idle), or Invisible. Settings persist on restart. This is standard bot presence; Discord user-client Rich Presence artwork/buttons are not provided by this API.

`/quote` selects an original ELVNX community quote from a local collection; it does not copy unattributed quotes from random third-party APIs. `/quote-daily` posts at **08:00 Asia/Kolkata (02:30 UTC)** with a five-minute delivery window. The scheduler stores a durable per-day claim before sending to avoid duplicate posts across restarts. It does not send late catch-up posts, and a crash/ambiguous network failure after claiming can leave that day's quote unsent; the database marks failures for operator review.

`/verify` posts an embed and button for rules acknowledgement. The role must have no global permission grants, be below the bot, and be explicitly confirmed as access available to every member. This is not CAPTCHA, real-world identity verification, or an anti-alt guarantee. Old panels stop working after a different whitelist role is configured. Permissions are rechecked on every click.

`/welcome-setup` saves welcome text and enables custom 850×470 PNG cards with the joining user's Discord avatar, matching the site's black/white palette with hand-drawn borders, stars, arrows, dotted texture, and handwritten accents. Space Grotesk, IBM Plex Mono, and Kalam are bundled under SIL Open Font Licenses so Linux rendering does not depend on installed desktop fonts. Avatar HTTP requests are HTTPS-only, limited to Discord CDN hosts, have redirects disabled, a five-second timeout, and a one-megabyte response cap. SVG user text is escaped before rendering. A missing avatar falls back to the brand layout. `cargo run -- --preview` renders sample cards into previews/ without connecting to Discord; sample totals are illustrative.

`/activity enabled:true` opts in to future human message counts and voice-connected duration. `/insight` shows self or a selected user's PNG card with message leaderboard. These totals cover only events the bot can see while online and enabled. Voice time measures connection, not speech; it includes muted/deafened sessions and excludes bots. Voice progress is flushed every 30 seconds; a sudden shutdown can lose the last interval. Existing sessions begin tracking on their next voice-state event. Counts are not automatically imported from history. Disabling tracking stops new counters but does not erase earlier totals; an operator can review retention in SQLite.

`/member-list` and `/bot-list` scan up to 25 guild members after an optional cursor, displaying up to 25 results. Enable Server Members Intent for the REST member listing. Bot-list filters each scanned page; it does not claim to list every bot in one response. `/rolelist` pages through roles in groups of 20.

`/antinuke` is explicitly opt-in and requires confirmation. It watches fresh audit entries for channel/role/webhook deletion, kicks, and bans; four actions by one actor within 15 seconds trigger containment by default. It removes non-managed roles granting destructive permissions only when the actor is below the bot. The owner, this bot, and configured trusted actor IDs are exempt. It cannot contain the owner, higher roles, managed integration grants, or permissions granted directly through everyone. No deleted channels/roles are recreated automatically. The owner receives an alert and the private log records outcomes. Add legitimate maintenance bots to trusted_actor_ids before enabling. The bot needs View Audit Log, Manage Roles, and suitable role placement.

`/honeypot` must be configured in a **dedicated marked trap channel**, not ordinary chat. The command posts a warning and requires confirmation because any non-exempt poster can be removed, including a legitimate person. The owner, administrators/managers/moderators, this bot, webhooks, and trusted actor IDs are exempt. It DMs the user, bans with zero days of message deletion, immediately unbans, and DMs the owner plus writes the private log. If unban fails, it tries one short repair retry and explicitly alerts that the ban remains and needs manual removal. DMs can be blocked. It never claims ban/unban is an atomic Discord operation.

`automod.json` contains validated configurable thresholds, link allow/deny domains, word matching, caps, emoji/attachment/message-size limits, staff/channel exemptions, and trusted antinuke/honeypot actors. `/automod-reload` reloads the file; `/automod` still controls enablement and invite/word settings. Defaults are conservative and no automatic bans are applied by content automod. Only a fixed local file is read; commands cannot choose arbitrary filesystem paths.

`/say` posts text or a media URL in any accessible text channel in this server; it requires Manage Server. Both sender and bot must be allowed to view/send in the destination. This command does not bypass private-channel permissions. `/dm` sends a human member a message without the moderator name, and stores sender/recipient/time in private audit history without saving DM text. Discord can reject DMs when recipients block the bot or disable DMs.

Stickies are persisted, delete their previous bot message, and repost after human activity. A two-second coalescing window reduces message churn and rate limits during bursts. They recover configuration and their message IDs after a restart. They are not Discord pinned messages. Concurrent posts and API outages can temporarily prevent the sticky being the final message.

`/bot-restart confirm:true` checks the live Discord guild owner, not an admin role. It closes SQLite and exits with code 75. The provided supervisor restarts only this deliberate exit; a standalone bot refuses the command. This does not run arbitrary shell commands.

## Local setup

1. Create a dedicated Discord application and bot at https://discord.com/developers/applications. Put the bot token in a local `.env` copied from `.env.example`. Never put it in a message, screenshot, source file, or commit. This bot cannot use a normal Discord user token.
2. Keep `DISCORD_GUILD_ID=1554095197959491595` for ELVNX-PVT, or replace it with your own guild. The global command check and gateway handlers enforce this one-guild scope.
3. Invite it with the `bot` and `applications.commands` scopes. Grant View Channels, Send Messages, Read Message History, Embed Links, Attach Files, Manage Messages, Manage Channels, Manage Roles, Moderate Members, Kick Members, Ban Members, Move Members, and Add Reactions according to the features you enable. **Administrator is not required.** Place its role above members/roles it should manage and below your administrative roles. Restrict the log channel to trusted staff.
4. For welcomes, join/leave logging, and autorole, enable Server Members Intent in the Developer Portal and set `ENABLE_MEMBER_EVENTS=true`. For content automod and the counting game enable Message Content Intent and `ENABLE_MESSAGE_CONTENT=true`. Stickies use message events and do not require reading message content. Presence intent is not used.
5. Set `REGISTER_COMMANDS=true` for the initial startup of this dedicated bot application, then set it back to false after commands appear. Registration replaces this application's guild command set; it does not change other bots' commands. Never reuse another bot's token without intending to replace its commands.
6. Run from this directory:

```powershell
Copy-Item .env.example .env
# Edit .env locally and add the bot token.
.\scripts\dev.ps1 build
.\scripts\supervise.ps1
```

Rust 1.99 GNU is installed for this local workspace. `scripts/dev.ps1` uses the checksum-verified w64devkit GNU C compiler in `../work/rust-tools` for native SQLite/TLS dependencies. On another Windows machine use a standard Rust MSVC installation with Visual Studio C++ build tools, or install a GNU toolchain and C compiler. On Linux install Rust and standard build essentials, then use `cargo build --locked` and `cargo run --locked`. Keep SQLite on a local persistent disk, not a shared network filesystem, and run only one instance against this database.

`cargo run -- --check` initializes and verifies the local database schema without a bot token or Discord connection. `scripts/dev.ps1 test` runs local tests; `scripts/dev.ps1 clippy` runs linting. Ctrl+C shuts down the gateway.

## Server configuration

Use `/configure` with an explicit setting and a channel/role ID, or `off` to clear it. Settings: LogChannel, WelcomeChannel, WelcomeMessage, AutoRole, TicketCategory, TicketStaffRole. Welcome text supports `{user}` (username, no ping) and `{server}`. Settings are stored per guild and cached in memory after startup; messages do not read configuration from the database on each event.

Create a dedicated private ticket category and choose a trusted staff role. Tickets explicitly deny everyone visibility and grant access to their owner, bot, and staff role. Server Administrators still bypass channel overwrites. One open ticket is allowed per member. Closing a ticket makes the owner's access read-only and preserves history; no automatic deletion or claim of erasure is made.

Self-service roles must be explicitly allowlisted, un-managed, have no global permission grants at use time, and be below the bot. **A zero-permission role can still unlock private categories through channel overwrites. Only allowlist roles whose channel access every member should be able to claim.** Do not allowlist adminr, avr-pvt, pvt-acess, log-acss, moderator, or bot integration roles just because they exist. Autorole similarly distributes only un-managed zero-permission roles and should only grant common access. Ordinary moderator assignment checks both actor and bot hierarchy and rejects privileged roles.

Automod is disabled by default. When enabled, it detects more than five messages in eight seconds, more than five mentions, optional Discord invite strings, and configured whole-word matches. It deletes matching messages and records the reason, without automatic bans or timeouts. Server owner and members with Administrator, Manage Messages, or Moderate Members are exempt. It is a simple local rule engine, not a phishing reputation service or Discord native AutoMod manager. Unicode obfuscation and alternate invite domains are not comprehensively detected.

Locks save and restore the existing everyone overwrite instead of granting send access blindly. Role/member-specific send grants can still bypass an everyone lock; Administrator always does. Avoid editing the everyone overwrite manually while the bot lock is active, because unlock restores its saved snapshot.

## Storage and security

SQLite stores config, warning/case metadata, ticket ownership/channel IDs, saved lock/lockdown overwrites, counting state, temporary voice-room ownership, outgoing bot DM IDs, activity totals, quote delivery claims, and sticky text/current message IDs. Warning reasons are retained until a moderator revokes the warning (revocation preserves the audit entry). With staff logging enabled, up to 1,000 observed message snapshots are held in memory for up to 24 hours. Edits/deletions copy available text and attachment links to the configured staff channel, where Discord retains those logs independently of the original message. Long text is attached as a transcript. No message bodies are stored in SQLite; DMs are excluded. The in-memory history disappears on restart. Private tickets remain in Discord after closure. Database backups and Discord staff channel access are your responsibility; the database has no application-level encryption or automatic retention/deletion schedule.

All command responses are ephemeral except the explicitly requested destination messages. User text cannot ping everyone, roles, or users through bot messages. Inputs have bounded lengths, SQL uses bound parameters, config mutations are serialized, automod state is time-limited/bounded, and high-impact commands require confirmation flags. Discord actions can succeed even if local audit storage subsequently fails; the moderator is told this explicitly. No bulk retry is made for ambiguous moderation or DM outcomes.

Secrets are loaded from the ignored environment. No token was generated or configured during local development. Live permission behavior, message events, DMs, and sticky movement require a test bot/server before production use. Never enable all permissions or automod against members without first testing your policy.

Not included yet: reaction/button role panels, timed reminders, giveaways, leveling, external dashboard, scheduled bans, antiraid reputation feeds, and multi-server hosting.

Primary references: https://docs.rs/poise/latest/poise/ and https://docs.rs/serenity/latest/serenity/ ; https://discord.com/developers/docs/events/gateway#gateway-intents .

## Additional community controls

`/email` lists support@11x.dpdns.org and admin@11x.dpdns.org. `/help page:1` lists every registered command and paginates descriptions. Standard responses, public posts, stickies, and logs use monochrome embeds; insight/welcome cards are dynamically rendered PNGs. Discord controls embed fonts and placement; image cards provide custom typography.

`/dm-clear confirm:true` deletes up to 100 tracked messages sent through `/dm`, optionally restricted to a member. Repeat for later batches. It cannot delete recipients' messages, incoming DMs, or untracked messages from earlier versions. DM text is never stored. Failed deletions remain tracked for retry.

`/channel-management` creates text channels, renames channels, edits topics, and deletes selected text channels after explicit confirmation. Creating a channel uses Discord's default permissions; use category permissions appropriately. `/server-lockdown enabled:true confirm:true` saves complete overwrite snapshots, blocks ordinary text/voice access, and keeps the bot able to respond. Run with enabled:false to restore. Administrator bypasses restrictions; moderation roles/member overrides may also be exempt. Snapshot restoration replaces later manual permission edits, so avoid editing overwrites during lockdown. Failed/ambiguous edits keep snapshots for restoration; do not delete the SQLite database while locked.

`/number-game` (alias `/numer-game`) configures an alternating-member counting channel beginning at 1. It requires Message Content Intent. `/count-toggle enabled:false` pauses the game and `enabled:true` resumes the saved channel and count. Mistakes and non-numeric messages are deleted without resetting the total. Concurrent submissions update atomically; only one next number can succeed. Use a dedicated channel. Counting messages are handled separately from ordinary activity counters and stickies.

`/join-to-create` uses an existing voice hub plus a text control panel. New rooms inherit the hub's category/overwrites. Owners use buttons to lock/unlock access and adjust capacity. Role-specific Connect grants may bypass an everyone lock. Rooms are deleted when empty; tracked empty rooms are checked after gateway reconnect/startup. Rooms retain their original owner until deletion. The bot removes only rooms recorded in its database. Keep Move Members and Manage Channels permissions available. No audio is recorded.

`/log` sets the private staff destination. Supported logs include moderation cases, joins/leaves, bans/unbans, cached before/after edits and deleted text, bulk deletion transcripts, channel and role changes, nickname/timeout changes, and voice joins/leaves/moves and mute/deafen/camera/stream changes. It does not retrieve a complete historical server audit or recover uncached content. Deletion events do not reliably identify the deleting actor, so the bot labels the message author separately. Cached bot/webhook message changes and activity within the log channel are excluded to reduce noise and prevent recursive logs. Integration/webhook/presence changes are not all mirrored.

VPS installation and operation: [deploy/README.md](deploy/README.md). The Linux service uses systemd and a separate private state directory; the website remains a separate deployment.



/daily-insight enabled:true channel:YOUR_CHANNEL posts a doodle report card at 23:50 IST after /activity enabled:true. It uses IST day totals, preserves voice time across midnight, and ranks members by messages then voice time. Reports cover online tracked activity through report time. Daily totals expire after 30 days, and each guild/date is claimed once to avoid duplicate sends. See deploy/DAILY-INSIGHT.md and deploy/MUSIC.md for operation.

AFK automation: /afk-move enabled:true channel:DND moves human members after 10 continuous minutes muted or deafened. Active status, leaving, or switching channels resets the timer. See deploy/AFK-MOVE.md for permissions and restart behavior.

Role reactions: /always-react channel:YOUR_CHANNEL adds ✅ to new human messages mentioning role 1554097852672770118 in that channel. Disable with /always-react enabled:false. Settings persist across restarts. See deploy/ALWAYS-REACT.md.

Appukuttan Legacy: /appukuttan-legacy channel:YOUR_CHANNEL posts the exact text ente appukuttan vanneyyyy when user 463980163852664832 joins voice from a disconnected state. Disable with /appukuttan-legacy enabled:false. Moves and mute/deafen changes do not trigger announcements. Settings persist across restarts. No message is sent just by enabling the command, and no user or role is pinged.

Server rules: /rules channel:YOUR_CHANNEL posts one message with a generated 1200x420 ELVNX doodle cover followed by a detailed rules embed. The artwork includes the current server name and IST date, with Space Grotesk, IBM Plex Mono, and Kalam. Eleven sections cover respect, privacy, safety, content, channel use, spam, voice etiquette, sharing, bot tools, reports, and moderation. Manage Server and destination posting access are required; the bot also needs Embed Links and Attach Files. Messages do not ping users or roles. Each invocation creates a new rules post without pinning or replacing old messages. Preview with the executable's --preview option.

Live status: /bot-status-live creates ELVNX BOT STATUS with elvnx-status and lavlink-status channels. It edits one embed per channel every 30 seconds, with saved IDs for restart recovery. Admins can pause using enabled:false. See deploy/LIVE-STATUS.md for metrics, read-only permissions, and freshness behavior.
