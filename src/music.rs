mod favorites;
mod recap;
mod requests;
mod stay;
use crate::{Context, Data, Error, reply, serenity as s};
use futures_util::{SinkExt, StreamExt};
use rand::seq::SliceRandom;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Notify};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

pub const COMMAND_COUNT: usize = 102;
const MAX_QUEUE: usize = 50;
#[derive(Clone, Copy, Default, poise::ChoiceParameter, PartialEq, Debug)]
enum RepeatMode {
    #[default]
    Off,
    Track,
    Queue,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Info {
    title: String,
    author: String,
    length: u64,
    #[serde(default)]
    is_stream: bool,
    #[serde(default)]
    is_seekable: bool,
    uri: Option<String>,
}
#[derive(Clone, Deserialize)]
struct Track {
    encoded: String,
    info: Info,
}
#[derive(Clone)]
struct Request {
    track: Track,
    user: u64,
    id: u64,
}
#[derive(Clone)]
struct State {
    repeat: RepeatMode,
    skip_votes: HashSet<u64>,
    channel: Option<s::ChannelId>,
    text: Option<s::ChannelId>,
    panel: Option<s::MessageId>,
    current: Option<Request>,
    queue: VecDeque<Request>,
    paused: bool,
    volume: u8,
    position: u64,
    voice_session: Option<String>,
    voice_token: Option<String>,
    endpoint: Option<String>,
    voice_ready: bool,
    next_id: u64,
    last_active: Instant,
    summary: Option<recap::Session>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            repeat: RepeatMode::Off,
            skip_votes: HashSet::new(),
            channel: None,
            text: None,
            panel: None,
            current: None,
            queue: VecDeque::new(),
            paused: false,
            volume: 50,
            position: 0,
            voice_session: None,
            voice_token: None,
            endpoint: None,
            voice_ready: false,
            next_id: 1,
            last_active: Instant::now(),
            summary: None,
        }
    }
}
fn next_proposal(state: &State, natural: bool) -> (State, Option<Request>) {
    let mut proposed = state.clone();
    let repeat = natural && proposed.repeat != RepeatMode::Off;
    let mut previous = proposed.current.clone();
    if repeat && let Some(request) = &mut previous {
        request.id = proposed.next_id;
        proposed.next_id = proposed.next_id.saturating_add(1);
    }
    let next = if repeat && proposed.repeat == RepeatMode::Track {
        previous
    } else {
        let next = proposed.queue.pop_front();
        if repeat && proposed.repeat == RepeatMode::Queue {
            if next.is_some()
                && let Some(previous) = previous.clone()
            {
                proposed.queue.push_back(previous);
            }
            next.or(previous)
        } else {
            next
        }
    };
    (proposed, next)
}
pub struct Service {
    db: sqlx::SqlitePool,
    client: reqwest::Client,
    url: String,
    password: String,
    guild: s::GuildId,
    bot: s::UserId,
    session: Mutex<Option<String>>,
    state: Mutex<State>,
    operations: Mutex<()>,
    panel_lock: Mutex<()>,
    pending_panels: Mutex<Vec<(s::ChannelId, s::MessageId)>>,
    voice: Notify,
    buttons: Mutex<HashMap<u64, Instant>>,
    stay_retry: Mutex<Option<Instant>>,
}
impl Service {
    pub fn start(
        ctx: s::Context,
        db: sqlx::SqlitePool,
        guild: s::GuildId,
        bot: s::UserId,
    ) -> Result<Option<Arc<Self>>, Error> {
        let Ok(url) = std::env::var("LAVALINK_URL") else {
            return Ok(None);
        };
        let parsed = reqwest::Url::parse(&url)?;
        if parsed.scheme() != "http"
            || parsed.host_str() != Some("127.0.0.1")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
        {
            return Err(crate::errors::user(
                "Lavalink must be the private loopback node",
            ));
        }
        let password = std::env::var("LAVALINK_PASSWORD")?;
        let mut headers = reqwest::header::HeaderMap::new();
        let mut auth = reqwest::header::HeaderValue::from_str(&password)?;
        auth.set_sensitive(true);
        headers.insert(reqwest::header::AUTHORIZATION, auth);
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let service = Arc::new(Self {
            db,
            client,
            url: url.trim_end_matches('/').to_string(),
            password,
            guild,
            bot,
            session: Mutex::new(None),
            state: Mutex::new(State::default()),
            operations: Mutex::new(()),
            panel_lock: Mutex::new(()),
            pending_panels: Mutex::new(Vec::new()),
            voice: Notify::new(),
            buttons: Mutex::new(HashMap::new()),
            stay_retry: Mutex::new(None),
        });
        let node = Arc::clone(&service);
        let ws_ctx = ctx.clone();
        tokio::spawn(async move {
            node.socket(ws_ctx).await;
        });
        let node = Arc::clone(&service);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                node.retry_panel_cleanup(&ctx).await;
                let _guard = node.operations.lock().await;
                let keep = match stay::maintain(&node, &ctx).await {
                    Ok(v) => v,
                    Err(error) => {
                        tracing::warn!(%error,"24/7 voice maintenance failed");
                        true
                    }
                };
                let state = node.state.lock().await.clone();
                if let Some(channel) = state.channel {
                    let human = ctx.cache.guild(guild).is_none_or(|g| {
                        g.voice_states.iter().any(|(id, v)| {
                            *id != bot
                                && v.channel_id == Some(channel)
                                && ctx.cache.user(*id).is_none_or(|u| !u.bot)
                        })
                    });
                    if !keep
                        && (!human
                            || (state.current.is_none()
                                && state.last_active.elapsed() > Duration::from_secs(180)))
                    {
                        let _ = node.stop(&ctx).await;
                    } else {
                        let _ = node.panel(&ctx).await;
                    }
                }
            }
        });
        Ok(Some(service))
    }
    pub(crate) async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        payload: Option<Value>,
    ) -> Result<Value, Error> {
        let mut req = self.client.request(method, format!("{}{path}", self.url));
        if let Some(value) = payload {
            req = req.json(&value);
        }
        let mut response = req
            .send()
            .await
            .map_err(|_| crate::errors::user("Audio node is unavailable"))?;
        if !response.status().is_success() {
            return Err(crate::errors::user(format!(
                "Audio node returned HTTP {}",
                response.status().as_u16()
            )));
        }
        if response.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(Value::Null);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if body.len() + chunk.len() > 2_000_000 {
                return Err(crate::errors::user("Audio response too large"));
            }
            body.extend(chunk);
        }
        Ok(serde_json::from_slice(&body)?)
    }
    pub async fn live_embed(&self) -> s::CreateEmbed {
        let started = Instant::now();
        let mut embed = crate::emoji::embed_for("headphones")
            .title("ELVNX / LAVALINK STATUS")
            .color(0x111111)
            .timestamp(s::Timestamp::now())
            .footer(s::CreateEmbedFooter::new(
                "30s refresh • stale timestamp may mean bot/updater is offline",
            ));
        let stats = match self.request(reqwest::Method::GET, "/v4/stats", None).await {
            Ok(stats) => stats,
            Err(_) => {
                return embed.color(0xbb3333).description(format!(
                    "{} Audio node unavailable. The bot will reconnect automatically.",
                    crate::emoji::text("offline")
                ));
            }
        };
        let latency = started.elapsed().as_millis();
        let info = self
            .request(reqwest::Method::GET, "/v4/info", None)
            .await
            .ok();
        let connected = self.session.lock().await.is_some();
        let state = self.state.lock().await.clone();
        let uptime = stats["uptime"].as_u64().unwrap_or(0) / 1000;
        embed = embed
            .description(if connected {
                format!(
                    "{} Node available · music connection ready",
                    crate::emoji::text("online")
                )
            } else {
                format!(
                    "{} Node available · music connection reconnecting",
                    crate::emoji::text("warning")
                )
            })
            .field(
                "Node version",
                info.as_ref()
                    .and_then(|v| v.pointer("/version/semver"))
                    .and_then(Value::as_str)
                    .unwrap_or("Unavailable"),
                true,
            )
            .field(
                "Node uptime",
                format!(
                    "{}d {}h {}m",
                    uptime / 86400,
                    uptime % 86400 / 3600,
                    uptime % 3600 / 60
                ),
                true,
            )
            .field("API response", format!("{latency} ms"), true)
            .field(
                "Players / playing",
                format!(
                    "{} / {}",
                    stats["players"].as_u64().unwrap_or(0),
                    stats["playingPlayers"].as_u64().unwrap_or(0)
                ),
                true,
            )
            .field(
                "Memory used / allocated",
                format!(
                    "{} / {} MiB",
                    stats["memory"]["used"].as_u64().unwrap_or(0) / 1048576,
                    stats["memory"]["allocated"].as_u64().unwrap_or(0) / 1048576
                ),
                true,
            )
            .field(
                "CPU (node / system)",
                format!(
                    "{:.1}% / {:.1}%",
                    stats["cpu"]["lavalinkLoad"].as_f64().unwrap_or(0.0) * 100.0,
                    stats["cpu"]["systemLoad"].as_f64().unwrap_or(0.0) * 100.0
                ),
                true,
            )
            .field(
                "Queue / volume",
                format!("{} tracks · {}%", state.queue.len(), state.volume),
                true,
            )
            .field(
                "Voice connection",
                state
                    .channel
                    .map(|channel| format!("<#{channel}>"))
                    .unwrap_or_else(|| "Disconnected".into()),
                true,
            );
        if let Some(request) = state.current {
            embed = embed.field(
                if state.paused {
                    "Paused"
                } else {
                    "Now playing"
                },
                safe(&request.track.info.title, 180),
                false,
            );
        } else {
            embed = embed.field("Playback", "Idle", false);
        }
        embed
    }
    async fn patch(&self, payload: Value) -> Result<(), Error> {
        let session =
            self.session.lock().await.clone().ok_or_else(|| {
                crate::errors::user("Audio node is reconnecting; try again shortly")
            })?;
        self.request(
            reqwest::Method::PATCH,
            &format!("/v4/sessions/{session}/players/{}", self.guild),
            Some(payload),
        )
        .await?;
        Ok(())
    }
    async fn load(&self, query: &str, source: Source) -> Result<Vec<Track>, Error> {
        let identifier = identifier(query, source)?;
        let mut url = reqwest::Url::parse(&format!("{}/v4/loadtracks", self.url))?;
        url.query_pairs_mut().append_pair("identifier", &identifier);
        let path = format!(
            "/v4/loadtracks?{}",
            url.query()
                .ok_or_else(|| crate::errors::user("Missing query"))?
        );
        let value = self.request(reqwest::Method::GET, &path, None).await?;
        parse_tracks(value)
    }
    fn voice_state(&self, ctx: &s::Context, channel: Option<s::ChannelId>) {
        ctx.shard.websocket_message(Message::Text(json!({"op":4,"d":{"guild_id":self.guild.to_string(),"channel_id":channel.map(|c|c.to_string()),"self_mute":false,"self_deaf":true}}).to_string()));
    }
    async fn connect_voice(&self) -> Result<(), Error> {
        let state = self.state.lock().await.clone();
        if let (Some(channel), Some(session), Some(token), Some(endpoint)) = (
            state.channel,
            state.voice_session,
            state.voice_token,
            state.endpoint,
        ) {
            self.patch(json!({"voice":{"sessionId":session,"token":token,"endpoint":endpoint,"channelId":channel.to_string()}})).await?;
            self.state.lock().await.voice_ready = true;
            self.voice.notify_waiters();
        }
        Ok(())
    }
    async fn next(&self, ctx: &s::Context) -> Result<(), Error> {
        self.advance(ctx, false, true).await
    }
    async fn advance(&self, ctx: &s::Context, natural: bool, skipped: bool) -> Result<(), Error> {
        let mut state = self.state.lock().await;
        let (proposed, next) = next_proposal(&state, natural);
        let volume = state.volume;
        if let Some(request) = &next {
            self.patch(json!({"track":{"encoded":request.track.encoded,"userData":{"requestId":request.id}},"volume":volume,"paused":false})).await?;
        } else {
            self.patch(json!({"track":{"encoded":null}})).await?;
        }
        let had_current = state.current.is_some();
        *state = proposed;
        let at = Instant::now();
        if let Some(summary) = state.summary.as_mut() {
            if had_current {
                summary.end_track(natural, skipped, at);
            } else {
                summary.active(false, at);
            }
        }
        if let Some(request) = &next {
            state
                .summary
                .get_or_insert_with(|| recap::Session::new(at))
                .start_track(request, at);
        }

        state.current = next;
        state.skip_votes.clear();
        state.position = 0;
        state.paused = false;
        state.last_active = Instant::now();
        let empty = state.current.is_none();
        let summary = if empty { state.summary.take() } else { None };
        let destination = state.text;
        drop(state);
        self.channel_status(ctx).await;
        if empty {
            if let Some(summary) = summary {
                recap::send(self, ctx, summary, destination, "The queue finished.").await;
            }
            self.delete_panel(ctx).await;
            Ok(())
        } else {
            self.panel(ctx).await
        }
    }
    async fn channel_status(&self, ctx: &s::Context) {
        let state = self.state.lock().await.clone();
        if let Some(channel) = state.channel {
            let status = state.current.as_ref().map(|r| {
                format!(
                    "{} {}",
                    if state.paused {
                        "Paused ·"
                    } else {
                        "Playing ·"
                    },
                    r.track.info.title
                )
                .chars()
                .take(500)
                .collect::<String>()
            });
            if ctx
                .http
                .edit_voice_status(channel, &json!({"status":status}), None)
                .await
                .is_err()
            {
                tracing::warn!(
                    "Could not update music channel status; check Set Voice Channel Status permission"
                );
            }
        }
    }
    async fn stop(&self, ctx: &s::Context) -> Result<(), Error> {
        let (summary, destination) = {
            let mut state = self.state.lock().await;
            let mut summary = state.summary.take();
            if let Some(s) = summary.as_mut() {
                s.active(false, Instant::now());
            }
            (summary, state.text)
        };
        if let Some(channel) = self.state.lock().await.channel {
            let _ = ctx
                .http
                .edit_voice_status(channel, &json!({"status":null}), None)
                .await;
        }
        if let Some(session) = self.session.lock().await.clone() {
            let _ = self
                .request(
                    reqwest::Method::DELETE,
                    &format!("/v4/sessions/{session}/players/{}", self.guild),
                    None,
                )
                .await;
        }
        {
            let mut state = self.state.lock().await;
            state.channel = None;
            state.current = None;
            state.queue.clear();
            state.skip_votes.clear();
            state.repeat = RepeatMode::Off;
            state.voice_ready = false;
            state.voice_session = None;
            state.voice_token = None;
            state.endpoint = None;
            state.paused = false;
            state.position = 0;
        }
        self.voice_state(ctx, None);
        self.delete_panel(ctx).await;
        if let Some(summary) = summary {
            recap::send(
                self,
                ctx,
                summary,
                destination,
                "Playback ended or the voice connection closed.",
            )
            .await;
        }
        Ok(())
    }
    async fn delete_panel(&self, ctx: &s::Context) {
        let _guard = self.panel_lock.lock().await;
        let panel = {
            let mut state = self.state.lock().await;
            state.text.take().zip(state.panel.take())
        };
        if let Some((channel, message)) = panel {
            match channel.delete_message(&ctx.http, message).await {
                Ok(()) => {}
                Err(s::Error::Http(error))
                    if error
                        .status_code()
                        .is_some_and(|status| status.as_u16() == 404) => {}
                Err(_) => {
                    tracing::warn!(
                        "Music stopped; panel deletion failed and will retry automatically"
                    );
                    self.pending_panels.lock().await.push((channel, message));
                }
            }
        }
    }
    async fn retry_panel_cleanup(&self, ctx: &s::Context) {
        let pending = self
            .pending_panels
            .lock()
            .await
            .iter()
            .take(5)
            .copied()
            .collect::<Vec<_>>();
        for (channel, message) in pending {
            let deleted = match channel.delete_message(&ctx.http, message).await {
                Ok(()) => true,
                Err(s::Error::Http(error)) => error
                    .status_code()
                    .is_some_and(|status| status.as_u16() == 404),
                Err(_) => false,
            };
            if deleted {
                self.pending_panels
                    .lock()
                    .await
                    .retain(|panel| *panel != (channel, message));
            }
        }
    }
    async fn panel(&self, ctx: &s::Context) -> Result<(), Error> {
        let _guard = self.panel_lock.lock().await;
        let state = self.state.lock().await.clone();
        if state.channel.is_none() {
            return Ok(());
        }
        if self.pending_panels.lock().await.len() >= 100 {
            return Err(crate::errors::user(
                "Music panel cleanup is pending. Try again shortly.",
            ));
        }
        let Some(channel) = state.text else {
            return Ok(());
        };
        let title = state
            .current
            .as_ref()
            .map(|r| r.track.info.title.as_str())
            .unwrap_or("Queue is empty");
        let artist = state
            .current
            .as_ref()
            .map(|r| r.track.info.author.as_str())
            .unwrap_or("Use /play while in a voice channel");
        let duration = state
            .current
            .as_ref()
            .map(|r| {
                if r.track.info.is_stream {
                    "LIVE".into()
                } else {
                    time(r.track.info.length)
                }
            })
            .unwrap_or_else(|| "—".into());
        let status = if state.channel.is_none() {
            "Disconnected"
        } else if state.paused {
            "Paused"
        } else if state.current.is_some() {
            "Playing"
        } else {
            "Ready"
        };
        let listeners = listener_count(ctx, self.guild, state.channel);
        let upcoming = state
            .queue
            .iter()
            .take(3)
            .map(|r| {
                format!(
                    "{} / {}",
                    safe(&r.track.info.title, 65),
                    track_duration(&r.track.info)
                )
            })
            .collect::<Vec<_>>();
        let elapsed = time(state.position);
        let progress = state
            .current
            .as_ref()
            .map(|r| {
                if r.track.info.is_stream || r.track.info.length == 0 {
                    0.0
                } else {
                    state.position.min(r.track.info.length) as f64 / r.track.info.length as f64
                }
            })
            .unwrap_or(0.0);
        let png = crate::cards::music(&crate::cards::MusicCard {
            title,
            artist,
            status,
            elapsed: &elapsed,
            duration: &duration,
            progress,
            volume: state.volume,
            repeat: repeat_label(state.repeat),
            listeners,
            upcoming: &upcoming,
        })?;
        let embed = player_embed(&state, listeners);
        let components = player_controls(&state);
        let file = s::CreateAttachment::bytes(png, "listening-room.png");
        if let Some(id) = state.panel {
            let result = channel
                .edit_message(
                    &ctx.http,
                    id,
                    s::EditMessage::new()
                        .embed(embed.clone())
                        .components(components.clone())
                        .attachments(s::EditAttachments::new().add(file.clone()))
                        .allowed_mentions(s::CreateAllowedMentions::new()),
                )
                .await;
            match result {
                Ok(_) => return Ok(()),
                Err(s::Error::Http(error))
                    if error.status_code().map(|status| status.as_u16()) == Some(404) =>
                {
                    self.state.lock().await.panel = None;
                }
                Err(error) => return Err(error.into()),
            }
        }
        {
            let message = channel
                .send_message(
                    &ctx.http,
                    s::CreateMessage::new()
                        .embed(embed)
                        .components(components)
                        .add_file(file)
                        .allowed_mentions(s::CreateAllowedMentions::new()),
                )
                .await?;
            self.state.lock().await.panel = Some(message.id);
        }
        Ok(())
    }
    async fn socket(self: Arc<Self>, ctx: s::Context) {
        loop {
            let result: Result<(), Error> = async {
                let mut request =
                    format!("{}/v4/websocket", self.url.replacen("http://", "ws://", 1))
                        .into_client_request()?;
                request
                    .headers_mut()
                    .insert("Authorization", self.password.parse()?);
                request
                    .headers_mut()
                    .insert("User-Id", self.bot.to_string().parse()?);
                request
                    .headers_mut()
                    .insert("Client-Name", "ELVNX-Rust/0.1.0".parse()?);
                let (mut stream, _) = tokio_tungstenite::connect_async_with_config(
                    request,
                    Some(tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
                        max_message_size: Some(2_000_000),
                        max_frame_size: Some(2_000_000),
                        ..Default::default()
                    }),
                    false,
                )
                .await?;
                while let Some(message) = stream.next().await {
                    match message? {
                        Message::Ping(bytes) => stream.send(Message::Pong(bytes)).await?,
                        Message::Text(text) => {
                            let event: Value = serde_json::from_str(&text)?;
                            match event["op"].as_str() {
                                Some("ready") => {
                                    *self.session.lock().await =
                                        event["sessionId"].as_str().map(str::to_string);
                                    tracing::info!("Lavalink connected");
                                }
                                Some("playerUpdate")
                                    if event["guildId"].as_str()
                                        == Some(self.guild.to_string().as_str()) =>
                                {
                                    self.state.lock().await.position =
                                        event["state"]["position"].as_u64().unwrap_or(0);
                                }
                                Some("event")
                                    if event["guildId"].as_str()
                                        == Some(self.guild.to_string().as_str()) =>
                                {
                                    if matches!(
                                        event["type"].as_str(),
                                        Some(
                                            "TrackEndEvent"
                                                | "TrackExceptionEvent"
                                                | "TrackStuckEvent"
                                        )
                                    ) {
                                        let end = event["type"] == "TrackEndEvent";
                                        let eligible = !end
                                            || matches!(
                                                event["reason"].as_str(),
                                                Some("finished" | "loadFailed")
                                            );
                                        let _guard = self.operations.lock().await;
                                        let current =
                                            self.state.lock().await.current.as_ref().map(|r| r.id);
                                        if eligible
                                            && current.is_some()
                                            && current
                                                == event["track"]["userData"]["requestId"].as_u64()
                                        {
                                            let outcome = self
                                                .advance(
                                                    &ctx,
                                                    end && event["reason"] == "finished",
                                                    false,
                                                )
                                                .await;
                                            if outcome.is_err() {
                                                tracing::warn!("Could not advance music queue");
                                            }
                                        }
                                    } else if event["type"] == "WebSocketClosedEvent" {
                                        let _guard = self.operations.lock().await;
                                        let _ = self.stop(&ctx).await;
                                    }
                                }
                                _ => {}
                            }
                        }
                        Message::Close(_) => break,
                        _ => {}
                    }
                }
                Ok(())
            }
            .await;
            if result.is_err() {
                tracing::warn!("Lavalink connection unavailable; retrying");
            }
            *self.session.lock().await = None;
            {
                let _guard = self.operations.lock().await;
                let _ = self.stop(&ctx).await;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}
#[derive(Clone, Copy, poise::ChoiceParameter)]
pub enum Source {
    Soundcloud,
    YouTube,
}
fn identifier(query: &str, source: Source) -> Result<String, Error> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 300 {
        return Err(crate::errors::user(
            "Enter a title or supported HTTPS link, up to 300 characters",
        ));
    }
    if query.contains("://") {
        let url = reqwest::Url::parse(query)?;
        let host = url.host_str().unwrap_or_default();
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || ![
                "youtube.com",
                "youtu.be",
                "soundcloud.com",
                "bandcamp.com",
                "twitch.tv",
            ]
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
        {
            return Err(crate::errors::user(
                "Use a YouTube, SoundCloud, Bandcamp or Twitch HTTPS link",
            ));
        }
        Ok(url.into())
    } else {
        Ok(format!(
            "{}:{query}",
            match source {
                Source::Soundcloud => "scsearch",
                Source::YouTube => "ytsearch",
            }
        ))
    }
}
fn parse_tracks(value: Value) -> Result<Vec<Track>, Error> {
    match value["loadType"].as_str() {
        Some("track") => Ok(vec![serde_json::from_value(value["data"].clone())?]),
        Some("search") => Ok(serde_json::from_value::<Vec<Track>>(value["data"].clone())?
            .into_iter()
            .take(1)
            .collect()),
        Some("playlist") => Ok(serde_json::from_value::<Vec<Track>>(
            value["data"]["tracks"].clone(),
        )?
        .into_iter()
        .take(MAX_QUEUE)
        .collect()),
        _ => Err(crate::errors::user(
            "No playable track found. Try another source or link; source services can restrict server playback.",
        )),
    }
}

