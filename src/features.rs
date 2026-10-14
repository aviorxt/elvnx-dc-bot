use crate::{Context, Data, Error, cards, reply, security, serenity as s, store};
use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

pub fn all() -> Vec<poise::Command<Data, Error>> {
    vec![
        settings_panel(),
        role_panel(),
        poll(),
        poll_close(),
        poll_result(),
        remind(),
        reminders(),
        reminder_cancel(),
        report(),
        report_setup(),
        config_backup(),
        config_restore(),
        config_import(),
        birthday(),
        birthday_remove(),
        birthday_setup(),
    ]
}
fn now() -> i64 {
    s::Timestamp::now().unix_timestamp()
}
fn ist(time: i64) -> Result<chrono::DateTime<chrono::Utc>, Error> {
    chrono::DateTime::from_timestamp(time + 19_800, 0).ok_or_else(|| "Invalid date".into())
}
fn due(value: &str, time: i64) -> Result<i64, Error> {
    let value = value.trim();
    let result = if let Some(unit) = value
        .chars()
        .last()
        .filter(|c| matches!(c, 'm' | 'h' | 'd'))
    {
        let amount = value[..value.len() - 1].parse::<i64>()?;
        let scale = match unit {
            'm' => 60,
            'h' => 3600,
            _ => 86400,
        };
        time.checked_add(amount.checked_mul(scale).ok_or("Time too large")?)
            .ok_or("Time too large")?
    } else {
        NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M")?
            .and_utc()
            .timestamp()
            - 19_800
    };
    if result <= time || result > time + 365 * 86400 {
        return Err(
            "Choose a future time within 365 days: 10m, 2h, 1d, or YYYY-MM-DD HH:MM IST".into(),
        );
    }
    Ok(result)
}
fn bounded(value: &str, max: usize) -> Result<&str, Error> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max {
        return Err(format!("Text must contain 1–{max} characters").into());
    }
    Ok(value)
}
fn positive_id(raw: &str) -> Result<u64, Error> {
    let value = raw.trim().parse::<u64>()?;
    if value == 0 {
        return Err("Discord IDs must be positive".into());
    }
    Ok(value)
}
fn private_channel(channel: &s::GuildChannel, guild: s::GuildId) -> bool {
    channel.permission_overwrites.iter().any(|o| {
        o.kind == s::PermissionOverwriteType::Role(s::RoleId::new(guild.get()))
            && o.deny.contains(s::Permissions::VIEW_CHANNEL)
    })
}
fn embed(title: &str, text: impl Into<String>) -> s::CreateEmbed {
    s::CreateEmbed::new()
        .title(format!("ELVNX / {title}"))
        .description(text)
        .color(0x111111)
}
async fn member_permissions(
    ctx: &s::Context,
    data: &Data,
    user: s::UserId,
    required: s::Permissions,
) -> Result<(s::PartialGuild, s::Member), Error> {
    let guild = data.guild.to_partial_guild(&ctx.http).await?;
    let member = data.guild.member(&ctx.http, user).await?;
    if member.user.bot || !guild.member_permissions(&member).contains(required) {
        return Err("You no longer have the required server permission".into());
    }
    Ok((guild, member))
}
async fn destination(
    ctx: &s::Context,
    guild: s::GuildId,
    user: s::UserId,
    id: s::ChannelId,
    voice: bool,
) -> Result<s::GuildChannel, Error> {
    let channel = id
        .to_channel(&ctx.http)
        .await?
        .guild()
        .ok_or("Server channel required")?;
    if channel.guild_id != guild
        || channel.kind
            != if voice {
                s::ChannelType::Voice
            } else {
                s::ChannelType::Text
            }
    {
        return Err("Choose the correct channel type in this server".into());
    }
    let partial = guild.to_partial_guild(&ctx.http).await?;
    let actor = guild.member(&ctx.http, user).await?;
    let bot_id = ctx.cache.current_user().id;
    let bot = guild.member(&ctx.http, bot_id).await?;
    let actor_need = if voice {
        s::Permissions::VIEW_CHANNEL | s::Permissions::CONNECT
    } else {
        s::Permissions::VIEW_CHANNEL | s::Permissions::SEND_MESSAGES
    };
    let bot_need = if voice {
        actor_need | s::Permissions::MOVE_MEMBERS
    } else {
        actor_need | s::Permissions::EMBED_LINKS | s::Permissions::ATTACH_FILES
    };
    if !partial
        .user_permissions_in(&channel, &actor)
        .contains(actor_need)
        || !partial
            .user_permissions_in(&channel, &bot)
            .contains(bot_need)
    {
        return Err(
            "You and the bot need destination access and the required channel permissions".into(),
        );
    }
    Ok(channel)
}
async fn private_reports(
    ctx: &s::Context,
    guild: s::GuildId,
    channel: &s::GuildChannel,
) -> Result<(), Error> {
    let partial = guild.to_partial_guild(&ctx.http).await?;
    let staff = s::Permissions::ADMINISTRATOR
        | s::Permissions::MANAGE_GUILD
        | s::Permissions::MODERATE_MEMBERS
        | s::Permissions::MANAGE_MESSAGES;
    let bot = ctx.cache.current_user().id;
    if !channel.permission_overwrites.iter().any(|o| {
        o.kind == s::PermissionOverwriteType::Role(s::RoleId::new(guild.get()))
            && o.deny.contains(s::Permissions::VIEW_CHANNEL)
    }) {
        return Err("Reports require a private staff channel that explicitly denies View Channel to everyone".into());
    }
    for overwrite in &channel.permission_overwrites {
        if !overwrite.allow.contains(s::Permissions::VIEW_CHANNEL) {
            continue;
        }
        match overwrite.kind {
            s::PermissionOverwriteType::Role(id) => {
                if !partial.roles.get(&id).is_some_and(|role| {
                    role.permissions.intersects(staff)
                        || (role.managed && role.tags.bot_id == Some(bot))
                }) {
                    return Err("Report channel grants access to a non-staff role".into());
                }
            }
            s::PermissionOverwriteType::Member(id) if id != bot && id != partial.owner_id => {
                let member = guild.member(&ctx.http, id).await?;
                if !partial.member_permissions(&member).intersects(staff) {
                    return Err("Report channel grants access to a non-staff member".into());
                }
            }
            _ => {}
        }
    }
    Ok(())
}
async fn safe_claim_role(ctx: &s::Context, data: &Data, role: s::RoleId) -> Result<s::Role, Error> {
    if !data.settings.read().await.self_roles.contains(&role.get()) {
        return Err("This role is no longer approved for public self-service".into());
    }
    let guild = data.guild.to_partial_guild(&ctx.http).await?;
    let role = guild.roles.get(&role).ok_or("Role removed")?.clone();
    let bot_id = ctx.cache.current_user().id;
    let bot = data.guild.member(&ctx.http, bot_id).await?;
    let dangerous = s::Permissions::ADMINISTRATOR
        | s::Permissions::MOVE_MEMBERS
        | s::Permissions::MUTE_MEMBERS
        | s::Permissions::DEAFEN_MEMBERS
        | s::Permissions::MANAGE_GUILD
        | s::Permissions::MANAGE_ROLES
        | s::Permissions::MANAGE_CHANNELS
        | s::Permissions::MANAGE_WEBHOOKS
        | s::Permissions::BAN_MEMBERS
        | s::Permissions::KICK_MEMBERS
        | s::Permissions::MODERATE_MEMBERS
        | s::Permissions::MANAGE_MESSAGES
        | s::Permissions::MENTION_EVERYONE;
    if role.managed
        || role.id.get() == data.guild.get()
        || role.permissions.intersects(dangerous)
        || !guild
            .member_permissions(&bot)
            .contains(s::Permissions::MANAGE_ROLES)
        || !security::higher(
            security::highest(&guild, &bot),
            (i64::from(role.position), role.id.get()),
        )
    {
        return Err("Role is no longer safe or below the bot".into());
    }
    Ok(role)
}

/// Post buttons for up to five roles already approved with /selfrole_allow.
#[poise::command(
    slash_command,
    guild_only,
    rename = "role-panel",
    required_permissions = "MANAGE_GUILD | MANAGE_ROLES",
    default_member_permissions = "MANAGE_GUILD | MANAGE_ROLES",
    user_cooldown = 10
)]
async fn role_panel(
    ctx: Context<'_>,
    #[description = "Panel text channel"] channel: s::GuildChannel,
    #[description = "Approved public role"] role_1: s::Role,
    #[description = "Approved public role"] role_2: Option<s::Role>,
    #[description = "Approved public role"] role_3: Option<s::Role>,
    #[description = "Approved public role"] role_4: Option<s::Role>,
    #[description = "Approved public role"] role_5: Option<s::Role>,
) -> Result<(), Error> {
    crate::commands::posting_channel(ctx, &channel).await?;
    let mut roles = vec![role_1];
    for role in [role_2, role_3, role_4, role_5].into_iter().flatten() {
        if !roles.iter().any(|r| r.id == role.id) {
            roles.push(role);
        }
    }
    for role in &roles {
        security::safe_role(ctx, role).await?;
        safe_claim_role(ctx.serenity_context(), ctx.data(), role.id).await?;
    }
    let ids = roles.iter().map(|r| r.id.get()).collect::<Vec<_>>();
    let id = sqlx::query("INSERT INTO role_panels(guild,channel,roles) VALUES(?,?,?)")
        .bind(ctx.data().guild.to_string())
        .bind(channel.id.to_string())
        .bind(serde_json::to_string(&ids)?)
        .execute(&ctx.data().db)
        .await?
        .last_insert_rowid();
    let buttons = roles
        .iter()
        .map(|role| {
            s::CreateButton::new(format!("elvnx:plus:role:{id}:{}", role.id))
                .label(role.name.chars().take(70).collect::<String>())
                .style(s::ButtonStyle::Secondary)
        })
        .collect::<Vec<_>>();
    let message=channel.id.send_message(ctx.http(),s::CreateMessage::new().embed(embed("CHOOSE YOUR ROLES","Choose your public interests. Click again to remove a role. Staff-only access cannot be claimed here."))
        .components(vec![s::CreateActionRow::Buttons(buttons)]).allowed_mentions(s::CreateAllowedMentions::new())).await?;
    sqlx::query("UPDATE role_panels SET message=? WHERE id=? AND guild=?")
        .bind(message.id.to_string())
        .bind(id)
        .bind(ctx.data().guild.to_string())
        .execute(&ctx.data().db)
        .await?;
    reply(ctx, format!("Role panel posted: {}", message.link())).await
}

