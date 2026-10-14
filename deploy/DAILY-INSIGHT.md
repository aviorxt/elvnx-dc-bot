# Daily insight

Enable `/activity enabled:true`, then `/daily-insight enabled:true channel:YOUR_CHANNEL`.
Disable reports with `/daily-insight enabled:false`. Configuration persists on the VPS.
The destination must permit the bot to view/send messages, embed links, and attach files.
Only members with Manage Server can configure delivery. Select a channel with an
appropriate audience for activity statistics.

Reports post at **23:50 Asia/Kolkata (18:20 UTC)** with a five-minute delivery window.
The PNG uses the existing monochrome doodle style and bundled Space Grotesk,
IBM Plex Mono, and Kalam fonts. It shows today's message count, total connected
voice time, active members, and three message leaders. The embed lists up to five
members, ranked by message count and then voice time. Mentions never notify users.

Metrics cover the current IST date through posting time, including counting-channel
messages, while tracking is enabled and the bot can observe events. They exclude
bots/webhooks, do not import history, and measure voice connection rather than speech.
Voice intervals crossing midnight are divided between the correct IST days.
Daily totals are stored for 30 days; existing lifetime totals are retained.
The first report may contain only part of the day. Tracking disabled or downtime
creates gaps. Active sessions begin on observed voice updates; a crash can lose
the last unflushed interval (up to 30 seconds).

SQLite claims each guild/date before sending. Restarts do not duplicate delivery;
a failed/ambiguous send or crash after claiming is recorded or remains claimed
for operator review, without automatic retry. If offline beyond the five-minute
window, the bot skips that day's report. The report is not enabled until a channel
is selected through the command.

Preview: `target/debug/elvnx-bot --preview` creates sample report artwork with
illustrative values, without connecting to Discord.