fn repeat_label(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Off => "Off",
        RepeatMode::Track => "Track",
        RepeatMode::Queue => "Queue",
    }
}
fn track_duration(info: &Info) -> String {
    if info.is_stream {
        "LIVE".into()
    } else {
        time(info.length)
    }
}
fn listener_count(ctx: &s::Context, guild: s::GuildId, channel: Option<s::ChannelId>) -> usize {
    ctx.cache
        .guild(guild)
        .map(|g| {
            g.voice_states
                .iter()
                .filter(|(id, v)| {
                    channel.is_some()
                        && v.channel_id == channel
                        && v.member
                            .as_ref()
                            .or_else(|| g.members.get(id))
                            .is_some_and(|m| !m.user.bot)
                })
                .count()
        })
        .unwrap_or(0)
}
fn queue_text(state: &State) -> String {
    if state.queue.is_empty() {
        return "The queue is empty. Add your next track with /play.".into();
    }
    format!(
        "**Up next / {} track(s)**\nTitles shortened to fit the full queue.\n\n{}",
        state.queue.len(),
        state
            .queue
            .iter()
            .take(MAX_QUEUE)
            .enumerate()
            .map(|(i, r)| format!(
                "`{:02}` {} · {}",
                i + 1,
                safe(&r.track.info.title, 24),
                track_duration(&r.track.info)
            ))
            .collect::<Vec<_>>()
            .join("\n")
    )
}
fn player_embed(state: &State, listeners: usize) -> s::CreateEmbed {
    let current = state.current.as_ref();
    let playing = current.is_some();
    let title = current
        .map(|r| r.track.info.title.as_str())
        .unwrap_or("Your queue, your soundtrack.");
    let artist = current
        .map(|r| r.track.info.author.as_str())
        .unwrap_or("Choose your next track with /play");
    let status = if !playing {
        "Ready"
    } else if state.paused {
        "Paused"
    } else {
        "Playing"
    };
    let duration = current
        .map(|r| track_duration(&r.track.info))
        .unwrap_or_else(|| "—".into());
    let queued = if state.queue.iter().any(|r| r.track.info.is_stream) {
        "Live / variable".into()
    } else {
        time(
            state
                .queue
                .iter()
                .fold(0u64, |total, r| total.saturating_add(r.track.info.length)),
        )
    };
    let next = state
        .queue
        .iter()
        .take(3)
        .enumerate()
        .map(|(i, r)| {
            format!(
                "`{:02}` **{}** · {}",
                i + 1,
                safe(&r.track.info.title, 75),
                track_duration(&r.track.info)
            )
        })
        .collect::<Vec<_>>();
    let mut embed = crate::emoji::embed_for(if state.paused { "pause" } else { "headphones" })
        .title("ELVNX / LISTENING ROOM")
        .description(format!("**{}**\n{}", safe(title, 160), safe(artist, 120)))
        .color(0x111111)
        .image("attachment://listening-room.png")
        .field(
            "Playback",
            format!(
                "{} **{status}**\n`{} / {duration}`",
                crate::emoji::text(if state.paused { "pause" } else { "play" }),
                time(state.position)
            ),
            true,
        )
        .field(
            "Mix",
            format!(
                "**{}%** volume\nLoop **{}**",
                state.volume,
                repeat_label(state.repeat)
            ),
            true,
        )
        .field(
            "In the room",
            format!(
                "**{listeners}** listener(s)\n{} skip vote(s)",
                state.skip_votes.len()
            ),
            true,
        )
        .field(
            format!("Up next / {} queued · {queued}", state.queue.len()),
            if next.is_empty() {
                "The queue is open. Add a track with `/play`.".into()
            } else {
                next.join("\n")
            },
            false,
        )
        .footer(s::CreateEmbedFooter::new(
            "11X / avrxt.dev • auto-updates ~15s • same-channel controls",
        ));
    if let Some(channel) = state.channel {
        embed = embed.field("Voice channel", format!("<#{channel}>"), true);
    }
    if let Some(request) = current {
        embed = embed.field("Selected by", format!("<@{}>", request.user), true);
        if let Some(uri) = &request.track.info.uri
            && reqwest::Url::parse(uri).is_ok()
            && identifier(uri, Source::YouTube)
                .or_else(|_| identifier(uri, Source::Soundcloud))
                .is_ok()
        {
            embed = embed.url(uri);
        }
    }
    embed
}
fn seek_target(info: &Info, position: u64, action: &str) -> Result<u64, Error> {
    if info.is_stream || !info.is_seekable || info.length == 0 {
        return Err(crate::errors::user(
            "This track is live or does not support seeking. You can pause, skip or change volume instead.",
        ));
    }
    Ok(match action {
        "back" => position.saturating_sub(30_000),
        "forward" => position.saturating_add(30_000),
        "restart" => 0,
        _ => return Err(crate::errors::user("Unknown seek control.")),
    }
    .min(info.length.saturating_sub(1)))
}
fn player_controls(state: &State) -> Vec<s::CreateActionRow> {
    use s::ButtonStyle as B;
    let idle = state.channel.is_none();
    let empty = state.current.is_none();
    let no_seek = state.current.as_ref().is_none_or(|r| {
        r.track.info.is_stream || !r.track.info.is_seekable || r.track.info.length == 0
    });
    let button = |name: &str, label: &str, icon: &str, disabled: bool, style: B| {
        crate::emoji::button_for(format!("elvnx:music:{name}"), icon)
            .label(label)
            .disabled(disabled)
            .style(style)
    };
    vec![
        s::CreateActionRow::Buttons(vec![
            button(
                "pause",
                if state.paused { "Resume" } else { "Pause" },
                if state.paused { "play" } else { "pause" },
                idle || empty,
                B::Primary,
            ),
            button("skip", "Skip", "next", idle || empty, B::Secondary),
            button("vote", "Vote skip", "check", idle || empty, B::Secondary),
            button(
                "less",
                "Volume −10",
                "volume_down",
                idle || state.volume == 0,
                B::Secondary,
            ),
            button(
                "more",
                "Volume +10",
                "volume_up",
                idle || state.volume == 100,
                B::Secondary,
            ),
        ]),
        s::CreateActionRow::Buttons(vec![
            button("back", "Back 30s", "repeat", idle || no_seek, B::Secondary),
            button(
                "forward",
                "Forward 30s",
                "next",
                idle || no_seek,
                B::Secondary,
            ),
            button(
                "restart",
                "Restart track",
                "repeat",
                idle || no_seek,
                B::Secondary,
            ),
            button(
                "repeat",
                &format!("Loop: {}", repeat_label(state.repeat)),
                "repeat",
                idle || empty,
                if state.repeat == RepeatMode::Off {
                    B::Secondary
                } else {
                    B::Primary
                },
            ),
            button(
                "shuffle",
                "Shuffle",
                "shuffle",
                idle || state.queue.len() < 2,
                B::Secondary,
            ),
        ]),
        s::CreateActionRow::Buttons(vec![
            button("add", "Add track", "plus", idle, B::Primary),
            button("queue", "Full queue", "queue", idle, B::Secondary),
            button(
                "details",
                "Track details",
                "search",
                idle || empty,
                B::Secondary,
            ),
            button("help", "Control guide", "review", false, B::Secondary),
            button("stop", "Stop / leave", "stop", idle, B::Danger),
        ]),
        s::CreateActionRow::SelectMenu(
            s::CreateSelectMenu::new(
                "elvnx:music:volume",
                s::CreateSelectMenuKind::String {
                    options: [0u8, 10, 25, 50, 75, 100]
                        .into_iter()
                        .map(|v| {
                            s::CreateSelectMenuOption::new(
                                if v == 0 {
                                    "Mute / 0%".into()
                                } else {
                                    format!("Volume / {v}%")
                                },
                                v.to_string(),
                            )
                            .default_selection(state.volume == v)
                        })
                        .collect(),
                },
            )
            .placeholder(format!("Volume presets / current {}%", state.volume))
            .disabled(idle),
        ),
    ]
}
pub fn preview() -> Result<Vec<u8>, Error> {
    crate::cards::music(&crate::cards::MusicCard {
        title: "Midnight, on repeat",
        artist: "ELVNX listening sessions",
        status: "PLAYING",
        elapsed: "1:24",
        duration: "4:08",
        progress: 84.0 / 248.0,
        volume: 50,
        repeat: "Queue",
        listeners: 6,
        upcoming: &[
            "After Hours / 3:42".into(),
            "A Little Closer / 4:16".into(),
            "Last Light / 2:58".into(),
        ],
    })
}
fn safe(text: &str, max: usize) -> String {
    text.chars()
        .take(max)
        .map(|c| {
            if matches!(c, '*' | '`' | '_' | '[' | ']' | '\n' | '\r') {
                ' '
            } else {
                c
            }
        })
        .collect()
}
fn time(ms: u64) -> String {
    format!("{}:{:02}", ms / 60000, (ms / 1000) % 60)
}
fn service(ctx: Context<'_>) -> Result<Arc<Service>, Error> {
    ctx.data()
        .music
        .clone()
        .ok_or_else(|| crate::errors::user("The music service is not configured"))
}
fn voice_channel(ctx: &s::Context, guild: s::GuildId, user: s::UserId) -> Option<s::ChannelId> {
    ctx.cache
        .guild(guild)
        .and_then(|g| g.voice_states.get(&user).and_then(|v| v.channel_id))
}
async fn permission(ctx: &s::Context, node: &Service, user: s::UserId) -> Result<(), Error> {
    let channel = node
        .state
        .lock()
        .await
        .channel
        .ok_or_else(|| crate::errors::user("Nothing is connected"))?;
    if voice_channel(ctx, node.guild, user) != Some(channel) {
        return Err(crate::errors::user(
            "Join the bot's voice channel to control music",
        ));
    }
    Ok(())
}
pub fn all() -> Vec<poise::Command<Data, Error>> {
    vec![
        play(),
        stay_command(),
        pause(),
        resume(),
        skip(),
        stop(),
        queue(),
        volume(),
        nowplaying(),
        lavlink_status(),
        dj_role(),
        vote_skip(),
        shuffle(),
        repeat(),
        playlist(),
    ]
}
/// Show private Lavalink connection, version, and active queue health.
#[poise::command(
    slash_command,
    guild_only,
    rename = "lavlink-status",
    user_cooldown = 10
)]
async fn lavlink_status(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let Some(node) = &ctx.data().music else {
        return reply(ctx, "Music node is not configured.").await;
    };
    let started = Instant::now();
    let info = match node.request(reqwest::Method::GET, "/v4/info", None).await {
        Ok(info) => info,
        Err(_) => {
            return reply(
                ctx,
                "Lavalink is unavailable. The bot will reconnect automatically.",
            )
            .await;
        }
    };
    let connected = node.session.lock().await.is_some();
    let state = node.state.lock().await.clone();
    reply(ctx, &format!("**Lavalink:** {}\n**Version:** {}\n**Response:** {} ms\n**Queued:** {} / {}\n**Playback:** {}",
        if connected { "Connected" } else { "Reconnecting" },
        info.pointer("/version/semver").and_then(Value::as_str).unwrap_or("Unknown"),
        started.elapsed().as_millis(), state.queue.len(), MAX_QUEUE,
        if state.current.is_some() { "Paused or playing" } else { "Idle" })).await
}
/// Search for a song or queue a supported link and open the music panel.
#[poise::command(slash_command, guild_only, user_cooldown = 5)]
async fn play(
    ctx: Context<'_>,
    #[description = "Song title or supported HTTPS link"] query: String,
    #[description = "Search source; default YouTube"] source: Option<Source>,
) -> Result<(), Error> {
    let node = service(ctx)?;
    let text = requests::request(
        ctx.serenity_context(),
        &node,
        ctx.author().id,
        ctx.channel_id(),
        &query,
        source.unwrap_or(Source::YouTube),
    )
    .await?;
    reply(ctx, text).await
}

