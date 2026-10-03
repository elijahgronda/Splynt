use crate::connect::SplyntConnectState;
use crate::models::{
    AlbumDetail, AlbumList, AlbumSummary, ArtistDetail, ConnectRequest, ConnectedLibrary, Envelope,
    GenreShelf, HomeOverview, LibraryOverview, LyricsLine, LyricsResult, PlayQueueSnapshot,
    PlaylistDetail, RadioResult, SearchResults, ServerInfo, SongSummary,
};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, Response, StatusCode},
    routing::get,
    Router,
};
use futures_util::{stream, StreamExt};
use rand::{distributions::Alphanumeric, Rng};
use reqwest::{header, Client, Url};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::{
    collections::HashSet,
    path::{Path as FilePath, PathBuf},
    sync::Mutex,
};
use tauri::{Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;

const API_VERSION: &str = "1.16.1";
const CLIENT_NAME: &str = "splynt-desktop";
const KEYRING_SERVICE: &str = "com.splice.desktop";
const KEYRING_ACCOUNT: &str = "active-server";

#[derive(Clone)]
pub(crate) struct ServerSession {
    client: Client,
    base_url: Url,
    username: String,
    token: String,
    salt: String,
}

#[derive(Default)]
pub(crate) struct SessionState(pub(crate) Mutex<Option<ServerSession>>);

pub(crate) struct MediaProxyState {
    base_url: String,
    token: String,
}

/// Transcode policy and the offline switch, owned by the host because the host
/// is what builds the upstream request.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaybackPrefs {
    #[serde(default)]
    stream_bit_rate: u32,
    #[serde(default)]
    stream_format: Option<String>,
    #[serde(default)]
    download_bit_rate: u32,
    #[serde(default)]
    download_format: Option<String>,
    #[serde(default)]
    offline_mode: bool,
}

#[derive(Default)]
pub(crate) struct PlaybackPrefsState(pub(crate) Mutex<PlaybackPrefs>);

impl PlaybackPrefsState {
    fn snapshot(&self) -> PlaybackPrefs {
        self.0.lock().map(|value| value.clone()).unwrap_or_default()
    }
}

/// Only the codecs a Subsonic server is asked to produce. Anything else is
/// dropped rather than forwarded, so a bad value cannot silently yield no audio.
fn sanitized_format(value: &Option<String>) -> Option<&str> {
    match value.as_deref() {
        Some(format @ ("mp3" | "aac" | "opus" | "wav")) => Some(format),
        _ => None,
    }
}

