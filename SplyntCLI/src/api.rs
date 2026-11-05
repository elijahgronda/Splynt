//! A Subsonic client that works the same way as the iPhone app's.
//!
//! Token auth: every call carries `u`, `t = md5(password + salt)`, `s`, and
//! asks for JSON. Provider playlists that octo-fiesta slips into album arrays
//! are dropped here, at the boundary, the same way the iOS client does it.

use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const API_VERSION: &str = "1.16.1";
const CLIENT: &str = "splynt-cli";

#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct Song {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: Option<String>,
    pub album: String,
    pub album_id: Option<String>,
    pub cover: Option<String>,
    pub duration: u32,
    pub track: Option<u32>,
    pub year: Option<u32>,
    pub suffix: Option<String>,
    pub bitrate: Option<u32>,
    pub starred: bool,
    pub explicit: bool,
    pub track_gain: Option<f32>,
    pub album_gain: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_peak: Option<f32>,
}

#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct Album {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub artist_id: Option<String>,
    pub cover: Option<String>,
    pub year: Option<u32>,
    pub song_count: u32,
    pub duration: u32,
    pub starred: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub cover: Option<String>,
    pub album_count: u32,
    pub starred: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub song_count: u32,
    pub cover: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SearchResults {
    pub songs: Vec<Song>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
}

#[derive(Clone, Debug)]
pub enum Lyrics {
    Synced(Vec<(f64, String)>),
    Plain(Vec<String>),
}

#[derive(Clone)]
pub struct Client {
    base: String,
    user: String,
    salt: String,
    token: String,
    agent: ureq::Agent,
}

/// True for `ext-{provider}-playlist-{id}`. Same rule as
/// `ExternalSource.isExternalPlaylist`: at least four segments, the third is
/// the type.
pub fn is_external_playlist(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    if !lower.starts_with("ext-") {
        return false;
    }
    let parts: Vec<&str> = lower.splitn(4, '-').collect();
    parts.len() >= 4 && !parts[1].is_empty() && parts[2] == "playlist"
}

pub fn is_external(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    lower.starts_with("ext-") && lower.splitn(4, '-').count() >= 4
}

pub fn provider_name(id: &str) -> Option<String> {
    if !is_external(id) {
        return None;
    }
    let p = id.splitn(4, '-').nth(1)?.to_ascii_lowercase();
    Some(match p.as_str() {
        "deezer" => "Deezer".into(),
        "qobuz" => "Qobuz".into(),
        "squidwtf" => "SquidWTF".into(),
        "yandex" => "Yandex".into(),
        other => {
            let mut c = other.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => return None,
            }
        }
    })
}

pub fn random_salt() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seed = format!("{nanos}-{}-{:p}", std::process::id(), &nanos);
    format!("{:x}", md5::compute(seed))[..12].to_string()
}

pub fn token_for(password: &str, salt: &str) -> String {
    format!("{:x}", md5::compute(format!("{password}{salt}")))
}

pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn normalize_server(raw: &str) -> String {
    let t = raw.trim().trim_end_matches('/');
    if t.starts_with("http://") || t.starts_with("https://") {
        t.to_string()
    } else {
        format!("https://{t}")
    }
}

fn s(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(x)) => x.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}
fn os(v: &Value, k: &str) -> Option<String> {
    let x = s(v, k);
    if x.is_empty() { None } else { Some(x) }
}
fn n(v: &Value, k: &str) -> Option<u32> {
    match v.get(k) {
        Some(Value::Number(x)) => x.as_u64().map(|x| x as u32),
        Some(Value::String(x)) => x.parse().ok(),
        _ => None,
    }
}
fn f(v: &Value, k: &str) -> Option<f32> {
    match v.get(k) {
        Some(Value::Number(x)) => x.as_f64().map(|x| x as f32),
        Some(Value::String(x)) => x.parse().ok(),
        _ => None,
    }
}
fn arr<'a>(v: &'a Value, path: &[&str]) -> Vec<&'a Value> {
    let mut cur = v;
    for p in path {
        match cur.get(*p) {
            Some(x) => cur = x,
            None => return Vec::new(),
        }
    }
    match cur {
        Value::Array(a) => a.iter().collect(),
        Value::Object(_) => vec![cur],
        _ => Vec::new(),
    }
}

