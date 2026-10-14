# Automatic DND voice moves

Enable with `/afk-move enabled:true channel:DND`. Disable with
`/afk-move enabled:false`. The selected destination persists across restarts;
the feature remains disabled until a channel is selected.

A human member who remains self-muted, server-muted, self-deafened, or
server-deafened for 10 continuous minutes is moved into the chosen normal voice
channel. Switching between mute and deaf without becoming fully active preserves
the timer. Becoming both unmuted and undeafened, leaving voice, or changing
channels resets it. Bots and members already in the destination are excluded.
The worker checks every 15 seconds, so moves normally occur within 15 seconds
after reaching the threshold. Failed moves retry no sooner than one minute.

The configuring member needs Manage Server and Move Members, plus destination
access. The bot needs Connect and Move Members in the source and View Channel, Connect, and
Move Members in the destination. The target member must be able to view and
connect to the destination; the bot does not expose otherwise-private channels.
Full channels or Discord API errors can prevent moves. Choose a destination
with suitable capacity and access for the members you intend to move.

The bot checks live Discord voice state and member permissions again before
moving. Discord cannot atomically combine a status check with a member move;
a last-moment status change can race with a request already in flight.
Successful moves are posted to the configured staff log without pinging members.
The existing voice-move event log also remains enabled.

Timers are bounded and memory-only. Gateway disconnects pause processing and reset timers on reconnect. They begin when the enabled bot observes the
muted/deafened state; a restart starts fresh timers for connected members rather
than immediately moving them based on unknown historical status. This is voice
state automation, not speech detection or recording.