const FEATURES: &[(&str, &str)] = &[
    ("automod", "AutoMod"),
    ("invites", "Invite filter"),
    ("activity", "Activity tracking"),
    ("welcome", "Welcome images/channel"),
    ("log", "Staff logs"),
    ("afk", "AFK moves"),
    ("react", "Role mention reactions"),
    ("legacy", "Appukuttan announcements"),
    ("insight", "Daily insight"),
    ("quotes", "Daily quotes"),
    ("birthday", "Birthday announcements"),
    ("reports", "Private reports"),
    ("live", "Live status boards"),
];
fn settings_embed(cfg: &store::Settings) -> s::CreateEmbed {
    let channel = |id: Option<u64>| {
        id.map(|id| format!("<#{id}>"))
            .unwrap_or_else(|| "Off / unconfigured".into())
    };
    embed("CONTROL ROOM","Staff dashboard · choose a feature below. Channel features can be configured or turned off with the buttons.")
        .field("AutoMod / invites",format!("{} / {}",cfg.automod,cfg.block_invites),true)
        .field("Activity",if cfg.activity_tracking{"Enabled"}else{"Disabled"},true)
        .field("Welcome / log",format!("{} / {}",channel(cfg.welcome_channel),channel(cfg.log_channel)),false)
        .field("AFK / reactions",format!("{} / {}",channel(cfg.afk_move_channel),channel(cfg.always_react_channel)),false)
        .field("Daily insight / quotes",format!("{} / {}",channel(cfg.daily_insight_channel),channel(cfg.daily_quote_channel)),false)
        .field("Birthdays / reports",format!("{} / {}",channel(cfg.birthday_channel),channel(cfg.report_channel)),false)
        .field("Appukuttan",channel(cfg.appukuttan_legacy_channel),false)
        .footer(s::CreateEmbedFooter::new("Only the original staff member can use this panel; permissions are rechecked"))
}
fn settings_components(user: s::UserId) -> Vec<s::CreateActionRow> {
    vec![
        s::CreateActionRow::SelectMenu(
            s::CreateSelectMenu::new(
                format!("elvnx:plus:settings:{user}"),
                s::CreateSelectMenuKind::String {
                    options: FEATURES
                        .iter()
                        .map(|(key, title)| s::CreateSelectMenuOption::new(*title, *key))
                        .collect(),
                },
            )
            .placeholder("Choose a setting"),
        ),
        s::CreateActionRow::Buttons(vec![
            s::CreateButton::new(format!("elvnx:plus:refresh:{user}"))
                .label("Refresh panel")
                .style(s::ButtonStyle::Secondary),
        ]),
    ]
}
/// Interactive staff dashboard for moderation and community settings.
#[poise::command(
    slash_command,
    guild_only,
    rename = "settings-panel",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn settings_panel(ctx: Context<'_>) -> Result<(), Error> {
    let cfg = ctx.data().settings.read().await.clone();
    ctx.send(
        poise::CreateReply::default()
            .embed(settings_embed(&cfg))
            .components(settings_components(ctx.author().id))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}
async fn setting(
    ctx: &s::Context,
    data: &Data,
    user: s::UserId,
    key: &str,
    channel: Option<s::ChannelId>,
    off: bool,
) -> Result<(), Error> {
    member_permissions(ctx, data, user, s::Permissions::MANAGE_GUILD).await?;
    if !FEATURES.iter().any(|(k, _)| *k == key) {
        return Err("Unknown setting".into());
    }
    if key == "live" {
        member_permissions(ctx, data, user, s::Permissions::MANAGE_CHANNELS).await?;
        return data.live_status.set_enabled(!off).await;
    }
    let selected = if let Some(id) = channel {
        Some(destination(ctx, data.guild, user, id, key == "afk").await?)
    } else {
        None
    };
    if key == "reports"
        && let Some(channel) = &selected
    {
        private_reports(ctx, data.guild, channel).await?;
    }
    if key == "log"
        && selected
            .as_ref()
            .is_some_and(|channel| !private_channel(channel, data.guild))
    {
        return Err("Logs require a private channel that denies View Channel to everyone".into());
    }
    let _guard = data.configuration_lock.lock().await;
    let mut cfg = data.settings.read().await.clone();
    let id = selected.map(|channel| channel.id.get());
    match key {
        "automod" => {
            if !cfg.automod && std::env::var("ENABLE_MESSAGE_CONTENT").as_deref() != Ok("true") {
                return Err("Enable Message Content Intent first".into());
            }
            cfg.automod = !cfg.automod;
        }
        "invites" => cfg.block_invites = !cfg.block_invites,
        "activity" => cfg.activity_tracking = !cfg.activity_tracking,
        "welcome" => {
            cfg.welcome_channel = id;
            cfg.welcome_images = id.is_some();
        }
        "log" => cfg.log_channel = id,
        "afk" => cfg.afk_move_channel = id,
        "react" => {
            if id.is_some() && std::env::var("ENABLE_MESSAGE_CONTENT").as_deref() != Ok("true") {
                return Err("Enable Message Content Intent first".into());
            }
            cfg.always_react_channel = id;
        }
        "legacy" => cfg.appukuttan_legacy_channel = id,
        "insight" => {
            if id.is_some() && !cfg.activity_tracking {
                return Err("Enable activity tracking first".into());
            }
            cfg.daily_insight_channel = id;
        }
        "quotes" => cfg.daily_quote_channel = id,
        "birthday" => cfg.birthday_channel = id,
        "reports" => cfg.report_channel = id,
        _ => return Err("Unknown setting".into()),
    }
    store::save(&data.db, data.guild.get(), &cfg).await?;
    *data.settings.write().await = cfg.clone();
    if key == "afk" {
        data.afk.configure(cfg.afk_move_channel).await;
    }
    if key == "activity" && !cfg.activity_tracking {
        data.voices.lock().await.clear();
    }
    Ok(())
}
fn poll_components(id: i64, options: &[String], closed: bool) -> Vec<s::CreateActionRow> {
    vec![s::CreateActionRow::Buttons(
        options
            .iter()
            .enumerate()
            .map(|(i, text)| {
                s::CreateButton::new(format!("elvnx:plus:vote:{id}:{i}"))
                    .label(text.chars().take(70).collect::<String>())
                    .style(s::ButtonStyle::Secondary)
                    .disabled(closed)
            })
            .collect(),
    )]
}
async fn poll_view(
    db: &SqlitePool,
    id: i64,
) -> Result<(s::CreateEmbed, Vec<s::CreateActionRow>, Option<Vec<u8>>), Error> {
    let row = sqlx::query("SELECT * FROM polls WHERE id=?")
        .bind(id)
        .fetch_one(db)
        .await?;
    let options: Vec<String> = serde_json::from_str(row.get("options"))?;
    let closed = row.get::<String, _>("state") != "open" || row.get::<i64, _>("due") <= now();
    let counts =
        sqlx::query("SELECT choice,COUNT(*) total FROM poll_votes WHERE poll=? GROUP BY choice")
            .bind(id)
            .fetch_all(db)
            .await?;
    let mut total = 0;
    let mut stats = Vec::new();
    let mut text = String::new();
    for (i, option) in options.iter().enumerate() {
        let votes = counts
            .iter()
            .find(|r| r.get::<i64, _>("choice") == i as i64)
            .map(|r| r.get::<i64, _>("total"))
            .unwrap_or(0);
        total += votes;
        text.push_str(&format!("**{}** · {votes} votes\n", option));
        stats.push(("OPTION", format!("{} / {votes}", option)));
    }
    let mut view = embed(
        if closed { "POLL RESULTS" } else { "LIVE POLL" },
        format!("**{}**\n\n{text}", row.get::<String, _>("question")),
    )
    .field("Votes", total.to_string(), true)
    .field(
        if closed { "Closed" } else { "Closes" },
        format!("<t:{}:F>", row.get::<i64, _>("due")),
        true,
    )
    .footer(s::CreateEmbedFooter::new(format!(
        "Poll #{id} · one vote per member · you can change your vote before closing"
    )));
    let image = if closed {
        stats.push(("TOTAL VOTES", total.to_string()));
        view = view.image("attachment://poll-results.png");
        Some(cards::render(
            "POLL RESULTS",
            row.get("question"),
            "One member. One voice.",
            &stats,
            None,
        )?)
    } else {
        None
    };
    Ok((view, poll_components(id, &options, closed), image))
}
/// Create a persistent button poll with two to five choices and a closing time.
#[poise::command(slash_command, guild_only, user_cooldown = 30)]
async fn poll(
    ctx: Context<'_>,
    #[description = "Poll question"] question: String,
    #[description = "2–5 choices separated by |"] options: String,
    #[description = "10m, 2h, 1d, or YYYY-MM-DD HH:MM IST; default 1h"] closes: Option<String>,
    #[description = "Destination; default current text channel"] channel: Option<s::GuildChannel>,
) -> Result<(), Error> {
    let channel = channel.map(|c| c.id).unwrap_or(ctx.channel_id());
    destination(
        ctx.serenity_context(),
        ctx.data().guild,
        ctx.author().id,
        channel,
        false,
    )
    .await?;
    let question = bounded(&question, 250)?;
    let options = options
        .split('|')
        .map(|option| bounded(option, 70).map(str::to_string))
        .collect::<Result<Vec<_>, _>>()?;
    if !(2..=5).contains(&options.len())
        || options
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != options.len()
    {
        return Err("Use 2–5 different choices separated by |".into());
    }
    let time = due(closes.as_deref().unwrap_or("1h"), now())?;
    let inserted=sqlx::query("INSERT INTO polls(guild,owner,channel,question,options,due) SELECT ?,?,?,?,?,? WHERE (SELECT COUNT(*) FROM polls WHERE guild=? AND owner=? AND state='open')<2 AND (SELECT COUNT(*) FROM polls WHERE guild=? AND state='open')<20")
        .bind(ctx.data().guild.to_string()).bind(ctx.author().id.to_string()).bind(channel.to_string()).bind(question).bind(serde_json::to_string(&options)?).bind(time).bind(ctx.data().guild.to_string()).bind(ctx.author().id.to_string()).bind(ctx.data().guild.to_string()).execute(&ctx.data().db).await?;
    if inserted.rows_affected() == 0 {
        return reply(
            ctx,
            "Limit reached: 2 active polls per member or 20 for the server.",
        )
        .await;
    }
    let id = inserted.last_insert_rowid();
    let (view, components, _) = poll_view(&ctx.data().db, id).await?;
    let sent = channel
        .send_message(
            ctx.http(),
            s::CreateMessage::new()
                .embed(view)
                .components(components)
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await;
    let message = match sent {
        Ok(message) => message,
        Err(error) => {
            sqlx::query("UPDATE polls SET state='failed',rendered=1 WHERE id=?")
                .bind(id)
                .execute(&ctx.data().db)
                .await?;
            return Err(error.into());
        }
    };
    sqlx::query("UPDATE polls SET message=? WHERE id=?")
        .bind(message.id.to_string())
        .bind(id)
        .execute(&ctx.data().db)
        .await?;
    reply(ctx, format!("Poll #{id} posted: {}", message.link())).await
}
async fn accessible_poll(ctx: Context<'_>, id: i64) -> Result<sqlx::sqlite::SqliteRow, Error> {
    let row = sqlx::query("SELECT * FROM polls WHERE id=? AND guild=?")
        .bind(id)
        .bind(ctx.data().guild.to_string())
        .fetch_optional(&ctx.data().db)
        .await?
        .ok_or("Poll not found in this server")?;
    let channel = s::ChannelId::new(row.get::<String, _>("channel").parse()?);
    let ch = channel
        .to_channel(ctx.http())
        .await?
        .guild()
        .ok_or("Server channel required")?;
    let partial = ctx.data().guild.to_partial_guild(ctx.http()).await?;
    let member = ctx.data().guild.member(ctx.http(), ctx.author().id).await?;
    if !partial
        .user_permissions_in(&ch, &member)
        .contains(s::Permissions::VIEW_CHANNEL)
    {
        return Err("You cannot view this poll's channel".into());
    }
    Ok(row)
}
/// Close your poll, or a poll you can moderate.
#[poise::command(slash_command, guild_only, rename = "poll-close")]
async fn poll_close(ctx: Context<'_>, #[description = "Poll ID"] id: i64) -> Result<(), Error> {
    let row = accessible_poll(ctx, id).await?;
    if row.get::<String, _>("owner") != ctx.author().id.to_string() {
        member_permissions(
            ctx.serenity_context(),
            ctx.data(),
            ctx.author().id,
            s::Permissions::MANAGE_MESSAGES,
        )
        .await?;
    }
    sqlx::query("UPDATE polls SET state='closed' WHERE id=? AND guild=? AND state='open'")
        .bind(id)
        .bind(ctx.data().guild.to_string())
        .execute(&ctx.data().db)
        .await?;
    refresh_poll(ctx.serenity_context(), &ctx.data().db, id).await?;
    reply(ctx, "Poll closed; votes preserved and result card updated.").await
}
/// View a poll's current tally or final results card.
#[poise::command(slash_command, guild_only, rename = "poll-result")]
async fn poll_result(ctx: Context<'_>, #[description = "Poll ID"] id: i64) -> Result<(), Error> {
    accessible_poll(ctx, id).await?;
    let (view, _, image) = poll_view(&ctx.data().db, id).await?;
    let mut response = poise::CreateReply::default().embed(view).ephemeral(true);
    if let Some(image) = image {
        response = response.attachment(s::CreateAttachment::bytes(image, "poll-results.png"));
    }
    ctx.send(response).await?;
    Ok(())
}
async fn refresh_poll(ctx: &s::Context, db: &SqlitePool, id: i64) -> Result<(), Error> {
    static DISPLAY_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    let _guard = DISPLAY_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let row = sqlx::query("SELECT channel,message,state,due FROM polls WHERE id=?")
        .bind(id)
        .fetch_one(db)
        .await?;
    let Some(raw) = row.get::<Option<String>, _>("message") else {
        return Ok(());
    };
    let channel = s::ChannelId::new(row.get::<String, _>("channel").parse()?);
    let (view, components, image) = poll_view(db, id).await?;
    let mut edit = s::EditMessage::new()
        .embed(view)
        .components(components)
        .allowed_mentions(s::CreateAllowedMentions::new());
    if let Some(image) = image {
        edit = edit.attachments(
            s::EditAttachments::new().add(s::CreateAttachment::bytes(image, "poll-results.png")),
        );
    }
    match channel
        .edit_message(&ctx.http, s::MessageId::new(raw.parse()?), edit)
        .await
    {
        Ok(_) => {
            if row.get::<String, _>("state") != "open" {
                sqlx::query("UPDATE polls SET rendered=1 WHERE id=?")
                    .bind(id)
                    .execute(db)
                    .await?;
            }
        }
        Err(s::Error::Http(error))
            if error
                .status_code()
                .is_some_and(|status| status.as_u16() == 404) =>
        {
            sqlx::query("UPDATE polls SET state='missing',rendered=1 WHERE id=?")
                .bind(id)
                .execute(db)
                .await?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
/// Schedule an opt-in DM reminder, or a staff-authorized channel reminder.
#[poise::command(slash_command, guild_only, user_cooldown = 10)]
async fn remind(
    ctx: Context<'_>,
    #[description = "Reminder text, max 1000 characters"] text: String,
    #[description = "10m, 2h, 1d, or YYYY-MM-DD HH:MM IST"] when: String,
    #[description = "Staff-only destination; omit for personal DM"] channel: Option<
        s::GuildChannel,
    >,
) -> Result<(), Error> {
    let text = bounded(&text, 1000)?;
    let time = due(&when, now())?;
    if let Some(channel) = &channel {
        member_permissions(
            ctx.serenity_context(),
            ctx.data(),
            ctx.author().id,
            s::Permissions::MANAGE_GUILD,
        )
        .await?;
        destination(
            ctx.serenity_context(),
            ctx.data().guild,
            ctx.author().id,
            channel.id,
            false,
        )
        .await?;
    }
    let created = now();
    let result=sqlx::query("INSERT INTO reminders(guild,user,channel,text,due,created) SELECT ?,?,?,?,?,? WHERE (SELECT COUNT(*) FROM reminders WHERE guild=? AND user=? AND state='pending')<10 AND (SELECT COUNT(*) FROM reminders WHERE guild=? AND state='pending')<200")
        .bind(ctx.data().guild.to_string()).bind(ctx.author().id.to_string()).bind(channel.map(|ch|ch.id.to_string())).bind(text).bind(time).bind(created).bind(ctx.data().guild.to_string()).bind(ctx.author().id.to_string()).bind(ctx.data().guild.to_string()).execute(&ctx.data().db).await?;
    if result.rows_affected() == 0 {
        return reply(
            ctx,
            "Reminder limit reached: 10 per member, 200 per server.",
        )
        .await;
    }
    reply(ctx,format!("Reminder #{} scheduled for <t:{time}:F> (<t:{time}:R>). Personal reminders require open bot DMs.",result.last_insert_rowid())).await
}
/// List your reminders, including failed delivery status.
#[poise::command(slash_command, guild_only)]
async fn reminders(ctx: Context<'_>) -> Result<(), Error> {
    let rows=sqlx::query("SELECT id,text,due,state FROM reminders WHERE guild=? AND user=? ORDER BY id DESC LIMIT 15").bind(ctx.data().guild.to_string()).bind(ctx.author().id.to_string()).fetch_all(&ctx.data().db).await?;
    let text = rows
        .iter()
        .map(|row| {
            format!(
                "#{} · {} · <t:{}:R>\n{}",
                row.get::<i64, _>("id"),
                row.get::<String, _>("state"),
                row.get::<i64, _>("due"),
                row.get::<String, _>("text")
                    .chars()
                    .take(120)
                    .collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    reply(
        ctx,
        if text.is_empty() {
            "No reminders saved.".into()
        } else {
            text
        },
    )
    .await
}
/// Cancel one of your pending reminders.
#[poise::command(slash_command, guild_only, rename = "reminder-cancel")]
async fn reminder_cancel(
    ctx: Context<'_>,
    #[description = "Reminder ID"] id: i64,
) -> Result<(), Error> {
    let changed=sqlx::query("UPDATE reminders SET state='cancelled' WHERE id=? AND guild=? AND user=? AND state='pending'").bind(id).bind(ctx.data().guild.to_string()).bind(ctx.author().id.to_string()).execute(&ctx.data().db).await?.rows_affected();
    reply(
        ctx,
        if changed > 0 {
            "Reminder cancelled."
        } else {
            "No matching pending reminder. It may already be delivering."
        },
    )
    .await
}
/// Opt in to an annual server birthday announcement; no year or age is collected.
#[poise::command(slash_command, guild_only, user_cooldown = 10)]
async fn birthday(
    ctx: Context<'_>,
    #[description = "Month, 1–12"] month: u32,
    #[description = "Day, 1–31"] day: u32,
) -> Result<(), Error> {
    if NaiveDate::from_ymd_opt(2000, month, day).is_none() {
        return Err("Invalid month/day".into());
    }
    sqlx::query("INSERT INTO birthdays VALUES(?,?,?,?) ON CONFLICT(guild,user) DO UPDATE SET month=excluded.month,day=excluded.day").bind(ctx.data().guild.to_string()).bind(ctx.author().id.to_string()).bind(month as i64).bind(day as i64).execute(&ctx.data().db).await?;
    reply(ctx,format!("Opt-in saved: {day:02}/{month:02}. Your day/month may be announced with your avatar in this server at 08:00 IST when birthday announcements are enabled. No year or age is saved. Feb 29 uses Feb 28 in non-leap years. Remove anytime with /birthday-remove.")).await
}
/// Remove your birthday opt-in and stop future announcements.
#[poise::command(slash_command, guild_only, rename = "birthday-remove")]
async fn birthday_remove(ctx: Context<'_>) -> Result<(), Error> {
    sqlx::query("DELETE FROM birthdays WHERE guild=? AND user=?")
        .bind(ctx.data().guild.to_string())
        .bind(ctx.author().id.to_string())
        .execute(&ctx.data().db)
        .await?;
    reply(
        ctx,
        "Birthday opt-in removed. Previously posted Discord messages remain.",
    )
    .await
}
/// Configure the opt-in birthday announcement channel.
#[poise::command(
    slash_command,
    guild_only,
    rename = "birthday-setup",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn birthday_setup(
    ctx: Context<'_>,
    #[description = "Announcement channel"] channel: Option<s::GuildChannel>,
    #[description = "Enable (default) or disable"] enabled: Option<bool>,
) -> Result<(), Error> {
    let channel = if enabled.unwrap_or(true) {
        Some(channel.ok_or("Select the birthday channel")?.id)
    } else {
        None
    };
    setting(
        ctx.serenity_context(),
        ctx.data(),
        ctx.author().id,
        "birthday",
        channel,
        channel.is_none(),
    )
    .await?;
    reply(ctx,"Birthday channel updated. Opted-in members are announced at 08:00 IST while the bot is online.").await
}
#[derive(poise::Modal)]
#[name = "ELVNX / Private Report"]
struct ReportForm {
    #[name = "What happened?"]
    #[paragraph]
    #[min_length = 10]
    #[max_length = 3000]
    details: String,
    #[name = "Reported member ID (optional)"]
    #[max_length = 20]
    member: Option<String>,
    #[name = "Discord message link (optional)"]
    #[max_length = 200]
    link: Option<String>,
}
/// Submit a confidential staff report through a private form; no automatic punishment.
#[poise::command(slash_command, guild_only, user_cooldown = 60)]
async fn report(ctx: Context<'_>) -> Result<(), Error> {
    use poise::Modal;
    let Some(channel) = ctx.data().settings.read().await.report_channel else {
        return reply(ctx,"Private reports are not configured. A staff member can use /report-setup with a private staff channel.").await;
    };
    let app = match ctx {
        poise::Context::Application(app) => app,
        _ => return Err("Use the slash command".into()),
    };
    let custom = format!("elvnx-report-{}", app.interaction.id);
    app.interaction
        .create_response(ctx.http(), ReportForm::create(None, custom.clone()))
        .await?;
    app.has_sent_initial_response
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let user = ctx.author().id;
    let guild = ctx.data().guild;
    let response = s::collector::ModalInteractionCollector::new(&ctx.serenity_context().shard)
        .filter(move |m| {
            m.data.custom_id == custom && m.user.id == user && m.guild_id == Some(guild)
        })
        .timeout(std::time::Duration::from_secs(600))
        .await;
    let Some(response) = response else {
        return Ok(());
    };
    response
        .create_response(
            ctx.http(),
            s::CreateInteractionResponse::Defer(
                s::CreateInteractionResponseMessage::new().ephemeral(true),
            ),
        )
        .await?;
    let outcome:Result<i64,Error>=async{
        if ctx.data().settings.read().await.report_channel!=Some(channel){return Err("Report destination changed; reopen /report".into());}
        let form=ReportForm::parse(response.data.clone()).map_err(|error|->Error{error.into()})?;
        let channel=destination(ctx.serenity_context(),guild,crate::bot_id(ctx),s::ChannelId::new(channel),false).await?;
        private_reports(ctx.serenity_context(),guild,&channel).await?;
        let reporter=guild.member(ctx.http(),user).await?;
        if reporter.user.bot{return Err("Human reports only".into());}
        let member=if let Some(raw)=form.member.filter(|raw|!raw.trim().is_empty()){
            let id=s::UserId::new(positive_id(&raw)?);guild.member(ctx.http(),id).await?;format!("<@{id}> (`{id}`)")
        }else{"Not specified".into()};
        if let Some(link)=&form.link && !link.trim().is_empty(){
            let url=reqwest::Url::parse(link.trim())?;
            if url.scheme()!="https"||url.host_str()!=Some("discord.com")||!url.username().is_empty()||url.password().is_some()||url.port().is_some(){return Err("Use a https://discord.com/channels/... message link".into());}
            let parts=url.path().split('/').filter(|p|!p.is_empty()).collect::<Vec<_>>();
            if parts.len()!=4||parts[0]!="channels"||parts[1]!=guild.to_string(){return Err("Message link must belong to this server".into());}
            let ch=s::ChannelId::new(positive_id(parts[2])?);let _message=positive_id(parts[3])?;
            let ch=ch.to_channel(ctx.http()).await?.guild().ok_or("Server channel required")?;
            let partial=guild.to_partial_guild(ctx.http()).await?;
            if !partial.user_permissions_in(&ch,&reporter).contains(s::Permissions::VIEW_CHANNEL){return Err("You cannot report a message from an inaccessible channel".into());}
        }
        let stamp=now();
        let inserted=sqlx::query("INSERT INTO reports(guild,user,created) SELECT ?,?,? WHERE (SELECT COUNT(*) FROM reports WHERE guild=? AND user=? AND created>?)<3")
            .bind(guild.to_string()).bind(user.to_string()).bind(stamp).bind(guild.to_string()).bind(user.to_string()).bind(stamp-86400).execute(&ctx.data().db).await?;
        if inserted.rows_affected()==0{return Err("Maximum three reports per 24 hours; contact staff directly for urgent issues".into());}
        let id=inserted.last_insert_rowid();
        let message=s::CreateMessage::new().embed(embed("CONFIDENTIAL REPORT",bounded(&form.details,3000)?)
            .field("Receipt",format!("#{id}"),true).field("Reporter",format!("<@{user}> (`{user}`)"),true).field("Reported member",member,false)
            .field("Message link",form.link.filter(|link|!link.trim().is_empty()).unwrap_or_else(||"Not provided".into()),false)
            .footer(s::CreateEmbedFooter::new("Staff review required · no automatic moderation action")).timestamp(s::Timestamp::now()))
            .allowed_mentions(s::CreateAllowedMentions::new());
        let delivered=channel.id.send_message(ctx.http(),message).await;
        let state=if delivered.is_ok(){"sent"}else{"failed"};
        if let Err(error)=sqlx::query("UPDATE reports SET state=? WHERE id=?").bind(state).bind(id).execute(&ctx.data().db).await{tracing::warn!(%error,"Could not save report delivery state");}
        delivered?;Ok(id)
    }.await;
    let text = match outcome {
        Ok(id) => format!(
            "Report #{id} delivered privately to staff. Thank you. Staff will review it; this does not automatically punish anyone."
        ),
        Err(error) => error.to_string(),
    };
    response
        .edit_response(
            ctx.http(),
            s::EditInteractionResponse::new()
                .embed(embed("REPORT RECEIPT", text))
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await?;
    Ok(())
}
/// Configure a strictly private staff destination for report forms.
#[poise::command(
    slash_command,
    guild_only,
    rename = "report-setup",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn report_setup(
    ctx: Context<'_>,
    #[description = "Private staff text channel"] channel: Option<s::GuildChannel>,
    #[description = "Enable (default) or disable"] enabled: Option<bool>,
) -> Result<(), Error> {
    let channel = if enabled.unwrap_or(true) {
        Some(channel.ok_or("Select a private staff channel")?.id)
    } else {
        None
    };
    setting(
        ctx.serenity_context(),
        ctx.data(),
        ctx.author().id,
        "reports",
        channel,
        channel.is_none(),
    )
    .await?;
    reply(
        ctx,
        "Private report destination updated. Reports are disabled when no destination is selected.",
    )
    .await
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Backup {
    version: u8,
    guild: u64,
    created: i64,
    settings: store::Settings,
}
async fn validate_backup(ctx: Context<'_>, backup: &Backup) -> Result<(), Error> {
    member_permissions(
        ctx.serenity_context(),
        ctx.data(),
        ctx.author().id,
        s::Permissions::MANAGE_GUILD | s::Permissions::MANAGE_ROLES,
    )
    .await?;
    if backup.version != 1 || backup.guild != ctx.data().guild.get() {
        return Err("Backup must use format 1 and belong to this server".into());
    }
    let cfg = &backup.settings;
    let current = ctx.data().settings.read().await.clone();
    if (cfg.antinuke && !current.antinuke)
        || (cfg.honeypot_channel.is_some() && cfg.honeypot_channel != current.honeypot_channel)
    {
        return Err("Enable antinuke or a new honeypot through its dedicated confirmation command before restoring these settings".into());
    }
    if cfg.blocked_words.len() > 50
        || cfg.blocked_words.iter().any(|w| w.len() > 60)
        || cfg.self_roles.len() > 100
        || cfg.welcome_message.chars().count() > 1500
        || cfg.activity_text.chars().count() > 128
        || [
            cfg.report_channel,
            cfg.birthday_channel,
            cfg.music_dj_role,
            cfg.appukuttan_legacy_channel,
            cfg.always_react_channel,
            cfg.afk_move_channel,
            cfg.counting_channel,
            cfg.join_to_create,
            cfg.voice_panel,
            cfg.daily_quote_channel,
            cfg.daily_insight_channel,
            cfg.verification_role,
            cfg.honeypot_channel,
            cfg.log_channel,
            cfg.welcome_channel,
            cfg.autorole,
            cfg.ticket_category,
            cfg.ticket_staff_role,
        ]
        .contains(&Some(0))
        || cfg.self_roles.contains(&0)
    {
        return Err("Backup exceeds settings limits".into());
    }
    if !["", "playing", "watching", "listening", "competing"].contains(&cfg.activity_kind.as_str())
        || !["", "active", "dnd", "sleep", "invisible"].contains(&cfg.online_status.as_str())
    {
        return Err("Backup contains invalid presence settings".into());
    }
    if (cfg.automod || cfg.counting_channel.is_some() || cfg.always_react_channel.is_some())
        && std::env::var("ENABLE_MESSAGE_CONTENT").as_deref() != Ok("true")
    {
        return Err("Message Content Intent is required by this backup".into());
    }
    for id in [
        cfg.appukuttan_legacy_channel,
        cfg.always_react_channel,
        cfg.counting_channel,
        cfg.voice_panel,
        cfg.daily_quote_channel,
        cfg.daily_insight_channel,
        cfg.birthday_channel,
        cfg.report_channel,
        cfg.log_channel,
        cfg.honeypot_channel,
        cfg.welcome_channel,
    ]
    .into_iter()
    .flatten()
    {
        destination(
            ctx.serenity_context(),
            ctx.data().guild,
            ctx.author().id,
            s::ChannelId::new(id),
            false,
        )
        .await?;
    }
    for id in [cfg.afk_move_channel, cfg.join_to_create]
        .into_iter()
        .flatten()
    {
        destination(
            ctx.serenity_context(),
            ctx.data().guild,
            ctx.author().id,
            s::ChannelId::new(id),
            true,
        )
        .await?;
    }
    let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
    if let Some(id) = cfg.ticket_category {
        let channel = s::ChannelId::new(id)
            .to_channel(ctx.http())
            .await?
            .guild()
            .ok_or("Invalid ticket category")?;
        if channel.guild_id != guild.id || channel.kind != s::ChannelType::Category {
            return Err("Invalid ticket category".into());
        }
    }
    if let Some(id) = cfg.report_channel {
        let channel = s::ChannelId::new(id)
            .to_channel(ctx.http())
            .await?
            .guild()
            .ok_or("Report channel missing")?;
        private_reports(ctx.serenity_context(), ctx.data().guild, &channel).await?;
    }
    if let Some(id) = cfg.log_channel {
        let channel = s::ChannelId::new(id)
            .to_channel(ctx.http())
            .await?
            .guild()
            .ok_or("Log channel missing")?;
        if !private_channel(&channel, ctx.data().guild) {
            return Err("Restored log channel must deny View Channel to everyone".into());
        }
    }
    for id in &cfg.self_roles {
        if !current.self_roles.contains(id) {
            return Err(
                "Reapprove restored public roles with /selfrole_allow before restoring the backup"
                    .into(),
            );
        }
        let role = guild
            .roles
            .get(&s::RoleId::new(*id))
            .ok_or("Restored role no longer exists")?;
        security::safe_role(ctx, role).await?;
    }
    for (id, approved) in [
        (cfg.verification_role, current.verification_role),
        (cfg.autorole, current.autorole),
    ] {
        if let Some(id) = id {
            if approved != Some(id) && !current.self_roles.contains(&id) {
                return Err(
                    "A restored automatic/verification role must already be explicitly approved"
                        .into(),
                );
            }
            let role = guild
                .roles
                .get(&s::RoleId::new(id))
                .ok_or("Restored role removed")?;
            security::safe_role(ctx, role).await?;
        }
    }
    for id in [cfg.ticket_staff_role, cfg.music_dj_role]
        .into_iter()
        .flatten()
    {
        if !guild
            .roles
            .get(&s::RoleId::new(id))
            .is_some_and(|role| !role.managed && role.id.get() != guild.id.get())
        {
            return Err("Staff/DJ role removed".into());
        }
    }
    Ok(())
}
fn differences(current: &store::Settings, next: &store::Settings) -> Result<String, Error> {
    let current = serde_json::to_value(current)?;
    let next = serde_json::to_value(next)?;
    let mut changes = Vec::new();
    for (key, value) in next.as_object().ok_or("Invalid settings")? {
        if current.get(key) != Some(value) {
            changes.push(format!(
                "**{key}**\n{} → {}",
                current.get(key).unwrap_or(&serde_json::Value::Null),
                value
            ));
        }
    }
    let text = if changes.is_empty() {
        "No settings changes.".into()
    } else {
        changes.join("\n\n")
    };
    if text.chars().count() > 3800 {
        return Ok(format!(
            "{}\n…download the backup to review all values.",
            text.chars().take(3600).collect::<String>()
        ));
    }
    Ok(text)
}
async fn review(ctx: Context<'_>, id: i64, backup: &Backup) -> Result<(), Error> {
    validate_backup(ctx, backup).await?;
    let current = ctx.data().settings.read().await.clone();
    sqlx::query("INSERT INTO restore_reviews VALUES(?,?,?,?,?) ON CONFLICT(guild,user,backup) DO UPDATE SET base=excluded.base,created=excluded.created")
        .bind(ctx.data().guild.to_string()).bind(ctx.author().id.to_string()).bind(id).bind(serde_json::to_string(&current)?).bind(now()).execute(&ctx.data().db).await?;
    let text = format!(
        "{}\n\nPreview only. To apply within 10 minutes, use `/config-restore id:{id} confirm:true`. No roles, channels, member records, secrets, or messages are recreated.",
        differences(&current, &backup.settings)?
    );
    ctx.send(
        poise::CreateReply::default()
            .embed(embed("RESTORE PREVIEW", text))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}
/// Export bot settings to a private JSON attachment; secrets and member data are excluded.
#[poise::command(
    slash_command,
    guild_only,
    rename = "config-backup",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD",
    user_cooldown = 10
)]
async fn config_backup(ctx: Context<'_>) -> Result<(), Error> {
    let backup = Backup {
        version: 1,
        guild: ctx.data().guild.get(),
        created: now(),
        settings: ctx.data().settings.read().await.clone(),
    };
    let json = serde_json::to_string_pretty(&backup)?;
    let id = sqlx::query("INSERT INTO config_backups(guild,actor,created,value) VALUES(?,?,?,?)")
        .bind(ctx.data().guild.to_string())
        .bind(ctx.author().id.to_string())
        .bind(now())
        .bind(&json)
        .execute(&ctx.data().db)
        .await?
        .last_insert_rowid();
    ctx.send(poise::CreateReply::default().embed(embed("SETTINGS BACKUP",format!("Backup #{id} saved for 30 days. Download the JSON. Preview restoration with `/config-restore id:{id}`. Excludes tokens, member reports, activity, birthday data, playlists, and message history.")))
        .attachment(s::CreateAttachment::bytes(json.into_bytes(),format!("elvnx-settings-{id}.json"))).ephemeral(true)).await?;
    Ok(())
}
/// Preview a settings backup; explicit confirmation applies the previously reviewed changes.
#[poise::command(
    slash_command,
    guild_only,
    rename = "config-restore",
    required_permissions = "MANAGE_GUILD | MANAGE_ROLES",
    default_member_permissions = "MANAGE_GUILD | MANAGE_ROLES"
)]
async fn config_restore(
    ctx: Context<'_>,
    #[description = "Stored backup ID"] id: i64,
    #[description = "Apply the previously reviewed backup; default preview"] confirm: Option<bool>,
) -> Result<(), Error> {
    let raw: Option<String> =
        sqlx::query_scalar("SELECT value FROM config_backups WHERE id=? AND guild=?")
            .bind(id)
            .bind(ctx.data().guild.to_string())
            .fetch_optional(&ctx.data().db)
            .await?;
    let backup: Backup = serde_json::from_str(&raw.ok_or("Backup not found in this server")?)?;
    if !confirm.unwrap_or(false) {
        return review(ctx, id, &backup).await;
    }
    validate_backup(ctx, &backup).await?;
    let _guard = ctx.data().configuration_lock.lock().await;
    let current = ctx.data().settings.read().await.clone();
    let reviewed: Option<String> = sqlx::query_scalar(
        "SELECT base FROM restore_reviews WHERE guild=? AND user=? AND backup=? AND created>?",
    )
    .bind(ctx.data().guild.to_string())
    .bind(ctx.author().id.to_string())
    .bind(id)
    .bind(now() - 600)
    .fetch_optional(&ctx.data().db)
    .await?;
    if reviewed != Some(serde_json::to_string(&current)?) {
        return reply(ctx,"Preview first with /config-restore without confirm. Settings changed or the preview expired; review again.").await;
    }
    store::save(&ctx.data().db, ctx.data().guild.get(), &backup.settings).await?;
    *ctx.data().settings.write().await = backup.settings.clone();
    ctx.data()
        .afk
        .configure(backup.settings.afk_move_channel)
        .await;
    if !backup.settings.activity_tracking {
        ctx.data().voices.lock().await.clear();
    }
    crate::extras::presence(ctx.serenity_context(), &backup.settings);
    sqlx::query("DELETE FROM restore_reviews WHERE guild=? AND user=? AND backup=?")
        .bind(ctx.data().guild.to_string())
        .bind(ctx.author().id.to_string())
        .bind(id)
        .execute(&ctx.data().db)
        .await?;
    reply(ctx,"Reviewed bot settings restored. Live-status IDs, member data, reminders, polls, playlists, and server structure were preserved.").await
}
/// Import a settings JSON attachment and preview it; cannot fetch arbitrary URLs.
#[poise::command(
    slash_command,
    guild_only,
    rename = "config-import",
    required_permissions = "MANAGE_GUILD | MANAGE_ROLES",
    default_member_permissions = "MANAGE_GUILD | MANAGE_ROLES",
    user_cooldown = 10
)]
async fn config_import(
    ctx: Context<'_>,
    #[description = "Exported settings JSON, max 256 KiB"] file: s::Attachment,
) -> Result<(), Error> {
    let url = reqwest::Url::parse(&file.url)?;
    if file.size > 262_144
        || url.scheme() != "https"
        || !matches!(
            url.host_str(),
            Some("cdn.discordapp.com" | "media.discordapp.net")
        )
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err("Use a Discord-hosted JSON attachment no larger than 256 KiB".into());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let mut response = client.get(url).send().await?.error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 262_144 {
            return Err("Backup too large".into());
        }
        bytes.extend(chunk);
    }
    let backup: Backup = serde_json::from_slice(&bytes)?;
    validate_backup(ctx, &backup).await?;
    let id = sqlx::query("INSERT INTO config_backups(guild,actor,created,value) VALUES(?,?,?,?)")
        .bind(ctx.data().guild.to_string())
        .bind(ctx.author().id.to_string())
        .bind(now())
        .bind(serde_json::to_string(&backup)?)
        .execute(&ctx.data().db)
        .await?
        .last_insert_rowid();
    review(ctx, id, &backup).await
}
async fn component_text(
    ctx: &s::Context,
    button: &s::ComponentInteraction,
    text: impl Into<String>,
) -> Result<(), Error> {
    button
        .edit_response(
            &ctx.http,
            s::EditInteractionResponse::new()
                .embed(embed("COMMUNITY CONTROLS", text))
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await?;
    Ok(())
}
async fn click_rate(user: s::UserId, kind: &'static str) -> Result<(), Error> {
    use std::{
        collections::HashMap,
        sync::OnceLock,
        time::{Duration, Instant},
    };
    static CLICKS: OnceLock<tokio::sync::Mutex<HashMap<(u64, &'static str), Instant>>> =
        OnceLock::new();
    let mut clicks = CLICKS
        .get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
        .lock()
        .await;
    let time = Instant::now();
    clicks.retain(|_, last| time.duration_since(*last) < Duration::from_secs(3));
    if clicks
        .get(&(user.get(), kind))
        .is_some_and(|last| time.duration_since(*last) < Duration::from_secs(2))
    {
        return Err("Wait two seconds between clicks".into());
    }
    if clicks.len() >= 1000 {
        return Err("Controls are busy; try shortly".into());
    }
    clicks.insert((user.get(), kind), time);
    Ok(())
}
pub async fn gateway(ctx: &s::Context, event: &s::FullEvent, data: &Data) -> Result<bool, Error> {
    if let s::FullEvent::GuildMemberRemoval { guild_id, user, .. } = event
        && *guild_id == data.guild
    {
        sqlx::query("DELETE FROM birthdays WHERE guild=? AND user=?")
            .bind(data.guild.to_string())
            .bind(user.id.to_string())
            .execute(&data.db)
            .await?;
        sqlx::query(
            "UPDATE reminders SET state='left' WHERE guild=? AND user=? AND state='pending'",
        )
        .bind(data.guild.to_string())
        .bind(user.id.to_string())
        .execute(&data.db)
        .await?;
        sqlx::query("DELETE FROM playlists WHERE guild=? AND user=?")
            .bind(data.guild.to_string())
            .bind(user.id.to_string())
            .execute(&data.db)
            .await?;
        return Ok(false);
    }
    let s::FullEvent::InteractionCreate {
        interaction: s::Interaction::Component(button),
    } = event
    else {
        return Ok(false);
    };
    if button.guild_id != Some(data.guild) || !button.data.custom_id.starts_with("elvnx:plus:") {
        return Ok(false);
    }
    let parts = button.data.custom_id.split(':').collect::<Vec<_>>();
    let action = parts.get(2).copied().unwrap_or("");
    let dashboard = matches!(action, "settings" | "refresh" | "channel" | "off");
    button
        .create_response(
            &ctx.http,
            if dashboard {
                s::CreateInteractionResponse::Acknowledge
            } else {
                s::CreateInteractionResponse::Defer(
                    s::CreateInteractionResponseMessage::new().ephemeral(true),
                )
            },
        )
        .await?;
    let outcome:Result<(),Error>=async{
        if button.user.bot{return Err("Human controls only".into());}
        if dashboard {
            let actor=parts.last().ok_or("Invalid panel")?.parse::<u64>()?;
            if actor!=button.user.id.get(){return Err("This panel belongs to another staff member".into());}
            member_permissions(ctx,data,button.user.id,s::Permissions::MANAGE_GUILD).await?;
            click_rate(button.user.id,"settings").await?;
            if action=="refresh"{
                let cfg=data.settings.read().await.clone();
                button.edit_response(&ctx.http,s::EditInteractionResponse::new().embed(settings_embed(&cfg)).components(settings_components(button.user.id))).await?;return Ok(());
            }
            if action=="settings"{
                let s::ComponentInteractionDataKind::StringSelect{values}=&button.data.kind else{return Err("Select a setting".into());};
                let key=values.first().ok_or("Choose one setting")?;
                if !FEATURES.iter().any(|(k,_)|k==key){return Err("Unknown setting".into());}
                if matches!(key.as_str(),"automod"|"invites"|"activity"){
                    setting(ctx,data,button.user.id,key,None,false).await?;
                    let cfg=data.settings.read().await.clone();
                    button.edit_response(&ctx.http,s::EditInteractionResponse::new().embed(settings_embed(&cfg)).components(settings_components(button.user.id))).await?;
                }else{
                    let mut rows=vec![s::CreateActionRow::Buttons(vec![s::CreateButton::new(format!("elvnx:plus:off:{key}:{}",button.user.id)).label("Turn off / pause").style(s::ButtonStyle::Danger)])];
                    if key=="live"{
                        rows.push(s::CreateActionRow::Buttons(vec![s::CreateButton::new(format!("elvnx:plus:channel:live:{}",button.user.id)).label("Enable / create boards").style(s::ButtonStyle::Secondary)]));
                    }else{
                        rows.push(s::CreateActionRow::SelectMenu(s::CreateSelectMenu::new(format!("elvnx:plus:channel:{key}:{}",button.user.id),s::CreateSelectMenuKind::Channel{channel_types:Some(vec![if key=="afk"{s::ChannelType::Voice}else{s::ChannelType::Text}]),default_channels:None}).placeholder("Choose destination")));
                    }
                    button.edit_response(&ctx.http,s::EditInteractionResponse::new().embed(embed("SETTING DESTINATION",format!("Configure **{key}**. Reports require a private staff channel; daily insight requires activity tracking."))).components(rows)).await?;
                }
            }else{
                let key=parts.get(3).ok_or("Missing setting")?;
                let channel=match &button.data.kind{s::ComponentInteractionDataKind::ChannelSelect{values}=>values.first().copied(),_=>None};
                setting(ctx,data,button.user.id,key,channel,action=="off").await?;
                let cfg=data.settings.read().await.clone();
                button.edit_response(&ctx.http,s::EditInteractionResponse::new().embed(settings_embed(&cfg)).components(settings_components(button.user.id))).await?;
            }
            return Ok(());
        }
        if action=="role"{
            click_rate(button.user.id,"role").await?;
            let id=parts.get(3).ok_or("Missing panel")?.parse::<i64>()?;
            let role=s::RoleId::new(parts.get(4).ok_or("Missing role")?.parse()?);
            let row=sqlx::query("SELECT channel,message,roles FROM role_panels WHERE id=? AND guild=?").bind(id).bind(data.guild.to_string()).fetch_optional(&data.db).await?.ok_or("Panel not found")?;
            let roles:Vec<u64>=serde_json::from_str(row.get("roles"))?;
            if row.get::<String,_>("channel")!=button.channel_id.to_string()||row.get::<Option<String>,_>("message")!=Some(button.message.id.to_string())||!roles.contains(&role.get()){return Err("Panel no longer matches the saved configuration".into());}
            let _guard=data.configuration_lock.lock().await;
            let role=safe_claim_role(ctx,data,role).await?;
            let guild=data.guild.to_partial_guild(&ctx.http).await?;
            let member=data.guild.member(&ctx.http,button.user.id).await?;
            let channel=button.channel_id.to_channel(&ctx.http).await?.guild().ok_or("Server channel required")?;
            if !guild.user_permissions_in(&channel,&member).contains(s::Permissions::VIEW_CHANNEL){return Err("You cannot view this panel".into());}
            let remove=member.roles.contains(&role.id);
            if remove{member.remove_role(&ctx.http,role.id).await?;}else{member.add_role(&ctx.http,role.id).await?;}
            component_text(ctx,button,format!("{} role **{}**.",if remove{"Removed"}else{"Added"},role.name)).await?;
        }else if action=="vote"{
            click_rate(button.user.id,"vote").await?;
            let id=parts.get(3).ok_or("Missing poll")?.parse::<i64>()?;
            let choice=parts.get(4).ok_or("Missing choice")?.parse::<usize>()?;
            let row=sqlx::query("SELECT channel,message,options,due,state FROM polls WHERE id=? AND guild=?").bind(id).bind(data.guild.to_string()).fetch_optional(&data.db).await?.ok_or("Poll not found")?;
            let options:Vec<String>=serde_json::from_str(row.get("options"))?;
            if choice>=options.len()||row.get::<String,_>("channel")!=button.channel_id.to_string()||row.get::<Option<String>,_>("message")!=Some(button.message.id.to_string()){return Err("This poll button is outdated".into());}
            let guild=data.guild.to_partial_guild(&ctx.http).await?;let member=data.guild.member(&ctx.http,button.user.id).await?;
            let channel=button.channel_id.to_channel(&ctx.http).await?.guild().ok_or("Server channel required")?;
            if !guild.user_permissions_in(&channel,&member).contains(s::Permissions::VIEW_CHANNEL){return Err("You cannot view this poll".into());}
            let changed=sqlx::query("INSERT INTO poll_votes(poll,user,choice) SELECT ?,?,? WHERE EXISTS(SELECT 1 FROM polls WHERE id=? AND guild=? AND state='open' AND due>?) ON CONFLICT(poll,user) DO UPDATE SET choice=excluded.choice")
                .bind(id).bind(button.user.id.to_string()).bind(choice as i64).bind(id).bind(data.guild.to_string()).bind(now()).execute(&data.db).await?.rows_affected();
            if changed==0{return Err("Poll is closed; your vote was not changed".into());}
            component_text(ctx,button,format!("Vote saved: **{}**. One vote per member; you can change it until closing. Counts refresh every 30 seconds.",options[choice])).await?;
        }else{return Err("Unknown control".into());}
        Ok(())
    }.await;
    if let Err(error) = outcome {
        button
            .edit_response(
                &ctx.http,
                s::EditInteractionResponse::new()
                    .embed(embed("ACTION NOT COMPLETED", error.to_string()))
                    .components(Vec::new())
                    .allowed_mentions(s::CreateAllowedMentions::new()),
            )
            .await?;
    }
    Ok(true)
}
pub fn start(ctx: s::Context, db: SqlitePool, guild: s::GuildId) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut closed_cursor = 0;
        loop {
            interval.tick().await;
            if let Err(error) = scheduled_polls(&ctx, &db, guild, &mut closed_cursor).await {
                tracing::warn!(%error,"Poll scheduler failed");
            }
            if let Err(error) = scheduled_reminders(&ctx, &db, guild).await {
                tracing::warn!(%error,"Reminder scheduler failed");
            }
            if let Err(error) = scheduled_birthdays(&ctx, &db, guild).await {
                tracing::warn!(%error,"Birthday scheduler failed");
            }
            if let Err(error) = cleanup(&db).await {
                tracing::warn!(%error,"Community retention cleanup failed");
            }
        }
    });
}
async fn scheduled_polls(
    ctx: &s::Context,
    db: &SqlitePool,
    guild: s::GuildId,
    closed_cursor: &mut i64,
) -> Result<(), Error> {
    sqlx::query("UPDATE polls SET state='closed' WHERE guild=? AND state='open' AND due<=?")
        .bind(guild.to_string())
        .bind(now())
        .execute(db)
        .await?;
    let ids = poll_batch(db, guild.get(), closed_cursor).await?;
    for id in ids {
        if let Err(error) = refresh_poll(ctx, db, id).await {
            tracing::warn!(%error,%id,"Poll display failed");
        }
    }
    Ok(())
}
async fn poll_batch(db: &SqlitePool, guild: u64, cursor: &mut i64) -> Result<Vec<i64>, Error> {
    let mut closed:Vec<i64>=sqlx::query_scalar("SELECT id FROM polls WHERE guild=? AND message IS NOT NULL AND state='closed' AND rendered=0 AND id>? ORDER BY id LIMIT 20").bind(guild.to_string()).bind(*cursor).fetch_all(db).await?;
    *cursor = if closed.len() == 20 {
        *closed.last().ok_or("Missing poll cursor")?
    } else {
        0
    };
    let open:Vec<i64>=sqlx::query_scalar("SELECT id FROM polls WHERE guild=? AND message IS NOT NULL AND state='open' ORDER BY id LIMIT 20").bind(guild.to_string()).fetch_all(db).await?;
    closed.extend(open);
    Ok(closed)
}
async fn scheduled_reminders(
    ctx: &s::Context,
    db: &SqlitePool,
    guild: s::GuildId,
) -> Result<(), Error> {
    let rows=sqlx::query("SELECT id,user,channel,text,due FROM reminders WHERE guild=? AND state='pending' AND due<=? ORDER BY due,id LIMIT 20").bind(guild.to_string()).bind(now()).fetch_all(db).await?;
    for row in rows {
        let id = row.get::<i64, _>("id");
        let state = if row.get::<i64, _>("due") < now() - 86400 {
            "expired"
        } else {
            "claimed"
        };
        let claim = sqlx::query("UPDATE reminders SET state=? WHERE id=? AND state='pending'")
            .bind(state)
            .bind(id)
            .execute(db)
            .await?
            .rows_affected();
        if claim == 0 || state == "expired" {
            continue;
        }
        let user = s::UserId::new(row.get::<String, _>("user").parse()?);
        let outcome: Result<(), Error> = async {
            let member = guild.member(&ctx.http, user).await?;
            let message = s::CreateMessage::new()
                .embed(embed("REMINDER", row.get::<String, _>("text")).footer(
                    s::CreateEmbedFooter::new(format!(
                        "Reminder #{id} · scheduled by member {user}"
                    )),
                ))
                .allowed_mentions(s::CreateAllowedMentions::new());
            if let Some(channel) = row.get::<Option<String>, _>("channel") {
                let partial = guild.to_partial_guild(&ctx.http).await?;
                if !partial
                    .member_permissions(&member)
                    .contains(s::Permissions::MANAGE_GUILD)
                {
                    return Err("Scheduling member is no longer staff".into());
                }
                let channel =
                    destination(ctx, guild, user, s::ChannelId::new(channel.parse()?), false)
                        .await?;
                channel.id.send_message(&ctx.http, message).await?;
            } else {
                member.user.direct_message(&ctx.http, message).await?;
            }
            Ok(())
        }
        .await;
        sqlx::query("UPDATE reminders SET state=? WHERE id=?")
            .bind(if outcome.is_ok() { "sent" } else { "failed" })
            .bind(id)
            .execute(db)
            .await?;
        if let Err(error) = outcome {
            tracing::warn!(%error,%id,"Reminder delivery failed; no automatic resend");
        }
    }
    Ok(())
}
async fn scheduled_birthdays(
    ctx: &s::Context,
    db: &SqlitePool,
    guild: s::GuildId,
) -> Result<(), Error> {
    let date = ist(now())?;
    if date.hour() != 8 || date.minute() >= 5 {
        return Ok(());
    }
    let cfg = store::settings(db, guild.get()).await?;
    let Some(channel) = cfg.birthday_channel else {
        return Ok(());
    };
    let rows = sqlx::query("SELECT user,month,day FROM birthdays WHERE guild=? AND month=?")
        .bind(guild.to_string())
        .bind(date.month() as i64)
        .fetch_all(db)
        .await?;
    let leap = NaiveDate::from_ymd_opt(date.year(), 2, 29).is_some();
    for row in rows {
        let day = row.get::<i64, _>("day");
        if day != date.day() as i64
            && !(date.month() == 2 && date.day() == 28 && day == 29 && !leap)
        {
            continue;
        }
        let user = s::UserId::new(row.get::<String, _>("user").parse()?);
        let Ok(member) = guild.member(&ctx.http, user).await else {
            continue;
        };
        if member.user.bot {
            continue;
        }
        let claim =
            sqlx::query("INSERT OR IGNORE INTO birthday_claims(guild,user,year) VALUES(?,?,?)")
                .bind(guild.to_string())
                .bind(user.to_string())
                .bind(date.year() as i64)
                .execute(db)
                .await?
                .rows_affected();
        if claim == 0 {
            continue;
        }
        let outcome: Result<(), Error> = async {
            let opted_in:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM birthdays WHERE guild=? AND user=? AND month=? AND day=?)").bind(guild.to_string()).bind(user.to_string()).bind(date.month() as i64).bind(day).fetch_one(db).await?;
            if !opted_in {return Err("Birthday opt-in removed".into());}
            let bot = ctx.cache.current_user().id;
            let channel = destination(ctx, guild, bot, s::ChannelId::new(channel), false).await?;
            let avatar = cards::avatar(&member.user).await;
            let png = cards::render(
                "HAPPY BIRTHDAY",
                &member.user.name,
                "Another chapter. More good memories.",
                &[
                    ("BIRTHDAY", format!("{:02}/{:02}", day, date.month())),
                    ("COMMUNITY", "ELVNX / 11X".into()),
                ],
                avatar.as_deref(),
            )?;
            channel
                .id
                .send_message(
                    &ctx.http,
                    s::CreateMessage::new()
                        .embed(
                            embed(
                                "BIRTHDAY SIGNAL",
                                format!(
                                    "Happy birthday, <@{user}>! Thanks for being part of ELVNX."
                                ),
                            )
                            .image("attachment://birthday.png"),
                        )
                        .add_file(s::CreateAttachment::bytes(png, "birthday.png"))
                        .allowed_mentions(s::CreateAllowedMentions::new()),
                )
                .await?;
            Ok(())
        }
        .await;
        sqlx::query("UPDATE birthday_claims SET state=? WHERE guild=? AND user=? AND year=?")
            .bind(if outcome.is_ok() { "sent" } else { "failed" })
            .bind(guild.to_string())
            .bind(user.to_string())
            .bind(date.year() as i64)
            .execute(db)
            .await?;
        if let Err(error) = outcome {
            tracing::warn!(%error,%user,"Birthday delivery failed; no duplicate retry");
        }
    }
    Ok(())
}
async fn cleanup(db: &SqlitePool) -> Result<(), Error> {
    let oldest = now() - 30 * 86400;
    let mut tx = db.begin().await?;
    sqlx::query(
        "DELETE FROM poll_votes WHERE poll IN(SELECT id FROM polls WHERE state!='open' AND due<?)",
    )
    .bind(oldest)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM polls WHERE state!='open' AND due<?")
        .bind(oldest)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM reminders WHERE state!='pending' AND created<?")
        .bind(oldest)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM reports WHERE created<?")
        .bind(oldest)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM config_backups WHERE created<? OR id NOT IN(SELECT b.id FROM config_backups b WHERE b.guild=config_backups.guild ORDER BY b.id DESC LIMIT 20)").bind(oldest).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM restore_reviews WHERE created<?")
        .bind(now() - 600)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM birthday_claims WHERE year<?")
        .bind(ist(now())?.year() as i64 - 2)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn failed_closed_polls_cannot_starve_active_polls_or_later_results() {
        let db = store::connect("sqlite::memory:").await.unwrap();
        for id in 1..=26 {
            sqlx::query("INSERT INTO polls(id,guild,owner,channel,message,question,options,due,state) VALUES(?,'1','2','3','4','q','[]',0,?)").bind(id).bind(if id==26{"open"}else{"closed"}).execute(&db).await.unwrap();
        }
        let mut cursor = 0;
        let first = poll_batch(&db, 1, &mut cursor).await.unwrap();
        assert!(first.contains(&26));
        assert_eq!(cursor, 20);
        let second = poll_batch(&db, 1, &mut cursor).await.unwrap();
        assert!(second.contains(&25));
        assert!(second.contains(&26));
        assert_eq!(cursor, 0);
        let third = poll_batch(&db, 1, &mut cursor).await.unwrap();
        assert!(third.contains(&1));
        assert!(poll_batch(&db, 9, &mut cursor).await.unwrap().is_empty());
    }
    #[test]
    fn ist_times_require_future_and_reject_overflow_and_zero_ids() {
        let stamp = chrono::DateTime::parse_from_rfc3339("2026-10-14T00:00:00Z")
            .unwrap()
            .timestamp();
        assert_eq!(due("2026-10-14 08:00", stamp).unwrap(), stamp + 9000);
        assert_eq!(due("10m", stamp).unwrap(), stamp + 600);
        for input in [
            "0m",
            "-1h",
            "999999999999999999d",
            "2026-10-14 04:00",
            "366d",
        ] {
            assert!(due(input, stamp).is_err());
        }
        assert!(positive_id("0").is_err());
        assert!(positive_id("-1").is_err());
    }
    #[tokio::test]
    async fn poll_votes_are_unique_and_cannot_change_after_closing() {
        let db = store::connect("sqlite::memory:").await.unwrap();
        sqlx::query("INSERT INTO polls(guild,owner,channel,question,options,due) VALUES('1','2','3','q','[\"a\",\"b\"]',?)").bind(now()+1000).execute(&db).await.unwrap();
        let vote = "INSERT INTO poll_votes(poll,user,choice) SELECT 1,'4',? WHERE EXISTS(SELECT 1 FROM polls WHERE id=1 AND guild=? AND state='open' AND due>?) ON CONFLICT(poll,user) DO UPDATE SET choice=excluded.choice";
        for choice in [0, 1] {
            assert_eq!(
                sqlx::query(vote)
                    .bind(choice)
                    .bind("1")
                    .bind(now())
                    .execute(&db)
                    .await
                    .unwrap()
                    .rows_affected(),
                1
            );
        }
        assert_eq!(
            sqlx::query(vote)
                .bind(0)
                .bind("other-guild")
                .bind(now())
                .execute(&db)
                .await
                .unwrap()
                .rows_affected(),
            0
        );
        sqlx::query("UPDATE polls SET state='closed'")
            .execute(&db)
            .await
            .unwrap();
        assert_eq!(
            sqlx::query(vote)
                .bind(0)
                .bind("1")
                .bind(now())
                .execute(&db)
                .await
                .unwrap()
                .rows_affected(),
            0
        );
        let choices: Vec<i64> = sqlx::query_scalar("SELECT choice FROM poll_votes")
            .fetch_all(&db)
            .await
            .unwrap();
        assert_eq!(choices, vec![1]);
    }
    #[tokio::test]
    async fn jobs_claim_once_and_cleanup_preserves_future_jobs_and_optins() {
        let db = store::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "INSERT INTO reminders(guild,user,text,due,created) VALUES('1','2','test',?,?)",
        )
        .bind(now() + 60)
        .bind(now() - 40 * 86400)
        .execute(&db)
        .await
        .unwrap();
        cleanup(&db).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM reminders")
                .fetch_one(&db)
                .await
                .unwrap(),
            1
        );
        let claim = "UPDATE reminders SET state='claimed' WHERE id=1 AND state='pending'";
        let (one, two) = tokio::join!(
            sqlx::query(claim).execute(&db),
            sqlx::query(claim).execute(&db)
        );
        assert_eq!(
            one.unwrap().rows_affected() + two.unwrap().rows_affected(),
            1
        );
        for _ in 0..2 {
            sqlx::query(
                "INSERT OR IGNORE INTO birthday_claims(guild,user,year) VALUES('1','2',2026)",
            )
            .execute(&db)
            .await
            .unwrap();
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM birthday_claims")
                .fetch_one(&db)
                .await
                .unwrap(),
            1
        );
        sqlx::query("INSERT INTO birthdays VALUES('1','2',2,29)")
            .execute(&db)
            .await
            .unwrap();
        cleanup(&db).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM birthdays")
                .fetch_one(&db)
                .await
                .unwrap(),
            1
        );
    }
    #[tokio::test]
    async fn restore_reviews_and_playlists_are_bound_to_actor_and_guild() {
        let db = store::connect("sqlite::memory:").await.unwrap();
        sqlx::query("INSERT INTO restore_reviews VALUES('1','2',3,'review',?)")
            .bind(now())
            .execute(&db)
            .await
            .unwrap();
        let review =
            "SELECT base FROM restore_reviews WHERE guild=? AND user=? AND backup=? AND created>?";
        for (guild, user, id, time, expected) in [
            ("1", "2", 3, now() - 600, true),
            ("9", "2", 3, now() - 600, false),
            ("1", "9", 3, now() - 600, false),
            ("1", "2", 4, now() - 600, false),
            ("1", "2", 3, now() + 1, false),
        ] {
            assert_eq!(
                sqlx::query_scalar::<_, String>(review)
                    .bind(guild)
                    .bind(user)
                    .bind(id)
                    .bind(time)
                    .fetch_optional(&db)
                    .await
                    .unwrap()
                    .is_some(),
                expected
            );
        }
        sqlx::query("INSERT INTO playlists VALUES('1','2','private','[]')")
            .execute(&db)
            .await
            .unwrap();
        assert_eq!(
            sqlx::query("DELETE FROM playlists WHERE guild='1' AND user='9'")
                .execute(&db)
                .await
                .unwrap()
                .rows_affected(),
            0
        );
        assert_eq!(
            sqlx::query("DELETE FROM playlists WHERE guild='9' AND user='2'")
                .execute(&db)
                .await
                .unwrap()
                .rows_affected(),
            0
        );
    }
}