pub fn song(v: &Value) -> Song {
    Song {
        id: s(v, "id"),
        title: os(v, "title").unwrap_or_else(|| "Unknown title".into()),
        artist: os(v, "artist").or_else(|| os(v, "displayArtist")).unwrap_or_else(|| "Unknown artist".into()),
        artist_id: os(v, "artistId"),
        album: s(v, "album"),
        album_id: os(v, "albumId"),
        cover: os(v, "coverArt"),
        duration: n(v, "duration").unwrap_or(0),
        track: n(v, "track"),
        year: n(v, "year"),
        suffix: os(v, "suffix"),
        bitrate: n(v, "bitRate"),
        starred: v.get("starred").is_some(),
        explicit: v.get("explicitStatus").and_then(|x| x.as_str()) == Some("explicit"),
        track_gain: v.get("replayGain").and_then(|r| f(r, "trackGain")),
        album_gain: v.get("replayGain").and_then(|r| f(r, "albumGain")),
        track_peak: v.get("replayGain").and_then(|r| f(r, "trackPeak")),
        album_peak: v.get("replayGain").and_then(|r| f(r, "albumPeak")),
    }
}

pub fn album(v: &Value) -> Album {
    Album {
        id: s(v, "id"),
        name: os(v, "name").or_else(|| os(v, "title")).unwrap_or_else(|| "Unknown album".into()),
        artist: os(v, "artist").or_else(|| os(v, "displayArtist")).unwrap_or_default(),
        artist_id: os(v, "artistId"),
        cover: os(v, "coverArt"),
        year: n(v, "year"),
        song_count: n(v, "songCount").unwrap_or(0),
        duration: n(v, "duration").unwrap_or(0),
        starred: v.get("starred").is_some(),
    }
}

pub fn artist(v: &Value) -> Artist {
    Artist {
        id: s(v, "id"),
        name: os(v, "name").unwrap_or_else(|| "Unknown artist".into()),
        cover: os(v, "coverArt").or_else(|| Some(s(v, "id"))).filter(|x| !x.is_empty()),
        album_count: n(v, "albumCount").unwrap_or(0),
        starred: v.get("starred").is_some(),
    }
}

pub fn playlist(v: &Value) -> Playlist {
    Playlist {
        id: s(v, "id"),
        name: os(v, "name").unwrap_or_else(|| "Untitled".into()),
        owner: s(v, "owner"),
        song_count: n(v, "songCount").unwrap_or(0),
        cover: os(v, "coverArt"),
    }
}

fn albums(list: Vec<&Value>) -> Vec<Album> {
    let mut seen = std::collections::HashSet::new();
    list.into_iter()
        .map(album)
        .filter(|a| !is_external_playlist(&a.id) && seen.insert(a.id.clone()))
        .collect()
}

fn songs(list: Vec<&Value>) -> Vec<Song> {
    list.into_iter().map(song).collect()
}

fn unique_songs(list: Vec<&Value>) -> Vec<Song> {
    let mut seen = std::collections::HashSet::new();
    list.into_iter().map(song).filter(|s| seen.insert(s.id.clone())).collect()
}

