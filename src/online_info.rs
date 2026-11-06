//! Public, role-scoped lifecycle notices. Never sweep channel history.
use crate::{Error, cards, serenity as s};
use base64::Engine;
use sqlx::SqlitePool;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Online,
    Restarting,
}
impl Phase {
    fn key(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Restarting => "restarting",
        }
    }
    fn heading(self) -> &'static str {
        match self {
            Self::Online => "ONLINE",
            Self::Restarting => "RESTARTING",
        }
    }
    fn detail(self) -> &'static str {
        match self {
            Self::Online => "Discord bot connected. Commands are ready.",
            Self::Restarting => "Bot restarting for maintenance. Please stand by.",
        }
    }
}

#[derive(Default)]
struct Runtime {
    announced_online: bool,
    stopping: bool,
}

pub struct Service {
    db: SqlitePool,
    guild: s::GuildId,
    destination: Option<(s::ChannelId, s::RoleId)>,
    runtime: Mutex<Runtime>,
}

fn snowflake(raw: &str) -> Result<u64, Error> {
    if raw.is_empty() || raw.len() > 20 || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return Err(crate::errors::user(
            "Online-info IDs must be Discord snowflakes.",
        ));
    }
    let id = raw.parse::<u64>()?;
    if id == 0 {
        return Err(crate::errors::user("Online-info IDs cannot be zero."));
    }
    Ok(id)
}

pub fn allowed_mentions(role: s::RoleId) -> s::CreateAllowedMentions {
    s::CreateAllowedMentions::new()
        .everyone(false)
        .all_users(false)
        .all_roles(false)
        .roles([role])
        .replied_user(false)
}

impl Service {
    pub fn load(db: SqlitePool, guild: s::GuildId) -> Result<Arc<Self>, Error> {
        let destination = match (
            std::env::var("ONLINE_INFO_CHANNEL_ID"),
            std::env::var("ONLINE_INFO_ROLE_ID"),
        ) {
            (Err(_), Err(_)) => None,
            (Ok(channel), Ok(role)) if channel.is_empty() && role.is_empty() => None,
            (Ok(channel), Ok(role)) => {
                let role = snowflake(&role)?;
                if role == guild.get() {
                    return Err(crate::errors::user("Online-info cannot mention @everyone."));
                }
                Some((
                    s::ChannelId::new(snowflake(&channel)?),
                    s::RoleId::new(role),
                ))
            }
            _ => {
                return Err(crate::errors::user(
                    "Configure both online-info channel and Legacy role IDs.",
                ));
            }
        };
        Ok(Arc::new(Self {
            db,
            guild,
            destination,
            runtime: Mutex::new(Runtime::default()),
        }))
    }

