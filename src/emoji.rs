use crate::{Error, serenity as s};
use std::{collections::HashMap, sync::OnceLock};
fn ids() -> &'static HashMap<String, u64> {
    static IDS: OnceLock<HashMap<String, u64>> = OnceLock::new();
    IDS.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/emojis/manifest.json")).unwrap_or_default()
    })
}
fn animated_ids() -> &'static HashMap<String, u64> {
    static IDS: OnceLock<HashMap<String, u64>> = OnceLock::new();
    IDS.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/emojis/animated.json")).unwrap_or_default()
    })
}
fn asset(key: &str) -> Option<(u64, bool, String)> {
    if let Some(id) = animated_ids().get(key) {
        Some((*id, true, format!("elvnx_live_{key}")))
    } else {
        ids()
            .get(key)
            .map(|id| (*id, false, format!("elvnx_{key}")))
    }
}
pub fn text(key: &str) -> String {
    asset(key)
        .map(|(id, animated, name)| format!("<{}:{name}:{id}>", if animated { "a" } else { "" }))
        .unwrap_or_default()
}
pub fn url(key: &str) -> Option<String> {
    asset(key).map(|(id, animated, _)| {
        format!(
            "https://cdn.discordapp.com/emojis/{id}.{}",
            if animated { "gif" } else { "png" }
        )
    })
}
pub fn reaction(key: &str) -> Result<s::ReactionType, Error> {
    let (id, animated, name) = asset(key).ok_or_else(|| {
        crate::errors::user("A custom UI emoji is missing. Staff should run the emoji setup again.")
    })?;
    Ok(s::ReactionType::Custom {
        animated,
        id: s::EmojiId::new(id),
        name: Some(name),
    })
}
pub fn embed() -> s::CreateEmbed {
    embed_for("brand")
}
pub fn embed_for(key: &str) -> s::CreateEmbed {
    let embed = s::CreateEmbed::new()
        .footer(s::CreateEmbedFooter::new("ELVNX / 11X • avrxt.dev"))
        .timestamp(s::Timestamp::now());
    match url(key) {
        Some(url) => embed.author(s::CreateEmbedAuthor::new("ELVNX / 11X").icon_url(url)),
        None => embed,
    }
}
pub fn command_icon(name: &str) -> &'static str {
    let name = name.to_ascii_lowercase();
    if name.contains("mail") || name == "dm" || name == "dm-clear" {
        "mail"
    } else if name.contains("play")
        || name.contains("music")
        || name.contains("lavlink")
        || ["shuffle", "repeat", "queue", "vote-skip", "dj-role"].contains(&name.as_str())
    {
        "headphones"
    } else if name.contains("insight") || name.contains("leader") {
        "trophy"
    } else if name.contains("event")
        || name.contains("schedule")
        || name.contains("remind")
        || name.contains("daily")
        || name.contains("birthday")
    {
        "calendar"
    } else if name.contains("verify")
        || name.contains("verification")
        || name.contains("honeypot")
        || name.contains("mod")
        || ["ban", "kick", "timeout", "warn", "warnings", "antinuke"].contains(&name.as_str())
    {
        "shield"
    } else if name.contains("voice") || name.contains("afk") || name.contains("join-to") {
        "microphone"
    } else if name.contains("media") || name.contains("attachement") {
        "upload"
    } else if name.contains("privacy") || name.contains("lock") {
        "lock"
    } else if name.contains("search") || name == "help" {
        "search"
    } else if name.contains("status") || name.contains("health") || name == "ping" {
        "equalizer"
    } else if name.contains("review") || name.contains("report") || name == "log" {
        "review"
    } else if name.contains("poll") {
        "queue"
    } else if name.contains("welcome") {
        "wave"
    } else if name.contains("setup")
        || name.contains("config")
        || name.contains("settings")
        || name.contains("management")
    {
        "settings"
    } else if name.contains("member")
        || name.contains("user")
        || name.contains("role")
        || name.contains("availibilty")
    {
        "user"
    } else if name == "say" {
        "typing"
    } else if name.contains("quote") {
        "heart"
    } else {
        "brand"
    }
}
pub fn named(title: &str) -> s::CreateEmbed {
    embed_for(command_icon(title))
}
fn key(id: &str) -> &'static str {
    if id.contains("music:") {
        if id.ends_with(":pause") {
            "pause"
        } else if id.ends_with(":skip") {
            "next"
        } else if id.ends_with(":stop") {
            "stop"
        } else if id.ends_with(":less") {
            "volume_down"
        } else if id.ends_with(":more") {
            "volume_up"
        } else if id.ends_with(":queue") {
            "queue"
        } else if id.ends_with(":shuffle") {
            "shuffle"
        } else if id.ends_with(":repeat") {
            "repeat"
        } else {
            "check"
        }
    } else if id.contains("unlock") {
        "unlock"
    } else if id.contains("lock") {
        "lock"
    } else if id.contains("less") {
        "minus"
    } else if id.contains("more") {
        "plus"
    } else if id.contains("deny") || id.contains("cancel") {
        "cross"
    } else if id.contains("verify") || id.contains("verification") {
        "shield"
    } else if id.contains("claim") || id.contains("review") {
        "review"
    } else if id.contains("resolve") || id.contains("confirm") {
        "check"
    } else {
        "brand"
    }
}
pub fn button(id: impl Into<String>) -> s::CreateButton {
    let id = id.into();
    let k = key(&id);
    button_for(id, k)
}
pub fn button_for(id: impl Into<String>, key: &str) -> s::CreateButton {
    let button = s::CreateButton::new(id);
    match reaction(key) {
        Ok(icon) => button.emoji(icon),
        Err(_) => button,
    }
}
pub fn link(url: impl Into<String>) -> s::CreateButton {
    let button = s::CreateButton::new_link(url);
    match reaction("link") {
        Ok(icon) => button.emoji(icon),
        Err(_) => button,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn feature_icons_choose_context_without_default_unicode_emojis() {
        assert_eq!(command_icon("play"), "headphones");
        assert_eq!(command_icon("dm"), "mail");
        assert_eq!(command_icon("daily-insight"), "trophy");
        assert_eq!(command_icon("honeypot"), "shield");
        assert_eq!(command_icon("server-health"), "equalizer");
        assert_eq!(command_icon("help"), "search");
    }
    #[test]
    fn application_icon_pack_has_unique_ids() {
        assert_eq!(ids().len(), 40);
        let unique: std::collections::HashSet<_> = ids().values().collect();
        assert_eq!(unique.len(), 40);
        assert!(ids().values().all(|id| *id > 0));
        let button = serde_json::to_value(button("elvnx:music:stop").label("Stop")).unwrap();
        assert_eq!(button["emoji"]["name"], "elvnx_live_stop");
        assert!(text("check").starts_with("<a:elvnx_live_check:"));
        assert_eq!(animated_ids().len(), 40);
        let all: std::collections::HashSet<_> =
            ids().values().chain(animated_ids().values()).collect();
        assert_eq!(all.len(), 80);
        assert_eq!(button["emoji"]["animated"], true);
        assert!(url("brand").unwrap().ends_with(".gif"));
    }
}
