use crate::{Context, Data, Error, cards, emoji, reply, security, serenity as s, store};
use sqlx::{Row, SqlitePool};

fn current_bot(ctx: &s::Context) -> s::UserId {
    ctx.cache.current_user().id
}
const TITLE: &str = "ELVNX / DO NOT POST";
fn now() -> i64 {
    s::Timestamp::now().unix_timestamp()
}
fn staff_permissions() -> s::Permissions {
    s::Permissions::ADMINISTRATOR
        | s::Permissions::MANAGE_GUILD
        | s::Permissions::MODERATE_MEMBERS
        | s::Permissions::MANAGE_ROLES
        | s::Permissions::MANAGE_MESSAGES
        | s::Permissions::BAN_MEMBERS
}
pub fn cover() -> Result<Vec<u8>, Error> {
    cards::render(
        "SECURITY / RESTRICTED",
        "DO NOT POST HERE",
        "This is a marked security trap. Use normal chat channels.",
        &[
            ("ACTION", "Softban".into()),
            ("MESSAGES", "No history purge".into()),
            ("APPEAL", "Contact staff".into()),
        ],
        None,
    )
}
fn warning() -> s::CreateEmbed {
    emoji::embed_for("warning").title(TITLE).color(0x17181c)
        .description(format!("{} **This channel is a honeypot security trap.**\n\nDo not send messages, commands, attachments or test posts here. Use the server's normal chat channels. Posting here can remove your account from the server, including if you are a real person.", emoji::text("warning")))
        .field("What happens?", "The bot attempts a softban: ban, then immediate unban. No historical messages are purged. You will need an invitation to rejoin.", false)
        .field("Made a mistake?", "Contact server staff through a trusted existing contact. A trigger is not proof that you are a spam bot. Never share passwords or login codes.", false)
        .field("Staff & integrations", "Server owner, staff permissions, this bot, webhooks and explicitly trusted actor IDs are exempt. Other bots are not automatically trusted.", false)
        .footer(s::CreateEmbedFooter::new("ELVNX / HONEYPOT • no chat • staff can disable with /honeypot"))
}
async fn validate(
    ctx: &s::Context,
    db: &SqlitePool,
    guild_id: s::GuildId,
    id: s::ChannelId,
) -> Result<s::GuildChannel, Error> {
    let cfg = store::settings(db, guild_id.get()).await?;
    if [
        cfg.report_channel,
        cfg.welcome_channel,
        cfg.counting_channel,
        cfg.birthday_channel,
        cfg.daily_quote_channel,
        cfg.daily_insight_channel,
        cfg.appukuttan_legacy_channel,
        cfg.always_react_channel,
        cfg.voice_panel,
    ]
    .contains(&Some(id.get()))
    {
        return Err(crate::errors::user(
            "Use a dedicated honeypot channel, separate from chat, reports, welcomes, counting and scheduled posts.",
        ));
    }
    let channel = id
        .to_channel(&ctx.http)
        .await?
        .guild()
        .ok_or_else(|| crate::errors::user("Choose a server text channel."))?;
    let guild = guild_id.to_partial_guild(&ctx.http).await?;
    let bot = guild_id.member(&ctx.http, current_bot(ctx)).await?;
    let required = s::Permissions::VIEW_CHANNEL
        | s::Permissions::SEND_MESSAGES
        | s::Permissions::EMBED_LINKS
        | s::Permissions::ATTACH_FILES
        | s::Permissions::READ_MESSAGE_HISTORY;
    if channel.guild_id != guild_id
        || channel.kind != s::ChannelType::Text
        || !guild.user_permissions_in(&channel, &bot).contains(required)
        || !guild
            .member_permissions(&bot)
            .contains(s::Permissions::BAN_MEMBERS)
    {
        return Err(crate::errors::user(
            "The bot needs Ban Members and View Channel, Send Messages, Embed Links, Attach Files and Read Message History in the dedicated text channel.",
        ));
    }
    let log = s::ChannelId::new(
        cfg.log_channel
            .ok_or_else(|| crate::errors::user("Configure a private staff log channel first."))?,
    )
    .to_channel(&ctx.http)
    .await?
    .guild()
    .ok_or_else(|| crate::errors::user("Private staff log channel unavailable."))?;
    if log.id == id
        || log.guild_id != guild_id
        || !log.permission_overwrites.iter().any(|o| {
            o.kind == s::PermissionOverwriteType::Role(s::RoleId::new(guild_id.get()))
                && o.deny.contains(s::Permissions::VIEW_CHANNEL)
        })
        || !guild.user_permissions_in(&log, &bot).contains(
            s::Permissions::VIEW_CHANNEL
                | s::Permissions::SEND_MESSAGES
                | s::Permissions::EMBED_LINKS,
        )
    {
        return Err(crate::errors::user(
            "The honeypot needs a separate, accessible staff log channel that explicitly denies View Channel to everyone.",
        ));
    }
    Ok(channel)
}
pub async fn refresh(
    ctx: &s::Context,
    db: &SqlitePool,
    guild: s::GuildId,
    id: s::ChannelId,
) -> Result<(), Error> {
    let channel = validate(ctx, db, guild, id).await?;
    let bot = current_bot(ctx);
    let current = sqlx::query("SELECT channel,message FROM honeypot_panels WHERE guild=?")
        .bind(guild.to_string())
        .fetch_optional(db)
        .await?;
    if let Some(row) = current
        && row.get::<String, _>("channel") == id.to_string()
    {
        let message = s::MessageId::new(row.get::<String, _>("message").parse()?);
        if let Ok(m) = id.message(&ctx.http, message).await
            && m.author.id == bot
            && m.embeds.iter().any(|e| e.title.as_deref() == Some(TITLE))
        {
            return Ok(());
        }
    }
    let old = id
        .messages(&ctx.http, s::GetMessages::new().limit(100))
        .await?
        .into_iter()
        .find(|m| {
            m.author.id == bot
                && (m.content.contains("**HONEYPOT")
                    || m.embeds.iter().any(|e| {
                        matches!(
                            e.title.as_deref(),
                            Some(TITLE) | Some("ELVNX / HONEYPOT PAUSED")
                        )
                    }))
        });
    let png = cover()?;
    let embed = warning().image("attachment://honeypot.png");
    let sent = if let Some(old) = old {
        id.edit_message(
            &ctx.http,
            old.id,
            s::EditMessage::new()
                .content("")
                .embeds(vec![embed])
                .components(vec![])
                .attachments(
                    s::EditAttachments::new().add(s::CreateAttachment::bytes(png, "honeypot.png")),
                )
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await?
    } else {
        id.send_message(
            &ctx.http,
            s::CreateMessage::new()
                .embed(embed)
                .add_file(s::CreateAttachment::bytes(png, "honeypot.png"))
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await?
    };
    sqlx::query("INSERT INTO honeypot_panels(guild,channel,message,activated) VALUES(?,?,?,?) ON CONFLICT(guild) DO UPDATE SET channel=excluded.channel,message=excluded.message,activated=excluded.activated").bind(guild.to_string()).bind(channel.id.to_string()).bind(sent.id.to_string()).bind(now()).execute(db).await?;
    Ok(())
}
pub async fn configure(
    ctx: Context<'_>,
    enabled: bool,
    channel: Option<s::GuildChannel>,
    confirm: bool,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if enabled && !confirm {
        return reply(ctx,"Canceled. This dedicated trap can remove real members too. Use confirm:true only after reviewing the warning and destination.").await;
    }
    let _guard = ctx.data().configuration_lock.lock().await;
    let mut cfg = ctx.data().settings.read().await.clone();
    if enabled {
        let channel =
            channel.ok_or_else(|| crate::errors::user("Choose a dedicated trap channel."))?;
        crate::commands::posting_channel(ctx, &channel).await?;
        refresh(
            ctx.serenity_context(),
            &ctx.data().db,
            ctx.data().guild,
            channel.id,
        )
        .await?;
        cfg.honeypot_channel = Some(channel.id.get());
    } else {
        cfg.honeypot_channel = None;
    }
    store::save(&ctx.data().db, ctx.data().guild.get(), &cfg).await?;
    *ctx.data().settings.write().await = cfg;
    if !enabled
        && let Some(row) = sqlx::query("SELECT channel,message FROM honeypot_panels WHERE guild=?")
            .bind(ctx.data().guild.to_string())
            .fetch_optional(&ctx.data().db)
            .await?
    {
        let channel = s::ChannelId::new(row.get::<String, _>("channel").parse()?);
        let id = s::MessageId::new(row.get::<String, _>("message").parse()?);
        if let Ok(m) = channel.message(ctx.http(), id).await
            && m.author.id == crate::bot_id(ctx)
        {
            let _=channel.edit_message(ctx.http(),id,s::EditMessage::new().embeds(vec![emoji::embed_for("lock").title("ELVNX / HONEYPOT PAUSED").description("Automatic trap removals are disabled. This remains a dedicated security channel; use normal chat channels. Staff can re-enable it with /honeypot.")]).components(vec![]).allowed_mentions(s::CreateAllowedMentions::new())).await;
        }
    }
    reply(ctx,if enabled {"Honeypot armed. The marked warning, hierarchy checks, duplicate-trigger protection, member notices and private staff alerts are active."}else{"Honeypot disabled. No new removals will be performed; any unfinished softban will still be repaired when safely possible."}).await
}
async fn claim(
    db: &SqlitePool,
    guild: s::GuildId,
    user: s::UserId,
    channel: s::ChannelId,
    message: s::MessageId,
    time: i64,
) -> Result<bool, Error> {
    Ok(sqlx::query("INSERT INTO honeypot_actions(guild,user,channel,message,created,state,attempts) VALUES(?,?,?,?,?,'claimed',0) ON CONFLICT(guild,user) DO UPDATE SET channel=excluded.channel,message=excluded.message,created=excluded.created,state='claimed',attempts=0 WHERE honeypot_actions.created<? AND honeypot_actions.state NOT IN('claimed','recovering','held')").bind(guild.to_string()).bind(user.to_string()).bind(channel.to_string()).bind(message.to_string()).bind(time).bind(time-300).execute(db).await?.rows_affected()==1)
}
fn reason(message: &str) -> String {
    format!("ELVNX honeypot / {message}")
}
fn owned_ban(actual: Option<&str>, message: &str) -> bool {
    actual == Some(reason(message).as_str())
}
#[derive(Debug, PartialEq, Eq)]
enum Repair {
    Removed,
    Clear,
    Unrelated,
}
async fn repair(
    ctx: &s::Context,
    guild: s::GuildId,
    user: s::UserId,
    message: &str,
) -> Result<Repair, Error> {
    match guild.get_ban(&ctx.http, user).await? {
        None => Ok(Repair::Clear),
        Some(ban) if !owned_ban(ban.reason.as_deref(), message) => Ok(Repair::Unrelated),
        Some(_) => {
            guild.unban(&ctx.http, user).await?;
            Ok(Repair::Removed)
        }
    }
}
async fn finish(
    db: &SqlitePool,
    guild: s::GuildId,
    user: s::UserId,
    message: &str,
    state: &str,
) -> Result<(), Error> {
    sqlx::query("UPDATE honeypot_actions SET state=? WHERE guild=? AND user=? AND message=?")
        .bind(state)
        .bind(guild.to_string())
        .bind(user.to_string())
        .bind(message)
        .execute(db)
        .await?;
    Ok(())
}
async fn alert(ctx: &s::Context, db: &SqlitePool, guild: s::GuildId, text: String) {
    let message = || {
        s::CreateMessage::new()
            .embed(
                emoji::embed_for("shield")
                    .title("ELVNX / HONEYPOT REPORT")
                    .description(&text)
                    .color(0x17181c),
            )
            .allowed_mentions(s::CreateAllowedMentions::new())
    };
    if let Ok(cfg) = store::settings(db, guild.get()).await
        && let Some(channel) = cfg.log_channel
        && let Err(error) = s::ChannelId::new(channel)
            .send_message(&ctx.http, message())
            .await
    {
        tracing::warn!(%error,"Honeypot staff log delivery failed");
    }
    if let Ok(g) = guild.to_partial_guild(&ctx.http).await
        && let Ok(owner) = g.owner_id.to_user(&ctx.http).await
        && let Err(error) = owner.direct_message(&ctx.http, message()).await
    {
        tracing::warn!(%error,"Honeypot owner DM unavailable");
    }
}
fn member_notice(outcome: &str) -> s::CreateEmbed {
    emoji::embed_for("shield").title("ELVNX / CHANNEL SAFETY NOTICE")
        .description("A message from your account was posted in ELVNX's marked honeypot channel. This is a security trap, not a chat channel. A trigger is not proof that you are a bot.")
        .field("Action",outcome,false)
        .field("Need help?","If this was accidental, contact server staff through a trusted existing contact. Never share your password or login codes.",false)
}
pub async fn message(ctx: &s::Context, data: &Data, m: &s::Message) -> Result<bool, Error> {
    let cfg = data.settings.read().await.clone();
    if m.guild_id != Some(data.guild)
        || cfg.honeypot_channel != Some(m.channel_id.get())
        || m.author.id == current_bot(ctx)
        || m.webhook_id.is_some()
    {
        return Ok(false);
    }
    let guild = data.guild.to_partial_guild(&ctx.http).await?;
    let member = data.guild.member(&ctx.http, m.author.id).await?;
    if member.user.id == guild.owner_id
        || data
            .rules
            .read()
            .await
            .trusted_actor_ids
            .contains(&member.user.id.get())
        || guild
            .member_permissions(&member)
            .intersects(staff_permissions())
    {
        return Ok(false);
    }
    if !claim(&data.db, data.guild, m.author.id, m.channel_id, m.id, now()).await? {
        return Ok(true);
    }
    let allowed:Result<(),Error>=async {
        validate(ctx,&data.db,data.guild,m.channel_id).await?;
        let row=sqlx::query("SELECT message,activated FROM honeypot_panels WHERE guild=? AND channel=?").bind(data.guild.to_string()).bind(m.channel_id.to_string()).fetch_optional(&data.db).await?.ok_or_else(||crate::errors::user("Honeypot is not armed with a current warning panel. Run /honeypot again."))?;
        let warning=m.channel_id.message(&ctx.http,s::MessageId::new(row.get::<String,_>("message").parse()?)).await?;
        if warning.author.id!=current_bot(ctx) || !warning.embeds.iter().any(|e|e.title.as_deref()==Some(TITLE)) || m.timestamp.unix_timestamp()<row.get::<i64,_>("activated") {return Err(crate::errors::user("The marked warning is missing or this message predates activation. No removal performed."));}
        let bot=data.guild.member(&ctx.http,current_bot(ctx)).await?;
        if !security::higher(security::highest(&guild,&bot),security::highest(&guild,&member)) {return Err(crate::errors::user("Target is above or equal to the bot's role. No removal performed."));}
        Ok(())
    }.await;
    if let Err(error) = allowed {
        finish(
            &data.db,
            data.guild,
            m.author.id,
            &m.id.to_string(),
            "blocked",
        )
        .await?;
        alert(
            ctx,
            &data.db,
            data.guild,
            format!(
                "{} **Trigger blocked safely**\nMember `{}` · channel <#{}>\n{}",
                emoji::text("warning"),
                m.author.id,
                m.channel_id,
                crate::errors::message(error.as_ref(), "honeypot")
            ),
        )
        .await;
        return Ok(true);
    }
    let notice=member.user.direct_message(&ctx.http,s::CreateMessage::new().embed(member_notice("A softban is being attempted: ban, then immediate unban, without purging historical messages. Staff will receive the actual result.")).allowed_mentions(s::CreateAllowedMentions::new())).await;
    let ban = data
        .guild
        .ban_with_reason(&ctx.http, m.author.id, 0, reason(&m.id.to_string()))
        .await;
    let mut repaired = repair(ctx, data.guild, m.author.id, &m.id.to_string()).await;
    if repaired.is_err() {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        repaired = repair(ctx, data.guild, m.author.id, &m.id.to_string()).await;
    }
    let (state, outcome) = match repaired {
        Ok(Repair::Removed) => (
            "complete",
            "Softban completed: the matching honeypot ban was removed. No historical messages were purged.",
        ),
        Ok(Repair::Clear) if ban.is_ok() => (
            "complete",
            "Softban completed: the member was removed and is no longer banned. No historical messages were purged.",
        ),
        Ok(Repair::Clear) => (
            "claimed",
            "The ban request failed and no ban is currently visible. No successful removal was confirmed. Bounded recovery will monitor this ambiguous request.",
        ),
        Ok(Repair::Unrelated) => (
            "held",
            "The current ban does not match this honeypot action. It was left unchanged for staff review.",
        ),
        Err(_) if ban.is_ok() => (
            "recovering",
            "URGENT: ban succeeded but unban could not be confirmed. Staff should check the ban state now. Bounded recovery will only remove the matching honeypot ban.",
        ),
        Err(_) => (
            "claimed",
            "The ban request and current ban state could not be confirmed. Staff should inspect permissions and the ban list. Bounded recovery will check this action.",
        ),
    };
    // The durable claim was written before the ban; a crash can be repaired by matching its unique ban reason.
    if let Err(error) = finish(&data.db, data.guild, m.author.id, &m.id.to_string(), state).await {
        tracing::error!(%error,"Honeypot outcome persistence failed; inspect the member's ban state");
    }
    let dm_sent = notice.is_ok();
    let mut dm_updated = false;
    if let Ok(notice) = notice {
        dm_updated = notice
            .channel_id
            .edit_message(
                &ctx.http,
                notice.id,
                s::EditMessage::new()
                    .embed(member_notice(outcome))
                    .allowed_mentions(s::CreateAllowedMentions::new()),
            )
            .await
            .is_ok();
    }
    let case = store::case(
        &data.db,
        data.guild.get(),
        m.author.id.get(),
        current_bot(ctx).get(),
        "honeypot",
        outcome,
    )
    .await;
    alert(ctx,&data.db,data.guild,format!("{} **Honeypot outcome**\nMember `{}` · channel <#{}> · message `{}`\n\n{}\n\nMember notice sent: **{}** · updated with result: **{}**\nAudit receipt saved: **{}**",emoji::text(if state=="complete" {"shield"} else {"warning"}),m.author.id,m.channel_id,m.id,outcome,dm_sent,dm_updated,case.is_ok())).await;
    Ok(true)
}
pub async fn tick(ctx: &s::Context, db: &SqlitePool, guild: s::GuildId) -> Result<(), Error> {
    let rows=sqlx::query("SELECT user,message,created,state,attempts FROM honeypot_actions WHERE guild=? AND state IN('claimed','recovering') AND created<? LIMIT 10").bind(guild.to_string()).bind(now()-180).fetch_all(db).await?;
    for row in rows {
        let user = s::UserId::new(row.get::<String, _>("user").parse()?);
        let message = row.get::<String, _>("message");
        match repair(ctx, guild, user, &message).await {
            Ok(Repair::Removed) => {
                finish(db, guild, user, &message, "complete").await?;
                alert(ctx,db,guild,format!("{} Recovered unfinished softban for `{user}`. The matching honeypot ban was removed.",emoji::text("check"))).await;
                continue;
            }
            Ok(Repair::Unrelated) => {
                finish(db, guild, user, &message, "held").await?;
                alert(ctx,db,guild,format!("Honeypot recovery held for `{user}`: the current ban does not match this action. No automatic unban performed. Staff must review.")).await;
                continue;
            }
            Ok(Repair::Clear) => {
                if row.get::<String, _>("state") == "recovering" {
                    finish(db, guild, user, &message, "complete").await?;
                    continue;
                }
                if row.get::<i64, _>("created") < now() - 900 {
                    finish(db, guild, user, &message, "failed").await?;
                    alert(ctx,db,guild,format!("Honeypot monitoring complete for `{user}`: no ban is visible. No automatic unban was needed.")).await;
                }
                continue;
            }
            Err(error) => {
                tracing::warn!(member=%user,%error,"Honeypot recovery attempt failed");
                sqlx::query("UPDATE honeypot_actions SET attempts=attempts+1 WHERE guild=? AND user=? AND message=?").bind(guild.to_string()).bind(user.to_string()).bind(&message).execute(db).await?;
            }
        }
        if row.get::<i64, _>("attempts") >= 2 || row.get::<i64, _>("created") < now() - 900 {
            finish(db, guild, user, &message, "held").await?;
            alert(ctx,db,guild,format!("Honeypot recovery needs manual review for `{user}`. Automatic recovery has stopped; check whether a ban remains.")).await;
        }
    }
    sqlx::query(
        "DELETE FROM honeypot_actions WHERE created<? AND state NOT IN('claimed','recovering')",
    )
    .bind(now() - 7 * 86400)
    .execute(db)
    .await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn duplicate_triggers_are_claimed_once_and_unfinished_removals_are_not_replaced() {
        let db = store::connect("sqlite::memory:").await.unwrap();
        let g = s::GuildId::new(1);
        let u = s::UserId::new(2);
        let c = s::ChannelId::new(3);
        assert!(
            claim(&db, g, u, c, s::MessageId::new(4), 1000)
                .await
                .unwrap()
        );
        assert!(
            !claim(&db, g, u, c, s::MessageId::new(5), 1001)
                .await
                .unwrap()
        );
        assert!(
            !claim(&db, g, u, c, s::MessageId::new(6), 2000)
                .await
                .unwrap()
        );
        finish(&db, g, u, "4", "complete").await.unwrap();
        assert!(
            !claim(&db, g, u, c, s::MessageId::new(7), 1200)
                .await
                .unwrap()
        );
        assert!(
            claim(&db, g, u, c, s::MessageId::new(8), 1400)
                .await
                .unwrap()
        );
        assert!(
            claim(&db, g, s::UserId::new(9), c, s::MessageId::new(10), 1400)
                .await
                .unwrap()
        );
    }
    #[test]
    fn recovery_never_unbans_unrelated_or_reasonless_bans() {
        assert!(owned_ban(Some("ELVNX honeypot / 42"), "42"));
        assert!(!owned_ban(Some("ELVNX honeypot / 43"), "42"));
        assert!(!owned_ban(Some("Staff moderation"), "42"));
        assert!(!owned_ban(None, "42"));
    }
    #[test]
    fn warning_is_explicit_and_not_an_accusation_of_being_a_bot() {
        let json = serde_json::to_value(warning()).unwrap();
        assert_eq!(json["title"], TITLE);
        assert!(json.to_string().contains("real person"));
        assert!(json.to_string().contains("No historical messages"));
    }
}
