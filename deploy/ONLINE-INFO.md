# Public online-info notices

Channel: `online-info` (`1560312201833877621`) under Server-Info.
Legacy role: `1554098593382531112`.
The channel is visible to ordinary guild members and read-only for them. Existing
Administrator roles keep normal Discord Administrator access. The bot can publish
and replace notices. No role permissions or assignments are changed.

Set both `ONLINE_INFO_CHANNEL_ID` and `ONLINE_INFO_ROLE_ID` in the protected server
environment to enable. Omit both to disable. The destination is rechecked against
the configured guild and must be a text channel with the configured role present.
The @everyone role is rejected. Allowed mentions explicitly contain only the
configured Legacy role, with users/@everyone/@here disabled. The role does not
need to be made globally mentionable when the bot already has the required permission.

On process startup, the bot posts a new Online image once after setup. A Discord
reconnect does not repeat the notice. Owner `/bot-restart` and graceful SIGINT or
SIGTERM shutdown publish Restarting before disconnecting. Maintenance stops also
use this notice; it is not a guarantee that an operator will immediately restart
the process. Hard crashes, SIGKILL, network/power loss cannot be announced in
advance; the next successful process start replaces the stale notice. Startup
announcements retry up to three times on transient failures. Shutdown publication
is bounded to 20 seconds and remains best effort so the service can exit.

The same tested binary can publish a maintenance notice without connecting a
second gateway using `--announce-restart`, under the service account and protected
environment. This is used before the first deployment from a release without the
signal hook. Run it to completion before stopping the main service; a sequential
signal/helper call skips an already tracked Restarting phase.

Only a successful new upload and durable pointer update allow old-message cleanup.
The previous tracked message is queued in SQLite and deleted only after its guild
and bot author are checked. No channel-history sweep or user-message deletion is
performed. Missing messages/channels are treated as already cleaned. Failed deletes
remain queued and retry every 30 seconds. The replacement and pending deletion
are written in one transaction. A failed persistence step attempts to remove the
newly sent message and retains the previous pointer. Discord/network ambiguity or
a process crash during publication can still require staff cleanup; no distributed
exactly-once notification guarantee is claimed.

Images use bundled purple artwork with code-rendered status, timestamp in IST,
and public version/build metadata. No host IP, credentials, user activity or private
logs are published. Render previews with `elvnx-bot --preview`:
`previews/elvnx-online.png` and `previews/elvnx-restarting.png`.
The complete artwork prompt and font provenance are in `assets/status/DESIGN.md`.

Tests cover role-only mentions, invalid IDs, scoped durable replacement, queued
cleanup idempotency, transactional rollback on queue failure, and distinct PNG
renders. Production verification checks both the maintenance and online image,
Legacy mention, deletion of the old bot-owned notice and public read-only access.
