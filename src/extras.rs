use crate::{Context, Data, Error, cards, log, reply, security, serenity as s, store};
use sqlx::{Row, SqlitePool};
use std::{collections::HashMap, sync::Arc, time::Instant};
use tokio::sync::Mutex;

pub fn all() -> Vec<poise::Command<Data, Error>> {
    vec![
        quote(),
        status(),
        bot_status(),
        quote_daily(),
        crate::daily::daily_insight(),
        verify(),
        uptime(),
        insight(),
        welcome_setup(),
        antinuke(),
        honeypot(),
        rolelist(),
        member_list(),
        bot_list(),
        activity(),
        automod_reload(),
    ]
}
const QUOTES: &[&str] = &[
    "Build something small. Make it useful. Make it better tomorrow.",
    "Consistency turns experiments into skills.",
    "A clear next step beats a perfect plan that never starts.",
    "Leave the system easier to understand than you found it.",
    "Care is visible in the details you choose to finish.",
    "Progress is a collection of honest attempts.",
    "Ask a better question before building a bigger answer.",
    "The best shortcut is understanding the problem.",
    "Make room for the work that matters.",
    "Learn, repair, repeat.",
];
pub fn quote_for(seed: u64) -> &'static str {
    QUOTES[(seed % QUOTES.len() as u64) as usize]
}
pub fn schedule_due(now: i64) -> Option<i64> {
    let day = now.div_euclid(86400);
    let elapsed = now - day * 86400 - 9000;
    if (0..300).contains(&elapsed) {
        Some(day)
    } else {
        None
    }
}
pub fn presence(ctx: &s::Context, cfg: &store::Settings) {
    let state = match cfg.online_status.as_str() {
        "dnd" => s::OnlineStatus::DoNotDisturb,
        "sleep" => s::OnlineStatus::Idle,
        "invisible" => s::OnlineStatus::Invisible,
        _ => s::OnlineStatus::Online,
    };
    let activity = if cfg.activity_text.is_empty() {
        None
    } else {
        Some(match cfg.activity_kind.as_str() {
            "watching" => s::ActivityData::watching(&cfg.activity_text),
            "listening" => s::ActivityData::listening(&cfg.activity_text),
            "competing" => s::ActivityData::competing(&cfg.activity_text),
            _ => s::ActivityData::playing(&cfg.activity_text),
        })
    };
    ctx.set_presence(activity, state);
}
async fn update(ctx: Context<'_>, f: impl FnOnce(&mut store::Settings)) -> Result<(), Error> {
    let _guard = ctx.data().configuration_lock.lock().await;
    let mut cfg = ctx.data().settings.read().await.clone();
    f(&mut cfg);
    store::save(&ctx.data().db, ctx.data().guild.get(), &cfg).await?;
    *ctx.data().settings.write().await = cfg;
    Ok(())
}

