use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub report_channel: Option<u64>,
    pub birthday_channel: Option<u64>,
    pub music_dj_role: Option<u64>,
    pub appukuttan_legacy_channel: Option<u64>,
    pub always_react_channel: Option<u64>,
    pub afk_move_channel: Option<u64>,
    pub counting_channel: Option<u64>,
    pub counting_disabled: bool,
    pub join_to_create: Option<u64>,
    pub voice_panel: Option<u64>,
    pub activity_kind: String,
    pub activity_text: String,
    pub online_status: String,
    pub daily_quote_channel: Option<u64>,
    pub daily_insight_channel: Option<u64>,
    pub verification_role: Option<u64>,
    pub welcome_images: bool,
    pub activity_tracking: bool,
    pub antinuke: bool,
    pub honeypot_channel: Option<u64>,
    pub log_channel: Option<u64>,
    pub welcome_channel: Option<u64>,
    pub welcome_message: String,
    pub autorole: Option<u64>,
    pub ticket_category: Option<u64>,
    pub ticket_staff_role: Option<u64>,
    pub self_roles: Vec<u64>,
    pub automod: bool,
    pub block_invites: bool,
    pub blocked_words: Vec<String>,
}

pub async fn connect(url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = sqlx::sqlite::SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5));
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await?;
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS settings(guild TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS live_status(guild TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS availability(guild TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS role_panels(id INTEGER PRIMARY KEY AUTOINCREMENT,guild TEXT NOT NULL,channel TEXT NOT NULL,message TEXT,roles TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS polls(id INTEGER PRIMARY KEY AUTOINCREMENT,guild TEXT NOT NULL,owner TEXT NOT NULL,channel TEXT NOT NULL,message TEXT,question TEXT NOT NULL,options TEXT NOT NULL,due INTEGER NOT NULL,state TEXT NOT NULL DEFAULT 'open',rendered INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE IF NOT EXISTS poll_votes(poll INTEGER NOT NULL,user TEXT NOT NULL,choice INTEGER NOT NULL,PRIMARY KEY(poll,user));
         CREATE TABLE IF NOT EXISTS reminders(id INTEGER PRIMARY KEY AUTOINCREMENT,guild TEXT NOT NULL,user TEXT NOT NULL,channel TEXT,text TEXT NOT NULL,due INTEGER NOT NULL,state TEXT NOT NULL DEFAULT 'pending',created INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS reports(id INTEGER PRIMARY KEY AUTOINCREMENT,guild TEXT NOT NULL,user TEXT NOT NULL,created INTEGER NOT NULL,state TEXT NOT NULL DEFAULT 'claimed');
         CREATE TABLE IF NOT EXISTS birthdays(guild TEXT NOT NULL,user TEXT NOT NULL,month INTEGER NOT NULL,day INTEGER NOT NULL,PRIMARY KEY(guild,user));
         CREATE TABLE IF NOT EXISTS birthday_claims(guild TEXT NOT NULL,user TEXT NOT NULL,year INTEGER NOT NULL,state TEXT NOT NULL DEFAULT 'claimed',PRIMARY KEY(guild,user,year));
         CREATE TABLE IF NOT EXISTS config_backups(id INTEGER PRIMARY KEY AUTOINCREMENT,guild TEXT NOT NULL,actor TEXT NOT NULL,created INTEGER NOT NULL,value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS restore_reviews(guild TEXT NOT NULL,user TEXT NOT NULL,backup INTEGER NOT NULL,base TEXT NOT NULL,created INTEGER NOT NULL,PRIMARY KEY(guild,user,backup));
         CREATE TABLE IF NOT EXISTS playlists(guild TEXT NOT NULL,user TEXT NOT NULL,name TEXT NOT NULL,tracks TEXT NOT NULL,PRIMARY KEY(guild,user,name));
         CREATE TABLE IF NOT EXISTS cases(id INTEGER PRIMARY KEY AUTOINCREMENT, guild TEXT NOT NULL, target TEXT NOT NULL, actor TEXT NOT NULL, action TEXT NOT NULL, reason TEXT NOT NULL, created TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, revoked INTEGER NOT NULL DEFAULT 0);
         CREATE INDEX IF NOT EXISTS cases_member ON cases(guild,target,id);
         CREATE TABLE IF NOT EXISTS locks(guild TEXT NOT NULL, channel TEXT NOT NULL, had_override INTEGER NOT NULL, allow_bits TEXT NOT NULL, deny_bits TEXT NOT NULL, PRIMARY KEY(guild,channel));
         CREATE TABLE IF NOT EXISTS tickets(guild TEXT NOT NULL, owner TEXT NOT NULL, channel TEXT NOT NULL UNIQUE, closed INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(guild,owner,channel));
         CREATE UNIQUE INDEX IF NOT EXISTS one_open_ticket ON tickets(guild,owner) WHERE closed=0;
         CREATE TABLE IF NOT EXISTS stickies(channel TEXT PRIMARY KEY, guild TEXT NOT NULL, content TEXT NOT NULL, message TEXT);
         CREATE TABLE IF NOT EXISTS activity(guild TEXT NOT NULL, user TEXT NOT NULL, messages INTEGER NOT NULL DEFAULT 0, voice INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(guild,user));
         CREATE TABLE IF NOT EXISTS daily_activity(guild TEXT NOT NULL, day INTEGER NOT NULL, user TEXT NOT NULL, messages INTEGER NOT NULL DEFAULT 0, voice INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(guild,day,user));
         CREATE TABLE IF NOT EXISTS scheduled_insights(guild TEXT NOT NULL, day INTEGER NOT NULL, state TEXT NOT NULL, PRIMARY KEY(guild,day));
         CREATE TABLE IF NOT EXISTS seen_messages(id TEXT PRIMARY KEY, created INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS scheduled_quotes(guild TEXT NOT NULL, day INTEGER NOT NULL, state TEXT NOT NULL, PRIMARY KEY(guild,day));
         CREATE TABLE IF NOT EXISTS outgoing_dms(guild TEXT NOT NULL,user TEXT NOT NULL,channel TEXT NOT NULL,message TEXT PRIMARY KEY);
         CREATE TABLE IF NOT EXISTS counting(guild TEXT PRIMARY KEY,current INTEGER NOT NULL DEFAULT 0,last_user TEXT);
         CREATE TABLE IF NOT EXISTS lockdowns(guild TEXT NOT NULL,channel TEXT NOT NULL,overwrites TEXT NOT NULL,PRIMARY KEY(guild,channel));
         CREATE TABLE IF NOT EXISTS voice_rooms(guild TEXT NOT NULL,channel TEXT PRIMARY KEY,owner TEXT NOT NULL);"
    ).execute(&db).await?;
    Ok(db)
}

pub async fn settings(db: &SqlitePool, guild: u64) -> Result<Settings, crate::Error> {
    let row = sqlx::query("SELECT value FROM settings WHERE guild=?")
        .bind(guild.to_string())
        .fetch_optional(db)
        .await?;
    Ok(match row {
        Some(row) => serde_json::from_str(row.get::<&str, _>("value"))?,
        None => Settings::default(),
    })
}

pub async fn save(db: &SqlitePool, guild: u64, value: &Settings) -> Result<(), crate::Error> {
    sqlx::query("INSERT INTO settings(guild,value) VALUES(?,?) ON CONFLICT(guild) DO UPDATE SET value=excluded.value")
        .bind(guild.to_string()).bind(serde_json::to_string(value)?).execute(db).await?;
    Ok(())
}

pub async fn case(
    db: &SqlitePool,
    guild: u64,
    target: u64,
    actor: u64,
    action: &str,
    reason: &str,
) -> Result<i64, sqlx::Error> {
    Ok(
        sqlx::query("INSERT INTO cases(guild,target,actor,action,reason) VALUES(?,?,?,?,?)")
            .bind(guild.to_string())
            .bind(target.to_string())
            .bind(actor.to_string())
            .bind(action)
            .bind(reason)
            .execute(db)
            .await?
            .last_insert_rowid(),
    )
}

pub async fn accept_count(
    db: &SqlitePool,
    guild: u64,
    user: u64,
    value: i64,
) -> Result<bool, sqlx::Error> {
    if value <= 0 {
        return Ok(false);
    }
    Ok(sqlx::query("UPDATE counting SET current=?,last_user=? WHERE guild=? AND current=? AND (last_user IS NULL OR last_user!=?)").bind(value).bind(user.to_string()).bind(guild.to_string()).bind(value-1).bind(user.to_string()).execute(db).await?.rows_affected()>0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn counting_toggle_preserves_channel_progress_and_old_config() {
        let db = connect("sqlite::memory:").await.unwrap();
        let mut config: Settings = serde_json::from_str(r#"{"counting_channel":123}"#).unwrap();
        assert!(!config.counting_disabled);
        sqlx::query("INSERT INTO counting VALUES('1',7,'10')")
            .execute(&db)
            .await
            .unwrap();
        config.counting_disabled = true;
        save(&db, 1, &config).await.unwrap();
        let mut restored = settings(&db, 1).await.unwrap();
        assert!(restored.counting_disabled);
        assert_eq!(restored.counting_channel, Some(123));
        restored.counting_disabled = false;
        save(&db, 1, &restored).await.unwrap();
        assert!(!settings(&db, 1).await.unwrap().counting_disabled);
        assert!(!accept_count(&db, 1, 10, 8).await.unwrap());
        assert!(accept_count(&db, 1, 11, 8).await.unwrap());
    }
    #[tokio::test]
    async fn counting_enforces_sequence_alternation_and_guild_isolation() {
        let db = connect("sqlite::memory:").await.unwrap();
        sqlx::query("INSERT INTO counting(guild) VALUES('1')")
            .execute(&db)
            .await
            .unwrap();
        assert!(!accept_count(&db, 1, 10, 2).await.unwrap());
        assert!(accept_count(&db, 1, 10, 1).await.unwrap());
        assert!(!accept_count(&db, 1, 10, 2).await.unwrap());
        assert!(!accept_count(&db, 2, 11, 2).await.unwrap());
        let (a, b) = tokio::join!(accept_count(&db, 1, 11, 2), accept_count(&db, 1, 12, 2));
        assert_ne!(a.unwrap(), b.unwrap());
        assert!(!accept_count(&db, 1, 13, 0).await.unwrap());
    }
    #[tokio::test]
    async fn settings_and_cases_are_scoped_and_persistent() {
        let db = connect("sqlite::memory:").await.unwrap();
        let config = Settings {
            automod: true,
            self_roles: vec![123],
            ..Default::default()
        };
        save(&db, 1, &config).await.unwrap();
        assert!(settings(&db, 1).await.unwrap().automod);
        assert!(!settings(&db, 2).await.unwrap().automod);
        case(&db, 1, 42, 7, "warn", "test").await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cases WHERE guild=?")
            .bind("2")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count, 0);
        sqlx::query("INSERT INTO tickets VALUES('1','42','100',0)")
            .execute(&db)
            .await
            .unwrap();
        assert!(
            sqlx::query("INSERT INTO tickets VALUES('1','42','101',0)")
                .execute(&db)
                .await
                .is_err()
        );
    }
}
