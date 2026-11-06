use crate::{Data, Error, emoji, serenity as s};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;

const PREFIX: &str = "elvnx:personal:";
fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
fn fail(text: &str) -> Error {
    crate::errors::user(text)
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    title: String,
    locked: bool,
    hidden: bool,
    status: String,
    topic_base: String,
    last_topic: String,
    members: BTreeMap<u64, bool>,
    pending: bool,
    pending_kind: String,
    baseline: Vec<s::PermissionOverwrite>,
}
struct Room {
    channel: s::GuildChannel,
    owner: s::UserId,
    role: s::RoleId,
    panel: Option<s::MessageId>,
    state: State,
}
async fn load(ctx: &s::Context, data: &Data, channel: s::ChannelId) -> Result<Room, Error> {
    let row = sqlx::query("SELECT * FROM personal_channels WHERE guild=? AND channel=?")
        .bind(data.guild.to_string())
        .bind(channel.to_string())
        .fetch_optional(&data.db)
        .await?
        .ok_or_else(|| fail("This is not a configured Personal channel."))?;
    let fresh = channel
        .to_channel(&ctx.http)
        .await?
        .guild()
        .ok_or_else(|| fail("This control needs a server text channel."))?;
    let parent = s::ChannelId::new(row.get::<String, _>("parent").parse()?);
    if fresh.guild_id != data.guild
        || fresh.kind != s::ChannelType::Text
        || fresh.parent_id != Some(parent)
    {
        return Err(fail(
            "This room moved outside its configured Personal category. Ask staff to review its controls.",
        ));
    }
    let mut state: State = serde_json::from_str(row.get::<&str, _>("value"))?;
    if !state.hidden && !state.locked && !state.pending {
        state.baseline = fresh.permission_overwrites.clone();
    }
    Ok(Room {
        channel: fresh,
        owner: s::UserId::new(row.get::<String, _>("owner").parse()?),
        role: s::RoleId::new(row.get::<String, _>("role").parse()?),
        panel: row
            .get::<Option<String>, _>("panel")
            .map(|x| x.parse::<u64>().map(s::MessageId::new))
            .transpose()?,
        state,
    })
}
fn authorized(actor: u64, owner: u64, staff: bool) -> bool {
    actor == owner || staff
}
async fn access(ctx: &s::Context, data: &Data, room: &Room, actor: s::UserId) -> Result<(), Error> {
    let guild = data.guild.to_partial_guild(&ctx.http).await?;
    let member = data.guild.member(&ctx.http, actor).await?;
    if member.user.bot
        || !authorized(
            actor.get(),
            room.owner.get(),
            actor == guild.owner_id
                || guild
                    .member_permissions(&member)
                    .contains(s::Permissions::MANAGE_GUILD),
        )
    {
        return Err(fail(
            "Only this channel's owner or staff with Manage Server can use these controls.",
        ));
    }
    let bot_id = ctx.cache.current_user().id;
    let bot = data.guild.member(&ctx.http, bot_id).await?;
    if !guild.user_permissions_in(&room.channel, &bot).contains(
        s::Permissions::MANAGE_CHANNELS
            | s::Permissions::MANAGE_ROLES
            | s::Permissions::VIEW_CHANNEL
            | s::Permissions::SEND_MESSAGES
            | s::Permissions::EMBED_LINKS,
    ) {
        return Err(fail(
            "The bot needs Manage Channels, Manage Roles and posting access here. Ask an administrator to restore its permissions.",
        ));
    }
    Ok(())
}
fn view() -> s::Permissions {
    s::Permissions::VIEW_CHANNEL
}
fn write() -> s::Permissions {
    s::Permissions::SEND_MESSAGES
        | s::Permissions::SEND_MESSAGES_IN_THREADS
        | s::Permissions::CREATE_PUBLIC_THREADS
        | s::Permissions::CREATE_PRIVATE_THREADS
}
fn normal() -> s::Permissions {
    view()
        | write()
        | s::Permissions::READ_MESSAGE_HISTORY
        | s::Permissions::ATTACH_FILES
        | s::Permissions::EMBED_LINKS
        | s::Permissions::ADD_REACTIONS
        | s::Permissions::USE_APPLICATION_COMMANDS
}
fn set(
    out: &mut Vec<s::PermissionOverwrite>,
    kind: s::PermissionOverwriteType,
    mask: s::Permissions,
    allow: s::Permissions,
    deny: s::Permissions,
) {
    if let Some(p) = out.iter_mut().find(|p| p.kind == kind) {
        p.allow = (p.allow & !mask) | allow;
        p.deny = (p.deny & !mask) | deny;
    } else {
        out.push(s::PermissionOverwrite { kind, allow, deny });
    }
}
fn permissions(
    current: &[s::PermissionOverwrite],
    guild: s::GuildId,
    owner: s::UserId,
    role: s::RoleId,
    bot: s::UserId,
    state: &State,
) -> Vec<s::PermissionOverwrite> {
    let mut out = current.to_vec();
    let gate = view() | write();
    for p in &mut out {
        if matches!(p.kind, s::PermissionOverwriteType::Member(user) if user==owner || user==bot) {
            continue;
        }
        let base = state.baseline.iter().find(|base| base.kind == p.kind);
        let original_allow = base.map_or(s::Permissions::empty(), |base| base.allow & gate);
        let original_deny = base.map_or(s::Permissions::empty(), |base| base.deny & gate);
        p.allow = (p.allow & !gate) | original_allow;
        p.deny = (p.deny & !gate) | original_deny;
        let denied = (if state.hidden {
            view()
        } else {
            s::Permissions::empty()
        }) | (if state.locked {
            write()
        } else {
            s::Permissions::empty()
        });
        p.allow &= !denied;
        p.deny |= denied;
    }
    set(
        &mut out,
        s::PermissionOverwriteType::Role(s::RoleId::new(guild.get())),
        view(),
        s::Permissions::empty(),
        view(),
    );
    let allow = (if state.hidden {
        s::Permissions::empty()
    } else {
        view()
    }) | (if state.locked {
        s::Permissions::empty()
    } else {
        state
            .baseline
            .iter()
            .find(|p| p.kind == s::PermissionOverwriteType::Role(role))
            .map_or(write(), |p| p.allow & write())
    });
    let deny = (if state.hidden {
        view()
    } else {
        s::Permissions::empty()
    }) | (if state.locked {
        write()
    } else {
        state
            .baseline
            .iter()
            .find(|p| p.kind == s::PermissionOverwriteType::Role(role))
            .map_or(s::Permissions::empty(), |p| p.deny & write())
    });
    set(
        &mut out,
        s::PermissionOverwriteType::Role(role),
        gate,
        allow,
        deny,
    );
    for (user, allowed) in &state.members {
        if *user == owner.get() || *user == bot.get() {
            continue;
        }
        let allow = if *allowed {
            normal() & !deny
        } else {
            s::Permissions::empty()
        };
        let denied = if *allowed { deny } else { gate };
        set(
            &mut out,
            s::PermissionOverwriteType::Member(s::UserId::new(*user)),
            normal(),
            allow,
            denied,
        );
    }
    for user in [owner, bot] {
        set(
            &mut out,
            s::PermissionOverwriteType::Member(user),
            normal(),
            normal(),
            s::Permissions::empty(),
        );
    }
    out
}
fn clean_status(text: &str) -> Result<String, Error> {
    let value = text.trim();
    if value.is_empty()
        || value.encode_utf16().count() > 100
        || value.chars().any(char::is_control)
        || value.contains("<@")
        || value.contains("@everyone")
        || value.contains("@here")
    {
        return Err(fail(
            "Use 1–100 characters on one line, without mentions or control characters.",
        ));
    }
    Ok(value.into())
}
fn topic(state: &mut State, current: &str, clear: bool) -> Result<String, Error> {
    let pending_managed_topic =
        state.pending && current.starts_with(&format!("{}\nStatus: ", state.topic_base));
    if current != state.last_topic && !pending_managed_topic {
        state.topic_base = current.into();
    }
    let result = if clear {
        state.status.clear();
        state.topic_base.clone()
    } else {
        format!(
            "{}{}Status: {}",
            state.topic_base,
            if state.topic_base.is_empty() {
                ""
            } else {
                "\n"
            },
            state.status
        )
    };
    if result.encode_utf16().count() > 1024 {
        return Err(fail(
            "The existing topic is too long to append a status. Ask staff to shorten it first.",
        ));
    }
    state.last_topic = result.clone();
    Ok(result)
}
async fn save(db: &SqlitePool, channel: s::ChannelId, state: &State) -> Result<(), Error> {
    sqlx::query("UPDATE personal_channels SET value=? WHERE channel=?")
        .bind(serde_json::to_string(state)?)
        .bind(channel.to_string())
        .execute(db)
        .await?;
    Ok(())
}
fn controls() -> Vec<s::CreateActionRow> {
    [
        [
            ("lock", "Lock", "lock"),
            ("unlock", "Unlock", "unlock"),
            ("hide", "Hide", "offline"),
            ("unhide", "Unhide", "online"),
        ],
        [
            ("add", "Add member", "plus"),
            ("remove", "Remove member", "minus"),
            ("status", "Set status", "typing"),
            ("clear", "Clear status", "cross"),
        ],
    ]
    .into_iter()
    .map(|row| {
        s::CreateActionRow::Buttons(
            row.into_iter()
                .map(|(id, label, icon)| {
                    emoji::button_for(format!("{PREFIX}{id}"), icon)
                        .label(label)
                        .style(s::ButtonStyle::Secondary)
                })
                .collect(),
        )
    })
    .collect()
}
fn embed(room: &Room) -> s::CreateEmbed {
    emoji::embed_for("shield").title(format!("{} / PERSONAL CONTROLS",room.state.title)).description(format!("Owner: <@{}>\n\n**Lock** makes this room read-only for others. **Hide** keeps it visible only to its owner and administrators. Unhide restores the matching identity role and approved guests; it never makes the room public.\n\nMember controls use a private selector. Status appears in this text channel's topic. Administrators retain Discord's built-in access.{}",room.owner,if room.state.pending{"\n\n**An update is pending.** Retry the same control to reconcile Discord permissions."}else{""}))
    .field("Messages",if room.state.locked{"Locked"}else{"Unlocked"},true).field("Visibility",if room.state.hidden{"Owner only"}else{"Private audience"},true)
    .field("Approved guests",room.state.members.values().filter(|allowed|**allowed).count().to_string(),true)
    .field("Channel status",if room.state.status.is_empty(){"Not set"}else{&room.state.status},false)
}
async fn refresh_room(ctx: &s::Context, data: &Data, channel: s::ChannelId) -> Result<(), Error> {
    let room = load(ctx, data, channel).await?;
    if let Some(id) = room.panel {
        match channel
            .edit_message(
                &ctx.http,
                id,
                s::EditMessage::new()
                    .embed(embed(&room))
                    .components(controls())
                    .allowed_mentions(s::CreateAllowedMentions::new()),
            )
            .await
        {
            Ok(_) => return Ok(()),
            Err(s::Error::Http(ref e)) if e.status_code() == Some(s::StatusCode::NOT_FOUND) => {}
            Err(e) => return Err(e.into()),
        }
    }
    let m = channel
        .send_message(
            &ctx.http,
            s::CreateMessage::new()
                .embed(embed(&room))
                .components(controls())
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await?;
    sqlx::query("UPDATE personal_channels SET panel=? WHERE guild=? AND channel=?")
        .bind(m.id.to_string())
        .bind(data.guild.to_string())
        .bind(channel.to_string())
        .execute(&data.db)
        .await?;
    Ok(())
}
pub async fn refresh_all(ctx: &s::Context, data: &Data) -> Result<(), Error> {
    let _guard = data.configuration_lock.lock().await;
    let channels: Vec<String> =
        sqlx::query_scalar("SELECT channel FROM personal_channels WHERE guild=?")
            .bind(data.guild.to_string())
            .fetch_all(&data.db)
            .await?;
    for channel in channels {
        if let Err(e) = refresh_room(ctx, data, s::ChannelId::new(channel.parse()?)).await {
            tracing::warn!(%e,channel,"Personal panel refresh failed");
        }
    }
    Ok(())
}
async fn quota(db: &SqlitePool, channel: &str, user: &str, time: i64) -> Result<(), Error> {
    sqlx::query("DELETE FROM personal_limits WHERE at<?")
        .bind(time - 86400)
        .execute(db)
        .await?;
    let changed=sqlx::query("INSERT INTO personal_limits VALUES(?,?,?) ON CONFLICT(channel,actor) DO UPDATE SET at=excluded.at WHERE personal_limits.at<=excluded.at-2").bind(channel).bind(user).bind(time).execute(db).await?.rows_affected();
    if changed == 0 {
        return Err(fail("Wait two seconds between Personal controls."));
    }
    Ok(())
}
pub async fn cleanup(db: &SqlitePool) -> Result<(), Error> {
    sqlx::query("DELETE FROM personal_sessions WHERE expires<?")
        .bind(now())
        .execute(db)
        .await?;
    sqlx::query("DELETE FROM personal_limits WHERE at<?")
        .bind(now() - 86400)
        .execute(db)
        .await?;
    Ok(())
}
async fn session(
    db: &SqlitePool,
    guild: s::GuildId,
    room: &Room,
    actor: s::UserId,
    action: &str,
) -> Result<String, Error> {
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query("DELETE FROM personal_sessions WHERE expires<? OR (channel=? AND actor=?)")
        .bind(now())
        .bind(room.channel.id.to_string())
        .bind(actor.to_string())
        .execute(&mut *tx)
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM personal_sessions")
        .fetch_one(&mut *tx)
        .await?;
    if count >= 200 {
        return Err(fail("Personal forms are busy. Try again in two minutes."));
    }
    let nonce = format!("{:032x}", rand::random::<u128>());
    sqlx::query("INSERT INTO personal_sessions VALUES(?,?,?,?,?,?,?)")
        .bind(&nonce)
        .bind(guild.to_string())
        .bind(room.channel.id.to_string())
        .bind(actor.to_string())
        .bind(
            room.panel
                .ok_or_else(|| fail("This panel is unavailable. Ask staff to refresh it."))?
                .to_string(),
        )
        .bind(action)
        .bind(now() + 120)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(nonce)
}
async fn claim(
    db: &SqlitePool,
    guild: s::GuildId,
    channel: s::ChannelId,
    actor: s::UserId,
    nonce: &str,
    time: i64,
) -> Result<(String, String), Error> {
    let row=sqlx::query("DELETE FROM personal_sessions WHERE nonce=? AND guild=? AND channel=? AND actor=? AND expires>=? RETURNING action,panel").bind(nonce).bind(guild.to_string()).bind(channel.to_string()).bind(actor.to_string()).bind(time).fetch_optional(db).await?.ok_or_else(||fail("This form expired or belongs to someone else. Open a fresh control from your channel panel."))?;
    Ok((row.get("action"), row.get("panel")))
}
async fn apply(
    ctx: &s::Context,
    data: &Data,
    room: &mut Room,
    topic_change: Option<String>,
) -> Result<(), Error> {
    // Persist the authorized intent first. HTTP failures leave a visible pending state,
    // so a retry reapplies the same private policy instead of forgetting an access change.
    let kind = if topic_change.is_some() {
        "topic"
    } else {
        "permissions"
    };
    if room.state.pending && !room.state.pending_kind.is_empty() && room.state.pending_kind != kind
    {
        return Err(fail(
            "The previous update is still pending. Retry its control first; for a pending status, use Set status or Clear status.",
        ));
    }
    room.state.pending = true;
    room.state.pending_kind = kind.into();
    save(&data.db, room.channel.id, &room.state).await?;
    let edit = edit_payload(
        topic_change,
        permissions(
            &room.channel.permission_overwrites,
            data.guild,
            room.owner,
            room.role,
            ctx.cache.current_user().id,
            &room.state,
        ),
    );
    if let Err(error) = room.channel.id.edit(&ctx.http, edit).await {
        let _ = refresh_room(ctx, data, room.channel.id).await;
        return Err(error.into());
    }
    room.state.pending = false;
    room.state.pending_kind.clear();
    save(&data.db, room.channel.id, &room.state).await?;
    if let Err(e) = refresh_room(ctx, data, room.channel.id).await {
        tracing::warn!(%e,"Personal change applied but panel refresh failed");
    }
    Ok(())
}
fn edit_payload(
    topic: Option<String>,
    overwrites: Vec<s::PermissionOverwrite>,
) -> s::EditChannel<'static> {
    match topic {
        Some(text) => s::EditChannel::new().topic(text),
        None => s::EditChannel::new().permissions(overwrites),
    }
}
fn response(text: impl Into<String>) -> s::EditInteractionResponse {
    s::EditInteractionResponse::new()
        .embed(
            emoji::embed_for("shield")
                .title("ELVNX / PERSONAL SPACE")
                .description(text),
        )
        .components(vec![])
        .allowed_mentions(s::CreateAllowedMentions::new())
}
fn result_text(result: Result<String, Error>, id: u64) -> String {
    match result {
        Ok(t) => t,
        Err(e) => crate::errors::with_reference(e.as_ref(), "personal-channel", id),
    }
}
#[derive(poise::Modal)]
#[name = "ELVNX / Channel status"]
struct Status {
    #[name = "Status · shown in the channel topic"]
    #[min_length = 1]
    #[max_length = 100]
    value: String,
}
fn status_form(value: String, id: String) -> s::CreateInteractionResponse {
    use poise::Modal;
    let defaults = if value.is_empty() {
        None
    } else {
        Some(Status { value })
    };
    Status::create(defaults, id)
}

