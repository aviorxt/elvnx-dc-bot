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
            include_bytes!("../assets/fonts/BarlowCondensed-ExtraBoldItalic.ttf").as_slice(),
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
// Shared cinematic shell. Dynamic content is always escaped before insertion.
fn shell(width: u32, height: u32) -> String {
    let background = base64::engine::general_purpose::STANDARD.encode(include_bytes!(
        "../assets/status/elvnx-purple-status-background.png"
    ));
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><defs><linearGradient id="silver" x2="0" y2="1"><stop stop-color="#ffffff"/><stop offset=".5" stop-color="#e9e1fc"/><stop offset="1" stop-color="#ada0c8"/></linearGradient><linearGradient id="violet"><stop stop-color="#8b35ff"/><stop offset="1" stop-color="#d4bcff"/></linearGradient></defs><image width="{width}" height="{height}" href="data:image/png;base64,{background}" preserveAspectRatio="xMidYMid slice"/><rect width="{width}" height="{height}" fill="#08040f" opacity=".65"/><path d="M18 46L46 18H{right}L{edge} 46V{bottom}L{right} {base}H46L18 {bottom}Z" fill="none" stroke="#8b35ff" stroke-width="2"/><path d="M46 23H165M{accent} {base}H{right}" stroke="#d4bcff" stroke-width="4"/>"##,
        right = width - 46,
        edge = width - 18,
        bottom = height - 46,
        base = height - 18,
        accent = width - 165
    )
}
pub fn rules_cover(server: &str, date: &str) -> Result<Vec<u8>, Error> {
    let svg = format!(
        r##"{}<text x="64" y="77" font-family="IBM Plex Mono" font-size="15" letter-spacing="3" fill="#bd94ff">11X / COMMUNITY CODE</text><text x="58" y="200" font-family="Barlow Condensed" font-style="italic" font-weight="800" font-size="116" fill="url(#silver)">THE HOUSE RULES</text><text x="65" y="245" font-family="Space Grotesk" font-size="24" fill="#e2d8f4">{}</text><path d="M65 273H1125" stroke="#7235ba"/><text x="65" y="319" font-family="IBM Plex Mono" font-size="18" fill="#d4bcff">01 / RESPECT     02 / PRIVACY     03 / GOOD ENERGY</text><text x="65" y="373" font-family="IBM Plex Mono" font-size="13" fill="#b9a9cf">UPDATED / {}   ·   ELVNX / AVRXT.DEV</text></svg>"##,
        shell(1200, 420),
        xml(&fit(server, 34)),
        xml(date)
    );
    render_svg(&svg, 1200, 420)
}

