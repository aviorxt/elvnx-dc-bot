use crate::{Context, Error, audit, reply, security, serenity as s, store};
use sqlx::Row;

pub fn all() -> Vec<poise::Command<crate::Data, Error>> {
    let mut commands = vec![
        help(),
        ping(),
        userinfo(),
        serverinfo(),
        warn(),
        warnings(),
        delwarn(),
        case(),
        timeout(),
        untimeout(),
        kick(),
        ban(),
        unban(),
        purge(),
        slowmode(),
        lock(),
        unlock(),
        configure(),
        automod(),
        selfrole_allow(),
        selfrole_remove(),
        selfrole(),
        role_add(),
        role_remove(),
        ticket(),
        ticket_close(),
        say(),
        dm(),
        sticky(),
        sticky_remove(),
        avrxt_dev(),
        bot_restart(),
    ];
    commands.extend(crate::extras::all());
    commands.extend(crate::community::all());
    commands.extend(crate::music::all());
    commands.extend(crate::features::all());
    commands.push(crate::afk::afk_move());
    commands.push(crate::always_react::always_react());
    commands.push(crate::legacy::appukuttan_legacy());
    commands.push(crate::rules::rules());
    commands.push(crate::live_status::bot_status_live());
    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_registration_restricts_admin_messaging_and_guild_scope() {
        let commands = all();
        assert_eq!(commands.len(), crate::music::COMMAND_COUNT);
        assert!(commands.iter().all(|c| c.guild_only));
        let names: std::collections::HashSet<_> =
            commands.iter().map(|c| c.name.as_ref()).collect();
        assert_eq!(names.len(), commands.len());
        for name in [
            "say",
            "dm",
            "sticky",
            "sticky_remove",
            "configure",
            "automod",
        ] {
            let c = commands.iter().find(|c| c.name == name).unwrap();
            assert!(
                c.required_permissions
                    .contains(s::Permissions::MANAGE_GUILD)
            );
        }
        for name in ["ban", "kick", "purge", "bot-restart"] {
            assert!(
                commands
                    .iter()
                    .find(|c| c.name == name)
                    .unwrap()
                    .parameters
                    .iter()
                    .any(|p| p.name == "confirm" && p.required)
            );
        }
    }
}

