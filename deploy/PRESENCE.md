# Opt-in presence API

ELVNX v0.9.0 adds a Rust presence service inspired by Lanyard's public presence
API. This is an independent implementation, not a full Lanyard protocol clone.
No extra Discord token, Redis instance, or second Gateway connection is needed.

## Member controls

- `/presence` returns your URL when opted in. `/presence member:@member` returns an opted-in member's URL. This never enables sharing for anyone.
- `/presence action:Enable` explains public exposure without saving consent.
- `/presence action:Enable confirm:true` opts in the caller only.
- `/presence action:Disable` revokes API sharing and closes streams within about
  two seconds (bounded network sends may add up to three seconds).
- `/presence action:Info` shows the caller's sharing setting privately.

All replies are ephemeral. Staff cannot opt other members in. Nobody is opted
in during deployment. Members must remain in the configured guild. Gateway
removal events revoke consent. Invisible users appear offline; Discord does not
reveal their true status. Discord privacy settings still determine which
activities the bot can see.

Public sharing includes profile ID/name/avatar, status, device indicators, and
up to eight activities with bounded names, details, state, timestamps, asset
references. Custom status and Rich Presence can themselves contain private
text: users must review their activity settings before opting in. No voice
channels, messages, email, party/session IDs, join/spectate secrets, arbitrary
activity URLs, or member-list endpoint are exposed. Only guild/user consent
and its creation timestamp persist in SQLite; presence remains in memory.
Third parties can copy published data; opting out cannot retract their copies.

This API consent is separate from `/privacy` activity-counter preferences.
Existing Discord availability panels are separate and unchanged.

## Endpoints

Intended HTTPS host: `https://presence.11x.dpdns.org` via Cloudflare Tunnel.

- `GET /health`: process health only, not Discord synchronization health.
- `GET /v1/users/{discord_id}`: `{ "success": true, "data": { ... } }`.
  The `data` object contains `discord_user`, `discord_status`, `activities`,
  `active_on_discord_desktop/mobile/web`, and `listening_to_spotify`.
  Spotify song/artist metadata is in the Spotify activity; no synthetic
  Spotify track ID is fabricated when Discord's stable SDK omits it.
- `GET /v1/users/{discord_id}/ws`: WebSocket for one opted-in member.
  Immediately sends `PRESENCE_UPDATE`, checks for changes every two seconds,
  and sends `HEARTBEAT` approximately every twenty seconds when unchanged.
  Updates have `{ "event": "PRESENCE_UPDATE", "payload": { "success": true,
  "data": { ... } } }`. Gateway outages emit `UNAVAILABLE` without stale
  activity. Revocation emits `REVOKED` and closes the connection. Clients
  should reconnect with exponential backoff on transient failures; streams
  expire after one hour. Do not reconnect indefinitely after revocation/404.

Unknown, opted-out, nonmember, and bot IDs return 404. Synchronizing or
disconnected presence returns 503, not a misleading offline status. Responses
use `Cache-Control: no-store` and public read-only CORS without credentials.
There is no HTTP write API or arbitrary-key KV store. The WebSocket protocol
is deliberately simpler than Lanyard's `/socket` protocol.

Example:

```js
const id = 'YOUR_DISCORD_ID';
const base = 'https://presence.11x.dpdns.org';
const response = await fetch(`${base}/v1/users/${id}`, {cache: 'no-store'});
if (response.ok) console.log((await response.json()).data);
const socket = new WebSocket(`wss://presence.11x.dpdns.org/v1/users/${id}/ws`);
socket.onmessage = ({data}) => {
  const event = JSON.parse(data);
  if (event.event === 'PRESENCE_UPDATE') console.log(event.payload.data);
  if (event.event === 'UNAVAILABLE' || event.event === 'REVOKED') {
    // Clear previously displayed activity immediately.
  }
};
```

## Hosting

Set `PRESENCE_API_ENABLED=true`, `PRESENCE_API_PORT=4100`, and enable existing
member/presence intents in the environment and Developer Portal. The listener
always binds `127.0.0.1`; never add an inbound security-group rule for 4100.
Cloudflare Tunnel routes only this hostname to `http://127.0.0.1:4100`, with a
404 fallback for other hosts. Keep tunnel credentials in a root-only token
file, outside the repo and command arguments. Use a dedicated systemd service.

The origin limits requests to 120/minute per direct peer, a bounded 1024-peer
map, and 64 concurrent WebSockets, with 1 KiB inbound frames, bounded send
timeouts and one-hour lifetimes. Behind Tunnel all clients share the local
peer limit; it is a protective aggregate cap, not per-visitor accounting.
Forwarded headers are deliberately not trusted. Add edge rate limits if more
traffic is expected. Do not cache `/v1/*` at Cloudflare or in a service worker.

Slash commands: 100 guild + 2 guild-install-only global commands, with runtime
checks still restricted to ELVNX-PVT. No duplicate command names across scopes.
Global registration can take time to appear in Discord clients.

## Validation

Tests cover secret-field exclusion, invisible/offline redaction, bounded IDs,
consent-only persistence, command privacy/scope, plus the existing regression
suite. Deployment checks must verify loopback binding, HTTPS 404 for an
unconsented ID, healthy process, and WebSocket rejection before consent. A
real member must voluntarily enable sharing to test their live activity;
deployment must not silently opt anyone in or fabricate a member's consent.
