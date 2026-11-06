use crate::{Data, Error, emoji, serenity as s, store};
use rand::Rng;
use sqlx::{Row, SqlitePool};

const RULES: &str = "**Crack the hidden number.**\n\nThe bot chooses a number from **1–1000**. Send one whole number in the current range. Each valid miss gets a **higher/lower** hint and narrows the range for everyone.\n\n• The correct guess earns **one win** and starts a fresh round automatically.\n• Wait **5 seconds** between your valid guesses.\n• Send numbers only, with no attachments or mentions.\n• Invalid guesses never reset a round.\n\n**!score** — your wins, guesses and current range\n**!leaderboard** — top ten players\n**!rules** — rules and current range\n**!reset** — Manage Server staff only; keeps scores.\n\nRound progress survives restarts. Scores store user IDs, wins and guess counts separately from activity tracking. Previous guesses and message contents are not retained. No wagers or prizes.";

fn target() -> i64 {
    rand::thread_rng().gen_range(1..=1000)
}
fn number(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() || text.len() > 4 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|n| (1..=1000).contains(n))
}
#[derive(Debug, PartialEq)]
enum Guess {
    Hint { higher: bool, low: i64, high: i64 },
    Win { number: i64, attempts: i64 },
    Rejected(&'static str),
    Duplicate,
}

async fn seed(db: &SqlitePool, channel: &str) -> Result<(), Error> {
    sqlx::query("INSERT INTO guess_game(channel,target) VALUES(?,?) ON CONFLICT DO NOTHING")
        .bind(channel)
        .bind(target())
        .execute(db)
        .await?;
    Ok(())
}
async fn play(
    db: &SqlitePool,
    channel: &str,
    user: &str,
    message: i64,
    text: &str,
    now: i64,
) -> Result<Guess, Error> {
    let Some(n) = number(text) else {
        return Ok(Guess::Rejected("Send a whole number from 1 to 1000."));
    };
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    let row = sqlx::query("SELECT * FROM guess_game WHERE channel=?")
        .bind(channel)
        .fetch_one(&mut *tx)
        .await?;
    if message <= row.get::<i64, _>("last_message") {
        return Ok(Guess::Duplicate);
    }
    let last: Option<i64> =
        sqlx::query_scalar("SELECT last_at FROM guess_game_scores WHERE channel=? AND user=?")
            .bind(channel)
            .bind(user)
            .fetch_optional(&mut *tx)
            .await?;
    if last.is_some_and(|at| now - at < 5) {
        return Ok(Guess::Rejected(
            "Wait 5 seconds between your valid guesses.",
        ));
    }
    let low = row.get::<i64, _>("lower_bound");
    let high = row.get::<i64, _>("upper_bound");
    if n < low || n > high {
        return Ok(Guess::Rejected(
            "That number is outside the current range. Use the range below.",
        ));
    }
    let secret = row.get::<i64, _>("target");
    let attempts = row.get::<i64, _>("attempts") + 1;
    let won = n == secret;
    sqlx::query("INSERT INTO guess_game_scores VALUES(?,?,?,1,?) ON CONFLICT(channel,user) DO UPDATE SET wins=wins+excluded.wins,guesses=guesses+1,last_at=excluded.last_at").bind(channel).bind(user).bind(i64::from(won)).bind(now).execute(&mut *tx).await?;
    let result = if won {
        sqlx::query("UPDATE guess_game SET target=?,lower_bound=1,upper_bound=1000,attempts=0,round=round+1,last_message=? WHERE channel=?").bind(target()).bind(message).bind(channel).execute(&mut *tx).await?;
        Guess::Win {
            number: n,
            attempts,
        }
    } else {
        let (low, high) = if n < secret {
            (n + 1, high)
        } else {
            (low, n - 1)
        };
        sqlx::query("UPDATE guess_game SET lower_bound=?,upper_bound=?,attempts=?,last_message=? WHERE channel=?").bind(low).bind(high).bind(attempts).bind(message).bind(channel).execute(&mut *tx).await?;
        Guess::Hint {
            higher: n < secret,
            low,
            high,
        }
    };
    tx.commit().await?;
    Ok(result)
}
async fn send(ctx: &s::Context, channel: s::ChannelId, text: String) -> Result<(), Error> {
    channel
        .send_message(
            &ctx.http,
            s::CreateMessage::new()
                .embed(
                    emoji::embed_for("search")
                        .title("ELVNX / CRACK THE CODE")
                        .description(text),
                )
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await?;
    Ok(())
}
async fn status(db: &SqlitePool, channel: &str, user: &str) -> Result<String, Error> {
    let row = sqlx::query(
        "SELECT lower_bound,upper_bound,attempts,round FROM guess_game WHERE channel=?",
    )
    .bind(channel)
    .fetch_one(db)
    .await?;
    let score =
        sqlx::query("SELECT wins,guesses FROM guess_game_scores WHERE channel=? AND user=?")
            .bind(channel)
            .bind(user)
            .fetch_optional(db)
            .await?;
    Ok(format!(
        "Current range: **{}–{}** · Round **{}**\nRound guesses: **{}**\nYour wins: **{}** · Your valid guesses: **{}**",
        row.get::<i64, _>("lower_bound"),
        row.get::<i64, _>("upper_bound"),
        row.get::<i64, _>("round"),
        row.get::<i64, _>("attempts"),
        score.as_ref().map_or(0, |s| s.get::<i64, _>("wins")),
        score.as_ref().map_or(0, |s| s.get::<i64, _>("guesses"))
    ))
}
pub async fn refresh(ctx: &s::Context, data: &Data) -> Result<(), Error> {
    let _guard = data.configuration_lock.lock().await;
    let mut cfg = data.settings.read().await.clone();
    let Some(channel) = cfg.guess_game_channel.map(s::ChannelId::new) else {
        return Ok(());
    };
    seed(&data.db, &channel.to_string()).await?;
    let cover = crate::cards::render(
        "CRACK THE CODE",
        "One number. One winner.",
        "GUESS / NARROW / WIN",
        &[
            ("RANGE", "1 > 1000".into()),
            ("WIN", "+1 VICTORY".into()),
            ("COOLDOWN", "5 SECONDS".into()),
        ],
        None,
    )?;
    let embed = emoji::embed_for("search")
        .title("ELVNX / GUESS NUMBER")
        .description(RULES)
        .image("attachment://guess-number.png");
    if let Some(id) = cfg.guess_game_panel {
        match channel
            .edit_message(
                &ctx.http,
                s::MessageId::new(id),
                s::EditMessage::new().embed(embed.clone()).attachments(
                    s::EditAttachments::new().add(s::CreateAttachment::bytes(
                        cover.clone(),
                        "guess-number.png",
                    )),
                ),
            )
            .await
        {
            Ok(_) => return Ok(()),
            Err(s::Error::Http(ref e)) if e.status_code() == Some(s::StatusCode::NOT_FOUND) => {}
            Err(e) => return Err(e.into()),
        }
    }
    let m = channel
        .send_message(
            &ctx.http,
            s::CreateMessage::new()
                .embed(embed)
                .add_file(s::CreateAttachment::bytes(cover, "guess-number.png"))
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await?;
    cfg.guess_game_panel = Some(m.id.get());
    store::save(&data.db, data.guild.get(), &cfg).await?;
    *data.settings.write().await = cfg;
    Ok(())
}
pub async fn message(ctx: &s::Context, data: &Data, m: &s::Message) -> Result<(), Error> {
    if m.guild_id != Some(data.guild)
        || m.author.bot
        || m.webhook_id.is_some()
        || data.settings.read().await.guess_game_channel != Some(m.channel_id.get())
    {
        return Ok(());
    }
    let channel = m.channel_id.to_string();
    let user = m.author.id.to_string();
    let now = chrono::Utc::now().timestamp();
    let cmd = m.content.trim().to_ascii_lowercase();
    if cmd.starts_with('!') {
        if !crate::word_chain::feedback(&data.db, &channel, &user, now).await? {
            return Ok(());
        }
        let text=match cmd.as_str(){
            "!rules"=>format!("{RULES}\n\n{}",status(&data.db,&channel,&user).await?),
            "!score"=>status(&data.db,&channel,&user).await?,
            "!leaderboard"=>{
                let rows=sqlx::query("SELECT user,wins FROM guess_game_scores WHERE channel=? AND wins>0 ORDER BY wins DESC,user ASC LIMIT 10").bind(&channel).fetch_all(&data.db).await?;
                if rows.is_empty(){"No winners yet. Send a number from 1 to 1000!".into()}else{rows.iter().enumerate().map(|(i,r)|format!("**{}.** <@{}> — **{}** wins",i+1,r.get::<String,_>("user"),r.get::<i64,_>("wins"))).collect::<Vec<_>>().join("\n")}
            },
            "!reset"=>{
                let guild=data.guild.to_partial_guild(&ctx.http).await?;let member=data.guild.member(&ctx.http,m.author.id).await?;
                if guild.owner_id!=m.author.id&&!guild.member_permissions(&member).contains(s::Permissions::MANAGE_GUILD){"Only staff with Manage Server can reset the round.".into()}else{
                    sqlx::query("UPDATE guess_game SET target=?,lower_bound=1,upper_bound=1000,attempts=0,round=round+1,last_message=? WHERE channel=?").bind(target()).bind(m.id.get() as i64).bind(&channel).execute(&data.db).await?;
                    "New hidden number selected. Range: **1–1000**. Scores are kept.".into()
                }
            },
            _=>"Use **!rules**, **!score** or **!leaderboard**, or send a number in the current range.".into()
        };
        return send(ctx, m.channel_id, text).await;
    }
    let result = if !m.attachments.is_empty()
        || !m.sticker_items.is_empty()
        || !m.mentions.is_empty()
        || !m.mention_roles.is_empty()
        || m.mention_everyone
    {
        Guess::Rejected("Send a number only, without attachments, stickers or mentions.")
    } else {
        play(
            &data.db,
            &channel,
            &user,
            m.id.get() as i64,
            &m.content,
            now,
        )
        .await?
    };
    match result {
        Guess::Hint { higher, low, high } => {
            send(
                ctx,
                m.channel_id,
                format!(
                    "**{}**\nThe number is **{}** than your guess.\nNew range: **{low}–{high}**",
                    emoji::text(if higher { "plus" } else { "minus" }),
                    if higher { "higher" } else { "lower" }
                ),
            )
            .await?
        }
        Guess::Win { number, attempts } => {
            m.react(&ctx.http, emoji::reaction("trophy")?).await?;
            send(ctx,m.channel_id,format!("{} **Correct — {number}!**\nYour guess earns **one win**. Solved in **{attempts}** total guesses.\n\nA new number is ready: **1–1000**. Use !leaderboard for winners.",emoji::text("trophy"))).await?;
        }
        Guess::Rejected(reason) => {
            if crate::word_chain::feedback(&data.db, &channel, &user, now).await? {
                send(
                    ctx,
                    m.channel_id,
                    format!("{reason}\n\n{}", status(&data.db, &channel, &user).await?),
                )
                .await?;
            }
        }
        Guess::Duplicate => {}
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_bounded_whole_numbers() {
        assert_eq!(number(" 1000 "), Some(1000));
        assert_eq!(number("1"), Some(1));
        for text in [
            "0",
            "1001",
            "-1",
            "1.5",
            "1e2",
            "<@1>",
            "999999999999",
            "",
            "１２",
        ] {
            assert_eq!(number(text), None)
        }
    }
    #[tokio::test]
    async fn narrowing_cooldown_replay_and_win_are_atomic() -> Result<(), Error> {
        let db = store::connect("sqlite::memory:").await?;
        seed(&db, "1").await?;
        sqlx::query("UPDATE guess_game SET target=500 WHERE channel='1'")
            .execute(&db)
            .await?;
        assert_eq!(
            play(&db, "1", "a", 1, "250", 100).await?,
            Guess::Hint {
                higher: true,
                low: 251,
                high: 1000
            }
        );
        assert_eq!(play(&db, "1", "a", 1, "250", 100).await?, Guess::Duplicate);
        assert!(matches!(
            play(&db, "1", "a", 2, "400", 103).await?,
            Guess::Rejected(_)
        ));
        assert!(matches!(
            play(&db, "1", "b", 3, "100", 104).await?,
            Guess::Rejected(_)
        ));
        assert_eq!(
            play(&db, "1", "b", 4, "750", 105).await?,
            Guess::Hint {
                higher: false,
                low: 251,
                high: 749
            }
        );
        assert_eq!(
            play(&db, "1", "a", 5, "500", 106).await?,
            Guess::Win {
                number: 500,
                attempts: 3
            }
        );
        let row = sqlx::query("SELECT * FROM guess_game WHERE channel='1'")
            .fetch_one(&db)
            .await?;
        assert_eq!(row.get::<i64, _>("lower_bound"), 1);
        assert_eq!(row.get::<i64, _>("upper_bound"), 1000);
        assert_eq!(row.get::<i64, _>("round"), 2);
        assert_eq!(row.get::<i64, _>("attempts"), 0);
        assert!((1..=1000).contains(&row.get::<i64, _>("target")));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT SUM(wins) FROM guess_game_scores")
                .fetch_one(&db)
                .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT SUM(guesses) FROM guess_game_scores")
                .fetch_one(&db)
                .await?,
            3
        );
        Ok(())
    }
    #[tokio::test]
    async fn concurrent_wins_and_restart_seed_preserve_progress() -> Result<(), Error> {
        let db = store::connect("sqlite::memory:").await?;
        seed(&db, "1").await?;
        seed(&db, "2").await?;
        sqlx::query("UPDATE guess_game SET target=500 WHERE channel='1'")
            .execute(&db)
            .await?;
        let (a, b) = tokio::join!(
            play(&db, "1", "a", 10, "500", 100),
            play(&db, "1", "a", 10, "500", 100)
        );
        assert_eq!(
            [a?, b?]
                .iter()
                .filter(|r| matches!(r, Guess::Win { .. }))
                .count(),
            1
        );
        seed(&db, "1").await?;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT round FROM guess_game WHERE channel='1'")
                .fetch_one(&db)
                .await?,
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT round FROM guess_game WHERE channel='2'")
                .fetch_one(&db)
                .await?,
            1
        );
        Ok(())
    }
}
