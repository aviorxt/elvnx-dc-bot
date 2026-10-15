use crate::{Context, Data, Error, serenity as s, store};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const MAX_MESSAGES: usize = 1000;
const RETENTION: Duration = Duration::from_secs(24 * 3600);

#[derive(Clone, Default)]
struct Snapshot {
    author: Option<s::UserId>,
    content: Option<String>,
    attachments: Vec<String>,
    ignored: bool,
}
impl Snapshot {
    fn message(message: &s::Message) -> Self {
        if message.author.bot || message.webhook_id.is_some() {
            return Self {
                ignored: true,
                ..Default::default()
            };
        }
        Self {
            author: Some(message.author.id),
            content: Some(clip(&message.content, 8192)),
            attachments: attachments(&message.attachments),
            ignored: false,
        }
    }
}
#[derive(Default)]
pub struct Cache {
    messages: HashMap<s::MessageId, (Instant, Snapshot)>,
}
impl Cache {
    fn prune(&mut self, now: Instant) {
        self.messages
            .retain(|_, (seen, _)| now.duration_since(*seen) < RETENTION);
    }
    fn put(&mut self, id: s::MessageId, message: Snapshot, now: Instant) {
        self.prune(now);
        if self.messages.len() >= MAX_MESSAGES
            && !self.messages.contains_key(&id)
            && let Some(oldest) = self
                .messages
                .iter()
                .min_by_key(|(_, v)| v.0)
                .map(|(id, _)| *id)
        {
            self.messages.remove(&oldest);
        }
        self.messages.insert(id, (now, message));
    }
}
fn attachments(files: &[s::Attachment]) -> Vec<String> {
    files
        .iter()
        .take(10)
        .map(|a| format!("{} — {}", clip(&a.filename, 100), clip(&a.url, 1000)))
        .collect()
}
fn clip(text: &str, max: usize) -> String {
    if text.encode_utf16().count() <= max {
        return text.to_string();
    }
    let mut out = String::new();
    let mut units = 0;
    for ch in text.chars() {
        if units + ch.len_utf16() > max.saturating_sub(1) {
            break;
        }
        units += ch.len_utf16();
        out.push(ch);
    }
    out.push('…');
    out
}
fn body(text: Option<&str>) -> String {
    match text {
        None => "Unavailable — message was not cached before this event.".into(),
        Some("") => "No text (message may contain attachments).".into(),
        Some(text) => format!("```\n{}\n```", clip(&text.replace("```", "` ` `"), 950)),
    }
}
fn author(snapshot: Option<&Snapshot>) -> String {
    snapshot
        .and_then(|m| m.author)
        .map(|id| format!("<@{id}> (`{id}`)"))
        .unwrap_or_else(|| "Unknown — not cached".into())
}
fn base(title: &str, description: String) -> s::CreateEmbed {
    let icon = if title.contains("VOICE") {
        "microphone"
    } else if title.contains("MESSAGE") {
        "typing"
    } else if title.contains("MEMBER") {
        "user"
    } else if title.contains("AUDIT") || title.contains("BAN") {
        "shield"
    } else {
        "review"
    };
    crate::emoji::embed_for(icon)
        .title(format!("ELVNX / {title}"))
        .description(clip(&description, 1800))
        .color(0x111111)
        .timestamp(s::Timestamp::now())
        .footer(s::CreateEmbedFooter::new("avrxt.dev • staff activity log"))
}
async fn send(
    ctx: &s::Context,
    channel: u64,
    embed: s::CreateEmbed,
    transcript: Option<String>,
) -> bool {
    let mut message = s::CreateMessage::new()
        .embed(embed)
        .allowed_mentions(s::CreateAllowedMentions::new());
    if let Some(text) = transcript {
        message = message.add_file(s::CreateAttachment::bytes(
            text.into_bytes(),
            "message-history.txt",
        ));
    }
    if let Err(error) = s::ChannelId::new(channel)
        .send_message(&ctx.http, message)
        .await
    {
        tracing::warn!(%error,"Activity log delivery failed");
        return false;
    }
    true
}
fn add_files(
    mut embed: s::CreateEmbed,
    label: &str,
    snapshot: Option<&Snapshot>,
) -> s::CreateEmbed {
    if let Some(snapshot) = snapshot
        && !snapshot.attachments.is_empty()
    {
        embed = embed.field(label, clip(&snapshot.attachments.join("\n"), 700), false);
    }
    embed
}
fn voice_action(
    old: Option<s::ChannelId>,
    new: Option<s::ChannelId>,
) -> Option<(&'static str, String)> {
    match (old, new) {
        (None, Some(to)) => Some(("VOICE JOIN", format!("Joined <#{to}>"))),
        (Some(from), None) => Some(("VOICE LEAVE", format!("Left <#{from}>"))),
        (Some(from), Some(to)) if from != to => {
            Some(("VOICE MOVE", format!("From <#{from}> → <#{to}>")))
        }
        _ => None,
    }
}

