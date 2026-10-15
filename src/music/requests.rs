use super::*;
#[derive(poise::Modal)]
#[name = "ELVNX / Add a track"]
struct AddTrack {
    #[name = "Song title or supported HTTPS link"]
    #[min_length = 1]
    #[max_length = 300]
    query: String,
    #[name = "Search source: youtube or soundcloud"]
    #[max_length = 10]
    source: Option<String>,
}
fn source(value: Option<&str>) -> Result<Source, Error> {
    match value
        .unwrap_or("youtube")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "youtube" => Ok(Source::YouTube),
        "soundcloud" => Ok(Source::Soundcloud),
        _ => Err(crate::errors::user(
            "Choose youtube or soundcloud as the search source.",
        )),
    }
}
pub(super) async fn limit(
    db: &sqlx::SqlitePool,
    guild: s::GuildId,
    user: s::UserId,
    kind: &str,
    at: i64,
    seconds: i64,
) -> Result<bool, Error> {
    let accepted=sqlx::query_scalar::<_,i64>("INSERT INTO music_request_limits(guild,user,kind,last) VALUES(?,?,?,?) ON CONFLICT(guild,user,kind) DO UPDATE SET last=excluded.last WHERE music_request_limits.last<=? RETURNING last").bind(guild.to_string()).bind(user.to_string()).bind(kind).bind(at).bind(at-seconds).fetch_optional(db).await?.is_some();
    sqlx::query("DELETE FROM music_request_limits WHERE last<?")
        .bind(at - 86400)
        .execute(db)
        .await?;
    Ok(accepted)
}
pub(super) async fn request(
    ctx: &s::Context,
    node: &Service,
    user: s::UserId,
    text: s::ChannelId,
    query: &str,
    source: Source,
) -> Result<String, Error> {
    if !limit(
        &node.db,
        node.guild,
        user,
        "search",
        s::Timestamp::now().unix_timestamp(),
        5,
    )
    .await?
    {
        return Err(crate::errors::user(
            "Wait five seconds between music requests, including /play and Add track.",
        ));
    }
    let _guard = node.operations.lock().await;
    let cfg = crate::store::settings(&node.db, node.guild.get()).await?;
    let voice = voice_channel(ctx, node.guild, user)
        .ok_or_else(|| crate::errors::user("Join a voice channel before requesting a song."))?;
    if cfg.music_stay_channel.is_some_and(|id| id != voice.get()) {
        return Err(crate::errors::user(
            "Join the configured 24/7 voice channel before requesting music.",
        ));
    }
    if node
        .state
        .lock()
        .await
        .channel
        .is_some_and(|id| id != voice)
    {
        return Err(crate::errors::user(
            "Join the bot's voice channel before adding a track.",
        ));
    }
    if node.session.lock().await.is_none() {
        return Err(crate::errors::user(
            "The audio node is reconnecting. Try again shortly.",
        ));
    }
    if node.state.lock().await.queue.len() >= MAX_QUEUE {
        return Err(crate::errors::user(
            "Queue is full (50 tracks). Wait for space before requesting more.",
        ));
    }
    let guild = node.guild.to_partial_guild(&ctx.http).await?;
    let member = node.guild.member(&ctx.http, user).await?;
    let channel = text
        .to_channel(&ctx.http)
        .await?
        .guild()
        .ok_or_else(|| crate::errors::user("Use a server text channel."))?;
    if member.user.bot
        || member.pending
        || channel.guild_id != node.guild
        || !guild
            .user_permissions_in(&channel, &member)
            .contains(s::Permissions::VIEW_CHANNEL | s::Permissions::SEND_MESSAGES)
    {
        return Err(crate::errors::user(
            "You need access to the music channel and completed membership screening.",
        ));
    }
    let query = query.trim();
    let query = if query.starts_with("<https://") && query.ends_with('>') {
        &query[1..query.len() - 1]
    } else {
        query
    };
    let tracks = node.load(query, source).await?;
    enqueue_user(ctx, node, user, text, tracks).await
}
async fn bound(node: &Service, b: &s::ComponentInteraction) -> Result<bool, Error> {
    if b.message.author.id != node.bot || b.guild_id != Some(node.guild) {
        return Ok(false);
    }
    let state = node.state.lock().await;
    if state.panel == Some(b.message.id) && state.text == Some(b.channel_id) {
        return Ok(true);
    }
    drop(state);
    let cfg = crate::store::settings(&node.db, node.guild.get()).await?;
    if cfg.music_request_channel != Some(b.channel_id.get()) {
        return Ok(false);
    }
    Ok(sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM music_request_panels WHERE guild=? AND channel=? AND message=?)").bind(node.guild.to_string()).bind(b.channel_id.to_string()).bind(b.message.id.to_string()).fetch_one(&node.db).await?)
}
pub(super) async fn modal(
    ctx: &s::Context,
    _data: &Data,
    node: &Service,
    b: &s::ComponentInteraction,
) {
    use poise::Modal;
    let opening:Result<(),Error>=async{
        if !bound(node,b).await?{return Err(crate::errors::user("This music panel is outdated. Use the current player or request channel."));}
        if voice_channel(ctx,node.guild,b.user.id).is_none(){return Err(crate::errors::user("Join a voice channel first, then choose Add track."));}
        if !limit(&node.db,node.guild,b.user.id,"form",s::Timestamp::now().unix_timestamp(),5).await?{return Err(crate::errors::user("Wait five seconds before opening another search form."));}
        let id=format!("elvnx-add-track-{}",b.id);b.create_response(&ctx.http,AddTrack::create(None,id.clone())).await?;
        let user=b.user.id;let guild=node.guild;
        if let Some(m)=s::collector::ModalInteractionCollector::new(&ctx.shard).filter(move|m|m.data.custom_id==id&&m.user.id==user&&m.guild_id==Some(guild)).timeout(Duration::from_secs(120)).await{
            m.create_response(&ctx.http,s::CreateInteractionResponse::Defer(s::CreateInteractionResponseMessage::new().ephemeral(true))).await?;
            let result:Result<String,Error>=async{
                if !bound(node,b).await?{return Err(crate::errors::user("The player or request channel changed while the form was open. Open Add track again."));}
                let form=AddTrack::parse(m.data.clone()).map_err(|_|crate::errors::user("Enter a song title or supported link."))?;
                request(ctx,node,user,b.channel_id,&form.query,source(form.source.as_deref())?).await
            }.await;
            let text=result.unwrap_or_else(|e|crate::errors::with_reference(e.as_ref(),"play",m.id.get()));
            m.edit_response(&ctx.http,s::EditInteractionResponse::new().embed(crate::emoji::embed_for("headphones").title("ELVNX / TRACK REQUEST").description(text)).allowed_mentions(s::CreateAllowedMentions::new())).await?;
        }Ok(())
    }.await;
    if let Err(error) = opening {
        let text = crate::errors::with_reference(error.as_ref(), "play", b.id.get());
        let _ = b
            .create_response(
                &ctx.http,
                s::CreateInteractionResponse::Message(
                    s::CreateInteractionResponseMessage::new()
                        .ephemeral(true)
                        .embed(crate::emoji::embed_for("warning").description(text))
                        .allowed_mentions(s::CreateAllowedMentions::new()),
                ),
            )
            .await;
    }
}
async fn claim_message(db: &sqlx::SqlitePool, id: s::MessageId, at: i64) -> Result<bool, Error> {
    let claimed =
        sqlx::query("INSERT OR IGNORE INTO music_request_receipts(id,created) VALUES(?,?)")
            .bind(id.to_string())
            .bind(at)
            .execute(db)
            .await?
            .rows_affected()
            == 1;
    sqlx::query("DELETE FROM music_request_receipts WHERE created<?")
        .bind(at - 86400)
        .execute(db)
        .await?;
    Ok(claimed)
}
pub(super) async fn message(ctx: &s::Context, data: &Data, node: &Service, m: &s::Message) {
    let cfg = data.settings.read().await.clone();
    if m.guild_id != Some(node.guild)
        || m.author.bot
        || m.webhook_id.is_some()
        || cfg.music_request_channel != Some(m.channel_id.get())
        || std::env::var("ENABLE_MESSAGE_CONTENT").as_deref() != Ok("true")
    {
        return;
    }
    let result:Result<String,Error>=async{
        if !claim_message(&node.db,m.id,s::Timestamp::now().unix_timestamp()).await?{return Ok(String::new());}
        if m.mention_everyone || !m.mentions.is_empty() || !m.mention_roles.is_empty() || !m.attachments.is_empty(){return Err(crate::errors::user("Post one song title or supported HTTPS music link, without mentions or attachments."));}
        if cfg.automod && !data.rules.read().await.exempt_channels.contains(&m.channel_id.get()) {
            let rules=data.rules.read().await;
            if crate::events::violation(&m.content,0,false,&cfg).is_some() || rules.text_reason(&m.content,0).is_some(){return Err(crate::errors::user("This request does not meet the server's message rules."));}
        }
        request(ctx,node,m.author.id,m.channel_id,m.content.trim(),Source::YouTube).await
    }.await;
    let text = match result {
        Ok(text) if text.is_empty() => return,
        Ok(text) => text,
        Err(error) => {
            if !limit(
                &node.db,
                node.guild,
                m.author.id,
                "feedback",
                s::Timestamp::now().unix_timestamp(),
                10,
            )
            .await
            .unwrap_or(false)
            {
                return;
            }
            crate::errors::with_reference(error.as_ref(), "play", m.id.get())
        }
    };
    if let Err(error) = m
        .channel_id
        .send_message(
            &ctx.http,
            s::CreateMessage::new()
                .reference_message(m)
                .embed(
                    crate::emoji::embed_for("headphones")
                        .title("ELVNX / TRACK REQUEST")
                        .description(text),
                )
                .allowed_mentions(s::CreateAllowedMentions::new().replied_user(false)),
        )
        .await
    {
        tracing::warn!(%error,"Music request response failed");
    }
}
pub(super) async fn board_message(
    ctx: &s::Context,
    channel: s::ChannelId,
    id: s::MessageId,
) -> Result<Option<s::Message>, Error> {
    match channel.message(&ctx.http, id).await {
        Ok(m) if m.author.id == ctx.cache.current_user().id => Ok(Some(m)),
        Ok(_) => Err(crate::errors::user(
            "The stored music board is not owned by this bot.",
        )),
        Err(s::Error::Http(error)) if error.status_code().is_some_and(|s| s.as_u16() == 404) => {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}
pub(super) async fn refresh(ctx: &s::Context, data: &Data) -> Result<(), Error> {
    let cfg = data.settings.read().await.clone();
    let Some(channel) = cfg.music_request_channel else {
        return Ok(());
    };
    let channel = s::ChannelId::new(channel);
    let fresh = channel
        .to_channel(&ctx.http)
        .await?
        .guild()
        .ok_or_else(|| crate::errors::user("Request channel unavailable."))?;
    if fresh.guild_id != data.guild || fresh.kind != s::ChannelType::Text {
        return Err(crate::errors::user("Choose a server text channel."));
    }
    let row = sqlx::query("SELECT channel,message FROM music_request_panels WHERE guild=?")
        .bind(data.guild.to_string())
        .fetch_optional(&data.db)
        .await?;
    let embed=crate::emoji::embed_for("headphones").title("ELVNX / MUSIC REQUESTS").description("**A shared soundtrack starts here.**\n\nJoin a voice channel, then post a song title or supported HTTPS music link. If the bot is already playing, join its voice channel first.\n\nUse **Add track** for a private search form. Requests queue normally; existing playback is preserved. Five seconds between requests. Attachments, mentions and unsupported URLs are not accepted.\n\nSession recaps appear when the queue finishes or playback stops.").image("attachment://music-requests.png");
    let cover = crate::cards::render(
        "MUSIC / REQUEST DESK",
        "Pass the soundtrack.",
        "Join voice. Pick a track. Make it a session.",
        &[
            ("REQUESTS", "Title or link".into()),
            ("LIMIT", "5 seconds".into()),
            ("QUEUE", "50 tracks".into()),
        ],
        None,
    )?;
    let buttons = vec![s::CreateActionRow::Buttons(vec![
        crate::emoji::button_for("elvnx:music:add", "plus")
            .label("Add track")
            .style(s::ButtonStyle::Primary),
    ])];
    let old = if let Some(row) = row
        && row.get::<String, _>("channel") == channel.to_string()
    {
        board_message(
            ctx,
            channel,
            s::MessageId::new(row.get::<String, _>("message").parse()?),
        )
        .await?
    } else {
        None
    };
    let message = if let Some(old) = old {
        channel
            .edit_message(
                &ctx.http,
                old.id,
                s::EditMessage::new()
                    .embed(embed)
                    .components(buttons)
                    .attachments(
                        s::EditAttachments::new()
                            .add(s::CreateAttachment::bytes(cover, "music-requests.png")),
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
                    .components(buttons)
                    .add_file(s::CreateAttachment::bytes(cover, "music-requests.png"))
                    .allowed_mentions(s::CreateAllowedMentions::new()),
            )
            .await?
    };
    sqlx::query("INSERT INTO music_request_panels(guild,channel,message) VALUES(?,?,?) ON CONFLICT(guild) DO UPDATE SET channel=excluded.channel,message=excluded.message").bind(data.guild.to_string()).bind(channel.to_string()).bind(message.id.to_string()).execute(&data.db).await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn search_limit_is_shared_persistent_and_scoped() {
        let db = crate::store::connect("sqlite::memory:").await.unwrap();
        let g = s::GuildId::new(1);
        let u = s::UserId::new(2);
        assert!(limit(&db, g, u, "search", 100, 5).await.unwrap());
        assert!(!limit(&db, g, u, "search", 104, 5).await.unwrap());
        assert!(limit(&db, g, u, "search", 105, 5).await.unwrap());
        assert!(
            limit(&db, s::GuildId::new(3), u, "search", 105, 5)
                .await
                .unwrap()
        );
    }
    #[tokio::test]
    async fn request_messages_are_claimed_once_without_storing_content() {
        let db = crate::store::connect("sqlite::memory:").await.unwrap();
        let id = s::MessageId::new(1);
        assert!(claim_message(&db, id, 100).await.unwrap());
        assert!(!claim_message(&db, id, 110).await.unwrap());
        assert!(claim_message(&db, s::MessageId::new(2), 120).await.unwrap());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM music_request_receipts")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count, 2);
    }
    #[test]
    fn modal_sources_are_explicit_and_do_not_accept_arbitrary_prefixes() {
        assert!(matches!(source(None).unwrap(), Source::YouTube));
        assert!(matches!(
            source(Some("SoundCloud")).unwrap(),
            Source::Soundcloud
        ));
        assert!(source(Some("file")).is_err());
    }
}
