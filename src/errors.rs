//! Only explicitly marked validation messages may be displayed verbatim.
use crate::{Data, Error, serenity as s};
use std::{error::Error as StdError, fmt};

#[derive(Debug)]
struct UserError(String);
impl fmt::Display for UserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl StdError for UserError {}
pub fn user(message: impl Into<String>) -> Error {
    Box::new(UserError(message.into()))
}

fn validation(text: &str) -> String {
    match text {
        "Join a voice channel first" | "Join voice first" | "Voice channel required" => "Join a server voice channel first, then run this command again. For /play, join the channel where you want to listen.".into(),
        "Join the bot's voice channel to control music" | "Join the bot's voice channel first" | "Join the bot's voice channel" => "Join the same voice channel as the bot, then try the music control again.".into(),
        "Nothing is connected" | "No voice connection" => "The bot is not connected to a voice channel. Join voice and use /play to start music first.".into(),
        "Nothing is playing" | "Nothing to save" => "There is no active track or queue. Join voice and use /play first.".into(),
        "The music service is not configured" => "Music is not set up on this bot. Ask an administrator to configure Lavalink; /lavlink-status shows its status.".into(),
        "Missing voice or music panel permissions" => "The bot needs View Channel, Connect and Speak in your voice channel, plus View Channel, Send Messages, Embed Links and Attach Files in the music panel channel. Ask staff to check those channel permissions.".into(),
        "Guild cache unavailable" | "Voice roster unavailable" => "The server's live member/voice information is still synchronizing. Wait a few seconds and try again.".into(),
        "Queue is full" => "The music queue has reached its limit. Wait for a track to finish or ask a DJ to remove tracks.".into(),
        "Your playlist was not found" => "That playlist was not found under your account. Use /playlist list to see your saved playlists.".into(),
        "Image allocation failed" => "The image could not be generated right now. Try again shortly; staff can check available memory if it continues.".into(),
        "Role not found" | "Role removed" => "That role no longer exists. Select a current server role and update the feature's setup if needed.".into(),
        "Join your room first" => "Join a temporary voice room that you created through the configured voice hub, then use its room controls.".into(),
        "Only the creator can manage this room" => "Only this temporary room's creator can change its lock or member limit. Create your own room through the voice hub.".into(),
        "Time too large" | "Invalid date" => "Choose a valid future time within 365 days: 10m, 2h, 1d, or YYYY-MM-DD HH:MM in IST.".into(),
        text if text.starts_with("Audio node returned HTTP ") => "The audio service could not process that request. Check /lavlink-status and try again shortly or use another track/source.".into(),
        _ => text.to_string(),
    }
}
fn discord_message(code: isize, status: u16) -> String {
    match code {
        50013 => "Discord denied the action because the bot lacks permission. Ask staff to check its channel permissions and move its role above any member/role it must manage. Use /help check_command:COMMAND for a permission diagnostic.".into(),
        50001 => "The bot cannot access that channel, member or resource. Choose an accessible destination or ask staff to grant View Channel and the permissions needed for this action.".into(),
        10003 => "That channel no longer exists. Select an existing channel, or rerun the feature's setup to choose a new destination.".into(),
        10007 | 10013 => "That member could not be found. Select someone who is currently in this server.".into(),
        10008 => "That message was deleted or is no longer accessible. Use a current message, or recreate the panel through its setup command.".into(),
        10011 => "That role no longer exists. Select an existing role and update the feature's setup.".into(),
        50007 => "Discord would not allow a direct message to this member. They may have server DMs disabled or have blocked the bot; ask them to check their privacy settings.".into(),
        40005 => "Discord rejected the upload size. Send a smaller file; the bot's media commands accept up to 8 MiB per file and Discord can impose additional limits.".into(),
        50035 => "Discord rejected one or more values. Check the command's text lengths, channel type, selected roles and file size, then try again. Staff can use the reference below if the inputs look valid.".into(),
        10062 | 40060 => "This interaction has expired or was already handled. Reopen the command or panel. Check the destination before repeating an action that posts messages or changes members.".into(),
        30013 => "Discord's channel limit has been reached. Ask staff to remove unused channels before creating another room or category.".into(),
        _ if status==429 => "Discord is rate-limiting requests. Wait a little before retrying; avoid repeatedly clicking the control.".into(),
        _ if status>=500 => "Discord is temporarily unavailable. Try again shortly. Check the destination before repeating an action that may already have completed.".into(),
        _ if status==401 => "The bot's Discord connection needs administrator attention. Staff should check its token and service status; no token should be posted in chat.".into(),
        _ if status==403 => "Discord refused this action. Ask staff to check the bot's permissions, channel access and role hierarchy.".into(),
        _ if status==404 => "The requested Discord resource is no longer available. Select a current member, channel, role or message and try again.".into(),
        _ => "Discord could not complete the action. Check the selected resource and try again shortly. Check the destination before repeating an action that may already have completed.".into(),
    }
}
/// Never forwards arbitrary library errors, URLs, SQL or stack traces to Discord.
pub fn message(error: &(dyn StdError + 'static), command: &str) -> String {
    let mut current = Some(error);
    while let Some(error) = current {
        if let Some(error) = error.downcast_ref::<UserError>() {
            return validation(&error.0);
        }
        if let Some(s::Error::Http(http)) = error.downcast_ref::<s::Error>() {
            return match http {s::HttpError::UnsuccessfulRequest(response)=>discord_message(response.error.code,response.status_code.as_u16()), _=>"Discord could not be reached. Try again shortly; check the destination before repeating an action that might already have completed.".into()};
        }
        if let Some(s::Error::Model(model)) = error.downcast_ref::<s::Error>() {
            return match model {
                s::ModelError::Hierarchy => "Discord's role hierarchy blocks this action. The target must be below both your highest role and the bot's highest role; the server owner cannot be moderated.".into(),
                s::ModelError::InvalidPermissions {required,present} => format!("The bot needs **{}** for this action. Ask staff to check its role and channel overrides.",permissions(*required & !*present)),
                s::ModelError::RoleNotFound => discord_message(10011,404),
                s::ModelError::MemberNotFound => discord_message(10007,404),
                s::ModelError::ChannelNotFound => discord_message(10003,404),
                s::ModelError::GuildNotFound | s::ModelError::ItemMissing => "The server data is not available in the bot's live cache yet. Wait a few seconds and try again.".into(),
                s::ModelError::MessageTooLong(_) | s::ModelError::EmbedTooLarge(_) | s::ModelError::EmbedAmount => "The message or generated card exceeds Discord's size limits. Shorten the text or split it into smaller messages.".into(),
                s::ModelError::BulkDeleteAmount => "Choose between 1 and 100 messages to purge. Older messages may need to be removed in smaller batches.".into(),
                _ => "Discord could not apply those values. Check the channel, role, member and text limits, then try again.".into(),
            };
        }
        if let Some(error) = error.downcast_ref::<sqlx::Error>() {
            return match error {sqlx::Error::RowNotFound=>"That saved item was not found. Use the command's list option, or select a current ID.".into(),sqlx::Error::PoolTimedOut=>"The bot's data service is busy. Wait a few seconds and try again.".into(),_=>"The bot could not read or save its data. Please try again shortly. Check the destination before repeating actions that post messages or change members; staff can investigate using the reference below.".into()};
        }
        if error.is::<std::num::ParseIntError>() || error.is::<std::num::ParseFloatError>() {
            return "A number or Discord ID is invalid. Use digits only, check the allowed range in the command options, and select members/channels directly where available.".into();
        }
        if error.is::<chrono::ParseError>() {
            return "The date/time could not be understood. Use YYYY-MM-DD HH:MM in IST (for example 2026-10-20 20:00), or a supported relative time such as 10m, 2h or 1d.".into();
        }
        if error.is::<serde_json::Error>() {
            return if command == "config-import" {
                "The backup is not valid settings JSON. Upload an unmodified JSON file from /config-backup; do not paste tokens or credentials.".into()
            } else {
                "The bot could not read the saved data or service response. Try again shortly; staff can check the reference below if it continues.".into()
            };
        }
        if let Some(error) = error.downcast_ref::<reqwest::Error>() {
            return if error.is_timeout() {
                "The external service took too long to respond. Try again shortly; check the destination before repeating actions that post or moderate.".into()
            } else {
                "The external service could not be reached. Try again shortly; staff can check the service status if it continues.".into()
            };
        }
        if let Some(error) = error.downcast_ref::<std::io::Error>() {
            return match error.kind(){std::io::ErrorKind::NotFound=>"A server-side file required by this feature is missing. Ask an administrator to check the bot's deployment and feature configuration.".into(),std::io::ErrorKind::PermissionDenied=>"The bot cannot access a required server-side file. Ask an administrator to check its service file permissions.".into(),_=>"The bot could not access its local storage. Ask an administrator to check disk space and service permissions.".into()};
        }
        current = error.source();
    }
    format!(
        "I couldn't finish /{command} because of an unexpected problem. Try again shortly. Check the destination before repeating an action that posts messages or changes members; share the reference below with staff if it continues."
    )
}
fn permissions(value: s::Permissions) -> String {
    let names = value.get_permission_names();
    if names.is_empty() {
        "the required permission".into()
    } else {
        names.join(", ")
    }
}
fn cooldown(seconds: f64) -> String {
    format!(
        "You're using this command too quickly. Wait **{} seconds**, then try again.",
        seconds.ceil().max(1.0) as u64
    )
}
fn bounded(text: &str) -> String {
    let mut used = 0;
    let mut out = String::new();
    for c in text.chars() {
        used += c.len_utf16();
        if used > 3400 {
            out.push('…');
            break;
        }
        out.push(c);
    }
    out
}
pub fn with_reference(error: &(dyn StdError + 'static), command: &str, reference: u64) -> String {
    tracing::warn!(%error,command,reference,"Action failed");
    format!(
        "{}\n\nReference `{reference}`",
        bounded(&message(error, command))
    )
}
pub async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    use poise::FrameworkError as F;
    let Some(ctx) = error.ctx() else {
        match error {
            F::UnknownInteraction {
                framework,
                interaction,
                ..
            } => {
                let _=interaction.create_response(&framework.serenity_context.http,s::CreateInteractionResponse::Message(s::CreateInteractionResponseMessage::new().ephemeral(true).embed(crate::emoji::embed_for("warning").title("ELVNX / COMMAND UNAVAILABLE").description("This command is outdated or unavailable. Close and reopen Discord's slash-command picker, then try /help. Ask staff to check command synchronization if it continues.").color(0x111111)).allowed_mentions(s::CreateAllowedMentions::new()))).await;
            }
            F::Setup { error, .. } => tracing::error!(%error,"Bot setup failed"),
            F::EventHandler { error, .. } => tracing::warn!(%error,"Bot event handler failed"),
            _ => tracing::warn!("Framework error without a command response context"),
        }
        return;
    };
    let command: &str = ctx.command().name.as_ref();
    let reference = ctx.id();
    let body=match &error {
        F::Command{error,..}|F::CommandCheckFailed{error:Some(error),..}=>{tracing::warn!(%error,command,reference,"Command failed");message(error.as_ref(),command)},
        F::CooldownHit{remaining_cooldown,..}=>cooldown(remaining_cooldown.as_secs_f64()),
        F::MissingBotPermissions{missing_permissions,..}=>format!("The bot is missing **{}** in this channel. Ask an administrator to grant those permissions, then retry /{command}. Channel overrides can deny permissions even when the bot's role grants them.",permissions(*missing_permissions)),
        F::MissingUserPermissions{missing_permissions:Some(missing),..}=>format!("You need **{}** to use /{command} here. Ask a server administrator if you should have access. Channel overrides may restrict your role.",permissions(*missing)),
        F::MissingUserPermissions{missing_permissions:None,..}|F::PermissionFetchFailed{..}=>"I couldn't verify your server permissions. Try again shortly; staff can check your role and channel access.".into(),
        F::NotAnOwner{..}=>"Only the configured bot owner can use this command. Ask the owner to perform this action.".into(),
        F::GuildOnly{..}=>"Use this command inside the ELVNX server; it is unavailable in direct messages.".into(),
        F::DmOnly{..}=>"Use this command in a direct message with the bot.".into(),
        F::NsfwOnly{..}=>"This command requires an age-restricted channel. Ask staff to choose an appropriate channel.".into(),
        F::ArgumentParse{..}=>format!("One of the /{command} options could not be understood. Reopen the slash command, select a valid member/channel/role and check numeric values. Use /help search:{command} for guidance."),
        F::SubcommandRequired{..}=>format!("Choose a /{command} subcommand from Discord's command menu before sending it."),
        F::CommandStructureMismatch{..}=>"Discord is showing an outdated version of this command. Reopen the command picker or refresh Discord, then select the command again. Ask staff to check registration if it continues.".into(),
        F::CommandCheckFailed{error:None,..}=>"This command is available only in the configured ELVNX server. Run it there instead.".into(),
        F::CommandPanic{..}=>{tracing::error!(command,reference,"Command panicked; response payload withheld");format!("An unexpected problem interrupted /{command}. Share the reference below with staff. Check the destination before repeating an action that may have completed.")},
        _=>format!("/{command} could not complete. Reopen the command and try again; staff can investigate with the reference below."),
    };
    let response = ctx
        .send(
            poise::CreateReply::default()
                .ephemeral(true)
                .embed(
                    crate::emoji::embed_for("warning")
                        .title(format!("ELVNX / {command}"))
                        .description(bounded(&body))
                        .color(0x111111)
                        .footer(s::CreateEmbedFooter::new(format!(
                            "avrxt.dev • Reference {reference}"
                        ))),
                )
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await;
    if response.is_err() {
        tracing::warn!(
            command,
            reference,
            "Could not deliver private command error response"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn voice_validation_is_actionable() {
        let e = user("Join a voice channel first");
        let reply = message(e.as_ref(), "play");
        assert!(reply.contains("Join a server voice channel"));
        assert!(reply.contains("/play"));
        assert!(!reply.contains("Request failed"));
    }
    #[test]
    fn untrusted_errors_are_never_echoed() {
        let e: Error =
            "https://private.example/?token=secret-password /etc/bot.env SELECT credential".into();
        let reply = message(e.as_ref(), "say");
        for secret in [
            "secret-password",
            "private.example",
            "/etc/bot.env",
            "SELECT",
        ] {
            assert!(!reply.contains(secret));
        }
        assert!(reply.contains("/say"));
    }
    #[test]
    fn permission_cooldown_and_discord_errors_explain_next_steps() {
        assert!(
            permissions(s::Permissions::SEND_MESSAGES | s::Permissions::ATTACH_FILES)
                .contains("Attach Files")
        );
        assert!(cooldown(1.1).contains("2 seconds"));
        for code in [
            50013, 50001, 10003, 10007, 10008, 10011, 50007, 40005, 50035, 10062,
        ] {
            let reply = discord_message(code, 400);
            assert!(reply.len() > 60);
            assert!(!reply.contains("Request failed"));
        }
        assert!(discord_message(50007, 403).contains("privacy settings"));
    }
    #[test]
    fn parser_and_database_failures_are_safe() {
        let n = "bad".parse::<u64>().unwrap_err();
        assert!(message(&n, "purge").contains("digits only"));
        assert!(message(&sqlx::Error::RowNotFound, "playlist").contains("list option"));
        let e = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "secret/path");
        let reply = message(&e, "config-backup");
        assert!(reply.contains("file permissions"));
        assert!(!reply.contains("secret/path"));
    }
    #[test]
    fn real_validation_errors_keep_specific_guidance() {
        let error = crate::security::reason("").unwrap_err();
        assert!(message(error.as_ref(), "warn").contains("Reason must contain"));
        let error = crate::features::due("not-a-time", 0).unwrap_err();
        assert!(message(error.as_ref(), "schedule-post").contains("YYYY-MM-DD HH:MM"));
    }
    #[test]
    fn discord_model_errors_explain_hierarchy_and_size() {
        assert!(message(&s::Error::Model(s::ModelError::Hierarchy), "ban").contains("below both"));
        assert!(
            message(&s::Error::Model(s::ModelError::MessageTooLong(50)), "say")
                .contains("Shorten the text")
        );
    }
    #[test]
    fn long_unicode_error_messages_fit_discord() {
        let reply = bounded(&"😀".repeat(3000));
        assert!(reply.encode_utf16().count() <= 3401);
    }
}
