use crate::{Context, Error, serenity as s};

pub fn reason(value: &str) -> Result<&str, Error> {
    let text = value.trim();
    if text.is_empty() || text.chars().count() > 400 {
        return Err(crate::errors::user("Reason must contain 1–400 characters."));
    }
    Ok(text)
}

pub fn higher(a: (i64, u64), b: (i64, u64)) -> bool {
    a.0 > b.0 || (a.0 == b.0 && a.1 < b.1)
}
pub fn highest(guild: &s::PartialGuild, member: &s::Member) -> (i64, u64) {
    member
        .roles
        .iter()
        .filter_map(|id| guild.roles.get(id))
        .map(|r| (i64::from(r.position), r.id.get()))
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)))
        .unwrap_or((0, guild.id.get()))
}

pub async fn target(
    ctx: Context<'_>,
    member: &s::Member,
    permission: s::Permissions,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or_else(|| crate::errors::user("Guild required."))?;
    let guild = guild_id.to_partial_guild(ctx.http()).await?;
    let actor = guild_id.member(ctx.http(), ctx.author().id).await?;
    let bot_id = ctx.serenity_context().cache.current_user().id;
    let bot = guild_id.member(ctx.http(), bot_id).await?;
    let target = guild_id.member(ctx.http(), member.user.id).await?;
    if !guild.member_permissions(&actor).contains(permission)
        || !guild.member_permissions(&bot).contains(permission)
    {
        return Err(crate::errors::user(
            "You and the bot must both have the required permission.",
        ));
    }
    if target.user.id == guild.owner_id
        || target.user.id == actor.user.id
        || target.user.id == bot_id
    {
        return Err(crate::errors::user(
            "Cannot target the owner, yourself, or this bot.",
        ));
    }
    if (actor.user.id != guild.owner_id
        && !higher(highest(&guild, &actor), highest(&guild, &target)))
        || !higher(highest(&guild, &bot), highest(&guild, &target))
    {
        return Err(crate::errors::user(
            "The target must be below both your highest role and the bot's highest role.",
        ));
    }
    Ok(())
}

pub async fn safe_role(ctx: Context<'_>, role: &s::Role) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or_else(|| crate::errors::user("Guild required."))?;
    let guild = guild_id.to_partial_guild(ctx.http()).await?;
    let current = guild
        .roles
        .get(&role.id)
        .ok_or_else(|| crate::errors::user("Role is not in this guild."))?;
    let bot = guild_id.member(ctx.http(), crate::bot_id(ctx)).await?;
    let actor = guild_id.member(ctx.http(), ctx.author().id).await?;
    let dangerous = s::Permissions::ADMINISTRATOR
        | s::Permissions::MANAGE_GUILD
        | s::Permissions::MANAGE_ROLES
        | s::Permissions::MANAGE_CHANNELS
        | s::Permissions::MANAGE_WEBHOOKS
        | s::Permissions::BAN_MEMBERS
        | s::Permissions::KICK_MEMBERS
        | s::Permissions::MODERATE_MEMBERS
        | s::Permissions::MANAGE_MESSAGES
        | s::Permissions::MENTION_EVERYONE;
    if current.id.get() == guild_id.get()
        || current.managed
        || current.permissions.intersects(dangerous)
    {
        return Err(crate::errors::user(
            "Managed, everyone, and privileged roles cannot be distributed by this bot.",
        ));
    }
    let rank = (i64::from(current.position), current.id.get());
    if !higher(highest(&guild, &bot), rank)
        || (actor.user.id != guild.owner_id && !higher(highest(&guild, &actor), rank))
    {
        return Err(crate::errors::user(
            "Role must be below the bot and the configuring member.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hierarchy_handles_ties_and_equal_roles() {
        assert!(higher((5, 100), (4, 1)));
        assert!(higher((5, 100), (5, 101)));
        assert!(!higher((5, 100), (5, 100)));
        assert!(!higher((4, 1), (5, 100)));
    }
    #[test]
    fn reason_is_bounded() {
        assert!(reason("  ").is_err());
        assert!(reason(&"x".repeat(401)).is_err());
        assert_eq!(reason(" test ").unwrap(), "test");
    }
}
