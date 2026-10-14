use crate::{Context, Data, Error, cards, reply, serenity as s};
use sqlx::{Row, SqlitePool};
use std::{sync::Arc, time::Duration};

pub fn all() -> Vec<poise::Command<Data, Error>> {
    vec![
        media_panel(),
        schedule_post(),
        event(),
        mod_review(),
        server_health(),
        privacy(),
    ]
}
fn now() -> i64 {
    s::Timestamp::now().unix_timestamp()
}
fn text(value: &str, max: usize) -> Result<&str, Error> {
    let value = value.trim();
    if value.is_empty() || value.encode_utf16().count() > max {
        return Err(format!("Enter 1–{max} characters.").into());
    }
    Ok(value)
}
fn embed(title: &str, body: impl Into<String>) -> s::CreateEmbed {
    s::CreateEmbed::new()
        .title(format!("ELVNX / {title}"))
        .description(body)
        .color(0x111111)
        .footer(s::CreateEmbedFooter::new("11X • developed by avrxt.dev"))
}
async fn destination(
    ctx: &s::Context,
    guild: s::GuildId,
    actor: s::UserId,
    id: s::ChannelId,
    files: bool,
) -> Result<s::GuildChannel, Error> {
    let channel = id
        .to_channel(&ctx.http)
        .await?
        .guild()
        .ok_or("Server text channel required")?;
    if channel.guild_id != guild || channel.kind != s::ChannelType::Text {
        return Err("Select a text channel in this server.".into());
    }
    let partial = guild.to_partial_guild(&ctx.http).await?;
    let member = guild.member(&ctx.http, actor).await?;
    if !partial
        .member_permissions(&member)
        .contains(s::Permissions::MANAGE_GUILD)
    {
        return Err("The creator no longer has Manage Server.".into());
    }
    let bot_id = ctx.cache.current_user().id;
    for user in [actor, bot_id] {
        let member = guild.member(&ctx.http, user).await?;
        let mut required = s::Permissions::VIEW_CHANNEL | s::Permissions::SEND_MESSAGES;
        if user == bot_id {
            required |= s::Permissions::EMBED_LINKS;
            if files {
                required |= s::Permissions::ATTACH_FILES;
            }
        }
        if !partial
            .user_permissions_in(&channel, &member)
            .contains(required)
        {
            return Err(
                "Creator/bot destination access or Embed Links/Attach Files permission is missing."
                    .into(),
            );
        }
    }
    Ok(channel)
}

/// Preview up to three media files, then confirm posting through the bot.
#[poise::command(
    slash_command,
    guild_only,
    rename = "media-panel",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD",
    user_cooldown = 15
)]
async fn media_panel(
    ctx: Context<'_>,
    #[description = "Destination"] channel: s::GuildChannel,
    #[description = "First image, video, or audio file, up to 8 MiB"] file: s::Attachment,
    #[description = "Optional second file"] file_two: Option<s::Attachment>,
    #[description = "Optional third file"] file_three: Option<s::Attachment>,
    #[description = "Panel title, up to 100 characters"] title: Option<String>,
    #[description = "Caption, up to 2000 characters"] caption: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    destination(
        ctx.serenity_context(),
        ctx.data().guild,
        ctx.author().id,
        channel.id,
        true,
    )
    .await?;
    let title = title.unwrap_or_else(|| "MEDIA COLLECTION".into());
    text(&title, 100)?;
    let caption = caption.unwrap_or_default();
    if caption.encode_utf16().count() > 2000 {
        return reply(ctx, "Caption must be at most 2000 characters.").await;
    }
    let mut files = Vec::new();
    for (index, file) in [Some(file), file_two, file_three]
        .into_iter()
        .flatten()
        .enumerate()
    {
        let (name, bytes) = crate::commands::download_media(&file).await?;
        files.push(s::CreateAttachment::bytes(
            bytes,
            format!("{}-{name}", index + 1),
        ));
    }
    let nonce = format!("media-confirm-{}", ctx.id());
    let cancel = format!("media-cancel-{}", ctx.id());
    let mut preview_reply = poise::CreateReply::default()
        .ephemeral(true)
        .embed(embed(
            &title,
            format!(
                "{caption}\n\nPreview only · destination <#{}> · confirm within two minutes.",
                channel.id
            ),
        ))
        .components(vec![s::CreateActionRow::Buttons(vec![
            s::CreateButton::new(&nonce)
                .label("Post media")
                .style(s::ButtonStyle::Primary),
            s::CreateButton::new(&cancel)
                .label("Cancel")
                .style(s::ButtonStyle::Secondary),
        ])])
        .allowed_mentions(s::CreateAllowedMentions::new());
    for file in &files {
        preview_reply = preview_reply.attachment(file.clone());
    }
    let preview = ctx.send(preview_reply).await?;
    let message = preview.message().await?;
    let clicked = s::collector::ComponentInteractionCollector::new(&ctx.serenity_context().shard)
        .message_id(message.id)
        .author_id(ctx.author().id)
        .timeout(Duration::from_secs(120))
        .await;
    let Some(button) = clicked else {
        preview
            .edit(
                ctx,
                poise::CreateReply::default()
                    .embed(embed(
                        "MEDIA PREVIEW EXPIRED",
                        "Nothing was posted. Run /media-panel to try again.",
                    ))
                    .components(vec![]),
            )
            .await?;
        return Ok(());
    };
    button
        .create_response(ctx.http(), s::CreateInteractionResponse::Acknowledge)
        .await?;
    let outcome: Result<String, Error> = async {
        if button.data.custom_id != nonce {
            return Ok("Canceled. Nothing was posted.".into());
        }
        destination(
            ctx.serenity_context(),
            ctx.data().guild,
            ctx.author().id,
            channel.id,
            true,
        )
        .await?;
        let mut post = s::CreateMessage::new()
            .embed(embed(&title, caption))
            .allowed_mentions(s::CreateAllowedMentions::new());
        for file in files {
            post = post.add_file(file);
        }
        let sent = channel.id.send_message(ctx.http(), post).await?;
        crate::store::case(
            &ctx.data().db,
            ctx.data().guild.get(),
            0,
            ctx.author().id.get(),
            "media-panel",
            &format!("Channel {}, message {}", channel.id, sent.id),
        )
        .await?;
        Ok(format!("Posted: {}", sent.link()))
    }
    .await;
    preview
        .edit(
            ctx,
            poise::CreateReply::default()
                .embed(embed(
                    "MEDIA PANEL",
                    outcome.unwrap_or_else(|_| {
                        "Could not complete posting. Check the destination before retrying.".into()
                    }),
                ))
                .components(vec![]),
        )
        .await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
