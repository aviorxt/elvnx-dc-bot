//! Self-service, opt-in live presence. Activity payloads never enter SQLite.
use crate::{Context, Error, reply, serenity as s};
use axum::{
    Json, Router,
    extract::{
        ConnectInfo, Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, RwLock, Semaphore};

pub struct Service {
    db: SqlitePool,
    guild: s::GuildId,
    availability: Arc<crate::availability::Service>,
    // This lock also orders revocations against reads and live publications.
    consent: RwLock<std::collections::HashSet<u64>>,
    limits: Mutex<HashMap<IpAddr, (Instant, u32)>>,
    sockets: Arc<Semaphore>,
    listener_enabled: bool,
}

fn fail(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({"success":false,"error":{"code":code}}))).into_response()
}
fn valid_id(raw: &str) -> Option<u64> {
    if raw.is_empty() || raw.len() > 20 || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse::<u64>().ok().filter(|v| *v > 0)
}
fn limited(value: &str, size: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(size)
        .collect()
}
fn status(presence: Option<&s::Presence>) -> &'static str {
    match presence.map(|p| p.status) {
        Some(s::OnlineStatus::Online) => "online",
        Some(s::OnlineStatus::Idle) => "idle",
        Some(s::OnlineStatus::DoNotDisturb) => "dnd",
        _ => "offline",
    }
}
fn activity(a: &s::Activity) -> Value {
    // Deliberate allowlist: no party IDs, join/spectate secrets, session IDs,
    // arbitrary streaming/button URLs, or future Discord fields.
    json!({
        "name":limited(&a.name,128), "type":a.kind,
        "application_id":a.application_id.map(|id| id.to_string()),
        "details":a.details.as_deref().map(|v|limited(v,256)),
        "state":a.state.as_deref().map(|v|limited(v,256)),
        "timestamps":a.timestamps,
        "assets":a.assets.as_ref().map(|v|json!({
            "large_image":v.large_image.as_deref().map(|v|limited(v,512)),
            "large_text":v.large_text.as_deref().map(|v|limited(v,256)),
            "small_image":v.small_image.as_deref().map(|v|limited(v,512)),
            "small_text":v.small_text.as_deref().map(|v|limited(v,256))
        }))
    })
}
fn project(user: &s::User, presence: Option<&s::Presence>) -> Value {
    let state = status(presence);
    let active = state != "offline";
    let activities: Vec<_> = presence
        .filter(|_| active)
        .into_iter()
        .flat_map(|p| p.activities.iter())
        .take(8)
        .map(activity)
        .collect();
    let device = |get: fn(&s::ClientStatus) -> Option<s::OnlineStatus>| {
        active
            && presence
                .and_then(|p| p.client_status.as_ref())
                .and_then(get)
                .is_some_and(|s| {
                    matches!(
                        s,
                        s::OnlineStatus::Online
                            | s::OnlineStatus::Idle
                            | s::OnlineStatus::DoNotDisturb
                    )
                })
    };
    json!({
        "discord_user":{"id":user.id.to_string(),"username":limited(&user.name,128),
            "global_name":user.global_name.as_deref().map(|v|limited(v,128)),
            "avatar":user.avatar.map(|v|v.to_string()),"avatar_url":user.face()},
        "discord_status":state,"activities":activities,
        "active_on_discord_desktop":device(|v|v.desktop),
        "active_on_discord_mobile":device(|v|v.mobile),
        "active_on_discord_web":device(|v|v.web),
        "listening_to_spotify":activities.iter().any(|v|v["name"]=="Spotify" && v["type"]==2),
    })
}
impl Service {
    pub async fn start(
        db: SqlitePool,
        guild: s::GuildId,
        availability: Arc<crate::availability::Service>,
    ) -> Result<Arc<Self>, Error> {
        let enabled = std::env::var("PRESENCE_API_ENABLED").as_deref() == Ok("true");
        let users: Vec<String> =
            sqlx::query_scalar("SELECT user FROM presence_consent WHERE guild=?")
                .bind(guild.to_string())
                .fetch_all(&db)
                .await?;
        let service = Arc::new(Self {
            db,
            guild,
            availability,
            consent: RwLock::new(users.iter().filter_map(|v| valid_id(v)).collect()),
            limits: Mutex::new(HashMap::new()),
            sockets: Arc::new(Semaphore::new(64)),
            listener_enabled: enabled,
        });
        if enabled {
            let port: u16 = std::env::var("PRESENCE_API_PORT")
                .unwrap_or_else(|_| "4100".into())
                .parse()?;
            if port == 0 {
                return Err(crate::errors::user("PRESENCE_API_PORT must be nonzero"));
            }
            // Never expose plaintext presence directly on a public interface.
            let listener =
                tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
            let app = Router::new()
                .route(
                    "/health",
                    get(|| async { Json(json!({"service":"elvnx-presence","ok":true})) }),
                )
                .route("/v1/users/{id}", get(read))
                .route("/v1/users/{id}/ws", get(upgrade))
                .with_state(Arc::clone(&service))
                .layer(middleware::from_fn_with_state(Arc::clone(&service), guard));
            tokio::spawn(async move {
                if let Err(error) = axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await
                {
                    tracing::error!(%error,"Presence API stopped");
                }
            });
            tracing::info!(port, "Opt-in presence API listening on loopback only");
        }
        Ok(service)
    }
    async fn snapshot(&self, id: u64) -> Result<Value, StatusCode> {
        let consent = self.consent.read().await;
        if !consent.contains(&id) {
            return Err(StatusCode::NOT_FOUND);
        }
        let (user, presence) = self.availability.api_snapshot(s::UserId::new(id)).await?;
        Ok(project(&user, presence.as_ref()))
    }
    async fn set_consent(&self, id: u64, enabled: bool) -> Result<(), Error> {
        update_consent(&self.db, self.guild, &self.consent, id, enabled).await
    }
    pub async fn observe(&self, event: &s::FullEvent) {
        if let s::FullEvent::GuildMemberRemoval { guild_id, user, .. } = event
            && *guild_id == self.guild
            && let Err(error) = self.set_consent(user.id.get(), false).await
        {
            tracing::error!(%error,"Presence consent removal failed; sharing revoked for this process");
        }
    }
}
async fn update_consent(
    db: &SqlitePool,
    guild: s::GuildId,
    lock: &RwLock<std::collections::HashSet<u64>>,
    id: u64,
    enabled: bool,
) -> Result<(), Error> {
    let mut consent = lock.write().await;
    if enabled {
        sqlx::query("INSERT INTO presence_consent(guild,user,created) VALUES(?,?,?) ON CONFLICT(guild,user) DO NOTHING")
                .bind(guild.to_string()).bind(id.to_string()).bind(chrono::Utc::now().timestamp()).execute(db).await?;
        consent.insert(id);
    } else {
        // Revoke in memory first, even if SQLite is temporarily unavailable.
        consent.remove(&id);
        sqlx::query("DELETE FROM presence_consent WHERE guild=? AND user=?")
            .bind(guild.to_string())
            .bind(id.to_string())
            .execute(db)
            .await?;
    }
    Ok(())
}
async fn guard(
    State(service): State<Arc<Service>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let allowed = {
        let mut limits = service.limits.lock().await;
        limits.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(60));
        if limits.len() >= 1024 && !limits.contains_key(&peer.ip()) {
            false
        } else {
            let item = limits.entry(peer.ip()).or_insert((Instant::now(), 0));
            item.1 += 1;
            item.1 <= 120
        }
    };
    let mut response = if allowed {
        next.run(request).await
    } else {
        fail(StatusCode::TOO_MANY_REQUESTS, "rate_limited")
    };
    let headers = response.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    if !allowed {
        headers.insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
    }
    response
}
async fn read(State(service): State<Arc<Service>>, Path(raw): Path<String>) -> Response {
    let Some(id) = valid_id(&raw) else {
        return fail(StatusCode::NOT_FOUND, "not_found");
    };
    match service.snapshot(id).await {
        Ok(data) => Json(json!({"success":true,"data":data})).into_response(),
        Err(StatusCode::NOT_FOUND) => fail(StatusCode::NOT_FOUND, "not_found"),
        Err(_) => fail(StatusCode::SERVICE_UNAVAILABLE, "presence_unavailable"),
    }
}
async fn upgrade(
    State(service): State<Arc<Service>>,
    Path(raw): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(id) = valid_id(&raw) else {
        return fail(StatusCode::NOT_FOUND, "not_found");
    };
    if let Err(code) = service.snapshot(id).await {
        return fail(
            code,
            if code == StatusCode::NOT_FOUND {
                "not_found"
            } else {
                "presence_unavailable"
            },
        );
    }
    let Ok(permit) = Arc::clone(&service.sockets).try_acquire_owned() else {
        return fail(StatusCode::TOO_MANY_REQUESTS, "stream_capacity");
    };
    ws.max_message_size(1024)
        .max_frame_size(1024)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            stream(socket, service, id).await;
        })
}
async fn stream(mut socket: WebSocket, service: Arc<Service>, id: u64) {
    let mut timer = tokio::time::interval(Duration::from_secs(2));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last = String::new();
    let mut heartbeat = Instant::now();
    let started = Instant::now();
    loop {
        tokio::select! {
            _=timer.tick()=>{
                if started.elapsed()>Duration::from_secs(3600) {break;}
                // Hold consent lock through bounded send, so Disable cannot finish
                // while an older activity publication is still in flight.
                let consent=service.consent.read().await;
                if !consent.contains(&id) {
                    let _=tokio::time::timeout(Duration::from_secs(3),socket.send(Message::Text(json!({"event":"REVOKED"}).to_string().into()))).await;
                    break;
                }
                let data=service.availability.api_snapshot(s::UserId::new(id)).await;
                let (event,payload)=match data {
                    Ok((user,presence))=>("PRESENCE_UPDATE",json!({"success":true,"data":project(&user,presence.as_ref())})),
                    Err(StatusCode::NOT_FOUND)=>break,
                    Err(_)=>("UNAVAILABLE",json!({"success":false,"error":{"code":"presence_unavailable"}})),
                };
                let value=payload.to_string();
                let message=if value!=last {Some(json!({"event":event,"payload":payload}).to_string())}
                    else if heartbeat.elapsed()>Duration::from_secs(20) {Some(json!({"event":"HEARTBEAT"}).to_string())}else{None};
                if let Some(message)=message {
                    if !matches!(tokio::time::timeout(Duration::from_secs(3),socket.send(Message::Text(message.into()))).await,Ok(Ok(()))) {break;}
                    heartbeat=Instant::now(); last=value;
                }
            }
            message=socket.recv()=>{
                match message {Some(Ok(Message::Ping(_)|Message::Pong(_)))=>{},_=>break}
            }
        }
    }
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum Action {
    Url,
    Enable,
    Disable,
    Info,
}
/// Get an opted-in member’s presence URL, or manage your own public sharing.
#[poise::command(slash_command, guild_only)]
pub async fn presence(
    ctx: Context<'_>,
    #[description = "Member whose public URL to show; defaults to yourself"] member: Option<
        s::User,
    >,
    #[description = "Defaults to Url; sharing settings apply only to yourself"] action: Option<
        Action,
    >,
    #[description = "Required for Enable: publicly share your profile, devices, status and activity details"]
    confirm: Option<bool>,
) -> Result<(), Error> {
    let service = &ctx.data().presence;
    let id = ctx.author().id.get();
    let action = action.unwrap_or(Action::Url);
    if member.is_some() && !matches!(action, Action::Url) {
        return reply(ctx, "The member selector is only for viewing public URLs. Omit member when enabling, disabling or inspecting your own sharing settings.").await;
    }
    match action {
        Action::Url => {
            let target = member.as_ref().map_or(ctx.author(), |user| user);
            if !service.listener_enabled {
                return reply(
                    ctx,
                    "The presence API is currently unavailable. Try again later.",
                )
                .await;
            }
            // Use the same consent and membership checks as the public API.
            match service.snapshot(target.id.get()).await {
                Ok(_) => reply(ctx, format!("**Public presence URL**\nhttps://presence.11x.dpdns.org/v1/users/{}\n**Live updates**\n`wss://presence.11x.dpdns.org/v1/users/{}/ws`", target.id, target.id)).await,
                Err(StatusCode::NOT_FOUND) if target.id == ctx.author().id => reply(ctx, "Your presence is private. Use `/presence action:Enable` to review what public sharing exposes before opting in.").await,
                Err(StatusCode::NOT_FOUND) => reply(ctx, "No public presence is available for that member. They must opt in themselves; you cannot enable sharing for someone else.").await,
                Err(_) => reply(ctx, "Presence is still synchronizing. Try again shortly.").await,
            }
        }
        Action::Enable => {
            if confirm != Some(true) {
                return reply(ctx,"**Public sharing requires your consent.** Anyone who knows your Discord ID may read your profile/avatar, online status, active devices and activity details (including games, Spotify and custom status). Activity text can include filenames or other information you put in Rich Presence. No voice channels, email, messages, session secrets or presence history are published. Invisible appears offline. Websites can copy published data; revocation cannot erase their copies. Run `/presence action:Enable confirm:true` to opt in for yourself.").await;
            }
            if !service.listener_enabled {
                return reply(ctx,"The presence API is not enabled on this bot. No sharing consent was saved. Ask the owner to configure its private listener and HTTPS proxy first.").await;
            }
            service.availability.api_snapshot(ctx.author().id).await.map_err(|_|crate::errors::user("Presence is still synchronizing or your membership is unavailable. Try again shortly."))?;
            service.set_consent(id, true).await?;
            reply(ctx,format!("Public presence sharing enabled for **your account only**. API: `https://presence.11x.dpdns.org/v1/users/{id}` · live: `wss://presence.11x.dpdns.org/v1/users/{id}/ws`. Use `/presence action:Disable` to revoke sharing. Activity stays in memory; only your consent is saved.")).await
        }
        Action::Disable => {
            service.set_consent(id, false).await?;
            reply(ctx,"Presence API sharing revoked. New reads return not found and live streams close on their next check (within about two seconds). Existing third-party copies cannot be recalled.").await
        }
        Action::Info => {
            let enabled = service.consent.read().await.contains(&id);
            reply(ctx,format!("Your public sharing: **{}**. API listener: **{}**. Only self-service opt-in is supported. No member list or presence history is exposed. `/presence action:Disable` revokes sharing.",if enabled{"Enabled"}else{"Disabled"},if service.listener_enabled{"Enabled"}else{"Disabled"})).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_are_bounded_snowflakes() {
        for raw in ["", "0", "-1", "1/2", "18446744073709551616", "1e2"] {
            assert_eq!(valid_id(raw), None);
        }
        assert_eq!(valid_id("1269352892146384957"), Some(1269352892146384957));
    }
    #[test]
    fn activity_projection_never_exports_secrets() {
        let raw = json!({"name":"Spotify","type":2,"details":"Song","state":"Artist","created_at":1,
            "secrets":{"join":"SECRET","spectate":"SECRET","match":"SECRET"},"party":{"id":"PRIVATE"},
            "session_id":"PRIVATE","sync_id":"TRACK","url":"https://private.example/secret","buttons":["Join"]});
        let a: s::Activity = serde_json::from_value(raw).unwrap();
        let public = activity(&a).to_string();
        assert!(public.contains("Song"));
        for secret in [
            "SECRET",
            "PRIVATE",
            "session_id",
            "secrets",
            "party",
            "buttons",
            "private.example",
        ] {
            assert!(!public.contains(secret));
        }
    }
    #[test]
    fn offline_strips_activities_and_device_indicators() {
        let user: s::User = serde_json::from_value(
            json!({"id":"1","username":"test","discriminator":"0","avatar":null}),
        )
        .unwrap();
        let p:s::Presence=serde_json::from_value(json!({"user":{"id":"1"},"status":"invisible","activities":[{"type":0,"name":"Hidden","created_at":1}],"client_status":{"desktop":"online"}})).unwrap();
        let public = project(&user, Some(&p));
        assert_eq!(public["discord_status"], "offline");
        assert_eq!(public["activities"], json!([]));
        assert_eq!(public["active_on_discord_desktop"], false);
    }
    #[tokio::test]
    async fn consent_changes_are_scoped_and_revocation_fails_closed() {
        let db = crate::store::connect("sqlite::memory:").await.unwrap();
        let lock = RwLock::new(std::collections::HashSet::new());
        update_consent(&db, s::GuildId::new(1), &lock, 2, true)
            .await
            .unwrap();
        update_consent(&db, s::GuildId::new(1), &lock, 2, true)
            .await
            .unwrap();
        assert!(lock.read().await.contains(&2));
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM presence_consent")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count, 1);
        update_consent(&db, s::GuildId::new(3), &lock, 2, true)
            .await
            .unwrap();
        update_consent(&db, s::GuildId::new(1), &lock, 2, false)
            .await
            .unwrap();
        assert!(!lock.read().await.contains(&2));
        let rows: Vec<String> = sqlx::query_scalar("SELECT guild FROM presence_consent")
            .fetch_all(&db)
            .await
            .unwrap();
        assert_eq!(rows, vec!["3"]);
        lock.write().await.insert(2);
        db.close().await;
        assert!(
            update_consent(&db, s::GuildId::new(1), &lock, 2, false)
                .await
                .is_err()
        );
        assert!(!lock.read().await.contains(&2));
        assert!(
            update_consent(&db, s::GuildId::new(1), &lock, 2, true)
                .await
                .is_err()
        );
        assert!(!lock.read().await.contains(&2));
    }
    #[tokio::test]
    async fn consent_schema_stores_only_user_choice_not_activity() {
        let db = crate::store::connect("sqlite::memory:").await.unwrap();
        sqlx::query("INSERT INTO presence_consent VALUES('1','2',0)")
            .execute(&db)
            .await
            .unwrap();
        let consent: Vec<String> =
            sqlx::query_scalar("SELECT user FROM presence_consent WHERE guild='1'")
                .fetch_all(&db)
                .await
                .unwrap();
        assert_eq!(consent, vec!["2"]);
        // Schema deliberately has no activity payload column.
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('presence_consent')")
                .fetch_all(&db)
                .await
                .unwrap();
        assert_eq!(columns, vec!["guild", "user", "created"]);
    }
}
