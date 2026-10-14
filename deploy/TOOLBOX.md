# Community toolbox

The guild has 100 top-level commands, Discord's limit. Search and permission diagnostics extend `/help` instead of creating more top-level registrations. Future commands should use subcommands or extend existing groups.

| Feature | Usage | Access |
|---|---|---|
| Media collection | `/media-panel channel:... file:... file_two:... file_three:... title:... caption:...` | Manage Server, creator/bot destination access; bot Embed Links and Attach Files |
| Scheduled post | `/schedule-post action:Create channel:... when:"2026-10-15 20:00" message:... file:...` | Manage Server; delivery rechecks creator and bot permissions |
| Queue management | `/schedule-post action:List`, `Edit id:... when:... message:...`, `Cancel id:...` | Manage Server |
| Community event | `/event action:Create name:... when:... channel:... voice:... temporary:true minutes:120` | Manage Server and Manage Events; temporary rooms also require Manage Channels for both creator and bot |
| Event management | `/event action:List` or `Cancel id:...` | Same staff permissions |
| Staff review | `/mod-review action:List`, `Claim id:...`, `Resolve id:... note:...`, `Reopen id:...` | Moderate Members and access to the private report destination |
| Reports and appeals | `/report kind:Report` or `kind:Appeal` | Members; private modal and existing report rate limit |
| Infrastructure snapshot | `/server-health` | Manage Server; private reply with doodle card |
| Health alerts | `/server-health alerts:true channel:PRIVATE_STAFF_CHANNEL`; disable with `alerts:false` | Manage Server; channel must pass the private staff audience check |
| Activity privacy | `/privacy action:Info`, `EraseAndStop confirm:true`, `Resume` | Member's own activity only |
| Command search | `/help search:voice` | All members |
| Permission diagnostic | `/help check_command:send-attachement channel:...` | All members; only channels they can view |

Media-panel previews are private, expire after two minutes, and require the initiating member's confirmation before posting. Up to three 8 MiB attachments use the shared upload validator, including signed temporary Discord URLs and missing MIME metadata. Files stay in memory; native attachments preserve Discord previews/playback. Mentions are disabled. SVGs are forwarded unchanged without parsing/execution. Audit stores destination/message IDs, not media content.

Scheduled posts store text and optional original media bytes in the private SQLite database because Discord's input URLs expire. Limits are 20 pending jobs and 128 MiB stored media per guild. Choose a future time within 365 days, relative (`10m`, `2h`, `1d`) or `YYYY-MM-DD HH:MM` in IST. Editing changes pending text/time only; cancel/recreate to replace media or destination. Delivery runs every minute and verifies current creator/bot permissions. An atomic claim reads the latest edited payload/time. A send interrupted by a crash becomes `uncertain`; failed sends are labeled `failed-check-channel` and never retried automatically because Discord delivery may be ambiguous. Check the channel before recreating. Jobs over 24 hours late expire. Text/media are erased after delivery, cancellation, or failed/uncertain outcomes; receipt metadata is cleaned seven days after the due time. SQLite free pages may retain old bytes until vacuum/reuse; logical erasure is not secure disk wiping.

Events use native Discord voice events and Interested/RSVP controls. Start at least 20 minutes ahead; duration is 15–720 minutes. A reminder posts to the selected channel roughly 15 minutes before start, without pinging anyone. Discord controls native interested-user notifications; the host starts the native event through Discord. Temporary voice rooms copy the selected voice room's category and explicit permission overwrites, without making it public. Only recorded bot-created rooms are deleted, after the end/cancellation and only when the connected gateway cache says they are empty. Existing voice rooms are never deleted. Event resources are tracked for restart recovery. Database-registration failure attempts to roll back the new event/empty room and reports any manual cleanup needed. Ten open events per guild; closed metadata retained 30 days. Scheduler pauses during gateway disconnects.

New reports/appeals receive persistent Claim, Resolve, Reopen buttons on the original private staff message. Button handling rechecks the guild, bot-owned message, tracked message ID, current staff channel privacy and the acting member's moderation/view permissions. Resolving requires a note and the assigned moderator; claiming is atomic. Reopening clears assignment/note. No automatic punishment or reporter notification occurs. Moderation case history remains available via `/case` and `/warnings`. Old report messages are not silently reclassified or backfilled. Queue receipts/notes follow the existing 30-day report receipt cleanup; Discord copies follow channel retention.

Privacy erasure removes cumulative and daily message/voice counters and records an opt-out so future writes are skipped. Daily counters otherwise retain 30 days. Resume starts future counting when enabled server-wide, without restoring old data. This does not delete moderation cases, report content, Discord log messages, already-posted insight cards, birthdays, playlists or scheduled posts; the command explains separate controls and staff contact. Activity contains counters, not message contents. In-memory presence remains separate.

Health uses read-only Linux `/proc` memory/load and `df` for the bot filesystem, plus database and Lavalink checks. Available RAM below 15%, disk usage at least 85%, one-minute load over twice the available CPU count, or unavailable Lavalink triggers a private alert after three consecutive minute samples. One recovery message follows restoration. Audience and creator permissions are rechecked before posting alerts. Alerts start disabled. This in-process monitor cannot alert while the bot/VPS itself is offline; an independent external monitor is needed for that case. No credentials or environment values are shown.

Permission diagnostics report command/base channel permission gaps. They cannot evaluate every dynamic target hierarchy, integration restriction, cooldown, DJ requirement, owner-only guard or feature configuration; those still apply at execution.

Artwork uses the existing monochrome doodle renderer and bundled Space Grotesk, IBM Plex Mono and Kalam fonts. `--preview` includes event-cover and health samples without connecting to Discord.
