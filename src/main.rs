mod afk;
mod always_react;
mod automod;
mod availability;
mod cards;
mod commands;
mod community;
mod daily;
mod eventlog;
mod events;
mod extras;
mod features;
mod legacy;
mod live_status;
mod music;
mod rules;
mod security;
mod sticky;
mod store;

use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, RwLock};
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;
fn bot_id(ctx: Context<'_>) -> serenity::UserId {
    ctx.serenity_context().cache.current_user().id
}

struct Data {
    db: SqlitePool,
    guild: serenity::GuildId,
    settings: RwLock<store::Settings>,
    configuration_lock: Mutex<()>,
    spam: Mutex<HashMap<u64, Vec<std::time::Instant>>>,
    started: std::time::Instant,
    stickies: Arc<sticky::Service>,
    voices: Arc<Mutex<HashMap<u64, std::time::Instant>>>,
    nuke: Mutex<HashMap<u64, Vec<std::time::Instant>>>,
    rules: RwLock<automod::Rules>,
    event_history: Mutex<eventlog::Cache>,
    music: Option<Arc<music::Service>>,
    afk: Arc<afk::Service>,
    live_status: Arc<live_status::Service>,
    availability: Arc<availability::Service>,
}

async fn reply(ctx: Context<'_>, text: impl Into<String>) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default()
            .embed(
                serenity::CreateEmbed::new()
                    .title("ELVNX / 11X")
                    .description(text.into())
                    .color(0x111111)
                    .footer(serenity::CreateEmbedFooter::new(
                        "avrxt.dev • private collective",
                    )),
            )
            .ephemeral(true)
            .allowed_mentions(serenity::CreateAllowedMentions::new()),
    )
    .await?;
    Ok(())
}

async fn log(http: &serenity::Http, data: &Data, text: String) {
    let channel = data.settings.read().await.log_channel;
    if let Some(channel) = channel {
        let embed = serenity::CreateEmbed::new()
            .title("ELVNX • moderation log")
            .description(text)
            .color(0x111111)
            .timestamp(serenity::Timestamp::now());
        if let Err(error) = serenity::ChannelId::new(channel)
            .send_message(
                http,
                serenity::CreateMessage::new()
                    .embed(embed)
                    .allowed_mentions(serenity::CreateAllowedMentions::new()),
            )
            .await
        {
            tracing::warn!(%error,"Unable to deliver moderation log");
        }
    }
}

