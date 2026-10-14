# Role mention reactions

Use `/always-react channel:YOUR_CHANNEL` to enable. Use
`/always-react enabled:false` to disable. The selected channel persists in
server settings; choosing a different channel replaces the previous selection.
Only members with Manage Server can configure it.

For new human messages in the selected channel, the bot checks Discord's
structured role mention list for role **1554097852672770118**. Matching messages
receive **✅** (`white_check_mark`). Other roles, other channels/servers, DMs,
bot messages, and webhook messages do not match. Message edits do not trigger
this feature. Thread messages are separate channels and do not inherit a parent
channel selection. The configuration response does not ping the role.

Enable Message Content Intent in the Discord Developer Portal and keep
`ENABLE_MESSAGE_CONTENT=true`. The bot needs View Channel, Read Message History,
and Add Reactions. Configuration also checks normal channel posting access for
the administrator and bot. Deleted messages or subsequently changed permissions
can make reactions fail; these failures are logged without interrupting other
moderation handlers. No message content is stored by this feature.