/// Show every available bot command and its purpose.
#[poise::command(slash_command, guild_only)]
async fn help(
    ctx: Context<'_>,
    #[description = "Details page, starting at 1"] page: Option<u32>,
) -> Result<(), Error> {
    let commands = &ctx.framework().options.commands;
    let page = page.unwrap_or(1).clamp(1, 10);
    let names = commands
        .iter()
        .map(|c| format!("/{}", c.name))
        .collect::<Vec<_>>()
        .join(" · ");
    let descriptions = commands
        .iter()
        .skip((page as usize - 1) * 12)
        .take(12)
        .map(|c| {
            format!(
                "**/{}** — {}",
                c.name,
                c.description.as_deref().unwrap_or("See command options.")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    reply(ctx,format!("**ALL {} COMMANDS**\n{names}\n\n**DETAILS / PAGE {page}**\n{descriptions}\n\nUse /help page:2 for the next descriptions. Required permissions apply at execution.",commands.len())).await
}
/// Bot uptime and readiness.
#[poise::command(slash_command, guild_only)]
async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    reply(
        ctx,
        format!(
            "ELVNX online • uptime {} seconds",
            ctx.data().started.elapsed().as_secs()
        ),
    )
    .await
}
/// Account and server membership information.
#[poise::command(slash_command, guild_only)]
async fn userinfo(
    ctx: Context<'_>,
    #[description = "Member"] member: s::Member,
) -> Result<(), Error> {
    reply(
        ctx,
        format!(
            "{} • {}\nAccount created: {}\nJoined: {}\nRoles: {}",
            member.user.name,
            member.user.id,
            member.user.created_at(),
            member.joined_at.map(|t| t.to_string()).unwrap_or_default(),
            member.roles.len()
        ),
    )
    .await
}
/// Server information.
#[poise::command(slash_command, guild_only)]
async fn serverinfo(ctx: Context<'_>) -> Result<(), Error> {
    let g = ctx.data().guild.to_partial_guild(ctx.http()).await?;
    reply(
        ctx,
        format!(
            "{}\nID: {}\nOwner: {}\nRoles: {}",
            g.name,
            g.id,
            g.owner_id,
            g.roles.len()
        ),
    )
    .await
}
/// Persist a moderation warning.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MODERATE_MEMBERS",
    default_member_permissions = "MODERATE_MEMBERS"
)]
async fn warn(
    ctx: Context<'_>,
    #[description = "Member"] member: s::Member,
    #[description = "Reason"] reason: String,
) -> Result<(), Error> {
    let reason = security::reason(&reason)?;
    security::target(ctx, &member, s::Permissions::MODERATE_MEMBERS).await?;
    audit(ctx, member.user.id.get(), "warn", reason).await
}
/// List the latest 20 active warnings for a member.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MODERATE_MEMBERS",
    default_member_permissions = "MODERATE_MEMBERS"
)]
async fn warnings(
    ctx: Context<'_>,
    #[description = "Member"] member: s::Member,
) -> Result<(), Error> {
    let rows=sqlx::query("SELECT id,reason,created FROM cases WHERE guild=? AND target=? AND action='warn' AND revoked=0 ORDER BY id DESC LIMIT 20").bind(ctx.data().guild.to_string()).bind(member.user.id.to_string()).fetch_all(&ctx.data().db).await?;
    let text = rows
        .into_iter()
        .map(|r| {
            format!(
                "#{} • {} • {}",
                r.get::<i64, _>("id"),
                r.get::<String, _>("created"),
                r.get::<String, _>("reason")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    reply(
        ctx,
        if text.is_empty() {
            "No active warnings.".into()
        } else {
            text.chars().take(1900).collect::<String>()
        },
    )
    .await
}
/// Revoke a warning without erasing the audit history.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MODERATE_MEMBERS",
    default_member_permissions = "MODERATE_MEMBERS"
)]
async fn delwarn(
    ctx: Context<'_>,
    #[description = "Warning case ID"] id: i64,
) -> Result<(), Error> {
    let result = sqlx::query(
        "UPDATE cases SET revoked=1 WHERE id=? AND guild=? AND action='warn' AND revoked=0",
    )
    .bind(id)
    .bind(ctx.data().guild.to_string())
    .execute(&ctx.data().db)
    .await?;
    if result.rows_affected() == 0 {
        return reply(ctx, "Active warning not found in this server.").await;
    }
    audit(ctx, 0, "revoke-warning", &format!("Warning #{id}")).await
}
/// Inspect a case in this server.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MODERATE_MEMBERS",
    default_member_permissions = "MODERATE_MEMBERS"
)]
async fn case(ctx: Context<'_>, #[description = "Case ID"] id: i64) -> Result<(), Error> {
    let row = sqlx::query("SELECT * FROM cases WHERE id=? AND guild=?")
        .bind(id)
        .bind(ctx.data().guild.to_string())
        .fetch_optional(&ctx.data().db)
        .await?;
    match row {
        Some(r) => {
            reply(
                ctx,
                format!(
                    "Case #{id} • {}\nMember: {}\nModerator: {}\nReason: {}\nRevoked: {}",
                    r.get::<String, _>("action"),
                    r.get::<String, _>("target"),
                    r.get::<String, _>("actor"),
                    r.get::<String, _>("reason"),
                    r.get::<i64, _>("revoked") != 0
                ),
            )
            .await
        }
        None => reply(ctx, "Case not found.").await,
    }
}
/// Timeout a member for 1 to 40320 minutes (28 days).
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MODERATE_MEMBERS",
    required_bot_permissions = "MODERATE_MEMBERS",
    default_member_permissions = "MODERATE_MEMBERS"
)]
async fn timeout(
    ctx: Context<'_>,
    #[description = "Member"] mut member: s::Member,
    #[description = "Minutes"] minutes: u32,
    #[description = "Reason"] reason: String,
) -> Result<(), Error> {
    let reason = security::reason(&reason)?;
    if !(1..=40320).contains(&minutes) {
        return reply(ctx, "Duration must be 1–40320 minutes.").await;
    }
    security::target(ctx, &member, s::Permissions::MODERATE_MEMBERS).await?;
    let until = s::Timestamp::from_unix_timestamp(
        s::Timestamp::now().unix_timestamp() + i64::from(minutes) * 60,
    )?;
    member
        .edit(
            ctx.http(),
            s::EditMember::new()
                .disable_communication_until_datetime(until)
                .audit_log_reason(reason),
        )
        .await?;
    audit(ctx, member.user.id.get(), "timeout", reason).await
}
/// Remove a member's timeout.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MODERATE_MEMBERS",
    required_bot_permissions = "MODERATE_MEMBERS",
    default_member_permissions = "MODERATE_MEMBERS"
)]
async fn untimeout(
    ctx: Context<'_>,
    #[description = "Member"] mut member: s::Member,
    #[description = "Reason"] reason: String,
) -> Result<(), Error> {
    let reason = security::reason(&reason)?;
    security::target(ctx, &member, s::Permissions::MODERATE_MEMBERS).await?;
    member
        .edit(
            ctx.http(),
            s::EditMember::new()
                .enable_communication()
                .audit_log_reason(reason),
        )
        .await?;
    audit(ctx, member.user.id.get(), "remove-timeout", reason).await
}
/// Kick a member. Confirmation must be true.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "KICK_MEMBERS",
    required_bot_permissions = "KICK_MEMBERS",
    default_member_permissions = "KICK_MEMBERS"
)]
async fn kick(
    ctx: Context<'_>,
    #[description = "Member"] member: s::Member,
    #[description = "Reason"] reason: String,
    #[description = "Confirm removal"] confirm: bool,
) -> Result<(), Error> {
    if !confirm {
        return reply(ctx, "Canceled.").await;
    }
    let reason = security::reason(&reason)?;
    security::target(ctx, &member, s::Permissions::KICK_MEMBERS).await?;
    member.kick_with_reason(ctx.http(), reason).await?;
    audit(ctx, member.user.id.get(), "kick", reason).await
}
/// Ban a member without deleting their messages. Confirmation must be true.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "BAN_MEMBERS",
    required_bot_permissions = "BAN_MEMBERS",
    default_member_permissions = "BAN_MEMBERS"
)]
async fn ban(
    ctx: Context<'_>,
    #[description = "Member"] member: s::Member,
    #[description = "Reason"] reason: String,
    #[description = "Confirm ban"] confirm: bool,
) -> Result<(), Error> {
    if !confirm {
        return reply(ctx, "Canceled.").await;
    }
    let reason = security::reason(&reason)?;
    security::target(ctx, &member, s::Permissions::BAN_MEMBERS).await?;
    ctx.data()
        .guild
        .ban_with_reason(ctx.http(), member.user.id, 0, reason)
        .await?;
    audit(ctx, member.user.id.get(), "ban", reason).await
}
/// Unban a user ID.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "BAN_MEMBERS",
    required_bot_permissions = "BAN_MEMBERS",
    default_member_permissions = "BAN_MEMBERS"
)]
async fn unban(
    ctx: Context<'_>,
    #[description = "User ID"] user: s::UserId,
    #[description = "Reason"] reason: String,
) -> Result<(), Error> {
    let reason = security::reason(&reason)?;
    ctx.data().guild.unban(ctx.http(), user).await?;
    audit(ctx, user.get(), "unban", reason).await
}
/// Delete up to 100 recent messages in this channel; skip messages older than 14 days.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_MESSAGES",
    required_bot_permissions = "MANAGE_MESSAGES",
    default_member_permissions = "MANAGE_MESSAGES"
)]
async fn purge(
    ctx: Context<'_>,
    #[description = "1–100 messages"] count: u8,
    #[description = "Confirm deletion"] confirm: bool,
) -> Result<(), Error> {
    if !confirm || !(1..=100).contains(&count) {
        return reply(ctx, "Confirm deletion and choose 1–100 messages.").await;
    }
    ctx.defer_ephemeral().await?;
    let cutoff = s::Timestamp::now().unix_timestamp() - 14 * 86400 + 60;
    let messages = ctx
        .channel_id()
        .messages(ctx.http(), s::GetMessages::new().limit(count))
        .await?;
    let ids: Vec<_> = messages
        .into_iter()
        .filter(|m| m.timestamp.unix_timestamp() > cutoff)
        .map(|m| m.id)
        .collect();
    if ids.is_empty() {
        return reply(ctx, "No eligible recent messages.").await;
    }
    ctx.channel_id()
        .delete_messages(ctx.http(), ids.iter().copied())
        .await?;
    audit(
        ctx,
        0,
        "purge",
        &format!("Deleted {} messages in {}", ids.len(), ctx.channel_id()),
    )
    .await
}
/// Set channel slowmode (0 disables; max 21600 seconds).
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_CHANNELS",
    required_bot_permissions = "MANAGE_CHANNELS",
    default_member_permissions = "MANAGE_CHANNELS"
)]
async fn slowmode(ctx: Context<'_>, #[description = "Seconds"] seconds: u16) -> Result<(), Error> {
    if seconds > 21600 {
        return reply(ctx, "Maximum is 21600 seconds.").await;
    }
    ctx.channel_id()
        .edit(
            ctx.http(),
            s::EditChannel::new().rate_limit_per_user(seconds),
        )
        .await?;
    audit(
        ctx,
        0,
        "slowmode",
        &format!("{seconds}s in {}", ctx.channel_id()),
    )
    .await
}
/// Lock this channel for everyone and preserve its prior permission overwrite.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_CHANNELS",
    required_bot_permissions = "MANAGE_CHANNELS",
    default_member_permissions = "MANAGE_CHANNELS"
)]
async fn lock(ctx: Context<'_>) -> Result<(), Error> {
    let _guard = ctx.data().configuration_lock.lock().await;
    let ch = ctx
        .channel_id()
        .to_channel(ctx.http())
        .await?
        .guild()
        .ok_or("Guild channel required")?;
    let role = s::RoleId::new(ctx.data().guild.get());
    let existing = ch
        .permission_overwrites
        .iter()
        .find(|p| p.kind == s::PermissionOverwriteType::Role(role));
    let (allow, deny) = existing
        .map(|p| (p.allow, p.deny))
        .unwrap_or((s::Permissions::empty(), s::Permissions::empty()));
    sqlx::query("INSERT INTO locks VALUES(?,?,?,?,?)")
        .bind(ctx.data().guild.to_string())
        .bind(ch.id.to_string())
        .bind(existing.is_some())
        .bind(allow.bits().to_string())
        .bind(deny.bits().to_string())
        .execute(&ctx.data().db)
        .await
        .map_err(|_| "Channel already locked or database unavailable")?;
    if let Err(e) = ch
        .id
        .create_permission(
            ctx.http(),
            s::PermissionOverwrite {
                allow: allow & !s::Permissions::SEND_MESSAGES,
                deny: deny | s::Permissions::SEND_MESSAGES,
                kind: s::PermissionOverwriteType::Role(role),
            },
        )
        .await
    {
        sqlx::query("DELETE FROM locks WHERE guild=? AND channel=?")
            .bind(ctx.data().guild.to_string())
            .bind(ch.id.to_string())
            .execute(&ctx.data().db)
            .await?;
        return Err(e.into());
    }
    audit(ctx, 0, "lock", &format!("Channel {}", ch.id)).await
}
/// Unlock this channel by restoring the original everyone overwrite.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_CHANNELS",
    required_bot_permissions = "MANAGE_CHANNELS",
    default_member_permissions = "MANAGE_CHANNELS"
)]
async fn unlock(ctx: Context<'_>) -> Result<(), Error> {
    let _guard = ctx.data().configuration_lock.lock().await;
    let row = sqlx::query("SELECT * FROM locks WHERE guild=? AND channel=?")
        .bind(ctx.data().guild.to_string())
        .bind(ctx.channel_id().to_string())
        .fetch_optional(&ctx.data().db)
        .await?
        .ok_or("No saved channel lock")?;
    let role = s::RoleId::new(ctx.data().guild.get());
    if row.get::<i64, _>("had_override") == 0 {
        ctx.channel_id()
            .delete_permission(ctx.http(), s::PermissionOverwriteType::Role(role))
            .await?;
    } else {
        ctx.channel_id()
            .create_permission(
                ctx.http(),
                s::PermissionOverwrite {
                    kind: s::PermissionOverwriteType::Role(role),
                    allow: s::Permissions::from_bits_truncate(
                        row.get::<String, _>("allow_bits").parse()?,
                    ),
                    deny: s::Permissions::from_bits_truncate(
                        row.get::<String, _>("deny_bits").parse()?,
                    ),
                },
            )
            .await?;
    }
    sqlx::query("DELETE FROM locks WHERE guild=? AND channel=?")
        .bind(ctx.data().guild.to_string())
        .bind(ctx.channel_id().to_string())
        .execute(&ctx.data().db)
        .await?;
    audit(ctx, 0, "unlock", &format!("Channel {}", ctx.channel_id())).await
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
enum Setting {
    LogChannel,
    WelcomeChannel,
    WelcomeMessage,
    AutoRole,
    TicketCategory,
    TicketStaffRole,
}
/// Configure logging, welcomes, autorole, or ticket settings. Use 'off' to clear.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn configure(
    ctx: Context<'_>,
    #[description = "Setting"] setting: Setting,
    #[description = "ID, welcome text, or off"] value: String,
) -> Result<(), Error> {
    let _guard = ctx.data().configuration_lock.lock().await;
    let mut cfg = ctx.data().settings.read().await.clone();
    let number = if value == "off" || matches!(setting, Setting::WelcomeMessage) {
        None
    } else {
        Some(value.trim().parse::<u64>()?)
    };
    if let Some(id) = number {
        if matches!(setting, Setting::AutoRole | Setting::TicketStaffRole) {
            let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
            let role = guild
                .roles
                .get(&s::RoleId::new(id))
                .ok_or("Role not found")?;
            if matches!(setting, Setting::AutoRole) {
                security::safe_role(ctx, role).await?;
            } else if id == guild.id.get() || role.managed {
                return Err("Use a normal private staff role".into());
            }
        } else {
            let ch = s::ChannelId::new(id)
                .to_channel(ctx.http())
                .await?
                .guild()
                .ok_or("Guild channel required")?;
            if ch.guild_id != ctx.data().guild {
                return Err("Channel belongs to another server".into());
            }
            if matches!(setting, Setting::TicketCategory) && ch.kind != s::ChannelType::Category {
                return Err("Choose a category ID".into());
            }
            if !matches!(setting, Setting::TicketCategory) && ch.kind != s::ChannelType::Text {
                return Err("Choose a text channel ID".into());
            }
        }
    }
    match setting {
        Setting::LogChannel => cfg.log_channel = number,
        Setting::WelcomeChannel => cfg.welcome_channel = number,
        Setting::WelcomeMessage => {
            if value.chars().count() > 1500 {
                return Err("Welcome text too long".into());
            }
            cfg.welcome_message = if value == "off" { String::new() } else { value };
        }
        Setting::AutoRole => cfg.autorole = number,
        Setting::TicketCategory => cfg.ticket_category = number,
        Setting::TicketStaffRole => cfg.ticket_staff_role = number,
    }
    store::save(&ctx.data().db, ctx.data().guild.get(), &cfg).await?;
    *ctx.data().settings.write().await = cfg;
    reply(ctx, "Configuration saved.").await
}
/// Configure automod: spam, mass mentions, blocked words, and optional invite blocking.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn automod(
    ctx: Context<'_>,
    #[description = "Enable automod"] enabled: bool,
    #[description = "Block Discord invites"] block_invites: bool,
    #[description = "Comma-separated blocked words (max 50)"] words: Option<String>,
) -> Result<(), Error> {
    if enabled && std::env::var("ENABLE_MESSAGE_CONTENT").as_deref() != Ok("true") {
        return reply(
            ctx,
            "Enable Message Content Intent and ENABLE_MESSAGE_CONTENT first.",
        )
        .await;
    }
    let _guard = ctx.data().configuration_lock.lock().await;
    let mut cfg = ctx.data().settings.read().await.clone();
    let words: Vec<_> = words
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if words.len() > 50 || words.iter().any(|w| w.len() > 60) {
        return Err("Maximum 50 words of 60 bytes each".into());
    }
    cfg.automod = enabled;
    cfg.block_invites = block_invites;
    cfg.blocked_words = words;
    store::save(&ctx.data().db, ctx.data().guild.get(), &cfg).await?;
    *ctx.data().settings.write().await = cfg;
    reply(
        ctx,
        "Automod settings saved. Actions delete violations; no automatic bans.",
    )
    .await
}
/// Allow members to claim a non-privileged role with /selfrole.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_ROLES",
    default_member_permissions = "MANAGE_ROLES"
)]
async fn selfrole_allow(
    ctx: Context<'_>,
    #[description = "Role"] role: s::Role,
) -> Result<(), Error> {
    security::safe_role(ctx, &role).await?;
    let _guard = ctx.data().configuration_lock.lock().await;
    let mut cfg = ctx.data().settings.read().await.clone();
    if !cfg.self_roles.contains(&role.id.get()) {
        cfg.self_roles.push(role.id.get());
    }
    store::save(&ctx.data().db, ctx.data().guild.get(), &cfg).await?;
    *ctx.data().settings.write().await = cfg;
    reply(ctx,"Role added to the self-role allowlist. Only add roles whose channel access should be available to every member.").await
}
/// Remove a role from the self-role allowlist.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_ROLES",
    default_member_permissions = "MANAGE_ROLES"
)]
async fn selfrole_remove(
    ctx: Context<'_>,
    #[description = "Role"] role: s::Role,
) -> Result<(), Error> {
    let _guard = ctx.data().configuration_lock.lock().await;
    let mut cfg = ctx.data().settings.read().await.clone();
    cfg.self_roles.retain(|id| *id != role.id.get());
    store::save(&ctx.data().db, ctx.data().guild.get(), &cfg).await?;
    *ctx.data().settings.write().await = cfg;
    reply(ctx, "Role removed from allowlist.").await
}
/// Toggle an explicitly approved self-service role on your account.
#[poise::command(slash_command, guild_only, required_bot_permissions = "MANAGE_ROLES")]
async fn selfrole(
    ctx: Context<'_>,
    #[description = "Approved role"] role: s::Role,
) -> Result<(), Error> {
    if !ctx
        .data()
        .settings
        .read()
        .await
        .self_roles
        .contains(&role.id.get())
    {
        return reply(ctx, "Role is not approved for self-service.").await;
    }
    let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
    let bot = ctx
        .data()
        .guild
        .member(ctx.http(), crate::bot_id(ctx))
        .await?;
    // Revalidate permissions at use time in case the configured role was promoted.
    let current = guild.roles.get(&role.id).ok_or("Role not found")?;
    if current.managed
        || !current.permissions.is_empty()
        || !security::higher(
            security::highest(&guild, &bot),
            (i64::from(current.position), current.id.get()),
        )
    {
        return reply(
            ctx,
            "Self-service roles must have no global permission grants and remain below the bot.",
        )
        .await;
    }
    let member = ctx.data().guild.member(ctx.http(), ctx.author().id).await?;
    if member.roles.contains(&role.id) {
        member.remove_role(ctx.http(), role.id).await?;
        reply(ctx, "Role removed.").await
    } else {
        member.add_role(ctx.http(), role.id).await?;
        reply(ctx, "Role added.").await
    }
}
/// Give a member a non-privileged role below your role and the bot.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_ROLES",
    required_bot_permissions = "MANAGE_ROLES",
    default_member_permissions = "MANAGE_ROLES"
)]
async fn role_add(
    ctx: Context<'_>,
    #[description = "Member"] member: s::Member,
    #[description = "Role"] role: s::Role,
) -> Result<(), Error> {
    security::target(ctx, &member, s::Permissions::MANAGE_ROLES).await?;
    security::safe_role(ctx, &role).await?;
    member.add_role(ctx.http(), role.id).await?;
    audit(
        ctx,
        member.user.id.get(),
        "role-add",
        &format!("Role {}", role.id),
    )
    .await
}
/// Remove a non-privileged role below your role and the bot.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_ROLES",
    required_bot_permissions = "MANAGE_ROLES",
    default_member_permissions = "MANAGE_ROLES"
)]
async fn role_remove(
    ctx: Context<'_>,
    #[description = "Member"] member: s::Member,
    #[description = "Role"] role: s::Role,
) -> Result<(), Error> {
    security::target(ctx, &member, s::Permissions::MANAGE_ROLES).await?;
    security::safe_role(ctx, &role).await?;
    member.remove_role(ctx.http(), role.id).await?;
    audit(
        ctx,
        member.user.id.get(),
        "role-remove",
        &format!("Role {}", role.id),
    )
    .await
}