pub struct MusicCard<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub status: &'a str,
    pub elapsed: &'a str,
    pub duration: &'a str,
    pub progress: f64,
    pub volume: u8,
    pub repeat: &'a str,
    pub listeners: usize,
    pub upcoming: &'a [String],
}
pub fn music(card: &MusicCard<'_>) -> Result<Vec<u8>, Error> {
    let title = fit(card.title, 58);
    let first = title.chars().take(29).collect::<String>();
    let second = title.chars().skip(29).collect::<String>();
    let progress = if card.progress.is_finite() {
        card.progress.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let width = 1072.0 * progress;
    let queue = if card.upcoming.is_empty() {
        vec!["Your next discovery starts with /play".into()]
    } else {
        card.upcoming.iter().take(3).cloned().collect()
    };
    let next=queue.iter().enumerate().map(|(i,title)|format!(r##"<text x="64" y="{}" font-family="IBM Plex Mono" font-size="15" fill="#d1c0e7">{:02} / {}</text>"##,472+i*29,i+1,xml(&fit(title,65)))).collect::<String>();
    let bars = (0..11)
        .map(|i| {
            let h = 35 + ((i * 47 + 19) % 110);
            let x = 860 + i * 23;
            format!(
                r##"<rect x="{x}" y="{}" width="12" height="{h}" rx="3" fill="url(#violet)"/>"##,
                270 - h
            )
        })
        .collect::<String>();
    let svg = format!(
        r##"{}<text x="64" y="65" font-family="IBM Plex Mono" font-size="14" fill="#bd94ff" letter-spacing="3">ELVNX / LISTENING ROOM</text><rect x="64" y="87" width="155" height="28" rx="4" fill="#6526b1"/><text x="77" y="107" font-family="IBM Plex Mono" font-size="12" fill="#fff">{status}</text><g font-family="Barlow Condensed" font-style="italic" font-weight="800" font-size="62" fill="url(#silver)"><text x="60" y="178">{first}</text><text x="60" y="240">{second}</text></g><text x="65" y="282" font-family="Space Grotesk" font-size="21" fill="#c4b4d9">{artist}</text>{bars}<text x="866" y="110" font-family="IBM Plex Mono" font-size="14" letter-spacing="6" fill="#d4bcff">11X / AUDIO</text><path d="M841 303H1127" stroke="#8b35ff"/><text x="65" y="330" font-family="IBM Plex Mono" font-size="13" fill="#d4bcff">VOL / {volume}%<tspan x="260">LOOP / {repeat}</tspan><tspan x="495">LISTENERS / {listeners}</tspan></text><rect x="64" y="365" width="1072" height="7" rx="3" fill="#35204e"/><rect x="64" y="365" width="{width:.2}" height="7" rx="3" fill="url(#violet)"/><circle cx="{knob:.2}" cy="368.5" r="6" fill="#f2e9ff"/><text x="64" y="400" font-family="IBM Plex Mono" font-size="13" fill="#cdb9e7">{elapsed}</text><text x="1136" y="400" text-anchor="end" font-family="IBM Plex Mono" font-size="13" fill="#cdb9e7">{duration}</text><text x="64" y="439" font-family="IBM Plex Mono" font-size="12" letter-spacing="3" fill="#a47cdb">UP NEXT /</text>{next}<text x="1135" y="558" text-anchor="end" font-family="IBM Plex Mono" font-size="11" fill="#ac95c6">AVRXT.DEV / 11X</text></svg>"##,
        shell(1200, 600),
        status = xml(card.status),
        first = xml(&first),
        second = xml(&second),
        artist = xml(&fit(card.artist, 50)),
        volume = card.volume.min(100),
        repeat = xml(card.repeat),
        listeners = card.listeners,
        knob = 64.0 + width,
        elapsed = xml(card.elapsed),
        duration = xml(card.duration)
    );
    render_svg(&svg, 1200, 600)
}
pub fn render(
    title: &str,
    name: &str,
    subtitle: &str,
    stats: &[(&str, String)],
    avatar: Option<&str>,
) -> Result<Vec<u8>, Error> {
    let image=avatar.map(|url|format!(r##"<image x="674" y="92" width="112" height="112" href="{}" clip-path="url(#pfp)" preserveAspectRatio="xMidYMid slice"/>"##,url.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;"))).unwrap_or_else(||r##"<text x="729" y="171" text-anchor="middle" font-family="Barlow Condensed" font-style="italic" font-weight="800" font-size="68" fill="url(#silver)">11X</text>"##.into());
    let mut body = String::new();
    if stats.is_empty() {
        body.push_str(r##"<text x="49" y="354" font-family="Barlow Condensed" font-weight="800" font-style="italic" font-size="121" letter-spacing="5" fill="url(#violet)">ELVNX</text><text x="54" y="399" font-family="IBM Plex Mono" font-size="13" letter-spacing="2" fill="#d4bcff">YOUR NEXT CHAPTER STARTS HERE.</text>"##);
    }
    for (i, (label, value)) in stats.iter().take(6).enumerate() {
        let x = 51 + (i % 3) * 250;
        let y = 252 + (i / 3) * 85;
        body.push_str(&format!(r##"<g transform="translate({x} {y})"><path d="M0 10L10 0H230V66L220 76H0Z" fill="#160b27" fill-opacity=".93" stroke="#5b2b91"/><path d="M0 12V37" stroke="#a65cff" stroke-width="3"/><text x="15" y="24" fill="#bd94ff" font-family="IBM Plex Mono" font-size="10">{}</text><text x="15" y="57" fill="#f6f0ff" font-family="Space Grotesk" font-size="22" font-weight="700">{}</text></g>"##,xml(&fit(label,27)),xml(&fit(value,16))));
    }
    let svg = format!(
        r##"{}<defs><clipPath id="pfp"><circle cx="730" cy="148" r="56"/></clipPath></defs><text x="53" y="61" fill="#bd94ff" font-family="IBM Plex Mono" font-size="11" letter-spacing="2">ELVNX / AVRXT.DEV</text><text x="49" y="118" fill="url(#silver)" font-size="47" font-family="Barlow Condensed" font-style="italic" font-weight="800">{}</text><text x="53" y="173" fill="#f4ecff" font-size="28" font-family="Space Grotesk" font-weight="500">{}</text><path d="M53 189H359" stroke="#8b35ff" stroke-width="2"/><text x="53" y="218" fill="#c6b4dc" font-size="14" font-family="Space Grotesk">{}</text><circle cx="730" cy="148" r="63" fill="#160b27" stroke="#a65cff" stroke-width="2"/>{image}{body}<text x="53" y="440" fill="#ac95c6" font-size="10" font-family="IBM Plex Mono" letter-spacing="2">PRIVATE COLLECTIVE / 11X</text><text x="793" y="440" text-anchor="end" fill="#bd94ff" font-size="10" font-family="IBM Plex Mono">CONNECTED BY US.</text></svg>"##,
        shell(850, 470),
        xml(&fit(title, 31)),
        xml(&fit(name, 26)),
        xml(&fit(subtitle, 64))
    );
    render_svg(&svg, 850, 470)
}

pub fn brand_icon() -> Result<Vec<u8>, Error> {
    let svg = format!(
        r##"{}<path d="M126 139L175 100H411L390 126H187L143 165Z" fill="#bd94ff"/><text x="250" y="343" text-anchor="middle" font-family="Barlow Condensed" font-style="italic" font-weight="800" font-size="251" letter-spacing="-9" fill="url(#silver)">11X</text><path d="M110 379H351L390 340" stroke="url(#violet)" stroke-width="10" fill="none"/><text x="256" y="431" text-anchor="middle" font-family="IBM Plex Mono" font-size="21" letter-spacing="9" fill="#bd94ff">ELVNX</text></svg>"##,
        shell(512, 512)
    );
    render_svg(&svg, 512, 512)
}

pub(crate) fn render_svg(svg: &str, width: u32, height: u32) -> Result<Vec<u8>, Error> {
    let options = resvg::usvg::Options {
        fontdb: fonts(),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(svg, &options)?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| crate::errors::user("Image allocation failed"))?;
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