/// Get a random original ELVNX quote.
#[poise::command(slash_command, guild_only)]
async fn quote(ctx: Context<'_>) -> Result<(), Error> {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .subsec_nanos() as u64;
    reply(ctx, format!("“{}”\n— ELVNX", quote_for(seed))).await
}
#[derive(Clone, Copy, poise::ChoiceParameter)]
enum PresenceKind {
    Playing,
    Watching,
    Listening,
    Competing,
}
/// Set the bot's custom activity text (standard Discord bot presence).
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn status(
    ctx: Context<'_>,
    #[description = "Activity"] kind: PresenceKind,
    #[description = "Text, max 100 characters; off clears"] text: String,
) -> Result<(), Error> {
    if text.chars().count() > 100 {
        return Err(crate::errors::user("Status too long"));
    }
    let kind = match kind {
        PresenceKind::Playing => "playing",
        PresenceKind::Watching => "watching",
        PresenceKind::Listening => "listening",
        PresenceKind::Competing => "competing",
    };
    update(ctx, |c| {
        c.activity_kind = kind.into();
        c.activity_text = if text == "off" { String::new() } else { text };
    })
    .await?;
    presence(ctx.serenity_context(), &*ctx.data().settings.read().await);
    reply(ctx, "Activity saved.").await
}
#[derive(Clone, Copy, poise::ChoiceParameter)]
enum BotState {
    Active,
    Dnd,
    Sleep,
    Invisible,
}
/// Set online, do-not-disturb, idle, or invisible status.
#[poise::command(
    slash_command,
    guild_only,
    rename = "bot-status",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn bot_status(
    ctx: Context<'_>,
    #[description = "State"] state: BotState,
) -> Result<(), Error> {
    let state = match state {
        BotState::Active => "active",
        BotState::Dnd => "dnd",
        BotState::Sleep => "sleep",
        BotState::Invisible => "invisible",
    };
    update(ctx, |c| c.online_status = state.into()).await?;
    presence(ctx.serenity_context(), &*ctx.data().settings.read().await);
    reply(ctx, "Presence saved.").await
}
/// Enable or disable a daily original quote at 08:00 Asia/Kolkata (02:30 UTC).
#[poise::command(
    slash_command,
    guild_only,
    rename = "quote-daily",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn quote_daily(
    ctx: Context<'_>,
    #[description = "Enabled"] enabled: bool,
    #[description = "Destination when enabling"] channel: Option<s::GuildChannel>,
) -> Result<(), Error> {
    let id = if enabled {
        let ch = channel.ok_or_else(|| crate::errors::user("Choose a destination channel"))?;
        crate::commands::posting_channel(ctx, &ch).await?;
        Some(ch.id.get())
    } else {
        None
    };
    update(ctx, |c| c.daily_quote_channel = id).await?;
    reply(
        ctx,
        "Daily quotes updated. Scheduled for 08:00 IST while the bot is online.",
    )
    .await
}
/// Configure secure access verification, inspect requests, or approve/deny a flagged member.
#[allow(clippy::too_many_arguments)]
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD | MANAGE_ROLES",
    required_bot_permissions = "MANAGE_ROLES",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn verify(
    ctx: Context<'_>,
    #[description = "Setup, status, approve, or deny a flagged member"] action: Option<
        crate::verification::Action,
    >,
    #[description = "Panel channel for setup"] channel: Option<s::GuildChannel>,
    #[description = "Zero-permission whitelist role for setup"] role: Option<s::Role>,
    #[description = "Confirm eligible members may receive this role's channel access"]
    confirm_access: Option<bool>,
    #[description = "Account age in days below which staff review is required, 1–365; default 7"]
    min_account_days: Option<u16>,
    #[description = "Wait after joining, 0–1440 minutes; default 10"] join_wait_minutes: Option<
        u16,
    >,
    #[description = "Private staff review channel; defaults to the report destination"]
    review_channel: Option<s::GuildChannel>,
    #[description = "Flagged member for Approve/Deny"] member: Option<s::Member>,
    #[description = "Internal reason for a staff decision, max 400 characters"] reason: Option<
        String,
    >,
) -> Result<(), Error> {
    crate::verification::command(
        ctx,
        action.unwrap_or(crate::verification::Action::Setup),
        channel,
        role,
        confirm_access.unwrap_or(false),
        min_account_days,
        join_wait_minutes,
        review_channel,
        member,
        reason,
    )
    .await
}
/// Check uptime for the current running process.
#[poise::command(slash_command, guild_only)]
async fn uptime(ctx: Context<'_>) -> Result<(), Error> {
    let seconds = ctx.data().started.elapsed().as_secs();
    reply(
        ctx,
        format!(
            "{}d {}h {}m {}s",
            seconds / 86400,
            seconds % 86400 / 3600,
            seconds % 3600 / 60,
            seconds % 60
        ),
    )
    .await
}
/// Activity PNG card for yourself or another member, with server leaderboard.
#[poise::command(slash_command, guild_only)]
async fn insight(
    ctx: Context<'_>,
    #[description = "Member; omit for self"] member: Option<s::Member>,
) -> Result<(), Error> {
    if !ctx.data().settings.read().await.activity_tracking {
        return reply(
            ctx,
            "Activity tracking is disabled. An admin can enable /activity.",
        )
        .await;
    }
    let user = member
        .map(|m| m.user)
        .unwrap_or_else(|| ctx.author().clone());
    let row = sqlx::query("SELECT messages,voice FROM activity WHERE guild=? AND user=?")
        .bind(ctx.data().guild.to_string())
        .bind(user.id.to_string())
        .fetch_optional(&ctx.data().db)
        .await?;
    let (messages, voice) = row
        .map(|r| (r.get::<i64, _>("messages"), r.get::<i64, _>("voice")))
        .unwrap_or((0, 0));
    let session = ctx
        .data()
        .voices
        .lock()
        .await
        .get(&user.id.get())
        .map(|i| i.elapsed().as_secs() as i64)
        .unwrap_or(0);
    let leaders=sqlx::query("SELECT user,messages,voice FROM activity WHERE guild=? ORDER BY messages DESC,voice DESC LIMIT 3").bind(ctx.data().guild.to_string()).fetch_all(&ctx.data().db).await?;
    let mut stats = vec![
        ("MESSAGES", messages.to_string()),
        (
            "VOICE CONNECTED",
            format!(
                "{}h {}m",
                (voice + session) / 3600,
                (voice + session) % 3600 / 60
            ),
        ),
        ("TRACKING", "Since bot enabled".into()),
    ];
    for (i, r) in leaders.iter().enumerate() {
        let raw = r.get::<String, _>("user");
        let id = s::UserId::new(raw.parse()?);
        let name = ctx
            .cache()
            .user(id)
            .map(|u| u.name.clone())
            .unwrap_or_else(|| {
                format!(
                    "…{}",
                    raw.chars()
                        .rev()
                        .take(6)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>()
                )
            });
        stats.push((
            match i {
                0 => "#1 MESSAGE LEADER",
                1 => "#2 MESSAGE LEADER",
                _ => "#3 MESSAGE LEADER",
            },
            format!(
                "{} / {}",
                name.chars().take(10).collect::<String>(),
                r.get::<i64, _>("messages")
            ),
        ));
    }
    let avatar = cards::avatar(&user).await;
    let png = cards::render(
        "ACTIVITY INSIGHT",
        &user.name,
        "Message totals • connected voice time",
        &stats,
        avatar.as_deref(),
    )?;
    ctx.send(
        poise::CreateReply::default()
            .attachment(s::CreateAttachment::bytes(png, "insight.png"))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}
/// Configure welcome PNGs using each joining member's profile picture.
#[poise::command(
    slash_command,
    guild_only,
    rename = "welcome-setup",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn welcome_setup(
    ctx: Context<'_>,
    #[description = "Welcome channel"] channel: s::GuildChannel,
    #[description = "Show PNG welcome card"] images: bool,
    #[description = "Welcome text; {user} and {server} supported"] text: Option<String>,
) -> Result<(), Error> {
    crate::commands::posting_channel(ctx, &channel).await?;
    let text = text.unwrap_or_else(|| "Welcome, {user}. You are part of ELVNX.".into());
    if text.chars().count() > 1500 {
        return Err(crate::errors::user("Welcome text too long"));
    }
    update(ctx, |c| {
        c.welcome_channel = Some(channel.id.get());
        c.welcome_message = text;
        c.welcome_images = images;
    })
    .await?;
    let avatar = cards::avatar(ctx.author()).await;
    let png = cards::render(
        "WELCOME TO ELVNX",
        &ctx.author().name,
        "A private collective. Build. Explore. Connect.",
        &[],
        avatar.as_deref(),
    )?;
    ctx.send(
        poise::CreateReply::default()
            .content("Saved. Preview below. Enable member intent for join events.")
            .attachment(s::CreateAttachment::bytes(png, "welcome-preview.png"))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}
/// Toggle audit-log antinuke containment (4 destructive actions within 15s by default).
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    required_bot_permissions = "VIEW_AUDIT_LOG | MANAGE_ROLES",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn antinuke(
    ctx: Context<'_>,
    #[description = "Enabled"] enabled: bool,
    #[description="Confirm removing dangerous roles from a detected actor below the bot"] confirm:bool,
) -> Result<(), Error> {
    if enabled && !confirm {
        return reply(
            ctx,
            "Confirmation required: detected actors can lose dangerous roles below this bot.",
        )
        .await;
    }
    update(ctx, |c| c.antinuke = enabled).await?;
    reply(ctx,"Antinuke updated. Owner, this bot, and configured trusted IDs are exempt; above-bot actors generate alerts only.").await
}
/// Configure a clearly marked trap channel; posting there triggers a softban and DMs.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD | BAN_MEMBERS",
    required_bot_permissions = "BAN_MEMBERS",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn honeypot(
    ctx: Context<'_>,
    #[description = "Enabled"] enabled: bool,
    #[description = "Trap channel when enabling"] channel: Option<s::GuildChannel>,
    #[description = "Confirm ordinary users posting there will be softbanned"] confirm: bool,
) -> Result<(), Error> {
    crate::honeypot::configure(ctx, enabled, channel, confirm).await
}
/// List server roles (paginated in groups of 20).
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn rolelist(
    ctx: Context<'_>,
    #[description = "Page, starting at 1"] page: Option<u32>,
) -> Result<(), Error> {
    let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
    let mut roles: Vec<_> = guild.roles.values().collect();
    roles.sort_by_key(|r| std::cmp::Reverse(r.position));
    let page = page.unwrap_or(1).clamp(1, 100);
    let text = roles
        .into_iter()
        .skip((page as usize - 1) * 20)
        .take(20)
        .map(|r| format!("{} • {} • position {}", r.name, r.id, r.position))
        .collect::<Vec<_>>()
        .join("\n");
    reply(
        ctx,
        if text.is_empty() {
            "No roles on this page.".into()
        } else {
            text.chars().take(1900).collect::<String>()
        },
    )
    .await
}
async fn members(
    ctx: Context<'_>,
    after: Option<s::UserId>,
    bots: Option<bool>,
) -> Result<(), Error> {
    let members = ctx
        .data()
        .guild
        .members(ctx.http(), Some(25), after)
        .await?;
    let cursor = members.iter().map(|m| m.user.id).max();
    let text = members
        .into_iter()
        .filter(|m| bots.is_none_or(|b| m.user.bot == b))
        .take(25)
        .map(|m| {
            format!(
                "{} • {}{}",
                m.user.name,
                m.user.id,
                if m.user.bot { " [bot]" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    reply(
        ctx,
        format!(
            "{}\nPage scans up to 25 members. Next cursor: {}",
            text.chars().take(1700).collect::<String>(),
            cursor
                .map(|c| c.to_string())
                .unwrap_or_else(|| "end".into())
        ),
    )
    .await
}
/// List members, using the returned cursor to get the next page.
#[poise::command(
    slash_command,
    guild_only,
    rename = "member-list",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn member_list(
    ctx: Context<'_>,
    #[description = "Previous page's cursor"] after: Option<s::UserId>,
) -> Result<(), Error> {
    members(ctx, after, None).await
}
/// List bots within each paginated member scan.
#[poise::command(
    slash_command,
    guild_only,
    rename = "bot-list",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn bot_list(
    ctx: Context<'_>,
    #[description = "Previous page's cursor"] after: Option<s::UserId>,
) -> Result<(), Error> {
    members(ctx, after, Some(true)).await
}
/// Opt in/out of member message counters and connected voice time.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn activity(
    ctx: Context<'_>,
    #[description = "Track future activity"] enabled: bool,
) -> Result<(), Error> {
    update(ctx, |c| c.activity_tracking = enabled).await?;
    if !enabled {
        ctx.data().voices.lock().await.clear();
    }
    reply(ctx,"Tracking updated. Counts begin while enabled and connected; historical Discord activity is not imported.").await
}
/// Validate and reload the local automod.json rule file without restarting.
#[poise::command(
    slash_command,
    guild_only,
    rename = "automod-reload",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn automod_reload(ctx: Context<'_>) -> Result<(), Error> {
    let rules = crate::automod::Rules::load()?;
    *ctx.data().rules.write().await = rules;
    reply(ctx, "Validated automod.json loaded.").await
}

pub fn start_scheduler(
    http: Arc<s::Http>,
    db: SqlitePool,
    guild: s::GuildId,
    voices: Arc<Mutex<HashMap<u64, Instant>>>,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            ticker.tick().await;
            if let Err(error) = tick(&http, &db, guild, &voices).await {
                tracing::warn!(%error,"Background scheduler failed");
            }
        }
    });
}
pub async fn gateway(ctx: &s::Context, event: &s::FullEvent, data: &Data) -> Result<bool, Error> {
    let bot_id = ctx.cache.current_user().id;
    let cfg = data.settings.read().await.clone();
    match event {
        s::FullEvent::VoiceStateUpdate { old: _, new }
            if new.guild_id == Some(data.guild) && cfg.activity_tracking =>
        {
            let member = data.guild.member(&ctx.http, new.user_id).await?;
            if member.user.bot {
                return Ok(false);
            }
            let mut voices = data.voices.lock().await;
            if new.channel_id.is_some() {
                voices.entry(new.user_id.get()).or_insert_with(Instant::now);
            } else if let Some(since) = voices.remove(&new.user_id.get()) {
                crate::daily::voice(
                    &data.db,
                    data.guild.get(),
                    new.user_id.get(),
                    since.elapsed().as_secs() as i64,
                    s::Timestamp::now().unix_timestamp(),
                )
                .await?;
            }
        }
        s::FullEvent::GuildAuditLogEntryCreate { entry, guild_id }
            if *guild_id == data.guild && cfg.antinuke =>
        {
            if !matches!(entry.action.num(), 12 | 20 | 22 | 32 | 52) {
                return Ok(false);
            }
            let rules = data.rules.read().await.clone();
            let guild = data.guild.to_partial_guild(&ctx.http).await?;
            if entry.user_id == guild.owner_id
                || entry.user_id == bot_id
                || rules.trusted_actor_ids.contains(&entry.user_id.get())
            {
                return Ok(false);
            }
            let recent =
                s::Timestamp::now().unix_timestamp() - entry.id.created_at().unix_timestamp();
            if !(0..60).contains(&recent) {
                return Ok(false);
            }
            let trigger = {
                let now = Instant::now();
                let mut map = data.nuke.lock().await;
                map.retain(|_, v| {
                    v.last()
                        .is_some_and(|t| t.elapsed().as_secs() < rules.antinuke_window_seconds)
                });
                if map.len() >= 1000 && !map.contains_key(&entry.user_id.get()) {
                    false
                } else {
                    let events = map.entry(entry.user_id.get()).or_default();
                    events.retain(|t| t.elapsed().as_secs() < rules.antinuke_window_seconds);
                    events.push(now);
                    let hit = events.len() >= rules.antinuke_limit;
                    if hit {
                        events.clear();
                    }
                    hit
                }
            };
            if trigger {
                let actor = data.guild.member(&ctx.http, entry.user_id).await?;
                let bot = data.guild.member(&ctx.http, bot_id).await?;
                let mut removed = Vec::new();
                if security::higher(
                    security::highest(&guild, &bot),
                    security::highest(&guild, &actor),
                ) {
                    let dangerous = s::Permissions::ADMINISTRATOR
                        | s::Permissions::MANAGE_GUILD
                        | s::Permissions::MANAGE_ROLES
                        | s::Permissions::MANAGE_CHANNELS
                        | s::Permissions::MANAGE_WEBHOOKS
                        | s::Permissions::BAN_MEMBERS
                        | s::Permissions::KICK_MEMBERS;
                    for role in actor.roles.iter().filter_map(|id| guild.roles.get(id)) {
                        if !role.managed
                            && role.permissions.intersects(dangerous)
                            && actor.remove_role(&ctx.http, role.id).await.is_ok()
                        {
                            removed.push(role.id.get());
                        }
                    }
                }
                let reason = format!(
                    "Burst of destructive audit actions by {}. Removed dangerous roles: {:?}. If none, actor may be above bot or use managed/global grants; manual containment required.",
                    entry.user_id, removed
                );
                let _ = store::case(
                    &data.db,
                    data.guild.get(),
                    entry.user_id.get(),
                    bot_id.get(),
                    "antinuke",
                    &reason,
                )
                .await;
                log(&ctx.http, data, reason.clone()).await;
                if let Ok(owner) = guild.owner_id.to_user(&ctx.http).await {
                    let _ = owner
                        .direct_message(
                            &ctx.http,
                            s::CreateMessage::new()
                                .embed(
                                    crate::emoji::embed()
                                        .title("ELVNX / SAFETY ALERT")
                                        .description(reason)
                                        .color(0x111111),
                                )
                                .allowed_mentions(s::CreateAllowedMentions::new()),
                        )
                        .await;
                }
            }
        }
        _ => {}
    }
    Ok(false)
}
async fn tick(
    http: &s::Http,
    db: &SqlitePool,
    guild: s::GuildId,
    voices: &Mutex<HashMap<u64, Instant>>,
) -> Result<(), Error> {
    let cfg = store::settings(db, guild.get()).await?;
    if cfg.activity_tracking {
        let mut sessions = voices.lock().await;
        for (user, since) in sessions.iter_mut() {
            let seconds = since.elapsed().as_secs();
            if seconds > 0 {
                crate::daily::voice(
                    db,
                    guild.get(),
                    *user,
                    seconds as i64,
                    s::Timestamp::now().unix_timestamp(),
                )
                .await?;
                *since += std::time::Duration::from_secs(seconds);
            }
        }
    }
    let now = s::Timestamp::now().unix_timestamp();
    if cfg.activity_tracking
        && let Some(channel) = cfg.daily_insight_channel
    {
        crate::daily::tick(http, db, guild, channel, now).await?;
    }
    sqlx::query("DELETE FROM daily_activity WHERE day<?")
        .bind(crate::daily::day(now) - 30)
        .execute(db)
        .await?;
    if let (Some(channel), Some(day)) = (cfg.daily_quote_channel, schedule_due(now)) {
        let claim = sqlx::query("INSERT OR IGNORE INTO scheduled_quotes VALUES(?,?,'claimed')")
            .bind(guild.to_string())
            .bind(day)
            .execute(db)
            .await?;
        if claim.rows_affected() > 0 {
            let message = s::CreateMessage::new()
                .embed(
                    crate::emoji::embed()
                        .title("ELVNX / DAILY SIGNAL")
                        .description(format!("“{}”\n— ELVNX", quote_for(day as u64)))
                        .footer(s::CreateEmbedFooter::new(
                            "08:00 IST • original community quote",
                        ))
                        .color(0x111111),
                )
                .allowed_mentions(s::CreateAllowedMentions::new());
            let outcome = s::ChannelId::new(channel).send_message(http, message).await;
            sqlx::query("UPDATE scheduled_quotes SET state=? WHERE guild=? AND day=?")
                .bind(if outcome.is_ok() { "sent" } else { "failed" })
                .bind(guild.to_string())
                .bind(day)
                .execute(db)
                .await?;
            if let Err(error) = outcome {
                return Err(error.into());
            }
        }
    }
    sqlx::query("DELETE FROM seen_messages WHERE created<?")
        .bind(now - 86400 * 7)
        .execute(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ist_schedule_is_0230_utc_and_does_not_backfill_late() {
        assert_eq!(schedule_due(8999), None);
        assert_eq!(schedule_due(9000), Some(0));
        assert_eq!(schedule_due(9299), Some(0));
        assert_eq!(schedule_due(9300), None);
        assert_eq!(schedule_due(86400 + 9000), Some(1));
    }
    #[test]
    fn quotes_are_bounded_original_text() {
        for i in 0..100 {
            assert!(!quote_for(i).is_empty());
            assert!(quote_for(i).len() < 200);
        }
    }
}
