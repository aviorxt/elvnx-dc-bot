use crate::{Context, Data, Error, reply, serenity as s, store};

pub const ROLE: u64 = 1554097852672770118;
fn matches(
    channel: Option<u64>,
    guild: u64,
    message_guild: Option<u64>,
    message_channel: u64,
    bot: bool,
    webhook: bool,
    roles: &[s::RoleId],
) -> bool {
    channel == Some(message_channel)
        && message_guild == Some(guild)
        && !bot
        && !webhook
        && roles.contains(&s::RoleId::new(ROLE))
}
/// Add a white check mark to human messages mentioning the configured ELVNX role.
#[poise::command(
    slash_command,
    guild_only,
    rename = "always-react",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
pub async fn always_react(
    ctx: Context<'_>,
    #[description = "Channel to watch when enabling"] channel: Option<s::GuildChannel>,
    #[description = "Enable (default) or disable"] enabled: Option<bool>,
) -> Result<(), Error> {
    let id = if enabled.unwrap_or(true) {
        if std::env::var("ENABLE_MESSAGE_CONTENT").as_deref() != Ok("true") {
            return Err(crate::errors::user(
                "Enable Message Content Intent and ENABLE_MESSAGE_CONTENT=true so the bot can reliably read role mentions",
            ));
        }
        let channel = channel.ok_or_else(|| crate::errors::user("Select a channel to watch"))?;
        crate::commands::posting_channel(ctx, &channel).await?;
        let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
        if !guild.roles.contains_key(&s::RoleId::new(ROLE)) {
            return Err(crate::errors::user(
                "The configured role no longer exists in this server",
            ));
        }
        let bot_id = ctx.serenity_context().cache.current_user().id;
        let bot = ctx.data().guild.member(ctx.http(), bot_id).await?;
        if !guild.user_permissions_in(&channel, &bot).contains(
            s::Permissions::VIEW_CHANNEL
                | s::Permissions::ADD_REACTIONS
                | s::Permissions::READ_MESSAGE_HISTORY,
        ) {
            return Err(crate::errors::user(
                "Grant the bot View Channel, Read Message History, and Add Reactions in this channel",
            ));
        }
        Some(channel.id.get())
    } else {
        None
    };
    let _guard = ctx.data().configuration_lock.lock().await;
    let mut cfg = ctx.data().settings.read().await.clone();
    cfg.always_react_channel = id;
    store::save(&ctx.data().db, ctx.data().guild.get(), &cfg).await?;
    *ctx.data().settings.write().await = cfg;
    reply(ctx, id.map(|id| format!("Always-react enabled in <#{id}>. Human messages mentioning <@&{ROLE}> receive {}. Bot and webhook messages are ignored.", crate::emoji::text("check"))).unwrap_or_else(|| "Always-react disabled.".into())).await
}
pub async fn message(ctx: &s::Context, data: &Data, message: &s::Message) -> Result<(), Error> {
    let channel = data.settings.read().await.always_react_channel;
    if matches(
        channel,
        data.guild.get(),
        message.guild_id.map(|id| id.get()),
        message.channel_id.get(),
        message.author.bot,
        message.webhook_id.is_some(),
        &message.mention_roles,
    ) {
        message
            .react(&ctx.http, crate::emoji::reaction("check")?)
            .await?;
    }
    Ok(())
}
