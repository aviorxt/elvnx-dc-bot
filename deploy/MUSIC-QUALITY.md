# Music fidelity and buffering

The quality configuration uses `opusEncodingQuality: 10`, `resamplingQuality: HIGH`,
a 5,000 ms audio-frame buffer, and seek ghosting. Encoder quality is CPU
complexity, not a claimed bitrate or a source upgrade. Buffering improves tolerance
of short stalls; seeking and buffered filter changes can take longer to settle.

Default player volume is 100% (unity gain), allowing LavaPlayer to pass compatible
YouTube Opus frames through when the input/output formats match and no processing
is required. Volume remains limited to 0–100%; changing it below 100% can require
transcoding. Use Discord's local per-user volume to adjust listening loudness while
keeping unity gain at the bot. No bass boost, equalizer or loudness boost is applied.

YouTube-source retains its existing working client fallback order and automatic
best-available-format selection. Some clients and live streams require transcoding;
Opus quality 10 and high-quality resampling apply when conversion is needed. This
does not turn a compressed YouTube source into lossless or guaranteed 320 kbps audio.

New join-to-create rooms use the server's current boost-tier bitrate ceiling instead
of a fixed 64 kbps. When music is requested, the bot also raises the selected voice
channel to the supported ceiling if it has Manage Channels. Other channels are
not bulk edited. Tier 0/1/2/3 limits are 96/128/256/384 kbps. This server currently
has boost tier 0, so its maximum is 96 kbps. If changing the channel bitrate fails,
the bot keeps the existing bitrate and continues playback.

Sources:
- https://lavalink.dev/configuration/config/file
- https://github.com/lavalink-devs/lavaplayer/blob/main/README.md
- https://github.com/lavalink-devs/youtube-source#available-clients
- https://docs.discord.com/developers/resources/channel#modify-channel

Before applying, check that no track is playing, back up the server YAML and binary,
then restart the bot and Lavalink. Verify the node, presence tunnel, voice connection,
YouTube stream access and active configuration. Do not publish credentials or
play an unsolicited test track in Discord. Restore the backed-up YAML/binary if
startup fails. Keep Rust's package version and lockfile synchronized.
