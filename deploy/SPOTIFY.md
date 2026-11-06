# Spotify links and YouTube playlists

## Current deployment: direct official Spotify playback

`SPOTIFY_DIRECT_ENABLED=true` now routes Spotify links to the signed-in official
Linux Spotify client. It does not search YouTube for these requests. Public embed
metadata supplies track IDs and queue information. The custom `spotifydirect`
Lavalink source routes the official client's PulseAudio monitor through a private
loopback PCM bridge into Lavalink's Opus encoder. No Spotify audio keys are
extracted or implemented by the bot. The unofficial go-librespot receiver is stopped.

Sources and build helper are in [spotify-direct](spotify-direct). The helper expects
Lavalink dependencies extracted under `/data/workspace/spotify-plugin/deps`:
`jar xf /opt/lavalink/Lavalink.jar BOOT-INF/lib`. Copy the Java source and Python
bridge to `/data/workspace` before building. The plugin JAR is installed under
`/var/lib/lavalink/plugins`. User services `elvnx-spotify-desktop` and
`elvnx-spotify-bridge` start with the VM's user session. PulseAudio's persistent
`elvnx_spotify` null sink receives only the Spotify client's output. The official
client needs the VM's graphical desktop and its saved Spotify sign-in.

The bridge listens only on `127.0.0.1:3680`, uses the user's local MPRIS session,
and streams 48 kHz stereo signed 16-bit PCM. Each playback has a unique session
ID so an old stop cannot terminate a newer track. Stop and skip release the capture
process; pause/resume control both Spotify and Lavalink. Seeking is disabled for
direct Spotify tracks. Public albums/playlists retain the 50-track queue limit.
Spotify account credentials and desktop profiles are never committed.

Actual official-client audio and 150 encoded Opus frames were verified without
joining a Discord voice channel. Discord still uses its server bitrate ceiling.
Source quality depends on the official client's audio quality setting and plan.
Set `SPOTIFY_DIRECT_ENABLED=false` and restart the bot to restore YouTube matching.

## Matching mode (direct flag disabled)

`/play query:LINK` accepts public Spotify track, album and playlist links, including
localized `open.spotify.com/intl-.../` links and `spotify:track:ID` URIs. It also
accepts YouTube playlist links. A YouTube video link containing a `list` parameter
loads the playlist, starting from the collection's beginning. Remove `list` to
request only the video. The queue holds at most 50 tracks.

Spotify is a metadata source. The bot reads the track information shown by Spotify's
public embeds and searches YouTube for full-length matches. It compares title,
primary artist and duration, skips unmatched entries, and preserves collection order.
It never plays Spotify's short audio previews. The selected recording may differ
from Spotify's recording; matches are best effort. No Spotify login, cookies or
client secret are required. Private playlists, podcasts and Spotify short links
are unsupported. Spotify can change its embed format or restrict public access.

Album/playlist loading uses the first 50 entries available in the embed, with four
concurrent audio searches and a 90-second deadline. `/play` defers its reply while
loading. Already queued tracks count toward the same 50-track cap.

Lavalink remains on its working YouTube plugin. No LavaSrc plugin or anonymous
Spotify token service is required. Spotify HTTPS requests use a separate bounded
HTTP client without the private audio node's Authorization header.

For audio fidelity, the node already uses Opus encoder quality 10, HIGH resampling,
unity gain (100%) and best-available YouTube format selection. On a successful music
request, the bot raises the selected voice channel to the server's supported bitrate
when it has Manage Channels. Limits by boost tier are 96/128/256/384 kbps. Failure
to adjust bitrate leaves playback available. The current server is tier 0, so the
ceiling is 96 kbps. This does not provide lossless or Spotify Premium audio.

Validation includes URL/URI normalization and rejection, YouTube playlist handling,
bounded metadata parsing, matching rejection for live/wrong-duration/wrong-artist
results, public Spotify metadata requests and full-length source matching. These
checks do not join a voice channel or play an unsolicited test track.
