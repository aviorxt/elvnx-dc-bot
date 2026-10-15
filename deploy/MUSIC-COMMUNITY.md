# Community music — v0.8.0

The approved channels #music-requests and #playlist-fav are under Legacy and copy the category permission overwrites, with five-second slowmode. Kona is the approved persistent voice destination. Request/favorites boards use bot-owned message bindings, illustrated monochrome covers and custom animated controls; they edit in place after restart. Configuration and channel ownership are rechecked. Non-404 board lookup failures do not create duplicate boards.

## Requests and Add track

Post one song title or supported HTTPS music link in #music-requests, or choose Add track on its permanent board or the active player. The modal is bound to member, guild and interaction, expires after two minutes and supports YouTube or SoundCloud searches. Submit-time checks repeat channel binding, membership screening, text access, current voice membership, bot voice/panel permissions, 24/7 destination and queue capacity. /play, modal and channel requests share the same queue implementation and persistent five-second search throttle. Requests add to playback rather than replacing the current song. The existing 50-track cap remains. Original bot/webhook messages are ignored, mentions/attachments rejected, automod rules checked, and gateway message IDs claimed once for 24 hours without retaining request text. Feedback errors are bounded to one per ten seconds per member.

## Shared favorites

Posting one supported HTTPS music link in #playlist-fav saves it without starting playback or requiring voice. Tracking parameters/fragments are stripped; normalized URLs are capped at 300 encoded characters, deduplicated within the guild and limited to 200 saved links. Five-second submission throttling and channel access checks apply. Links are bookmarks and are not guaranteed playable by their source.

`/playlist favorites action:View page:1` browses ten links per page. Play queues up to ten supported tracks from that page, subject to available queue space and current voice permissions. Failed source lookups may be skipped; a total loading timeout queues nothing. Links remain saved after playback. Remove requires the saved link and allows the original submitter or Manage Server staff. Board buttons browse/play the first page; other pages use the command. No saved links were injected by deployment.

## Session recap

When the queue ends or the bot stops/disconnects, it takes the in-memory summary once and posts a matching generated report card to the session's text channel. Counts distinguish track starts, completed tracks, manual skips and failures. Active playback time is an estimate measured with a monotonic clock and excludes pauses; seeking does not inflate it. Repeats count as new track starts. Recent track labels are capped at five and requester identities at 1,000 per session. The recap lists up to five requesters, excluding members who opted out through /privacy. No audio is recorded. Delivery is best effort; service crashes/restarts lose in-memory session totals. Already-posted recaps remain in Discord until deleted by staff. `/configure` MusicRecaps with on/off controls future recaps.

## 24/7 mode

`/playlist always-on enabled:true channel:Kona` enables persistent idle presence. Manage Server and actor/bot access to the selected normal voice channel are required. The selected channel is stored; after restart or unexpected disconnection the bot retries joining, with a one-minute retry bound and stalled-handshake recovery. The normal empty-channel/three-minute idle disconnect is bypassed for the configured destination. This does not override Discord permissions or guarantee VPS/network uptime. The bot stays self-deafened and does not record audio.

Stop / leave and /stop explicitly disable 24/7 before disconnecting. `/playlist always-on enabled:false` disables it while allowing an active song to continue with normal cleanup; an idle bot disconnects. Mode changes synchronize both stored configuration and the live settings cache. Requests cannot move the bot away from its configured 24/7 room.

## Configuration and retention

/configure adds MusicRequestChannel and MusicFavoriteChannel (channel ID or off) plus MusicRecaps (on/off); the two text destinations must differ. The guild allocation remains 100 commands; most additions live under /playlist, with the direct /24-7 alias in the separate global allocation. Member/Message Content intents are required for channel requests and favorites, and setup verifies permissions. New data: board message IDs; request receipt IDs and throttle timestamps (pruned after one day); shared URLs with guild, submitter and timestamp (until removed); persistent voice-channel choice. No bot credentials, query bodies or audio are stored in these tables. Existing Discord posts/logs have independent retention.

## Direct 24/7 command — v0.8.1

Use `/24-7 action:Enable channel:Kona` or `/24-7 action:Disable`. The channel selector accepts normal voice channels only. The alias shares the existing staff-only persistent mode implementation, including actor/bot access checks and explicit Stop behavior. `/playlist always-on` remains available.

The alias is registered in the global allocation because the guild allocation is already 100. It is restricted to Guild installation and Guild interaction context, defaults to Manage Server permission, and retains the same runtime ELVNX-PVT guild check as every other command. No DM or user-install context is allowed. Registration deliberately maintains 100 guild commands and one distinct global alias, with no duplicate name across scopes.
