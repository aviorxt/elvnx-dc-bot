use crate::{Context, Error, reply, serenity as s};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, RwLock};

const SECTIONS: [(&str, &str); 1] = [("overview", "availability-panel")];
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Board {
    key: String,
    channel: u64,
    messages: Vec<u64>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Config {
    enabled: bool,
    category: Option<u64>,
    audience: Option<u64>,
    boards: Vec<Board>,
    retired: Vec<Board>,
}
#[derive(Default)]
struct Observed {
    ready: bool,
    nonce: String,
    chunks: HashSet<u32>,
    expected: u32,
    presences: HashMap<s::UserId, s::Presence>,
    requested: Option<Instant>,
}
#[derive(Default)]
struct Snapshot {
    ready: bool,
    name: String,
    rows: [Vec<String>; 4],
}
pub struct Service {
    ctx: s::Context,
    db: SqlitePool,
    guild: s::GuildId,
    config: Mutex<Config>,
    operations: Mutex<()>,
    observed: RwLock<Observed>,
    connected: AtomicBool,
    dirty: AtomicBool,
    generation: AtomicU64,
    rendered: Mutex<HashMap<u64, (String, Instant)>>,
}
fn clean(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(max)
        .flat_map(|c| {
            if "\\`*_~|[]>".contains(c) {
                vec!['\\', c]
            } else {
                vec![c]
            }
        })
        .collect()
}
fn online(status: s::OnlineStatus) -> bool {
    matches!(
        status,
        s::OnlineStatus::Online | s::OnlineStatus::Idle | s::OnlineStatus::DoNotDisturb
    )
}
fn visible_voice(
    base: s::Permissions,
    audience: Option<s::RoleId>,
    guild: s::GuildId,
    overwrites: &[s::PermissionOverwrite],
) -> bool {
    if base.contains(s::Permissions::ADMINISTRATOR) {
        return true;
    }
    let mut permissions = base;
    for id in [Some(s::RoleId::new(guild.get())), audience]
        .into_iter()
        .flatten()
    {
        if let Some(overwrite) = overwrites
            .iter()
            .find(|o| o.kind == s::PermissionOverwriteType::Role(id))
        {
            permissions = (permissions & !overwrite.deny) | overwrite.allow;
        }
    }
    // An individual denial means visibility is not guaranteed for this audience.
    !overwrites.iter().any(|o| {
        matches!(o.kind, s::PermissionOverwriteType::Member(_))
            && o.deny.contains(s::Permissions::VIEW_CHANNEL)
    }) && permissions.contains(s::Permissions::VIEW_CHANNEL)
}
fn group_text(rows: &[String]) -> String {
    if rows.is_empty() {
        return "No members right now.".into();
    }
    let mut text = String::new();
    let mut shown = 0;
    for row in rows {
        if text.encode_utf16().count() + row.encode_utf16().count() + 1 > 900 {
            break;
        }
        text.push_str(row);
        text.push('\n');
        shown += 1;
    }
    if shown < rows.len() {
        text.push_str(&format!(
            "\n… {} more. Total count includes everyone.",
            rows.len() - shown
        ));
    }
    text
}
impl Service {
    pub async fn start(
        ctx: s::Context,
        db: SqlitePool,
        guild: s::GuildId,
    ) -> Result<Arc<Self>, Error> {
        let raw: Option<String> =
            sqlx::query_scalar("SELECT value FROM availability WHERE guild=?")
                .bind(guild.to_string())
                .fetch_optional(&db)
                .await?;
        let config = raw
            .map(|v| serde_json::from_str(&v))
            .transpose()?
            .unwrap_or_default();
        let service = Arc::new(Self {
            ctx,
            db,
            guild,
            config: Mutex::new(config),
            operations: Mutex::new(()),
            observed: RwLock::new(Observed::default()),
            connected: AtomicBool::new(true),
            dirty: AtomicBool::new(true),
            generation: AtomicU64::new(1),
            rendered: Mutex::new(HashMap::new()),
        });
        let worker = Arc::clone(&service);
        tokio::spawn(async move {
            // Wait until framework setup has returned before requesting a roster.
            let mut timer = tokio::time::interval_at(
                tokio::time::Instant::now() + Duration::from_secs(5),
                Duration::from_secs(5),
            );
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut heartbeat = Instant::now();
            let mut retry_after = Instant::now();
            loop {
                timer.tick().await;
                if Instant::now() < retry_after {
                    continue;
                }
                if worker.connected.load(Ordering::Relaxed)
                    && std::env::var("ENABLE_PRESENCE_EVENTS").as_deref() == Ok("true")
                {
                    let observed = worker.observed.read().await;
                    let needs = !observed.ready
                        && observed
                            .requested
                            .is_none_or(|t| t.elapsed() > Duration::from_secs(60));
                    drop(observed);
                    if needs && worker.ctx.cache.guild(worker.guild).is_some() {
                        worker.request_roster().await;
                    }
                }
                let dirty = worker.dirty.swap(false, Ordering::Relaxed);
                if !dirty && heartbeat.elapsed() < Duration::from_secs(60) {
                    continue;
                }
                let _guard = worker.operations.lock().await;
                let mut cfg = worker.config.lock().await.clone();
                if cfg.enabled {
                    if let Err(error) = worker.refresh(&mut cfg).await {
                        tracing::warn!(%error,"Availability refresh failed; check tracked channels and permissions");
                        worker.dirty.store(true, Ordering::Relaxed);
                        retry_after = Instant::now() + Duration::from_secs(30);
                    }
                    heartbeat = Instant::now();
                }
            }
        });
        Ok(service)
    }
    async fn save(&self, cfg: &Config) -> Result<(), Error> {
        *self.config.lock().await = cfg.clone();
        sqlx::query("INSERT INTO availability VALUES(?,?) ON CONFLICT(guild) DO UPDATE SET value=excluded.value").bind(self.guild.to_string()).bind(serde_json::to_string(cfg)?).execute(&self.db).await?;
        Ok(())
    }
    async fn request_roster(&self) {
        let mut observed = self.observed.write().await;
        // GuildCreate, connection events and the worker can arrive together.
        // Keep a single nonce in flight so valid chunks are not discarded.
        if observed
            .requested
            .is_some_and(|time| time.elapsed() < Duration::from_secs(60))
        {
            return;
        }
        let nonce = format!(
            "elvnx-av-{}",
            self.generation.fetch_add(1, Ordering::Relaxed)
        );
        observed.ready = false;
        observed.nonce = nonce.clone();
        observed.chunks.clear();
        observed.presences.clear();
        observed.expected = 0;
        observed.requested = Some(Instant::now());
        drop(observed);
        self.ctx.shard.chunk_guild(
            self.guild,
            None,
            true,
            s::ChunkGuildFilter::None,
            Some(nonce),
        );
        self.dirty.store(true, Ordering::Relaxed);
    }
    pub async fn observe(&self, event: &s::FullEvent) {
        match event {
            s::FullEvent::GuildCreate { guild, .. } if guild.id == self.guild => {
                self.observed.write().await.presences = guild.presences.clone();
                if std::env::var("ENABLE_PRESENCE_EVENTS").as_deref() == Ok("true") {
                    self.request_roster().await;
                }
            }
            s::FullEvent::GuildMembersChunk { chunk } if chunk.guild_id == self.guild => {
                let mut observed = self.observed.write().await;
                if chunk.nonce.as_deref() == Some(observed.nonce.as_str()) {
                    let was_ready = observed.ready;
                    for presence in chunk.presences.iter().flatten() {
                        observed
                            .presences
                            .insert(presence.user.id, presence.clone());
                    }
                    observed.chunks.insert(chunk.chunk_index);
                    observed.expected = chunk.chunk_count;
                    observed.ready = observed.expected > 0
                        && observed.chunks.len() == observed.expected as usize;
                    if observed.ready && !was_ready {
                        tracing::info!(
                            presence_count = observed.presences.len(),
                            chunks = observed.expected,
                            "Availability member roster synchronized"
                        );
                    }
                }
            }
            s::FullEvent::PresenceUpdate { new_data } if new_data.guild_id == Some(self.guild) => {
                self.observed
                    .write()
                    .await
                    .presences
                    .insert(new_data.user.id, new_data.clone());
            }
            s::FullEvent::GuildMemberRemoval { guild_id, user, .. } if *guild_id == self.guild => {
                self.observed.write().await.presences.remove(&user.id);
            }
            s::FullEvent::ShardStageUpdate { event } => {
                let connected = event.new == s::ConnectionStage::Connected;
                self.connected.store(connected, Ordering::Relaxed);
                if !connected {
                    let mut observed = self.observed.write().await;
                    observed.ready = false;
                    observed.requested = None;
                } else if std::env::var("ENABLE_PRESENCE_EVENTS").as_deref() == Ok("true")
                    && self.ctx.cache.guild(self.guild).is_some()
                {
                    self.request_roster().await;
                }
            }
            s::FullEvent::VoiceStateUpdate { new, .. } if new.guild_id == Some(self.guild) => {}
            s::FullEvent::GuildMemberAddition { new_member }
                if new_member.guild_id == self.guild => {}
            s::FullEvent::GuildMemberUpdate { event, .. } if event.guild_id == self.guild => {}
            s::FullEvent::ChannelUpdate { new, .. } if new.guild_id == self.guild => {}
            s::FullEvent::ChannelDelete { channel, .. } if channel.guild_id == self.guild => {}
            s::FullEvent::GuildRoleUpdate { new, .. } if new.guild_id == self.guild => {}
            _ => return,
        }
        self.dirty.store(true, Ordering::Relaxed);
    }
    /// Fail closed during gateway outages, roster reloads and guild unavailability.
    pub async fn api_snapshot(
        &self,
        id: s::UserId,
    ) -> Result<(s::User, Option<s::Presence>), axum::http::StatusCode> {
        let observed = self.observed.read().await;
        if !observed.ready || !self.connected.load(Ordering::Relaxed) {
            return Err(axum::http::StatusCode::SERVICE_UNAVAILABLE);
        }
        let guild = self
            .ctx
            .cache
            .guild(self.guild)
            .ok_or(axum::http::StatusCode::SERVICE_UNAVAILABLE)?;
        let member = guild
            .members
            .get(&id)
            .filter(|m| !m.user.bot)
            .ok_or(axum::http::StatusCode::NOT_FOUND)?;
        Ok((member.user.clone(), observed.presences.get(&id).cloned()))
    }
    async fn snapshot(&self, cfg: &Config) -> Snapshot {
        let observed = self.observed.read().await;
        let Some(guild) = self.ctx.cache.guild(self.guild) else {
            return Snapshot::default();
        };
        let mut snapshot = Snapshot {
            name: guild.name.clone(),
            ready: observed.ready && self.connected.load(Ordering::Relaxed),
            ..Default::default()
        };
        if !snapshot.ready {
            return snapshot;
        }
        let audience = cfg.audience.map(s::RoleId::new);
        // If a restricted audience role was removed, suppress lists instead of widening access.
        if audience.is_some_and(|id| !guild.roles.contains_key(&id)) {
            snapshot.ready = false;
            return snapshot;
        }
        let base = guild
            .roles
            .get(&s::RoleId::new(self.guild.get()))
            .map(|r| r.permissions)
            .unwrap_or_default()
            | audience
                .and_then(|id| guild.roles.get(&id))
                .map(|r| r.permissions)
                .unwrap_or_default();
        let mut members = guild
            .members
            .values()
            .filter(|m| !m.user.bot)
            .collect::<Vec<_>>();
        members.sort_by_key(|m| m.display_name().to_lowercase());
        for member in members {
            let presence = observed.presences.get(&member.user.id);
            let status = presence
                .map(|p| p.status)
                .unwrap_or(s::OnlineStatus::Offline);
            let label = match status {
                s::OnlineStatus::Idle => "Idle",
                s::OnlineStatus::DoNotDisturb => "DND",
                _ => {
                    if online(status) {
                        "Online"
                    } else {
                        "Offline"
                    }
                }
            };
            snapshot.rows[if online(status) { 0 } else { 1 }].push(format!(
                "<@{}>{}",
                member.user.id,
                if online(status) {
                    format!(" · {label}")
                } else {
                    String::new()
                }
            ));
            if let Some(activity) = presence.and_then(|p| {
                p.activities
                    .iter()
                    .find(|a| a.kind == s::ActivityType::Playing)
            }) && online(status)
            {
                let extra = [activity.details.as_deref(), activity.state.as_deref()]
                    .into_iter()
                    .flatten()
                    .filter(|s| !s.is_empty())
                    .map(|s| clean(s, 70))
                    .collect::<Vec<_>>()
                    .join(" · ");
                snapshot.rows[2].push(format!(
                    "<@{}>\n↳ **{}**{}",
                    member.user.id,
                    clean(&activity.name, 90),
                    if extra.is_empty() {
                        String::new()
                    } else {
                        format!(" · {extra}")
                    }
                ));
            }
            if let Some(channel) = guild
                .voice_states
                .get(&member.user.id)
                .and_then(|v| v.channel_id)
                .and_then(|id| guild.channels.get(&id))
                && visible_voice(base, audience, self.guild, &channel.permission_overwrites)
            {
                let state = &guild.voice_states[&member.user.id];
                snapshot.rows[3].push(format!(
                    "<@{}>\n↳ {}{}",
                    member.user.id,
                    clean(&channel.name, 70),
                    if state.self_deaf || state.deaf {
                        " · Deafened"
                    } else if state.self_mute || state.mute {
                        " · Muted"
                    } else {
                        " · In voice"
                    }
                ));
            }
        }
        snapshot
    }
    async fn channel(
        &self,
        id: u64,
        kind: s::ChannelType,
    ) -> Result<Option<s::GuildChannel>, Error> {
        match s::ChannelId::new(id).to_channel(&self.ctx.http).await {
            Ok(channel) => {
                let channel = channel
                    .guild()
                    .ok_or_else(|| crate::errors::user("Server channel required"))?;
                if channel.guild_id != self.guild || channel.kind != kind {
                    return Err(crate::errors::user(
                        "Stored availability resource belongs to a different server or type",
                    ));
                }
                Ok(Some(channel))
            }
            Err(s::Error::Http(e)) if e.status_code().is_some_and(|s| s.as_u16() == 404) => {
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }
    async fn setup(&self, enabled: bool, audience: Option<s::RoleId>) -> Result<Config, Error> {
        let _guard = self.operations.lock().await;
        let mut cfg = self.config.lock().await.clone();
        if !enabled {
            cfg.enabled = false;
            self.save(&cfg).await?;
            self.rendered.lock().await.clear();
            self.refresh(&mut cfg).await?;
            return Ok(cfg);
        }
        if std::env::var("ENABLE_PRESENCE_EVENTS").as_deref() != Ok("true")
            || std::env::var("ENABLE_MEMBER_EVENTS").as_deref() != Ok("true")
        {
            return Err(crate::errors::user(
                "Enable Discord Presence and Server Members intents plus ENABLE_PRESENCE_EVENTS=true and ENABLE_MEMBER_EVENTS=true before creating the panel",
            ));
        }
        if cfg.category.is_some()
            && audience.map(|id| id.get()) != cfg.audience
            && audience.is_some()
        {
            return Err(crate::errors::user(
                "This panel keeps its original audience. Omit the audience option when resuming; changing its audience requires an administrator to migrate its saved configuration and permissions together.",
            ));
        }
        if cfg.category.is_none() {
            let guild = self.guild.to_partial_guild(&self.ctx.http).await?;
            cfg.audience = audience.map(|id| id.get()).or_else(|| {
                guild
                    .roles
                    .values()
                    .find(|r| !r.managed && r.name.eq_ignore_ascii_case("Legacy"))
                    .map(|r| r.id.get())
            });
        }
        let bot = self.ctx.cache.current_user().id;
        let read = s::Permissions::VIEW_CHANNEL | s::Permissions::READ_MESSAGE_HISTORY;
        let deny = s::Permissions::SEND_MESSAGES
            | s::Permissions::SEND_MESSAGES_IN_THREADS
            | s::Permissions::CREATE_PUBLIC_THREADS
            | s::Permissions::CREATE_PRIVATE_THREADS
            | s::Permissions::ADD_REACTIONS;
        let mut overwrites = vec![
            s::PermissionOverwrite {
                kind: s::PermissionOverwriteType::Role(s::RoleId::new(self.guild.get())),
                allow: if cfg.audience.is_none() {
                    read
                } else {
                    s::Permissions::empty()
                },
                deny: deny
                    | if cfg.audience.is_some() {
                        s::Permissions::VIEW_CHANNEL
                    } else {
                        s::Permissions::empty()
                    },
            },
            s::PermissionOverwrite {
                kind: s::PermissionOverwriteType::Member(bot),
                allow: read
                    | s::Permissions::SEND_MESSAGES
                    | s::Permissions::EMBED_LINKS
                    | s::Permissions::ATTACH_FILES,
                deny: s::Permissions::empty(),
            },
        ];
        if let Some(role) = cfg.audience {
            overwrites.push(s::PermissionOverwrite {
                kind: s::PermissionOverwriteType::Role(s::RoleId::new(role)),
                allow: read,
                deny,
            });
        }
        self.save(&cfg).await?;
        let category = if let Some(id) = cfg.category {
            self.channel(id, s::ChannelType::Category).await?
        } else {
            None
        };
        let category = match category {
            Some(category) => category,
            None => {
                let category = self
                    .guild
                    .create_channel(
                        &self.ctx.http,
                        s::CreateChannel::new("ELVNX AVAILABILITY")
                            .kind(s::ChannelType::Category)
                            .permissions(overwrites.clone()),
                    )
                    .await?;
                cfg.category = Some(category.id.get());
                self.save(&cfg).await?;
                category
            }
        };
        for (key, name) in SECTIONS {
            let found = cfg.boards.iter().position(|b| b.key == key);
            let existing = if let Some(index) = found {
                self.channel(cfg.boards[index].channel, s::ChannelType::Text)
                    .await?
            } else {
                None
            };
            if let Some(channel) = existing {
                if channel.parent_id != Some(category.id) {
                    return Err(crate::errors::user(
                        "A tracked availability channel was moved; restore its parent category before resuming",
                    ));
                }
            } else {
                let channel=self.guild.create_channel(&self.ctx.http,s::CreateChannel::new(name).kind(s::ChannelType::Text).category(category.id).permissions(overwrites.clone()).topic("Live member availability • invisible appears offline • visible voice channels only")).await?;
                let board = Board {
                    key: key.into(),
                    channel: channel.id.get(),
                    messages: Vec::new(),
                };
                if let Some(index) = found {
                    cfg.boards[index] = board;
                } else {
                    cfg.boards.push(board);
                }
                self.save(&cfg).await?;
            }
        }
        cfg.enabled = true;
        self.save(&cfg).await?;
        self.rendered.lock().await.clear();
        self.refresh(&mut cfg).await?;
        Ok(cfg)
    }
    async fn refresh(&self, cfg: &mut Config) -> Result<(), Error> {
        let mut keep = Vec::new();
        for board in std::mem::take(&mut cfg.boards) {
            if board.key == "overview" {
                keep.push(board);
            } else {
                cfg.retired.push(board);
            }
        }
        cfg.boards = keep;
        self.save(cfg).await?;
        let snapshot = self.snapshot(cfg).await;
        let mut first_error = None;
        for index in 0..cfg.boards.len() {
            if let Err(error) = self.publish(cfg, index, &snapshot).await
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        self.save(cfg).await?;
        if let Some(error) = first_error {
            return Err(error);
        }
        self.retire_channels(cfg).await?;
        Ok(())
    }
    async fn retire_channels(&self, cfg: &mut Config) -> Result<(), Error> {
        let retired = cfg.retired.clone();
        for board in retired {
            if let Some(channel) = self.channel(board.channel, s::ChannelType::Text).await? {
                if channel.parent_id != cfg.category.map(s::ChannelId::new) {
                    continue;
                }
                for id in &board.messages {
                    match channel
                        .id
                        .message(&self.ctx.http, s::MessageId::new(*id))
                        .await
                    {
                        Ok(message) if message.author.id == self.ctx.cache.current_user().id => {
                            channel
                                .id
                                .delete_message(&self.ctx.http, message.id)
                                .await?;
                        }
                        Err(s::Error::Http(e))
                            if e.status_code().is_some_and(|s| s.as_u16() == 404) => {}
                        Ok(_) => {
                            return Err(crate::errors::user(
                                "Refusing to remove another author's availability message",
                            ));
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
                // Keep any channel containing custom/admin content; only empty owned status channels are removed.
                if channel
                    .id
                    .messages(&self.ctx.http, s::GetMessages::new().limit(1))
                    .await?
                    .is_empty()
                {
                    channel.id.delete(&self.ctx.http).await?;
                }
            }
            cfg.retired.retain(|b| b.channel != board.channel);
            self.save(cfg).await?;
        }
        Ok(())
    }
    async fn publish(
        &self,
        cfg: &mut Config,
        index: usize,
        snapshot: &Snapshot,
    ) -> Result<(), Error> {
        let board = cfg.boards[index].clone();
        let Some(channel) = self.channel(board.channel, s::ChannelType::Text).await? else {
            return Err(crate::errors::user(
                "Availability channel deleted; rerun /availibilty-panel to recreate it",
            ));
        };
        // Fail closed if the configured audience was changed through channel overrides.
        if let Err(error) = self.check_audience(cfg, &channel) {
            for id in &board.messages {
                match channel
                    .id
                    .message(&self.ctx.http, s::MessageId::new(*id))
                    .await
                {
                    Ok(message) if message.author.id == self.ctx.cache.current_user().id => {
                        channel.id.edit_message(&self.ctx.http,message.id,s::EditMessage::new().embed(crate::emoji::embed_for("user").title("ELVNX / AVAILABILITY PROTECTED").description("Member lists hidden because this channel's audience permissions changed. Restore the configured private audience to resume.").color(0x111111)).attachments(s::EditAttachments::new()).allowed_mentions(s::CreateAllowedMentions::new())).await?;
                    }
                    Err(s::Error::Http(e))
                        if e.status_code().is_some_and(|s| s.as_u16() == 404) => {}
                    Ok(_) => {
                        return Err(crate::errors::user(
                            "Refusing to redact another author's message",
                        ));
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            self.rendered.lock().await.remove(&board.channel);
            return Err(error);
        }
        let label = match board.key.as_str() {
            "online" => "ONLINE",
            "offline" => "OFFLINE",
            "gaming" => "GAMING",
            "voice" => "ON VC",
            _ => "AVAILABILITY",
        };
        let text = vec![if !cfg.enabled {
            "Availability updates paused. An administrator can resume /availibilty-panel."
                .to_string()
        } else if !snapshot.ready {
            "Synchronizing member presence. Live data is unavailable until the full roster is ready.".to_string()
        } else {
            format!(
                "**{}** · Who's around?\nLive Discord presence and game activity. Idle/DND count as Online; invisible appears Offline. Voice lists include only audience-visible channels.",
                clean(&snapshot.name, 70)
            )
        }];
        let fingerprint = serde_json::to_string(&(
            &text,
            if cfg.enabled && snapshot.ready {
                Some(&snapshot.rows)
            } else {
                None
            },
        ))?;
        if self
            .rendered
            .lock()
            .await
            .get(&board.channel)
            .is_some_and(|(previous, time)| {
                previous == &fingerprint && time.elapsed() < Duration::from_secs(60)
            })
        {
            return Ok(());
        }
        let png = if board.key == "overview" {
            Some(crate::cards::render(
                "WHO'S AROUND?",
                &snapshot.name,
                "Online • Offline • Gaming • On VC",
                &[
                    (
                        "ONLINE",
                        if snapshot.ready {
                            snapshot.rows[0].len().to_string()
                        } else {
                            "—".into()
                        },
                    ),
                    (
                        "OFFLINE",
                        if snapshot.ready {
                            snapshot.rows[1].len().to_string()
                        } else {
                            "—".into()
                        },
                    ),
                    (
                        "GAMING",
                        if snapshot.ready {
                            snapshot.rows[2].len().to_string()
                        } else {
                            "—".into()
                        },
                    ),
                    (
                        "ON VC",
                        if snapshot.ready {
                            snapshot.rows[3].len().to_string()
                        } else {
                            "—".into()
                        },
                    ),
                ],
                None,
            )?)
        } else {
            None
        };
        let mut ids = Vec::new();
        for (page, body) in text.iter().enumerate() {
            let title = format!("ELVNX / {label} · {}", page + 1);
            let mut embed=crate::emoji::embed_for("user").title(&title).description(body).color(0x111111).timestamp(s::Timestamp::now()).footer(s::CreateEmbedFooter::new("11X / live availability · invisible appears Offline · stale timestamp means the updater may be offline"));
            if cfg.enabled && snapshot.ready {
                for (group, label) in ["● ONLINE", "○ OFFLINE", "◇ GAMING", "↗ ON VC"]
                    .into_iter()
                    .enumerate()
                {
                    embed = embed.field(
                        format!("{label} · {}", snapshot.rows[group].len()),
                        group_text(&snapshot.rows[group]),
                        group < 2,
                    );
                }
            }
            if png.is_some() {
                embed = embed.image("attachment://availability.png");
            }
            let old = board.messages.get(page).copied();
            let existing = if let Some(id) = old {
                match channel
                    .id
                    .message(&self.ctx.http, s::MessageId::new(id))
                    .await
                {
                    Ok(message) => Some(message),
                    Err(s::Error::Http(e))
                        if e.status_code().is_some_and(|s| s.as_u16() == 404) =>
                    {
                        None
                    }
                    Err(e) => return Err(e.into()),
                }
            } else {
                channel
                    .id
                    .messages(&self.ctx.http, s::GetMessages::new().limit(30))
                    .await?
                    .into_iter()
                    .find(|m| {
                        m.author.id == self.ctx.cache.current_user().id
                            && m.embeds
                                .first()
                                .is_some_and(|e| e.title.as_deref() == Some(title.as_str()))
                    })
            };
            let id = if let Some(message) = existing {
                if message.author.id != self.ctx.cache.current_user().id {
                    return Err(crate::errors::user(
                        "Stored availability message is not owned by this bot",
                    ));
                }
                let mut edit = s::EditMessage::new()
                    .embed(embed)
                    .allowed_mentions(s::CreateAllowedMentions::new());
                if let Some(png) = &png {
                    edit = edit.attachments(
                        s::EditAttachments::new()
                            .add(s::CreateAttachment::bytes(png.clone(), "availability.png")),
                    );
                }
                channel
                    .id
                    .edit_message(&self.ctx.http, message.id, edit)
                    .await?
                    .id
                    .get()
            } else {
                let mut message = s::CreateMessage::new()
                    .embed(embed)
                    .allowed_mentions(s::CreateAllowedMentions::new());
                if let Some(png) = &png {
                    message = message
                        .add_file(s::CreateAttachment::bytes(png.clone(), "availability.png"));
                }
                channel
                    .id
                    .send_message(&self.ctx.http, message)
                    .await?
                    .id
                    .get()
            };
            ids.push(id);
            cfg.boards[index].messages = ids
                .iter()
                .copied()
                .chain(board.messages.iter().skip(ids.len()).copied())
                .collect();
            self.save(cfg).await?;
        }
        for id in board.messages.iter().skip(ids.len()) {
            let message = channel
                .id
                .message(&self.ctx.http, s::MessageId::new(*id))
                .await;
            match message {
                Ok(message) if message.author.id == self.ctx.cache.current_user().id => {
                    channel
                        .id
                        .delete_message(&self.ctx.http, message.id)
                        .await?;
                }
                Err(s::Error::Http(e)) if e.status_code().is_some_and(|s| s.as_u16() == 404) => {}
                Ok(_) => {
                    return Err(crate::errors::user(
                        "Refusing to delete another author's message",
                    ));
                }
                Err(e) => return Err(e.into()),
            }
        }
        cfg.boards[index].messages = ids;
        self.save(cfg).await?;
        self.rendered
            .lock()
            .await
            .insert(board.channel, (fingerprint, Instant::now()));
        Ok(())
    }
    fn check_audience(&self, cfg: &Config, channel: &s::GuildChannel) -> Result<(), Error> {
        let Some(audience) = cfg.audience else {
            return Ok(());
        };
        let guild = self
            .ctx
            .cache
            .guild(self.guild)
            .ok_or_else(|| crate::errors::user("Guild cache unavailable"))?;
        let role = guild.roles.get(&s::RoleId::new(audience)).ok_or_else(|| {
            crate::errors::user("Availability audience role was removed; pause the panel")
        })?;
        if !role.permissions.contains(s::Permissions::ADMINISTRATOR)
            && !channel.permission_overwrites.iter().any(|o| {
                o.kind == s::PermissionOverwriteType::Role(s::RoleId::new(audience))
                    && o.allow.contains(s::Permissions::VIEW_CHANNEL)
            })
        {
            return Err(crate::errors::user(
                "The availability audience lost channel access; restore its View Channel override",
            ));
        }
        if !channel.permission_overwrites.iter().any(|o| {
            o.kind == s::PermissionOverwriteType::Role(s::RoleId::new(self.guild.get()))
                && o.deny.contains(s::Permissions::VIEW_CHANNEL)
        }) {
            return Err(crate::errors::user(
                "Restricted availability channel became public; restore its everyone View Channel denial",
            ));
        }
        for overwrite in channel
            .permission_overwrites
            .iter()
            .filter(|o| o.allow.contains(s::Permissions::VIEW_CHANNEL))
        {
            match overwrite.kind {
                s::PermissionOverwriteType::Role(id) if id.get() != audience => {
                    if !guild
                        .roles
                        .get(&id)
                        .is_some_and(|r| r.permissions.contains(s::Permissions::ADMINISTRATOR))
                    {
                        return Err(crate::errors::user(
                            "Availability channel grants access beyond its configured audience; restore its private permissions",
                        ));
                    }
                }
                s::PermissionOverwriteType::Member(id)
                    if id != self.ctx.cache.current_user().id
                        && id != guild.owner_id
                        && !guild.members.get(&id).is_some_and(|m| {
                            guild
                                .member_permissions(m)
                                .contains(s::Permissions::ADMINISTRATOR)
                        }) =>
                {
                    return Err(crate::errors::user(
                        "Availability channel grants access to an additional member; restore its audience permissions",
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }
}
/// Create or resume one live availability embed with member groups and a doodle cover.
#[poise::command(
    slash_command,
    guild_only,
    rename = "availibilty-panel",
    required_permissions = "MANAGE_GUILD | MANAGE_CHANNELS",
    default_member_permissions = "MANAGE_GUILD | MANAGE_CHANNELS",
    required_bot_permissions = "MANAGE_CHANNELS",
    user_cooldown = 10
)]
pub async fn availibilty_panel(
    ctx: Context<'_>,
    #[description = "Enable (default) or pause updates"] enabled: Option<bool>,
    #[description = "Viewer role; default Legacy if available, otherwise all members (first setup)"]
    audience: Option<s::Role>,
) -> Result<(), Error> {
    if let Some(role) = &audience {
        let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
        if !guild.roles.contains_key(&role.id) || role.managed || role.id.get() == guild.id.get() {
            return reply(
                ctx,
                "Select an ordinary role in this server, or omit it to use the default Legacy audience.",
            )
            .await;
        }
    }
    let outcome = ctx
        .data()
        .availability
        .setup(enabled.unwrap_or(true), audience.map(|r| r.id))
        .await;
    match outcome{Ok(cfg)=>reply(ctx,if cfg.enabled{format!("Availability panel enabled in <#{}>. One read-only channel shows a single live embed with the cover, Online, Offline, Gaming, and On VC. Live changes refresh within about five seconds, subject to Discord rate limits. Re-running reuses tracked resources; invisible users appear Offline.",cfg.boards.first().map(|board|board.channel).ok_or_else(|| crate::errors::user("Missing availability channel"))?)}else{"Availability updates paused; channels remain.".into()}).await,Err(error)=>reply(ctx,format!("**Availability setup could not be completed**\n{}",crate::errors::with_reference(error.as_ref(),"availibilty-panel",ctx.id()))).await}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presence_groups_treat_invisible_as_offline_and_idle_dnd_as_online() {
        assert!(!online(s::OnlineStatus::Invisible));
        assert!(!online(s::OnlineStatus::Offline));
        for status in [
            s::OnlineStatus::Online,
            s::OnlineStatus::Idle,
            s::OnlineStatus::DoNotDisturb,
        ] {
            assert!(online(status));
        }
    }
    #[test]
    fn private_voice_activity_is_not_visible_to_a_public_panel() {
        let guild = s::GuildId::new(1);
        let role = s::RoleId::new(2);
        let mut overwrites = vec![
            s::PermissionOverwrite {
                kind: s::PermissionOverwriteType::Role(s::RoleId::new(1)),
                allow: s::Permissions::empty(),
                deny: s::Permissions::VIEW_CHANNEL,
            },
            s::PermissionOverwrite {
                kind: s::PermissionOverwriteType::Role(role),
                allow: s::Permissions::VIEW_CHANNEL,
                deny: s::Permissions::empty(),
            },
        ];
        assert!(!visible_voice(
            s::Permissions::VIEW_CHANNEL,
            None,
            guild,
            &overwrites
        ));
        assert!(visible_voice(
            s::Permissions::VIEW_CHANNEL,
            Some(role),
            guild,
            &overwrites
        ));
        overwrites.push(s::PermissionOverwrite {
            kind: s::PermissionOverwriteType::Member(s::UserId::new(3)),
            allow: s::Permissions::empty(),
            deny: s::Permissions::VIEW_CHANNEL,
        });
        assert!(!visible_voice(
            s::Permissions::VIEW_CHANNEL,
            Some(role),
            guild,
            &overwrites
        ));
        assert!(visible_voice(
            s::Permissions::ADMINISTRATOR,
            Some(role),
            guild,
            &overwrites
        ));
    }
    #[test]
    fn long_rosters_fit_one_embed_field_with_visible_overflow() {
        let rows = (0..100)
            .map(|i| format!("{i} {}", "😀".repeat(500)))
            .collect::<Vec<_>>();
        let result = group_text(&rows);
        assert!(result.encode_utf16().count() <= 1024);
        assert!(result.contains("100 more"));
        let short = group_text(&vec!["<@123>".into(); 100]);
        assert!(short.encode_utf16().count() <= 1024);
        assert_eq!(group_text(&[]), "No members right now.");
        assert!(!clean("name\n**markdown**", 50).contains('\n'));
    }
    #[tokio::test]
    async fn only_panel_resource_ids_persist_not_presence_history() {
        let db = crate::store::connect("sqlite::memory:").await.unwrap();
        let cfg = Config {
            enabled: true,
            category: Some(10),
            audience: Some(20),
            boards: vec![Board {
                key: "online".into(),
                channel: 30,
                messages: vec![40],
            }],
            retired: Vec::new(),
        };
        let raw = serde_json::to_string(&cfg).unwrap();
        assert!(!raw.contains("presences"));
        assert!(!raw.contains("activities"));
        sqlx::query("INSERT INTO availability VALUES('1',?)")
            .bind(&raw)
            .execute(&db)
            .await
            .unwrap();
        let loaded: Option<String> =
            sqlx::query_scalar("SELECT value FROM availability WHERE guild='1'")
                .fetch_optional(&db)
                .await
                .unwrap();
        assert_eq!(loaded, Some(raw));
        let other: Option<String> =
            sqlx::query_scalar("SELECT value FROM availability WHERE guild='2'")
                .fetch_optional(&db)
                .await
                .unwrap();
        assert!(other.is_none());
    }
}