    pub fn start(self: &Arc<Self>, http: Arc<s::Http>) {
        if self.destination.is_none() {
            return;
        }
        let service = Arc::clone(self);
        tokio::spawn(async move {
            for attempt in 1..=3 {
                match service.online(&http).await {
                    Ok(()) => break,
                    Err(error) => {
                        tracing::warn!(%error, attempt, "Online notice could not be published")
                    }
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                if let Err(error) = service.cleanup(&http).await {
                    tracing::warn!(%error, "Old lifecycle notice cleanup will retry");
                }
            }
        });
    }

    async fn online(&self, http: &s::Http) -> Result<(), Error> {
        let mut runtime = self.runtime.lock().await;
        if runtime.announced_online || runtime.stopping {
            return Ok(());
        }
        self.publish(http, Phase::Online).await?;
        runtime.announced_online = true;
        Ok(())
    }

    pub async fn restarting(&self, http: &s::Http) -> Result<(), Error> {
        let mut runtime = self.runtime.lock().await;
        if runtime.stopping {
            return Ok(());
        }
        runtime.stopping = true;
        self.publish(http, Phase::Restarting).await
    }

    async fn publish(&self, http: &s::Http, phase: Phase) -> Result<(), Error> {
        let Some((channel, role)) = self.destination else {
            return Ok(());
        };
        // Separate maintenance helper and signal handler must not ping twice.
        let old: Option<(String, String)> =
            sqlx::query_as("SELECT channel,phase FROM online_info_messages WHERE guild=?")
                .bind(self.guild.to_string())
                .fetch_optional(&self.db)
                .await?;
        if phase == Phase::Restarting
            && old.as_ref().is_some_and(|(old_channel, old_phase)| {
                old_channel == &channel.to_string() && old_phase == phase.key()
            })
        {
            return Ok(());
        }
        let fresh =
            channel.to_channel(http).await?.guild().ok_or_else(|| {
                crate::errors::user("Online-info requires a server text channel.")
            })?;
        let guild = self.guild.to_partial_guild(http).await?;
        if fresh.guild_id != self.guild
            || fresh.kind != s::ChannelType::Text
            || !guild.roles.contains_key(&role)
        {
            return Err(crate::errors::user(
                "Online-info channel and role must belong to this server.",
            ));
        }
        let bot = http.get_current_user().await?;
        let member = self.guild.member(http, bot.id).await?;
        let permissions = guild.user_permissions_in(&fresh, &member);
        if !permissions.contains(
            s::Permissions::VIEW_CHANNEL
                | s::Permissions::SEND_MESSAGES
                | s::Permissions::EMBED_LINKS
                | s::Permissions::ATTACH_FILES
                | s::Permissions::READ_MESSAGE_HISTORY,
        ) {
            return Err(crate::errors::user(
                "Online-info needs bot View, Send, Embed, Attach and Read History permissions.",
            ));
        }
        if !guild.roles[&role].mentionable
            && !permissions.contains(s::Permissions::MENTION_EVERYONE)
        {
            return Err(crate::errors::user(
                "The bot cannot notify the configured Legacy role in online-info.",
            ));
        }
        let at = chrono::Utc::now();
        let image =
            tokio::task::spawn_blocking(move || render_card(phase, at, env!("ELVNX_RELEASE")))
                .await??;
        let message = channel
            .send_message(
                http,
                s::CreateMessage::new()
                    .content(format!("<@&{role}>"))
                    .allowed_mentions(allowed_mentions(role))
                    .embed(
                        s::CreateEmbed::new()
                            .title(format!("ELVNX / {}", phase.heading()))
                            .description(phase.detail())
                            .color(0x8b35ff)
                            .image("attachment://elvnx-status.png")
                            .footer(s::CreateEmbedFooter::new(
                                "ELVNX / 11X • avrxt.dev • Bot status",
                            ))
                            .timestamp(s::Timestamp::now()),
                    )
                    .add_file(s::CreateAttachment::bytes(image, "elvnx-status.png")),
            )
            .await?;
        if let Err(error) = remember(&self.db, self.guild, channel, message.id, phase).await {
            // Keep the old tracked notice if durable replacement failed.
            if let Err(cleanup_error) = channel.delete_message(http, message.id).await {
                tracing::error!(%cleanup_error, message = %message.id, "Untracked lifecycle notice needs staff cleanup");
            }
            return Err(error);
        }
        if let Err(error) = self.cleanup(http).await {
            tracing::warn!(%error, "New lifecycle notice is saved; old-message cleanup is queued");
        }
        tracing::info!(phase = phase.key(), "Public lifecycle notice published");
        Ok(())
    }

    async fn cleanup(&self, http: &s::Http) -> Result<(), Error> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT channel,message FROM online_info_cleanup WHERE guild=? LIMIT 10",
        )
        .bind(self.guild.to_string())
        .fetch_all(&self.db)
        .await?;
        if rows.is_empty() {
            return Ok(());
        }
        let bot = http.get_current_user().await?.id;
        for (channel, message) in rows {
            let id = s::ChannelId::new(snowflake(&channel)?);
            let message_id = s::MessageId::new(snowflake(&message)?);
            let fresh = match id.to_channel(http).await {
                Ok(channel) => channel.guild().ok_or_else(|| {
                    crate::errors::user("Lifecycle cleanup requires a guild channel.")
                })?,
                Err(s::Error::Http(error))
                    if error
                        .status_code()
                        .is_some_and(|status| status.as_u16() == 404) =>
                {
                    sqlx::query(
                        "DELETE FROM online_info_cleanup WHERE guild=? AND channel=? AND message=?",
                    )
                    .bind(self.guild.to_string())
                    .bind(channel)
                    .bind(message)
                    .execute(&self.db)
                    .await?;
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            if fresh.guild_id != self.guild {
                return Err(crate::errors::user(
                    "Lifecycle cleanup refused a different server.",
                ));
            }
            match id.message(http, message_id).await {
                Ok(old) if old.author.id == bot => {
                    match id.delete_message(http, message_id).await {
                        Ok(()) => {}
                        Err(s::Error::Http(error))
                            if error
                                .status_code()
                                .is_some_and(|status| status.as_u16() == 404) => {}
                        Err(error) => return Err(error.into()),
                    }
                }
                Ok(_) => {
                    return Err(crate::errors::user(
                        "Lifecycle cleanup refused a message not owned by this bot.",
                    ));
                }
                Err(s::Error::Http(error))
                    if error
                        .status_code()
                        .is_some_and(|status| status.as_u16() == 404) => {}
                Err(error) => return Err(error.into()),
            }
            sqlx::query(
                "DELETE FROM online_info_cleanup WHERE guild=? AND channel=? AND message=?",
            )
            .bind(self.guild.to_string())
            .bind(channel)
            .bind(message)
            .execute(&self.db)
            .await?;
        }
        Ok(())
    }
}

async fn remember(
    db: &SqlitePool,
    guild: s::GuildId,
    channel: s::ChannelId,
    message: s::MessageId,
    phase: Phase,
) -> Result<(), Error> {
    let mut transaction = db.begin().await?;
    let old: Option<(String, String)> =
        sqlx::query_as("SELECT channel,message FROM online_info_messages WHERE guild=?")
            .bind(guild.to_string())
            .fetch_optional(&mut *transaction)
            .await?;
    sqlx::query("INSERT INTO online_info_messages(guild,channel,message,phase) VALUES(?,?,?,?) ON CONFLICT(guild) DO UPDATE SET channel=excluded.channel,message=excluded.message,phase=excluded.phase")
        .bind(guild.to_string()).bind(channel.to_string()).bind(message.to_string()).bind(phase.key()).execute(&mut *transaction).await?;
    if let Some((old_channel, old_message)) = old
        && (old_channel != channel.to_string() || old_message != message.to_string())
    {
        sqlx::query(
            "INSERT OR IGNORE INTO online_info_cleanup(guild,channel,message) VALUES(?,?,?)",
        )
        .bind(guild.to_string())
        .bind(old_channel)
        .bind(old_message)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

pub fn render_card(
    phase: Phase,
    at: chrono::DateTime<chrono::Utc>,
    release: &str,
) -> Result<Vec<u8>, Error> {
    let background = base64::engine::general_purpose::STANDARD.encode(include_bytes!(
        "../assets/status/elvnx-purple-status-background.png"
    ));
    let stamp = at
        .with_timezone(&chrono::FixedOffset::east_opt(19800).expect("valid IST offset"))
        .format("%d %b %Y  %I:%M %p IST")
        .to_string();
    let size = if phase == Phase::Online { 210 } else { 152 };
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="1600" height="720">
<defs><linearGradient id="silver" x2="0" y2="1"><stop stop-color="#fff"/><stop offset=".45" stop-color="#f4f2ff"/><stop offset=".52" stop-color="#a8a1bc"/><stop offset="1" stop-color="#fff"/></linearGradient><linearGradient id="violet" x2="0" y2="1"><stop stop-color="#f1ddff"/><stop offset=".38" stop-color="#ba7bff"/><stop offset=".7" stop-color="#8d39ed"/><stop offset="1" stop-color="#5522b8"/></linearGradient></defs>
<image width="1600" height="720" preserveAspectRatio="xMidYMid slice" xlink:href="data:image/png;base64,{background}"/>
<rect x="95" y="115" width="1085" height="550" rx="25" fill="#05030c" opacity=".22"/>
<text x="154" y="135" fill="#d4bcff" font-family="Barlow Condensed" font-style="italic" font-weight="800" font-size="39" letter-spacing="3">11X / ELVNX</text>
<text x="155" y="168" fill="#bfb2d9" font-family="IBM Plex Mono" font-size="13" letter-spacing="4">COMMUNITY STATUS</text>
<g font-family="Barlow Condensed" font-style="italic" font-weight="800"><text x="145" y="317" fill="#160a2b" stroke="#160a2b" stroke-width="9" font-size="174">ELVNX</text><text x="141" y="309" fill="url(#silver)" stroke="#d6caea" stroke-width=".8" font-size="174">ELVNX</text>
<text x="138" y="476" fill="#220d42" stroke="#220d42" stroke-width="10" font-size="{size}">{heading}</text><text x="134" y="468" fill="url(#violet)" stroke="#c79aff" stroke-width="1.1" font-size="{size}">{heading}</text></g>
<path d="M153 499H1070" stroke="#6a32b8" stroke-width="2"/><path d="M153 499H378" stroke="#c093ff" stroke-width="4"/>
<text x="155" y="546" fill="#eee6ff" font-family="Space Grotesk" font-size="24">{detail}</text>
<circle cx="164" cy="599" r="6" fill="#bd94ff"/><text x="183" y="605" fill="#d8c2ff" font-family="IBM Plex Mono" font-size="16">{stamp}</text>
<text x="155" y="640" fill="#a99bbd" font-family="IBM Plex Mono" font-size="13">{release} / AVRXT.DEV</text>
<path d="M1010 169l19-29h11l-19 29zm23 0l19-29h11l-19 29zm23 0l19-29h11l-19 29z" fill="#a568fa"/>
</svg>"##,
        heading = phase.heading(),
        detail = cards::xml(phase.detail()),
        release = cards::xml(release)
    );
    cards::render_svg(&svg, 1600, 720)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mentions_are_limited_to_the_configured_role() {
        let value = serde_json::to_value(allowed_mentions(s::RoleId::new(42))).unwrap();
        assert_eq!(value["parse"], serde_json::json!([]));
        assert_eq!(value["roles"], serde_json::json!(["42"]));
        assert_eq!(value["users"], serde_json::json!([]));
        assert_eq!(value["replied_user"], false);
        for invalid in ["0", "", "-1", "1/2", "18446744073709551616"] {
            assert!(snowflake(invalid).is_err());
        }
    }
    #[tokio::test]
    async fn replacement_durably_queues_only_the_previous_tracked_notice() {
        let db = crate::store::connect("sqlite::memory:").await.unwrap();
        let guild = s::GuildId::new(1);
        remember(
            &db,
            guild,
            s::ChannelId::new(2),
            s::MessageId::new(3),
            Phase::Online,
        )
        .await
        .unwrap();
        remember(
            &db,
            guild,
            s::ChannelId::new(2),
            s::MessageId::new(4),
            Phase::Restarting,
        )
        .await
        .unwrap();
        remember(
            &db,
            guild,
            s::ChannelId::new(2),
            s::MessageId::new(4),
            Phase::Restarting,
        )
        .await
        .unwrap();
        let pending: Vec<(String, String)> =
            sqlx::query_as("SELECT channel,message FROM online_info_cleanup")
                .fetch_all(&db)
                .await
                .unwrap();
        assert_eq!(pending, vec![("2".into(), "3".into())]);
        let current: (String, String) =
            sqlx::query_as("SELECT message,phase FROM online_info_messages WHERE guild='1'")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(current, ("4".into(), "restarting".into()));
        remember(
            &db,
            s::GuildId::new(5),
            s::ChannelId::new(6),
            s::MessageId::new(7),
            Phase::Online,
        )
        .await
        .unwrap();
        let other: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM online_info_cleanup WHERE guild='5'")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(other, 0);
    }
    #[tokio::test]
    async fn failed_replacement_does_not_discard_the_current_notice() {
        let db = crate::store::connect("sqlite::memory:").await.unwrap();
        let guild = s::GuildId::new(1);
        remember(
            &db,
            guild,
            s::ChannelId::new(2),
            s::MessageId::new(3),
            Phase::Online,
        )
        .await
        .unwrap();
        sqlx::query("CREATE TRIGGER reject_cleanup BEFORE INSERT ON online_info_cleanup BEGIN SELECT RAISE(ABORT,'test rejection'); END;").execute(&db).await.unwrap();
        assert!(
            remember(
                &db,
                guild,
                s::ChannelId::new(2),
                s::MessageId::new(4),
                Phase::Restarting
            )
            .await
            .is_err()
        );
        let current: String =
            sqlx::query_scalar("SELECT message FROM online_info_messages WHERE guild='1'")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(current, "3");
    }
    #[test]
    fn both_dynamic_status_cards_render_as_distinct_pngs() {
        let at = chrono::DateTime::from_timestamp(1792080000, 0).unwrap();
        let online = render_card(Phase::Online, at, "0.10.0+build.preview").unwrap();
        let restart = render_card(Phase::Restarting, at, "0.10.0+build.preview").unwrap();
        assert_eq!(&online[..8], b"\x89PNG\r\n\x1a\n");
        assert_ne!(online, restart);
        assert!(online.len() < 8 * 1024 * 1024);
    }
}