/// Open a private support ticket (one open ticket per member).
#[poise::command(
    slash_command,
    guild_only,
    required_bot_permissions = "MANAGE_CHANNELS",
    user_cooldown = 60
)]
async fn ticket(
    ctx: Context<'_>,
    #[description = "Short issue summary"] summary: String,
) -> Result<(), Error> {
    let summary = security::reason(&summary)?;
    ctx.defer_ephemeral().await?;
    let _guard = ctx.data().configuration_lock.lock().await;
    let cfg = ctx.data().settings.read().await.clone();
    let category = cfg
        .ticket_category
        .ok_or("Ticket category not configured")?;
    let staff = cfg
        .ticket_staff_role
        .ok_or("Ticket staff role not configured")?;
    let existing: Option<String> =
        sqlx::query_scalar("SELECT channel FROM tickets WHERE guild=? AND owner=? AND closed=0")
            .bind(ctx.data().guild.to_string())
            .bind(ctx.author().id.to_string())
            .fetch_optional(&ctx.data().db)
            .await?;
    if let Some(id) = existing {
        return reply(ctx, format!("You already have ticket <#{id}>.")).await;
    }
    let permissions = s::Permissions::VIEW_CHANNEL
        | s::Permissions::SEND_MESSAGES
        | s::Permissions::READ_MESSAGE_HISTORY
        | s::Permissions::ATTACH_FILES
        | s::Permissions::EMBED_LINKS;
    let ch = ctx
        .data()
        .guild
        .create_channel(
            ctx.http(),
            s::CreateChannel::new(format!("ticket-{}", ctx.author().id))
                .kind(s::ChannelType::Text)
                .category(s::ChannelId::new(category))
                .permissions([
                    s::PermissionOverwrite {
                        kind: s::PermissionOverwriteType::Role(s::RoleId::new(
                            ctx.data().guild.get(),
                        )),
                        allow: s::Permissions::empty(),
                        deny: s::Permissions::VIEW_CHANNEL,
                    },
                    s::PermissionOverwrite {
                        kind: s::PermissionOverwriteType::Member(ctx.author().id),
                        allow: permissions,
                        deny: s::Permissions::empty(),
                    },
                    s::PermissionOverwrite {
                        kind: s::PermissionOverwriteType::Role(s::RoleId::new(staff)),
                        allow: permissions,
                        deny: s::Permissions::empty(),
                    },
                    s::PermissionOverwrite {
                        kind: s::PermissionOverwriteType::Member(crate::bot_id(ctx)),
                        allow: permissions | s::Permissions::MANAGE_CHANNELS,
                        deny: s::Permissions::empty(),
                    },
                ]),
        )
        .await?;
    if let Err(error) = sqlx::query("INSERT INTO tickets VALUES(?,?,?,0)")
        .bind(ctx.data().guild.to_string())
        .bind(ctx.author().id.to_string())
        .bind(ch.id.to_string())
        .execute(&ctx.data().db)
        .await
    {
        ch.id.delete(ctx.http()).await?;
        return Err(error.into());
    }
    ch.id.send_message(ctx.http(),s::CreateMessage::new().content(format!("Ticket owner: {}\nIssue: {summary}\nUse /ticket_close to close. Closed tickets are retained for staff review.",ctx.author().id)).allowed_mentions(s::CreateAllowedMentions::new())).await?;
    audit(
        ctx,
        ctx.author().id.get(),
        "ticket-open",
        &format!("Channel {}", ch.id),
    )
    .await?;
    reply(ctx, format!("Your private ticket: <#{}>", ch.id)).await
}
/// Close this ticket without deleting its history.
#[poise::command(
    slash_command,
    guild_only,
    required_bot_permissions = "MANAGE_CHANNELS"
)]
async fn ticket_close(ctx: Context<'_>) -> Result<(), Error> {
    let _guard = ctx.data().configuration_lock.lock().await;
    let owner: Option<String> =
        sqlx::query_scalar("SELECT owner FROM tickets WHERE guild=? AND channel=? AND closed=0")
            .bind(ctx.data().guild.to_string())
            .bind(ctx.channel_id().to_string())
            .fetch_optional(&ctx.data().db)
            .await?;
    let owner: s::UserId = s::UserId::new(owner.ok_or("Not an open ticket")?.parse()?);
    let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
    let member = ctx.data().guild.member(ctx.http(), ctx.author().id).await?;
    if owner != ctx.author().id
        && !guild
            .member_permissions(&member)
            .contains(s::Permissions::MANAGE_CHANNELS)
    {
        return reply(
            ctx,
            "Only the ticket owner or channel moderators can close it.",
        )
        .await;
    }
    ctx.channel_id()
        .create_permission(
            ctx.http(),
            s::PermissionOverwrite {
                kind: s::PermissionOverwriteType::Member(owner),
                allow: s::Permissions::VIEW_CHANNEL | s::Permissions::READ_MESSAGE_HISTORY,
                deny: s::Permissions::SEND_MESSAGES,
            },
        )
        .await?;
    sqlx::query("UPDATE tickets SET closed=1 WHERE guild=? AND channel=?")
        .bind(ctx.data().guild.to_string())
        .bind(ctx.channel_id().to_string())
        .execute(&ctx.data().db)
        .await?;
    audit(
        ctx,
        owner.get(),
        "ticket-close",
        &format!("Channel {}", ctx.channel_id()),
    )
    .await
}

