use crate::{Context, Data, Error, cards, emoji, reply, security, serenity as s};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::time::Duration;

#[derive(Clone, Copy, Debug, poise::ChoiceParameter)]
pub enum Action {
    Setup,
    Status,
    Approve,
    Deny,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Policy {
    role: u64,
    channel: Option<u64>,
    message: Option<u64>,
    review: Option<u64>,
    account_days: u16,
    wait_minutes: u16,
    revision: String,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            role: 0,
            channel: None,
            message: None,
            review: None,
            account_days: 7,
            wait_minutes: 10,
            revision: "legacy".into(),
        }
    }
}
fn now() -> i64 {
    s::Timestamp::now().unix_timestamp()
}
fn embed(title: &str, text: impl Into<String>) -> s::CreateEmbed {
    emoji::embed_for("shield")
        .title(format!("ELVNX / {title}"))
        .description(text)
        .color(0x111111)
        .footer(s::CreateEmbedFooter::new(
            "11X • account screening, not proof of identity",
        ))
}
async fn policy(db: &SqlitePool, data: &Data) -> Result<Policy, Error> {
    let raw: Option<String> =
        sqlx::query_scalar("SELECT value FROM verification_policy WHERE guild=?")
            .bind(data.guild.to_string())
            .fetch_optional(db)
            .await?;
    match raw {
        Some(raw) => Ok(serde_json::from_str(&raw)?),
        None => Ok(Policy {
            role: data.settings.read().await.verification_role.unwrap_or(0),
            review: data.settings.read().await.report_channel,
            ..Default::default()
        }),
    }
}
pub async fn reserved(data: &Data, role: u64) -> Result<bool, Error> {
    Ok(data.settings.read().await.verification_role == Some(role)
        || policy(&data.db, data).await?.role == role)
}
pub async fn refresh_panel(ctx: &s::Context, data: &Data) -> Result<(), Error> {
    let p = policy(&data.db, data).await?;
    if let (Some(channel), Some(message)) = (p.channel, p.message) {
        let channel = s::ChannelId::new(channel);
        let message = s::MessageId::new(message);
        let old = channel.message(&ctx.http, message).await?;
        if old.author.id != ctx.cache.current_user().id {
            return Err(crate::errors::user(
                "Verification panel ownership changed. Recreate it with /verify Setup.",
            ));
        }
        if let Some(e) = old.embeds.into_iter().next() {
            let mut embed =
                s::CreateEmbed::from(e).author(s::CreateEmbedAuthor::new("ELVNX / 11X").icon_url(
                    emoji::url("shield").ok_or_else(|| {
                        crate::errors::user("Verification custom icon unavailable")
                    })?,
                ));
            embed = embed.timestamp(s::Timestamp::now());
            channel
                .edit_message(
                    &ctx.http,
                    message,
                    s::EditMessage::new()
                        .embed(embed)
                        .components(vec![s::CreateActionRow::Buttons(vec![
                            emoji::button(format!("elvnx:verify:{}", p.role))
                                .label("Verify access")
                                .style(s::ButtonStyle::Success),
                            emoji::button_for("elvnx:verification:help", "review")
                                .label("Staff help")
                                .style(s::ButtonStyle::Secondary),
                        ])])
                        .allowed_mentions(s::CreateAllowedMentions::new()),
                )
                .await?;
        }
    }
    Ok(())
}
fn joined(member: &s::Member) -> Result<i64, Error> {
    member.joined_at.map(|t| t.unix_timestamp()).ok_or_else(|| {
        crate::errors::user(
            "Your server join time could not be checked. Try again shortly or ask staff for help.",
        )
    })
}
fn eligibility(created: i64, stamp: i64, p: &Policy, rejoins: i64, burst: i64) -> Vec<String> {
    let mut risk = Vec::new();
    let age = (stamp - created).max(0);
    if age < i64::from(p.account_days) * 86400 {
        risk.push(format!(
            "Account age {} days is below the {}-day review threshold",
            age / 86400,
            p.account_days
        ));
    }
    if rejoins >= 3 {
        risk.push(format!(
            "Repeated joins: {rejoins} in the observed 24-hour window"
        ));
    }
    if burst >= 8 {
        risk.push("Join surge: at least 8 observed arrivals in two minutes".into());
    }
    risk
}
async fn member(
    ctx: &s::Context,
    data: &Data,
    p: &Policy,
    user: s::UserId,
) -> Result<s::Member, Error> {
    if p.role == 0 || data.settings.read().await.verification_role != Some(p.role) {
        return Err(crate::errors::user(
            "Verification is not configured or its policy changed. Ask staff to run /verify Setup.",
        ));
    }
    let guild = data.guild.to_partial_guild(&ctx.http).await?;
    let role = guild.roles.get(&s::RoleId::new(p.role)).ok_or_else(|| {
        crate::errors::user("The verification role was deleted. Staff must update /verify Setup.")
    })?;
    let bot_id = ctx.cache.current_user().id;
    let bot = data.guild.member(&ctx.http, bot_id).await?;
    if role.id.get() == data.guild.get()
        || role.managed
        || !role.permissions.is_empty()
        || !guild
            .member_permissions(&bot)
            .contains(s::Permissions::MANAGE_ROLES)
        || !security::higher(
            security::highest(&guild, &bot),
            (i64::from(role.position), role.id.get()),
        )
    {
        return Err(crate::errors::user(
            "The verification role is unsafe or above the bot. Staff must restore a zero-permission role below the bot's highest role.",
        ));
    }
    let member = data.guild.member(&ctx.http, user).await?;
    if member.user.bot {
        return Err(crate::errors::user(
            "Verification is available to human server members only.",
        ));
    }
    if member.pending {
        return Err(crate::errors::user(
            "Complete Discord's native server rules/membership screening first, then return to this verification panel.",
        ));
    }
    Ok(member)
}
fn wait(member: &s::Member, p: &Policy) -> Result<(), Error> {
    let left = joined(member)? + i64::from(p.wait_minutes) * 60 - now();
    if left > 0 {
        return Err(crate::errors::user(format!(
            "Please wait {} more minute(s) after joining before verifying. This join delay helps protect the server from raids.",
            (left + 59) / 60
        )));
    }
    Ok(())
}
async fn risk(
    db: &SqlitePool,
    data: &Data,
    p: &Policy,
    m: &s::Member,
) -> Result<Vec<String>, Error> {
    let rejoins: Option<i64> = sqlx::query_scalar(
        "SELECT count FROM verification_joins WHERE guild=? AND user=? AND window>=?",
    )
    .bind(data.guild.to_string())
    .bind(m.user.id.to_string())
    .bind(now() - 86400)
    .fetch_optional(db)
    .await?;
    let burst: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM verification_joins WHERE guild=? AND joined>=?")
            .bind(data.guild.to_string())
            .bind(now() - 120)
            .fetch_one(db)
            .await?;
    Ok(eligibility(
        m.user.id.created_at().unix_timestamp(),
        now(),
        p,
        rejoins.unwrap_or(0),
        burst,
    ))
}
fn digest(nonce: &str, code: &str) -> Vec<u8> {
    Sha256::digest(format!("{nonce}:{}", code.trim().to_ascii_uppercase())).to_vec()
}
fn challenge() -> (String, String) {
    let mut rng = rand::thread_rng();
    let alphabet = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let code = (0..6)
        .map(|_| alphabet[rng.gen_range(0..alphabet.len())] as char)
        .collect();
    (format!("{:032x}", rng.r#gen::<u128>()), code)
}
async fn quota(
    db: &SqlitePool,
    guild: s::GuildId,
    user: s::UserId,
    time: i64,
) -> Result<bool, Error> {
    let bucket = time.div_euclid(600);
    Ok(sqlx::query("INSERT INTO verification_attempts(guild,user,window,count) VALUES(?,?,?,1) ON CONFLICT(guild,user) DO UPDATE SET count=CASE WHEN window=excluded.window THEN count+1 ELSE 1 END,window=excluded.window WHERE window!=excluded.window OR count<5")
        .bind(guild.to_string()).bind(user.to_string()).bind(bucket).execute(db).await?.rows_affected()==1)
}
async fn staff(ctx: &s::Context, data: &Data, p: &Policy, user: s::UserId) -> Result<(), Error> {
    if p.role == 0 {
        return Err(crate::errors::user(
            "Configure verification first using /verify Setup.",
        ));
    }
    let guild = data.guild.to_partial_guild(&ctx.http).await?;
    let actor = data.guild.member(&ctx.http, user).await?;
    if actor.user.bot
        || !guild
            .member_permissions(&actor)
            .contains(s::Permissions::MANAGE_GUILD | s::Permissions::MANAGE_ROLES)
    {
        return Err(crate::errors::user(
            "Verification decisions require Manage Server and Manage Roles.",
        ));
    }
    let role = guild
        .roles
        .get(&s::RoleId::new(p.role))
        .ok_or_else(|| crate::errors::user("Verification role removed"))?;
    if actor.user.id != guild.owner_id
        && !security::higher(
            security::highest(&guild, &actor),
            (i64::from(role.position), role.id.get()),
        )
    {
        return Err(crate::errors::user(
            "Your highest role must be above the verification role.",
        ));
    }
    if let Some(id) = p.review {
        let channel = s::ChannelId::new(id)
            .to_channel(&ctx.http)
            .await?
            .guild()
            .ok_or_else(|| crate::errors::user("Staff review channel unavailable."))?;
        if channel.guild_id != data.guild
            || !guild
                .user_permissions_in(&channel, &actor)
                .contains(s::Permissions::VIEW_CHANNEL | s::Permissions::READ_MESSAGE_HISTORY)
        {
            return Err(crate::errors::user(
                "You need View Channel and Read Message History in the private verification review channel.",
            ));
        }
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub async fn command(
    ctx: Context<'_>,
    action: Action,
    channel: Option<s::GuildChannel>,
    role: Option<s::Role>,
    confirm: bool,
    days: Option<u16>,
    minutes: Option<u16>,
    review: Option<s::GuildChannel>,
    target: Option<s::Member>,
    reason: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    match action {
        Action::Setup => {
            if std::env::var("ENABLE_MEMBER_EVENTS").as_deref() != Ok("true") {
                return Err(crate::errors::user(
                    "Enable the Server Members intent and ENABLE_MEMBER_EVENTS=true before setting up join/rejoin screening.",
                ));
            }
            if !confirm {
                return reply(ctx,"Use confirm_access:true to approve the whitelist role's channel access for eligible members. Flagged accounts will require a staff decision.").await;
            }
            let _guard = ctx.data().configuration_lock.lock().await;
            let role = role
                .ok_or_else(|| crate::errors::user("Choose the zero-permission whitelist role."))?;
            security::safe_role(ctx, &role).await?;
            if !role.permissions.is_empty() {
                return Err(crate::errors::user(
                    "The verification role must have no global permissions.",
                ));
            }
            let channel = channel
                .ok_or_else(|| crate::errors::user("Choose the verification panel channel."))?;
            crate::commands::posting_channel(ctx, &channel).await?;
            let guild = ctx.data().guild.to_partial_guild(ctx.http()).await?;
            let bot = guild.id.member(ctx.http(), crate::bot_id(ctx)).await?;
            if !guild
                .user_permissions_in(&channel, &bot)
                .contains(s::Permissions::EMBED_LINKS | s::Permissions::ATTACH_FILES)
            {
                return Err(crate::errors::user(
                    "The bot needs Embed Links and Attach Files in the verification channel.",
                ));
            }
            let mut cfg = ctx.data().settings.read().await.clone();
            if cfg.autorole == Some(role.id.get()) {
                return Err(crate::errors::user(
                    "Remove this role from /configure autorole before using it for verification; otherwise members bypass screening.",
                ));
            }
            let account_days = days.unwrap_or(7);
            let wait_minutes = minutes.unwrap_or(10);
            if !(1..=365).contains(&account_days) || wait_minutes > 1440 {
                return Err(crate::errors::user(
                    "Account-age threshold must be 1–365 days; join wait must be 0–1440 minutes.",
                ));
            }
            let review_id = review.as_ref().map(|c| c.id.get()).or(cfg.report_channel);
            let review_channel = s::ChannelId::new(review_id.ok_or_else(|| {
                crate::errors::user("Choose a private review_channel or run /setup-report first.")
            })?)
            .to_channel(ctx.http())
            .await?
            .guild()
            .ok_or_else(|| {
                crate::errors::user("Choose a private server text channel for staff review.")
            })?;
            if review_channel.guild_id != ctx.data().guild
                || review_channel.kind != s::ChannelType::Text
            {
                return Err(crate::errors::user(
                    "Choose a private text channel in this server for staff review.",
                ));
            }
            crate::commands::posting_channel(ctx, &review_channel).await?;
            crate::features::private_reports(
                ctx.serenity_context(),
                ctx.data().guild,
                &review_channel,
            )
            .await?;
            if !guild
                .user_permissions_in(&review_channel, &bot)
                .contains(s::Permissions::EMBED_LINKS)
            {
                return Err(crate::errors::user(
                    "The bot needs Embed Links in the staff review channel.",
                ));
            }
            let png = cards::render(
                "ACCESS / VERIFIED",
                "ELVNX / 11X",
                "Read the rules. Complete your access check.",
                &[
                    ("ACCOUNT", format!("{account_days}+ days")),
                    ("JOIN WAIT", format!("{wait_minutes} min")),
                    ("PROTECTION", "Code + review".into()),
                ],
                None,
            )?;
            let sent=channel.id.send_message(ctx.http(),s::CreateMessage::new().embed(embed("VERIFICATION",format!("{} **Welcome to ELVNX.** Read the server rules, then verify.\n\nA one-time visual code and fresh membership checks protect access. Accounts younger than **{account_days} days**, repeated rejoins and join surges go to staff review. New members wait **{wait_minutes} minutes**.\n\nScreening flags are not proof of an alt account. No IP, email, password or phone is requested. Need an accessible alternative? Choose **Staff help**.\n\nTemporary challenges expire after two minutes; security review records are retained for up to 30 days.",emoji::text("shield"))).image("attachment://verification.png"))
                .add_file(s::CreateAttachment::bytes(png,"verification.png")).components(vec![s::CreateActionRow::Buttons(vec![emoji::button(format!("elvnx:verify:{}",role.id)).label("Verify access").style(s::ButtonStyle::Success),emoji::button_for("elvnx:verification:help","review").label("Staff help").style(s::ButtonStyle::Secondary)])]).allowed_mentions(s::CreateAllowedMentions::new())).await?;
            let policy = Policy {
                role: role.id.get(),
                channel: Some(channel.id.get()),
                message: Some(sent.id.get()),
                review: review_id,
                account_days,
                wait_minutes,
                revision: format!("{:032x}", rand::random::<u128>()),
            };
            cfg.verification_role = Some(role.id.get());
            cfg.self_roles.retain(|id| *id != role.id.get());
            let saved:Result<(),Error>=async{let mut tx=ctx.data().db.begin().await?;sqlx::query("INSERT INTO verification_policy VALUES(?,?) ON CONFLICT(guild) DO UPDATE SET value=excluded.value").bind(ctx.data().guild.to_string()).bind(serde_json::to_string(&policy)?).execute(&mut *tx).await?;sqlx::query("INSERT INTO settings VALUES(?,?) ON CONFLICT(guild) DO UPDATE SET value=excluded.value").bind(ctx.data().guild.to_string()).bind(serde_json::to_string(&cfg)?).execute(&mut *tx).await?;tx.commit().await?;Ok(())}.await;
            if let Err(error) = saved {
                let _ = sent.delete(ctx.http()).await;
                return Err(error);
            }
            *ctx.data().settings.write().await = cfg;
            reply(ctx,format!("Secure verification posted in <#{}>. Review destination <#{}>. Only the new panel is active. Existing member roles are preserved; public self-role claims for the verification role are blocked.",channel.id,review_channel.id)).await
        }
        Action::Status => {
            let p = policy(&ctx.data().db, ctx.data()).await?;
            staff(ctx.serenity_context(), ctx.data(), &p, ctx.author().id).await?;
            let rows=sqlx::query("SELECT user,risks,created FROM verification_requests WHERE guild=? AND state='pending' ORDER BY created LIMIT 12").bind(ctx.data().guild.to_string()).fetch_all(&ctx.data().db).await?;
            let list = rows
                .iter()
                .map(|r| {
                    format!(
                        "<@{}> · <t:{}:R>",
                        r.get::<String, _>("user"),
                        r.get::<i64, _>("created")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            reply(ctx,format!("**Verification policy**\nAccount review threshold: {} days\nJoin wait: {} minutes\nReview channel: {}\nChallenges: 2 minutes, 3 code attempts; 5 starts per 10-minute window.\n\n**Pending requests**\n{}\nUse /verify Approve or Deny with a member. Flags indicate risk, not proof of an alternate account.",p.account_days,p.wait_minutes,p.review.map(|id|format!("<#{id}>")).unwrap_or_else(||"Not configured".into()),if list.is_empty(){"None"}else{&list})).await
        }
        Action::Approve | Action::Deny => {
            let p = policy(&ctx.data().db, ctx.data()).await?;
            let target = target.ok_or_else(|| {
                crate::errors::user(
                    "Select the member whose verification request you want to review.",
                )
            })?;
            let reason = reason.unwrap_or_else(|| {
                if matches!(action, Action::Approve) {
                    "Staff reviewed access request".into()
                } else {
                    "Staff declined access request".into()
                }
            });
            security::reason(&reason)?;
            let result = decision(
                ctx.serenity_context(),
                ctx.data(),
                &p,
                ctx.author().id,
                target.user.id,
                matches!(action, Action::Approve),
                &reason,
            )
            .await?;
            reply(ctx, result).await
        }
    }
}
fn panel_valid(p: &Policy, button: &s::ComponentInteraction, bot: s::UserId) -> bool {
    button.message.author.id == bot
        && p.channel.is_none_or(|id| id == button.channel_id.get())
        && p.message.is_none_or(|id| id == button.message.id.get())
}
async fn begin(
    ctx: &s::Context,
    data: &Data,
    p: &Policy,
    button: &s::ComponentInteraction,
    manual: bool,
) -> Result<(), Error> {
    let _guard = data.configuration_lock.lock().await;
    if policy(&data.db, data).await?.revision != p.revision {
        return Err(crate::errors::user(
            "Verification policy changed. Use the latest panel.",
        ));
    }
    if !panel_valid(p, button, ctx.cache.current_user().id) {
        return Err(crate::errors::user(
            "This verification panel is outdated. Use the latest staff-posted panel.",
        ));
    }
    let m = member(ctx, data, p, button.user.id).await?;
    if m.roles.contains(&s::RoleId::new(p.role)) {
        return Err(crate::errors::user(
            "You already have the verified access role.",
        ));
    }
    wait(&m, p)?;
    let denied:Option<i64>=sqlx::query_scalar("SELECT created FROM verification_requests WHERE guild=? AND user=? AND revision=? AND state='denied' AND created>?").bind(data.guild.to_string()).bind(m.user.id.to_string()).bind(&p.revision).bind(now()-3600).fetch_optional(&data.db).await?;
    if denied.is_some() {
        return Err(crate::errors::user(
            "Staff declined this request. Contact staff about the decision, or wait one hour before requesting review again.",
        ));
    }
    let pending:i64=sqlx::query_scalar("SELECT COUNT(*) FROM verification_requests WHERE guild=? AND user=? AND revision=? AND joined=? AND state IN('pending','granting')").bind(data.guild.to_string()).bind(m.user.id.to_string()).bind(&p.revision).bind(joined(&m)?).fetch_one(&data.db).await?;
    if pending != 0 {
        return Err(crate::errors::user(
            "Your request is already waiting for staff review. Starting another code cannot bypass that review.",
        ));
    }
    if !quota(&data.db, data.guild, m.user.id, now()).await? {
        return Err(crate::errors::user(
            "Too many verification starts. Wait until the next 10-minute window before trying again; contact staff for help.",
        ));
    }
    let mut risks = risk(&data.db, data, p, &m).await?;
    if manual {
        risks.push("Member requested an accessible/manual review alternative".into());
        request_review(ctx, data, p, &m, risks).await?;
        button.edit_response(&ctx.http,s::EditInteractionResponse::new().embed(embed("REVIEW REQUESTED","Your access request is waiting for staff review. You can close this message; no role has been granted yet.")).allowed_mentions(s::CreateAllowedMentions::new())).await?;
        return Ok(());
    }
    let (nonce, code) = challenge();
    let png = cards::render(
        "ONE-TIME ACCESS CODE",
        &code,
        "Enter this code within two minutes.",
        &[
            ("EXPIRES", "2 minutes".into()),
            ("ATTEMPTS", "3 attempts".into()),
        ],
        None,
    )?;
    sqlx::query("INSERT INTO verification_sessions(guild,user,nonce,answer,expires,attempts,used,joined,role,revision,risks) VALUES(?,?,?,?,?,0,0,?,?,?,?) ON CONFLICT(guild,user) DO UPDATE SET nonce=excluded.nonce,answer=excluded.answer,expires=excluded.expires,attempts=0,used=0,joined=excluded.joined,role=excluded.role,revision=excluded.revision,risks=excluded.risks")
        .bind(data.guild.to_string()).bind(m.user.id.to_string()).bind(&nonce).bind(digest(&nonce,&code)).bind(now()+120).bind(joined(&m)?).bind(p.role.to_string()).bind(&p.revision).bind(serde_json::to_string(&risks)?).execute(&data.db).await?;
    button.edit_response(&ctx.http,s::EditInteractionResponse::new().embed(embed("ACCESS CHALLENGE","Read the six-character code in the image, then press **Enter code**. Do not enter your Discord password or any login code.\n\nIf you cannot read the image, return to the public panel and choose **Staff help**.").image("attachment://access-code.png")).attachments(s::EditAttachments::new().add(s::CreateAttachment::bytes(png,"access-code.png"))).components(vec![s::CreateActionRow::Buttons(vec![emoji::button_for(format!("elvnx:verification:open:{nonce}"),"shield").label("Enter code").style(s::ButtonStyle::Primary)])]).allowed_mentions(s::CreateAllowedMentions::new())).await?;
    Ok(())
}
async fn consume(
    db: &SqlitePool,
    guild: s::GuildId,
    user: s::UserId,
    nonce: &str,
    code: &str,
    stamp: i64,
) -> Result<Option<sqlx::sqlite::SqliteRow>, Error> {
    let hash = digest(nonce, code);
    let row=sqlx::query("UPDATE verification_sessions SET used=1 WHERE guild=? AND user=? AND nonce=? AND answer=? AND used=0 AND attempts<3 AND expires>? RETURNING *").bind(guild.to_string()).bind(user.to_string()).bind(nonce).bind(hash).bind(stamp).fetch_optional(db).await?;
    if row.is_none() {
        sqlx::query("UPDATE verification_sessions SET attempts=attempts+1,used=CASE WHEN attempts>=2 THEN 1 ELSE used END WHERE guild=? AND user=? AND nonce=? AND used=0 AND expires>?").bind(guild.to_string()).bind(user.to_string()).bind(nonce).bind(stamp).execute(db).await?;
    }
    Ok(row)
}
async fn complete(
    ctx: &s::Context,
    data: &Data,
    p: &Policy,
    user: s::UserId,
    nonce: &str,
    code: &str,
) -> Result<String, Error> {
    let _guard = data.configuration_lock.lock().await;
    if policy(&data.db, data).await?.revision != p.revision {
        return Err(crate::errors::user(
            "Verification policy changed. Start again from the latest panel.",
        ));
    }
    let row=consume(&data.db,data.guild,user,nonce,code,now()).await?.ok_or_else(||crate::errors::user("The code is incorrect, expired, already used, or has reached three attempts. If you still have attempts, choose Enter code again; otherwise restart from the public panel."))?;
    if row.get::<String, _>("revision") != p.revision
        || row.get::<String, _>("role") != p.role.to_string()
    {
        return Err(crate::errors::user(
            "The verification policy changed. Start again from the latest public panel.",
        ));
    }
    let m = member(ctx, data, p, user).await?;
    if joined(&m)? != row.get::<i64, _>("joined") {
        return Err(crate::errors::user(
            "Your membership changed after the challenge started. Start verification again.",
        ));
    }
    wait(&m, p)?;
    if m.roles.contains(&s::RoleId::new(p.role)) {
        return Ok("You already have the verified access role.".into());
    }
    let mut risks: Vec<String> = serde_json::from_str(&row.get::<String, _>("risks"))?;
    risks.extend(risk(&data.db, data, p, &m).await?);
    risks.sort();
    risks.dedup();
    if !risks.is_empty() {
        request_review(ctx, data, p, &m, risks).await?;
        return Ok("Code accepted. Your account needs staff review before access is granted. This is a screening flag, not a claim that you are an alt. You can close this message.".into());
    }
    m.add_role(&ctx.http, s::RoleId::new(p.role)).await?;
    if let Err(error) = crate::store::case(
        &data.db,
        data.guild.get(),
        user.get(),
        user.get(),
        "verify",
        "One-time challenge and fresh membership/account checks passed",
    )
    .await
    {
        tracing::warn!(%error,"Verified role granted but audit receipt failed");
    }
    Ok(format!(
        "{} Verified. Your whitelist access role has been granted. Welcome to ELVNX.",
        emoji::text("check")
    ))
}
async fn review_destination(
    ctx: &s::Context,
    data: &Data,
    p: &Policy,
) -> Result<s::ChannelId, Error> {
    let channel=s::ChannelId::new(p.review.ok_or_else(||crate::errors::user("Your account needs staff review, but a review destination is not configured. Ask staff to run /verify Setup with review_channel."))?).to_channel(&ctx.http).await?.guild().ok_or_else(||crate::errors::user("Staff review channel unavailable"))?;
    if channel.guild_id != data.guild || channel.kind != s::ChannelType::Text {
        return Err(crate::errors::user(
            "The verification review destination must be a private server text channel.",
        ));
    }
    crate::features::private_reports(ctx, data.guild, &channel).await?;
    Ok(channel.id)
}
async fn save_review(
    db: &SqlitePool,
    guild: s::GuildId,
    user: s::UserId,
    p: &Policy,
    join: i64,
    risks: &[String],
) -> Result<bool, Error> {
    let inserted=sqlx::query("INSERT INTO verification_requests(guild,user,role,revision,joined,risks,created,state) VALUES(?,?,?,?,?,?,?,'pending') ON CONFLICT(guild,user) DO UPDATE SET role=excluded.role,revision=excluded.revision,joined=excluded.joined,risks=excluded.risks,created=excluded.created,state='pending',channel=NULL,message=NULL WHERE verification_requests.state NOT IN('pending','granting') OR (verification_requests.state!='granting' AND (verification_requests.revision!=excluded.revision OR verification_requests.joined!=excluded.joined))")
        .bind(guild.to_string()).bind(user.to_string()).bind(p.role.to_string()).bind(&p.revision).bind(join).bind(serde_json::to_string(&risks)?).bind(now()).execute(db).await?;
    sqlx::query("UPDATE verification_sessions SET used=1 WHERE guild=? AND user=?")
        .bind(guild.to_string())
        .bind(user.to_string())
        .execute(db)
        .await?;
    Ok(inserted.rows_affected() != 0)
}
async fn request_review(
    ctx: &s::Context,
    data: &Data,
    p: &Policy,
    m: &s::Member,
    risks: Vec<String>,
) -> Result<(), Error> {
    let channel = review_destination(ctx, data, p).await?;
    if !save_review(&data.db, data.guild, m.user.id, p, joined(m)?, &risks).await? {
        return Ok(());
    }
    let message=channel.send_message(&ctx.http,s::CreateMessage::new().embed(embed("ACCESS REVIEW",format!("{} **Screening flag, not proof of an alternate account.**\n\nMember <@{}> (`{}`)\nAccount created <t:{}:F>\nJoined <t:{}:F>\n\n{}\n\nApprove grants the configured whitelist role. Deny only declines this access request; it does not ban the member. Authorized staff can also use /verify Approve or Deny.",emoji::text("review"),m.user.id,m.user.id,m.user.id.created_at().unix_timestamp(),joined(m)?,risks.join("\n"))))
        .components(vec![s::CreateActionRow::Buttons(vec![emoji::button_for(format!("elvnx:verification:approve:{}",m.user.id),"check").label("Approve access").style(s::ButtonStyle::Success),emoji::button_for(format!("elvnx:verification:deny:{}",m.user.id),"cross").label("Decline access").style(s::ButtonStyle::Danger)])]).allowed_mentions(s::CreateAllowedMentions::new())).await;
    match message {
        Ok(message) => {
            sqlx::query("UPDATE verification_requests SET channel=?,message=? WHERE guild=? AND user=? AND revision=? AND state='pending'").bind(channel.to_string()).bind(message.id.to_string()).bind(data.guild.to_string()).bind(m.user.id.to_string()).bind(&p.revision).execute(&data.db).await?;
        }
        Err(error) => {
            tracing::warn!(%error,"Access request saved; staff notification failed, inspect /verify Status");
        }
    }
    Ok(())
}
fn approval_embed(guild: s::GuildId, role: u64) -> s::CreateEmbed {
    emoji::embed_for("check").title("ELVNX / ACCESS APPROVED")
        .description(format!("{} **You're verified. Welcome to ELVNX.**\n\nStaff reviewed and approved your verification request. Your whitelist access role has been granted. You can now explore the channels available to your role.\n\nRead the server rules, respect other members and enjoy your stay. No further verification code or payment is required.",emoji::text("check")))
        .field("Access","Whitelist role granted",true).field("Server",guild.to_string(),true)
        .footer(s::CreateEmbedFooter::new(format!("11X • verification complete • role {role}")))
}
fn approval_message(guild: s::GuildId, role: u64) -> s::CreateMessage {
    s::CreateMessage::new()
        .embed(approval_embed(guild, role))
        .components(vec![s::CreateActionRow::Buttons(vec![
            emoji::link(format!("https://discord.com/channels/{guild}")).label("Open ELVNX"),
        ])])
        .allowed_mentions(s::CreateAllowedMentions::new())
}
async fn notify_approved(
    ctx: &s::Context,
    data: &Data,
    target: s::UserId,
    role: u64,
) -> Result<(), Error> {
    let user = target.to_user(&ctx.http).await?;
    let mut message = approval_message(data.guild, role);
    match cards::render(
        "ACCESS / APPROVED",
        &user.name,
        "Your next chapter starts here.",
        &[
            ("VERIFIED", "Staff approved".into()),
            ("ACCESS", "Unlocked".into()),
        ],
        None,
    ) {
        Ok(png) => {
            message = message
                .add_file(s::CreateAttachment::bytes(png, "access-approved.png"))
                .embeds(vec![
                    approval_embed(data.guild, role).image("attachment://access-approved.png"),
                ]);
        }
        Err(error) => {
            tracing::warn!(%error,"Approval cover failed; sending the accessible text embed")
        }
    }
    let sent = user.direct_message(&ctx.http, message).await?;
    if let Err(error) = sqlx::query(
        "INSERT OR IGNORE INTO outgoing_dms(guild,user,channel,message) VALUES(?,?,?,?)",
    )
    .bind(data.guild.to_string())
    .bind(target.to_string())
    .bind(sent.channel_id.to_string())
    .bind(sent.id.to_string())
    .execute(&data.db)
    .await
    {
        tracing::warn!(%error,"Approval DM delivered but receipt persistence failed");
    }
    Ok(())
}
async fn decision(
    ctx: &s::Context,
    data: &Data,
    p: &Policy,
    actor: s::UserId,
    target: s::UserId,
    approve: bool,
    reason: &str,
) -> Result<String, Error> {
    let _guard = data.configuration_lock.lock().await;
    if policy(&data.db, data).await?.revision != p.revision {
        return Err(crate::errors::user(
            "Verification policy changed. Use the latest review panel.",
        ));
    }
    staff(ctx, data, p, actor).await?;
    let row=sqlx::query("SELECT * FROM verification_requests WHERE guild=? AND user=? AND state IN('pending','denied')").bind(data.guild.to_string()).bind(target.to_string()).fetch_optional(&data.db).await?.ok_or_else(||crate::errors::user("No reviewable verification request was found. Ask the member to start from the latest panel."))?;
    if row.get::<String, _>("revision") != p.revision
        || row.get::<String, _>("role") != p.role.to_string()
    {
        return Err(crate::errors::user(
            "This request uses an outdated policy. The member must verify again.",
        ));
    }
    if approve {
        let m = member(ctx, data, p, target).await?;
        wait(&m, p)?;
        if joined(&m)? != row.get::<i64, _>("joined") {
            return Err(crate::errors::user(
                "This member left/rejoined after submitting the request. They must verify again.",
            ));
        }
        let claimed=sqlx::query("UPDATE verification_requests SET state='granting',created=unixepoch() WHERE guild=? AND user=? AND revision=? AND state IN('pending','denied')").bind(data.guild.to_string()).bind(target.to_string()).bind(&p.revision).execute(&data.db).await?;
        if claimed.rows_affected() == 0 {
            return Err(crate::errors::user(
                "Another staff member already handled this request.",
            ));
        }
        if let Err(error) = m.add_role(&ctx.http, s::RoleId::new(p.role)).await {
            let _=sqlx::query("UPDATE verification_requests SET state='pending' WHERE guild=? AND user=? AND revision=? AND state='granting'").bind(data.guild.to_string()).bind(target.to_string()).bind(&p.revision).execute(&data.db).await;
            return Err(error.into());
        }
    }
    let changed=sqlx::query("UPDATE verification_requests SET state=?,created=? WHERE guild=? AND user=? AND revision=? AND state=?").bind(if approve{"approved"}else{"denied"}).bind(now()).bind(data.guild.to_string()).bind(target.to_string()).bind(&p.revision).bind(if approve {"granting".to_string()} else {row.get::<String,_>("state")}).execute(&data.db).await?;
    if changed.rows_affected() == 0 {
        return Err(crate::errors::user(
            "This request has already been handled.",
        ));
    }
    let dm_delivered = if approve {
        match notify_approved(ctx, data, target, p.role).await {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(member=%target,%error,"Access approved; member DM could not be delivered");
                false
            }
        }
    } else {
        false
    };
    let _ = crate::store::case(
        &data.db,
        data.guild.get(),
        target.get(),
        actor.get(),
        if approve {
            "verification-approve"
        } else {
            "verification-deny"
        },
        reason,
    )
    .await;
    if let (Some(channel), Some(message)) = (
        row.get::<Option<String>, _>("channel"),
        row.get::<Option<String>, _>("message"),
    ) {
        let outcome: Result<(), Error> = async {
            let channel = s::ChannelId::new(channel.parse()?);
            let message = s::MessageId::new(message.parse()?);
            let old = channel.message(&ctx.http, message).await?;
            if old.author.id != ctx.cache.current_user().id {
                return Ok(());
            }
            let mut e = old
                .embeds
                .into_iter()
                .next()
                .ok_or_else(|| crate::errors::user("Review embed missing"))?;
            e.fields
                .retain(|f| f.name != "Staff decision" && f.name != "Member notification");
            channel
                .edit_message(
                    &ctx.http,
                    message,
                    s::EditMessage::new()
                        .embed(
                            s::CreateEmbed::from(e)
                                .field(
                                    "Staff decision",
                                    format!(
                                        "{} by <@{}>\n{}",
                                        if approve { "Approved" } else { "Declined" },
                                        actor,
                                        reason
                                    ),
                                    false,
                                )
                                .field(
                                    "Member notification",
                                    if approve {
                                        if dm_delivered {
                                            "Approval DM delivered"
                                        } else {
                                            "Approval DM unavailable; access was still granted"
                                        }
                                    } else {
                                        "No approval DM sent"
                                    },
                                    false,
                                ),
                        )
                        .components(vec![])
                        .allowed_mentions(s::CreateAllowedMentions::new()),
                )
                .await?;
            Ok(())
        }
        .await;
        if outcome.is_err() {
            tracing::warn!("Access decision completed; staff message refresh failed");
        }
    }
    Ok(if approve{if dm_delivered {"Access approved, whitelist role granted and confirmation DM delivered."} else {"Access approved and whitelist role granted. The confirmation DM could not be delivered; the member may have closed DMs."}}else{"Request declined. No ban or kick was performed. The member may contact staff or request review again after one hour."}.into())
}
#[derive(poise::Modal)]
#[name = "ELVNX / One-time Access Code"]
struct Code {
    #[name = "Six-character code from the image"]
    #[min_length = 6]
    #[max_length = 6]
    code: String,
}
pub async fn gateway(ctx: &s::Context, event: &s::FullEvent, data: &Data) -> Result<bool, Error> {
    match event {
        s::FullEvent::GuildMemberAddition { new_member: m }
            if m.guild_id == data.guild && !m.user.bot =>
        {
            sqlx::query("INSERT INTO verification_joins VALUES(?,?,?,?,1) ON CONFLICT(guild,user) DO UPDATE SET count=CASE WHEN window<? THEN 1 ELSE count+1 END,window=CASE WHEN window<? THEN excluded.window ELSE window END,joined=excluded.joined").bind(data.guild.to_string()).bind(m.user.id.to_string()).bind(now()).bind(now()).bind(now()-86400).bind(now()-86400).execute(&data.db).await?;
            return Ok(false);
        }
        s::FullEvent::GuildMemberRemoval { guild_id, user, .. } if *guild_id == data.guild => {
            sqlx::query("DELETE FROM verification_sessions WHERE guild=? AND user=?")
                .bind(data.guild.to_string())
                .bind(user.id.to_string())
                .execute(&data.db)
                .await?;
            sqlx::query("UPDATE verification_requests SET state='left' WHERE guild=? AND user=? AND state IN('pending','granting')").bind(data.guild.to_string()).bind(user.id.to_string()).execute(&data.db).await?;
            return Ok(false);
        }
        _ => {}
    }
    let s::FullEvent::InteractionCreate {
        interaction: s::Interaction::Component(button),
    } = event
    else {
        return Ok(false);
    };
    let id = &button.data.custom_id;
    if !id.starts_with("elvnx:verify:") && !id.starts_with("elvnx:verification:") {
        return Ok(false);
    }
    if button.guild_id != Some(data.guild)
        || button.message.author.id != ctx.cache.current_user().id
    {
        return Ok(false);
    }
    if let Some(nonce) = id.strip_prefix("elvnx:verification:open:") {
        use poise::Modal;
        let active=sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM verification_sessions WHERE guild=? AND user=? AND nonce=? AND used=0 AND attempts<3 AND expires>?").bind(data.guild.to_string()).bind(button.user.id.to_string()).bind(nonce).bind(now()).fetch_one(&data.db).await;
        let active = match active {
            Ok(active) => active,
            Err(error) => {
                button
                    .create_response(
                        &ctx.http,
                        s::CreateInteractionResponse::Message(
                            s::CreateInteractionResponseMessage::new()
                                .ephemeral(true)
                                .embed(embed(
                                    "ACCESS CHECK",
                                    crate::errors::with_reference(
                                        &error,
                                        "verify",
                                        button.id.get(),
                                    ),
                                ))
                                .allowed_mentions(s::CreateAllowedMentions::new()),
                        ),
                    )
                    .await?;
                return Ok(true);
            }
        };
        if active == 0 {
            button.create_response(&ctx.http,s::CreateInteractionResponse::Message(s::CreateInteractionResponseMessage::new().ephemeral(true).embed(embed("CHALLENGE EXPIRED","This challenge is no longer active. Start again from the public verification panel.")))).await?;
            return Ok(true);
        }
        let custom = format!("verification-code-{}", button.id);
        button
            .create_response(&ctx.http, Code::create(None, custom.clone()))
            .await?;
        let user = button.user.id;
        let guild = data.guild;
        let response = s::collector::ModalInteractionCollector::new(&ctx.shard)
            .filter(move |m| {
                m.data.custom_id == custom && m.user.id == user && m.guild_id == Some(guild)
            })
            .timeout(Duration::from_secs(120))
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
            let result: Result<String, Error> = async {
                let code = Code::parse(response.data.clone()).map_err(|e| -> Error { e.into() })?;
                let p = policy(&data.db, data).await?;
                complete(ctx, data, &p, user, nonce, &code.code).await
            }
            .await;
            let message = match result {
                Ok(text) => text,
                Err(error) => {
                    crate::errors::with_reference(error.as_ref(), "verify", response.id.get())
                }
            };
            response
                .edit_response(
                    &ctx.http,
                    s::EditInteractionResponse::new()
                        .embed(embed("ACCESS CHECK", message))
                        .allowed_mentions(s::CreateAllowedMentions::new()),
                )
                .await?;
        }
        return Ok(true);
    }
    button
        .create_response(
            &ctx.http,
            s::CreateInteractionResponse::Defer(
                s::CreateInteractionResponseMessage::new().ephemeral(true),
            ),
        )
        .await?;
    let result:Result<Option<String>,Error>=async{
        let p=policy(&data.db,data).await?;
        if let Some(role)=id.strip_prefix("elvnx:verify:"){
            if role.parse::<u64>()?!=p.role{return Err(crate::errors::user("This verification role/panel is no longer active."));}
            begin(ctx,data,&p,button,false).await?;return Ok(None);
        }
        if id=="elvnx:verification:help"{begin(ctx,data,&p,button,true).await?;return Ok(None);}
        let (approve,raw)=if let Some(raw)=id.strip_prefix("elvnx:verification:approve:"){(true,raw)}else if let Some(raw)=id.strip_prefix("elvnx:verification:deny:"){(false,raw)}else{return Err(crate::errors::user("Unknown verification action. Use the latest panel."));};
        let target=s::UserId::new(raw.parse()?);staff(ctx,data,&p,button.user.id).await?;
        let dest=review_destination(ctx,data,&p).await?;
        let matched:i64=sqlx::query_scalar("SELECT COUNT(*) FROM verification_requests WHERE guild=? AND user=? AND channel=? AND message=? AND revision=?").bind(data.guild.to_string()).bind(target.to_string()).bind(button.channel_id.to_string()).bind(button.message.id.to_string()).bind(&p.revision).fetch_one(&data.db).await?;
        if dest!=button.channel_id||matched!=1{return Err(crate::errors::user("This review panel is no longer active."));}
        Ok(Some(decision(ctx,data,&p,button.user.id,target,approve,"Staff reviewed through the verification panel").await?))
    }.await;
    match result {
        Ok(Some(text)) => {
            button
                .edit_response(
                    &ctx.http,
                    s::EditInteractionResponse::new()
                        .embed(embed("ACCESS REVIEW", text))
                        .allowed_mentions(s::CreateAllowedMentions::new()),
                )
                .await?;
        }
        Ok(None) => {}
        Err(error) => {
            button
                .edit_response(
                    &ctx.http,
                    s::EditInteractionResponse::new()
                        .embed(embed(
                            "ACCESS CHECK",
                            crate::errors::with_reference(
                                error.as_ref(),
                                "verify",
                                button.id.get(),
                            ),
                        ))
                        .allowed_mentions(s::CreateAllowedMentions::new()),
                )
                .await?;
        }
    }
    Ok(true)
}
pub async fn cleanup(db: &SqlitePool) -> Result<(), Error> {
    sqlx::query("DELETE FROM verification_sessions WHERE expires<?")
        .bind(now())
        .execute(db)
        .await?;
    sqlx::query("DELETE FROM verification_attempts WHERE window<?")
        .bind((now() - 86400).div_euclid(600))
        .execute(db)
        .await?;
    sqlx::query("DELETE FROM verification_joins WHERE joined<?")
        .bind(now() - 86400)
        .execute(db)
        .await?;
    sqlx::query(
        "UPDATE verification_requests SET state='pending' WHERE state='granting' AND created<?",
    )
    .bind(now() - 300)
    .execute(db)
    .await?;
    sqlx::query("DELETE FROM verification_requests WHERE created<?")
        .bind(now() - 30 * 86400)
        .execute(db)
        .await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approval_notice_is_private_branded_and_does_not_include_staff_notes() {
        let json = serde_json::to_value(approval_message(s::GuildId::new(1), 3)).unwrap();
        assert_eq!(json["allowed_mentions"]["parse"], serde_json::json!([]));
        assert_eq!(json["embeds"][0]["title"], "ELVNX / ACCESS APPROVED");
        assert_eq!(
            json["components"][0]["components"][0]["url"],
            "https://discord.com/channels/1"
        );
        assert!(json.to_string().contains("elvnx_live_check"));
        assert!(!json.to_string().contains("Staff decision"));
    }
    #[test]
    fn risks_are_screening_not_identity_proof() {
        let p = Policy::default();
        assert!(eligibility(0, 10 * 86400, &p, 1, 1).is_empty());
        assert_eq!(eligibility(0, 86400, &p, 3, 8).len(), 3);
    }
    #[tokio::test]
    async fn review_deduplicates_and_cannot_replace_an_inflight_grant() {
        let db = crate::store::connect("sqlite::memory:").await.unwrap();
        let mut p = Policy {
            role: 3,
            revision: "a".into(),
            ..Default::default()
        };
        let guild = s::GuildId::new(1);
        let user = s::UserId::new(2);
        assert!(save_review(&db, guild, user, &p, 10, &[]).await.unwrap());
        assert!(!save_review(&db, guild, user, &p, 10, &[]).await.unwrap());
        sqlx::query("UPDATE verification_requests SET state='granting'")
            .execute(&db)
            .await
            .unwrap();
        p.revision = "b".into();
        assert!(!save_review(&db, guild, user, &p, 20, &[]).await.unwrap());
        let revision: String = sqlx::query_scalar("SELECT revision FROM verification_requests")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(revision, "a");
        sqlx::query("UPDATE verification_requests SET state='pending'")
            .execute(&db)
            .await
            .unwrap();
        assert!(save_review(&db, guild, user, &p, 20, &[]).await.unwrap());
        assert!(!save_review(&db, guild, user, &p, 20, &[]).await.unwrap());
        assert!(
            save_review(&db, guild, s::UserId::new(4), &p, 20, &[])
                .await
                .unwrap()
        );
    }
    #[tokio::test]
    async fn challenges_are_bound_expiring_and_once_only() {
        let db = crate::store::connect("sqlite::memory:").await.unwrap();
        sqlx::query("INSERT INTO verification_sessions(guild,user,nonce,answer,expires,joined,role,revision,risks) VALUES('1','2','nonce',?,200,1,'3','r','[]')").bind(digest("nonce","ABC234")).execute(&db).await.unwrap();
        assert!(
            consume(
                &db,
                s::GuildId::new(1),
                s::UserId::new(4),
                "nonce",
                "ABC234",
                100
            )
            .await
            .unwrap()
            .is_none()
        );
        assert!(
            consume(
                &db,
                s::GuildId::new(9),
                s::UserId::new(2),
                "nonce",
                "ABC234",
                100
            )
            .await
            .unwrap()
            .is_none()
        );
        assert!(
            consume(
                &db,
                s::GuildId::new(1),
                s::UserId::new(2),
                "nonce",
                "ABC234",
                200
            )
            .await
            .unwrap()
            .is_none()
        );
        assert!(
            consume(
                &db,
                s::GuildId::new(1),
                s::UserId::new(2),
                "nonce",
                " abc234 ",
                100
            )
            .await
            .unwrap()
            .is_some()
        );
        assert!(
            consume(
                &db,
                s::GuildId::new(1),
                s::UserId::new(2),
                "nonce",
                "ABC234",
                100
            )
            .await
            .unwrap()
            .is_none()
        );
    }
    #[tokio::test]
    async fn wrong_codes_lock_after_three_and_rate_limits_persist() {
        let db = crate::store::connect("sqlite::memory:").await.unwrap();
        sqlx::query("INSERT INTO verification_sessions(guild,user,nonce,answer,expires,joined,role,revision,risks) VALUES('1','2','nonce',?,200,1,'3','r','[]')").bind(digest("nonce","ABC234")).execute(&db).await.unwrap();
        for _ in 0..3 {
            assert!(
                consume(
                    &db,
                    s::GuildId::new(1),
                    s::UserId::new(2),
                    "nonce",
                    "ZZZZZZ",
                    100
                )
                .await
                .unwrap()
                .is_none()
            );
        }
        assert!(
            consume(
                &db,
                s::GuildId::new(1),
                s::UserId::new(2),
                "nonce",
                "ABC234",
                100
            )
            .await
            .unwrap()
            .is_none()
        );
        for _ in 0..5 {
            assert!(
                quota(&db, s::GuildId::new(1), s::UserId::new(2), 100)
                    .await
                    .unwrap()
            );
        }
        assert!(
            !quota(&db, s::GuildId::new(1), s::UserId::new(2), 101)
                .await
                .unwrap()
        );
        assert!(
            quota(&db, s::GuildId::new(1), s::UserId::new(2), 601)
                .await
                .unwrap()
        );
    }
}
