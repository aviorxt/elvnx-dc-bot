# Automatic member presence API

Presence is available automatically for current non-bot members of the configured
Discord server. Members do not need to opt in. There are no Enable or Disable
actions. `/presence` returns your URL; `/presence member:@member` returns a
member's URL. `/presence action:Info` explains sharing. Replies are ephemeral.

Discord privacy settings still apply. Invisible appears offline. The bot cannot
reveal hidden status or activities. The API publishes profile ID/name/avatar,
status, active device indicators, and up to eight bounded activities with details,
state, timestamps and assets. Activity text may contain user-supplied information
from Rich Presence. No messages, voice channels, email, party/session IDs,
join/spectate secrets, or arbitrary activity URLs are exposed. Presence stays in
memory; no activity history is stored. The legacy presence_consent table no longer
gates access. `/privacy` activity-counter preferences are separate from this API.

## Endpoints

Base URL: `https://presence.11-x.online` through Cloudflare Tunnel.

- `GET /health`: process health, not Discord synchronization health.
- `GET /v1/users/{discord_id}`: `{ "success": true, "data": { ... } }`.
- `GET /v1/users/{discord_id}/ws`: WebSocket updates for one member.

Data contains `discord_user`, `discord_status`, `activities`,
`active_on_discord_desktop/mobile/web`, and `listening_to_spotify`. Spotify
metadata comes from Discord; missing track IDs are not invented.

Unknown, nonmember and bot IDs return 404. Synchronizing/disconnected presence
returns 503. Responses use `Cache-Control: no-store` and public read-only CORS.
There is no member-list endpoint or HTTP write API.

WebSockets check for changes every two seconds and emit `PRESENCE_UPDATE` with
`payload: { success: true, data: ... }`. Unchanged streams send `HEARTBEAT`
approximately every twenty seconds. Gateway outages emit `UNAVAILABLE`; clear
displayed data until a fresh update. Leaving the guild closes the stream on the
next check. Streams expire after one hour. Reconnect with backoff after transient
failures; stop retrying after a nonmember/404 response.

```js
const id = 'YOUR_DISCORD_ID';
const base = 'https://presence.11-x.online';
const response = await fetch(`${base}/v1/users/${id}`, {cache: 'no-store'});
if (response.ok) console.log((await response.json()).data);
const socket = new WebSocket(`wss://presence.11-x.online/v1/users/${id}/ws`);
socket.onmessage = ({data}) => {
  const event = JSON.parse(data);
  if (event.event === 'PRESENCE_UPDATE') console.log(event.payload.data);
};
```

## Hosting

Enable member/presence intents in the Developer Portal, then configure
`ENABLE_MEMBER_EVENTS=true`, `ENABLE_PRESENCE_EVENTS=true`,
`PRESENCE_API_ENABLED=true`, and `PRESENCE_API_PORT=4100` in the private environment.
The listener binds only to `127.0.0.1`. Cloudflare Tunnel routes the hostname to
`http://127.0.0.1:4100`. Do not expose port 4100 directly or cache `/v1/*`.
Keep tunnel tokens and bot credentials outside Git.

The origin allows 120 requests/minute per direct peer, 1024 tracked peers and
64 concurrent WebSockets, with bounded inbound frames and send timeouts. Tunnel
clients share the loopback peer limit; forwarded headers are not trusted.

The deployed connector is cloudflared 2026.10.7. Tests cover secret-field exclusion,
invisible/offline redaction and bounded IDs. Deployment checks verify HTTPS health,
access for a current member without stored opt-in, and rejection of nonmembers.