pub async fn posting_channel(ctx: Context<'_>, channel: &s::GuildChannel) -> Result<(), Error> {
    if channel.guild_id != ctx.data().guild || channel.kind != s::ChannelType::Text {
        return Err("Choose a text channel in this server".into());
    }
    let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
    for user in [ctx.author().id, crate::bot_id(ctx)] {
        let member = guild.id.member(ctx.http(), user).await?;
        if !guild
            .user_permissions_in(channel, &member)
            .contains(s::Permissions::VIEW_CHANNEL | s::Permissions::SEND_MESSAGES)
        {
            return Err("You and the bot need access to the destination channel".into());
        }
    }
    Ok(())
}
/// Post text or a media URL in a channel using the bot; mentions are disabled.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD",
    user_cooldown = 5
)]
async fn say(
    ctx: Context<'_>,
    #[description = "Destination"] channel: s::GuildChannel,
    #[description = "Text, max 2000 characters"] message: String,
) -> Result<(), Error> {
    if message.trim().is_empty() || message.chars().count() > 2000 {
        return Err("Message must be 1–2000 characters".into());
    }
    posting_channel(ctx, &channel).await?;
    let sent = channel
        .id
        .send_message(
            ctx.http(),
            s::CreateMessage::new()
                .embed(
                    s::CreateEmbed::new()
                        .title("ELVNX / MESSAGE")
                        .description(message)
                        .color(0x111111)
                        .footer(s::CreateEmbedFooter::new("11X • private collective")),
                )
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await?;
    audit(
        ctx,
        0,
        "say",
        &format!("Channel {}, message {}", channel.id, sent.id),
    )
    .await
}
/// DM a server member without disclosing the moderator's name to the recipient.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD",
    user_cooldown = 30
)]
async fn dm(
    ctx: Context<'_>,
    #[description = "Recipient"] member: s::Member,
    #[description = "Message, max 2000 characters"] message: String,
) -> Result<(), Error> {
    if member.user.bot || message.trim().is_empty() || message.chars().count() > 2000 {
        return Err("Choose a human member and 1–2000 characters".into());
    }
    ctx.defer_ephemeral().await?;
    ctx.data().guild.member(ctx.http(), member.user.id).await?;
    let sent = member
        .user
        .direct_message(
            ctx.http(),
            s::CreateMessage::new()
                .embed(
                    s::CreateEmbed::new()
                        .title("ELVNX / MESSAGE")
                        .description(message)
                        .color(0x111111)
                        .footer(s::CreateEmbedFooter::new("11X • private collective")),
                )
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await?;
    if let Err(error) = sqlx::query("INSERT INTO outgoing_dms VALUES(?,?,?,?)")
        .bind(ctx.data().guild.to_string())
        .bind(member.user.id.to_string())
        .bind(sent.channel_id.to_string())
        .bind(sent.id.to_string())
        .execute(&ctx.data().db)
        .await
    {
        tracing::error!(%error, "Delivered DM but tracking failed");
        return reply(
            ctx,
            "DM delivered, but its deletion tracking could not be saved. Do not resend.",
        )
        .await;
    }
    audit(
        ctx,
        member.user.id.get(),
        "dm",
        "Delivered moderator message; message content not retained",
    )
    .await
}
/// Create or replace a sticky message that is reposted after channel activity.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn sticky(
    ctx: Context<'_>,
    #[description = "Channel"] channel: s::GuildChannel,
    #[description = "Sticky text"] message: String,
) -> Result<(), Error> {
    posting_channel(ctx, &channel).await?;
    ctx.data()
        .stickies
        .set(ctx.http(), channel.id, message)
        .await?;
    audit(ctx, 0, "sticky-set", &format!("Channel {}", channel.id)).await
}
/// Remove the sticky configuration and its current bot message.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn sticky_remove(
    ctx: Context<'_>,
    #[description = "Channel"] channel: s::GuildChannel,
) -> Result<(), Error> {
    posting_channel(ctx, &channel).await?;
    ctx.data().stickies.remove(ctx.http(), channel.id).await?;
    audit(ctx, 0, "sticky-remove", &format!("Channel {}", channel.id)).await
}
/// List the public AVRXT.DEV website destinations.
#[poise::command(slash_command, guild_only, rename = "avrxt-dev")]
async fn avrxt_dev(ctx: Context<'_>) -> Result<(), Error> {
    reply(ctx,"**AVRXT.DEV**\nHome https://www.avrxt.dev\nAbout https://www.avrxt.dev/about\nProfile https://www.avrxt.dev/me\nDocs https://www.avrxt.dev/docs\nTools https://www.avrxt.dev/tools\nTerminal https://www.avrxt.dev/terminal\nStatus https://www.avrxt.dev/status\nContact https://www.avrxt.dev/contact\nSubscribe https://www.avrxt.dev/subscribe\nSignature https://www.avrxt.dev/signature\nUses https://www.avrxt.dev/uses\nMusic https://www.avrxt.dev/muzix/chart\nBrand assets https://www.avrxt.dev/assets\nAPI docs https://www.avrxt.dev/system/api/doc\nPrivacy https://www.avrxt.dev/legal/privacy\nTerms https://www.avrxt.dev/legal/terms\nSecurity https://www.avrxt.dev/legal/security\nRefund https://www.avrxt.dev/legal/refund\nOwnership https://www.avrxt.dev/annon/ownership\nData requests https://www.avrxt.dev/delete-request\nDiscord privacy https://www.avrxt.dev/dc/privacy\nDiscord terms https://www.avrxt.dev/dc/terms").await
}
/// Server owner only: shut down cleanly so the configured supervisor can restart the bot.
#[poise::command(
    slash_command,
    guild_only,
    rename = "bot-restart",
    default_member_permissions = "ADMINISTRATOR"
)]
async fn bot_restart(
    ctx: Context<'_>,
    #[description = "Confirm restart"] confirm: bool,
) -> Result<(), Error> {
    let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
    if guild.owner_id != ctx.author().id {
        return reply(ctx, "Only the Discord server owner can restart this bot.").await;
    }
    if !confirm {
        return reply(ctx, "Canceled.").await;
    }
    if std::env::var("BOT_SUPERVISED").as_deref() != Ok("true") {
        return reply(
            ctx,
            "Start with the supervisor first; a standalone process cannot restart itself safely.",
        )
        .await;
    }
    audit(
        ctx,
        ctx.author().id.get(),
        "bot-restart",
        "Owner requested a supervised restart",
    )
    .await?;
    ctx.data().db.close().await;
    std::process::exit(75)
}
