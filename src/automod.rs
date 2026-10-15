use crate::Error;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    pub spam_limit: usize,
    pub mention_limit: usize,
    pub max_characters: usize,
    pub max_attachments: usize,
    pub max_custom_emoji: usize,
    pub caps_min_letters: usize,
    pub caps_percent: usize,
    pub block_all_links: bool,
    pub allowed_domains: Vec<String>,
    pub denied_domains: Vec<String>,
    pub blocked_words: Vec<String>,
    pub exempt_channels: Vec<u64>,
    pub exempt_roles: Vec<u64>,
    pub trusted_actor_ids: Vec<u64>,
    pub antinuke_limit: usize,
    pub antinuke_window_seconds: u64,
}
impl Default for Rules {
    fn default() -> Self {
        Self {
            spam_limit: 5,
            mention_limit: 5,
            max_characters: 1900,
            max_attachments: 8,
            max_custom_emoji: 20,
            caps_min_letters: 20,
            caps_percent: 90,
            block_all_links: false,
            allowed_domains: vec!["avrxt.dev".into(), "github.com".into()],
            denied_domains: vec![],
            blocked_words: vec![],
            exempt_channels: vec![],
            exempt_roles: vec![],
            trusted_actor_ids: vec![],
            antinuke_limit: 4,
            antinuke_window_seconds: 15,
        }
    }
}
impl Rules {
    pub fn load() -> Result<Self, Error> {
        let r: Self = match std::fs::read_to_string("automod.json") {
            Ok(v) => serde_json::from_str(&v)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e.into()),
        };
        r.validate()?;
        Ok(r)
    }
    pub fn validate(&self) -> Result<(), Error> {
        if !(3..=30).contains(&self.spam_limit)
            || !(1..=30).contains(&self.mention_limit)
            || !(100..=4000).contains(&self.max_characters)
            || !(1..=10).contains(&self.max_attachments)
            || !(50..=100).contains(&self.caps_percent)
            || self.caps_min_letters < 10
            || !(3..=20).contains(&self.antinuke_limit)
            || !(5..=120).contains(&self.antinuke_window_seconds)
            || self.blocked_words.len() > 500
            || self.allowed_domains.len() > 100
            || self.denied_domains.len() > 100
            || self.trusted_actor_ids.len() > 100
            || self.exempt_roles.len() > 100
            || self.exempt_channels.len() > 100
        {
            return Err(crate::errors::user(
                "Automod thresholds are outside safe limits",
            ));
        }
        Ok(())
    }
    pub fn text_reason(&self, text: &str, attachments: usize) -> Option<&'static str> {
        if text.chars().count() > self.max_characters {
            return Some("oversized message");
        }
        if attachments > self.max_attachments {
            return Some("attachment flood");
        }
        if text.matches("<:").count() + text.matches("<a:").count() > self.max_custom_emoji {
            return Some("emoji flood");
        }
        let letters: Vec<_> = text.chars().filter(|c| c.is_alphabetic()).collect();
        if letters.len() >= self.caps_min_letters
            && letters.iter().filter(|c| c.is_uppercase()).count() * 100
                >= letters.len() * self.caps_percent
        {
            return Some("excessive uppercase");
        }
        let lower = text.to_lowercase();
        if lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| self.blocked_words.iter().any(|bad| w == bad.to_lowercase()))
        {
            return Some("configured blocked word");
        }
        for token in text.split_whitespace() {
            let token = token.trim_matches(['<', '>', '(', ')', ',']);
            if let Ok(url) = reqwest::Url::parse(token)
                && matches!(url.scheme(), "http" | "https")
                && let Some(host) = url.host_str()
            {
                let matches =
                    |domain: &String| host == domain || host.ends_with(&format!(".{domain}"));
                if self.denied_domains.iter().any(matches) {
                    return Some("denied link domain");
                }
                if self.block_all_links && !self.allowed_domains.iter().any(matches) {
                    return Some("unapproved link");
                }
            }
        }
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn domains_use_boundaries_and_caps_use_minimum() {
        let r = Rules {
            block_all_links: true,
            ..Default::default()
        };
        assert!(r.text_reason("https://www.avrxt.dev", 0).is_none());
        assert!(r.text_reason("https://avrxt.dev.evil.test", 0).is_some());
        assert!(r.text_reason("OK", 0).is_none());
        assert!(r.text_reason(&"LOUD".repeat(10), 0).is_some());
    }
    #[test]
    fn unsafe_limits_are_rejected() {
        let r = Rules {
            antinuke_limit: 1,
            ..Default::default()
        };
        assert!(r.validate().is_err());
    }
}
