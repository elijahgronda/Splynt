use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectRequest {
    pub(crate) server: String,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) allows_self_signed: bool,
    #[serde(default = "default_remember_me")]
    pub(crate) remember_me: bool,
}

fn default_remember_me() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServerInfo {
    pub(crate) display_host: String,
    pub(crate) username: String,
    pub(crate) server_type: Option<String>,
    pub(crate) server_version: Option<String>,
    pub(crate) api_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AlbumSummary {
    pub(crate) id: String,
    #[serde(rename(deserialize = "name", serialize = "title"))]
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) artist: String,
    pub(crate) artist_id: Option<String>,
    pub(crate) year: Option<u32>,
    pub(crate) cover_art: Option<String>,
    pub(crate) duration: Option<u64>,
    pub(crate) song_count: Option<u32>,
    pub(crate) created: Option<String>,
    pub(crate) starred: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtistSummary {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) cover_art: Option<String>,
    pub(crate) album_count: Option<u32>,
    pub(crate) starred: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaylistSummary {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) owner: Option<String>,
    pub(crate) cover_art: Option<String>,
    pub(crate) song_count: Option<u32>,
    pub(crate) duration: Option<u64>,
    pub(crate) changed: Option<String>,
    pub(crate) public: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SongSummary {
    pub(crate) id: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) artist: String,
    pub(crate) artist_id: Option<String>,
    #[serde(default)]
    pub(crate) album: String,
    pub(crate) album_id: Option<String>,
    pub(crate) cover_art: Option<String>,
    pub(crate) duration: Option<u64>,
    pub(crate) track: Option<u32>,
    pub(crate) disc_number: Option<u32>,
    pub(crate) year: Option<u32>,
    pub(crate) suffix: Option<String>,
    pub(crate) bit_rate: Option<u32>,
    pub(crate) starred: Option<String>,
    pub(crate) created: Option<String>,
    pub(crate) explicit_status: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AlbumDetail {
    pub(crate) id: String,
    #[serde(rename(deserialize = "name", serialize = "title"))]
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) artist: String,
    pub(crate) artist_id: Option<String>,
    pub(crate) year: Option<u32>,
    pub(crate) cover_art: Option<String>,
    pub(crate) duration: Option<u64>,
    pub(crate) song_count: Option<u32>,
    pub(crate) starred: Option<String>,
    #[serde(default, rename(deserialize = "song"))]
    pub(crate) songs: Vec<SongSummary>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaylistDetail {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) owner: Option<String>,
    pub(crate) cover_art: Option<String>,
    pub(crate) song_count: Option<u32>,
    pub(crate) duration: Option<u64>,
    pub(crate) changed: Option<String>,
    pub(crate) public: Option<bool>,
    #[serde(default, rename(deserialize = "entry"))]
    pub(crate) songs: Vec<SongSummary>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtistDetail {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) cover_art: Option<String>,
    pub(crate) album_count: Option<u32>,
    pub(crate) starred: Option<String>,
    #[serde(default, rename(deserialize = "album"))]
    pub(crate) albums: Vec<AlbumSummary>,
    #[serde(default)]
    pub(crate) top_songs: Vec<SongSummary>,
    /// Albums credited to someone else that the server still associates with
    /// this artist. Not part of getArtist, so it is derived from search.
    #[serde(default, skip_deserializing)]
    pub(crate) appearances: Vec<AlbumSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectedLibrary {
    pub(crate) server: ServerInfo,
    pub(crate) albums: Vec<AlbumSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HomeOverview {
    pub(crate) newest: Vec<AlbumSummary>,
    pub(crate) recent: Vec<AlbumSummary>,
    pub(crate) frequent: Vec<AlbumSummary>,
    pub(crate) random: Vec<AlbumSummary>,
    pub(crate) genres: Vec<GenreShelf>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenreShelf {
    pub(crate) name: String,
    pub(crate) albums: Vec<AlbumSummary>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenreList {
    #[serde(default)]
    pub(crate) genre: Vec<GenreEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenreEntry {
    #[serde(rename = "value", default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) album_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryOverview {
    pub(crate) albums: Vec<AlbumSummary>,
    pub(crate) artists: Vec<ArtistSummary>,
    pub(crate) playlists: Vec<PlaylistSummary>,
    pub(crate) starred_songs: Vec<SongSummary>,
    pub(crate) starred_albums: Vec<AlbumSummary>,
    pub(crate) starred_artists: Vec<ArtistSummary>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchResults {
    #[serde(default, rename(deserialize = "song"))]
    pub(crate) songs: Vec<SongSummary>,
    #[serde(default, rename(deserialize = "album"))]
    pub(crate) albums: Vec<AlbumSummary>,
    #[serde(default, rename(deserialize = "artist"))]
    pub(crate) artists: Vec<ArtistSummary>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LyricsLine {
    #[serde(default)]
    pub(crate) start: f64,
    #[serde(default)]
    pub(crate) value: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LyricsResult {
    pub(crate) synced: bool,
    pub(crate) lines: Vec<LyricsLine>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlayQueue {
    pub(crate) current: Option<String>,
    #[serde(default)]
    pub(crate) position: u64,
    #[serde(default, rename = "entry")]
    pub(crate) songs: Vec<SongSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlayQueueSnapshot {
    pub(crate) songs: Vec<SongSummary>,
    pub(crate) current_id: Option<String>,
    pub(crate) position: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StructuredLyrics {
    #[serde(default)]
    pub(crate) synced: bool,
    #[serde(default)]
    pub(crate) line: Vec<LyricsLine>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LyricsList {
    #[serde(default)]
    pub(crate) structured_lyrics: Vec<StructuredLyrics>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct SongList {
    #[serde(default)]
    pub(crate) song: Vec<SongSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RadioResult {
    pub(crate) title: String,
    pub(crate) songs: Vec<SongSummary>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Envelope {
    #[serde(rename = "subsonic-response")]
    pub(crate) response: SubsonicResponse,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SubsonicResponse {
    pub(crate) status: String,
    pub(crate) version: Option<String>,
    #[serde(rename = "type")]
    pub(crate) server_type: Option<String>,
    pub(crate) server_version: Option<String>,
    pub(crate) error: Option<SubsonicError>,
    pub(crate) album_list2: Option<AlbumList>,
    pub(crate) artists: Option<ArtistsContainer>,
    pub(crate) playlists: Option<PlaylistsContainer>,
    pub(crate) starred2: Option<StarredContainer>,
    pub(crate) search_result3: Option<SearchResults>,
    pub(crate) album: Option<AlbumDetail>,
    pub(crate) playlist: Option<PlaylistDetail>,
    pub(crate) artist: Option<ArtistDetail>,
    pub(crate) song: Option<SongSummary>,
    pub(crate) lyrics_list: Option<LyricsList>,
    pub(crate) play_queue: Option<PlayQueue>,
    pub(crate) similar_songs2: Option<SongList>,
    pub(crate) random_songs: Option<SongList>,
    pub(crate) top_songs: Option<SongList>,
    pub(crate) genres: Option<GenreList>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SubsonicError {
    pub(crate) message: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct AlbumList {
    #[serde(default)]
    pub(crate) album: Vec<AlbumSummary>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ArtistsContainer {
    #[serde(default)]
    pub(crate) index: Vec<ArtistIndex>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ArtistIndex {
    #[serde(default)]
    pub(crate) artist: Vec<ArtistSummary>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PlaylistsContainer {
    #[serde(default)]
    pub(crate) playlist: Vec<PlaylistSummary>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct StarredContainer {
    #[serde(default)]
    pub(crate) song: Vec<SongSummary>,
    #[serde(default)]
    pub(crate) album: Vec<AlbumSummary>,
    #[serde(default)]
    pub(crate) artist: Vec<ArtistSummary>,
}