async fn enqueue(ctx: Context<'_>, node: &Service, tracks: Vec<Track>) -> Result<(), Error> {
    let message = enqueue_user(
        ctx.serenity_context(),
        node,
        ctx.author().id,
        ctx.channel_id(),
        tracks,
    )
    .await?;
    reply(ctx, message).await
}
async fn enqueue_user(
    ctx: &s::Context,
    node: &Service,
    user: s::UserId,
    text: s::ChannelId,
    tracks: Vec<Track>,
) -> Result<String, Error> {
    let channel = voice_channel(ctx, node.guild, user)
        .ok_or_else(|| crate::errors::user("Join a voice channel first"))?;
    let cfg = crate::store::settings(&node.db, node.guild.get()).await?;
    if cfg.music_stay_channel.is_some_and(|id| id != channel.get()) {
        return Err(crate::errors::user(
            "24/7 mode is assigned to another voice channel. Join that channel to request music.",
        ));
    }
    let guild = node.guild.to_partial_guild(&ctx.http).await?;
    let voice = channel
        .to_channel(&ctx.http)
        .await?
        .guild()
        .ok_or_else(|| crate::errors::user("Voice channel required"))?;
    let member = node.guild.member(&ctx.http, user).await?;
    if member.user.bot || member.pending {
        return Err(crate::errors::user(
            "Complete server membership screening before requesting music.",
        ));
    }
    let bot = node.guild.member(&ctx.http, node.bot).await?;
    let panel_id = node.state.lock().await.text.unwrap_or(text);
    let panel = panel_id
        .to_channel(&ctx.http)
        .await?
        .guild()
        .ok_or_else(|| crate::errors::user("Server text channel required"))?;
    if voice.guild_id != node.guild
        || voice.kind != s::ChannelType::Voice
        || panel.guild_id != node.guild
        || panel.kind != s::ChannelType::Text
        || !guild
            .user_permissions_in(&voice, &member)
            .contains(s::Permissions::VIEW_CHANNEL | s::Permissions::CONNECT)
        || !guild.user_permissions_in(&voice, &bot).contains(
            s::Permissions::VIEW_CHANNEL | s::Permissions::CONNECT | s::Permissions::SPEAK,
        )
        || !guild
            .user_permissions_in(&panel, &member)
            .contains(s::Permissions::VIEW_CHANNEL)
        || !guild.user_permissions_in(&panel, &bot).contains(
            s::Permissions::VIEW_CHANNEL
                | s::Permissions::SEND_MESSAGES
                | s::Permissions::EMBED_LINKS
                | s::Permissions::ATTACH_FILES,
        )
    {
        return Err(crate::errors::user(
            "Missing voice or music panel permissions",
        ));
    }
    if node
        .state
        .lock()
        .await
        .channel
        .is_some_and(|id| id != channel)
    {
        return Err(crate::errors::user("Join the bot's voice channel first"));
    }
    if tracks.is_empty() {
        return Err(crate::errors::user("No playable tracks were found."));
    }
    if node.state.lock().await.queue.len() >= MAX_QUEUE {
        return Err(crate::errors::user("Queue is full (50 tracks)."));
    }
    let join = {
        let mut state = node.state.lock().await;
        let join = state.channel.is_none();
        state.channel = Some(channel);
        if state.text.is_none() {
            state.text = Some(text);
        }
        let space = MAX_QUEUE - state.queue.len();
        let count = tracks.len().min(space);
        for track in tracks.into_iter().take(space) {
            let id = state.next_id;
            state.next_id = state.next_id.saturating_add(1);
            state.queue.push_back(Request {
                track,
                user: user.get(),
                id,
            });
        }
        state.last_active = Instant::now();
        (join, count)
    };
    if join.0 {
        node.voice_state(ctx, Some(channel));
        let ready = tokio::time::timeout(Duration::from_secs(12), async {
            loop {
                let notified = node.voice.notified();
                if node.state.lock().await.voice_ready {
                    break;
                }
                notified.await;
            }
        })
        .await;
        if ready.is_err() {
            node.stop(ctx).await?;
            return Err(crate::errors::user(
                "Voice connection timed out. Check Connect/Speak permissions and try again.",
            ));
        }
    }
    let empty = node.state.lock().await.current.is_none();
    let updated = if empty {
        node.next(ctx).await
    } else {
        node.panel(ctx).await
    };
    if let Err(error) = updated {
        tracing::warn!(%error,"Music request stored but player/panel update was not confirmed");
        return Ok(format!(
            "Added {} track(s) to the local queue, but playback or the panel update could not be confirmed. Check /nowplaying or /lavlink-status before submitting the same request again.",
            join.1
        ));
    }
    Ok(format!(
        "Queued {} track(s). Controls are in <#{}>.",
        join.1,
        node.state
            .lock()
            .await
            .text
            .ok_or_else(|| crate::errors::user("Missing panel channel"))?
    ))
}
async fn action(
    ctx: &s::Context,
    node: &Service,
    user: s::UserId,
    action: &str,
    value: Option<u8>,
    data: &Data,
) -> Result<String, Error> {
    if !["queue", "vote", "details", "help"].contains(&action) {
        dj_permission(ctx, node, user).await?;
    }
    match action {
        "vote" => return cast_skip_vote(ctx, node, user).await,
        "help"=>return Ok("**Listening Room / control guide**\n\nJoin the bot's voice channel first. Everyone listening can view the queue, inspect the track and vote to skip. A majority of eligible listeners skips the track.\nIf a DJ role is configured, playback changes require that role or Manage Server. Seek controls move 30 seconds and are unavailable for live/non-seekable tracks. Loop cycles Off → Track → Queue. Volume presets include mute.\nThe player updates about every 15 seconds. Stop clears the queue, disconnects and removes the panel; empty channels clean up automatically.".into()),
        "details"=>{
            let state=node.state.lock().await;
            let r=state.current.as_ref().ok_or_else(||crate::errors::user("Nothing is playing. Add a track with /play."))?;
            return Ok(format!("**{}**\n{}\n\nDuration: **{}**\nPosition: **{}**\nSeeking: **{}**\nRequested by: <@{}>\n\nPlayback controls apply only to the current voice session.",safe(&r.track.info.title,160),safe(&r.track.info.author,120),track_duration(&r.track.info),time(state.position),if r.track.info.is_seekable && !r.track.info.is_stream {"Available"}else{"Unavailable"},r.user));
        }
        "back"|"forward"|"restart"=>{
            let state=node.state.lock().await.clone();
            let current=state.current.as_ref().ok_or_else(||crate::errors::user("Nothing is playing. Add a track with /play."))?;
            seek_target(&current.track.info,state.position,action)?;
            let session=node.session.lock().await.clone().ok_or_else(||crate::errors::user("Audio node is reconnecting. Try again shortly."))?;
            let live=node.request(reqwest::Method::GET,&format!("/v4/sessions/{session}/players/{}",node.guild),None).await?;
            if live["track"]["userData"]["requestId"].as_u64()!=Some(current.id){return Err(crate::errors::user("The track changed. Wait for the panel to update, then try again."));}
            let position=live["state"]["position"].as_u64().ok_or_else(||crate::errors::user("Current playback position is unavailable. Try again shortly."))?;
            let target=seek_target(&current.track.info,position,action)?;
            node.patch(json!({"position":target})).await?;
            node.state.lock().await.position=target;
            node.panel(ctx).await?;
            return Ok(format!("Playback moved to **{}** / {}.",time(target),track_duration(&current.track.info)));
        }
        "shuffle" => {
            if node.state.lock().await.queue.len()<2 {return Err(crate::errors::user("Add at least two upcoming tracks before shuffling."));}
            node.state
                .lock()
                .await
                .queue
                .make_contiguous()
                .shuffle(&mut rand::thread_rng());
        }
        "repeat" => {
            let mut state = node.state.lock().await;
            state.repeat = match state.repeat {
                RepeatMode::Off => RepeatMode::Track,
                RepeatMode::Track => RepeatMode::Queue,
                RepeatMode::Queue => RepeatMode::Off,
            };
        }
        "stop" => {
            stay::disable(data).await?;
            node.stop(ctx).await?;
            return Ok("Stopped, queue cleared, and disconnected.".into());
        }
        "skip" => node.next(ctx).await?,
        "pause" | "resume" | "toggle" => {
            let paused = if action == "toggle" {
                !node.state.lock().await.paused
            } else {
                action == "pause"
            };
            if node.state.lock().await.current.is_none() {
                return Err(crate::errors::user("Nothing is playing"));
            }
            node.patch(json!({"paused":paused})).await?;
            {let mut state=node.state.lock().await;state.paused=paused;if let Some(summary)=state.summary.as_mut(){summary.active(!paused,Instant::now());}}
            node.channel_status(ctx).await;
        }
        "volume" | "less" | "more" => {
            let volume = if action == "less" {
                node.state.lock().await.volume.saturating_sub(10)
            } else if action == "more" {
                node.state.lock().await.volume.saturating_add(10).min(100)
            } else {
                value.ok_or_else(|| crate::errors::user("Select volume"))?
            };
            if volume > 100 {
                return Err(crate::errors::user("Volume must be 0–100"));
            }
            node.patch(json!({"volume":volume})).await?;
            node.state.lock().await.volume = volume;
        }
        "queue" => {return Ok(queue_text(&*node.state.lock().await));}
        _ => return Err(crate::errors::user("Unknown control")),
    }
    node.panel(ctx).await?;
    let state = node.state.lock().await;
    Ok(match action {
        "shuffle" => format!(
            "Shuffled **{}** upcoming tracks. The current song stays in place.",
            state.queue.len()
        ),
        "repeat" => format!("Loop set to **{}**.", repeat_label(state.repeat)),
        "pause" | "resume" | "toggle" => {
            if state.paused {
                "Playback paused. Choose Resume when you're ready.".into()
            } else {
                "Playback resumed.".into()
            }
        }
        "volume" | "less" | "more" => format!(
            "Volume set to **{}%**{}.",
            state.volume,
            if state.volume == 0 { " (muted)" } else { "" }
        ),
        "skip" => "Skipped the track.".into(),
        _ => "Playback updated.".into(),
    })
}
async fn control(ctx: Context<'_>, verb: &str, value: Option<u8>) -> Result<(), Error> {
    let node = service(ctx)?;
    let _guard = node.operations.lock().await;
    permission(ctx.serenity_context(), &node, ctx.author().id).await?;
    let result = action(
        ctx.serenity_context(),
        &node,
        ctx.author().id,
        verb,
        value,
        ctx.data(),
    )
    .await?;
    reply(ctx, result).await
}
/// Pause the current song (listeners in the same voice channel).
#[poise::command(slash_command, guild_only, user_cooldown = 2)]
async fn pause(ctx: Context<'_>) -> Result<(), Error> {
    control(ctx, "pause", None).await
}
/// Resume the paused song.
#[poise::command(slash_command, guild_only, user_cooldown = 2)]
async fn resume(ctx: Context<'_>) -> Result<(), Error> {
    control(ctx, "resume", None).await
}
/// Skip the current song and advance the queue.
#[poise::command(slash_command, guild_only, user_cooldown = 2)]
async fn skip(ctx: Context<'_>) -> Result<(), Error> {
    control(ctx, "skip", None).await
}
/// Clear the queue and leave voice.
#[poise::command(slash_command, guild_only, user_cooldown = 2)]
async fn stop(ctx: Context<'_>) -> Result<(), Error> {
    control(ctx, "stop", None).await
}
/// Show the next queued songs.
#[poise::command(slash_command, guild_only)]
async fn queue(ctx: Context<'_>) -> Result<(), Error> {
    control(ctx, "queue", None).await
}
/// Set playback volume safely between 0 and 100 percent.
#[poise::command(slash_command, guild_only, user_cooldown = 2)]
async fn volume(
    ctx: Context<'_>,
    #[description = "Percent, 0–100"] percent: u8,
) -> Result<(), Error> {
    control(ctx, "volume", Some(percent)).await
}
/// Refresh the music panel and show its channel.
#[poise::command(slash_command, guild_only)]
async fn nowplaying(ctx: Context<'_>) -> Result<(), Error> {
    let node = service(ctx)?;
    node.panel(ctx.serenity_context()).await?;
    reply(
        ctx,
        format!(
            "Panel: {}",
            node.state
                .lock()
                .await
                .text
                .map(|id| format!("<#{id}>"))
                .unwrap_or_else(|| "No active panel. Use /play.".into())
        ),
    )
    .await
}
async fn dj_permission(ctx: &s::Context, node: &Service, user: s::UserId) -> Result<(), Error> {
    let cfg = crate::store::settings(&node.db, node.guild.get()).await?;
    if let Some(role) = cfg.music_dj_role {
        let guild = node.guild.to_partial_guild(&ctx.http).await?;
        let member = node.guild.member(&ctx.http, user).await?;
        if !member.roles.iter().any(|id| id.get() == role)
            && !guild
                .member_permissions(&member)
                .contains(s::Permissions::MANAGE_GUILD)
        {
            return Err(crate::errors::user(
                "Music controls require the configured DJ role. Listeners can use /vote-skip.",
            ));
        }
    }
    Ok(())
}
fn majority(listeners: usize) -> usize {
    listeners / 2 + 1
}
async fn cast_skip_vote(
    ctx: &s::Context,
    node: &Service,
    user: s::UserId,
) -> Result<String, Error> {
    permission(ctx, node, user).await?;
    let channel = node
        .state
        .lock()
        .await
        .channel
        .ok_or_else(|| crate::errors::user("No voice connection"))?;
    let listeners: HashSet<u64> = {
        let guild = ctx
            .cache
            .guild(node.guild)
            .ok_or_else(|| crate::errors::user("Voice roster unavailable"))?;
        guild
            .voice_states
            .iter()
            .filter(|(_, voice)| voice.channel_id == Some(channel))
            .filter(|(id, voice)| {
                voice
                    .member
                    .as_ref()
                    .or_else(|| guild.members.get(id))
                    .is_some_and(|m| !m.user.bot)
            })
            .map(|(id, _)| id.get())
            .collect()
    };
    if !listeners.contains(&user.get()) {
        return Err(crate::errors::user(
            "Your current voice membership could not be verified",
        ));
    }
    let (votes, required) = {
        let mut state = node.state.lock().await;
        if state.current.is_none() {
            return Err(crate::errors::user("Nothing is playing"));
        }
        state.skip_votes.retain(|id| listeners.contains(id));
        state.skip_votes.insert(user.get());
        (state.skip_votes.len(), majority(listeners.len()))
    };
    if votes >= required {
        node.next(ctx).await?;
        Ok(format!("Vote passed ({votes}/{required}); skipped."))
    } else {
        node.panel(ctx).await?;
        Ok(format!(
            "Skip vote recorded: {votes}/{required}. One vote per listener."
        ))
    }
}
/// Require a DJ role for music controls; listeners retain play and vote-skip.
#[poise::command(
    slash_command,
    guild_only,
    rename = "dj-role",
    required_permissions = "MANAGE_GUILD"
)]
async fn dj_role(
    ctx: Context<'_>,
    #[description = "DJ role"] role: Option<s::Role>,
    #[description = "Enable DJ restriction or disable it"] enabled: Option<bool>,
) -> Result<(), Error> {
    let _lock = ctx.data().configuration_lock.lock().await;
    let mut cfg = ctx.data().settings.write().await.clone();
    if enabled == Some(false) {
        cfg.music_dj_role = None;
    } else {
        let role =
            role.ok_or_else(|| crate::errors::user("Select a DJ role or set enabled false"))?;
        let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
        if role.id.get() == guild.id.get() || role.managed || !guild.roles.contains_key(&role.id) {
            return Err(crate::errors::user(
                "Choose an ordinary role in this server",
            ));
        }
        cfg.music_dj_role = Some(role.id.get());
    }
    crate::store::save(&ctx.data().db, ctx.data().guild.get(), &cfg).await?;
    *ctx.data().settings.write().await = cfg;
    reply(
        ctx,
        "DJ policy updated. Joining the same voice channel is still required.",
    )
    .await
}
/// Cast one vote to skip; a majority of current human listeners must agree.
#[poise::command(slash_command, guild_only, rename = "vote-skip", user_cooldown = 2)]
async fn vote_skip(ctx: Context<'_>) -> Result<(), Error> {
    control(ctx, "vote", None).await
}
/// Shuffle waiting tracks without changing the current song.
#[poise::command(slash_command, guild_only, user_cooldown = 2)]
async fn shuffle(ctx: Context<'_>) -> Result<(), Error> {
    control(ctx, "shuffle", None).await
}
/// Repeat the current track, cycle the queue, or turn repeat off.
#[poise::command(slash_command, guild_only, user_cooldown = 2)]
async fn repeat(
    ctx: Context<'_>,
    #[description = "Repeat behavior"] mode: RepeatMode,
) -> Result<(), Error> {
    let node = service(ctx)?;
    let _guard = node.operations.lock().await;
    permission(ctx.serenity_context(), &node, ctx.author().id).await?;
    dj_permission(ctx.serenity_context(), &node, ctx.author().id).await?;
    node.state.lock().await.repeat = mode;
    node.panel(ctx.serenity_context()).await?;
    reply(
        ctx,
        format!("Repeat: {mode:?}. Manual skips and failed tracks always advance."),
    )
    .await
}
fn playlist_name(name: &str) -> Result<String, Error> {
    let name = name.trim().to_ascii_lowercase();
    if name.is_empty()
        || name.len() > 32
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(crate::errors::user(
            "Playlist names use 1–32 letters, numbers, hyphens, or underscores",
        ));
    }
    Ok(name)
}
/// Manage your private saved playlists.
#[poise::command(
    slash_command,
    guild_only,
    subcommands(
        "playlist_save",
        "playlist_play",
        "playlist_list",
        "playlist_delete",
        "playlist_favorites",
        "playlist_always_on"
    )
)]
async fn playlist(ctx: Context<'_>) -> Result<(), Error> {
    reply(
        ctx,
        "Choose save, play, list, delete, favorites or always-on.",
    )
    .await
}
/// Save the current song and waiting queue to your private playlist.
#[poise::command(slash_command, guild_only, rename = "save", user_cooldown = 5)]
async fn playlist_save(
    ctx: Context<'_>,
    #[description = "Private playlist name"] name: String,
) -> Result<(), Error> {
    let name = playlist_name(&name)?;
    let node = service(ctx)?;
    let _guard = node.operations.lock().await;
    permission(ctx.serenity_context(), &node, ctx.author().id).await?;
    let state = node.state.lock().await.clone();
    let tracks: Vec<String> = state
        .current
        .iter()
        .chain(state.queue.iter())
        .take(MAX_QUEUE)
        .map(|r| {
            r.track
                .info
                .uri
                .clone()
                .ok_or_else(|| crate::errors::user("A track has no shareable source URL"))
        })
        .collect::<Result<_, _>>()?;
    if tracks.is_empty() {
        return Err(crate::errors::user("Nothing to save"));
    }
    for uri in &tracks {
        identifier(uri, Source::YouTube)?;
    }
    let changed=sqlx::query("INSERT INTO playlists(guild,user,name,tracks) SELECT ?,?,?,? WHERE EXISTS(SELECT 1 FROM playlists WHERE guild=? AND user=? AND name=?) OR ((SELECT COUNT(*) FROM playlists WHERE guild=? AND user=?)<20 AND (SELECT COUNT(*) FROM playlists WHERE guild=?)<100) ON CONFLICT(guild,user,name) DO UPDATE SET tracks=excluded.tracks")
        .bind(node.guild.to_string()).bind(ctx.author().id.to_string()).bind(&name).bind(serde_json::to_string(&tracks)?)
        .bind(node.guild.to_string()).bind(ctx.author().id.to_string()).bind(&name).bind(node.guild.to_string()).bind(ctx.author().id.to_string()).bind(node.guild.to_string()).execute(&node.db).await?.rows_affected();
    if changed == 0 {
        return Err(crate::errors::user(
            "Playlist limit reached (20 per member / 100 server)",
        ));
    }
    reply(
        ctx,
        format!(
            "Saved private playlist **{name}** with {} tracks.",
            tracks.len()
        ),
    )
    .await
}
/// Browse or play shared favorites; members can remove their own saved links.
#[poise::command(slash_command, guild_only, rename = "favorites", user_cooldown = 5)]
async fn playlist_favorites(
    ctx: Context<'_>,
    action: favorites::FavoriteAction,
    #[min = 1]
    #[max = 20]
    page: Option<u16>,
    link: Option<String>,
) -> Result<(), Error> {
    let node = service(ctx)?;
    let text = favorites::action(
        ctx.serenity_context(),
        &node,
        ctx.author().id,
        ctx.channel_id(),
        action,
        page.unwrap_or(1),
        link.as_deref(),
    )
    .await?;
    reply(ctx, text).await
}
#[derive(Clone, Copy, poise::ChoiceParameter)]
enum StayAction {
    Enable,
    Disable,
}
/// Enable or disable persistent voice presence; select a voice channel when enabling.
#[poise::command(
    slash_command,
    guild_only,
    rename = "24-7",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD",
    user_cooldown = 3
)]
async fn stay_command(
    ctx: Context<'_>,
    #[description = "Enable or Disable 24/7 voice mode"] action: StayAction,
    #[description = "Voice channel; required when enabling"]
    #[channel_types("Voice")]
    channel: Option<s::GuildChannel>,
) -> Result<(), Error> {
    stay::configure(ctx, matches!(action, StayAction::Enable), channel).await
}
/// Keep the bot in a selected voice channel while idle, including across restarts.
#[poise::command(
    slash_command,
    guild_only,
    rename = "always-on",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD"
)]
async fn playlist_always_on(
    ctx: Context<'_>,
    enabled: bool,
    channel: Option<s::GuildChannel>,
) -> Result<(), Error> {
    stay::configure(ctx, enabled, channel).await
}
/// Add songs from your own saved playlist to the shared voice queue.
#[poise::command(slash_command, guild_only, rename = "play", user_cooldown = 10)]
async fn playlist_play(
    ctx: Context<'_>,
    #[description = "Private playlist name"] name: String,
) -> Result<(), Error> {
    let name = playlist_name(&name)?;
    let node = service(ctx)?;
    let _guard = node.operations.lock().await;
    let voice = voice_channel(ctx.serenity_context(), node.guild, ctx.author().id)
        .ok_or_else(|| crate::errors::user("Join voice first"))?;
    if node
        .state
        .lock()
        .await
        .channel
        .is_some_and(|id| id != voice)
    {
        return Err(crate::errors::user("Join the bot's voice channel"));
    }
    if node.session.lock().await.is_none() {
        return Err(crate::errors::user("Audio server is reconnecting"));
    }
    let row = sqlx::query("SELECT tracks FROM playlists WHERE guild=? AND user=? AND name=?")
        .bind(node.guild.to_string())
        .bind(ctx.author().id.to_string())
        .bind(&name)
        .fetch_optional(&node.db)
        .await?
        .ok_or_else(|| crate::errors::user("Your playlist was not found"))?;
    let uris: Vec<String> = serde_json::from_str(row.get("tracks"))?;
    if uris.is_empty() || uris.len() > MAX_QUEUE {
        return Err(crate::errors::user("Invalid saved playlist"));
    }
    let space = MAX_QUEUE.saturating_sub(node.state.lock().await.queue.len());
    if space == 0 {
        return Err(crate::errors::user("Queue is full"));
    }
    let tracks = tokio::time::timeout(Duration::from_secs(30), async {
        let mut tracks = Vec::new();
        for uri in uris.iter().take(space) {
            identifier(uri, Source::YouTube)?;
            let loaded = node.load(uri, Source::YouTube).await?;
            if let Some(track) = loaded.into_iter().next() {
                tracks.push(track);
            }
        }
        Ok::<_, Error>(tracks)
    })
    .await
    .map_err(|_| crate::errors::user("Playlist loading timed out; no tracks were queued"))??;
    if tracks.is_empty() {
        return Err(crate::errors::user("No playable tracks found"));
    }
    enqueue(ctx, &node, tracks).await
}
/// List only your private playlist names and track counts.
#[poise::command(slash_command, guild_only, rename = "list")]
async fn playlist_list(ctx: Context<'_>) -> Result<(), Error> {
    let rows =
        sqlx::query("SELECT name,tracks FROM playlists WHERE guild=? AND user=? ORDER BY name")
            .bind(ctx.data().guild.to_string())
            .bind(ctx.author().id.to_string())
            .fetch_all(&ctx.data().db)
            .await?;
    let mut lines = Vec::new();
    for row in rows {
        let tracks: Vec<String> = serde_json::from_str(row.get("tracks"))?;
        lines.push(format!(
            "**{}** · {} tracks",
            row.get::<String, _>("name"),
            tracks.len()
        ));
    }
    reply(
        ctx,
        if lines.is_empty() {
            "No saved playlists.".into()
        } else {
            lines.join("\n")
        },
    )
    .await
}
/// Delete one of your own saved playlists.
#[poise::command(slash_command, guild_only, rename = "delete")]
async fn playlist_delete(
    ctx: Context<'_>,
    #[description = "Private playlist name"] name: String,
    #[description = "Confirm deletion"] confirm: bool,
) -> Result<(), Error> {
    if !confirm {
        return reply(ctx, "Set confirm true to delete this playlist.").await;
    }
    let name = playlist_name(&name)?;
    let changed = sqlx::query("DELETE FROM playlists WHERE guild=? AND user=? AND name=?")
        .bind(ctx.data().guild.to_string())
        .bind(ctx.author().id.to_string())
        .bind(name)
        .execute(&ctx.data().db)
        .await?
        .rows_affected();
    reply(
        ctx,
        if changed == 1 {
            "Your playlist was deleted."
        } else {
            "Your playlist was not found."
        },
    )
    .await
}
pub async fn gateway(ctx: &s::Context, event: &s::FullEvent, data: &Data) {
    let Some(node) = &data.music else { return };
    if matches!(event,s::FullEvent::GuildCreate{guild,..} if guild.id==node.guild)
        && let Err(error) = refresh_requests(ctx, data).await
    {
        tracing::warn!(%error,"Music request panel refresh failed");
    }
    if let s::FullEvent::Message { new_message } = event {
        requests::message(ctx, data, node, new_message).await;
        favorites::message(ctx, data, node, new_message).await;
    }
    if let s::FullEvent::InteractionCreate {
        interaction: s::Interaction::Component(button),
    } = event
        && button.guild_id == Some(node.guild)
        && button.message.author.id == node.bot
        && button.data.custom_id == "elvnx:music:add"
    {
        requests::modal(ctx, data, node, button).await;
        return;
    }
    if let s::FullEvent::InteractionCreate {
        interaction: s::Interaction::Component(button),
    } = event
        && button.guild_id == Some(node.guild)
        && button.message.author.id == node.bot
        && ["elvnx:music:play-favorites", "elvnx:music:view-favorites"]
            .contains(&button.data.custom_id.as_str())
    {
        favorites::button(ctx, node, button).await;
        return;
    }
    match event {
        s::FullEvent::VoiceStateUpdate { new, .. }
            if new.guild_id == Some(node.guild) && new.user_id == node.bot =>
        {
            if new.channel_id.is_none() {
                let _guard = node.operations.lock().await;
                if node.state.lock().await.channel.is_some() {
                    let _ = node.stop(ctx).await;
                }
            } else {
                {
                    let mut state = node.state.lock().await;
                    if state.channel.is_some() {
                        state.channel = new.channel_id;
                        state.voice_session = Some(new.session_id.clone());
                    }
                }
                if node.connect_voice().await.is_err() {
                    tracing::warn!("Lavalink voice handshake failed");
                }
            }
        }
        s::FullEvent::VoiceServerUpdate { event: e } if e.guild_id == Some(node.guild) => {
            {
                let mut state = node.state.lock().await;
                state.voice_token = Some(e.token.clone());
                state.endpoint = e.endpoint.clone();
            }
            if node.connect_voice().await.is_err() {
                tracing::warn!("Lavalink voice handshake failed");
            }
        }
        s::FullEvent::InteractionCreate {
            interaction: s::Interaction::Component(button),
        } if button.guild_id == Some(node.guild)
            && button.data.custom_id.starts_with("elvnx:music:")
            && button.message.author.id == node.bot =>
        {
            let _ = button
                .create_response(
                    &ctx.http,
                    s::CreateInteractionResponse::Defer(
                        s::CreateInteractionResponseMessage::new().ephemeral(true),
                    ),
                )
                .await;
            let outcome: Result<String, Error> = async {
                let _guard = node.operations.lock().await;
                let state = node.state.lock().await.clone();
                if state.panel != Some(button.message.id) || state.text != Some(button.channel_id) {
                    return Err(crate::errors::user(
                        "This panel is outdated. Use /nowplaying.",
                    ));
                }
                permission(ctx, node, button.user.id).await?;
                {
                    let mut clicks = node.buttons.lock().await;
                    clicks.retain(|_, time| time.elapsed() < Duration::from_secs(10));
                    if clicks
                        .get(&button.user.id.get())
                        .is_some_and(|time| time.elapsed() < Duration::from_secs(2))
                    {
                        return Err(crate::errors::user(
                            "Wait two seconds between music controls.",
                        ));
                    }
                    if clicks.len() >= 1000 {
                        return Err(crate::errors::user(
                            "Music controls are busy. Try again shortly.",
                        ));
                    }
                    clicks.insert(button.user.id.get(), Instant::now());
                }
                let verb = button.data.custom_id.trim_start_matches("elvnx:music:");
                let value = if verb == "volume" {
                    let s::ComponentInteractionDataKind::StringSelect { values } =
                        &button.data.kind
                    else {
                        return Err(crate::errors::user("Choose a volume preset from the menu."));
                    };
                    Some(
                        values
                            .first()
                            .and_then(|v| v.parse::<u8>().ok())
                            .filter(|v| [0, 10, 25, 50, 75, 100].contains(v))
                            .ok_or_else(|| {
                                crate::errors::user("Choose one of the displayed volume presets.")
                            })?,
                    )
                } else {
                    None
                };
                action(
                    ctx,
                    node,
                    button.user.id,
                    if verb == "pause" { "toggle" } else { verb },
                    value,
                    data,
                )
                .await
            }
            .await;
            let text = match outcome {
                Ok(text) => text,
                Err(error) => {
                    crate::errors::with_reference(error.as_ref(), "play", button.id.get())
                }
            };
            let _ = button
                .edit_response(
                    &ctx.http,
                    s::EditInteractionResponse::new()
                        .embed(
                            crate::emoji::embed_for("headphones")
                                .title("ELVNX / MUSIC")
                                .description(text)
                                .color(0x111111),
                        )
                        .allowed_mentions(s::CreateAllowedMentions::new()),
                )
                .await;
        }
        _ => {}
    }
}

