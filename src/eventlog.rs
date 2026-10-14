use crate::{Data, serenity as s};
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
    s::CreateEmbed::new()
        .title(format!("ELVNX / {title}"))
        .description(clip(&description, 1800))
        .color(0x111111)
        .timestamp(s::Timestamp::now())
        .footer(s::CreateEmbedFooter::new("avrxt.dev • staff activity log"))
}
async fn send(ctx: &s::Context, channel: u64, embed: s::CreateEmbed, transcript: Option<String>) {
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
    }
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
    let now = Instant::now();
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
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
