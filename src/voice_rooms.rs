use crate::{Context, Data, Error, cards, emoji, reply, serenity as s, store};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::time::Duration;

const TITLE: &str = "ELVNX / VOICE LOUNGE";
fn now() -> i64 {
    s::Timestamp::now().unix_timestamp()
}
fn bot(ctx: &s::Context) -> s::UserId {
    ctx.cache.current_user().id
}
fn gate() -> s::Permissions {
    s::Permissions::VIEW_CHANNEL | s::Permissions::CONNECT
}
fn staff() -> s::Permissions {
    s::Permissions::ADMINISTRATOR
        | s::Permissions::MANAGE_GUILD
        | s::Permissions::MANAGE_CHANNELS
        | s::Permissions::MODERATE_MEMBERS
}
#[derive(Clone, Serialize, Deserialize)]
struct State {
    base: Vec<s::PermissionOverwrite>,
    parent: Option<s::ChannelId>,
    created: i64,
    locked: bool,
    hidden: bool,
    sharing: bool,
    owner_left: Option<i64>,
}
struct Room {
    channel: s::GuildChannel,
    owner: s::UserId,
    state: State,
}
fn present(ctx: &s::Context, guild: s::GuildId, user: s::UserId) -> Option<s::ChannelId> {
    ctx.cache
        .guild(guild)
        .and_then(|g| g.voice_states.get(&user).and_then(|v| v.channel_id))
}
fn occupied(ctx: &s::Context, guild: s::GuildId, channel: s::ChannelId) -> Option<usize> {
    ctx.cache.guild(guild).map(|g| {
        g.voice_states
            .values()
            .filter(|v| v.channel_id == Some(channel))
            .count()
    })
}
async fn save(
    db: &SqlitePool,
    channel: s::ChannelId,
    guild: s::GuildId,
    state: &State,
) -> Result<(), Error> {
    sqlx::query("INSERT INTO voice_room_state(channel,guild,value) VALUES(?,?,?) ON CONFLICT(channel) DO UPDATE SET value=excluded.value").bind(channel.to_string()).bind(guild.to_string()).bind(serde_json::to_string(state)?).execute(db).await?;
    Ok(())
}
async fn forget(db: &SqlitePool, guild: s::GuildId, channel: s::ChannelId) -> Result<(), Error> {
    let mut tx = db.begin().await?;
    for table in ["voice_rooms", "voice_room_state", "voice_room_guests"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE guild=? AND channel=?"))
            .bind(guild.to_string())
            .bind(channel.to_string())
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE voice_room_invites SET state='closed' WHERE guild=? AND channel=? AND state IN('pending','granting')").bind(guild.to_string()).bind(channel.to_string()).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
fn set_member(
    overwrites: &mut Vec<s::PermissionOverwrite>,
    user: s::UserId,
    allow: s::Permissions,
    deny: s::Permissions,
) {
    let kind = s::PermissionOverwriteType::Member(user);
    if let Some(p) = overwrites.iter_mut().find(|p| p.kind == kind) {
        p.allow = (p.allow & !deny) | allow;
        p.deny = (p.deny & !allow) | deny;
    } else {
        overwrites.push(s::PermissionOverwrite { kind, allow, deny });
    }
}
fn template(
    category: Option<&s::GuildChannel>,
    hub: &s::GuildChannel,
) -> Vec<s::PermissionOverwrite> {
    if let Some(category) = category {
        return category.permission_overwrites.clone();
    }
    let mut base = hub.permission_overwrites.clone();
    // A category-less lobby commonly denies these bits solely to keep the hub silent.
    let kind = s::PermissionOverwriteType::Role(s::RoleId::new(hub.guild_id.get()));
    let voice = s::Permissions::SPEAK | s::Permissions::USE_VAD | s::Permissions::STREAM;
    if let Some(p) = base.iter_mut().find(|p| p.kind == kind) {
        p.deny.remove(voice);
        p.allow.insert(voice);
    } else {
        base.push(s::PermissionOverwrite {
            kind,
            allow: voice,
            deny: s::Permissions::empty(),
        });
    }
    base
}
async fn category(
    ctx: &s::Context,
    guild: s::GuildId,
    parent: Option<s::ChannelId>,
) -> Result<Option<s::GuildChannel>, Error> {
    if let Some(id) = parent {
        let ch = id
            .to_channel(&ctx.http)
            .await?
            .guild()
            .ok_or_else(|| crate::errors::user("The room category is unavailable."))?;
        if ch.guild_id != guild || ch.kind != s::ChannelType::Category {
            return Err(crate::errors::user(
                "Room category changed. Ask staff to update /join-to-create.",
            ));
        }
        Ok(Some(ch))
    } else {
        Ok(None)
    }
}
async fn eligible(
    ctx: &s::Context,
    guild: s::GuildId,
    parent: Option<s::ChannelId>,
    user: s::UserId,
) -> Result<s::Member, Error> {
    let m = guild.member(&ctx.http, user).await?;
    if m.user.bot || m.pending {
        return Err(crate::errors::user(
            "Choose a human member who has completed Discord's membership screening.",
        ));
    }
    let g = guild.to_partial_guild(&ctx.http).await?;
    let permissions = if let Some(c) = category(ctx, guild, parent).await? {
        g.user_permissions_in(&c, &m)
    } else {
        g.member_permissions(&m)
    };
    if !permissions.contains(gate()) {
        return Err(crate::errors::user(
            "This member does not have access to the voice category. Complete server verification or ask staff; room controls cannot bypass category access.",
        ));
    }
    Ok(m)
}
fn same_roles(a: &[s::PermissionOverwrite], b: &[s::PermissionOverwrite]) -> bool {
    let roles = |ps: &[s::PermissionOverwrite]| {
        let mut result = ps
            .iter()
            .filter_map(|p| match p.kind {
                s::PermissionOverwriteType::Role(id) => {
                    Some((id.get(), p.allow.bits(), p.deny.bits()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        result.sort_unstable();
        result
    };
    roles(a) == roles(b)
}
async fn load(
    ctx: &s::Context,
    db: &SqlitePool,
    guild: s::GuildId,
    channel: s::ChannelId,
) -> Result<Room, Error> {
    let owner: String =
        sqlx::query_scalar("SELECT owner FROM voice_rooms WHERE guild=? AND channel=?")
            .bind(guild.to_string())
            .bind(channel.to_string())
            .fetch_optional(db)
            .await?
            .ok_or_else(|| crate::errors::user("This is not a bot-created temporary room."))?;
    let owner = s::UserId::new(owner.parse()?);
    let mut ch = channel
        .to_channel(&ctx.http)
        .await?
        .guild()
        .ok_or_else(|| crate::errors::user("The room has been removed."))?;
    if ch.guild_id != guild || ch.kind != s::ChannelType::Voice {
        return Err(crate::errors::user(
            "This room is no longer a normal voice channel.",
        ));
    }
    let raw: Option<String> =
        sqlx::query_scalar("SELECT value FROM voice_room_state WHERE guild=? AND channel=?")
            .bind(guild.to_string())
            .bind(channel.to_string())
            .fetch_optional(db)
            .await?;
    let state = if let Some(raw) = raw {
        serde_json::from_str::<State>(&raw)?
    } else {
        let cfg = store::settings(db, guild.get()).await?;
        let mut base = ch.permission_overwrites.clone();
        if let Some(hub) = cfg.join_to_create {
            let hub = s::ChannelId::new(hub)
                .to_channel(&ctx.http)
                .await?
                .guild()
                .ok_or_else(|| crate::errors::user("Voice hub unavailable."))?;
            if hub.parent_id == ch.parent_id && same_roles(&base, &hub.permission_overwrites) {
                let parent = category(ctx, guild, ch.parent_id).await?;
                let mut repaired = template(parent.as_ref(), &hub);
                // Preserve explicit member moderation exceptions when repairing the old lobby copy.
                for p in &base {
                    if matches!(p.kind, s::PermissionOverwriteType::Member(_)) {
                        repaired.retain(|r| r.kind != p.kind);
                        repaired.push(p.clone());
                    }
                }
                base = repaired;
            }
        }
        set_member(&mut base, owner, gate(), s::Permissions::empty());
        set_member(
            &mut base,
            bot(ctx),
            gate()
                | s::Permissions::MANAGE_CHANNELS
                | s::Permissions::MOVE_MEMBERS
                | s::Permissions::MANAGE_ROLES,
            s::Permissions::empty(),
        );
        let state = State {
            base: base.clone(),
            parent: ch.parent_id,
            created: now() - 31,
            locked: false,
            hidden: false,
            sharing: true,
            owner_left: if present(ctx, guild, owner) == Some(channel) {
                None
            } else {
                Some(now())
            },
        };
        ch = channel
            .edit(&ctx.http, s::EditChannel::new().permissions(base))
            .await?;
        save(db, channel, guild, &state).await?;
        state
    };
    if state.parent != ch.parent_id {
        return Err(crate::errors::user(
            "Staff moved this room to another category. Ask staff to reconfigure it before using room controls.",
        ));
    }
    Ok(Room {
        channel: ch,
        owner,
        state,
    })
}
fn compose(
    room: &Room,
    guild: &s::PartialGuild,
    bot: s::UserId,
    guests: &[(s::UserId, bool)],
) -> Vec<s::PermissionOverwrite> {
    let mut out = room.channel.permission_overwrites.clone();
    let managed = gate() | s::Permissions::STREAM;
    // Preserve Speak, server mutes and every unrelated permission; only these UI-managed bits change.
    for p in &mut out {
        if !room.state.base.iter().any(|o| o.kind == p.kind) {
            p.allow.remove(managed);
            p.deny.remove(managed);
        }
    }
    for original in &room.state.base {
        if let Some(p) = out.iter_mut().find(|p| p.kind == original.kind) {
            p.allow = (p.allow & !managed) | (original.allow & managed);
            p.deny = (p.deny & !managed) | (original.deny & managed);
        } else {
            out.push(original.clone());
        }
    }
    let everyone = s::PermissionOverwriteType::Role(s::RoleId::new(guild.id.get()));
    if !out.iter().any(|p| p.kind == everyone) {
        out.push(s::PermissionOverwrite {
            kind: everyone,
            allow: s::Permissions::empty(),
            deny: s::Permissions::empty(),
        });
    }
    for p in &mut out {
        let protected = match p.kind {
            s::PermissionOverwriteType::Role(id) => guild
                .roles
                .get(&id)
                .is_some_and(|r| r.permissions.intersects(staff())),
            s::PermissionOverwriteType::Member(id) => id == room.owner || id == bot,
            _ => false,
        };
        if !protected {
            let mut denied = s::Permissions::empty();
            if room.state.locked {
                denied.insert(s::Permissions::CONNECT);
            }
            if room.state.hidden {
                denied.insert(s::Permissions::VIEW_CHANNEL);
            }

            p.allow.remove(denied);
            p.deny.insert(denied);
        }
    }
    for (user, blocked) in guests {
        if *blocked {
            set_member(&mut out, *user, s::Permissions::empty(), gate());
        } else {
            set_member(&mut out, *user, gate(), s::Permissions::empty());
        }
    }
    if !guests.contains(&(room.owner, true)) {
        set_member(&mut out, room.owner, gate(), s::Permissions::empty());
    }
    set_member(
        &mut out,
        bot,
        gate()
            | s::Permissions::MANAGE_CHANNELS
            | s::Permissions::MOVE_MEMBERS
            | s::Permissions::MANAGE_ROLES,
        s::Permissions::empty(),
    );
    if !room.state.sharing {
        for p in &mut out {
            if p.kind != s::PermissionOverwriteType::Member(bot) {
                p.allow.remove(s::Permissions::STREAM);
                p.deny.insert(s::Permissions::STREAM);
            }
        }
    }
    out
}
async fn apply(ctx: &s::Context, data: &Data, room: &mut Room) -> Result<(), Error> {
    let rows =
        sqlx::query("SELECT user,blocked FROM voice_room_guests WHERE guild=? AND channel=?")
            .bind(data.guild.to_string())
            .bind(room.channel.id.to_string())
            .fetch_all(&data.db)
            .await?;
    let guests = rows
        .iter()
        .map(|r| {
            Ok((
                s::UserId::new(r.get::<String, _>("user").parse::<u64>()?),
                r.get::<bool, _>("blocked"),
            ))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let guild = data.guild.to_partial_guild(&ctx.http).await?;
    let permissions = compose(room, &guild, bot(ctx), &guests);
    room.channel = room
        .channel
        .id
        .edit(&ctx.http, s::EditChannel::new().permissions(permissions))
        .await?;
    save(&data.db, room.channel.id, data.guild, &room.state).await
}
async fn current(
    ctx: &s::Context,
    data: &Data,
    user: s::UserId,
    owner: bool,
) -> Result<Room, Error> {
    let channel = present(ctx, data.guild, user).ok_or_else(|| {
        crate::errors::user(
            "Join your temporary voice room first, then use the voice control panel.",
        )
    })?;
    let room = load(ctx, &data.db, data.guild, channel).await?;
    if owner && room.owner != user {
        return Err(crate::errors::user(
            "Only this room's owner can use that control. Use Room details to see the owner, or Claim room after the owner has been away for two minutes.",
        ));
    }
    eligible(ctx, data.guild, room.state.parent, user).await?;
    Ok(room)
}
async fn quota(
    db: &SqlitePool,
    guild: s::GuildId,
    user: s::UserId,
    kind: &str,
    time: i64,
    seconds: i64,
    max: i64,
) -> Result<bool, Error> {
    Ok(sqlx::query("INSERT INTO voice_room_limits(guild,user,kind,window,count) VALUES(?,?,?,?,1) ON CONFLICT(guild,user,kind) DO UPDATE SET window=excluded.window,count=CASE WHEN window=excluded.window THEN count+1 ELSE 1 END WHERE window!=excluded.window OR count<?").bind(guild.to_string()).bind(user.to_string()).bind(kind).bind(time.div_euclid(seconds)).bind(max).execute(db).await?.rows_affected()==1)
}
pub fn cover() -> Result<Vec<u8>, Error> {
    cards::render(
        "VOICE / YOUR SPACE",
        "ELVNX LOUNGE",
        "Join. Make it yours. Invite your people.",
        &[
            ("CONTROL", "Owner only".into()),
            ("INVITES", "10 min / accept".into()),
            ("CLEANUP", "Automatic".into()),
        ],
        None,
    )
}
fn controls() -> Vec<s::CreateActionRow> {
    let rows = [
        vec![
            ("info", "Room details", "microphone"),
            ("rename", "Rename", "typing"),
            ("limit", "User limit", "user"),
            ("lock", "Lock", "lock"),
            ("unlock", "Unlock", "unlock"),
        ],
        vec![
            ("hide", "Hide", "offline"),
            ("show", "Show", "online"),
            ("sharing", "Screen share", "play"),
            ("invite", "Invite member", "mail"),
            ("revoke", "Remove access", "shield"),
        ],
        vec![
            ("transfer", "Transfer owner", "user"),
            ("claim", "Claim room", "check"),
            ("disconnect", "Disconnect member", "cross"),
            ("settings", "Audio settings", "settings"),
            ("delete", "Delete room", "warning"),
        ],
    ];
    rows.into_iter()
        .map(|row| {
            s::CreateActionRow::Buttons(
                row.into_iter()
                    .map(|(id, label, key)| {
                        emoji::button_for(format!("elvnx:voice:{id}"), key)
                            .label(label)
                            .style(s::ButtonStyle::Secondary)
                    })
                    .collect(),
            )
        })
        .collect()
}
fn panel(hub: s::ChannelId) -> s::CreateEmbed {
    emoji::embed_for("headphones").title(TITLE).description(format!("{} **Your room. Your people.**\n\nJoin <#{hub}> to create a temporary room, then use these controls while connected. Only the current owner can change the room.\n\n**Shape your space** · rename, limits, privacy and screen sharing.\n**Bring your people** · send an expiring invitation DM to a server member. They accept before room access is granted.\n**Pass the mic** · transfer ownership or claim an abandoned room after two minutes.\n\nEmpty bot-created rooms clean up automatically. Server verification, moderation permissions and Discord voice limits still apply.",emoji::text("microphone"))).image("attachment://voice-lounge.png")
}
pub async fn refresh(
    ctx: &s::Context,
    db: &SqlitePool,
    guild: s::GuildId,
    force: bool,
) -> Result<(), Error> {
    let cfg = store::settings(db, guild.get()).await?;
    let (Some(hub), Some(channel)) = (cfg.join_to_create, cfg.voice_panel) else {
        return Ok(());
    };
    let channel = s::ChannelId::new(channel);
    let cfgrow = sqlx::query("SELECT channel,message FROM voice_room_panels WHERE guild=?")
        .bind(guild.to_string())
        .fetch_optional(db)
        .await?;
    let mut existing = None;
    if let Some(row) = cfgrow
        && row.get::<String, _>("channel") == channel.to_string()
    {
        let id = s::MessageId::new(row.get::<String, _>("message").parse()?);
        if let Ok(m) = channel.message(&ctx.http, id).await
            && m.author.id == bot(ctx)
        {
            if !force {
                return Ok(());
            }
            existing = Some(m);
        }
    }
    if existing.is_none() {
        existing = channel
            .messages(&ctx.http, s::GetMessages::new().limit(100))
            .await?
            .into_iter()
            .find(|m| {
                m.author.id == bot(ctx)
                    && m.embeds.iter().any(|e| {
                        matches!(
                            e.title.as_deref(),
                            Some(TITLE) | Some("ELVNX / VOICE CONTROL")
                        )
                    })
            });
    }
    let png = cover()?;
    let embed = panel(s::ChannelId::new(hub));
    let m = if let Some(old) = existing {
        channel
            .edit_message(
                &ctx.http,
                old.id,
                s::EditMessage::new()
                    .content("")
                    .embed(embed)
                    .components(controls())
                    .attachments(
                        s::EditAttachments::new()
                            .add(s::CreateAttachment::bytes(png, "voice-lounge.png")),
                    )
                    .allowed_mentions(s::CreateAllowedMentions::new()),
            )
            .await?
    } else {
        channel
            .send_message(
                &ctx.http,
                s::CreateMessage::new()
                    .embed(embed)
                    .components(controls())
                    .add_file(s::CreateAttachment::bytes(png, "voice-lounge.png"))
                    .allowed_mentions(s::CreateAllowedMentions::new()),
            )
            .await?
    };
    sqlx::query("INSERT INTO voice_room_panels(guild,channel,message) VALUES(?,?,?) ON CONFLICT(guild) DO UPDATE SET channel=excluded.channel,message=excluded.message").bind(guild.to_string()).bind(channel.to_string()).bind(m.id.to_string()).execute(db).await?;
    Ok(())
}
pub async fn configure(
    ctx: Context<'_>,
    enabled: bool,
    hub: Option<s::GuildChannel>,
    panel: Option<s::GuildChannel>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let _guard = ctx.data().configuration_lock.lock().await;
    let mut cfg = ctx.data().settings.read().await.clone();
    if enabled {
        let hub = hub.ok_or_else(|| crate::errors::user("Choose the voice hub."))?;
        let panel = panel
            .ok_or_else(|| crate::errors::user("Choose a text channel for the lounge panel."))?;
        if hub.guild_id != ctx.data().guild
            || hub.kind != s::ChannelType::Voice
            || panel.kind != s::ChannelType::Text
        {
            return Err(crate::errors::user(
                "Select a normal voice hub and server text panel.",
            ));
        }
        crate::commands::posting_channel(ctx, &panel).await?;
        let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
        let member = ctx
            .data()
            .guild
            .member(ctx.http(), crate::bot_id(ctx))
            .await?;
        if !guild.member_permissions(&member).contains(
            s::Permissions::MANAGE_CHANNELS
                | s::Permissions::MOVE_MEMBERS
                | s::Permissions::MANAGE_ROLES,
        ) || !guild.user_permissions_in(&panel, &member).contains(
            s::Permissions::ATTACH_FILES
                | s::Permissions::EMBED_LINKS
                | s::Permissions::READ_MESSAGE_HISTORY,
        ) {
            return Err(crate::errors::user(
                "The bot needs Manage Channels, Move Members and Manage Roles, plus Embed Links, Attach Files and Read Message History in the panel channel.",
            ));
        }
        cfg.join_to_create = Some(hub.id.get());
        cfg.voice_panel = Some(panel.id.get());
    } else {
        cfg.join_to_create = None;
        cfg.voice_panel = None;
    }
    store::save(&ctx.data().db, ctx.data().guild.get(), &cfg).await?;
    *ctx.data().settings.write().await = cfg;
    if enabled {
        refresh(
            ctx.serenity_context(),
            &ctx.data().db,
            ctx.data().guild,
            true,
        )
        .await?;
    }
    reply(ctx,if enabled{"Voice lounge enabled. Created rooms use category voice permissions; owner controls and targeted invitation DMs are ready."}else{"New room creation is disabled. Existing temporary rooms retain their controls and empty-room cleanup."}).await
}
async fn create(
    ctx: &s::Context,
    data: &Data,
    user: s::UserId,
    hub: s::ChannelId,
) -> Result<(), Error> {
    let _guard = data.configuration_lock.lock().await;
    if present(ctx, data.guild, user) != Some(hub) {
        return Ok(());
    }
    let existing: Option<String> =
        sqlx::query_scalar("SELECT channel FROM voice_rooms WHERE guild=? AND owner=? LIMIT 1")
            .bind(data.guild.to_string())
            .bind(user.to_string())
            .fetch_optional(&data.db)
            .await?;
    if let Some(raw) = existing {
        let id = s::ChannelId::new(raw.parse()?);
        match load(ctx, &data.db, data.guild, id).await {
            Ok(room) => {
                eligible(ctx, data.guild, room.state.parent, user).await?;
                data.guild.move_member(&ctx.http, user, id).await?;
                return Ok(());
            }
            Err(_) => {
                if matches!(id.to_channel(&ctx.http).await, Err(s::Error::Http(s::HttpError::UnsuccessfulRequest(ref r))) if r.status_code.as_u16() == 404)
                {
                    forget(&data.db, data.guild, id).await?;
                } else {
                    return Err(crate::errors::user(
                        "Your existing room needs staff review before another can be created.",
                    ));
                }
            }
        }
    }
    if !quota(&data.db, data.guild, user, "create", now(), 300, 3).await? {
        return Err(crate::errors::user(
            "You created rooms too quickly. Wait for the next five-minute window before trying again.",
        ));
    }
    let hub = hub
        .to_channel(&ctx.http)
        .await?
        .guild()
        .ok_or_else(|| crate::errors::user("Voice hub unavailable."))?;
    let member = eligible(ctx, data.guild, hub.parent_id, user).await?;
    let parent = category(ctx, data.guild, hub.parent_id).await?;
    let mut base = template(parent.as_ref(), &hub);
    set_member(&mut base, user, gate(), s::Permissions::empty());
    set_member(
        &mut base,
        bot(ctx),
        gate()
            | s::Permissions::MANAGE_CHANNELS
            | s::Permissions::MOVE_MEMBERS
            | s::Permissions::MANAGE_ROLES,
        s::Permissions::empty(),
    );
    let mut builder = s::CreateChannel::new(format!(
        "{}'s room",
        member.user.name.chars().take(40).collect::<String>()
    ))
    .kind(s::ChannelType::Voice)
    .permissions(base.clone())
    .bitrate(64000);
    if let Some(parent) = hub.parent_id {
        builder = builder.category(parent);
    }
    let channel = data.guild.create_channel(&ctx.http, builder).await?;
    let state = State {
        base,
        parent: hub.parent_id,
        created: now(),
        locked: false,
        hidden: false,
        sharing: true,
        owner_left: None,
    };
    let saved: Result<(), Error> = async {
        let mut tx = data.db.begin().await?;
        sqlx::query("INSERT INTO voice_rooms(guild,channel,owner) VALUES(?,?,?)")
            .bind(data.guild.to_string())
            .bind(channel.id.to_string())
            .bind(user.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO voice_room_state(channel,guild,value) VALUES(?,?,?)")
            .bind(channel.id.to_string())
            .bind(data.guild.to_string())
            .bind(serde_json::to_string(&state)?)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
    .await;
    if let Err(error) = saved {
        let _ = channel.id.delete(&ctx.http).await;
        return Err(error);
    }
    if present(ctx, data.guild, user) != Some(hub.id)
        || member
            .move_to_voice_channel(&ctx.http, channel.id)
            .await
            .is_err()
    {
        if occupied(ctx, data.guild, channel.id) == Some(0)
            && channel.id.delete(&ctx.http).await.is_ok()
        {
            forget(&data.db, data.guild, channel.id).await?;
        }
        return Err(crate::errors::user(
            "The room was created, but Discord could not move you. Rejoin the hub to try your existing room again.",
        ));
    }
    Ok(())
}
fn private_reply(title: &str, text: impl Into<String>) -> s::EditInteractionResponse {
    s::EditInteractionResponse::new()
        .embed(
            emoji::embed_for("microphone")
                .title(format!("ELVNX / {title}"))
                .description(text),
        )
        .allowed_mentions(s::CreateAllowedMentions::new())
}
fn scoped(action: &str, room: &Room, user: s::UserId) -> String {
    format!("elvnx:voice:{action}:{}:{user}", room.channel.id)
}
fn user_menu(action: &str, room: &Room, user: s::UserId) -> s::CreateActionRow {
    s::CreateActionRow::SelectMenu(
        s::CreateSelectMenu::new(
            scoped(&format!("select-{action}"), room, user),
            s::CreateSelectMenuKind::User {
                default_users: None,
            },
        )
        .placeholder("Choose one server member")
        .min_values(1)
        .max_values(1),
    )
}
fn clean_name(text: &str) -> Result<String, Error> {
    let value = text.trim();
    if value.is_empty() || value.encode_utf16().count() > 80 || value.chars().any(char::is_control)
    {
        Err(crate::errors::user(
            "Use a room name of 1–80 characters without line breaks or control characters.",
        ))
    } else {
        Ok(value.into())
    }
}
fn limit(text: &str) -> Result<u32, Error> {
    text.trim()
        .parse::<u32>()
        .ok()
        .filter(|n| *n <= 99)
        .ok_or_else(|| {
            crate::errors::user("Enter a whole number from 0 to 99. Zero means unlimited.")
        })
}
#[derive(poise::Modal)]
#[name = "ELVNX / Rename room"]
struct Rename {
    #[name = "Room name"]
    #[min_length = 1]
    #[max_length = 80]
    value: String,
}
#[derive(poise::Modal)]
#[name = "ELVNX / Room capacity"]
struct Capacity {
    #[name = "Member limit · 0 means unlimited"]
    #[min_length = 1]
    #[max_length = 2]
    value: String,
}
async fn modal(
    ctx: &s::Context,
    data: &Data,
    b: &s::ComponentInteraction,
    rename: bool,
) -> Result<(), Error> {
    use poise::Modal;
    let room = {
        let _guard = data.configuration_lock.lock().await;
        current(ctx, data, b.user.id, true).await?
    };
    let room_id = room.channel.id;
    let id = format!("voice-form-{}", b.id);
    b.create_response(
        &ctx.http,
        if rename {
            Rename::create(None, id.clone())
        } else {
            Capacity::create(None, id.clone())
        },
    )
    .await?;
    let user = b.user.id;
    let guild = data.guild;
    if let Some(m) = s::collector::ModalInteractionCollector::new(&ctx.shard)
        .filter(move |m| m.data.custom_id == id && m.user.id == user && m.guild_id == Some(guild))
        .timeout(Duration::from_secs(120))
        .await
    {
        m.create_response(
            &ctx.http,
            s::CreateInteractionResponse::Defer(
                s::CreateInteractionResponseMessage::new().ephemeral(true),
            ),
        )
        .await?;
        let result: Result<(), Error> = async {
            let _guard = data.configuration_lock.lock().await;
            let room = current(ctx, data, user, true).await?;
            if room.channel.id != room_id {
                return Err(crate::errors::user(
                    "You changed rooms. Open the control again.",
                ));
            }
            if rename {
                if !quota(&data.db, data.guild, user, "rename", now(), 600, 2).await? { return Err(crate::errors::user("Rename limit reached. Wait for the next ten-minute window before renaming again.")); }
                let value = Rename::parse(m.data.clone()).map_err(|e| -> Error { e.into() })?;
                room_id
                    .edit(
                        &ctx.http,
                        s::EditChannel::new().name(clean_name(&value.value)?),
                    )
                    .await?;
            } else {
                let value = Capacity::parse(m.data.clone()).map_err(|e| -> Error { e.into() })?;
                room_id
                    .edit(
                        &ctx.http,
                        s::EditChannel::new().user_limit(limit(&value.value)?),
                    )
                    .await?;
            }
            Ok(())
        }
        .await;
        let text = match result {
            Ok(()) => {
                "Room updated. Existing members are not disconnected when lowering the limit."
                    .into()
            }
            Err(e) => crate::errors::with_reference(e.as_ref(), "join-to-create", m.id.get()),
        };
        m.edit_response(&ctx.http, private_reply("ROOM UPDATED", text))
            .await?;
    }
    Ok(())
}
async fn invite(
    ctx: &s::Context,
    data: &Data,
    room: &Room,
    owner: &s::User,
    target: s::UserId,
) -> Result<String, Error> {
    if target == owner.id {
        return Err(crate::errors::user("Choose another server member."));
    }
    let member = eligible(ctx, data.guild, room.state.parent, target).await?;
    if present(ctx, data.guild, target) == Some(room.channel.id) {
        return Err(crate::errors::user("That member is already in your room."));
    }
    let duplicate:i64=sqlx::query_scalar("SELECT COUNT(*) FROM voice_room_invites WHERE guild=? AND channel=? AND user=? AND created>? AND state IN('pending','accepted','declined')").bind(data.guild.to_string()).bind(room.channel.id.to_string()).bind(target.to_string()).bind(now()-600).fetch_one(&data.db).await?;
    if duplicate > 0 {
        return Err(crate::errors::user(
            "This member was already invited recently. Wait ten minutes before inviting them again.",
        ));
    }
    if !quota(&data.db, data.guild, owner.id, "invite", now(), 600, 5).await? {
        return Err(crate::errors::user(
            "Invitation limit reached: five per ten-minute window. Try again later.",
        ));
    }
    let nonce = format!("{:032x}", rand::random::<u128>());
    sqlx::query("INSERT INTO voice_room_invites(nonce,guild,channel,owner,user,created,expires,state) VALUES(?,?,?,?,?,?,?,'pending')").bind(&nonce).bind(data.guild.to_string()).bind(room.channel.id.to_string()).bind(owner.id.to_string()).bind(target.to_string()).bind(now()).bind(now()+600).execute(&data.db).await?;
    let message=s::CreateMessage::new().embed(emoji::embed_for("mail").title("ELVNX / YOU'RE INVITED").description(format!("**{}** invited you to **{}**.\n\nAccept to receive access to this temporary voice room, then choose Join room. You won't be moved automatically. This invitation expires <t:{}:R> and is intended only for your account.\n\nServer membership, verification, room capacity and moderation permissions still apply.",owner.name,room.channel.name,now()+600))).components(vec![s::CreateActionRow::Buttons(vec![emoji::button_for(format!("elvnx:voice:accept:{nonce}"),"check").label("Accept invitation").style(s::ButtonStyle::Success),emoji::button_for(format!("elvnx:voice:decline:{nonce}"),"cross").label("Decline").style(s::ButtonStyle::Secondary)])]).allowed_mentions(s::CreateAllowedMentions::new());
    if let Err(error) = member.user.direct_message(&ctx.http, message).await {
        let _ = sqlx::query("UPDATE voice_room_invites SET state='failed' WHERE nonce=?")
            .bind(&nonce)
            .execute(&data.db)
            .await;
        tracing::warn!(%error,"Voice invitation DM unavailable");
        return Err(crate::errors::user(
            "The invitation DM could not be delivered. The member may have closed DMs. No room access was granted.",
        ));
    }
    Ok(format!(
        "Invitation DM delivered to <@{target}>. It expires in ten minutes; access is granted only after they accept."
    ))
}
async fn pending_invite(
    db: &SqlitePool,
    guild: s::GuildId,
    user: s::UserId,
    nonce: &str,
    at: i64,
) -> Result<Option<sqlx::sqlite::SqliteRow>, Error> {
    Ok(sqlx::query("SELECT * FROM voice_room_invites WHERE nonce=? AND guild=? AND user=? AND state='pending' AND expires>?").bind(nonce).bind(guild.to_string()).bind(user.to_string()).bind(at).fetch_optional(db).await?)
}
async fn respond_invite(
    ctx: &s::Context,
    data: &Data,
    b: &s::ComponentInteraction,
    nonce: &str,
    accept: bool,
) -> Result<String, Error> {
    let row = pending_invite(&data.db, data.guild, b.user.id, nonce, now())
        .await?
        .ok_or_else(|| {
            crate::errors::user(
                "This invitation expired, was already used, or belongs to another member.",
            )
        })?;
    if !accept {
        sqlx::query("UPDATE voice_room_invites SET state='declined' WHERE nonce=? AND user=? AND state='pending'").bind(nonce).bind(b.user.id.to_string()).execute(&data.db).await?;
        return Ok("Invitation declined. No room access was granted.".into());
    }
    let channel = s::ChannelId::new(row.get::<String, _>("channel").parse()?);
    let owner = s::UserId::new(row.get::<String, _>("owner").parse()?);
    let mut room = load(ctx, &data.db, data.guild, channel).await?;
    if room.owner != owner || present(ctx, data.guild, owner) != Some(channel) {
        return Err(crate::errors::user(
            "The room owner left or ownership changed. Ask the current host for a new invitation.",
        ));
    }
    eligible(ctx, data.guild, room.state.parent, b.user.id).await?;
    if room.channel.user_limit.is_some_and(|limit| {
        limit > 0 && occupied(ctx, data.guild, channel).unwrap_or(usize::MAX) >= limit as usize
    }) {
        return Err(crate::errors::user(
            "The room is full. Ask the host to increase its member limit, then try again before this invitation expires.",
        ));
    }
    let changed=sqlx::query("UPDATE voice_room_invites SET state='granting' WHERE nonce=? AND user=? AND state='pending' AND expires>?").bind(nonce).bind(b.user.id.to_string()).bind(now()).execute(&data.db).await?;
    if changed.rows_affected() != 1 {
        return Err(crate::errors::user("This invitation was already handled."));
    }
    sqlx::query("INSERT INTO voice_room_guests(guild,channel,user,blocked) VALUES(?,?,?,0) ON CONFLICT(guild,channel,user) DO UPDATE SET blocked=0").bind(data.guild.to_string()).bind(channel.to_string()).bind(b.user.id.to_string()).execute(&data.db).await?;
    if let Err(error) = apply(ctx, data, &mut room).await {
        let _ = sqlx::query(
            "UPDATE voice_room_invites SET state='failed' WHERE nonce=? AND state='granting'",
        )
        .bind(nonce)
        .execute(&data.db)
        .await;
        sqlx::query(
            "UPDATE voice_room_guests SET blocked=1 WHERE guild=? AND channel=? AND user=?",
        )
        .bind(data.guild.to_string())
        .bind(channel.to_string())
        .bind(b.user.id.to_string())
        .execute(&data.db)
        .await?;
        // A timed-out Discord request may have succeeded. Reconcile denial immediately and on later controls.
        if let Err(cleanup) = apply(ctx, data, &mut room).await {
            tracing::warn!(%cleanup,"Voice invite access rollback needs reconciliation");
        }
        return Err(error);
    }
    sqlx::query(
        "UPDATE voice_room_invites SET state='accepted' WHERE nonce=? AND state='granting'",
    )
    .bind(nonce)
    .execute(&data.db)
    .await?;
    Ok(format!(
        "Invitation accepted. Your access to **{}** is ready.\n\n[Join room](https://discord.com/channels/{}/{channel})\n\nYou can leave whenever you want. Access lasts until the host removes it or the temporary room closes.",
        room.channel.name, data.guild
    ))
}
async fn target_action(
    ctx: &s::Context,
    data: &Data,
    room: &mut Room,
    actor: s::UserId,
    target: s::UserId,
    action: &str,
) -> Result<String, Error> {
    if action == "invite" {
        let owner = actor.to_user(&ctx.http).await?;
        return invite(ctx, data, room, &owner, target).await;
    }
    if target == actor || target == bot(ctx) {
        return Err(crate::errors::user("Choose another human room member."));
    }
    let member = data.guild.member(&ctx.http, target).await?;
    if action == "transfer" {
        eligible(ctx, data.guild, room.state.parent, target).await?;
        if present(ctx, data.guild, target) != Some(room.channel.id) {
            return Err(crate::errors::user(
                "The new owner must currently be in this room.",
            ));
        }
        let owns: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM voice_rooms WHERE guild=? AND owner=?)",
        )
        .bind(data.guild.to_string())
        .bind(target.to_string())
        .fetch_one(&data.db)
        .await?;
        if owns {
            return Err(crate::errors::user(
                "That member already owns a temporary room.",
            ));
        }
        sqlx::query("UPDATE voice_rooms SET owner=? WHERE guild=? AND channel=? AND owner=?")
            .bind(target.to_string())
            .bind(data.guild.to_string())
            .bind(room.channel.id.to_string())
            .bind(actor.to_string())
            .execute(&data.db)
            .await?;
        let old = room.owner;
        sqlx::query("DELETE FROM voice_room_guests WHERE guild=? AND channel=? AND user=?")
            .bind(data.guild.to_string())
            .bind(room.channel.id.to_string())
            .bind(target.to_string())
            .execute(&data.db)
            .await?;
        room.owner = target;
        room.state.owner_left = None;
        sqlx::query(
            "INSERT OR REPLACE INTO voice_room_guests(guild,channel,user,blocked) VALUES(?,?,?,0)",
        )
        .bind(data.guild.to_string())
        .bind(room.channel.id.to_string())
        .bind(old.to_string())
        .execute(&data.db)
        .await?;
        if let Err(error) = apply(ctx, data, room).await {
            tracing::warn!(%error,"Voice ownership transferred but permission refresh failed");
            return Ok(format!(
                "Ownership transferred to <@{target}>. Discord could not refresh room access; the new owner should retry a room control or contact staff."
            ));
        }
        return Ok(format!(
            "Ownership transferred to <@{target}>. You remain a guest; only the new owner can change the room."
        ));
    }
    let guild = data.guild.to_partial_guild(&ctx.http).await?;
    if guild.owner_id == target || guild.member_permissions(&member).intersects(staff()) {
        return Err(crate::errors::user(
            "Room controls cannot remove server staff.",
        ));
    }
    if action == "revoke" {
        sqlx::query(
            "INSERT OR REPLACE INTO voice_room_guests(guild,channel,user,blocked) VALUES(?,?,?,1)",
        )
        .bind(data.guild.to_string())
        .bind(room.channel.id.to_string())
        .bind(target.to_string())
        .execute(&data.db)
        .await?;
        sqlx::query("UPDATE voice_room_invites SET state='revoked' WHERE guild=? AND channel=? AND user=? AND state IN('pending','granting')").bind(data.guild.to_string()).bind(room.channel.id.to_string()).bind(target.to_string()).execute(&data.db).await?;
        apply(ctx, data, room).await?;
    } else if action != "disconnect" {
        return Err(crate::errors::user("Unknown member control."));
    }
    if present(ctx, data.guild, target) == Some(room.channel.id) {
        member.disconnect_from_voice(&ctx.http).await?;
    } else if action == "disconnect" {
        return Err(crate::errors::user(
            "That member is no longer in your room.",
        ));
    }
    Ok(if action == "revoke" {
        format!(
            "Removed <@{target}>'s access to this room and disconnected them if present. Their server membership is unchanged."
        )
    } else {
        format!(
            "Disconnected <@{target}> from this room. They can rejoin if its access settings allow it."
        )
    })
}
async fn action(
    ctx: &s::Context,
    data: &Data,
    b: &s::ComponentInteraction,
    name: &str,
) -> Result<Option<String>, Error> {
    let parts = name.split(':').collect::<Vec<_>>();
    let name = parts[0];
    let mut room = current(ctx, data, b.user.id, !matches!(name, "info" | "claim")).await?;
    if parts.len() > 1
        && (parts.len() != 3
            || parts[1] != room.channel.id.to_string()
            || parts[2] != b.user.id.to_string())
    {
        return Err(crate::errors::user(
            "This control belongs to a different member or room. Open a fresh control.",
        ));
    }
    if name.starts_with("select-") {
        let s::ComponentInteractionDataKind::UserSelect { values } = &b.data.kind else {
            return Err(crate::errors::user("Choose a server member."));
        };
        let target = *values
            .first()
            .ok_or_else(|| crate::errors::user("Choose one member."))?;
        return Ok(Some(
            target_action(
                ctx,
                data,
                &mut room,
                b.user.id,
                target,
                name.trim_start_matches("select-"),
            )
            .await?,
        ));
    }
    match name {
        "info" => {
            let count = occupied(ctx, data.guild, room.channel.id).ok_or_else(|| {
                crate::errors::user("Voice membership is still syncing; try shortly.")
            })?;
            let limit = room.channel.user_limit.unwrap_or(0);
            let png = cards::render(
                "VOICE / YOUR ROOM",
                &room.channel.name,
                "Your people. Your space.",
                &[
                    (
                        "MEMBERS",
                        format!(
                            "{count} / {}",
                            if limit == 0 {
                                "Unlimited".into()
                            } else {
                                limit.to_string()
                            }
                        ),
                    ),
                    (
                        "ACCESS",
                        if room.state.locked {
                            "Invite only".into()
                        } else {
                            "Category access".into()
                        },
                    ),
                    (
                        "AUDIO",
                        format!("{} kbps", room.channel.bitrate.unwrap_or(64000) / 1000),
                    ),
                ],
                None,
            )?;
            b.edit_response(&ctx.http,private_reply("ROOM DETAILS",format!("Owner <@{}> · <#{}>\nLocked: **{}** · Hidden: **{}** · Sharing: **{}**\nCreated <t:{}:R>\n\nLocks preserve approved guests. Hidden rooms remain visible to approved guests and administrators. Server moderation still applies.",room.owner,room.channel.id,room.state.locked,room.state.hidden,room.state.sharing,room.state.created)).attachments(s::EditAttachments::new().add(s::CreateAttachment::bytes(png,"room-details.png")))).await?;
            return Ok(None);
        }
        "invite" | "transfer" | "disconnect" | "revoke" => {
            b.edit_response(&ctx.http,private_reply("MEMBER CONTROL",match name {"invite"=>"Choose a server member to receive one expiring invitation DM. No access is granted until acceptance.","transfer"=>"Choose a member currently in your room. They become the owner; you remain a guest.","revoke"=>"Choose a member to remove from this room and block from rejoining it. Staff are protected.",_=>"Choose a member to disconnect from your room. Staff are protected; this does not remove server membership."}).components(vec![user_menu(name,&room,b.user.id)])).await?;
            return Ok(None);
        }
        "delete" => {
            b.edit_response(&ctx.http,private_reply("CLOSE ROOM?","This permanently deletes your temporary room and disconnects everyone inside. Choose Confirm close only if you want to end the room.").components(vec![s::CreateActionRow::Buttons(vec![emoji::button_for(scoped("confirm-delete",&room,b.user.id),"warning").label("Confirm close room").style(s::ButtonStyle::Danger)])])).await?;
            return Ok(None);
        }
        "confirm-delete" => {
            room.channel.id.delete(&ctx.http).await?;
            forget(&data.db, data.guild, room.channel.id).await?;
            return Ok(Some("Your temporary room has been closed.".into()));
        }
        "lock" => room.state.locked = true,
        "unlock" => room.state.locked = false,
        "hide" => room.state.hidden = true,
        "show" => room.state.hidden = false,
        "sharing" => room.state.sharing = !room.state.sharing,
        "claim" => {
            if present(ctx, data.guild, room.owner) == Some(room.channel.id) {
                return Err(crate::errors::user(
                    "The current owner is still in this room. Ask them to transfer ownership.",
                ));
            }
            if room.state.owner_left.is_none() {
                room.state.owner_left = Some(now());
                save(&data.db, room.channel.id, data.guild, &room.state).await?;
            }
            if room.state.owner_left.is_none_or(|time| time > now() - 120) {
                return Err(crate::errors::user(
                    "The owner must be absent for two minutes before you can claim this room.",
                ));
            }
            let owns: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM voice_rooms WHERE guild=? AND owner=?)",
            )
            .bind(data.guild.to_string())
            .bind(b.user.id.to_string())
            .fetch_one(&data.db)
            .await?;
            if owns {
                return Err(crate::errors::user("You already own a temporary room."));
            }
            sqlx::query("UPDATE voice_rooms SET owner=? WHERE guild=? AND channel=? AND owner=?")
                .bind(b.user.id.to_string())
                .bind(data.guild.to_string())
                .bind(room.channel.id.to_string())
                .bind(room.owner.to_string())
                .execute(&data.db)
                .await?;
            sqlx::query("DELETE FROM voice_room_guests WHERE guild=? AND channel=? AND user=?")
                .bind(data.guild.to_string())
                .bind(room.channel.id.to_string())
                .bind(b.user.id.to_string())
                .execute(&data.db)
                .await?;
            room.owner = b.user.id;
            room.state.owner_left = None;
        }
        "settings" => {
            let guild = data.guild.to_partial_guild(&ctx.http).await?;
            let max = match guild.premium_tier {
                s::PremiumTier::Tier1 => 128000,
                s::PremiumTier::Tier2 => 256000,
                s::PremiumTier::Tier3 => 384000,
                _ => 96000,
            };
            let options = [64000, 96000, 128000, 256000, 384000]
                .into_iter()
                .filter(|n| *n <= max)
                .map(|n| {
                    s::CreateSelectMenuOption::new(format!("{} kbps", n / 1000), n.to_string())
                })
                .collect();
            b.edit_response(&ctx.http,private_reply("AUDIO QUALITY","Choose an available bitrate. Server boost limits are respected. Your microphone, connection and Discord settings also affect audio quality.").components(vec![s::CreateActionRow::SelectMenu(s::CreateSelectMenu::new(scoped("bitrate",&room,b.user.id),s::CreateSelectMenuKind::String{options}).placeholder("Choose voice bitrate"))])).await?;
            return Ok(None);
        }
        "bitrate" => {
            let s::ComponentInteractionDataKind::StringSelect { values } = &b.data.kind else {
                return Err(crate::errors::user("Choose an audio bitrate."));
            };
            let rate = values
                .first()
                .and_then(|v| v.parse::<u32>().ok())
                .filter(|n| [64000, 96000, 128000, 256000, 384000].contains(n))
                .ok_or_else(|| crate::errors::user("Invalid bitrate selection."))?;
            let g = data.guild.to_partial_guild(&ctx.http).await?;
            let max = match g.premium_tier {
                s::PremiumTier::Tier1 => 128000,
                s::PremiumTier::Tier2 => 256000,
                s::PremiumTier::Tier3 => 384000,
                _ => 96000,
            };
            if rate > max {
                return Err(crate::errors::user(
                    "The server no longer supports this bitrate. Open Audio settings again.",
                ));
            }
            room.channel
                .id
                .edit(&ctx.http, s::EditChannel::new().bitrate(rate))
                .await?;
            return Ok(Some(format!("Voice bitrate set to {} kbps.", rate / 1000)));
        }
        _ => {
            return Err(crate::errors::user(
                "Unknown voice control. Use the latest lounge panel.",
            ));
        }
    }
    apply(ctx, data, &mut room).await?;
    Ok(Some(format!(
        "Room updated.\nLocked: **{}** · Hidden: **{}** · Screen sharing: **{}**",
        room.state.locked, room.state.hidden, room.state.sharing
    )))
}
pub async fn gateway(ctx: &s::Context, event: &s::FullEvent, data: &Data) -> Result<bool, Error> {
    if matches!(event, s::FullEvent::Ready { .. })
        && let Err(error) = refresh(ctx, &data.db, data.guild, true).await
    {
        tracing::warn!(%error,"Voice lounge panel refresh failed");
    }
    if let s::FullEvent::ChannelDelete { channel, .. } = event
        && channel.guild_id == data.guild
    {
        let _guard = data.configuration_lock.lock().await;
        forget(&data.db, data.guild, channel.id).await?;
    }
    if let s::FullEvent::GuildMemberRemoval { guild_id, user, .. } = event
        && *guild_id == data.guild
    {
        let _guard = data.configuration_lock.lock().await;
        sqlx::query("UPDATE voice_room_invites SET state='revoked' WHERE guild=? AND (user=? OR owner=?) AND state IN('pending','granting')").bind(data.guild.to_string()).bind(user.id.to_string()).bind(user.id.to_string()).execute(&data.db).await?;
        let rows = sqlx::query("SELECT channel FROM voice_rooms WHERE guild=?")
            .bind(data.guild.to_string())
            .fetch_all(&data.db)
            .await?;
        for row in rows {
            let channel = s::ChannelId::new(row.get::<String, _>("channel").parse()?);
            sqlx::query("INSERT OR REPLACE INTO voice_room_guests(guild,channel,user,blocked) VALUES(?,?,?,1)").bind(data.guild.to_string()).bind(channel.to_string()).bind(user.id.to_string()).execute(&data.db).await?;
            if let Ok(mut room) = load(ctx, &data.db, data.guild, channel).await {
                // Departed owners retain claim history but cannot keep a member-specific access grant.
                room.channel
                    .permission_overwrites
                    .retain(|p| p.kind != s::PermissionOverwriteType::Member(user.id));
                room.state
                    .base
                    .retain(|p| p.kind != s::PermissionOverwriteType::Member(user.id));
                let mut permissions = room.channel.permission_overwrites.clone();
                set_member(&mut permissions, user.id, s::Permissions::empty(), gate());
                if let Err(error) = channel
                    .edit(&ctx.http, s::EditChannel::new().permissions(permissions))
                    .await
                {
                    tracing::warn!(%error,"Departed voice member access cleanup failed");
                }
                save(&data.db, channel, data.guild, &room.state).await?;
            }
        }
    }
    if let s::FullEvent::VoiceStateUpdate { old, new } = event
        && new.guild_id == Some(data.guild)
    {
        let cfg = data.settings.read().await.clone();
        if let Some(hub) = cfg.join_to_create
            && new.channel_id.map(|id| id.get()) == Some(hub)
            && old.as_ref().and_then(|v| v.channel_id) != new.channel_id
            && let Err(error) = create(ctx, data, new.user_id, s::ChannelId::new(hub)).await
        {
            tracing::warn!(%error,member=%new.user_id,"Voice room creation failed");
            if let Ok(user) = new.user_id.to_user(&ctx.http).await
                && !user.bot
            {
                let _ = user
                    .direct_message(
                        &ctx.http,
                        s::CreateMessage::new()
                            .embed(
                                emoji::embed_for("warning")
                                    .title("ELVNX / VOICE ROOM")
                                    .description(crate::errors::message(
                                        error.as_ref(),
                                        "join-to-create",
                                    )),
                            )
                            .allowed_mentions(s::CreateAllowedMentions::new()),
                    )
                    .await;
            }
        }
        if let Some(channel) = new.channel_id
            && old.as_ref().and_then(|v| v.channel_id) != Some(channel)
        {
            let tracked: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM voice_rooms WHERE guild=? AND channel=?)",
            )
            .bind(data.guild.to_string())
            .bind(channel.to_string())
            .fetch_one(&data.db)
            .await?;
            if tracked
                && let Ok(member) = data.guild.member(&ctx.http, new.user_id).await
                && !member.user.bot
            {
                let blocked: Option<bool> = sqlx::query_scalar(
                    "SELECT blocked FROM voice_room_guests WHERE guild=? AND channel=? AND user=?",
                )
                .bind(data.guild.to_string())
                .bind(channel.to_string())
                .bind(new.user_id.to_string())
                .fetch_optional(&data.db)
                .await?;
                let room = load(ctx, &data.db, data.guild, channel).await?;
                let denied = blocked == Some(true)
                    || eligible(ctx, data.guild, room.state.parent, new.user_id)
                        .await
                        .is_err();
                let guild = data.guild.to_partial_guild(&ctx.http).await?;
                if denied
                    && guild.owner_id != new.user_id
                    && !guild.member_permissions(&member).intersects(staff())
                {
                    member.disconnect_from_voice(&ctx.http).await?;
                }
            }
        }
        let _guard = data.configuration_lock.lock().await;
        tick(ctx, &data.db, data.guild).await?;
    }
    let s::FullEvent::InteractionCreate {
        interaction: s::Interaction::Component(b),
    } = event
    else {
        return Ok(false);
    };
    let Some(name) = b.data.custom_id.strip_prefix("elvnx:voice:") else {
        return Ok(false);
    };
    if b.message.author.id != bot(ctx) {
        return Ok(false);
    }
    let invite_action = name
        .strip_prefix("accept:")
        .map(|n| (true, n))
        .or_else(|| name.strip_prefix("decline:").map(|n| (false, n)));
    if invite_action.is_none() && b.guild_id != Some(data.guild) {
        return Ok(false);
    }
    if name == "rename" || name == "limit" {
        let valid: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM voice_room_panels WHERE guild=? AND channel=? AND message=?",
        )
        .bind(data.guild.to_string())
        .bind(b.channel_id.to_string())
        .bind(b.message.id.to_string())
        .fetch_one(&data.db)
        .await?;
        if valid == 1 {
            if let Err(error) = modal(ctx, data, b, name == "rename").await {
                let _ = b
                    .create_response(
                        &ctx.http,
                        s::CreateInteractionResponse::Message(
                            s::CreateInteractionResponseMessage::new()
                                .ephemeral(true)
                                .embed(emoji::embed_for("warning").description(
                                    crate::errors::with_reference(
                                        error.as_ref(),
                                        "join-to-create",
                                        b.id.get(),
                                    ),
                                )),
                        ),
                    )
                    .await;
            }
            return Ok(true);
        }
    }
    b.create_response(
        &ctx.http,
        s::CreateInteractionResponse::Defer(
            s::CreateInteractionResponseMessage::new().ephemeral(true),
        ),
    )
    .await?;
    let result: Result<Option<String>, Error> = async {
        let _guard = data.configuration_lock.lock().await;
        if let Some((accept, nonce)) = invite_action {
            return Ok(Some(respond_invite(ctx, data, b, nonce, accept).await?));
        }
        if !quota(&data.db, data.guild, b.user.id, "controls", now(), 10, 12).await? {
            return Err(crate::errors::user(
                "Too many clicks. Wait a few seconds, then try again.",
            ));
        }
        if !name.contains(':') {
            let valid: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM voice_room_panels WHERE guild=? AND channel=? AND message=?",
            )
            .bind(data.guild.to_string())
            .bind(b.channel_id.to_string())
            .bind(b.message.id.to_string())
            .fetch_one(&data.db)
            .await?;
            if valid != 1 {
                return Err(crate::errors::user(
                    "This voice panel is outdated. Use the current lounge panel.",
                ));
            }
        }
        action(ctx, data, b, name).await
    }
    .await;
    match result {
        Ok(Some(text)) => {
            b.edit_response(
                &ctx.http,
                private_reply("VOICE LOUNGE", text).components(vec![]),
            )
            .await?;
        }
        Ok(None) => {}
        Err(error) => {
            b.edit_response(
                &ctx.http,
                private_reply(
                    "VOICE CONTROL",
                    crate::errors::with_reference(error.as_ref(), "join-to-create", b.id.get()),
                ),
            )
            .await?;
        }
    }
    Ok(true)
}
pub async fn tick(ctx: &s::Context, db: &SqlitePool, guild: s::GuildId) -> Result<(), Error> {
    if ctx.cache.guild(guild).is_none() {
        return Ok(());
    }
    let rows = sqlx::query("SELECT channel,owner FROM voice_rooms WHERE guild=?")
        .bind(guild.to_string())
        .fetch_all(db)
        .await?;
    for row in rows {
        let channel = s::ChannelId::new(row.get::<String, _>("channel").parse()?);
        if matches!(channel.to_channel(&ctx.http).await, Err(s::Error::Http(s::HttpError::UnsuccessfulRequest(ref r))) if r.status_code.as_u16() == 404)
        {
            forget(db, guild, channel).await?;
            continue;
        }
        match load(ctx, db, guild, channel).await {
            Ok(mut room) => {
                let owner_here = present(ctx, guild, room.owner) == Some(channel);
                if owner_here && room.state.owner_left.is_some() {
                    room.state.owner_left = None;
                    save(db, channel, guild, &room.state).await?;
                } else if !owner_here && room.state.owner_left.is_none() {
                    room.state.owner_left = Some(now());
                    save(db, channel, guild, &room.state).await?;
                }
                if now() - room.state.created >= 30
                    && occupied(ctx, guild, channel) == Some(0)
                    && channel.delete(&ctx.http).await.is_ok()
                {
                    forget(db, guild, channel).await?;
                }
            }
            Err(error) => {
                tracing::warn!(%channel,%error,"Temporary voice room reconciliation failed")
            }
        }
    }
    sqlx::query(
        "UPDATE voice_room_invites SET state='expired' WHERE state='pending' AND expires<?",
    )
    .bind(now())
    .execute(db)
    .await?;
    sqlx::query("DELETE FROM voice_room_invites WHERE created<?")
        .bind(now() - 86400)
        .execute(db)
        .await?;
    sqlx::query("DELETE FROM voice_room_limits WHERE (kind IN('invite','rename') AND window<?) OR (kind='controls' AND window<?) OR (kind='create' AND window<?)").bind((now()-86400)/600).bind((now()-86400)/10).bind((now()-86400)/300).execute(db).await?;
    Ok(())
}

pub fn start(
    ctx: s::Context,
    db: SqlitePool,
    guild: s::GuildId,
    lock: std::sync::Arc<tokio::sync::Mutex<()>>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let _guard = lock.lock().await;
            if let Err(error) = tick(&ctx, &db, guild).await {
                tracing::warn!(%error,"Voice room cleanup failed");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn overwrite(g: u64, allow: s::Permissions, deny: s::Permissions) -> s::PermissionOverwrite {
        s::PermissionOverwrite {
            kind: s::PermissionOverwriteType::Role(s::RoleId::new(g)),
            allow,
            deny,
        }
    }
    #[test]
    fn category_voice_template_does_not_copy_the_silent_hub() {
        let mut hub:s::GuildChannel=serde_json::from_value(serde_json::json!({"id":"10","guild_id":"1","type":2,"name":"hub","position":0,"permission_overwrites":[]})).unwrap();
        hub.permission_overwrites = vec![overwrite(
            1,
            gate(),
            s::Permissions::SPEAK | s::Permissions::USE_VAD | s::Permissions::STREAM,
        )];
        let mut parent = hub.clone();
        parent.kind = s::ChannelType::Category;
        parent.permission_overwrites = vec![
            overwrite(
                1,
                s::Permissions::SPEAK | s::Permissions::USE_VAD,
                s::Permissions::VIEW_CHANNEL,
            ),
            overwrite(2, gate() | s::Permissions::SPEAK, s::Permissions::empty()),
        ];
        let result = template(Some(&parent), &hub);
        assert_eq!(result, parent.permission_overwrites);
        assert!(result[0].deny.contains(s::Permissions::VIEW_CHANNEL));
        assert!(!result[0].deny.contains(s::Permissions::SPEAK));
        assert_eq!(
            hub.permission_overwrites[0].deny,
            s::Permissions::SPEAK | s::Permissions::USE_VAD | s::Permissions::STREAM
        );
    }
    #[test]
    fn names_limits_and_panel_fit_discord_bounds() {
        assert!(clean_name("  Friends room  ").is_ok());
        assert!(clean_name("\n").is_err());
        assert!(clean_name(&"x".repeat(81)).is_err());
        assert_eq!(limit("0").unwrap(), 0);
        assert_eq!(limit("99").unwrap(), 99);
        assert!(limit("100").is_err());
        assert!(limit("-1").is_err());
        let rows = controls();
        assert_eq!(rows.len(), 3);
        let value = serde_json::to_value(rows).unwrap();
        assert!(
            value
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["components"].as_array().unwrap().len() <= 5)
        );
    }

    #[test]
    fn locks_override_role_allows_without_removing_mutes_and_unlock_restores_access() {
        let channel:s::GuildChannel=serde_json::from_value(serde_json::json!({"id":"10","guild_id":"1","type":2,"name":"room","position":0,"permission_overwrites":[]})).unwrap();
        let guild:s::PartialGuild=serde_json::from_value(serde_json::json!({"id":"1","name":"test","owner_id":"99","roles":[],"emojis":[],"features":[],"verification_level":0,"default_message_notifications":0,"explicit_content_filter":0,"mfa_level":0,"premium_tier":0,"nsfw_level":0,"system_channel_flags":0,"preferred_locale":"en-US","stickers":[],"premium_progress_bar_enabled":false})).unwrap();
        let base = vec![
            overwrite(
                1,
                s::Permissions::SPEAK | s::Permissions::CONNECT,
                s::Permissions::VIEW_CHANNEL,
            ),
            overwrite(2, gate() | s::Permissions::STREAM, s::Permissions::empty()),
        ];
        let mut room = Room {
            channel,
            owner: s::UserId::new(5),
            state: State {
                base: base.clone(),
                parent: None,
                created: 0,
                locked: true,
                hidden: true,
                sharing: false,
                owner_left: None,
            },
        };
        room.channel.permission_overwrites = base;
        set_member(
            &mut room.channel.permission_overwrites,
            s::UserId::new(6),
            s::Permissions::empty(),
            s::Permissions::SPEAK,
        );
        let mut out = compose(
            &room,
            &guild,
            s::UserId::new(9),
            &[(s::UserId::new(6), false), (s::UserId::new(7), true)],
        );
        assert!(
            out.iter()
                .find(|p| p.kind == s::PermissionOverwriteType::Role(s::RoleId::new(2)))
                .unwrap()
                .deny
                .contains(gate())
        );
        let guest = out
            .iter()
            .find(|p| p.kind == s::PermissionOverwriteType::Member(s::UserId::new(6)))
            .unwrap();
        assert!(guest.allow.contains(gate()));
        assert!(guest.deny.contains(s::Permissions::SPEAK));
        assert!(
            out.iter()
                .find(|p| p.kind == s::PermissionOverwriteType::Member(room.owner))
                .unwrap()
                .deny
                .contains(s::Permissions::STREAM)
        );
        room.channel.permission_overwrites = out.clone();
        room.state.locked = false;
        room.state.hidden = false;
        room.state.sharing = true;
        out = compose(
            &room,
            &guild,
            s::UserId::new(9),
            &[(s::UserId::new(7), true)],
        );
        assert!(
            out.iter()
                .find(|p| p.kind == s::PermissionOverwriteType::Role(s::RoleId::new(2)))
                .unwrap()
                .allow
                .contains(gate() | s::Permissions::STREAM)
        );
        assert!(
            out.iter()
                .find(|p| p.kind == s::PermissionOverwriteType::Member(s::UserId::new(7)))
                .unwrap()
                .deny
                .contains(gate())
        );
    }
    #[tokio::test]
    async fn invitations_are_bound_to_recipient_guild_expiry_and_state() {
        let db = store::connect("sqlite::memory:").await.unwrap();
        sqlx::query("INSERT INTO voice_room_invites(nonce,guild,channel,owner,user,created,expires,state) VALUES('secret','1','10','5','6',100,700,'pending')").execute(&db).await.unwrap();
        let g = s::GuildId::new(1);
        let user = s::UserId::new(6);
        assert!(
            pending_invite(&db, g, user, "secret", 699)
                .await
                .unwrap()
                .is_some()
        );
        for (guild, recipient, nonce, at) in [
            (2, 6, "secret", 101),
            (1, 7, "secret", 101),
            (1, 6, "wrong", 101),
            (1, 6, "secret", 700),
        ] {
            assert!(
                pending_invite(
                    &db,
                    s::GuildId::new(guild),
                    s::UserId::new(recipient),
                    nonce,
                    at
                )
                .await
                .unwrap()
                .is_none()
            );
        }
        for state in ["granting", "accepted", "declined", "revoked", "failed"] {
            sqlx::query("UPDATE voice_room_invites SET state=?")
                .bind(state)
                .execute(&db)
                .await
                .unwrap();
            assert!(
                pending_invite(&db, g, user, "secret", 200)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }
    #[tokio::test]
    async fn invite_rate_limit_is_persistent_and_scoped() {
        let db = store::connect("sqlite::memory:").await.unwrap();
        let g = s::GuildId::new(1);
        let u = s::UserId::new(2);
        for _ in 0..5 {
            assert!(quota(&db, g, u, "invite", 100, 600, 5).await.unwrap());
        }
        assert!(!quota(&db, g, u, "invite", 101, 600, 5).await.unwrap());
        assert!(
            quota(&db, g, s::UserId::new(3), "invite", 101, 600, 5)
                .await
                .unwrap()
        );
        assert!(quota(&db, g, u, "invite", 601, 600, 5).await.unwrap());
    }
}
