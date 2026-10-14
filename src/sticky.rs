use crate::{Error, serenity as s};
use sqlx::{Row, SqlitePool};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::{Mutex, RwLock};

#[derive(Clone)]
struct Sticky {
    content: String,
    message: Option<s::MessageId>,
}
pub struct Service {
    db: SqlitePool,
    guild: s::GuildId,
    items: RwLock<HashMap<s::ChannelId, Sticky>>,
    pending: Mutex<HashSet<s::ChannelId>>,
    serial: Mutex<()>,
}
impl Service {
    pub async fn load(db: SqlitePool, guild: s::GuildId) -> Result<Arc<Self>, Error> {
        let rows = sqlx::query("SELECT channel,content,message FROM stickies WHERE guild=?")
            .bind(guild.to_string())
            .fetch_all(&db)
            .await?;
        let mut items = HashMap::new();
        for r in rows {
            items.insert(
                s::ChannelId::new(r.get::<String, _>("channel").parse()?),
                Sticky {
                    content: r.get("content"),
                    message: r
                        .get::<Option<String>, _>("message")
                        .map(|v| v.parse::<u64>().map(s::MessageId::new))
                        .transpose()?,
                },
            );
        }
        Ok(Arc::new(Self {
            db,
            guild,
            items: RwLock::new(items),
            pending: Mutex::new(HashSet::new()),
            serial: Mutex::new(()),
        }))
    }
    pub async fn set(
        &self,
        http: &s::Http,
        channel: s::ChannelId,
        content: String,
    ) -> Result<(), Error> {
        if content.trim().is_empty() || content.chars().count() > 1900 {
            return Err("Sticky text must be 1–1900 characters".into());
        }
        let _guard = self.serial.lock().await;
        sqlx::query("INSERT INTO stickies(channel,guild,content) VALUES(?,?,?) ON CONFLICT(channel) DO UPDATE SET content=excluded.content").bind(channel.to_string()).bind(self.guild.to_string()).bind(&content).execute(&self.db).await?;
        let previous = self
            .items
            .read()
            .await
            .get(&channel)
            .and_then(|v| v.message);
        self.items.write().await.insert(
            channel,
            Sticky {
                content,
                message: previous,
            },
        );
        self.repost(http, channel).await
    }
    async fn repost(&self, http: &s::Http, channel: s::ChannelId) -> Result<(), Error> {
        let Some(item) = self.items.read().await.get(&channel).cloned() else {
            return Ok(());
        };
        if let Some(message) = item.message
            && let Err(error) = channel.delete_message(http, message).await
        {
            // 404 means the previous sticky was already deleted; other errors must
            // stop reposting to avoid accumulating duplicate messages.
            let missing =
                matches!(&error,s::Error::Http(e) if e.status_code().map(|c|c.as_u16())==Some(404));
            if !missing {
                return Err(error.into());
            }
        }
        let message = channel
            .send_message(
                http,
                s::CreateMessage::new()
                    .embed(
                        s::CreateEmbed::new()
                            .title("ELVNX / STICKY")
                            .description(item.content)
                            .color(0x111111)
                            .footer(s::CreateEmbedFooter::new("11X • always in view")),
                    )
                    .allowed_mentions(s::CreateAllowedMentions::new()),
            )
            .await?;
        if let Some(item) = self.items.write().await.get_mut(&channel) {
            item.message = Some(message.id);
        }
        sqlx::query("UPDATE stickies SET message=? WHERE guild=? AND channel=?")
            .bind(message.id.to_string())
            .bind(self.guild.to_string())
            .bind(channel.to_string())
            .execute(&self.db)
            .await?;
        Ok(())
    }
    pub async fn remove(&self, http: &s::Http, channel: s::ChannelId) -> Result<(), Error> {
        let _guard = self.serial.lock().await;
        let item = self.items.read().await.get(&channel).cloned();
        if let Some(item) = item
            && let Some(id) = item.message
        {
            channel.delete_message(http, id).await?;
        }
        sqlx::query("DELETE FROM stickies WHERE guild=? AND channel=?")
            .bind(self.guild.to_string())
            .bind(channel.to_string())
            .execute(&self.db)
            .await?;
        self.items.write().await.remove(&channel);
        Ok(())
    }
    pub async fn queue(self: &Arc<Self>, http: Arc<s::Http>, channel: s::ChannelId) {
        if !self.items.read().await.contains_key(&channel) {
            return;
        }
        if !self.pending.lock().await.insert(channel) {
            return;
        }
        let service = Arc::clone(self);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let _guard = service.serial.lock().await;
            if let Err(error) = service.repost(&http, channel).await {
                tracing::warn!(%error,%channel,"Sticky repost failed");
            }
            service.pending.lock().await.remove(&channel);
        });
    }
}
