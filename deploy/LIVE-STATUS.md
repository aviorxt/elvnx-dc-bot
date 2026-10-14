# Live Discord status boards

Run `/bot-status-live` to create **ELVNX BOT STATUS**, containing:

- `elvnx-status`: bot identity/version, process uptime, gateway heartbeat latency,
  registered command count, server/member count, database health, and feature flags.
- `lavlink-status`: node availability/version/uptime, API response time, player
  counts, memory, CPU load, queue/volume, voice channel, and current playback.

The monochrome embeds update in place every **30 seconds**. No new message is
posted on an ordinary refresh, and no users or roles are pinged. Bot and node
credentials, addresses, database records, and environment values are not exposed.

Manage Server and Manage Channels are required to configure the feature; the bot
needs Manage Channels. New category permissions deny ordinary posting, reactions,
and thread creation. Existing guild-level visibility permissions still apply;
the category does not explicitly grant everyone View Channel. The bot receives
category-scoped View/Read/Send/Embed/Attach permissions. Administrators can still
post and manage the channels through Discord's normal permission bypass.

Channel/category/message IDs are stored in the private SQLite database. Running
the command again reuses tracked resources and refreshes the boards. Restarts
resume updates when enabled. Missing tracked messages are replaced; owned status
messages in the latest 20 messages can be recovered after an interrupted ID save.
Temporary database-save errors retain new IDs in memory and retry persistence.
Discord API ambiguity or a crash before saving a newly created resource can still
require administrator review. Keep the database backed up with its WAL/SHM state.

If a channel or category is manually deleted, rerun the command to repair the
setup. Background updates do not recreate deleted channels. Lost permissions
cause refresh errors in the private service logs. Both boards are attempted even
when one channel fails to update.

Use `/bot-status-live enabled:false` to pause updates and label the existing
messages as paused. Run the command again to resume. Pausing does not delete
channels or messages.

The bot cannot update its own embed when its process is fully offline. Each embed
includes a freshness timestamp and a footer explaining that stale data can mean
the bot/updater is offline. This is a self-reported live board, not an independent
external uptime monitor. Uptime resets on process/node restart. Node metrics and
message edits can be delayed by outages or rate limits.