pub async fn refresh_requests(ctx: &s::Context, data: &Data) -> Result<(), Error> {
    let _guard = data.configuration_lock.lock().await;
    requests::refresh(ctx, data).await?;
    favorites::refresh(ctx, data).await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn track(id: u64) -> Request {
        Request {
            track: Track {
                encoded: id.to_string(),
                info: Info {
                    title: format!("Song {id}"),
                    author: "artist".into(),
                    length: 10000,
                    is_stream: false,
                    is_seekable: true,
                    uri: Some("https://youtu.be/test".into()),
                },
            },
            user: 1,
            id,
        }
    }

    #[test]
    fn music_features_keep_the_command_allocation_and_protect_stay_configuration() {
        let commands = crate::commands::all();
        assert_eq!(commands.len(), COMMAND_COUNT);
        let parent = commands.iter().find(|c| c.name == "playlist").unwrap();
        let stay = parent
            .subcommands
            .iter()
            .find(|c| c.name == "always-on")
            .unwrap();
        assert!(
            stay.required_permissions
                .contains(s::Permissions::MANAGE_GUILD)
        );
        assert!(parent.subcommands.iter().any(|c| c.name == "favorites"));
        let cfg: crate::store::Settings = serde_json::from_str("{}").unwrap();
        assert!(cfg.music_request_channel.is_none());
        assert!(cfg.music_stay_channel.is_none());
        assert!(!cfg.music_recaps_disabled);
    }
    #[test]
    fn seeking_clamps_positions_and_rejects_live_or_nonseekable_tracks() {
        let mut info = track(1).track.info;
        info.length = 120_000;
        assert_eq!(seek_target(&info, 10_000, "back").unwrap(), 0);
        assert_eq!(seek_target(&info, 110_000, "forward").unwrap(), 119_999);
        assert_eq!(seek_target(&info, u64::MAX, "forward").unwrap(), 119_999);
        assert_eq!(seek_target(&info, 90_000, "restart").unwrap(), 0);
        assert!(seek_target(&info, 1, "invalid").is_err());
        info.is_stream = true;
        assert!(seek_target(&info, 1, "forward").is_err());
        info.is_stream = false;
        info.is_seekable = false;
        assert!(seek_target(&info, 1, "restart").is_err());
        info.is_seekable = true;
        info.length = 0;
        assert!(seek_target(&info, 1, "back").is_err());
    }
    #[test]
    fn premium_controls_disable_unsupported_actions_and_use_animated_custom_icons() {
        let mut state = State {
            channel: Some(s::ChannelId::new(1)),
            current: Some(track(1)),
            volume: 100,
            ..Default::default()
        };
        state.current.as_mut().unwrap().track.info.is_stream = true;
        let json = serde_json::to_value(player_controls(&state)).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 4);
        let buttons = json
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|r| r["components"].as_array().unwrap())
            .collect::<Vec<_>>();
        for action in ["back", "forward", "restart", "shuffle", "more"] {
            assert_eq!(
                buttons
                    .iter()
                    .find(|b| b["custom_id"] == format!("elvnx:music:{action}"))
                    .unwrap()["disabled"],
                true
            );
        }
        assert_eq!(
            buttons
                .iter()
                .find(|b| b["custom_id"] == "elvnx:music:pause")
                .unwrap()["disabled"],
            false
        );
        for row in json.as_array().unwrap().iter().take(3) {
            assert!(row["components"].as_array().unwrap().len() <= 5);
            for button in row["components"].as_array().unwrap() {
                assert!(button["emoji"]["id"].is_string());
                assert_eq!(button["emoji"]["animated"], true);
            }
        }
        state.paused = true;
        state.repeat = RepeatMode::Queue;
        let json = serde_json::to_value(player_controls(&state)).unwrap();
        assert_eq!(json[0]["components"][0]["label"], "Resume");
        assert_eq!(json[1]["components"][3]["label"], "Loop: Queue");
    }
    #[test]
    fn player_embed_is_bounded_with_untrusted_long_metadata_and_live_queue() {
        let mut current = track(1);
        current.track.info.title = "😀[`*".repeat(500);
        current.track.info.author = "artist".repeat(500);
        let mut live = track(2);
        live.track.info.is_stream = true;
        let state = State {
            channel: Some(s::ChannelId::new(1)),
            current: Some(current.clone()),
            queue: VecDeque::from([current, live]),
            ..Default::default()
        };
        let json = serde_json::to_value(player_embed(&state, 12)).unwrap();
        assert!(json["description"].as_str().unwrap().encode_utf16().count() < 4096);
        let mut total = json["description"].as_str().unwrap().encode_utf16().count();
        for field in json["fields"].as_array().unwrap() {
            let name = field["name"].as_str().unwrap();
            let value = field["value"].as_str().unwrap();
            assert!(name.encode_utf16().count() <= 256);
            assert!(value.encode_utf16().count() <= 1024);
            total += name.encode_utf16().count() + value.encode_utf16().count();
        }
        assert!(total < 5500);
        assert!(json.to_string().contains("Live / variable"));
        assert_eq!(json["image"]["url"], "attachment://listening-room.png");
    }
    #[test]
    fn full_queue_includes_all_fifty_entries_within_the_discord_budget() {
        let mut state = State::default();
        for id in 1..=MAX_QUEUE as u64 {
            let mut r = track(id);
            r.track.info.title = "😀".repeat(1000);
            r.track.info.length = u64::MAX;
            state.queue.push_back(r);
        }
        let text = queue_text(&state);
        assert!(text.encode_utf16().count() <= 4096);
        assert!(text.contains("`50`"));
        assert!(!text.contains("`51`"));
        assert_eq!(text.lines().filter(|l| l.starts_with('`')).count(), 50);
    }
    #[test]
    fn invalid_track_link_does_not_break_the_panel() {
        let mut r = track(1);
        r.track.info.uri = Some("not a URL".into());
        let mut state = State {
            current: Some(r),
            ..Default::default()
        };
        let json = serde_json::to_value(player_embed(&state, 0)).unwrap();
        assert!(json.get("url").is_none());
        state.current.as_mut().unwrap().track.info.uri = Some("https://youtu.be/example".into());
        let json = serde_json::to_value(player_embed(&state, 0)).unwrap();
        assert_eq!(json["url"], "https://youtu.be/example");
    }
    #[test]
    fn listening_room_preview_renders_native_png() {
        let bytes = preview().unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(bytes.len() < 2_000_000);
    }
    #[test]
    fn repeat_uses_fresh_request_ids_and_manual_skip_never_loops() {
        let mut state = State {
            current: Some(track(1)),
            queue: VecDeque::from([track(2), track(3)]),
            next_id: 4,
            repeat: RepeatMode::Track,
            ..Default::default()
        };
        let (proposed, next) = next_proposal(&state, true);
        assert_eq!(next.unwrap().id, 4);
        assert_eq!(proposed.queue.len(), 2);
        assert_eq!(state.current.as_ref().unwrap().id, 1);
        let (proposed, next) = next_proposal(&state, false);
        assert_eq!(next.unwrap().id, 2);
        assert_eq!(proposed.queue.len(), 1);
        state.repeat = RepeatMode::Queue;
        let (proposed, next) = next_proposal(&state, true);
        assert_eq!(next.unwrap().id, 2);
        assert_eq!(proposed.queue.len(), 2);
        assert_eq!(proposed.queue.back().unwrap().id, 4);
        state.queue.clear();
        let (proposed, next) = next_proposal(&state, true);
        assert_eq!(next.unwrap().id, 4);
        assert!(proposed.queue.is_empty());
        let (_, next) = next_proposal(&state, false);
        assert!(next.is_none());
        for count in 1..10 {
            assert!(majority(count) > count / 2);
            assert!(majority(count) <= count);
        }
    }
    #[test]
    fn private_playlist_names_cannot_inject_mentions_or_paths() {
        assert_eq!(playlist_name(" My-Songs_1 ").unwrap(), "my-songs_1");
        for name in ["../secrets", "<@123>", "", "a b", "https://example.org"] {
            assert!(playlist_name(name).is_err());
        }
    }
    #[test]
    fn sources_block_local_networks_credentials_and_lookalikes() {
        for url in [
            "https://127.0.0.1/",
            "http://youtube.com/a",
            "https://youtube.com.evil.org/watch?v=x",
            "https://user:pass@youtube.com/a",
            "file:///etc/passwd",
            "https://youtube.com:444/a",
        ] {
            assert!(identifier(url, Source::YouTube).is_err(), "{url}");
        }
        assert!(identifier("https://youtu.be/abc", Source::YouTube).is_ok());
        assert_eq!(
            identifier("hello", Source::Soundcloud).unwrap(),
            "scsearch:hello"
        );
    }
    #[test]
    fn empty_and_failed_loads_do_not_create_fake_tracks() {
        assert!(parse_tracks(json!({"loadType":"empty","data":{}})).is_err());
        assert!(parse_tracks(json!({"loadType":"error","data":{}})).is_err());
        assert_eq!(time(65000), "1:05");
    }
}
