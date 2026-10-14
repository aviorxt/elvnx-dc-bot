use crate::{Context, Error, cards, reply, serenity as s};

const RULES: &[(&str, &str)] = &[
    (
        "01 / Respect each other",
        "Be respectful. No harassment, hate speech, threats, bullying, slurs, or targeted personal attacks. Disagree with ideas without attacking people.",
    ),
    (
        "02 / Protect privacy",
        "Do not share personal information, private messages, screenshots, or recordings without permission. Ask before recording voice or sharing someone else's content outside the server.",
    ),
    (
        "03 / Keep the space safe",
        "No scams, phishing, malicious files, impersonation, or attempts to steal accounts. Never request passwords, authentication codes, bot tokens, or private access credentials.",
    ),
    (
        "04 / Keep content appropriate",
        "No explicit sexual content, graphic violence, abuse, or extremist promotion. Respect members' boundaries and stop when asked. Use spoiler warnings where appropriate.",
    ),
    (
        "05 / Use the right channels",
        "Keep conversations relevant to each channel. Follow category notices and role-based access. Do not leak private-category content, staff logs, submissions, or assets.",
    ),
    (
        "06 / No spam or unwanted promotion",
        "Avoid flooding, repetitive messages, mass mentions, unsolicited DMs, and disruptive reactions. Ask staff before advertising, recruiting, or promoting another community.",
    ),
    (
        "07 / Voice and gaming etiquette",
        "Keep volume reasonable. No microphone abuse, soundboard spam, disruptive music, or intentional interruption. Follow the session host. Muted or deafened members may be moved to DND when AFK automation is enabled.",
    ),
    (
        "08 / Share responsibly",
        "Share files, artwork, movies, streams, and other assets only when you have permission to share them. Do not distribute pirated material or claim someone else's work as your own.",
    ),
    (
        "09 / Respect server tools",
        "Use bot commands in appropriate channels. Do not exploit verification, evade moderation, bypass access restrictions, or disrupt counting and other community games. Follow marked channel notices.",
    ),
    (
        "10 / Report concerns privately",
        "Contact staff privately with relevant message links and context. Do not start public harassment or retaliation. Use support@11x.dpdns.org if you need another way to reach the community.",
    ),
    (
        "11 / Moderation and appeals",
        "Staff may remove content, warn, restrict access, timeout, kick, or ban based on severity and repeated behavior. Serious incidents may require immediate action. Appeal privately and respectfully; do not evade a restriction or harass staff.",
    ),
];

/// Post detailed server rules with a dynamically generated ELVNX cover image.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    default_member_permissions = "MANAGE_GUILD",
    user_cooldown = 10
)]
pub async fn rules(
    ctx: Context<'_>,
    #[description = "Text channel for the rules message"] channel: s::GuildChannel,
) -> Result<(), Error> {
    crate::commands::posting_channel(ctx, &channel).await?;
    let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
    let bot_id = ctx.serenity_context().cache.current_user().id;
    let bot = ctx.data().guild.member(ctx.http(), bot_id).await?;
    if !guild
        .user_permissions_in(&channel, &bot)
        .contains(s::Permissions::EMBED_LINKS | s::Permissions::ATTACH_FILES)
    {
        return Err("Grant the bot Embed Links and Attach Files in the rules channel".into());
    }
    let date = s::Timestamp::from_unix_timestamp(s::Timestamp::now().unix_timestamp() + 19_800)?
        .to_string()
        .chars()
        .take(10)
        .collect::<String>();
    let cover = cards::rules_cover(&guild.name, &date)?;
    let mut embed = s::CreateEmbed::new()
        .title(format!("{} / SERVER RULES", guild.name))
        .description("A private space to build, play, watch, and share. These standards apply across text, voice, gaming, movie, and asset channels. By participating, you agree to follow them.")
        .color(0x111111)
        .footer(s::CreateEmbedFooter::new("ELVNX / 11X · Read the rules. Respect the space. Contact staff if unsure."))
        .timestamp(s::Timestamp::now());
    for (title, text) in RULES {
        embed = embed.field(*title, *text, false);
    }
    let message = channel
        .id
        .send_message(
            ctx.http(),
            s::CreateMessage::new()
                .embeds(vec![
                    s::CreateEmbed::new()
                        .color(0x111111)
                        .image("attachment://rules-cover.png"),
                    embed,
                ])
                .add_file(s::CreateAttachment::bytes(cover, "rules-cover.png"))
                .allowed_mentions(s::CreateAllowedMentions::new()),
        )
        .await?;
    reply(ctx, format!("Rules posted: {}", message.link())).await
}
