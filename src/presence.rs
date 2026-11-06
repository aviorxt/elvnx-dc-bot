//! Live presence for current server members. Activity payloads never enter SQLite.
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
use tokio::sync::{Mutex, Semaphore};

pub struct Service {
    availability: Arc<crate::availability::Service>,
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
        _db: SqlitePool,
        _guild: s::GuildId,
        availability: Arc<crate::availability::Service>,
    ) -> Result<Arc<Self>, Error> {
        let enabled = std::env::var("PRESENCE_API_ENABLED").as_deref() == Ok("true");
        let service = Arc::new(Self {
            availability,
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
            tracing::info!(port, "Server member presence API listening on loopback only");
        }
        Ok(service)
    }
    async fn snapshot(&self, id: u64) -> Result<Value, StatusCode> {
        let (user, presence) = self.availability.api_snapshot(s::UserId::new(id)).await?;
        Ok(project(&user, presence.as_ref()))
    }
    pub async fn observe(&self, _event: &s::FullEvent) {}
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
    Info,
}
/// Get a server member's live presence URL. Sharing is automatic.
#[poise::command(slash_command, guild_only)]
pub async fn presence(
    ctx: Context<'_>,
    #[description = "Member whose public URL to show; defaults to yourself"] member: Option<s::User>,
    #[description = "Defaults to Url"] action: Option<Action>,
) -> Result<(), Error> {
    let service = &ctx.data().presence;
    if matches!(action, Some(Action::Info)) {
        return reply(ctx, "Presence is automatic for current server members; no enable command is needed. The API shares Discord-visible profile, status, devices and activities. Invisible appears offline. Discord privacy settings still apply. Activity stays in memory; no presence history is stored.").await;
    }
    if !service.listener_enabled {
        return reply(ctx, "The presence API is currently unavailable. Try again later.").await;
    }
    let target = member.as_ref().unwrap_or_else(|| ctx.author());
    match service.snapshot(target.id.get()).await {
        Ok(_) => reply(ctx, format!("**Public presence URL**\nhttps://presence.11-x.online/v1/users/{}\n**Live updates**\n`wss://presence.11-x.online/v1/users/{}/ws`", target.id, target.id)).await,
        Err(StatusCode::NOT_FOUND) => reply(ctx, "Presence is available only for current non-bot server members.").await,
        Err(_) => reply(ctx, "Presence is still synchronizing. Try again shortly.").await,
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
}
