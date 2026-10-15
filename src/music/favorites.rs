use super::*;
#[derive(Clone, Copy, poise::ChoiceParameter)]
pub(super) enum FavoriteAction {
    View,
    Play,
    Remove,
}
fn link(raw: &str) -> Result<String, Error> {
    let raw = raw.trim().trim_start_matches('<').trim_end_matches('>');
    if raw.chars().count() > 300 {
        return Err(crate::errors::user(
            "Use one supported music link, up to 300 characters.",
        ));
    }
    identifier(raw, Source::YouTube)?;
    let mut url = reqwest::Url::parse(raw).map_err(|_| {
        crate::errors::user("Post a supported HTTPS music link, not a search title.")
    })?;
    if url.scheme() != "https" {
        return Err(crate::errors::user("Use a supported HTTPS music link."));
    }
    url.set_fragment(None);
    let pairs = url
        .query_pairs()
        .filter(|(k, _)| !k.starts_with("utm_") && !matches!(k.as_ref(), "si" | "feature"))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect::<Vec<_>>();
    url.set_query(None);
    if !pairs.is_empty() {
        url.query_pairs_mut().extend_pairs(pairs);
    }
    let normalized: String = url.into();
    if normalized.len() > 300 {
        return Err(crate::errors::user(
            "Use a shorter canonical music link (up to 300 encoded characters).",
        ));
    }
    Ok(normalized)
}
async fn access(ctx: &s::Context, node: &Service, user: s::UserId) -> Result<s::ChannelId, Error> {
    let cfg = crate::store::settings(&node.db, node.guild.get()).await?;
    let id = cfg
        .music_favorite_channel
        .ok_or_else(|| crate::errors::user("Staff must configure MusicFavoriteChannel first."))?;
    let channel = s::ChannelId::new(id);
    let fresh = channel
        .to_channel(&ctx.http)
        .await?
        .guild()
        .ok_or_else(|| crate::errors::user("Favorites channel unavailable."))?;
    let guild = node.guild.to_partial_guild(&ctx.http).await?;
    let member = node.guild.member(&ctx.http, user).await?;
    if fresh.guild_id != node.guild
        || fresh.kind != s::ChannelType::Text
        || member.pending
        || member.user.bot
        || !guild
            .user_permissions_in(&fresh, &member)
            .contains(s::Permissions::VIEW_CHANNEL)
    {
        return Err(crate::errors::user(
            "You need access to the shared favorites channel and completed membership screening.",
        ));
    }
    Ok(channel)
}
async fn save(
    db: &sqlx::SqlitePool,
    guild: s::GuildId,
    user: s::UserId,
    url: &str,
) -> Result<bool, Error> {
    let n=sqlx::query("INSERT INTO music_favorites(guild,url,user,created) SELECT ?,?,?,? WHERE (SELECT COUNT(*) FROM music_favorites WHERE guild=?)<200 ON CONFLICT(guild,url) DO NOTHING").bind(guild.to_string()).bind(url).bind(user.to_string()).bind(s::Timestamp::now().unix_timestamp()).bind(guild.to_string()).execute(db).await?.rows_affected();
    if n == 1 {
        return Ok(true);
    }
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM music_favorites WHERE guild=? AND url=?)")
            .bind(guild.to_string())
            .bind(url)
            .fetch_one(db)
            .await?;
    if exists {
        Ok(false)
    } else {
        Err(crate::errors::user(
            "Shared favorites are full (200 links). Remove a saved link before adding another.",
        ))
    }
}
pub(super) async fn action(
    ctx: &s::Context,
    node: &Service,
    user: s::UserId,
    text: s::ChannelId,
    action: FavoriteAction,
    page: u16,
    url: Option<&str>,
) -> Result<String, Error> {
    access(ctx, node, user).await?;
    if !(1..=20).contains(&page) {
        return Err(crate::errors::user("Choose a favorites page from 1 to 20."));
    }
    if matches!(action, FavoriteAction::Remove) {
        let url =
            link(url.ok_or_else(|| crate::errors::user("Provide the saved link to remove."))?)?;
        let guild = node.guild.to_partial_guild(&ctx.http).await?;
        let member = node.guild.member(&ctx.http, user).await?;
        let staff = guild
            .member_permissions(&member)
            .contains(s::Permissions::MANAGE_GUILD);
        let n =
            sqlx::query("DELETE FROM music_favorites WHERE guild=? AND url=? AND (user=? OR ?)")
                .bind(node.guild.to_string())
                .bind(url)
                .bind(user.to_string())
                .bind(staff)
                .execute(&node.db)
                .await?
                .rows_affected();
        return if n == 1 {
            Ok("Removed the shared favorite.".into())
        } else {
            Err(crate::errors::user(
                "That saved link was not found, or belongs to another member. Staff can remove any saved link.",
            ))
        };
    }
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT url FROM music_favorites WHERE guild=? ORDER BY created,url LIMIT 10 OFFSET ?",
    )
    .bind(node.guild.to_string())
    .bind(i64::from(page - 1) * 10)
    .fetch_all(&node.db)
    .await?;
    if rows.is_empty() {
        return Ok(format!(
            "No saved links on page {page}. Post a supported music link in the favorites channel first."
        ));
    }
    if matches!(action, FavoriteAction::View) {
        return Ok(format!(
            "**Shared favorites / page {page}**\nUse `/playlist favorites` with Play and this page to queue these links.\n\n{}",
            rows.iter()
                .enumerate()
                .map(|(i, u)| format!("`{:02}` <{u}>", usize::from(page - 1) * 10 + i + 1))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    if !requests::limit(
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
            "Wait five seconds between music requests.",
        ));
    }
    let _guard = node.operations.lock().await;
    let voice = voice_channel(ctx, node.guild, user)
        .ok_or_else(|| crate::errors::user("Join voice before playing shared favorites."))?;
    if node
        .state
        .lock()
        .await
        .channel
        .is_some_and(|id| id != voice)
    {
        return Err(crate::errors::user("Join the bot's voice channel first."));
    }
    let cfg = crate::store::settings(&node.db, node.guild.get()).await?;
    if cfg.music_stay_channel.is_some_and(|id| id != voice.get()) {
        return Err(crate::errors::user(
            "Join the configured 24/7 voice channel first.",
        ));
    }
    let space = MAX_QUEUE.saturating_sub(node.state.lock().await.queue.len());
    if space == 0 {
        return Err(crate::errors::user("Queue is full."));
    }
    let tracks=tokio::time::timeout(Duration::from_secs(30),async{let mut tracks=Vec::new();for url in rows.iter().take(space){match node.load(url,Source::YouTube).await{Ok(found)=>if let Some(track)=found.into_iter().next(){tracks.push(track);},Err(_)=>continue}}tracks}).await.map_err(|_|crate::errors::user("Loading favorites timed out; no tracks were queued. Try another page or /play a single link."))?;
    enqueue_user(ctx, node, user, text, tracks).await
}
pub(super) async fn message(ctx: &s::Context, data: &Data, node: &Service, m: &s::Message) {
    if m.guild_id != Some(node.guild)
        || m.author.bot
        || m.webhook_id.is_some()
        || data.settings.read().await.music_favorite_channel != Some(m.channel_id.get())
        || std::env::var("ENABLE_MESSAGE_CONTENT").as_deref() != Ok("true")
    {
        return;
    }
    let result:Result<String,Error>=async{
        if !requests::limit(&node.db,node.guild,m.author.id,"favorite",s::Timestamp::now().unix_timestamp(),5).await?{return Ok(String::new());}
        if m.mention_everyone||!m.mentions.is_empty()||!m.mention_roles.is_empty()||!m.attachments.is_empty(){return Err(crate::errors::user("Post one supported music link without mentions or attachments."));}
        access(ctx,node,m.author.id).await?;let url=link(&m.content)?;
        Ok(if save(&node.db,node.guild,m.author.id,&url).await?{"Saved to shared favorites. Use the Play favorites button or /playlist favorites to play it later."}else{"This link is already in shared favorites."}.into())
    }.await;
    let text = match result {
        Ok(s) if s.is_empty() => return,
        Ok(s) => s,
        Err(e) => crate::errors::with_reference(e.as_ref(), "play", m.id.get()),
    };
    let _ = m
        .channel_id
        .send_message(
            &ctx.http,
            s::CreateMessage::new()
                .reference_message(m)
                .embed(
                    crate::emoji::embed_for("heart")
                        .title("ELVNX / SHARED FAVORITES")
                        .description(text),
                )
                .allowed_mentions(s::CreateAllowedMentions::new().replied_user(false)),
        )
        .await;
}
pub(super) async fn refresh(ctx: &s::Context, data: &Data) -> Result<(), Error> {
    let Some(raw) = data.settings.read().await.music_favorite_channel else {
        return Ok(());
    };
    let channel = s::ChannelId::new(raw);
    let fresh = channel
        .to_channel(&ctx.http)
        .await?
        .guild()
        .ok_or_else(|| crate::errors::user("Favorites channel unavailable."))?;
    if fresh.guild_id != data.guild || fresh.kind != s::ChannelType::Text {
        return Err(crate::errors::user(
            "Choose a server text channel for favorites.",
        ));
    }
    let row = sqlx::query("SELECT channel,message FROM music_favorite_panels WHERE guild=?")
        .bind(data.guild.to_string())
        .fetch_optional(&data.db)
        .await?;
    let embed=crate::emoji::embed_for("heart").title("ELVNX / PLAYLIST FAVORITES").description("**Save it now. Play it together later.**\n\nPost one supported HTTPS music link here to add it to the server's shared favorites. Saving a link does not start playback, and does not require joining voice.\n\nJoin voice and choose **Play favorites** to queue the first page (up to ten tracks), or use `/playlist favorites` to browse and play other pages. Links remain saved after playback.\n\n200 saved links maximum · duplicates ignored · five seconds between submissions.");
    let cover = crate::cards::render(
        "MUSIC / SAVED ROTATION",
        "Keep the good ones.",
        "Save a link. Share a session.",
        &[
            ("COLLECTION", "Shared".into()),
            ("CAPACITY", "200 links".into()),
            ("PLAYBACK", "10 per page".into()),
        ],
        None,
    )?;
    let embed = embed.image("attachment://playlist-favorites.png");
    let buttons = vec![s::CreateActionRow::Buttons(vec![
        crate::emoji::button_for("elvnx:music:play-favorites", "play")
            .label("Play favorites")
            .style(s::ButtonStyle::Primary),
        crate::emoji::button_for("elvnx:music:view-favorites", "heart")
            .label("Browse favorites")
            .style(s::ButtonStyle::Secondary),
    ])];
    let old = if let Some(row) = row
        && row.get::<String, _>("channel") == channel.to_string()
    {
        requests::board_message(
            ctx,
            channel,
            s::MessageId::new(row.get::<String, _>("message").parse()?),
        )
        .await?
    } else {
        None
    };
    let m = if let Some(old) = old {
        channel
            .edit_message(
                &ctx.http,
                old.id,
                s::EditMessage::new()
                    .embed(embed)
                    .components(buttons)
                    .attachments(
                        s::EditAttachments::new()
                            .add(s::CreateAttachment::bytes(cover, "playlist-favorites.png")),
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
                    .add_file(s::CreateAttachment::bytes(cover, "playlist-favorites.png"))
                    .allowed_mentions(s::CreateAllowedMentions::new()),
            )
            .await?
    };
    sqlx::query("INSERT INTO music_favorite_panels(guild,channel,message) VALUES(?,?,?) ON CONFLICT(guild) DO UPDATE SET channel=excluded.channel,message=excluded.message").bind(data.guild.to_string()).bind(channel.to_string()).bind(m.id.to_string()).execute(&data.db).await?;
    Ok(())
}
pub(super) async fn button(ctx: &s::Context, node: &Service, b: &s::ComponentInteraction) {
    let _ = b
        .create_response(
            &ctx.http,
            s::CreateInteractionResponse::Defer(
                s::CreateInteractionResponseMessage::new().ephemeral(true),
            ),
        )
        .await;
    let result:Result<String,Error>=async{
        let channel=access(ctx,node,b.user.id).await?;
        let bound:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM music_favorite_panels WHERE guild=? AND channel=? AND message=?)").bind(node.guild.to_string()).bind(b.channel_id.to_string()).bind(b.message.id.to_string()).fetch_one(&node.db).await?;
        if channel!=b.channel_id||!bound{return Err(crate::errors::user("This favorites panel is outdated."));}
        action(ctx,node,b.user.id,b.channel_id,if b.data.custom_id.ends_with("play-favorites"){FavoriteAction::Play}else{FavoriteAction::View},1,None).await
    }.await;
    let text =
        result.unwrap_or_else(|e| crate::errors::with_reference(e.as_ref(), "play", b.id.get()));
    let _ = b
        .edit_response(
            &ctx.http,
            s::EditInteractionResponse::new()
                .embed(
                    crate::emoji::embed_for("heart")
                        .title("ELVNX / SHARED FAVORITES")
                        .description(text),
                )
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await;
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn favorites_reject_searches_private_urls_and_strip_tracking() {
        assert!(link("a song").is_err());
        assert!(link(&format!("https://youtu.be/abc?q={}", "😀".repeat(70))).is_err());
        assert!(link("https://127.0.0.1/x").is_err());
        assert_eq!(
            link("https://youtu.be/abc?si=tracking&utm_source=foo").unwrap(),
            "https://youtu.be/abc"
        );
    }
    #[tokio::test]
    async fn favorites_are_unique_scoped_and_capped() {
        let db = crate::store::connect("sqlite::memory:").await.unwrap();
        let g = s::GuildId::new(1);
        let u = s::UserId::new(2);
        assert!(save(&db, g, u, "https://youtu.be/0").await.unwrap());
        assert!(!save(&db, g, u, "https://youtu.be/0").await.unwrap());
        for i in 1..200 {
            assert!(
                save(&db, g, u, &format!("https://youtu.be/{i}"))
                    .await
                    .unwrap()
            );
        }
        assert!(save(&db, g, u, "https://youtu.be/overflow").await.is_err());
        assert!(
            save(&db, s::GuildId::new(3), u, "https://youtu.be/overflow")
                .await
                .unwrap()
        );
    }
}
