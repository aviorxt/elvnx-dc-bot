# Command and control responses

All 100 registered commands use the shared `errors::on_error` handler. Expected validation failures are explicitly represented by `errors::user(...)`. The handler preserves their actionable instructions instead of replacing them with a generic permissions warning. `/play` checks voice membership before acquiring the music service/operation lock, so a user outside voice receives the join instruction even while the audio node is unavailable.

Examples:

| Situation | Response/action |
|---|---|
| `/play` outside voice | Join a server voice channel, then run the command again |
| Music control from another channel | Join the bot's voice channel |
| Empty queue | Start playback with `/play` first |
| Missing user/bot permissions | Names the missing permissions and explains channel overrides |
| Hierarchy prevents moderation | Explains that the target must be below the user and bot; owner is protected |
| Cooldown | Shows rounded-up remaining seconds |
| Deleted channel, message, member or role | Select a current resource or recreate/update the affected panel |
| Closed recipient DMs | Check recipient privacy settings; no generic permissions blame |
| Invalid number or time | Digits/range guidance or the IST date format and relative-time examples |
| Bad backup JSON | Use an unmodified `/config-backup` export |
| Discord rejects an upload | File-size guidance and Discord limits |
| Database/network/service failure | Explains temporary unavailability, provides next steps and a reference |
| Outdated command schema/interaction | Reopen the picker/panel and have staff check synchronization |
| Unexpected failure or panic | Safe explanation, support reference and a reminder to check whether a posting/moderation action already completed |

Command failures use private monochrome embeds with the command name, next steps, developer footer and interaction reference. Mentions are disabled. Known modal/component paths—report forms/setup, settings/poll controls, staff review, music, media preview/posting, verification and temporary voice room controls—use the same safe message mapping and references. Existing partial-success responses remain explicit; errors do not claim that an action was rolled back or that a message was not delivered.

Only deliberately marked application validation messages are displayed verbatim. Arbitrary error strings, backend response bodies, URLs, SQL, file paths, panic payloads and credentials are not copied into Discord. Serenity HTTP/model errors, SQLx, numeric/time parsing, JSON, reqwest and local storage failures are classified into safe responses. Unrecognized failures retain a safe fallback rather than guessing the cause. Staff can correlate the interaction reference with private service logs. Background jobs/setup failures without an interaction are logged, not posted into unrelated channels.

When adding a command, use `errors::user("human instruction")` for intentional validation and preserve typed library errors with `?`. Do not wrap arbitrary `error.to_string()` or backend content in `errors::user`; that would bypass the trust boundary. Inline forms/buttons should call `errors::with_reference(...)` when handling a caught error. Discord outages, expired interaction tokens or a missing response permission can prevent delivery; that failed response is recorded in local logs.

Regression coverage includes voice guidance, real moderation/scheduling validation, permission names, cooldown rounding, Discord resource/DM errors, hierarchy and text-size errors, numeric/database/storage handling, untrusted-error redaction and UTF-16 embed length bounds. Existing moderation, upload, music, privacy, scheduling and guild command tests remain enabled.
