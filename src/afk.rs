use crate::{Context, Error, reply, serenity as s, store};
use sqlx::SqlitePool;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const WAIT: Duration = Duration::from_secs(600);
#[derive(Clone)]
struct Entry {
    channel: s::ChannelId,
    since: Instant,
    retry: Instant,
}
#[derive(Default)]
struct Tracker {
    connected: bool,
    destination: Option<s::ChannelId>,
    members: HashMap<s::UserId, Entry>,
}
impl Tracker {
    fn observe(
        &mut self,
        user: s::UserId,
        channel: Option<s::ChannelId>,
        inactive: bool,
        now: Instant,
    ) {
        let Some(channel) = channel.filter(|ch| {
            Some(*ch) != self.destination
                && inactive
                && self.destination.is_some()
                && self.connected
        }) else {
            self.members.remove(&user);
            return;
        };
        if let Some(entry) = self.members.get_mut(&user) {
            if entry.channel != channel {
                *entry = Entry {
                    channel,
                    since: now,
                    retry: now,
                };
            }
        } else if self.members.len() < 10_000 {
            self.members.insert(
                user,
                Entry {
                    channel,
                    since: now,
                    retry: now,
                },
            );
        }
    }
    fn ready(entry: &Entry, now: Instant) -> bool {
        now.duration_since(entry.since) >= WAIT && now >= entry.retry
    }
}
pub struct Service {
    tracker: Mutex<Tracker>,
}
impl Service {
    pub fn start(
        ctx: s::Context,
        db: SqlitePool,
        guild: s::GuildId,
        destination: Option<u64>,
    ) -> Arc<Self> {
        let service = Arc::new(Self {
            tracker: Mutex::new(Tracker {
                connected: true,
                destination: destination.map(s::ChannelId::new),
                members: HashMap::new(),
            }),
        });
        let worker = Arc::clone(&service);
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(Duration::from_secs(15));
            loop {
                timer.tick().await;
                if let Err(error) = worker.tick(&ctx, &db, guild).await {
                    tracing::warn!(%error, "AFK move check failed");
                }
            }
        });
        service
    }
    pub async fn configure(&self, destination: Option<u64>) {
        let mut tracker = self.tracker.lock().await;
        tracker.destination = destination.map(s::ChannelId::new);
        tracker.members.clear();
    }
    pub async fn observe(&self, ctx: &s::Context, voice: &s::VoiceState) {
        let bot = ctx.cache.user(voice.user_id).is_some_and(|user| user.bot)
            || voice.member.as_ref().is_some_and(|member| member.user.bot);
        self.tracker.lock().await.observe(
            voice.user_id,
            voice.channel_id,
            !bot && inactive(voice),
            Instant::now(),
        );
    }
    pub async fn connection(&self, connected: bool) {
        let mut tracker = self.tracker.lock().await;
        tracker.connected = connected;
        tracker.members.clear();
    }
    async fn tick(
        &self,
        ctx: &s::Context,
        db: &SqlitePool,
        guild: s::GuildId,
    ) -> Result<(), Error> {
        {
            let tracker = self.tracker.lock().await;
            if tracker.destination.is_none() || !tracker.connected {
                return Ok(());
            }
        }
        let Some(snapshot) = ctx
            .cache
            .guild(guild)
            .map(|g| g.voice_states.values().cloned().collect::<Vec<_>>())
        else {
            return Ok(());
        };
        let now = Instant::now();
        let (destination, candidates) = {
            let mut tracker = self.tracker.lock().await;
            tracker.members.retain(|user, _| {
                snapshot
                    .iter()
                    .any(|v| v.user_id == *user && v.channel_id.is_some())
            });
            for voice in &snapshot {
                let bot = ctx.cache.user(voice.user_id).is_some_and(|u| u.bot)
                    || voice.member.as_ref().is_some_and(|m| m.user.bot);
                tracker.observe(
                    voice.user_id,
                    voice.channel_id,
                    !bot && inactive(voice),
                    now,
                );
            }
            let candidates = tracker
                .members
                .iter_mut()
                .filter_map(|(user, entry)| {
                    if Tracker::ready(entry, now) {
                        entry.retry = now + Duration::from_secs(60);
                        Some((*user, entry.clone()))
                    } else {
                        None
                    }
                })
                .take(25)
                .collect::<Vec<_>>();
            (tracker.destination, candidates)
        };
        let Some(destination) = destination else {
            return Ok(());
        };
        if candidates.is_empty() {
            return Ok(());
        }
        let dest = destination
            .to_channel(&ctx.http)
            .await?
            .guild()
            .ok_or("DND destination was removed")?;
        if dest.guild_id != guild || dest.kind != s::ChannelType::Voice {
            return Err("DND destination must remain a normal server voice channel".into());
        }
        let partial = guild.to_partial_guild(&ctx.http).await?;
        let bot_id = ctx.cache.current_user().id;
        let bot = guild.member(&ctx.http, bot_id).await?;
        if !partial.user_permissions_in(&dest, &bot).contains(
            s::Permissions::VIEW_CHANNEL | s::Permissions::CONNECT | s::Permissions::MOVE_MEMBERS,
        ) {
            return Err(
                "Bot needs View Channel, Connect, and Move Members in the DND channel".into(),
            );
        }
        for (user, original) in candidates {
            let voice = match guild.get_user_voice_state(&ctx.http, user).await {
                Ok(voice) => voice,
                Err(error) => {
                    tracing::warn!(%user, %error, "AFK live voice state unavailable; check bot Connect permission in the source channel");
                    continue;
                }
            };
            if voice.channel_id != Some(original.channel) || !inactive(&voice) {
                continue;
            }
            let member = match guild.member(&ctx.http, user).await {
                Ok(member) => member,
                Err(_) => continue,
            };
            if member.user.bot {
                continue;
            }
            // Do not use moderation access to expose a private destination to a member.
            if !partial
                .user_permissions_in(&dest, &member)
                .contains(s::Permissions::VIEW_CHANNEL | s::Permissions::CONNECT)
            {
                continue;
            }
            let source = original
                .channel
                .to_channel(&ctx.http)
                .await?
                .guild()
                .ok_or("Source voice channel unavailable")?;
            if !partial
                .user_permissions_in(&source, &bot)
                .contains(s::Permissions::MOVE_MEMBERS)
            {
                continue;
            }
            let still_due = {
                let tracker = self.tracker.lock().await;
                tracker.connected
                    && tracker.destination == Some(destination)
                    && tracker.members.get(&user).is_some_and(|entry| {
                        entry.since == original.since
                            && entry.channel == original.channel
                            && Instant::now().duration_since(entry.since) >= WAIT
                    })
            };
            if !still_due {
                continue;
            }
            match member.move_to_voice_channel(&ctx.http, destination).await {
                Ok(_) => {
                    self.tracker.lock().await.members.remove(&user);
                    let cfg = store::settings(db, guild.get()).await?;
                    if let Some(log) = cfg.log_channel {
                        let _ = s::ChannelId::new(log).send_message(&ctx.http, s::CreateMessage::new().embed(s::CreateEmbed::new().title("ELVNX / AFK MOVE").description(format!("<@{user}> moved from <#{}> to <#{destination}> after 10 continuous minutes muted or deafened.", original.channel)).color(0x111111)).allowed_mentions(s::CreateAllowedMentions::new())).await;
                    }
                }
                Err(error) => {
                    tracing::warn!(%user, %error, "AFK move failed; retrying no sooner than one minute")
                }
            }
        }
        Ok(())
    }
}
fn inactive(voice: &s::VoiceState) -> bool {
    voice.self_mute || voice.self_deaf || voice.mute || voice.deaf
}
/// Move continuously muted/deafened members to a DND voice channel after 10 minutes.
#[poise::command(
    slash_command,
    guild_only,
    rename = "afk-move",
    required_permissions = "MANAGE_GUILD | MOVE_MEMBERS",
    default_member_permissions = "MANAGE_GUILD | MOVE_MEMBERS"
)]
pub async fn afk_move(
    ctx: Context<'_>,
    #[description = "Enable or disable automatic AFK moves"] enabled: bool,
    #[description = "DND voice channel when enabling"] channel: Option<s::GuildChannel>,
) -> Result<(), Error> {
    let id = if enabled {
        let dest = channel.ok_or("Select the DND voice channel")?;
        if dest.guild_id != ctx.data().guild || dest.kind != s::ChannelType::Voice {
            return Err("Choose a normal voice channel in this server".into());
        }
        let partial = ctx.data().guild.to_partial_guild(ctx.http()).await?;
        let actor = ctx.data().guild.member(ctx.http(), ctx.author().id).await?;
        let bot_id = ctx.serenity_context().cache.current_user().id;
        let bot = ctx.data().guild.member(ctx.http(), bot_id).await?;
        if !partial
            .user_permissions_in(&dest, &actor)
            .contains(s::Permissions::VIEW_CHANNEL | s::Permissions::CONNECT)
        {
            return Err("You must be able to view and connect to the destination".into());
        }
        if !partial.user_permissions_in(&dest, &bot).contains(
            s::Permissions::VIEW_CHANNEL | s::Permissions::CONNECT | s::Permissions::MOVE_MEMBERS,
        ) {
            return Err(
                "Grant the bot View Channel, Connect, and Move Members in the DND channel".into(),
            );
        }
        Some(dest.id.get())
    } else {
        None
    };
    let _guard = ctx.data().configuration_lock.lock().await;
    let mut cfg = ctx.data().settings.read().await.clone();
    cfg.afk_move_channel = id;
    store::save(&ctx.data().db, ctx.data().guild.get(), &cfg).await?;
    *ctx.data().settings.write().await = cfg;
    ctx.data().afk.configure(id).await;
    reply(ctx, if enabled { "AFK moves enabled. Members muted or deafened continuously for 10 minutes move to the selected DND channel. Bots are excluded. Unmuting and undeafening, leaving, or changing channels resets the timer. Checks run every 15 seconds; timers restart after a bot restart." } else { "AFK moves disabled; pending timers cleared." }).await
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continuous_mute_threshold_and_reset() {
        let start = Instant::now();
        let user = s::UserId::new(1);
        let source = s::ChannelId::new(2);
        let dest = s::ChannelId::new(3);
        let mut tracker = Tracker {
            connected: true,
            destination: Some(dest),
            members: HashMap::new(),
        };
        tracker.observe(user, Some(source), true, start);
        assert!(!Tracker::ready(
            &tracker.members[&user],
            start + Duration::from_secs(599)
        ));
        tracker.observe(user, Some(source), true, start + Duration::from_secs(400));
        assert!(Tracker::ready(&tracker.members[&user], start + WAIT));
        tracker.observe(user, Some(source), false, start + WAIT);
        assert!(tracker.members.is_empty());
        tracker.observe(user, Some(source), true, start + WAIT);
        tracker.observe(user, Some(s::ChannelId::new(4)), true, start + WAIT + WAIT);
        assert!(!Tracker::ready(
            &tracker.members[&user],
            start + WAIT + WAIT
        ));
        tracker.observe(user, Some(dest), true, start + WAIT + WAIT);
        assert!(tracker.members.is_empty());
    }
    #[test]
    fn leave_disable_and_retry_do_not_move_members() {
        let now = Instant::now();
        let user = s::UserId::new(1);
        let mut tracker = Tracker {
            connected: true,
            destination: Some(s::ChannelId::new(3)),
            members: HashMap::new(),
        };
        tracker.observe(user, Some(s::ChannelId::new(2)), true, now);
        tracker.members.get_mut(&user).unwrap().retry = now + WAIT + Duration::from_secs(60);
        assert!(!Tracker::ready(&tracker.members[&user], now + WAIT));
        tracker.observe(user, None, true, now + WAIT);
        assert!(tracker.members.is_empty());
        tracker.destination = None;
        tracker.observe(user, Some(s::ChannelId::new(2)), true, now);
        assert!(tracker.members.is_empty());
    }
    #[tokio::test]
    async fn connection_gap_discards_unknown_history() {
        let now = Instant::now();
        let user = s::UserId::new(1);
        let service = Service {
            tracker: Mutex::new(Tracker {
                connected: true,
                destination: Some(s::ChannelId::new(3)),
                members: HashMap::new(),
            }),
        };
        service
            .tracker
            .lock()
            .await
            .observe(user, Some(s::ChannelId::new(2)), true, now);
        service.connection(false).await;
        {
            let mut tracker = service.tracker.lock().await;
            assert_eq!(tracker.destination, Some(s::ChannelId::new(3)));
            tracker.observe(user, Some(s::ChannelId::new(2)), true, now + WAIT);
            assert!(tracker.members.is_empty());
        }
        service.connection(true).await;
        let mut tracker = service.tracker.lock().await;
        tracker.observe(user, Some(s::ChannelId::new(2)), true, now + WAIT);
        assert!(!Tracker::ready(&tracker.members[&user], now + WAIT));
    }
}
