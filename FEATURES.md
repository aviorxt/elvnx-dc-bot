# Community and music additions

All commands are scoped to ELVNX-PVT. Replies containing settings, report receipts, reminders, playlists, and backups are private. Public posts suppress mentions. The new features do not create channels or post announcements until configured.

| Command | Purpose | Access |
| --- | --- | --- |
| `/settings-panel` | Private interactive dashboard: automod, invite blocking, activity, welcome, logs, AFK moves, reactions, join announcements, daily reports/quotes, birthdays, private reports, live boards | Manage Server; live-board setup also requires Manage Channels |
| `/role-panel channel role_1 … role_5` | Post persistent toggle buttons for approved public roles | Manage Server + Manage Roles |
| `/poll question options closes channel` | Button poll, 2–5 choices separated by `\|`, automatic closing | Members with destination View/Send |
| `/poll-close id` | Close early and publish result image | Author or Manage Messages |
| `/poll-result id` | Private tally/result card | Members who can view the poll channel |
| `/remind text when channel` | Personal DM reminder, or staff channel reminder | Channel delivery requires Manage Server |
| `/reminders`, `/reminder-cancel id` | List/cancel your own reminders | Member |
| `/report` | Confidential modal with details, optional member ID and message link | Member |
| `/setup-report channel enabled` | Select a strictly private staff report channel | Manage Server |
| `/config-backup` | Download settings JSON and save a local snapshot | Manage Server |
| `/config-import file` | Validate and preview an exported JSON attachment | Manage Server + Manage Roles |
| `/config-restore id confirm` | Preview first, then explicitly apply a reviewed snapshot | Manage Server + Manage Roles |
| `/birthday month day`, `/birthday-remove` | Opt in/out with day and month only | Member |
| `/birthday-setup channel enabled` | Birthday cards at 08:00 IST | Manage Server |
| `/dj-role role enabled` | Restrict playback controls to DJ role or Manage Server | Manage Server |
| `/vote-skip` | One vote per listener; more than half of current human listeners must agree | Same voice channel |
| `/shuffle`, `/repeat mode` | Shuffle waiting tracks; repeat off/track/queue | Same voice channel + configured DJ policy |
| `/playlist save name` | Save current track plus waiting queue, up to 50 URLs | Same voice channel; saves privately |
| `/playlist play name` | Queue your own saved songs | Same voice channel |
| `/playlist list`, `/playlist delete name confirm` | List/delete your own playlists | Member |

## Setup

Start with `/settings-panel`. For role panels, first approve each genuinely public role using `/selfrole_allow`, then run `/role-panel`. Do not approve roles that grant private category access. Buttons recheck the allowlist, current permissions, and bot hierarchy on every click; revoking approval disables old grants.

With no channel selected, `/setup-report` creates or reuses a private `member-reports` channel for administrators and moderators. Creation requires the caller and bot to have Manage Channels. Selecting an existing channel preserves its permissions and rejects non-staff access with an actionable explanation. Reports require a text channel with an explicit everyone View Channel deny and only staff access grants. A separate staff-only channel is appropriate: the existing read-only log role is not a staff role and must not receive confidential reports. Both configuration and delivery recheck privacy. Reports include a receipt ID, reporter, selected member, details, and source link in one private embed. They never automatically punish members. Report details stay in Discord; SQLite stores only delivery metadata. Maximum three submissions per member per 24 hours. There is no automatic resend after an ambiguous delivery failure.

Birthday announcements are opt-in, at 08:00 IST within a five-minute online window. No birth year is collected. February 29 is observed on February 28 in non-leap years. Cards use Discord avatars and bundled Space Grotesk, IBM Plex Mono, and Kalam fonts with black/white doodles. Removing the opt-in stops future jobs; earlier Discord posts remain. Calendar jobs do not backfill missed birthday announcements.

Reminders accept `10m`, `2h`, `1d`, or `YYYY-MM-DD HH:MM` in IST, up to 365 days ahead. Limits: 10 pending reminders/member and 200/server. Delivery runs every 30 seconds, persists across restarts, and rechecks membership/access. Reminders more than 24 hours overdue expire. Jobs are claimed before delivery, so a crash or ambiguous network failure can leave a job unsent instead of duplicating it. Blocked DMs appear as failed delivery.

Polls allow one changeable vote/member until closing, two active polls/member and 20/server. Counts update every 30 seconds; closing disables buttons and renders a results card. Deleted poll messages are marked missing rather than recreated. Recorded voters are not shown publicly.

## Backups and retention

Backups contain bot settings only, not tokens, OAuth credentials, member reports/activity, message content, reminders, polls, birthdays, playlists, live-board resource IDs, server structure, or the root-owned automod rules file. Imports accept only bounded Discord-hosted attachments without redirects. They must belong to the same guild and pass current role/channel validation. Automatic/public roles must already be approved; restoration cannot introduce new role grants. Review is bound to the acting staff member, backup ID, current configuration, and a ten-minute expiry. Any intervening settings change requires reviewing again.

SQLite keeps the latest 20 settings snapshots/server for up to 30 days. Closed polls/votes, completed/failed reminder payloads, and report metadata are removed after 30 days. Pending reminders remain until delivery/cancellation. Birthday claims older than two years are removed; opt-ins remain until removed or the member leaves. Playlists are private to their guild/member, limited to 20/member and 100/server, and remain until deleted or the member leaves. They store source URLs only. Discord posts/attachments follow the server's own retention.

## Music behavior

The music panel adds Shuffle, Repeat mode, and Vote skip buttons and displays repeat mode/vote count. DJ restrictions cover pause, resume, direct skip, stop, volume, shuffle, and repeat. Play, queue inspection, and vote skip remain available to listeners. Staff controls still require joining the bot's voice channel. Votes are unique to the current track and reset on advance/stop. Repeat applies only to a naturally finished track; errors and manual skips advance normally. Queue repeat cycles the completed song behind waiting tracks with a fresh request identifier. Stop resets repeat and clears votes, and deletes the panel.

Playlist loading is limited to supported HTTPS source URLs, 50 tracks, a 30-second total loading deadline, and the existing queue cap. Unavailable songs abort the request before enqueueing. The shared control panel shows requested tracks; saved playlists themselves cannot be read or deleted by another member's commands. No playlist URL is fetched directly by the bot; the private Lavalink node resolves validated sources.

Use `--preview` to render birthday and poll cards locally without connecting to Discord. Preview counts are illustrative.

Imported backups cannot newly enable antinuke or move the honeypot to a new channel; use those commands' explicit setup confirmations first. Closed-poll retries rotate in bounded batches independently from active polls, so failed result messages do not block live tallies.