pub async fn gateway(ctx: &s::Context, event: &s::FullEvent, data: &Data) -> Result<bool, Error> {
    if let s::FullEvent::InteractionCreate {
        interaction: s::Interaction::Modal(m),
    } = event
    {
        let Some(nonce) = m.data.custom_id.strip_prefix("elvnx:personal:form:") else {
            return Ok(false);
        };
        m.create_response(
            &ctx.http,
            s::CreateInteractionResponse::Defer(
                s::CreateInteractionResponseMessage::new().ephemeral(true),
            ),
        )
        .await?;
        let result: Result<String, Error> = async {
            use poise::Modal;
            let _guard = data.configuration_lock.lock().await;
            if m.guild_id != Some(data.guild) {
                return Err(fail("Use this form in its original server."));
            }
            let (action, panel) =
                claim(&data.db, data.guild, m.channel_id, m.user.id, nonce, now()).await?;
            let mut room = load(ctx, data, m.channel_id).await?;
            if action != "status" || room.panel.map(|p| p.to_string()).as_deref() != Some(&panel) {
                return Err(fail(
                    "This status form is outdated. Open the current panel.",
                ));
            }
            access(ctx, data, &room, m.user.id).await?;
            room.state.status = clean_status(
                &Status::parse(m.data.clone())
                    .map_err(|e| -> Error { e.into() })?
                    .value,
            )?;
            let value = topic(
                &mut room.state,
                room.channel.topic.as_deref().unwrap_or(""),
                false,
            )?;
            apply(ctx, data, &mut room, Some(value)).await?;
            Ok("Channel status updated in the topic and control panel.".into())
        }
        .await;
        m.edit_response(&ctx.http, response(result_text(result, m.id.get())))
            .await?;
        return Ok(true);
    }
    let s::FullEvent::InteractionCreate {
        interaction: s::Interaction::Component(b),
    } = event
    else {
        return Ok(false);
    };
    let Some(action) = b.data.custom_id.strip_prefix(PREFIX) else {
        return Ok(false);
    };
    if action == "status" {
        let result: Result<(), Error> = async {
            let _guard = data.configuration_lock.lock().await;
            if b.guild_id != Some(data.guild) || b.message.author.id != ctx.cache.current_user().id
            {
                return Err(fail("This is not a valid Personal panel."));
            }
            let room = load(ctx, data, b.channel_id).await?;
            if room.panel != Some(b.message.id) {
                return Err(fail("Use the latest Personal panel in this channel."));
            }
            access(ctx, data, &room, b.user.id).await?;
            if !action.starts_with("pick:") {
                quota(
                    &data.db,
                    &b.channel_id.to_string(),
                    &b.user.id.to_string(),
                    now(),
                )
                .await?;
            }
            let nonce = session(&data.db, data.guild, &room, b.user.id, "status").await?;
            b.create_response(
                &ctx.http,
                status_form(room.state.status, format!("{PREFIX}form:{nonce}")),
            )
            .await?;
            Ok(())
        }
        .await;
        if let Err(e) = result {
            b.create_response(
                &ctx.http,
                s::CreateInteractionResponse::Message(
                    s::CreateInteractionResponseMessage::new()
                        .ephemeral(true)
                        .embed(emoji::embed_for("warning").description(
                            crate::errors::with_reference(
                                e.as_ref(),
                                "personal-channel",
                                b.id.get(),
                            ),
                        ))
                        .allowed_mentions(s::CreateAllowedMentions::new()),
                ),
            )
            .await?;
        }
        return Ok(true);
    }
    b.create_response(
        &ctx.http,
        s::CreateInteractionResponse::Defer(
            s::CreateInteractionResponseMessage::new().ephemeral(true),
        ),
    )
    .await?;
    let result:Result<Option<String>,Error>=async{
        let _guard=data.configuration_lock.lock().await;
        if b.guild_id!=Some(data.guild)||b.message.author.id!=ctx.cache.current_user().id{return Err(fail("This is not a valid Personal control."))}
        let mut room=load(ctx,data,b.channel_id).await?;
        access(ctx,data,&room,b.user.id).await?;
        if !action.starts_with("pick:") { quota(&data.db,&b.channel_id.to_string(),&b.user.id.to_string(),now()).await?; }
        if let Some(nonce)=action.strip_prefix("pick:"){
            let (kind,panel)=claim(&data.db,data.guild,b.channel_id,b.user.id,nonce,now()).await?;
            if room.panel.map(|p|p.to_string()).as_deref()!=Some(&panel)||!matches!(kind.as_str(),"add"|"remove"){return Err(fail("This member selector is outdated. Open a fresh control."))}
            let s::ComponentInteractionDataKind::UserSelect{values}=&b.data.kind else{return Err(fail("Choose one server member from the selector."))};
            if values.len()!=1{return Err(fail("Choose exactly one server member."))}
            let target=values[0];
            if target==room.owner||target==ctx.cache.current_user().id{return Err(fail("The channel owner and bot cannot be removed or reassigned through guest controls."))}
            let member=data.guild.member(&ctx.http,target).await?;
            let guild=data.guild.to_partial_guild(&ctx.http).await?;
            if member.user.bot||guild.owner_id==target||guild.member_permissions(&member).contains(s::Permissions::ADMINISTRATOR){return Err(fail("Choose a regular human member. Discord administrators already have access and cannot be blocked by channel permissions."))}
            if !room.state.members.contains_key(&target.get())&&room.state.members.len()>=30{return Err(fail("This room has reached its limit of 30 managed member entries. Ask staff to review its guest list."))}
            room.state.members.insert(target.get(),kind=="add");
            apply(ctx,data,&mut room,None).await?;
            return Ok(Some(if kind=="remove"{"That member is blocked from viewing and posting here.".into()}else if room.state.hidden{"Member approved. Access becomes visible when you unhide the channel.".into()}else{"Member added to this private channel.".into()}))
        }
        if room.panel!=Some(b.message.id){return Err(fail("Use the latest Personal panel in this channel."))}
        match action{
            "add"|"remove"=>{
                let nonce=session(&data.db,data.guild,&room,b.user.id,action).await?;
                b.edit_response(&ctx.http,response(if action=="add"{"Choose a member to add to this private room."}else{"Choose a member to remove from this room."}).components(vec![s::CreateActionRow::SelectMenu(s::CreateSelectMenu::new(format!("{PREFIX}pick:{nonce}"),s::CreateSelectMenuKind::User{default_users:None}).placeholder("Select one server member").min_values(1).max_values(1))])).await?;
                return Ok(None)
            },
            "lock"=>room.state.locked=true,
            "unlock"=>room.state.locked=false,
            "hide"=>room.state.hidden=true,
            "unhide"=>room.state.hidden=false,
            "clear"=>{},
            _=>return Err(fail("Unknown Personal control. Use the current channel panel.")),
        }
        let change=if action=="clear"{Some(topic(&mut room.state,room.channel.topic.as_deref().unwrap_or(""),true)?)}else{None};
        apply(ctx,data,&mut room,change).await?;
        Ok(Some(match action{"lock"=>"Channel locked. Its owner and administrators can still chat.","unlock"=>"Channel unlocked for its private audience.","hide"=>"Channel hidden from the identity role and guests. The owner and administrators retain access.","unhide"=>"Private audience restored. The channel is still hidden from everyone else.",_=>"Channel status cleared; the original topic is preserved."}.into()))
    }.await;
    match result {
        Ok(None) => {}
        Ok(Some(text)) => {
            b.edit_response(&ctx.http, response(text)).await?;
        }
        Err(e) => {
            b.edit_response(
                &ctx.http,
                response(crate::errors::with_reference(
                    e.as_ref(),
                    "personal-channel",
                    b.id.get(),
                )),
            )
            .await?;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bits(
        out: &[s::PermissionOverwrite],
        kind: s::PermissionOverwriteType,
    ) -> &s::PermissionOverwrite {
        out.iter().find(|p| p.kind == kind).unwrap()
    }
    #[test]
    fn hide_lock_and_unhide_never_expose_everyone_or_block_owner() {
        let g = s::GuildId::new(1);
        let owner = s::UserId::new(2);
        let role = s::RoleId::new(3);
        let bot = s::UserId::new(4);
        let other = s::PermissionOverwrite {
            kind: s::PermissionOverwriteType::Role(s::RoleId::new(9)),
            allow: s::Permissions::MANAGE_MESSAGES,
            deny: s::Permissions::empty(),
        };
        let mut state = State {
            hidden: true,
            locked: true,
            ..Default::default()
        };
        state.members.insert(5, true);
        state.members.insert(6, false);
        state.baseline = vec![other.clone()];
        let p = permissions(std::slice::from_ref(&other), g, owner, role, bot, &state);
        assert!(
            bits(&p, s::PermissionOverwriteType::Member(owner))
                .allow
                .contains(normal())
        );
        assert!(
            bits(&p, s::PermissionOverwriteType::Role(role))
                .deny
                .contains(view() | write())
        );
        assert!(
            bits(&p, s::PermissionOverwriteType::Member(s::UserId::new(5)))
                .deny
                .contains(view() | write())
        );
        state.hidden = false;
        state.locked = false;
        let p = permissions(&p, g, owner, role, bot, &state);
        assert!(
            bits(&p, s::PermissionOverwriteType::Role(s::RoleId::new(1)))
                .deny
                .contains(view())
        );
        assert!(
            bits(&p, s::PermissionOverwriteType::Role(role))
                .allow
                .contains(view() | write())
        );
        assert!(
            bits(&p, s::PermissionOverwriteType::Member(s::UserId::new(5)))
                .allow
                .contains(normal())
        );
        assert!(
            bits(&p, s::PermissionOverwriteType::Member(s::UserId::new(6)))
                .deny
                .contains(view())
        );
        assert_eq!(bits(&p, other.kind).allow, other.allow);
        assert!(!authorized(5, 2, false));
        assert!(authorized(2, 2, false));
        assert!(authorized(5, 2, true));
    }
    #[test]
    fn status_bounds_and_external_topic_edits_are_preserved() -> Result<(), Error> {
        assert_eq!(clean_status(" Building ")?, "Building");
        for text in ["", "line\nbreak", "<@2>", "@everyone"] {
            assert!(clean_status(text).is_err())
        }
        let mut state = State {
            topic_base: "Original topic".into(),
            last_topic: "Original topic".into(),
            status: "Busy".into(),
            ..Default::default()
        };
        assert_eq!(
            topic(&mut state, "Original topic", false)?,
            "Original topic\nStatus: Busy"
        );
        assert_eq!(
            topic(&mut state, "Staff edited topic", true)?,
            "Staff edited topic"
        );
        assert_eq!(state.status, "");
        Ok(())
    }
    #[tokio::test]
    async fn forms_bind_actor_channel_server_and_expiry_once() -> Result<(), Error> {
        let db = crate::store::connect("sqlite::memory:").await?;
        sqlx::query("INSERT INTO personal_sessions VALUES('nonce','1','2','3','4','add',100)")
            .execute(&db)
            .await?;
        assert!(
            claim(
                &db,
                s::GuildId::new(1),
                s::ChannelId::new(2),
                s::UserId::new(9),
                "nonce",
                90
            )
            .await
            .is_err()
        );
        assert!(
            claim(
                &db,
                s::GuildId::new(1),
                s::ChannelId::new(8),
                s::UserId::new(3),
                "nonce",
                90
            )
            .await
            .is_err()
        );
        assert!(
            claim(
                &db,
                s::GuildId::new(9),
                s::ChannelId::new(2),
                s::UserId::new(3),
                "nonce",
                90
            )
            .await
            .is_err()
        );
        assert!(
            claim(
                &db,
                s::GuildId::new(1),
                s::ChannelId::new(2),
                s::UserId::new(3),
                "nonce",
                101
            )
            .await
            .is_err()
        );
        assert_eq!(
            claim(
                &db,
                s::GuildId::new(1),
                s::ChannelId::new(2),
                s::UserId::new(3),
                "nonce",
                99
            )
            .await?,
            ("add".into(), "4".into())
        );
        assert!(
            claim(
                &db,
                s::GuildId::new(1),
                s::ChannelId::new(2),
                s::UserId::new(3),
                "nonce",
                99
            )
            .await
            .is_err()
        );
        quota(&db, "2", "3", 100).await?;
        assert!(quota(&db, "2", "3", 101).await.is_err());
        quota(&db, "2", "3", 102).await?;
        Ok(())
    }
    #[test]
    fn panel_uses_eight_scoped_custom_emoji_controls() {
        let rows = serde_json::to_value(controls()).unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 2);
        for row in rows.as_array().unwrap() {
            for b in row["components"].as_array().unwrap() {
                assert!(b["custom_id"].as_str().unwrap().starts_with(PREFIX));
                assert!(b["emoji"]["id"].is_string());
            }
        }
    }
    #[test]
    fn extra_member_and_role_allows_cannot_bypass_lock_or_hide() {
        let g = s::GuildId::new(1);
        let owner = s::UserId::new(2);
        let role = s::RoleId::new(3);
        let bot = s::UserId::new(4);
        let original = vec![
            s::PermissionOverwrite {
                kind: s::PermissionOverwriteType::Role(s::RoleId::new(8)),
                allow: view() | write() | s::Permissions::MANAGE_MESSAGES,
                deny: s::Permissions::empty(),
            },
            s::PermissionOverwrite {
                kind: s::PermissionOverwriteType::Member(s::UserId::new(9)),
                allow: view() | write(),
                deny: s::Permissions::empty(),
            },
        ];
        let mut state = State {
            hidden: true,
            locked: true,
            baseline: original.clone(),
            ..Default::default()
        };
        state.members.insert(owner.get(), false);
        let hidden = permissions(&original, g, owner, role, bot, &state);
        for p in &original {
            let controlled = bits(&hidden, p.kind);
            assert!(controlled.deny.contains(view() | write()));
            assert!(!controlled.allow.intersects(view() | write()));
        }
        assert!(
            bits(&hidden, s::PermissionOverwriteType::Member(owner))
                .allow
                .contains(normal())
        );
        state.hidden = false;
        state.locked = false;
        let restored = permissions(&hidden, g, owner, role, bot, &state);
        for p in &original {
            assert_eq!(bits(&restored, p.kind).allow, p.allow);
            assert_eq!(bits(&restored, p.kind).deny, p.deny);
        }
        assert!(
            bits(
                &restored,
                s::PermissionOverwriteType::Role(s::RoleId::new(1))
            )
            .deny
            .contains(view())
        );
    }
    #[test]
    fn topic_updates_never_write_permissions() {
        let topic = serde_json::to_value(edit_payload(
            Some("Private topic\nStatus: Busy".into()),
            vec![],
        ))
        .unwrap();
        assert_eq!(topic["topic"], "Private topic\nStatus: Busy");
        assert!(topic.get("permission_overwrites").is_none());
        let access = serde_json::to_value(edit_payload(None, vec![])).unwrap();
        assert!(access.get("topic").is_none());
        assert!(access["permission_overwrites"].is_array());
    }
    #[test]
    fn empty_status_form_omits_invalid_short_prefill() {
        let empty = serde_json::to_value(status_form(String::new(), "form".into())).unwrap();
        assert!(empty["data"]["components"][0]["components"][0]["value"].is_null());
        let filled = serde_json::to_value(status_form("Building".into(), "form".into())).unwrap();
        assert_eq!(
            filled["data"]["components"][0]["components"][0]["value"],
            "Building"
        );
    }
}