pub async fn handle(ctx: &s::Context, event: &s::FullEvent, data: &Data) {
    let Some(destination) = data.settings.read().await.log_channel else {
        data.event_history.lock().await.messages.clear();
        return;
    };
    let private = ctx
        .cache
        .guild(data.guild)
        .and_then(|g| g.channels.get(&s::ChannelId::new(destination)).cloned())
        .is_some_and(|c| private_channel(&c));
    if !private {
        data.event_history.lock().await.messages.clear();
        return;
    }
    let now = Instant::now();
    if let s::FullEvent::GuildAuditLogEntryCreate { guild_id, entry } = event
        && *guild_id == data.guild
    {
        let receipt =
            sqlx::query("INSERT OR IGNORE INTO audit_receipts(guild,id,created) VALUES(?,?,?)")
                .bind(data.guild.to_string())
                .bind(entry.id.to_string())
                .bind(s::Timestamp::now().unix_timestamp())
                .execute(&data.db)
                .await;
        if !matches!(receipt,Ok(ref r) if r.rows_affected()==1) {
            return;
        }
        let _ = sqlx::query("DELETE FROM audit_receipts WHERE created<?")
            .bind(s::Timestamp::now().unix_timestamp() - 7 * 86400)
            .execute(&data.db)
            .await;
    }
    if let Some(embed) = server_event(event, data.guild) {
        let delivered = send(ctx, destination, embed, None).await;
        if !delivered && let s::FullEvent::GuildAuditLogEntryCreate { entry, .. } = event {
            let _ = sqlx::query("DELETE FROM audit_receipts WHERE guild=? AND id=?")
                .bind(data.guild.to_string())
                .bind(entry.id.to_string())
                .execute(&data.db)
                .await;
        }
    }
    match event {
        s::FullEvent::Message { new_message: m }
            if m.guild_id == Some(data.guild) && m.channel_id.get() != destination =>
        {
            data.event_history
                .lock()
                .await
                .put(m.id, Snapshot::message(m), now);
        }
        s::FullEvent::MessageUpdate {
            old_if_available,
            new,
            event: e,
        } if e.guild_id == Some(data.guild) && e.channel_id.get() != destination => {
            if e.content.is_none() && e.attachments.is_none() {
                return;
            }
            if e.author.as_ref().is_some_and(|u| u.bot)
                || new.as_ref().is_some_and(|m| m.author.bot)
                || e.webhook_id.flatten().is_some()
            {
                return;
            }
            let (before, after) = {
                let mut cache = data.event_history.lock().await;
                cache.prune(now);
                let before = cache
                    .messages
                    .get(&e.id)
                    .map(|(_, s)| s.clone())
                    .or_else(|| old_if_available.as_ref().map(Snapshot::message));
                if before.as_ref().is_some_and(|s| s.ignored) {
                    return;
                }
                let mut after = before
                    .clone()
                    .or_else(|| new.as_ref().map(Snapshot::message))
                    .unwrap_or_default();
                if let Some(content) = &e.content {
                    after.content = Some(clip(content, 8192));
                }
                if let Some(files) = &e.attachments {
                    after.attachments = attachments(files);
                }
                if let Some(user) = &e.author {
                    after.author = Some(user.id);
                }
                cache.put(e.id, after.clone(), now);
                (before, after)
            };
            if before.as_ref().is_some_and(|old| {
                old.content == after.content && old.attachments == after.attachments
            }) {
                return;
            }
            let description = format!(
                "**Author:** {}\n**Channel:** <#{}>\n**Message:** [{}](https://discord.com/channels/{}/{}/{})",
                author(Some(&after)),
                e.channel_id,
                e.id,
                data.guild,
                e.channel_id,
                e.id
            );
            let mut embed = base("MESSAGE EDIT", description)
                .field(
                    "Before",
                    body(before.as_ref().and_then(|s| s.content.as_deref())),
                    false,
                )
                .field("After", body(after.content.as_deref()), false);
            embed = add_files(
                add_files(embed, "Attachments before", before.as_ref()),
                "Attachments after",
                Some(&after),
            );
            let transcript = if before
                .as_ref()
                .and_then(|s| s.content.as_ref())
                .is_some_and(|s| s.encode_utf16().count() > 950)
                || after
                    .content
                    .as_ref()
                    .is_some_and(|s| s.encode_utf16().count() > 950)
            {
                Some(format!(
                    "Message {} in channel {}\nBEFORE\n{}\n\nAFTER\n{}",
                    e.id,
                    e.channel_id,
                    before
                        .as_ref()
                        .and_then(|s| s.content.as_deref())
                        .unwrap_or("Not cached"),
                    after.content.as_deref().unwrap_or("Unavailable")
                ))
            } else {
                None
            };
            send(ctx, destination, embed, transcript).await;
        }
        s::FullEvent::MessageDelete {
            channel_id,
            deleted_message_id,
            guild_id,
        } if *guild_id == Some(data.guild) && channel_id.get() != destination => {
            let snapshot = {
                let mut cache = data.event_history.lock().await;
                cache.prune(now);
                cache.messages.remove(deleted_message_id).map(|(_, s)| s)
            };
            if snapshot.as_ref().is_some_and(|s| s.ignored) {
                return;
            }
            let embed=add_files(base("MESSAGE DELETE",format!("**Author:** {}\n**Channel:** <#{channel_id}>\n**Message ID:** `{deleted_message_id}`\nThe deletion event does not identify who deleted it.",author(snapshot.as_ref()))).field("Deleted message",body(snapshot.as_ref().and_then(|s|s.content.as_deref())),false),"Attachments",snapshot.as_ref());
            let transcript = snapshot
                .as_ref()
                .and_then(|s| s.content.as_ref())
                .filter(|text| text.encode_utf16().count() > 950)
                .map(|text| {
                    format!("Deleted message {deleted_message_id} in {channel_id}\n{text}")
                });
            send(ctx, destination, embed, transcript).await;
        }
        s::FullEvent::MessageDeleteBulk {
            channel_id,
            multiple_deleted_messages_ids: ids,
            guild_id,
        } if *guild_id == Some(data.guild) && channel_id.get() != destination => {
            let mut cached = 0;
            let mut transcript = String::new();
            {
                let mut cache = data.event_history.lock().await;
                cache.prune(now);
                for id in ids {
                    if let Some((_, s)) = cache.messages.remove(id) {
                        if s.ignored {
                            continue;
                        }
                        cached += 1;
                        transcript.push_str(&format!(
                            "MESSAGE {id}\nAUTHOR {}\n{}\nATTACHMENTS {}\n\n",
                            author(Some(&s)),
                            s.content.as_deref().unwrap_or("No cached text"),
                            s.attachments.join("\n")
                        ));
                    }
                }
            }
            let embed = base(
                "BULK MESSAGE DELETE",
                format!(
                    "**Channel:** <#{channel_id}>\n**Deleted:** {}\n**Cached:** {cached}\nDiscord does not identify the deleting actor in this event. Uncached contents are unavailable.",
                    ids.len()
                ),
            );
            send(
                ctx,
                destination,
                embed,
                if cached > 0 {
                    Some(clip(&transcript, 120000))
                } else {
                    None
                },
            )
            .await;
        }
        s::FullEvent::VoiceStateUpdate { old, new } if new.guild_id == Some(data.guild) => {
            if let Some((title, text)) =
                voice_action(old.as_ref().and_then(|s| s.channel_id), new.channel_id)
            {
                send(
                    ctx,
                    destination,
                    base(
                        title,
                        format!("**Member:** <@{}> (`{}`)\n{text}", new.user_id, new.user_id),
                    ),
                    None,
                )
                .await;
            } else if let Some(old) = old {
                let mut changes = Vec::new();
                for (label, before, after) in [
                    ("Self mute", old.self_mute, new.self_mute),
                    ("Self deaf", old.self_deaf, new.self_deaf),
                    ("Server mute", old.mute, new.mute),
                    ("Server deaf", old.deaf, new.deaf),
                    ("Camera", old.self_video, new.self_video),
                    (
                        "Stream",
                        old.self_stream.unwrap_or(false),
                        new.self_stream.unwrap_or(false),
                    ),
                ] {
                    if before != after {
                        changes.push(format!("{label}: {before} → {after}"));
                    }
                }
                if !changes.is_empty() {
                    send(
                        ctx,
                        destination,
                        base(
                            "VOICE STATUS",
                            format!("**Member:** <@{}>\n{}", new.user_id, changes.join("\n")),
                        ),
                        None,
                    )
                    .await;
                }
            }
        }
        s::FullEvent::GuildMemberUpdate {
            old_if_available: Some(old),
            new: Some(new),
            event,
        } if event.guild_id == data.guild => {
            let mut changes = Vec::new();
            if old.pending != new.pending {
                changes.push(format!(
                    "Membership screening pending: {} → {}",
                    old.pending, new.pending
                ));
            }
            if old.avatar != new.avatar {
                changes.push("Server avatar changed".into());
            }
            if old.nick != new.nick {
                changes.push(format!(
                    "Nickname: {} → {}",
                    old.nick.as_deref().unwrap_or("None"),
                    new.nick.as_deref().unwrap_or("None")
                ));
            }
            let added = new
                .roles
                .iter()
                .filter(|r| !old.roles.contains(r))
                .map(|r| format!("<@&{r}>"))
                .collect::<Vec<_>>();
            let removed = old
                .roles
                .iter()
                .filter(|r| !new.roles.contains(r))
                .map(|r| format!("<@&{r}>"))
                .collect::<Vec<_>>();
            if !added.is_empty() {
                changes.push(format!("Roles added: {}", added.join(", ")));
            }
            if !removed.is_empty() {
                changes.push(format!("Roles removed: {}", removed.join(", ")));
            }
            if old.communication_disabled_until != new.communication_disabled_until {
                changes.push(format!(
                    "Timeout: {:?} → {:?}",
                    old.communication_disabled_until, new.communication_disabled_until
                ));
            }
            if !changes.is_empty() {
                send(
                    ctx,
                    destination,
                    base(
                        "MEMBER UPDATE",
                        format!("**Member:** <@{}>\n{}", new.user.id, changes.join("\n")),
                    ),
                    None,
                )
                .await;
            }
        }
        s::FullEvent::GuildMemberUpdate {
            old_if_available: None,
            event,
            ..
        } if event.guild_id == data.guild => {
            send(ctx,destination,base("MEMBER UPDATE",format!("**Member:** <@{}> (`{}`)\nPrevious state unavailable — member was not cached. No before/after change is inferred.",event.user.id,event.user.id)).field("Current roles",clip(&event.roles.iter().map(|r|format!("<@&{r}>")).collect::<Vec<_>>().join(" "),900).chars().collect::<String>() + " ",false),None).await;
        }
        _ => {}
    }
}

