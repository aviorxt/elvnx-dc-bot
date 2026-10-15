# ELVNX custom emoji and motion design

40 original static PNG assets and 40 animated GIF variants are bundled in assets/emojis. The application owns the 80 custom emoji; guild emoji slots are not used. The bot selects the animated variants by default, with static assets available as a fallback. Discord client animation/reduced-motion settings can pause playback.

The symbols themselves animate: checks draw, bars bounce, hands turn, locks open, arrows travel, gears turn, envelopes open and typing dots move. Badge borders remain still. All assets are monochrome and match the existing ELVNX/AVRXT UI. No default Unicode emoji are used as UI fallbacks. Readable control labels remain present.

Messages use context-specific icons for music, DMs, moderation, reports, activity, setup and help. Branded footers and timestamps provide consistent context. Music Pause/Resume controls select the matching animated symbol. Existing live verification controls refresh on startup; old historical messages are not bulk-edited.

Generate with `python scripts/make-emojis.py` (Pillow). Validate motion, stationary badge bounds and Discord upload sizes with `python scripts/preview-emojis.py`; the animated contact sheet is previews/emojis-animated.gif. Use `sudo python3 deploy/emoji-sync.py` on the VPS; --inspect is read-only. It creates missing named assets and preserves unrelated ones. Copy manifest.json AND animated.json back before compiling. IDs are public assets, never credentials. The bot token remains in the root-only environment.

This pack belongs to the current bot application. Use new names for a redesigned published emoji: Discord's emoji edit endpoint cannot replace its image. Retire unpublished design experiments only before a build references them. Keep all published IDs until references have migrated.

Discord application emoji reference: https://github.com/discord/discord-api-docs/blob/main/developers/resources/emoji.mdx
