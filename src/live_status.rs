use crate::{Context, Error, reply, serenity as s, store};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Config {
    enabled: bool,
    category: Option<u64>,
    bot_channel: Option<u64>,
    bot_message: Option<u64>,
    music_channel: Option<u64>,
    music_message: Option<u64>,
}
pub struct Service {
    ctx: s::Context,
    db: SqlitePool,
    guild: s::GuildId,
    manager: Arc<s::ShardManager>,
    music: Option<Arc<crate::music::Service>>,
    started: Instant,
    config: Mutex<Config>,
    operations: Mutex<()>,
}
async fn load(db: &SqlitePool, guild: s::GuildId) -> Result<Config, Error> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM live_status WHERE guild=?")
        .bind(guild.to_string())
        .fetch_optional(db)
        .await?;
    Ok(value
        .map(|v| serde_json::from_str(&v))
        .transpose()?
        .unwrap_or_default())
}
impl Service {
    pub async fn start(
        ctx: s::Context,
        db: SqlitePool,
        guild: s::GuildId,
        manager: Arc<s::ShardManager>,
        music: Option<Arc<crate::music::Service>>,
        started: Instant,
    ) -> Result<Arc<Self>, Error> {
        let config = load(&db, guild).await?;
        let service = Arc::new(Self {
            ctx,
            db,
            guild,
            manager,
            music,
            started,
            config: Mutex::new(config),
            operations: Mutex::new(()),
        });
        let worker = Arc::clone(&service);
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(Duration::from_secs(30));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                timer.tick().await;
                let _guard = worker.operations.lock().await;
                let mut cfg = worker.config.lock().await.clone();
                if cfg.enabled
                    && let Err(error) = worker.refresh(&mut cfg).await
                {
                    tracing::warn!(%error, "Live status refresh failed; check tracked channels and permissions");
                }
            }
        });
        Ok(service)
    }
    async fn save(&self, cfg: &Config) -> Result<(), Error> {
        // Retain new IDs in memory even if a temporary database failure prevents persistence.
        *self.config.lock().await = cfg.clone();
        sqlx::query("INSERT INTO live_status VALUES(?,?) ON CONFLICT(guild) DO UPDATE SET value=excluded.value")
            .bind(self.guild.to_string()).bind(serde_json::to_string(cfg)?).execute(&self.db).await?;
        Ok(())
    }
    async fn channel(
        &self,
        id: Option<u64>,
        kind: s::ChannelType,
    ) -> Result<Option<s::GuildChannel>, Error> {
        let Some(id) = id else {
            return Ok(None);
        };
        match s::ChannelId::new(id).to_channel(&self.ctx.http).await {
            Ok(channel) => {
                let channel = channel.guild().ok_or_else(|| {
                    crate::errors::user("Stored status channel is not a guild channel")
                })?;
                if channel.guild_id != self.guild || channel.kind != kind {
                    return Err(crate::errors::user(
                        "Stored status channel has the wrong server/type",
                    ));
                }
                Ok(Some(channel))
            }
            Err(s::Error::Http(error))
                if error
                    .status_code()
                    .is_some_and(|status| status.as_u16() == 404) =>
            {
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }
    async fn setup(&self, enabled: bool) -> Result<Config, Error> {
        let _guard = self.operations.lock().await;
        let mut cfg = self.config.lock().await.clone();
        if !enabled {
            cfg.enabled = false;
            self.save(&cfg).await?;
            self.refresh(&mut cfg).await?;
            return Ok(cfg);
        }
        // Check durable storage before creating Discord resources.
        self.save(&cfg).await?;
        let category =
            if let Some(channel) = self.channel(cfg.category, s::ChannelType::Category).await? {
                channel
            } else {
                let bot_id = self.ctx.cache.current_user().id;
                let permissions = vec![
                    s::PermissionOverwrite {
                        kind: s::PermissionOverwriteType::Role(s::RoleId::new(self.guild.get())),
                        allow: s::Permissions::READ_MESSAGE_HISTORY,
                        deny: s::Permissions::SEND_MESSAGES
                            | s::Permissions::SEND_MESSAGES_IN_THREADS
                            | s::Permissions::CREATE_PUBLIC_THREADS
                            | s::Permissions::CREATE_PRIVATE_THREADS
                            | s::Permissions::ADD_REACTIONS,
                    },
                    s::PermissionOverwrite {
                        kind: s::PermissionOverwriteType::Member(bot_id),
                        allow: s::Permissions::VIEW_CHANNEL
                            | s::Permissions::READ_MESSAGE_HISTORY
                            | s::Permissions::SEND_MESSAGES
                            | s::Permissions::EMBED_LINKS
                            | s::Permissions::ATTACH_FILES,
                        deny: s::Permissions::empty(),
                    },
                ];
                let category = self
                    .guild
                    .create_channel(
                        &self.ctx.http,
                        s::CreateChannel::new("ELVNX BOT STATUS")
                            .kind(s::ChannelType::Category)
                            .permissions(permissions),
                    )
                    .await?;
                cfg.category = Some(category.id.get());
                self.save(&cfg).await?;
                category
            };
        for music in [false, true] {
            let old = if music {
                cfg.music_channel
            } else {
                cfg.bot_channel
            };
            if let Some(channel) = self.channel(old, s::ChannelType::Text).await? {
                if channel.parent_id != Some(category.id) {
                    channel
                        .id
                        .edit(&self.ctx.http, s::EditChannel::new().category(category.id))
                        .await?;
                }
            } else {
                let channel = self.guild.create_channel(&self.ctx.http, s::CreateChannel::new(if music { "lavlink-status" } else { "elvnx-status" })
                    .kind(s::ChannelType::Text).category(category.id).permissions(category.permission_overwrites.clone())
                    .topic("ELVNX live status · refreshes every 30 seconds · check the embed timestamp for freshness")).await?;
                if music {
                    cfg.music_channel = Some(channel.id.get());
                    cfg.music_message = None;
                } else {
                    cfg.bot_channel = Some(channel.id.get());
                    cfg.bot_message = None;
                }
                self.save(&cfg).await?;
            }
        }
        cfg.enabled = true;
        self.save(&cfg).await?;
        self.refresh(&mut cfg).await?;
        Ok(cfg)
    }
    pub async fn set_enabled(&self, enabled: bool) -> Result<(), Error> {
        self.setup(enabled).await?;
        Ok(())
    }
    async fn bot_embed(&self) -> s::CreateEmbed {
        let bot = self.ctx.cache.current_user().clone();
        let guild = self
            .ctx
            .cache
            .guild(self.guild)
            .map(|g| (g.name.clone(), g.member_count));
        let runners = self.manager.runners.lock().await;
        let runner = runners.get(&self.ctx.shard_id);
        let connected = runner.is_some_and(|runner| runner.stage == s::ConnectionStage::Connected);
        let latency = runner
            .and_then(|r| r.latency)
            .map(|latency| format!("{} ms", latency.as_millis()))
            .unwrap_or_else(|| "Awaiting heartbeat".into());
        drop(runners);
        let started = Instant::now();
        let db_ok = sqlx::query_scalar::<_, i64>("SELECT 1")
            .fetch_one(&self.db)
            .await
            .is_ok();
        let db_ms = started.elapsed().as_millis();
        let seconds = self.started.elapsed().as_secs();
        let mut embed = crate::emoji::embed()
            .title("ELVNX / BOT STATUS")
            .color(if connected && db_ok {
                0x111111
            } else {
                0xbb3333
            })
            .description(if connected && db_ok {
                format!(
                    "{} Online · gateway and database responding",
                    crate::emoji::text("online")
                )
            } else {
                format!(
                    "{} Degraded · check gateway/database health",
                    crate::emoji::text("warning")
                )
            })
            .field("Bot", format!("{} · `{}`", bot.name, bot.id), false)
            .field(
                "Process uptime",
                format!(
                    "{}d {}h {}m {}s",
                    seconds / 86400,
                    seconds % 86400 / 3600,
                    seconds % 3600 / 60,
                    seconds % 60
                ),
                true,
            )
            .field("Gateway heartbeat", latency, true)
            .field(
                "Registered commands",
                crate::music::COMMAND_COUNT.to_string(),
                true,
            )
            .field("Bot version", env!("CARGO_PKG_VERSION"), true)
            .field(
                "Database",
                if db_ok {
                    format!("Healthy · {db_ms} ms")
                } else {
                    "Unavailable".into()
                },
                true,
            )
            .field(
                "Server",
                guild
                    .as_ref()
                    .map(|g| g.0.clone())
                    .unwrap_or_else(|| "Cache unavailable".into()),
                false,
            )
            .field(
                "Server members",
                guild
                    .map(|g| g.1.to_string())
                    .unwrap_or_else(|| "Unavailable".into()),
                true,
            )
            .timestamp(s::Timestamp::now())
            .footer(s::CreateEmbedFooter::new(
                "30s refresh • stale timestamp may mean bot/updater is offline",
            ));
        if let Some(avatar) = bot.avatar_url() {
            embed = embed.thumbnail(avatar);
        }
        if let Ok(settings) = store::settings(&self.db, self.guild.get()).await {
            embed = embed.field(
                "Features",
                format!(
                    "AutoMod: {} · Activity tracking: {}",
                    if settings.automod { "On" } else { "Off" },
                    if settings.activity_tracking {
                        "On"
                    } else {
                        "Off"
                    }
                ),
                false,
            );
        }
        embed
    }
    async fn edit_board(
        &self,
        channel: s::ChannelId,
        id: u64,
        embed: &s::CreateEmbed,
    ) -> Result<bool, Error> {
        match channel
            .edit_message(
                &self.ctx.http,
                s::MessageId::new(id),
                s::EditMessage::new()
                    .embed(embed.clone())
                    .allowed_mentions(s::CreateAllowedMentions::new()),
            )
            .await
        {
            Ok(_) => Ok(true),
            Err(s::Error::Http(error))
                if error
                    .status_code()
                    .is_some_and(|status| status.as_u16() == 404) =>
            {
                Ok(false)
            }
            Err(error) => Err(error.into()),
        }
    }
    async fn publish(
        &self,
        channel: u64,
        old: Option<u64>,
        embed: s::CreateEmbed,
        title: &str,
    ) -> Result<u64, Error> {
        let channel = self
            .channel(Some(channel), s::ChannelType::Text)
            .await?
            .ok_or_else(|| {
                crate::errors::user("Status channel deleted; rerun /bot-status-live to repair it")
            })?;
        if let Some(id) = old
            && self.edit_board(channel.id, id, &embed).await?
        {
            return Ok(id);
        }
        let bot = self.ctx.cache.current_user().id;
        // Recover an owned board after an interrupted ID save or a stale saved ID.
        let recovered = channel
            .id
            .messages(&self.ctx.http, s::GetMessages::new().limit(20))
            .await?
            .into_iter()
            .find(|message| {
                message.author.id == bot
                    && message.webhook_id.is_none()
                    && message
                        .embeds
                        .first()
                        .and_then(|embed| embed.title.as_deref())
                        == Some(title)
            });
        if let Some(message) = recovered
            && self
                .edit_board(channel.id, message.id.get(), &embed)
                .await?
        {
            return Ok(message.id.get());
        }
        let message = channel
            .id
            .send_message(
                &self.ctx.http,
                s::CreateMessage::new()
                    .embed(embed)
                    .allowed_mentions(s::CreateAllowedMentions::new()),
            )
            .await?;
        Ok(message.id.get())
    }
    async fn refresh(&self, cfg: &mut Config) -> Result<(), Error> {
        let mut first_error = None;
        for music in [false, true] {
            let channel = if music {
                cfg.music_channel
            } else {
                cfg.bot_channel
            };
            let Some(channel) = channel else {
                continue;
            };
            let old = if music {
                cfg.music_message
            } else {
                cfg.bot_message
            };
            let embed = if !cfg.enabled {
                crate::emoji::embed().title(if music { "ELVNX / LAVALINK STATUS" } else { "ELVNX / BOT STATUS" }).description("Live updates paused. An administrator can enable /bot-status-live to resume.").color(0x111111).timestamp(s::Timestamp::now())
            } else if music {
                if let Some(node) = &self.music {
                    node.live_embed().await
                } else {
                    crate::emoji::embed()
                        .title("ELVNX / LAVALINK STATUS")
                        .description("Music node is not configured.")
                        .color(0x111111)
                        .timestamp(s::Timestamp::now())
                }
            } else {
                self.bot_embed().await
            };
            match self
                .publish(
                    channel,
                    old,
                    embed,
                    if music {
                        "ELVNX / LAVALINK STATUS"
                    } else {
                        "ELVNX / BOT STATUS"
                    },
                )
                .await
            {
                Ok(id) if old != Some(id) => {
                    if music {
                        cfg.music_message = Some(id);
                    } else {
                        cfg.bot_message = Some(id);
                    }
                    self.save(cfg).await?;
                }
                Ok(_) => {}
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        // Retry persistence after a previous transient failure, including unchanged IDs.
        self.save(cfg).await?;
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }
}

/// Create or resume two live bot and Lavalink status channels.
#[poise::command(
    slash_command,
    guild_only,
    rename = "bot-status-live",
    required_permissions = "MANAGE_GUILD | MANAGE_CHANNELS",
    default_member_permissions = "MANAGE_GUILD | MANAGE_CHANNELS",
    required_bot_permissions = "MANAGE_CHANNELS",
    user_cooldown = 10
)]
pub async fn bot_status_live(
    ctx: Context<'_>,
    #[description = "Enable (default) or pause updates; channels remain"] enabled: Option<bool>,
) -> Result<(), Error> {
    let enabled = enabled.unwrap_or(true);
    let cfg = ctx.data().live_status.setup(enabled).await?;
    reply(ctx, if enabled { format!("Live status enabled.\nBot: <#{}>\nLavalink: <#{}>\nUpdates every 30 seconds. A stale timestamp can indicate an offline bot/updater. Reusing this command preserves the tracked channels and messages.", cfg.bot_channel.ok_or_else(|| crate::errors::user("Bot status channel missing"))?, cfg.music_channel.ok_or_else(|| crate::errors::user("Music status channel missing"))?) } else { "Live status updates paused. Channels and messages are preserved.".into() }).await
}