#[derive(Default)]
pub(crate) struct DownloadState {
    manifest_lock: tokio::sync::Mutex<()>,
    cancelled: Mutex<HashSet<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadItem {
    song: SongSummary,
    bytes: u64,
    downloaded_at: u64,
    file_name: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadManifest {
    version: u32,
    #[serde(default)]
    items: Vec<DownloadItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    id: String,
    received: u64,
    total: Option<u64>,
    status: &'static str,
    message: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CacheStats {
    files: u64,
    bytes: u64,
}

#[derive(Clone)]
struct MediaRouterState {
    app: tauri::AppHandle,
    token: String,
}

#[derive(Deserialize)]
struct MediaQuery {
    id: String,
    token: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedCredentials {
    server: String,
    username: String,
    password: String,
    allows_self_signed: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CredentialStore {
    active_profile_id: Option<String>,
    #[serde(default)]
    profiles: Vec<SavedProfile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedProfile {
    id: String,
    credentials: SavedCredentials,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SavedProfileSummary {
    id: String,
    display_host: String,
    username: String,
    allows_self_signed: bool,
    is_active: bool,
}

#[tauri::command]
pub(crate) async fn connect_server(
    request: ConnectRequest,
    state: tauri::State<'_, SessionState>,
    connect_state: tauri::State<'_, SplyntConnectState>,
) -> Result<ConnectedLibrary, String> {
    let remember_me = request.remember_me;
    let saved = SavedCredentials {
        server: request.server.clone(),
        username: request.username.clone(),
        password: request.password.clone(),
        allows_self_signed: request.allows_self_signed,
    };
    let connect_identity = (
        request.server.clone(),
        request.username.clone(),
        request.password.clone(),
    );
    let library = establish_session(request, &state).await?;
    let _ = connect_state.configure(
        &connect_identity.0,
        &connect_identity.1,
        &connect_identity.2,
    );
    if remember_me {
        // The valid login remains active even if this platform's secure store
        // is temporarily unavailable.
        let _ = tauri::async_runtime::spawn_blocking(move || save_profile(saved)).await;
    } else {
        let _ = tauri::async_runtime::spawn_blocking(clear_active_profile).await;
    }
    Ok(library)
}

async fn establish_session(
    request: ConnectRequest,
    state: &tauri::State<'_, SessionState>,
) -> Result<ConnectedLibrary, String> {
    let base_url = normalize_base_url(&request.server)?;
    let client = Client::builder()
        .danger_accept_invalid_certs(request.allows_self_signed)
        .connect_timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|error| format!("Could not create a secure connection: {error}"))?;
    let salt: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(16)
        .map(char::from)
        .collect();
    let token = format!(
        "{:x}",
        md5::compute(format!("{}{}", request.password, salt))
    );

    let ping = api_request(
        &client,
        &base_url,
        "ping.view",
        &request.username,
        &token,
        &salt,
        &[],
    )
    .await?;
    ensure_ok(&ping)?;
    let album_response = api_request(
        &client,
        &base_url,
        "getAlbumList2.view",
        &request.username,
        &token,
        &salt,
        &[("type", "newest"), ("size", "50")],
    )
    .await?;
    ensure_ok(&album_response)?;
    let albums = album_response
        .response
        .album_list2
        .map(|list| list.album)
        .unwrap_or_default();
    let server = ServerInfo {
        display_host: display_host(&base_url),
        username: request.username.clone(),
        server_type: ping.response.server_type,
        server_version: ping.response.server_version,
        api_version: ping.response.version,
    };

    *state
        .0
        .lock()
        .map_err(|_| "The desktop session could not be opened.".to_string())? =
        Some(ServerSession {
            client,
            base_url,
            username: request.username,
            token,
            salt,
        });
    Ok(ConnectedLibrary { server, albums })
}

#[tauri::command]
pub(crate) async fn restore_session(
    state: tauri::State<'_, SessionState>,
    connect_state: tauri::State<'_, SplyntConnectState>,
) -> Result<Option<ConnectedLibrary>, String> {
    let saved = tauri::async_runtime::spawn_blocking(load_active_profile)
        .await
        .map_err(|_| "The saved login could not be read.".to_string())?
        .ok()
        .flatten();
    let Some(saved) = saved else { return Ok(None) };
    let request = ConnectRequest {
        server: saved.server,
        username: saved.username,
        password: saved.password,
        allows_self_signed: saved.allows_self_signed,
        remember_me: true,
    };
    let connect_identity = (
        request.server.clone(),
        request.username.clone(),
        request.password.clone(),
    );
    let library = establish_session(request, &state).await?;
    let _ = connect_state.configure(
        &connect_identity.0,
        &connect_identity.1,
        &connect_identity.2,
    );
    Ok(Some(library))
}

#[tauri::command]
pub(crate) async fn list_profiles() -> Result<Vec<SavedProfileSummary>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let store = load_credential_store()?;
        let active_profile_id = store.active_profile_id;
        Ok(store
            .profiles
            .into_iter()
            .filter_map(|profile| {
                let base = normalize_base_url(&profile.credentials.server).ok()?;
                Some(SavedProfileSummary {
                    is_active: active_profile_id.as_deref() == Some(profile.id.as_str()),
                    id: profile.id,
                    display_host: display_host(&base),
                    username: profile.credentials.username,
                    allows_self_signed: profile.credentials.allows_self_signed,
                })
            })
            .collect())
    })
    .await
    .map_err(|_| "Saved profiles could not be read.".to_string())?
}

#[tauri::command]
pub(crate) async fn connect_profile(
    profile_id: String,
    state: tauri::State<'_, SessionState>,
    connect_state: tauri::State<'_, SplyntConnectState>,
) -> Result<ConnectedLibrary, String> {
    let requested_id = profile_id.clone();
    let saved = tauri::async_runtime::spawn_blocking(move || {
        load_credential_store()?
            .profiles
            .into_iter()
            .find(|profile| profile.id == requested_id)
            .map(|profile| profile.credentials)
            .ok_or_else(|| "That saved profile is no longer available.".to_string())
    })
    .await
    .map_err(|_| "The saved profile could not be read.".to_string())??;
    let request = ConnectRequest {
        server: saved.server.clone(),
        username: saved.username.clone(),
        password: saved.password.clone(),
        allows_self_signed: saved.allows_self_signed,
        remember_me: true,
    };
    let library = establish_session(request, &state).await?;
    let _ = connect_state.configure(&saved.server, &saved.username, &saved.password);
    tauri::async_runtime::spawn_blocking(move || set_active_profile(&profile_id))
        .await
        .map_err(|_| "The active profile could not be saved.".to_string())??;
    Ok(library)
}

#[tauri::command]
pub(crate) async fn forget_profile(profile_id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || remove_profile(&profile_id))
        .await
        .map_err(|_| "The saved profile could not be removed.".to_string())?
}

#[tauri::command]
pub(crate) async fn disconnect_server(
    forget_saved_login: Option<bool>,
    state: tauri::State<'_, SessionState>,
    connect_state: tauri::State<'_, SplyntConnectState>,
) -> Result<(), String> {
    // A visible "Sign out" must actually remove the saved credential. Do that
    // before tearing down the live session so a credential-store failure does
    // not leave the UI signed out while the next launch silently signs back in.
    if forget_saved_login.unwrap_or(true) {
        tauri::async_runtime::spawn_blocking(remove_active_profile)
            .await
            .map_err(|_| "The saved login could not be removed.".to_string())??;
    }
    *state
        .0
        .lock()
        .map_err(|_| "The desktop session could not be closed.".to_string())? = None;
    connect_state.stop();
    Ok(())
}

#[tauri::command]
pub(crate) async fn load_home(
    state: tauri::State<'_, SessionState>,
) -> Result<HomeOverview, String> {
    let session = current_session(&state)?;
    let (newest, recent, frequent, random, genres) = futures_util::try_join!(
        album_list(&session, "newest", 30),
        album_list(&session, "recent", 24),
        album_list(&session, "frequent", 24),
        album_list(&session, "random", 24),
        genre_shelves(&session),
    )?;
    Ok(HomeOverview {
        newest,
        recent,
        frequent,
        random,
        genres,
    })
}

/// The largest few genres in the library, each with a shelf of albums. A server
/// without getGenres simply contributes no shelves rather than failing Home.
async fn genre_shelves(session: &ServerSession) -> Result<Vec<GenreShelf>, String> {
    const SHELVES: usize = 4;
    let Ok(response) = session_request(session, "getGenres.view", &[]).await else {
        return Ok(Vec::new());
    };
    let mut genres = response.response.genres.unwrap_or_default().genre;
    genres.retain(|genre| !genre.name.trim().is_empty() && genre.album_count > 0);
    genres.sort_by_key(|a| std::cmp::Reverse(a.album_count));
    genres.truncate(SHELVES);

    let requests = genres.into_iter().map(|genre| {
        let session = session.clone();
        async move {
            let albums = session_request(
                &session,
                "getAlbumList2.view",
                &[("type", "byGenre"), ("genre", &genre.name), ("size", "20")],
            )
            .await
            .ok()
            .and_then(|response| response.response.album_list2)
            .unwrap_or_else(AlbumList::default)
            .album;
            GenreShelf {
                name: genre.name,
                albums,
            }
        }
    });
    let shelves: Vec<GenreShelf> = stream::iter(requests).buffered(4).collect().await;
    Ok(shelves
        .into_iter()
        .filter(|shelf| !shelf.albums.is_empty())
        .collect())
}

#[tauri::command]
pub(crate) async fn load_library(
    state: tauri::State<'_, SessionState>,
) -> Result<LibraryOverview, String> {
    let session = current_session(&state)?;
    let (albums, artists_response, playlists_response, starred_response) = futures_util::try_join!(
        all_albums(&session),
        session_request(&session, "getArtists.view", &[]),
        session_request(&session, "getPlaylists.view", &[]),
        session_request(&session, "getStarred2.view", &[]),
    )?;
    let artists = artists_response
        .response
        .artists
        .map(|container| {
            container
                .index
                .into_iter()
                .flat_map(|index| index.artist)
                .collect()
        })
        .unwrap_or_default();
    let playlists = playlists_response
        .response
        .playlists
        .map(|container| container.playlist)
        .unwrap_or_default();
    let starred = starred_response.response.starred2.unwrap_or_default();
    Ok(LibraryOverview {
        albums,
        artists,
        playlists,
        starred_songs: starred.song,
        starred_albums: starred.album,
        starred_artists: starred.artist,
    })
}

#[tauri::command]
pub(crate) async fn search_catalog(
    query: String,
    state: tauri::State<'_, SessionState>,
) -> Result<SearchResults, String> {
    if query.trim().is_empty() {
        return Ok(SearchResults::default());
    }
    let session = current_session(&state)?;
    let response = session_request(
        &session,
        "search3.view",
        &[
            ("query", query.trim()),
            ("songCount", "50"),
            ("albumCount", "30"),
            ("artistCount", "20"),
        ],
    )
    .await?;
    Ok(response.response.search_result3.unwrap_or_default())
}

#[tauri::command]
pub(crate) async fn get_album(
    id: String,
    state: tauri::State<'_, SessionState>,
) -> Result<AlbumDetail, String> {
    let session = current_session(&state)?;
    session_request(&session, "getAlbum.view", &[("id", &id)])
        .await?
        .response
        .album
        .ok_or_else(|| "The server did not return that album.".to_string())
}

#[tauri::command]
pub(crate) async fn get_playlist(
    id: String,
    state: tauri::State<'_, SessionState>,
) -> Result<PlaylistDetail, String> {
    let session = current_session(&state)?;
    session_request(&session, "getPlaylist.view", &[("id", &id)])
        .await?
        .response
        .playlist
        .ok_or_else(|| "The server did not return that playlist.".to_string())
}

#[tauri::command]
pub(crate) async fn get_artist(
    id: String,
    state: tauri::State<'_, SessionState>,
) -> Result<ArtistDetail, String> {
    let session = current_session(&state)?;
    let mut artist = session_request(&session, "getArtist.view", &[("id", &id)])
        .await?
        .response
        .artist
        .ok_or_else(|| "The server did not return that artist.".to_string())?;
    artist.top_songs = session_request(
        &session,
        "getTopSongs.view",
        &[("artist", artist.name.as_str()), ("count", "20")],
    )
    .await
    .ok()
    .and_then(|response| response.response.top_songs)
    .unwrap_or_default()
    .song;

    let own: HashSet<String> = artist.albums.iter().map(|album| album.id.clone()).collect();
    artist.appearances = session_request(
        &session,
        "search3.view",
        &[
            ("query", artist.name.as_str()),
            ("albumCount", "40"),
            ("songCount", "0"),
            ("artistCount", "0"),
        ],
    )
    .await
    .ok()
    .and_then(|response| response.response.search_result3)
    .map(|results| results.albums)
    .unwrap_or_default()
    .into_iter()
    .filter(|album| {
        !own.contains(&album.id)
            && album.artist_id.as_deref() != Some(artist.id.as_str())
            && is_guest_credit(&album.artist, &artist.name)
    })
    .take(20)
    .collect();
    Ok(artist)
}

/// True when `credit` names `artist` alongside someone else ("X & Geese",
/// "Y feat. Geese"). search3 also matches album titles, which is how
/// "Genèse" and "Geese in the Flyway" ended up under Appears on; a credit
/// that is exactly the artist is their own discography, not an appearance.
pub(crate) fn is_guest_credit(credit: &str, artist: &str) -> bool {
    let wanted = artist.trim().to_lowercase();
    let credit = credit.trim().to_lowercase();
    if wanted.is_empty() || credit == wanted {
        return false;
    }
    let mut parts = vec![credit];
    for separator in [
        ",",
        ";",
        "/",
        "&",
        " feat. ",
        " feat ",
        " ft. ",
        " ft ",
        " featuring ",
        " with ",
        " and ",
        " x ",
        " vs. ",
        " vs ",
    ] {
        parts = parts
            .iter()
            .flat_map(|part| {
                part.split(separator)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .collect();
    }
    parts.iter().any(|part| part.trim() == wanted)
}

#[tauri::command]
pub(crate) async fn get_songs_by_ids(
    ids: Vec<String>,
    state: tauri::State<'_, SessionState>,
) -> Result<Vec<SongSummary>, String> {
    let session = current_session(&state)?;
    let requests = ids.into_iter().take(1000).map(|id| {
        let session = session.clone();
        async move {
            session_request(&session, "getSong.view", &[("id", id.as_str())])
                .await
                .map(|envelope| envelope.response.song)
        }
    });
    let results: Vec<Result<Option<SongSummary>, String>> =
        stream::iter(requests).buffered(8).collect().await;
    // One song that no longer resolves, such as an `ext-` id that has moved,
    // used to fail the whole batch, and a mirrored or handed-off queue lost
    // every track around it. Only a batch where nothing resolved is an error.
    let mut songs = Vec::with_capacity(results.len());
    let mut first_error = None;
    for result in results {
        match result {
            Ok(Some(song)) => songs.push(song),
            Ok(None) => {}
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    match first_error {
        Some(error) if songs.is_empty() => Err(error),
        _ => Ok(songs),
    }
}

#[tauri::command]
pub(crate) async fn set_starred(
    id: String,
    item_type: String,
    starred: bool,
    state: tauri::State<'_, SessionState>,
) -> Result<(), String> {
    let session = current_session(&state)?;
    let key = match item_type.as_str() {
        "song" => "id",
        "album" => "albumId",
        "artist" => "artistId",
        _ => return Err("Splynt cannot star that item type.".to_string()),
    };
    let endpoint = if starred { "star.view" } else { "unstar.view" };
    session_request(&session, endpoint, &[(key, &id)]).await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn create_playlist(
    name: String,
    state: tauri::State<'_, SessionState>,
) -> Result<PlaylistDetail, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Enter a playlist name.".to_string());
    }
    let session = current_session(&state)?;
    session_request(&session, "createPlaylist.view", &[("name", trimmed)])
        .await?
        .response
        .playlist
        .ok_or_else(|| "The server created the playlist but returned no details.".to_string())
}

#[tauri::command]
pub(crate) async fn save_play_queue(
    ids: Vec<String>,
    current_id: Option<String>,
    position: f64,
    state: tauri::State<'_, SessionState>,
) -> Result<(), String> {
    let session = current_session(&state)?;
    let ids = ids.into_iter().take(1_000).collect::<Vec<_>>();
    let mut owned = ids.iter().map(|id| ("id", id.clone())).collect::<Vec<_>>();
    if let Some(current) = current_id.filter(|id| ids.iter().any(|entry| entry == id)) {
        owned.push(("current", current));
    }
    let safe_position = if position.is_finite() {
        (position.max(0.0) * 1_000.0).round().min(u64::MAX as f64) as u64
    } else {
        0
    };
    owned.push(("position", safe_position.to_string()));
    let borrowed = owned
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect::<Vec<_>>();
    session_request(&session, "savePlayQueue.view", &borrowed).await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn get_play_queue(
    state: tauri::State<'_, SessionState>,
) -> Result<Option<PlayQueueSnapshot>, String> {
    let session = current_session(&state)?;
    let queue = session_request(&session, "getPlayQueue.view", &[])
        .await?
        .response
        .play_queue;
    Ok(queue.and_then(|queue| {
        (!queue.songs.is_empty()).then_some(PlayQueueSnapshot {
            songs: queue.songs,
            current_id: queue.current,
            position: queue.position as f64 / 1_000.0,
        })
    }))
}

#[tauri::command]
pub(crate) async fn get_lyrics(
    id: String,
    source: Option<String>,
    artist: Option<String>,
    title: Option<String>,
    album: Option<String>,
    duration: Option<f64>,
    state: tauri::State<'_, SessionState>,
) -> Result<LyricsResult, String> {
    let session = current_session(&state)?;
    let source = source.as_deref().unwrap_or("auto");
    if source != "lrclib" {
        match server_lyrics(&session, &id).await {
            Ok(result) if !result.lines.is_empty() => return Ok(result),
            Ok(_) if source == "server" => return Ok(LyricsResult::default()),
            Err(error) if source == "server" => return Err(error),
            _ => {}
        }
    }
    if source == "server" {
        return Ok(LyricsResult::default());
    }
    let (Some(artist), Some(title)) = (
        artist.filter(|value| !value.trim().is_empty()),
        title.filter(|value| !value.trim().is_empty()),
    ) else {
        return Ok(LyricsResult::default());
    };
    Ok(lrclib_lyrics(&artist, &title, album.as_deref(), duration)
        .await
        .unwrap_or_default())
}

async fn server_lyrics(session: &ServerSession, id: &str) -> Result<LyricsResult, String> {
    let response = session_request(session, "getLyricsBySongId.view", &[("id", id)]).await?;
    let Some(lyrics) = response
        .response
        .lyrics_list
        .and_then(|list| list.structured_lyrics.into_iter().next())
    else {
        return Ok(LyricsResult::default());
    };
    Ok(LyricsResult {
        synced: lyrics.synced,
        lines: lyrics
            .line
            .into_iter()
            .map(|mut line| {
                line.start /= 1000.0;
                line
            })
            .filter(|line| !line.value.trim().is_empty())
            .collect(),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibResponse {
    synced_lyrics: Option<String>,
    plain_lyrics: Option<String>,
}

/// Mirrors the iOS source choice. LRCLIB is always best-effort: a public
/// service outage must produce an honest empty state, never break playback or
/// turn an otherwise successful server request into an error.
async fn lrclib_lyrics(
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration: Option<f64>,
) -> Option<LyricsResult> {
    let mut url = Url::parse("https://lrclib.net/api/get").ok()?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("artist_name", artist);
        query.append_pair("track_name", title);
        if let Some(album) = album.filter(|value| !value.trim().is_empty()) {
            query.append_pair("album_name", album);
        }
        if let Some(duration) = duration.filter(|value| value.is_finite() && *value > 0.0) {
            query.append_pair("duration", &duration.round().to_string());
        }
    }
    let client = Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .ok()?;
    let response = client.get(url).send().await.ok()?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return None;
    }
    if !response.status().is_success() {
        return None;
    }
    let body: LrclibResponse = response.json().await.ok()?;
    if let Some(synced) = body.synced_lyrics {
        let lines = parse_lrc(&synced);
        if !lines.is_empty() {
            return Some(LyricsResult {
                synced: true,
                lines,
            });
        }
    }
    let lines = body
        .plain_lyrics
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !matches!(*line, "♪" | "♫" | "♪♪"))
        .map(|value| LyricsLine {
            start: 0.0,
            value: value.to_string(),
        })
        .collect::<Vec<_>>();
    (!lines.is_empty()).then_some(LyricsResult {
        synced: false,
        lines,
    })
}

fn parse_lrc(source: &str) -> Vec<LyricsLine> {
    let mut lines = source
        .lines()
        .filter_map(|raw| {
            let raw = raw.trim();
            let close = raw.find(']')?;
            let timestamp = raw.strip_prefix('[')?.get(..close - 1)?;
            let (minutes, seconds) = timestamp.split_once(':')?;
            let minutes = minutes.parse::<f64>().ok()?;
            let seconds = seconds.parse::<f64>().ok()?;
            let value = raw.get(close + 1..)?.trim();
            if value.is_empty() || matches!(value, "♪" | "♫" | "♪♪") {
                return None;
            }
            Some(LyricsLine {
                start: minutes * 60.0 + seconds,
                value: value.to_string(),
            })
        })
        .collect::<Vec<_>>();
    lines.sort_by(|left, right| left.start.total_cmp(&right.start));
    lines
}

#[tauri::command]
pub(crate) async fn get_radio(
    seed_id: String,
    title: String,
    count: Option<u32>,
    state: tauri::State<'_, SessionState>,
) -> Result<RadioResult, String> {
    let session = current_session(&state)?;
    let size = count.unwrap_or(40).clamp(10, 100).to_string();
    let similar = session_request(
        &session,
        "getSimilarSongs2.view",
        &[("id", &seed_id), ("count", &size)],
    )
    .await;
    let songs = match similar {
        Ok(response) => response.response.similar_songs2.unwrap_or_default().song,
        Err(_) => {
            session_request(&session, "getRandomSongs.view", &[("size", &size)])
                .await?
                .response
                .random_songs
                .unwrap_or_default()
                .song
        }
    };
    Ok(RadioResult {
        title: if title.trim().is_empty() {
            "Splynt Radio".to_string()
        } else {
            title.trim().to_string()
        },
        songs,
    })
}

#[tauri::command]
pub(crate) async fn scrobble(
    id: String,
    submission: bool,
    state: tauri::State<'_, SessionState>,
) -> Result<(), String> {
    let session = current_session(&state)?;
    let submission = if submission { "true" } else { "false" };
    session_request(
        &session,
        "scrobble.view",
        &[("id", &id), ("submission", submission)],
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn add_song_to_playlist(
    playlist_id: String,
    song_id: String,
    state: tauri::State<'_, SessionState>,
) -> Result<(), String> {
    let session = current_session(&state)?;
    session_request(
        &session,
        "updatePlaylist.view",
        &[("playlistId", &playlist_id), ("songIdToAdd", &song_id)],
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn add_songs_to_playlist(
    playlist_id: String,
    song_ids: Vec<String>,
    state: tauri::State<'_, SessionState>,
) -> Result<(), String> {
    if song_ids.is_empty() {
        return Ok(());
    }
    if song_ids.len() > 5_000 {
        return Err("Too many songs were selected for one playlist update.".to_string());
    }
    let session = current_session(&state)?;
    let mut params: Vec<(&str, String)> = vec![("playlistId", playlist_id)];
    params.extend(song_ids.into_iter().map(|id| ("songIdToAdd", id)));
    let borrowed = params
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect::<Vec<_>>();
    session_request(&session, "updatePlaylist.view", &borrowed).await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn remove_song_from_playlist(
    playlist_id: String,
    song_index: u32,
    state: tauri::State<'_, SessionState>,
) -> Result<(), String> {
    let session = current_session(&state)?;
    let index = song_index.to_string();
    session_request(
        &session,
        "updatePlaylist.view",
        &[("playlistId", &playlist_id), ("songIndexToRemove", &index)],
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn rename_playlist(
    playlist_id: String,
    name: String,
    state: tauri::State<'_, SessionState>,
) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Enter a playlist name.".to_string());
    }
    let session = current_session(&state)?;
    session_request(
        &session,
        "updatePlaylist.view",
        &[("playlistId", &playlist_id), ("name", trimmed)],
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn delete_playlist(
    playlist_id: String,
    state: tauri::State<'_, SessionState>,
) -> Result<(), String> {
    let session = current_session(&state)?;
    session_request(&session, "deletePlaylist.view", &[("id", &playlist_id)]).await?;
    Ok(())
}

/// Subsonic has no reorder verb: updatePlaylist only adds and removes. The
/// order is rewritten by clearing the playlist and re-adding in the new
/// sequence, which is why the caller is expected to confirm first.
#[tauri::command]
pub(crate) async fn set_playlist_songs(
    playlist_id: String,
    song_ids: Vec<String>,
    state: tauri::State<'_, SessionState>,
) -> Result<PlaylistDetail, String> {
    if song_ids.len() > 5_000 {
        return Err("That playlist is too large to reorder.".to_string());
    }
    let session = current_session(&state)?;
    let existing = session_request(&session, "getPlaylist.view", &[("id", &playlist_id)])
        .await?
        .response
        .playlist
        .ok_or_else(|| "The server did not return that playlist.".to_string())?;

    let mut params: Vec<(&str, String)> = vec![("playlistId", playlist_id.clone())];
    // Descending, so each removal index still refers to the original list.
    for index in (0..existing.songs.len()).rev() {
        params.push(("songIndexToRemove", index.to_string()));
    }
    for id in &song_ids {
        params.push(("songIdToAdd", id.clone()));
    }
    let borrowed: Vec<(&str, &str)> = params
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    session_request(&session, "updatePlaylist.view", &borrowed).await?;

    session_request(&session, "getPlaylist.view", &[("id", &playlist_id)])
        .await?
        .response
        .playlist
        .ok_or_else(|| "The playlist was updated but could not be reloaded.".to_string())
}

#[tauri::command]
pub(crate) async fn list_downloads(
    app: tauri::AppHandle,
    state: tauri::State<'_, SessionState>,
    downloads: tauri::State<'_, DownloadState>,
) -> Result<Vec<DownloadItem>, String> {
    let _guard = downloads.manifest_lock.lock().await;
    Ok(
        read_download_manifest(&current_download_profile_dir(&app, &state)?)
            .await
            .items,
    )
}

#[tauri::command]
pub(crate) async fn download_song(
    song: SongSummary,
    app: tauri::AppHandle,
    state: tauri::State<'_, SessionState>,
    downloads: tauri::State<'_, DownloadState>,
    prefs: tauri::State<'_, PlaybackPrefsState>,
) -> Result<DownloadItem, String> {
    if song.id.is_empty() || song.id.len() > 1_024 {
        return Err("That song cannot be downloaded.".to_string());
    }
    let prefs = prefs.snapshot();
    if prefs.offline_mode {
        return Err("Offline mode is on. Turn it off to download new music.".to_string());
    }
    let requested_format = sanitized_format(&prefs.download_format).map(str::to_string);
    let session = current_session(&state)?;
    if let Ok(mut cancelled) = downloads.cancelled.lock() {
        cancelled.remove(&song.id);
    }
    let directory = download_profile_dir(&app, &session)?;
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|_| "The downloads folder could not be created.".to_string())?;
    let file_name = download_file_name(&song, requested_format.as_deref());
    let destination = directory.join(&file_name);
    let partial = directory.join(format!("{file_name}.part"));
    {
        let _guard = downloads.manifest_lock.lock().await;
        let manifest = read_download_manifest(&directory).await;
        if let Some(item) = manifest.items.iter().find(|item| item.song.id == song.id) {
            if destination.is_file() {
                return Ok(item.clone());
            }
        }
    }

    let existing = tokio::fs::metadata(&partial)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let mut url = match requested_format.as_deref() {
        Some(format) => {
            let mut url = authenticated_url(&session, "stream.view")?;
            url.query_pairs_mut().append_pair("format", format);
            if prefs.download_bit_rate > 0 && format != "wav" {
                url.query_pairs_mut()
                    .append_pair("maxBitRate", &prefs.download_bit_rate.to_string());
            }
            url
        }
        None => authenticated_url(&session, "download.view")?,
    };
    url.query_pairs_mut().append_pair("id", &song.id);
    let mut request = session.client.get(url);
    if existing > 0 {
        request = request.header(header::RANGE, format!("bytes={existing}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|error| connection_error(error, &session.base_url))?;
    if !response.status().is_success() {
        return Err(format!(
            "The server could not download this track (HTTP {}).",
            response.status().as_u16()
        ));
    }
    let resumed = existing > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let starting_bytes = if resumed { existing } else { 0 };
    let total = response
        .headers()
        .get(header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.rsplit('/').next())
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| {
            response
                .content_length()
                .map(|length| length + starting_bytes)
        });
    let mut options = tokio::fs::OpenOptions::new();
    options.create(true).write(true);
    if resumed {
        options.append(true);
    } else {
        options.truncate(true);
    }
    let mut file = options
        .open(&partial)
        .await
        .map_err(|_| "The download file could not be opened.".to_string())?;
    let mut received = starting_bytes;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let is_cancelled = downloads
            .cancelled
            .lock()
            .map(|cancelled| cancelled.contains(&song.id))
            .unwrap_or(false);
        if is_cancelled {
            emit_download_progress(&app, &song.id, received, total, "paused", None);
            return Err("Download paused. Resume it whenever you are ready.".to_string());
        }
        let chunk =
            chunk.map_err(|_| "The download was interrupted. It can be resumed.".to_string())?;
        file.write_all(&chunk)
            .await
            .map_err(|_| "The downloaded data could not be saved.".to_string())?;
        received += chunk.len() as u64;
        emit_download_progress(&app, &song.id, received, total, "downloading", None);
    }
    file.flush()
        .await
        .map_err(|_| "The download could not be finalized.".to_string())?;
    drop(file);
    if destination.exists() {
        let _ = tokio::fs::remove_file(&destination).await;
    }
    tokio::fs::rename(&partial, &destination)
        .await
        .map_err(|_| "The download could not be finalized.".to_string())?;
    let item = DownloadItem {
        song,
        bytes: received,
        downloaded_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        file_name,
    };
    {
        let _guard = downloads.manifest_lock.lock().await;
        let mut manifest = read_download_manifest(&directory).await;
        manifest
            .items
            .retain(|existing| existing.song.id != item.song.id);
        manifest.items.push(item.clone());
        write_download_manifest(&directory, &manifest).await?;
    }
    emit_download_progress(&app, &item.song.id, received, total, "complete", None);
    Ok(item)
}

#[tauri::command]
pub(crate) fn pause_download(
    id: String,
    downloads: tauri::State<'_, DownloadState>,
) -> Result<(), String> {
    downloads
        .cancelled
        .lock()
        .map_err(|_| "The download could not be paused.".to_string())?
        .insert(id);
    Ok(())
}

#[tauri::command]
pub(crate) async fn remove_download(
    id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, SessionState>,
    downloads: tauri::State<'_, DownloadState>,
) -> Result<(), String> {
    let _guard = downloads.manifest_lock.lock().await;
    let directory = current_download_profile_dir(&app, &state)?;
    let mut manifest = read_download_manifest(&directory).await;
    if let Some(item) = manifest.items.iter().find(|item| item.song.id == id) {
        let _ = tokio::fs::remove_file(directory.join(&item.file_name)).await;
        let _ = tokio::fs::remove_file(directory.join(format!("{}.part", item.file_name))).await;
    }
    manifest.items.retain(|item| item.song.id != id);
    write_download_manifest(&directory, &manifest).await
}

#[tauri::command]
pub(crate) async fn clear_downloads(
    app: tauri::AppHandle,
    state: tauri::State<'_, SessionState>,
    downloads: tauri::State<'_, DownloadState>,
) -> Result<(), String> {
    let _guard = downloads.manifest_lock.lock().await;
    let directory = current_download_profile_dir(&app, &state)?;
    if directory.exists() {
        tokio::fs::remove_dir_all(&directory)
            .await
            .map_err(|_| "The downloads folder could not be cleared.".to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn set_playback_prefs(
    prefs: PlaybackPrefs,
    state: tauri::State<'_, PlaybackPrefsState>,
) -> Result<(), String> {
    *state
        .0
        .lock()
        .map_err(|_| "Playback preferences could not be applied.".to_string())? = prefs;
    Ok(())
}

#[tauri::command]
pub(crate) async fn cache_stats(app: tauri::AppHandle) -> Result<CacheStats, String> {
    let directory = artwork_cache_dir(&app)
        .ok_or_else(|| "No profile is available for cached artwork.".to_string())?;
    tauri::async_runtime::spawn_blocking(move || directory_stats(&directory))
        .await
        .map_err(|_| "Cached artwork could not be measured.".to_string())
}

#[tauri::command]
pub(crate) async fn clear_artwork_cache(app: tauri::AppHandle) -> Result<(), String> {
    let Some(directory) = artwork_cache_dir(&app) else {
        return Ok(());
    };
    if directory.exists() {
        tokio::fs::remove_dir_all(directory)
            .await
            .map_err(|_| "Cached artwork could not be cleared.".to_string())?;
    }
    Ok(())
}

fn download_profile_dir(
    app: &tauri::AppHandle,
    session: &ServerSession,
) -> Result<PathBuf, String> {
    download_profile_dir_for_identity(app, session.base_url.as_str(), &session.username)
}

fn download_profile_dir_for_identity(
    app: &tauri::AppHandle,
    server: &str,
    username: &str,
) -> Result<PathBuf, String> {
    let scope = profile_scope(server, username);
    app.path()
        .app_data_dir()
        .map(|path| path.join("downloads").join(scope))
        .map_err(|_| "The application data folder is unavailable.".to_string())
}

fn profile_scope(server: &str, username: &str) -> String {
    let identity = format!(
        "{}|{}",
        normalize_server(server),
        username.trim().to_lowercase()
    );
    let digest = sha2::Sha256::digest(identity.as_bytes());
    digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn active_download_profile_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let saved = load_active_profile()?
        .ok_or_else(|| "No saved profile is available for offline downloads.".to_string())?;
    download_profile_dir_for_identity(app, &saved.server, &saved.username)
}

fn current_download_profile_dir(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, SessionState>,
) -> Result<PathBuf, String> {
    match current_session(state) {
        Ok(session) => download_profile_dir(app, &session),
        Err(_) => active_download_profile_dir(app),
    }
}

fn download_file_name(song: &SongSummary, requested_format: Option<&str>) -> String {
    let digest = sha2::Sha256::digest(song.id.as_bytes());
    let stem = digest[..20]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if let Some(format) = requested_format {
        return format!("{stem}.{format}");
    }
    let suffix = song
        .suffix
        .as_deref()
        .filter(|value| !value.is_empty() && value.len() <= 8)
        .filter(|value| {
            value
                .chars()
                .all(|character| character.is_ascii_alphanumeric())
        })
        .unwrap_or("audio")
        .to_lowercase();
    format!("{stem}.{suffix}")
}

async fn read_download_manifest(directory: &FilePath) -> DownloadManifest {
    let path = directory.join("manifest.json");
    tokio::fs::read(path)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| DownloadManifest {
            version: 1,
            items: Vec::new(),
        })
}

async fn write_download_manifest(
    directory: &FilePath,
    manifest: &DownloadManifest,
) -> Result<(), String> {
    tokio::fs::create_dir_all(directory)
        .await
        .map_err(|_| "The downloads folder could not be created.".to_string())?;
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|_| "Download metadata could not be prepared.".to_string())?;
    let path = directory.join("manifest.json");
    let temporary = directory.join("manifest.json.tmp");
    tokio::fs::write(&temporary, bytes)
        .await
        .map_err(|_| "Download metadata could not be saved.".to_string())?;
    if path.exists() {
        let _ = tokio::fs::remove_file(&path).await;
    }
    tokio::fs::rename(temporary, path)
        .await
        .map_err(|_| "Download metadata could not be finalized.".to_string())
}

fn emit_download_progress(
    app: &tauri::AppHandle,
    id: &str,
    received: u64,
    total: Option<u64>,
    status: &'static str,
    message: Option<String>,
) {
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            id: id.to_string(),
            received,
            total,
            status,
            message,
        },
    );
}

async fn downloaded_path_for_playback(app: &tauri::AppHandle, id: &str) -> Option<PathBuf> {
    let session_dir = current_session(&app.state::<SessionState>())
        .ok()
        .and_then(|session| download_profile_dir(app, &session).ok());
    let saved_dir = active_download_profile_dir(app).ok();
    for directory in [session_dir, saved_dir].into_iter().flatten() {
        let manifest = read_download_manifest(&directory).await;
        let Some(item) = manifest.items.into_iter().find(|item| item.song.id == id) else {
            continue;
        };
        let path = directory.join(item.file_name);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn active_artwork_cache_path(app: &tauri::AppHandle, id: &str) -> Option<PathBuf> {
    let directory = artwork_cache_dir(app)?;
    let digest = sha2::Sha256::digest(id.as_bytes());
    let name = digest[..20]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Some(directory.join(format!("{name}.media")))
}

/// Scoped to the signed-in session when there is one, so artwork still caches
/// for a login the user chose not to save.
fn artwork_cache_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    let scope = match current_session(&app.state::<SessionState>()) {
        Ok(session) => profile_scope(session.base_url.as_str(), &session.username),
        Err(_) => {
            let saved = load_active_profile().ok()??;
            profile_scope(&saved.server, &saved.username)
        }
    };
    app.path()
        .app_cache_dir()
        .ok()
        .map(|path| path.join("artwork").join(scope))
}

fn directory_stats(directory: &FilePath) -> CacheStats {
    let mut stats = CacheStats::default();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(path) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                stats.files += 1;
                stats.bytes = stats.bytes.saturating_add(metadata.len());
            }
        }
    }
    stats
}

fn parse_byte_range(
    value: Option<&reqwest::header::HeaderValue>,
    length: u64,
) -> Option<(u64, u64)> {
    let value = value?.to_str().ok()?.strip_prefix("bytes=")?;
    if value.contains(',') {
        return None;
    }
    let (start, end) = value.split_once('-')?;
    if start.is_empty() {
        let suffix = end.parse::<u64>().ok()?.min(length);
        return Some((length.saturating_sub(suffix), length.saturating_sub(1)));
    }
    let start = start.parse::<u64>().ok()?;
    if start >= length {
        return None;
    }
    let end = if end.is_empty() {
        length - 1
    } else {
        end.parse::<u64>().ok()?.min(length - 1)
    };
    (end >= start).then_some((start, end))
}

async fn local_media_response(path: PathBuf, headers: &HeaderMap) -> Response<Body> {
    let Ok(mut file) = tokio::fs::File::open(&path).await else {
        return media_error(
            StatusCode::NOT_FOUND,
            "The downloaded track is unavailable.",
        );
    };
    let Ok(metadata) = file.metadata().await else {
        return media_error(
            StatusCode::NOT_FOUND,
            "The downloaded track is unavailable.",
        );
    };
    let full_length = metadata.len();
    if full_length == 0 {
        return media_error(StatusCode::NOT_FOUND, "The downloaded track is empty.");
    }
    let mut signature = [0_u8; 12];
    let signature_length = file.read(&mut signature).await.unwrap_or(0);
    let requested = headers.get(header::RANGE);
    let (start, end, status) = match requested {
        Some(value) => match parse_byte_range(Some(value), full_length) {
            Some((start, end)) => (start, end, StatusCode::PARTIAL_CONTENT),
            None => {
                // The header matters on this path too, not just the success
                // one: the audio elements are CORS-checked so the equalizer
                // can route them through Web Audio, and a 416 without it
                // reaches the player as an opaque CORS failure rather than as
                // the range error it is.
                return Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header("Access-Control-Allow-Origin", "*")
                    .header(header::CONTENT_RANGE, format!("bytes */{full_length}"))
                    .body(Body::empty())
                    .unwrap_or_else(|_| Response::new(Body::empty()));
            }
        },
        None => (0, full_length - 1, StatusCode::OK),
    };
    if file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return media_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The downloaded track could not be read.",
        );
    }
    let content_length = end - start + 1;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let content_type = if signature_length >= 3 && signature[..3] == [0xff, 0xd8, 0xff] {
        "image/jpeg"
    } else if signature_length >= 8 && signature[..8] == [137, 80, 78, 71, 13, 10, 26, 10] {
        "image/png"
    } else if signature_length >= 12 && &signature[..4] == b"RIFF" && &signature[8..12] == b"WEBP" {
        "image/webp"
    } else if signature_length >= 6
        && (&signature[..6] == b"GIF87a" || &signature[..6] == b"GIF89a")
    {
        "image/gif"
    } else {
        match extension.to_ascii_lowercase().as_str() {
            "mp3" => "audio/mpeg",
            "m4a" | "mp4" | "aac" => "audio/mp4",
            "flac" => "audio/flac",
            "ogg" | "opus" => "audio/ogg",
            "wav" => "audio/wav",
            _ => "application/octet-stream",
        }
    };
    let mut builder = Response::builder()
        .status(status)
        .header("Access-Control-Allow-Origin", "*")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, content_length)
        .header("X-Splice-Source", "download")
        .header("Cache-Control", "private, max-age=31536000, immutable");
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{full_length}"),
        );
    }
    builder
        .body(Body::from_stream(ReaderStream::new(
            file.take(content_length),
        )))
        .unwrap_or_else(|_| {
            media_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The downloaded track is unavailable.",
            )
        })
}

pub(crate) async fn start_media_proxy(app: tauri::AppHandle) -> Result<MediaProxyState, String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| format!("The protected media listener could not start: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("The protected media address is unavailable: {error}"))?;
    let token: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect();
    let router = Router::new()
        .route("/{kind}", get(proxy_media_http))
        .with_state(MediaRouterState {
            app,
            token: token.clone(),
        });
    tauri::async_runtime::spawn(async move {
        if let Err(error) = axum::serve(listener, router).await {
            eprintln!("protected media listener stopped: {error}");
        }
    });
    Ok(MediaProxyState {
        base_url: format!("http://{address}"),
        token,
    })
}

#[tauri::command]
pub(crate) fn media_url(
    kind: String,
    id: String,
    proxy: tauri::State<'_, MediaProxyState>,
) -> Result<String, String> {
    if !matches!(kind.as_str(), "cover" | "stream") || id.is_empty() {
        return Err("Invalid media request.".to_string());
    }
    let mut url = Url::parse(&format!("{}/{kind}", proxy.base_url))
        .map_err(|_| "Could not create the protected media address.".to_string())?;
    url.query_pairs_mut()
        .append_pair("id", &id)
        .append_pair("token", &proxy.token);
    Ok(url.to_string())
}

async fn proxy_media_http(
    State(router): State<MediaRouterState>,
    Path(kind): Path<String>,
    Query(query): Query<MediaQuery>,
    headers: HeaderMap,
) -> Response<Body> {
    if query.token != router.token {
        return media_error(StatusCode::UNAUTHORIZED, "Invalid protected media token.");
    }
    let endpoint = match kind.as_str() {
        "cover" => "getCoverArt.view",
        "stream" => "stream.view",
        _ => return media_error(StatusCode::NOT_FOUND, "Unknown media type."),
    };
    if query.id.is_empty() {
        return media_error(StatusCode::BAD_REQUEST, "Missing media identifier.");
    }
    if kind == "stream" {
        if let Some(path) = downloaded_path_for_playback(&router.app, &query.id).await {
            return local_media_response(path, &headers).await;
        }
    }
    if kind == "cover" {
        if let Some(path) = active_artwork_cache_path(&router.app, &query.id) {
            if path.is_file() {
                return local_media_response(path, &headers).await;
            }
        }
    }
    let prefs = router.app.state::<PlaybackPrefsState>().snapshot();
    if prefs.offline_mode {
        return media_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Offline mode is on. Only downloaded music is available.",
        );
    }
    let session = match current_session(&router.app.state::<SessionState>()) {
        Ok(session) => session,
        Err(_) => return media_error(StatusCode::UNAUTHORIZED, "Sign in to play media."),
    };
    let mut url = match authenticated_url(&session, endpoint) {
        Ok(url) => url,
        Err(_) => return media_error(StatusCode::INTERNAL_SERVER_ERROR, "Media is unavailable."),
    };
    url.query_pairs_mut().append_pair("id", &query.id);
    if kind == "cover" {
        url.query_pairs_mut().append_pair("size", "800");
    } else {
        match sanitized_format(&prefs.stream_format) {
            Some(format) => {
                url.query_pairs_mut().append_pair("format", format);
                if prefs.stream_bit_rate > 0 && format != "wav" {
                    url.query_pairs_mut()
                        .append_pair("maxBitRate", &prefs.stream_bit_rate.to_string());
                }
            }
            None => {
                url.query_pairs_mut().append_pair("format", "raw");
            }
        }
        url.query_pairs_mut()
            .append_pair("estimateContentLength", "true");
    }

    let mut request = session.client.get(url);
    if let Some(range) = headers.get(header::RANGE) {
        request = request.header(header::RANGE, range);
    }
    let upstream = match request.send().await {
        Ok(response) => response,
        Err(_) => return media_error(StatusCode::BAD_GATEWAY, "The media server is unavailable."),
    };
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let upstream_headers = upstream.headers().clone();
    if kind == "cover" && status.is_success() {
        let bytes = match upstream.bytes().await {
            Ok(bytes) => bytes,
            Err(_) => return media_error(StatusCode::BAD_GATEWAY, "Artwork is unavailable."),
        };
        if let Some(path) = active_artwork_cache_path(&router.app, &query.id) {
            if let Some(parent) = path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
                let temporary = path.with_extension("tmp");
                if tokio::fs::write(&temporary, &bytes).await.is_ok() {
                    if path.exists() {
                        let _ = tokio::fs::remove_file(&path).await;
                    }
                    let _ = tokio::fs::rename(temporary, path).await;
                }
            }
        }
        let mut builder = Response::builder()
            .status(status)
            .header("Access-Control-Allow-Origin", "*")
            .header("Cache-Control", "private, max-age=31536000, immutable")
            .header(header::CONTENT_LENGTH, bytes.len());
        if let Some(value) = upstream_headers.get(header::CONTENT_TYPE) {
            builder = builder.header(header::CONTENT_TYPE, value);
        }
        return builder.body(Body::from(bytes)).unwrap_or_else(|_| {
            media_error(StatusCode::INTERNAL_SERVER_ERROR, "Artwork is unavailable.")
        });
    }
    let mut builder = Response::builder()
        .status(status)
        .header("Access-Control-Allow-Origin", "*")
        .header(
            "Cache-Control",
            if kind == "cover" {
                "private, max-age=86400"
            } else {
                "no-store"
            },
        );
    for name in [
        header::CONTENT_TYPE,
        header::CONTENT_LENGTH,
        header::CONTENT_RANGE,
        header::ACCEPT_RANGES,
    ] {
        if let Some(value) = upstream_headers.get(&name) {
            builder = builder.header(name.as_str(), value);
        }
    }
    builder
        .body(Body::from_stream(upstream.bytes_stream()))
        .unwrap_or_else(|_| media_error(StatusCode::INTERNAL_SERVER_ERROR, "Media is unavailable."))
}

fn media_error(status: StatusCode, message: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .header("Access-Control-Allow-Origin", "*")
        .body(Body::from(message.to_string()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn current_session(state: &tauri::State<'_, SessionState>) -> Result<ServerSession, String> {
    state
        .0
        .lock()
        .map_err(|_| "The desktop session is unavailable.".to_string())?
        .clone()
        .ok_or_else(|| "Sign in to your music server first.".to_string())
}

async fn album_list(
    session: &ServerSession,
    list_type: &str,
    size: u32,
) -> Result<Vec<AlbumSummary>, String> {
    album_list_page(session, list_type, size, 0).await
}

async fn album_list_page(
    session: &ServerSession,
    list_type: &str,
    size: u32,
    offset: u32,
) -> Result<Vec<AlbumSummary>, String> {
    let size = size.to_string();
    let offset = offset.to_string();
    let response = session_request(
        session,
        "getAlbumList2.view",
        &[("type", list_type), ("size", &size), ("offset", &offset)],
    )
    .await?;
    Ok(response
        .response
        .album_list2
        .unwrap_or_else(AlbumList::default)
        .album)
}

async fn all_albums(session: &ServerSession) -> Result<Vec<AlbumSummary>, String> {
    const PAGE_SIZE: u32 = 500;
    const MAX_ALBUMS: usize = 50_000;
    let mut albums = Vec::new();
    loop {
        let page = album_list_page(
            session,
            "alphabeticalByArtist",
            PAGE_SIZE,
            albums.len() as u32,
        )
        .await?;
        let is_last = page.len() < PAGE_SIZE as usize;
        albums.extend(page);
        if is_last || albums.len() >= MAX_ALBUMS {
            albums.truncate(MAX_ALBUMS);
            return Ok(albums);
        }
    }
}

async fn session_request(
    session: &ServerSession,
    endpoint: &str,
    extra: &[(&str, &str)],
) -> Result<Envelope, String> {
    let response = api_request(
        &session.client,
        &session.base_url,
        endpoint,
        &session.username,
        &session.token,
        &session.salt,
        extra,
    )
    .await?;
    ensure_ok(&response)?;
    Ok(response)
}

async fn api_request(
    client: &Client,
    base_url: &Url,
    endpoint: &str,
    username: &str,
    token: &str,
    salt: &str,
    extra: &[(&str, &str)],
) -> Result<Envelope, String> {
    let url = endpoint_url(base_url, endpoint)?;
    let mut query = vec![
        ("u", username),
        ("t", token),
        ("s", salt),
        ("v", API_VERSION),
        ("c", CLIENT_NAME),
        ("f", "json"),
    ];
    query.extend_from_slice(extra);
    let response = client
        .get(url)
        .query(&query)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|error| connection_error(error, base_url))?;
    if !response.status().is_success() {
        return Err(format!(
            "The server returned HTTP {}.",
            response.status().as_u16()
        ));
    }
    response.json::<Envelope>().await.map_err(|_| {
        "The address responded, but it was not a Subsonic-compatible server.".to_string()
    })
}

fn authenticated_url(session: &ServerSession, endpoint: &str) -> Result<Url, String> {
    let mut url = endpoint_url(&session.base_url, endpoint)?;
    url.query_pairs_mut()
        .append_pair("u", &session.username)
        .append_pair("t", &session.token)
        .append_pair("s", &session.salt)
        .append_pair("v", API_VERSION)
        .append_pair("c", CLIENT_NAME);
    Ok(url)
}

fn ensure_ok(envelope: &Envelope) -> Result<(), String> {
    if envelope.response.status.eq_ignore_ascii_case("ok") {
        return Ok(());
    }
    Err(envelope
        .response
        .error
        .as_ref()
        .and_then(|error| error.message.clone())
        .unwrap_or_else(|| "The server rejected this request.".to_string()))
}

fn connection_error(error: reqwest::Error, base_url: &Url) -> String {
    if error.is_timeout() {
        format!("Timed out while connecting to {}.", display_host(base_url))
    } else if error.is_connect() {
        format!(
            "Could not reach {}. Check the address and network.",
            display_host(base_url)
        )
    } else {
        format!("Could not connect to {}: {error}", display_host(base_url))
    }
}

pub(crate) fn normalize_base_url(raw: &str) -> Result<Url, String> {
    let trimmed = raw.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        return Err("Enter your server address.".to_string());
    }
    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let mut url =
        Url::parse(&candidate).map_err(|_| "Enter a valid server address.".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("The server address must use HTTP or HTTPS.".to_string());
    }
    let mut path = url.path().trim_end_matches('/').to_string();
    for suffix in ["/app", "/login", "/rest"] {
        if path.to_ascii_lowercase().ends_with(suffix) {
            path.truncate(path.len() - suffix.len());
            break;
        }
    }
    url.set_path(if path.is_empty() { "/" } else { &path });
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

pub(crate) fn endpoint_url(base_url: &Url, endpoint: &str) -> Result<Url, String> {
    let base = base_url.as_str().trim_end_matches('/');
    Url::parse(&format!("{base}/rest/{endpoint}"))
        .map_err(|_| "Could not build the Subsonic API address.".to_string())
}

pub(crate) fn display_host(url: &Url) -> String {
    let host = url.host_str().unwrap_or("server");
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    let path = url.path().trim_end_matches('/');
    format!("{host}{port}{path}")
}

pub(crate) fn normalize_server(server: &str) -> String {
    normalize_base_url(server)
        .map(|url| display_host(&url).to_lowercase())
        .unwrap_or_else(|_| server.trim().to_lowercase())
}

fn profile_id(credentials: &SavedCredentials) -> String {
    let identity = format!(
        "{}|{}",
        normalize_server(&credentials.server),
        credentials.username.trim().to_lowercase()
    );
    let digest = sha2::Sha256::digest(identity.as_bytes());
    digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The decoded store, held for the life of the process.
///
/// Reading the Keychain is not a free lookup on macOS, it is a user-visible
/// event: the ACL on the item is bound to the binary's code signature, so a
/// build signed ad-hoc — which is every local dev build, since the signature is
/// derived from the binary's contents and changes on every rebuild — asks the
/// user for permission on each access.
///
/// Launch alone performed two reads, `restore_session` followed by
/// `list_profiles`, so it produced two dialogs before the user had touched
/// anything. They read the same item, so the second is pure cost. Every writer
/// below refreshes this, which is why a cache is safe here: `load` is the only
/// reader and `save_credential_store` is the only writer.
static CREDENTIAL_CACHE: Mutex<Option<CredentialStore>> = Mutex::new(None);

fn cache_credential_store(store: &CredentialStore) {
    if let Ok(mut cache) = CREDENTIAL_CACHE.lock() {
        *cache = Some(store.clone());
    }
}

fn save_credential_store(store: &CredentialStore) -> Result<(), String> {
    let serialized = serde_json::to_string(store)
        .map_err(|_| "The saved profiles could not be encoded.".to_string())?;
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|error| format!("The secure credential store is unavailable: {error}"))?;
    entry
        .set_password(&serialized)
        .map_err(|error| format!("The profiles could not be saved securely: {error}"))?;
    cache_credential_store(store);
    Ok(())
}

fn load_credential_store() -> Result<CredentialStore, String> {
    if let Ok(cache) = CREDENTIAL_CACHE.lock() {
        if let Some(store) = cache.as_ref() {
            return Ok(store.clone());
        }
    }
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|error| format!("The secure credential store is unavailable: {error}"))?;
    let serialized = match entry.get_password() {
        Ok(value) => value,
        Err(_) => {
            let empty = CredentialStore::default();
            cache_credential_store(&empty);
            return Ok(empty);
        }
    };
    if let Ok(store) = serde_json::from_str::<CredentialStore>(&serialized) {
        cache_credential_store(&store);
        return Ok(store);
    }
    if let Ok(credentials) = serde_json::from_str::<SavedCredentials>(&serialized) {
        let id = profile_id(&credentials);
        let migrated = CredentialStore {
            active_profile_id: Some(id.clone()),
            profiles: vec![SavedProfile { id, credentials }],
        };
        cache_credential_store(&migrated);
        return Ok(migrated);
    }
    Err("The saved profiles are invalid.".to_string())
}

fn load_active_profile() -> Result<Option<SavedCredentials>, String> {
    let store = load_credential_store()?;
    let Some(active) = store.active_profile_id else {
        return Ok(None);
    };
    Ok(store
        .profiles
        .into_iter()
        .find(|profile| profile.id == active)
        .map(|profile| profile.credentials))
}

fn save_profile(credentials: SavedCredentials) -> Result<(), String> {
    let mut store = load_credential_store()?;
    let id = profile_id(&credentials);
    if let Some(existing) = store.profiles.iter_mut().find(|profile| profile.id == id) {
        existing.credentials = credentials;
    } else {
        store.profiles.push(SavedProfile {
            id: id.clone(),
            credentials,
        });
    }
    store.active_profile_id = Some(id);
    save_credential_store(&store)
}

fn set_active_profile(profile_id: &str) -> Result<(), String> {
    let mut store = load_credential_store()?;
    if !store
        .profiles
        .iter()
        .any(|profile| profile.id == profile_id)
    {
        return Err("That saved profile is no longer available.".to_string());
    }
    store.active_profile_id = Some(profile_id.to_string());
    save_credential_store(&store)
}

fn remove_profile(profile_id: &str) -> Result<(), String> {
    let mut store = load_credential_store()?;
    store.profiles.retain(|profile| profile.id != profile_id);
    if store.active_profile_id.as_deref() == Some(profile_id) {
        store.active_profile_id = None;
    }
    save_or_delete_store(&store)
}

fn remove_active_profile() -> Result<(), String> {
    let mut store = load_credential_store()?;
    if let Some(active) = store.active_profile_id.take() {
        store.profiles.retain(|profile| profile.id != active);
    }
    save_or_delete_store(&store)
}

fn clear_active_profile() -> Result<(), String> {
    let mut store = load_credential_store()?;
    store.active_profile_id = None;
    save_or_delete_store(&store)
}

fn save_or_delete_store(store: &CredentialStore) -> Result<(), String> {
    if !store.profiles.is_empty() {
        return save_credential_store(store);
    }
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|error| format!("The secure credential store is unavailable: {error}"))?;
    let _ = entry.delete_credential();
    // The item is gone, so the cached view is an empty store rather than stale.
    cache_credential_store(&CredentialStore::default());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        display_host, endpoint_url, is_guest_credit, normalize_base_url, normalize_server,
        parse_lrc,
    };
    use crate::models::{AlbumDetail, ArtistDetail, Envelope, PlaylistDetail, SearchResults};

    #[test]
    fn appears_on_needs_a_shared_credit_not_a_title_match() {
        assert!(is_guest_credit("Kendrick Lamar & Geese", "Geese"));
        assert!(is_guest_credit("Someone feat. Geese", "geese"));
        assert!(is_guest_credit("A, B, Geese", "Geese"));
        assert!(
            !is_guest_credit("Geese", "Geese"),
            "their own albums are discography"
        );
        assert!(
            !is_guest_credit("Passi", "Geese"),
            "a title match is not an appearance"
        );
        assert!(!is_guest_credit("Anthony Phillips", "Geese"));
        assert!(!is_guest_credit("Geesey", "Geese"));
        assert!(!is_guest_credit("Various Artists", ""));
    }

    /// Subsonic sends singular keys and the frontend reads plural ones, so the
    /// rename on these fields has to run in one direction only. When it ran both
    /// ways the host answered `song`/`entry`/`album` to a UI reading
    /// `songs`/`albums`, and album, playlist, artist and search pages all
    /// arrived with undefined lists. Both sides compiled perfectly.
    #[test]
    fn detail_payloads_keep_subsonic_names_inbound_and_frontend_names_outbound() {
        let album: AlbumDetail =
            serde_json::from_str(r#"{"id":"a","name":"T","artist":"A","song":[]}"#).unwrap();
        let json = serde_json::to_value(&album).unwrap();
        assert!(json.get("songs").is_some(), "album must serialize `songs`");
        assert!(
            json.get("song").is_none(),
            "album must not serialize `song`"
        );

        let playlist: PlaylistDetail =
            serde_json::from_str(r#"{"id":"p","name":"N","entry":[]}"#).unwrap();
        let json = serde_json::to_value(&playlist).unwrap();
        assert!(
            json.get("songs").is_some(),
            "playlist must serialize `songs`"
        );
        assert!(json.get("entry").is_none());

        let artist: ArtistDetail =
            serde_json::from_str(r#"{"id":"r","name":"N","album":[]}"#).unwrap();
        let json = serde_json::to_value(&artist).unwrap();
        assert!(
            json.get("albums").is_some(),
            "artist must serialize `albums`"
        );
        assert!(json.get("album").is_none());

        let results: SearchResults =
            serde_json::from_str(r#"{"song":[],"album":[],"artist":[]}"#).unwrap();
        let json = serde_json::to_value(&results).unwrap();
        for key in ["songs", "albums", "artists"] {
            assert!(json.get(key).is_some(), "search must serialize `{key}`");
        }
        for key in ["song", "album", "artist"] {
            assert!(json.get(key).is_none(), "search must not serialize `{key}`");
        }
    }

    #[test]
    fn normalizes_connect_identity_servers() {
        assert_eq!(
            normalize_server(" MUSIC.EXAMPLE.test "),
            "music.example.test"
        );
        assert_eq!(
            normalize_server("https://Music.Example.test:4533/rest/"),
            "music.example.test:4533"
        );
        assert_eq!(
            normalize_server("http://Music.Example.test/navidrome/REST"),
            "music.example.test/navidrome"
        );
        assert_eq!(
            normalize_server("music.example.test:4533/proxy/"),
            "music.example.test:4533/proxy"
        );
    }

    #[test]
    fn builds_subsonic_endpoint_below_optional_base_path() {
        let base = normalize_base_url("https://music.example.test/navidrome/app").unwrap();
        assert_eq!(display_host(&base), "music.example.test/navidrome");
        assert_eq!(
            endpoint_url(&base, "ping.view").unwrap().as_str(),
            "https://music.example.test/navidrome/rest/ping.view"
        );
    }

    #[test]
    fn rejects_non_http_server_addresses() {
        assert!(normalize_base_url("file:///tmp/music").is_err())
    }

    #[test]
    fn accepts_bare_hosts_and_trims_sentence_punctuation() {
        let base = normalize_base_url("music.example.test.").unwrap();
        assert_eq!(base.as_str(), "https://music.example.test/");
    }

    #[test]
    fn decodes_subsonic_collection_names_and_songs() {
        let body = r#"{
            "subsonic-response": {
                "status": "ok", "version": "1.16.1",
                "album": {
                    "id": "album-1", "name": "Real Album", "artist": "Real Artist",
                    "song": [{"id": "song-1", "title": "Real Song", "artist": "Real Artist", "album": "Real Album"}]
                }
            }
        }"#;
        let envelope: Envelope = serde_json::from_str(body).unwrap();
        let album = envelope.response.album.unwrap();
        assert_eq!(album.title, "Real Album");
        assert_eq!(album.songs[0].title, "Real Song");
        assert_eq!(serde_json::to_value(album).unwrap()["title"], "Real Album");
    }

    #[test]
    fn parses_synced_lyrics_and_ignores_instrumental_markers() {
        let lines = parse_lrc("[00:01.25]First line\n[00:05.100]♪\n[01:02.50]Later line");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].value, "First line");
        assert!((lines[0].start - 1.25).abs() < 0.001);
        assert!((lines[1].start - 62.5).abs() < 0.001);
    }
}
