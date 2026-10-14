use crate::{Data, Error, log, security, serenity as s, store};
use std::time::{Duration, Instant};

pub fn violation(
    text: &str,
    mentions: usize,
    spam: bool,
    cfg: &store::Settings,
) -> Option<&'static str> {
    if !cfg.automod {
        return None;
    }
    if spam {
        return Some("message flood");
    }
    if mentions > 5 {
        return Some("mass mentions");
    }
    let lower = text.to_lowercase();
    if cfg.block_invites
        && (lower.contains("discord.gg/")
            || lower.contains("discord.com/invite/")
            || lower.contains("discordapp.com/invite/"))
    {
        return Some("Discord invite");
    }
    if lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| cfg.blocked_words.iter().any(|bad| word == bad))
    {
        return Some("blocked word");
    }
    None
}

pub async fn handle(
    ctx: &s::Context,
    event: &s::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    data: &Data,
) -> Result<(), Error> {
    let bot_id = ctx.cache.current_user().id;
    data.availability.observe(event).await;
    if let s::FullEvent::ShardStageUpdate { event } = event {
        data.afk
            .connection(event.new == s::ConnectionStage::Connected)
            .await;
    }
    if let s::FullEvent::VoiceStateUpdate { old, new } = event
        && new.guild_id == Some(data.guild)
    {
        data.afk.observe(ctx, new).await;
        if let Err(error) = crate::legacy::voice(ctx, data, old.as_ref(), new).await {
            tracing::warn!(%error, "Appukuttan join announcement failed; check destination permissions");
        }
    }
    crate::music::gateway(ctx, event, data).await;
    crate::eventlog::handle(ctx, event, data).await;
    if crate::toolbox::gateway(ctx, event, data).await? {
        return Ok(());
    }
    if crate::features::gateway(ctx, event, data).await? {
        return Ok(());
    }
    if let s::FullEvent::Message { new_message } = event {
        if let Err(error) = crate::always_react::message(ctx, data, new_message).await {
            tracing::warn!(%error, "Always-react failed; check channel reaction permissions");
        }
        crate::daily::message(data, new_message).await?;
    }
    if crate::community::gateway(ctx, event, data).await? {
        return Ok(());
    }
    if crate::extras::gateway(ctx, event, data).await? {
        return Ok(());
    }
    match event {
        s::FullEvent::Message {
            new_message: message,
        } if message.guild_id == Some(data.guild)
            && !message.author.bot
            && message.webhook_id.is_none() =>
        {
            let cfg = data.settings.read().await.clone();
            let rules = data.rules.read().await.clone();
            let spam = if cfg.automod {
                let now = Instant::now();
                let mut map = data.spam.lock().await;
                map.retain(|_, times| {
                    times
                        .last()
                        .is_some_and(|t| now.duration_since(*t) < Duration::from_secs(15))
                });
                if map.len() >= 10000 && !map.contains_key(&message.author.id.get()) {
                    false
                } else {
                    let times = map.entry(message.author.id.get()).or_default();
                    times.retain(|t| now.duration_since(*t) < Duration::from_secs(8));
                    if times.len() < 100 {
                        times.push(now);
                    }
                    times.len() > rules.spam_limit
                }
            } else {
                false
            };
            let mentions = message.mentions.len()
                + message.mention_roles.len()
                + usize::from(message.mention_everyone) * (rules.mention_limit + 1);
            let custom = if cfg.automod {
                if mentions > rules.mention_limit {
                    Some("mass mentions")
                } else {
                    rules.text_reason(&message.content, message.attachments.len())
                }
            } else {
                None
            };
            if let Some(reason) = violation(&message.content, 0, spam, &cfg)
                .or(custom)
                .filter(|_| !rules.exempt_channels.contains(&message.channel_id.get()))
            {
                let guild = data.guild.to_partial_guild(&ctx.http).await?;
                let member = data.guild.member(&ctx.http, message.author.id).await?;
                if member.user.id != guild.owner_id
                    && !member
                        .roles
                        .iter()
                        .any(|r| rules.exempt_roles.contains(&r.get()))
                    && !guild.member_permissions(&member).intersects(
                        s::Permissions::ADMINISTRATOR
                            | s::Permissions::MANAGE_MESSAGES
                            | s::Permissions::MODERATE_MEMBERS,
                    )
                {
                    message.delete(&ctx.http).await?;
                    let id = store::case(
                        &data.db,
                        data.guild.get(),
                        message.author.id.get(),
                        bot_id.get(),
                        "automod-delete",
                        reason,
                    )
                    .await?;
                    log(&ctx.http,data,format!("Case #{id} • automod • {reason}\nMember: {}\nChannel: {}\nMessage content not retained.",message.author.id,message.channel_id)).await;
                }
            }
            data.stickies
                .queue(ctx.http.clone(), message.channel_id)
                .await;
        }
        s::FullEvent::GuildMemberAddition { new_member: member }
            if member.guild_id == data.guild =>
        {
            let cfg = data.settings.read().await.clone();
            if !member.user.bot {
                if let Some(role) = cfg.autorole {
                    let guild = data.guild.to_partial_guild(&ctx.http).await?;
                    let bot = data.guild.member(&ctx.http, bot_id).await?;
                    if let Some(role) = guild.roles.get(&s::RoleId::new(role))
                        && !role.managed
                        && role.permissions.is_empty()
                        && security::higher(
                            security::highest(&guild, &bot),
                            (i64::from(role.position), role.id.get()),
                        )
                    {
                        member.add_role(&ctx.http, role.id).await?;
                    }
                }
                if let Some(channel) = cfg.welcome_channel {
                    let text = if cfg.welcome_message.is_empty() {
                        format!("Welcome to ELVNX, {}!", member.user.name)
                    } else {
                        cfg.welcome_message
                            .replace("{user}", &member.user.name)
                            .replace("{server}", "ELVNX")
                    };
                    let mut welcome = s::CreateMessage::new()
                        .embed(
                            s::CreateEmbed::new()
                                .title("ELVNX / WELCOME")
                                .description(text)
                                .color(0x111111),
                        )
                        .allowed_mentions(s::CreateAllowedMentions::new());
                    if cfg.welcome_images {
                        let avatar = crate::cards::avatar(&member.user).await;
                        let png = crate::cards::render(
                            "WELCOME TO ELVNX",
                            &member.user.name,
                            "A private collective. Build. Explore. Connect.",
                            &[],
                            avatar.as_deref(),
                        )?;
                        welcome = welcome.add_file(s::CreateAttachment::bytes(png, "welcome.png"));
                    }
                    s::ChannelId::new(channel)
                        .send_message(&ctx.http, welcome)
                        .await?;
                }
            }
            log(
                &ctx.http,
                data,
                format!("Member joined: {} ({})", member.user.name, member.user.id),
            )
            .await;
        }
        s::FullEvent::GuildMemberRemoval { guild_id, user, .. } if *guild_id == data.guild => {
            log(
                &ctx.http,
                data,
                format!("Member left: {} ({})", user.name, user.id),
            )
            .await
        }
        s::FullEvent::GuildBanAddition {
            guild_id,
            banned_user,
        } if *guild_id == data.guild => {
            log(
                &ctx.http,
                data,
                format!("Member banned: {}", banned_user.id),
            )
            .await
        }
        s::FullEvent::GuildBanRemoval {
            guild_id,
            unbanned_user,
        } if *guild_id == data.guild => {
            log(
                &ctx.http,
                data,
                format!("Member unbanned: {}", unbanned_user.id),
            )
            .await
        }
        s::FullEvent::ChannelCreate { channel } if channel.guild_id == data.guild => {
            log(
                &ctx.http,
                data,
                format!("Channel created: {} ({})", channel.name, channel.id),
            )
            .await
        }
        s::FullEvent::ChannelDelete { channel, .. } if channel.guild_id == data.guild => {
            log(
                &ctx.http,
                data,
                format!("Channel deleted: {} ({})", channel.name, channel.id),
            )
            .await
        }
        s::FullEvent::ChannelUpdate { new, .. } if new.guild_id == data.guild => {
            log(
                &ctx.http,
                data,
                format!("Channel updated: {} ({})", new.name, new.id),
            )
            .await
        }
        s::FullEvent::GuildRoleCreate { new } if new.guild_id == data.guild => {
            log(
                &ctx.http,
                data,
                format!("Role created: {} ({})", new.name, new.id),
            )
            .await
        }
        s::FullEvent::GuildRoleUpdate { new, .. } if new.guild_id == data.guild => {
            log(
                &ctx.http,
                data,
                format!("Role updated: {} ({})", new.name, new.id),
            )
            .await
        }
        s::FullEvent::GuildRoleDelete {
            guild_id,
            removed_role_id,
            ..
        } if *guild_id == data.guild => {
            log(&ctx.http, data, format!("Role deleted: {removed_role_id}")).await
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automod_requires_opt_in_and_matches_whole_words() {
        let mut cfg = store::Settings {
            blocked_words: vec!["bad".into()],
            ..Default::default()
        };
        assert_eq!(violation("bad", 0, false, &cfg), None);
        cfg.automod = true;
        assert_eq!(violation("badminton", 0, false, &cfg), None);
        assert_eq!(violation("BAD!", 0, false, &cfg), Some("blocked word"));
    }
    #[test]
    fn mentions_and_spam_are_bounded() {
        let cfg = store::Settings {
            automod: true,
            block_invites: true,
            ..Default::default()
        };
        assert!(violation("hello", 5, false, &cfg).is_none());
        assert_eq!(violation("", 6, false, &cfg), Some("mass mentions"));
        assert_eq!(violation("", 0, true, &cfg), Some("message flood"));
        assert_eq!(
            violation("https://discord.gg/example", 0, false, &cfg),
            Some("Discord invite")
        );
    }
}
