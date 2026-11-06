# Spotify links and YouTube playlists

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
