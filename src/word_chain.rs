use crate::{Data, Error, emoji, serenity as s, store};
use sqlx::{Row, SqlitePool};

const RULES: &str = "**One word. One connection.**\n\nStart with **violet** → your word must begin with **t**. After that, use the final letter of the latest accepted word.\n\n• Take turns: another member must play before you play again.\n• Use 2–24 English letters; no links, attachments or mentions.\n• No repeated words within a round. Spelling is a community honour rule; the bot validates letters and chain rules, not a dictionary.\n• Each accepted word earns **1 point**. Wait at least 3 seconds between your own accepted turns.\n• Incorrect entries receive a cross and never reset the chain. A new round starts after 500 accepted words.\n\n**!score** — your points and the current word\n**!leaderboard** — top ten players\n**!rules** — rules and current word\n**!reset** — staff only; starts a new round and keeps scores.\n\nGame scores and user IDs persist separately from activity statistics. Used words are kept for the current round only.";

#[derive(Debug, PartialEq)]
enum Turn {
    Accepted { turns: i64, completed: bool },
    Rejected(&'static str),
    Duplicate,
}

fn word(text: &str) -> Option<String> {
    let text = text.trim();
    (text.len() >= 2 && text.len() <= 24 && text.bytes().all(|b| b.is_ascii_alphabetic()))
        .then(|| text.to_ascii_lowercase())
}

async fn seed(db: &SqlitePool, channel: &str) -> Result<(), Error> {
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query("INSERT INTO word_chain(channel) VALUES(?) ON CONFLICT DO NOTHING")
        .bind(channel)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO word_chain_used VALUES(?,'violet') ON CONFLICT DO NOTHING")
        .bind(channel)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn accept(
    db: &SqlitePool,
    channel: &str,
    user: &str,
    message: i64,
    text: &str,
    now: i64,
) -> Result<Turn, Error> {
    let Some(word) = word(text) else {
        return Ok(Turn::Rejected("Send one word using 2–24 English letters."));
    };
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    let row = sqlx::query(
        "SELECT last_word,last_user,last_message,turns FROM word_chain WHERE channel=?",
    )
    .bind(channel)
    .fetch_one(&mut *tx)
    .await?;
    if message <= row.get::<i64, _>("last_message") {
        return Ok(Turn::Duplicate);
    }
    if row.get::<Option<String>, _>("last_user").as_deref() == Some(user) {
        return Ok(Turn::Rejected("Let another member take the next turn."));
    }
    let last: String = row.get("last_word");
    if word.as_bytes()[0] != *last.as_bytes().last().unwrap_or(&b't') {
        return Ok(Turn::Rejected(
            "Your word must start with the last letter of the latest accepted word. Use !score to see it.",
        ));
    }
    let used: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM word_chain_used WHERE channel=? AND word=?")
            .bind(channel)
            .bind(&word)
            .fetch_one(&mut *tx)
            .await?;
    if used > 0 {
        return Ok(Turn::Rejected(
            "That word was already used this round. Try another.",
        ));
    }
    let at: Option<i64> =
        sqlx::query_scalar("SELECT last_at FROM word_chain_scores WHERE channel=? AND user=?")
            .bind(channel)
            .bind(user)
            .fetch_optional(&mut *tx)
            .await?;
    if at.is_some_and(|at| now - at < 3) {
        return Ok(Turn::Rejected(
            "Wait 3 seconds between your accepted turns.",
        ));
    }
    let turns = row.get::<i64, _>("turns") + 1;
    let completed = turns >= 500;
    sqlx::query("INSERT INTO word_chain_scores VALUES(?,?,1,?) ON CONFLICT(channel,user) DO UPDATE SET points=points+1,last_at=excluded.last_at").bind(channel).bind(user).bind(now).execute(&mut *tx).await?;
    if completed {
        sqlx::query("DELETE FROM word_chain_used WHERE channel=?")
            .bind(channel)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO word_chain_used VALUES(?,'violet')")
            .bind(channel)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE word_chain SET last_word='violet',last_user=NULL,last_message=?,turns=0,round=round+1 WHERE channel=?").bind(message).bind(channel).execute(&mut *tx).await?;
    } else {
        sqlx::query("INSERT INTO word_chain_used VALUES(?,?)")
            .bind(channel)
            .bind(&word)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE word_chain SET last_word=?,last_user=?,last_message=?,turns=? WHERE channel=?",
        )
        .bind(&word)
        .bind(user)
        .bind(message)
        .bind(turns)
        .bind(channel)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Turn::Accepted { turns, completed })
}

pub(crate) async fn feedback(
    db: &SqlitePool,
    channel: &str,
    user: &str,
    now: i64,
) -> Result<bool, Error> {
    sqlx::query("DELETE FROM word_chain_feedback WHERE at<?")
        .bind(now - 86400)
        .execute(db)
        .await?;
    Ok(sqlx::query("INSERT INTO word_chain_feedback VALUES(?,?,?) ON CONFLICT(channel,user) DO UPDATE SET at=excluded.at WHERE word_chain_feedback.at<=excluded.at-12").bind(channel).bind(user).bind(now).execute(db).await?.rows_affected()>0)
}

async fn send(ctx: &s::Context, channel: s::ChannelId, text: String) -> Result<(), Error> {
    channel
        .send_message(
            &ctx.http,
            s::CreateMessage::new()
                .embed(
                    emoji::embed_for("link")
                        .title("ELVNX / WORD CHAIN")
                        .description(text),
                )
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await?;
    Ok(())
}

pub async fn refresh(ctx: &s::Context, data: &Data) -> Result<(), Error> {
    let _guard = data.configuration_lock.lock().await;
    let mut cfg = data.settings.read().await.clone();
    let Some(channel) = cfg.word_chain_channel.map(s::ChannelId::new) else {
        return Ok(());
    };
    seed(&data.db, &channel.to_string()).await?;
    let cover = crate::cards::render(
        "WORD CHAIN",
        "One word. Infinite connections.",
        "PLAY TOGETHER / ELVNX GAMES",
        &[
            ("START", "VIOLET > T".into()),
            ("TURN", "ONE MEMBER".into()),
            ("POINTS", "+1 PER WORD".into()),
        ],
        None,
    )?;
    let embed = emoji::embed_for("link")
        .title("ELVNX / WORD CHAIN")
        .description(RULES)
        .image("attachment://word-chain.png");
    if let Some(id) = cfg.word_chain_panel {
        match channel
            .edit_message(
                &ctx.http,
                s::MessageId::new(id),
                s::EditMessage::new().embed(embed.clone()).attachments(
                    s::EditAttachments::new()
                        .add(s::CreateAttachment::bytes(cover.clone(), "word-chain.png")),
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
                .add_file(s::CreateAttachment::bytes(cover, "word-chain.png"))
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await?;
    cfg.word_chain_panel = Some(m.id.get());
    store::save(&data.db, data.guild.get(), &cfg).await?;
    *data.settings.write().await = cfg;
    Ok(())
}

pub async fn message(ctx: &s::Context, data: &Data, m: &s::Message) -> Result<(), Error> {
    if m.guild_id != Some(data.guild)
        || m.author.bot
        || m.webhook_id.is_some()
        || data.settings.read().await.word_chain_channel != Some(m.channel_id.get())
    {
        return Ok(());
    }
    let channel = m.channel_id.to_string();
    let user = m.author.id.to_string();
    let now = chrono::Utc::now().timestamp();
    let command = m.content.trim().to_ascii_lowercase();
    if command.starts_with('!') {
        if !feedback(&data.db, &channel, &user, now).await? {
            return Ok(());
        }
        let text = match command.as_str() {
            "!rules" => format!("{RULES}\n\n{}", current(&data.db, &channel, &user).await?),
            "!score" => current(&data.db, &channel, &user).await?,
            "!leaderboard" => {
                let rows=sqlx::query("SELECT user,points FROM word_chain_scores WHERE channel=? ORDER BY points DESC,user ASC LIMIT 10").bind(&channel).fetch_all(&data.db).await?;
                if rows.is_empty() {
                    "No scores yet. Begin with a word starting with **t**.".into()
                } else {
                    rows.iter()
                        .enumerate()
                        .map(|(i, r)| {
                            format!(
                                "**{}.** <@{}> — **{}** points",
                                i + 1,
                                r.get::<String, _>("user"),
                                r.get::<i64, _>("points")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            }
            "!reset" => {
                let guild = data.guild.to_partial_guild(&ctx.http).await?;
                let member = data.guild.member(&ctx.http, m.author.id).await?;
                if guild.owner_id != m.author.id
                    && !guild
                        .member_permissions(&member)
                        .contains(s::Permissions::MANAGE_GUILD)
                {
                    "Only staff with Manage Server can reset the round.".into()
                } else {
                    let mut tx = data.db.begin_with("BEGIN IMMEDIATE").await?;
                    sqlx::query("DELETE FROM word_chain_used WHERE channel=?")
                        .bind(&channel)
                        .execute(&mut *tx)
                        .await?;
                    sqlx::query("INSERT INTO word_chain_used VALUES(?,'violet')")
                        .bind(&channel)
                        .execute(&mut *tx)
                        .await?;
                    sqlx::query("UPDATE word_chain SET last_word='violet',last_user=NULL,last_message=?,turns=0,round=round+1 WHERE channel=?").bind(m.id.get() as i64).bind(&channel).execute(&mut *tx).await?;
                    tx.commit().await?;
                    "New round started. Scores are kept. **Violet → t** — who is next?".into()
                }
            }
            _ => "Use **!rules**, **!score** or **!leaderboard**, or play a word.".into(),
        };
        return send(ctx, m.channel_id, text).await;
    }
    let turn = if !m.attachments.is_empty()
        || !m.sticker_items.is_empty()
        || !m.mentions.is_empty()
        || !m.mention_roles.is_empty()
        || m.mention_everyone
    {
        Turn::Rejected("Play a word only, without attachments, stickers or mentions.")
    } else {
        accept(
            &data.db,
            &channel,
            &user,
            m.id.get() as i64,
            &m.content,
            now,
        )
        .await?
    };
    match turn {
        Turn::Accepted { turns, completed } => {
            m.react(&ctx.http, emoji::reaction("check")?).await?;
            if completed {
                send(ctx,m.channel_id,"**500 words connected — round complete!** Scores are kept. A fresh round begins with **violet → t**.".into()).await?;
            } else if turns % 25 == 0 {
                send(ctx,m.channel_id,format!("**{turns} words connected.** Keep the chain going! Use !leaderboard for scores.")).await?;
            }
        }
        Turn::Rejected(reason) => {
            if feedback(&data.db, &channel, &user, now).await? {
                m.react(&ctx.http, emoji::reaction("cross")?).await?;
                send(
                    ctx,
                    m.channel_id,
                    format!("{reason}\n\n{}", current(&data.db, &channel, &user).await?),
                )
                .await?;
            }
        }
        Turn::Duplicate => {}
    }
    Ok(())
}

async fn current(db: &SqlitePool, channel: &str, user: &str) -> Result<String, Error> {
    let row = sqlx::query("SELECT last_word,turns,round FROM word_chain WHERE channel=?")
        .bind(channel)
        .fetch_one(db)
        .await?;
    let points: Option<i64> =
        sqlx::query_scalar("SELECT points FROM word_chain_scores WHERE channel=? AND user=?")
            .bind(channel)
            .bind(user)
            .fetch_optional(db)
            .await?;
    let last: String = row.get("last_word");
    Ok(format!(
        "Last accepted word: **{last}**\nNext letter: **{}** · Round **{}** · Chain **{} / 500**\nYour points: **{}**",
        last.chars().last().unwrap_or('t'),
        row.get::<i64, _>("round"),
        row.get::<i64, _>("turns"),
        points.unwrap_or(0)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_single_ascii_words() {
        assert_eq!(word("  TrEe  "), Some("tree".into()));
        for text in [
            "a",
            "two words",
            "<@1>",
            "https://a.b",
            "éclair",
            "tree!",
            "123",
            "abcdefghijklmnopqrstuvwxyz",
        ] {
            assert_eq!(word(text), None);
        }
    }
    #[tokio::test]
    async fn atomic_turns_reject_replay_repeats_and_wrong_players() -> Result<(), Error> {
        let db = store::connect("sqlite::memory:").await?;
        seed(&db, "1").await?;
        assert_eq!(
            accept(&db, "1", "a", 1, "tree", 100).await?,
            Turn::Accepted {
                turns: 1,
                completed: false
            }
        );
        assert_eq!(
            accept(&db, "1", "a", 1, "tree", 100).await?,
            Turn::Duplicate
        );
        assert!(matches!(
            accept(&db, "1", "a", 2, "eagle", 104).await?,
            Turn::Rejected(_)
        ));
        assert!(matches!(
            accept(&db, "1", "b", 3, "wrong", 104).await?,
            Turn::Rejected(_)
        ));
        accept(&db, "1", "b", 4, "east", 104).await?;
        assert!(matches!(
            accept(&db, "1", "a", 5, "tree", 108).await?,
            Turn::Rejected(_)
        ));
        let score: i64 = sqlx::query_scalar("SELECT SUM(points) FROM word_chain_scores")
            .fetch_one(&db)
            .await?;
        assert_eq!(score, 2);
        let (a, b) = tokio::join!(
            accept(&db, "1", "c", 6, "trail", 110),
            accept(&db, "1", "d", 7, "tower", 110)
        );
        assert_eq!(
            [a?, b?]
                .iter()
                .filter(|r| matches!(r, Turn::Accepted { .. }))
                .count(),
            1
        );
        seed(&db, "2").await?;
        assert!(matches!(
            accept(&db, "2", "a", 8, "tree", 110).await?,
            Turn::Accepted { turns: 1, .. }
        ));
        Ok(())
    }
    #[tokio::test]
    async fn cooldown_feedback_and_round_completion() -> Result<(), Error> {
        let db = store::connect("sqlite::memory:").await?;
        seed(&db, "1").await?;
        accept(&db, "1", "a", 1, "tree", 100).await?;
        accept(&db, "1", "b", 2, "east", 101).await?;
        assert!(matches!(
            accept(&db, "1", "a", 3, "trail", 102).await?,
            Turn::Rejected(_)
        ));
        assert!(feedback(&db, "1", "a", 100).await?);
        assert!(!feedback(&db, "1", "a", 105).await?);
        assert!(feedback(&db, "1", "a", 112).await?);
        sqlx::query("UPDATE word_chain SET turns=499 WHERE channel='1'")
            .execute(&db)
            .await?;
        assert_eq!(
            accept(&db, "1", "a", 4, "trail", 104).await?,
            Turn::Accepted {
                turns: 500,
                completed: true
            }
        );
        let row =
            sqlx::query("SELECT last_word,turns,round,last_user FROM word_chain WHERE channel='1'")
                .fetch_one(&db)
                .await?;
        assert_eq!(row.get::<String, _>("last_word"), "violet");
        assert_eq!(row.get::<i64, _>("turns"), 0);
        assert_eq!(row.get::<i64, _>("round"), 2);
        assert_eq!(row.get::<Option<String>, _>("last_user"), None);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM word_chain_used")
                .fetch_one(&db)
                .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT SUM(points) FROM word_chain_scores")
                .fetch_one(&db)
                .await?,
            3
        );
        Ok(())
    }
}
