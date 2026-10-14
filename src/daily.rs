use crate::{Context, Error, cards, reply, serenity as s, store};
use sqlx::{Row, SqlitePool};
pub async fn message(data: &crate::Data, m: &s::Message) -> Result<(), Error> {
    if m.guild_id != Some(data.guild)
        || m.author.bot
        || m.webhook_id.is_some()
        || !data.settings.read().await.activity_tracking
    {
        return Ok(());
    }
    let mut tx = data.db.begin().await?;
    let inserted = sqlx::query("INSERT OR IGNORE INTO seen_messages VALUES(?,?)")
        .bind(m.id.to_string())
        .bind(s::Timestamp::now().unix_timestamp())
        .execute(&mut *tx)
        .await?;
    if inserted.rows_affected() > 0 {
        sqlx::query("INSERT INTO activity(guild,user,messages) VALUES(?,?,1) ON CONFLICT(guild,user) DO UPDATE SET messages=messages+1").bind(data.guild.to_string()).bind(m.author.id.to_string()).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO daily_activity(guild,day,user,messages) VALUES(?,?,?,1) ON CONFLICT(guild,day,user) DO UPDATE SET messages=messages+1").bind(data.guild.to_string()).bind(day(m.timestamp.unix_timestamp())).bind(m.author.id.to_string()).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub const IST_OFFSET: i64 = 19_800;
pub fn day(now: i64) -> i64 {
    (now + IST_OFFSET).div_euclid(86_400)
}
pub fn due(now: i64) -> Option<i64> {
    let seconds = (now + IST_OFFSET).rem_euclid(86_400);
    (85_800..86_100).contains(&seconds).then(|| day(now))
}
pub fn voice_parts(end: i64, seconds: i64) -> Vec<(i64, i64)> {
    let mut start = end - seconds.max(0);
    let mut parts = Vec::new();
    while start < end {
        let bucket = day(start);
        let stop = end.min((bucket + 1) * 86_400 - IST_OFFSET);
        parts.push((bucket, stop - start));
        start = stop;
    }
    parts
}
pub async fn voice(
    db: &SqlitePool,
    guild: u64,
    user: u64,
    seconds: i64,
    end: i64,
) -> Result<(), Error> {
    let mut tx = db.begin().await?;
    sqlx::query("INSERT INTO activity(guild,user,voice) VALUES(?,?,?) ON CONFLICT(guild,user) DO UPDATE SET voice=voice+excluded.voice")
        .bind(guild.to_string()).bind(user.to_string()).bind(seconds).execute(&mut *tx).await?;
    for (day, amount) in voice_parts(end, seconds) {
        sqlx::query("INSERT INTO daily_activity(guild,day,user,voice) VALUES(?,?,?,?) ON CONFLICT(guild,day,user) DO UPDATE SET voice=voice+excluded.voice")
            .bind(guild.to_string()).bind(day).bind(user.to_string()).bind(amount).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
/// Configure a daily activity report card at 23:50 IST.
#[poise::command(
    slash_command,
    guild_only,
    rename = "daily-insight",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
pub async fn daily_insight(
    ctx: Context<'_>,
    #[description = "Enable or disable daily reports"] enabled: bool,
    #[description = "Report destination when enabling"] channel: Option<s::GuildChannel>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let id = if enabled {
        if !ctx.data().settings.read().await.activity_tracking {
            return reply(
                ctx,
                "Enable /activity first. Reports use future tracked activity only.",
            )
            .await;
        }
        let ch = channel.ok_or("Choose a report channel")?;
        crate::commands::posting_channel(ctx, &ch).await?;
        let bot_id = ctx.serenity_context().cache.current_user().id;
        let bot = ctx.data().guild.member(ctx.http(), bot_id).await?;
        let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
        if !guild
            .user_permissions_in(&ch, &bot)
            .contains(s::Permissions::EMBED_LINKS | s::Permissions::ATTACH_FILES)
        {
            return Err("Grant Embed Links and Attach Files in the report channel".into());
        }
        Some(ch.id.get())
    } else {
        None
    };
    let _guard = ctx.data().configuration_lock.lock().await;
    let mut cfg = ctx.data().settings.read().await.clone();
    cfg.daily_insight_channel = id;
    store::save(&ctx.data().db, ctx.data().guild.get(), &cfg).await?;
    *ctx.data().settings.write().await = cfg;
    reply(ctx, if enabled { "Daily insight enabled: 11:50 PM IST. Reports cover today’s tracked messages and connected voice time through the report time. No historical data is imported." } else { "Daily insight disabled." }).await
}
pub async fn tick(
    http: &s::Http,
    db: &SqlitePool,
    guild: s::GuildId,
    channel: u64,
    now: i64,
) -> Result<(), Error> {
    let Some(day) = due(now) else {
        return Ok(());
    };
    let rows = sqlx::query("SELECT user,messages,voice FROM daily_activity WHERE guild=? AND day=? AND (messages>0 OR voice>0) ORDER BY messages DESC,voice DESC,user LIMIT 5")
        .bind(guild.to_string()).bind(day).fetch_all(db).await?;
    let totals = sqlx::query("SELECT COALESCE(SUM(messages),0) messages, COALESCE(SUM(voice),0) voice, COUNT(*) members FROM daily_activity WHERE guild=? AND day=? AND (messages>0 OR voice>0)")
        .bind(guild.to_string()).bind(day).fetch_one(db).await?;
    let date = s::Timestamp::from_unix_timestamp(day * 86_400)?
        .to_string()
        .chars()
        .take(10)
        .collect::<String>();
    let voice = totals.get::<i64, _>("voice");
    let mut stats = vec![
        ("MESSAGES", totals.get::<i64, _>("messages").to_string()),
        ("VOICE CONNECTED", duration(voice)),
        (
            "ACTIVE MEMBERS",
            totals.get::<i64, _>("members").to_string(),
        ),
    ];
    let mut leaders = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let raw = row.get::<String, _>("user");
        let id = s::UserId::new(raw.parse()?);
        let name = id.to_user(http).await.map(|u| u.name).unwrap_or_else(|_| {
            format!("Member {}", raw.chars().rev().take(4).collect::<String>())
        });
        if i < 3 {
            stats.push((
                [
                    "#1 MESSAGE LEADER",
                    "#2 MESSAGE LEADER",
                    "#3 MESSAGE LEADER",
                ][i],
                format!(
                    "{} / {}",
                    name.chars().take(9).collect::<String>(),
                    row.get::<i64, _>("messages")
                ),
            ));
        }
        leaders.push(format!(
            "{}. <@{}> · {} messages · {} voice",
            i + 1,
            raw,
            row.get::<i64, _>("messages"),
            duration(row.get::<i64, _>("voice"))
        ));
    }
    let png = cards::render(
        "DAILY INSIGHT",
        &format!("ELVNX · {date}"),
        "Today so far · 00:00–23:50 IST · observed activity",
        &stats,
        None,
    )?;
    let claim = sqlx::query("INSERT OR IGNORE INTO scheduled_insights VALUES(?,?,'claimed')")
        .bind(guild.to_string())
        .bind(day)
        .execute(db)
        .await?;
    if claim.rows_affected() == 0 {
        return Ok(());
    }
    let message = s::CreateMessage::new().embed(s::CreateEmbed::new().title(format!("ELVNX / DAILY INSIGHT · {date}")).description(if leaders.is_empty() { "No tracked activity today.".into() } else { leaders.join("\n") }).image("attachment://daily-insight.png").footer(s::CreateEmbedFooter::new("Ranked by messages, then voice time • online tracked periods only • voice connection is not speech")).color(0x111111))
        .add_file(s::CreateAttachment::bytes(png, "daily-insight.png")).allowed_mentions(s::CreateAllowedMentions::new());
    let result = s::ChannelId::new(channel).send_message(http, message).await;
    sqlx::query("UPDATE scheduled_insights SET state=? WHERE guild=? AND day=?")
        .bind(if result.is_ok() { "sent" } else { "failed" })
        .bind(guild.to_string())
        .bind(day)
        .execute(db)
        .await?;
    result?;
    Ok(())
}
fn duration(seconds: i64) -> String {
    format!("{}h {}m", seconds / 3600, seconds % 3600 / 60)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn daily_schedule_and_midnight_voice_split() {
        assert_eq!(due(65_999), None);
        assert_eq!(due(66_000), Some(0));
        assert_eq!(due(66_299), Some(0));
        assert_eq!(due(66_300), None);
        assert_eq!(due(86_400 + 66_000), Some(1));
        assert_eq!(voice_parts(66_610, 30), vec![(0, 20), (1, 10)]);
        assert_eq!(voice_parts(66_610, 0), Vec::<(i64, i64)>::new());
    }
    #[tokio::test]
    async fn voice_daily_rollover_is_atomic_and_scoped() {
        let db = store::connect("sqlite::memory:").await.unwrap();
        voice(&db, 1, 2, 30, 66_610).await.unwrap();
        let total: i64 =
            sqlx::query_scalar("SELECT voice FROM activity WHERE guild='1' AND user='2'")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(total, 30);
        let buckets: Vec<i64> =
            sqlx::query_scalar("SELECT voice FROM daily_activity WHERE guild='1' ORDER BY day")
                .fetch_all(&db)
                .await
                .unwrap();
        assert_eq!(buckets, vec![20, 10]);
        let first = sqlx::query("INSERT OR IGNORE INTO scheduled_insights VALUES('1',0,'claimed')")
            .execute(&db)
            .await
            .unwrap();
        let second =
            sqlx::query("INSERT OR IGNORE INTO scheduled_insights VALUES('1',0,'claimed')")
                .execute(&db)
                .await
                .unwrap();
        assert_eq!(first.rows_affected(), 1);
        assert_eq!(second.rows_affected(), 0);
    }
}