async fn audit(ctx: Context<'_>, target: u64, action: &str, reason: &str) -> Result<(), Error> {
    let id = store::case(
        &ctx.data().db,
        ctx.data().guild.get(),
        target,
        ctx.author().id.get(),
        action,
        reason,
    )
    .await;
    match id {
        Ok(id) => {
            log(
                ctx.http(),
                ctx.data(),
                format!(
                    "Case #{id} • {action}\nMember: {target}\nModerator: {}\nReason: {reason}",
                    ctx.author().id
                ),
            )
            .await;
            reply(ctx, format!("Completed {action}. Case #{id}.")).await
        }
        Err(error) => {
            tracing::error!(%error,"Moderation completed but case persistence failed");
            reply(ctx,"Discord action completed, but its case could not be saved. Check the database before continuing.").await
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "elvnx_bot=info,warn".into()),
        )
        .init();
    if std::env::args().any(|a| a == "--version") {
        println!(
            "ELVNX v{} / build {} / developed and maintained by avrxt.dev",
            env!("CARGO_PKG_VERSION"),
            env!("ELVNX_BUILD_NUMBER")
        );
        return Ok(());
    }
    std::fs::create_dir_all("data")?;
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://data/elvnx.db".into());
    let db = store::connect(&url).await?;
    if std::env::args().any(|a| a == "--preview") {
        std::fs::create_dir_all("previews")?;
        std::fs::write(
            "previews/availability.png",
            cards::render(
                "WHO'S AROUND?",
                "ELVNX-PVT",
                "Online • Offline • Gaming • On VC",
                &[
                    ("ONLINE", "12".into()),
                    ("OFFLINE", "8".into()),
                    ("GAMING", "4".into()),
                    ("ON VC", "6".into()),
                ],
                None,
            )?,
        )?;
        std::fs::write(
            "previews/birthday.png",
            cards::render(
                "HAPPY BIRTHDAY",
                "aviorxt",
                "Another chapter. More good memories.",
                &[
                    ("BIRTHDAY", "14/10".into()),
                    ("COMMUNITY", "ELVNX / 11X".into()),
                ],
                None,
            )?,
        )?;
        std::fs::write(
            "previews/poll-results.png",
            cards::render(
                "POLL RESULTS",
                "Next community night?",
                "One member. One voice.",
                &[
                    ("MOVIE NIGHT", "18 votes".into()),
                    ("GAME NIGHT", "12 votes".into()),
                    ("TOTAL VOTES", "30".into()),
                ],
                None,
            )?,
        )?;
        std::fs::write(
            "previews/rules-cover.png",
            cards::rules_cover("ELVNX-PVT", "2026-10-14")?,
        )?;
        std::fs::write(
            "previews/welcome.png",
            cards::render(
                "WELCOME TO ELVNX",
                "aviorxt",
                "A private collective. Build. Explore. Connect.",
                &[],
                None,
            )?,
        )?;
        std::fs::write(
            "previews/insight.png",
            cards::render(
                "ACTIVITY INSIGHT",
                "aviorxt",
                "Message totals • connected voice time",
                &[
                    ("MESSAGES", "1,284".into()),
                    ("VOICE CONNECTED", "24h 18m".into()),
                    ("TRACKING", "Since enabled".into()),
                    ("#1 MESSAGE LEADER", "aviorxt / 1284".into()),
                    ("#2 MESSAGE LEADER", "member / 942".into()),
                    ("#3 MESSAGE LEADER", "friend / 721".into()),
                ],
                None,
            )?,
        )?;
        tracing::info!("Rendered local sample cards; preview statistics are illustrative");
        std::fs::write(
            "previews/daily-insight.png",
            cards::render(
                "DAILY INSIGHT",
                "ELVNX · 2026-10-14",
                "Today so far · 00:00–23:50 IST · observed activity",
                &[
                    ("MESSAGES", "1,842".into()),
                    ("VOICE CONNECTED", "32h 17m".into()),
                    ("ACTIVE MEMBERS", "28".into()),
                    ("#1 MESSAGE LEADER", "aviorxt / 428".into()),
                    ("#2 MESSAGE LEADER", "member / 312".into()),
                    ("#3 MESSAGE LEADER", "friend / 189".into()),
                ],
                None,
            )?,
        )?;
        return Ok(());
    }
    if std::env::args().any(|a| a == "--check") {
        tracing::info!(
            "Database schema and local configuration storage ready; no Discord connection made"
        );
        return Ok(());
    }
    let token = std::env::var("DISCORD_TOKEN")
        .map_err(|_| "Set DISCORD_TOKEN in the ignored .env file.")?;
    let guild = serenity::GuildId::new(
        std::env::var("DISCORD_GUILD_ID")
            .map_err(|_| "Set DISCORD_GUILD_ID.")?
            .parse()?,
    );
    let settings = store::settings(&db, guild.get()).await?;
    let stickies = sticky::Service::load(db.clone(), guild).await?;
    let mut intents = serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::GUILD_MODERATION
        | serenity::GatewayIntents::GUILD_VOICE_STATES;
    if std::env::var("ENABLE_MESSAGE_CONTENT").as_deref() == Ok("true") {
        intents |= serenity::GatewayIntents::MESSAGE_CONTENT;
    }
    if std::env::var("ENABLE_MEMBER_EVENTS").as_deref() == Ok("true") {
        intents |= serenity::GatewayIntents::GUILD_MEMBERS;
    }
    if std::env::var("ENABLE_PRESENCE_EVENTS").as_deref() == Ok("true") {
        intents |= serenity::GatewayIntents::GUILD_PRESENCES;
    }
    let framework=poise::Framework::builder().options(poise::FrameworkOptions {
        commands: commands::all(),
        pre_command: |ctx| Box::pin(async move {
            if ctx.command().name == "report" { return; }
            if let Err(error) = ctx.defer_ephemeral().await {
                tracing::warn!(%error, "Could not defer command response");
            }
        }),
        command_check:Some(|ctx|Box::pin(async move { Ok(ctx.guild_id()==Some(ctx.data().guild)) })),
        on_error:|error|Box::pin(async move {
            match error {
                poise::FrameworkError::Command {error,ctx,..} => {
                    tracing::warn!(%error,command=%ctx.command().name,"Command failed");
                    let _=reply(ctx,"Request failed. Check your permissions, role hierarchy, and command inputs. A moderator can inspect the bot's local logs.").await;
                }
                other=>{let _=poise::builtins::on_error(other).await;}
            }
        }),
        event_handler:|framework,event|Box::pin(events::handle(framework.serenity_context,event,framework,framework.user_data)),
        ..Default::default()
    }).setup(move|ctx,ready,framework|Box::pin(async move {
        if std::env::var("REGISTER_COMMANDS").as_deref()==Ok("true") {
            poise::builtins::register_in_guild(ctx,&framework.options().commands,guild).await?;
            // This bot is guild-only; remove obsolete global registrations from earlier versions.
            serenity::Command::set_global_commands(ctx,Vec::new()).await?;
        }
        tracing::info!(bot=%ready.user.name,guild=%guild,commands=music::COMMAND_COUNT,"ELVNX connected");
        let music=music::Service::start(ctx.clone(),db.clone(),guild,ready.user.id)?;
        let afk = afk::Service::start(ctx.clone(), db.clone(), guild, settings.afk_move_channel);
        let started = std::time::Instant::now();
        let live_status = live_status::Service::start(ctx.clone(), db.clone(), guild, Arc::clone(framework.shard_manager()), music.clone(), started).await?;
        let availability=availability::Service::start(ctx.clone(),db.clone(),guild).await?;
        let data=Data {db,guild,settings:RwLock::new(settings),configuration_lock:Mutex::new(()),spam:Mutex::new(HashMap::new()),started,stickies,voices:Arc::new(Mutex::new(HashMap::new())),nuke:Mutex::new(HashMap::new()),rules:RwLock::new(automod::Rules::load()?),event_history:Mutex::new(eventlog::Cache::default()),music,afk,live_status,availability};
        extras::presence(ctx,&*data.settings.read().await);
        extras::start_scheduler(ctx.http.clone(),data.db.clone(),guild,data.voices.clone());
        features::start(ctx.clone(),data.db.clone(),guild);
        Ok(data)
    })).build();
    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await?;
    let manager = Arc::clone(&client.shard_manager);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            manager.shutdown_all().await;
        }
    });
    client.start().await?;
    Ok(())
}
