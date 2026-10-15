use crate::{Context, Data, Error, reply, serenity as s, store};

const USER: u64 = 463980163852664832;
const MESSAGE: &str = "ente appukuttan vanneyyyy";

/// Announce Appukuttan joining voice in a selected text channel.
#[poise::command(
    slash_command,
    guild_only,
    rename = "appukuttan-legacy",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
pub async fn appukuttan_legacy(
    ctx: Context<'_>,
    #[description = "Text channel for join announcements"] channel: Option<s::GuildChannel>,
    #[description = "Enable (default) or disable"] enabled: Option<bool>,
) -> Result<(), Error> {
    let id = if enabled.unwrap_or(true) {
        let channel =
            channel.ok_or_else(|| crate::errors::user("Select the announcement text channel"))?;
        crate::commands::posting_channel(ctx, &channel).await?;
        Some(channel.id.get())
    } else {
        None
    };
    let _guard = ctx.data().configuration_lock.lock().await;
    let mut cfg = ctx.data().settings.read().await.clone();
    cfg.appukuttan_legacy_channel = id;
    store::save(&ctx.data().db, ctx.data().guild.get(), &cfg).await?;
    *ctx.data().settings.write().await = cfg;
    reply(ctx, id.map(|id| format!("Appukuttan Legacy enabled in <#{id}>. When user {USER} joins voice, I’ll post: {MESSAGE}. Voice moves and mute/deafen changes do not trigger it.")).unwrap_or_else(|| "Appukuttan Legacy disabled.".into())).await
}

pub async fn voice(
    ctx: &s::Context,
    data: &Data,
    old: Option<&s::VoiceState>,
    new: &s::VoiceState,
) -> Result<(), Error> {
    if new.guild_id != Some(data.guild)
        || new.user_id.get() != USER
        || new.channel_id.is_none()
        || old.is_some_and(|voice| voice.channel_id.is_some())
    {
        return Ok(());
    }
    let Some(channel) = data.settings.read().await.appukuttan_legacy_channel else {
        return Ok(());
    };
    s::ChannelId::new(channel)
        .send_message(
            &ctx.http,
            s::CreateMessage::new()
                .content(MESSAGE)
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await?;
    Ok(())
}
