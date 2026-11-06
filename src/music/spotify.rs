//! Read public Spotify embed metadata and match it to full playable tracks.
//! Never use preview URLs, Spotify cookies, or the private Lavalink HTTP client.
use super::*;
use futures_util::stream::{self, StreamExt};

#[derive(Clone, Debug)]
struct Metadata {
    id: String,
    title: String,
    artist: String,
    duration: u64,
}

fn parse_metadata(html: &str) -> Result<Vec<Metadata>, Error> {
    let marker = html.find("id=\"__NEXT_DATA__\"")
        .ok_or_else(|| crate::errors::user("Spotify metadata is unavailable. Use a public track, album or playlist link."))?;
    let script = &html[marker..];
    let start = script.find('>').ok_or_else(|| crate::errors::user("Invalid Spotify page"))? + 1;
    let end = script[start..].find("</script>").ok_or_else(|| crate::errors::user("Invalid Spotify page"))? + start;
    let data: Value = serde_json::from_str(&script[start..end])?;
    let entity = &data["props"]["pageProps"]["state"]["data"]["entity"];
    let items: Vec<&Value> = if entity["type"] == "track" {
        vec![entity]
    } else {
        entity["trackList"].as_array().map(|v| v.iter().take(MAX_QUEUE).collect()).unwrap_or_default()
    };
    let tracks: Vec<_> = items.into_iter().filter_map(|v| {
        if v["entityType"].as_str().is_some_and(|t| t != "track") { return None; }
        let title = v["title"].as_str().or_else(|| v["name"].as_str())?;
        let artists = v["artists"].as_array().map(|a| a.iter().filter_map(|v|v["name"].as_str()).collect::<Vec<_>>().join(", "));
        let artist = artists.as_deref().filter(|v| !v.is_empty()).or_else(||v["subtitle"].as_str())?;
        let duration = v["duration"].as_u64()?;
        if title.is_empty() || artist.is_empty() || duration == 0 { return None; }
        Some(Metadata { id: v["uri"].as_str().and_then(|v|v.strip_prefix("spotify:track:")).unwrap_or_default().to_string(), title: title.chars().take(180).collect(), artist: artist.chars().take(120).collect(), duration })
    }).collect();
    if tracks.is_empty() {
        return Err(crate::errors::user("No public Spotify tracks were found. Private playlists and podcast links are not supported."));
    }
    Ok(tracks)
}

async fn metadata(identifier: &str) -> Result<Vec<Metadata>, Error> {
    // identifier has already passed the exact-host, path and 22-character ID checks.
    let path = identifier.strip_prefix("https://open.spotify.com/")
        .ok_or_else(|| crate::errors::user("Invalid Spotify link"))?;
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    let client=CLIENT.get_or_init(||reqwest::Client::builder()
        .user_agent("ELVNX-public-metadata/1.0")
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15)).build().expect("valid public Spotify metadata client"));
    let mut response = client.get(format!("https://open.spotify.com/embed/{path}")).send().await?
        .error_for_status().map_err(|_| crate::errors::user("Spotify metadata could not be loaded. Use a public link or try again later."))?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 { return Err(crate::errors::user("Spotify metadata response is too large")); }
        bytes.extend_from_slice(&chunk);
    }
    parse_metadata(std::str::from_utf8(&bytes)?)
}

fn words(value: &str) -> Vec<String> {
    value.split(|c: char| !c.is_alphanumeric()).filter(|v| !v.is_empty()).map(str::to_lowercase).collect()
}
fn best_match(tracks: Vec<Track>, expected: &Metadata) -> Option<Track> {
    let title_words = words(&expected.title);
    let artist_words = words(expected.artist.split(',').next().unwrap_or(&expected.artist));
    tracks.into_iter().filter_map(|t| {
        if t.info.is_stream || t.info.length.abs_diff(expected.duration) > (expected.duration / 10).max(15000) { return None; }
        let text = words(&format!("{} {}",t.info.title,t.info.author));
        let title_hits = title_words.iter().filter(|w| text.contains(w)).count();
        let artist_hits = artist_words.iter().filter(|w| text.contains(w)).count();
        if title_hits * 4 < title_words.len() * 3 || artist_words.is_empty() || artist_hits < artist_words.len() { return None; }
        let score = title_hits * 10 + artist_hits * 5;
        Some((score,t))
    }).max_by_key(|(score,_)| *score).map(|(_,track)|track)
}

