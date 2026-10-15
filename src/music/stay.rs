use super::*;
pub(super) async fn disable(data: &Data) -> Result<(), Error> {
    let _guard = data.configuration_lock.lock().await;
    let mut cfg = data.settings.read().await.clone();
    if cfg.music_stay_channel.is_some() {
        cfg.music_stay_channel = None;
        crate::store::save(&data.db, data.guild.get(), &cfg).await?;
        *data.settings.write().await = cfg;
    }
    Ok(())
}
pub(super) async fn maintain(node: &Service, ctx: &s::Context) -> Result<bool, Error> {
    let cfg = crate::store::settings(&node.db, node.guild.get()).await?;
    let Some(raw) = cfg.music_stay_channel else {
        return Ok(false);
    };
    let channel = s::ChannelId::new(raw);
    let state = node.state.lock().await.clone();
    if let Some(current) = state.channel {
        if current == channel {
            let actual = ctx
                .cache
                .guild(node.guild)
                .map(|g| g.voice_states.get(&node.bot).and_then(|v| v.channel_id));
            let waited = node
                .stay_retry
                .lock()
                .await
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(60));
            if waited && (actual == Some(None) || !state.voice_ready) {
                node.stop(ctx).await?;
            }
            return Ok(true);
        }
        return Ok(false);
    }
    if node.session.lock().await.is_none() {
        return Ok(true);
    }
    {
        let mut retry = node.stay_retry.lock().await;
        if retry.is_some_and(|at| at.elapsed() < Duration::from_secs(60)) {
            return Ok(true);
        }
        *retry = Some(Instant::now());
    }
    let fresh = channel
        .to_channel(&ctx.http)
        .await?
        .guild()
        .ok_or_else(|| crate::errors::user("24/7 voice channel unavailable."))?;
    let guild = node.guild.to_partial_guild(&ctx.http).await?;
    let bot = node.guild.member(&ctx.http, node.bot).await?;
    if fresh.guild_id != node.guild
        || fresh.kind != s::ChannelType::Voice
        || !guild.user_permissions_in(&fresh, &bot).contains(
            s::Permissions::VIEW_CHANNEL | s::Permissions::CONNECT | s::Permissions::SPEAK,
        )
    {
        return Err(crate::errors::user(
            "24/7 mode needs a normal voice channel and bot View, Connect and Speak permissions.",
        ));
    }
    {
        let mut state = node.state.lock().await;
        state.channel = Some(channel);
        state.voice_ready = false;
        state.voice_session = None;
        state.voice_token = None;
        state.endpoint = None;
    }
    node.voice_state(ctx, Some(channel));
    Ok(true)
}
pub(super) async fn configure(
    ctx: Context<'_>,
    enabled: bool,
    channel: Option<s::GuildChannel>,
) -> Result<(), Error> {
    let node = service(ctx)?;
    let _op = node.operations.lock().await;
    if !enabled {
        disable(ctx.data()).await?;
        if node.state.lock().await.current.is_none() {
            node.stop(ctx.serenity_context()).await?;
        }
        return reply(ctx,"24/7 mode disabled. Current playback, if any, continues with normal empty-channel cleanup.").await;
    }
    let selected =
        channel.ok_or_else(|| crate::errors::user("Choose the voice channel for 24/7 mode."))?;
    let fresh = selected
        .id
        .to_channel(ctx.http())
        .await?
        .guild()
        .ok_or_else(|| crate::errors::user("Choose a server voice channel."))?;
    let guild = node.guild.to_partial_guild(ctx.http()).await?;
    let bot = node.guild.member(ctx.http(), node.bot).await?;
    let actor = node.guild.member(ctx.http(), ctx.author().id).await?;
    if fresh.guild_id != node.guild
        || fresh.kind != s::ChannelType::Voice
        || !guild.user_permissions_in(&fresh, &bot).contains(
            s::Permissions::VIEW_CHANNEL | s::Permissions::CONNECT | s::Permissions::SPEAK,
        )
        || !guild
            .user_permissions_in(&fresh, &actor)
            .contains(s::Permissions::VIEW_CHANNEL | s::Permissions::CONNECT)
    {
        return Err(crate::errors::user(
            "Choose an accessible normal voice channel where the bot has View Channel, Connect and Speak.",
        ));
    }
    if node
        .state
        .lock()
        .await
        .channel
        .is_some_and(|id| id != fresh.id)
    {
        return Err(crate::errors::user(
            "Finish playback in the other voice channel before enabling 24/7 here.",
        ));
    }
    {
        let _guard = ctx.data().configuration_lock.lock().await;
        let mut cfg = ctx.data().settings.read().await.clone();
        cfg.music_stay_channel = Some(fresh.id.get());
        crate::store::save(&ctx.data().db, node.guild.get(), &cfg).await?;
        *ctx.data().settings.write().await = cfg;
    }
    *node.stay_retry.lock().await = None;
    maintain(&node, ctx.serenity_context()).await?;
    reply(ctx,format!("24/7 mode enabled for <#{}>. The bot stays while idle and retries unexpected disconnects. Stop / leave disables this mode; server permissions and service availability still apply.",fresh.id)).await
}