/// Require an explicit @everyone View Channel deny. Dedicated log-reader roles remain supported.
pub fn private_channel(channel: &s::GuildChannel) -> bool {
    channel.kind == s::ChannelType::Text
        && channel.permission_overwrites.iter().any(|p| {
            p.kind == s::PermissionOverwriteType::Role(s::RoleId::new(channel.guild_id.get()))
                && p.deny.contains(s::Permissions::VIEW_CHANNEL)
                && !p.allow.contains(s::Permissions::VIEW_CHANNEL)
        })
}
pub fn cover() -> Result<Vec<u8>, Error> {
    crate::cards::render(
        "ACTIVITY / AUDIT DESK",
        "The server journal",
        "Every action has a story. Keep the record clear.",
        &[
            ("MESSAGES", "Before / after".into()),
            ("VOICE", "Sessions / state".into()),
            ("SERVER", "Roles / access".into()),
        ],
        None,
    )
}

pub async fn refresh(ctx: &s::Context, data: &Data) -> Result<(), Error> {
    use sqlx::Row;
    let Some(channel) = data.settings.read().await.log_channel else {
        return Ok(());
    };
    let id = s::ChannelId::new(channel);
    let fresh = id
        .to_channel(&ctx.http)
        .await?
        .guild()
        .ok_or_else(|| crate::errors::user("Log destination is not a server channel."))?;
    if fresh.guild_id != data.guild || !private_channel(&fresh) {
        return Err(crate::errors::user("Log destination must remain private."));
    }
    let row = sqlx::query("SELECT channel,message,version FROM activity_panels WHERE guild=?")
        .bind(data.guild.to_string())
        .fetch_optional(&data.db)
        .await?;
    if row.as_ref().is_some_and(|r| {
        r.get::<String, _>("channel") == id.to_string()
            && r.get::<String, _>("version") == env!("CARGO_PKG_VERSION")
    }) {
        return Ok(());
    }
    let guild = data.guild.to_partial_guild(&ctx.http).await?;
    let bot_id = ctx.cache.current_user().id;
    let bot = data.guild.member(&ctx.http, bot_id).await?;
    let audit = guild
        .member_permissions(&bot)
        .contains(s::Permissions::VIEW_AUDIT_LOG);
    let banner=base("ACTIVITY DESK", "**A clear record. A private space.**\n\nMessage edits and deletions · voice joins, moves and state · members, roles and timeouts · channel and thread changes · bans and audit events.\n\nMessage text is kept only in a bounded memory cache for up to 24 hours. Copies posted here remain until staff delete them. Uncached content and unavailable actors are labelled honestly.".into()).field("Audit attribution",if audit{"Enabled · Discord-provided actor, action and target"}else{"Unavailable · grant View Audit Log to enable actor records"},false).image("attachment://activity-desk.png");
    let file = s::CreateAttachment::bytes(cover()?, "activity-desk.png");
    let existing = row
        .as_ref()
        .filter(|r| r.get::<String, _>("channel") == id.to_string())
        .and_then(|r| r.get::<String, _>("message").parse::<u64>().ok());
    let message = if let Some(message) = existing {
        let message = id.message(&ctx.http, s::MessageId::new(message)).await?;
        if message.author.id != ctx.cache.current_user().id {
            return Err(crate::errors::user("Log header is not owned by this bot."));
        }
        id.edit_message(
            &ctx.http,
            message.id,
            s::EditMessage::new()
                .embed(banner)
                .attachments(s::EditAttachments::new().add(file))
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await?
    } else {
        id.send_message(
            &ctx.http,
            s::CreateMessage::new()
                .embed(banner)
                .add_file(file)
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await?
    };
    sqlx::query("INSERT INTO activity_panels(guild,channel,message,version) VALUES(?,?,?,?) ON CONFLICT(guild) DO UPDATE SET channel=excluded.channel,message=excluded.message,version=excluded.version").bind(data.guild.to_string()).bind(id.to_string()).bind(message.id.to_string()).bind(env!("CARGO_PKG_VERSION")).execute(&data.db).await?;
    Ok(())
}
pub async fn configure(
    ctx: Context<'_>,
    enabled: bool,
    channel: Option<s::GuildChannel>,
) -> Result<(), Error> {
    let _guard = ctx.data().configuration_lock.lock().await;
    let id = if enabled {
        let selected =
            channel.ok_or_else(|| crate::errors::user("Select a private log channel."))?;
        let fresh = selected
            .id
            .to_channel(ctx.http())
            .await?
            .guild()
            .ok_or_else(|| crate::errors::user("Choose a server text channel."))?;
        crate::commands::posting_channel(ctx, &fresh).await?;
        if !private_channel(&fresh) {
            return Err(crate::errors::user(
                "Keep logs private: deny View Channel to @everyone, then allow your trusted staff/log-reader roles.",
            ));
        }
        let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
        let bot = ctx
            .data()
            .guild
            .member(ctx.http(), crate::bot_id(ctx))
            .await?;
        let needed = s::Permissions::EMBED_LINKS
            | s::Permissions::ATTACH_FILES
            | s::Permissions::READ_MESSAGE_HISTORY;
        if !guild.user_permissions_in(&fresh, &bot).contains(needed) {
            return Err(crate::errors::user(
                "The bot needs Embed Links, Attach Files and Read Message History in the log channel.",
            ));
        }
        Some(fresh.id.get())
    } else {
        None
    };
    let mut cfg = ctx.data().settings.read().await.clone();
    cfg.log_channel = id;
    store::save(&ctx.data().db, ctx.data().guild.get(), &cfg).await?;
    *ctx.data().settings.write().await = cfg;
    if enabled {
        refresh(ctx.serenity_context(), ctx.data()).await?;
    }
    if !enabled {
        ctx.data().event_history.lock().await.messages.clear();
    }
    crate::reply(ctx,if enabled{"Activity desk enabled. The coverage and privacy notice are posted in the selected channel. Audit records require View Audit Log; unavailable details are explicitly marked."}else{"Activity logging disabled and the message cache cleared. Existing Discord log messages remain until staff remove them."}).await
}
fn channel_summary(c: &s::GuildChannel) -> String {
    clip(
        &format!(
            "Name: {}\nType: {:?}\nParent: {}\nTopic: {}\nNSFW: {} · Slowmode: {}s\nBitrate: {} · Capacity: {}\nPermission overrides: {}\nThread: {:?}",
            c.name,
            c.kind,
            c.parent_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "None".into()),
            c.topic.as_deref().unwrap_or("None"),
            c.nsfw,
            c.rate_limit_per_user.unwrap_or(0),
            c.bitrate.unwrap_or(0),
            c.user_limit.unwrap_or(0),
            c.permission_overwrites.len(),
            c.thread_metadata
        ),
        850,
    )
}
fn permission_changes(old: &s::GuildChannel, new: &s::GuildChannel) -> String {
    let mut lines = Vec::new();
    for after in &new.permission_overwrites {
        let before = old
            .permission_overwrites
            .iter()
            .find(|p| p.kind == after.kind);
        if before != Some(after) {
            lines.push(format!(
                "{:?}: allow {} → {}; deny {} → {}",
                after.kind,
                before.map(|p| p.allow.bits()).unwrap_or(0),
                after.allow.bits(),
                before.map(|p| p.deny.bits()).unwrap_or(0),
                after.deny.bits()
            ));
        }
    }
    for before in &old.permission_overwrites {
        if !new
            .permission_overwrites
            .iter()
            .any(|p| p.kind == before.kind)
        {
            lines.push(format!("Removed override: {:?}", before.kind));
        }
    }
    if lines.is_empty() {
        "Unchanged".into()
    } else {
        clip(&lines.join("\n"), 900)
    }
}
fn channel_event(
    title: &str,
    old: Option<&s::GuildChannel>,
    new: &s::GuildChannel,
) -> Option<s::CreateEmbed> {
    if old.is_some_and(|c| {
        channel_summary(c) == channel_summary(new)
            && c.permission_overwrites == new.permission_overwrites
    }) {
        return None;
    }
    let mut embed = base(
        title,
        format!(
            "**Channel:** <#{}> (`{}`)\nActor details appear separately when Discord supplies an audit event.",
            new.id, new.id
        ),
    );
    if let Some(old) = old {
        embed = embed.field("Before", channel_summary(old), true).field(
            "Permissions changed",
            permission_changes(old, new),
            false,
        );
    } else {
        embed = embed.field("Before", "Unavailable — no cached baseline", true);
    }
    Some(embed.field("Current / final state", channel_summary(new), true))
}
fn role_summary(r: &s::Role) -> String {
    clip(
        &format!(
            "Name: {}\nPermissions: {}\nFlags: {:?}\nPosition: {} · Managed: {}\nHoisted: {} · Mentionable: {}\nColor: #{:06x}",
            r.name,
            r.permissions.bits(),
            r.permissions,
            r.position,
            r.managed,
            r.hoist,
            r.mentionable,
            r.colour.0
        ),
        850,
    )
}
fn server_event(event: &s::FullEvent, guild: s::GuildId) -> Option<s::CreateEmbed> {
    match event {
        s::FullEvent::GuildMemberAddition{new_member:m} if m.guild_id==guild => Some(base("MEMBER JOIN",format!("**Member:** <@{}> (`{}`)\n**Username:** {}\n**Account created:** <t:{}:F>\n**Bot:** {} · **Rules pending:** {}",m.user.id,m.user.id,m.user.name,m.user.id.created_at().unix_timestamp(),m.user.bot,m.pending))),
        s::FullEvent::GuildMemberRemoval{guild_id,user,member_data_if_available} if *guild_id==guild => Some(base("MEMBER DEPARTURE",format!("**Member:** <@{}> (`{}`)\n**Username:** {}\n**Joined:** {}\nA departure alone does not identify a leave, kick or ban. See separate audit records where available.",user.id,user.id,user.name,member_data_if_available.as_ref().and_then(|m|m.joined_at).map(|t|format!("<t:{}:F>",t.unix_timestamp())).unwrap_or_else(||"Unavailable".into())))),
        s::FullEvent::GuildBanAddition{guild_id,banned_user} if *guild_id==guild=>Some(base("MEMBER BAN",format!("**Member:** <@{}> (`{}`)\nBan confirmed by Discord. Actor/reason appear separately when audit data is available.",banned_user.id,banned_user.id))),
        s::FullEvent::GuildBanRemoval{guild_id,unbanned_user} if *guild_id==guild=>Some(base("MEMBER UNBAN",format!("**Member:** <@{}> (`{}`)\nBan removed. Actor/reason appear separately when audit data is available.",unbanned_user.id,unbanned_user.id))),
        s::FullEvent::ChannelCreate{channel} if channel.guild_id==guild=>channel_event("CHANNEL CREATE",None,channel),
        s::FullEvent::ChannelDelete{channel,..} if channel.guild_id==guild=>channel_event("CHANNEL DELETE",None,channel),
        s::FullEvent::ChannelUpdate{old,new} if new.guild_id==guild=>channel_event("CHANNEL UPDATE",old.as_ref(),new),
        s::FullEvent::ThreadCreate{thread} if thread.guild_id==guild=>channel_event("THREAD CREATE",None,thread),
        s::FullEvent::ThreadUpdate{old,new} if new.guild_id==guild=>channel_event("THREAD UPDATE",old.as_ref(),new),
        s::FullEvent::ThreadDelete{thread,..} if thread.guild_id==guild=>Some(base("THREAD DELETE",format!("**Thread ID:** `{}`\n**Parent:** <#{}>",thread.id,thread.parent_id))),
        s::FullEvent::GuildRoleCreate{new} if new.guild_id==guild=>Some(base("ROLE CREATE",format!("**Role:** <@&{}> (`{}`)",new.id,new.id)).field("Configuration",role_summary(new),false)),
        s::FullEvent::GuildRoleUpdate{old_data_if_available:old,new} if new.guild_id==guild=>{
            if old.as_ref().is_some_and(|r|role_summary(r)==role_summary(new)){return None;}
            Some(base("ROLE UPDATE",format!("**Role:** <@&{}> (`{}`)",new.id,new.id)).field("Before",old.as_ref().map(role_summary).unwrap_or_else(||"Unavailable — no cached baseline".into()),true).field("After",role_summary(new),true))
        },
        s::FullEvent::GuildRoleDelete{guild_id,removed_role_id,removed_role_data_if_available} if *guild_id==guild=>Some(base("ROLE DELETE",format!("**Role ID:** `{removed_role_id}`")).field("Last known state",removed_role_data_if_available.as_ref().map(role_summary).unwrap_or_else(||"Unavailable — not cached".into()),false)),
        s::FullEvent::GuildAuditLogEntryCreate{guild_id,entry} if *guild_id==guild=>{
            // Do not dump arbitrary change payloads: webhook tokens/invite codes may be present.
            let mut e=base("AUDIT RECORD",format!("**Action:** `{:?}`\n**Actor:** <@{}> (`{}`)\n**Target ID:** {}\n**Audit reference:** `{}`\n**Recorded:** <t:{}:F>",entry.action,entry.user_id,entry.user_id,entry.target_id.map(|id|format!("`{id}`")).unwrap_or_else(||"Not supplied".into()),entry.id,entry.id.created_at().unix_timestamp())).field("Reason",clip(entry.reason.as_deref().unwrap_or("Not supplied by Discord"),900),false);
            if let Some(o)=&entry.options {e=e.field("Scope",format!("Channel: {}\nCount: {}\nMessage: {}",o.channel_id.map(|i|format!("<#{i}>")).unwrap_or_else(||"Not supplied".into()),o.count.map(|n|n.to_string()).unwrap_or_else(||"Not supplied".into()),o.message_id.map(|i|i.to_string()).unwrap_or_else(||"Not supplied".into())),false);}
            Some(e)
        },
        s::FullEvent::GuildUpdate{old_data_if_available:old,new_data:new} if new.id==guild=>Some(base("SERVER UPDATE",format!("**Server ID:** `{guild}`")).field("Before",old.as_ref().map(|o|clip(&format!("Name: {}\nVerification: {:?}\nMFA: {:?}\nContent filter: {:?}\nAFK timeout: {:?}",o.name,o.verification_level,o.mfa_level,o.explicit_content_filter,o.afk_metadata),850)).unwrap_or_else(||"Unavailable — no cached baseline".into()),true).field("After",clip(&format!("Name: {}\nVerification: {:?}\nMFA: {:?}\nContent filter: {:?}\nAFK timeout: {:?}",new.name,new.verification_level,new.mfa_level,new.explicit_content_filter,new.afk_metadata),850),true)),
        s::FullEvent::WebhookUpdate{guild_id,belongs_to_channel_id} if *guild_id==guild=>Some(base("WEBHOOK UPDATE",format!("**Channel:** <#{belongs_to_channel_id}>\nWebhook configuration changed. URLs and credentials are never copied into this log."))),
        s::FullEvent::InviteCreate{data} if data.guild_id==Some(guild)=>Some(base("INVITE CREATE",format!("**Channel:** <#{}>\n**Creator:** {}\n**Lifetime:** {} seconds (0 = unlimited)\n**Maximum uses:** {} (0 = unlimited)\nInvite code intentionally omitted.",data.channel_id,data.inviter.as_ref().map(|u|format!("<@{}>",u.id)).unwrap_or_else(||"Unavailable".into()),data.max_age,data.max_uses))),
        s::FullEvent::InviteDelete{data} if data.guild_id==Some(guild)=>Some(base("INVITE DELETE",format!("**Channel:** <#{}>\nInvite code intentionally omitted. Actor may appear in a separate audit record.",data.channel_id))),
        s::FullEvent::GuildEmojisUpdate{guild_id,current_state} if *guild_id==guild=>Some(base("SERVER EMOJI UPDATE",format!("**Current emoji count:** {}\nChanges recorded separately when audit data is available.",current_state.len()))),
        s::FullEvent::GuildStickersUpdate{guild_id,current_state} if *guild_id==guild=>Some(base("SERVER STICKER UPDATE",format!("**Current sticker count:** {}",current_state.len()))),
        _=>None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_destination_requires_explicit_everyone_deny_and_accepts_reader_role() {
        let mut c:s::GuildChannel=serde_json::from_value(serde_json::json!({"id":"10","guild_id":"1","type":0,"name":"logs","position":0,"permission_overwrites":[]})).unwrap();
        assert!(!private_channel(&c));
        c.permission_overwrites.push(s::PermissionOverwrite {
            kind: s::PermissionOverwriteType::Role(s::RoleId::new(1)),
            allow: s::Permissions::empty(),
            deny: s::Permissions::VIEW_CHANNEL,
        });
        c.permission_overwrites.push(s::PermissionOverwrite {
            kind: s::PermissionOverwriteType::Role(s::RoleId::new(2)),
            allow: s::Permissions::VIEW_CHANNEL,
            deny: s::Permissions::empty(),
        });
        assert!(private_channel(&c));
        c.permission_overwrites[0].deny = s::Permissions::empty();
        assert!(!private_channel(&c));
    }
    #[test]
    fn permission_change_with_same_count_is_reported_and_long_topics_fit() {
        let old:s::GuildChannel=serde_json::from_value(serde_json::json!({"id":"10","guild_id":"1","type":0,"name":"logs","position":0,"permission_overwrites":[{"id":"1","type":0,"allow":"1024","deny":"0"}]})).unwrap();
        let mut new = old.clone();
        new.permission_overwrites[0].allow = s::Permissions::empty();
        new.permission_overwrites[0].deny = s::Permissions::VIEW_CHANNEL;
        new.topic = Some("😀".repeat(2000));
        assert_ne!(permission_changes(&old, &new), "Unchanged");
        let embed =
            serde_json::to_value(channel_event("CHANNEL UPDATE", Some(&old), &new).unwrap())
                .unwrap();
        for f in embed["fields"].as_array().unwrap() {
            assert!(f["value"].as_str().unwrap().encode_utf16().count() <= 1024);
        }
        assert!(channel_event("CHANNEL UPDATE", Some(&old), &old).is_none());
    }
    #[test]
    fn invite_log_never_discloses_invite_code() {
        let data: s::InviteDeleteEvent = serde_json::from_value(
            serde_json::json!({"channel_id":"2","guild_id":"1","code":"SECRET-INVITE"}),
        )
        .unwrap();
        let event = s::FullEvent::InviteDelete { data };
        let rendered =
            serde_json::to_string(&server_event(&event, s::GuildId::new(1)).unwrap()).unwrap();
        assert!(!rendered.contains("SECRET-INVITE"));
        assert!(server_event(&event, s::GuildId::new(3)).is_none());
    }
    #[test]
    fn cache_evicts_oldest_and_expires_without_losing_recent_text() {
        let now = Instant::now();
        let mut cache = Cache::default();
        for id in 1..=MAX_MESSAGES as u64 + 1 {
            cache.put(
                s::MessageId::new(id),
                Snapshot::default(),
                now + Duration::from_millis(id),
            );
        }
        assert_eq!(cache.messages.len(), MAX_MESSAGES);
        assert!(!cache.messages.contains_key(&s::MessageId::new(1)));
        cache.prune(now + RETENTION + Duration::from_secs(2));
        assert!(cache.messages.is_empty());
    }
    #[test]
    fn log_fields_fit_discord_limits_and_keep_unknown_distinct_from_empty() {
        let text = "😀".repeat(1000);
        assert!(body(Some(&text)).encode_utf16().count() <= 1024);
        assert!(body(None).contains("not cached"));
        assert!(body(Some("")).contains("No text"));
        assert!(!body(Some("```@everyone```")).contains("```@everyone```"));
    }
    #[test]
    fn voice_events_distinguish_join_leave_move_and_stationary() {
        let a = Some(s::ChannelId::new(1));
        let b = Some(s::ChannelId::new(2));
        assert_eq!(voice_action(None, a).unwrap().0, "VOICE JOIN");
        assert_eq!(voice_action(a, None).unwrap().0, "VOICE LEAVE");
        assert_eq!(voice_action(a, b).unwrap().0, "VOICE MOVE");
        assert!(voice_action(a, a).is_none());
    }
}