enum PostAction {
    Create,
    List,
    Edit,
    Cancel,
}
/// Schedule a text/media post in IST; list, edit, or cancel pending posts.
#[poise::command(
    slash_command,
    guild_only,
    rename = "schedule-post",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD",
    user_cooldown = 5
)]
async fn schedule_post(
    ctx: Context<'_>,
    action: PostAction,
    #[description = "Pending post ID for edit/cancel"] id: Option<i64>,
    #[description = "Destination for a new post"] channel: Option<s::GuildChannel>,
    #[description = "10m, 2h, or YYYY-MM-DD HH:MM IST"] when: Option<String>,
    #[description = "Text/caption, up to 2000 characters"] message: Option<String>,
    #[description = "Optional media upload, up to 8 MiB"] file: Option<s::Attachment>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let guild = ctx.data().guild.to_string();
    match action {
        PostAction::Create => {
            let channel = channel.ok_or("Select a destination")?;
            let due = crate::features::due(when.as_deref().ok_or("Choose a time")?, now())?;
            let body = message.unwrap_or_default();
            if body.encode_utf16().count() > 2000 || (body.trim().is_empty() && file.is_none()) {
                return reply(
                    ctx,
                    "Provide text or media; text must be at most 2000 characters.",
                )
                .await;
            }
            destination(
                ctx.serenity_context(),
                ctx.data().guild,
                ctx.author().id,
                channel.id,
                file.is_some(),
            )
            .await?;
            let (filename, bytes) = if let Some(file) = file {
                let (n, b) = crate::commands::download_media(&file).await?;
                (Some(n), Some(b))
            } else {
                (None, None)
            };
            let inserted=sqlx::query("INSERT INTO scheduled_posts(guild,actor,channel,due,text,filename,media,created) SELECT ?,?,?,?,?,?,?,? WHERE (SELECT COUNT(*) FROM scheduled_posts WHERE guild=? AND state='pending')<20 AND (SELECT COALESCE(SUM(length(media)),0) FROM scheduled_posts WHERE guild=? AND state='pending')+?<=134217728")
                .bind(&guild).bind(ctx.author().id.to_string()).bind(channel.id.to_string()).bind(due).bind(body).bind(filename).bind(&bytes).bind(now()).bind(&guild).bind(&guild).bind(bytes.as_ref().map_or(0,|b|b.len()) as i64).execute(&ctx.data().db).await?;
            if inserted.rows_affected() == 0 {
                return reply(ctx,"Queue limit reached: 20 pending posts or 128 MiB of media. Cancel old posts first.").await;
            }
            reply(ctx,format!("Post #{} scheduled for <t:{due}:F>. Delivery checks current permissions. Media is stored privately until delivery/cancellation; use /schedule-post List to review.",inserted.last_insert_rowid())).await
        }
        PostAction::List => {
            let rows=sqlx::query("SELECT id,channel,due,state FROM scheduled_posts WHERE guild=? ORDER BY id DESC LIMIT 20").bind(&guild).fetch_all(&ctx.data().db).await?;
            let body = rows
                .iter()
                .map(|r| {
                    format!(
                        "#{} · <#{}> · <t:{}:F> · {}",
                        r.get::<i64, _>("id"),
                        r.get::<String, _>("channel"),
                        r.get::<i64, _>("due"),
                        r.get::<String, _>("state")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            reply(
                ctx,
                if body.is_empty() {
                    "No scheduled posts.".into()
                } else {
                    body
                },
            )
            .await
        }
        PostAction::Cancel => {
            let result=sqlx::query("UPDATE scheduled_posts SET state='canceled',media=NULL,filename=NULL,text='' WHERE guild=? AND id=? AND state='pending'").bind(&guild).bind(id.ok_or("Choose a post ID")?).execute(&ctx.data().db).await?;
            reply(
                ctx,
                if result.rows_affected() == 1 {
                    "Canceled and stored media erased."
                } else {
                    "No pending post found; a sending post cannot be canceled."
                },
            )
            .await
        }
        PostAction::Edit => {
            if file.is_some() || channel.is_some() {
                return reply(ctx,"Editing changes text and time only. Cancel/recreate to change media or destination.").await;
            }
            let due = when
                .as_deref()
                .map(|v| crate::features::due(v, now()))
                .transpose()?;
            if let Some(body) = &message {
                text(body, 2000)?;
            }
            if due.is_none() && message.is_none() {
                return reply(ctx, "Provide a new time or text.").await;
            }
            let result=sqlx::query("UPDATE scheduled_posts SET due=COALESCE(?,due),text=COALESCE(?,text) WHERE guild=? AND id=? AND state='pending'").bind(due).bind(message).bind(&guild).bind(id.ok_or("Choose a post ID")?).execute(&ctx.data().db).await?;
            reply(
                ctx,
                if result.rows_affected() == 1 {
                    "Pending post updated."
                } else {
                    "No pending post found."
                },
            )
            .await
        }
    }
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
enum EventAction {
    Create,
    List,
    Cancel,
}
/// Create a native Discord event with RSVP, reminders, and an optional temporary voice room.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD | MANAGE_EVENTS",
    default_member_permissions = "MANAGE_GUILD | MANAGE_EVENTS",
    user_cooldown = 10
)]
#[allow(clippy::too_many_arguments)] // Discord slash options must be individual arguments.
async fn event(
    ctx: Context<'_>,
    action: EventAction,
    #[description = "Event ID for cancellation"] id: Option<String>,
    #[description = "Event name"] name: Option<String>,
    #[description = "YYYY-MM-DD HH:MM IST or relative time"] when: Option<String>,
    #[description = "Announcement/reminder destination"] channel: Option<s::GuildChannel>,
    #[description = "Existing voice room, or permission template for a temporary room"]
    voice: Option<s::GuildChannel>,
    #[description = "Create a temporary room matching the selected voice room's permissions"]
    temporary: Option<bool>,
    #[description = "Duration in minutes, 15–720"] minutes: Option<u32>,
    #[description = "Event details, up to 1000 characters"] description: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let guild = ctx.data().guild;
    match action {
        EventAction::List => {
            let rows=sqlx::query("SELECT id,name,due,state FROM community_events WHERE guild=? ORDER BY due DESC LIMIT 15").bind(guild.to_string()).fetch_all(&ctx.data().db).await?;
            let body = rows
                .iter()
                .map(|r| {
                    format!(
                        "`{}` · {} · <t:{}:F> · {}",
                        r.get::<String, _>("id"),
                        r.get::<String, _>("name"),
                        r.get::<i64, _>("due"),
                        r.get::<String, _>("state")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            reply(
                ctx,
                if body.is_empty() {
                    "No bot-managed events.".into()
                } else {
                    body
                },
            )
            .await
        }
        EventAction::Cancel => {
            let raw = id.ok_or("Choose an event ID")?;
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM community_events WHERE guild=? AND id=? AND state='open'",
            )
            .bind(guild.to_string())
            .bind(&raw)
            .fetch_one(&ctx.data().db)
            .await?;
            if exists == 0 {
                return reply(ctx, "No open bot-managed event found.").await;
            }
            let id = s::ScheduledEventId::new(raw.parse()?);
            let event = guild.scheduled_event(ctx.http(), id, false).await?;
            let status = if event.status == s::ScheduledEventStatus::Active {
                s::ScheduledEventStatus::Completed
            } else {
                s::ScheduledEventStatus::Canceled
            };
            guild
                .edit_scheduled_event(ctx.http(), id, s::EditScheduledEvent::new().status(status))
                .await?;
            sqlx::query(
                "UPDATE community_events SET state='canceled',ends=? WHERE guild=? AND id=?",
            )
            .bind(now())
            .bind(guild.to_string())
            .bind(raw)
            .execute(&ctx.data().db)
            .await?;
            reply(
                ctx,
                "Event canceled. Its temporary room will be removed only when empty.",
            )
            .await
        }
        EventAction::Create => {
            let _guard = ctx.data().configuration_lock.lock().await;
            let name = name.ok_or("Choose a name")?;
            text(&name, 80)?;
            let description = description.unwrap_or_else(|| "ELVNX community event".into());
            text(&description, 1000)?;
            let due = crate::features::due(when.as_deref().ok_or("Choose a start time")?, now())?;
            let minutes = minutes.unwrap_or(120);
            if due < now() + 1200 || !(15..=720).contains(&minutes) {
                return reply(
                    ctx,
                    "Start at least 20 minutes from now; duration must be 15–720 minutes.",
                )
                .await;
            }
            let ends = due + i64::from(minutes) * 60;
            let channel = channel.ok_or("Choose an announcement channel")?;
            destination(
                ctx.serenity_context(),
                guild,
                ctx.author().id,
                channel.id,
                true,
            )
            .await?;
            let template = voice.ok_or("Choose a voice room")?;
            if template.guild_id != guild || template.kind != s::ChannelType::Voice {
                return reply(ctx, "Choose a voice room in this server.").await;
            }
            let partial = guild.to_partial_guild(ctx.http()).await?;
            for user in [ctx.author().id, crate::bot_id(ctx)] {
                let member = guild.member(ctx.http(), user).await?;
                if !partial
                    .user_permissions_in(&template, &member)
                    .contains(s::Permissions::VIEW_CHANNEL | s::Permissions::CONNECT)
                {
                    return reply(
                        ctx,
                        "You and the bot need access to the selected voice room.",
                    )
                    .await;
                }
                let mut needed = s::Permissions::MANAGE_EVENTS;
                if temporary == Some(true) {
                    needed |= s::Permissions::MANAGE_CHANNELS;
                }
                if !partial.member_permissions(&member).contains(needed) {
                    return reply(
                        ctx,
                        "Manage Events is required; temporary rooms also require Manage Channels.",
                    )
                    .await;
                }
            }
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM community_events WHERE guild=? AND state='open'",
            )
            .bind(guild.to_string())
            .fetch_one(&ctx.data().db)
            .await?;
            if count >= 10 {
                return reply(
                    ctx,
                    "Maximum 10 open events; finish or cancel existing events first.",
                )
                .await;
            }
            let png = cards::render(
                "YOU'RE INVITED",
                &name,
                "ELVNX / community event",
                &[
                    ("START", "IST scheduled".into()),
                    ("DURATION", format!("{minutes} minutes")),
                ],
                None,
            )?;
            let owned = temporary == Some(true);
            let room = if owned {
                let mut builder = s::CreateChannel::new(format!("event-{name}"))
                    .kind(s::ChannelType::Voice)
                    .permissions(template.permission_overwrites.clone());
                if let Some(parent) = template.parent_id {
                    builder = builder.category(parent);
                }
                guild.create_channel(ctx.http(), builder).await?
            } else {
                template
            };
            let created = guild
                .create_scheduled_event(
                    ctx.http(),
                    s::CreateScheduledEvent::new(
                        s::ScheduledEventType::Voice,
                        &name,
                        s::Timestamp::from_unix_timestamp(due)?,
                    )
                    .channel_id(room.id)
                    .end_time(s::Timestamp::from_unix_timestamp(ends)?)
                    .description(&description),
                )
                .await;
            let event = match created {
                Ok(event) => event,
                Err(error) => {
                    if owned && room_empty(ctx.serenity_context(), guild, room.id) {
                        let _ = room.id.delete(ctx.http()).await;
                    }
                    return Err(error.into());
                }
            };
            if sqlx::query("INSERT INTO community_events(id,guild,actor,channel,room,owned,due,ends,name) VALUES(?,?,?,?,?,?,?,?,?)")
                .bind(event.id.to_string()).bind(guild.to_string()).bind(ctx.author().id.to_string()).bind(channel.id.to_string()).bind(room.id.to_string()).bind(owned).bind(due).bind(ends).bind(&name).execute(&ctx.data().db).await.is_err() {
                let removed = guild.delete_scheduled_event(ctx.http(), event.id).await.is_ok();
                if removed && owned && room_empty(ctx.serenity_context(),guild,room.id) { let _=room.id.delete(ctx.http()).await; }
                return reply(ctx, format!("Event tracking could not be saved. Event ID: {}. Event rollback: {}. Check Discord before retrying; any remaining temporary room must be removed manually.",event.id,if removed {"completed"}else{"failed"})).await;
            }
            let url = format!("https://discord.com/events/{guild}/{}", event.id);
            let sent=channel.id.send_message(ctx.http(),s::CreateMessage::new().embed(embed("COMMUNITY EVENT",format!("**{name}**\n{description}\n\n<t:{due}:F> · <t:{due}:R>\nVoice room <#{}>\nUse **Interested / RSVP** on the Discord event. Reminder posts here 15 minutes before start.",room.id)).image("attachment://event-cover.png"))
                .add_file(s::CreateAttachment::bytes(png,"event-cover.png")).components(vec![s::CreateActionRow::Buttons(vec![s::CreateButton::new_link(&url).label("Interested / RSVP")])]).allowed_mentions(s::CreateAllowedMentions::new())).await;
            reply(ctx,format!("Event created: {url}\nAnnouncement: {}. Native Discord RSVP notifications are managed by Discord. Temporary rooms are removed after the event ends and everyone leaves.",if sent.is_ok(){"posted"}else{"failed; event remains created—do not recreate it"})).await
        }
    }
}
fn room_empty(ctx: &s::Context, guild: s::GuildId, room: s::ChannelId) -> bool {
    ctx.cache
        .guild(guild)
        .is_some_and(|g| !g.voice_states.values().any(|v| v.channel_id == Some(room)))
}
fn not_found(error: &s::Error) -> bool {
    matches!(error, s::Error::Http(error) if error.status_code() == Some(reqwest::StatusCode::NOT_FOUND))
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
enum ReviewAction {
    List,
    Claim,
    Resolve,
    Reopen,
}
pub(crate) fn review_buttons(id: i64) -> Vec<s::CreateActionRow> {
    vec![s::CreateActionRow::Buttons(vec![
        s::CreateButton::new(format!("review-claim-{id}"))
            .label("Claim")
            .style(s::ButtonStyle::Primary),
        s::CreateButton::new(format!("review-resolve-{id}"))
            .label("Resolve")
            .style(s::ButtonStyle::Success),
        s::CreateButton::new(format!("review-reopen-{id}"))
            .label("Reopen")
            .style(s::ButtonStyle::Secondary),
    ])]
}
async fn review_change(
    db: &SqlitePool,
    guild: s::GuildId,
    channel: s::ChannelId,
    id: i64,
    actor: s::UserId,
    action: ReviewAction,
    note: Option<&str>,
) -> Result<bool, Error> {
    let guild = guild.to_string();
    let channel = channel.to_string();
    let changed=match action{
        ReviewAction::Claim=>sqlx::query("UPDATE review_queue SET assignee=?,state='claimed' WHERE guild=? AND channel=? AND report=? AND state='open' AND assignee IS NULL").bind(actor.to_string()).bind(&guild).bind(&channel).bind(id).execute(db).await?,
        ReviewAction::Resolve=>{let note=text(note.ok_or("Provide a resolution note")?,500)?;sqlx::query("UPDATE review_queue SET state='resolved',note=? WHERE guild=? AND channel=? AND report=? AND state='claimed' AND assignee=?").bind(note).bind(&guild).bind(&channel).bind(id).bind(actor.to_string()).execute(db).await?},
        ReviewAction::Reopen=>sqlx::query("UPDATE review_queue SET state='open',assignee=NULL,note='' WHERE guild=? AND channel=? AND report=? AND state='resolved'").bind(&guild).bind(&channel).bind(id).execute(db).await?,
        ReviewAction::List=>return Ok(false),
    };
    Ok(changed.rows_affected() == 1)
}
async fn review_access(
    ctx: &s::Context,
    data: &Data,
    user: s::UserId,
    channel: s::ChannelId,
) -> Result<(), Error> {
    if data.settings.read().await.report_channel != Some(channel.get()) {
        return Err("This is not the current report destination.".into());
    }
    let channel = channel
        .to_channel(&ctx.http)
        .await?
        .guild()
        .ok_or("Staff channel unavailable")?;
    crate::features::private_reports(ctx, data.guild, &channel).await?;
    let guild = data.guild.to_partial_guild(&ctx.http).await?;
    let member = data.guild.member(&ctx.http, user).await?;
    if member.user.bot
        || !guild
            .member_permissions(&member)
            .contains(s::Permissions::MODERATE_MEMBERS)
        || !guild
            .user_permissions_in(&channel, &member)
            .contains(s::Permissions::VIEW_CHANNEL)
    {
        return Err("You need Moderate Members and access to the private staff channel.".into());
    }
    Ok(())
}
async fn refresh_review(ctx: &s::Context, data: &Data, id: i64) -> Result<(), Error> {
    let row = sqlx::query(
        "SELECT channel,message,state,assignee,note FROM review_queue WHERE guild=? AND report=?",
    )
    .bind(data.guild.to_string())
    .bind(id)
    .fetch_one(&data.db)
    .await?;
    let channel = s::ChannelId::new(row.get::<String, _>("channel").parse()?);
    let message = s::MessageId::new(row.get::<String, _>("message").parse()?);
    let original = channel.message(&ctx.http, message).await?;
    if original.author.id != ctx.cache.current_user().id {
        return Err("Report message ownership changed".into());
    }
    let Some(mut original_embed) = original.embeds.into_iter().next() else {
        return Ok(());
    };
    original_embed.fields.retain(|f| f.name != "Review status");
    let status = format!(
        "{} · {}\n{}",
        row.get::<String, _>("state"),
        row.get::<Option<String>, _>("assignee")
            .map(|id| format!("<@{id}>"))
            .unwrap_or_else(|| "unassigned".into()),
        row.get::<String, _>("note")
    );
    channel
        .edit_message(
            &ctx.http,
            message,
            s::EditMessage::new()
                .embed(s::CreateEmbed::from(original_embed).field("Review status", status, false))
                .components(review_buttons(id))
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await?;
    Ok(())
}
#[derive(poise::Modal)]
#[name = "ELVNX / Resolve Report"]
struct Resolution {
    #[name = "Internal resolution note"]
    #[paragraph]
    #[min_length = 1]
    #[max_length = 500]
    note: String,
}
pub async fn gateway(ctx: &s::Context, event: &s::FullEvent, data: &Data) -> Result<bool, Error> {
    let s::FullEvent::InteractionCreate {
        interaction: s::Interaction::Component(button),
    } = event
    else {
        return Ok(false);
    };
    if !button.data.custom_id.starts_with("review-") {
        return Ok(false);
    }
    let parsed = button
        .data
        .custom_id
        .strip_prefix("review-")
        .and_then(|v| v.split_once('-'))
        .and_then(|(action, id)| {
            Some((
                match action {
                    "claim" => ReviewAction::Claim,
                    "resolve" => ReviewAction::Resolve,
                    "reopen" => ReviewAction::Reopen,
                    _ => return None,
                },
                id.parse::<i64>().ok()?,
            ))
        });
    let access:Result<(ReviewAction,i64),Error>=async{
        if button.guild_id!=Some(data.guild)||button.message.author.id!=ctx.cache.current_user().id{return Err("Invalid staff panel".into());}
        review_access(ctx,data,button.user.id,button.channel_id).await?;
        let (action,id)=parsed.ok_or("Invalid report action")?;
        let matched:i64=sqlx::query_scalar("SELECT COUNT(*) FROM review_queue WHERE guild=? AND channel=? AND message=? AND report=?").bind(data.guild.to_string()).bind(button.channel_id.to_string()).bind(button.message.id.to_string()).bind(id).fetch_one(&data.db).await?;
        if matched!=1{return Err("Report tracking is unavailable or expired. Use the staff channel.".into());}Ok((action,id))
    }.await;
    let (action, id) = match access {
        Ok(v) => v,
        Err(error) => {
            button
                .create_response(
                    &ctx.http,
                    s::CreateInteractionResponse::Message(
                        s::CreateInteractionResponseMessage::new()
                            .ephemeral(true)
                            .embed(embed("REVIEW UNAVAILABLE", error.to_string()))
                            .allowed_mentions(s::CreateAllowedMentions::new()),
                    ),
                )
                .await?;
            return Ok(true);
        }
    };
    if matches!(action, ReviewAction::Resolve) {
        use poise::Modal;
        let custom = format!("resolution-{}", button.id);
        button
            .create_response(&ctx.http, Resolution::create(None, custom.clone()))
            .await?;
        let user = button.user.id;
        let guild = data.guild;
        let response = s::collector::ModalInteractionCollector::new(&ctx.shard)
            .filter(move |m| {
                m.data.custom_id == custom && m.user.id == user && m.guild_id == Some(guild)
            })
            .timeout(Duration::from_secs(180))
            .await;
        if let Some(response) = response {
            response
                .create_response(
                    &ctx.http,
                    s::CreateInteractionResponse::Defer(
                        s::CreateInteractionResponseMessage::new().ephemeral(true),
                    ),
                )
                .await?;
            let result: Result<bool, Error> = async {
                review_access(ctx, data, user, button.channel_id).await?;
                let form =
                    Resolution::parse(response.data.clone()).map_err(|e| -> Error { e.into() })?;
                review_change(
                    &data.db,
                    guild,
                    button.channel_id,
                    id,
                    user,
                    action,
                    Some(&form.note),
                )
                .await
            }
            .await;
            let body = review_outcome(&result);
            if matches!(result, Ok(true)) {
                let _ = refresh_review(ctx, data, id).await;
                let _ = crate::store::case(
                    &data.db,
                    guild.get(),
                    0,
                    user.get(),
                    "mod-review",
                    &format!("Resolved report #{id}"),
                )
                .await;
            }
            response
                .edit_response(
                    &ctx.http,
                    s::EditInteractionResponse::new()
                        .embed(embed("REPORT REVIEW", body))
                        .allowed_mentions(s::CreateAllowedMentions::new()),
                )
                .await?;
        }
    } else {
        button
            .create_response(
                &ctx.http,
                s::CreateInteractionResponse::Defer(
                    s::CreateInteractionResponseMessage::new().ephemeral(true),
                ),
            )
            .await?;
        let result = review_change(
            &data.db,
            data.guild,
            button.channel_id,
            id,
            button.user.id,
            action,
            None,
        )
        .await;
        let body = review_outcome(&result);
        if matches!(result, Ok(true)) {
            let _ = refresh_review(ctx, data, id).await;
            let _ = crate::store::case(
                &data.db,
                data.guild.get(),
                0,
                button.user.id.get(),
                "mod-review",
                &format!("Report #{id}: {action:?}"),
            )
            .await;
        }
        button
            .edit_response(
                &ctx.http,
                s::EditInteractionResponse::new()
                    .embed(embed("REPORT REVIEW", body))
                    .allowed_mentions(s::CreateAllowedMentions::new()),
            )
            .await?;
    }
    Ok(true)
}
fn review_outcome(result: &Result<bool, Error>) -> String {
    match result{Ok(true)=>"Report updated. This does not automatically punish or notify the reporter.".into(),Ok(false)=>"No eligible report found. Claim an open report first; only its assignee can resolve it.".into(),Err(e)=>e.to_string()}
}
/// Review the confidential report queue; claim, resolve, or reopen a receipt.
#[poise::command(
    slash_command,
    guild_only,
    rename = "mod-review",
    required_permissions = "MODERATE_MEMBERS",
    default_member_permissions = "MODERATE_MEMBERS"
)]
async fn mod_review(
    ctx: Context<'_>,
    action: ReviewAction,
    #[description = "Report receipt ID"] id: Option<i64>,
    #[description = "Internal resolution note, up to 500 characters"] note: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let channel = ctx
        .data()
        .settings
        .read()
        .await
        .report_channel
        .ok_or("Configure /setup-report first")?;
    let channel = s::ChannelId::new(channel)
        .to_channel(ctx.http())
        .await?
        .guild()
        .ok_or("Report channel unavailable")?;
    crate::features::private_reports(ctx.serenity_context(), ctx.data().guild, &channel).await?;
    let partial = ctx.data().guild.to_partial_guild(ctx.http()).await?;
    let member = ctx.data().guild.member(ctx.http(), ctx.author().id).await?;
    if !partial
        .user_permissions_in(&channel, &member)
        .contains(s::Permissions::VIEW_CHANNEL)
    {
        return reply(ctx, "You need access to the private report channel.").await;
    }
    let guild = ctx.data().guild.to_string();
    match action {
        ReviewAction::List => {
            let rows=sqlx::query("SELECT report,message,state,assignee FROM review_queue WHERE guild=? AND channel=? ORDER BY report DESC LIMIT 15").bind(&guild).bind(channel.id.to_string()).fetch_all(&ctx.data().db).await?;
            let list = rows
                .iter()
                .map(|r| {
                    format!(
                        "[#{}](https://discord.com/channels/{}/{}/{}) · {} · {}",
                        r.get::<i64, _>("report"),
                        guild,
                        channel.id,
                        r.get::<String, _>("message"),
                        r.get::<String, _>("state"),
                        r.get::<Option<String>, _>("assignee")
                            .map(|id| format!("<@{id}>"))
                            .unwrap_or_else(|| "unassigned".into())
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            reply(ctx,if list.is_empty(){"No queued reports. New submissions appear here; older reports remain in the staff channel. Moderation case history is available via /case and /warnings.".into()}else{list}).await
        }
        _ => {
            let id = id.ok_or("Choose a report receipt")?;
            let changed = review_change(
                &ctx.data().db,
                ctx.data().guild,
                channel.id,
                id,
                ctx.author().id,
                action,
                note.as_deref(),
            )
            .await?;
            if !changed {
                return reply(ctx,"No eligible report found. Claim an open report first; only its assignee can resolve it.").await;
            }
            let _ = refresh_review(ctx.serenity_context(), ctx.data(), id).await;
            crate::audit(ctx, 0, "mod-review", &format!("Report #{id}: {action:?}")).await
        }
    }
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
enum PrivacyAction {
    Info,
    EraseAndStop,
    Resume,
}
/// Learn about stored data, erase your activity counters and stop tracking, or resume.
#[poise::command(slash_command, guild_only, user_cooldown = 10)]
async fn privacy(
    ctx: Context<'_>,
    action: PrivacyAction,
    #[description = "Required confirmation for erasing your activity"] confirm: Option<bool>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let guild = ctx.data().guild.to_string();
    let user = ctx.author().id.to_string();
    match action {
        PrivacyAction::Info => {
            let opted: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM activity_optout WHERE guild=? AND user=?")
                    .bind(&guild)
                    .bind(&user)
                    .fetch_one(&ctx.data().db)
                    .await?;
            reply(ctx,format!("**Activity tracking:** {}\nWhen enabled server-wide, we save message counts and voice duration, not message contents in the activity database. Daily counters are retained for 30 days; cumulative counters remain until erased. Presence is kept in memory.\n\nUse **EraseAndStop confirm:true** to erase your activity counters and stop future counting. **Resume** permits future counting. Birthday opt-in uses /birthday-remove. Playlists use /playlist delete.\n\nModeration cases, reports, Discord log messages and already-posted report images are separate staff records; this command does not delete them. Contact staff for other deletion requests. Scheduled-post uploads are stored privately until delivery/cancellation; cancel them with /schedule-post.",if opted>0{"you opted out"}else{"subject to server settings"})).await
        }
        PrivacyAction::EraseAndStop => {
            if confirm != Some(true) {
                return reply(ctx,"Use confirm:true to erase your message/voice counters and stop future activity counting.").await;
            }
            let mut voices = ctx.data().voices.lock().await;
            let mut tx = ctx.data().db.begin().await?;
            sqlx::query("INSERT OR IGNORE INTO activity_optout VALUES(?,?)")
                .bind(&guild)
                .bind(&user)
                .execute(&mut *tx)
                .await?;
            for table in ["activity", "daily_activity"] {
                sqlx::query(&format!("DELETE FROM {table} WHERE guild=? AND user=?"))
                    .bind(&guild)
                    .bind(&user)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            voices.remove(&ctx.author().id.get());
            drop(voices);
            reply(ctx,"Your stored activity counters were erased. Future message/voice activity will not be counted unless you use Resume. Existing Discord logs and report images remain; contact staff for those.").await
        }
        PrivacyAction::Resume => {
            let mut voices = ctx.data().voices.lock().await;
            sqlx::query("DELETE FROM activity_optout WHERE guild=? AND user=?")
                .bind(guild)
                .bind(user)
                .execute(&ctx.data().db)
                .await?;
            voices.remove(&ctx.author().id.get());
            if ctx.data().settings.read().await.activity_tracking
                && ctx
                    .serenity_context()
                    .cache
                    .guild(ctx.data().guild)
                    .is_some_and(|g| {
                        g.voice_states
                            .get(&ctx.author().id)
                            .is_some_and(|v| v.channel_id.is_some())
                    })
            {
                voices.insert(ctx.author().id.get(), std::time::Instant::now());
            }
            drop(voices);
            reply(ctx,"Opt-out removed. Future activity can be counted when server-wide tracking is enabled; erased history is not restored.").await
        }
    }
}

pub(crate) async fn permission_check(
    ctx: Context<'_>,
    command: String,
    channel: Option<s::GuildChannel>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let commands = crate::commands::all();
    let name = command.trim().trim_start_matches('/');
    let Some(cmd) = commands.iter().find(|c| c.name == name) else {
        return reply(ctx, "Unknown command. Use /help search:WORD.").await;
    };
    let channel = match channel {
        Some(c) => c,
        None => ctx
            .channel_id()
            .to_channel(ctx.http())
            .await?
            .guild()
            .ok_or("Server channel required")?,
    };
    if channel.guild_id != ctx.data().guild {
        return reply(ctx, "Choose a channel in this server.").await;
    }
    let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
    let member = guild.id.member(ctx.http(), ctx.author().id).await?;
    let actor = guild.user_permissions_in(&channel, &member);
    if !actor.contains(s::Permissions::VIEW_CHANNEL) {
        return reply(ctx, "You cannot inspect a channel you cannot view.").await;
    }
    let bot = guild.id.member(ctx.http(), crate::bot_id(ctx)).await?;
    let bot_permissions = guild.user_permissions_in(&channel, &bot);
    let expected = cmd.required_permissions;
    let actor_missing = expected & !actor;
    let bot_expected =
        s::Permissions::VIEW_CHANNEL | s::Permissions::SEND_MESSAGES | s::Permissions::EMBED_LINKS;
    let bot_missing = (cmd.required_bot_permissions | bot_expected) & !bot_permissions;
    reply(ctx,format!("**/{name}** in <#{}>\nYour missing command permissions: `{actor_missing:?}`\nBot missing base permissions: `{bot_missing:?}`\n\nThis is a permission diagnostic, not a bypass. Discord integration restrictions, cooldowns, role hierarchy, DJ rules, feature setup, target channel permissions and owner-only checks can impose additional requirements. Media commands also need Attach Files.",channel.id)).await
}
pub(crate) async fn command_search(ctx: Context<'_>, query: String) -> Result<(), Error> {
    let query = text(&query, 80)?.to_lowercase();
    let found = crate::commands::all()
        .into_iter()
        .filter(|c| {
            c.name.to_lowercase().contains(&query)
                || c.description
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&query)
        })
        .take(15)
        .map(|c| format!("**/{}** — {}", c.name, c.description.unwrap_or_default()))
        .collect::<Vec<_>>()
        .join("\n");
    reply(
        ctx,
        if found.is_empty() {
            "No matching commands. Try a broader word or /help.".into()
        } else {
            found
        },
    )
    .await
}

#[derive(Default, Debug)]
struct Health {
    total: u64,
    available: u64,
    disk: Option<u8>,
    load: Option<f64>,
}
fn memory(raw: &str) -> (u64, u64) {
    let value = |key: &str| {
        raw.lines()
            .find_map(|line| {
                line.strip_prefix(key)
                    .and_then(|v| v.split_whitespace().next())
                    .and_then(|v| v.parse::<u64>().ok())
            })
            .unwrap_or(0)
    };
    (value("MemTotal:"), value("MemAvailable:"))
}
fn resource_health() -> Health {
    let (total, available) = memory(&std::fs::read_to_string("/proc/meminfo").unwrap_or_default());
    let load = std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|v| v.split_whitespace().next()?.parse().ok());
    let disk = std::process::Command::new("df")
        .args(["-P", "/var/lib/elvnx-bot"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|v| {
            v.lines()
                .nth(1)?
                .split_whitespace()
                .nth(4)?
                .trim_end_matches('%')
                .parse()
                .ok()
        });
    Health {
        total,
        available,
        disk,
        load,
    }
}
fn health_problems(h: &Health) -> Vec<&'static str> {
    let mut out = Vec::new();
    if h.total > 0 && h.available * 100 / h.total < 15 {
        out.push("Available RAM below 15%");
    }
    if h.disk.is_some_and(|v| v >= 85) {
        out.push("Disk usage at least 85%");
    }
    let cpus = std::thread::available_parallelism().map_or(1, usize::from) as f64;
    if h.load.is_some_and(|v| v > cpus * 2.0) {
        out.push("High one-minute CPU load");
    }
    out
}
/// View private VPS health or configure staff-only resource/Lavalink alerts.
#[poise::command(
    slash_command,
    guild_only,
    rename = "server-health",
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD",
    user_cooldown = 10
)]
async fn server_health(
    ctx: Context<'_>,
    #[description = "Enable/disable alerts; omit for a health snapshot"] alerts: Option<bool>,
    #[description = "Private staff destination when enabling alerts"] channel: Option<
        s::GuildChannel,
    >,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if let Some(enabled) = alerts {
        if enabled {
            let channel = channel.ok_or("Choose a private staff channel")?;
            destination(
                ctx.serenity_context(),
                ctx.data().guild,
                ctx.author().id,
                channel.id,
                false,
            )
            .await?;
            crate::features::private_reports(ctx.serenity_context(), ctx.data().guild, &channel)
                .await?;
            sqlx::query("INSERT INTO health_alerts VALUES(?,?,?) ON CONFLICT(guild) DO UPDATE SET actor=excluded.actor,channel=excluded.channel").bind(ctx.data().guild.to_string()).bind(ctx.author().id.to_string()).bind(channel.id.to_string()).execute(&ctx.data().db).await?;
        } else {
            sqlx::query("DELETE FROM health_alerts WHERE guild=?")
                .bind(ctx.data().guild.to_string())
                .execute(&ctx.data().db)
                .await?;
        }
    }
    let h = tokio::task::spawn_blocking(resource_health).await?;
    let stamp = std::time::Instant::now();
    let db = sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&ctx.data().db)
        .await
        .is_ok();
    let db_ms = stamp.elapsed().as_millis();
    let music = match &ctx.data().music {
        Some(node) => {
            if node
                .request(reqwest::Method::GET, "/v4/info", None)
                .await
                .is_ok()
            {
                "Healthy"
            } else {
                "Unavailable"
            }
        }
        None => "Not configured",
    };
    let png = cards::render(
        "SYSTEM HEALTH",
        "ELVNX / 11X",
        "Private infrastructure snapshot",
        &[
            ("GATEWAY", format!("{} ms", ctx.ping().await.as_millis())),
            ("DATABASE", format!("{db_ms} ms")),
            (
                "MEMORY",
                if h.total > 0 {
                    format!("{} MiB available", h.available / 1024)
                } else {
                    "Unavailable".into()
                },
            ),
        ],
        None,
    )?;
    ctx.send(poise::CreateReply::default().ephemeral(true).attachment(s::CreateAttachment::bytes(png,"health.png"))
        .embed(embed("SERVER HEALTH",format!("**RAM:** {} / {} MiB available\n**Disk:** {}\n**1m load:** {}\n**Database:** {} ({db_ms} ms)\n**Lavalink:** {music}\n**Process uptime:** {} minutes\n**Resource alerts:** {}\nAlerts require three consecutive unhealthy samples; recovery posts once. Bot-offline alerts require an independent external monitor.",h.available/1024,h.total/1024,h.disk.map(|v|format!("{v}% used")).unwrap_or_else(||"Unavailable".into()),h.load.map(|v|format!("{v:.2}")).unwrap_or_else(||"Unavailable".into()),if db{"Healthy"}else{"Unavailable"},ctx.data().started.elapsed().as_secs()/60,health_problems(&h).join("; "))).image("attachment://health.png"))
        .allowed_mentions(s::CreateAllowedMentions::new())).await?;
    Ok(())
}

pub fn start(
    ctx: s::Context,
    db: SqlitePool,
    guild: s::GuildId,
    music: Option<Arc<crate::music::Service>>,
    manager: Arc<s::ShardManager>,
) {
    tokio::spawn(async move {
        // Ambiguous sends are never automatically replayed after a crash.
        let _=sqlx::query("UPDATE scheduled_posts SET state='uncertain',media=NULL,filename=NULL,text='' WHERE guild=? AND state='sending'").bind(guild.to_string()).execute(&db).await;
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut streak = 0u8;
        let mut alerted = false;
        let mut alert_destination = None;
        loop {
            interval.tick().await;
            let connected = manager
                .runners
                .lock()
                .await
                .get(&ctx.shard_id)
                .is_some_and(|runner| runner.stage == s::ConnectionStage::Connected);
            if !connected {
                continue;
            }
            if let Err(error) = posts_tick(&ctx, &db, guild).await {
                tracing::warn!(%error,"Post scheduler failed");
            }
            if let Err(error) = events_tick(&ctx, &db, guild).await {
                tracing::warn!(%error,"Event scheduler failed");
            }
            if let Err(error) = health_tick(
                &ctx,
                &db,
                guild,
                music.as_ref(),
                &mut streak,
                &mut alerted,
                &mut alert_destination,
            )
            .await
            {
                tracing::warn!(%error,"Health monitor failed");
            }
            let _ = sqlx::query(
                "DELETE FROM scheduled_posts WHERE state!='pending' AND state!='sending' AND due<?",
            )
            .bind(now() - 7 * 86400)
            .execute(&db)
            .await;
            let _ = sqlx::query(
                "DELETE FROM review_queue WHERE report NOT IN (SELECT id FROM reports)",
            )
            .execute(&db)
            .await;
            let _ = sqlx::query("DELETE FROM community_events WHERE state='cleaned' AND ends<?")
                .bind(now() - 30 * 86400)
                .execute(&db)
                .await;
        }
    });
}
async fn claim_post(
    db: &SqlitePool,
    guild: s::GuildId,
    id: i64,
    time: i64,
) -> Result<Option<sqlx::sqlite::SqliteRow>, Error> {
    Ok(sqlx::query("UPDATE scheduled_posts SET state='sending' WHERE id=? AND guild=? AND state='pending' AND due<=? RETURNING *").bind(id).bind(guild.to_string()).bind(time).fetch_optional(db).await?)
}
async fn posts_tick(ctx: &s::Context, db: &SqlitePool, guild: s::GuildId) -> Result<(), Error> {
    let ids:Vec<i64>=sqlx::query_scalar("SELECT id FROM scheduled_posts WHERE guild=? AND state='pending' AND due<=? ORDER BY due LIMIT 5").bind(guild.to_string()).bind(now()).fetch_all(db).await?;
    for id in ids {
        let Some(row) = claim_post(db, guild, id, now()).await? else {
            continue;
        };
        let result: Result<(), Error> = async {
            if row.get::<i64, _>("due") < now() - 86400 {
                return Err("Scheduled post expired during extended downtime".into());
            }
            let bytes = row.get::<Option<Vec<u8>>, _>("media");
            let channel = destination(
                ctx,
                guild,
                s::UserId::new(row.get::<String, _>("actor").parse()?),
                s::ChannelId::new(row.get::<String, _>("channel").parse()?),
                bytes.is_some(),
            )
            .await?;
            let mut message = s::CreateMessage::new()
                .embed(embed("SCHEDULED POST", row.get::<String, _>("text")))
                .allowed_mentions(s::CreateAllowedMentions::new());
            if let Some(bytes) = bytes {
                message = message.add_file(s::CreateAttachment::bytes(
                    bytes,
                    row.get::<Option<String>, _>("filename")
                        .unwrap_or_else(|| "media".into()),
                ));
            }
            channel.id.send_message(&ctx.http, message).await?;
            Ok(())
        }
        .await;
        // Discord HTTP failures can have ambiguous delivery; the queue never retries blindly.
        sqlx::query(
            "UPDATE scheduled_posts SET state=?,media=NULL,filename=NULL,text='' WHERE id=?",
        )
        .bind(if result.is_ok() {
            "sent"
        } else {
            "failed-check-channel"
        })
        .bind(id)
        .execute(db)
        .await?;
    }
    Ok(())
}
async fn events_tick(ctx: &s::Context, db: &SqlitePool, guild: s::GuildId) -> Result<(), Error> {
    let rows = sqlx::query(
        "SELECT * FROM community_events WHERE guild=? AND state!='cleaned' ORDER BY due LIMIT 20",
    )
    .bind(guild.to_string())
    .fetch_all(db)
    .await?;
    for row in rows {
        let id = row.get::<String, _>("id");
        let due = row.get::<i64, _>("due");
        let ends = row.get::<i64, _>("ends");
        let state = row.get::<String, _>("state");
        if state == "open"
            && row.get::<i64, _>("reminded") == 0
            && now() >= due - 900
            && now() < due
        {
            let claimed =
                sqlx::query("UPDATE community_events SET reminded=1 WHERE id=? AND reminded=0")
                    .bind(&id)
                    .execute(db)
                    .await?;
            if claimed.rows_affected() > 0 {
                let result:Result<(),Error>=async{
                    let event=guild.scheduled_event(&ctx.http,s::ScheduledEventId::new(id.parse()?),false).await?;
                    if event.status!=s::ScheduledEventStatus::Scheduled{return Ok(());}
                    let channel=destination(ctx,guild,s::UserId::new(row.get::<String,_>("actor").parse()?),s::ChannelId::new(row.get::<String,_>("channel").parse()?),false).await?;
                    channel.id.send_message(&ctx.http,s::CreateMessage::new().embed(embed("EVENT REMINDER",format!("**{}** starts <t:{due}:R>.\n[Open event / RSVP](https://discord.com/events/{guild}/{id})",row.get::<String,_>("name")))).allowed_mentions(s::CreateAllowedMentions::new())).await?;Ok(())
                }.await;
                if result.is_err() {
                    tracing::warn!(event=%id,"Event reminder failed; check destination permissions");
                }
            }
        }
        if now() >= ends {
            if state == "open" {
                let event_id = s::ScheduledEventId::new(id.parse()?);
                match guild.scheduled_event(&ctx.http, event_id, false).await {
                    Ok(event) => {
                        let status = match event.status {
                            s::ScheduledEventStatus::Active => {
                                Some(s::ScheduledEventStatus::Completed)
                            }
                            s::ScheduledEventStatus::Scheduled => {
                                Some(s::ScheduledEventStatus::Canceled)
                            }
                            _ => None,
                        };
                        if let Some(status) = status {
                            guild
                                .edit_scheduled_event(
                                    &ctx.http,
                                    event_id,
                                    s::EditScheduledEvent::new().status(status),
                                )
                                .await?;
                        }
                    }
                    Err(error) if not_found(&error) => {}
                    Err(error) => return Err(error.into()),
                }
                sqlx::query("UPDATE community_events SET state='ended' WHERE id=?")
                    .bind(&id)
                    .execute(db)
                    .await?;
            }
            let room = s::ChannelId::new(row.get::<String, _>("room").parse()?);
            if row.get::<bool, _>("owned") {
                if !room_empty(ctx, guild, room) {
                    continue;
                }
                match room.to_channel(&ctx.http).await {
                    Ok(_) => {
                        room.delete(&ctx.http).await?;
                    }
                    Err(error) if not_found(&error) => {}
                    Err(error) => return Err(error.into()),
                }
            }
            sqlx::query("UPDATE community_events SET state='cleaned' WHERE id=?")
                .bind(id)
                .execute(db)
                .await?;
        }
    }
    Ok(())
}
async fn health_tick(
    ctx: &s::Context,
    db: &SqlitePool,
    guild: s::GuildId,
    music: Option<&Arc<crate::music::Service>>,
    streak: &mut u8,
    alerted: &mut bool,
    last: &mut Option<String>,
) -> Result<(), Error> {
    let Some(row) = sqlx::query("SELECT actor,channel FROM health_alerts WHERE guild=?")
        .bind(guild.to_string())
        .fetch_optional(db)
        .await?
    else {
        *streak = 0;
        *alerted = false;
        *last = None;
        return Ok(());
    };
    let target = row.get::<String, _>("channel");
    if last.as_ref() != Some(&target) {
        *streak = 0;
        *alerted = false;
        *last = Some(target.clone());
    }
    let h = tokio::task::spawn_blocking(resource_health).await?;
    let mut problems = health_problems(&h);
    if let Some(node) = music
        && node
            .request(reqwest::Method::GET, "/v4/info", None)
            .await
            .is_err()
    {
        problems.push("Lavalink unavailable");
    }
    if problems.is_empty() {
        *streak = 0;
    } else {
        *streak = streak.saturating_add(1);
    }
    let send = (*streak >= 3 && !*alerted) || (problems.is_empty() && *alerted);
    if send {
        let channel = destination(
            ctx,
            guild,
            s::UserId::new(row.get::<String, _>("actor").parse()?),
            s::ChannelId::new(target.parse()?),
            false,
        )
        .await?;
        crate::features::private_reports(ctx, guild, &channel).await?;
        channel
            .id
            .send_message(
                &ctx.http,
                s::CreateMessage::new()
                    .embed(embed(
                        if problems.is_empty() {
                            "SYSTEM RECOVERED"
                        } else {
                            "SYSTEM ALERT"
                        },
                        if problems.is_empty() {
                            "Resource and Lavalink checks are healthy again.".into()
                        } else {
                            problems.join("\n")
                        },
                    ))
                    .allowed_mentions(s::CreateAllowedMentions::new()),
            )
            .await?;
        *alerted = !problems.is_empty();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_and_alert_thresholds() {
        assert_eq!(
            memory("MemTotal: 2048000 kB\nMemAvailable: 1024000 kB"),
            (2048000, 1024000)
        );
        assert!(
            health_problems(&Health {
                total: 100,
                available: 16,
                disk: Some(84),
                load: Some(0.1)
            })
            .is_empty()
        );
        assert_eq!(
            health_problems(&Health {
                total: 100,
                available: 14,
                disk: Some(85),
                load: None
            })
            .len(),
            2
        );
    }
    #[tokio::test]
    async fn scheduled_claims_and_report_ownership() {
        let db = crate::store::connect("sqlite::memory:").await.unwrap();
        sqlx::query("INSERT INTO scheduled_posts(guild,actor,channel,due,text,created) VALUES('1','2','3',1,'test',1)").execute(&db).await.unwrap();
        let guild = s::GuildId::new(1);
        let channel = s::ChannelId::new(3);
        let owner = s::UserId::new(2);
        let other = s::UserId::new(5);
        assert!(
            claim_post(&db, s::GuildId::new(9), 1, 2)
                .await
                .unwrap()
                .is_none()
        );
        assert!(claim_post(&db, guild, 1, 0).await.unwrap().is_none());
        assert!(claim_post(&db, guild, 1, 2).await.unwrap().is_some());
        assert!(claim_post(&db, guild, 1, 2).await.unwrap().is_none());
        sqlx::query("INSERT INTO review_queue(report,guild,channel,message) VALUES(1,'1','3','4')")
            .execute(&db)
            .await
            .unwrap();
        assert!(
            review_change(&db, guild, channel, 1, owner, ReviewAction::Claim, None)
                .await
                .unwrap()
        );
        assert!(
            !review_change(&db, guild, channel, 1, other, ReviewAction::Claim, None)
                .await
                .unwrap()
        );
        assert!(
            !review_change(
                &db,
                guild,
                channel,
                1,
                other,
                ReviewAction::Resolve,
                Some("Rejected")
            )
            .await
            .unwrap()
        );
        assert!(
            !review_change(
                &db,
                s::GuildId::new(9),
                channel,
                1,
                owner,
                ReviewAction::Resolve,
                Some("Rejected")
            )
            .await
            .unwrap()
        );
        assert!(
            review_change(
                &db,
                guild,
                channel,
                1,
                owner,
                ReviewAction::Resolve,
                Some("Reviewed")
            )
            .await
            .unwrap()
        );
        assert!(
            review_change(&db, guild, channel, 1, other, ReviewAction::Reopen, None)
                .await
                .unwrap()
        );
    }
    #[tokio::test]
    async fn optout_prevents_voice_history() {
        let db = crate::store::connect("sqlite::memory:").await.unwrap();
        sqlx::query("INSERT INTO activity_optout VALUES('1','2')")
            .execute(&db)
            .await
            .unwrap();
        crate::daily::voice(&db, 1, 2, 600, now()).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM activity")
                .fetch_one(&db)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM daily_activity")
                .fetch_one(&db)
                .await
                .unwrap(),
            0
        );
    }
}
