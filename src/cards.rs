use crate::{Error, serenity as s};
use base64::Engine;
use std::sync::{Arc, OnceLock};

fn fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    Arc::clone(FONTS.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        for bytes in [
            include_bytes!("../assets/fonts/SpaceGrotesk.ttf").as_slice(),
            include_bytes!("../assets/fonts/Kalam-Regular.ttf").as_slice(),
            include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf").as_slice(),
        ] {
            db.load_font_data(bytes.to_vec());
        }
        db.set_sans_serif_family("Space Grotesk");
        db.set_monospace_family("IBM Plex Mono");
        Arc::new(db)
    }))
}

pub fn xml(text: &str) -> String {
    text.chars()
        .take(60)
        .filter(|c| !c.is_control())
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn fit(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        format!("{}…", text.chars().take(max - 1).collect::<String>())
    } else {
        text.to_string()
    }
}
pub async fn avatar(user: &s::User) -> Option<String> {
    let url = user
        .avatar_url()
        .unwrap_or_else(|| user.default_avatar_url());
    let url = reqwest::Url::parse(&url).ok()?;
    if url.scheme() != "https"
        || !matches!(
            url.host_str(),
            Some("cdn.discordapp.com" | "media.discordapp.net")
        )
    {
        return None;
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()?;
    let mut response = client.get(url).send().await.ok()?.error_for_status().ok()?;
    if response.content_length().is_some_and(|n| n > 1_000_000) {
        return None;
    }
    let mime = response
        .headers()
        .get("content-type")?
        .to_str()
        .ok()?
        .split(';')
        .next()?
        .to_string();
    if !matches!(
        mime.as_str(),
        "image/png" | "image/jpeg" | "image/webp" | "image/gif"
    ) {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > 1_000_000 {
            return None;
        }
        bytes.extend(chunk);
    }
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}
pub fn rules_cover(server: &str, date: &str) -> Result<Vec<u8>, Error> {
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="420"><defs><pattern id="dots" width="24" height="24" patternUnits="userSpaceOnUse"><circle cx="2" cy="2" r=".7" fill="#292929"/></pattern></defs><rect width="1200" height="420" fill="#080808"/><rect width="1200" height="420" fill="url(#dots)"/><g fill="none" stroke="#666" stroke-linecap="round" stroke-linejoin="round"><path d="M25 28Q594 14 1176 25L1172 393Q604 410 27 393Z"/><path d="M30 34L165 28M1177 315L1179 387L1040 396" stroke="#aaa"/><path d="M58 223Q388 206 810 220M78 233Q316 217 607 228" stroke="#bbb" stroke-width="2"/><path d="M845 106L855 126L880 130L861 146L861 170L842 156L818 163L827 140L813 122L838 123Z" stroke="#ccc" stroke-width="2"/><path d="M1118 229L1132 229M1125 222L1125 236M915 309L925 309M920 304L920 314" stroke="#aaa" stroke-width="2"/><path d="M851 285Q888 313 936 293" stroke-dasharray="6 8"/><path d="M928 283L941 289L931 302" stroke="#aaa" stroke-width="2"/></g><text x="57" y="65" font-family="IBM Plex Mono" font-size="13" fill="#aaa" letter-spacing="2.6">ELVNX / PRIVATE COLLECTIVE / 11X</text><text x="55" y="157" font-family="Space Grotesk" font-size="68" font-weight="700" fill="#f7f7f7">THE HOUSE RULES.</text><text x="59" y="200" font-family="Space Grotesk" font-size="24" fill="#ccc">{}</text><g transform="translate(969 94) rotate(8 65 80)" fill="none" stroke="#ddd" stroke-linejoin="round"><path d="M12 16Q69 8 123 15L121 108Q99 148 67 164Q33 148 10 110Z" fill="#141414" stroke-width="2"/><path d="M22 25L109 24L109 107Q89 132 67 145Q43 131 23 107Z" stroke="#777"/><path d="M38 82L58 103L97 57" stroke-width="6" stroke-linecap="round"/><path d="M17 185Q71 177 128 188M26 191L112 194" stroke="#888"/></g><g font-family="IBM Plex Mono" font-size="12" fill="#ddd"><rect x="58" y="265" width="160" height="39" rx="9" fill="#141414" stroke="#555"/><text x="78" y="290">01 / RESPECT</text><rect x="233" y="265" width="165" height="39" rx="9" fill="#141414" stroke="#555"/><text x="253" y="290">02 / PRIVACY</text><rect x="413" y="265" width="209" height="39" rx="9" fill="#141414" stroke="#555"/><text x="433" y="290">03 / GOOD ENERGY</text></g><text x="60" y="361" font-family="Kalam" font-size="25" fill="#ccc" transform="rotate(-1 60 361)">keep it kind. keep it ours.</text><text x="942" y="366" font-family="IBM Plex Mono" font-size="11" fill="#888">UPDATED / {}</text></svg>"##,
        xml(&fit(server, 34)),
        xml(date)
    );
    let options = resvg::usvg::Options {
        fontdb: fonts(),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(&svg, &options)?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(1200, 420).ok_or("Image allocation failed")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap.encode_png()?)
}
pub fn render(
    title: &str,
    name: &str,
    subtitle: &str,
    stats: &[(&str, String)],
    avatar: Option<&str>,
) -> Result<Vec<u8>, Error> {
    let image=avatar.map(|url|format!(r#"<image x="658" y="90" width="120" height="120" href="{url}" clip-path="url(#pfp)" preserveAspectRatio="xMidYMid slice"/>"#)).unwrap_or_else(||r##"<circle cx="718" cy="150" r="60" fill="#181818"/><text x="718" y="165" text-anchor="middle" font-family="Space Grotesk" font-size="42" font-weight="700" fill="white">11X</text>"##.into());
    let mut body = String::new();
    if stats.is_empty() {
        body.push_str(r##"<path d="M51 253Q422 258 790 250" stroke="#444" fill="none"/><text x="56" y="342" font-family="Space Grotesk" font-size="86" font-weight="700" letter-spacing="9" fill="#f4f4f4">ELVNX</text><path d="M63 352Q248 343 476 354M100 360Q300 348 457 362" stroke="#888" fill="none" stroke-linecap="round"/><text x="60" y="397" font-family="Kalam" font-size="21" fill="#ccc" transform="rotate(-2 60 397)">your next chapter starts here.</text><g transform="translate(647 310) rotate(-12)" fill="none" stroke="#ddd" stroke-width="2" stroke-linejoin="round"><path d="M0 10L99 -32L75 53L45 24L0 10ZM45 24L99 -32M45 24L33 52L59 36"/><path d="M-2 50Q-58 42 -71 82Q-84 118 -137 104" stroke-dasharray="5 7"/><path d="M107 32L115 41M118 12L133 15M113 -13L130 -18"/></g>"##);
    }
    for (i, (label, value)) in stats.iter().take(6).enumerate() {
        let x = 50 + (i % 3) * 250;
        let y = 283 + (i / 3) * 88;
        body.push_str(&format!(r##"<g transform="translate({x} {})"><path d="M3 5Q113 0 227 4L230 72Q115 76 1 72Z" fill="#141414" stroke="#494949" stroke-width="1.2"/><path d="M6 8L23 7M223 53L224 68L210 69" fill="none" stroke="#888"/><text x="15" y="26" fill="#999" font-family="IBM Plex Mono" font-size="10">{}</text><text x="15" y="57" fill="white" font-family="Space Grotesk" font-size="22" font-weight="700">{}</text></g>"##,y-25,xml(label),xml(&fit(value,16))));
    }
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="850" height="470"><defs><clipPath id="pfp"><circle cx="718" cy="150" r="60"/></clipPath><pattern id="grid" width="24" height="24" patternUnits="userSpaceOnUse"><circle cx="2" cy="2" r="0.65" fill="#2a2a2a"/></pattern></defs><rect width="850" height="470" fill="#080808"/><rect width="850" height="470" fill="url(#grid)"/><g fill="none" stroke="#555" stroke-linecap="round"><path d="M24 28Q415 15 825 26L821 449Q420 456 26 447Z"/><path d="M28 32L113 29M824 362L826 438L735 445" stroke="#999"/><path d="M627 97L633 110L647 113L635 122L633 138L622 126L607 128L614 114L610 100L624 105Z" stroke="#ddd" stroke-width="1.6"/><path d="M791 217L797 217M794 214L794 221M601 231L610 230M605 226L606 235" stroke="#aaa" stroke-width="1.8"/><path d="M646 97Q711 62 780 102Q806 151 774 211Q714 237 657 203Q627 151 649 98M655 86Q722 69 785 115" stroke="#aaa" stroke-width="1.4"/></g><text x="51" y="64" fill="#aaa" font-family="IBM Plex Mono" font-size="11" letter-spacing="1.8">ELVNX / AVRXT.DEV</text><text x="51" y="119" fill="white" font-size="33" font-family="Space Grotesk" font-weight="700">{}</text><text x="51" y="174" fill="white" font-size="28" font-family="Space Grotesk" font-weight="500">{}</text><path d="M52 187Q165 180 302 185" fill="none" stroke="#888" stroke-width="1.6" stroke-linecap="round"/><text x="51" y="216" fill="#aaa" font-size="14" font-family="Space Grotesk">{}</text><text x="594" y="58" fill="#ccc" font-size="17" font-family="Kalam" transform="rotate(3 594 58)">{}</text>{image}{body}<text x="52" y="442" fill="#888" font-size="10" font-family="IBM Plex Mono" letter-spacing="1.5">PRIVATE COLLECTIVE / 11X</text><text x="733" y="442" fill="#999" font-size="13" font-family="Kalam">made of us.</text></svg>"##,
        xml(title),
        xml(&fit(name, 26)),
        xml(&fit(subtitle, 64)),
        if stats.is_empty() {
            "hey, you made it."
        } else {
            "little things add up."
        }
    );
    let options = resvg::usvg::Options {
        fontdb: fonts(),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(&svg, &options)?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(850, 470).ok_or("Image allocation failed")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap.encode_png()?)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escapes_untrusted_names_and_renders_png() {
        assert_eq!(xml("<admin & boss>"), "&lt;admin &amp; boss&gt;");
        let bytes = render(
            "Welcome",
            "Member",
            "ELVNX",
            &[("Member", "42".into())],
            None,
        )
        .unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }
}
