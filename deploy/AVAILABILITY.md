# Live member availability

Run `/availibilty-panel` to create one **availability-panel** channel under **ELVNX AVAILABILITY**. It edits a single embed message containing the doodle cover, group counts, Online and Offline lists, Gaming with game names/rich-presence details, and On VC with voice/mute/deafen information.

Manage Server and Manage Channels are required. The bot needs Manage Channels, View Channel, Read Message History, Send Messages, Embed Links, and Attach Files. Presence and Server Members intents must be enabled in Discord, with `ENABLE_PRESENCE_EVENTS=true` and `ENABLE_MEMBER_EVENTS=true`. Presence permission was confirmed on the deployed application before enabling the flag.

On first setup, the existing **Legacy** common role is the default viewer audience when available. Otherwise all server members can view it. An optional `audience` role can select a different ordinary role on first setup. The audience remains fixed on subsequent setup calls; omit the option when resuming. Bots are excluded from member lists. Administrators retain Discord's normal permission bypass. The command does not assign roles to anyone.

Presence/voice/member events mark the panel for refresh at five-second intervals, subject to network and Discord rate limits. Unchanged messages refresh their timestamps every minute. Names and statuses are grouped and sorted inside four fields of one embed. Each field stays within Discord limits and shows a visible overflow count for large groups; counts include all members. Groups overlap: gaming members may also be Online and On VC; invisible members in visible voice channels can appear Offline and On VC. Activity names come from Discord's Playing activities, not game-account APIs. Hidden/unshared activities cannot be discovered.

A complete gateway member roster and presence snapshot are requested on startup/reconnection. Incomplete synchronization or disconnected sessions show unavailable/synchronizing notices instead of declaring everyone Offline. A stale timestamp means the bot/updater may be offline. A missed presence event during disconnection is reconciled with a fresh roster request.

Private voice channels are excluded unless the entire configured audience has visibility. Individual View Channel denials also exclude the voice channel from that shared board. A restricted panel whose permissions become broader has its existing bot-owned member lists redacted where the bot still has editing access; an administrator must restore the configured audience permissions. Pause immediately with `/availibilty-panel enabled:false`.

Channel IDs, message IDs, enabled state, and audience role ID persist in the private SQLite `availability` table. No presence/game/voice-history snapshots are stored there. Current presence is held in memory; Discord receives the displayed member lists and game details, following the server's Discord retention. Rerunning setup reuses tracked channels/messages; deleted channels can be recreated through setup. The bot never edits or deletes another author's messages and reconciles its own recent messages after interrupted publication. Settings backups do not include these resource IDs.

The cover uses the bundled Space Grotesk, IBM Plex Mono, and Kalam fonts with the existing doodle renderer. `--preview` creates an illustrative `previews/availability.png` without connecting to Discord.

Older multi-channel panels migrate automatically when active: the existing overview message is reused, its four member groups move into embed fields, and tracked bot messages in the four obsolete status channels are removed. Only empty obsolete channels within the original category are deleted; custom/admin content is preserved. Migration IDs are retained until cleanup succeeds.