pub(super) async fn load(node: &Service, identifier: &str) -> Result<Vec<Track>, Error> {
    let tracks = metadata(identifier).await?;
    if std::env::var("SPOTIFY_DIRECT_ENABLED").as_deref() == Ok("true") {
        use base64::Engine as _;
        let mut direct = Vec::new();
        for track in tracks {
            if track.id.len()!=22 || !track.id.bytes().all(|b|b.is_ascii_alphanumeric()) { continue; }
            let payload=json!({"id":track.id,"title":track.title,"artist":track.artist,"duration":track.duration}).to_string();
            let query=format!("spdirect:{}",base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload));
            direct.extend(parse_tracks(node.load_identifier(&query).await?)?);
        }
        if direct.is_empty(){return Err(crate::errors::user("No direct Spotify tracks are available for this link"));}
        return Ok(direct);
    }
    let total = tracks.len();
    let results = stream::iter(tracks.into_iter().map(|track| async move {
        let query = format!("ytsearch:{} {}",track.artist,track.title);
        let value = node.load_identifier(&query).await?;
        let candidates = if value["loadType"] == "search" {
            serde_json::from_value::<Vec<Track>>(value["data"].clone())?
        } else { Vec::new() };
        Ok::<_,Error>(best_match(candidates,&track))
    })).buffered(4).collect::<Vec<_>>().await;
    let matched: Vec<_> = results.into_iter().filter_map(|r|r.ok().flatten()).collect();
    tracing::info!(total,matched=matched.len(),"Spotify public metadata matched to playable audio");
    if matched.is_empty() { return Err(crate::errors::user("No close playable matches were found for this Spotify link. Try a YouTube link or song title.")); }
    Ok(matched)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(entity: Value) -> String {
        format!("<script id=\"__NEXT_DATA__\" type=\"application/json\">{}</script>",json!({"props":{"pageProps":{"state":{"data":{"entity":entity}}}}}))
    }
    #[test]
    fn metadata_reads_tracks_and_caps_ordered_collections_without_preview_playback() {
        let track=json!({"type":"track","title":"Song","artists":[{"name":"Artist"}],"duration":200000,"audioPreview":{"url":"https://private.example/preview.mp3"}});
        let parsed=parse_metadata(&page(track)).unwrap();
        assert_eq!(parsed[0].title,"Song"); assert_eq!(parsed[0].artist,"Artist");
        let list:Vec<_>=(0..60).map(|i|json!({"title":format!("Song {i}"),"subtitle":"Artist","duration":200000,"entityType":"track"})).collect();
        let parsed=parse_metadata(&page(json!({"type":"playlist","trackList":list}))).unwrap();
        assert_eq!(parsed.len(),50); assert_eq!(parsed[49].title,"Song 49");
        assert!(parse_metadata(&page(json!({"type":"playlist","trackList":[]}))).is_err());
        assert!(parse_metadata("not a Spotify metadata page").is_err());
    }
    #[test]
    fn matching_rejects_live_wrong_artists_and_wrong_durations() {
        let expected=Metadata{id:String::new(),title:"Test Song".into(),artist:"Test Artist".into(),duration:200000};
        let make=|title:&str,author:&str,length:u64,live:bool|Track{encoded:"test".into(),info:Info{title:title.into(),author:author.into(),length,is_stream:live,is_seekable:true,uri:None}};
        assert!(best_match(vec![make("Test Song","Test Artist",200000,false)],&expected).is_some());
        assert!(best_match(vec![make("Test Song","Other Performer",200000,false)],&expected).is_none());
        assert!(best_match(vec![make("Test Song","Test Artist",200000,true)],&expected).is_none());
        assert!(best_match(vec![make("Test Song","Test Artist",30000,false)],&expected).is_none());
    }
    #[tokio::test]
    #[ignore = "Live public Spotify metadata smoke test"]
    async fn live_spotify_metadata() {
        for path in ["track/4cOdK2wGLETKBW3PvgPWqT","album/4hDok0OAJd57SGIT8xuWJH","playlist/37i9dQZF1DXcBWIGoYBM5M"] {
            let tracks=metadata(&format!("https://open.spotify.com/{path}")).await.unwrap();
            assert!(!tracks.is_empty()); assert!(tracks.len()<=MAX_QUEUE);
            println!("{path}: {} public tracks",tracks.len());
        }
    }
    #[tokio::test]
    #[ignore = "Live audio-node matching; requires private LAVALINK_PASSWORD"]
    async fn live_spotify_match_and_youtube_playlist() {
        let password=std::env::var("LAVALINK_PASSWORD").expect("private node password required");
        let mut headers=reqwest::header::HeaderMap::new();
        let mut auth=reqwest::header::HeaderValue::from_str(&password).unwrap();
        auth.set_sensitive(true);
        headers.insert(reqwest::header::AUTHORIZATION,auth);
        let node=Service {
            db: crate::store::connect("sqlite::memory:").await.unwrap(),
            client: reqwest::Client::builder().default_headers(headers).timeout(Duration::from_secs(20)).redirect(reqwest::redirect::Policy::none()).build().unwrap(),
            url: "http://127.0.0.1:2333".into(),password,guild:s::GuildId::new(1),bot:s::UserId::new(1),
            session:Mutex::new(None),state:Mutex::new(State::default()),operations:Mutex::new(()),request_generation:std::sync::atomic::AtomicU64::new(0),panel_lock:Mutex::new(()),pending_panels:Mutex::new(Vec::new()),voice:Notify::new(),buttons:Mutex::new(HashMap::new()),stay_retry:Mutex::new(None),
        };
        let tracks=node.load("https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT?si=tracking",Source::YouTube).await.unwrap();
        assert_eq!(tracks.len(),1);
        assert!(tracks[0].info.title.to_lowercase().contains("never gonna give you up"));
        assert!(!tracks[0].info.is_stream);
        println!("Spotify metadata matched a full-length playable-source track ({} ms)",tracks[0].info.length);
        let tracks=node.load("https://www.youtube.com/watch?v=abc&list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI",Source::YouTube).await.unwrap();
        assert!(!tracks.is_empty());assert!(tracks.len()<=MAX_QUEUE);
        println!("YouTube playlist loaded {} queued-track candidates",tracks.len());
    }
}