impl Client {
    pub fn new(base: &str, user: &str, salt: &str, token: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(8)))
            .timeout_recv_response(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .build()
            .new_agent();
        Self {
            base: base.trim_end_matches('/').to_string(),
            user: user.to_string(),
            salt: salt.to_string(),
            token: token.to_string(),
            agent,
        }
    }

    pub fn server(&self) -> &str {
        &self.base
    }

    pub fn username(&self) -> &str {
        &self.user
    }

    pub fn url(&self, endpoint: &str, params: &[(&str, String)]) -> String {
        let mut url = format!(
            "{}/rest/{}.view?u={}&t={}&s={}&v={}&c={}&f=json",
            self.base,
            endpoint,
            encode(&self.user),
            self.token,
            self.salt,
            API_VERSION,
            CLIENT
        );
        for (k, v) in params {
            url.push('&');
            url.push_str(k);
            url.push('=');
            url.push_str(&encode(v));
        }
        url
    }

    fn call(&self, endpoint: &str, params: &[(&str, String)]) -> Result<Value, String> {
        let url = self.url(endpoint, params);
        let mut resp = self.agent.get(&url).call().map_err(|e| {
            crate::log::error(format!("{endpoint}: {e}"));
            friendly(&e.to_string())
        })?;
        let status = resp.status();
        let body: Value = resp
            .body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_json()
            .map_err(|_| {
                crate::log::error(format!("{endpoint}: HTTP {} with a body that isn't Subsonic JSON", status.as_u16()));
                if status.is_success() {
                    "The server answered with something other than Subsonic JSON. Check the address.".to_string()
                } else {
                    format!("The server answered HTTP {}.", status.as_u16())
                }
            })?;
        let r = body.get("subsonic-response").cloned().ok_or("Not a Subsonic server.")?;
        if r.get("status").and_then(|x| x.as_str()) != Some("ok") {
            let msg = r
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("The server refused the request.");
            crate::log::warn(format!("{endpoint}: server error: {msg}"));
            return Err(msg.to_string());
        }
        Ok(r)
    }

    pub fn ping(&self) -> Result<(), String> {
        self.call("ping", &[]).map(|_| ())
    }

    pub fn album_list(&self, kind: &str, size: u32, offset: u32) -> Result<Vec<Album>, String> {
        let r = self.call(
            "getAlbumList2",
            &[("type", kind.into()), ("size", size.to_string()), ("offset", offset.to_string())],
        )?;
        Ok(albums(arr(&r, &["albumList2", "album"])))
    }

    pub fn album(&self, id: &str) -> Result<(Album, Vec<Song>), String> {
        let r = self.call("getAlbum", &[("id", id.into())])?;
        let a = r.get("album").ok_or("Album not found")?;
        Ok((album(a), unique_songs(arr(a, &["song"]))))
    }

    pub fn artists(&self) -> Result<Vec<Artist>, String> {
        let r = self.call("getArtists", &[])?;
        let mut out = Vec::new();
        for idx in arr(&r, &["artists", "index"]) {
            out.extend(arr(idx, &["artist"]).into_iter().map(artist));
        }
        Ok(out)
    }

    pub fn artist(&self, id: &str) -> Result<(Artist, Vec<Album>), String> {
        let r = self.call("getArtist", &[("id", id.into())])?;
        let a = r.get("artist").ok_or("Artist not found")?;
        Ok((artist(a), albums(arr(a, &["album"]))))
    }

    pub fn top_songs(&self, name: &str, count: u32) -> Result<Vec<Song>, String> {
        let r = self.call("getTopSongs", &[("artist", name.into()), ("count", count.to_string())])?;
        Ok(unique_songs(arr(&r, &["topSongs", "song"])))
    }

    pub fn similar_songs(&self, id: &str, count: u32) -> Result<Vec<Song>, String> {
        let r = self.call("getSimilarSongs2", &[("id", id.into()), ("count", count.to_string())])?;
        Ok(unique_songs(arr(&r, &["similarSongs2", "song"])))
    }

    pub fn playlists(&self) -> Result<Vec<Playlist>, String> {
        let r = self.call("getPlaylists", &[])?;
        Ok(arr(&r, &["playlists", "playlist"]).into_iter().map(playlist).collect())
    }

    pub fn playlist(&self, id: &str) -> Result<(Playlist, Vec<Song>), String> {
        let r = self.call("getPlaylist", &[("id", id.into())])?;
        let p = r.get("playlist").ok_or("Playlist not found")?;
        // Keep duplicates: a playlist may hold the same song twice on purpose.
        Ok((playlist(p), songs(arr(p, &["entry"]))))
    }

    pub fn add_to_playlist(&self, playlist_id: &str, song_id: &str) -> Result<(), String> {
        self.call(
            "updatePlaylist",
            &[("playlistId", playlist_id.into()), ("songIdToAdd", song_id.into())],
        )
        .map(|_| ())
    }

    pub fn starred(&self) -> Result<SearchResults, String> {
        let r = self.call("getStarred2", &[])?;
        Ok(SearchResults {
            songs: unique_songs(arr(&r, &["starred2", "song"])),
            albums: albums(arr(&r, &["starred2", "album"])),
            artists: arr(&r, &["starred2", "artist"]).into_iter().map(artist).collect(),
        })
    }

    pub fn search(&self, query: &str) -> Result<SearchResults, String> {
        // octo-fiesta aggregates several providers, so bound the counts.
        let r = self.call(
            "search3",
            &[
                ("query", query.into()),
                ("songCount", "60".into()),
                ("albumCount", "40".into()),
                ("artistCount", "30".into()),
            ],
        )?;
        Ok(SearchResults {
            songs: unique_songs(arr(&r, &["searchResult3", "song"])),
            albums: albums(arr(&r, &["searchResult3", "album"])),
            artists: arr(&r, &["searchResult3", "artist"]).into_iter().map(artist).collect(),
        })
    }

    pub fn create_playlist(&self, name: &str, ids: &[String]) -> Result<(), String> {
        let mut url = self.url("createPlaylist", &[("name", name.into())]);
        for id in ids.iter().take(400) {
            url.push_str("&songId=");
            url.push_str(&encode(id));
        }
        let mut resp = self.agent.get(&url).call().map_err(|e| friendly(&e.to_string()))?;
        let body: Value = resp.body_mut().read_json().map_err(|e| e.to_string())?;
        match body.pointer("/subsonic-response/status").and_then(|x| x.as_str()) {
            Some("ok") => Ok(()),
            _ => Err(body
                .pointer("/subsonic-response/error/message")
                .and_then(|m| m.as_str())
                .unwrap_or("The server refused to create the playlist.")
                .to_string()),
        }
    }

    pub fn star(&self, id: &str, kind: StarKind, on: bool) -> Result<(), String> {
        let key = match kind {
            StarKind::Song => "id",
            StarKind::Album => "albumId",
            StarKind::Artist => "artistId",
        };
        self.call(if on { "star" } else { "unstar" }, &[(key, id.into())]).map(|_| ())
    }

    pub fn scrobble(&self, id: &str, submission: bool) -> Result<(), String> {
        let mut params = vec![("id", id.to_string()), ("submission", submission.to_string())];
        if submission {
            let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
            params.push(("time", ms.to_string()));
        }
        self.call("scrobble", &params).map(|_| ())
    }

    /// LRCLIB, the public lyrics database. It needs only artist and title and
    /// never sees the server account.
    pub fn lrclib(&self, song: &Song) -> Option<Lyrics> {
        let mut url = format!(
            "https://lrclib.net/api/get?artist_name={}&track_name={}",
            encode(&song.artist),
            encode(&song.title)
        );
        if !song.album.is_empty() {
            url.push_str(&format!("&album_name={}", encode(&song.album)));
        }
        if song.duration > 0 {
            url.push_str(&format!("&duration={}", song.duration));
        }
        let mut resp = self.agent.get(&url).header("User-Agent", "splynt-cli").call().ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let v: Value = resp.body_mut().read_json().ok()?;
        if let Some(synced) = v.get("syncedLyrics").and_then(|x| x.as_str()).filter(|x| !x.trim().is_empty()) {
            let mut lines = Vec::new();
            for l in synced.lines() {
                let Some(rest) = l.strip_prefix('[') else { continue };
                let Some((ts, text)) = rest.split_once(']') else { continue };
                let Some((m, sec)) = ts.split_once(':') else { continue };
                let (Ok(m), Ok(sec)) = (m.parse::<f64>(), sec.parse::<f64>()) else { continue };
                lines.push((m * 60.0 + sec, text.trim().to_string()));
            }
            if !lines.is_empty() {
                return Some(Lyrics::Synced(lines));
            }
        }
        let plain = v.get("plainLyrics").and_then(|x| x.as_str())?;
        if plain.trim().is_empty() {
            return None;
        }
        Some(Lyrics::Plain(plain.lines().map(|l| l.to_string()).collect()))
    }

    pub fn lyrics(&self, song: &Song) -> Option<Lyrics> {
        if let Ok(r) = self.call("getLyricsBySongId", &[("id", song.id.clone())]) {
            for l in arr(&r, &["lyricsList", "structuredLyrics"]) {
                let synced = l.get("synced").and_then(|x| x.as_bool()).unwrap_or(false);
                let lines = arr(l, &["line"]);
                if lines.is_empty() {
                    continue;
                }
                if synced {
                    return Some(Lyrics::Synced(
                        lines
                            .iter()
                            .map(|x| (n(x, "start").unwrap_or(0) as f64 / 1000.0, s(x, "value")))
                            .collect(),
                    ));
                }
                return Some(Lyrics::Plain(lines.iter().map(|x| s(x, "value")).collect()));
            }
        }
        let r = self
            .call("getLyrics", &[("artist", song.artist.clone()), ("title", song.title.clone())])
            .ok()?;
        let text = r.get("lyrics").map(|l| s(l, "value")).unwrap_or_default();
        if text.trim().is_empty() {
            return None;
        }
        Some(Lyrics::Plain(text.lines().map(|l| l.to_string()).collect()))
    }

    pub fn play_queue(&self) -> Option<(Vec<Song>, Option<String>, f64)> {
        let r = self.call("getPlayQueue", &[]).ok()?;
        let pq = r.get("playQueue")?;
        let list = unique_songs(arr(pq, &["entry"]));
        if list.is_empty() {
            return None;
        }
        let pos = pq.get("position").and_then(|p| p.as_f64()).unwrap_or(0.0) / 1000.0;
        Some((list, os(pq, "current"), pos))
    }

    pub fn save_play_queue(&self, ids: &[String], current: Option<&str>, position: f64) {
        let mut url = self.url("savePlayQueue", &[]);
        // Long queues can overflow a URL; 400 ids stays well under common limits.
        for id in ids.iter().take(400) {
            url.push_str("&id=");
            url.push_str(&encode(id));
        }
        if let Some(c) = current {
            url.push_str("&current=");
            url.push_str(&encode(c));
        }
        url.push_str(&format!("&position={}", (position.max(0.0) * 1000.0) as u64));
        let _ = self.agent.get(&url).call();
    }

    pub fn cover_url(&self, id: &str, size: u32) -> String {
        self.url("getCoverArt", &[("id", id.into()), ("size", size.to_string())])
    }

    pub fn stream_url(&self, id: &str, max_bitrate: u32, format: Option<&str>) -> String {
        let mut params = vec![("id", id.to_string())];
        if max_bitrate > 0 {
            params.push(("maxBitRate", max_bitrate.to_string()));
        }
        if let Some(f) = format {
            params.push(("format", f.to_string()));
        }
        if max_bitrate > 0 || format.is_some() {
            // Navidrome then sends a Content-Length for the transcode, which
            // is what lets a transcoded stream seek.
            params.push(("estimateContentLength", "true".into()));
        }
        self.url("stream", &params)
    }

    pub fn fetch_bytes(&self, url: &str, limit: u64) -> Result<Vec<u8>, String> {
        let mut resp = self.agent.get(url).call().map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status().as_u16()));
        }
        resp.body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(|e| e.to_string())
    }

    pub fn open_stream(&self, url: &str) -> Result<(Box<dyn std::io::Read + Send>, Option<u64>), String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(8)))
            .http_status_as_error(false)
            .build()
            .new_agent();
        let resp = agent.get(url).call().map_err(|e| friendly(&e.to_string()))?;
        if !resp.status().is_success() {
            return Err(format!("The server answered HTTP {}.", resp.status().as_u16()));
        }
        let len = resp.body().content_length();
        let reader = resp.into_body().into_reader();
        Ok((Box::new(reader), len))
    }

    /// The same stream from byte `offset`. The bool is true when the server
    /// answered 206 and the body starts at the offset.
    pub fn open_stream_at(&self, url: &str, offset: u64) -> Result<(Box<dyn std::io::Read + Send>, bool), String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(8)))
            .http_status_as_error(false)
            .build()
            .new_agent();
        let resp = agent
            .get(url)
            .header("Range", format!("bytes={offset}-"))
            .call()
            .map_err(|e| friendly(&e.to_string()))?;
        let code = resp.status().as_u16();
        if !(200..300).contains(&code) {
            return Err(format!("HTTP {code}"));
        }
        Ok((Box::new(resp.into_body().into_reader()), code == 206))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StarKind {
    Song,
    Album,
    Artist,
}

fn friendly(raw: &str) -> String {
    let l = raw.to_ascii_lowercase();
    if l.contains("dns") || l.contains("resolve") || l.contains("lookup") {
        "Couldn't find that server. Check the address.".into()
    } else if l.contains("refused") {
        "The server refused the connection. Is it running?".into()
    } else if l.contains("timeout") || l.contains("timed out") {
        "The server took too long to answer.".into()
    } else if l.contains("certificate") || l.contains("tls") {
        format!("TLS failed: {raw}")
    } else {
        raw.to_string()
    }
}
