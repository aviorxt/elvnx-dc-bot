use crate::{Context, Data, Error, audit, reply, serenity as s, store};
use sqlx::Row;
pub fn all() -> Vec<poise::Command<Data, Error>> {
    vec![
        email(),
        dm_clear(),
        channel_management(),
        number_game(),
        count_toggle(),
        server_lockdown(),
        server_info(),
        join_to_create(),
        log_channel(),
    ]
}
/// ELVNX's official support and administration email addresses.
#[poise::command(slash_command, guild_only)]
async fn email(ctx: Context<'_>) -> Result<(), Error> {
    reply(ctx,"**ELVNX / EMAIL DIRECTORY**\nSupport: support@11x.dpdns.org\nAdministration: admin@11x.dpdns.org").await
}
/// Delete up to 100 tracked outgoing bot DMs; cannot delete recipients' messages.
#[poise::command(
    slash_command,
    guild_only,
    rename = "dm-clear",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD",
    user_cooldown = 30
)]
async fn dm_clear(
    ctx: Context<'_>,
    #[description = "Confirm deletion of this bot's tracked messages"] confirm: bool,
    #[description = "Optional recipient; omit for all tracked recipients"] member: Option<
        s::Member,
    >,
) -> Result<(), Error> {
    if !confirm {
        return reply(
            ctx,
            "Canceled. Only tracked messages authored by this bot can be deleted.",
        )
        .await;
    }
    let user = member.map(|m| m.user.id.to_string());
    let rows =
        sqlx::query("SELECT * FROM outgoing_dms WHERE guild=? AND (? IS NULL OR user=?) LIMIT 100")
            .bind(ctx.data().guild.to_string())
            .bind(&user)
            .bind(&user)
            .fetch_all(&ctx.data().db)
            .await?;
    let mut deleted = 0;
    for row in &rows {
        let channel = s::ChannelId::new(row.get::<String, _>("channel").parse()?);
        let id = s::MessageId::new(row.get::<String, _>("message").parse()?);
        let outcome = channel.delete_message(ctx.http(), id).await;
        let missing = matches!(&outcome,Err(s::Error::Http(e)) if e.status_code().map(|c|c.as_u16())==Some(404));
        if outcome.is_ok() || missing {
            sqlx::query("DELETE FROM outgoing_dms WHERE message=? AND guild=?")
                .bind(id.to_string())
                .bind(ctx.data().guild.to_string())
                .execute(&ctx.data().db)
                .await?;
            deleted += 1;
        }
    }
    audit(ctx,0,"dm-clear",&format!("Cleared {deleted}/{} tracked bot messages. Repeat for further batches; failures are retained for review.",rows.len())).await
}
#[derive(Clone, Copy, poise::ChoiceParameter)]
enum ChannelAction {
    CreateText,
    Rename,
    Topic,
    Delete,
}
/// Create, rename, edit topics, or explicitly delete a text channel.
#[poise::command(
    slash_command,
    guild_only,
    rename = "channel-management",
    required_permissions = "MANAGE_CHANNELS",
    required_bot_permissions = "MANAGE_CHANNELS",
    default_member_permissions = "MANAGE_CHANNELS"
)]
async fn channel_management(
    ctx: Context<'_>,
    #[description = "Action"] action: ChannelAction,
    #[description = "Existing channel for edit/delete"] channel: Option<s::GuildChannel>,
    #[description = "Name or topic"] value: Option<String>,
    #[description = "Required for deletion"] confirm: Option<bool>,
) -> Result<(), Error> {
    let value = value.unwrap_or_default();
    if value.chars().count() > 1000 {
        return Err(crate::errors::user("Value too long"));
    }
    if matches!(action, ChannelAction::CreateText) {
        if value.is_empty() || value.len() > 80 {
            return Err(crate::errors::user("Channel name must be 1–80 bytes"));
        }
        let ch = ctx
            .data()
            .guild
            .create_channel(
                ctx.http(),
                s::CreateChannel::new(value).kind(s::ChannelType::Text),
            )
            .await?;
        return audit(ctx, 0, "channel-create", &format!("Channel {}", ch.id)).await;
    }
    let ch = channel.ok_or_else(|| crate::errors::user("Select a channel"))?;
    crate::commands::posting_channel(ctx, &ch).await?;
    match action {
        ChannelAction::Rename => {
            if value.is_empty() || value.len() > 80 {
                return Err(crate::errors::user("Channel name must be 1–80 bytes"));
            }
            ch.id
                .edit(ctx.http(), s::EditChannel::new().name(value))
                .await?;
        }
        ChannelAction::Topic => {
            ch.id
                .edit(ctx.http(), s::EditChannel::new().topic(value))
                .await?;
        }
        ChannelAction::Delete => {
            if confirm != Some(true) {
                return reply(
                    ctx,
                    "Confirm deletion explicitly; Discord channel history cannot be restored.",
                )
                .await;
            }
            if ch.id == ctx.channel_id() {
                return reply(
                    ctx,
                    "Run this from another channel so the result can be delivered.",
                )
                .await;
            }
            ch.id.delete(ctx.http()).await?;
        }
        ChannelAction::CreateText => {}
    }
    audit(ctx, 0, "channel-management", &format!("Channel {}", ch.id)).await
}
async fn setup_number(
    ctx: Context<'_>,
    enabled: bool,
    channel: Option<s::GuildChannel>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let _guard = ctx.data().configuration_lock.lock().await;
    let mut cfg = ctx.data().settings.read().await.clone();
    let id = if enabled {
        if std::env::var("ENABLE_MESSAGE_CONTENT").as_deref() != Ok("true") {
            return Err(crate::errors::user(
                "Enable Message Content Intent in Discord and ENABLE_MESSAGE_CONTENT=true before starting the counting game",
            ));
        }
        let ch = if let Some(channel) = channel {
            channel
        } else {
            let id = cfg.counting_channel.ok_or_else(|| {
                crate::errors::user("Choose counting channel when enabling the game")
            })?;
            s::ChannelId::new(id)
                .to_channel(ctx.http())
                .await?
                .guild()
                .ok_or_else(|| crate::errors::user("Guild text channel required"))?
        };
        crate::commands::posting_channel(ctx, &ch).await?;
        let bot_id = ctx.serenity_context().cache.current_user().id;
        let bot = ctx.data().guild.member(ctx.http(), bot_id).await?;
        let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
        if !guild.user_permissions_in(&ch, &bot).contains(
            s::Permissions::MANAGE_MESSAGES
                | s::Permissions::ADD_REACTIONS
                | s::Permissions::READ_MESSAGE_HISTORY,
        ) {
            return Err(crate::errors::user(
                "Grant the bot Manage Messages, Add Reactions, and Read Message History in the counting channel",
            ));
        }
        sqlx::query("INSERT INTO counting(guild) VALUES(?) ON CONFLICT(guild) DO NOTHING")
            .bind(ctx.data().guild.to_string())
            .execute(&ctx.data().db)
            .await?;
        let current: i64 = sqlx::query_scalar("SELECT current FROM counting WHERE guild=?")
            .bind(ctx.data().guild.to_string())
            .fetch_one(&ctx.data().db)
            .await?;
        ch.id.send_message(ctx.http(),s::CreateMessage::new().embed(crate::emoji::embed().title("ELVNX / COUNTING GAME").description(format!("Next number: **{}**. Post the next whole number; alternate users. Wrong numbers and non-number posts are deleted. Progress is preserved when toggling the game.", current + 1)).color(0x111111)).allowed_mentions(s::CreateAllowedMentions::new())).await?;
        Some(ch.id.get())
    } else {
        cfg.counting_channel
    };
    cfg.counting_channel = id;
    cfg.counting_disabled = !enabled;
    store::save(&ctx.data().db, ctx.data().guild.get(), &cfg).await?;
    *ctx.data().settings.write().await = cfg;
    reply(ctx, "Counting game updated.").await
}
/// Set up an alternating-member number-following game.
#[poise::command(
    slash_command,
    guild_only,
    rename = "number-game",
    required_permissions = "MANAGE_GUILD",
    required_bot_permissions = "MANAGE_MESSAGES",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn number_game(
    ctx: Context<'_>,
    #[description = "Enabled"] enabled: bool,
    #[description = "Counting channel"] channel: Option<s::GuildChannel>,
) -> Result<(), Error> {
    setup_number(ctx, enabled, channel).await
}
/// Enable or disable counting; preserves the existing count.
#[poise::command(
    slash_command,
    guild_only,
    rename = "count-toggle",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn count_toggle(
    ctx: Context<'_>,
    #[description = "Enable or disable counting"] enabled: bool,
    #[description = "Counting channel; required for first setup"] channel: Option<s::GuildChannel>,
) -> Result<(), Error> {
    setup_number(ctx, enabled, channel).await
}
/// Apply/restore saved posting and voice-connection overwrites across accessible channels.
#[poise::command(
    slash_command,
    guild_only,
    rename = "server-lockdown",
    required_permissions = "MANAGE_GUILD | MANAGE_CHANNELS",
    required_bot_permissions = "MANAGE_CHANNELS",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn server_lockdown(
    ctx: Context<'_>,
    #[description = "Enable or restore"] enabled: bool,
    #[description = "Confirm permission changes across server channels"] confirm: bool,
) -> Result<(), Error> {
    if !confirm {
        return reply(ctx, "Canceled.").await;
    }
    let _guard = ctx.data().configuration_lock.lock().await;
    let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
    let mut changed = 0;
    let mut failed = 0;
    if enabled {
        for (_, channel) in guild.id.channels(ctx.http()).await? {
            if !matches!(
                channel.kind,
                s::ChannelType::Text | s::ChannelType::Voice | s::ChannelType::Category
            ) {
                continue;
            }
            let saved = sqlx::query("INSERT OR IGNORE INTO lockdowns VALUES(?,?,?)")
                .bind(guild.id.to_string())
                .bind(channel.id.to_string())
                .bind(serde_json::to_string(&channel.permission_overwrites)?)
                .execute(&ctx.data().db)
                .await?;
            if saved.rows_affected() == 0 {
                continue;
            }
            let mut overwrites = channel.permission_overwrites.clone();
            let everyone = s::PermissionOverwriteType::Role(s::RoleId::new(guild.id.get()));
            if !overwrites.iter().any(|p| p.kind == everyone) {
                overwrites.push(s::PermissionOverwrite {
                    kind: everyone,
                    allow: s::Permissions::empty(),
                    deny: s::Permissions::empty(),
                });
            }
            let blocked = s::Permissions::SEND_MESSAGES
                | s::Permissions::SEND_MESSAGES_IN_THREADS
                | s::Permissions::CONNECT;
            for p in &mut overwrites {
                let staff = match p.kind {
                    s::PermissionOverwriteType::Role(id) => guild.roles.get(&id).is_some_and(|r| {
                        r.permissions.intersects(
                            s::Permissions::ADMINISTRATOR
                                | s::Permissions::MANAGE_GUILD
                                | s::Permissions::MODERATE_MEMBERS,
                        )
                    }),
                    s::PermissionOverwriteType::Member(id) => {
                        id == guild.owner_id || id == ctx.author().id || id == crate::bot_id(ctx)
                    }
                    _ => true,
                };
                if !staff {
                    p.allow &= !blocked;
                    p.deny |= blocked;
                }
            }
            let bot = s::PermissionOverwriteType::Member(crate::bot_id(ctx));
            if let Some(p) = overwrites.iter_mut().find(|p| p.kind == bot) {
                p.deny &= !blocked;
                p.allow |= blocked;
            } else {
                overwrites.push(s::PermissionOverwrite {
                    kind: bot,
                    allow: blocked,
                    deny: s::Permissions::empty(),
                });
            }
            match channel
                .id
                .edit(ctx.http(), s::EditChannel::new().permissions(overwrites))
                .await
            {
                Ok(_) => changed += 1,
                Err(_) => {
                    failed += 1;
                    // Retain the snapshot: Discord may have applied an edit even
                    // when the response was lost. Restoration remains available.
                }
            }
        }
    } else {
        let rows = sqlx::query("SELECT channel,overwrites FROM lockdowns WHERE guild=?")
            .bind(guild.id.to_string())
            .fetch_all(&ctx.data().db)
            .await?;
        for row in rows {
            let id = s::ChannelId::new(row.get::<String, _>("channel").parse()?);
            let permissions: Vec<s::PermissionOverwrite> =
                serde_json::from_str(row.get::<&str, _>("overwrites"))?;
            if id
                .edit(ctx.http(), s::EditChannel::new().permissions(permissions))
                .await
                .is_ok()
            {
                sqlx::query("DELETE FROM lockdowns WHERE guild=? AND channel=?")
                    .bind(guild.id.to_string())
                    .bind(id.to_string())
                    .execute(&ctx.data().db)
                    .await?;
                changed += 1;
            } else {
                failed += 1;
            }
        }
    }
    audit(ctx,0,"server-lockdown",&format!("Enabled: {enabled}. Changed: {changed}, failed: {failed}. Administrator/owner and role hierarchy exceptions remain; saved overwrites persist for restoration.")).await
}
/// Server details with a matching monochrome embed.
#[poise::command(slash_command, guild_only, rename = "server-info")]
async fn server_info(ctx: Context<'_>) -> Result<(), Error> {
    let g = ctx.data().guild.to_partial_guild(ctx.http()).await?;
    reply(
        ctx,
        format!(
            "**{}**\nServer ID: {}\nOwner: {}\nRoles: {}\nCreated: {}",
            g.name,
            g.id,
            g.owner_id,
            g.roles.len(),
            g.id.created_at()
        ),
    )
    .await
}
/// Set a voice hub that creates temporary rooms, plus owner-control buttons.
#[poise::command(
    slash_command,
    guild_only,
    rename = "join-to-create",
    required_permissions = "MANAGE_GUILD | MANAGE_CHANNELS",
    required_bot_permissions = "MANAGE_CHANNELS | MOVE_MEMBERS | MANAGE_ROLES",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn join_to_create(
    ctx: Context<'_>,
    #[description = "Enabled"] enabled: bool,
    #[description = "Existing voice hub"] hub: Option<s::GuildChannel>,
    #[description = "Text channel for controls"] panel: Option<s::GuildChannel>,
) -> Result<(), Error> {
    crate::voice_rooms::configure(ctx, enabled, hub, panel).await
}
/// Configure the staff log destination for supported moderation and gateway events.
#[poise::command(
    slash_command,
    guild_only,
    rename = "log",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn log_channel(
    ctx: Context<'_>,
    #[description = "Enabled"] enabled: bool,
    #[description = "Private staff log channel"] channel: Option<s::GuildChannel>,
) -> Result<(), Error> {
    crate::eventlog::configure(ctx, enabled, channel).await
}

pub async fn gateway(ctx: &s::Context, event: &s::FullEvent, data: &Data) -> Result<bool, Error> {
    let cfg = data.settings.read().await.clone();
    match event {
        s::FullEvent::Message { new_message: m }
            if m.guild_id == Some(data.guild)
                && !m.author.bot
                && m.webhook_id.is_none()
                && std::env::var("ENABLE_MESSAGE_CONTENT").as_deref() == Ok("true")
                && !cfg.counting_disabled
                && cfg.counting_channel == Some(m.channel_id.get()) =>
        {
            let value = m.content.trim().parse::<i64>().ok().filter(|n| *n > 0);
            let accepted = if let Some(value) = value {
                store::accept_count(&data.db, data.guild.get(), m.author.id.get(), value).await?
            } else {
                false
            };
            if accepted {
                let _ = m.react(&ctx.http, crate::emoji::reaction("check")?).await;
            } else {
                m.delete(&ctx.http).await?;
            }
            return Ok(true);
        }
        _ => {}
    }
    Ok(false)
}
